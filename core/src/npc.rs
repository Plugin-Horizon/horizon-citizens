//! One NPC as Citizens keeps it in `saves.yml`: `npc.<id>` with `name`, `uuid`, `traitnames`,
//! `metadata`, `navigator` and `traits`. The NPC is its YAML map; the methods read and change the
//! keys this mod understands and leave every other key (Sentinel's, poses, inventory, ...) as it is,
//! so the file stays readable by Citizens itself.

use crate::tree::TreeExt;
use crate::yaml::Node;

/// Where an NPC stands: a Bukkit world name and a position with its rotation.
#[derive(Clone, Debug, PartialEq)]
pub struct Location {
    pub world: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
    pub body_yaw: f32,
}

/// A Citizens skin: a name to fetch and/or the cached signed texture.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Skin {
    pub name: Option<String>,
    pub texture: Option<String>,
    pub signature: Option<String>,
}

impl Skin {
    pub fn has_texture(&self) -> bool {
        self.texture.as_deref().is_some_and(|t| !t.is_empty())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nameplate {
    Shown,
    Hover,
    Hidden,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hand {
    Right,
    Left,
    Both,
    /// Citizens' `SHIFT_LEFT` / `SHIFT_RIGHT`: only while the player sneaks.
    ShiftLeft,
    ShiftRight,
    /// A hand name this port does not know: the command matches no click.
    Unknown,
}

impl Hand {
    pub fn parse(s: &str) -> Option<Hand> {
        match s.to_ascii_uppercase().as_str() {
            "RIGHT" => Some(Hand::Right),
            "LEFT" => Some(Hand::Left),
            "BOTH" => Some(Hand::Both),
            "SHIFT_LEFT" => Some(Hand::ShiftLeft),
            "SHIFT_RIGHT" => Some(Hand::ShiftRight),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Hand::Right => "RIGHT",
            Hand::Left => "LEFT",
            Hand::Both => "BOTH",
            Hand::ShiftLeft => "SHIFT_LEFT",
            Hand::ShiftRight => "SHIFT_RIGHT",
            Hand::Unknown => "UNKNOWN",
        }
    }
    /// Whether a command for this hand runs on a click with `click` (`Right` or `Left`) while
    /// the player sneaks or not. A plain `RIGHT`/`LEFT` command also runs on a sneaking click;
    /// a `SHIFT_*` command only on one. `Unknown` never runs.
    pub fn matches(self, click: Hand, sneaking: bool) -> bool {
        match self {
            Hand::Both => true,
            Hand::Right | Hand::Left => self == click,
            Hand::ShiftRight => sneaking && click == Hand::Right,
            Hand::ShiftLeft => sneaking && click == Hand::Left,
            Hand::Unknown => false,
        }
    }
}

/// One `commandtrait` entry.
#[derive(Clone, Debug, PartialEq)]
pub struct NpcCommand {
    /// Its key in the `commands` map (or its place in the list form).
    pub key: String,
    pub command: String,
    pub hand: Hand,
    /// Run as the clicking player rather than the console.
    pub player: bool,
    /// Run as the player with operator rights.
    pub op: bool,
    /// Why the port does not run it (review C4): a limit it cannot enforce yet (`perms`, `n`,
    /// `cost`, per-command `cooldown`/`globalcooldown`/`delay`, or an `executionmode` other than
    /// `LINEAR`). Such a command is skipped rather than run for everyone, every time.
    pub unenforced: Option<String>,
    /// `cooldown` (seconds), `globalcooldown` (seconds), `delay` (ticks), `n`, `gn`: 0 when unset.
    pub cooldown: i64,
    pub global_cooldown: i64,
    pub delay: i64,
    pub n: i64,
    pub gn: i64,
    /// `cost` and `experienceCost`: -1 when unset (Citizens' default).
    pub cost: f64,
    pub experience_cost: i64,
}

/// What `/npc command add` writes for a new `commandtrait` entry (Citizens' `NPCCommandBuilder`).
#[derive(Clone, Debug, PartialEq)]
pub struct CommandSpec {
    pub command: String,
    pub hand: Hand,
    pub player: bool,
    pub op: bool,
    /// `-n`: run as the NPC itself.
    pub npc: bool,
    pub cooldown: i64,
    pub global_cooldown: i64,
    pub delay: i64,
    pub n: i64,
    pub gn: i64,
    pub cost: f64,
    pub experience_cost: i64,
    pub permissions: Vec<String>,
}

impl CommandSpec {
    /// Citizens' builder defaults: no limits, `n`/`gn`/`cost`/`experienceCost` at -1.
    pub fn new(command: &str, hand: Hand) -> CommandSpec {
        CommandSpec {
            command: command.to_owned(),
            hand,
            player: false,
            op: false,
            npc: false,
            cooldown: 0,
            global_cooldown: 0,
            delay: 0,
            n: -1,
            gn: -1,
            cost: -1.0,
            experience_cost: -1,
            permissions: Vec::new(),
        }
    }
}

/// One hologram line.
#[derive(Clone, Debug, PartialEq)]
pub struct HologramLine {
    pub text: String,
    pub margin_top: f64,
    pub margin_bottom: f64,
    /// `renderer.billboard` (`CENTER`, `FIXED`, `VERTICAL`, `HORIZONTAL`).
    pub billboard: String,
    pub see_through: bool,
    pub shadowed: bool,
}

impl HologramLine {
    fn plain(text: String) -> Self {
        HologramLine {
            text,
            margin_top: 0.0,
            margin_bottom: 0.0,
            billboard: "CENTER".into(),
            see_through: false,
            shadowed: true,
        }
    }
}

/// The equipment slots of Citizens' `equipment` trait, with the slot names the game uses.
pub const EQUIPMENT_SLOTS: [(&str, &str); 6] = [
    ("hand", "mainhand"),
    ("off_hand", "offhand"),
    ("helmet", "head"),
    ("chestplate", "chest"),
    ("leggings", "legs"),
    ("boots", "feet"),
];

#[derive(Clone, Debug, PartialEq)]
pub struct Equipped {
    /// The game's slot name (`mainhand`, `head`, ...).
    pub slot: &'static str,
    /// The item id without namespace, as Citizens stores it (`diamond_sword`).
    pub item: String,
    pub count: i64,
    /// The item's components as SNBT entries (`"minecraft:enchantments":{...}`), from its meta.
    pub components: Vec<String>,
}

/// One stack of the Citizens inventory.
#[derive(Clone, Debug, PartialEq)]
pub struct InventoryItem {
    /// Its place in `traits.inventory`.
    pub index: usize,
    pub item: String,
    pub count: i64,
    pub components: Vec<String>,
}

/// A potion effect an NPC keeps (Citizens' `potioneffects`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Effect {
    /// Ticks; `None` for good.
    pub duration: Option<i64>,
    pub amplifier: i64,
}

/// A stack as Citizens saves it (`type`, `amount`, `meta`, `durability`): its id without
/// namespace, its count and its components. `None` for an empty or air stack.
fn stack_of(m: &Node) -> Option<(String, i64, Vec<String>)> {
    let item = m.get("type").and_then(Node::text)?.to_ascii_lowercase();
    if item.is_empty() || item == "air" {
        return None;
    }
    let count = m.get("amount").and_then(Node::as_i64).unwrap_or(1).max(1);
    let mut components = match m.get("meta").and_then(Node::text) {
        Some(meta) => crate::items::components_of(&meta).unwrap_or_else(|e| {
            eprintln!("[citizens] item {item}: meta not read ({e}); the item has no components");
            Vec::new()
        }),
        None => Vec::new(),
    };
    // Citizens' durability is the damage the item has taken (the meta's `Damage` wins if both).
    if let Some(d) = m.get("durability").and_then(Node::as_i64)
        && !components.iter().any(|c| c.starts_with("\"minecraft:damage\""))
    {
        components.push(format!("\"minecraft:damage\":{d}"));
    }
    Some((item, count, components))
}

#[derive(Clone, Debug, PartialEq)]
pub struct NpcData {
    pub id: i32,
    pub map: Node,
}

/// A flag as Citizens reads it: a YAML boolean, or a quoted `'True'` / `'False'` (any case).
/// The NPC keys the port reads, as `check_original` matches them (`*` one segment, trailing `**`
/// any rest). Keys written but not read are not listed. Kept by hand: update it with the readers.
pub const NPC_READ_PATHS: &[&str] = &[
    "name",
    "uuid",
    "traitnames",
    "navigator.speedmodifier",
    "metadata.protected",
    "metadata.glowing",
    "metadata.nameplate-visible",
    "traits.type",
    "traits.location.world",
    "traits.location.x",
    "traits.location.y",
    "traits.location.z",
    "traits.location.yaw",
    "traits.location.pitch",
    "traits.location.bodyYaw",
    "traits.spawned",
    "traits.owner.uuid",
    "traits.scoreboardtrait.color",
    "traits.gravity.enabled",
    "traits.skintrait.textureRaw",
    "traits.skintrait.signature",
    "traits.skintrait.skinName",
    "traits.lookclose.enabled",
    "traits.lookclose.range",
    "traits.lookclose.enableRandomLook",
    "traits.lookclose.randomYawRange.**",
    "traits.lookclose.randomPitchRange.**",
    "traits.hologramtrait.lines.**",
    "traits.hologramtrait.lineHeight",
    "traits.commandtrait.**",
    "traits.equipment.*.type",
    "traits.equipment.*.amount",
    "traits.equipment.*.meta",
    "traits.equipment.*.durability",
    "traits.inventory.**",
    "traits.potioneffects.persistent.invisibility.**",
    "traits.waypoints.provider",
    "traits.waypoints.linear.points.**",
    "traits.text.texts.**",
    "traits.sentinel.**",
];

fn bool_of(n: &Node) -> Option<bool> {
    n.as_bool().or_else(|| match n.text()?.to_ascii_lowercase().as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    })
}

fn text_at(m: &Node, path: &[&str]) -> Option<String> {
    m.at(path).and_then(Node::text)
}

fn f64_at(m: &Node, path: &[&str]) -> Option<f64> {
    m.at(path).and_then(Node::as_f64)
}

impl NpcData {
    /// A new NPC with what Citizens writes for `/npc create`.
    pub fn new(id: i32, uuid: u128, name: &str, kind: &str) -> NpcData {
        let mut n = NpcData {
            id,
            map: Node::map(),
        };
        n.map.put(&["metadata", "protected"], Node::bool(true));
        n.map.put(&["traits", "type"], Node::str(kind));
        n.map
            .put(&["traits", "scoreboardtrait", "tags"], Node::seq(vec![Node::str("CITIZENS_NPC")]));
        n.map.set("traitnames", Node::str("scoreboardtrait,spawned,type,location"));
        n.map.set("name", Node::str(name));
        n.map.put(&["navigator", "usedefaultstuckaction"], Node::bool(false));
        n.map.put(&["navigator", "speedmodifier"], Node::float(1.0));
        n.map.put(&["navigator", "avoidwater"], Node::bool(false));
        n.map.set("uuid", Node::str(&crate::ids::format_uuid(uuid)));
        n
    }

