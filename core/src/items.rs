//! Citizens' item meta (`equipment.<slot>.meta`, `inventory[].meta`): a base64 Java stream of
//! Bukkit's `ItemMeta` map, turned into the component patch of a Horizon item.
//!
//! Mapped: `display-name` (a JSON string) -> `minecraft:custom_name`, `lore` (JSON strings) ->
//! `minecraft:lore`, `enchants` (namespaced id -> level) -> `minecraft:enchantments`,
//! `repair-cost` -> `minecraft:repair_cost`, `Damage` -> `minecraft:damage`, `max-stack-size` ->
//! `minecraft:max_stack_size`, `potion-type` -> `minecraft:potion_contents`. Not mapped (listed in
//! [`ItemMeta::unmapped`], so the caller can log them): armour `trim`, banner `base-color` and
//! `patterns`, and any other key.

use crate::javaser::{self, Value};
use crate::text;

/// What a meta blob holds, in components.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemMeta {
    pub display_name: Option<String>,
    pub lore: Vec<String>,
    /// Namespaced enchantment id and level, in file order.
    pub enchants: Vec<(String, i32)>,
    pub repair_cost: Option<i32>,
    pub damage: Option<i32>,
    pub max_stack_size: Option<i32>,
    pub potion: Option<String>,
    /// Keys read but not mapped to components.
    pub unmapped: Vec<String>,
}

/// Decodes a base64 `meta` value of Citizens' saves.
pub fn decode_meta(b64: &str) -> Result<ItemMeta, String> {
    let bytes = base64_decode(b64.trim()).ok_or("meta is not base64")?;
    let stream = javaser::read(&bytes).map_err(|e| e.to_string())?;
    let wrapper = stream.as_object().ok_or("meta is not an object")?;
    // Bukkit wraps the map: `org.bukkit.util.io.Wrapper { map }`.
    let map = if wrapper.is_a("org.bukkit.util.io.Wrapper") {
        wrapper.field("map").ok_or("wrapper without map")?
    } else {
        &stream
    };
    let entries = map.map_entries().ok_or("meta map is not a map")?;
    let mut meta = ItemMeta::default();
    for (k, v) in entries {
        let key = k.as_str().ok_or("meta key is not a string")?.to_owned();
        match key.as_str() {
            "==" | "meta-type" => {}
            "display-name" => {
                meta.display_name = Some(json_string(v.as_str().ok_or("display-name is not text")?));
            }
            "lore" => {
                for line in v.list_items().ok_or("lore is not a list")? {
                    meta.lore.push(json_string(line.as_str().ok_or("lore line is not text")?));
                }
            }
            "enchants" => {
                let pairs = v.map_entries().ok_or("enchants is not a map")?;
                for (id, level) in pairs {
                    let id = id.as_str().ok_or("enchantment id is not text")?.to_owned();
                    let level = level.boxed_int().ok_or("enchantment level is not an int")?;
                    meta.enchants.push((id, level));
                }
            }
            "repair-cost" => meta.repair_cost = Some(boxed(&v)?),
            "Damage" => meta.damage = Some(boxed(&v)?),
            "max-stack-size" => meta.max_stack_size = Some(boxed(&v)?),
            "potion-type" => meta.potion = Some(v.as_str().ok_or("potion-type is not text")?.to_owned()),
            _ => meta.unmapped.push(key),
        }
    }
    Ok(meta)
}

fn boxed(v: &Value) -> Result<i32, String> {
    v.boxed_int().ok_or_else(|| "a number is not an int".to_owned())
}

/// Bukkit keeps a name as a JSON string literal (quoted); a plain string stays as it is.
fn json_string(raw: &str) -> String {
    if raw.starts_with('"') {
        serde_json::from_str::<String>(raw).unwrap_or_else(|_| raw.to_owned())
    } else {
        raw.to_owned()
    }
}

