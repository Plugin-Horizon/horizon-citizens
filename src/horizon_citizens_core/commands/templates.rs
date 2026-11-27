//! `/template` (`/tpl`) and `/waypoints` (`/wp`).
//!
//! Written from the public Citizens wiki ("Commands", "Templates" and "Waypoints" pages) and the
//! node names of its `plugin.yml`; Citizens' source was not read (Open Software License 3.0). What
//! the wiki does not say is decided here and listed in the README ("Docs gaps"):
//!
//! Templates (`plugins/Citizens/templates/<namespace>/<file>.yml`, each file holding one or more
//! templates as top-level keys):
//! - `yaml_replace.replacements` is merged into the NPC's saved data, map by map. With
//!   `override: true` a template value replaces what the NPC has; without it (or when the key is
//!   missing) only keys the NPC lacks are filled in. The NPC's `uuid` is never replaced.
//! - `commands.on_spawn` and `commands.on_template_apply` are not run (the wiki's example is a
//!   Citizens command with options this port does not run on spawn); `apply` counts them and says so.
//! - `generate` writes `<namespace>/<name>.yml` with one template holding the selected NPC's saved
//!   data (without `uuid`, `traits.location`, `traits.spawned` and `traits.owner`) and
//!   `override: true`; it never overwrites a file. Without a namespace the file goes straight into
//!   `templates/`.
//! - A name without a namespace matches in every namespace; if several match, the command lists
//!   them and does nothing.
//!
//! Waypoints (only the linear provider keeps points; `guided` is saved but not walked):
//! - `add x y z (world) (--index i)` inserts at the 0-based index `i` (the end when missing or too
//!   large); the world is the sender's, else the NPC's.
//! - `remove` without arguments removes the last point; `--index i` removes that point;
//!   `x y z (world)` removes the first point in the same block.
//! - `disableteleport` flips `traits.waypoints.disableteleport`; nothing reads it yet.
//! - `hpa` (no description on the wiki) is not implemented.

use super::*;
use crate::horizon_citizens_core::yaml;
use std::path::{Path, PathBuf};

pub const ROOTS: &[&str] = &["template", "tpl", "waypoints", "wp"];

/// `tpl` is `template`, `wp` is `waypoints`.
pub fn canonical(root: &str) -> &str {
    match root {
        "tpl" => "template",
        "wp" => "waypoints",
        r => r,
    }
}

/// The usages registered with the game (aliases are added by the caller).
pub const USAGES: &[&str] = &[
    "template",
    "template help",
    "template list",
    "template apply <template:word>",
    "template generate <template:word>",
    "waypoints",
    "waypoints help",
    "waypoints add <x:number> <y:number> <z:number>",
    "waypoints add <x:number> <y:number> <z:number> <world:word>",
    "waypoints add <x:number> <y:number> <z:number> --index <index:integer>",
    "waypoints add <x:number> <y:number> <z:number> <world:word> --index <index:integer>",
    "waypoints remove",
    "waypoints remove <x:number> <y:number> <z:number>",
    "waypoints remove <x:number> <y:number> <z:number> <world:word>",
    "waypoints remove --index <index:integer>",
    "waypoints provider linear",
    "waypoints provider wander",
    "waypoints provider guided",
    "waypoints hpa",
];

const TEMPLATE_HELP: &[&str] = &[
    "template apply (template namespace:)[template name]",
    "template generate (template namespace:)[name]",
    "template list",
];
const WAYPOINT_HELP: &[&str] = &[
    "waypoints add [x] [y] [z] (world) (--index idx)",
    "waypoints hpa",
    "waypoints provider [provider name]",
    "waypoints remove (x y z world) (--index idx)",
];

/// The permission node of `/template <sub>` and `/waypoints <sub>`.
pub fn node(root: &str, sub: &str) -> &'static str {
    let sub = sub.to_ascii_lowercase();
    if canonical(root) == "template" {
        return match sub.as_str() {
            "apply" => "citizens.templates.apply",
            "generate" => "citizens.templates.generate",
            "list" => "citizens.templates.list",
            _ => "citizens.templates.help",
        };
    }
    match sub.as_str() {
        "add" => "citizens.waypoints.add",
        "remove" => "citizens.waypoints.remove",
        "provider" => "citizens.waypoints.provider",
        "disableteleport" => "citizens.waypoints.disableteleport",
        "hpa" => "citizens.admin",
        _ => "citizens.waypoints.help",
    }
}

