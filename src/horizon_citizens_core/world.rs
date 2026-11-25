//! The NPCs at run time: which are out in the world under which entity id, what to send the game
//! (console commands and the like, run by the glue at the next tick), clicks, look-close, walking
//! along waypoints, saving.
//!
//! Nothing here calls the game: every change is an [`Op`] the glue takes with [`World::take_ops`]
//! and runs outside its lock, and every event for other mods is an [`NpcEvent`] taken with
//! [`World::take_events`].

use crate::horizon_citizens_core::config::Config;
use crate::horizon_citizens_core::tree::TreeExt;
use crate::horizon_citizens_core::yaml::Node;
use crate::horizon_citizens_core::look::{self, PlayerView, Rng};
use crate::horizon_citizens_core::npc::{CostItem, StackData, Hand, Location, NpcCommand, NpcData, Skin};
use crate::horizon_citizens_core::saves::Saves;
use crate::horizon_citizens_core::spawn;
use crate::horizon_citizens_core::text;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;

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
    /// A command's price, paid: take `levels` experience levels and set each slot to the count it
    /// keeps (0 empties it). Sent only after every check of the command passed.
    Charge { player: u128, levels: i32, slots: Vec<(u32, u32)> },
    /// `NMS.setShouldJump`: the mob's jump control fires on its next tick.
    Jump { entity: u32 },
    /// Seat a rider on a vehicle entity (`Entity.startRiding`): the player `player`, else the entity `rider`.
    Ride { player: Option<u128>, rider: u32, vehicle: u32 },
    /// Take a rider off its vehicle (`leaveVehicle`): the player `player`, else the entity `entity`.
    Dismount { player: Option<u128>, entity: u32 },
    /// Open a chest menu of `rows` rows for a player (`Host::open_menu`); the glue answers with
    /// [`World::menu_opened`].
    OpenMenu { player: u128, title: String, rows: u8, items: Vec<Option<StackData>> },
    /// Replace everything an open menu shows (`menu_set_items`).
    SetMenuItems { player: u128, window: i32, items: Vec<Option<StackData>> },
}

/// Which NPC data a menu edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuKind {
    Inventory,
    Drops,
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
}

#[derive(Clone, Debug)]
struct MenuSession {
    npc: i32,
    kind: MenuKind,
    rows: u8,
    window: Option<i32>,
    slots: Vec<Option<StackData>>,
    chances: Vec<f64>,
}

impl MenuSession {
    /// `DropsTrait.DropsGUI`: items live in rows 0, 2 and 4; rows 1 and 3 hold the chance barriers.
    fn is_item_slot(&self, slot: usize) -> bool {
        self.kind == MenuKind::Inventory || (slot / 9) % 2 == 0
    }

