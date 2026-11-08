//! The NPCs at run time: which are out in the world under which entity id, what to send the game
//! (console commands and the like, run by the glue at the next tick), clicks, look-close, walking
//! along waypoints, saving.
//!
//! Nothing here calls the game: every change is an [`Op`] the glue takes with [`World::take_ops`]
//! and runs outside its lock, and every event for other mods is an [`NpcEvent`] taken with
//! [`World::take_events`].

use crate::config::Config;
use crate::look::{self, PlayerView, Rng};
use crate::npc::{Hand, Location, NpcData, Skin};
use crate::saves::Saves;
use crate::spawn;
use crate::text;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;

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
    /// (player, npc) -> tick of the last click whose commands ran.
    cooldowns: HashMap<(u128, i32), u64>,
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
}

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
            cooldowns: HashMap::new(),
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
            .unwrap_or_else(|| crate::ids::format_uuid(uuid))
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
                let u = crate::ids::random_uuid();
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
        if let Some(color) = npc.team_color() {
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
        } else if let (Some(color), Some(u)) = (npc.team_color(), npc.uuid()) {
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
    fn talk(&mut self, player: &PlayerView, npc: &crate::npc::NpcData) {
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
        let last = self.cooldowns.get(&(player.uuid, id)).copied();
        let cooled = last.is_none_or(|t| self.tick.saturating_sub(t) >= u64::from(self.cfg.commands.global_cooldown_ticks));
        let commands: Vec<_> = npc.commands().into_iter().filter(|c| c.hand.matches(hand, player.sneaking)).collect();
        if cooled && !commands.is_empty() {
            self.cooldowns.insert((player.uuid, id), self.tick);
            for c in commands {
                if let Some(why) = &c.unenforced {
                    eprintln!("[citizens] NPC {id}: command {} skipped ({why} is not enforced on Horizon yet)", c.key);
                    continue;
                }
                let line = text::fill_placeholders(&c.command, &player.name, &npc.name(), id);
                let line = line.trim_start_matches('/').to_owned();
                if c.player || c.op {
                    if self.cfg.commands.player_commands_via_execute {
                        let who = crate::ids::format_uuid(player.uuid);
                        self.ops
                            .push(Op::Console(format!("execute as {who} at {who} run {line}")));
                    } else {
                        eprintln!("[citizens] NPC {id}: player command skipped (player-commands-via-execute is off): {line}");
                    }
                } else {
                    self.ops.push(Op::Console(line));
                }
            }
        }
        true
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
    pub fn skin_fetched(&mut self, fetched: crate::skin::Fetched) {
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
        if self.tick % 2 == 0 {
            self.look_close(players);
        }
        if let Some(since) = self.dirty_since
            && self.tick.saturating_sub(since) >= u64::from(self.cfg.save_delay_seconds) * 20
            && let Err(e) = self.save_now()
        {
            eprintln!("[citizens] could not save NPCs: {e}");
        }
        if self.tick % 1200 == 0 {
            let tick = self.tick;
            let cooldown = u64::from(self.cfg.commands.global_cooldown_ticks);
            self.cooldowns.retain(|_, t| tick.saturating_sub(*t) < cooldown);
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
                    .get_path(&["navigator", "speedmodifier"])
                    .and_then(crate::yaml::Node::as_f64)
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
            if let Some(l) = self.live.get_mut(&id) {
                l.at = Some(loc.clone());
                l.look = Some((loc.yaw, loc.pitch));
            }
            self.ops.push(Op::Console(spawn::teleport(uuid, &dimension, &loc)));
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
                        .and_then(|n| n.map.get_path(&["navigator", "speedmodifier"]))
                        .and_then(crate::yaml::Node::as_f64)
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
        let mut npc = NpcData::new(id, crate::ids::random_uuid(), name, &kind.to_ascii_uppercase());
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
        self.mark_dirty();
        self.events.push(NpcEvent::Created { npc: id });
        if spawned {
            self.spawn(id);
        }
        id
    }

    pub fn remove(&mut self, id: i32) -> bool {
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
            .set("uuid", crate::yaml::Node::str(&crate::ids::format_uuid(crate::ids::random_uuid())));
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
        w.skin_fetched(crate::skin::Fetched {
            npc: id,
            requested: "Steve".into(),
            result: Ok(crate::skin::FetchedSkin {
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
