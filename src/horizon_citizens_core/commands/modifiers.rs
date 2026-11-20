//! More `/npc` modifiers: `ai`, `aggressive`, `passive`, `collidable`, `chunkload`, `leashable`,
//! `targetable`, `knockback`, `swim`, `pickupitems`, `sitting`, `playerlist`, `skinlayers`,
//! `entitypose`, `trackingrange`, `activationrange`, `respawn`, `scaledmaxhealth` and `metadata`.
//!
//! Written from the public Citizens wiki command table (name, arguments, one-line description)
//! and its `plugin.yml` permission nodes only; Citizens' source was not read (it is under the Open
//! Software License 3.0). What the wiki does not say is decided here and listed in the README
//! ("Docs gaps"):
//! - the texts of the replies are this port's own;
//! - the save keys (`traits.<modifier>.*`, `metadata.<key>`) are this port's choice;
//! - a flag nobody set reads as off, except `collidable`, which reads as on (an entity collides
//!   unless told otherwise);
//! - only `ai` changes the spawned entity (a non-player mob with AI on is summoned without
//!   `NoAI`). Every other setting is saved and described, and the reply says it has no effect on
//!   Horizon yet.

use super::*;

enum Kind {
    /// `true`/`false` as the first word, or as the value of `--<flag>`; nothing flips it.
    Flag { on: &'static str, off: &'static str, flag: Option<&'static str>, temp: bool, default: bool },
    /// One number (`min` and up); nothing prints the current value.
    Number { label: &'static str, min: f64, integer: bool },
}

struct Simple {
    name: &'static str,
    kind: Kind,
    /// Whether the saved value changes the spawned entity.
    live: bool,
}

const fn flag(
    name: &'static str,
    on: &'static str,
    off: &'static str,
    flag: Option<&'static str>,
    temp: bool,
    live: bool,
) -> Simple {
    Simple { name, kind: Kind::Flag { on, off, flag, temp, default: false }, live }
}

const SIMPLE: &[Simple] = &[
    flag("aggressive", "is now aggressive", "is no longer aggressive", None, true, false),
    flag("ai", "will now use vanilla AI", "will no longer use vanilla AI", None, false, true),
    flag("chunkload", "now keeps chunks loaded", "no longer keeps chunks loaded", None, true, false),
    Simple {
        name: "collidable",
        kind: Kind::Flag { on: "is now collidable", off: "is no longer collidable", flag: None, temp: false, default: true },
        live: false,
    },
    flag("knockback", "now takes knockback", "no longer takes knockback", Some("explicit"), false, false),
    flag("leashable", "can now be leashed", "can no longer be leashed", None, false, false),
    flag("passive", "no longer damages other entities", "now damages other entities", Some("set"), false, false),
    flag("pickupitems", "can now pick up items", "can no longer pick up items", Some("set"), false, false),
    flag("sitting", "is now sitting", "is no longer sitting", Some("explicit"), false, false),
    flag("swim", "will now swim", "will no longer swim", Some("set"), false, false),
    flag("targetable", "can now be targeted", "can no longer be targeted", None, false, false),
    Simple { name: "activationrange", kind: Kind::Number { label: "activation range", min: 0.0, integer: false }, live: false },
    Simple { name: "respawn", kind: Kind::Number { label: "respawn delay", min: -1.0, integer: true }, live: false },
    Simple { name: "scaledmaxhealth", kind: Kind::Number { label: "scaled max health", min: 0.0, integer: false }, live: false },
    Simple { name: "trackingrange", kind: Kind::Number { label: "tracking range", min: -1.0, integer: false }, live: false },
];

/// The sub-commands of this file.
pub const NAMES: &[&str] = &[
    "aggressive", "ai", "chunkload", "collidable", "knockback", "leashable", "passive", "pickupitems", "sitting", "swim",
    "targetable", "activationrange", "respawn", "scaledmaxhealth", "trackingrange", "playerlist", "skinlayers",
    "entitypose", "metadata",
];

const SKIN_LAYERS: [&str; 5] = ["cape", "hat", "jacket", "sleeves", "pants"];

const M_FLAG: &str = "[[{0}]] {1}.";
const M_NOT_ENFORCED: &str = "This setting is saved, but Horizon does not act on it yet.";
const M_TEMPORARY: &str = "Temporary changes (-t) are not supported on Horizon yet.";
const M_NUM_DESCRIBE: &str = "[[{0}]]''s {1} is [[{2}]].";
const M_NUM_SET: &str = "[[{0}]]''s {1} set to [[{2}]].";
const M_NUM_MIN: &str = "The value must be at least [[{0}]].";
const M_COLLIDABLE_FLUIDS: &str = "[[{0}]] fluid collision is now [[{1}]].";
const M_PLAYERLIST_ON: &str = "[[{0}]] will now be put in the player list.";
const M_PLAYERLIST_OFF: &str = "[[{0}]] will no longer be put in the player list.";
const M_LAYERS: &str = "[[{0}]]''s skin layers: {1}.";
const M_LAYERS_SET: &str = "[[{0}]]''s skin layers set: {1}.";
const M_POSE_DESCRIBE: &str = "[[{0}]]''s entity pose is [[{1}]].";
const M_POSE_SET: &str = "[[{0}]]''s entity pose set to [[{1}]].";
const M_META_GET: &str = "Metadata [[{0}]] is [[{1}]].";
const M_META_UNSET: &str = "Metadata [[{0}]] is not set.";
const M_META_SET: &str = "Metadata [[{0}]] set to [[{1}]].";
const M_META_REMOVED: &str = "Metadata [[{0}]] removed.";

pub fn handles(name: &str) -> bool {
    NAMES.contains(&name)
}

pub fn run(w: &mut World, sender: &Sender, name: &str, args: &Args) -> Res {
    match name {
        "playerlist" => playerlist(w, sender, args),
        "skinlayers" => skinlayers(w, sender, args),
        "entitypose" => entitypose(w, sender, args),
        "metadata" => metadata(w, sender, args),
        _ => {
            let Some(spec) = SIMPLE.iter().find(|s| s.name == name) else {
                return Err(usage_of(w, name));
            };
            match spec.kind {
                Kind::Flag { on, off, flag, temp, default } => flag_cmd(w, sender, spec, on, off, flag, temp, default, args),
                Kind::Number { label, min, integer } => number_cmd(w, sender, spec, label, min, integer, args),
            }
        }
    }
}

fn saved_bool(w: &World, id: i32, path: &[&str]) -> Option<bool> {
    w.saves.get(id).and_then(|n| n.map.at(path)).and_then(Node::as_bool)
}

fn finish(w: &mut World, id: i32, live: bool, mut out: Reply) -> Reply {
    w.mark_dirty();
    if live {
        w.respawn(id);
    } else {
        out.extend(ok(w, M_NOT_ENFORCED, &[]));
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn flag_cmd(
    w: &mut World,
    sender: &Sender,
    spec: &Simple,
    on: &str,
    off: &str,
    flag: Option<&str>,
    temp: bool,
    default: bool,
    args: &Args,
) -> Res {
    let id = target(w, sender)?;
    let name = spec.name;
    if temp && args.has('t') {
        return Err(err(w, M_TEMPORARY, &[]));
    }
    let npc_name = name_at(w, id);
    // `collidable --fluids [true|false]` changes the fluid setting, not the flag itself.
    if name == "collidable"
        && let Some(v) = args.value("fluids")
    {
        let fluids = match v.to_ascii_lowercase().as_str() {
            "true" => true,
            "false" => false,
            _ => return Err(usage_of(w, name)),
        };
        {
            let npc = npc_mut(w, id)?;
            npc.map.put(&["traits", name, "fluids"], Node::bool(fluids));
            npc.add_trait_name(name);
        }
        let out = ok(w, M_COLLIDABLE_FLUIDS, &[&npc_name, if fluids { "enabled" } else { "disabled" }]);
        return Ok(finish(w, id, false, out));
    }
    let current = saved_bool(w, id, &["traits", name, "enabled"]).unwrap_or(default);
    let given = args.words.first().map(String::as_str).or_else(|| flag.and_then(|f| args.value(f)));
    let value = match given.map(str::to_ascii_lowercase).as_deref() {
        Some("true") => true,
        Some("false") => false,
        Some(_) => return Err(usage_of(w, name)),
        None => !current,
    };
    {
        let npc = npc_mut(w, id)?;
        npc.map.put(&["traits", name, "enabled"], Node::bool(value));
        npc.add_trait_name(name);
    }
    let out = ok(w, M_FLAG, &[&npc_name, if value { on } else { off }]);
    Ok(finish(w, id, spec.live, out))
}

fn number_cmd(w: &mut World, sender: &Sender, spec: &Simple, label: &str, min: f64, integer: bool, args: &Args) -> Res {
    let id = target(w, sender)?;
    let name = spec.name;
    let npc_name = name_at(w, id);
    let Some(raw) = args.words.first() else {
        let current = w
            .saves
            .get(id)
            .and_then(|n| n.map.at(&["traits", name, "value"]))
            .and_then(Node::as_f64)
            .map_or_else(|| "not set".to_owned(), |v| if integer { format!("{}", v as i64) } else { dec2(v) });
        return Ok(ok(w, M_NUM_DESCRIBE, &[&npc_name, label, &current]));
    };
    let value: f64 = match raw.parse() {
        Ok(v) if f64::is_finite(v) => v,
        _ => return Err(err(w, M_INVALID_NUMBER, &[])),
    };
    if value < min {
        return Err(err(w, M_NUM_MIN, &[&dec2(min)]));
    }
    {
        let npc = npc_mut(w, id)?;
        let node = if integer { Node::int(value as i64) } else { Node::float(value) };
        npc.map.put(&["traits", name, "value"], node);
        npc.add_trait_name(name);
    }
    let shown = if integer { format!("{}", value as i64) } else { dec2(value) };
    let out = ok(w, M_NUM_SET, &[&npc_name, label, &shown]);
    Ok(finish(w, id, spec.live, out))
}

/// `/npc playerlist (-a|-r)`: `-a` adds, `-r` removes, nothing flips. (Horizon's NPCs are not
/// players, so there is no tab-list entry to add: the value is only saved.)
fn playerlist(w: &mut World, sender: &Sender, args: &Args) -> Res {
    let id = target(w, sender)?;
    let npc_name = name_at(w, id);
    let current = saved_bool(w, id, &["traits", "playerlist", "enabled"]).unwrap_or(false);
    let value = if args.has('a') {
        true
    } else if args.has('r') {
        false
    } else {
        !current
    };
    {
        let npc = npc_mut(w, id)?;
        npc.map.put(&["traits", "playerlist", "enabled"], Node::bool(value));
        npc.add_trait_name("playerlist");
    }
    let out = ok(w, if value { M_PLAYERLIST_ON } else { M_PLAYERLIST_OFF }, &[&npc_name]);
    Ok(finish(w, id, false, out))
}

/// `/npc skinlayers (--cape|--hat|--jacket|--sleeves|--pants [true|false])`. No argument lists
/// the saved layers (`default` for one nobody set; the wiki does not say what the default is).
fn skinlayers(w: &mut World, sender: &Sender, args: &Args) -> Res {
    let id = target(w, sender)?;
    let npc_name = name_at(w, id);
    let mut changed = Vec::new();
    for part in SKIN_LAYERS {
        if let Some(v) = args.value(part) {
            let value = match v.to_ascii_lowercase().as_str() {
                "true" => true,
                "false" => false,
                _ => return Err(usage_of(w, "skinlayers")),
            };
            let npc = npc_mut(w, id)?;
            npc.map.put(&["traits", "skinlayers", part], Node::bool(value));
            npc.add_trait_name("skinlayers");
            changed.push(format!("{part} {value}"));
        }
    }
    if changed.is_empty() {
        let parts: Vec<String> = SKIN_LAYERS
            .iter()
            .map(|p| {
                let v = saved_bool(w, id, &["traits", "skinlayers", p]).map_or("default".to_owned(), |b| b.to_string());
                format!("{p} {v}")
            })
            .collect();
        return Ok(ok(w, M_LAYERS, &[&npc_name, &parts.join(", ")]));
    }
    let out = ok(w, M_LAYERS_SET, &[&npc_name, &changed.join(", ")]);
    Ok(finish(w, id, false, out))
}

/// `/npc entitypose [pose]`: the pose name is saved upper case. The wiki lists no valid names,
/// so any word is accepted.
fn entitypose(w: &mut World, sender: &Sender, args: &Args) -> Res {
    let id = target(w, sender)?;
    let npc_name = name_at(w, id);
    let Some(raw) = args.words.first() else {
        let current = w
            .saves
            .get(id)
            .and_then(|n| n.map.at(&["traits", "entitypose", "pose"]))
            .and_then(Node::text)
            .unwrap_or_else(|| "not set".to_owned());
        return Ok(ok(w, M_POSE_DESCRIBE, &[&npc_name, &current]));
    };
    let pose = raw.to_ascii_uppercase();
    {
        let npc = npc_mut(w, id)?;
        npc.map.put(&["traits", "entitypose", "pose"], Node::str(&pose));
        npc.add_trait_name("entitypose");
    }
    let out = ok(w, M_POSE_SET, &[&npc_name, &pose]);
    Ok(finish(w, id, false, out))
}

fn node_text(n: &Node) -> String {
    if let Some(b) = n.as_bool() {
        b.to_string()
    } else if let Some(i) = n.as_i64() {
        i.to_string()
    } else {
        n.text().unwrap_or_default()
    }
}

/// `/npc metadata set|get|remove [key] (value)`: free-form values under `metadata.<key>`. A
/// value of `true`/`false` is saved as a boolean, a whole number as a number, anything else as
/// text. `-t` (temporary) is refused.
fn metadata(w: &mut World, sender: &Sender, args: &Args) -> Res {
    let id = target(w, sender)?;
    if args.has('t') {
        return Err(err(w, M_TEMPORARY, &[]));
    }
    let op = args.words.first().map(|s| s.to_ascii_lowercase()).unwrap_or_default();
    let Some(key) = args.words.get(1).cloned() else {
        return Err(usage_of(w, "metadata"));
    };
    match op.as_str() {
        "get" => {
            let current = w.saves.get(id).and_then(|n| n.map.at(&["metadata", &key])).map(node_text);
            Ok(match current {
                Some(v) => ok(w, M_META_GET, &[&key, &v]),
                None => ok(w, M_META_UNSET, &[&key]),
            })
        }
        "set" => {
            let value = args.rest(2);
            if value.is_empty() {
                return Err(usage_of(w, "metadata"));
            }
            let node = match value.to_ascii_lowercase().as_str() {
                "true" => Node::bool(true),
                "false" => Node::bool(false),
                _ => match value.parse::<i64>() {
                    Ok(i) => Node::int(i),
                    Err(_) => Node::str(&value),
                },
            };
            npc_mut(w, id)?.map.put(&["metadata", &key], node);
            w.mark_dirty();
            Ok(ok(w, M_META_SET, &[&key, &value]))
        }
        "remove" => {
            npc_mut(w, id)?.map.drop_path(&["metadata", &key]);
            w.mark_dirty();
            Ok(ok(w, M_META_REMOVED, &[&key]))
        }
        _ => Err(usage_of(w, "metadata")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::horizon_citizens_core::config::Config;

    fn setup() -> (World, Sender) {
        let mut cfg = Config::default();
        cfg.skins.fetch = false;
        let mut w = World::new(cfg, Saves::empty(), None);
        let s = Sender {
            uuid: Some(0xA1),
            name: "Alex".into(),
            permission: 2,
            grants: Vec::new(),
            at: Some((
                "minecraft:overworld".into(),
                Location { world: "world".into(), x: 1.0, y: 64.0, z: 1.0, yaw: 0.0, pitch: 0.0, body_yaw: 0.0 },
            )),
        };
        super::super::run(&mut w, &s, "npc create Bob", || Ok(Saves::empty()));
        (w, s)
    }

    fn cmd(w: &mut World, s: &Sender, line: &str) -> Vec<String> {
        super::super::run(w, s, line, || Ok(Saves::empty()))
    }

    fn flag_of(w: &World, name: &str) -> Option<bool> {
        w.saves.get(0).unwrap().map.at(&["traits", name, "enabled"]).and_then(Node::as_bool)
    }

    #[test]
    fn a_flag_flips_and_takes_a_value() {
        let (mut w, s) = setup();
        let r = cmd(&mut w, &s, "npc leashable");
        assert_eq!(r[0], "&a&eBob&a can now be leashed.", "{r:?}");
        assert!(r[1].contains("does not act on it"), "{r:?}");
        assert_eq!(flag_of(&w, "leashable"), Some(true));
        cmd(&mut w, &s, "npc leashable");
        assert_eq!(flag_of(&w, "leashable"), Some(false));
        cmd(&mut w, &s, "npc swim --set true");
        assert_eq!(flag_of(&w, "swim"), Some(true));
        cmd(&mut w, &s, "npc aggressive true");
        assert_eq!(flag_of(&w, "aggressive"), Some(true));
    }

    #[test]
    fn collidable_reads_as_on_until_set() {
        let (mut w, s) = setup();
        let r = cmd(&mut w, &s, "npc collidable");
        assert!(r[0].contains("is no longer collidable"), "{r:?}");
        let r = cmd(&mut w, &s, "npc collidable --fluids true");
        assert!(r[0].contains("fluid collision is now") && r[0].contains("enabled"), "{r:?}");
    }

    #[test]
    fn numbers_are_described_and_checked() {
        let (mut w, s) = setup();
        let r = cmd(&mut w, &s, "npc trackingrange");
        assert_eq!(r[0], "&a&eBob&a's tracking range is &enot set&a.", "{r:?}");
        let r = cmd(&mut w, &s, "npc trackingrange 48");
        assert_eq!(r[0], "&a&eBob&a's tracking range set to &e48&a.", "{r:?}");
        let r = cmd(&mut w, &s, "npc scaledmaxhealth -3");
        assert!(r[0].contains("at least"), "{r:?}");
        let r = cmd(&mut w, &s, "npc respawn abc");
        assert!(r[0].contains("not a valid number"), "{r:?}");
    }

    #[test]
    fn metadata_round_trips() {
        let (mut w, s) = setup();
        let r = cmd(&mut w, &s, "npc metadata set mood very happy");
        assert_eq!(r[0], "&aMetadata &emood&a set to &every happy&a.", "{r:?}");
        let r = cmd(&mut w, &s, "npc metadata get mood");
        assert_eq!(r[0], "&aMetadata &emood&a is &every happy&a.", "{r:?}");
        cmd(&mut w, &s, "npc metadata set level 7");
        assert_eq!(w.saves.get(0).unwrap().map.at(&["metadata", "level"]).and_then(Node::as_i64), Some(7));
        cmd(&mut w, &s, "npc metadata remove mood");
        let r = cmd(&mut w, &s, "npc metadata get mood");
        assert!(r[0].contains("is not set"), "{r:?}");
        let r = cmd(&mut w, &s, "npc metadata set x 1 -t");
        assert!(r[0].contains("Temporary"), "{r:?}");
    }

    #[test]
    fn skin_layers_and_playerlist() {
        let (mut w, s) = setup();
        let r = cmd(&mut w, &s, "npc skinlayers --cape false --hat true");
        assert!(r[0].contains("cape false, hat true"), "{r:?}");
        let r = cmd(&mut w, &s, "npc skinlayers");
        assert!(r[0].contains("cape false, hat true, jacket default"), "{r:?}");
        let r = cmd(&mut w, &s, "npc playerlist -a");
        assert!(r[0].contains("will now be put in the player list"), "{r:?}");
        let r = cmd(&mut w, &s, "npc playerlist -r");
        assert!(r[0].contains("no longer"), "{r:?}");
    }
}
