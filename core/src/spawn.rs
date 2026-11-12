//! The console commands that put an NPC into the world and change it there.
//!
//! A player NPC is a `minecraft:mannequin` (the game's own player-shaped entity, which wears a
//! game profile's skin and is not in the Tab list); other types are that mob with no AI. Each
//! gets a fixed UUID (the NPC's), so every later command names it by UUID, and the tags
//! `citizens_npc` and `citizens_npc_<id>`. Hologram lines are `text_display`s with UUIDs derived
//! from the NPC's.

use crate::config::Config;
use crate::ids::{derived_uuid, format_uuid, int_array_snbt};
use crate::npc::{Location, Nameplate, NpcData, Skin};
use crate::text;
use crate::tree::TreeExt;
use crate::yaml::Node;

pub const TAG: &str = "citizens_npc";
pub const HOLOGRAM_TAG: &str = "citizens_hologram";

/// A game profile for a mannequin: the skin's signed texture, or a name for the game to look up.
#[derive(Clone, Debug, PartialEq)]
pub enum Profile {
    Textured {
        id: u128,
        name: String,
        texture: String,
        signature: Option<String>,
    },
    Named(String),
    None,
}

pub fn profile_for(npc: &NpcData, uuid: u128, cfg: &Config) -> Profile {
    let skin: Skin = npc.skin();
    let name = text::profile_name(&npc.name(), &format!("npc{}", npc.id));
    if let Some(texture) = skin.texture.clone().filter(|t| !t.is_empty()) {
        return Profile::Textured {
            id: uuid,
            name,
            texture,
            signature: skin.signature.clone(),
        };
    }
    if let Some(n) = skin.name.filter(|n| !n.is_empty()) {
        return Profile::Named(text::profile_name(&n, &name));
    }
    if cfg.skins.try_fetch_default_skin {
        Profile::Named(name)
    } else {
        Profile::None
    }
}

fn quoted(s: &str) -> String {
    serde_json::Value::String(s.to_owned()).to_string()
}

fn profile_snbt(p: &Profile) -> Option<String> {
    match p {
        Profile::Textured {
            id,
            name,
            texture,
            signature,
        } => {
            let sig = signature
                .as_ref()
                .map(|s| format!(",signature:{}", quoted(s)))
                .unwrap_or_default();
            Some(format!(
                "{{id:{},name:{},properties:[{{name:\"textures\",value:{}{sig}}}]}}",
                int_array_snbt(*id),
                quoted(name),
                quoted(texture)
            ))
        }
        Profile::Named(n) => Some(format!("{{name:{}}}", quoted(n))),
        Profile::None => None,
    }
}

/// `minecraft:mannequin` for `PLAYER`, else the Bukkit type in lower case (`SNOW_GOLEM` ->
/// `minecraft:snow_golem`), with Bukkit's older names mapped.
pub fn entity_kind(bukkit: &str) -> String {
    let b = bukkit.to_ascii_lowercase();
    let id = match b.as_str() {
        "player" => "mannequin",
        "snowman" => "snow_golem",
        "mushroom_cow" => "mooshroom",
        "pig_zombie" => "zombified_piglin",
        "iron_golem" | "villager_golem" => "iron_golem",
        other => other,
    };
    if id.contains(':') { id.to_owned() } else { format!("minecraft:{id}") }
}

fn num(v: f64) -> String {
    let s = format!("{:.4}", v);
    let s = s.trim_end_matches('0');
    let s = s.strip_suffix('.').unwrap_or(s);
    if s == "-0" { "0".into() } else { s.to_owned() }
}

