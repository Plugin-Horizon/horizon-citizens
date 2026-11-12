//! What other mods use: the NPC registry, their events (clicks, spawns, damage, deaths) and the
//! calls that move, turn, dress and change NPCs. A Sentinel-like mod builds on these.
//!
//! Every call goes through one process-wide [`World`] behind a lock; events are delivered by the
//! glue once a tick, outside that lock, so a listener may call back into this API.
//! Calls made before the server opens its world (or after it stopped) answer `None`/`false`.

use crate::npc::{Location, NpcData, Skin};
use crate::world::{NpcEvent, Op, World};
use crate::tree::TreeExt;
use crate::yaml::Node;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard, RwLock};

pub use crate::world::DespawnReason;

static WORLD: Mutex<Option<World>> = Mutex::new(None);

type Listener = Arc<dyn Fn(&NpcEvent) + Send + Sync>;
static LISTENERS: RwLock<Vec<Listener>> = RwLock::new(Vec::new());

/// A hit on an NPC, asked synchronously from the game's damage hook.
#[derive(Clone, Debug, PartialEq)]
pub struct DamageQuery {
    pub npc: i32,
    pub entity: u32,
    pub damage_type: String,
    /// The entity behind the damage (`DamageSource.getEntity`), 0 for none.
    pub causing: i32,
    /// The direct entity (an arrow), 0 for none.
    pub direct: i32,
    pub amount: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DamageDecision {
    /// Leave it to the next filter (and finally to the game).
    Pass,
    Cancel,
    /// Deal this much instead.
    Amount(f32),
}

type DamageFilter = Arc<dyn Fn(&DamageQuery) -> DamageDecision + Send + Sync>;
static DAMAGE_FILTERS: RwLock<Vec<DamageFilter>> = RwLock::new(Vec::new());

/// What the damage hook reads without the world's lock: NPC entities and the protected ones.
#[derive(Default)]
struct Snapshot {
    npcs: HashMap<u32, i32>,
    protected: HashSet<u32>,
}
static SNAPSHOT: RwLock<Option<Snapshot>> = RwLock::new(None);
/// Damage seen by the hook, handed to the world at the next tick: (entity, amount, type, causing).
static DAMAGE_SEEN: Mutex<Vec<(u32, f32, String, i32)>> = Mutex::new(Vec::new());

fn world_lock() -> MutexGuard<'static, Option<World>> {
    WORLD.lock().unwrap_or_else(|e| e.into_inner())
}

/// Runs `f` on the world, when there is one. Do not call the game from inside `f`.
pub fn with_world<R>(f: impl FnOnce(&mut World) -> R) -> Option<R> {
    world_lock().as_mut().map(f)
}

// ---- for the glue ---------------------------------------------------------------------------

/// The world opened: the NPCs of `world` are now the registry.
pub fn install(world: World) {
    *world_lock() = Some(world);
    refresh_snapshot();
}

/// The server stopped: the registry is gone (saved first by the glue).
pub fn uninstall() -> Option<World> {
    *SNAPSHOT.write().unwrap_or_else(|e| e.into_inner()) = None;
    world_lock().take()
}

/// Rebuilds the damage hook's view (the glue calls it every tick).
pub fn refresh_snapshot() {
    let snap = with_world(|w| Snapshot {
        npcs: w
            .npc_entities()
            .into_iter()
            .filter_map(|e| Some((e, w.npc_of_entity(e)?)))
            .collect(),
        protected: w.protected_entities().into_iter().collect(),
    });
    *SNAPSHOT.write().unwrap_or_else(|e| e.into_inner()) = snap;
}

/// From the game's damage hook (no host, mid-hit; never waits for the world's lock): whether to
/// cancel the damage or change its amount. Protected NPCs take none; other NPCs ask the filters.
pub fn on_damage(entity: i32, damage_type: &str, causing: i32, direct: i32, amount: f32) -> DamageDecision {
    let Ok(entity) = u32::try_from(entity) else {
        return DamageDecision::Pass;
    };
    let npc = {
        let snap = SNAPSHOT.read().unwrap_or_else(|e| e.into_inner());
        let Some(snap) = snap.as_ref() else {
            return DamageDecision::Pass;
        };
        let Some(npc) = snap.npcs.get(&entity).copied() else {
            return DamageDecision::Pass;
        };
        if snap.protected.contains(&entity) {
            return DamageDecision::Cancel;
        }
        npc
    };
    let query = DamageQuery {
        npc,
        entity,
        damage_type: damage_type.to_owned(),
        causing,
        direct,
        amount,
    };
    let filters: Vec<DamageFilter> = DAMAGE_FILTERS.read().unwrap_or_else(|e| e.into_inner()).clone();
    let mut decision = DamageDecision::Pass;
    for f in filters {
        match f(&query) {
            DamageDecision::Pass => {}
            d => {
                decision = d;
                break;
            }
        }
    }
    let dealt = match decision {
        DamageDecision::Cancel => None,
        DamageDecision::Amount(a) => Some(a),
        DamageDecision::Pass => Some(amount),
    };
    if let Some(a) = dealt {
        DAMAGE_SEEN
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((entity, a, damage_type.to_owned(), causing));
    }
    decision
}

