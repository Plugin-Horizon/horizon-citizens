//! Legacy colour codes (`&a`, `§l`, `&#rrggbb`, `<#rrggbb>`) to text components and plain text.
//! Citizens names and hologram lines are written with these.

use serde_json::{Map, Value, json};

const COLORS: [(char, &str); 16] = [
    ('0', "black"),
    ('1', "dark_blue"),
    ('2', "dark_green"),
    ('3', "dark_aqua"),
    ('4', "dark_red"),
    ('5', "dark_purple"),
    ('6', "gold"),
    ('7', "gray"),
    ('8', "dark_gray"),
    ('9', "blue"),
    ('a', "green"),
    ('b', "aqua"),
    ('c', "red"),
    ('d', "light_purple"),
    ('e', "yellow"),
    ('f', "white"),
];

/// The colour name of a legacy code (`a` -> `green`).
pub fn color_name(code: char) -> Option<&'static str> {
    let code = code.to_ascii_lowercase();
    COLORS.iter().find(|(c, _)| *c == code).map(|(_, n)| *n)
}

/// The colour name of a Bukkit `ChatColor` name (`DARK_AQUA` -> `dark_aqua`).
pub fn chat_color(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    COLORS.iter().find(|(_, n)| *n == lower).map(|(_, n)| *n)
}

#[derive(Clone, Default, PartialEq)]
struct Style {
    color: Option<String>,
    bold: bool,
    italic: bool,
    underlined: bool,
    strikethrough: bool,
    obfuscated: bool,
}

/// Splits `s` into (text, style) runs.
fn runs(s: &str) -> Vec<(String, Style)> {
    let chars: Vec<char> = s.chars().collect();
    let mut out: Vec<(String, Style)> = Vec::new();
    let mut style = Style::default();
    let mut cur = String::new();
    let mut i = 0;
    let flush = |cur: &mut String, style: &Style, out: &mut Vec<(String, Style)>| {
        if !cur.is_empty() {
            out.push((std::mem::take(cur), style.clone()));
        }
    };
    while i < chars.len() {
        let c = chars[i];
        // <#rrggbb>
        if c == '<' && i + 8 < chars.len() + 1 && chars.get(i + 1) == Some(&'#') && chars.get(i + 8) == Some(&'>') {
            let hex: String = chars[i + 2..i + 8].iter().collect();
            if hex.chars().all(|h| h.is_ascii_hexdigit()) {
                flush(&mut cur, &style, &mut out);
                style = Style {
                    color: Some(format!("#{}", hex.to_ascii_lowercase())),
                    ..Style::default()
                };
                i += 9;
                continue;
            }
        }
        if (c == '&' || c == '§') && i + 1 < chars.len() {
            let code = chars[i + 1].to_ascii_lowercase();
            // &#rrggbb
            if code == '#' && i + 8 <= chars.len() {
                let hex: String = chars[i + 2..(i + 8).min(chars.len())].iter().collect();
                if hex.len() == 6 && hex.chars().all(|h| h.is_ascii_hexdigit()) {
                    flush(&mut cur, &style, &mut out);
                    style = Style {
                        color: Some(format!("#{}", hex.to_ascii_lowercase())),
                        ..Style::default()
                    };
                    i += 8;
                    continue;
                }
            }
            if let Some(name) = color_name(code) {
                flush(&mut cur, &style, &mut out);
                style = Style {
                    color: Some(name.into()),
                    ..Style::default()
                };
                i += 2;
                continue;
            }
            let format: Option<fn(&mut Style)> = match code {
                'l' => Some(|s: &mut Style| s.bold = true),
                'o' => Some(|s: &mut Style| s.italic = true),
                'n' => Some(|s: &mut Style| s.underlined = true),
                'm' => Some(|s: &mut Style| s.strikethrough = true),
                'k' => Some(|s: &mut Style| s.obfuscated = true),
                'r' => Some(|s: &mut Style| *s = Style::default()),
                _ => None,
            };
            if let Some(apply) = format {
                flush(&mut cur, &style, &mut out);
                apply(&mut style);
                i += 2;
                continue;
            }
        }
        cur.push(c);
        i += 1;
    }
    flush(&mut cur, &style, &mut out);
    out
}

