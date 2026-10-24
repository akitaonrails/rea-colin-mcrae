//! The **trackside / cinematic camera** director's selection logic
//! (`rea.official-eu.camera-modes`, mode `0xb`): choosing the nearest trackside
//! point, the in-range gate, and the dwell timing.
//!
//! The candidate points are a fixed array `DAT_008294e4` (stride `0x10`); the
//! director `FUN_00435ba0`/driver tail `FUN_004343a0` pick the nearest
//! (`FUN_00486340`) when the viewed car is in range and switch to it after a
//! dwell interval. The points table is runtime/stage data (empty in the static
//! image), so it is taken as a slice here; the selection math and the recovered
//! range/dwell constants are reconstructed.

/// A trackside camera candidate (`DAT_008294e4`, stride `0x10`): a Q16.16 world
/// position plus a flags byte (`+0xc`), where a non-zero flag marks a panning cam
/// that tracks the car rather than staring from a fixed pose.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TracksidePoint {
    /// Q16.16 world X (`+0`).
    pub x: i32,
    /// Q16.16 world Y (`+4`).
    pub y: i32,
    /// Q16.16 world Z (`+8`).
    pub z: i32,
    /// Behaviour flags (`+0xc`); non-zero = panning cam.
    pub flags: u8,
}

impl TracksidePoint {
    /// Whether this is a panning cam (flags `!= 0`) that eases its look-at toward
    /// the car, versus a fixed stare.
    #[must_use]
    pub const fn is_panning(self) -> bool {
        self.flags != 0
    }
}

/// The per-axis rejection distance (`0x4b_0000` = 75.0 Q16.16): a candidate is
/// out of range if the car is this far on any single axis.
pub const TRACKSIDE_RANGE_AXIS_Q16: i32 = 0x4b_0000;

/// The squared 2-D (X,Z) rejection distance (`0x15f9_0000` = 75.0^2 in Q16.16
/// units^2).
pub const TRACKSIDE_RANGE_DIST2_Q16: i32 = 0x15f9_0000;

/// The base trackside dwell length in frames before the PAL doubling (`0x4b` =
/// 75).
const TRACKSIDE_DWELL_FRAMES: i32 = 0x4b;

/// The 2-D (X,Z) squared distance between a point and the car, in Q16.16 units^2
/// (`(dx^2 + dz^2) >> 16`), as `i64` to avoid overflow.
#[must_use]
fn dist2_xz_q16(point: TracksidePoint, car_x: i32, car_z: i32) -> i64 {
    let dx = i64::from(point.x) - i64::from(car_x);
    let dz = i64::from(point.z) - i64::from(car_z);
    (dx * dx + dz * dz) >> 16
}

/// The nearest trackside point to the car in the 2-D (X,Z) plane (`FUN_00486340`
/// without the optional 3-D refine), or `None` for an empty set.
#[must_use]
pub fn nearest_trackside(points: &[TracksidePoint], car_x: i32, car_z: i32) -> Option<usize> {
    points
        .iter()
        .enumerate()
        .min_by_key(|(_, p)| dist2_xz_q16(**p, car_x, car_z))
        .map(|(i, _)| i)
}

/// Whether a candidate is close enough to use (the driver's accept gate): no
/// per-axis delta reaches [`TRACKSIDE_RANGE_AXIS_Q16`] and the 2-D squared
/// distance is below [`TRACKSIDE_RANGE_DIST2_Q16`].
#[must_use]
pub fn trackside_in_range(point: TracksidePoint, car: [i32; 3]) -> bool {
    let dx = (i64::from(point.x) - i64::from(car[0])).abs();
    let dy = (i64::from(point.y) - i64::from(car[1])).abs();
    let dz = (i64::from(point.z) - i64::from(car[2])).abs();
    let axis = i64::from(TRACKSIDE_RANGE_AXIS_Q16);
    if dx >= axis || dy >= axis || dz >= axis {
        return false;
    }
    dist2_xz_q16(point, car[0], car[2]) < i64::from(TRACKSIDE_RANGE_DIST2_Q16)
}

