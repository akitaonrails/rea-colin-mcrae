//! The **interior camera placements** (`rea.official-eu.camera-views`): the
//! cockpit (modes `1`/`2`, `FUN_00401090`/`FUN_004010c0`/`FUN_00401230`) and the
//! helmet (mode `8`, `FUN_00467630`/`FUN_004678c0`) views, driven by the exact
//! `.rdata` tables.
//!
//! The cockpit eye sits at a car-space offset (`DAT_004ea040`) placed into world
//! space off the car basis (as the chase placement does), and the interior views
//! pitch the orientation down about the car's right axis by a per-view BAM angle
//! (`DAT_004ea190`), while the mirror/exterior block keeps the car basis level.
//! The helmet view offsets the eye by a per-state `{X, 0, Z}` (`DAT_004f3d70`)
//! and overlays a dynamic "look into the corner" rotation.
//!
//! Reconstructed here: the exact tables, the cockpit eye placement + pitched
//! orientation, and the helmet per-state local offset. The raw-view/state ->
//! table-row remap LUTs (`DAT_0050c510/0050c514/0057c600`), the per-car
//! ride-height terms, and the helmet dynamic look overlay + its look-state
//! selection are runtime/uncovered and left documented.

use crate::c3d_camera::matmul_q16;
use crate::fixed_point::{cos_bam, sin_bam, TRIG_TABLE_LEN};
use crate::gameplay_camera::chase_target_world;

/// Q16.16 one.
const ONE: i32 = 0x1_0000;

/// The cockpit eye offsets (`DAT_004ea040`, Q16.16 `{x, y, z}`, stride `0xc`): 14
/// interior views (rows `0..14`) then 14 mirror/exterior views (rows `14..28`).
/// The row is `within_view + mirror*14`. X is always centered (`0`).
pub const COCKPIT_EYE_OFFSETS: [[i32; 3]; 28] = [
    [0x0000_0000, 0x0001_0000, 0x0000_4ccc],
    [0x0000_0000, 0x0001_0000, 0x0000_0ccc],
    [0x0000_0000, 0x0000_f333, 0x0000_2666],
    [0x0000_0000, 0x0001_0000, 0x0000_1999],
    [0x0000_0000, 0x0000_f333, 0x0000_2666],
    [0x0000_0000, 0x0000_f333, 0x0000_4ccc],
    [0x0000_0000, 0x0001_0ccc, 0x0000_1999],
    [0x0000_0000, 0x0001_0ccc, 0x0000_1999],
    [0x0000_0000, 0x0001_0000, 0x0000_2666],
    [0x0000_0000, 0x0001_0000, 0x0000_2666],
    [0x0000_0000, 0x0001_0000, 0x0000_2666],
    [0x0000_0000, 0x0001_0000, 0x0000_0ccc],
    [0x0000_0000, 0x0001_0000, 0x0000_eb85],
    [0x0000_0000, 0x0000_eb85, 0x0000_5eb8],
    [0x0000_0000, 0x0000_8000, 0x0001_ef7c],
    [0x0000_0000, 0x0000_8000, 0x0001_b4dd],
    [0x0000_0000, 0x0000_8000, 0x0001_ba3d],
    [0x0000_0000, 0x0000_8000, 0x0001_e062],
    [0x0000_0000, 0x0000_8000, 0x0001_c8f5],
    [0x0000_0000, 0x0000_8000, 0x0001_cc6a],
    [0x0000_0000, 0x0000_8000, 0x0001_eb85],
    [0x0000_0000, 0x0000_8000, 0x0001_c47a],
    [0x0000_0000, 0x0000_8000, 0x0001_c000],
    [0x0000_0000, 0x0000_8000, 0x0001_d5c2],
    [0x0000_0000, 0x0000_8000, 0x0001_dc28],
    [0x0000_0000, 0x0000_8000, 0x0001_bae1],
    [0x0000_0000, 0x0000_8000, 0x0001_3305],
    [0x0000_0000, 0x0000_8000, 0x0001_825a],
];

/// The cockpit interior view-pitch (`DAT_004ea190`, BAM angle on the `0x1000`
/// turn, one per interior view): `0x71` (~9.9 deg) for all but view `12`
/// (`0xe3`, ~19.9 deg). Applied only to interior views; the mirror block is level.
pub const COCKPIT_VIEW_PITCH: [u16; 14] = [
    0x71, 0x71, 0x71, 0x71, 0x71, 0x71, 0x71, 0x71, 0x71, 0x71, 0x71, 0x71, 0xe3, 0x71,
];

/// The cockpit eye offset for a `within_view` index (`0..14`) and the
/// mirror/exterior flag (`FUN_00401230`'s `DAT_004ea040[view + mirror*14]`).
#[must_use]
pub fn cockpit_eye_offset(view: usize, mirror: bool) -> [i32; 3] {
    let row = (view % 14) + usize::from(mirror) * 14;
    COCKPIT_EYE_OFFSETS[row]
}

