//! The **cinematic / TV camera** placement (`rea.official-eu.camera-views`):
//! modes `7` (fixed bonnet-TV) and `9` (randomized replay action cam), the shared
//! builder `FUN_0046bc80` / evaluator `FUN_0046c520`.
//!
//! Unlike the chase camera, these place the eye at a **world-space** offset from
//! the car (not rotated by the car basis) and orient it by a **look-at toward the
//! car**, so the shot holds a fixed angle while the car moves through it. Mode `7`
//! uses a fixed offset direction; mode `9` randomizes the offset each shot from
//! the game LCG ([`crate::game_rng::GameLcg`]).
//!
//! The engine normalizes with a runtime reciprocal-sqrt table (`DAT_0082b380`);
//! this reconstruction uses a real square root as the equivalent backend
//! realization (the table holds a standard `1/sqrt`, not game-specific data).

use crate::game_rng::GameLcg;

/// Q16.16 one (`1.0`).
const ONE: i32 = 0x1_0000;

/// A Q16.16 multiply `(a*b) >> 16`.
fn q_mul(a: i32, b: i32) -> i32 {
    ((i64::from(a) * i64::from(b)) >> 16) as i32
}

fn to_f(q: i32) -> f64 {
    f64::from(q) / 65536.0
}

fn from_f(f: f64) -> i32 {
    (f * 65536.0) as i32
}

fn vlen(v: [i32; 3]) -> f64 {
    let (x, y, z) = (to_f(v[0]), to_f(v[1]), to_f(v[2]));
    (x * x + y * y + z * z).sqrt()
}

