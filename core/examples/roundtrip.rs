//! Reads each `saves.yml` given and writes it back in memory, then says whether the result is
//! byte for byte the same. Prints counts, line numbers and key names only, never values (the
//! files may hold player names and skins).
//!
//! `cargo run --example roundtrip -- path/to/saves.yml ...`

use horizon_citizens_core::saves::Saves;

fn key_of(line: &str) -> String {
    let t = line.trim_start();
    match t.split_once(':') {
        Some((k, _)) if !k.starts_with('-') && !k.contains(' ') => k.to_owned(),
        _ => "<value line>".into(),
    }
}

fn main() {
    let mut bad = 0;
    for path in std::env::args().skip(1) {
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                println!("{path}: unreadable ({})", e.kind());
                bad += 1;
                continue;
            }
        };
        let saves = match Saves::parse(&text) {
            Ok(s) => s,
            Err(e) => {
                println!("{path}: parse error at line {}", e.line);
                bad += 1;
                continue;
            }
        };
        let out = saves.to_yaml();
        if out == text {
            println!("{path}: {} NPCs, identical ({} bytes)", saves.npcs.len(), text.len());
            continue;
        }
        let (a, b): (Vec<&str>, Vec<&str>) = (text.lines().collect(), out.lines().collect());
        let first = a.iter().zip(&b).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
        println!(
            "{path}: {} NPCs, DIFFERENT: {} vs {} lines, first difference at line {} (key `{}` vs `{}`), {} differing lines",
            saves.npcs.len(),
            a.len(),
            b.len(),
            first + 1,
            a.get(first).map_or("<end>".into(), |l| key_of(l)),
            b.get(first).map_or("<end>".into(), |l| key_of(l)),
            a.iter().zip(&b).filter(|(x, y)| x != y).count()
        );
        bad += 1;
    }
    std::process::exit(i32::from(bad > 0));
}
