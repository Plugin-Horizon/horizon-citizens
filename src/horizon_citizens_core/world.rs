//! The NPCs at run time: which are out in the world under which entity id, what to send the game
//! (console commands and the like, run by the glue at the next tick), clicks, look-close, walking
//! along waypoints, saving.
//!
//! Nothing here calls the game: every change is an [`Op`] the glue takes with [`World::take_ops`]
//! and runs outside its lock, and every event for other mods is an [`NpcEvent`] taken with
//! [`World::take_events`].

use crate::horizon_citizens_core::config::Config;
use crate::horizon_citizens_core::entity_set::{self, EntitySet};
use crate::horizon_citizens_core::tree::TreeExt;
use crate::horizon_citizens_core::yaml::Node;
use crate::horizon_citizens_core::look::{self, PlayerView, Rng};
use crate::horizon_citizens_core::npc::{CostItem, StackData, Hand, Location, NpcCommand, NpcData, Skin};
use crate::horizon_citizens_core::saves::Saves;
use crate::horizon_citizens_core::spawn;
use crate::horizon_citizens_core::text;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;

mod behave;
mod shop_ui;
pub use behave::LiveStats;
pub use shop_ui::{ShopPlayer, Src, Target};

/// Nodes that skip a command's cooldown, global cooldown and use limit (Citizens' `ignoreerrors`).
pub const IGNORE_ERRORS: [&str; 3] = [
    "citizens.npc.command.ignoreerrors.cooldown",
    "citizens.npc.command.ignoreerrors.globalcooldown",
    "citizens.npc.command.ignoreerrors.nused",
];

/// Nodes that skip a command's costs (Citizens' `chargeCommandCosts`): everything, money, experience, items.
pub const COST_IGNORE: [&str; 4] = [
    "citizens.npc.command.ignoreerrors.*",
    "citizens.npc.command.ignoreerrors.cost",
    "citizens.npc.command.ignoreerrors.expcost",
    "citizens.npc.command.ignoreerrors.itemcost",
];

/// What a click may be charged against: the clicking player's experience level and main-inventory
/// stacks (`slot`, namespaced item id, count; slots 0 to 35), read by the host before the click.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlayerCost {
    pub levels: i32,
    pub slots: Vec<(u32, String, u32)>,
    /// The balance in the economy (0 without one).
    pub money: f64,
}

/// `ItemAction.takeItems`: the slots (and the counts they keep, 0 = emptied) that pay `costs`, or
/// `None` when the stacks do not cover them. Slots are walked in order; a stack pays the first
/// still-open cost of its item, then the next.
pub fn plan_item_charge(slots: &[(u32, String, u32)], costs: &[CostItem]) -> Option<Vec<(u32, u32)>> {
    let mut req: Vec<i64> = costs.iter().map(|c| c.amount).collect();
    let mut out = Vec::new();
    for (slot, id, count) in slots {
        let mut left = i64::from(*count);
        for (j, c) in costs.iter().enumerate() {
            if left == 0 {
                break;
            }
            if req[j] <= 0 || c.id != *id {
                continue;
            }
            let taken = left.min(req[j]);
            left -= taken;
            req[j] -= taken;
        }
        if left != i64::from(*count) {
            out.push((*slot, left as u32));
        }
    }
    (req.iter().sum::<i64>() <= 0).then_some(out)
}

fn text_fill(s: &str, player: &str, npc: &str) -> String {
    s.replace("<player>", player).replace("<npc>", &text::strip(npc))
}

/// The command that shows (or clears) the block an enderman carries.
fn carried_command(uuid: u128, block: Option<&str>) -> String {
    let id = crate::horizon_citizens_core::ids::format_uuid(uuid);
    match block {
        Some(b) => {
            let b = if b.contains(':') { b.to_owned() } else { format!("minecraft:{b}") };
            format!("data merge entity {id} {{carriedBlockState:{{Name:\"{b}\"}}}}")
        }
        None => format!("data remove entity {id} carriedBlockState"),
    }
}

/// What a click costs: money, experience levels (-1 unset) and item stacks. `global` is the commandtrait's own
/// price (charged once per click), else one command's.
pub struct Price {
    pub money: f64,
    pub exp: i64,
    pub items: Vec<CostItem>,
    pub global: bool,
}

/// Something for the game to do, at the next tick, outside every player's hook.
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    /// A command as the server console runs it.
    Console(String),
    /// Take an NPC's entity out of the world, no death, no loot.
    RemoveMob { dimension: String, entity: u32 },
    /// A chat line (legacy colour codes) to a player.
    Tell { player: u128, legacy: String },
    /// Look a skin name up (off the server thread); the answer comes back through
    /// [`World::skin_fetched`].
    FetchSkin { npc: i32, name: String },
    /// Turn an NPC's entity (`Host::set_entity_rotation`: head and body, no `rotate` command);
    /// `fallback` is the console command for when the host refuses.
    Rotate { dimension: String, entity: u32, yaw: f32, pitch: f32, fallback: String },
    /// `lookclose --headonly`: the head turns to `yaw` and `pitch`, the body keeps `body_yaw`.
    RotateHead { entity: u32, body_yaw: f32, yaw: f32, pitch: f32 },
    /// A command's price, paid: take `levels` experience levels and set each slot to the count it
    /// keeps (0 empties it). Sent only after every check of the command passed.
    Charge { player: u128, levels: i32, slots: Vec<(u32, u32)>, money: f64 },
    /// `NMS.setShouldJump`: the mob's jump control fires on its next tick.
    Jump { entity: u32 },
    /// Seat a rider on a vehicle entity (`Entity.startRiding`): the player `player`, else the entity `rider`.
    Ride { player: Option<u128>, rider: u32, vehicle: u32 },
    /// Take a rider off its vehicle (`leaveVehicle`): the player `player`, else the entity `entity`.
    Dismount { player: Option<u128>, entity: u32 },
    /// Open a chest menu of `rows` rows for a player (`Host::open_menu`); the glue answers with
    /// [`World::menu_opened`].
    /// `in_place` swaps the screen the player has open without the client centring the mouse.
    OpenMenu { player: u128, title: String, rows: u8, items: Vec<Option<StackData>>, in_place: bool },
    /// Open a hopper menu (five slots) for a player; the glue answers with [`World::menu_opened`] as for `OpenMenu`.
    OpenHopper { player: u128, title: String, items: Vec<Option<StackData>> },
    /// Close the player's open menu (a chat prompt of a shop editor, an open-shop action).
    CloseMenu { player: u128 },
    /// A shop purchase went through: set the experience level, and each slot (`Inventory` numbers 0-35) to the stack.
    ShopApply { player: u128, levels: Option<i32>, slots: Vec<(u32, Option<StackData>)>, money: f64 },
    /// Replace everything an open menu shows (`menu_set_items`).
    SetMenuItems { player: u128, window: i32, items: Vec<Option<StackData>> },
    /// A command line run as the NPC's own entity (`entity_run_command`) at this operator level.
    EntityCommand { entity: u32, line: String, level: u8 },
    /// A host call for an NPC's entity (disguise, pose, targetable, ranges, navigation ...).
    Entity { entity: u32, set: EntitySet },
    /// `/npc spectate`: the player's view follows this entity (`None`: back at themselves).
    Camera { player: u128, entity: Option<u32> },
    /// A trader shop: a merchant screen (`player_open_merchant`) with these trades, in order. The glue
    /// answers with [`World::merchant_opened`].
    OpenMerchant { player: u128, title: String, trades: Vec<Trade> },
}

/// One trade of a trader shop: up to two payments and the stack received (`MerchantRecipe`).
#[derive(Clone, Debug, PartialEq)]
pub struct Trade {
    pub costs: Vec<StackData>,
    pub result: StackData,
}

/// What the host told about a `NoAI` NPC that follows paths itself.
#[derive(Clone, Debug, Default)]
struct RealNav {
    /// Where its entity stands.
    pos: Option<[f64; 3]>,
    /// `mob_is_done`: no path, or the path finished.
    done: bool,
    /// The point a path was last asked for, and the tick.
    goal: Option<([f64; 3], u64)>,
}

/// Which NPC data a menu edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuKind {
    Inventory,
    Drops,
    /// `GenericEquipperGUI`: three rows, the NPC's equipment in row 1.
    Equipment,
    /// `NPCConfigurator`: one button, edit the name through the chat.
    Config,
    /// `ItemRequirementGUI` (`/npc command itemcost`): five rows of stacks a click takes from the player.
    ItemCost,
    /// `SaddleEquipperGUI` (a pig or strider): a hopper with the saddle slot.
    Saddle,
    /// `EndermanEquipperGUI`: a hopper with the carried block's slot.
    Enderman,
}

/// A chat line a player's next lines go to (the original's chat-based editors).
#[derive(Clone, Debug, PartialEq)]
pub enum ChatEdit {
    /// `/npc text`: each line typed is a line the NPC says (`TextEditor`).
    Text(i32),
    /// The configurator's name button: the next line is the new name.
    Rename(i32),
}

/// The equipment slots of the menu's row 1 (`GenericEquipperGUI`): hand, off hand, helmet, chestplate,
/// leggings, boots, body, saddle; row 2 holds the cosmetic item of each.
const EQUIPMENT_MENU: [&str; 8] = ["hand", "off_hand", "helmet", "chestplate", "leggings", "boots", "body", "saddle"];

/// The equipment cell an item goes to: by its name's ending, the shield or elytra, else the hand.
fn equipment_cell_for(id: &str) -> usize {
    let name = id.trim_start_matches("minecraft:");
    if name.ends_with("_helmet") || name.ends_with("_head") || name == "turtle_helmet" {
        2
    } else if name.ends_with("_chestplate") || name == "elytra" {
        3
    } else if name.ends_with("_leggings") {
        4
    } else if name.ends_with("_boots") {
        5
    } else if name == "shield" {
        1
    } else if name.ends_with("_horse_armor") || name.ends_with("_wolf_armor") || name.ends_with("_nautilus_armor") || name.ends_with("_carpet") || name == "wolf_armor" {
        6
    } else if name == "saddle" {
        7
    } else {
        0
    }
}

/// What a click in an open menu asks the glue to do.
#[derive(Clone, Debug, PartialEq)]
pub enum MenuAction {
    Nothing,
    /// Give this stack to the player (it left the menu; the menu is already refreshed).
    Take(StackData),
    /// Offer the stack in this player-inventory slot (`Inventory` numbers) to the menu: read it, call
    /// [`World::menu_deposit`], and empty the slot when that answers true.
    Deposit { inv_slot: u32 },
    /// Read the stack in this player-inventory slot and hand it to [`World::menu_read`]; the slot stays as it is.
    Read { inv_slot: u32 },
}

#[derive(Clone, Debug)]
struct MenuSession {
    npc: i32,
    kind: MenuKind,
    rows: u8,
    window: Option<i32>,
    slots: Vec<Option<StackData>>,
    chances: Vec<f64>,
    /// The equipment menu: which slots have no cosmetic item (shown as "No cosmetic").
    cosmetic_off: [bool; 8],
    /// A hopper menu (five slots) instead of a chest of `rows` rows.
    hopper: bool,
    /// The item cost menu: the command it edits (`None`: the trait's own cost).
    cmd_key: Option<String>,
}

impl MenuSession {
    /// A chest menu of `rows` rows.
    fn chest(npc: i32, kind: MenuKind, rows: u8, slots: Vec<Option<StackData>>, chances: Vec<f64>) -> MenuSession {
        MenuSession { npc, kind, rows, window: None, slots, chances, cosmetic_off: [false; 8], hopper: false, cmd_key: None }
    }

    /// The number of the menu's own slots.
    fn size(&self) -> usize {
        if self.hopper { 5 } else { usize::from(self.rows) * 9 }
    }

    /// `DropsTrait.DropsGUI`: items live in rows 0, 2 and 4; rows 1 and 3 hold the chance barriers.
    fn is_item_slot(&self, slot: usize) -> bool {
        match self.kind {
            MenuKind::Inventory | MenuKind::ItemCost => true,
            MenuKind::Drops => (slot / 9) % 2 == 0,
            MenuKind::Equipment => (9..9 + EQUIPMENT_MENU.len()).contains(&slot) || ((18..18 + EQUIPMENT_MENU.len()).contains(&slot) && !self.cosmetic_off[slot - 18]),
            MenuKind::Config => false,
            MenuKind::Saddle | MenuKind::Enderman => slot == 1,
        }
    }

    fn shown(&self) -> Vec<Option<StackData>> {
        let mut out = self.slots.clone();
        let unused = || {
            let mut b = StackData::new("minecraft:barrier", 1);
            b.meta.display_name = Some("Unused".to_owned());
            Some(b)
        };
        if self.kind == MenuKind::Config {
            let mut b = StackData::new("minecraft:name_tag", 1);
            b.meta.display_name = Some("Edit name".to_owned());
            out[0] = Some(b);
            return out;
        }
        if matches!(self.kind, MenuKind::Saddle | MenuKind::Enderman) {
            let (icon, lore) = if self.kind == MenuKind::Saddle { ("minecraft:saddle", "Place a saddle here ->") } else { ("minecraft:ender_pearl", "Place a block to hold here ->") };
            let mut b = StackData::new(icon, 1);
            b.meta.lore = vec![lore.to_owned()];
            out[0] = Some(b);
            for cell in out.iter_mut().skip(2) {
                *cell = unused();
            }
            return out;
        }
        if self.kind == MenuKind::Equipment {
            let labels = ["Place in hand item below", "Place offhand item below", "Place helmet below", "Place chestplate below", "Place leggings below", "Place boots below", "Place body item below", "Place saddle item below"];
            let icons = ["minecraft:diamond_sword", "minecraft:shield", "minecraft:diamond_helmet", "minecraft:diamond_chestplate", "minecraft:diamond_leggings", "minecraft:diamond_boots", "minecraft:elytra", "minecraft:saddle"];
            for (i, cell) in out.iter_mut().enumerate() {
                if i < 8 {
                    let mut b = StackData::new(icons[i], 1);
                    b.meta.lore = vec![labels[i].to_owned()];
                    *cell = Some(b);
                } else if (18..26).contains(&i) && self.cosmetic_off[i - 18] {
                    let mut b = StackData::new("minecraft:barrier", 1);
                    b.meta.display_name = Some("No cosmetic".to_owned());
                    b.meta.lore = vec!["Click to enable cosmetic for this equipment".to_owned()];
                    *cell = Some(b);
                } else if !self.is_item_slot(i) {
                    *cell = unused();
                }
            }
            return out;
        }
        if self.kind == MenuKind::Drops {
            for (i, cell) in out.iter_mut().enumerate() {
                if !self.is_item_slot(i) {
                    let pct = if i >= 9 && self.slots[i - 9].is_some() { (self.chances[i - 9] * 100.0 + 1e-6).floor() as i64 } else { 100 };
                    let mut b = StackData::new("minecraft:barrier", 1);
                    b.meta.display_name = Some(format!("Drop chance {pct}%"));
                    *cell = Some(b);
                }
            }
        }
        out
    }
}