    pub fn name(&self) -> String {
        text_at(&self.map, &["name"]).unwrap_or_default()
    }
    pub fn set_name(&mut self, name: &str) {
        self.map.set("name", Node::str(name));
    }
    pub fn uuid(&self) -> Option<u128> {
        text_at(&self.map, &["uuid"]).and_then(|s| crate::ids::parse_uuid(&s))
    }
    pub fn set_uuid(&mut self, uuid: u128) {
        self.map.set("uuid", Node::str(&crate::ids::format_uuid(uuid)));
    }

    /// The Bukkit entity type (`PLAYER`, `SNOW_GOLEM`).
    pub fn kind(&self) -> String {
        text_at(&self.map, &["traits", "type"]).unwrap_or_else(|| "PLAYER".into())
    }
    pub fn set_kind(&mut self, kind: &str) {
        self.map.put(&["traits", "type"], Node::str(&kind.to_ascii_uppercase()));
        self.add_trait_name("type");
    }
    pub fn is_player(&self) -> bool {
        self.kind().eq_ignore_ascii_case("PLAYER")
    }

    /// The comma list of traits the NPC has (`traitnames`).
    pub fn trait_names(&self) -> Vec<String> {
        text_at(&self.map, &["traitnames"])
            .map(|t| {
                t.split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    }
    pub fn add_trait_name(&mut self, name: &str) {
        let mut names = self.trait_names();
        if !names.iter().any(|n| n == name) {
            names.push(name.to_owned());
            self.map.set("traitnames", Node::str(&names.join(",")));
        }
    }
    pub fn has_trait(&self, name: &str) -> bool {
        self.trait_names().iter().any(|n| n == name) || self.map.at(&["traits", name]).is_some()
    }

    pub fn location(&self) -> Option<Location> {
        let l = self.map.at(&["traits", "location"])?.as_map()?;
        let yaw = l.get("yaw").and_then(Node::as_f64).unwrap_or(0.0) as f32;
        Some(Location {
            world: l.get("world").and_then(Node::text)?,
            x: l.get("x").and_then(Node::as_f64)?,
            y: l.get("y").and_then(Node::as_f64)?,
            z: l.get("z").and_then(Node::as_f64)?,
            yaw,
            pitch: l.get("pitch").and_then(Node::as_f64).unwrap_or(0.0) as f32,
            body_yaw: l.get("bodyYaw").and_then(Node::as_f64).map_or(yaw, |v| v as f32),
        })
    }
    /// Sets the location. `worldid` (the Bukkit world's UUID) stays when the world is the same and
    /// is dropped otherwise: Citizens then finds the world by its name.
    pub fn set_location(&mut self, loc: &Location) {
        let same_world = self.location().is_some_and(|old| old.world == loc.world);
        let l = self.map.map_at(&["traits", "location"]);
        l.set("world", Node::str(&loc.world));
        if !same_world {
            l.remove("worldid");
        }
        l.set("x", Node::float(round4(loc.x)));
        l.set("y", Node::float(round4(loc.y)));
        l.set("z", Node::float(round4(loc.z)));
        l.set("pitch", Node::float(round4(f64::from(loc.pitch))));
        l.set("bodyYaw", Node::float(round4(f64::from(loc.body_yaw))));
        l.set("yaw", Node::float(round4(f64::from(loc.yaw))));
        self.add_trait_name("location");
    }

    /// `traits.spawned`: whether the NPC is out in the world (missing: yes).
    pub fn spawned(&self) -> bool {
        self.map
            .at(&["traits", "spawned"])
            .and_then(bool_of)
            .unwrap_or(true)
    }
    pub fn set_spawned(&mut self, on: bool) {
        self.map.put(&["traits", "spawned"], Node::bool(on));
        self.add_trait_name("spawned");
    }

    pub fn skin(&self) -> Skin {
        let s = |k: &str| text_at(&self.map, &["traits", "skintrait", k]).filter(|v| !v.is_empty());
        Skin {
            name: s("skinName"),
            texture: s("textureRaw"),
            signature: s("signature"),
        }
    }
    /// Sets the skin. A name alone clears the cached texture, which the mod then fetches.
    pub fn set_skin(&mut self, skin: &Skin) {
        let t = self.map.map_at(&["traits", "skintrait"]);
        for (key, v) in [
            ("signature", &skin.signature),
            ("skinName", &skin.name),
            ("textureRaw", &skin.texture),
        ] {
            match v {
                Some(v) => t.set(key, Node::str(v)),
                None => {
                    t.remove(key);
                }
            }
        }
        self.add_trait_name("skintrait");
    }
    /// The cached skin profile Citizens keeps beside the texture (`metadata.cached-skin-uuid`).
    pub fn set_cached_skin_profile(&mut self, uuid: u128, name: &str) {
        self.map.put(
            &["metadata", "cached-skin-uuid"],
            Node::str(&crate::ids::format_uuid(uuid)),
        );
        self.map.put(&["metadata", "cached-skin-uuid-name"], Node::str(name));
    }

    /// `metadata.protected`: no damage, no targeting (missing: protected, Citizens' default).
    pub fn protected(&self) -> bool {
        self.map
            .at(&["metadata", "protected"])
            .and_then(bool_of)
            .unwrap_or(true)
    }
    pub fn set_protected(&mut self, on: bool) {
        self.map.put(&["metadata", "protected"], Node::bool(on));
    }

    pub fn glowing(&self) -> bool {
        self.map
            .at(&["metadata", "glowing"])
            .and_then(bool_of)
            .unwrap_or(false)
    }
    pub fn set_glowing(&mut self, on: bool) {
        self.map.put(&["metadata", "glowing"], Node::bool(on));
    }

    /// The scoreboard team colour (`traits.scoreboardtrait.color`, a `ChatColor` name).
    pub fn team_color(&self) -> Option<String> {
        text_at(&self.map, &["traits", "scoreboardtrait", "color"])
    }
    pub fn set_team_color(&mut self, color: &str) {
        self.map
            .put(&["traits", "scoreboardtrait", "color"], Node::str(&color.to_ascii_uppercase()));
        self.add_trait_name("scoreboardtrait");
    }

    pub fn nameplate(&self) -> Nameplate {
        match self.map.at(&["metadata", "nameplate-visible"]) {
            None => Nameplate::Shown,
            Some(n) => match n.text().unwrap_or_default().to_ascii_lowercase().as_str() {
                "hover" => Nameplate::Hover,
                "false" | "no" | "off" => Nameplate::Hidden,
                _ => Nameplate::Shown,
            },
        }
    }
    pub fn set_nameplate(&mut self, n: Nameplate) {
        let v = match n {
            // Citizens stores this metadata as a string ('true'/'false'), not a YAML boolean.
            Nameplate::Shown => Node::str("true"),
            Nameplate::Hidden => Node::str("false"),
            Nameplate::Hover => Node::str("hover"),
        };
        self.map.put(&["metadata", "nameplate-visible"], v);
    }

    pub fn look_close(&self) -> bool {
        self.map
            .at(&["traits", "lookclose", "enabled"])
            .and_then(bool_of)
            .unwrap_or(false)
    }
    pub fn set_look_close(&mut self, on: bool) {
        self.map.put(&["traits", "lookclose", "enabled"], Node::bool(on));
        self.add_trait_name("lookclose");
    }
    /// The look-close range of this NPC, when it has its own.
    pub fn look_range(&self) -> Option<f64> {
        f64_at(&self.map, &["traits", "lookclose", "range"]).filter(|r| *r > 0.0)
    }
    pub fn set_look_range(&mut self, r: f64) {
        self.map.put(&["traits", "lookclose", "range"], Node::float(r));
    }
    pub fn random_look(&self) -> bool {
        self.map
            .at(&["traits", "lookclose", "enableRandomLook"])
            .and_then(bool_of)
            .unwrap_or(false)
    }
    pub fn set_random_look(&mut self, on: bool) {
        self.map
            .put(&["traits", "lookclose", "enableRandomLook"], Node::bool(on));
    }
    /// Sets `randomYawRange` (`yaw`) or `randomPitchRange` as (min, max).
    pub fn set_random_range(&mut self, yaw: bool, min: f64, max: f64) {
        let key = if yaw { "randomYawRange" } else { "randomPitchRange" };
        self.map.put(
            &["traits", "lookclose", key],
            Node::seq(vec![Node::float(min), Node::float(max)]),
        );
        self.add_trait_name("lookclose");
    }
    /// `randomYawRange` / `randomPitchRange` as (min, max).
    pub fn random_ranges(&self) -> ((f64, f64), (f64, f64)) {
        let range = |k: &str, d: (f64, f64)| {
            self.map
                .at(&["traits", "lookclose", k])
                .and_then(Node::as_seq)
                .and_then(|s| Some((s.first()?.as_f64()?, s.get(1)?.as_f64()?)))
                .unwrap_or(d)
        };
        (range("randomYawRange", (0.0, 360.0)), range("randomPitchRange", (-10.0, 0.0)))
    }

    /// `traits.gravity.enabled`: true means the NPC does not fall (Citizens' `/npc gravity`
    /// toggles it on to switch gravity off).
    pub fn no_gravity(&self) -> bool {
        self.map
            .at(&["traits", "gravity", "enabled"])
            .and_then(bool_of)
            .unwrap_or(false)
    }
    pub fn set_no_gravity(&mut self, on: bool) {
        self.map.put(&["traits", "gravity", "enabled"], Node::bool(on));
        self.add_trait_name("gravity");
    }

    pub fn owner(&self) -> Option<u128> {
        text_at(&self.map, &["traits", "owner", "uuid"]).and_then(|s| crate::ids::parse_uuid(&s))
    }
    /// Makes the server the owner (`/npc owner SERVER`): Citizens saves `owner: server` and no
    /// UUID.
    pub fn set_server_owner(&mut self) {
        self.map.drop_path(&["traits", "owner", "uuid"]);
        self.map.put(&["traits", "owner", "owner"], Node::str("server"));
        self.add_trait_name("owner");
    }
    /// A list Citizens saves as `0: ..., 1: ...` (or a sequence), as text.
    pub fn string_list(&self, path: &[&str]) -> Vec<String> {
        match self.map.at(path) {
            Some(m) if m.is_map() => m.keys().filter_map(|k| m.get(k).and_then(Node::text)).collect(),
            Some(s) if s.is_seq() => s.items().iter().filter_map(Node::text).collect(),
            _ => Vec::new(),
        }
    }
    /// Writes such a list as Citizens does (`0`, `1`, ...); an empty list removes it.
    pub fn set_string_list(&mut self, path: &[&str], items: &[String]) {
        if items.is_empty() {
            self.map.drop_path(path);
            return;
        }
        let mut m = Node::map();
        for (i, item) in items.iter().enumerate() {
            m.set(&i.to_string(), Node::str(item));
        }
        self.map.put(path, m);
    }
    /// A sound Citizens plays for the NPC (`metadata.ambient-sound`, `hurt-sound`, `death-sound`):
    /// none when unset or empty.
    pub fn sound(&self, kind: &str) -> Option<String> {
        text_at(&self.map, &["metadata", &format!("{kind}-sound")]).filter(|s| !s.is_empty())
    }
    /// `followtrait.followingUUID`.
    pub fn follow_target(&self) -> Option<u128> {
        text_at(&self.map, &["traits", "followtrait", "followingUUID"]).and_then(|s| crate::ids::parse_uuid(&s))
    }
    /// `followtrait.margin` (-1 when unset).
    pub fn follow_margin(&self) -> f64 {
        f64_at(&self.map, &["traits", "followtrait", "margin"]).unwrap_or(-1.0)
    }
    pub fn set_follow(&mut self, target: Option<u128>) {
        match target {
            Some(u) => self
                .map
                .put(&["traits", "followtrait", "followingUUID"], Node::str(&crate::ids::format_uuid(u))),
            None => {
                self.map.drop_path(&["traits", "followtrait", "followingUUID"]);
            }
        }
        self.add_trait_name("followtrait");
    }
    /// `navigator.speedmodifier` (1 when unset).
    pub fn speed_modifier(&self) -> f64 {
        f64_at(&self.map, &["navigator", "speedmodifier"]).unwrap_or(1.0)
    }
    /// The wander provider's (xrange, yrange, delay) when it is the NPC's waypoint provider.
    pub fn wander(&self) -> Option<(i64, i64, i64)> {
        if text_at(&self.map, &["traits", "waypoints", "provider"]).as_deref() != Some("wander") {
            return None;
        }
        let i = |k: &str, d: i64| {
            self.map
                .at(&["traits", "waypoints", "wander", k])
                .and_then(Node::as_i64)
                .unwrap_or(d)
        };
        Some((i("xrange", 25), i("yrange", 3), i("delay", -1)))
    }
    /// The pose to hold (`traits.poses.defaultPose`) as (yaw, pitch).
    pub fn default_pose(&self) -> Option<(f32, f32)> {
        let name = text_at(&self.map, &["traits", "poses", "defaultPose"])?.to_ascii_lowercase();
        self.string_list(&["traits", "poses", "list"]).iter().find_map(|p| {
            let mut it = p.split(';');
            let (n, pitch, yaw) = (it.next()?, it.next()?, it.next()?);
            (n.eq_ignore_ascii_case(&name)).then(|| Some((yaw.parse().ok()?, pitch.parse().ok()?)))?
        })
    }
    pub fn set_owner(&mut self, u: u128) {
        self.map
            .put(&["traits", "owner", "uuid"], Node::str(&crate::ids::format_uuid(u)));
        self.add_trait_name("owner");
    }

    pub fn hologram_lines(&self) -> Vec<HologramLine> {
        let Some(lines) = self
            .map
            .at(&["traits", "hologramtrait", "lines"])
            .and_then(Node::as_seq)
        else {
            return Vec::new();
        };
        lines
            .iter()
            .filter_map(|l| match l {
                m if m.is_map() => {
                    let flag = |k: &str, d: bool| {
                        m.at(&["renderer", k]).and_then(bool_of).unwrap_or(d)
                    };
                    Some(HologramLine {
                        text: m.get("text").and_then(Node::text)?,
                        margin_top: f64_at(m, &["margin", "top"]).unwrap_or(0.0),
                        margin_bottom: f64_at(m, &["margin", "bottom"]).unwrap_or(0.0),
                        billboard: text_at(m, &["renderer", "billboard"])
                            .unwrap_or_else(|| "CENTER".into()),
                        see_through: flag("seeThrough", false),
                        shadowed: flag("shadowed", true),
                    })
                }
                // Older saves keep plain strings.
                _ if matches!(l, Node::Scalar { .. }) => Some(HologramLine::plain(l.text()?)),
                _ => None,
            })
            .collect()
    }
    /// Line height in blocks (`lineHeight`, Citizens' -1 or missing: `default`).
    pub fn hologram_line_height(&self, default: f64) -> f64 {
        f64_at(&self.map, &["traits", "hologramtrait", "lineHeight"])
            .filter(|h| *h > 0.0)
            .unwrap_or(default)
    }
    fn hologram_seq(&mut self) -> &mut Vec<Node> {
        let t = self.map.map_at(&["traits", "hologramtrait"]);
        if !t.get("lines").is_some_and(Node::is_seq) {
            t.set("lines", Node::seq(Vec::new()));
        }
        match t.get_mut("lines") {
            Some(Node::Seq { items, .. }) => items,
            _ => unreachable!("just set to a sequence"),
        }
    }
    fn line_node(text: &str) -> Node {
        let mut renderer = Node::map();
        renderer.set("seeThrough", Node::bool(false));
        renderer.set("interpolationDelay", Node::int(0));
        renderer.set("shadowed", Node::bool(true));
        renderer.set("interpolationDuration", Node::int(0));
        renderer.set("billboard", Node::str("CENTER"));
        let mut margin = Node::map();
        margin.set("top", Node::float(0.0));
        margin.set("bottom", Node::float(0.0));
        let mut m = Node::map();
        m.set("renderer", renderer);
        m.set("margin", margin);
        m.set("text", Node::str(text));
        m
    }
    pub fn add_hologram_line(&mut self, text: &str) {
        self.hologram_seq().push(Self::line_node(text));
        self.add_trait_name("hologramtrait");
    }
    /// Changes line `i`'s text (keeping its margins and renderer); false past the end.
    pub fn set_hologram_line(&mut self, i: usize, text: &str) -> bool {
        let seq = self.hologram_seq();
        match seq.get_mut(i) {
            Some(m) if m.is_map() => {
                m.set("text", Node::str(text));
                true
            }
            Some(other) => {
                *other = Self::line_node(text);
                true
            }
            None => false,
        }
    }
    /// Inserts a line before index `i` (`i == len` appends); false past the end.
    pub fn insert_hologram_line(&mut self, i: usize, text: &str) -> bool {
        let seq = self.hologram_seq();
        if i > seq.len() {
            return false;
        }
        seq.insert(i, Self::line_node(text));
        self.add_trait_name("hologramtrait");
        true
    }
    /// Sets the top or bottom margin of line `i`; false past the end.
    pub fn set_hologram_margin(&mut self, i: usize, top: bool, margin: f64) -> bool {
        let seq = self.hologram_seq();
        match seq.get_mut(i) {
            Some(node) => {
                if !node.is_map() {
                    let text = node.text().unwrap_or_default();
                    *node = Self::line_node(&text);
                }
                if node.is_map() {
                    node.map_at(&["margin"]).set(if top { "top" } else { "bottom" }, Node::float(margin));
                }
                true
            }
            None => false,
        }
    }
    pub fn remove_hologram_line(&mut self, i: usize) -> bool {
        let seq = self.hologram_seq();
        if i < seq.len() {
            seq.remove(i);
            true
        } else {
            false
        }
    }
    pub fn clear_hologram(&mut self) {
        self.hologram_seq().clear();
    }

    pub fn commands(&self) -> Vec<NpcCommand> {
        let Some(node) = self.map.at(&["traits", "commandtrait", "commands"]) else {
            return Vec::new();
        };
        let mode = self
            .map
            .at(&["traits", "commandtrait", "executionmode"])
            .and_then(Node::text)
            .unwrap_or_default();
        let entry = |key: String, n: &Node| -> Option<NpcCommand> {
            let m = n.as_map()?;
            let set = |k: &str| match m.get(k) {
                None => false,
                Some(s) if s.is_seq() => !s.items().is_empty(),
                Some(x) if x.is_map() => !x.entries().is_empty(),
                Some(v) => {
                    let t = v.text().unwrap_or_default();
                    let t = t.trim();
                    !(t.is_empty() || t == "null" || t.parse::<f64>().is_ok_and(|x| x <= 0.0))
                }
            };
            let unenforced = if !mode.is_empty() && !mode.eq_ignore_ascii_case("LINEAR") {
                Some(format!("executionmode {mode}"))
            } else {
                // Citizens saves the permission list as `permissions`, the run-as-NPC flag as `npc`.
                ["permissions", "perms", "n", "gn", "npc", "cost", "cooldown", "globalcooldown", "delay", "experienceCost"]
                    .into_iter()
                    .find(|k| set(k))
                    .map(str::to_owned)
            };
            Some(NpcCommand {
                key,
                command: m.get("command").and_then(Node::text)?,
                hand: match m.get("hand").and_then(Node::text) {
                    None => Hand::Both,
                    Some(h) => Hand::parse(&h).unwrap_or_else(|| {
                        eprintln!("[citizens] unknown command hand {h:?}: the command never runs");
                        Hand::Unknown
                    }),
                },
                player: m.get("player").and_then(bool_of).unwrap_or(false),
                op: m.get("op").and_then(bool_of).unwrap_or(false),
                unenforced,
                cooldown: m.get("cooldown").and_then(Node::as_i64).unwrap_or(0),
                global_cooldown: m.get("globalcooldown").and_then(Node::as_i64).unwrap_or(0),
                delay: m.get("delay").and_then(Node::as_i64).unwrap_or(0),
                n: m.get("n").and_then(Node::as_i64).unwrap_or(-1),
                gn: m.get("gn").and_then(Node::as_i64).unwrap_or(-1),
                cost: m.get("cost").and_then(Node::as_f64).unwrap_or(-1.0),
                experience_cost: m.get("experienceCost").and_then(Node::as_i64).unwrap_or(-1),
            })
        };
        match node {
            m if m.is_map() => m
                .entries()
                .iter()
                .filter_map(|e| entry(e.key.clone(), &e.value))
                .collect(),
            s if s.is_seq() => s
                .items()
                .iter()
                .enumerate()
                .filter_map(|(i, v)| entry(i.to_string(), v))
                .collect(),
            _ => Vec::new(),
        }
    }
    /// Adds a command and returns its key. Kept in the map form when the NPC has it (or nothing),
    /// in the list form when that is what the file has.
    pub fn add_command(&mut self, command: &str, hand: Hand, player: bool, op: bool) -> String {
        let mut spec = CommandSpec::new(command, hand);
        spec.player = player;
        spec.op = op;
        self.add_command_spec(&spec)
    }
    /// Adds a command the way Citizens saves one: only the keys that are set, and the lowest
    /// unused number as its id.
    pub fn add_command_spec(&mut self, spec: &CommandSpec) -> String {
        let mut entry = Node::map();
        entry.set("command", Node::str(&spec.command));
        entry.set("hand", Node::str(spec.hand.name()));
        if spec.player {
            entry.set("player", Node::bool(true));
        }
        if spec.npc {
            entry.set("npc", Node::bool(true));
        }
        if spec.op {
            entry.set("op", Node::bool(true));
        }
        for (key, v) in [
            ("cooldown", spec.cooldown),
            ("globalcooldown", spec.global_cooldown),
            ("n", spec.n),
            ("gn", spec.gn),
            ("delay", spec.delay),
        ] {
            if v > 0 {
                entry.set(key, Node::int(v));
            }
        }
        if spec.cost != -1.0 {
            entry.set("cost", Node::float(spec.cost));
        }
        if spec.experience_cost != -1 {
            entry.set("experienceCost", Node::int(spec.experience_cost));
        }
        if !spec.permissions.is_empty() {
            let mut perms = Node::map();
            for (i, p) in spec.permissions.iter().enumerate() {
                perms.set(&i.to_string(), Node::str(p));
            }
            entry.set("permissions", perms);
        }
        self.add_trait_name("commandtrait");
        let t = self.map.map_at(&["traits", "commandtrait"]);
        match t.get_mut("commands") {
            Some(Node::Seq { items, .. }) => {
                items.push(entry);
                (items.len() - 1).to_string()
            }
            Some(m) if m.is_map() => {
                // Citizens' `getNewId`: the lowest id not in use.
                let used: Vec<i64> = m.keys().filter_map(|k| k.parse::<i64>().ok()).collect();
                let next = (0..).find(|i| !used.contains(i)).unwrap_or(0).to_string();
                m.set(&next, entry);
                next
            }
            _ => {
                let mut m = Node::map();
                // Citizens numbers commands from 0 (`[0]`, `[1]` in its list).
                m.set("0", entry);
                t.set("commands", m);
                "0".into()
            }
        }
    }
    pub fn remove_command(&mut self, key: &str) -> bool {
        let Some(node) = self.map.at(&["traits", "commandtrait", "commands"]).cloned() else {
            return false;
        };
        let t = self.map.map_at(&["traits", "commandtrait"]);
        let mut node = node;
        if node.is_map() {
            let removed = node.remove(key).is_some();
            t.set("commands", node);
            return removed;
        }
        match key.parse::<usize>() {
            Ok(i) if node.is_seq() && i < node.items().len() => {
                if let Node::Seq { items, .. } = &mut node {
                    items.remove(i);
                }
                t.set("commands", node);
                true
            }
            _ => false,
        }
    }
    pub fn clear_commands(&mut self) {
        self.map.drop_path(&["traits", "commandtrait", "commands"]);
    }

    pub fn equipment(&self) -> Vec<Equipped> {
        let mut out = Vec::new();
        for (citizens, slot) in EQUIPMENT_SLOTS {
            let Some(m) = self.map.at(&["traits", "equipment", citizens]) else {
                continue;
            };
            if let Some((item, count, components)) = stack_of(m) {
                out.push(Equipped {
                    slot,
                    item,
                    count,
                    components,
                });
            }
        }
        out
    }
    /// The Citizens inventory (`traits.inventory`, the items an NPC carries, not shown): what
    /// Sentinel's autoswitch reads. Empty stacks are skipped; `index` is the place in the list.
    pub fn inventory(&self) -> Vec<InventoryItem> {
        let Some(items) = self.map.at(&["traits", "inventory"]).and_then(Node::as_seq) else {
            return Vec::new();
        };
        items
            .iter()
            .enumerate()
            .filter_map(|(index, m)| {
                let (item, count, components) = stack_of(m)?;
                Some(InventoryItem {
                    index,
                    item,
                    count,
                    components,
                })
            })
            .collect()
    }
    /// The invisibility potion of the NPC (`traits.potioneffects.persistent.invisibility`).
    /// `duration` is in ticks, `None` when it lasts for good (negative or missing).
    pub fn invisibility(&self) -> Option<Effect> {
        let m = self.map.at(&["traits", "potioneffects", "persistent", "invisibility"])?;
        let duration = m.get("duration").and_then(Node::as_i64).filter(|d| *d > 0);
        Some(Effect {
            duration,
            amplifier: m.get("amplifier").and_then(Node::as_i64).unwrap_or(0).clamp(0, 255),
        })
    }
    /// Sets (or with `None` empties) one Citizens equipment slot (`hand`, `helmet`, ...).
    pub fn set_equipment(&mut self, citizens_slot: &str, item: Option<(&str, i64)>) {
        match item {
            Some((item, count)) => {
                let m = self.map.map_at(&["traits", "equipment", citizens_slot]);
                m.set("type", Node::str(&item.trim_start_matches("minecraft:").to_ascii_lowercase()));
                m.set("amount", Node::int(count));
                // The Java-serialised item meta of the old item does not describe the new one.
                m.remove("meta");
                m.remove("editable_components");
                self.add_trait_name("equipment");
            }
            None => {
                self.map.drop_path(&["traits", "equipment", citizens_slot]);
            }
        }
    }

    /// The linear waypoints (`traits.waypoints.linear.points[].location`).
    pub fn waypoints(&self) -> Vec<Location> {
        let Some(points) = self
            .map
            .at(&["traits", "waypoints", "linear", "points"])
            .and_then(Node::as_seq)
        else {
            return Vec::new();
        };
        points
            .iter()
            .filter_map(|p| {
                let l = p.as_map()?.get("location")?.as_map()?;
                Some(Location {
                    world: l.get("world").and_then(Node::text)?,
                    x: l.get("x").and_then(Node::as_f64)?,
                    y: l.get("y").and_then(Node::as_f64)?,
                    z: l.get("z").and_then(Node::as_f64)?,
                    yaw: l.get("yaw").and_then(Node::as_f64).unwrap_or(0.0) as f32,
                    pitch: l.get("pitch").and_then(Node::as_f64).unwrap_or(0.0) as f32,
                    body_yaw: 0.0,
                })
            })
            .collect()
    }
    /// Whether the waypoint provider is the linear one (Citizens' default when unset).
    pub fn linear_waypoints(&self) -> bool {
        text_at(&self.map, &["traits", "waypoints", "provider"]).is_none_or(|p| p == "linear")
    }

    /// The talk lines of the text trait (`traits.text.texts`), said in turn to a player who
    /// right-clicks the NPC.
    pub fn talk_lines(&self) -> Vec<String> {
        self.map
            .at(&["traits", "text", "texts"])
            .and_then(Node::as_seq)
            .map(|s| s.iter().filter_map(Node::text).collect())
            .unwrap_or_default()
    }
    /// Replaces the talk lines; none removes the trait.
    pub fn set_talk_lines(&mut self, lines: &[String]) {
        if lines.is_empty() {
            self.remove_trait("text");
            return;
        }
        let seq = lines.iter().map(|l| Node::str(l)).collect();
        self.map.put(&["traits", "text", "texts"], Node::seq(seq));
        self.add_trait_name("text");
    }

    fn waypoint_points(&self) -> Vec<Node> {
        self.map
            .at(&["traits", "waypoints", "linear", "points"])
            .and_then(Node::as_seq)
            .map(<[Node]>::to_vec)
            .unwrap_or_default()
    }
    fn store_waypoint_points(&mut self, points: Vec<Node>) {
        self.map.put(&["traits", "waypoints", "linear", "points"], Node::seq(points));
        self.add_trait_name("waypoints");
    }
    /// Appends a linear waypoint (`/npc path add`), in the layout `waypoints()` reads.
    pub fn add_waypoint(&mut self, loc: &Location) {
        let mut location = Node::map();
        if let Some(m) = location.as_map_mut() {
            m.set("world", Node::str(&loc.world));
            m.set("x", Node::float(loc.x));
            m.set("y", Node::float(loc.y));
            m.set("z", Node::float(loc.z));
            m.set("yaw", Node::float(f64::from(loc.yaw)));
            m.set("pitch", Node::float(f64::from(loc.pitch)));
        }
        let mut point = Node::map();
        if let Some(m) = point.as_map_mut() {
            m.set("location", location);
        }
        let mut points = self.waypoint_points();
        points.push(point);
        self.store_waypoint_points(points);
    }
    /// Removes the waypoint at `i` (0-based, as `/npc path list` numbers them).
    pub fn remove_waypoint(&mut self, i: usize) -> bool {
        let mut points = self.waypoint_points();
        if i >= points.len() {
            return false;
        }
        points.remove(i);
        self.store_waypoint_points(points);
        true
    }
    pub fn clear_waypoints(&mut self) {
        self.map.drop_path(&["traits", "waypoints", "linear", "points"]);
    }

    /// The raw data of a trait this mod does not interpret (`sentinel`), for other mods.
    pub fn trait_data(&self, name: &str) -> Option<&Node> {
        self.map.at(&["traits", name])
    }
    pub fn set_trait_data(&mut self, name: &str, data: Node) {
        self.map.put(&["traits", name], data);
        self.add_trait_name(name);
    }
    pub fn remove_trait(&mut self, name: &str) {
        self.map.drop_path(&["traits", name]);
        let names: Vec<String> = self.trait_names().into_iter().filter(|n| n != name).collect();
        self.map.set("traitnames", Node::str(&names.join(",")));
    }
}

fn round4(v: f64) -> f64 {
    (v * 10_000.0).round() / 10_000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::yaml;

    fn sample() -> NpcData {
        let m = yaml::parse(
            "metadata:
  protected: 'False'
  nameplate-visible: hover
traits:
  location:
    world: world
    worldid: 00000000-0000-4000-8000-000000000001
    x: 1.5
    y: 64.0
    z: -2.5
    pitch: 0.0
    bodyYaw: 90.0
    yaw: 90.0
  type: PLAYER
  commandtrait:
    commands:
    - command: say one
      hand: RIGHT
      player: false
    - command: say two
      hand: LEFT
      player: true
  equipment:
    hand:
      type: diamond_sword
      amount: 1
      meta: rO0AB
    boots:
      type: AIR
      amount: 1
  sentinel:
    respawnTime: 100
traitnames: type,location,sentinel
name: '&aGuide'
uuid: 00000000-0000-4000-8000-0000000000aa
",
        )
        .unwrap();
        NpcData { id: 3, map: m }
    }

    #[test]
    fn reads_the_traits() {
        let n = sample();
        assert_eq!(n.name(), "&aGuide");
        assert!(!n.protected());
        assert_eq!(n.nameplate(), Nameplate::Hover);
        assert!(n.is_player());
        assert!(n.spawned());
        let loc = n.location().unwrap();
        assert_eq!((loc.x, loc.y, loc.z, loc.yaw), (1.5, 64.0, -2.5, 90.0));
        let cmds = n.commands();
        assert_eq!(cmds.len(), 2);
        assert_eq!(cmds[1].hand, Hand::Left);
        assert!(cmds[1].player);
        let eq = n.equipment();
        assert_eq!(eq.len(), 1);
        assert_eq!(eq[0].slot, "mainhand");
        assert_eq!(n.trait_data("sentinel").unwrap().as_map().unwrap().get("respawnTime").unwrap().as_i64(), Some(100));
    }

    #[test]
    fn changes_keep_the_rest() {
        let mut n = sample();
        let before = yaml::write(&n.map);
        n.set_protected(true);
        n.add_hologram_line("&a&lOPEN");
        n.set_hologram_line(0, "&c&lCLOSED");
        let key = n.add_command("say three", Hand::Both, false, false);
        assert_eq!(key, "2");
        assert!(n.remove_command("0"));
        n.set_location(&Location {
            world: "world".into(),
            x: 10.123456,
            y: 65.0,
            z: 3.0,
            yaw: 45.0,
            pitch: 0.0,
            body_yaw: 45.0,
        });
        let after = yaml::write(&n.map);
        assert!(after.contains("worldid"), "same world keeps its id");
        assert!(after.contains("x: 10.1235"));
        assert!(after.contains("text: '&c&lCLOSED'"));
        assert!(after.contains("respawnTime: 100"));
        assert_ne!(before, after);
        assert_eq!(n.commands().len(), 2);
        assert_eq!(n.hologram_lines()[0].text, "&c&lCLOSED");
        assert!(n.trait_names().contains(&"hologramtrait".to_string()));
    }

    #[test]
    fn command_map_form() {
        let mut n = NpcData::new(1, 7, "A", "PLAYER");
        // Citizens numbers from 0 (review C5).
        assert_eq!(n.add_command("say a", Hand::Right, true, false), "0");
        assert_eq!(n.add_command("say b", Hand::Right, true, false), "1");
        assert!(n.remove_command("0"));
        assert_eq!(n.commands()[0].command, "say b");
        assert!(!n.remove_command("9"));
    }

    #[test]
    fn new_npc_defaults() {
        let n = NpcData::new(5, 0xabc, "Bob", "PLAYER");
        assert!(n.protected());
        assert!(n.spawned());
        assert!(!n.look_close());
        assert_eq!(n.uuid(), Some(0xabc));
        assert_eq!(n.nameplate(), Nameplate::Shown);
        n.skin();
    }

    #[test]
    fn shift_hands_only_on_sneaking_clicks() {
        // Review C3: SHIFT_LEFT / SHIFT_RIGHT were read as BOTH and ran on every click.
        assert_eq!(Hand::parse("shift_right"), Some(Hand::ShiftRight));
        assert_eq!(Hand::parse("SHIFT_LEFT"), Some(Hand::ShiftLeft));
        assert!(!Hand::ShiftRight.matches(Hand::Right, false));
        assert!(Hand::ShiftRight.matches(Hand::Right, true));
        assert!(!Hand::ShiftRight.matches(Hand::Left, true));
        assert!(!Hand::ShiftLeft.matches(Hand::Left, false));
        assert!(Hand::ShiftLeft.matches(Hand::Left, true));
        assert!(Hand::Right.matches(Hand::Right, true));
        assert!(!Hand::Right.matches(Hand::Left, false));
        assert!(Hand::Both.matches(Hand::Left, false));
        assert!(!Hand::Unknown.matches(Hand::Right, true));
        assert_eq!(Hand::ShiftRight.name(), "SHIFT_RIGHT");
    }

    #[test]
    fn unknown_hand_runs_on_nothing() {
        let mut n = NpcData::new(1, 7, "A", "PLAYER");
        n.add_command("say a", Hand::Right, false, false);
        let mut cmds = n.map.map_at(&["traits", "commandtrait", "commands"]).clone();
        if let Some(e) = cmds.get_mut("0").filter(|e| e.is_map()) {
            e.set("hand", Node::str("MIDDLE"));
        }
        n.map.map_at(&["traits", "commandtrait"]).set("commands", cmds);
        let c = n.commands();
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].hand, Hand::Unknown);
    }

