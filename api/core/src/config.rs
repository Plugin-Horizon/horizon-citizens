//! `config/citizens.toml`. Key names follow Citizens' `config.yml` where there is one.

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
    /// Where the lowest line sits above the NPC's feet for a player NPC (over the name tag).
    pub base_height: f64,
}

impl Default for Hologram {
    fn default() -> Self {
        Hologram {
            default_line_height: 0.4,
            base_height: 2.3,
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
    pub fn parse(text: &str) -> Result<Config, String> {
        toml::from_str(text).map_err(|e| e.to_string())
    }
}

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