/// The cockpit pitch-down BAM angle for a view (`FUN_004010c0`): the interior
/// `DAT_004ea190[view]`, or `0` for the mirror/exterior block (pitch is skipped
/// when the mirror flag is set).
#[must_use]
pub fn cockpit_view_pitch(view: usize, mirror: bool) -> u16 {
    if mirror {
        0
    } else {
        COCKPIT_VIEW_PITCH[view % 14]
    }
}

/// The 3x3 orientation of a world matrix as a rotation-only matrix (zero
/// translation), for composing further rotations.
fn orientation_only(m: &[i32; 16]) -> [i32; 16] {
    [
        m[0], m[1], m[2], 0,
        m[4], m[5], m[6], 0,
        m[8], m[9], m[10], 0,
        0, 0, 0, ONE,
    ]
}

/// A rotation of `angle` (BAM) about the right (X) axis, pitching the forward
/// axis toward `-up` for a positive angle (the cockpit's downward pitch); the
/// runtime-built sine table is injected.
#[must_use]
pub fn rotation_x_bam(angle: u16, trig: &[i32; TRIG_TABLE_LEN]) -> [i32; 16] {
    let s = sin_bam(trig, angle).raw();
    let c = cos_bam(trig, angle).raw();
    [
        ONE, 0, 0, 0,
        0, c, s, 0,
        0, -s, c, 0,
        0, 0, 0, ONE,
    ]
}

/// The cockpit camera world matrix for a view: the eye at `car_pos + car_basis *
/// eye_offset` (as the chase placement), oriented on the car basis and, for an
/// interior view, pitched down by [`cockpit_view_pitch`]. The per-car ride-height
/// terms added to the eye in `FUN_00401230` are runtime car data and omitted.
#[must_use]
pub fn cockpit_camera(car_world: &[i32; 16], view: usize, mirror: bool, trig: &[i32; TRIG_TABLE_LEN]) -> [i32; 16] {
    let base = chase_target_world(car_world, cockpit_eye_offset(view, mirror));
    let pitch = cockpit_view_pitch(view, mirror);
    let orient = if pitch == 0 {
        orientation_only(&base)
    } else {
        matmul_q16(&rotation_x_bam(pitch, trig), &orientation_only(car_world))
    };
    [
        orient[0], orient[1], orient[2], 0,
        orient[4], orient[5], orient[6], 0,
        orient[8], orient[9], orient[10], 0,
        base[12], base[13], base[14], ONE,
    ]
}

/// A helmet per-state record (`DAT_004f3d70`, stride `0x20`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HelmetState {
    /// `+0x00`: the yaw gain (BAM) for the look overlay.
    pub yaw_gain: i16,
    /// `+0x04`: the X scalar (Q16.16), biased by `0x20000` (2.0).
    pub x_scalar: i32,
    /// `+0x08`: the base Z (Q16.16), biased by `0xa0000` (10.0).
    pub base_z: i32,
    /// `+0x0c`: the rotation scalar (Q16.16, `*360` -> trig for the look overlay).
    pub rot_scalar: i32,
    /// `+0x10..0x1c`: the four head ease targets (Q16.16).
    pub ease: [i32; 4],
}

/// The helmet per-state table (`DAT_004f3d70`, 6 states).
pub const HELMET_STATES: [HelmetState; 6] = [
    HelmetState { yaw_gain: 0x0000, x_scalar: 0x0000_0000, base_z: -0x0004_0000, rot_scalar: 0x0000_6ccc, ease: [0x0000_b333; 4] },
    HelmetState { yaw_gain: 0x00e3, x_scalar: 0x0008_0000, base_z: -0x0002_0000, rot_scalar: 0x0000_0000, ease: [0x0001_0000; 4] },
    HelmetState { yaw_gain: 0x03f4, x_scalar: 0x0002_0000, base_z: -0x0014_0000, rot_scalar: 0x0001_0000, ease: [0x0001_0000; 4] },
    HelmetState { yaw_gain: 0x0000, x_scalar: 0x0001_0000, base_z: -0x0003_0000, rot_scalar: 0x0000_c000, ease: [0x0001_0000; 4] },
    HelmetState { yaw_gain: 0x038e, x_scalar: -0x000a_0000, base_z: -0x001e_0000, rot_scalar: -0x0000_6666, ease: [0x0001_0000, 0x0001_0000, 0x0000_b333, 0x0001_0000] },
    HelmetState { yaw_gain: 0x038e, x_scalar: -0x000a_0000, base_z: -0x001e_0000, rot_scalar: 0x0000_6666, ease: [0x0001_0000, 0x0001_0000, 0x0000_b333, 0x0001_0000] },
];

/// The `0x20000` (2.0) X bias and `0xa0000` (10.0) Z bias of `FUN_004678c0`.
const HELMET_X_BIAS: i32 = 0x0002_0000;
const HELMET_Z_BIAS: i32 = 0x000a_0000;