/// The trackside **entry** dwell in frames (`(pal+1)*0x4b`, 75/150): the director
/// counts up to this while the camera is *not* in trackside mode, and only then
/// may enter it (the exit back to chase is RNG-driven, see
/// [`trackside_director_exit`], not this dwell).
#[must_use]
pub fn trackside_dwell_frames(pal: bool) -> i32 {
    TRACKSIDE_DWELL_FRAMES * (i32::from(pal) + 1)
}

/// The panning-cam look-at ease coefficient (`FUN_004864f0`):
/// `0x8554d0000 / ((pal+1)*0x190000)`, which the recovered numerator makes exact
/// at `0x5555 / (pal+1)` - `21845` (NTSC, a `1/3` ease per frame) / `10922` (PAL,
/// `1/6`). The smoothed look-at point moves toward the car by this fraction.
#[must_use]
pub fn trackside_lookat_ease_coeff(pal: bool) -> i32 {
    (0x8_554d_0000i64 / ((i64::from(pal) + 1) * 0x19_0000)) as i32
}

/// Ease a panning cam's smoothed look-at point toward the live car position by
/// `coeff` (Q16.16), componentwise: `smoothed += (car - smoothed) * coeff >> 16`
/// (`FUN_004864f0`; the per-car state is `DAT_008294f8/fc/00829500[car*0xc]`). A
/// fixed (non-panning) cam snaps instead - the caller uses `car` directly.
#[must_use]
pub fn trackside_lookat_ease(smoothed: [i32; 3], car: [i32; 3], coeff: i32) -> [i32; 3] {
    let mut out = smoothed;
    for i in 0..3 {
        let diff = i64::from(car[i]) - i64::from(smoothed[i]);
        out[i] = (i64::from(smoothed[i]) + ((diff * i64::from(coeff)) >> 16)) as i32;
    }
    out
}

/// The chase mode the director's RNG exit falls back to when it leaves the
/// trackside cam (`FUN_004343a0`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DirectorExitMode {
    /// Mode `9`: the replay action cam (`rand` fraction `< 0x8000`).
    ReplayAction,
    /// Mode `10`: the third chase (`rand` fraction `>= 0x8000`).
    ThirdChase,
}

impl DirectorExitMode {
    /// The raw camera-mode value (`9` or `10`).
    #[must_use]
    pub const fn raw(self) -> u32 {
        match self {
            DirectorExitMode::ReplayAction => 9,
            DirectorExitMode::ThirdChase => 10,
        }
    }
}

/// The 20%-per-frame exit probability threshold (`0x3333` of a Q16.16 fraction).
pub const DIRECTOR_EXIT_PROBABILITY_Q16: i32 = 0x3333;

