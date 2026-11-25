//! horizon-citizens as an arismoko native trait mod (`mod_api`). The logic is the unchanged
//! `horizon-citizens-core` crate (`core/`); this file only wires its events, commands and work
//! queue to arismoko's hooks and host methods. See PORT_NOTES.md for what is missing meanwhile.
//!
//! Public interface for other mods (Sentinel's bridge, unchanged from 0.1.0):
//! `mod_citizens::{api, npc, yaml, DespawnReason, NpcEvent}`.

pub mod horizon_citizens_core;
use horizon_citizens_core as core;
use mod_api::{
    AttackEntityEvent, Command, CommandArgumentValue, CommandInvocation, CommandSource, Config,
    ConfigKind, ConfigLoadingEvent, ConfigReloadingEvent, EntityInteractEvent,
    EntityJoinLevelEvent, EntityLeaveLevelEvent, Flow, Hand, Hooks, HostSide, LevelRandomHost,
    LivingIncomingDamageEvent, NodeDefault, PermissionGatherNodesEvent, PermissionLevel, PermissionNode,
    PermissionValueType, PlayerCommandEvent, PlayerCommandsSentEvent, PlayerRef, RegisterCommandsEvent, RegisterNodeCommandsEvent,
    RegisterConfigsEvent, RemovalReason, RunModCommandEvent, ServerHost, ServerStoppingEvent,
    ServerTickEvent, ServerTickPhase, Text, ItemStack, MenuClickedEvent, MenuClosedEvent, MenuReplace, ServerChatEvent,
    MerchantTradeSelectedEvent, MerchantOffer, ItemCost, Pose as GamePose, SynchedState,
};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};

pub use core::api;
// For dependent mods (Sentinel keeps its trait data as a `yaml::Node` at an `npc::Location`).
pub use core::world::{DespawnReason, NpcEvent};
pub use core::{npc, yaml};

const MOD_ID: &str = "citizens";

pub struct Citizens {
    state: Mutex<State>,
}

struct State {
    /// The effective config (see [`effective_config`]).
    cfg: core::config::Config,
    /// The TOML the engine loaded: used when the server has no Citizens `config.yml`.
    toml: core::config::Config,
    installed: bool,
    skins_tx: Sender<core::skin::Fetched>,
    skins_rx: Receiver<core::skin::Fetched>,
    work_tx: Sender<core::skin::Done>,
    work_rx: Receiver<core::skin::Done>,
    /// The NPC the main hand claimed in the click being handled. The off hand of that same click
    /// is refused for it, as the original (asked once per click) did not run it at all.
    last_claim: Option<i32>,
}

impl Default for Citizens {
    fn default() -> Self {
        let (tx, rx) = channel();
        let (work_tx, work_rx) = channel();
        Self {
            state: Mutex::new(State {
                cfg: core::config::Config::default(),
                toml: core::config::Config::default(),
                installed: false,
                skins_tx: tx,
                skins_rx: rx,
                work_tx,
                work_rx,
                last_claim: None,
            }),
        }
    }
}

fn lock(m: &Mutex<State>) -> std::sync::MutexGuard<'_, State> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The server folder: the parent of the open world's folder. None before a world is open.
fn server_of(host: &dyn ServerHost) -> Option<PathBuf> {
    host.world_dir()
        .and_then(|w| w.parent().map(Path::to_path_buf))
}

/// The server folder on a config event (only a server side has one).
fn server_of_side(host: &HostSide) -> Option<PathBuf> {
    match host {
        // `world_dir` panics during the Config phase ("unavailable during Config"); a dedicated server
        // runs in its own folder, so the working directory stands in. TODO(arismoko-api: server_dir)
        HostSide::Server(_) => std::env::current_dir().ok(),
        HostSide::Client(_) => None,
    }
}

/// `saves-file` against the server folder.
fn saves_path(host: &dyn ServerHost, cfg: &core::config::Config) -> Option<PathBuf> {
    let file = PathBuf::from(&cfg.saves_file);
    if file.is_absolute() {
        return Some(file);
    }
    server_of(host).map(|server| server.join(file))
}

