//! Skin lookups: a player name to its profile id (`api.mojang.com`), the id to the signed
//! `textures` property (`sessionserver.mojang.com`, `?unsigned=false`). The HTTP calls are the
//! glue's (off the server thread); this reads the answers.

use crate::horizon_citizens_core::config::Skins;
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
    let id = crate::horizon_citizens_core::ids::parse_uuid(v.get("id")?.as_str()?)?;
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

/// Skin jobs that run off the server thread (`/npc skin --url|--file|-e`).
#[derive(Clone, Debug, PartialEq)]
pub enum Work {
    /// mineskin.org makes a signed skin from a URL or a PNG (`MojangSkinGenerator`).
    Generate { source: String, png: Option<Vec<u8>>, slim: bool },
    /// Save the skin's PNG (at most 10000 bytes, as the original copies) to `path`.
    Export { url: String, path: String },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Generated {
    pub uuid: String,
    pub value: String,
    pub signature: Option<String>,
}

/// A finished job: who asked, what it was called, and the answer.
#[derive(Clone, Debug, PartialEq)]
pub struct Done {
    pub npc: i32,
    pub by: Option<u128>,
    /// The URL or file name the player gave (or the exported file's name).
    pub label: String,
    pub export: bool,
    pub result: Result<Option<Generated>, String>,
}

/// mineskin's answer: `data.uuid` and `data.texture.value`/`signature`.
pub fn parse_mineskin(body: &str) -> Option<Generated> {
    let v: Value = serde_json::from_str(body).ok()?;
    let d = v.get("data")?;
    let t = d.get("texture")?;
    Some(Generated {
        uuid: d.get("uuid")?.as_str()?.to_owned(),
        value: t.get("value")?.as_str()?.to_owned(),
        signature: t.get("signature").and_then(Value::as_str).map(str::to_owned),
    })
}

/// The skin URL inside a texture property (`textures.SKIN.url`).
pub fn texture_url(texture_b64: &str, decode: impl Fn(&str) -> Option<Vec<u8>>) -> Result<String, String> {
    let bytes = decode(texture_b64).ok_or("invalid base64")?;
    let v: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    v.get("textures")
        .and_then(|t| t.get("SKIN"))
        .and_then(|s| s.get("url"))
        .and_then(Value::as_str)
        .map(|u| u.replace('\\', ""))
        .ok_or_else(|| "no skin url".to_owned())
}

#[cfg(test)]
mod tests {
    #[test]
    fn mineskin_and_texture_urls() {
        let g = super::parse_mineskin(r#"{"data":{"uuid":"u","texture":{"value":"v","signature":"s"}}}"#).unwrap();
        assert_eq!((g.uuid.as_str(), g.value.as_str(), g.signature.as_deref()), ("u", "v", Some("s")));
        assert!(super::parse_mineskin("{}").is_none());
        let url = super::texture_url("x", |_| Some(br#"{"textures":{"SKIN":{"url":"http://textures.minecraft.net/a"}}}"#.to_vec()));
        assert_eq!(url.unwrap(), "http://textures.minecraft.net/a");
    }

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