    #[test]
    fn nameplate_written_as_string() {
        // Review C7: Citizens keeps `nameplate-visible` as a string.
        let mut n = NpcData::new(1, 7, "A", "PLAYER");
        n.set_nameplate(Nameplate::Hidden);
        let out = yaml::write(&n.map);
        assert!(out.contains("nameplate-visible: 'false'"), "{out}");
        assert_eq!(n.nameplate(), Nameplate::Hidden);
        n.set_nameplate(Nameplate::Shown);
        assert_eq!(n.nameplate(), Nameplate::Shown);
    }

    #[test]
    fn limits_the_port_cannot_enforce_are_flagged() {
        // Review C4: a command limited by permission or uses must not run for everyone.
        let mut n = NpcData::new(1, 7, "A", "PLAYER");
        n.add_command("say a", Hand::Right, false, false);
        n.add_command("say b", Hand::Right, false, false);
        let mut cmds = n.map.map_at(&["traits", "commandtrait", "commands"]).clone();
        if let Some(e) = cmds.get_mut("1").filter(|e| e.is_map()) {
            e.set("perms", Node::seq(vec![Node::str("some.node")]));
            e.set("n", Node::int(-1));
        }
        n.map.map_at(&["traits", "commandtrait"]).set("commands", cmds);
        let c = n.commands();
        assert_eq!(c[0].unenforced, None);
        assert_eq!(c[1].unenforced.as_deref(), Some("perms"));
        n.map
            .map_at(&["traits", "commandtrait"])
            .set("executionmode", Node::str("SEQUENTIAL"));
        assert!(n.commands().iter().all(|c| c.unenforced.is_some()));
        n.map.map_at(&["traits", "commandtrait"]).set("executionmode", Node::str("LINEAR"));
        assert_eq!(n.commands()[0].unenforced, None);
    }
}

#[cfg(test)]
mod citizens_item_tests {
    use super::*;
    use crate::config::Config;
    use crate::spawn;
    use crate::yaml;

