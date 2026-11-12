//! `cargo run --example check_original -- <file>...`: loads Citizens files with the port's own
//! loaders and prints a redacted summary: which key paths of the file the port reads, which it
//! does not, and parse errors. Values are never printed.
//!
//! `config.yml` goes through `Config::from_citizens_yaml`, `saves.yml` through `Saves::parse`.
//! Key paths of a saved NPC are shown as `npc.*.<path>`; list items as `[]`.

use horizon_citizens_core::config::{Config, CITIZENS_CONFIG_KEYS};
use horizon_citizens_core::npc::NPC_READ_PATHS;
use horizon_citizens_core::saves::Saves;
use horizon_citizens_core::yaml::{self, Node};
use std::collections::BTreeSet;

fn key_paths(node: &Node, prefix: &str, out: &mut BTreeSet<String>) {
    let join = |k: &str| if prefix.is_empty() { k.to_owned() } else { format!("{prefix}.{k}") };
    match node {
        Node::Map { entries, .. } if !entries.is_empty() => {
            for e in entries {
                key_paths(&e.value, &join(&e.key), out);
            }
        }
        Node::Seq { items, .. } if !items.is_empty() => {
            for item in items {
                key_paths(item, &format!("{prefix}[]"), out);
            }
        }
        _ => {
            out.insert(prefix.to_owned());
        }
    }
}

/// `pattern` segments: `*` is one segment, a trailing `**` is any rest (including none).
fn matches(pattern: &str, path: &str) -> bool {
    let p: Vec<&str> = pattern.split('.').collect();
    let s: Vec<&str> = path.split('.').collect();
    if p.last() == Some(&"**") {
        let head = &p[..p.len() - 1];
        return s.len() >= head.len() && head.iter().zip(&s).all(|(a, b)| *a == "*" || a == b);
    }
    p.len() == s.len() && p.iter().zip(&s).all(|(a, b)| *a == "*" || a == b)
}

fn report(label: &str, paths: &BTreeSet<String>, read: &dyn Fn(&str) -> bool) {
    let (yes, no): (Vec<&String>, Vec<&String>) = paths.iter().partition(|p| read(p));
    println!("  {label}: read {} key paths, not read {}", yes.len(), no.len());
    for p in no {
        println!("    not read: {p}");
    }
}

fn check_config(text: &str) {
    match Config::from_citizens_yaml(text) {
        Ok(_) => println!("  loader: ok"),
        Err(e) => println!("  loader: error: {e}"),
    }
    let Ok(doc) = yaml::parse_document(text) else {
        return;
    };
    let mut paths = BTreeSet::new();
    key_paths(&doc.root, "", &mut paths);
    report("config.yml", &paths, &|p| CITIZENS_CONFIG_KEYS.contains(&p));
}

fn check_saves(text: &str) {
    let saves = match Saves::parse(text) {
        Ok(s) => s,
        Err(e) => {
            println!("  parse error: {e}");
            return;
        }
    };
    println!("  loader: ok, {} NPC records", saves.npcs.len());
    let Ok(doc) = yaml::parse_document(text) else {
        return;
    };
    let mut top = BTreeSet::new();
    let mut npc_paths = BTreeSet::new();
    for e in doc.root.entries() {
        if e.key == "npc" {
            for npc in e.value.entries() {
                key_paths(&npc.value, "", &mut npc_paths);
            }
        } else {
            top.insert(e.key.clone());
        }
    }
    println!("  top-level keys: {}", top.iter().cloned().collect::<Vec<_>>().join(", "));
    report("npc records", &npc_paths, &|p| NPC_READ_PATHS.iter().any(|pat| matches(pat, p)));
}

fn main() {
    for file in std::env::args().skip(1) {
        println!("{file}");
        let text = match std::fs::read_to_string(&file) {
            Ok(t) => t,
            Err(e) => {
                println!("  cannot read: {}", e.kind());
                continue;
            }
        };
        let name = std::path::Path::new(&file).file_name().and_then(|n| n.to_str()).unwrap_or("");
        match name {
            "config.yml" => check_config(&text),
            "saves.yml" => check_saves(&text),
            _ => println!("  not a Citizens config.yml or saves.yml"),
        }
    }
}