/// What other mods hear (see `api`).
#[derive(Clone, Debug, PartialEq)]
pub enum NpcEvent {
    RightClick { npc: i32, uuid: u128, player: u128, player_name: String },
    LeftClick { npc: i32, uuid: u128, player: u128, player_name: String },
    Spawned { npc: i32, entity: u32, dimension: String },
    Despawned { npc: i32, reason: DespawnReason },
    Damaged { npc: i32, entity: u32, amount: f32, damage_type: String, causing: i32 },
    Died { npc: i32 },
    Created { npc: i32 },
    Removed { npc: i32 },
    Selected { npc: i32, player: u128 },
    /// A waypoint or `navigate_to` goal was reached.
    NavigationComplete { npc: i32 },
    /// A shop item was bought (`NPCShopPurchaseEvent`): the shop's key (a global name, or an NPC's UUID), the buyer,
    /// and the page and slot of the item.
    ShopPurchase { shop: String, player: u128, player_name: String, page: usize, slot: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DespawnReason {
    /// `/npc despawn` or the API.
    Command,
    /// Its chunk unloaded.
    ChunkUnload,
    /// It died (a non-protected NPC).
    Death,
    /// The NPC was removed.
    Removal,
    /// The server stops.
    Stop,
    /// Moved to another world.
    Teleport,
}

/// An NPC's presence in the world.
#[derive(Clone, Debug, Default)]
struct Live {
    dimension: String,
    /// The entity, once the game told it joined.
    entity: Option<u32>,
    /// The tick its summon was sent, while the entity has not joined yet.
    summoned: Option<u64>,
    holograms: Vec<u128>,
    look: Option<(f32, f32)>,
    next_random_look: u64,
    /// The player it looks at now (`LookClose.lookingAt`), kept until it leaves range or is switched.
    look_target: Option<u128>,
    /// The tick `randomswitchtargets` may pick another target.
    next_switch: u64,
    /// Where the NPC stands now (moves with teleports and walking).
    at: Option<Location>,
    /// The last tick it took damage (to tell a death from a chunk unload).
    hurt: Option<u64>,
    /// `Entity.moveDist` and `nextStep`: horizontal distance walked × 0.6, and where the next
    /// footstep sounds.
    walked: f64,
    next_step: f64,
}

/// Walking toward a point (waypoints and `navigate_to`).
#[derive(Clone, Debug)]
struct Walk {
    target: [f64; 3],
    /// Blocks per tick.
    speed: f64,
    /// The waypoint index it walks to, for the linear waypoint provider.
    waypoint: Option<usize>,
    /// Set by `followtrait`: the target moves, and arriving is not a finished route.
    follow: bool,
    /// `pathto --margin`: the walk is over this close to the target (`distanceMargin`); 0 keeps the default.
    margin: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sender {
    pub uuid: Option<u128>,
    pub name: String,
    pub permission: u8,
    /// Where the sender stands (none for the console).
    pub at: Option<(String, Location)>,
    /// The `citizens.*` nodes the permissions hook granted this sender. Only asked for a sender
    /// below the command level (operators pass without them).
    pub grants: Vec<String>,
}

pub struct World {
    pub cfg: Config,
    pub saves: Saves,
    pub saves_path: Option<PathBuf>,
    live: BTreeMap<i32, Live>,
    by_entity: HashMap<u32, i32>,
    walks: BTreeMap<i32, Walk>,
    /// NPCs whose entity takes paths (`mob_set_navigation_ticks` accepted): they walk with the game's
    /// pathfinding instead of straight lines.
    real: BTreeMap<i32, RealNav>,
    /// The trade each open merchant screen was selected on: player -> (window, trade index).
    merchants: HashMap<u128, (Option<i32>, Option<usize>)>,
    /// NPCs that died and wait for a respawn through the API.
    dead: BTreeSet<i32>,
    ops: Vec<Op>,
    events: Vec<NpcEvent>,
    /// Each player's selected NPC.
    selected: HashMap<u128, i32>,
    /// Per command (player, npc, key): tick of the last use (`cooldown`, the global default delay).
    cmd_last: HashMap<(u128, i32, String), u64>,
    /// Per command (npc, key): tick of the last use by anybody (`globalcooldown`).
    cmd_global: HashMap<(i32, String), u64>,
    /// Per command (player, npc, key): times used (`n`).
    cmd_used: HashMap<(u128, i32, String), i64>,
    /// Per command (npc, key): times used by anybody (`gn`, only counted, as in Citizens).
    cmd_global_uses: HashMap<(i32, String), i64>,
    /// SEQUENTIAL: the id of the last command a player ran on an NPC; CYCLE: of the NPC (player 0).
    cmd_last_id: HashMap<(u128, i32), i64>,
    /// Commands waiting for their `delay`: (due tick, op).
    delayed: Vec<(u64, Op)>,
    /// Command permissions the clicking player holds (the host asked before the click).
    pub held_perms: BTreeSet<String>,
    /// The stack in the main hand of the player who runs `setequipment` (set by the host before the command).
    pub hand_item: Option<crate::horizon_citizens_core::npc::StackData>,
    /// The same stack as SNBT (`debug -i`).
    pub hand_snbt: Option<String>,
    /// The clicking player's level and inventory (the host reads them before the click).
    pub player_cost: Option<PlayerCost>,
    /// `MountTrait.currentMount`: NPC id -> (its entity, the entity it sits on).
    mounted: HashMap<i32, (u32, u32)>,
    menus: HashMap<u128, MenuSession>,
    /// The stored shops (`shops.yml`), the file's path and whether it needs writing.
    pub shops: crate::horizon_citizens_core::shop::StoredShops,
    shops_path: Option<PathBuf>,
    shops_dirty: bool,
    shop_ui: shop_ui::ShopUi,
    /// What the host read about a player just before a shop interaction.
    pub shop_player: HashMap<u128, ShopPlayer>,
    /// Players with the NPC copier on: the NPC they copy.
    pub copiers: HashMap<u128, i32>,
    /// Players with the linear waypoint editor on: the NPC they edit.
    pub path_editors: HashMap<u128, i32>,
    /// Players whose next chat lines go to a chat-based editor.
    pub chat_edits: HashMap<u128, ChatEdit>,
    /// Where online players look (yaw, pitch), set by the host every tick.
    pub rotations: HashMap<u128, [f32; 2]>,
    /// `rotate -s`: NPC -> the yaw and pitch it is turning to.
    smooth: HashMap<i32, (f32, f32)>,
    /// Entities that hurt a followed one (`follow -p`): where they stand, set by the host.
    pub foes: HashMap<u128, (String, [f64; 3])>,
    foe_entity: HashMap<u128, u32>,
    /// `lookclose -r`: (NPC, player) -> whether the NPC's eyes see the player (`hasLineOfSight`), set by the host.
    pub sight: HashMap<(i32, u128), bool>,
    /// The nodes of lookclose `--filter` permission/group clauses each online player holds, set by the host.
    pub player_perms: HashMap<u128, BTreeSet<String>>,
    /// `Text.cooldowns`: (NPC, player) -> the tick it may talk to that player again.
    talk_cd: HashMap<(i32, u128), u64>,
    /// `WardenTrait.anger`: NPC id -> entity uuid -> anger.
    warden_anger: HashMap<i32, BTreeMap<u128, i32>>,
    teams_made: BTreeSet<String>,
    fetching: BTreeSet<i32>,
    pub tick: u64,
    /// (sender) -> tick until which a second `/citizens reload` counts as the confirmation.
    pub reload_warned: HashMap<u128, u64>,
    /// The players of the last tick (names for `/npc follow`, `tpto`, `rotate`).
    pub online: Vec<PlayerView>,
    /// Names of players who may be offline (UUID -> name), looked up by the glue before a command.
    pub profiles: HashMap<u128, String>,
    /// NPC -> tick of its next wander.
    wander_at: HashMap<i32, u64>,
    dirty_since: Option<u64>,
    rng: Rng,
    pub stopping: bool,
    /// Creates and removes, newest last, for `/npc undo`.
    history: Vec<Hist>,
    /// What the last deposit into a menu left with the player.
    deposit_left: i64,
    /// Players in the equipment editor of an NPC (`/npc equip` for a sheep or a horse): their next right click on it equips.
    pub equip_editors: HashMap<u128, i32>,
    /// Whether an economy provider answers (the glue sets it): shop money buttons, command and NPC costs.
    pub economy: bool,
    /// `Economy.format`, the glue's, for shop descriptions.
    pub money_fmt: Option<fn(f64) -> String>,
    /// Takes the cost of creating an NPC from a player's account (the glue sets it with the economy): the formatted amount,
    /// or the economy's reason it refused.
    pub targets: BTreeMap<i32, (u128, bool)>,
    /// Temporary NPCs that go at a tick (`create --temporaryduration`).
    temp_expire: Vec<(u64, i32)>,
    pub create_charge: Option<fn(u128, f64) -> Result<String, String>>,
    /// The behavior trees of the NPCs that have the `behavior` trait.
    behaviors: BTreeMap<i32, behave::BehaviorRun>,
    signals: crate::horizon_citizens_core::behavior::Signals,
    /// Health and motion the glue read for NPCs with a behavior tree.
    live_stats: HashMap<i32, behave::LiveStats>,
}

/// One step `/npc undo` can take back.
enum Hist {
    Created(i32),
    Removed(NpcData),
}

/// How many steps `/npc undo` remembers.
const HISTORY_LIMIT: usize = 100;

impl World {
    pub fn new(cfg: Config, saves: Saves, saves_path: Option<PathBuf>) -> World {
        // `shops.yml` lives beside `saves.yml`; an unreadable one is left alone (no path, nothing written).
        let mut shops_path = saves_path.as_ref().and_then(|p| p.parent()).map(|d| d.join("shops.yml"));
        let shops = match &shops_path {
            Some(p) => match crate::horizon_citizens_core::shop::StoredShops::load(p, &cfg.shops) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("[citizens] could not read shops: {e}; shops are off until it is fixed");
                    shops_path = None;
                    crate::horizon_citizens_core::shop::StoredShops::empty()
                }
            },
            None => crate::horizon_citizens_core::shop::StoredShops::empty(),
        };
        World {
            cfg,
            saves,
            saves_path,
            live: BTreeMap::new(),
            by_entity: HashMap::new(),
            walks: BTreeMap::new(),
            real: BTreeMap::new(),
            merchants: HashMap::new(),
            dead: BTreeSet::new(),
            ops: Vec::new(),
            events: Vec::new(),
            selected: HashMap::new(),
            cmd_last: HashMap::new(),
            cmd_global: HashMap::new(),
            cmd_used: HashMap::new(),
            cmd_global_uses: HashMap::new(),
            cmd_last_id: HashMap::new(),
            delayed: Vec::new(),
            held_perms: BTreeSet::new(),
            hand_item: None,
            hand_snbt: None,
            player_cost: None,
            mounted: HashMap::new(),
            menus: HashMap::new(),
            shops,
            shops_path,
            shops_dirty: false,
            shop_ui: shop_ui::ShopUi::default(),
            shop_player: HashMap::new(),
            copiers: HashMap::new(),
            path_editors: HashMap::new(),
            chat_edits: HashMap::new(),
            rotations: HashMap::new(),
            smooth: HashMap::new(),
            foes: HashMap::new(),
            foe_entity: HashMap::new(),
            sight: HashMap::new(),
            player_perms: HashMap::new(),
            talk_cd: HashMap::new(),
            warden_anger: HashMap::new(),
            teams_made: BTreeSet::new(),
            fetching: BTreeSet::new(),
            tick: 0,
            reload_warned: HashMap::new(),
            online: Vec::new(),
            profiles: HashMap::new(),
            wander_at: HashMap::new(),
            dirty_since: None,
            rng: Rng::new(0x5EED_C17E),
            stopping: false,
            history: Vec::new(),
            deposit_left: 0,
            equip_editors: HashMap::new(),
            economy: false,
            money_fmt: None,
            create_charge: None,
            targets: BTreeMap::new(),
            temp_expire: Vec::new(),
            behaviors: BTreeMap::new(),
            signals: crate::horizon_citizens_core::behavior::Signals::default(),
            live_stats: HashMap::new(),
        }
    }

    pub fn take_ops(&mut self) -> Vec<Op> {
        std::mem::take(&mut self.ops)
    }
    pub fn take_events(&mut self) -> Vec<NpcEvent> {
        std::mem::take(&mut self.events)
    }
    pub fn push_op(&mut self, op: Op) {
        self.ops.push(op);
    }
    /// A player's name for a UUID (online, else looked up), else the UUID as text.
    pub fn player_name(&self, uuid: u128) -> String {
        self.online
            .iter()
            .find(|p| p.uuid == uuid)
            .map(|p| p.name.clone())
            .or_else(|| self.profiles.get(&uuid).cloned())
            .unwrap_or_else(|| crate::horizon_citizens_core::ids::format_uuid(uuid))
    }
    /// The UUID of a player by name (online, else looked up).
    pub fn player_by_name(&self, name: &str) -> Option<u128> {
        self.online
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(name))
            .map(|p| p.uuid)
            .or_else(|| self.profiles.iter().find(|(_, n)| n.eq_ignore_ascii_case(name)).map(|(u, _)| *u))
    }
    pub fn mark_dirty(&mut self) {
        self.dirty_since.get_or_insert(self.tick);
    }

    /// Writes `saves.yml` now; the error for the log.
    pub fn save_now(&mut self) -> Result<(), String> {
        self.dirty_since = None;
        let mut result = match &self.saves_path {
            Some(p) => self.saves.save(p),
            None => Ok(()),
        };
        if self.shops_dirty {
            match &self.shops_path {
                Some(p) => match self.shops.save(p) {
                    Ok(()) => self.shops_dirty = false,
                    Err(e) => result = result.and(Err(e)),
                },
                None => self.shops_dirty = false,
            }
        }
        result
    }

    /// `/citizens reload`: the shops are read again.
    pub fn reload_shops(&mut self) -> Result<(), String> {
        let Some(p) = &self.shops_path else { return Ok(()) };
        self.shops = crate::horizon_citizens_core::shop::StoredShops::load(p, &self.cfg.shops)?;
        self.shops_dirty = false;
        Ok(())
    }

    // ---- lookups ------------------------------------------------------------------------

    pub fn npc_of_entity(&self, entity: u32) -> Option<i32> {
        self.by_entity.get(&entity).copied()
    }
    pub fn entity_of(&self, npc: i32) -> Option<u32> {
        self.live.get(&npc).and_then(|l| l.entity)
    }
    pub fn dimension_of(&self, npc: i32) -> Option<String> {
        self.live.get(&npc).map(|l| l.dimension.clone()).or_else(|| {
            let loc = self.saves.get(npc)?.location()?;
            self.cfg.worlds.dimension(&loc.world)
        })
    }
    /// Whether NPC `npc` has been put into the world (summoned), even if its entity has not
    /// joined the level yet.
    pub fn is_out(&self, npc: i32) -> bool {
        self.live.contains_key(&npc)
    }
    pub fn is_spawned(&self, npc: i32) -> bool {
        self.entity_of(npc).is_some()
    }
    /// The entity ids of protected NPCs, for the damage hook (which has no host and must not
    /// wait for this world's lock).
    pub fn protected_entities(&self) -> Vec<u32> {
        self.live
            .iter()
            .filter(|(id, _)| self.saves.get(**id).is_some_and(NpcData::protected))
            .filter_map(|(_, l)| l.entity)
            .collect()
    }
    /// `follow -p`: (entity followed, NPC that protects it) for the NPCs out in the world.
    pub fn protectees(&self) -> Vec<(u32, i32)> {
        self.saves
            .npcs
            .iter()
            .filter(|n| n.follow_protect() && self.is_out(n.id))
            .filter_map(|n| {
                let who = n.follow_target()?;
                let entity = self
                    .online
                    .iter()
                    .find(|p| p.uuid == who)
                    .map(|p| p.entity_id as u32)
                    .or_else(|| self.entity_of(self.saves.by_uuid(who)?.id))?;
                Some((entity, n.id))
            })
            .collect()
    }
    /// A followed entity was hurt (`FollowTrait.onEntityDamage`): its protectors go for the damager `foe`
    /// (uuid, entity, position) and keep after it until it is gone.
    pub fn protect_hit(&mut self, victim: u32, foe: Option<(u128, u32, [f64; 3])>) {
        let Some((uuid, entity, pos)) = foe else { return };
        let protectors: Vec<i32> = self.protectees().into_iter().filter(|(e, _)| *e == victim).map(|(_, n)| n).collect();
        for id in protectors {
            if self.saves.get(id).and_then(NpcData::uuid) == Some(uuid) {
                continue;
            }
            self.set_target(id, uuid, true);
        }
        let dimension = self.online.iter().find(|p| p.entity_id as u32 == victim).map(|p| p.dimension.clone()).unwrap_or_default();
        self.foe_entity.insert(uuid, entity);
        self.foes.insert(uuid, (dimension, pos));
    }
    /// The foes the host tracks: (uuid, entity).
    pub fn foe_entities(&self) -> Vec<(u128, u32)> {
        self.foe_entity.iter().map(|(u, e)| (*u, *e)).collect()
    }
    /// Where a foe stands now, or None when it is gone (the NPCs let it go).
    pub fn foe_report(&mut self, uuid: u128, pos: Option<[f64; 3]>) {
        match pos {
            Some(p) => {
                if let Some(f) = self.foes.get_mut(&uuid) {
                    f.1 = p;
                }
            }
            None => {
                self.foes.remove(&uuid);
                self.foe_entity.remove(&uuid);
                let gone: Vec<i32> = self.targets.iter().filter(|(_, t)| t.0 == uuid).map(|(id, _)| *id).collect();
                for id in gone {
                    self.clear_target(id);
                }
            }
        }
    }
    pub fn npc_entities(&self) -> Vec<u32> {
        self.by_entity.keys().copied().collect()
    }
    /// Where an NPC is now (its location while out of the world).
    pub fn current_location(&self, npc: i32) -> Option<Location> {
        self.live
            .get(&npc)
            .and_then(|l| l.at.clone())
            .or_else(|| self.saves.get(npc)?.location())
    }
    pub fn selected(&self, player: u128) -> Option<i32> {
        self.selected.get(&player).copied().filter(|id| self.saves.get(*id).is_some())
    }
    pub fn deselect(&mut self, player: u128) {
        self.selected.remove(&player);
    }
    pub fn select(&mut self, player: u128, npc: i32) {
        self.selected.insert(player, npc);
        self.events.push(NpcEvent::Selected { npc, player });
    }

    // ---- spawning -----------------------------------------------------------------------

    fn ensure_team(&mut self, color: &str) {
        let key = color.to_ascii_lowercase();
        if self.teams_made.insert(key) {
            self.ops.extend(spawn::team_setup(color).into_iter().map(Op::Console));
        }
    }

    /// Sends the summon of NPC `id` at its location (when it has one in a known world).
    pub fn spawn(&mut self, id: i32) -> bool {
        let Some(npc) = self.saves.get(id).cloned() else {
            return false;
        };
        let uuid = match npc.uuid() {
            Some(u) => u,
            None => {
                // Review C8: an old or hand-made entry without (or with an unreadable) `uuid`
                // gets one, written back at the next save, instead of never spawning.
                let u = crate::horizon_citizens_core::ids::random_uuid();
                eprintln!("[citizens] NPC {id} had no readable uuid: given a new one");
                if let Some(n) = self.saves.get_mut(id) {
                    n.set_uuid(u);
                }
                self.mark_dirty();
                u
            }
        };
        let Some(loc) = self.current_location(id) else {
            return false;
        };
        let Some(dimension) = self.cfg.worlds.dimension(&loc.world) else {
            return false;
        };
        self.dead.remove(&id);
        if npc.is_player()
            && !npc.skin().has_texture()
            && self.cfg.skins.fetch
            && !self.fetching.contains(&id)
        {
            let name = npc
                .skin()
                .name
                .filter(|n| !n.is_empty())
                .or_else(|| self.cfg.skins.try_fetch_default_skin.then(|| text::strip(&npc.name())));
            if let Some(name) = name.filter(|n| valid_player_name(n)) {
                self.fetching.insert(id);
                self.ops.push(Op::FetchSkin { npc: id, name });
            }
        }
        self.ops
            .push(Op::Console(spawn::summon(&npc, uuid, &dimension, &loc, &self.cfg)));
        let holograms = self.hologram_ops(&npc, uuid, &dimension, &loc);
        let live = self.live.entry(id).or_default();
        live.dimension = dimension;
        live.summoned = Some(self.tick);
        live.at = Some(loc);
        live.look = None;
        live.holograms = holograms.iter().map(|(u, _)| *u).collect();
        live.holograms.dedup();
        for (_, cmd) in holograms {
            self.ops.push(Op::Console(cmd));
        }
        // A default pose (`/npc pose --default`) is held from the start.
        if let Some((yaw, pitch)) = npc.default_pose() {
            self.face(id, yaw, pitch);
        }
        true
    }

    fn hologram_ops(&self, npc: &NpcData, uuid: u128, dimension: &str, loc: &Location) -> Vec<(u128, String)> {
        let mut out = Vec::new();
        for (u, cmd) in spawn::hologram(npc, uuid, dimension, loc, &self.cfg) {
            // A copy saved with its chunk would make the summon fail (same UUID).
            out.push((u, spawn::kill(u)));
            out.push((u, cmd));
        }
        out
    }

    /// Puts the hologram lines up again (after a change or a move).
    pub fn refresh_holograms(&mut self, id: i32) {
        let Some(npc) = self.saves.get(id).cloned() else {
            return;
        };
        let (Some(uuid), Some(live)) = (npc.uuid(), self.live.get(&id).cloned()) else {
            return;
        };
        for u in &live.holograms {
            self.ops.push(Op::Console(spawn::kill(*u)));
        }
        let Some(loc) = live.at.clone() else {
            return;
        };
        let ops = self.hologram_ops(&npc, uuid, &live.dimension, &loc);
        if let Some(l) = self.live.get_mut(&id) {
            l.holograms = ops.iter().map(|(u, _)| *u).collect();
            l.holograms.dedup();
        }
        self.ops.extend(ops.into_iter().map(|(_, c)| Op::Console(c)));
    }

    /// Takes NPC `id` out of the world.
    pub fn despawn(&mut self, id: i32, reason: DespawnReason) {
        let Some(live) = self.live.remove(&id) else {
            return;
        };
        self.walks.remove(&id);
        self.real.remove(&id);
        match live.entity {
            Some(e) => {
                self.by_entity.remove(&e);
                self.ops.push(Op::RemoveMob {
                    dimension: live.dimension.clone(),
                    entity: e,
                });
            }
            None => {
                if let Some(u) = self.saves.get(id).and_then(NpcData::uuid) {
                    self.ops.push(Op::Console(spawn::kill(u)));
                }
            }
        }
        for u in live.holograms {
            self.ops.push(Op::Console(spawn::kill(u)));
        }
        self.events.push(NpcEvent::Despawned { npc: id, reason });
    }

    /// Respawns an NPC so a change that needs a new entity (skin, type) shows.
    pub fn respawn(&mut self, id: i32) {
        if self.live.contains_key(&id) {
            self.despawn(id, DespawnReason::Command);
        }
        if self.saves.get(id).is_some_and(NpcData::spawned) {
            self.spawn(id);
        }
    }

    /// Sends the in-place flags (glow, protection, gravity, name) of a spawned NPC again.
    pub fn refresh(&mut self, id: i32) {
        let Some(npc) = self.saves.get(id).cloned() else {
            return;
        };
        let (Some(uuid), Some(live)) = (npc.uuid(), self.live.get(&id)) else {
            return;
        };
        self.ops.push(Op::Console(spawn::refresh(&npc, uuid, &live.dimension)));
        if let Some(color) = npc.team_key() {
            self.ensure_team(&color);
            self.ops.push(Op::Console(spawn::team_join(&color, uuid)));
        }
    }

    /// An entity joined a level. Returns true to cancel it: a second copy of an NPC that is out
    /// already, or a leftover of an NPC that no longer exists (`tagged`: it carries
    /// [`spawn::TAG`]).
    pub fn on_entity_join(&mut self, dimension: &str, entity: u32, uuid: Option<u128>, tagged: bool) -> bool {
        let npc = uuid.and_then(|u| self.saves.by_uuid(u)).map(|n| (n.id, n.clone()));
        let Some((id, npc)) = npc else {
            return tagged;
        };
        if let Some(live) = self.live.get(&id)
            && let Some(e) = live.entity
            && e != entity
        {
            return true;
        }
        if !npc.spawned() || self.dead.contains(&id) {
            return true;
        }
        let fresh = self.live.get(&id).is_none_or(|l| l.summoned.is_none());
        let live = self.live.entry(id).or_default();
        live.dimension = dimension.to_owned();
        live.entity = Some(entity);
        live.summoned = None;
        if live.at.is_none() {
            live.at = npc.location();
        }
        self.by_entity.insert(entity, id);
        if fresh {
            // A copy that came back with its chunk: bring it up to date.
            self.refresh(id);
            self.refresh_holograms(id);
        } else if let (Some(color), Some(u)) = (npc.team_key(), npc.uuid()) {
            self.ensure_team(&color);
            self.ops.push(Op::Console(spawn::team_join(&color, u)));
        }
        self.push_settings(id, entity);
        self.events.push(NpcEvent::Spawned {
            npc: id,
            entity,
            dimension: dimension.to_owned(),
        });
        false
    }

    /// The settings of an NPC's entity that are set through host calls (see `entity_set`), and the
    /// request to let a `NoAI` mob follow paths (real pathfinding).
    fn push_settings(&mut self, id: i32, entity: u32) {
        let Some(npc) = self.saves.get(id) else { return };
        let kind = spawn::entity_kind(&npc.kind());
        let mut sets = entity_set::settings(npc, &kind);
        if !npc.is_player() {
            sets.push(EntitySet::NavigationTicks(true));
        }
        let carried = npc.uuid().zip(npc.carried_block()).filter(|_| kind == "minecraft:enderman");
        self.ops.extend(sets.into_iter().map(|set| Op::Entity { entity, set }));
        if let Some((uuid, block)) = carried {
            self.ops.push(Op::Console(carried_command(uuid, Some(&block))));
        }
    }

    /// An NPC's saved settings changed: send them to its entity again.
    pub fn apply_settings(&mut self, id: i32) {
        if let Some(entity) = self.entity_of(id) {
            self.push_settings(id, entity);
        }
    }

    /// A host call for the NPC's entity, now (`/npc warden dig`); false when it is not in the world.
    pub fn entity_op(&mut self, id: i32, set: EntitySet) -> bool {
        let Some(entity) = self.entity_of(id) else { return false };
        self.ops.push(Op::Entity { entity, set });
        true
    }

    /// The host let the entity follow paths: from now on its walks use the game's pathfinding.
    pub fn nav_ready(&mut self, entity: u32) {
        if let Some(id) = self.by_entity.get(&entity).copied() {
            self.real.insert(id, RealNav::default());
        }
    }