    fn shown(&self) -> Vec<Option<StackData>> {
        let mut out = self.slots.clone();
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
    /// The clicking player's level and inventory (the host reads them before the click).
    pub player_cost: Option<PlayerCost>,
    /// `MountTrait.currentMount`: NPC id -> (its entity, the entity it sits on).
    mounted: HashMap<i32, (u32, u32)>,
    menus: HashMap<u128, MenuSession>,
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
        World {
            cfg,
            saves,
            saves_path,
            live: BTreeMap::new(),
            by_entity: HashMap::new(),
            walks: BTreeMap::new(),
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
            player_cost: None,
            mounted: HashMap::new(),
            menus: HashMap::new(),
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
        match &self.saves_path {
            Some(p) => self.saves.save(p),
            None => Ok(()),
        }
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
        self.events.push(NpcEvent::Spawned {
            npc: id,
            entity,
            dimension: dimension.to_owned(),
        });
        false
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
        let commands: Vec<_> = npc.commands().into_iter().filter(|c| c.hand.matches(hand, player.sneaking)).collect();
        if !commands.is_empty() {
            self.run_commands(player, hand, &npc, id, commands);
        }
        true
    }

    /// The permissions the commands of the NPC behind `entity` ask (and the `ignoreerrors` nodes), for the
    /// host to look up before a click: see `held_perms`.
    pub fn command_perms(&self, entity: u32) -> Vec<String> {
        let Some(npc) = self.npc_of_entity(entity).and_then(|id| self.saves.get(id)) else {
            return Vec::new();
        };
        let mut v: Vec<String> = npc.commands().into_iter().flat_map(|c| c.perms).collect();
        if !v.is_empty() || !npc.commands().is_empty() {
            v.extend(IGNORE_ERRORS.iter().map(|s| (*s).to_owned()));
            v.extend(COST_IGNORE.iter().map(|s| (*s).to_owned()));
        }
        v.sort();
        v.dedup();
        v
    }

    fn command_error(&mut self, player: &PlayerView, npc: &NpcData, kind: &str, msg: &str, secs: i64, n: i64, sent: &mut BTreeSet<String>) {
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

    /// `CommandTrait.chargeCommandCosts` for `experienceCost` (levels) and `itemCost`: the levels and
    /// slots a command's price takes, or `None` (after the error message) when the player cannot pay.
    /// A player holding the matching `ignoreerrors` node pays nothing.
    fn check_charge(
        &mut self,
        player: &PlayerView,
        npc: &NpcData,
        c: &NpcCommand,
        em: &crate::horizon_citizens_core::config::ErrorMessages,
        sent: &mut BTreeSet<String>,
    ) -> Option<(i32, Vec<(u32, u32)>)> {
        let held = |n: &str| self.held_perms.contains(n);
        if held(COST_IGNORE[0]) {
            return Some((0, Vec::new()));
        }
        let pay_xp = c.experience_cost != -1 && !held(COST_IGNORE[2]);
        let pay_items = !c.item_cost.is_empty() && !held(COST_IGNORE[3]);
        let have = self.player_cost.clone().unwrap_or_default();
        let mut levels = 0;
        if pay_xp {
            if i64::from(have.levels) < c.experience_cost {
                self.command_error(player, npc, "missing_experience", &em.not_enough_experience, 0, c.experience_cost, sent);
                return None;
            }
            levels = c.experience_cost as i32;
        }
        let mut slots = Vec::new();
        if pay_items {
            match plan_item_charge(&have.slots, &c.item_cost) {
                Some(plan) => slots = plan,
                None => {
                    let first = &c.item_cost[0];
                    let msg = em.missing_item.replace("{1}", &first.amount.to_string()).replace("{0}", &first.name);
                    self.command_error(player, npc, "missing_item", &msg, 0, 0, sent);
                    return None;
                }
            }
        }
        Some((levels, slots))
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
                let mut last = self.cmd_last_id.get(&(who, id)).copied().unwrap_or(-1);
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
            if let Some(why) = &c.unenforced {
                eprintln!("[citizens] NPC {id}: command {} skipped ({why} is not enforced on Horizon yet)", c.key);
                continue;
            }
            let em = self.cfg.commands.error_messages.clone();
            // `chargeCommandCosts`: the price is checked first; one that cannot be paid stops the click.
            let (charge_levels, charge_slots) = match self.check_charge(player, npc, &c, &em, &mut sent) {
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
            }
            if charge_levels > 0 || !charge_slots.is_empty() {
                // The rest of this click sees what is left.
                if let Some(pc) = self.player_cost.as_mut() {
                    pc.levels -= charge_levels;
                    for (slot, keep) in &charge_slots {
                        if let Some(e) = pc.slots.iter_mut().find(|e| e.0 == *slot) {
                            e.2 = *keep;
                        }
                    }
                    pc.slots.retain(|e| e.2 > 0);
                }
                self.ops.push(Op::Charge { player: player.uuid, levels: charge_levels, slots: charge_slots });
            }
            let line = text::fill_placeholders(&c.command, &player.name, &npc.name(), id);
            let line = line.trim_start_matches('/').to_owned();
            // `CommandTrait.NPCCommand.run`: `npc` swaps the clicker for the NPC when the NPC is a player.
            let npc_uuid = npc.uuid().filter(|_| c.npc && c.player && npc.kind().eq_ignore_ascii_case("PLAYER"));
            let op = if let Some(me) = npc_uuid {
                let who = crate::horizon_citizens_core::ids::format_uuid(me);
                Op::Console(format!("execute as {who} at {who} run {line}"))
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
            if c.delay > 0 {
                self.delayed.push((now + c.delay as u64, op));
            } else {
                self.ops.push(op);
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
            },
        );
        true
    }
    /// Drops a walk set by following or wandering (the setting changed).
    pub fn cancel_follow_walk(&mut self, id: i32) {
        if self.walks.get(&id).is_some_and(|w| w.follow || w.waypoint.is_none()) {
            self.walks.remove(&id);
        }
    }
    pub fn cancel_navigation(&mut self, id: i32) {
        self.walks.remove(&id);
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
    // ---- the inventory and drops menus ----------------------------------------------------

    /// `/npc inventory`: the NPC's `inventory` trait in a chest for `player`. Four rows, more when a saved
    /// slot lies beyond them (the size Citizens' own trait uses is not in the source this port has).
    pub fn open_inventory(&mut self, player: u128, npc: i32) {
        let Some(data) = self.saves.get(npc) else { return };
        let stacks = data.inventory();
        let top = stacks.iter().map(|(s, _)| *s).max().unwrap_or(0) as usize;
        let rows = ((top / 9) + 1).clamp(4, 6) as u8;
        let mut slots = vec![None; usize::from(rows) * 9];
        for (slot, st) in stacks {
            if let Some(cell) = slots.get_mut(slot.max(0) as usize) {
                *cell = Some(st);
            }
        }
        let title = text::strip(&data.name());
        let chances = vec![1.0; slots.len()];
        self.start_menu(player, title, MenuSession { npc, kind: MenuKind::Inventory, rows, window: None, slots, chances });
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
        self.start_menu(player, "Add items for drops".into(), MenuSession { npc, kind: MenuKind::Drops, rows: 5, window: None, slots, chances });
    }

    fn start_menu(&mut self, player: u128, title: String, session: MenuSession) {
        let items = session.shown();
        let rows = session.rows;
        self.menus.insert(player, session);
        self.ops.push(Op::OpenMenu { player, title, rows, items });
    }

    /// The glue opened the menu: its window id.
    pub fn menu_opened(&mut self, player: u128, window: Option<i32>) {
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
    /// barrier changes its chance (left click up 10 %, right click down 10 %: the step of Citizens'
    /// percentage handler is not in the source this port has). A slot of the player's own inventory
    /// (the chest menu's bottom 36 slots) is offered for deposit.
    pub fn menu_click(&mut self, player: u128, window: i32, slot: i32, button: i32) -> MenuAction {
        let Some(m) = self.menus.get_mut(&player).filter(|m| m.window == Some(window)) else {
            return MenuAction::Nothing;
        };
        let size = usize::from(m.rows) * 9;
        let Ok(slot) = usize::try_from(slot) else { return MenuAction::Nothing };
        if slot >= size + 36 {
            return MenuAction::Nothing;
        }
        if slot >= size {
            let p = slot - size;
            let inv_slot = if p < 27 { 9 + p } else { p - 27 };
            return MenuAction::Deposit { inv_slot: inv_slot as u32 };
        }
        if !m.is_item_slot(slot) {
            if slot >= 9 && m.slots[slot - 9].is_some() {
                let c = &mut m.chances[slot - 9];
                let step = if button == 1 { -0.1 } else { 0.1 };
                *c = (((*c + step) * 100.0).round() / 100.0).clamp(0.0, 1.0);
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
        let Some(m) = self.menus.get_mut(&player) else { return false };
        let free = (0..m.slots.len()).find(|i| m.is_item_slot(*i) && m.slots[*i].is_none());
        let Some(i) = free else { return false };
        m.slots[i] = Some(stack);
        m.chances[i] = 1.0;
        self.refresh_menu(player);
        true
    }

    /// The menu was closed: the NPC keeps what it shows (`DropsGUI.onClose`, the inventory's close).
    pub fn menu_closed(&mut self, player: u128, window: i32) {
        if self.menus.get(&player).is_none_or(|m| m.window != Some(window)) {
            return;
        }
        let Some(m) = self.menus.remove(&player) else { return };
        let Some(npc) = self.saves.get_mut(m.npc) else { return };
        match m.kind {
            MenuKind::Inventory => npc.set_inventory(
                m.slots.into_iter().enumerate().filter_map(|(i, s)| Some((i as i64, s?))).collect(),
            ),
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
        self.check_all_mounts();
        if self.tick % 2 == 0 {
            self.look_close(players);
        }
        if let Some(since) = self.dirty_since
            && self.tick.saturating_sub(since) >= u64::from(self.cfg.save_delay_seconds) * 20
            && let Err(e) = self.save_now()
        {
            eprintln!("[citizens] could not save NPCs: {e}");
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

    fn look_close(&mut self, players: &[PlayerView]) {
        let mut turns = Vec::new();
        for (id, live) in &self.live {
            if live.entity.is_none() || self.walks.contains_key(id) {
                continue;
            }
            let Some(npc) = self.saves.get(*id) else {
                continue;
            };
            if !npc.look_close() {
                continue;
            }
            let Some(at) = &live.at else {
                continue;
            };
            let range = npc.look_range().unwrap_or(self.cfg.look_close.range);
            let from = look::eye([at.x, at.y, at.z]);
            let want = match look::target(players, &live.dimension, from, range) {
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
        let delay = u64::from(self.cfg.look_close.random_look_delay_ticks);
        for (id, yaw, pitch) in turns {
            if let Some(l) = self.live.get_mut(&id) {
                l.next_random_look = self.tick + delay;
            }
            self.face(id, yaw, pitch);
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
        for npc in &self.saves.npcs {
            let id = npc.id;
            if !self.is_out(id) {
                continue;
            }
            let Some(at) = self.current_location(id) else { continue };
            let speed = 0.2 * npc.speed_modifier();
            if let Some(target) = npc.follow_target() {
                let same = self.locate(target).filter(|(d, _)| self.dimension_of(id).as_ref() == Some(d));
                let Some((_, pos)) = same else { continue };
                let d = [pos[0] - at.x, pos[1] - at.y, pos[2] - at.z];
                let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                // Citizens' default distance margin is 2 blocks.
                let margin = if npc.follow_margin() > 0.0 { npc.follow_margin() } else { 2.0 };
                if len <= margin {
                    clear.push(id);
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
            self.walks.insert(id, Walk { target, speed, waypoint: None, follow });
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
                },
            );
        }
        let mut done = Vec::new();
        let mut moves = Vec::new();
        for (id, walk) in &self.walks {
            let Some(live) = self.live.get(id) else {
                done.push((*id, None));
                continue;
            };
            let Some(at) = &live.at else {
                continue;
            };
            let d = [walk.target[0] - at.x, walk.target[1] - at.y, walk.target[2] - at.z];
            let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            let mut next = at.clone();
            if len <= walk.speed {
                next.x = walk.target[0];
                next.y = walk.target[1];
                next.z = walk.target[2];
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
        assert!(ops.contains(&Op::Charge { player: 0xA1, levels: 5, slots: vec![] }));
        assert!(ops.contains(&Op::Console("say paid".into())));
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
        assert!(ops.contains(&Op::Charge { player: 0xA1, levels: 0, slots: vec![(2, 0), (5, 3)] }));
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
        let uuid = crate::horizon_citizens_core::ids::format_uuid(w.saves.get(id).unwrap().uuid().unwrap());
        w.take_ops();
        assert!(w.on_click(10, &player(1.0, 1.0), Hand::Right, None));
        let cmds = consoles(&mut w);
        assert_eq!(cmds, vec![format!("execute as {uuid} at {uuid} run spawn")]);
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
        let Some(Op::OpenMenu { rows, items, .. }) = ops.first() else { panic!("{ops:?}") };
        assert_eq!(*rows, 4);
        assert_eq!(items[2].as_ref().map(|s| s.count), Some(5));
        w.menu_opened(0xA1, Some(7));
        // A click in another window is not ours.
        assert_eq!(w.menu_click(0xA1, 8, 2, 0), MenuAction::Nothing);
        // The first slot of the player's own part (36) is main-inventory slot 9; the last is hotbar 8.
        assert_eq!(w.menu_click(0xA1, 7, 36, 0), MenuAction::Deposit { inv_slot: 9 });
        assert_eq!(w.menu_click(0xA1, 7, 71, 0), MenuAction::Deposit { inv_slot: 8 });
        assert!(w.menu_deposit(0xA1, StackData::new("minecraft:stone", 64)));
        assert!(matches!(menu_ops(&mut w).last(), Some(Op::SetMenuItems { window: 7, .. })));
        let MenuAction::Take(apples) = w.menu_click(0xA1, 7, 2, 0) else { panic!("take") };
        assert_eq!((apples.id.as_str(), apples.count), ("minecraft:apple", 5));
        w.menu_closed(0xA1, 7);
        let saved = w.saves.get(id).unwrap().inventory();
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
        assert_eq!(w.menu_click(0xA1, 3, 9, 0), MenuAction::Nothing);
        let Some(Op::SetMenuItems { items, .. }) = menu_ops(&mut w).pop() else { panic!("refresh") };
        assert_eq!(items[9].as_ref().and_then(|s| s.meta.display_name.clone()).as_deref(), Some("Drop chance 60%"));
        assert!(w.menu_deposit(0xA1, StackData::new("minecraft:apple", 1)));
        w.menu_closed(0xA1, 3);
        let d = w.saves.get(id).unwrap().drops();
        assert_eq!(d.len(), 2);
        assert_eq!((d[0].0.id.as_str(), d[0].1), ("minecraft:bone", 0.6));
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
}