/// The entity data of the NPC's entity, as `/summon` takes it.
pub fn npc_snbt(npc: &NpcData, uuid: u128, loc: &Location, cfg: &Config) -> String {
    let kind = entity_kind(&npc.kind());
    let mut parts: Vec<String> = vec![
        format!("UUID:{}", int_array_snbt(uuid)),
        format!("Tags:[\"{TAG}\",\"{TAG}_{}\"]", npc.id),
        format!("Rotation:[{}f,{}f]", num(f64::from(loc.yaw)), num(f64::from(loc.pitch))),
        "PersistenceRequired:1b".into(),
        "Silent:1b".into(),
    ];
    if npc.protected() {
        parts.push("Invulnerable:1b".into());
    }
    if npc.no_gravity() {
        parts.push("NoGravity:1b".into());
    }
    if npc.glowing() {
        parts.push("Glowing:1b".into());
    }
    match nameplate(npc) {
        Nameplate::Shown => {
            parts.push(format!("CustomName:{}", text::component_snbt(&npc.name())));
            parts.push("CustomNameVisible:1b".into());
        }
        Nameplate::Hover => parts.push(format!("CustomName:{}", text::component_snbt(&npc.name()))),
        Nameplate::Hidden => {}
    }
    if kind == "minecraft:mannequin" {
        if let Some(p) = profile_snbt(&profile_for(npc, uuid, cfg)) {
            parts.push(format!("profile:{p}"));
        }
        parts.push("immovable:1b".into());
        parts.push("hide_description:1b".into());
    } else {
        parts.push("NoAI:1b".into());
    }
    // Traits Citizens saves that show in the entity's own data (`/npc age|powered|sheep|slimesize`).
    let tp = |trait_: &str, key: &str| npc.map.at(&["traits", trait_, key]);
    if let Some(age) = tp("age", "age").and_then(Node::as_i64).filter(|a| *a < 0) {
        parts.push(format!("Age:{age}"));
        parts.push("IsBaby:1b".into());
    }
    if kind == "minecraft:creeper" && npc.map.at(&["traits", "powered"]).and_then(Node::as_bool) == Some(true) {
        parts.push("powered:1b".into());
    }
    if kind == "minecraft:sheep" {
        let colour = tp("sheeptrait", "color").and_then(Node::text).unwrap_or_default();
        if let Some(i) = crate::commands::DYE_COLOURS.iter().position(|c| c.eq_ignore_ascii_case(&colour)) {
            parts.push(format!("Color:{i}b"));
        }
        if tp("sheeptrait", "sheared").and_then(Node::as_bool) == Some(true) {
            parts.push("Sheared:1b".into());
        }
    }
    if kind == "minecraft:slime" || kind == "minecraft:magma_cube" {
        if let Some(size) = tp("slimesize", "size").and_then(Node::as_i64) {
            parts.push(format!("Size:{}", (size - 1).max(0)));
        }
    }
    if let Some(effect) = npc.invisibility() {
        // Citizens' persistent invisibility potion, as the entity's own effect.
        let duration = effect.duration.map_or(-1, |d| d.min(i64::from(i32::MAX)));
        parts.push(format!(
            "ActiveEffects:[{{id:\"minecraft:invisibility\",amplifier:{}b,duration:{duration},show_particles:0b,show_icon:0b,ambient:0b}}]",
            effect.amplifier
        ));
    }
    let equipment = npc.equipment();
    if !equipment.is_empty() {
        let items: Vec<String> = equipment
            .iter()
            .map(|e| {
                let components = if e.components.is_empty() {
                    String::new()
                } else {
                    format!(",components:{{{}}}", e.components.join(","))
                };
                format!(
                    "{}:{{id:\"minecraft:{}\",count:{}{components}}}",
                    e.slot,
                    e.item.trim_start_matches("minecraft:"),
                    e.count
                )
            })
            .collect();
        let chances: Vec<String> = equipment.iter().map(|e| format!("{}:0.0f", e.slot)).collect();
        parts.push(format!("equipment:{{{}}}", items.join(",")));
        parts.push(format!("drop_chances:{{{}}}", chances.join(",")));
    }
    format!("{{{}}}", parts.join(","))
}

fn in_dim(dimension: &str, command: &str) -> String {
    format!("execute in {dimension} run {command}")
}

/// Summons the NPC.
pub fn summon(npc: &NpcData, uuid: u128, dimension: &str, loc: &Location, cfg: &Config) -> String {
    in_dim(
        dimension,
        &format!(
            "summon {} {} {} {} {}",
            entity_kind(&npc.kind()),
            num(loc.x),
            num(loc.y),
            num(loc.z),
            npc_snbt(npc, uuid, loc, cfg)
        ),
    )
}

