//! horizon-citizens: the game glue, a trait mod (docs/native-mods.md, "Trait mods").
//!
//! Everything with logic is in `horizon-citizens-core`; this file hands it the hooks and runs
//! what it asks for (console commands, removals, chat lines, skin lookups) at the next tick,
//! outside every player's hook. Other mods use `mod_citizens::api` (the core's API, re-exported).
//!
//! Written without a build of the game (see STATUS.md): the event field names follow the hook
//! points' parameters in `native_hooks.rs` at the pinned revision.

use horizon_citizens_core as core;
use horizon_client::mod_api::{
    AttackEntityEvent, ConfigLoadingEvent, ConfigReloadingEvent, EntityJoinLevelEvent, EntityLeaveLevelEvent,
    Flow, Hooks, Host, InteractEntityEvent, LivingIncomingDamageEvent, NativeCommand, NativeConfig,
    NativeConfigKind, NativeServerPlayer, PerformCommandEvent, PerformConsoleCommandEvent,
    RegisterCommandsEvent, RegisterConfigsEvent, ServerStoppingEvent, Text, TickEvent,
};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};

pub use core::api;
// For dependent mods (Sentinel keeps its trait data as a `yaml::Node` at an `npc::Location`).
pub use core::{npc, yaml};
pub use core::world::{DespawnReason, NpcEvent};

const MOD_ID: &str = "citizens";

pub fn hooks() -> Box<dyn Hooks> {
    let (tx, rx) = channel();
    Box::new(Citizens {
        state: Mutex::new(State {
            cfg: core::config::Config::default(),
            installed: false,
            skins_tx: tx,
            skins_rx: rx,
        }),
    })
}

struct Citizens {
    state: Mutex<State>,
}

struct State {
    cfg: core::config::Config,
    installed: bool,
    skins_tx: Sender<core::skin::Fetched>,
    skins_rx: Receiver<core::skin::Fetched>,
}

fn lock(m: &Mutex<State>) -> std::sync::MutexGuard<'_, State> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// `saves-file` against the server folder (the world folder's parent).
fn saves_path(host: &dyn Host, cfg: &core::config::Config) -> Option<PathBuf> {
    let file = PathBuf::from(&cfg.saves_file);
    if file.is_absolute() {
        return Some(file);
    }
    let world = host.world_dir()?;
    Some(world.parent().map_or_else(|| PathBuf::from("."), |p| p.to_path_buf()).join(file))
}

/// Names for `/npc owner`: the owners of all NPCs (UUID -> name) and the typed word as a name,
/// from Horizon's profile helpers (the user cache covers players who are offline).
fn owner_profiles(host: &dyn Host, line: &str) -> Vec<(u128, String)> {
    let mut words = line.split_whitespace();
    if (words.next(), words.next()) != (Some("npc"), Some("owner")) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for o in api::with_world(|w| w.saves.npcs.iter().filter_map(|n| n.owner()).collect::<Vec<_>>()).unwrap_or_default() {
        if let Some(name) = host.profile_by_uuid(o) {
            out.push((o, name));
        }
    }
    if let Some(word) = words.next()
        && let Some((uuid, name)) = host.profile_by_name(word)
    {
        out.push((uuid, name));
    }
    out
}

fn player_view(p: &NativeServerPlayer) -> core::look::PlayerView {
    core::look::PlayerView {
        uuid: p.uuid,
        name: p.name.clone(),
        dimension: p.dimension.clone(),
        pos: p.position,
        game_mode: p.game_mode,
        permission: p.permission,
        entity_id: p.entity_id,
        // hooks-all `server_player_state`: NativeServerPlayer.sneaking.
        sneaking: p.sneaking,
    }
}

/// The `citizens.*` nodes the permissions hook grants a player below the command level. Operators
/// of that level pass without asking.
fn granted_nodes(host: &dyn Host, p: &NativeServerPlayer, command_level: u8) -> Vec<String> {
    if p.permission >= command_level {
        return Vec::new();
    }
    core::commands::NODES
        .iter()
        .copied()
        .filter(|node| host.has_permission(p.uuid, node, command_level))
        .map(str::to_owned)
        .collect()
}