    /// The NPCs that follow paths and their entities (the glue reads where they stand every tick).
    pub fn real_entities(&self) -> Vec<(i32, u32)> {
        self.real.keys().filter_map(|id| self.entity_of(*id).map(|e| (*id, e))).collect()
    }

    /// Where a path-following NPC's entity stands, and whether its path is finished.
    pub fn nav_report(&mut self, id: i32, pos: [f64; 3], done: bool) {
        if let Some(r) = self.real.get_mut(&id) {
            r.pos = Some(pos);
            r.done = done;
        }
    }

    /// Whether the NPC walks with the game's pathfinding (false: straight lines).
    pub fn has_real_pathing(&self, id: i32) -> bool {
        self.real.contains_key(&id)
    }

    /// An entity left a level: an NPC's chunk unloaded, it died, or the mod removed it.
    pub fn on_entity_leave(&mut self, entity: u32) {
        let Some(id) = self.by_entity.remove(&entity) else {
            return;
        };
        let Some(live) = self.live.get(&id) else {
            return;
        };
        if live.entity != Some(entity) {
            return;
        }
        let died = live.hurt.is_some_and(|t| self.tick.saturating_sub(t) <= 2);
        let live = self.live.remove(&id).unwrap_or_default();
        self.walks.remove(&id);
        self.real.remove(&id);
        if died {
            self.play_sound(id, "death");
            for u in live.holograms {
                self.ops.push(Op::Console(spawn::kill(u)));
            }
            self.dead.insert(id);
            self.events.push(NpcEvent::Died { npc: id });
            self.events.push(NpcEvent::Despawned {
                npc: id,
                reason: DespawnReason::Death,
            });
        } else {
            // The hologram displays unload with the chunk and are put up again on respawn.
            self.events.push(NpcEvent::Despawned {
                npc: id,
                reason: DespawnReason::ChunkUnload,
            });
        }
    }

    /// An NPC entity took damage (from the damage hook, after the protection check).
    pub fn on_damaged(&mut self, entity: u32, amount: f32, damage_type: &str, causing: i32) {
        let Some(id) = self.by_entity.get(&entity).copied() else {
            return;
        };
        self.play_sound(id, "hurt");
        if let Some(l) = self.live.get_mut(&id) {
            l.hurt = Some(self.tick);
        }
        self.events.push(NpcEvent::Damaged {
            npc: id,
            entity,
            amount,
            damage_type: damage_type.to_owned(),
            causing,
        });
    }

    /// Takes every NPC out before the server saves its worlds, so none is saved with a chunk.
    pub fn stop(&mut self) {
        self.stopping = true;
        let ids: Vec<i32> = self.live.keys().copied().collect();
        for id in ids {
            self.despawn(id, DespawnReason::Stop);
        }
    }

    // ---- clicks -------------------------------------------------------------------------

    /// A player right-clicked (`Hand::Right`) or hit (`Hand::Left`) an entity. Returns whether it
    /// is an NPC: the click is then the mod's (the game does nothing more with it for a right
    /// click, and for a hit on a protected NPC).
    /// Says one of the NPC's talk lines (`/npc text add`), picked at random, to the player who
    /// right-clicked it.
    fn talk(&mut self, player: &PlayerView, npc: &crate::horizon_citizens_core::npc::NpcData) {
        let lines = npc.talk_lines();
        if lines.is_empty() {
            return;
        }
        let pick = ((self.rng.next_f64() * lines.len() as f64) as usize).min(lines.len() - 1);
        let text = self.cfg.talk.format.replace("<npc>", &npc.name()).replace("<text>", &lines[pick]);
        let legacy = self.cfg.messages.format(&text);
        self.ops.push(Op::Tell {
            player: player.uuid,
            legacy,
        });
        let id = npc.id;
        self.say_to_bystanders(id, &lines[pick], &[player.uuid]);
    }

    /// `CitizensNPC.speak` for the players who were not addressed (`talkToBystanders`): with no recipients everyone
    /// within `npc.chat.options.range` hears the line; with recipients, bystanders hear it only when
    /// `bystanders-hear-targeted-chat` is on, in the bystander format (the list of several names is cut by
    /// `multiple-targets-format` and `max-number-of-targets-to-show`).
    pub fn say_to_bystanders(&mut self, id: i32, message: &str, targets: &[u128]) {
        let cfg = self.cfg.talk.clone();
        if !targets.is_empty() && !cfg.bystanders_hear_targeted_chat {
            return;
        }
        let (Some(at), Some(dim)) = (self.current_location(id), self.dimension_of(id)) else { return };
        let name = self.saves.get(id).map(NpcData::name).unwrap_or_default();
        let template = match targets.len() {
            0 => cfg.format.clone(),
            1 => cfg.with_target_to_bystanders.replace("<target>", &self.player_name(targets[0])),
            _ => {
                let names: Vec<String> = targets.iter().map(|t| self.player_name(*t)).collect();
                let list = text::format_targets(&cfg.multiple_targets_format, &names, cfg.max_targets).unwrap_or_default();
                cfg.with_targets_to_bystanders.replace("<targets>", &list)
            }
        };
        let line = template.replace("<npc>", &text::strip(&name)).replace("<text>", message);
        let legacy = self.cfg.messages.format(&line);
        let near: Vec<u128> = self
            .online
            .iter()
            .filter(|p| p.dimension == dim && !targets.contains(&p.uuid) && look::dist2([at.x, at.y, at.z], p.pos) <= cfg.range * cfg.range)
            .map(|p| p.uuid)
            .collect();
        for player in near {
            self.ops.push(Op::Tell { player, legacy: legacy.clone() });
        }
    }

    pub fn on_click(&mut self, entity: u32, player: &PlayerView, hand: Hand, holding: Option<&str>) -> bool {
        let Some(id) = self.npc_of_entity(entity) else {
            return false;
        };
        let Some(npc) = self.saves.get(id).cloned() else {
            return false;
        };
        let uuid = npc.uuid().unwrap_or(0);
        if hand == Hand::Right
            && player.permission >= self.cfg.command_level
            && holding.is_some_and(|h| same_item(h, &self.cfg.selection.item))
        {
            self.select(player.uuid, id);
            let msg = self
                .cfg
                .selection
                .message
                .replace("<npc>", &npc.name())
                .replace("<id>", &id.to_string());
            let line = self.cfg.messages.format(&msg);
            self.ops.push(Op::Tell {
                player: player.uuid,
                legacy: line,
            });
            return true;
        }
        if hand == Hand::Right && self.equip_editors.get(&player.uuid) == Some(&id) {
            // The equipment editor's `Equipper`: this click equips the NPC instead of talking to it.
            self.equipper_click(player, &npc, id, holding);
            return true;
        }
        if hand == Hand::Right {
            self.talk(player, &npc);
        }
        let event = match hand {
            Hand::Left => NpcEvent::LeftClick {
                npc: id,
                uuid,
                player: player.uuid,
                player_name: player.name.clone(),
            },
            _ => NpcEvent::RightClick {
                npc: id,
                uuid,
                player: player.uuid,
                player_name: player.name.clone(),
            },
        };
        self.events.push(event);
        if hand == Hand::Right && npc.has_trait("shop") {
            self.shop_right_click(player.uuid, id);
        }
        let commands: Vec<_> = npc.commands().into_iter().filter(|c| c.hand.matches(hand, player.sneaking)).collect();
        if !self.charge_trait(player, &npc) {
            return true;
        }
        if !commands.is_empty() {
            self.run_commands(player, hand, &npc, id, commands);
        }
        true
    }

    /// `CommandTrait.dispatch`: the trait's own price is charged first; a click that cannot pay stops there.
    fn charge_trait(&mut self, player: &PlayerView, npc: &NpcData) -> bool {
        if !npc.has_trait("commandtrait") {
            return true;
        }
        let em = self.cfg.commands.error_messages.clone();
        let price = Price { money: npc.trait_cost(), exp: npc.trait_experience_cost(), items: npc.trait_item_cost(), global: true };
        let mut sent = BTreeSet::new();
        match self.check_charge(player, npc, &price, &em, &mut sent) {
            Some((levels, slots, money)) => {
                self.pay(player.uuid, levels, slots, money);
                true
            }
            None => false,
        }
    }

    /// `/npc command execute <player> <hand>` (`CommandTrait.dispatch(player, hand)`): the commands of one hand run
    /// for an online player as if they had clicked. A sneaking player's hand becomes its shift form; only the
    /// commands of exactly that hand (and `BOTH`) run. Whether the player is online.
    pub fn dispatch_commands(&mut self, id: i32, who: u128, hand: Hand) -> bool {
        let Some(player) = self.online.iter().find(|p| p.uuid == who).cloned() else { return false };
        let Some(npc) = self.saves.get(id).cloned() else { return false };
        let hand = if player.sneaking {
            if hand == Hand::Left { Hand::ShiftLeft } else { Hand::ShiftRight }
        } else {
            hand
        };
        let commands: Vec<_> = npc.commands().into_iter().filter(|c| c.hand == hand || c.hand == Hand::Both).collect();
        if !self.charge_trait(&player, &npc) {
            return true;
        }
        if !commands.is_empty() {
            self.run_commands(&player, hand, &npc, id, commands);
        }
        true
    }

    /// The permissions the selected NPC's commands ask, for a player the glue looks them up for (`execute`).
    pub fn command_perms_of(&self, id: i32) -> Vec<String> {
        let Some(npc) = self.saves.get(id) else { return Vec::new() };
        let mut v: Vec<String> = npc.commands().into_iter().flat_map(|c| c.perms).collect();
        if !v.is_empty() || !npc.commands().is_empty() || npc.has_trait("commandtrait") {
            v.extend(IGNORE_ERRORS.iter().map(|s| (*s).to_owned()));
            v.extend(COST_IGNORE.iter().map(|s| (*s).to_owned()));
        }
        v.sort();
        v.dedup();
        v
    }

    /// `/npc command forgetplayer`: what the commands remember of one player (or of all): cooldowns, use counts and
    /// the place in a sequence, in memory and in the saved sequence of `rememberlastused`.
    pub fn forget_player(&mut self, id: i32, who: Option<u128>) {
        self.cmd_last.retain(|k, _| k.1 != id || who.is_some_and(|w| w != k.0));
        self.cmd_used.retain(|k, _| k.1 != id || who.is_some_and(|w| w != k.0));
        self.cmd_last_id.retain(|k, _| k.1 != id || k.0 == 0 || who.is_some_and(|w| w != k.0));
        if let Some(n) = self.saves.get_mut(id) {
            n.forget_remembered(who.map(crate::horizon_citizens_core::ids::format_uuid).as_deref());
        }
        self.mark_dirty();
    }

    /// `/npc command clearerror <type> (player)`: forgets what causes one error for a player (or for all).
    /// `kind` is the lower-case name of `CommandTraitError`; the ones that are not remembered clear nothing.
    pub fn clear_command_errors(&mut self, id: i32, kind: &str, who: Option<u128>) {
        let keep = |k: &u128| who.is_some_and(|w| w != *k);
        match kind {
            "maximum_times_used" => self.cmd_used.retain(|k, _| k.1 != id || keep(&k.0)),
            "on_cooldown" => self.cmd_last.retain(|k, _| k.1 != id || keep(&k.0)),
            "on_global_cooldown" => self.cmd_global.retain(|k, _| k.0 != id),
            _ => {}
        }
    }

    /// `SheepEquipper` and `HorseEquipper`: a right click in the equipment editor. Shears toggle the sheared state,
    /// a dye colours the wool (and is used up), anything else makes the wool white; a horse-like NPC opens its inventory.
    fn equipper_click(&mut self, player: &PlayerView, npc: &NpcData, id: i32, holding: Option<&str>) {
        let kind = npc.kind().to_ascii_uppercase();
        let say = |w: &World, t: &str| crate::horizon_citizens_core::commands::say(w, t);
        let lines: Vec<String>;
        if kind == "SHEEP" {
            let held = holding.unwrap_or("").trim_start_matches("minecraft:").to_owned();
            let name = text::strip(&npc.name());
            if held == "shears" {
                let now = npc.map.at(&["traits", "sheeptrait", "sheared"]).and_then(Node::as_bool).unwrap_or(false);
                if let Some(n) = self.saves.get_mut(id) {
                    n.map.put(&["traits", "sheeptrait", "sheared"], Node::bool(!now));
                    n.add_trait_name("sheeptrait");
                }
                lines = say(self, &if now { format!("{name} is no longer sheared.") } else { format!("{name} is now sheared.") });
            } else {
                let colour = held.strip_suffix("_dye").map(str::to_ascii_uppercase).unwrap_or_else(|| "WHITE".to_owned());
                if let Some(n) = self.saves.get_mut(id) {
                    n.map.put(&["traits", "sheeptrait", "color"], Node::str(&colour));
                    n.add_trait_name("sheeptrait");
                }
                if held.ends_with("_dye") {
                    self.ops.push(Op::Console(format!("clear {} minecraft:{held} 1", player.name)));
                }
                lines = say(self, &format!("{name} is now {}.", colour.to_ascii_lowercase().replace('_', " ")));
            }
            self.mark_dirty();
            self.respawn(id);
        } else {
            // A horse-like NPC: its inventory (the engine has no call that opens the mount's own screen).
            if let Some(n) = self.saves.get_mut(id) {
                n.add_trait_name("inventory");
            }
            self.open_inventory(player.uuid, id);
            return;
        }
        for legacy in lines {
            self.ops.push(Op::Tell { player: player.uuid, legacy });
        }
    }

    /// `create -t`: the NPC is not saved; with `ticks` it is removed then (if it is still temporary).
    pub fn make_temporary(&mut self, id: i32, ticks: Option<u64>) {
        self.saves.temporary.insert(id);
        if let Some(t) = ticks {
            self.temp_expire.push((self.tick + t, id));
        }
    }

    /// `/npc target`: the NPC walks to this player until cleared; `aggressive` also hits them there.
    pub fn set_target(&mut self, id: i32, who: u128, aggressive: bool) {
        self.targets.insert(id, (who, aggressive));
        self.cancel_follow_walk(id);
    }

    pub fn clear_target(&mut self, id: i32) {
        self.targets.remove(&id);
        self.cancel_follow_walk(id);
    }

    /// The permissions the commands of the NPC behind `entity` ask (and the `ignoreerrors` nodes), for the
    /// host to look up before a click: see `held_perms`.
    pub fn command_perms(&self, entity: u32) -> Vec<String> {
        let Some(id) = self.npc_of_entity(entity) else { return Vec::new() };
        self.command_perms_of(id)
    }

    fn command_error(&mut self, player: &PlayerView, npc: &NpcData, kind: &str, msg: &str, secs: i64, n: i64, sent: &mut BTreeSet<String>) {
        // `/npc command errormessage` replaces the configured text of one error.
        let custom = npc.custom_error_message(kind);
        let msg = custom.as_deref().unwrap_or(msg);
        if npc.hide_command_errors() || !sent.insert(kind.to_owned()) || msg.trim().is_empty() {
            return;
        }
        let text = msg
            .replace("{minutes}", &(secs / 60).to_string())
            .replace("{seconds_over}", &(secs % 60).to_string())
            .replace("{0}", &if kind.contains("cooldown") { secs } else { n }.to_string());
        let text = text_fill(&text, &player.name, &npc.name());
        self.ops.push(Op::Tell { player: player.uuid, legacy: text });
    }

    /// `CommandTrait.chargeCommandCosts` / `chargeGlobalCommandCosts`: what a price takes from the player (levels,
    /// the slots and the counts they keep, the money), or `None` after the error messages when the player
    /// cannot pay. Every part that cannot be paid says so, as the original's transactions do.
    fn check_charge(
        &mut self,
        player: &PlayerView,
        npc: &NpcData,
        price: &Price,
        em: &crate::horizon_citizens_core::config::ErrorMessages,
        sent: &mut BTreeSet<String>,
    ) -> Option<(i32, Vec<(u32, u32)>, f64)> {
        let held = |n: &str| self.held_perms.contains(n);
        if held(COST_IGNORE[0]) {
            return Some((0, Vec::new(), 0.0));
        }
        let pay_money = price.money > 0.0 && !held(COST_IGNORE[1]);
        let pay_xp = if price.global { price.exp > 0 } else { price.exp != -1 } && !held(COST_IGNORE[2]);
        let pay_items = !price.items.is_empty() && !held(COST_IGNORE[3]);
        let have = self.player_cost.clone().unwrap_or_default();
        let mut possible = true;
        let mut levels = 0;
        let mut money = 0.0;
        let mut slots = Vec::new();
        if pay_money {
            if have.money < price.money {
                let shown = if price.money == price.money.trunc() { format!("{}", price.money as i64) } else { price.money.to_string() };
                let msg = em.not_enough_money.replace("{0}", &shown);
                self.command_error(player, npc, "missing_money", &msg, 0, 0, sent);
                possible = false;
            } else {
                money = price.money;
            }
        }
        if pay_xp {
            if i64::from(have.levels) < price.exp {
                self.command_error(player, npc, "missing_experience", &em.not_enough_experience, 0, price.exp, sent);
                possible = false;
            } else {
                levels = price.exp as i32;
            }
        }
        if pay_items {
            match plan_item_charge(&have.slots, &price.items) {
                Some(plan) => slots = plan,
                None => {
                    let first = &price.items[0];
                    let msg = em.missing_item.replace("{1}", &first.amount.to_string()).replace("{0}", &first.name);
                    self.command_error(player, npc, "missing_item", &msg, 0, 0, sent);
                    possible = false;
                }
            }
        }
        possible.then_some((levels, slots, money))
    }

    /// `Transaction.run` of a charge: the rest of this click sees what is left, and the glue takes it.
    fn pay(&mut self, player: u128, levels: i32, slots: Vec<(u32, u32)>, money: f64) {
        if levels <= 0 && slots.is_empty() && money <= 0.0 {
            return;
        }
        if let Some(pc) = self.player_cost.as_mut() {
            pc.levels -= levels;
            pc.money -= money;
            for (slot, keep) in &slots {
                if let Some(e) = pc.slots.iter_mut().find(|e| e.0 == *slot) {
                    e.2 = *keep;
                }
            }
            pc.slots.retain(|e| e.2 > 0);
        }
        self.ops.push(Op::Charge { player, levels, slots, money });
    }

