//! Look-close: an NPC turns its head to the nearest player in range, and otherwise (random look
//! on) to a random direction now and then.

/// A player as the look and click logic needs them.
#[derive(Clone, Debug, PartialEq)]
pub struct PlayerView {
    pub uuid: u128,
    pub name: String,
    pub dimension: String,
    /// Feet.
    pub pos: [f64; 3],
    pub game_mode: i32,
    pub permission: u8,
    pub entity_id: i32,
    /// Sneaking at the time of the click (Citizens' `SHIFT_LEFT`/`SHIFT_RIGHT` commands).
    pub sneaking: bool,
}

pub const EYE_HEIGHT: f64 = 1.62;

/// Yaw and pitch (degrees, the game's convention: yaw 0 looks to +Z, 90 to -X; pitch down is
/// positive) from `from` looking at `to`.
pub fn look_at(from: [f64; 3], to: [f64; 3]) -> (f32, f32) {
    let dx = to[0] - from[0];
    let dy = to[1] - from[1];
    let dz = to[2] - from[2];
    let horizontal = (dx * dx + dz * dz).sqrt();
    let yaw = (-dx).atan2(dz).to_degrees();
    let pitch = -(dy.atan2(horizontal).to_degrees());
    (wrap(yaw as f32), pitch.clamp(-90.0, 90.0) as f32)
}

/// Degrees into -180..180.
pub fn wrap(a: f32) -> f32 {
    let mut a = a % 360.0;
    if a >= 180.0 {
        a -= 360.0;
    }
    if a < -180.0 {
        a += 360.0;
    }
    a
}

/// The smallest difference between two angles.
pub fn angle_diff(a: f32, b: f32) -> f32 {
    wrap(a - b).abs()
}

/// The player an NPC at `npc_eye` in `dimension` looks at: the nearest one within `range`, not a
/// spectator.
pub fn target<'a>(
    players: &'a [PlayerView],
    dimension: &str,
    npc_eye: [f64; 3],
    range: f64,
) -> Option<&'a PlayerView> {
    let r2 = range * range;
    players
        .iter()
        .filter(|p| p.dimension == dimension && p.game_mode != 3)
        .map(|p| (p, dist2(eye(p.pos), npc_eye)))
        .filter(|(_, d)| *d <= r2)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(p, _)| p)
}

pub fn eye(feet: [f64; 3]) -> [f64; 3] {
    [feet[0], feet[1] + EYE_HEIGHT, feet[2]]
}

pub fn dist2(a: [f64; 3], b: [f64; 3]) -> f64 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
}

/// A small deterministic random source for random looks (xorshift).
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed | 1)
    }
    pub fn next_f64(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
    pub fn between(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next_f64()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, z: f64, mode: i32) -> PlayerView {
        PlayerView {
            uuid: 1,
            name: "A".into(),
            dimension: "minecraft:overworld".into(),
            pos: [x, 64.0, z],
            game_mode: mode,
            permission: 0,
            entity_id: 1,
            sneaking: false,
        }
    }

    #[test]
    fn yaw_follows_the_game() {
        let o = [0.0, 0.0, 0.0];
        assert_eq!(look_at(o, [0.0, 0.0, 5.0]).0, 0.0);
        assert!((look_at(o, [-5.0, 0.0, 0.0]).0 - 90.0).abs() < 1e-4);
        assert!((look_at(o, [5.0, 0.0, 0.0]).0 + 90.0).abs() < 1e-4);
        assert!((look_at(o, [0.0, 0.0, -5.0]).0.abs() - 180.0).abs() < 1e-4);
        assert!((look_at(o, [0.0, -5.0, 5.0]).1 - 45.0).abs() < 1e-4, "looking down is positive");
    }

    #[test]
    fn nearest_player_in_range() {
        let players = vec![p(8.0, 0.0, 0), p(3.0, 0.0, 3), p(5.0, 0.0, 1), p(30.0, 0.0, 0)];
        let t = target(&players, "minecraft:overworld", eye([0.0, 64.0, 0.0]), 10.0).unwrap();
        assert_eq!(t.pos[0], 5.0, "the spectator is skipped");
        assert!(target(&players, "minecraft:the_end", eye([0.0, 64.0, 0.0]), 10.0).is_none());
        assert!(target(&players[3..], "minecraft:overworld", eye([0.0, 64.0, 0.0]), 10.0).is_none());
    }

    #[test]
    fn angles() {
        assert_eq!(wrap(190.0), -170.0);
        assert_eq!(wrap(-190.0), 170.0);
        assert_eq!(angle_diff(179.0, -179.0), 2.0);
        let mut r = Rng::new(7);
        let v = r.between(-10.0, 0.0);
        assert!((-10.0..0.0).contains(&v));
    }
}