/// The helmet eye's car-space local offset for a state (`FUN_004678c0`), with the
/// eased head params `px`/`pz` (Q16.16, `0x10000` = neutral): `X = (x_scalar -
/// 0x20000)*px + 0x20000`, `Y = 0`, `Z = (base_z + 0xa0000)*pz - 0xa0000`. Neutral
/// params reproduce `{x_scalar, 0, base_z}`.
#[must_use]
pub fn helmet_local_offset(state: &HelmetState, px: i32, pz: i32) -> [i32; 3] {
    let q_mul = |a: i32, b: i32| ((i64::from(a) * i64::from(b)) >> 16) as i32;
    let x = q_mul(state.x_scalar - HELMET_X_BIAS, px) + HELMET_X_BIAS;
    let z = q_mul(state.base_z + HELMET_Z_BIAS, pz) - HELMET_Z_BIAS;
    [x, 0, z]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixed_point::build_sine_table;

    /// An identity-oriented car at `pos` (world matrix: identity basis +
    /// translation at 12/13/14).
    fn car_at(pos: [i32; 3]) -> [i32; 16] {
        [
            ONE, 0, 0, 0,
            0, ONE, 0, 0,
            0, 0, ONE, 0,
            pos[0], pos[1], pos[2], ONE,
        ]
    }

    #[test]
    fn cockpit_tables_match_the_dump() {
        assert_eq!(COCKPIT_EYE_OFFSETS.len(), 28);
        // Interior view 0 and mirror view 0.
        assert_eq!(cockpit_eye_offset(0, false), [0, 0x0001_0000, 0x0000_4ccc]);
        assert_eq!(cockpit_eye_offset(0, true), [0, 0x0000_8000, 0x0001_ef7c]);
        // Mirror eyes sit at half height (0.5).
        for v in 0..14 {
            assert_eq!(cockpit_eye_offset(v, true)[1], 0x0000_8000);
        }
        // Pitch: 0x71 everywhere interior but view 12 (0xe3); mirror is level.
        assert_eq!(cockpit_view_pitch(0, false), 0x71);
        assert_eq!(cockpit_view_pitch(12, false), 0xe3);
        assert_eq!(cockpit_view_pitch(12, true), 0);
    }

    #[test]
    fn mirror_view_keeps_the_car_basis_and_places_the_eye() {
        let car = car_at([0x10_0000, 0, 0x20_0000]); // (16, 0, 32)
        let m = cockpit_camera(&car, 0, true, &build_sine_table());
        // Orientation stays the (identity) car basis.
        assert_eq!([m[0], m[1], m[2]], [ONE, 0, 0]);
        assert_eq!([m[8], m[9], m[10]], [0, 0, ONE]);
        // Eye = car_pos + basis * offset (offset x=0, so x/z = car + offset).
        let off = cockpit_eye_offset(0, true);
        assert_eq!(m[12], 0x10_0000 + off[0]);
        assert_eq!(m[13], off[1]);
        assert_eq!(m[14], 0x20_0000 + off[2]);
    }

    #[test]
    fn interior_view_pitches_the_forward_axis_down() {
        let car = car_at([0, 0, 0]);
        let m = cockpit_camera(&car, 0, false, &build_sine_table());
        // The pitched forward row (indices 8,9,10) tilts toward -Y (down).
        assert!(m[9] < 0, "forward should gain a downward (-Y) component, got {}", m[9]);
        // Still a forward-ish, unit-length basis row.
        assert!(m[10] > 0);
        // A zero-pitch (mirror) view leaves the forward level for comparison.
        let level = cockpit_camera(&car, 0, true, &build_sine_table());
        assert_eq!(level[9], 0);
    }

    #[test]
    fn helmet_states_match_the_dump_and_neutral_offset_is_scalar_basez() {
        assert_eq!(HELMET_STATES.len(), 6);
        assert_eq!(HELMET_STATES[0].base_z, -0x0004_0000); // -4.0
        assert_eq!(HELMET_STATES[1].x_scalar, 0x0008_0000); // 8.0
        // Neutral head params reproduce {x_scalar, 0, base_z}.
        for st in &HELMET_STATES {
            assert_eq!(helmet_local_offset(st, ONE, ONE), [st.x_scalar, 0, st.base_z]);
        }
        // A half-param pulls X and Z toward their biases.
        let st = &HELMET_STATES[1]; // x_scalar 8.0, base_z -2.0
        let half = helmet_local_offset(st, 0x8000, 0x8000);
        assert_eq!(half[0], (0x0008_0000 - 0x0002_0000) / 2 + 0x0002_0000); // (6.0)/2 + 2.0 = 5.0
        assert_eq!(half[2], (-0x0002_0000 + 0x000a_0000) / 2 - 0x000a_0000); // (8.0)/2 - 10.0 = -6.0
    }
}