/// The config in force: Citizens' own `plugins/Citizens/config.yml` when the server has one (the
/// TOML is then not used), else the TOML. The spawn world is the server's `level-name` either way.
fn effective_config(server: Option<&Path>, toml: &core::config::Config) -> core::config::Config {
    let mut cfg = toml.clone();
    let Some(server) = server else {
        return cfg;
    };
    let original = server.join("plugins/Citizens/config.yml");
    match std::fs::read_to_string(&original) {
        Ok(text) => match core::config::Config::from_citizens_yaml(&text) {
            Ok(c) => cfg = c,
            Err(e) => eprintln!(
                "[citizens] {}: {e}; the TOML config is used",
                original.display()
            ),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => eprintln!("[citizens] {}: {e}", original.display()),
    }
    // Only `level-name` is read from server.properties (the rest holds secrets).
    if let Ok(props) = std::fs::read_to_string(server.join("server.properties"))
        && let Some(level) = core::config::level_name_from_properties(&props)
    {
        cfg = cfg.with_level_name(&level);
    }
    cfg
}

/// A player as the core sees it, with the rotation Citizens' `Sender` and look-close need.
struct PlayerInfo {
    view: core::look::PlayerView,
    /// Yaw and pitch in degrees.
    rot: [f32; 2],
}

/// Reads a player's state. None when the player has no body in a level (between worlds).
fn player_info(host: &dyn ServerHost, p: PlayerRef) -> Option<PlayerInfo> {
    let entity = p.entity();
    let pos = entity.position(host)?;
    let rot = entity.rotation(host).unwrap_or([0.0, 0.0]);
    let dimension = p.level(host)?.dimension_id().to_string();
    Some(PlayerInfo {
        view: core::look::PlayerView {
            uuid: p.uuid(),
            name: p.name(host).unwrap_or_default(),
            dimension,
            pos,
            game_mode: p.game_mode(host).map_or(0, |mode| mode.id()),
            permission: p.permission_level(host).unwrap_or(0),
            entity_id: entity.id() as i32,
            sneaking: entity.is_shift_key_down(host).unwrap_or(false),
        },
        rot,
    })
}

/// Operator level as the number Citizens compares (`PermissionLevel` has no numeric value).
fn level_num(level: PermissionLevel) -> u8 {
    match level {
        PermissionLevel::All => 0,
        PermissionLevel::Moderators => 1,
        PermissionLevel::Gamemasters => 2,
        PermissionLevel::Admins => 3,
        PermissionLevel::Owners => 4,
        _ => 0,
    }
}

/// The typed Boolean node of a Citizens permission name (`citizens.npc.create`).
fn perm_node(name: &str) -> PermissionNode {
    PermissionNode::new(name, PermissionValueType::Boolean)
}

/// Whether `p` holds the Boolean node `name` from the server's handler. A refused query (an undeclared name,
/// a handler failure) is a denial.
fn holds(host: &mut dyn ServerHost, p: PlayerRef, name: &str) -> bool {
    p.permission_bool(host, &perm_node(name)).unwrap_or(false)
}

/// Whether `p` may run any usage under `root`: the operator level, or a node of one of its usages (or a parent).
fn may_any(host: &mut dyn ServerHost, p: PlayerRef, root: &str, level: u8, usages: &[String]) -> bool {
    if p.permission_level(&*host).unwrap_or(0) >= level {
        return true;
    }
    usages
        .iter()
        .filter(|u| u.split_whitespace().next() == Some(root))
        .any(|u| {
            let node = core::commands::node_of_usage(u);
            holds(host, p, node) || core::commands::parents_of(node).iter().any(|&parent| holds(host, p, parent))
        })
}

/// The `citizens.*` nodes a player below `command_level` holds, as the handler answers them. Wildcard parents
/// (`citizens.npc.*`) are declared nodes: a player holding one holds its children (`parents_of`).
fn granted_nodes(host: &mut dyn ServerHost, p: PlayerRef, level: u8, command_level: u8) -> Vec<String> {
    if level >= command_level {
        return Vec::new();
    }
    // The per-type create nodes are asked by `create`, so they are part of the grants.
    let mut all: Vec<String> = core::commands::all_nodes().into_iter().map(str::to_owned).collect();
    all.extend(core::commands::create_type_nodes());
    all.sort();
    all.dedup();
    all.into_iter()
        .filter(|node| {
            holds(host, p, node)
                || core::commands::parents_of(node)
                    .iter()
                    .any(|&parent| holds(host, p, parent))
        })
        .collect()
}

/// The id of the item in the player's main hand (`minecraft:stick`), None for an empty hand.
fn held_item_id(host: &dyn ServerHost, player: PlayerRef) -> Option<String> {
    let living = host.living(player.entity())?;
    let stack = living.hand_item(host, Hand::Main)?;
    Some(stack.item().id(host))
}

/// Whether a PlaceholderAPI mod answers (set each tick; `papi(...)` in an expression asks it).
static PAPI_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// `Placeholders.replace`: PlaceholderAPI's `%...%` for a player, the text itself without that mod.
fn papi_text(player: Option<u128>, text: &str) -> String {
    #[cfg(feature = "placeholderapi")]
    {
        if PAPI_ON.load(std::sync::atomic::Ordering::Relaxed) {
            return placeholderapi::set_placeholders(player, text);
        }
    }
    let _ = player;
    text.to_owned()
}

/// Whether the PlaceholderAPI mod is installed.
fn papi_installed(host: &dyn ServerHost) -> bool {
    let mut on = false;
    mod_api::if_installed!(host, placeholderapi, {
        on = true;
    });
    on
}

/// Whether an economy provider answers (the vault mod with a registered economy).
fn economy_on(host: &dyn ServerHost) -> bool {
    let mut on = false;
    mod_api::if_installed!(host, vault, {
        on = vault::economy().is_some();
    });
    on
}

/// A player's balance in the economy (0 without one).
fn balance_of(host: &dyn ServerHost, uuid: u128) -> f64 {
    let mut b = 0.0;
    mod_api::if_installed!(host, vault, {
        b = vault::balance(uuid).to_f64();
    });
    let _ = uuid;
    b
}

/// `Economy.format` for the shop descriptions.
fn format_money(amount: f64) -> String {
    #[cfg(feature = "vault")]
    {
        if let Some(m) = vault::Money::from_f64(amount) {
            return vault::format(m);
        }
    }
    core::shop::java_double(amount)
}

/// `PaymentHook`: takes the cost of creating an NPC from the creator's account; the formatted amount, or the
/// economy's reason for refusing.
fn create_charge(uuid: u128, cost: f64) -> Result<String, String> {
    #[cfg(feature = "vault")]
    {
        let Some(m) = vault::Money::from_f64(cost) else { return Err("the cost is not a valid amount".to_owned()) };
        let r = vault::withdraw("Citizens", uuid, m);
        if matches!(r.kind, vault::ResponseType::Success) { Ok(vault::format(m)) } else { Err(r.error.to_string()) }
    }
    #[cfg(not(feature = "vault"))]
    {
        let _ = (uuid, cost);
        Err("there is no economy".to_owned())
    }
}

/// Takes (`delta` below 0) or gives (above 0) money in the economy, as `Citizens`.
fn money_move(host: &dyn ServerHost, uuid: u128, delta: f64) {
    mod_api::if_installed!(host, vault, {
        if let Some(m) = vault::Money::from_f64(delta.abs()) {
            let r = if delta < 0.0 { vault::withdraw("Citizens", uuid, m) } else { vault::deposit("Citizens", uuid, m) };
            if !matches!(r.kind, vault::ResponseType::Success) {
                eprintln!("[citizens] economy refused {delta}: {}", r.error);
            }
        }
    });
    let _ = (uuid, delta);
}

/// What a command's price is charged against: the experience level, the money and the main inventory (slots 0 to 35).
fn cost_state(host: &dyn ServerHost, p: PlayerRef, uuid: u128) -> core::world::PlayerCost {
    core::world::PlayerCost {
        money: balance_of(host, uuid),
        levels: p.experience(host).map_or(0, |x| x.level),
        slots: p
            .inventory_slots(host)
            .unwrap_or_default()
            .into_iter()
            .filter(|(slot, _)| *slot < 36)
            .map(|(slot, st)| {
                let id = st.item().id(host).to_string();
                let id = if id.contains(':') { id } else { format!("minecraft:{id}") };
                (slot, id, st.count())
            })
            .collect(),
    }
}

/// A stack of the NPC's saved items as the game's item, with the components the meta maps. The bare item
/// when the host refuses the text.
fn item_stack_of(host: &dyn ServerHost, s: &core::npc::StackData) -> Option<ItemStack> {
    let comps = core::items::components(&s.meta).join(",");
    let snbt = format!("{{id:\"{}\",count:{},components:{{{comps}}}}}", s.id, s.count.clamp(1, 99));
    host.item_from_snbt(&snbt)
        .or_else(|| host.item_from_snbt(&format!("{{id:\"{}\",count:{}}}", s.id, s.count.clamp(1, 99))))
}

/// A game stack as the NPC saves it. Only what Citizens' meta maps is read (plain name and lore, enchantments,
/// damage): colours and other components of the stack are not kept.
fn stack_data_of(host: &dyn ServerHost, st: &ItemStack) -> core::npc::StackData {
    let id = st.item().id(host).to_string();
    let id = if id.contains(':') { id } else { format!("minecraft:{id}") };
    let mut d = core::npc::StackData::new(&id, i64::from(st.count()));
    d.meta.display_name = st.display_name_plain(host);
    d.meta.lore = st.lore(host).iter().map(Text::plain).collect();
    d.meta.enchants = st.enchantments().into_iter().map(|(e, l)| (e, l as i32)).collect();
    d.meta.damage = st.damage_component().map(|x| x as i32);
    d
}

/// Pays a command's price: takes the levels, then sets each slot to the count it keeps.
fn charge(host: &mut dyn ServerHost, uuid: u128, levels: i32, slots: &[(u32, u32)]) {
    let Some(p) = host.player(uuid) else { return };
    if levels > 0 {
        let now = p.experience(&*host).map_or(0, |x| x.level);
        let _ = p.set_experience_levels(host, (now - levels).max(0));
    }
    if !slots.is_empty() {
        let held = p.inventory_slots(&*host).unwrap_or_default();
        for (slot, keep) in slots {
            let Some((_, st)) = held.iter().find(|(i, _)| i == slot) else { continue };
            let mut st = st.clone();
            let left = if *keep > 0 && st.set_count(*keep).is_ok() { Some(st) } else { None };
            let _ = p.set_inventory_slot(host, *slot, left);
        }
    }
}

/// What a shop interaction needs to know about a player: name, world, level, the 36 main-inventory stacks, and which of
/// the wanted permission nodes they hold (an operator holds every node).
fn shop_state(host: &mut dyn ServerHost, p: PlayerRef, wanted: &[String], cfg: &core::config::Config) -> Option<core::world::ShopPlayer> {
    let info = player_info(&*host, p)?;
    let mut inv: Vec<Option<core::npc::StackData>> = vec![None; 36];
    for (slot, st) in p.inventory_slots(&*host).unwrap_or_default() {
        if let Some(cell) = inv.get_mut(slot as usize) {
            *cell = Some(stack_data_of(&*host, &st));
        }
    }
    let perms: std::collections::BTreeSet<String> = wanted.iter().filter(|n| holds(host, p, n)).cloned().collect();
    Some(core::world::ShopPlayer {
        name: info.view.name,
        world: cfg.worlds.world_name(&info.view.dimension),
        levels: p.experience(&*host).map_or(0, |x| x.level),
        inv,
        perms,
        op: info.view.permission >= cfg.command_level,
        money: balance_of(&*host, info.view.uuid),
    })
}

/// Reads the player for the shop code (before a shop command, a click on a shop menu, a right click on a shop NPC).
fn prime_shop(host: &mut dyn ServerHost, uuid: u128, cfg: &core::config::Config) {
    let Some(p) = host.player(uuid) else { return };
    let wanted = api::with_world(|w| w.shop_wanted_nodes()).unwrap_or_default();
    if let Some(state) = shop_state(host, p, &wanted, cfg) {
        api::with_world(|w| {
            w.shop_player.insert(uuid, state);
        });
    }
}

/// Applies a purchase to the player: the level, and each changed slot. A stack that only changed in count keeps
/// its other components (the port reads just name, lore, enchantments and damage).
fn shop_apply(host: &mut dyn ServerHost, uuid: u128, levels: Option<i32>, slots: &[(u32, Option<core::npc::StackData>)]) {
    let Some(p) = host.player(uuid) else { return };
    if let Some(l) = levels {
        let _ = p.set_experience_levels(host, l.max(0));
    }
    let held = p.inventory_slots(&*host).unwrap_or_default();
    for (slot, new) in slots {
        let stack = match new {
            None => None,
            Some(n) => {
                let same = held.iter().find(|(i, _)| i == slot).and_then(|(_, st)| {
                    let old = stack_data_of(&*host, st);
                    (old.id == n.id && old.meta.display_name == n.meta.display_name && old.meta.lore == n.meta.lore).then(|| st.clone())
                });
                match same {
                    Some(mut st) => {
                        if st.set_count(n.count.clamp(1, 99) as u32).is_ok() {
                            Some(st)
                        } else {
                            item_stack_of(&*host, n)
                        }
                    }
                    None => item_stack_of(&*host, n),
                }
            }
        };
        let _ = p.set_inventory_slot(host, *slot, stack);
    }
}

/// One host call of `entity_set` for an NPC's entity. A call the game refuses (a setting the kind has no
/// use for) is dropped: a fox's state on a panda, a disguise as a type the clients do not have.
fn apply_entity_set(host: &mut dyn ServerHost, entity: u32, set: core::entity_set::EntitySet) {
    use horizon_citizens_core::entity_set::{EntitySet, Pose, Synched};
    let Some(e) = host.entity(entity) else { return };
    match set {
        EntitySet::Disguise(kind) => {
            let key = match &kind {
                None => None,
                Some(id) => {
                    let found = host.entity_type_keys().into_iter().find(|k| k.id(&*host).to_string() == *id);
                    if found.is_none() {
                        eprintln!("[citizens] unknown disguise type {id}");
                        return;
                    }
                    found
                }
            };
            let _ = e.set_disguise(host, key);
        }
        EntitySet::Pose(p) => {
            let pose = match p {
                Pose::Digging => GamePose::Digging,
                Pose::Emerging => GamePose::Emerging,
                Pose::Roaring => GamePose::Roaring,
            };
            let _ = e.set_pose(host, pose);
        }
        EntitySet::Synched(state, on) => {
            let state = match state {
                Synched::FoxInterested => SynchedState::FoxInterested,
                Synched::FoxPouncing => SynchedState::FoxPouncing,
                Synched::FoxFaceplanted => SynchedState::FoxFaceplanted,
                Synched::PandaSneezing => SynchedState::PandaSneezing,
                Synched::PandaRolling => SynchedState::PandaRolling,
                Synched::PandaSitting => SynchedState::PandaSitting,
                Synched::AllayDancing => SynchedState::AllayDancing,
                Synched::VexCharging => SynchedState::VexCharging,
                Synched::PolarBearStanding => SynchedState::PolarBearStanding,
            };
            let _ = e.set_synched_state(host, state, on);
        }
        EntitySet::Targetable(b) => {
            let _ = e.set_targetable(host, b);
        }
        EntitySet::Leashable(b) => {
            let _ = e.set_leashable(host, b);
        }
        EntitySet::TrackingRange(r) => {
            let _ = e.set_tracking_range(host, r);
        }
        EntitySet::ActivationRange(r) => {
            let _ = e.set_activation_range(host, r);
        }
        EntitySet::DestroyBlocks(b) => {
            if let Some(d) = host.dragon(e) {
                let _ = d.set_destroy_blocks(host, b);
            }
        }
        EntitySet::NavigationTicks(b) => {
            if let Some(m) = host.mob(e)
                && m.set_navigation_ticks(host, b)
                && b
            {
                api::with_world(|w| w.nav_ready(entity));
            }
        }
        EntitySet::MoveTo { target, speed } => {
            if let Some(m) = host.mob(e) {
                let _ = m.move_to(host, target, speed);
            }
        }
        EntitySet::NoPhysics(b) => {
            let _ = e.set_no_physics(host, b);
        }
        EntitySet::Velocity(v) => {
            let _ = e.set_velocity(host, v);
        }
        EntitySet::StopNavigation => {
            if let Some(m) = host.mob(e) {
                let _ = m.stop(host);
            }
        }
    }
}

/// A trader shop's merchant screen. Each trade pays with up to two stacks and gives the display item; a
/// trade the game cannot build (an unknown item) is left out.
fn open_merchant(host: &mut dyn ServerHost, player: u128, title: &str, trades: &[core::world::Trade]) -> Option<i32> {
    let mut offers = Vec::new();
    for t in trades {
        let costs: Vec<ItemStack> = t.costs.iter().filter_map(|c| item_stack_of(&*host, c)).collect();
        let (Some(result), Some(first)) = (item_stack_of(&*host, &t.result), costs.first().cloned()) else { continue };
        // The original gives each trade 100000000 uses.
        offers.push(MerchantOffer::new(ItemCost::new(first), costs.get(1).cloned().map(ItemCost::new), result, 100_000_000, 0, 0.0));
    }
    let p = host.player(player)?;
    let title = plain(&*host, title);
    p.open_merchant(host, title, offers).ok().flatten()
}

fn sender_of(info: &PlayerInfo, cfg: &core::config::Config) -> core::world::Sender {
    core::world::Sender {
        uuid: Some(info.view.uuid),
        name: info.view.name.clone(),
        permission: info.view.permission,
        grants: Vec::new(),
        at: Some((
            info.view.dimension.clone(),
            core::npc::Location {
                world: cfg.worlds.world_name(&info.view.dimension),
                x: info.view.pos[0],
                y: info.view.pos[1],
                z: info.view.pos[2],
                yaw: info.rot[0],
                pitch: info.rot[1],
                body_yaw: info.rot[0],
            },
        )),
    }
}

/// A sender with no body: the console, a command block, a function.
fn bodiless_sender(name: &str, level: PermissionLevel) -> core::world::Sender {
    core::world::Sender {
        uuid: None,
        name: name.into(),
        permission: level_num(level),
        grants: Vec::new(),
        at: None,
    }
}

/// The typed line, rebuilt from the parsed command: the literal path, then the present arguments
/// in order. Citizens' usages take words, `text` (the rest of the line) and numbers only.
fn command_line(host: &dyn ServerHost, cmd: &CommandInvocation) -> String {
    let mut words = cmd.literals.clone();
    words.extend(cmd.arguments.iter().map(|a| arg_text(host, &a.value)));
    words.retain(|w| !w.is_empty());
    words.join(" ")
}

fn arg_text(host: &dyn ServerHost, value: &CommandArgumentValue) -> String {
    match value {
        // An `item` argument (`setequipment hand stick`): the core wants the item id word.
        CommandArgumentValue::Item(stack) => stack.item().id(host),
        CommandArgumentValue::String(s) => s.clone(),
        CommandArgumentValue::Integer(i) => i.to_string(),
        CommandArgumentValue::Number(n) => n.to_string(),
        CommandArgumentValue::Boolean(b) => b.to_string(),
        _ => String::new(),
    }
}

/// The kinds an NPC entity can have: mannequins and the types of non-player NPCs. Other kinds
/// joining a level are not looked at (the hook hears every item and arrow).
fn npc_kind(kind: &str) -> bool {
    kind == "minecraft:mannequin"
        || api::with_world(|w| {
            w.saves
                .npcs
                .iter()
                .any(|n| !n.is_player() && core::spawn::entity_kind(&n.kind()) == kind)
        })
        .unwrap_or(false)
}

/// The name of a player by UUID: online, else the server's user cache (`usercache.json`). Never asked
/// of Mojang, so an owner who was never on the server shows as the UUID.
fn profile_by_uuid(host: &dyn ServerHost, uuid: u128) -> Option<String> {
    if let Some(name) = host.player(uuid).and_then(|p| p.name(host)) {
        return Some(name);
    }
    host.profile_by_uuid(uuid).map(|p| p.name)
}

/// A player by account name, any case: online, else the user cache.
fn profile_by_name(host: &dyn ServerHost, name: &str) -> Option<(u128, String)> {
    if let Some(player) = host.players().player_by_name(name)
        && let Some(n) = player.name(host)
    {
        return Some((player.uuid(), n));
    }
    host.profile_by_name(name).map(|p| (p.uuid, p.name))
}

/// Names for `/npc owner`: the owners of all NPCs (UUID -> name) and the typed word as a name.
fn owner_profiles(host: &dyn ServerHost, line: &str) -> Vec<(u128, String)> {
    let mut words = line.split_whitespace();
    if (words.next(), words.next()) != (Some("npc"), Some("owner")) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for o in api::with_world(|w| {
        w.saves
            .npcs
            .iter()
            .filter_map(|n| n.owner())
            .collect::<Vec<_>>()
    })
    .unwrap_or_default()
    {
        if let Some(name) = profile_by_uuid(host, o) {
            out.push((o, name));
        }
    }
    if let Some(word) = words.next()
        && let Some((uuid, name)) = profile_by_name(host, word)
    {
        out.push((uuid, name));
    }
    out
}

/// Citizens' markup line as a chat line, its colour codes read by the host (`text_from_legacy`). A line
/// the host refuses keeps its words.
fn plain(host: &dyn ServerHost, markup: &str) -> Text {
    let coded = core::text::to_section(markup);
    match host.text_from_legacy(&coded) {
        Ok(text) => text,
        Err(_) => {
            let mut out = String::new();
            let mut chars = coded.chars();
            while let Some(c) = chars.next() {
                if c == '§' {
                    chars.next();
                } else {
                    out.push(c);
                }
            }
            Text::literal(out)
        }
    }
}

fn reply(host: &mut dyn ServerHost, lines: Vec<String>) {
    for line in lines {
        let text = plain(host, &line);
        let _ = host.commands().feedback(text);
    }
}

/// A blocking GET with a short timeout; anything but 200 is an error (Mojang answers 204 / 404
/// for an unknown name).
/// A skin job off the server thread: mineskin's generator, or the export of a skin PNG.
fn run_skin_work(work: core::skin::Work) -> Result<Option<core::skin::Generated>, String> {
    use std::io::Read;
    match work {
        core::skin::Work::Export { url, path } => {
            let resp = ureq::get(&url).timeout(std::time::Duration::from_secs(10)).call().map_err(|e| e.to_string())?;
            let mut bytes = Vec::new();
            resp.into_reader().take(10_000).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
            if let Some(dir) = Path::new(&path).parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
            Ok(None)
        }
        core::skin::Work::Generate { source, png, slim } => {
            let resp = match png {
                Some(png) => {
                    let mut body = Vec::new();
                    body.extend_from_slice(b"--*****\r\nContent-Disposition: form-data; name=\"file\"; filename=\"skin.png\"\r\nContent-Type: image/png\r\n\r\n");
                    body.extend_from_slice(&png);
                    body.extend_from_slice(b"\r\n--*****\r\nContent-Disposition: form-data; name=\"name\";\r\n\r\n\r\n");
                    if slim {
                        body.extend_from_slice(b"--*****\r\nContent-Disposition: form-data; name=\"variant\";\r\n\r\nslim\r\n");
                    }
                    body.extend_from_slice(b"--*****--\r\n");
                    let target = if slim { "https://api.mineskin.org/generate/upload?model=slim" } else { "https://api.mineskin.org/generate/upload" };
                    ureq::post(target)
                        .set("User-Agent", "Citizens/2.0")
                        .set("Content-Type", "multipart/form-data;boundary=*****")
                        .timeout(std::time::Duration::from_secs(30))
                        .send_bytes(&body)
                }
                None => {
                    let mut req = serde_json::json!({ "url": source, "name": "" });
                    if slim {
                        req["variant"] = serde_json::json!("slim");
                    }
                    ureq::post("https://api.mineskin.org/generate/url")
                        .set("User-Agent", "Citizens/2.0")
                        .set("Accept", "application/json")
                        .set("Content-Type", "application/json")
                        .timeout(std::time::Duration::from_secs(30))
                        .send_string(&req.to_string())
                }
            }
            .map_err(|e| e.to_string())?;
            if resp.status() != 200 {
                return Err(format!("HTTP {}", resp.status()));
            }
            let body = resp.into_string().map_err(|e| e.to_string())?;
            core::skin::parse_mineskin(&body).map(Some).ok_or_else(|| "bad answer".to_owned())
        }
    }
}

fn http_get(url: &str) -> Result<String, String> {
    let resp = ureq::get(url)
        .timeout(std::time::Duration::from_secs(10))
        .call()
        .map_err(|e| e.to_string())?;
    if resp.status() != 200 {
        return Err(format!("HTTP {}", resp.status()));
    }
    resp.into_string().map_err(|e| e.to_string())
}

impl Citizens {
    fn load_config(&self, server: Option<&Path>, text: &str, what: &str) {
        match core::config::Config::parse(text) {
            Ok(toml) => {
                let cfg = effective_config(server, &toml);
                {
                    let mut st = lock(&self.state);
                    st.toml = toml;
                    st.cfg = cfg.clone();
                }
                api::with_world(|w| w.cfg = cfg);
                eprintln!("[citizens] config {what}");
            }
            Err(e) => eprintln!("[citizens] config not {what}, keeping the old one: {e}"),
        }
    }

    /// Reads saves.yml once the world is open.
    fn install(&self, host: &dyn ServerHost) {
        let mut st = lock(&self.state);
        if st.installed {
            return;
        }
        st.cfg = effective_config(server_of(host).as_deref(), &st.toml);
        let Some(path) = saves_path(host, &st.cfg) else {
            return;
        };
        st.installed = true;
        let saves = match core::saves::Saves::load(&path) {
            Ok(s) => s,
            Err(e) => {
                // Never overwrite a file that could not be read.
                eprintln!(
                    "[citizens] could not read {}: {e}; NPCs are off until it is fixed",
                    path.display()
                );
                return;
            }
        };
        eprintln!(
            "[citizens] {} NPCs from {}",
            saves.npcs.len(),
            path.display()
        );
        core::expr::set_papi(papi_text);
        api::install(core::world::World::new(st.cfg.clone(), saves, Some(path)));
    }

    /// Runs the queued game work and hands the events to other mods.
    fn run_work(&self, host: &mut dyn ServerHost) {
        let (ops, events) = api::take_work();
        let tx = lock(&self.state).skins_tx.clone();
        let skins = lock(&self.state).cfg.skins.clone();
        for op in ops {
            match op {
                core::world::Op::Console(line) => {
                    // A `kill <uuid>` before a summon finds nothing unless a copy was saved, and `team add` of
                    // a team that exists (every spawn sends it) reports failure: neither is an error.
                    if !host.commands().run_command(&line)
                        && !line.starts_with("kill ")
                        && !line.starts_with("team add ")
                    {
                        eprintln!("[citizens] command failed: {line}");
                    }
                }
                core::world::Op::RemoveMob {
                    dimension: _,
                    entity,
                } => {
                    // The level is not checked against the NPC's dimension (as in the Horizon port);
                    // entity ids are unique on the server, so the right mob is removed.
                    if let Some(e) = host.entity(entity) {
                        let _ = e.remove(host, RemovalReason::Discarded);
                    }
                }
                core::world::Op::Rotate {
                    entity,
                    yaw,
                    pitch,
                    fallback,
                    ..
                } => {
                    // Body and head, as `Host::set_entity_rotation` did; the console `rotate`
                    // command is the fallback when the entity is gone.
                    let turned = match host.entity(entity) {
                        Some(e) => {
                            let turned = e.set_rotation(host, yaw, pitch);
                            let _ = e.set_head_yaw(host, yaw);
                            turned
                        }
                        None => false,
                    };
                    if !turned {
                        let _ = host.commands().run_command(&fallback);
                    }
                }
                core::world::Op::RotateHead { entity, body_yaw, yaw, pitch } => {
                    if let Some(e) = host.entity(entity) {
                        let _ = e.set_rotation(host, body_yaw, pitch);
                        let _ = e.set_head_yaw(host, yaw);
                    }
                }
                core::world::Op::Charge { player, levels, slots, money } => {
                    charge(host, player, levels, &slots);
                    if money > 0.0 {
                        money_move(&*host, player, -money);
                    }
                }
                core::world::Op::OpenMenu { player, title, rows, items, in_place } => {
                    let stacks: Vec<Option<ItemStack>> =
                        items.iter().map(|i| i.as_ref().and_then(|s| item_stack_of(&*host, s))).collect();
                    let title = plain(&*host, &title);
                    let replace = if in_place { MenuReplace::InPlace } else { MenuReplace::CloseFirst };
                    let window = host.open_menu(player, rows, title, stacks, replace).ok().flatten();
                    api::with_world(|w| w.menu_opened(player, window));
                }
                core::world::Op::OpenHopper { player, title, items } => {
                    let stacks: Vec<Option<ItemStack>> =
                        items.iter().map(|i| i.as_ref().and_then(|s| item_stack_of(&*host, s))).collect();
                    let title = plain(&*host, &title);
                    let window = host.open_hopper_menu(player, title, stacks, MenuReplace::CloseFirst).ok().flatten();
                    api::with_world(|w| w.menu_opened(player, window));
                }
                core::world::Op::CloseMenu { player } => host.close_menu(player),
                core::world::Op::ShopApply { player, levels, slots, money } => {
                    shop_apply(host, player, levels, &slots);
                    if money != 0.0 {
                        money_move(&*host, player, money);
                    }
                }
                core::world::Op::SetMenuItems { player, window, items } => {
                    let stacks: Vec<Option<ItemStack>> =
                        items.iter().map(|i| i.as_ref().and_then(|s| item_stack_of(&*host, s))).collect();
                    let _ = host.menu_set_items(player, window, stacks);
                }
                core::world::Op::Entity { entity, set } => apply_entity_set(host, entity, set),
                core::world::Op::EntityCommand { entity, line, level } => {
                    if let Some(e) = host.entity(entity)
                        && !e.run_command(host, &line, PermissionLevel::from_level(i32::from(level)))
                    {
                        eprintln!("[citizens] command as NPC failed: {line}");
                    }
                }
                core::world::Op::Camera { player, entity } => {
                    if let Some(p) = host.player(player) {
                        let target = entity.and_then(|e| host.entity(e));
                        if entity.is_none() || target.is_some() {
                            let _ = p.set_camera_entity(host, target);
                        }
                    }
                }
                core::world::Op::OpenMerchant { player, title, trades } => {
                    let window = open_merchant(host, player, &title, &trades);
                    api::with_world(|w| w.merchant_opened(player, window));
                }
                core::world::Op::Jump { entity } => {
                    if let Some(mob) = host.entity(entity).and_then(|e| host.mob(e)) {
                        let _ = mob.jump(host);
                    }
                }
                core::world::Op::Ride { player, rider, vehicle } => {
                    let rider = match player {
                        Some(uuid) => host.player(uuid).map(|p| p.entity()),
                        None => host.entity(rider),
                    };
                    if let (Some(rider), Some(vehicle)) = (rider, host.entity(vehicle)) {
                        let _ = rider.start_riding(host, vehicle, false);
                    }
                }
                core::world::Op::Dismount { player, entity } => {
                    let rider = match player {
                        Some(uuid) => host.player(uuid).map(|p| p.entity()),
                        None => host.entity(entity),
                    };
                    if let Some(rider) = rider {
                        let _ = rider.stop_riding(host);
                    }
                }
                core::world::Op::Tell { player, legacy } => {
                    if let Some(p) = host.player(player) {
                        let text = plain(host, &legacy);
                        let _ = p.tell(host, text);
                    }
                }
                core::world::Op::SkinWork { npc, by, label, work } => {
                    let tx = lock(&self.state).work_tx.clone();
                    std::thread::spawn(move || {
                        let export = matches!(work, core::skin::Work::Export { .. });
                        let result = run_skin_work(work);
                        let _ = tx.send(core::skin::Done { npc, by, label, export, result });
                    });
                }
                core::world::Op::FetchSkin { npc, name } => {
                    let tx = tx.clone();
                    let skins = skins.clone();
                    std::thread::spawn(move || {
                        let result = core::skin::fetch(&skins, &name, http_get);
                        let _ = tx.send(core::skin::Fetched {
                            npc,
                            requested: name,
                            result,
                        });
                    });
                }
            }
        }
        api::deliver(&events);
    }
}

#[mod_api::horizon_mod]
impl Hooks for Citizens {
    fn register_configs(&self, _host: HostSide, event: &mut RegisterConfigsEvent<'_>) {
        event.configs.push(Config::new(
            MOD_ID,
            ConfigKind::Common,
            core::config::DEFAULT_TOML,
        ));
    }

    fn config_loading(&self, host: HostSide, event: &mut ConfigLoadingEvent<'_>) {
        if event.config.mod_id == MOD_ID {
            self.load_config(
                server_of_side(&host).as_deref(),
                &event.config.text,
                "loaded",
            );
        }
    }

    fn config_reloading(&self, host: HostSide, event: &mut ConfigReloadingEvent<'_>) {
        if event.config.mod_id == MOD_ID {
            self.load_config(
                server_of_side(&host).as_deref(),
                &event.config.text,
                "reloaded",
            );
        }
    }

    fn register_commands(&self, _host: &mut dyn ServerHost, event: &mut RegisterCommandsEvent<'_>) {
        // The bare roots (`/npc`, `/citizens`, `/trait`) have no node: a node there would gate every
        // sub-command under it. They stay open and are registered here; the sub-commands go to
        // `register_node_commands`.
        let level = lock(&self.state).cfg.command_level;
        for usage in core::commands::usages() {
            if !usage.contains(' ') {
                event.commands.push(Command::op(&usage, level));
            }
        }
    }

    fn register_node_commands(&self, _host: &mut dyn ServerHost, event: &mut RegisterNodeCommandsEvent<'_>) {
        let level = lock(&self.state).cfg.command_level;
        for usage in core::commands::usages() {
            if !usage.contains(' ') {
                continue;
            }
            // Each sub-command path carries Citizens' node (`citizens.npc.create`, `node_of_usage`), so a
            // permissions mod can hand one to a non-operator; `level` decides when none answers.
            // The help nodes are `default: op` in plugin.yml, like every other node: no usage is level 0.
            let node = core::commands::node_of_usage(&usage);
            event.commands.push(Command::op(&usage, level).with_node(&perm_node(node)));
        }
    }

    fn permission_gather_nodes(&self, event: &mut PermissionGatherNodesEvent<'_>) {
        // Every node the commands ask, wildcard parents (`citizens.npc.*`) included (a node whose name ends
        // in `.*` is an ordinary node), at the configured `command-level` (the level the command gate uses,
        // so both agree; 2 when gathering runs before the config is read).
        let level = i32::from(lock(&self.state).cfg.command_level);
        let mut all: Vec<String> = core::commands::all_nodes()
            .into_iter()
            .map(str::to_owned)
            .collect();
        all.extend(core::commands::create_type_nodes());
        for node in all {
            let _ = event.nodes.add(perm_node(&node), NodeDefault::Level(PermissionLevel::from_level(level)));
        }
    }

    /// A player without a sub-command's node is refused with Citizens' own line, not "unknown command": the
    /// usage is hidden from them (`register_commands`). Help needs its node too (plugin.yml default: op).
    fn player_command(&self, host: &mut dyn ServerHost, event: &mut PlayerCommandEvent<'_>) -> Flow {
        let words: Vec<String> = event
            .command
            .trim_start_matches('/')
            .split_whitespace()
            .map(str::to_ascii_lowercase)
            .collect();
        let root = words.first().map(String::as_str).unwrap_or("");
        if !core::commands::is_root(root) {
            return Flow::Continue;
        }
        let sub = if root == "trait" { "" } else { words.get(1).map(String::as_str).unwrap_or("") };
        // A word that is no sub-command of ours is left to the engine ("unknown argument", as before).
        if !sub.is_empty()
            && root != "trait"
            && !core::commands::usages()
                .iter()
                .any(|u| u.split_whitespace().nth(1) == Some(sub))
        {
            return Flow::Continue;
        }
        let node = core::commands::node_for(root, sub);
        let allowed = host.player(event.player).is_some_and(|p| holds(host, p, node));
        if allowed {
            return Flow::Continue;
        }
        event.cancelled = true;
        event.replies.push(Text::literal(
            "\u{a7}cYou don't have permission to execute that command.",
        ));
        Flow::Cancel
    }

    /// Tab list: the `npc`, `citizens` and `trait` roots leave the tree of a player who may run none of their
    /// usages (as Bukkit hides a command whose node the sender lacks). A player holding ONE node (or a
    /// parent such as `citizens.npc.*`) keeps the root; the engine hides the sub-commands whose own node
    /// they lack (`with_node` paths).
    fn player_commands_sent(&self, host: &mut dyn ServerHost, event: &mut PlayerCommandsSentEvent<'_>) {
        let level = lock(&self.state).cfg.command_level;
        let usages = core::commands::usages();
        let mut hide: Vec<&str> = Vec::new();
        for root in ["npc", "citizens", "trait", "template", "tpl", "waypoints", "wp"] {
            let allowed = match host.player(event.player) {
                Some(p) => may_any(host, p, root, level, &usages),
                None => false,
            };
            if !allowed {
                hide.push(root);
            }
        }
        event.names.retain(|n| !hide.contains(&n.to_ascii_lowercase().as_str()));
    }

    /// `/npc`, `/citizens`, `/trait`, `/template` and `/waypoints`, typed by a player or the console (their Citizens lines).
    fn run_mod_command(
        &self,
        host: &mut dyn ServerHost,
        event: &mut RunModCommandEvent<'_>,
    ) -> Flow {
        let root = event
            .command
            .literals
            .first()
            .map(String::as_str)
            .unwrap_or("");
        if !core::commands::is_root(root) {
            return Flow::Continue;
        }
        self.install(&*host);
        let cfg = lock(&self.state).cfg.clone();
        let line = command_line(&*host, event.command);
        let console = matches!(event.source, CommandSource::Console { .. });
        let sender = match event.source {
            CommandSource::Player { player, .. } => {
                let Some(info) = player_info(&*host, *player) else {
                    return Flow::Cancel;
                };
                let mut sender = sender_of(&info, &cfg);
                sender.grants = granted_nodes(
                    host,
                    *player,
                    info.view.permission,
                    cfg.command_level,
                );
                if holds(host, *player, "citizens.npc.ignore-cost") {
                    sender.grants.push("citizens.npc.ignore-cost".to_owned());
                }
                sender
            }
            CommandSource::Console { permission } => bodiless_sender("Console", *permission),
            other => bodiless_sender("Command block", other.permission()),
        };
        // `setequipment <slot> hand` copies the stack in the sender's hand.
        if line.split_whitespace().nth(1).is_some_and(|w| w.eq_ignore_ascii_case("setequipment") || w.eq_ignore_ascii_case("debug")) {
            let stack = match event.source {
                CommandSource::Player { player, .. } => {
                    host.living(player.entity()).and_then(|l| l.hand_item(&*host, Hand::Main))
                }
                _ => None,
            };
            let hand = stack.as_ref().map(|st| stack_data_of(&*host, st));
            let snbt = stack.as_ref().and_then(|st| host.item_to_snbt(st));
            api::with_world(|w| {
                w.hand_item = hand;
                w.hand_snbt = snbt;
            });
        }
        let profiles = owner_profiles(&*host, &line);
        let path = saves_path(&*host, &cfg);
        // The shop commands read the sender (and a named player) before they open a menu.
        if matches!(line.split_whitespace().nth(1), Some("shop" | "showshop" | "shopitem")) {
            let mut who: Vec<u128> = sender.uuid.into_iter().collect();
            for word in line.split_whitespace().skip(2) {
                if let Some(p) = host.players().player_by_name(word) {
                    who.push(p.uuid());
                }
            }
            for uuid in who {
                prime_shop(host, uuid, &cfg);
            }
        }
        // `/npc command execute <player> <hand>` charges the named player: their balance, levels, stacks and nodes.
        let words: Vec<&str> = line.split_whitespace().collect();
        if matches!(words.get(1), Some(&"command" | &"cmd")) && words.get(2).is_some_and(|w| w.eq_ignore_ascii_case("execute")) {
            let named = words.get(3).and_then(|word| {
                core::ids::parse_uuid(word)
                    .and_then(|u| host.player(u))
                    .or_else(|| host.players().player_by_name(word))
            });
            if let Some(p) = named {
                let uuid = p.uuid();
                let selected = api::with_world(|w| w.selected(sender.uuid.unwrap_or(u128::MAX))).flatten();
                let wanted = selected.and_then(|id| api::with_world(|w| w.command_perms_of(id))).unwrap_or_default();
                let granted: std::collections::BTreeSet<String> = wanted.into_iter().filter(|n| holds(host, p, n)).collect();
                let cost = cost_state(&*host, p, uuid);
                api::with_world(|w| {
                    w.held_perms = granted;
                    w.player_cost = Some(cost);
                });
            }
        }
        let lines = api::with_world(|w| {
            w.profiles.extend(profiles);
            core::commands::run(w, &sender, &line, || match &path {
                Some(p) => core::saves::Saves::load(p),
                None => Err("no world open".into()),
            })
        })
        .unwrap_or_else(|| {
            vec![
                cfg.messages
                    .error("NPCs are not loaded (see the server log)."),
            ]
        });
        reply(host, lines);
        // Console commands that change the world run now: the console is outside every hook.
        if console {
            self.run_work(host);
        }
        Flow::Cancel
    }

    /// A claimed click is `Consume` (no client swing, the entity's and the item's own use do not run), where
    /// `Flow::Cancel` was. Falling through is `None`.
    fn entity_interact(
        &self,
        host: HostSide<'_>,
        event: &mut EntityInteractEvent<'_>,
    ) -> Option<mod_api::InteractionResult> {
        let HostSide::Server(host) = host else {
            return None;
        };
        let Ok(entity) = u32::try_from(event.entity) else {
            return None;
        };
        if event.hand == Hand::Off {
            // Asked after the main hand of the same click: refused when the main hand claimed it.
            let claimed = lock(&self.state).last_claim == Some(event.entity);
            return if claimed {
                Some(mod_api::InteractionResult::Consume)
            } else {
                None
            };
        }
        let Some(player) = host.players().acting_player() else {
            return None;
        };
        let Some(info) = player_info(&*host, player) else {
            return None;
        };
        let held = held_item_id(&*host, player);
        let wanted = api::with_world(|w| w.command_perms(entity)).unwrap_or_default();
        let granted: std::collections::BTreeSet<String> = wanted.into_iter().filter(|n| holds(host, player, n)).collect();
        let cost = cost_state(&*host, player, info.view.uuid);
        if api::with_world(|w| w.has_shops()).unwrap_or(false) {
            let cfg = lock(&self.state).cfg.clone();
            prime_shop(host, player.uuid(), &cfg);
        }
        let claimed = api::with_world(|w| {
            w.held_perms = granted;
            w.player_cost = Some(cost);
            w.on_click(entity, &info.view, core::npc::Hand::Right, held.as_deref())
        })
        .unwrap_or(false);
        lock(&self.state).last_claim = claimed.then_some(event.entity);
        // Claimed: the hand swings, the entity's and the item's own use do not run.
        if claimed {
            Some(mod_api::InteractionResult::Consume)
        } else {
            None
        }
    }

    fn attack_entity(&self, host: &mut dyn ServerHost, event: &mut AttackEntityEvent<'_>) -> Flow {
        let Ok(entity) = u32::try_from(event.entity) else {
            return Flow::Continue;
        };
        let Some(player) = host.players().acting_player() else {
            return Flow::Continue;
        };
        let Some(info) = player_info(&*host, player) else {
            return Flow::Continue;
        };
        let wanted = api::with_world(|w| w.command_perms(entity)).unwrap_or_default();
        let granted: std::collections::BTreeSet<String> = wanted.into_iter().filter(|n| holds(host, player, n)).collect();
        let cost = cost_state(&*host, player, info.view.uuid);
        let refused = api::with_world(|w| {
            w.held_perms = granted;
            w.player_cost = Some(cost);
            w.on_click(entity, &info.view, core::npc::Hand::Left, None) && w.hit_refused(entity)
        })
        .unwrap_or(false);
        if refused {
            Flow::Cancel
        } else {
            Flow::Continue
        }
    }

    fn living_incoming_damage(
        &self,
        _level: &mut dyn LevelRandomHost,
        event: &mut LivingIncomingDamageEvent<'_>,
    ) -> Flow {
        // Before any reduction, the amount is the original damage.
        let amount = event.container.original();
        let source = event.source;
        match api::on_damage(
            event.entity,
            source.damage_type,
            source.causing,
            source.direct,
            amount,
        ) {
            api::DamageDecision::Pass => Flow::Continue,
            api::DamageDecision::Cancel => Flow::Cancel,
            api::DamageDecision::Amount(a) => {
                event.container.set_new_damage(a);
                Flow::Continue
            }
        }
    }

    fn entity_join_level(
        &self,
        host: &mut dyn ServerHost,
        event: &mut EntityJoinLevelEvent<'_>,
    ) -> Flow {
        if !npc_kind(event.kind) {
            return Flow::Continue;
        }
        let Some(e) = host.entity(event.entity) else {
            return Flow::Continue;
        };
        let uuid = e.uuid(host);
        let tagged = e
            .tags(&*host)
            .is_some_and(|t| t.iter().any(|t| t == core::spawn::TAG));
        let cancel =
            api::with_world(|w| w.on_entity_join(event.dimension, event.entity, uuid, tagged))
                .unwrap_or(false);
        if cancel { Flow::Cancel } else { Flow::Continue }
    }

    fn entity_leave_level(
        &self,
        _host: &mut dyn ServerHost,
        event: &mut EntityLeaveLevelEvent<'_>,
    ) {
        api::with_world(|w| w.on_entity_leave(event.entity));
    }

    fn server_tick(&self, host: &mut dyn ServerHost, event: &mut ServerTickEvent<'_>) {
        if !matches!(
            event.phase,
            ServerTickPhase::ServerTickStart {
                world_running: true,
                ..
            }
        ) {
            return;
        }
        self.install(&*host);
        let fetched: Vec<core::skin::Fetched> = lock(&self.state).skins_rx.try_iter().collect();
        let works: Vec<core::skin::Done> = lock(&self.state).work_rx.try_iter().collect();
        let players: Vec<core::look::PlayerView> = host
            .players()
            .server_players()
            .into_iter()
            .filter_map(|p| player_info(&*host, p))
            .map(|info| info.view)
            .collect();
        let rotations: Vec<(u128, [f32; 2])> = host
            .players()
            .server_players()
            .into_iter()
            .filter_map(|p| player_info(&*host, p).map(|i| (i.view.uuid, i.rot)))
            .collect();
        // Where the path-following NPCs stand and whether their paths are finished.
        let followers = api::with_world(|w| w.real_entities()).unwrap_or_default();
        let reports: Vec<(i32, [f64; 3], bool)> = followers
            .into_iter()
            .filter_map(|(id, e)| {
                let ent = host.entity(e)?;
                let pos = ent.position(&*host)?;
                let done = host.mob(ent).and_then(|m| m.is_done(&*host)).unwrap_or(true);
                Some((id, pos, done))
            })
            .collect();
        // `lookclose -r`: which players each realistic NPC sees; `--filter` clauses: the nodes each player holds.
        let wanted = api::with_world(|w| (w.sight_wanted(), w.look_filter_nodes())).unwrap_or_default();
        let mut sight: Vec<((i32, u128), bool)> = Vec::new();
        let mut held: Vec<(u128, std::collections::BTreeSet<String>)> = Vec::new();
        if !wanted.0.is_empty() || !wanted.1.is_empty() {
            for p in host.players().server_players() {
                let Some(info) = player_info(&*host, p) else { continue };
                if let Some(level) = p.level(&*host) {
                    for (id, eye, dim) in &wanted.0 {
                        if *dim == info.view.dimension {
                            let to = core::look::eye(info.view.pos);
                            sight.push(((*id, info.view.uuid), level.line_of_sight(&*host, *eye, to).unwrap_or(true)));
                        }
                    }
                }
                let nodes: std::collections::BTreeSet<String> =
                    wanted.1.iter().filter(|n| holds(host, p, n)).cloned().collect();
                held.push((info.view.uuid, nodes));
            }
        }
        // `follow -p`: who hurt a followed entity, and where the foes being chased stand.
        let hits: Vec<(u32, Option<(u128, u32, [f64; 3])>)> = api::take_protect_hits()
            .into_iter()
            .map(|(victim, causing)| {
                let foe = u32::try_from(causing).ok().and_then(|c| {
                    let e = host.entity(c)?;
                    let u = e.uuid(host)?;
                    Some((u, c, e.position(&*host)?))
                });
                (victim, foe)
            })
            .collect();
        let foes: Vec<(u128, Option<[f64; 3]>)> = api::with_world(|w| w.foe_entities())
            .unwrap_or_default()
            .into_iter()
            .map(|(u, e)| (u, host.entity(e).and_then(|x| x.position(&*host))))
            .collect();
        PAPI_ON.store(papi_installed(&*host), std::sync::atomic::Ordering::Relaxed);
        let economy = economy_on(&*host);
        // Health and motion of the NPCs that run a behavior tree (their expression scope).
        let tracked = api::with_world(|w| w.behavior_entities()).unwrap_or_default();
        let stats: Vec<(i32, core::world::LiveStats)> = tracked
            .into_iter()
            .filter_map(|(id, e)| {
                let ent = host.entity(e)?;
                Some((
                    id,
                    core::world::LiveStats {
                        health: host.living(ent).and_then(|l| l.health(&*host)).map_or(0.0, f64::from),
                        max_health: host.living(ent).and_then(|l| l.max_health(&*host)).map_or(0.0, f64::from),
                        velocity: ent.velocity(&*host).unwrap_or([0.0; 3]),
                    },
                ))
            })
            .collect();
        api::with_world(|w| {
            w.economy = economy;
            w.money_fmt = economy.then_some(format_money as fn(f64) -> String);
            w.create_charge = economy.then_some(create_charge as fn(u128, f64) -> Result<String, String>);
            for (id, st) in stats {
                w.set_live_stats(id, st);
            }
            for (id, pos, done) in reports {
                w.nav_report(id, pos, done);
            }
            w.rotations = rotations.into_iter().collect();
            w.sight = sight.into_iter().collect();
            for (victim, foe) in hits {
                w.protect_hit(victim, foe);
            }
            for (u, pos) in foes {
                w.foe_report(u, pos);
            }
            w.player_perms = held.into_iter().collect();
            for f in fetched {
                w.skin_fetched(f);
            }
            for d in works {
                w.skin_work_done(d);
            }
            api::flush_damage(w);
            w.tick(&players);
        });
        self.run_work(host);
        api::refresh_snapshot();
    }

    fn menu_clicked(&self, host: &mut dyn ServerHost, event: &mut MenuClickedEvent) {
        let (uuid, window) = (event.uuid, event.window);
        if api::with_world(|w| w.shop_session_open(uuid)).unwrap_or(false) {
            let cfg = lock(&self.state).cfg.clone();
            prime_shop(host, uuid, &cfg);
        }
        let Some(action) = api::with_world(|w| w.menu_click(uuid, window, event.slot, event.button, event.mode == 1)) else {
            return;
        };
        let Some(p) = host.player(uuid) else { return };
        match action {
            core::world::MenuAction::Nothing => {}
            core::world::MenuAction::Take(stack) => {
                if let Some(s) = item_stack_of(&*host, &stack) {
                    let _ = p.give_stack(host, s);
                }
            }
            core::world::MenuAction::Read { inv_slot } => {
                let held = p.inventory_slots(&*host).unwrap_or_default();
                if let Some((_, st)) = held.iter().find(|(i, _)| *i == inv_slot) {
                    let data = stack_data_of(&*host, st);
                    api::with_world(|w| w.menu_read(uuid, data));
                }
            }
            core::world::MenuAction::Deposit { inv_slot } => {
                let held = p.inventory_slots(&*host).unwrap_or_default();
                if let Some((_, st)) = held.iter().find(|(i, _)| *i == inv_slot) {
                    let data = stack_data_of(&*host, st);
                    if api::with_world(|w| w.menu_deposit(uuid, data)).unwrap_or(false) {
                        // A hopper slot takes one; the rest of the stack stays.
                        let left = api::with_world(|w| w.take_deposit_left()).unwrap_or(0);
                        let mut rest = st.clone();
                        let keep = (left > 0 && rest.set_count(left.clamp(1, 99) as u32).is_ok()).then_some(rest);
                        let _ = p.set_inventory_slot(host, inv_slot, keep);
                    }
                }
            }
        }
        self.run_work(host);
    }

    fn right_click_block(&self, _host: HostSide, event: &mut mod_api::RightClickBlockEvent<'_>) -> Flow {
        let (player, pos) = (event.click.player, event.click.pos);
        let ours = api::with_world(|w| core::commands::copier_click(w, player, pos) || core::commands::path_click(w, player, pos, false))
            .unwrap_or(false);
        if ours { Flow::Cancel } else { Flow::Continue }
    }

    fn left_click_block(&self, host: &mut dyn ServerHost, event: &mut mod_api::LeftClickBlockEvent) -> Flow {
        let Some(player) = host.players().acting_player() else { return Flow::Continue };
        let (uuid, pos) = (player.uuid(), event.pos);
        if api::with_world(|w| core::commands::path_click(w, uuid, pos, true)).unwrap_or(false) {
            Flow::Cancel
        } else {
            Flow::Continue
        }
    }

    /// A shop editor's text prompt takes the player's next chat line (kept out of public chat). What it asks
    /// the game for runs with the next tick.
    fn server_chat(&self, _: &mut dyn ServerHost, event: &mut ServerChatEvent<'_>) -> Flow {
        let taken = api::with_world(|w| {
            w.shop_chat(event.player, event.raw_text) || core::commands::chat_edit(w, event.player, event.raw_text)
        })
        .unwrap_or(false);
        if taken { Flow::Cancel } else { Flow::Continue }
    }

    fn merchant_trade_selected(&self, _: &mut dyn ServerHost, event: &mut MerchantTradeSelectedEvent) {
        let (player, window, index) = (event.player, event.window, event.index);
        api::with_world(|w| w.merchant_selected(player, window, index));
    }

    fn menu_closed(&self, _: &mut dyn ServerHost, event: &mut MenuClosedEvent) {
        let (uuid, window) = (event.uuid, event.window);
        api::with_world(|w| w.menu_closed(uuid, window));
    }

    fn server_stopping(&self, host: &mut dyn ServerHost, _event: &mut ServerStoppingEvent) {
        // NPC entities are taken out before the worlds are saved, so no chunk keeps a copy;
        // saves.yml is written last.
        api::with_world(core::world::World::stop);
        self.run_work(host);
        if let Some(mut w) = api::uninstall()
            && let Err(e) = w.save_now()
        {
            eprintln!("[citizens] could not save NPCs: {e}");
        }
        lock(&self.state).installed = false;
    }
}