fn normalize(v: [i32; 3]) -> [f64; 3] {
    let len = vlen(v);
    if len == 0.0 {
        return [0.0, 0.0, 0.0];
    }
    [to_f(v[0]) / len, to_f(v[1]) / len, to_f(v[2]) / len]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn norm_f(v: [f64; 3]) -> [f64; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len == 0.0 {
        [0.0, 0.0, 0.0]
    } else {
        [v[0] / len, v[1] / len, v[2] / len]
    }
}

/// The fixed cinematic offset direction scaled to its distance (mode `7`):
/// `normalize(1.0, 2.0, 1.0) * 20.0` (`0x14_0000`) - a high three-quarter TV
/// angle, ~`(8.16, 16.33, 8.16)` Q16.16.
#[must_use]
pub fn cinematic_offset_fixed() -> [i32; 3] {
    let dir = normalize([ONE, 2 * ONE, ONE]);
    [
        from_f(dir[0] * 20.0),
        from_f(dir[1] * 20.0),
        from_f(dir[2] * 20.0),
    ]
}

/// The randomized cinematic offset for a replay action shot (mode `9`,
/// `FUN_0046bc80` with `param_3 == 0`), drawn from `rng` in the recovered order:
///
/// 1. `dist = r1*5.5 + 4.5` in `[4.5, 10.0)`;
/// 2. `x = r2*2*dist - dist` in `[-dist, dist)`;
/// 3. `z = r3*2*dist - dist`;
/// 4. `y = r4*0.5*len(x,z) + 0.5` (always above the ground);
/// 5. the `(x, y, z)` direction renormalized to length `dist`.
#[must_use]
pub fn cinematic_offset_random(rng: &mut GameLcg) -> [i32; 3] {
    let dist = q_mul(rng.next_q16(), 0x5_8000) + 0x4_8000; // [4.5, 10.0) Q16.16
    let two_dist = dist * 2;
    let x = q_mul(rng.next_q16(), two_dist) - dist;
    let z = q_mul(rng.next_q16(), two_dist) - dist;
    let len_xz = from_f((to_f(x) * to_f(x) + to_f(z) * to_f(z)).sqrt());
    let y = q_mul(rng.next_q16(), len_xz / 2) + 0x8000; // r4*0.5*len + 0.5
    let dir = normalize([x, y, z]);
    let d = to_f(dist);
    [from_f(dir[0] * d), from_f(dir[1] * d), from_f(dir[2] * d)]
}

/// Build the cinematic camera world matrix (`[i32;16]` Q16.16, row-major: right
/// `0..3`, up `4..7`, forward `8..11`, translation `12..15`). The eye sits at
/// `car_pos + world_offset`; the orientation looks back toward the car: `forward
/// = normalize(car - eye) = normalize(-world_offset)`, `right = normalize(up_ref
/// x forward)`, `up = forward x right`. `up_ref` is the car forward (mode `7`) or
/// a world-up jittered reference (mode `9`).
#[must_use]
pub fn cinematic_camera(car_pos: [i32; 3], world_offset: [i32; 3], up_ref: [i32; 3]) -> [i32; 16] {
    let eye = [
        car_pos[0].wrapping_add(world_offset[0]),
        car_pos[1].wrapping_add(world_offset[1]),
        car_pos[2].wrapping_add(world_offset[2]),
    ];
    let forward = normalize([-world_offset[0], -world_offset[1], -world_offset[2]]);
    let up_ref = norm_f([to_f(up_ref[0]), to_f(up_ref[1]), to_f(up_ref[2])]);
    let right = norm_f(cross(up_ref, forward));
    let up = cross(forward, right);
    [
        from_f(right[0]),
        from_f(right[1]),
        from_f(right[2]),
        0,
        from_f(up[0]),
        from_f(up[1]),
        from_f(up[2]),
        0,
        from_f(forward[0]),
        from_f(forward[1]),
        from_f(forward[2]),
        0,
        eye[0],
        eye[1],
        eye[2],
        ONE,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn len3(v: [i32; 3]) -> f64 {
        vlen(v)
    }

    #[test]
    fn fixed_offset_points_high_and_three_quarter() {
        let o = cinematic_offset_fixed();
        // Direction ratio 1:2:1, total length 20.0.
        assert!((len3(o) - 20.0).abs() < 0.01, "length ~20, got {}", len3(o));
        assert!(o[0] > 0 && o[1] > 0 && o[2] > 0);
        // Y (up) is about twice X and Z.
        assert!((to_f(o[1]) - 2.0 * to_f(o[0])).abs() < 0.05);
        assert!((o[0] - o[2]).abs() <= 2, "x and z equal");
    }

    #[test]
    fn random_offset_stays_within_the_recovered_envelope() {
        let mut rng = GameLcg::new(0xBEEF);
        for _ in 0..2000 {
            let o = cinematic_offset_random(&mut rng);
            let len = len3(o);
            // The offset is renormalized to `dist` in [4.5, 10.0).
            assert!(
                (4.0..10.5).contains(&len),
                "offset length {len} out of envelope"
            );
            // The camera always sits above the car (y > 0).
            assert!(
                o[1] > 0,
                "random cam should be above ground, got y={}",
                o[1]
            );
        }
    }

    #[test]
    fn random_offset_is_deterministic_for_a_seed() {
        let a = cinematic_offset_random(&mut GameLcg::new(42));
        let b = cinematic_offset_random(&mut GameLcg::new(42));
        assert_eq!(a, b);
    }

    #[test]
    fn camera_looks_back_toward_the_car_with_an_orthonormal_basis() {
        let car = [0, 0, 0];
        let offset = cinematic_offset_fixed();
        let m = cinematic_camera(car, offset, [0, ONE, 0]);
        // Translation = car + offset.
        assert_eq!([m[12], m[13], m[14]], offset);
        // Forward points from the eye back toward the car (opposite the offset).
        let fwd = [to_f(m[8]), to_f(m[9]), to_f(m[10])];
        assert!(
            fwd[0] < 0.0 && fwd[1] < 0.0 && fwd[2] < 0.0,
            "forward looks toward car"
        );
        // Basis rows are unit length and mutually perpendicular.
        let right = [to_f(m[0]), to_f(m[1]), to_f(m[2])];
        let up = [to_f(m[4]), to_f(m[5]), to_f(m[6])];
        for row in [right, up, fwd] {
            let l = (row[0] * row[0] + row[1] * row[1] + row[2] * row[2]).sqrt();
            assert!((l - 1.0).abs() < 0.01, "row not unit length: {l}");
        }
        let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        assert!(dot(right, fwd).abs() < 0.01);
        assert!(dot(right, up).abs() < 0.01);
        assert!(dot(up, fwd).abs() < 0.01);
    }
}
