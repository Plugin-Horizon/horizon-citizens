//! Skin lookups: a player name to its profile id (`api.mojang.com`), the id to the signed
//! `textures` property (`sessionserver.mojang.com`, `?unsigned=false`). The HTTP calls are the
//! glue's (off the server thread); this reads the answers.

use crate::config::Skins;
use serde_json::Value;

pub fn name_url(cfg: &Skins, name: &str) -> String {
    format!("{}{}", cfg.name_lookup_url, name)
}

pub fn profile_url(cfg: &Skins, id: u128) -> String {
    format!("{}{:032x}?unsigned=false", cfg.profile_url, id)
}

/// `{"id": "<32 hex>", "name": "..."}` -> (id, name with its real case).
pub fn parse_name_lookup(body: &str) -> Option<(u128, String)> {
    let v: Value = serde_json::from_str(body).ok()?;
    let id = crate::ids::parse_uuid(v.get("id")?.as_str()?)?;
    Some((id, v.get("name")?.as_str()?.to_owned()))
}

/// The `textures` property of a session server profile: (value, signature).
pub fn parse_profile(body: &str) -> Option<(String, Option<String>)> {
    let v: Value = serde_json::from_str(body).ok()?;
    v.get("properties")?.as_array()?.iter().find_map(|p| {
        (p.get("name")?.as_str()? == "textures").then(|| {
            (
                p.get("value").and_then(Value::as_str).unwrap_or_default().to_owned(),
                p.get("signature").and_then(Value::as_str).map(str::to_owned),
            )
        })
    })
}

/// A finished lookup, handed back to the server thread.
#[derive(Clone, Debug, PartialEq)]
pub struct Fetched {
    pub npc: i32,
    pub requested: String,
    pub result: Result<FetchedSkin, String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FetchedSkin {
    pub profile: u128,
    pub name: String,
    pub texture: String,
    pub signature: Option<String>,
}

/// Runs a whole lookup with `get` (a blocking HTTP GET returning the body, or an error; 204 / 404
/// should be an error).
pub fn fetch(cfg: &Skins, name: &str, get: impl Fn(&str) -> Result<String, String>) -> Result<FetchedSkin, String> {
    let (id, real) = parse_name_lookup(&get(&name_url(cfg, name))?)
        .ok_or_else(|| format!("no Minecraft account named {name}"))?;
    let (texture, signature) =
        parse_profile(&get(&profile_url(cfg, id))?).ok_or_else(|| format!("{name} has no skin texture"))?;
    Ok(FetchedSkin {
        profile: id,
        name: real,
        texture,
        signature,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lookup_reads_both_answers() {
        let cfg = Skins::default();
        let got = fetch(&cfg, "steve", |url| {
            if url.contains("api.mojang.com") {
                assert!(url.ends_with("/steve"));
                Ok(r#"{"id":"0123456789abcdef0123456789abcdef","name":"Steve"}"#.into())
            } else {
                assert!(url.ends_with("0123456789abcdef0123456789abcdef?unsigned=false"));
                Ok(r#"{"id":"x","name":"Steve","properties":[{"name":"textures","value":"VkFM","signature":"U0lH"}]}"#.into())
            }
        })
        .unwrap();
        assert_eq!(got.name, "Steve");
        assert_eq!(got.texture, "VkFM");
        assert_eq!(got.signature.as_deref(), Some("U0lH"));
    }

    #[test]
    fn unknown_names_fail() {
        let cfg = Skins::default();
        assert!(fetch(&cfg, "nobody", |_| Err("404".into())).is_err());
        assert!(fetch(&cfg, "x", |_| Ok("{}".into())).is_err());
    }
}