/// The components of an item as SNBT entries (`"minecraft:x":value`), without the braces. Empty
/// when the meta has nothing mapped.
pub fn components(meta: &ItemMeta) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(name) = &meta.display_name {
        out.push(format!("\"minecraft:custom_name\":{}", text::component_snbt(name)));
    }
    if !meta.lore.is_empty() {
        let lines: Vec<String> = meta.lore.iter().map(|l| text::component_snbt(l)).collect();
        out.push(format!("\"minecraft:lore\":[{}]", lines.join(",")));
    }
    if !meta.enchants.is_empty() {
        let levels: Vec<String> = meta
            .enchants
            .iter()
            .map(|(id, level)| format!("{}:{level}", quoted(&namespaced(id))))
            .collect();
        out.push(format!("\"minecraft:enchantments\":{{levels:{{{}}}}}", levels.join(",")));
    }
    if let Some(v) = meta.repair_cost {
        out.push(format!("\"minecraft:repair_cost\":{v}"));
    }
    if let Some(v) = meta.damage {
        out.push(format!("\"minecraft:damage\":{v}"));
    }
    if let Some(v) = meta.max_stack_size {
        out.push(format!("\"minecraft:max_stack_size\":{v}"));
    }
    if let Some(p) = &meta.potion {
        out.push(format!("\"minecraft:potion_contents\":{{potion:{}}}", quoted(&namespaced(p))));
    }
    out
}

fn namespaced(id: &str) -> String {
    if id.contains(':') { id.to_owned() } else { format!("minecraft:{}", id.to_ascii_lowercase()) }
}

fn quoted(s: &str) -> String {
    serde_json::Value::String(s.to_owned()).to_string()
}

/// The components of a base64 meta blob, or the reason it could not be read.
pub fn components_of(b64: &str) -> Result<Vec<String>, String> {
    decode_meta(b64).map(|m| components(&m))
}

/// Standard base64 (with or without padding); `None` on any other byte.
pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut acc: u32 = 0;
    let mut bits = 0;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            b'\r' | b'\n' | b' ' => continue,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// Synthetic meta blobs in Citizens' layout (invented values): a named, enchanted blade with a
/// repair cost, a damage value and an unmapped banner colour; and a potion.
#[cfg(test)]
pub mod fixtures {
    pub const BLADE: &str = "rO0ABXNyABpvcmcuYnVra2l0LnV0aWwuaW8uV3JhcHBlcgAAAAAAAAAAAgABTAADbWFwdAAPTGphdmEvdXRpbC9NYXA7eHBzcgA1Y29tLmdvb2dsZS5jb21tb24uY29sbGVjdC5JbW11dGFibGVNYXAkU2VyaWFsaXplZEZvcm0AAAAAAAAAAAIAAlsABGtleXN0ABNbTGphdmEvbGFuZy9PYmplY3Q7WwAGdmFsdWVzdAATW0xqYXZhL2xhbmcvT2JqZWN0O3hwdXIAE1tMamF2YS5sYW5nLk9iamVjdDsAAAAAAAAAAAIAAHhwAAAAB3QAAj09dAAJbWV0YS10eXBldAAMZGlzcGxheS1uYW1ldAAIZW5jaGFudHN0AAtyZXBhaXItY29zdHQABkRhbWFnZXQACmJhc2UtY29sb3J1cgATW0xqYXZhLmxhbmcuT2JqZWN0OwAAAAAAAAAAAgAAeHAAAAAHdAAISXRlbU1ldGF0AAlFTkNIQU5URUR0AAwiVGVzdCBCbGFkZSJzcgA3Y29tLmdvb2dsZS5jb21tb24uY29sbGVjdC5JbW11dGFibGVCaU1hcCRTZXJpYWxpemVkRm9ybQAAAAAAAAAAAgAAeHIANWNvbS5nb29nbGUuY29tbW9uLmNvbGxlY3QuSW1tdXRhYmxlTWFwJFNlcmlhbGl6ZWRGb3JtAAAAAAAAAAACAAJbAARrZXlzdAATW0xqYXZhL2xhbmcvT2JqZWN0O1sABnZhbHVlc3QAE1tMamF2YS9sYW5nL09iamVjdDt4cHVyABNbTGphdmEubGFuZy5PYmplY3Q7AAAAAAAAAAACAAB4cAAAAAJ0ABNtaW5lY3JhZnQ6c2hhcnBuZXNzdAAUbWluZWNyYWZ0OnVuYnJlYWtpbmd1cgATW0xqYXZhLmxhbmcuT2JqZWN0OwAAAAAAAAAAAgAAeHAAAAACc3IAEWphdmEubGFuZy5JbnRlZ2VyAAAAAAAAAAACAAFJAAV2YWx1ZXhyABBqYXZhLmxhbmcuTnVtYmVyAAAAAAAAAAACAAB4cAAAAAVzcgARamF2YS5sYW5nLkludGVnZXIAAAAAAAAAAAIAAUkABXZhbHVleHIAEGphdmEubGFuZy5OdW1iZXIAAAAAAAAAAAIAAHhwAAAAA3NyABFqYXZhLmxhbmcuSW50ZWdlcgAAAAAAAAAAAgABSQAFdmFsdWV4cgAQamF2YS5sYW5nLk51bWJlcgAAAAAAAAAAAgAAeHAAAAACc3IAEWphdmEubGFuZy5JbnRlZ2VyAAAAAAAAAAACAAFJAAV2YWx1ZXhyABBqYXZhLmxhbmcuTnVtYmVyAAAAAAAAAAACAAB4cAAAAAR0AANSRUQ=";
    pub const POTION: &str = "rO0ABXNyABpvcmcuYnVra2l0LnV0aWwuaW8uV3JhcHBlcgAAAAAAAAAAAgABTAADbWFwdAAPTGphdmEvdXRpbC9NYXA7eHBzcgA1Y29tLmdvb2dsZS5jb21tb24uY29sbGVjdC5JbW11dGFibGVNYXAkU2VyaWFsaXplZEZvcm0AAAAAAAAAAAIAAlsABGtleXN0ABNbTGphdmEvbGFuZy9PYmplY3Q7WwAGdmFsdWVzdAATW0xqYXZhL2xhbmcvT2JqZWN0O3hwdXIAE1tMamF2YS5sYW5nLk9iamVjdDsAAAAAAAAAAAIAAHhwAAAAA3QAAj09dAAJbWV0YS10eXBldAALcG90aW9uLXR5cGV1cgATW0xqYXZhLmxhbmcuT2JqZWN0OwAAAAAAAAAAAgAAeHAAAAADdAAISXRlbU1ldGF0AAZQT1RJT050ABptaW5lY3JhZnQ6c3Ryb25nX3N3aWZ0bmVzcw==";
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::fixtures::{BLADE, POTION};