    /// An NPC with a decorated sword in the hand, a potion of invisibility and two stacks in its
    /// inventory (invented values; the blobs are synthetic).
    fn carrier() -> NpcData {
        let map = yaml::parse(&format!(
            "name: Guard
traitnames: equipment,inventory,potioneffects
traits:
  type: PLAYER
  location:
    world: stage
    x: 1.0
    y: 64.0
    z: 2.0
    yaw: 0.0
    pitch: 0.0
  equipment:
    hand:
      type: DIAMOND_SWORD
      amount: 1
      meta: '{BLADE}'
    helmet:
      type: LEATHER_HELMET
      amount: 1
      durability: 9
    cosmetic_chestplate:
      type: air
  inventory:
    - type: GOLDEN_APPLE
      amount: 3
    - type: AIR
      amount: 1
    - type: BOW
      amount: 1
      meta: '{POTION}'
  potioneffects:
    persistent:
      invisibility:
        type: INVISIBILITY
        duration: -1
        amplifier: 0
        ambient: false
        particles: false
        icon: false
",
            BLADE = crate::items::fixtures::BLADE,
            POTION = crate::items::fixtures::POTION,
        ))
        .unwrap();
        NpcData { id: 3, map }
    }

    #[test]
    fn equipment_carries_the_decoded_components() {
        let n = carrier();
        let eq = n.equipment();
        assert_eq!(eq.len(), 2, "air and cosmetic slots are not shown");
        let hand = eq.iter().find(|e| e.slot == "mainhand").unwrap();
        assert_eq!(hand.item, "diamond_sword");
        assert_eq!(hand.count, 1);
        assert!(hand.components.iter().any(|c| c.starts_with("\"minecraft:custom_name\"")));
        assert!(hand.components.iter().any(|c| c.contains("minecraft:sharpness")));
        let head = eq.iter().find(|e| e.slot == "head").unwrap();
        assert_eq!(head.components, vec!["\"minecraft:damage\":9".to_owned()], "durability is the damage");
    }

