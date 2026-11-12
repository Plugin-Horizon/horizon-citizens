//! `config/citizens.toml`. Key names follow Citizens' `config.yml` where there is one.

use crate::tree::TreeExt;
use crate::yaml;
use serde::Deserialize;
use std::collections::BTreeMap;

pub const DEFAULT_TOML: &str = include_str!("../citizens.toml");

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, rename_all = "kebab-case")]
pub struct Config {
    /// `saves.yml`, relative to the server folder (where `server.properties` is). The same file a
    /// Paper server's Citizens uses, so both can read it.
    pub saves_file: String,
    /// Seconds between saves while NPCs changed (they are saved at stop too).
    pub save_delay_seconds: u32,
    /// Operator level for `/npc`.
    pub command_level: u8,
    pub worlds: Worlds,
    pub look_close: LookClose,
    pub hologram: Hologram,
    pub skins: Skins,
    pub commands: Commands,
    pub selection: Selection,
    pub talk: Talk,
    pub messages: Messages,
    /// Blocks around a player within which an NPC is (re)spawned when it is not out: its chunk is
    /// loaded there.
    pub spawn_radius: f64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            saves_file: "plugins/Citizens/saves.yml".into(),
            save_delay_seconds: 5,
            command_level: 2,
            worlds: Worlds::default(),
            look_close: LookClose::default(),
            hologram: Hologram::default(),
            skins: Skins::default(),
            commands: Commands::default(),
            selection: Selection::default(),
            talk: Talk::default(),
            messages: Messages::default(),
            spawn_radius: 96.0,
        }
    }
}

/// Bukkit world names (`location.world`) to dimensions.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, rename_all = "kebab-case")]
pub struct Worlds {
    /// The world folder name of the overworld (`level-name`); `<name>_nether` and
    /// `<name>_the_end` are the other two, as on Paper.
    pub main: String,
    /// More names: `{ "spawn" = "minecraft:overworld" }`.
    pub extra: BTreeMap<String, String>,
}

impl Default for Worlds {
    fn default() -> Self {
        Worlds {
            main: "world".into(),
            extra: BTreeMap::new(),
        }
    }
}