    #[test]
    fn blade_meta_maps_to_components() {
        let m = decode_meta(BLADE).unwrap();
        assert_eq!(m.display_name.as_deref(), Some("Test Blade"));
        assert_eq!(
            m.enchants,
            vec![("minecraft:sharpness".to_owned(), 5), ("minecraft:unbreaking".to_owned(), 3)]
        );
        assert_eq!(m.repair_cost, Some(2));
        assert_eq!(m.damage, Some(4));
        assert_eq!(m.unmapped, vec!["base-color".to_owned()]);
        let c = components(&m);
        assert!(c[0].starts_with("\"minecraft:custom_name\":"), "{c:?}");
        assert!(c.contains(&"\"minecraft:enchantments\":{levels:{\"minecraft:sharpness\":5,\"minecraft:unbreaking\":3}}".to_owned()), "{c:?}");
        assert!(c.contains(&"\"minecraft:repair_cost\":2".to_owned()));
        assert!(c.contains(&"\"minecraft:damage\":4".to_owned()));
    }

    #[test]
    fn potion_meta_maps_to_potion_contents() {
        let m = decode_meta(POTION).unwrap();
        assert_eq!(m.potion.as_deref(), Some("minecraft:strong_swiftness"));
        assert_eq!(components(&m), vec!["\"minecraft:potion_contents\":{potion:\"minecraft:strong_swiftness\"}".to_owned()]);
    }

    #[test]
    fn a_name_is_a_json_string_literal() {
        assert_eq!(json_string("\"Say \\\"hi\\\"\""), "Say \"hi\"");
        assert_eq!(json_string("plain"), "plain");
    }

    #[test]
    fn broken_meta_is_an_error_not_an_empty_item() {
        assert!(decode_meta("not base64!").is_err());
        assert!(decode_meta("AAAA").is_err(), "no stream header");
        let cut = &BLADE[..BLADE.len() / 2];
        assert!(decode_meta(cut).is_err(), "a cut stream");
    }

    #[test]
    fn base64_reads_padding_and_line_breaks() {
        assert_eq!(base64_decode("aGk="), Some(b"hi".to_vec()));
        assert_eq!(base64_decode("aG\nk="), Some(b"hi".to_vec()));
        assert_eq!(base64_decode("a*"), None);
    }
}