/// Hands the damage seen since the last tick to the world (the glue, before `World::tick`).
pub fn flush_damage(w: &mut World) {
    let seen = std::mem::take(&mut *DAMAGE_SEEN.lock().unwrap_or_else(|e| e.into_inner()));
    for (entity, amount, kind, causing) in seen {
        w.on_damaged(entity, amount, &kind, causing);
    }
}

/// Takes the queued game work and events (under the lock), for the glue to run outside it.
pub fn take_work() -> (Vec<Op>, Vec<NpcEvent>) {
    with_world(|w| (w.take_ops(), w.take_events())).unwrap_or_default()
}

/// Delivers events to every listener (outside the world's lock).
pub fn deliver(events: &[NpcEvent]) {
    if events.is_empty() {
        return;
    }
    let listeners: Vec<Listener> = LISTENERS.read().unwrap_or_else(|e| e.into_inner()).clone();
    for e in events {
        for l in &listeners {
            l(e);
        }
    }
}

// ---- for other mods -------------------------------------------------------------------------

/// Hears every NPC event (clicks, spawns, despawns, damage, deaths, creation, removal).
pub fn subscribe(listener: impl Fn(&NpcEvent) + Send + Sync + 'static) {
    LISTENERS
        .write()
        .unwrap_or_else(|e| e.into_inner())
        .push(Arc::new(listener));
}

/// Decides about damage to non-protected NPCs (a guard mod lets its NPCs be hurt, or not by
/// their friends). Asked in order; the first answer that is not `Pass` counts.
pub fn add_damage_filter(filter: impl Fn(&DamageQuery) -> DamageDecision + Send + Sync + 'static) {
    DAMAGE_FILTERS
        .write()
        .unwrap_or_else(|e| e.into_inner())
        .push(Arc::new(filter));
}

/// One NPC, as plain data.
#[derive(Clone, Debug, PartialEq)]
pub struct NpcInfo {
    pub id: i32,
    pub uuid: u128,
    pub name: String,
    /// The Bukkit type (`PLAYER`).
    pub kind: String,
    pub protected: bool,
    /// Its entity id while it is out in the world.
    pub entity: Option<u32>,
    pub dimension: Option<String>,
    pub location: Option<Location>,
    pub skin: Skin,
    pub traits: Vec<String>,
}

fn info(w: &World, n: &NpcData) -> NpcInfo {
    NpcInfo {
        id: n.id,
        uuid: n.uuid().unwrap_or(0),
        name: n.name(),
        kind: n.kind(),
        protected: n.protected(),
        entity: w.entity_of(n.id),
        dimension: w.dimension_of(n.id),
        location: w.current_location(n.id),
        skin: n.skin(),
        traits: n.trait_names(),
    }
}

pub fn npcs() -> Vec<NpcInfo> {
    with_world(|w| w.saves.npcs.iter().map(|n| info(w, n)).collect()).unwrap_or_default()
}
pub fn npc(id: i32) -> Option<NpcInfo> {
    with_world(|w| w.saves.get(id).map(|n| info(w, n))).flatten()
}
pub fn by_uuid(uuid: u128) -> Option<i32> {
    with_world(|w| w.saves.by_uuid(uuid).map(|n| n.id)).flatten()
}
pub fn by_entity(entity: u32) -> Option<i32> {
    with_world(|w| w.npc_of_entity(entity)).flatten()
}
pub fn entity_of(id: i32) -> Option<u32> {
    with_world(|w| w.entity_of(id)).flatten()
}
/// NPCs that have trait `name` (`sentinel`).
pub fn with_trait(name: &str) -> Vec<i32> {
    with_world(|w| w.saves.npcs.iter().filter(|n| n.has_trait(name)).map(|n| n.id).collect())
        .unwrap_or_default()
}

