//! UUID text and the forms the game takes them in.

use std::hash::{BuildHasher, Hasher};

/// `1b4e28ba-2fa1-11d2-883f-0016d3cca427` (or without dashes) to a number.
pub fn parse_uuid(s: &str) -> Option<u128> {
    let hex: String = s.trim().chars().filter(|c| *c != '-').collect();
    if hex.len() != 32 {
        return None;
    }
    u128::from_str_radix(&hex, 16).ok()
}

pub fn format_uuid(u: u128) -> String {
    let h = format!("{u:032x}");
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

/// The four ints of `UUID:[I;a,b,c,d]`, most significant first.
pub fn int_array(u: u128) -> [i32; 4] {
    [
        (u >> 96) as u32 as i32,
        (u >> 64) as u32 as i32,
        (u >> 32) as u32 as i32,
        u as u32 as i32,
    ]
}

pub fn int_array_snbt(u: u128) -> String {
    let [a, b, c, d] = int_array(u);
    format!("[I;{a},{b},{c},{d}]")
}

/// A random version 4 UUID (std's per-process random hasher keys plus the time).
pub fn random_uuid() -> u128 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let mut hi = std::collections::hash_map::RandomState::new().build_hasher();
    hi.write_u128(now);
    let mut lo = std::collections::hash_map::RandomState::new().build_hasher();
    lo.write_u128(now.rotate_left(64));
    lo.write_u64(hi.finish());
    let u = (u128::from(hi.finish()) << 64) | u128::from(lo.finish());
    with_version(u, 4)
}

/// A stable UUID derived from `base` for its `n`th helper entity (hologram lines).
pub fn derived_uuid(base: u128, n: u32) -> u128 {
    let mut x = base ^ (u128::from(n + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15_F39C_C060_5CED_C834));
    x = x.rotate_left(29) ^ 0x5A17_C0DE;
    with_version(x, 4)
}

fn with_version(u: u128, version: u128) -> u128 {
    let u = (u & !(0xF << 76)) | (version << 76);
    // RFC 4122 variant.
    (u & !(0x3 << 62)) | (0x2 << 62)
}

/// The version nibble of a UUID.
pub fn version(u: u128) -> u8 {
    ((u >> 76) & 0xF) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let s = "1b4e28ba-2fa1-41d2-883f-0016d3cca427";
        let u = parse_uuid(s).unwrap();
        assert_eq!(format_uuid(u), s);
        assert_eq!(parse_uuid("1b4e28ba2fa141d2883f0016d3cca427"), Some(u));
        assert_eq!(parse_uuid("nope"), None);
    }

    #[test]
    fn int_arrays_are_most_significant_first() {
        let u = parse_uuid("00000001-0000-0002-0000-0003ffffffff").unwrap();
        assert_eq!(int_array(u), [1, 2, 3, -1]);
        assert_eq!(int_array_snbt(u), "[I;1,2,3,-1]");
    }

    #[test]
    fn random_and_derived_are_v4() {
        let a = random_uuid();
        assert_eq!(version(a), 4);
        assert_ne!(a, random_uuid());
        let d0 = derived_uuid(a, 0);
        assert_eq!(d0, derived_uuid(a, 0));
        assert_ne!(d0, derived_uuid(a, 1));
        assert_eq!(version(d0), 4);
    }
}