/// The director's trackside exit roll (`FUN_004343a0`): a first RNG draw below
/// [`DIRECTOR_EXIT_PROBABILITY_Q16`] (~20%) authorizes leaving the trackside cam,
/// and a second draw picks the destination - `< 0x8000` (50/50) is the replay
/// action cam, else the third chase. `None` keeps the trackside cam this frame.
#[must_use]
pub fn trackside_director_exit(rng: &mut crate::game_rng::GameLcg) -> Option<DirectorExitMode> {
    if rng.next_q16() >= DIRECTOR_EXIT_PROBABILITY_Q16 {
        return None;
    }
    Some(if rng.next_q16() < 0x8000 {
        DirectorExitMode::ReplayAction
    } else {
        DirectorExitMode::ThirdChase
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(x: i32, z: i32, flags: u8) -> TracksidePoint {
        TracksidePoint { x, y: 0, z, flags }
    }

    #[test]
    fn range_constants_match_recovery() {
        assert_eq!(TRACKSIDE_RANGE_AXIS_Q16, 75 * 0x1_0000);
        // 0x15f90000 >> 16 == 5625 == 75^2.
        assert_eq!(TRACKSIDE_RANGE_DIST2_Q16 >> 16, 75 * 75);
        assert_eq!(trackside_dwell_frames(false), 75);
        assert_eq!(trackside_dwell_frames(true), 150);
    }

    #[test]
    fn nearest_picks_the_closest_in_xz() {
        let points = [
            pt(0x64_0000, 0, 0), // 100.0 away on X
            pt(0x0a_0000, 0, 0), // 10.0 away on X (nearest)
            pt(0, 0x32_0000, 0), // 50.0 away on Z
        ];
        assert_eq!(nearest_trackside(&points, 0, 0), Some(1));
        assert_eq!(nearest_trackside(&[], 0, 0), None);
        // Y is ignored in the 2-D selection.
        let only = [pt(0x1_0000, 0, 0)];
        assert_eq!(nearest_trackside(&only, 0, 0), Some(0));
    }

    #[test]
    fn in_range_gate_rejects_far_axis_and_distance() {
        // 10.0 on X and Z: well inside both gates.
        assert!(trackside_in_range(pt(0x0a_0000, 0x0a_0000, 0), [0, 0, 0]));
        // 80.0 on a single axis: beyond the 75.0 axis gate.
        assert!(!trackside_in_range(pt(0x50_0000, 0, 0), [0, 0, 0]));
        // 60.0 on X and 60.0 on Z: each axis is in, but the 2-D distance
        // (sqrt(2)*60 ~= 84.9 > 75) fails the squared-distance gate.
        assert!(!trackside_in_range(pt(0x3c_0000, 0x3c_0000, 0), [0, 0, 0]));
        // A large Y delta also rejects.
        assert!(!trackside_in_range(pt(0, 0x50_0000, 0), [0, 0, 0]));
    }

    #[test]
    fn lookat_ease_coeff_is_one_third_per_frame() {
        assert_eq!(trackside_lookat_ease_coeff(false), 0x5555); // 21845, ~1/3
        assert_eq!(trackside_lookat_ease_coeff(true), 0x2aaa); // 10922, ~1/6
        // One NTSC step moves ~1/3 of the way from 0 toward 3.0.
        let stepped = trackside_lookat_ease(
            [0, 0, 0],
            [0x3_0000, 0, 0],
            trackside_lookat_ease_coeff(false),
        );
        assert!(
            (stepped[0] - 0x1_0000).abs() <= 2,
            "~1.0 after one 1/3 step, got {:#x}",
            stepped[0]
        );
        // Already-at-target stays put.
        assert_eq!(
            trackside_lookat_ease(
                [0x3_0000, 0, 0],
                [0x3_0000, 0, 0],
                trackside_lookat_ease_coeff(false)
            ),
            [0x3_0000, 0, 0]
        );
    }

    #[test]
    fn director_exit_roll_authorizes_then_picks_mode() {
        use crate::game_rng::GameLcg;
        // Over many draws the exit fires ~20% of the time and splits ~50/50.
        let mut rng = GameLcg::new(0xC0FFEE);
        let (mut exits, mut replay, mut third) = (0u32, 0u32, 0u32);
        for _ in 0..4000 {
            match trackside_director_exit(&mut rng) {
                None => {}
                Some(DirectorExitMode::ReplayAction) => {
                    exits += 1;
                    replay += 1;
                }
                Some(DirectorExitMode::ThirdChase) => {
                    exits += 1;
                    third += 1;
                }
            }
        }
        // ~20% exits (allow a wide band for the sample size).
        assert!(
            (600..1000).contains(&exits),
            "exit rate out of band: {exits}/4000"
        );
        // The two modes are both produced and roughly balanced.
        assert!(replay > 0 && third > 0);
        let ratio = replay as f64 / exits as f64;
        assert!((0.35..0.65).contains(&ratio), "mode split skewed: {ratio}");
        assert_eq!(DirectorExitMode::ReplayAction.raw(), 9);
        assert_eq!(DirectorExitMode::ThirdChase.raw(), 10);
    }
}