    /// `commandtrait`: which commands of a click run (execution mode, `perms`, `cooldown`,
    /// `globalcooldown`, `n`, `gn`), and when (`delay`).
    fn run_commands(&mut self, player: &PlayerView, hand: Hand, npc: &NpcData, id: i32, mut commands: Vec<NpcCommand>) {
        use crate::horizon_citizens_core::npc::ExecutionMode;
        commands.sort_by_key(|c| c.id);
        let mode = npc.execution_mode();
        let max = commands.last().map_or(-1, |c| c.id);
        let mut sent = BTreeSet::new();
        let list: Vec<NpcCommand> = match mode {
            ExecutionMode::Random => {
                let pick = ((self.rng.next_f64() * commands.len() as f64) as usize).min(commands.len() - 1);
                vec![commands[pick].clone()]
            }
            ExecutionMode::Linear => commands,
            ExecutionMode::Sequential | ExecutionMode::Cycle => {
                let who = if mode == ExecutionMode::Cycle { 0 } else { player.uuid };
                let remembered = if mode == ExecutionMode::Sequential && npc.remember_last_used() {
                    npc.remembered_last_id(&crate::horizon_citizens_core::ids::format_uuid(player.uuid))
                } else {
                    None
                };
                let mut last = self.cmd_last_id.get(&(who, id)).copied().or(remembered).unwrap_or(-1);
                let mut picked = None;
                for c in &commands {
                    if c.id <= last {
                        if last != max {
                            continue;
                        }
                        last = -1;
                    }
                    picked = Some(c.clone());
                    break;
                }
                picked.into_iter().collect()
            }
        };
        let sequential = matches!(mode, ExecutionMode::Sequential | ExecutionMode::Cycle);
        for c in list {
            let em = self.cfg.commands.error_messages.clone();
            // `chargeCommandCosts`: the price is checked first; one that cannot be paid stops the click.
            let price = Price { money: c.cost, exp: c.experience_cost, items: c.item_cost.clone(), global: false };
            let (charge_levels, charge_slots, charge_money) = match self.check_charge(player, npc, &price, &em, &mut sent) {
                Some(plan) => plan,
                None => break,
            };
            if c.perms.iter().any(|p| !self.held_perms.contains(p)) {
                self.command_error(player, npc, "no_permission", &em.no_permission, 0, 0, &mut sent);
                continue;
            }
            let now = self.tick;
            let global_delay = u64::from(self.cfg.commands.global_cooldown_ticks);
            let key = (player.uuid, id, c.key.clone());
            let gkey = (id, c.key.clone());
            let ig: Vec<bool> = IGNORE_ERRORS.iter().map(|p| self.held_perms.contains(*p)).collect();
            let own_ticks = if c.cooldown != 0 { c.cooldown.max(0) as u64 * 20 } else { global_delay };
            if !ig[0]
                && let Some(&t) = self.cmd_last.get(&key)
            {
                if now < t + own_ticks {
                    let secs = ((t + own_ticks - now) as i64 + 19) / 20;
                    self.command_error(player, npc, "on_cooldown", &em.on_cooldown, secs, 0, &mut sent);
                    continue;
                }
                self.cmd_last.remove(&key);
            }
            if !ig[1]
                && c.global_cooldown > 0
                && let Some(&t) = self.cmd_global.get(&gkey)
            {
                let span = c.global_cooldown as u64 * 20;
                if now < t + span {
                    let secs = ((t + span - now) as i64 + 19) / 20;
                    self.command_error(player, npc, "on_global_cooldown", &em.on_global_cooldown, secs, 0, &mut sent);
                    continue;
                }
                self.cmd_global.remove(&gkey);
            }
            let used = self.cmd_used.get(&key).copied().unwrap_or(0);
            if !ig[2] && c.n > 0 && c.n <= used {
                self.command_error(player, npc, "maximum_times_used", &em.maximum_times_used, 0, c.n, &mut sent);
                continue;
            }
            if c.cooldown > 0 || global_delay > 0 {
                self.cmd_last.insert(key.clone(), now);
            }
            if c.global_cooldown > 0 {
                self.cmd_global.insert(gkey.clone(), now);
            }
            if c.n > 0 {
                self.cmd_used.insert(key, used + 1);
            }
            if c.gn > 0 {
                *self.cmd_global_uses.entry(gkey).or_insert(0) += 1;
            }
            if sequential {
                let who = if mode == ExecutionMode::Cycle { 0 } else { player.uuid };
                self.cmd_last_id.insert((who, id), c.id);
                if mode == ExecutionMode::Sequential && npc.remember_last_used() {
                    let me = crate::horizon_citizens_core::ids::format_uuid(player.uuid);
                    if let Some(n) = self.saves.get_mut(id) {
                        n.set_remembered_last_id(&me, c.id);
                    }
                    self.mark_dirty();
                }
            }
            self.pay(player.uuid, charge_levels, charge_slots, charge_money);
            let line = text::fill_placeholders(&c.command, &player.name, &npc.name(), id);
            let line = line.trim_start_matches('/').to_owned();
            // `CommandTrait.NPCCommand.run`: `npc` swaps the clicker for the NPC when the NPC is a player.
            let npc_uuid = npc.uuid().filter(|_| c.npc && c.player && npc.kind().eq_ignore_ascii_case("PLAYER"));
            let op = if let Some(me) = npc_uuid {
                if let Some(entity) = self.entity_of(id) {
                    // The NPC's own entity runs it: no operator rights unless the command is `-o`.
                    Op::EntityCommand { entity, line, level: if c.op { 4 } else { 0 } }
                } else {
                    let who = crate::horizon_citizens_core::ids::format_uuid(me);
                    Op::Console(format!("execute as {who} at {who} run {line}"))
                }
            } else if c.player || c.op {
                if self.cfg.commands.player_commands_via_execute {
                    let who = crate::horizon_citizens_core::ids::format_uuid(player.uuid);
                    Op::Console(format!("execute as {who} at {who} run {line}"))
                } else {
                    eprintln!("[citizens] NPC {id}: player command skipped (player-commands-via-execute is off): {line}");
                    continue;
                }
            } else {
                Op::Console(line)
            };
            // `temporaryPermissions`: the player holds them while the command runs (or for the set duration).
            let (temp, duration) = npc.temporary_permissions();
            let mut before = Vec::new();
            let mut after = Vec::new();
            if !temp.is_empty() {
                let who = crate::horizon_citizens_core::ids::format_uuid(player.uuid);
                let secs = if duration > 0 { (duration + 19) / 20 } else { 60 };
                for node in &temp {
                    before.push(Op::Console(format!("lp user {who} permission settemp {node} true {secs}s")));
                    if duration <= 0 {
                        after.push(Op::Console(format!("lp user {who} permission unsettemp {node}")));
                    }
                }
            }
            if c.delay > 0 {
                let due = now + c.delay as u64;
                for o in before.into_iter().chain([op]).chain(after) {
                    self.delayed.push((due, o));
                }
            } else {
                self.ops.extend(before);
                self.ops.push(op);
                self.ops.extend(after);
            }
        }
        let _ = hand;
    }

    /// Whether a hit on this entity is refused (a protected NPC).
    pub fn hit_refused(&self, entity: u32) -> bool {
        self.npc_of_entity(entity)
            .and_then(|id| self.saves.get(id))
            .is_some_and(NpcData::protected)
    }

    // ---- moving -------------------------------------------------------------------------

    /// Moves an NPC (spawned or not) to `loc`; a spawned one in another world is respawned there.
    pub fn teleport(&mut self, id: i32, loc: Location) -> bool {
        let Some(dimension) = self.cfg.worlds.dimension(&loc.world) else {
            return false;
        };
        let Some(npc) = self.saves.get_mut(id) else {
            return false;
        };
        npc.set_location(&loc);
        let uuid = npc.uuid();
        self.mark_dirty();
        self.walks.remove(&id);
        let Some(live) = self.live.get_mut(&id) else {
            return true;
        };
        if live.dimension != dimension {
            self.despawn(id, DespawnReason::Teleport);
            self.spawn(id);
            return true;
        }
        live.at = Some(loc.clone());
        live.look = None;
        if let Some(u) = uuid {
            self.ops.push(Op::Console(spawn::teleport(u, &dimension, &loc)));
        }
        self.refresh_holograms(id);
        true
    }

    /// Starts walking toward `target` at `speed` blocks per tick (the API's and the waypoints').
    pub fn navigate_to(&mut self, id: i32, target: [f64; 3], speed: f64) -> bool {
        if !self.is_out(id) {
            return false;
        }
        self.walks.insert(
            id,
            Walk {
                target,
                speed: speed.clamp(0.01, 2.0),
                waypoint: None,
                follow: false,
                margin: 0.0,
            },
        );
        true
    }
    /// `pathto --margin`: the running walk ends this many blocks from its target.
    pub fn set_walk_margin(&mut self, id: i32, margin: f64) {
        if let Some(w) = self.walks.get_mut(&id) {
            w.margin = margin.max(0.0);
        }
    }
    /// Drops a walk set by following or wandering (the setting changed).
    pub fn cancel_follow_walk(&mut self, id: i32) {
        if self.walks.get(&id).is_some_and(|w| w.follow || w.waypoint.is_none()) {
            self.walks.remove(&id);
        }
    }
    pub fn cancel_navigation(&mut self, id: i32) {
        if self.walks.remove(&id).is_some() {
            self.stop_path(id);
        }
    }
    /// Takes the path the game is following away (real pathfinding only).
    fn stop_path(&mut self, id: i32) {
        if let Some(r) = self.real.get_mut(&id) {
            r.goal = None;
            if let Some(entity) = self.live.get(&id).and_then(|l| l.entity) {
                self.ops.push(Op::Entity { entity, set: EntitySet::StopNavigation });
            }
        }
    }
    pub fn is_navigating(&self, id: i32) -> bool {
        self.walks.contains_key(&id)
    }

    /// Turns a spawned NPC's head and body (the API's; look-close does the same).
    pub fn face(&mut self, id: i32, yaw: f32, pitch: f32) {
        let Some(uuid) = self.saves.get(id).and_then(NpcData::uuid) else {
            return;
        };
        let Some(live) = self.live.get_mut(&id) else {
            return;
        };
        live.look = Some((yaw, pitch));
        if let Some(at) = &mut live.at {
            at.yaw = yaw;
            at.pitch = pitch;
        }
        let fallback = spawn::rotate(uuid, &live.dimension, yaw, pitch);
        self.ops.push(match live.entity {
            Some(entity) => Op::Rotate { dimension: live.dimension.clone(), entity, yaw, pitch, fallback },
            None => Op::Console(fallback),
        });
    }

    /// `rotate -s`: turn to `yaw`/`pitch` over several ticks (RotationTrait's physical session, `rotateToHave`).
    pub fn rotate_smooth(&mut self, id: i32, yaw: f32, pitch: f32) {
        self.smooth.insert(id, (yaw, pitch));
    }

    /// One tick of every smooth rotation: at most `max_yaw_per_tick` (40) and `max_pitch_per_tick` (10) degrees.
    fn smooth_tick(&mut self) {
        let ids: Vec<i32> = self.smooth.keys().copied().collect();
        for id in ids {
            let Some((ty, tp)) = self.smooth.get(&id).copied() else { continue };
            let Some(live) = self.live.get(&id).filter(|l| l.entity.is_some()) else {
                self.smooth.remove(&id);
                continue;
            };
            let (cy, cp) = live.look.or(live.at.as_ref().map(|a| (a.yaw, a.pitch))).unwrap_or((0.0, 0.0));
            let npc = self.saves.get(id);
            let limit = |key: &str, d: f64| {
                npc.and_then(|n| n.map.at(&["traits", "rotationsettings", key]).and_then(Node::as_f64)).unwrap_or(d) as f32
            };
            let immediate = npc
                .and_then(|n| n.map.at(&["traits", "rotationsettings", "mode"]).and_then(Node::as_str).map(|m| m.eq_ignore_ascii_case("immediate")))
                .unwrap_or(false);
            let lock_pitch = npc.and_then(|n| n.map.at(&["traits", "rotationsettings", "lock_pitch"]).and_then(Node::as_bool)).unwrap_or(false);
            let (my, mp) = (limit("max_yaw_per_tick", 40.0), limit("max_pitch_per_tick", 10.0));
            let step = |from: f32, to: f32, max: f32| {
                let d = look::wrap(to - from);
                if immediate || d.abs() <= max { to } else { look::wrap(from + d.signum() * max) }
            };
            let (ny, np) = (step(cy, ty, my), if lock_pitch { cp } else { step(cp, tp, mp) });
            if look::angle_diff(ny, ty) < 0.1 && (np - tp).abs() < 0.1 || lock_pitch && look::angle_diff(ny, ty) < 0.1 {
                self.smooth.remove(&id);
            }
            self.face(id, ny, np);
        }
    }

    /// Turns only the head (`lookclose --headonly`): the body stays where it faces.
    pub fn face_head(&mut self, id: i32, yaw: f32, pitch: f32) {
        let Some(live) = self.live.get_mut(&id) else { return };
        let Some(entity) = live.entity else { return };
        live.look = Some((yaw, pitch));
        let body_yaw = live.at.as_ref().map_or(yaw, |a| a.body_yaw);
        self.ops.push(Op::RotateHead { entity, body_yaw, yaw, pitch });
    }

    // ---- skins --------------------------------------------------------------------------

    /// A skin lookup finished.
    pub fn skin_fetched(&mut self, fetched: crate::horizon_citizens_core::skin::Fetched) {
        self.fetching.remove(&fetched.npc);
        let Some(npc) = self.saves.get_mut(fetched.npc) else {
            return;
        };
        match fetched.result {
            Ok(s) => {
                let name = npc.skin().name;
                npc.set_skin(&Skin {
                    name,
                    texture: Some(s.texture),
                    signature: s.signature,
                });
                npc.set_cached_skin_profile(s.profile, &s.name);
                self.mark_dirty();
                self.respawn(fetched.npc);
            }
            Err(e) => eprintln!("[citizens] NPC {}: skin {} not found: {e}", fetched.npc, fetched.requested),
        }
    }

    // ---- the tick -----------------------------------------------------------------------

    /// One server tick, with the players online.
    /// `Text.run`: an NPC with `talk-close` says a line to each player within its `range` (5 by default),
    /// then not to that player again for 5 to 10 seconds (`npc.text.default-random-text-delay-*`).
    fn talk_close(&mut self, players: &[PlayerView]) {
        let ids: Vec<i32> = self.live.keys().copied().collect();
        for id in ids {
            let Some(npc) = self.saves.get(id).filter(|n| n.talk_close()).cloned() else { continue };
            let (Some(at), Some(dim)) = (self.current_location(id), self.dimension_of(id)) else { continue };
            if self.entity_of(id).is_none() {
                continue;
            }
            let range = npc.talk_range();
            for p in players.iter().filter(|p| p.dimension == dim) {
                if look::dist2([at.x, at.y, at.z], p.pos) > range * range {
                    continue;
                }
                if self.talk_cd.get(&(id, p.uuid)).is_some_and(|t| self.tick < *t) {
                    continue;
                }
                self.talk(p, &npc);
                let wait = 100 + (self.rng.next_f64() * 100.0) as u64;
                self.talk_cd.insert((id, p.uuid), self.tick + wait);
            }
        }
    }

    /// The dimension an online player stands in.
    pub fn online_dimension(&self, player: u128) -> Option<String> {
        self.online.iter().find(|p| p.uuid == player).map(|p| p.dimension.clone())
    }

    /// `WardenTrait.addAnger`: the warden's `anger` suspects are written to its entity data.
    pub fn warden_anger(&mut self, npc: i32, entity: u128, anger: i32) {
        let map = self.warden_anger.entry(npc).or_default();
        map.insert(entity, anger);
        let suspects: Vec<String> = map
            .iter()
            .map(|(u, a)| format!("{{uuid:{},anger:{a}}}", crate::horizon_citizens_core::ids::int_array_snbt(*u)))
            .collect();
        if let Some(uuid) = self.saves.get(npc).and_then(NpcData::uuid) {
            let who = crate::horizon_citizens_core::ids::format_uuid(uuid);
            self.ops.push(Op::Console(format!("data merge entity {who} {{anger:{{suspects:[{}]}}}}", suspects.join(","))));
        }
    }

    // ---- the inventory and drops menus ----------------------------------------------------

    /// `/npc inventory`: the NPC's `inventory` trait in a chest for `player` (`Inventory.openInventory`).
    pub fn open_inventory(&mut self, player: u128, npc: i32) {
        let Some(data) = self.saves.get(npc) else { return };
        let stacks = data.inventory_stacks();
        // `Inventory.onSpawn`: 36 slots for a player NPC, else the 72 saved ones cut to 54.
        let rows: u8 = if data.kind().eq_ignore_ascii_case("PLAYER") { 4 } else { 6 };
        let mut slots = vec![None; usize::from(rows) * 9];
        for (slot, st) in stacks {
            if let Some(cell) = slots.get_mut(slot.max(0) as usize) {
                *cell = Some(st);
            }
        }
        let name = text::strip(&data.name());
        let name = if name.chars().count() >= 19 { format!("{}~", name.chars().take(19).collect::<String>()) } else { name };
        let title = format!("{name}'s Inventory");
        let chances = vec![1.0; slots.len()];
        self.start_menu(player, title, MenuSession::chest(npc, MenuKind::Inventory, rows, slots, chances));
    }

    /// `/npc drops`: five rows, the drops in rows 0 and 2 with a chance barrier under each.
    pub fn open_drops(&mut self, player: u128, npc: i32) {
        let Some(data) = self.saves.get(npc) else { return };
        let mut slots = vec![None; 45];
        let mut chances = vec![1.0; 45];
        let mut k = 0;
        let drops = data.drops();
        'rows: for i in [0usize, 2] {
            for j in 0..9 {
                let Some((st, chance)) = drops.get(k) else { break 'rows };
                slots[i * 9 + j] = Some(st.clone());
                chances[i * 9 + j] = *chance;
                k += 1;
            }
        }
        self.start_menu(player, "Add items for drops".into(), MenuSession::chest(npc, MenuKind::Drops, 5, slots, chances));
    }

    /// `/npc equip` with no arguments: the equipment menu (`GenericEquipperGUI`). Click one of your stacks to put it on the
    /// NPC (helmet, chestplate, leggings, boots, shield, horse armor, saddle by its name, else the hand); click a worn
    /// stack to take it off. Row 2 holds each slot's cosmetic item: click "No cosmetic" to enable one, an empty
    /// cosmetic cell to turn it off again, a cosmetic stack to take it.
    pub fn open_equipment(&mut self, player: u128, npc: i32) {
        use crate::horizon_citizens_core::npc::Cosmetic;
        let Some(data) = self.saves.get(npc) else { return };
        let mut slots = vec![None; 27];
        let mut off = [false; 8];
        for (i, name) in EQUIPMENT_MENU.iter().enumerate() {
            if let Some(n) = data.map.at(&["traits", "equipment", name]).and_then(StackData::from_node) {
                slots[9 + i] = Some(n);
            }
            match data.cosmetic(name) {
                Cosmetic::Off => off[i] = true,
                Cosmetic::Empty => {}
                Cosmetic::Item(st) => slots[18 + i] = Some(st),
            }
        }
        let mut session = MenuSession::chest(npc, MenuKind::Equipment, 3, slots, vec![1.0; 27]);
        session.cosmetic_off = off;
        self.start_menu(player, "NPC Equipment".into(), session);
    }

    /// `/npc command itemcost (id)`: the stacks of the trait's (or one command's) item cost in a chest; what is in it when
    /// the menu closes is the new cost.
    pub fn open_item_cost(&mut self, player: u128, npc: i32, key: Option<String>) {
        let Some(data) = self.saves.get(npc) else { return };
        let stacks = match &key {
            None => data.trait_item_stacks(),
            Some(k) => data.command_item_stacks(k),
        };
        let mut slots = vec![None; 45];
        for (cell, st) in slots.iter_mut().zip(stacks) {
            *cell = Some(st);
        }
        let mut session = MenuSession::chest(npc, MenuKind::ItemCost, 5, slots, vec![1.0; 45]);
        session.cmd_key = key;
        self.start_menu(player, "Drag items for requirements".into(), session);
    }

    /// `SaddleEquipperGUI`: a hopper with the saddle slot of a pig or strider.
    pub fn open_saddle(&mut self, player: u128, npc: i32) {
        let Some(data) = self.saves.get(npc) else { return };
        let mut slots = vec![None; 5];
        if data.saddle() {
            slots[1] = Some(StackData::new("minecraft:saddle", 1));
        }
        let mut s = MenuSession::chest(npc, MenuKind::Saddle, 1, slots, vec![1.0; 5]);
        s.hopper = true;
        self.start_menu(player, "NPC Equipment".into(), s);
    }

    /// `EndermanEquipperGUI`: a hopper with the block the enderman carries.
    pub fn open_enderman(&mut self, player: u128, npc: i32) {
        let Some(data) = self.saves.get(npc) else { return };
        let mut slots = vec![None; 5];
        if let Some(block) = data.carried_block() {
            slots[1] = Some(StackData::new(&block, 1));
        }
        let mut s = MenuSession::chest(npc, MenuKind::Enderman, 1, slots, vec![1.0; 5]);
        s.hopper = true;
        self.start_menu(player, "NPC Equipment".into(), s);
    }

    /// `/npc configgui` (`NPCConfigurator`): a menu with the one button that renames the NPC.
    pub fn open_configgui(&mut self, player: u128, npc: i32) {
        self.start_menu(player, "NPC Configurator".into(), MenuSession::chest(npc, MenuKind::Config, 1, vec![None; 9], vec![1.0; 9]));
    }

    fn start_menu(&mut self, player: u128, title: String, session: MenuSession) {
        let items = session.shown();
        let rows = session.rows;
        let hopper = session.hopper;
        self.shop_ui.drop_session(player);
        self.menus.insert(player, session);
        if hopper {
            self.ops.push(Op::OpenHopper { player, title, items });
        } else {
            self.ops.push(Op::OpenMenu { player, title, rows, items, in_place: false });
        }
    }

    /// The glue opened the menu: its window id.
    pub fn menu_opened(&mut self, player: u128, window: Option<i32>) {
        if self.shop_opened(player, window) {
            return;
        }
        match window {
            Some(w) => {
                if let Some(m) = self.menus.get_mut(&player) {
                    m.window = Some(w);
                }
            }
            None => {
                self.menus.remove(&player);
            }
        }
    }

    fn refresh_menu(&mut self, player: u128) {
        if let Some(m) = self.menus.get(&player)
            && let Some(window) = m.window
        {
            let items = m.shown();
            self.ops.push(Op::SetMenuItems { player, window, items });
        }
    }

