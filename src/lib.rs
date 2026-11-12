//! horizon-citizens as an arismoko native trait mod (`mod_api`). The logic is the unchanged
//! `horizon-citizens-core` crate (`core/`); this file only wires its events, commands and work
//! queue to arismoko's hooks and host methods. See PORT_NOTES.md for what is missing meanwhile.
//!
//! Public interface for other mods (Sentinel's bridge, unchanged from 0.1.0):
//! `mod_citizens::{api, npc, yaml, DespawnReason, NpcEvent}`.
use horizon_citizens_core as core;
use mod_api::{
    AttackEntityEvent, Command, CommandArgumentValue, CommandInvocation, CommandSource, Config,
    ConfigKind, ConfigLoadingEvent, ConfigReloadingEvent, EntityInteractEvent,
    EntityJoinLevelEvent, EntityLeaveLevelEvent, Flow, Hand, Hooks, HostSide, LevelRandomHost,
    LivingIncomingDamageEvent, PermissionLevel, PlayerRef, RegisterCommandsEvent,
    RegisterConfigsEvent, RemovalReason, RunModCommandEvent, ServerHost, ServerStoppingEvent,
    ServerTickEvent, ServerTickPhase, Text,
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
    /// The NPC the main hand claimed in the click being handled. The off hand of that same click
    /// is refused for it, as the original (asked once per click) did not run it at all.
    last_claim: Option<i32>,
}

impl Default for Citizens {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self {
            state: Mutex::new(State {
                cfg: core::config::Config::default(),
                toml: core::config::Config::default(),
                installed: false,
                skins_tx: tx,
                skins_rx: rx,
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

/// The `citizens.*` nodes a player below `command_level` holds: the help nodes unless a permissions
/// provider says otherwise (`permission_value`), every other node through `has_permission` (the
/// provider's answer, else operator level 2).
fn granted_nodes(host: &dyn ServerHost, p: PlayerRef, level: u8, command_level: u8) -> Vec<String> {
    if level >= command_level {
        return Vec::new();
    }
    core::commands::NODES
        .iter()
        .copied()
        .filter(|node| {
            if node.ends_with(".help") {
                p.permission_value(host, node).unwrap_or(true)
            } else {
                p.has_permission(host, node)
            }
        })
        .map(str::to_owned)
        .collect()
}

/// The id of the item in the player's main hand (`minecraft:stick`), None for an empty hand.
fn held_item_id(host: &dyn ServerHost, player: PlayerRef) -> Option<String> {
    let living = host.living(player.entity())?;
    let stack = living.hand_item(host, Hand::Main)?;
    Some(stack.item().id(host))
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
fn command_line(cmd: &CommandInvocation) -> String {
    let mut words = cmd.literals.clone();
    words.extend(cmd.arguments.iter().map(|a| arg_text(&a.value)));
    words.retain(|w| !w.is_empty());
    words.join(" ")
}

fn arg_text(value: &CommandArgumentValue) -> String {
    match value {
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
    host.profile_by_uuid(uuid).map(|p| p.name)
}

/// A player by account name, any case: online, else the user cache. Never asked of Mojang.
fn profile_by_name(host: &dyn ServerHost, name: &str) -> Option<(u128, String)> {
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
                    // A `kill <uuid>` before a summon finds nothing unless a copy was saved: not an error.
                    if !host.commands().run_command(&line) && !line.starts_with("kill ") {
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
                core::world::Op::Tell { player, legacy } => {
                    if let Some(p) = host.player(player) {
                        let text = plain(host, &legacy);
                        let _ = p.tell(host, text);
                    }
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
        let level = lock(&self.state).cfg.command_level;
        for usage in core::commands::usages() {
            // Each sub-command path carries Citizens' node (`citizens.npc.create`, `node_of_usage`), so a
            // permissions mod can hand one to a non-operator; `level` decides when none answers. The bare
            // `/npc`, `/citizens` and `/trait` have none: a node there would gate every sub-command under it.
            let command = Command::op(&usage, level);
            event.commands.push(if usage.contains(' ') {
                command.with_node(core::commands::node_of_usage(&usage))
            } else {
                command
            });
        }
    }

    /// `/npc`, `/citizens` and `/trait`, typed by a player or the console (their Citizens lines).
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
        if root != "npc" && root != "citizens" && root != "trait" {
            return Flow::Continue;
        }
        self.install(&*host);
        let cfg = lock(&self.state).cfg.clone();
        let line = command_line(event.command);
        let console = matches!(event.source, CommandSource::Console { .. });
        let sender = match event.source {
            CommandSource::Player { player, .. } => {
                let Some(info) = player_info(&*host, *player) else {
                    return Flow::Cancel;
                };
                let mut sender = sender_of(&info, &cfg);
                sender.grants = granted_nodes(
                    &*host,
                    *player,
                    info.view.permission,
                    cfg.command_level,
                );
                sender
            }
            CommandSource::Console { permission } => bodiless_sender("Console", *permission),
            other => bodiless_sender("Command block", other.permission()),
        };
        let profiles = owner_profiles(&*host, &line);
        let path = saves_path(&*host, &cfg);
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

    fn entity_interact(
        &self,
        host: &mut dyn ServerHost,
        event: &mut EntityInteractEvent<'_>,
    ) -> Flow {
        let Ok(entity) = u32::try_from(event.entity) else {
            return Flow::Continue;
        };
        if event.hand == Hand::Off {
            // Asked after the main hand of the same click: refused when the main hand claimed it.
            let claimed = lock(&self.state).last_claim == Some(event.entity);
            return if claimed {
                Flow::Cancel
            } else {
                Flow::Continue
            };
        }
        let Some(player) = host.players().acting_player() else {
            return Flow::Continue;
        };
        let Some(info) = player_info(&*host, player) else {
            return Flow::Continue;
        };
        let held = held_item_id(&*host, player);
        let claimed = api::with_world(|w| {
            w.on_click(entity, &info.view, core::npc::Hand::Right, held.as_deref())
        })
        .unwrap_or(false);
        lock(&self.state).last_claim = claimed.then_some(event.entity);
        // Claimed: the hand swings, the entity's and the item's own use do not run.
        if claimed {
            Flow::Cancel
        } else {
            Flow::Continue
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
        let refused = api::with_world(|w| {
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
        let players: Vec<core::look::PlayerView> = host
            .players()
            .server_players()
            .into_iter()
            .filter_map(|p| player_info(&*host, p))
            .map(|info| info.view)
            .collect();
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