fn sender_of(p: &NativeServerPlayer, cfg: &core::config::Config) -> core::world::Sender {
    core::world::Sender {
        uuid: Some(p.uuid),
        name: p.name.clone(),
        permission: p.permission,
        grants: Vec::new(),
        at: Some((
            p.dimension.clone(),
            core::npc::Location {
                world: cfg.worlds.world_name(&p.dimension),
                x: p.position[0],
                y: p.position[1],
                z: p.position[2],
                yaw: p.yaw,
                pitch: p.pitch,
                body_yaw: p.yaw,
            },
        )),
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

impl Citizens {
    fn load_config(&self, text: &str, what: &str) {
        match core::config::Config::parse(text) {
            Ok(cfg) => {
                lock(&self.state).cfg = cfg.clone();
                api::with_world(|w| w.cfg = cfg);
                eprintln!("[citizens] config {what}");
            }
            Err(e) => eprintln!("[citizens] config not {what}, keeping the old one: {e}"),
        }
    }

    /// Reads saves.yml once the world is open.
    fn install(&self, host: &dyn Host) {
        let mut st = lock(&self.state);
        if st.installed {
            return;
        }
        let Some(path) = saves_path(host, &st.cfg) else {
            return;
        };
        st.installed = true;
        let saves = match core::saves::Saves::load(&path) {
            Ok(s) => s,
            Err(e) => {
                // Never overwrite a file that could not be read.
                eprintln!("[citizens] could not read {}: {e}; NPCs are off until it is fixed", path.display());
                return;
            }
        };
        eprintln!("[citizens] {} NPCs from {}", saves.npcs.len(), path.display());
        api::install(core::world::World::new(st.cfg.clone(), saves, Some(path)));
    }

    /// Runs the queued game work and hands the events to other mods.
    fn run_work(&self, host: &mut dyn Host) {
        let (ops, events) = api::take_work();
        let tx = lock(&self.state).skins_tx.clone();
        let skins = lock(&self.state).cfg.skins.clone();
        for op in ops {
            match op {
                core::world::Op::Console(line) => {
                    if !host.run_command(&line) {
                        eprintln!("[citizens] command failed: {line}");
                    }
                }
                core::world::Op::RemoveMob { dimension, entity } => {
                    if !host.remove_mob(&dimension, entity) {
                        // Not the level in place, or not a mob Horizon lists: by its UUID.
                        if let Some(u) = host.entity(entity).and_then(|e| host.entity_uuid(e)) {
                            host.run_command(&format!(
                                "execute in {dimension} run {}",
                                core::spawn::kill(u)
                            ));
                        }
                    }
                }
                core::world::Op::Rotate { dimension, entity, yaw, pitch, fallback } => {
                    // Needs the pending API fragment `npc_entity_rotation` (see the bundle's hooks).
                    if !host.set_entity_rotation(&dimension, entity, yaw, pitch, Some(yaw)) {
                        host.run_command(&fallback);
                    }
                }
                core::world::Op::Tell { player, legacy } => {
                    host.tell(player, Text::legacy(&core::text::to_section(&legacy)));
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

    fn reply(&self, host: &mut dyn Host, lines: Vec<String>) {
        for l in lines {
            host.feedback(Text::legacy(&core::text::to_section(&l)));
        }
    }
}

/// A blocking GET with a short timeout; anything but 200 is an error (Mojang answers 204 / 404
/// for an unknown name).
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

impl Hooks for Citizens {
    fn register_configs(&self, _host: &mut dyn Host, event: &mut RegisterConfigsEvent<'_>) {
        event.configs.push(NativeConfig::new(
            MOD_ID,
            NativeConfigKind::Common,
            core::config::DEFAULT_TOML,
        ));
    }

    fn config_loading(&self, _host: &mut dyn Host, event: &mut ConfigLoadingEvent<'_>) {
        if event.config.mod_id == MOD_ID {
            self.load_config(&event.config.text, "loaded");
        }
    }

    fn config_reloading(&self, _host: &mut dyn Host, event: &mut ConfigReloadingEvent<'_>) {
        if event.config.mod_id == MOD_ID {
            self.load_config(&event.config.text, "reloaded");
        }
    }

    fn register_commands(&self, _host: &mut dyn Host, event: &mut RegisterCommandsEvent<'_>) {
        let level = lock(&self.state).cfg.command_level;
        for usage in core::commands::usages() {
            // The node lets a permissions mod grant the command to players below the level.
            let node = core::commands::node_of_usage(&usage);
            event.commands.push(NativeCommand::op(&usage, level).permission(node));
        }
    }

    fn perform_command(&self, host: &mut dyn Host, event: &mut PerformCommandEvent<'_>) -> Flow {
        let root = event.command.split_whitespace().next().unwrap_or("");
        if root != "npc" && root != "citizens" && root != "trait" {
            return Flow::Continue;
        }
        self.install(host);
        let cfg = lock(&self.state).cfg.clone();
        let mut sender = sender_of(event.player, &cfg);
        sender.grants = granted_nodes(&*host, event.player, cfg.command_level);
        let path = saves_path(host, &cfg);
        let profiles = owner_profiles(&*host, event.command);
        let lines = api::with_world(|w| {
            w.profiles.extend(profiles);
            core::commands::run(w, &sender, event.command, || match &path {
                Some(p) => core::saves::Saves::load(p),
                None => Err("no world open".into()),
            })
        })
        .unwrap_or_else(|| vec![cfg.messages.error("NPCs are not loaded (see the server log).")]);
        self.reply(host, lines);
        Flow::Cancel
    }

    fn perform_console_command(&self, host: &mut dyn Host, event: &mut PerformConsoleCommandEvent<'_>) -> Flow {
        let root = event.command.split_whitespace().next().unwrap_or("");
        if root != "npc" && root != "citizens" && root != "trait" {
            return Flow::Continue;
        }
        self.install(host);
        let cfg = lock(&self.state).cfg.clone();
        let sender = core::world::Sender {
            uuid: None,
            name: "Console".into(),
            permission: 4,
            grants: Vec::new(),
            at: None,
        };
        let path = saves_path(host, &cfg);
        let profiles = owner_profiles(&*host, event.command);
        let lines = api::with_world(|w| {
            w.profiles.extend(profiles);
            core::commands::run(w, &sender, event.command, || match &path {
                Some(p) => core::saves::Saves::load(p),
                None => Err("no world open".into()),
            })
        })
        .unwrap_or_else(|| vec!["NPCs are not loaded (see the server log).".into()]);
        self.reply(host, lines);
        // Console commands that change the world run now: the console is outside every hook.
        self.run_work(host);
        Flow::Cancel
    }

    fn interact_entity(&self, host: &mut dyn Host, event: &mut InteractEntityEvent<'_>) -> Flow {
        let Ok(entity) = u32::try_from(event.entity) else {
            return Flow::Continue;
        };
        let Some(player) = host.acting_player() else {
            return Flow::Continue;
        };
        let holding = event.stack.map(|s| s.item.to_string());
        let view = player_view(&player);
        let claimed = api::with_world(|w| w.on_click(entity, &view, core::npc::Hand::Right, holding.as_deref()))
            .unwrap_or(false);
        // Claimed: the hand swings, the entity's and the item's own use do not run, and the off
        // hand is not tried (one click event per click, as Citizens).
        if claimed { Flow::Cancel } else { Flow::Continue }
    }

    fn attack_entity(&self, host: &mut dyn Host, event: &mut AttackEntityEvent<'_>) -> Flow {
        let Ok(entity) = u32::try_from(event.entity) else {
            return Flow::Continue;
        };
        let Some(player) = host.acting_player() else {
            return Flow::Continue;
        };
        let view = player_view(&player);
        let refused = api::with_world(|w| {
            w.on_click(entity, &view, core::npc::Hand::Left, None) && w.hit_refused(entity)
        })
        .unwrap_or(false);
        if refused { Flow::Cancel } else { Flow::Continue }
    }

    fn living_incoming_damage(&self, event: &mut LivingIncomingDamageEvent<'_>) -> Flow {
        match api::on_damage(event.entity, event.damage_type, event.causing, event.direct, event.amount) {
            api::DamageDecision::Pass => Flow::Continue,
            api::DamageDecision::Cancel => Flow::Cancel,
            api::DamageDecision::Amount(a) => {
                event.amount = a;
                Flow::Continue
            }
        }
    }

    fn entity_join_level(&self, host: &mut dyn Host, event: &mut EntityJoinLevelEvent<'_>) -> Flow {
        if !npc_kind(event.kind) {
            return Flow::Continue;
        }
        let Some(e) = host.entity(event.entity) else {
            return Flow::Continue;
        };
        let uuid = host.entity_uuid(e);
        let tagged = host
            .entity_tags(e)
            .is_some_and(|t| t.iter().any(|t| t == core::spawn::TAG));
        let cancel = api::with_world(|w| w.on_entity_join(event.dimension, event.entity, uuid, tagged))
            .unwrap_or(false);
        if cancel { Flow::Cancel } else { Flow::Continue }
    }

    fn entity_leave_level(&self, _host: &mut dyn Host, event: &mut EntityLeaveLevelEvent<'_>) {
        api::with_world(|w| w.on_entity_leave(event.entity));
    }

    fn tick(&self, host: &mut dyn Host, _event: &mut TickEvent) {
        self.install(host);
        let fetched: Vec<core::skin::Fetched> = lock(&self.state).skins_rx.try_iter().collect();
        let players: Vec<core::look::PlayerView> = host.server_players().iter().map(player_view).collect();
        api::with_world(|w| {
            for f in fetched {
                w.skin_fetched(f);
            }
            api::flush_damage(w);
            w.tick(&players);
        });
        self.run_work(host);
        api::refresh_snapshot();
    }

    fn server_stopping(&self, host: &mut dyn Host, _event: &mut ServerStoppingEvent) {
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
