//! Reconstruction of the per-player 3D-audio listener geometry `FUN_0047ec20`.
//!
//! Every in-race handler calls it just before the engine sound
//! ([`crate::engine_sound`]). It writes the five listener words the engine
//! volume and position helpers read:
//!
//! - `DAT_005ff9cc`: the Q16 dot of the camera heading with the car's
//!   horizontal forward axis (front/rear balance);
//! - `DAT_005ff9ec/e4/d4[p]`: proximity scores `0..=1000` from the camera to
//!   the front emitter, the car centre and the rear emitter;
//! - `DAT_005ff9dc/d0[p]`: the front and rear emitter angles (`±0x400`).
//!
//! The camera heading is `FUN_00434ab0`: a 12-bit yaw from the camera
//! record's forward row (`+0x20/+0x28`, read by `FUN_0041a860`). The camera
//! position is the record's translation (`+0x30..+0x38`, `FUN_0041a800`). The
//! emitters sit at `P ± forward * L`, with `P = car+0xc8`, `forward =
//! car+0x258` and `L = car+0x2c`. This is a description of decompiler
//! pseudocode, not original source. Evidence: `rea.official-eu.engine-sound-tick`.
//!
//! Quirks kept as recovered: the rear emitter is `P + forward * (-L)` (the
//! negation happens before the Q16 multiply), the front angle normalizes a 3D
//! direction while the rear angle normalizes only its horizontal part, and the
//! pre-shift that keeps the squares in range tests and shifts only `x` and `z`.

use crate::engine_sound::EngineListener;
use crate::legacy_math::{signed_arcsine_phase, signed_ratio_angle, sin_q16, sqrt_q16};

/// The camera mode whose emitter angles are measured from the camera itself
/// (other modes measure from the car centre).
pub const CAMERA_RELATIVE_MODE: i32 = 6;

/// Inputs for one player's listener update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListenerInput {
    /// The camera record's world matrix (`0x550498 + p*0x60`), row-major Q16.
    pub camera: [i32; 16],
    /// The camera record's mode word (`0x550494 + p*0x60`).
    pub camera_mode: i32,
    /// `car+0x258`: the car's forward axis.
    pub forward_258: [i32; 3],
    /// `car+0xc8`: the car centre.
    pub centre_c8: [i32; 3],
    /// `car+0x2c`: the emitter half-length.
    pub half_length_2c: i32,
}

/// The listener words, including the centre proximity the engine does not read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListenerOutput {
    pub listener: EngineListener,
    /// `DAT_005ff9e4[p]`.
    pub centre_proximity_9e4: i32,
}

/// A degenerate input the shipped integer path cannot represent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListenerError {
    /// `i32::MIN` reached the heading or arcsine lookup, which the shipped
    /// `NEG` cannot make positive.
    MinimumOperand,
}

impl std::fmt::Display for ListenerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "listener geometry reached an i32::MIN operand")
    }
}

impl std::error::Error for ListenerError {}

fn qmul(a: i32, b: i32) -> i32 {
    ((i64::from(a) * i64::from(b)) >> 16) as i32
}

fn sin(phase: i32) -> i32 {
    sin_q16(phase as u16).raw()
}

fn cos(phase: i32) -> i32 {
    sin(phase.wrapping_add(0x400))
}

/// `FUN_00434ab0`: the 12-bit camera yaw from the forward row's `x` and `z`.
pub fn camera_heading(forward_x: i32, forward_z: i32) -> Result<i16, ListenerError> {
    let s = signed_ratio_angle(forward_x.wrapping_abs(), forward_z.wrapping_abs())
        .map_err(|_| ListenerError::MinimumOperand)?;
    let mut heading = s;
    if forward_z < 0 {
        heading = 0x800_i16.wrapping_sub(s);
    }
    if forward_x < 0 {
        heading = heading.wrapping_neg();
    }
    Ok(heading)
}

/// Divide by 512 truncating toward zero when `x` or `z` exceeds `±0x640000`.
fn preshift(x: &mut i32, z: &mut i32) {
    let big = |v: i32| !(-0x64_0000..=0x64_0000).contains(&v);
    if big(*x) || big(*z) {
        *x /= 512;
        *z /= 512;
    }
}