    /// A click in the menu `window` of `player`. The menu's own slots: a stack is taken out; a drops
    /// barrier changes its chance (`PercentageSlotHandler`: left +10, right -10, shift 1). A slot of the player's own inventory
    /// (the chest menu's bottom 36 slots) is offered for deposit.
    pub fn menu_click(&mut self, player: u128, window: i32, slot: i32, button: i32, shift: bool) -> MenuAction {
        if let Some(action) = self.shop_click(player, window, slot, button, shift) {
            return action;
        }
        let Some(m) = self.menus.get_mut(&player).filter(|m| m.window == Some(window)) else {
            return MenuAction::Nothing;
        };
        let size = m.size();
        let Ok(slot) = usize::try_from(slot) else { return MenuAction::Nothing };
        if slot >= size + 36 {
            return MenuAction::Nothing;
        }
        if m.kind == MenuKind::Equipment && (18..26).contains(&slot) {
            // `GenericEquipperGUI.setCosmetic`: the "No cosmetic" button enables one, an empty enabled cell turns it off.
            let i = slot - 18;
            if m.cosmetic_off[i] {
                m.cosmetic_off[i] = false;
                self.refresh_menu(player);
                return MenuAction::Nothing;
            }
            if m.slots[slot].is_none() {
                m.cosmetic_off[i] = true;
                self.refresh_menu(player);
                return MenuAction::Nothing;
            }
        }
        if matches!(m.kind, MenuKind::Saddle | MenuKind::Enderman) && slot < size {
            // The one slot hands its stack back and clears the setting.
            if slot == 1 && m.slots[1].is_some() {
                let (npc, kind) = (m.npc, m.kind);
                let taken = m.slots[1].take();
                self.refresh_menu(player);
                if kind == MenuKind::Saddle {
                    self.saddle_live(npc, false);
                } else {
                    self.carried_live(npc, None);
                }
                return taken.map_or(MenuAction::Nothing, MenuAction::Take);
            }
            return MenuAction::Nothing;
        }
        if m.kind == MenuKind::Config {
            // The name button: close the menu, the next chat line is the name.
            if slot == 0 {
                let npc = m.npc;
                self.menus.remove(&player);
                self.chat_edits.insert(player, ChatEdit::Rename(npc));
                self.ops.push(Op::CloseMenu { player });
                for legacy in crate::horizon_citizens_core::commands::say(self, "Enter the new name of the NPC in the chat.") {
                    self.ops.push(Op::Tell { player, legacy });
                }
            }
            return MenuAction::Nothing;
        }
        if slot >= size {
            let p = slot - size;
            let inv_slot = if p < 27 { 9 + p } else { p - 27 };
            return MenuAction::Deposit { inv_slot: inv_slot as u32 };
        }
        if !m.is_item_slot(slot) {
            if m.kind == MenuKind::Drops && slot >= 9 && m.slots[slot - 9].is_some() {
                // `PercentageSlotHandler`: 10 per click, 1 with shift, right click down, kept in 0..=100.
                let c = &mut m.chances[slot - 9];
                let dx = (if shift { 1 } else { 10 }) * if button == 1 { -1 } else { 1 };
                let pct = ((*c * 100.0 + 1e-6).floor() as i64 + dx).clamp(0, 100);
                *c = pct as f64 / 100.0;
                self.refresh_menu(player);
            }
            return MenuAction::Nothing;
        }
        let Some(taken) = m.slots[slot].take() else { return MenuAction::Nothing };
        m.chances[slot] = 1.0;
        self.refresh_menu(player);
        MenuAction::Take(taken)
    }

    /// Puts `stack` into the first free item slot of the player's open menu; whether there was room.
    pub fn menu_deposit(&mut self, player: u128, stack: StackData) -> bool {
        if let Some(taken) = self.shop_deposit(player, stack.clone()) {
            return taken;
        }
        let Some(m) = self.menus.get_mut(&player) else { return false };
        if m.kind == MenuKind::Config {
            return false;
        }
        if matches!(m.kind, MenuKind::Saddle | MenuKind::Enderman) {
            // One saddle, or one block, into the hopper's slot; the rest of the stack stays with the player.
            let ok = m.slots[1].is_none() && (m.kind == MenuKind::Enderman || stack.id == "minecraft:saddle");
            if !ok {
                return false;
            }
            let (npc, kind) = (m.npc, m.kind);
            m.slots[1] = Some(StackData::new(&stack.id, 1));
            self.deposit_left = stack.count - 1;
            self.refresh_menu(player);
            if kind == MenuKind::Saddle {
                self.saddle_live(npc, true);
            } else {
                self.carried_live(npc, Some(&stack.id));
            }
            return true;
        }
        let free = if m.kind == MenuKind::Equipment {
            let c = equipment_cell_for(&stack.id);
            if m.slots[9 + c].is_none() {
                Some(9 + c)
            } else if !m.cosmetic_off[c] && m.slots[18 + c].is_none() {
                Some(18 + c)
            } else {
                None
            }
        } else {
            (0..m.slots.len()).find(|i| m.is_item_slot(*i) && m.slots[*i].is_none())
        };
        let Some(i) = free else { return false };
        m.slots[i] = Some(stack);
        m.chances[i] = 1.0;
        self.refresh_menu(player);
        true
    }

    /// How much of the last deposited stack the menu left with the player (a hopper slot takes one).
    pub fn take_deposit_left(&mut self) -> i64 {
        std::mem::take(&mut self.deposit_left)
    }

    /// The saddle trait changed through a menu: saved, and shown on the entity at once.
    fn saddle_live(&mut self, npc: i32, on: bool) {
        let Some(n) = self.saves.get_mut(npc) else { return };
        n.set_saddle(on);
        let uuid = n.uuid();
        self.mark_dirty();
        if let Some(u) = uuid.filter(|_| self.is_out(npc)) {
            let id = crate::horizon_citizens_core::ids::format_uuid(u);
            let line = if on {
                format!("data merge entity {id} {{equipment:{{saddle:{{id:\"minecraft:saddle\",count:1}}}}}}")
            } else {
                format!("data remove entity {id} equipment.saddle")
            };
            self.ops.push(Op::Console(line));
        }
    }

    /// The block an enderman carries changed through a menu: saved (as the equipment trait's hand), and shown at once.
    fn carried_live(&mut self, npc: i32, block: Option<&str>) {
        let Some(n) = self.saves.get_mut(npc) else { return };
        n.set_equipment("hand", block.map(|b| (b, 1)));
        let uuid = n.uuid();
        self.mark_dirty();
        if let Some(u) = uuid.filter(|_| self.is_out(npc)) {
            self.ops.push(Op::Console(carried_command(u, block)));
        }
    }

    /// The menu was closed: the NPC keeps what it shows (`DropsGUI.onClose`, the inventory's close).
    pub fn menu_closed(&mut self, player: u128, window: i32) {
        if self.shop_closed(player, window) {
            return;
        }
        if self.menus.get(&player).is_none_or(|m| m.window != Some(window)) {
            return;
        }
        let Some(m) = self.menus.remove(&player) else { return };
        let Some(npc) = self.saves.get_mut(m.npc) else { return };
        match m.kind {
            MenuKind::Inventory => npc.set_inventory(
                m.slots.into_iter().enumerate().filter_map(|(i, s)| Some((i as i64, s?))).collect(),
            ),
            MenuKind::Equipment => {
                use crate::horizon_citizens_core::npc::Cosmetic;
                for (i, name) in EQUIPMENT_MENU.iter().enumerate() {
                    match &m.slots[9 + i] {
                        Some(st) => npc.map.put(&["traits", "equipment", name], st.to_node()),
                        None => {
                            npc.map.drop_path(&["traits", "equipment", name]);
                        }
                    }
                    let cosmetic = if m.cosmetic_off[i] {
                        Cosmetic::Off
                    } else {
                        match &m.slots[18 + i] {
                            Some(st) => Cosmetic::Item(st.clone()),
                            None => Cosmetic::Empty,
                        }
                    };
                    npc.set_cosmetic(name, &cosmetic);
                }
                npc.add_trait_name("equipment");
                let id = m.npc;
                self.mark_dirty();
                self.respawn(id);
                return;
            }
            MenuKind::ItemCost => {
                let stacks: Vec<StackData> = m.slots.into_iter().flatten().collect();
                match &m.cmd_key {
                    None => npc.set_trait_item_stacks(&stacks),
                    Some(k) => {
                        npc.set_command_item_stacks(k, &stacks);
                    }
                }
            }
            MenuKind::Config | MenuKind::Saddle | MenuKind::Enderman => {}
            MenuKind::Drops => npc.set_drops(
                m.slots
                    .into_iter()
                    .zip(m.chances)
                    .enumerate()
                    .filter_map(|(i, (s, c))| if (i / 9) % 2 == 0 { Some((s?, c)) } else { None })
                    .collect(),
            ),
        }
        self.mark_dirty();
    }

    /// `MountTrait.checkMounted`: seats the NPC on the NPC its `mountedon` names, once both are spawned
    /// (again after either respawned, its entity being new).
    pub fn check_mounted(&mut self, id: i32) {
        let Some(mine) = self.entity_of(id) else {
            self.mounted.remove(&id);
            return;
        };
        let on = self
            .saves
            .get(id)
            .and_then(|n| n.map.at(&["traits", "mounttrait", "mountedon"]).and_then(Node::text))
            .and_then(|s| crate::horizon_citizens_core::ids::parse_uuid(&s));
        let Some(vehicle) = on.and_then(|u| self.saves.by_uuid(u)).map(|n| n.id).and_then(|o| self.entity_of(o)) else {
            return;
        };
        if self.mounted.get(&id) != Some(&(mine, vehicle)) {
            self.mounted.insert(id, (mine, vehicle));
            self.ops.push(Op::Ride { player: None, rider: mine, vehicle });
        }
    }

    fn check_all_mounts(&mut self) {
        let ids: Vec<i32> = self.saves.npcs.iter().map(|n| n.id).collect();
        for id in ids {
            self.check_mounted(id);
        }
    }

    /// `MountTrait.unmount`: off the vehicle and forgotten, when it sits on one.
    pub fn unmount(&mut self, id: i32) {
        if self.mounted.remove(&id).is_none() {
            return;
        }
        if let Some(entity) = self.entity_of(id) {
            self.ops.push(Op::Dismount { player: None, entity });
        }
        if let Some(n) = self.saves.get_mut(id) {
            n.map.drop_path(&["traits", "mounttrait", "mountedon"]);
        }
        self.mark_dirty();
    }

    pub fn tick(&mut self, players: &[PlayerView]) {
        self.tick += 1;
        if self.stopping {
            return;
        }
        self.online = players.to_vec();
        self.spawn_near(players);
        self.follow_and_wander();
        if self.tick % 20 == 0 {
            // Vanilla mobs voice their ambient sound every few seconds.
            let ids: Vec<i32> = self.live.keys().copied().collect();
            for id in ids {
                if self.rng.next_f64() < 0.25 {
                    self.play_sound(id, "ambient");
                }
            }
        }
        self.walk();
        self.smooth_tick();
        self.behavior_tick();
        self.check_all_mounts();
        self.shop_tick();
        if self.tick % 5 == 0 {
            self.talk_close(players);
        }
        if self.tick % 2 == 0 {
            self.look_close(players);
        }
        if let Some(since) = self.dirty_since
            && self.tick.saturating_sub(since) >= u64::from(self.cfg.save_delay_seconds) * 20
            && let Err(e) = self.save_now()
        {
            eprintln!("[citizens] could not save NPCs: {e}");
        }
        if !self.temp_expire.is_empty() {
            let now = self.tick;
            let (due, wait): (Vec<_>, Vec<_>) = std::mem::take(&mut self.temp_expire).into_iter().partition(|(t, _)| *t <= now);
            self.temp_expire = wait;
            for (_, id) in due {
                if self.saves.temporary.contains(&id) {
                    self.remove_unrecorded(id);
                    self.saves.temporary.remove(&id);
                }
            }
        }
        if !self.delayed.is_empty() {
            let now = self.tick;
            let (due, wait): (Vec<_>, Vec<_>) = std::mem::take(&mut self.delayed).into_iter().partition(|(t, _)| *t <= now);
            self.delayed = wait;
            self.ops.extend(due.into_iter().map(|(_, op)| op));
        }
        if self.tick % 1200 == 0 {
            let tick = self.tick;
            // A cooldown over an hour ago is over whatever it was.
            self.cmd_last.retain(|_, t| tick.saturating_sub(*t) < 72_000);
            self.cmd_global.retain(|_, t| tick.saturating_sub(*t) < 72_000);
        }
    }

    /// NPCs that should be out and have a player close enough that their chunk is loaded.
    fn spawn_near(&mut self, players: &[PlayerView]) {
        if self.tick % 20 != 1 {
            return;
        }
        let r2 = self.cfg.spawn_radius * self.cfg.spawn_radius;
        let mut want = Vec::new();
        for npc in &self.saves.npcs {
            if !npc.spawned() || self.dead.contains(&npc.id) {
                continue;
            }
            if let Some(l) = self.live.get(&npc.id) {
                // Out, or summoned less than five seconds ago.
                if l.entity.is_some() || l.summoned.is_some_and(|t| self.tick.saturating_sub(t) < 100) {
                    continue;
                }
            }
            let Some(loc) = npc.location() else {
                continue;
            };
            let Some(dim) = self.cfg.worlds.dimension(&loc.world) else {
                continue;
            };
            let near = players.iter().any(|p| {
                p.dimension == dim && look::dist2(p.pos, [loc.x, loc.y, loc.z]) <= r2
            });
            if near {
                want.push(npc.id);
            }
        }
        for id in want {
            self.spawn(id);
        }
    }

    /// Nodes the host must look up per player for the `lookclose --filter` of live NPCs.
    pub fn look_filter_nodes(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .live
            .iter()
            .filter(|(_, l)| l.entity.is_some())
            .filter_map(|(id, _)| self.saves.get(*id)?.lookclose_text("filter"))
            .flat_map(|f| look::filter_nodes(&f))
            .collect();
        v.sort();
        v.dedup();
        v
    }

    /// The out `lookclose -r` NPCs: (id, eye, dimension), for the host's line-of-sight checks.
    pub fn sight_wanted(&self) -> Vec<(i32, [f64; 3], String)> {
        self.live
            .iter()
            .filter(|(_, l)| l.entity.is_some())
            .filter(|(id, _)| self.saves.get(**id).is_some_and(|n| n.look_close() && n.lookclose_flag("realisticlooking", false)))
            .filter_map(|(id, l)| {
                let at = l.at.as_ref()?;
                Some((*id, look::eye([at.x, at.y, at.z]), l.dimension.clone()))
            })
            .collect()
    }

    fn look_close(&mut self, players: &[PlayerView]) {
        let mut turns = Vec::new();
        let mut tracked: Vec<(i32, Option<u128>, u64)> = Vec::new();
        for (id, live) in &self.live {
            if live.entity.is_none() {
                continue;
            }
            let Some(npc) = self.saves.get(*id) else {
                continue;
            };
            // `disablewhilenavigating` is on unless saved off.
            if self.walks.contains_key(id) && npc.lookclose_flag("disablewhilenavigating", true) {
                continue;
            }
            if !npc.look_close() {
                continue;
            }
            let Some(at) = &live.at else {
                continue;
            };
            let range = npc.look_range().unwrap_or(self.cfg.look_close.range);
            let from = look::eye([at.x, at.y, at.z]);
            // `targetnpcs`: the other NPCs that stand out count as players.
            let others: Vec<PlayerView> = if npc.lookclose_flag("targetnpcs", false) {
                self.live
                    .iter()
                    .filter(|(oid, o)| **oid != *id && o.entity.is_some() && o.dimension == live.dimension)
                    .filter_map(|(oid, o)| {
                        let l = o.at.as_ref()?;
                        Some(PlayerView {
                            uuid: u128::MAX - *oid as u128,
                            name: String::new(),
                            dimension: o.dimension.clone(),
                            pos: [l.x, l.y, l.z],
                            game_mode: 0,
                            permission: 0,
                            entity_id: 0,
                            sneaking: false,
                        })
                    })
                    .collect()
            } else {
                Vec::new()
            };
            let pool: Vec<PlayerView> = if others.is_empty() { Vec::new() } else { players.iter().cloned().chain(others).collect() };
            let seen: &[PlayerView] = if pool.is_empty() { players } else { &pool };
            // `getNearbyPlayers`: in range, not a spectator, passing the filter and (realistic) in sight.
            let filter = npc.lookclose_text("filter");
            let realistic = npc.lookclose_flag("realisticlooking", false);
            let near: Vec<&PlayerView> = seen
                .iter()
                .filter(|p| p.dimension == live.dimension && p.game_mode != 3)
                .filter(|p| look::dist2(look::eye(p.pos), from) <= range * range)
                .filter(|p| filter.as_deref().is_none_or(|f| look::filter_passes(f, p, &self.player_perms)))
                .filter(|p| !realistic || self.sight.get(&(*id, p.uuid)).copied().unwrap_or(true))
                .collect();
            // `findNewTarget`: keep the target while it is valid; `randomswitchtargets` picks another one
            // every random-look delay; otherwise the nearest.
            let mut target = live.look_target.filter(|t| near.iter().any(|p| p.uuid == *t));
            let mut next_switch = live.next_switch;
            if target.is_some() && npc.lookclose_flag("randomSwitchTargets", false) && self.tick >= live.next_switch {
                let others: Vec<&&PlayerView> = near.iter().filter(|p| Some(p.uuid) != target).collect();
                if !others.is_empty() {
                    let i = ((self.rng.next_f64() * others.len() as f64) as usize).min(others.len() - 1);
                    target = Some(others[i].uuid);
                    next_switch = self.tick + u64::from(npc.random_look_delay().unwrap_or(self.cfg.look_close.random_look_delay_ticks));
                }
            }
            if target.is_none() {
                target = near
                    .iter()
                    .min_by(|a, b| look::dist2(look::eye(a.pos), from).total_cmp(&look::dist2(look::eye(b.pos), from)))
                    .map(|p| p.uuid);
            }
            tracked.push((*id, target, next_switch));
            let want = match target.and_then(|t| near.iter().find(|p| p.uuid == t)) {
                Some(p) => Some(look::look_at(from, look::eye(p.pos))),
                None if npc.random_look() && self.tick >= live.next_random_look => {
                    let ((y0, y1), (p0, p1)) = npc.random_ranges();
                    Some((
                        look::wrap(self.rng.between(y0, y1) as f32),
                        self.rng.between(p0, p1) as f32,
                    ))
                }
                None => None,
            };
            let Some((yaw, pitch)) = want else {
                continue;
            };
            let min = self.cfg.look_close.min_change_degrees;
            let changed = live
                .look
                .is_none_or(|(y, p)| look::angle_diff(y, yaw) >= min || (p - pitch).abs() >= min);
            if changed {
                turns.push((*id, yaw, pitch));
            }
        }
        for (id, target, next_switch) in tracked {
            if let Some(l) = self.live.get_mut(&id) {
                l.look_target = target;
                l.next_switch = next_switch;
            }
        }
        let default_delay = u64::from(self.cfg.look_close.random_look_delay_ticks);
        for (id, yaw, pitch) in turns {
            let delay = self.saves.get(id).and_then(NpcData::random_look_delay).map_or(default_delay, u64::from);
            if let Some(l) = self.live.get_mut(&id) {
                l.next_random_look = self.tick + delay;
            }
            // `linkedbody`: the body follows the head, so a head-only turn turns everything.
            if self.saves.get(id).is_some_and(|n| n.lookclose_flag("headonly", false) && !n.lookclose_flag("linkedbody", false)) {
                self.face_head(id, yaw, pitch);
            } else {
                self.face(id, yaw, pitch);
            }
        }
    }

    /// Plays the NPC's `hurt`, `death` or `ambient` sound (`/npc sound`) where it stands.
    fn play_sound(&mut self, id: i32, kind: &str) {
        let Some(sound) = self.saves.get(id).and_then(|n| n.sound(kind)) else {
            return;
        };
        let Some(at) = self.current_location(id) else {
            return;
        };
        let Some(dim) = self.dimension_of(id) else {
            return;
        };
        self.ops.push(Op::Console(format!(
            "execute in {dim} run playsound {sound} neutral @a {} {} {} 1 1",
            at.x, at.y, at.z
        )));
    }

    /// Where entity `uuid` (an online player or an NPC) stands, with its dimension.
    pub fn locate(&self, uuid: u128) -> Option<(String, [f64; 3])> {
        if let Some(p) = self.online.iter().find(|p| p.uuid == uuid) {
            return Some((p.dimension.clone(), p.pos));
        }
        if let Some((d, p)) = self.foes.get(&uuid) {
            return Some((d.clone(), *p));
        }
        let id = self.saves.by_uuid(uuid)?.id;
        let l = self.current_location(id)?;
        Some((self.dimension_of(id)?, [l.x, l.y, l.z]))
    }