impl Worlds {
    pub fn dimension(&self, world: &str) -> Option<String> {
        if let Some(d) = self.extra.get(world) {
            return Some(d.clone());
        }
        if world == self.main {
            Some("minecraft:overworld".into())
        } else if world == format!("{}_nether", self.main) {
            Some("minecraft:the_nether".into())
        } else if world == format!("{}_the_end", self.main) {
            Some("minecraft:the_end".into())
        } else {
            None
        }
    }
    pub fn world_name(&self, dimension: &str) -> String {
        if let Some((name, _)) = self.extra.iter().find(|(_, d)| d.as_str() == dimension) {
            return name.clone();
        }
        match dimension {
            "minecraft:the_nether" => format!("{}_nether", self.main),
            "minecraft:the_end" => format!("{}_the_end", self.main),
            _ => self.main.clone(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, rename_all = "kebab-case")]
pub struct LookClose {
    /// `npc.default.look-close.enabled`: for new NPCs.
    pub enabled: bool,
    /// `npc.default.look-close.range`.
    pub range: f64,
    /// `npc.default.look-close.random-look-enabled`: for new NPCs.
    pub random_look_enabled: bool,
    /// `npc.default.look-close.random-look-delay`, in ticks.
    pub random_look_delay_ticks: u32,
    /// Smallest change in degrees that is sent.
    pub min_change_degrees: f32,
}

impl Default for LookClose {
    fn default() -> Self {
        LookClose {
            enabled: false,
            range: 10.0,
            random_look_enabled: false,
            random_look_delay_ticks: 60,
            min_change_degrees: 2.0,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, rename_all = "kebab-case")]
pub struct Hologram {
    /// `npc.hologram.default-line-height`.
    pub default_line_height: f64,
    /// Where the lowest line sits above the NPC's feet for a player NPC, less one line height
    /// (Citizens on Paper: line `i` from the bottom at height 1.8 + 0.2 + line height × (i + 1)).
    pub base_height: f64,
}

impl Default for Hologram {
    fn default() -> Self {
        Hologram {
            default_line_height: 0.4,
            base_height: 2.0,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, rename_all = "kebab-case")]
pub struct Skins {
    /// `npc.skins.try-fetch-default-skin`: an NPC without a skin wears its own name's.
    pub try_fetch_default_skin: bool,
    /// Look skin names up at Mojang and keep the signed texture in `saves.yml` (as Citizens
    /// does). Off: the name is handed to the clients, which look it up themselves.
    pub fetch: bool,
    /// `general.authlib.profile-url`.
    pub profile_url: String,
    pub name_lookup_url: String,
}

impl Default for Skins {
    fn default() -> Self {
        Skins {
            try_fetch_default_skin: true,
            fetch: true,
            profile_url: "https://sessionserver.mojang.com/session/minecraft/profile/".into(),
            name_lookup_url: "https://api.mojang.com/users/profiles/minecraft/".into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, rename_all = "kebab-case")]
pub struct Commands {
    /// `npc.commands.global-cooldown`, in ticks: a player's clicks on one NPC closer than this
    /// run its commands once.
    pub global_cooldown_ticks: u32,
    /// Commands marked `player: true` run as the player through `execute as <player> run ...`
    /// (with the console's rights: Horizon has no "run as this player" helper). Off: they are
    /// skipped with a log line.
    pub player_commands_via_execute: bool,
}

impl Default for Commands {
    fn default() -> Self {
        Commands {
            global_cooldown_ticks: 20,
            player_commands_via_execute: true,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, rename_all = "kebab-case")]
pub struct Selection {
    /// `npc.selection.item`: right-clicking an NPC with it selects the NPC (operators of
    /// `command-level`).
    pub item: String,
    /// `npc.selection.message`.
    pub message: String,
}

impl Default for Selection {
    fn default() -> Self {
        Selection {
            item: "minecraft:stick".into(),
            message: "Selected [[<npc>]] (ID [[<id>]]).".into(),
        }
    }
}

/// What an NPC with text lines says to the player who right-clicks it (`npc.chat.format`).
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, rename_all = "kebab-case")]
pub struct Talk {
    /// `<npc>` and `<text>` are filled in. Citizens' `npc.chat.format.no-targets`.
    pub format: String,
}

impl Default for Talk {
    fn default() -> Self {
        Talk {
            format: "[<npc>]: <text>".into(),
        }
    }
}

/// `general.color-scheme`, as legacy codes.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, rename_all = "kebab-case")]
pub struct Messages {
    pub message: String,
    pub highlight: String,
    pub error: String,
}

impl Default for Messages {
    fn default() -> Self {
        Messages {
            message: "&a".into(),
            highlight: "&e".into(),
            error: "&c".into(),
        }
    }
}

impl Messages {
    /// Citizens' `[[highlight]]` markup in the message colour.
    pub fn format(&self, s: &str) -> String {
        let mut out = self.message.clone();
        let mut rest = s;
        while let Some(start) = rest.find("[[") {
            let Some(end) = rest[start..].find("]]") else {
                break;
            };
            out.push_str(&rest[..start]);
            out.push_str(&self.highlight);
            out.push_str(&rest[start + 2..start + end]);
            out.push_str(&self.message);
            rest = &rest[start + end + 2..];
        }
        out.push_str(rest);
        out
    }
    pub fn error(&self, s: &str) -> String {
        format!("{}{s}", self.error)
    }
}

impl Config {
    /// The shipped TOML (`config/citizens.toml`), the fallback when the server has no Citizens
    /// `config.yml`.
    pub fn parse(text: &str) -> Result<Config, String> {
        toml::from_str(text).map_err(|e| e.to_string())
    }

    /// Citizens' own `plugins/Citizens/config.yml`: the keys listed in [`CITIZENS_CONFIG_KEYS`]
    /// that this port has a setting for; every other key keeps the default. Durations are
    /// ticks (`3s` = 60).
    pub fn from_citizens_yaml(text: &str) -> Result<Config, String> {
        let root = yaml::parse(text).map_err(|e| e.to_string())?;
        let mut cfg = Config::default();
        let at = |key: &str| root.at(&key.split('.').collect::<Vec<_>>()).cloned();
        let float = |key: &str| at(key).and_then(|n| n.as_f64());
        let boolean = |key: &str| at(key).and_then(|n| n.as_bool());
        let text_of = |key: &str| at(key).and_then(|n| n.text());
        let ticks = |key: &str| text_of(key).and_then(|t| duration_ticks(&t));

        if let Some(v) = boolean("npc.default.look-close.enabled") {
            cfg.look_close.enabled = v;
        }
        if let Some(v) = float("npc.default.look-close.range") {
            cfg.look_close.range = v;
        }
        if let Some(v) = boolean("npc.default.look-close.random-look-enabled") {
            cfg.look_close.random_look_enabled = v;
        }
        if let Some(v) = ticks("npc.default.look-close.random-look-delay") {
            cfg.look_close.random_look_delay_ticks = v;
        }
        if let Some(v) = float("npc.hologram.default-line-height") {
            cfg.hologram.default_line_height = v;
        }
        if let Some(v) = ticks("npc.commands.global-cooldown") {
            cfg.commands.global_cooldown_ticks = v;
        }
        if let Some(v) = boolean("npc.skins.try-fetch-default-skin") {
            cfg.skins.try_fetch_default_skin = v;
        }
        if let Some(v) = text_of("general.authlib.profile-url") {
            cfg.skins.profile_url = v;
        }
        if let Some(v) = text_of("npc.selection.item") {
            // A Bukkit material name (`stick`) is the namespaced item (`minecraft:stick`).
            cfg.selection.item = namespaced_item(&v);
        }
        if let Some(v) = text_of("npc.selection.message") {
            cfg.selection.message = v;
        }
        if let Some(v) = text_of("npc.chat.format.no-targets") {
            cfg.talk.format = v;
        }
        if let Some(file) = text_of("storage.file") {
            // Citizens keeps the file in its own folder.
            cfg.saves_file = format!("plugins/Citizens/{file}");
        }
        let colours = [
            ("general.color-scheme.message", &mut cfg.messages.message),
            ("general.color-scheme.message-highlight", &mut cfg.messages.highlight),
            ("general.color-scheme.message-error", &mut cfg.messages.error),
        ];
        for (key, slot) in colours {
            if let Some(code) = text_of(key).and_then(|c| minimessage_colour(&c)) {
                *slot = code;
            }
        }
        Ok(cfg)
    }

    /// The overworld's folder name: the server's `level-name` (Citizens' NPCs of a Paper server
    /// live in that world, not in the default `world`).
    pub fn with_level_name(mut self, level_name: &str) -> Config {
        self.worlds.main = level_name.to_owned();
        self
    }
}

/// The `level-name` of a `server.properties` (`None` when missing or empty). Only that key is
/// read: the rest of the file holds secrets.
pub fn level_name_from_properties(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let value = line.trim().strip_prefix("level-name=")?.trim();
        (!value.is_empty()).then(|| value.to_owned())
    })
}

/// A Citizens duration (`1s`, `3s`, `1hr`, `250ms`, `2min`, `1d`) in server ticks (20 a second).
pub fn duration_ticks(s: &str) -> Option<u32> {
    let s = s.trim();
    let split = s.find(|c: char| !(c.is_ascii_digit() || c == '.'))?;
    let (number, unit) = s.split_at(split);
    let number: f64 = number.parse().ok()?;
    let per_unit = match unit.trim().to_ascii_lowercase().as_str() {
        "t" | "tick" | "ticks" => 1.0,
        "ms" => 0.05,
        "s" | "sec" | "secs" | "second" | "seconds" => 20.0,
        "m" | "min" | "mins" | "minute" | "minutes" => 1200.0,
        "h" | "hr" | "hrs" | "hour" | "hours" => 72_000.0,
        "d" | "day" | "days" => 1_728_000.0,
        _ => return None,
    };
    let ticks = (number * per_unit).round();
    (ticks.is_finite() && ticks >= 0.0 && ticks <= f64::from(u32::MAX)).then_some(ticks as u32)
}

/// `stick` -> `minecraft:stick`; a namespaced id stays.
fn namespaced_item(name: &str) -> String {
    if name.contains(':') { name.to_owned() } else { format!("minecraft:{}", name.to_ascii_lowercase()) }
}

/// Citizens' MiniMessage colour tags as the legacy codes the port writes (`<green>` -> `&a`).
/// `None` for anything else, so the default stays.
fn minimessage_colour(tag: &str) -> Option<String> {
    let code = match tag.trim() {
        "<black>" => "&0",
        "<dark_blue>" => "&1",
        "<dark_green>" => "&2",
        "<dark_aqua>" => "&3",
        "<dark_red>" => "&4",
        "<dark_purple>" => "&5",
        "<gold>" => "&6",
        "<gray>" => "&7",
        "<dark_gray>" => "&8",
        "<blue>" => "&9",
        "<green>" => "&a",
        "<aqua>" => "&b",
        "<red>" => "&c",
        "<light_purple>" => "&d",
        "<yellow>" => "&e",
        "<white>" => "&f",
        _ => return None,
    };
    Some(code.to_owned())
}

/// The keys of Citizens' `config.yml` that [`Config::from_citizens_yaml`] reads (`check_original`
/// reports every other key of a file as not read).
pub const CITIZENS_CONFIG_KEYS: &[&str] = &[
    "npc.default.look-close.enabled",
    "npc.default.look-close.range",
    "npc.default.look-close.random-look-enabled",
    "npc.default.look-close.random-look-delay",
    "npc.hologram.default-line-height",
    "npc.commands.global-cooldown",
    "npc.skins.try-fetch-default-skin",
    "general.authlib.profile-url",
    "npc.selection.item",
    "npc.selection.message",
    "npc.chat.format.no-targets",
    "storage.file",
    "general.color-scheme.message",
    "general.color-scheme.message-highlight",
    "general.color-scheme.message-error",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_is_the_default() {
        assert_eq!(Config::parse(DEFAULT_TOML).unwrap(), Config::default());
        assert_eq!(Config::parse("").unwrap(), Config::default());
    }

    #[test]
    fn worlds_map_like_paper() {
        let mut w = Worlds {
            main: "stage".into(),
            ..Worlds::default()
        };
        assert_eq!(w.dimension("stage").as_deref(), Some("minecraft:overworld"));
        assert_eq!(w.dimension("stage_nether").as_deref(), Some("minecraft:the_nether"));
        assert_eq!(w.dimension("other"), None);
        w.extra.insert("other".into(), "minecraft:the_end".into());
        assert_eq!(w.dimension("other").as_deref(), Some("minecraft:the_end"));
        assert_eq!(w.world_name("minecraft:the_end"), "other");
        assert_eq!(w.world_name("minecraft:overworld"), "stage");
    }

    #[test]
    fn highlights() {
        let m = Messages::default();
        assert_eq!(m.format("Selected [[Bob]] (ID [[3]])."), "&aSelected &eBob&a (ID &e3&a).");
    }
}

#[cfg(test)]
mod citizens_yaml_tests {
    use super::*;

    const FIXTURE: &str = "# invented values, same layout as Citizens' config.yml
npc:
  default:
    look-close:
      enabled: true
      range: 12
      random-look-enabled: true
      random-look-delay: 2s
      realistic-looking: false
  hologram:
    default-line-height: 0.5
  commands:
    global-cooldown: 500ms
  skins:
    try-fetch-default-skin: false
  selection:
    item: diamond
    message: 'Picked [[<npc>]] (ID [[<id>]]).'
  chat:
    format:
      no-targets: '<<npc>>: <text>'
general:
  authlib:
    profile-url: 'https://example.invalid/profile/'
  color-scheme:
    message: '<gold>'
    message-highlight: '<white>'
    message-error: '<nonsense>'
storage:
  file: npcs.yml
  save-task-frequency: 1hr
";

    #[test]
    fn durations_are_ticks() {
        assert_eq!(duration_ticks("1s"), Some(20));
        assert_eq!(duration_ticks("3s"), Some(60));
        assert_eq!(duration_ticks("1.5s"), Some(30));
        assert_eq!(duration_ticks("500ms"), Some(25));
        assert_eq!(duration_ticks("2min"), Some(2400));
        assert_eq!(duration_ticks("1hr"), Some(72_000));
        assert_eq!(duration_ticks("1d"), Some(1_728_000));
        assert_eq!(duration_ticks("7t"), Some(7));
        assert_eq!(duration_ticks("10"), None, "a number without a unit");
        assert_eq!(duration_ticks("abc"), None);
        assert_eq!(duration_ticks("-1s"), None);
    }

    #[test]
    fn the_original_config_sets_the_port() {
        let c = Config::from_citizens_yaml(FIXTURE).unwrap();
        assert!(c.look_close.enabled);
        assert_eq!(c.look_close.range, 12.0);
        assert!(c.look_close.random_look_enabled);
        assert_eq!(c.look_close.random_look_delay_ticks, 40);
        assert_eq!(c.hologram.default_line_height, 0.5);
        assert_eq!(c.commands.global_cooldown_ticks, 25);
        assert!(!c.skins.try_fetch_default_skin);
        assert_eq!(c.skins.profile_url, "https://example.invalid/profile/");
        assert_eq!(c.selection.item, "minecraft:diamond");
        assert_eq!(c.selection.message, "Picked [[<npc>]] (ID [[<id>]]).");
        assert_eq!(c.talk.format, "<<npc>>: <text>");
        assert_eq!(c.saves_file, "plugins/Citizens/npcs.yml");
        assert_eq!(c.messages.message, "&6", "MiniMessage colours become legacy codes");
        assert_eq!(c.messages.highlight, "&f");
        assert_eq!(c.messages.error, "&c", "an unknown tag keeps the default");
    }

    #[test]
    fn a_missing_key_keeps_the_default() {
        assert_eq!(Config::from_citizens_yaml("").unwrap(), Config::default());
        assert_eq!(Config::from_citizens_yaml("npc: {}\n").unwrap(), Config::default());
    }

    #[test]
    fn the_spawn_world_is_the_level_name() {
        assert_eq!(level_name_from_properties("motd=x\nlevel-name=stage\n").as_deref(), Some("stage"));
        assert_eq!(level_name_from_properties("level-name=\n"), None);
        assert_eq!(level_name_from_properties("# level-name=stage\n"), None, "a comment is not the key");
        let c = Config::default().with_level_name("stage");
        assert_eq!(c.worlds.dimension("stage").as_deref(), Some("minecraft:overworld"));
        assert_eq!(c.worlds.dimension("world"), None);
        assert_eq!(c.worlds.world_name("minecraft:overworld"), "stage");
    }

    #[test]
    fn the_key_list_names_real_keys() {
        let doc = yaml::parse(FIXTURE).unwrap();
        let mut missing = Vec::new();
        for key in CITIZENS_CONFIG_KEYS {
            if doc.at(&key.split('.').collect::<Vec<_>>()).is_none() {
                missing.push(*key);
            }
        }
        // Keys absent from the fixture are fine (their defaults apply); the list only has to
        // be spelled as Citizens spells them, which the fixture covers.
        for key in ["npc.default.look-close.enabled", "npc.selection.item", "storage.file"] {
            assert!(!missing.contains(&key), "{key}");
        }
    }
}