/// The inline table normalize: `v * (2^32 / sqrt(sum of squares))`, or zero.
fn scale_by_inverse_root(sum: u32, components: &mut [i32]) {
    let root = sqrt_q16(sum) as i32;
    if root == 0 {
        components.iter_mut().for_each(|c| *c = 0);
        return;
    }
    let inverse = (0x1_0000_0000_i64 / i64::from(root)) as i32;
    components.iter_mut().for_each(|c| *c = qmul(*c, inverse));
}

fn normalize_xz(mut x: i32, mut z: i32) -> (i32, i32) {
    preshift(&mut x, &mut z);
    let sum = (qmul(x, x) as u32).wrapping_add(qmul(z, z) as u32);
    let mut v = [x, z];
    scale_by_inverse_root(sum, &mut v);
    (v[0], v[1])
}

fn normalize_xyz(mut x: i32, y: i32, mut z: i32) -> [i32; 3] {
    preshift(&mut x, &mut z);
    let sum = (qmul(x, x) as u32)
        .wrapping_add(qmul(y, y) as u32)
        .wrapping_add(qmul(z, z) as u32);
    let mut v = [x, y, z];
    scale_by_inverse_root(sum, &mut v);
    v
}

/// `((400 - min(400, Σ((c - e) >> 16)²)) * 1000) / 400`, with the shipped
/// wrapping 32-bit square sum.
fn proximity(camera: [i32; 3], emitter: [i32; 3]) -> i32 {
    let mut sum = 0_i32;
    for i in 0..3 {
        let d = camera[i].wrapping_sub(emitter[i]) >> 16;
        sum = sum.wrapping_add(d.wrapping_mul(d));
    }
    let sum = sum.min(400);
    (400 - sum).wrapping_mul(1000) / 400
}

fn offset(centre: [i32; 3], forward: [i32; 3], length: i32) -> [i32; 3] {
    std::array::from_fn(|i| qmul(forward[i], length).wrapping_add(centre[i]))
}

/// The angle and widening shared by both emitters. `(nx, nz)` is the
/// normalized horizontal direction from the reference point to the emitter.
fn emitter_angle(heading: i32, nx: i32, nz: i32) -> Result<i16, ListenerError> {
    let dot = qmul(cos(heading), nx.wrapping_neg()).wrapping_add(qmul(sin(heading), nz));
    let phase = signed_arcsine_phase(dot).map_err(|_| ListenerError::MinimumOperand)?;
    let stored = phase.wrapping_neg();
    // k = ((0x10000 - cos(4 * phase)) * 750) in Q16, 0..=1500.
    let c = sin(i32::from(phase as u16).wrapping_mul(-4).wrapping_add(0x400));
    let k = ((i64::from(0x10000 - c) * 0x2ee_0000) >> 16) as i32 >> 16;
    let widened = (k.wrapping_mul(i32::from(stored)) / 1000) as i16;
    Ok(widened.wrapping_add(stored).clamp(-0x400, 0x400))
}

/// `FUN_0047f6e0(p, point)`: the azimuth (`±90`) of a world point for the
/// impact and breakage cues. The point is taken relative to the camera in
/// mode 6, else to the car centre, normalized in 3D (with the x/z pre-shift),
/// and run through the emitter angle and widening; unlike the engine's
/// emitter position there is no interior check.
pub fn point_azimuth(
    camera: &[i32; 16],
    camera_mode: i32,
    centre_c8: [i32; 3],
    point: [i32; 3],
) -> Result<i32, ListenerError> {
    let heading = i32::from(camera_heading(camera[8], camera[10])?);
    let reference = if camera_mode == CAMERA_RELATIVE_MODE {
        [camera[12], camera[13], camera[14]]
    } else {
        centre_c8
    };
    let d = normalize_xyz(
        point[0].wrapping_sub(reference[0]),
        point[1].wrapping_sub(reference[1]),
        point[2].wrapping_sub(reference[2]),
    );
    let angle = emitter_angle(heading, d[0], d[2])?;
    Ok((i32::from(angle) * 0x1680) >> 16)
}

/// `FUN_0047f9c0(p, v)`: a volume scaled by the car-centre proximity score
/// `DAT_005ff9e4[p]` (`0..=1000`), or a third of it at a score of `0x14d`
/// or less.
pub fn centre_curve(centre_proximity: i32, v: i32) -> i32 {
    if centre_proximity > 0x14d {
        centre_proximity.wrapping_mul(v) / 1000
    } else {
        v / 3
    }
}