    /// `followtrait` (walk toward the followed entity until within the margin) and the wander
    /// provider (a random point in the x/y box around the NPC every `delay` ticks), as walks in
    /// a straight line (Horizon's pathfinding only drives mobs with AI; the NPCs have none).
    fn follow_and_wander(&mut self) {
        let mut set: Vec<(i32, [f64; 3], f64, bool)> = Vec::new();
        let mut clear: Vec<i32> = Vec::new();
        let mut hits: Vec<(i32, u128)> = Vec::new();
        for npc in &self.saves.npcs {
            let id = npc.id;
            if !self.is_out(id) {
                continue;
            }
            let Some(at) = self.current_location(id) else { continue };
            let speed = 0.2 * npc.speed_modifier();
            let targeted = self.targets.get(&id).copied();
            if let Some(target) = npc.follow_target().or(targeted.map(|t| t.0)) {
                let same = self.locate(target).filter(|(d, _)| self.dimension_of(id).as_ref() == Some(d));
                let Some((_, pos)) = same else { continue };
                let d = [pos[0] - at.x, pos[1] - at.y, pos[2] - at.z];
                let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                // Citizens' default distance margin is 2 blocks.
                let margin = if npc.follow_margin() > 0.0 { npc.follow_margin() } else { 2.0 };
                if len <= margin {
                    clear.push(id);
                    if targeted.is_some_and(|t| t.1) && self.tick % 20 == 0 {
                        hits.push((id, target));
                    }
                } else {
                    let f = (len - margin) / len;
                    set.push((id, [at.x + d[0] * f, at.y + d[1] * f, at.z + d[2] * f], speed, true));
                }
            } else if let Some((xr, yr, delay)) = npc.wander()
                && !self.walks.contains_key(&id)
                && self.tick >= self.wander_at.get(&id).copied().unwrap_or(0)
            {
                let (x, z) = (self.rng.between(-(xr as f64), xr as f64), self.rng.between(-(xr as f64), xr as f64));
                let y = self.rng.between(-(yr as f64), yr as f64).round();
                set.push((id, [at.x + x, at.y + y, at.z + z], speed, false));
                // Citizens' default delay is not in its plugin source; two to six seconds is a guess.
                let wait = if delay > 0 { delay as u64 } else { 40 + (self.rng.next_f64() * 80.0) as u64 };
                self.wander_at.insert(id, self.tick + wait);
            }
        }
        for (id, victim) in hits {
            if let Some(me) = self.saves.get(id).and_then(NpcData::uuid) {
                let (v, a) = (crate::horizon_citizens_core::ids::format_uuid(victim), crate::horizon_citizens_core::ids::format_uuid(me));
                self.ops.push(Op::Console(format!("damage {v} 1 minecraft:mob_attack by {a}")));
            }
        }
        for id in clear {
            if self.walks.get(&id).is_some_and(|w| w.follow) {
                self.walks.remove(&id);
            }
        }
        for (id, target, speed, follow) in set {
            // A route or a `pathto` in progress is not taken over by following.
            if follow && self.walks.get(&id).is_some_and(|w| !w.follow) {
                continue;
            }
            self.walks.insert(id, Walk { target, speed, waypoint: None, follow, margin: 0.0 });
        }
    }

    /// Moves walking NPCs a step, and starts linear waypoint routes.
    fn walk(&mut self) {
        // Waypoint NPCs without a walk get one to their next point.
        let mut starts = Vec::new();
        for npc in &self.saves.npcs {
            if !self.is_spawned(npc.id) || self.walks.contains_key(&npc.id) || !npc.linear_waypoints() {
                continue;
            }
            let points = npc.waypoints();
            if points.len() < 2 {
                continue;
            }
            let speed = 0.2
                * npc
                    .map
                    .at(&["navigator", "speedmodifier"])
                    .and_then(Node::as_f64)
                    .unwrap_or(1.0);
            let at = self.current_location(npc.id);
            let next = at.map_or(0, |a| nearest_point(&points, [a.x, a.y, a.z]));
            starts.push((npc.id, points[next].clone(), next, speed));
        }
        for (id, p, i, speed) in starts {
            self.walks.insert(
                id,
                Walk {
                    target: [p.x, p.y, p.z],
                    speed,
                    waypoint: Some(i),
                    follow: false,
                    margin: 0.0,
                },
            );
        }
        let mut done = Vec::new();
        let mut moves = Vec::new();
        let mut real_pos: Vec<(i32, [f64; 3])> = Vec::new();
        let mut paths: Vec<(i32, u32, [f64; 3], f64)> = Vec::new();
        for (id, walk) in &self.walks {
            let Some(live) = self.live.get(id) else {
                done.push((*id, None));
                continue;
            };
            let Some(at) = &live.at else {
                continue;
            };
            if let (Some(real), Some(entity)) = (self.real.get(id), live.entity) {
                // The game's own pathfinding drives the entity: ask for a path, read where it is.
                let pos = real.pos.unwrap_or([at.x, at.y, at.z]);
                let dx = walk.target[0] - pos[0];
                let dz = walk.target[2] - pos[2];
                let near = (dx * dx + dz * dz).sqrt() <= walk.margin.max(1.0) && (walk.target[1] - pos[1]).abs() <= 2.0;
                let need = match real.goal {
                    None => true,
                    Some((g, t)) => {
                        let moved = ((g[0] - walk.target[0]).powi(2) + (g[1] - walk.target[1]).powi(2) + (g[2] - walk.target[2]).powi(2)).sqrt();
                        moved > 0.5 && (!walk.follow || self.tick.saturating_sub(t) >= 10)
                    }
                };
                real_pos.push((*id, pos));
                if near {
                    done.push((*id, walk.waypoint));
                } else if need {
                    paths.push((*id, entity, walk.target, (walk.speed / 0.2).clamp(0.05, 5.0)));
                } else if real.done && real.goal.is_some_and(|(_, t)| self.tick.saturating_sub(t) >= 3) {
                    // The path ended (or the game found none): the walk is over.
                    done.push((*id, walk.waypoint));
                }
                continue;
            }
            let d = [walk.target[0] - at.x, walk.target[1] - at.y, walk.target[2] - at.z];
            let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            let mut next = at.clone();
            if len <= walk.speed.max(walk.margin) {
                if len <= walk.speed {
                    next.x = walk.target[0];
                    next.y = walk.target[1];
                    next.z = walk.target[2];
                }
                done.push((*id, walk.waypoint));
            } else {
                next.x += d[0] / len * walk.speed;
                next.y += d[1] / len * walk.speed;
                next.z += d[2] / len * walk.speed;
            }
            if d[0].abs() + d[2].abs() > 1e-6 {
                let (yaw, _) = look::look_at([at.x, 0.0, at.z], [walk.target[0], 0.0, walk.target[2]]);
                next.yaw = yaw;
                next.body_yaw = yaw;
                next.pitch = 0.0;
            }
            moves.push((*id, live.dimension.clone(), next));
        }
        for (id, pos) in real_pos {
            if let Some(l) = self.live.get_mut(&id)
                && let Some(at) = l.at.as_mut()
            {
                at.x = pos[0];
                at.y = pos[1];
                at.z = pos[2];
            }
        }
        for (id, entity, target, speed) in paths {
            if let Some(r) = self.real.get_mut(&id) {
                r.goal = Some((target, self.tick));
            }
            self.ops.push(Op::Entity { entity, set: EntitySet::MoveTo { target, speed } });
        }
        for (id, dimension, loc) in moves {
            let Some(uuid) = self.saves.get(id).and_then(NpcData::uuid) else {
                continue;
            };
            let mut step = false;
            if let Some(l) = self.live.get_mut(&id) {
                if let Some(old) = &l.at {
                    // Entity.move: moveDist grows by the horizontal distance × 0.6; past nextStep
                    // a step sounds and nextStep becomes (int) moveDist + 1.
                    l.walked += ((loc.x - old.x).powi(2) + (loc.z - old.z).powi(2)).sqrt() * 0.6;
                    if l.walked > l.next_step {
                        l.next_step = l.walked.floor() + 1.0;
                        step = true;
                    }
                }
                l.at = Some(loc.clone());
                l.look = Some((loc.yaw, loc.pitch));
            }
            self.ops.push(Op::Console(spawn::teleport(uuid, &dimension, &loc)));
            if step {
                self.ops.extend(spawn::step_sound(&dimension, &loc).into_iter().map(Op::Console));
            }
        }
        for (id, waypoint) in done {
            let following = self.walks.get(&id).is_some_and(|w| w.follow);
            self.walks.remove(&id);
            self.stop_path(id);
            if following {
                continue;
            }
            if self.saves.get(id).is_some_and(|n| n.waypoints().len() >= 2) && waypoint.is_some() {
                // The next point of the route.
                let points = self.saves.get(id).map(NpcData::waypoints).unwrap_or_default();
                let i = (waypoint.unwrap_or(0) + 1) % points.len();
                let speed = 0.2
                    * self
                        .saves
                        .get(id)
                        .and_then(|n| n.map.at(&["navigator", "speedmodifier"]))
                        .and_then(Node::as_f64)
                        .unwrap_or(1.0);
                self.walks.insert(
                    id,
                    Walk {
                        target: [points[i].x, points[i].y, points[i].z],
                        speed,
                        waypoint: Some(i),
                        follow: false,
                        margin: 0.0,
                    },
                );
            } else {
                self.events.push(NpcEvent::NavigationComplete { npc: id });
            }
            self.refresh_holograms(id);
        }
    }

    // ---- creating and removing -----------------------------------------------------------

    /// Creates an NPC at `loc` (spawned), with the defaults of `config` and Citizens.
    pub fn create(&mut self, name: &str, kind: &str, loc: Location, owner: Option<u128>) -> i32 {
        self.create_with(name, kind, loc, owner, true, |_| {})
    }

    /// [`World::create`], with a say in the NPC before it first appears and whether it appears
    /// at all (`/npc create -u`).
    pub fn create_with(
        &mut self,
        name: &str,
        kind: &str,
        loc: Location,
        owner: Option<u128>,
        spawned: bool,
        edit: impl FnOnce(&mut NpcData),
    ) -> i32 {
        let id = self.saves.next_id();
        let mut npc = NpcData::new(id, crate::horizon_citizens_core::ids::random_uuid(), name, &kind.to_ascii_uppercase());
        npc.set_location(&loc);
        npc.set_spawned(spawned);
        if let Some(o) = owner {
            npc.set_owner(o);
        }
        if self.cfg.look_close.enabled {
            npc.set_look_close(true);
        }
        edit(&mut npc);
        self.saves.insert(npc);
        self.record(Hist::Created(id));
        self.mark_dirty();
        self.events.push(NpcEvent::Created { npc: id });
        if spawned {
            self.spawn(id);
        }
        id
    }

    fn record(&mut self, step: Hist) {
        self.history.push(step);
        if self.history.len() > HISTORY_LIMIT {
            self.history.remove(0);
        }
    }

    /// Takes back the last create or remove (`/npc undo`); what it did, for the reply.
    pub fn undo_last(&mut self) -> Option<String> {
        loop {
            match self.history.pop()? {
                Hist::Created(id) => {
                    let Some(name) = self.saves.get(id).map(|n| n.name()) else {
                        continue;
                    };
                    self.remove_unrecorded(id);
                    return Some(format!("create {name}"));
                }
                Hist::Removed(npc) => {
                    let id = npc.id;
                    if self.saves.get(id).is_some() {
                        continue;
                    }
                    let name = npc.name();
                    let spawned = npc.spawned();
                    self.saves.insert(npc);
                    self.mark_dirty();
                    self.events.push(NpcEvent::Created { npc: id });
                    if spawned {
                        self.spawn(id);
                    }
                    return Some(format!("remove {name}"));
                }
            }
        }
    }

    pub fn remove(&mut self, id: i32) -> bool {
        if let Some(npc) = self.saves.get(id).cloned() {
            self.record(Hist::Removed(npc));
        }
        self.remove_unrecorded(id)
    }

    fn remove_unrecorded(&mut self, id: i32) -> bool {
        if self.saves.get(id).is_none() {
            return false;
        }
        self.despawn(id, DespawnReason::Removal);
        // `ShopTrait.onRemove`: the NPC's own shop goes with it.
        if let Some(uuid) = self.saves.get(id).and_then(NpcData::uuid)
            && self.saves.get(id).is_some_and(|n| n.has_trait("shop"))
            && self.shops.npc.remove(&crate::horizon_citizens_core::ids::format_uuid(uuid)).is_some()
        {
            self.shops_changed();
        }
        self.saves.remove(id);
        self.selected.retain(|_, n| *n != id);
        self.dead.remove(&id);
        self.mark_dirty();
        self.events.push(NpcEvent::Removed { npc: id });
        true
    }

    /// Copies NPC `id` to `loc` under a new id and UUID.
    pub fn copy(&mut self, id: i32, loc: Location) -> Option<i32> {
        let mut npc = self.saves.get(id)?.clone();
        let new_id = self.saves.next_id();
        npc.id = new_id;
        npc.map
            .set("uuid", Node::str(&crate::horizon_citizens_core::ids::format_uuid(crate::horizon_citizens_core::ids::random_uuid())));
        npc.set_location(&loc);
        self.saves.insert(npc);
        self.mark_dirty();
        self.events.push(NpcEvent::Created { npc: new_id });
        if self.saves.get(new_id).is_some_and(NpcData::spawned) {
            self.spawn(new_id);
        }
        Some(new_id)
    }
}

fn nearest_point(points: &[Location], at: [f64; 3]) -> usize {
    points
        .iter()
        .enumerate()
        .min_by(|a, b| {
            look::dist2([a.1.x, a.1.y, a.1.z], at).total_cmp(&look::dist2([b.1.x, b.1.y, b.1.z], at))
        })
        .map_or(0, |(i, _)| i)
}

fn same_item(a: &str, b: &str) -> bool {
    a.trim_start_matches("minecraft:") == b.trim_start_matches("minecraft:")
}