    #[test]
    fn inventory_lists_the_stacks_in_order() {
        let inv = carrier().inventory();
        assert_eq!(inv.len(), 2, "the air stack is skipped");
        assert_eq!((inv[0].index, inv[0].item.as_str(), inv[0].count), (0, "golden_apple", 3));
        assert!(inv[0].components.is_empty());
        assert_eq!((inv[1].index, inv[1].item.as_str()), (2, "bow"));
        assert!(inv[1].components.iter().any(|c| c.contains("minecraft:potion_contents")));
    }

    #[test]
    fn invisibility_is_read_and_summoned_as_an_effect() {
        let n = carrier();
        let e = n.invisibility().expect("invisible");
        assert_eq!(e, Effect { duration: None, amplifier: 0 }, "-1 is for good");
        let loc = Location { world: "stage".into(), x: 1.0, y: 64.0, z: 2.0, yaw: 0.0, pitch: 0.0, body_yaw: 0.0 };
        let snbt = spawn::npc_snbt(&n, 7, &loc, &Config::default());
        assert!(
            snbt.contains("ActiveEffects:[{id:\"minecraft:invisibility\",amplifier:0b,duration:-1,"),
            "{snbt}"
        );
        assert!(snbt.contains("hand:{id:\"minecraft:diamond_sword\",count:1,components:{"), "{snbt}");
        assert!(!snbt.contains("cosmetic"), "cosmetic slots are not summoned");
    }