/// `FUN_0047ec20(p)`.
pub fn update_listener(input: &ListenerInput) -> Result<ListenerOutput, ListenerError> {
    let cam = &input.camera;
    let heading = i32::from(camera_heading(cam[8], cam[10])?);
    let forward = input.forward_258;
    let centre = input.centre_c8;
    let length = input.half_length_2c;

    let (nx, nz) = normalize_xz(forward[0], forward[2]);
    let heading_dot = qmul(cos(heading), nz).wrapping_add(qmul(sin(heading), nx));

    let camera_pos = [cam[12], cam[13], cam[14]];
    let front = offset(centre, forward, length);
    let rear = offset(centre, forward, length.wrapping_neg());
    let front_proximity = proximity(camera_pos, front);
    let centre_proximity = proximity(camera_pos, centre);
    let rear_proximity = proximity(camera_pos, rear);

    let reference = if input.camera_mode == CAMERA_RELATIVE_MODE {
        camera_pos
    } else {
        centre
    };
    let d = normalize_xyz(
        front[0].wrapping_sub(reference[0]),
        front[1].wrapping_sub(reference[1]),
        front[2].wrapping_sub(reference[2]),
    );
    let front_angle = emitter_angle(heading, d[0], d[2])?;

    let (rx, rz) = normalize_xz(
        rear[0].wrapping_sub(reference[0]),
        rear[2].wrapping_sub(reference[2]),
    );
    let rear_angle = emitter_angle(heading, rx, rz)?;

    Ok(ListenerOutput {
        listener: EngineListener {
            heading_dot_9cc: heading_dot,
            front_angle_9dc: front_angle,
            rear_angle_9d0: rear_angle,
            front_proximity_9ec: front_proximity,
            rear_proximity_9d4: rear_proximity,
        },
        centre_proximity_9e4: centre_proximity,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE: i32 = 0x10000;

    fn camera_at(pos: [i32; 3], forward: [i32; 3]) -> [i32; 16] {
        let mut m = [0; 16];
        m[0] = ONE;
        m[5] = ONE;
        m[8] = forward[0];
        m[9] = forward[1];
        m[10] = forward[2];
        m[12] = pos[0];
        m[13] = pos[1];
        m[14] = pos[2];
        m[15] = ONE;
        m
    }

    #[test]
    fn heading_follows_the_recovered_quadrant_rules() {
        assert_eq!(camera_heading(0, ONE), Ok(0));
        assert_eq!(camera_heading(ONE, 0), Ok(0x400));
        assert_eq!(camera_heading(-ONE, 0), Ok(-0x400));
        // Facing -z: the zero-x gate yields 0, then 0x800 - 0.
        assert_eq!(camera_heading(0, -ONE), Ok(0x800));
        // Equal magnitudes hit the ratio table's zero sentinel.
        assert_eq!(camera_heading(ONE, ONE), Ok(0));
        let diag = camera_heading(ONE, 2 * ONE).unwrap();
        assert!((0x120..=0x140).contains(&diag), "{diag:#x}");
        let back = camera_heading(ONE, -2 * ONE).unwrap();
        assert_eq!(back, 0x800 - diag);
        assert_eq!(camera_heading(-ONE, -2 * ONE).unwrap(), -back);
        assert_eq!(
            camera_heading(i32::MIN, 1),
            Err(ListenerError::MinimumOperand)
        );
    }

    #[test]
    fn proximity_is_full_at_the_emitter_and_zero_beyond_twenty_units() {
        assert_eq!(proximity([0; 3], [0; 3]), 1000);
        assert_eq!(
            proximity([10 * ONE, 0, 0], [0; 3]),
            (400 - 100) * 1000 / 400
        );
        assert_eq!(proximity([20 * ONE, 0, 0], [0; 3]), 0);
        assert_eq!(proximity([100 * ONE, 0, 0], [0; 3]), 0);
    }

    #[test]
    fn preshift_truncates_toward_zero() {
        let (mut x, mut z) = (-0x64_0001, 0x1ff);
        preshift(&mut x, &mut z);
        assert_eq!((x, z), (-0x3200, 0));
        let (mut x, mut z) = (0x64_0000, -0x64_0000);
        preshift(&mut x, &mut z);
        assert_eq!((x, z), (0x64_0000, -0x64_0000));
    }

    #[test]
    fn camera_behind_a_car_facing_away_balances_and_hears_the_rear() {
        // Car at the origin facing +z, camera 6 units behind looking along +z.
        let input = ListenerInput {
            camera: camera_at([0, 0, -6 * ONE], [0, 0, ONE]),
            camera_mode: 5,
            forward_258: [0, 0, ONE],
            centre_c8: [0; 3],
            half_length_2c: 2 * ONE,
        };
        let out = update_listener(&input).unwrap();
        let l = out.listener;
        // Same heading: the dot is ~1.0.
        assert!(
            (0xfff0..=0x10010).contains(&l.heading_dot_9cc),
            "{:#x}",
            l.heading_dot_9cc
        );
        // Rear emitter at z=-2 (16 sq units away), front at z=+2 (64).
        assert_eq!(l.rear_proximity_9d4, (400 - 16) * 1000 / 400);
        assert_eq!(l.front_proximity_9ec, (400 - 64) * 1000 / 400);
        assert_eq!(out.centre_proximity_9e4, (400 - 36) * 1000 / 400);
        // Emitters straight ahead/behind the centre along the view axis: no
        // lateral angle.
        assert_eq!(l.front_angle_9dc, 0);
        assert_eq!(l.rear_angle_9d0, 0);
    }

    #[test]
    fn a_side_view_puts_the_emitters_at_opposite_angles() {
        // Camera on the +x side looking toward -x at a car facing +z.
        let input = ListenerInput {
            camera: camera_at([6 * ONE, 0, 0], [-ONE, 0, 0]),
            camera_mode: 5,
            forward_258: [0, 0, ONE],
            centre_c8: [0; 3],
            half_length_2c: 2 * ONE,
        };
        let l = update_listener(&input).unwrap().listener;
        assert!(l.front_angle_9dc != 0);
        assert_eq!(l.front_angle_9dc, -l.rear_angle_9d0);
        assert!(l.front_angle_9dc.abs() <= 0x400);
        // Perpendicular heading: the balance dot is ~0.
        assert!(l.heading_dot_9cc.abs() < 0x10, "{:#x}", l.heading_dot_9cc);
    }

    #[test]
    fn mode_six_measures_angles_from_the_camera() {
        let base = ListenerInput {
            camera: camera_at([6 * ONE, 0, 0], [-ONE, 0, 0]),
            camera_mode: 5,
            forward_258: [0, 0, ONE],
            centre_c8: [0; 3],
            half_length_2c: 2 * ONE,
        };
        let from_car = update_listener(&base).unwrap().listener;
        let from_camera = update_listener(&ListenerInput {
            camera_mode: CAMERA_RELATIVE_MODE,
            ..base
        })
        .unwrap()
        .listener;
        assert_ne!(from_car.front_angle_9dc, from_camera.front_angle_9dc);
        assert_eq!(
            from_car.front_proximity_9ec,
            from_camera.front_proximity_9ec
        );
    }

    #[test]
    fn a_point_to_the_side_has_a_full_azimuth() {
        // Camera behind a car facing +z, looking +z: a point on the car's
        // right (+x) is at a quarter turn, just short of exactly lateral.
        let camera = camera_at([0, 0, -6 * ONE], [0, 0, ONE]);
        let side = point_azimuth(&camera, 5, [0; 3], [4 * ONE, 0, 1]).unwrap();
        assert!(side.abs() > 80 && side.abs() <= 90, "{side}");
        let ahead = point_azimuth(&camera, 5, [0; 3], [0, 0, 4 * ONE]).unwrap();
        assert_eq!(ahead, 0);
        let other = point_azimuth(&camera, 5, [0; 3], [-4 * ONE, 0, 1]).unwrap();
        // `>> 16` floors, so the mirror image can differ by one.
        assert!(other.signum() == -side.signum() && (other + side).abs() <= 1);
    }

    #[test]
    fn widening_is_zero_on_axis_and_clamped() {
        assert_eq!(emitter_angle(0, 0, ONE), Ok(0));
        // An exactly lateral direction reads the arcsine table's zero sentinel.
        assert_eq!(emitter_angle(0, -ONE, 0), Ok(0));
        // Just short of lateral: phase near 0x400, cos(4 * phase) near 1.
        let a = emitter_angle(0, -(ONE - 16), 0).unwrap();
        assert!(a.abs() > 0x3c0, "{a:#x}");
        // 45 degrees widens by up to 1.5x but stays within the clamp.
        let h = 0xb505;
        let w = emitter_angle(0, -h, h).unwrap();
        assert!(w.abs() > 0x200 && w.abs() <= 0x400, "{w:#x}");
    }
}