pub const NODES: &[&str] = &[
    "citizens.templates.apply",
    "citizens.templates.generate",
    "citizens.templates.list",
    "citizens.templates.help",
    "citizens.waypoints.add",
    "citizens.waypoints.remove",
    "citizens.waypoints.provider",
    "citizens.waypoints.disableteleport",
    "citizens.waypoints.help",
];

const M_NO_DIR: &str = "Templates are not available: the server folder is unknown.";
const M_TEMPLATE_UNKNOWN: &str = "No template named [[{0}]] was found.";
const M_TEMPLATE_AMBIGUOUS: &str = "Several templates are named [[{0}]]: {1}. Use the full name (namespace:name).";
const M_TEMPLATE_APPLIED: &str = "Applied template [[{0}]] to [[{1}]].";
const M_TEMPLATE_COMMANDS: &str = "[[{0}]] template command(s) were not run: they are not supported on Horizon yet.";
const M_TEMPLATE_BAD_NAME: &str = "A template name may only use letters, digits, '_' and '-'.";
const M_TEMPLATE_EXISTS: &str = "The template [[{0}]] already exists; it was not overwritten.";
const M_TEMPLATE_WRITE: &str = "Could not write the template, see console.";
const M_TEMPLATE_GENERATED: &str = "Generated the template [[{0}]] from [[{1}]].";
const M_TEMPLATE_NONE: &str = "No templates found.";
const M_WP_ADDED: &str = "Added a waypoint at [[{0}, {1}, {2}]] in [[{3}]] (position [[{4}]]).";
const M_WP_REMOVED: &str = "Removed waypoint [[{0}]].";
const M_WP_NONE: &str = "No waypoint to remove.";
const M_WP_NOT_LINEAR: &str = "The waypoint provider is [[{0}]]; it does not keep points, switch to linear first.";
const M_WP_UNKNOWN_PROVIDER: &str = "Unknown waypoint provider [[{0}]]. Valid providers are linear, wander and guided.";
const M_WP_TELEPORT_OFF: &str = "[[{0}]] will no longer teleport when stuck.";
const M_WP_TELEPORT_ON: &str = "[[{0}]] may teleport again when stuck.";

fn help(w: &World, lines: &[&str]) -> Reply {
    let mut out = say(w, &header("Help"));
    for l in lines {
        out.extend(say(w, &format!("/{l}")));
    }
    out
}

// ---- templates -----------------------------------------------------------------------------

pub struct Found {
    pub ns: String,
    pub name: String,
    pub body: Node,
}

impl Found {
    fn key(&self) -> String {
        if self.ns.is_empty() { self.name.clone() } else { format!("{}:{}", self.ns, self.name) }
    }
}

/// The templates of one file: each top-level key is one.
pub fn parse_templates(ns: &str, text: &str) -> Vec<Found> {
    let Ok(root) = yaml::parse(text) else { return Vec::new() };
    root.keys()
        .filter_map(|k| {
            let body = root.get(k)?.clone();
            body.is_map().then(|| Found { ns: ns.to_owned(), name: k.to_owned(), body })
        })
        .collect()
}

fn yml_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut files: Vec<PathBuf> = rd
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "yml" || x == "yaml"))
        .collect();
    files.sort();
    files
}

fn templates_dir(w: &World) -> Option<PathBuf> {
    w.saves_path.as_ref().and_then(|p| p.parent()).map(|d| d.join("templates"))
}

fn read_all(dir: &Path) -> Vec<Found> {
    let mut out = Vec::new();
    let read = |ns: &str, files: Vec<PathBuf>, out: &mut Vec<Found>| {
        for f in files {
            if let Ok(text) = std::fs::read_to_string(&f) {
                out.extend(parse_templates(ns, &text));
            }
        }
    };
    read("", yml_files(dir), &mut out);
    if let Ok(rd) = std::fs::read_dir(dir) {
        let mut dirs: Vec<PathBuf> = rd.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.is_dir()).collect();
        dirs.sort();
        for d in dirs {
            let ns = d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            read(&ns, yml_files(&d), &mut out);
        }
    }
    out.sort_by(|a, b| (&a.ns, &a.name).cmp(&(&b.ns, &b.name)));
    out
}

pub enum Pick<'a> {
    One(&'a Found),
    Missing,
    Ambiguous(Vec<String>),
}