/// Creates a spawned NPC (`kind` a Bukkit type, `PLAYER` for a player NPC).
pub fn create(name: &str, kind: &str, at: Location) -> Option<i32> {
    with_world(|w| {
        w.cfg.worlds.dimension(&at.world)?;
        Some(w.create(name, kind, at, None))
    })
    .flatten()
}
pub fn remove(id: i32) -> bool {
    with_world(|w| w.remove(id)).unwrap_or(false)
}
/// Puts it out (again), at its saved location; also after a death.
pub fn spawn(id: i32) -> bool {
    with_world(|w| {
        let Some(n) = w.saves.get_mut(id) else {
            return false;
        };
        n.set_spawned(true);
        w.mark_dirty();
        w.is_spawned(id) || w.spawn(id)
    })
    .unwrap_or(false)
}
pub fn despawn(id: i32) -> bool {
    with_world(|w| {
        let Some(n) = w.saves.get_mut(id) else {
            return false;
        };
        n.set_spawned(false);
        w.mark_dirty();
        w.despawn(id, DespawnReason::Command);
        true
    })
    .unwrap_or(false)
}
pub fn teleport(id: i32, to: Location) -> bool {
    with_world(|w| w.teleport(id, to)).unwrap_or(false)
}
/// Walks toward `target` at `speed` blocks per tick, in a straight line (see the README).
pub fn navigate_to(id: i32, target: [f64; 3], speed: f64) -> bool {
    with_world(|w| w.navigate_to(id, target, speed)).unwrap_or(false)
}
pub fn cancel_navigation(id: i32) {
    with_world(|w| w.cancel_navigation(id));
}
pub fn is_navigating(id: i32) -> bool {
    with_world(|w| w.is_navigating(id)).unwrap_or(false)
}
pub fn face(id: i32, yaw: f32, pitch: f32) {
    with_world(|w| w.face(id, yaw, pitch));
}
/// Turns the NPC's eyes to `target`.
pub fn look_at(id: i32, target: [f64; 3]) {
    with_world(|w| {
        if let Some(at) = w.current_location(id) {
            let (yaw, pitch) = crate::look::look_at(crate::look::eye([at.x, at.y, at.z]), target);
            w.face(id, yaw, pitch);
        }
    });
}
/// Swings the main hand (an attack's animation).
pub fn swing(id: i32) {
    with_world(|w| {
        let (Some(u), Some(d)) = (w.saves.get(id).and_then(NpcData::uuid), w.dimension_of(id)) else {
            return;
        };
        if w.is_spawned(id) {
            w.push_op(Op::Console(crate::spawn::swing(u, &d)));
        }
    });
}
/// The Citizens inventory of an NPC (`traits.inventory`), for other mods (Sentinel's autoswitch).
/// Empty when the NPC has none or does not exist.
pub fn inventory(id: i32) -> Vec<crate::npc::InventoryItem> {
    with_world(|w| w.saves.get(id).map(NpcData::inventory).unwrap_or_default()).unwrap_or_default()
}

/// The equipment of an NPC (the shown slots), with the components of each item.
pub fn equipment(id: i32) -> Vec<crate::npc::Equipped> {
    with_world(|w| w.saves.get(id).map(NpcData::equipment).unwrap_or_default()).unwrap_or_default()
}

/// The potion effect of invisibility the NPC keeps, if any.
pub fn invisibility(id: i32) -> Option<crate::npc::Effect> {
    with_world(|w| w.saves.get(id).and_then(NpcData::invisibility)).flatten()
}