/// Height of a player NPC: where Citizens puts a name hologram.
const PLAYER_HEIGHT: f64 = 1.8;
/// Which derived UUID the name hologram takes (the lines take 0, 1, ...).
const NAME_HOLOGRAM: u32 = 0xFFFF;

/// Whether the NPC's name is shown as a hologram instead of its name tag: a player NPC whose
/// name (in its saved colour-code form) is longer than a game profile name may be (16). Seen on
/// Paper: such an NPC's own name tag is hidden and a text display
/// with the name sits at its height, raised 0.2 by the display's translation.
pub fn name_hologram(npc: &NpcData) -> bool {
    npc.kind().eq_ignore_ascii_case("PLAYER")
        && npc.nameplate() != Nameplate::Hidden
        && npc.name().chars().count() > 16
}

/// The name tag the entity itself shows (hidden while a name hologram shows the name).
fn nameplate(npc: &NpcData) -> Nameplate {
    if name_hologram(npc) {
        Nameplate::Hidden
    } else {
        npc.nameplate()
    }
}

/// The hologram lines' positions and summon commands, top line first; the name hologram (when
/// there is one) last.
pub fn hologram(npc: &NpcData, uuid: u128, dimension: &str, loc: &Location, cfg: &Config) -> Vec<(u128, String)> {
    let lines = npc.hologram_lines();
    let height = npc.hologram_line_height(cfg.hologram.default_line_height);
    let mut y = loc.y + cfg.hologram.base_height + height;
    let mut out = Vec::with_capacity(lines.len() + 1);
    // Bottom up: the first line sits lowest (Citizens on Paper).
    for (i, line) in lines.iter().enumerate() {
        y += line.margin_bottom;
        let id = derived_uuid(uuid, i as u32);
        let billboard = line.billboard.to_ascii_lowercase();
        let billboard = match billboard.as_str() {
            "fixed" | "vertical" | "horizontal" | "center" => billboard,
            _ => "center".into(),
        };
        let data = format!(
            "{{UUID:{},Tags:[\"{HOLOGRAM_TAG}\",\"{TAG}_{}\"],text:{},billboard:\"{billboard}\",see_through:{}b,shadow:{}b,alignment:\"center\"}}",
            int_array_snbt(id),
            npc.id,
            text::component_snbt(&line.text),
            u8::from(line.see_through),
            u8::from(line.shadowed),
        );
        out.push((
            id,
            in_dim(
                dimension,
                &format!("summon minecraft:text_display {} {} {} {data}", num(loc.x), num(y), num(loc.z)),
            ),
        ));
        y += height + line.margin_top;
    }
    out.reverse();
    if name_hologram(npc) {
        let id = derived_uuid(uuid, NAME_HOLOGRAM);
        let data = format!(
            "{{UUID:{},Tags:[\"{HOLOGRAM_TAG}\",\"{TAG}_{}\"],text:{},billboard:\"center\",see_through:0b,shadow:1b,alignment:\"center\",transformation:{{translation:[0f,0.2f,0f],left_rotation:[0f,0f,0f,1f],right_rotation:[0f,0f,0f,1f],scale:[1f,1f,1f]}}}}",
            int_array_snbt(id),
            npc.id,
            text::component_snbt(&npc.name()),
        );
        out.push((
            id,
            in_dim(
                dimension,
                &format!(
                    "summon minecraft:text_display {} {} {} {data}",
                    num(loc.x),
                    num(loc.y + PLAYER_HEIGHT),
                    num(loc.z)
                ),
            ),
        ));
    }
    out
}

/// Removes an entity by UUID without a death (`kill` on a display or marker; NPC entities the
/// mod knows the id of are removed with `remove_mob` instead).
pub fn kill(uuid: u128) -> String {
    format!("kill {}", format_uuid(uuid))
}

