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

/// The trackside dwell length in frames (`(pal+1)*0x4b`, 75/150), the interval the
/// director holds a trackside cam before re-evaluating.
#[must_use]
pub fn trackside_dwell_frames(pal: bool) -> i32 {
    TRACKSIDE_DWELL_FRAMES * (i32::from(pal) + 1)
}

// NOT reconstructed here (left for a verification pass, not invented): the
// panning cam's look-at ease coefficient `0x8554d0000/((pal+1)*0x190000)` from
// `FUN_004864f0` - the recovered numerator and the stated quotient (~21856)
// disagree under exact arithmetic, so the constant is left to re-derive rather
// than pin a wrong value; and the director's RNG-driven exit (mode 9 vs 10).

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
            pt(0x64_0000, 0, 0),       // 100.0 away on X
            pt(0x0a_0000, 0, 0),       // 10.0 away on X (nearest)
            pt(0, 0x32_0000, 0),       // 50.0 away on Z
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
}