/// Puts `item` (`minecraft:diamond_sword`, None to empty) in a Citizens equipment slot (`hand`,
/// `off_hand`, `helmet`, `chestplate`, `leggings`, `boots`), saved and shown at once.
pub fn set_equipment(id: i32, slot: &str, item: Option<(&str, i64)>) -> bool {
    let Some(game_slot) = crate::npc::EQUIPMENT_SLOTS
        .iter()
        .find(|(c, _)| *c == slot)
        .map(|(_, g)| *g)
    else {
        return false;
    };
    with_world(|w| {
        let Some(n) = w.saves.get_mut(id) else {
            return false;
        };
        n.set_equipment(slot, item);
        let uuid = n.uuid();
        w.mark_dirty();
        if let (Some(u), Some(d), true) = (uuid, w.dimension_of(id), w.is_spawned(id)) {
            let target = match game_slot {
                "mainhand" => "weapon.mainhand",
                "offhand" => "weapon.offhand",
                other => match other {
                    "head" => "armor.head",
                    "chest" => "armor.chest",
                    "legs" => "armor.legs",
                    _ => "armor.feet",
                },
            };
            let what = match item {
                Some((i, c)) => format!("{} {c}", qualified(i)),
                None => "minecraft:air".into(),
            };
            w.push_op(Op::Console(format!(
                "execute in {d} run item replace entity {} {target} with {what}",
                crate::ids::format_uuid(u)
            )));
        }
        true
    })
    .unwrap_or(false)
}
fn qualified(item: &str) -> String {
    if item.contains(':') { item.to_owned() } else { format!("minecraft:{item}") }
}
pub fn set_name(id: i32, name: &str) -> bool {
    with_world(|w| {
        let Some(n) = w.saves.get_mut(id) else {
            return false;
        };
        n.set_name(name);
        w.mark_dirty();
        w.refresh(id);
        true
    })
    .unwrap_or(false)
}
pub fn set_protected(id: i32, on: bool) -> bool {
    with_world(|w| {
        let Some(n) = w.saves.get_mut(id) else {
            return false;
        };
        n.set_protected(on);
        w.mark_dirty();
        w.refresh(id);
        true
    })
    .unwrap_or(false)
}
/// Look-close on or off (`range` in blocks, None keeps the NPC's or the config's).
pub fn set_look_close(id: i32, on: bool, range: Option<f64>) -> bool {
    with_world(|w| {
        let Some(n) = w.saves.get_mut(id) else {
            return false;
        };
        n.set_look_close(on);
        if let Some(r) = range {
            n.set_look_range(r);
        }
        w.mark_dirty();
        true
    })
    .unwrap_or(false)
}
/// Glow on or off, in a `ChatColor` (`GOLD`) when given.
pub fn set_glowing(id: i32, on: bool, color: Option<&str>) -> bool {
    with_world(|w| {
        let Some(n) = w.saves.get_mut(id) else {
            return false;
        };
        n.set_glowing(on);
        if let Some(c) = color {
            n.set_team_color(c);
        }
        w.mark_dirty();
        w.refresh(id);
        true
    })
    .unwrap_or(false)
}
/// Sets the skin by name (looked up) or by a signed texture.
pub fn set_skin(id: i32, skin: Skin) -> bool {
    with_world(|w| {
        let Some(n) = w.saves.get_mut(id) else {
            return false;
        };
        n.set_skin(&skin);
        w.mark_dirty();
        w.respawn(id);
        true
    })
    .unwrap_or(false)
}
pub fn hologram_lines(id: i32) -> Vec<String> {
    with_world(|w| {
        w.saves
            .get(id)
            .map(|n| n.hologram_lines().into_iter().map(|l| l.text).collect())
            .unwrap_or_default()
    })
    .unwrap_or_default()
}
/// Replaces the hologram lines.
pub fn set_hologram_lines(id: i32, lines: &[String]) -> bool {
    with_world(|w| {
        let Some(n) = w.saves.get_mut(id) else {
            return false;
        };
        let now: Vec<String> = n.hologram_lines().into_iter().map(|l| l.text).collect();
        if now == lines {
            return true;
        }
        let keep = now.len().min(lines.len());
        for (i, l) in lines.iter().enumerate().take(keep) {
            n.set_hologram_line(i, l);
        }
        for _ in keep..now.len() {
            n.remove_hologram_line(keep);
        }
        for l in &lines[keep..] {
            n.add_hologram_line(l);
        }
        w.mark_dirty();
        w.refresh_holograms(id);
        true
    })
    .unwrap_or(false)
}
/// The commands of the `commandtrait`.
pub fn commands(id: i32) -> Vec<crate::npc::NpcCommand> {
    with_world(|w| w.saves.get(id).map(NpcData::commands).unwrap_or_default()).unwrap_or_default()
}
pub fn clear_commands(id: i32) -> bool {
    with_world(|w| {
        let Some(n) = w.saves.get_mut(id) else {
            return false;
        };
        n.clear_commands();
        w.mark_dirty();
        true
    })
    .unwrap_or(false)
}
/// A trait's saved data as YAML (`sentinel`), for a mod that keeps its own trait in `saves.yml`.
pub fn trait_data(id: i32, name: &str) -> Option<Node> {
    with_world(|w| w.saves.get(id).and_then(|n| n.trait_data(name).cloned())).flatten()
}
/// Replaces a trait's data; saved with the NPCs.
pub fn set_trait_data(id: i32, name: &str, data: Node) -> bool {
    with_world(|w| {
        let Some(n) = w.saves.get_mut(id) else {
            return false;
        };
        n.set_trait_data(name, data);
        w.mark_dirty();
        true
    })
    .unwrap_or(false)
}
/// An NPC's metadata value (`metadata.<key>`), for flags other mods keep there.
pub fn metadata(id: i32, key: &str) -> Option<Node> {
    with_world(|w| w.saves.get(id).and_then(|n| n.map.at(&["metadata", key]).cloned())).flatten()
}
pub fn set_metadata(id: i32, key: &str, value: Node) -> bool {
    with_world(|w| {
        let Some(n) = w.saves.get_mut(id) else {
            return false;
        };
        n.map.put(&["metadata", key], value);
        w.mark_dirty();
        true
    })
    .unwrap_or(false)
}
/// The NPC a player has selected.
pub fn selected(player: u128) -> Option<i32> {
    with_world(|w| w.selected(player)).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::saves::Saves;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// One test owns the process-wide registry (tests run in threads).
    #[test]
    fn the_registry_end_to_end() {
        let mut cfg = Config::default();
        cfg.skins.fetch = false;
        install(World::new(cfg, Saves::empty(), None));
        let at = Location {
            world: "world".into(),
            x: 0.0,
            y: 64.0,
            z: 0.0,
            yaw: 0.0,
            pitch: 0.0,
            body_yaw: 0.0,
        };
        let id = create("Guard", "PLAYER", at.clone()).unwrap();
        let uuid = npc(id).unwrap().uuid;
        assert_eq!(by_uuid(uuid), Some(id));
        with_world(|w| w.on_entity_join("minecraft:overworld", 40, Some(uuid), true));
        refresh_snapshot();
        assert_eq!(by_entity(40), Some(id));

        // Protected: the hook cancels without asking filters.
        assert_eq!(on_damage(40, "minecraft:generic", 0, 0, 2.0), DamageDecision::Cancel);
        assert!(set_protected(id, false));
        refresh_snapshot();
        add_damage_filter(|q| if q.causing == 7 { DamageDecision::Cancel } else { DamageDecision::Pass });
        assert_eq!(on_damage(40, "minecraft:player_attack", 7, 7, 2.0), DamageDecision::Cancel);
        assert_eq!(on_damage(40, "minecraft:player_attack", 8, 8, 2.0), DamageDecision::Pass);
        assert_eq!(on_damage(41, "minecraft:generic", 0, 0, 2.0), DamageDecision::Pass, "not an NPC");

        static HEARD: AtomicUsize = AtomicUsize::new(0);
        subscribe(|e| {
            if matches!(e, NpcEvent::Damaged { .. }) {
                HEARD.fetch_add(1, Ordering::SeqCst);
            }
        });
        with_world(flush_damage);
        let (_, events) = take_work();
        deliver(&events);
        assert_eq!(HEARD.load(Ordering::SeqCst), 1, "only the damage that went through");

        let mut data = Node::map();
        data.set("respawnTime", Node::int(100));
        assert!(set_trait_data(id, "sentinel", data));
        assert_eq!(with_trait("sentinel"), vec![id]);
        assert_eq!(
            trait_data(id, "sentinel").unwrap().as_map().unwrap().get("respawnTime").unwrap().as_i64(),
            Some(100)
        );
        assert!(set_equipment(id, "hand", Some(("diamond_sword", 1))));
        let (ops, _) = take_work();
        assert!(ops.iter().any(|o| matches!(o, Op::Console(c) if c.contains("item replace entity") && c.contains("weapon.mainhand with minecraft:diamond_sword 1"))));
        assert!(!set_equipment(id, "nose", None));
        assert!(set_hologram_lines(id, &["a".into(), "b".into()]));
        assert!(set_hologram_lines(id, &["c".into()]));
        assert_eq!(hologram_lines(id), vec!["c".to_string()]);
        assert!(navigate_to(id, [3.0, 64.0, 0.0], 0.3));
        assert!(is_navigating(id));
        assert!(despawn(id));
        assert_eq!(entity_of(id), None);
        assert!(remove(id));
        assert!(uninstall().is_some());
        assert!(npcs().is_empty());
        assert_eq!(on_damage(40, "x", 0, 0, 1.0), DamageDecision::Pass);
    }
}