/// `&` codes as `§` codes, for the game's legacy text parser (chat lines). Hex colours, which it
/// has no code for, are dropped.
pub fn to_section(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '&' && i + 1 < chars.len() {
            let code = chars[i + 1].to_ascii_lowercase();
            if code == '#'
                && i + 8 <= chars.len()
                && chars[i + 2..i + 8].iter().all(|h| h.is_ascii_hexdigit())
            {
                i += 8;
                continue;
            }
            if code.is_ascii_hexdigit() || "klmnor".contains(code) {
                out.push('\u{a7}');
                out.push(code);
                i += 2;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Text without colour and format codes.
pub fn strip(s: &str) -> String {
    runs(s).into_iter().map(|(t, _)| t).collect()
}

/// A text component (JSON, which SNBT also reads) for `s` with its legacy codes. Italic is set
/// off explicitly, as names and display texts would otherwise not be, but item names would.
pub fn component(s: &str) -> Value {
    let parts: Vec<Value> = runs(s)
        .into_iter()
        .map(|(text, st)| {
            let mut m = Map::new();
            m.insert("text".into(), Value::String(text));
            if let Some(c) = st.color {
                m.insert("color".into(), Value::String(c));
            }
            for (on, key) in [
                (st.bold, "bold"),
                (st.italic, "italic"),
                (st.underlined, "underlined"),
                (st.strikethrough, "strikethrough"),
                (st.obfuscated, "obfuscated"),
            ] {
                if on {
                    m.insert(key.into(), Value::Bool(true));
                }
            }
            Value::Object(m)
        })
        .collect();
    match parts.len() {
        0 => json!({"text": ""}),
        1 => parts.into_iter().next().unwrap_or_else(|| json!({"text": ""})),
        _ => json!({"text": "", "extra": parts}),
    }
}

/// [`component`] as SNBT text.
pub fn component_snbt(s: &str) -> String {
    component(s).to_string()
}

/// A name usable as a game profile name: up to 16 of `[A-Za-z0-9_]`, from the plain text.
pub fn profile_name(s: &str, fallback: &str) -> String {
    let n: String = strip(s)
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .take(16)
        .collect();
    if n.is_empty() { fallback.chars().take(16).collect() } else { n }
}

/// Fills Citizens' placeholders for the clicking player: `<p>`, `<player>`, `%player%`, and
/// `<npc>` / `<id>` for the NPC.
pub fn fill_placeholders(s: &str, player: &str, npc_name: &str, npc_id: i32) -> String {
    s.replace("<player>", player)
        .replace("<p>", player)
        .replace("%player%", player)
        .replace("<npc>", &strip(npc_name))
        .replace("<id>", &npc_id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_become_components() {
        assert_eq!(component("Guide"), json!({"text": "Guide"}));
        assert_eq!(
            component("&a&lOPEN"),
            json!({"text": "OPEN", "color": "green", "bold": true})
        );
        assert_eq!(
            component("&6Gold &r&#ff0000Red"),
            json!({"text": "", "extra": [
                {"text": "Gold ", "color": "gold"},
                {"text": "Red", "color": "#ff0000"}
            ]})
        );
        assert_eq!(strip("<#00ff00>x&ly"), "xy");
        assert_eq!(strip("A & B"), "A & B");
    }

    #[test]
    fn profile_names_are_valid() {
        assert_eq!(profile_name("&6Mr. Guide [A]", "npc"), "MrGuideA");
        assert_eq!(profile_name("&c!!!", "npc3"), "npc3");
        assert_eq!(profile_name("abcdefghijklmnopqrstuvwxyz", "x").len(), 16);
    }

    #[test]
    fn placeholders() {
        assert_eq!(
            fill_placeholders("server lobby <p> <id>", "Alex", "&aGuide", 4),
            "server lobby Alex 4"
        );
    }

    #[test]
    fn section_codes() {
        assert_eq!(to_section("&aHi &#ff0000you & me"), "\u{a7}aHi you & me");
    }

    #[test]
    fn chat_colors() {
        assert_eq!(chat_color("DARK_AQUA"), Some("dark_aqua"));
        assert_eq!(chat_color("nope"), None);
    }
}