/// `namespace:name`, or `name` alone (any namespace).
pub fn pick<'a>(all: &'a [Found], reference: &str) -> Pick<'a> {
    let (ns, name) = match reference.split_once(':') {
        Some((ns, name)) => (Some(ns), name),
        None => (None, reference),
    };
    let hits: Vec<&Found> = all
        .iter()
        .filter(|f| f.name.eq_ignore_ascii_case(name) && ns.is_none_or(|n| f.ns.eq_ignore_ascii_case(n)))
        .collect();
    match hits.len() {
        0 => Pick::Missing,
        1 => Pick::One(hits[0]),
        _ => Pick::Ambiguous(hits.iter().map(|f| f.key()).collect()),
    }
}

fn merge(dst: &mut Node, src: &Node, overwrite: bool, top: bool) {
    let keys: Vec<String> = src.keys().map(str::to_owned).collect();
    for key in keys {
        if top && key == "uuid" {
            continue;
        }
        let Some(v) = src.get(&key) else { continue };
        if v.is_map() && dst.get(&key).is_some_and(Node::is_map) {
            if let Some(d) = dst.get_mut(&key) {
                merge(d, v, overwrite, false);
            }
        } else if overwrite || dst.get(&key).is_none() {
            dst.set(&key, v.clone());
        }
    }
}

/// Applies a template body to an NPC; returns how many template commands it holds and did not run.
pub fn apply_body(npc: &mut NpcData, body: &Node) -> usize {
    if let Some(ys) = body.get("yaml_replace") {
        let overwrite = ys.get("override").and_then(Node::as_bool).unwrap_or(false);
        if let Some(rep) = ys.get("replacements") {
            merge(&mut npc.map, rep, overwrite, true);
            if let Some(traits) = rep.get("traits") {
                let names: Vec<String> = traits.keys().map(str::to_owned).collect();
                for t in names {
                    if !["type", "location", "spawned", "owner"].contains(&t.as_str()) {
                        npc.add_trait_name(&t);
                    }
                }
            }
        }
    }
    ["on_spawn", "on_template_apply"]
        .iter()
        .filter_map(|k| body.get("commands")?.get(k))
        .map(|l| l.items().len())
        .sum()
}

/// A template as `generate` writes it.
pub fn generated(name: &str, npc: &NpcData) -> Node {
    let mut data = npc.map.clone();
    for path in [&["uuid"][..], &["traits", "location"], &["traits", "spawned"], &["traits", "owner"]] {
        data.drop_path(path);
    }
    let mut replace = Node::map();
    replace.set("replacements", data);
    replace.set("override", Node::bool(true));
    let mut body = Node::map();
    body.set("yaml_replace", replace);
    let mut root = Node::map();
    root.set(name, body);
    root
}