/// A name Mojang could have: 3 to 16 of `[A-Za-z0-9_]`.
pub fn valid_player_name(s: &str) -> bool {
    (3..=16).contains(&s.len()) && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loc(x: f64, z: f64) -> Location {
        Location {
            world: "world".into(),
            x,
            y: 64.0,
            z,
            yaw: 0.0,
            pitch: 0.0,
            body_yaw: 0.0,
        }
    }

    fn player(x: f64, z: f64) -> PlayerView {
        PlayerView {
            uuid: 0xA1,
            name: "Alex".into(),
            dimension: "minecraft:overworld".into(),
            pos: [x, 64.0, z],
            game_mode: 0,
            permission: 0,
            entity_id: 900,
            sneaking: false,
        }
    }

    fn world() -> World {
        let mut cfg = Config::default();
        cfg.skins.fetch = false;
        World::new(cfg, Saves::empty(), None)
    }

    fn consoles(w: &mut World) -> Vec<String> {
        w.take_ops()
            .into_iter()
            .filter_map(|o| match o {
                Op::Console(c) | Op::Rotate { fallback: c, .. } => Some(c),
                _ => None,
            })
            .collect()
    }

    /// Creates an NPC and lets its entity join as id `entity`.
    fn spawned(w: &mut World, name: &str, entity: u32) -> i32 {
        let id = w.create(name, "PLAYER", loc(0.0, 0.0), None);
        let uuid = w.saves.get(id).unwrap().uuid();
        assert!(!w.on_entity_join("minecraft:overworld", entity, uuid, true));
        id
    }

    #[test]
    fn create_summons_and_the_join_maps_the_entity() {
        let mut w = world();
        let id = w.create("Guide", "PLAYER", loc(1.0, 2.0), Some(7));
        assert_eq!(id, 0);
        let cmds = consoles(&mut w);
        assert!(cmds.iter().any(|c| c.contains("summon minecraft:mannequin 1 64 2")));
        let uuid = w.saves.get(id).unwrap().uuid();
        assert!(!w.is_spawned(id));
        assert!(!w.on_entity_join("minecraft:overworld", 55, uuid, true));
        assert_eq!(w.npc_of_entity(55), Some(id));
        assert_eq!(w.entity_of(id), Some(55));
        assert!(w.take_events().contains(&NpcEvent::Spawned {
            npc: id,
            entity: 55,
            dimension: "minecraft:overworld".into()
        }));
        // A second copy (from a chunk) is refused; a tagged stranger too; others are let be.
        assert!(w.on_entity_join("minecraft:overworld", 56, uuid, true));
        assert!(w.on_entity_join("minecraft:overworld", 57, Some(12345), true));
        assert!(!w.on_entity_join("minecraft:overworld", 58, Some(12345), false));
    }

    #[test]
    fn clicks_fire_events_and_commands() {
        let mut w = world();
        let id = spawned(&mut w, "Guide", 10);
        w.saves.get_mut(id).unwrap().add_command("say hi <p>", Hand::Right, false, false);
        w.saves.get_mut(id).unwrap().add_command("spawn", Hand::Both, true, false);
        w.take_ops();
        w.take_events();
        let p = player(1.0, 1.0);
        assert!(w.on_click(10, &p, Hand::Right, None));
        let ev = w.take_events();
        assert!(matches!(ev[0], NpcEvent::RightClick { npc, player: 0xA1, .. } if npc == id));
        let cmds = consoles(&mut w);
        assert_eq!(cmds[0], "say hi Alex");
        assert!(cmds[1].starts_with("execute as 00000000-0000-0000-0000-0000000000a1 at "));
        assert!(cmds[1].ends_with(" run spawn"));
        // Within the cooldown the event comes, the commands do not.
        assert!(w.on_click(10, &p, Hand::Right, None));
        assert_eq!(w.take_events().len(), 1);
        assert!(consoles(&mut w).is_empty());
        // A left click runs only BOTH/LEFT commands, after the cooldown.
        for _ in 0..25 {
            w.tick(&[]);
        }
        w.take_ops();
        assert!(w.on_click(10, &p, Hand::Left, None));
        assert!(matches!(w.take_events()[0], NpcEvent::LeftClick { .. }));
        assert_eq!(consoles(&mut w).len(), 1);
        assert!(!w.on_click(99, &p, Hand::Right, None), "not an NPC");
    }

    /// One command with `experienceCost` and/or `itemCost` (diamond x3), the way Citizens saves them.
    fn costed(w: &mut World, id: i32, xp: Option<i64>, items: bool, n: Option<i64>) {
        use crate::horizon_citizens_core::yaml::Node;
        let npc = w.saves.get_mut(id).unwrap();
        npc.add_command("say paid", Hand::Right, false, false);
        let mut cmds = npc.map.map_at(&["traits", "commandtrait", "commands"]).clone();
        if let Some(e) = cmds.get_mut("0").filter(|e| e.is_map()) {
            if let Some(x) = xp {
                e.set("experienceCost", Node::int(x));
            }
            if let Some(n) = n {
                e.set("n", Node::int(n));
            }
            if items {
                let mut stack = Node::map();
                stack.set("==", Node::str("org.bukkit.inventory.ItemStack"));
                stack.set("type", Node::str("DIAMOND"));
                stack.set("amount", Node::int(3));
                e.set("itemCost", Node::seq(vec![stack]));
            }
        }
        npc.map.map_at(&["traits", "commandtrait"]).set("commands", cmds);
    }

    fn diamonds(w: &mut World, levels: i32, slots: &[(u32, u32)]) {
        w.player_cost = Some(PlayerCost {
            levels,
            slots: slots.iter().map(|(s, n)| (*s, "minecraft:diamond".to_owned(), *n)).collect(),
            money: 0.0,
        });
    }

    fn tells(ops: &[Op]) -> Vec<String> {
        ops.iter().filter_map(|o| if let Op::Tell { legacy, .. } = o { Some(legacy.clone()) } else { None }).collect()
    }

    #[test]
    fn experience_cost_is_checked_then_charged_in_levels() {
        let mut w = world();
        let id = spawned(&mut w, "Guide", 10);
        costed(&mut w, id, Some(5), false, None);
        w.take_ops();
        let p = player(1.0, 1.0);
        diamonds(&mut w, 4, &[]);
        assert!(w.on_click(10, &p, Hand::Right, None));
        let ops = w.take_ops();
        assert_eq!(tells(&ops), vec!["You need at least 5 experience.".to_owned()]);
        assert!(!ops.iter().any(|o| matches!(o, Op::Console(_) | Op::Charge { .. })));
        diamonds(&mut w, 7, &[]);
        assert!(w.on_click(10, &p, Hand::Right, None));
        let ops = w.take_ops();
        assert!(ops.contains(&Op::Charge { player: 0xA1, levels: 5, slots: vec![], money: 0.0 }));
        assert!(ops.contains(&Op::Console("say paid".into())));
    }

    /// A command with a money `cost`, and the trait's own `cost` when `global`.
    fn priced(w: &mut World, id: i32, cost: f64, global: bool) {
        use crate::horizon_citizens_core::yaml::Node;
        let npc = w.saves.get_mut(id).unwrap();
        npc.add_command("say paid", Hand::Right, false, false);
        if global {
            npc.set_trait_cost(cost);
            return;
        }
        let mut cmds = npc.map.map_at(&["traits", "commandtrait", "commands"]).clone();
        if let Some(e) = cmds.get_mut("0").filter(|e| e.is_map()) {
            e.set("cost", Node::float(cost));
        }
        npc.map.map_at(&["traits", "commandtrait"]).set("commands", cmds);
    }

    fn rich(w: &mut World, money: f64) {
        w.player_cost = Some(PlayerCost { levels: 0, slots: vec![], money });
    }

    #[test]
    fn money_cost_is_checked_against_the_balance_then_charged() {
        let mut w = world();
        let id = spawned(&mut w, "Guide", 10);
        priced(&mut w, id, 5.0, false);
        w.take_ops();
        let p = player(1.0, 1.0);
        rich(&mut w, 3.0);
        assert!(w.on_click(10, &p, Hand::Right, None));
        let ops = w.take_ops();
        assert_eq!(tells(&ops), vec!["You need at least $5.".to_owned()]);
        assert!(!ops.iter().any(|o| matches!(o, Op::Console(_) | Op::Charge { .. })));
        rich(&mut w, 8.0);
        assert!(w.on_click(10, &p, Hand::Right, None));
        let ops = w.take_ops();
        assert!(ops.contains(&Op::Charge { player: 0xA1, levels: 0, slots: vec![], money: 5.0 }));
        assert!(ops.contains(&Op::Console("say paid".into())));
    }

    #[test]
    fn the_traits_own_cost_is_charged_once_per_click() {
        let mut w = world();
        let id = spawned(&mut w, "Guide", 10);
        priced(&mut w, id, 2.5, true);
        w.take_ops();
        let p = player(1.0, 1.0);
        rich(&mut w, 1.0);
        assert!(w.on_click(10, &p, Hand::Right, None));
        let ops = w.take_ops();
        assert_eq!(tells(&ops), vec!["You need at least $2.5.".to_owned()]);
        assert!(!ops.iter().any(|o| matches!(o, Op::Console(_))), "a click that cannot pay runs nothing");
        rich(&mut w, 10.0);
        assert!(w.on_click(10, &p, Hand::Right, None));
        let ops = w.take_ops();
        assert!(ops.contains(&Op::Charge { player: 0xA1, levels: 0, slots: vec![], money: 2.5 }));
        assert!(ops.contains(&Op::Console("say paid".into())));
    }

    #[test]
    fn the_ignore_node_skips_a_money_cost() {
        let mut w = world();
        let id = spawned(&mut w, "Guide", 10);
        priced(&mut w, id, 5.0, false);
        w.take_ops();
        rich(&mut w, 0.0);
        w.held_perms = ["citizens.npc.command.ignoreerrors.cost".to_owned()].into_iter().collect();
        assert!(w.on_click(10, &player(1.0, 1.0), Hand::Right, None));
        let ops = w.take_ops();
        assert!(ops.contains(&Op::Console("say paid".into())));
        assert!(!ops.iter().any(|o| matches!(o, Op::Charge { .. })));
    }

    #[test]
    fn item_cost_takes_stacks_in_slot_order_and_names_the_missing_item() {
        let mut w = world();
        let id = spawned(&mut w, "Guide", 10);
        costed(&mut w, id, None, true, None);
        w.take_ops();
        let p = player(1.0, 1.0);
        diamonds(&mut w, 0, &[(2, 2), (5, 4)]);
        assert!(w.on_click(10, &p, Hand::Right, None));
        let ops = w.take_ops();
        assert!(ops.contains(&Op::Charge { player: 0xA1, levels: 0, slots: vec![(2, 0), (5, 3)], money: 0.0 }));
        assert!(ops.contains(&Op::Console("say paid".into())));
        diamonds(&mut w, 0, &[(2, 2)]);
        assert!(w.on_click(10, &p, Hand::Right, None));
        let ops = w.take_ops();
        assert_eq!(tells(&ops), vec!["Missing 3 diamond".to_owned()]);
        assert!(!ops.iter().any(|o| matches!(o, Op::Console(_) | Op::Charge { .. })));
    }

    #[test]
    fn cost_is_skipped_for_ignoreerrors_and_not_taken_when_a_limit_refuses() {
        let mut w = world();
        let id = spawned(&mut w, "Guide", 10);
        costed(&mut w, id, Some(5), true, Some(1));
        w.take_ops();
        let p = player(1.0, 1.0);
        diamonds(&mut w, 0, &[]);
        w.held_perms.insert("citizens.npc.command.ignoreerrors.cost".into());
        assert!(w.on_click(10, &p, Hand::Right, None));
        assert!(tells(&w.take_ops()).iter().any(|t| t.contains("need at least 5")), "only the money node is ignored");
        w.held_perms.insert("citizens.npc.command.ignoreerrors.expcost".into());
        w.held_perms.insert("citizens.npc.command.ignoreerrors.itemcost".into());
        assert!(w.on_click(10, &p, Hand::Right, None));
        let ops = w.take_ops();
        assert!(ops.contains(&Op::Console("say paid".into())));
        assert!(!ops.iter().any(|o| matches!(o, Op::Charge { .. })));
        // n = 1 is used up: the limit refuses and the player pays nothing.
        w.held_perms.clear();
        for _ in 0..25 {
            w.tick(&[]);
        }
        diamonds(&mut w, 9, &[(0, 3)]);
        assert!(w.on_click(10, &p, Hand::Right, None));
        let ops = w.take_ops();
        assert!(!ops.iter().any(|o| matches!(o, Op::Charge { .. })));
        assert!(tells(&ops).iter().any(|t| t == "You have reached the maximum number of uses (1)."));
    }

    #[test]
    fn npc_flag_runs_a_player_command_as_the_npc() {
        use crate::horizon_citizens_core::yaml::Node;
        let mut w = world();
        let id = spawned(&mut w, "Guide", 10);
        {
            let n = w.saves.get_mut(id).unwrap();
            n.add_command("spawn", Hand::Right, true, false);
            let mut cmds = n.map.map_at(&["traits", "commandtrait", "commands"]).clone();
            if let Some(e) = cmds.get_mut("0").filter(|e| e.is_map()) {
                e.set("npc", Node::bool(true));
            }
            n.map.map_at(&["traits", "commandtrait"]).set("commands", cmds);
        }
        w.take_ops();
        assert!(w.on_click(10, &player(1.0, 1.0), Hand::Right, None));
        let ops = w.take_ops();
        assert!(ops.contains(&Op::EntityCommand { entity: 10, line: "spawn".into(), level: 0 }), "{ops:?}");
        assert!(!ops.iter().any(|o| matches!(o, Op::Console(c) if c.contains("execute as"))), "{ops:?}");
    }

    fn menu_ops(w: &mut World) -> Vec<Op> {
        w.take_ops().into_iter().filter(|o| matches!(o, Op::OpenMenu { .. } | Op::SetMenuItems { .. })).collect()
    }

    #[test]
    fn inventory_menu_takes_deposits_and_saves_on_close() {
        let mut w = world();
        let id = spawned(&mut w, "Guide", 10);
        w.saves.get_mut(id).unwrap().set_inventory(vec![(2, StackData::new("minecraft:apple", 5))]);
        w.take_ops();
        w.open_inventory(0xA1, id);
        let ops = menu_ops(&mut w);
        let Some(Op::OpenMenu { rows, items, title, .. }) = ops.first() else { panic!("{ops:?}") };
        assert_eq!((*rows, title.as_str()), (4, "Guide's Inventory"));
        assert_eq!(items[2].as_ref().map(|s| s.count), Some(5));
        w.menu_opened(0xA1, Some(7));
        // A click in another window is not ours.
        assert_eq!(w.menu_click(0xA1, 8, 2, 0, false), MenuAction::Nothing);
        // The first slot of the player's own part (36) is main-inventory slot 9; the last is hotbar 8.
        assert_eq!(w.menu_click(0xA1, 7, 36, 0, false), MenuAction::Deposit { inv_slot: 9 });
        assert_eq!(w.menu_click(0xA1, 7, 71, 0, false), MenuAction::Deposit { inv_slot: 8 });
        assert!(w.menu_deposit(0xA1, StackData::new("minecraft:stone", 64)));
        assert!(matches!(menu_ops(&mut w).last(), Some(Op::SetMenuItems { window: 7, .. })));
        let MenuAction::Take(apples) = w.menu_click(0xA1, 7, 2, 0, false) else { panic!("take") };
        assert_eq!((apples.id.as_str(), apples.count), ("minecraft:apple", 5));
        w.menu_closed(0xA1, 7);
        let saved = w.saves.get(id).unwrap().inventory_stacks();
        assert_eq!(saved.len(), 1, "the apples left, the stone came in");
        assert_eq!((saved[0].0, saved[0].1.id.as_str()), (0, "minecraft:stone"));
    }

    #[test]
    fn drops_menu_shows_chances_and_saves_them() {
        let mut w = world();
        let id = spawned(&mut w, "Guide", 10);
        w.saves.get_mut(id).unwrap().set_drops(vec![(StackData::new("minecraft:bone", 2), 0.5)]);
        w.take_ops();
        w.open_drops(0xA1, id);
        let ops = menu_ops(&mut w);
        let Some(Op::OpenMenu { rows, title, items, .. }) = ops.first() else { panic!("{ops:?}") };
        assert_eq!((*rows, title.as_str()), (5, "Add items for drops"));
        assert_eq!(items[0].as_ref().map(|s| s.id.as_str()), Some("minecraft:bone"));
        let barrier = |i: usize| items[i].as_ref().and_then(|s| s.meta.display_name.clone());
        assert_eq!(barrier(9).as_deref(), Some("Drop chance 50%"));
        assert_eq!(barrier(10).as_deref(), Some("Drop chance 100%"));
        w.menu_opened(0xA1, Some(3));
        // Left click on the barrier: 60 %. Row 4 takes a new drop at 100 %.
        assert_eq!(w.menu_click(0xA1, 3, 9, 0, false), MenuAction::Nothing);
        let Some(Op::SetMenuItems { items, .. }) = menu_ops(&mut w).pop() else { panic!("refresh") };
        assert_eq!(items[9].as_ref().and_then(|s| s.meta.display_name.clone()).as_deref(), Some("Drop chance 60%"));
        // Shift moves one point, right click down ten; never past 0 or 100.
        w.menu_click(0xA1, 3, 9, 1, true);
        w.menu_click(0xA1, 3, 9, 1, false);
        w.menu_click(0xA1, 3, 9, 1, false);
        w.menu_click(0xA1, 3, 9, 1, false);
        w.menu_click(0xA1, 3, 9, 1, false);
        w.menu_click(0xA1, 3, 9, 1, false);
        w.menu_click(0xA1, 3, 9, 1, false);
        w.menu_click(0xA1, 3, 9, 0, true);
        menu_ops(&mut w);
        assert!(w.menu_deposit(0xA1, StackData::new("minecraft:apple", 1)));
        w.menu_closed(0xA1, 3);
        let d = w.saves.get(id).unwrap().drops();
        assert_eq!(d.len(), 2);
        assert_eq!((d[0].0.id.as_str(), d[0].1), ("minecraft:bone", 0.01));
        assert_eq!((d[1].0.id.as_str(), d[1].1), ("minecraft:apple", 1.0));
    }

    #[test]
    fn plan_item_charge_matches_citizens_take_items() {
        let cost = |id: &str, n: i64| CostItem { id: id.into(), amount: n, name: id.into() };
        let slots = vec![(0, "minecraft:stone".to_owned(), 64), (1, "minecraft:diamond".to_owned(), 1), (3, "minecraft:diamond".to_owned(), 5)];
        assert_eq!(plan_item_charge(&slots, &[cost("minecraft:diamond", 4)]), Some(vec![(1, 0), (3, 2)]));
        assert_eq!(plan_item_charge(&slots, &[cost("minecraft:diamond", 7)]), None);
        assert_eq!(plan_item_charge(&slots, &[cost("minecraft:diamond", 2), cost("minecraft:stone", 1)]), Some(vec![(0, 63), (1, 0), (3, 4)]));
    }

    #[test]
    fn commandtrait_limits_modes_and_delay() {
        use crate::horizon_citizens_core::yaml::Node;
        let mut w = world();
        let id = spawned(&mut w, "Guide", 10);
        {
            let n = w.saves.get_mut(id).unwrap();
            n.add_command("say a", Hand::Right, false, false);
            n.add_command("say b", Hand::Right, false, false);
            let mut cmds = n.map.map_at(&["traits", "commandtrait", "commands"]).clone();
            if let Some(e) = cmds.get_mut("0").filter(|e| e.is_map()) {
                e.set("n", Node::int(1));
                e.set("permissions", Node::seq(vec![Node::str("vip.use")]));
            }
            if let Some(e) = cmds.get_mut("1").filter(|e| e.is_map()) {
                e.set("delay", Node::int(40));
            }
            n.map.map_at(&["traits", "commandtrait"]).set("commands", cmds);
        }
        w.take_ops();
        let p = player(1.0, 1.0);
        // No permission: "a" is refused with the configured text, "b" is delayed 40 ticks.
        assert!(w.on_click(10, &p, Hand::Right, None));
        let ops = w.take_ops();
        assert!(ops.iter().any(|o| matches!(o, Op::Tell { legacy, .. } if legacy == "You don't have permission to do that.")));
        assert!(!ops.iter().any(|o| matches!(o, Op::Console(c) if c == "say b")));
        for _ in 0..41 {
            w.tick(&[]);
        }
        assert!(consoles(&mut w).contains(&"say b".to_owned()));
        // With the permission "a" runs once (n = 1), then the use limit answers.
        w.held_perms.insert("vip.use".into());
        assert!(w.on_click(10, &p, Hand::Right, None));
        assert!(consoles(&mut w).contains(&"say a".to_owned()));
        for _ in 0..25 {
            w.tick(&[]);
        }
        w.take_ops();
        assert!(w.on_click(10, &p, Hand::Right, None));
        let ops = w.take_ops();
        assert!(!ops.iter().any(|o| matches!(o, Op::Console(c) if c == "say a")));
        assert!(ops.iter().any(|o| matches!(o, Op::Tell { legacy, .. } if legacy == "You have reached the maximum number of uses (1).")));
    }

    #[test]
    fn sequential_runs_one_command_per_click() {
        use crate::horizon_citizens_core::yaml::Node;
        let mut w = world();
        let id = spawned(&mut w, "Guide", 10);
        {
            let n = w.saves.get_mut(id).unwrap();
            n.add_command("say a", Hand::Right, false, false);
            n.add_command("say b", Hand::Right, false, false);
            n.map.map_at(&["traits", "commandtrait"]).set("executionmode", Node::str("SEQUENTIAL"));
        }
        w.take_ops();
        let p = player(1.0, 1.0);
        let mut seen = Vec::new();
        for _ in 0..3 {
            assert!(w.on_click(10, &p, Hand::Right, None));
            seen.extend(consoles(&mut w));
            for _ in 0..25 {
                w.tick(&[]);
            }
        }
        assert_eq!(seen, ["say a", "say b", "say a"]);
    }

    #[test]
    fn the_selection_item_selects_for_operators() {
        let mut w = world();
        let id = spawned(&mut w, "Guide", 10);
        w.take_events();
        let mut p = player(0.0, 0.0);
        assert!(w.on_click(10, &p, Hand::Right, Some("minecraft:stick")));
        assert_eq!(w.selected(p.uuid), None, "not an operator: a normal click");
        p.permission = 2;
        w.take_events();
        assert!(w.on_click(10, &p, Hand::Right, Some("minecraft:stick")));
        assert_eq!(w.selected(p.uuid), Some(id));
        assert!(w.take_ops().iter().any(|o| matches!(o, Op::Tell { legacy, .. } if legacy.contains("Guide"))));
    }

    #[test]
    fn protection_and_damage() {
        let mut w = world();
        let id = spawned(&mut w, "Guide", 10);
        assert!(w.hit_refused(10));
        assert_eq!(w.protected_entities(), vec![10]);
        w.saves.get_mut(id).unwrap().set_protected(false);
        assert!(!w.hit_refused(10));
        assert!(w.protected_entities().is_empty());
        w.take_events();
        w.on_damaged(10, 4.0, "minecraft:player_attack", 900);
        w.on_entity_leave(10);
        let ev = w.take_events();
        assert!(ev.contains(&NpcEvent::Died { npc: id }));
        w.take_ops();
        // Dead NPCs stay out until respawned through the API.
        for _ in 0..40 {
            w.tick(&[player(0.0, 0.0)]);
        }
        assert!(!consoles(&mut w).iter().any(|c| c.contains("summon minecraft:mannequin")));
        assert!(w.spawn(id));
    }

    #[test]
    fn a_chunk_unload_respawns_when_a_player_comes_near() {
        let mut w = world();
        let id = spawned(&mut w, "Guide", 10);
        w.on_entity_leave(10);
        assert!(!w.is_spawned(id));
        w.take_ops();
        for _ in 0..25 {
            w.tick(&[player(500.0, 500.0)]);
        }
        assert!(consoles(&mut w).is_empty(), "nobody near");
        for _ in 0..25 {
            w.tick(&[player(5.0, 5.0)]);
        }
        let cmds = consoles(&mut w);
        assert_eq!(cmds.iter().filter(|c| c.contains("summon minecraft:mannequin")).count(), 1);
    }

    #[test]
    fn look_close_turns_to_the_nearest_player() {
        let mut w = world();
        let id = spawned(&mut w, "Guide", 10);
        w.saves.get_mut(id).unwrap().set_look_close(true);
        w.take_ops();
        w.tick(&[player(-5.0, 0.0)]);
        w.tick(&[player(-5.0, 0.0)]);
        let cmds = consoles(&mut w);
        let rot: Vec<_> = cmds.iter().filter(|c| c.contains(" rotate ")).collect();
        assert_eq!(rot.len(), 1);
        assert!(rot[0].contains(" 90 0"), "{}", rot[0]);
        // No change, nothing sent.
        w.tick(&[player(-5.0, 0.0)]);
        w.tick(&[player(-5.0, 0.0)]);
        assert!(!consoles(&mut w).iter().any(|c| c.contains(" rotate ")));
        // Out of range: stays.
        w.tick(&[player(-50.0, 0.0)]);
        w.tick(&[player(-50.0, 0.0)]);
        assert!(!consoles(&mut w).iter().any(|c| c.contains(" rotate ")));
    }

    fn mob_entity(w: &mut World, name: &str, entity: u32) -> i32 {
        let id = w.create(name, "ZOMBIE", loc(0.0, 0.0), None);
        let uuid = w.saves.get(id).unwrap().uuid();
        assert!(!w.on_entity_join("minecraft:overworld", entity, uuid, true));
        id
    }

    fn entity_ops(w: &mut World) -> Vec<EntitySet> {
        w.take_ops().into_iter().filter_map(|o| if let Op::Entity { set, .. } = o { Some(set) } else { None }).collect()
    }

    #[test]
    fn a_mob_npc_asks_to_follow_paths_and_a_mannequin_does_not() {
        let mut w = world();
        mob_entity(&mut w, "Zed", 10);
        assert!(entity_ops(&mut w).contains(&EntitySet::NavigationTicks(true)));
        spawned(&mut w, "Guide", 11);
        assert!(!entity_ops(&mut w).contains(&EntitySet::NavigationTicks(true)));
    }

    #[test]
    fn real_pathing_asks_for_a_path_and_reads_the_position_back() {
        let mut w = world();
        let id = mob_entity(&mut w, "Zed", 10);
        w.take_ops();
        // Until the game takes the switch, the walk is the straight line.
        assert!(!w.has_real_pathing(id));
        w.nav_ready(10);
        assert!(w.has_real_pathing(id));
        assert_eq!(w.real_entities(), vec![(id, 10)]);
        assert!(w.navigate_to(id, [10.0, 64.0, 0.0], 0.2));
        w.take_events();
        w.tick(&[]);
        let ops = w.take_ops();
        assert!(!ops.iter().any(|o| matches!(o, Op::Console(c) if c.contains(" tp "))), "the game moves it: {ops:?}");
        let path: Vec<_> = ops
            .iter()
            .filter_map(|o| if let Op::Entity { set: EntitySet::MoveTo { target, speed }, .. } = o { Some((*target, *speed)) } else { None })
            .collect();
        assert_eq!(path, vec![([10.0, 64.0, 0.0], 1.0)]);
        // On its way: the path is kept, nothing is asked again, the NPC's position follows the entity.
        w.nav_report(id, [4.0, 64.0, 0.0], false);
        w.tick(&[]);
        assert!(entity_ops(&mut w).is_empty());
        assert_eq!(w.current_location(id).map(|l| l.x), Some(4.0));
        assert!(w.is_navigating(id));
        // Arrived: the walk ends, the path stops and the API hears it.
        w.nav_report(id, [9.5, 64.0, 0.0], true);
        w.tick(&[]);
        assert!(entity_ops(&mut w).contains(&EntitySet::StopNavigation));
        assert!(w.take_events().contains(&NpcEvent::NavigationComplete { npc: id }));
        assert!(!w.is_navigating(id));
    }

    #[test]
    fn a_path_that_ends_short_ends_the_walk() {
        let mut w = world();
        let id = mob_entity(&mut w, "Zed", 10);
        w.nav_ready(10);
        assert!(w.navigate_to(id, [30.0, 64.0, 0.0], 0.2));
        w.tick(&[]);
        w.take_ops();
        w.take_events();
        // The game found no way (its path is done far from the goal): after a few ticks the walk is over.
        for _ in 0..4 {
            w.nav_report(id, [2.0, 64.0, 0.0], true);
            w.tick(&[]);
        }
        assert!(!w.is_navigating(id));
        assert!(w.take_events().contains(&NpcEvent::NavigationComplete { npc: id }));
    }

    #[test]
    fn real_pathing_ends_with_the_entity() {
        let mut w = world();
        let id = mob_entity(&mut w, "Zed", 10);
        w.nav_ready(10);
        w.on_entity_leave(10);
        assert!(!w.has_real_pathing(id));
    }

    #[test]
    fn navigation_steps_and_completes() {
        let mut w = world();
        let id = spawned(&mut w, "Walker", 10);
        assert!(w.navigate_to(id, [1.0, 64.0, 0.0], 0.5));
        w.take_ops();
        w.take_events();
        w.tick(&[]);
        let cmds = consoles(&mut w);
        assert!(cmds.iter().any(|c| c.contains(" tp ") && c.contains(" 0.5 64 0 ")), "{cmds:?}");
        w.tick(&[]);
        assert!(w.take_events().contains(&NpcEvent::NavigationComplete { npc: id }));
        assert!(!w.is_navigating(id));
    }

    /// Vanilla: a step every (int) moveDist + 1 with moveDist = distance × 0.6, so 10 blocks give 6.
    #[test]
    fn a_walking_npc_makes_footsteps() {
        let mut w = world();
        let id = spawned(&mut w, "Walker", 10);
        assert!(w.navigate_to(id, [10.0, 64.0, 0.0], 0.2));
        w.take_ops();
        let mut steps = 0;
        for _ in 0..60 {
            w.tick(&[]);
            steps += consoles(&mut w).iter().filter(|c| c.contains("block.stone.step")).count();
        }
        assert_eq!(steps, 6);
    }

    #[test]
    fn stop_takes_every_npc_out() {
        let mut w = world();
        let id = spawned(&mut w, "Guide", 10);
        w.saves.get_mut(id).unwrap().add_hologram_line("hi");
        w.refresh_holograms(id);
        w.take_ops();
        w.stop();
        let ops = w.take_ops();
        assert!(ops.contains(&Op::RemoveMob {
            dimension: "minecraft:overworld".into(),
            entity: 10
        }));
        assert!(ops.iter().any(|o| matches!(o, Op::Console(c) if c.starts_with("kill "))));
        assert!(!w.is_spawned(id));
    }

    #[test]
    fn teleport_moves_and_saves() {
        let mut w = world();
        let id = spawned(&mut w, "Guide", 10);
        w.take_ops();
        assert!(w.teleport(id, loc(20.0, 30.0)));
        let cmds = consoles(&mut w);
        assert!(cmds.iter().any(|c| c.contains(" tp ") && c.contains(" 20 64 30 ")));
        assert_eq!(w.saves.get(id).unwrap().location().unwrap().x, 20.0);
        assert!(!w.teleport(id, Location { world: "nowhere".into(), ..loc(0.0, 0.0) }));
    }

    #[test]
    fn remove_and_copy() {
        let mut w = world();
        let id = spawned(&mut w, "Guide", 10);
        let copy = w.copy(id, loc(3.0, 3.0)).unwrap();
        assert_ne!(copy, id);
        assert_ne!(w.saves.get(copy).unwrap().uuid(), w.saves.get(id).unwrap().uuid());
        assert!(w.remove(id));
        assert!(w.saves.get(id).is_none());
        assert!(!w.remove(id));
        assert_eq!(w.npc_of_entity(10), None);
    }

    #[test]
    fn skin_lookups_are_asked_once() {
        let mut cfg = Config::default();
        cfg.skins.fetch = true;
        let mut w = World::new(cfg, Saves::empty(), None);
        let id = w.create("Steve", "PLAYER", loc(0.0, 0.0), None);
        let ops = w.take_ops();
        assert!(ops.contains(&Op::FetchSkin { npc: id, name: "Steve".into() }));
        w.respawn(id);
        assert!(!w.take_ops().iter().any(|o| matches!(o, Op::FetchSkin { .. })));
        w.skin_fetched(crate::horizon_citizens_core::skin::Fetched {
            npc: id,
            requested: "Steve".into(),
            result: Ok(crate::horizon_citizens_core::skin::FetchedSkin {
                profile: 5,
                name: "Steve".into(),
                texture: "VEVY".into(),
                signature: Some("U0lH".into()),
            }),
        });
        assert!(w.saves.get(id).unwrap().skin().has_texture());
        assert!(consoles(&mut w).iter().any(|c| c.contains("value:\"VEVY\"")));
    }

    #[test]
    fn waypoint_routes_loop() {
        let text = "npc:
  '1':
    traits:
      type: PLAYER
      location:
        world: world
        x: 0.0
        y: 64.0
        z: 0.0
      waypoints:
        provider: linear
        linear:
          points:
          - location:
              world: world
              x: 0.0
              y: 64.0
              z: 0.0
          - location:
              world: world
              x: 0.4
              y: 64.0
              z: 0.0
    name: W
    uuid: 00000000-0000-4000-8000-000000000001
";
        let mut cfg = Config::default();
        cfg.skins.fetch = false;
        let mut w = World::new(cfg, Saves::parse(text).unwrap(), None);
        assert!(!w.on_entity_join("minecraft:overworld", 3, Some(0x4000_8000_0000_0000_0001), false));
        for _ in 0..10 {
            w.tick(&[]);
        }
        let tps = consoles(&mut w).iter().filter(|c| c.contains(" tp ")).count();
        assert!(tps >= 8, "walks every tick: {tps}");
        assert!(w.is_navigating(1));
    }
    #[test]
    fn cosmetic_equipment_shows_in_place_of_the_real_item() {
        use crate::horizon_citizens_core::npc::Cosmetic;
        let mut w = world();
        let id = spawned(&mut w, "Guide", 10);
        w.open_equipment(0xA1, id);
        w.take_ops();
        w.menu_opened(0xA1, Some(2));
        // The hand's cosmetic cell says "No cosmetic"; a click enables it.
        assert_eq!(w.menu_click(0xA1, 2, 18, 0, false), MenuAction::Nothing);
        assert!(w.menu_deposit(0xA1, StackData::new("minecraft:stick", 1)));
        assert!(w.menu_deposit(0xA1, StackData::new("minecraft:blaze_rod", 1)), "the real hand is taken: the cosmetic one gets it");
        w.menu_closed(0xA1, 2);
        let n = w.saves.get(id).unwrap();
        assert!(matches!(n.cosmetic("hand"), Cosmetic::Item(s) if s.id == "minecraft:blaze_rod"));
        assert_eq!(n.cosmetic("boots"), Cosmetic::Off);
        let shown = n.equipment();
        assert_eq!(shown.len(), 1);
        assert_eq!((shown[0].slot, shown[0].item.as_str()), ("mainhand", "blaze_rod"));
    }

    #[test]
    fn an_enabled_empty_cosmetic_hides_the_real_item() {
        use crate::horizon_citizens_core::npc::Cosmetic;
        let mut w = world();
        let id = spawned(&mut w, "Guide", 10);
        let n = w.saves.get_mut(id).unwrap();
        n.set_equipment("helmet", Some(("iron_helmet", 1)));
        assert_eq!(n.equipment().len(), 1);
        n.set_cosmetic("helmet", &Cosmetic::Empty);
        assert!(n.equipment().is_empty());
        n.set_cosmetic("helmet", &Cosmetic::Off);
        assert_eq!(n.equipment().len(), 1);
    }

    #[test]
    fn the_saddle_hopper_toggles_the_saddle() {
        let mut w = world();
        let id = w.create("Piggy", "PIG", loc(0.0, 0.0), None);
        let uuid = w.saves.get(id).unwrap().uuid();
        w.on_entity_join("minecraft:overworld", 11, uuid, true);
        w.take_ops();
        w.open_saddle(0xA1, id);
        assert!(w.take_ops().iter().any(|o| matches!(o, Op::OpenHopper { items, .. } if items.len() == 5)));
        w.menu_opened(0xA1, Some(3));
        assert!(!w.menu_deposit(0xA1, StackData::new("minecraft:stick", 1)), "only a saddle fits");
        assert!(w.menu_deposit(0xA1, StackData::new("minecraft:saddle", 3)));
        assert_eq!(w.take_deposit_left(), 2);
        assert!(w.saves.get(id).unwrap().saddle());
        assert!(consoles(&mut w).iter().any(|c| c.contains("data merge entity") && c.contains("saddle")));
        assert!(matches!(w.menu_click(0xA1, 3, 1, 0, false), MenuAction::Take(s) if s.id == "minecraft:saddle"));
        assert!(!w.saves.get(id).unwrap().saddle());
    }

    #[test]
    fn the_enderman_hopper_sets_the_carried_block() {
        let mut w = world();
        let id = w.create("Ender", "ENDERMAN", loc(0.0, 0.0), None);
        let uuid = w.saves.get(id).unwrap().uuid();
        w.on_entity_join("minecraft:overworld", 12, uuid, true);
        w.take_ops();
        w.open_enderman(0xA1, id);
        w.take_ops();
        w.menu_opened(0xA1, Some(4));
        assert!(w.menu_deposit(0xA1, StackData::new("minecraft:dirt", 1)));
        assert_eq!(w.saves.get(id).unwrap().carried_block().as_deref(), Some("minecraft:dirt"));
        assert!(consoles(&mut w).iter().any(|c| c.contains("carriedBlockState") && c.contains("minecraft:dirt")));
        assert!(w.saves.get(id).unwrap().equipment().is_empty(), "no equipment NBT for an enderman");
    }

    #[test]
    fn the_sheep_equipper_dyes_shears_and_whitens() {
        let mut w = world();
        let id = w.create("Dolly", "SHEEP", loc(0.0, 0.0), None);
        let uuid = w.saves.get(id).unwrap().uuid();
        w.on_entity_join("minecraft:overworld", 13, uuid, true);
        w.take_ops();
        w.equip_editors.insert(0xA1, id);
        let p = player(1.0, 0.0);
        assert!(w.on_click(13, &p, Hand::Right, Some("minecraft:red_dye")));
        let colour = w.saves.get(id).unwrap().map.at(&["traits", "sheeptrait", "color"]).and_then(Node::text);
        assert_eq!(colour.as_deref(), Some("RED"));
        assert!(consoles(&mut w).iter().any(|c| c.starts_with("clear Alex minecraft:red_dye")));
        // The respawn after the first click made a new entity.
        w.on_entity_join("minecraft:overworld", 14, uuid, true);
        assert!(w.on_click(14, &p, Hand::Right, Some("minecraft:shears")));
        let sheared = w.saves.get(id).unwrap().map.at(&["traits", "sheeptrait", "sheared"]).and_then(Node::as_bool);
        assert_eq!(sheared, Some(true));
        w.on_entity_join("minecraft:overworld", 15, uuid, true);
        assert!(w.on_click(15, &p, Hand::Right, None));
        let colour = w.saves.get(id).unwrap().map.at(&["traits", "sheeptrait", "color"]).and_then(Node::text);
        assert_eq!(colour.as_deref(), Some("WHITE"));
    }

    #[test]
    fn bystanders_hear_a_targeted_line_when_the_setting_is_on() {
        let mut w = world();
        let id = spawned(&mut w, "Guide", 10);
        let mut a = player(1.0, 0.0);
        a.uuid = 0xA1;
        let mut b = player(2.0, 0.0);
        b.uuid = 0xB2;
        b.name = "Bea".into();
        w.online = vec![a.clone(), b.clone()];
        w.say_to_bystanders(id, "psst", &[a.uuid]);
        assert!(w.take_ops().iter().all(|o| !matches!(o, Op::Tell { .. })), "off by default");
        w.cfg.talk.bystanders_hear_targeted_chat = true;
        w.say_to_bystanders(id, "psst", &[a.uuid]);
        let told: Vec<(u128, String)> = w.take_ops().into_iter().filter_map(|o| if let Op::Tell { player, legacy } = o { Some((player, legacy)) } else { None }).collect();
        assert_eq!(told.len(), 1);
        assert_eq!(told[0].0, 0xB2);
        assert!(told[0].1.contains("psst") && told[0].1.contains("Alex"), "{:?}", told[0].1);
    }

    #[test]
    fn a_purchase_tells_other_mods() {
        // The event carries the shop key, the buyer and the item's place.
        let e = NpcEvent::ShopPurchase { shop: "bank".into(), player: 1, player_name: "Alex".into(), page: 0, slot: 4 };
        assert_eq!(e.clone(), e);
    }

    fn say_paid(w: &mut World, id: i32) {
        w.saves.get_mut(id).unwrap().add_command("say paid", Hand::Right, false, false);
        w.take_ops();
    }

    #[test]
    fn a_custom_error_message_replaces_the_configured_one() {
        let mut w = world();
        let id = spawned(&mut w, "Guide", 10);
        say_paid(&mut w, id);
        w.saves.get_mut(id).unwrap().set_custom_error_message("on_cooldown", "Slow down");
        let p = player(1.0, 1.0);
        assert!(w.on_click(10, &p, Hand::Right, None));
        w.take_ops();
        assert!(w.on_click(10, &p, Hand::Right, None));
        assert_eq!(tells(&w.take_ops()), vec!["Slow down".to_owned()]);
    }

    #[test]
    fn execute_runs_the_commands_of_one_hand_for_a_player() {
        let mut w = world();
        let id = spawned(&mut w, "Guide", 10);
        say_paid(&mut w, id);
        assert!(!w.dispatch_commands(id, 0xA1, Hand::Right), "not online");
        w.online = vec![player(1.0, 1.0)];
        assert!(w.dispatch_commands(id, 0xA1, Hand::Left));
        assert!(consoles(&mut w).is_empty(), "the command is for the right hand");
        assert!(w.dispatch_commands(id, 0xA1, Hand::Right));
        assert!(consoles(&mut w).contains(&"say paid".to_owned()));
    }

    #[test]
    fn forgetting_a_player_clears_the_cooldown_and_the_clear_error_command_too() {
        let mut w = world();
        let id = spawned(&mut w, "Guide", 10);
        say_paid(&mut w, id);
        let p = player(1.0, 1.0);
        assert!(w.on_click(10, &p, Hand::Right, None));
        w.take_ops();
        assert!(w.on_click(10, &p, Hand::Right, None));
        assert_eq!(tells(&w.take_ops()).len(), 1, "on cooldown");
        w.clear_command_errors(id, "on_cooldown", Some(0xA1));
        assert!(w.on_click(10, &p, Hand::Right, None));
        assert!(consoles(&mut w).contains(&"say paid".to_owned()));
        w.forget_player(id, None);
        assert!(w.on_click(10, &p, Hand::Right, None));
        assert!(consoles(&mut w).contains(&"say paid".to_owned()));
    }

    #[test]
    fn temporary_permissions_surround_the_command() {
        let mut w = world();
        let id = spawned(&mut w, "Guide", 10);
        say_paid(&mut w, id);
        w.saves.get_mut(id).unwrap().set_temporary_permissions(&["kit.use".to_owned()], 0);
        assert!(w.on_click(10, &player(1.0, 1.0), Hand::Right, None));
        let c = consoles(&mut w);
        assert_eq!(c.len(), 3, "{c:?}");
        assert!(c[0].contains("permission settemp kit.use true"));
        assert_eq!(c[1], "say paid");
        assert!(c[2].contains("permission unsettemp kit.use"));
    }

    #[test]
    fn the_item_cost_menu_saves_what_is_in_it() {
        let mut w = world();
        let id = spawned(&mut w, "Guide", 10);
        say_paid(&mut w, id);
        w.open_item_cost(0xA1, id, None);
        w.take_ops();
        w.menu_opened(0xA1, Some(5));
        assert!(w.menu_deposit(0xA1, StackData::new("minecraft:diamond", 3)));
        w.menu_closed(0xA1, 5);
        let cost = w.saves.get(id).unwrap().trait_item_cost();
        assert_eq!((cost[0].id.as_str(), cost[0].amount), ("minecraft:diamond", 3));
        w.open_item_cost(0xA1, id, Some("0".to_owned()));
        w.take_ops();
        w.menu_opened(0xA1, Some(6));
        assert!(w.menu_deposit(0xA1, StackData::new("minecraft:emerald", 1)));
        w.menu_closed(0xA1, 6);
        assert_eq!(w.saves.get(id).unwrap().commands()[0].item_cost.len(), 1);
    }

    #[test]
    fn a_remembered_sequence_survives_a_forgotten_memory() {
        use crate::horizon_citizens_core::npc::ExecutionMode;
        let mut w = world();
        let id = spawned(&mut w, "Guide", 10);
        let n = w.saves.get_mut(id).unwrap();
        n.add_command("say one", Hand::Right, false, false);
        n.add_command("say two", Hand::Right, false, false);
        n.set_execution_mode(ExecutionMode::Sequential);
        n.set_remember_last_used(true);
        w.take_ops();
        let p = player(1.0, 1.0);
        assert!(w.on_click(10, &p, Hand::Right, None));
        assert_eq!(consoles(&mut w), vec!["say one".to_owned()]);
        w.cmd_last_id.clear();
        w.cmd_last.clear();
        assert!(w.on_click(10, &p, Hand::Right, None));
        assert_eq!(consoles(&mut w), vec!["say two".to_owned()]);
    }

    #[test]
    fn creating_charges_the_cost_unless_it_is_ignored() {
        use crate::horizon_citizens_core::commands::run;
        fn pay(who: u128, cost: f64) -> Result<String, String> {
            if who == 1 { Ok(format!("{cost}")) } else { Err("not enough money".into()) }
        }
        let mut w = world();
        w.create_charge = Some(pay);
        let mut s = Sender { uuid: Some(1), name: "Rich".into(), permission: 0, grants: vec!["citizens.npc.create".into(), "citizens.npc.create.player".into()], at: Some(("minecraft:overworld".into(), Location { world: "world".into(), x: 0.0, y: 64.0, z: 0.0, yaw: 0.0, pitch: 0.0, body_yaw: 0.0 })) };
        let out = run(&mut w, &s, "npc create Bob", || Err("x".into())).join("|");
        assert!(out.contains("Withdrew") && out.contains("100"), "{out}");
        s.uuid = Some(2);
        let out = run(&mut w, &s, "npc create Eve", || Err("x".into())).join("|");
        assert!(out.contains("not enough money"), "{out}");
        s.grants.push("citizens.npc.ignore-cost".into());
        let out = run(&mut w, &s, "npc create Eve", || Err("x".into())).join("|");
        assert!(!out.contains("not enough money"), "{out}");
    }

    fn creator() -> Sender {
        Sender {
            uuid: Some(7),
            name: "Op".into(),
            permission: 4,
            grants: Vec::new(),
            at: Some(("minecraft:overworld".into(), Location { world: "world".into(), x: 0.0, y: 64.0, z: 0.0, yaw: 0.0, pitch: 0.0, body_yaw: 0.0 })),
        }
    }

    #[test]
    fn the_original_aliases_resolve() {
        use crate::horizon_citizens_core::commands::run;
        let mut w = world();
        let s = creator();
        let id = spawned(&mut w, "Guide", 10);
        w.select(7, id);
        for line in ["npc cload", "npc pushable", "npc vel 0 1 0", "npc nophysics", "npc target -c", "npc donkey"] {
            let out = run(&mut w, &s, line, || Err("x".into())).join("|");
            assert!(!out.contains("not supported") && !out.contains("Too many"), "{line}: {out}");
        }
    }

    #[test]
    fn a_temporary_npc_is_not_written_and_a_baby_has_an_age() {
        use crate::horizon_citizens_core::commands::run;
        let mut w = world();
        let s = creator();
        let out = run(&mut w, &s, "npc create Temp -t -b --type cow", || Err("x".into())).join("|");
        assert!(out.contains("as a baby"), "{out}");
        let id = w.selected(7).unwrap();
        assert!(w.saves.temporary.contains(&id));
        assert_eq!(w.saves.get(id).unwrap().map.at(&["traits", "age", "age"]).and_then(Node::as_i64), Some(-24000));
        assert!(!w.saves.to_yaml().contains("Temp"), "a temporary NPC is not saved");
    }

    #[test]
    fn a_targeted_player_is_walked_to_until_cancelled() {
        let mut w = world();
        let id = spawned(&mut w, "Guide", 10);
        w.online = vec![player(30.0, 0.0)];
        w.set_target(id, 0xA1, false);
        for _ in 0..3 {
            w.tick(&[player(30.0, 0.0)]);
        }
        assert!(w.is_navigating(id));
        w.clear_target(id);
        assert!(!w.is_navigating(id));
    }

    #[test]
    fn lookclose_options_are_saved() {
        use crate::horizon_citizens_core::commands::run;
        let mut w = world();
        let s = creator();
        let id = spawned(&mut w, "Guide", 10);
        w.select(7, id);
        let out = run(&mut w, &s, "npc lookclose --targetnpcs true --randomlookdelay 3s --headonly true", || Err("x".into())).join("|");
        assert!(!out.contains("not supported"), "{out}");
        let n = w.saves.get(id).unwrap();
        assert!(n.lookclose_flag("targetnpcs", false) && n.lookclose_flag("headonly", false));
        assert_eq!(n.random_look_delay(), Some(60));
    }
}