pub fn teleport(uuid: u128, dimension: &str, loc: &Location) -> String {
    in_dim(
        dimension,
        &format!(
            "tp {} {} {} {} {} {}",
            format_uuid(uuid),
            num(loc.x),
            num(loc.y),
            num(loc.z),
            num(f64::from(loc.yaw)),
            num(f64::from(loc.pitch))
        ),
    )
}

/// Block groups and their step sound, for a walking NPC's footstep (`Entity.playStepSound`: the
/// block's sound type at 15 % volume). The block under the feet (y - 0.2) picks the sound;
/// anything else gets stone's, air included (air's sound type is stone: a walker
/// walks on air at y 44 and Paper played 41 stone steps in 30 s).
const STEP_SOUNDS: &[(&str, &str)] = &[
    ("#minecraft:wool", "block.wool.step"),
    ("#minecraft:wool_carpets", "block.wool.step"),
    ("minecraft:grass_block", "block.grass.step"),
    ("#minecraft:leaves", "block.grass.step"),
    ("#minecraft:planks", "block.wood.step"),
    ("#minecraft:logs", "block.wood.step"),
    ("#minecraft:wooden_slabs", "block.wood.step"),
    ("#minecraft:wooden_stairs", "block.wood.step"),
    ("#minecraft:sand", "block.sand.step"),
    ("minecraft:gravel", "block.gravel.step"),
    ("minecraft:dirt", "block.gravel.step"),
    ("minecraft:snow_block", "block.snow.step"),
    ("minecraft:snow", "block.snow.step"),
];

/// The commands that sound one footstep of an NPC at `loc` (players hear it as a player's step).
pub fn step_sound(dimension: &str, loc: &Location) -> Vec<String> {
    let at = format!("{} {} {}", num(loc.x), num(loc.y), num(loc.z));
    let below = format!("{} {} {}", num(loc.x), num(loc.y - 0.2), num(loc.z));
    let play = |sound: &str| format!("playsound minecraft:{sound} player @a {at} 0.15 1");
    let mut out: Vec<String> = STEP_SOUNDS
        .iter()
        .map(|(block, sound)| in_dim(dimension, &format!("execute if block {below} {block} run {}", play(sound))))
        .collect();
    let others: String = STEP_SOUNDS
        .iter()
        .map(|(block, _)| format!("unless block {below} {block} "))
        .collect();
    out.push(in_dim(dimension, &format!("execute {others}run {}", play("block.stone.step"))));
    out
}

/// Turns the entity (head and body) without moving it.
pub fn rotate(uuid: u128, dimension: &str, yaw: f32, pitch: f32) -> String {
    in_dim(
        dimension,
        &format!("rotate {} {} {}", format_uuid(uuid), num(f64::from(yaw)), num(f64::from(pitch))),
    )
}

/// Swings the NPC's main hand (`/swing`), for other mods (an attack).
pub fn swing(uuid: u128, dimension: &str) -> String {
    in_dim(dimension, &format!("swing {} mainhand", format_uuid(uuid)))
}

/// The scoreboard team of a glow colour (Citizens' `scoreboardtrait`), and the commands that make it.
pub fn team_name(color: &str) -> String {
    format!("citizens_{}", color.to_ascii_lowercase())
}

pub fn team_setup(color: &str) -> Vec<String> {
    let team = team_name(color);
    let c = text::chat_color(color).unwrap_or("white");
    vec![
        format!("team add {team}"),
        format!("team modify {team} color {c}"),
        format!("team modify {team} collisionRule never"),
    ]
}

pub fn team_join(color: &str, uuid: u128) -> String {
    format!("team join {} {}", team_name(color), format_uuid(uuid))
}