    #[test]
    fn a_timed_invisibility_keeps_its_ticks() {
        let mut n = carrier();
        n.map.put(&["traits", "potioneffects", "persistent", "invisibility", "duration"], Node::int(200));
        assert_eq!(n.invisibility().unwrap().duration, Some(200));
    }

    #[test]
    fn a_plain_npc_has_no_effect_and_no_inventory() {
        let n = NpcData::new(1, 2, "Plain", "PLAYER");
        assert!(n.invisibility().is_none());
        assert!(n.inventory().is_empty());
    }

    #[test]
    fn a_left_click_command_does_not_run_on_a_right_click() {
        // Commandtrait `hand` is an exact match (Citizens' `ClickType`), `player` only sets who runs it.
        let mut n = NpcData::new(1, 2, "Hands", "PLAYER");
        n.add_command("say left", Hand::Left, true, false);
        n.add_command("say right", Hand::Right, false, false);
        let cmds = n.commands();
        let left: Vec<_> = cmds.iter().filter(|c| c.hand.matches(Hand::Left, false)).map(|c| c.command.clone()).collect();
        let right: Vec<_> = cmds.iter().filter(|c| c.hand.matches(Hand::Right, false)).map(|c| c.command.clone()).collect();
        assert_eq!(left, ["say left"]);
        assert_eq!(right, ["say right"]);
        assert!(cmds.iter().any(|c| c.hand == Hand::Left && c.player));
    }

    #[test]
    fn the_read_paths_name_the_keys_the_port_reads() {
        assert!(NPC_READ_PATHS.contains(&"traits.potioneffects.persistent.invisibility.**"));
        assert!(NPC_READ_PATHS.contains(&"traits.equipment.*.meta"));
    }
}