fn safe_name(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Applies the template `reference` to NPC `id`; the number of template commands not run, or the error line.
pub fn apply_named(w: &mut World, id: i32, reference: &str) -> Result<usize, Reply> {
    let dir = templates_dir(w).ok_or_else(|| err(w, M_NO_DIR, &[]))?;
    let all = read_all(&dir);
    let found = match pick(&all, reference) {
        Pick::One(f) => f,
        Pick::Missing => return Err(err(w, M_TEMPLATE_UNKNOWN, &[reference])),
        Pick::Ambiguous(keys) => return Err(err(w, M_TEMPLATE_AMBIGUOUS, &[reference, &keys.join(", ")])),
    };
    let skipped = apply_body(npc_mut(w, id)?, &found.body);
    w.mark_dirty();
    w.respawn(id);
    Ok(skipped)
}

pub fn template(w: &mut World, sender: &Sender, rest: &str) -> Res {
    let args = Args::parse(rest);
    let sub = args.words.first().map(|s| s.to_ascii_lowercase()).unwrap_or_default();
    match sub.as_str() {
        "" | "help" => Ok(help(w, TEMPLATE_HELP)),
        "list" => {
            let dir = templates_dir(w).ok_or_else(|| err(w, M_NO_DIR, &[]))?;
            let all = read_all(&dir);
            if all.is_empty() {
                return Ok(ok(w, M_TEMPLATE_NONE, &[]));
            }
            let mut out = say(w, &header("Templates"));
            for f in &all {
                out.extend(say(w, &format!("     - [[{}]]", f.key())));
            }
            Ok(out)
        }
        "apply" => {
            let id = target(w, sender)?;
            let Some(reference) = args.words.get(1) else {
                return Err(vec![w.cfg.messages.error("/template apply (template namespace:)[template name]")]);
            };
            let skipped = apply_named(w, id, reference)?;
            let mut out = ok(w, M_TEMPLATE_APPLIED, &[reference, &name_at(w, id)]);
            if skipped > 0 {
                out.extend(ok(w, M_TEMPLATE_COMMANDS, &[&skipped.to_string()]));
            }
            Ok(out)
        }
        "generate" => {
            let id = target(w, sender)?;
            let Some(reference) = args.words.get(1) else {
                return Err(vec![w.cfg.messages.error("/template generate (template namespace:)[name]")]);
            };
            let (ns, name) = reference.split_once(':').unwrap_or(("", reference));
            if !safe_name(name) || (!ns.is_empty() && !safe_name(ns)) {
                return Err(err(w, M_TEMPLATE_BAD_NAME, &[]));
            }
            let dir = templates_dir(w).ok_or_else(|| err(w, M_NO_DIR, &[]))?;
            let folder = if ns.is_empty() { dir } else { dir.join(ns) };
            let file = folder.join(format!("{name}.yml"));
            if file.exists() {
                return Err(err(w, M_TEMPLATE_EXISTS, &[reference]));
            }
            let npc = w.saves.get(id).ok_or_else(|| err(w, M_NPC_NOT_FOUND, &[]))?;
            let text = yaml::write(&generated(name, npc));
            if let Err(e) = std::fs::create_dir_all(&folder).and_then(|()| std::fs::write(&file, text)) {
                eprintln!("[citizens] template write failed: {e}");
                return Err(err(w, M_TEMPLATE_WRITE, &[]));
            }
            Ok(ok(w, M_TEMPLATE_GENERATED, &[reference, &name_at(w, id)]))
        }
        _ => Err(usage_text(w, TEMPLATE_HELP)),
    }
}

fn usage_text(w: &World, lines: &[&str]) -> Reply {
    let mut out = err(w, M_TOO_MANY, &[]);
    for l in lines {
        out.push(w.cfg.messages.error(&format!("/{l}")));
    }
    out
}

// ---- waypoints -----------------------------------------------------------------------------

fn provider_of(npc: &NpcData) -> String {
    npc.map.at(&["traits", "waypoints", "provider"]).and_then(Node::text).unwrap_or_else(|| "linear".to_owned())
}

fn parse_point(w: &World, sender: &Sender, id: i32, words: &[String]) -> Result<(f64, f64, f64, String), Reply> {
    let mut xyz = [0.0; 3];
    for (i, slot) in xyz.iter_mut().enumerate() {
        *slot = match words.get(i).map(|s| s.parse::<f64>()) {
            Some(Ok(v)) if v.is_finite() => v,
            Some(_) => return Err(err(w, M_INVALID_NUMBER, &[])),
            None => return Err(err(w, M_TOO_FEW, &[])),
        };
    }
    let world = words
        .get(3)
        .cloned()
        .or_else(|| sender.at.as_ref().map(|(_, l)| l.world.clone()))
        .or_else(|| w.saves.get(id).and_then(NpcData::location).map(|l| l.world))
        .unwrap_or_default();
    Ok((xyz[0], xyz[1], xyz[2], world))
}

fn index_of(w: &World, args: &Args) -> Result<Option<usize>, Reply> {
    match args.value("index") {
        None => Ok(None),
        Some(v) => v.parse::<usize>().map(Some).map_err(|_| err(w, M_INVALID_NUMBER, &[])),
    }
}

pub fn waypoints(w: &mut World, sender: &Sender, rest: &str) -> Res {
    let args = Args::parse(rest);
    let sub = args.words.first().map(|s| s.to_ascii_lowercase()).unwrap_or_default();
    if sub.is_empty() || sub == "help" {
        return Ok(help(w, WAYPOINT_HELP));
    }
    if !["add", "remove", "provider", "disableteleport", "hpa"].contains(&sub.as_str()) {
        return Err(usage_text(w, WAYPOINT_HELP));
    }
    let id = target(w, sender)?;
    let provider = w.saves.get(id).map(provider_of).unwrap_or_default();
    let words: Vec<String> = args.words.iter().skip(1).cloned().collect();
    match sub.as_str() {
        "add" => {
            if provider != "linear" {
                return Err(err(w, M_WP_NOT_LINEAR, &[&provider]));
            }
            let (x, y, z, world) = parse_point(w, sender, id, &words)?;
            if w.cfg.worlds.dimension(&world).is_none() {
                return Err(err(w, M_WORLD_NOT_FOUND, &[]));
            }
            let index = index_of(w, &args)?;
            let loc = Location { world: world.clone(), x, y, z, yaw: 0.0, pitch: 0.0, body_yaw: 0.0 };
            let npc = npc_mut(w, id)?;
            let at = index.unwrap_or(usize::MAX).min(npc.waypoints().len());
            npc.insert_waypoint(at, &loc);
            w.mark_dirty();
            Ok(ok(w, M_WP_ADDED, &[&dec2(x), &dec2(y), &dec2(z), &world, &at.to_string()]))
        }
        "remove" => {
            if provider != "linear" {
                return Err(err(w, M_WP_NOT_LINEAR, &[&provider]));
            }
            let points = w.saves.get(id).map(NpcData::waypoints).unwrap_or_default();
            let index = if let Some(i) = index_of(w, &args)? {
                Some(i)
            } else if words.is_empty() {
                points.len().checked_sub(1)
            } else {
                let (x, y, z, world) = parse_point(w, sender, id, &words)?;
                let block = |v: f64| v.floor() as i64;
                points.iter().position(|p| {
                    p.world == world && block(p.x) == block(x) && block(p.y) == block(y) && block(p.z) == block(z)
                })
            };
            let Some(i) = index.filter(|i| *i < points.len()) else {
                return Err(err(w, M_WP_NONE, &[]));
            };
            npc_mut(w, id)?.remove_waypoint(i);
            w.mark_dirty();
            Ok(ok(w, M_WP_REMOVED, &[&i.to_string()]))
        }
        "hpa" => {
            // The original only prints a debug path of its hierarchical graph to the server's console.
            eprintln!("[citizens] waypoints hpa: NPC {id} (the original prints a debug path here)");
            Ok(Vec::new())
        }
        "provider" => {
            let Some(name) = words.first().map(|s| s.to_ascii_lowercase()) else {
                return Err(vec![w.cfg.messages.error("/waypoints provider [provider name]")]);
            };
            if !["linear", "wander", "guided"].contains(&name.as_str()) {
                return Err(err(w, M_WP_UNKNOWN_PROVIDER, &[&name]));
            }
            let npc = npc_mut(w, id)?;
            npc.map.put(&["traits", "waypoints", "provider"], Node::str(&name));
            npc.add_trait_name("waypoints");
            w.mark_dirty();
            w.cancel_follow_walk(id);
            Ok(ok(w, M_PROVIDER_SET, &[&name]))
        }
        _ => {
            let npc_name = name_at(w, id);
            let off = !w
                .saves
                .get(id)
                .and_then(|n| n.map.at(&["traits", "waypoints", "disableteleport"]))
                .and_then(Node::as_bool)
                .unwrap_or(false);
            npc_mut(w, id)?.map.put(&["traits", "waypoints", "disableteleport"], Node::bool(off));
            w.mark_dirty();
            Ok(ok(w, if off { M_WP_TELEPORT_OFF } else { M_WP_TELEPORT_ON }, &[&npc_name]))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::horizon_citizens_core::config::Config;

    const EXAMPLE: &str = "towninfo:\n  yaml_replace:\n    replacements:\n      name: Town Crier\n      traits:\n        type: VILLAGER\n    override: true\n  commands:\n    on_spawn:\n      - say --range 5 Hello!\n";

    #[test]
    fn the_wiki_example_parses_and_applies() {
        let all = parse_templates("awesometemplates", EXAMPLE);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].key(), "awesometemplates:towninfo");
        let mut npc = NpcData::new(3, 0x33, "Bob", "PLAYER");
        let skipped = apply_body(&mut npc, &all[0].body);
        assert_eq!(skipped, 1, "the on_spawn command is counted, not run");
        assert_eq!(npc.name(), "Town Crier");
        assert_eq!(npc.kind(), "VILLAGER");
        assert_eq!(npc.uuid(), Some(0x33));
    }

    #[test]
    fn without_override_only_missing_keys_are_filled() {
        let text = "t:\n  yaml_replace:\n    replacements:\n      name: New\n      metadata:\n        glowing: true\n";
        let all = parse_templates("", text);
        let mut npc = NpcData::new(1, 1, "Old", "PLAYER");
        apply_body(&mut npc, &all[0].body);
        assert_eq!(npc.name(), "Old");
        assert_eq!(npc.map.at(&["metadata", "glowing"]).and_then(Node::as_bool), Some(true));
    }

    #[test]
    fn a_template_never_replaces_the_uuid() {
        let text = "t:\n  yaml_replace:\n    replacements:\n      uuid: 00000000-0000-0000-0000-000000000099\n    override: true\n";
        let all = parse_templates("", text);
        let mut npc = NpcData::new(1, 7, "Bob", "PLAYER");
        apply_body(&mut npc, &all[0].body);
        assert_eq!(npc.uuid(), Some(7));
    }

    #[test]
    fn picking_by_name_and_namespace() {
        let mut all = parse_templates("a", EXAMPLE);
        all.extend(parse_templates("b", EXAMPLE));
        assert!(matches!(pick(&all, "a:towninfo"), Pick::One(f) if f.ns == "a"));
        assert!(matches!(pick(&all, "towninfo"), Pick::Ambiguous(k) if k == vec!["a:towninfo", "b:towninfo"]));
        assert!(matches!(pick(&all, "nope"), Pick::Missing));
    }

    #[test]
    fn a_generated_template_drops_identity_and_applies_back() {
        let mut npc = NpcData::new(5, 0x55, "Guide", "VILLAGER");
        npc.set_location(&Location { world: "w".into(), x: 1.0, y: 2.0, z: 3.0, yaw: 0.0, pitch: 0.0, body_yaw: 0.0 });
        let text = yaml::write(&generated("copy", &npc));
        let all = parse_templates("ns", &text);
        assert_eq!(all[0].key(), "ns:copy");
        let data = all[0].body.get("yaml_replace").and_then(|y| y.get("replacements")).unwrap();
        assert!(data.get("uuid").is_none());
        assert!(data.path("traits.location").is_none());
        assert_eq!(data.get("name").and_then(Node::text).as_deref(), Some("Guide"));
    }

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

    #[test]
    fn waypoints_add_insert_and_remove() {
        let (mut w, s) = setup();
        cmd(&mut w, &s, "waypoints add 1 64 1");
        cmd(&mut w, &s, "wp add 5 64 5");
        let r = cmd(&mut w, &s, "waypoints add 9 64 9 --index 1");
        assert!(r[0].contains("9, 64, 9") && r[0].contains("position") , "{r:?}");
        let xs: Vec<f64> = w.saves.get(0).unwrap().waypoints().iter().map(|p| p.x).collect();
        assert_eq!(xs, vec![1.0, 9.0, 5.0]);
        let r = cmd(&mut w, &s, "waypoints remove 9 64 9");
        assert_eq!(r[0], "&aRemoved waypoint &e1&a.", "{r:?}");
        cmd(&mut w, &s, "waypoints remove");
        let xs: Vec<f64> = w.saves.get(0).unwrap().waypoints().iter().map(|p| p.x).collect();
        assert_eq!(xs, vec![1.0]);
        let r = cmd(&mut w, &s, "waypoints remove --index 4");
        assert!(r[0].contains("No waypoint to remove"), "{r:?}");
    }

    #[test]
    fn waypoint_provider_and_disableteleport() {
        let (mut w, s) = setup();
        let r = cmd(&mut w, &s, "waypoints provider guided");
        assert_eq!(r[0], "&aSet the waypoint provider to &eguided&a.", "{r:?}");
        let r = cmd(&mut w, &s, "waypoints add 1 2 3");
        assert!(r[0].contains("does not keep points"), "{r:?}");
        let r = cmd(&mut w, &s, "waypoints provider nonsense");
        assert!(r[0].contains("Unknown waypoint provider"), "{r:?}");
        let r = cmd(&mut w, &s, "waypoints disableteleport");
        assert!(r[0].contains("no longer teleport when stuck"), "{r:?}");
    }

    #[test]
    fn template_commands_ask_their_own_nodes() {
        assert_eq!(node("tpl", "apply"), "citizens.templates.apply");
        assert_eq!(node("template", ""), "citizens.templates.help");
        assert_eq!(node("wp", "add"), "citizens.waypoints.add");
        assert_eq!(node("waypoints", "bogus"), "citizens.waypoints.help");
    }
}