/// `data merge` of the flags that change in place (glow, protection, gravity, name).
pub fn refresh(npc: &NpcData, uuid: u128, dimension: &str) -> String {
    let (name, visible) = match nameplate(npc) {
        Nameplate::Shown => (text::component_snbt(&npc.name()), 1),
        Nameplate::Hover => (text::component_snbt(&npc.name()), 0),
        Nameplate::Hidden => ("\"\"".into(), 0),
    };
    in_dim(
        dimension,
        &format!(
            "data merge entity {} {{Glowing:{}b,Invulnerable:{}b,NoGravity:{}b,CustomName:{name},CustomNameVisible:{visible}b}}",
            format_uuid(uuid),
            u8::from(npc.glowing()),
            u8::from(npc.protected()),
            u8::from(npc.no_gravity()),
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::npc::Hand;

    fn npc() -> NpcData {
        let mut n = NpcData::new(4, 0x11, "&6Guide", "PLAYER");
        n.set_location(&Location {
            world: "world".into(),
            x: 100.5,
            y: 46.0,
            z: -62.25,
            yaw: 90.0,
            pitch: 5.5,
            body_yaw: 90.0,
        });
        n.set_skin(&Skin {
            name: None,
            texture: Some("dGV4dHVyZQ==".into()),
            signature: Some("c2ln".into()),
        });
        n
    }

    #[test]
    fn a_player_npc_is_a_mannequin_with_its_skin() {
        let n = npc();
        let loc = n.location().unwrap();
        let cmd = summon(&n, 0x11, "minecraft:overworld", &loc, &Config::default());
        assert!(cmd.starts_with(
            "execute in minecraft:overworld run summon minecraft:mannequin 100.5 46 -62.25 {UUID:[I;0,0,0,17],"
        ), "{cmd}");
        assert!(cmd.contains("Tags:[\"citizens_npc\",\"citizens_npc_4\"]"));
        assert!(cmd.contains("Rotation:[90f,5.5f]"));
        assert!(cmd.contains("Invulnerable:1b"));
        assert!(cmd.contains(
            "profile:{id:[I;0,0,0,17],name:\"Guide\",properties:[{name:\"textures\",value:\"dGV4dHVyZQ==\",signature:\"c2ln\"}]}"
        ));
        assert!(cmd.contains("CustomName:{\"color\":\"gold\",\"text\":\"Guide\"}") || cmd.contains("CustomName:{\"text\":\"Guide\",\"color\":\"gold\"}"), "{cmd}");
        assert!(cmd.contains("CustomNameVisible:1b"));
        assert!(cmd.contains("immovable:1b,hide_description:1b"));
        assert!(!cmd.contains("NoAI"));
    }

    #[test]
    fn a_name_alone_is_looked_up_by_the_game() {
        let mut n = NpcData::new(1, 2, "Bob", "PLAYER");
        assert_eq!(profile_for(&n, 2, &Config::default()), Profile::Named("Bob".into()));
        n.set_skin(&Skin {
            name: Some("Steve".into()),
            ..Skin::default()
        });
        assert_eq!(profile_for(&n, 2, &Config::default()), Profile::Named("Steve".into()));
        let mut cfg = Config::default();
        cfg.skins.try_fetch_default_skin = false;
        let n = NpcData::new(1, 2, "Bob", "PLAYER");
        assert_eq!(profile_for(&n, 2, &cfg), Profile::None);
    }

    #[test]
    fn other_types_have_no_ai_and_equipment_does_not_drop() {
        let mut n = NpcData::new(9, 5, "Frosty", "SNOW_GOLEM");
        n.set_protected(false);
        n.set_nameplate(Nameplate::Hidden);
        n.set_equipment("helmet", Some(("carved_pumpkin", 1)));
        n.add_command("say hi", Hand::Right, false, false);
        let loc = Location {
            world: "world".into(),
            x: 0.0,
            y: 64.0,
            z: 0.0,
            yaw: 0.0,
            pitch: 0.0,
            body_yaw: 0.0,
        };
        let s = npc_snbt(&n, 5, &loc, &Config::default());
        assert!(s.contains("NoAI:1b"));
        assert!(!s.contains("Invulnerable"));
        assert!(!s.contains("CustomName"));
        assert!(s.contains("equipment:{head:{id:\"minecraft:carved_pumpkin\",count:1}}"));
        assert!(s.contains("drop_chances:{head:0.0f}"));
        assert_eq!(entity_kind("SNOW_GOLEM"), "minecraft:snow_golem");
        assert_eq!(entity_kind("SNOWMAN"), "minecraft:snow_golem");
    }

    #[test]
    fn hologram_lines_stack_up() {
        let mut n = npc();
        n.add_hologram_line("&a&lOPEN");
        n.add_hologram_line("second");
        let loc = n.location().unwrap();
        let h = hologram(&n, 0x11, "minecraft:overworld", &loc, &Config::default());
        assert_eq!(h.len(), 2);
        // Paper's Citizens: 1.8 + 0.2 + 0.4 and one line height more.
        // The first line is the lowest.
        assert!(h[1].1.contains("text_display 100.5 48.4 -62.25"), "{}", h[1].1);
        assert!(h[0].1.contains("text_display 100.5 48.8 -62.25"), "{}", h[0].1);
        assert!(h[1].1.contains("\"bold\":true"));
        assert_ne!(h[0].0, h[1].0);
    }

    /// a "&f&lExample (Private)": the name over 16 characters is a hologram at the
    /// NPC's height with translation 0.2, the entity's own name tag is hidden, and a line of
    /// height 0.25 sits at 2.25 (a long-named NPC).
    #[test]
    fn a_long_name_is_a_hologram() {
        let mut n = NpcData::new(4, 0x11, "&f&lExample (Private)", "PLAYER");
        n.set_location(&npc().location().unwrap());
        n.add_hologram_line("&c&lCLOSED");
        n.map.put(&["traits", "hologramtrait", "lineHeight"], Node::float(0.25));
        let loc = n.location().unwrap();
        let h = hologram(&n, 0x11, "minecraft:overworld", &loc, &Config::default());
        assert_eq!(h.len(), 2);
        assert!(h[0].1.contains("text_display 100.5 48.25 -62.25"), "{}", h[0].1);
        assert!(h[1].1.contains("text_display 100.5 47.8 -62.25"), "{}", h[1].1);
        assert!(h[1].1.contains("translation:[0f,0.2f,0f]"), "{}", h[1].1);
        assert!(h[1].1.contains("Example (Private)"), "{}", h[1].1);
        let s = npc_snbt(&n, 5, &loc, &Config::default());
        assert!(!s.contains("CustomName"), "{s}");
        assert!(refresh(&n, 5, "minecraft:overworld").contains("CustomName:\"\",CustomNameVisible:0b"));
        // 16 characters or fewer: the name tag, no name hologram
        let short = npc();
        assert!(!name_hologram(&short));
        assert!(npc_snbt(&short, 5, &loc, &Config::default()).contains("CustomNameVisible:1b"));
    }

    /// a walker NPC: a footstep picks the sound by the block under the feet, stone by default.
    #[test]
    fn a_footstep_sounds_by_the_block_below() {
        let loc = npc().location().unwrap();
        let c = step_sound("minecraft:overworld", &loc);
        assert!(c[0].contains("execute if block 100.5 45.8 -62.25 #minecraft:wool run playsound minecraft:block.wool.step player @a 100.5 46 -62.25 0.15 1"), "{}", c[0]);
        let last = c.last().unwrap();
        assert!(last.contains("unless block 100.5 45.8 -62.25 minecraft:snow run playsound minecraft:block.stone.step"), "{last}");
        assert!(!last.contains("#minecraft:air"), "{last}");
        assert!(last.starts_with("execute in minecraft:overworld run execute unless block"), "{last}");
    }

    #[test]
    fn commands_name_the_uuid() {
        assert_eq!(kill(1), "kill 00000000-0000-0000-0000-000000000001");
        assert_eq!(
            rotate(1, "minecraft:the_nether", 12.5, -3.0),
            "execute in minecraft:the_nether run rotate 00000000-0000-0000-0000-000000000001 12.5 -3"
        );
        assert_eq!(team_setup("DARK_AQUA")[1], "team modify citizens_dark_aqua color dark_aqua");
        let n = npc();
        assert!(refresh(&n, 0x11, "minecraft:overworld").contains("Glowing:0b,Invulnerable:1b"));
    }
}
