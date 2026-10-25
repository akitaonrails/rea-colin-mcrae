//! The camera **mode-change crossfade** (`rea.official-eu.camera-modes`): the
//! cosine-LUT ease weight and the pose lerp that blend the previous camera (A)
//! into the new one (B) while a transition runs.
//!
//! The driver `FUN_004343a0` counts a per-player `counter` (`DAT_00550558`) down
//! from the `duration` (`DAT_00550640`) set by the change gate
//! ([`super::camera_mode::resolve_transition`]). Each frame it derives a blend
//! weight from the engine BAM cosine table and lerps the A and B camera records
//! together via `FUN_00436930`, holding [`super::camera_mode::CameraMode::Blending`]
//! (`0xc`) in the mode field until the counter drains, then swaps the active A/B
//! slot.
//!
//! The weight curve is `w = 1 - ((cos(theta) + 1) / 2)^2`, with `theta` indexed
//! into the 4096-entry Q16.16 cosine table `DAT_0082d380`. That table is built at
//! runtime (it reads as all zero in the static image), so it is injected here as
//! a parameter; the index math and the post-curve are fully recovered.

/// The engine BAM trig table length (`DAT_0082d380`, 4096 Q16.16 entries).
pub const BAM_TABLE_LEN: usize = 4096;

/// `FUN_004343a0`'s cosine-LUT index for a crossfade at `counter`/`duration`.
///
/// The elapsed fraction `p` (Q16.16) is `0x10000 - counter*0x10000/duration`;
/// the recovered index is `(0x400 - ftol(p * 180 * -1/5760)) & 0xfff`
/// (`= (0x400 + p/32) & 0xfff`), i.e. `0x400` at the start of the blend sweeping
/// up by `0x800` across it. A non-positive `duration` yields the start index.
#[must_use]
pub fn blend_lut_index(counter: i32, duration: i32) -> usize {
    if duration <= 0 {
        return 0x400;
    }
    let counter = counter.clamp(0, duration);
    let frac_remaining = (i64::from(counter) * 0x1_0000) / i64::from(duration);
    let p = 0x1_0000 - frac_remaining; // elapsed fraction, Q16.16
    // ftol(p * 180 * (-1/5760)) == -(p*180/5760) truncated toward zero.
    let k = -(p * 180 / 5760);
    ((0x400 - k) & 0xfff) as usize
}

/// The crossfade blend weight (Q16.16, `0..=0x10000`) for a frame, given the
/// runtime cosine table: `cos = lut[blend_lut_index(..)]`, `half = (cos+1)/2`,
/// `weight = 1 - half^2`.
#[must_use]
pub fn blend_weight_q16(counter: i32, duration: i32, cos_lut: &[i32; BAM_TABLE_LEN]) -> i32 {
    let cos = i64::from(cos_lut[blend_lut_index(counter, duration)]);
    let half = ((cos + 0x1_0000) * 0x8000) >> 16; // (cos + 1) / 2, Q16.16
    (0x1_0000 - ((half * half) >> 16)) as i32
}

/// A Q16.16 linear interpolation `a + (b - a) * frac` (the per-scalar step of
/// `FUN_00436930`, whose `frac` is the [`blend_weight_q16`]).
#[must_use]
pub fn lerp_q16(a: i32, b: i32, frac: i32) -> i32 {
    let delta = i64::from(b) - i64::from(a);
    (i64::from(a) + ((delta * i64::from(frac)) >> 16)) as i32
}

/// Q16.16 one.
const ONE: i32 = 0x1_0000;

/// A Q16.16 cross product `a × b` (`>>16` per multiply).
fn cross_q16(a: [i32; 3], b: [i32; 3]) -> [i32; 3] {
    let m = |x: i32, y: i32| ((i64::from(x) * i64::from(y)) >> 16) as i32;
    [
        m(a[1], b[2]).wrapping_sub(m(a[2], b[1])),
        m(a[2], b[0]).wrapping_sub(m(a[0], b[2])),
        m(a[0], b[1]).wrapping_sub(m(a[1], b[0])),
    ]
}

/// Normalize a Q16.16 vector to unit length. `FUN_00435400` uses the engine
/// reciprocal-magnitude table `DAT_0082b380`, whose startup generation is now
/// recovered in [`crate::legacy_math`]. This still uses an `f64` square root as
/// a documented approximation because the complete camera normalization path
/// has not yet been migrated to the exact lookup contract (the same stance as
/// [`super::camera_cinematic`]). A zero vector normalizes to zero.
fn normalize_q16(v: [i32; 3]) -> [i32; 3] {
    let f = |x: i32| f64::from(x) / 65536.0;
    let (x, y, z) = (f(v[0]), f(v[1]), f(v[2]));
    let len = (x * x + y * y + z * z).sqrt();
    if len == 0.0 {
        return [0, 0, 0];
    }
    let q = |c: f64| (c / len * 65536.0).round() as i32;
    [q(x), q(y), q(z)]
}

/// The recovered world-matrix crossfade blend (`FUN_00435400`, the up-anchored
/// `param_7 == 0` path that `FUN_00436930` calls): blend the two camera world
/// matrices `a` (from) and `b` (to) at weight `frac` (Q16.16) and return the
/// displayed matrix.
///
/// Each is `[i32;16]` row-major Q16.16 (right `0..3`, up `4..7`, forward `8..11`,
/// translation `12..15`, element `15` = [`ONE`]). The blend is **hybrid, not a
/// naive element-wise matrix lerp**: the translation is a plain Q16.16 lerp,
/// while the orientation is lerped then **re-orthonormalized about the up axis**
/// (Gram-Schmidt): `up = normalize(lerp(a.up, b.up))`, `forward =
/// normalize(lerp(a.right, b.right) × up)`, `right = up × forward`. The result
/// is always a clean orthonormal frame, so blending two valid camera bases never
/// produces a skewed matrix. (The five camera-record scalars are blended
/// separately by [`CameraScalars::blend`].)
#[must_use]
pub fn blend_pose(a: &[i32; 16], b: &[i32; 16], frac: i32) -> [i32; 16] {
    let row = |m: &[i32; 16], i: usize| [m[i], m[i + 1], m[i + 2]];
    let lerp3 = |x: [i32; 3], y: [i32; 3]| {
        [
            lerp_q16(x[0], y[0], frac),
            lerp_q16(x[1], y[1], frac),
            lerp_q16(x[2], y[2], frac),
        ]
    };
    // Up-anchored Gram-Schmidt: the interpolated right only seeds the forward.
    let right_lerp = lerp3(row(a, 0), row(b, 0));
    let up = normalize_q16(lerp3(row(a, 4), row(b, 4)));
    let forward = normalize_q16(cross_q16(right_lerp, up));
    let right = cross_q16(up, forward);
    let trans = lerp3(row(a, 12), row(b, 12));

    let mut out = [0i32; 16];
    out[0..3].copy_from_slice(&right);
    out[4..7].copy_from_slice(&up);
    out[8..11].copy_from_slice(&forward);
    out[12..15].copy_from_slice(&trans);
    out[15] = ONE;
    out
}

/// The five scalar camera parameters the crossfade lerps (`FUN_00436930`), the
/// camera record fields `+0x48`/`+0x4c`/`+0x50`/`+0x54`/`+0x58` (Q16.16).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CameraScalars {
    /// `+0x48`.
    pub p48: i32,
    /// `+0x4c` (`0x10000` = 1.0 at rest).
    pub p4c: i32,
    /// `+0x50`: pitch / look-ahead.
    pub p50: i32,
    /// `+0x54` (`0x00a0_0000` = 160.0 at rest - lens/FOV-like).
    pub p54: i32,
    /// `+0x58`: longitudinal look target.
    pub p58: i32,
}

impl CameraScalars {
    /// Lerp every field from `self` (A) toward `b` (B) by `frac` (Q16.16).
    #[must_use]
    pub fn blend(self, b: CameraScalars, frac: i32) -> CameraScalars {
        CameraScalars {
            p48: lerp_q16(self.p48, b.p48, frac),
            p4c: lerp_q16(self.p4c, b.p4c, frac),
            p50: lerp_q16(self.p50, b.p50, frac),
            p54: lerp_q16(self.p54, b.p54, frac),
            p58: lerp_q16(self.p58, b.p58, frac),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::legacy_math::cos_q16;

    /// The exact source-owned reconstruction of the startup cosine table.
    fn cos_table() -> [i32; BAM_TABLE_LEN] {
        std::array::from_fn(|phase| cos_q16(phase as u16).raw())
    }

    #[test]
    fn lut_index_sweeps_0x400_to_0xc00() {
        // Start of the blend (counter == duration): elapsed 0 -> index 0x400.
        assert_eq!(blend_lut_index(25, 25), 0x400);
        // End of the blend (counter == 0): elapsed 1.0 -> index 0x400 + 0x800.
        assert_eq!(blend_lut_index(0, 25), 0xc00);
        // Halfway.
        assert_eq!(
            blend_lut_index(12, 25),
            0x400 + ((0x1_0000 - 12 * 0x1_0000 / 25) * 180 / 5760) as usize
        );
        // Degenerate duration -> start index, no panic.
        assert_eq!(blend_lut_index(0, 0), 0x400);
    }

    #[test]
    fn blend_weight_follows_one_minus_half_squared() {
        let lut = cos_table();
        // Start: cos at index 0x400 (quarter table) = cos(pi/2) = 0, half = 0.5,
        // weight = 1 - 0.25 = 0.75 (0xc000).
        let w_start = blend_weight_q16(25, 25, &lut);
        assert!(
            (w_start - 0xc000).abs() <= 2,
            "start weight ~0.75, got {w_start:#x}"
        );
        // End: index 0xc00 (three-quarter table) = cos(3pi/2) = 0, same 0.75.
        let w_end = blend_weight_q16(0, 25, &lut);
        assert!(
            (w_end - 0xc000).abs() <= 2,
            "end weight ~0.75, got {w_end:#x}"
        );
        // Weight stays within [0, 1] across the whole sweep.
        for counter in 0..=25 {
            let w = blend_weight_q16(counter, 25, &lut);
            assert!((0..=0x1_0000).contains(&w), "weight {w:#x} out of range");
        }
    }

    #[test]
    fn lerp_endpoints_and_midpoint() {
        assert_eq!(lerp_q16(0, 0x10_0000, 0), 0);
        assert_eq!(lerp_q16(0, 0x10_0000, 0x1_0000), 0x10_0000);
        assert_eq!(lerp_q16(0, 0x10_0000, 0x8000), 0x8_0000);
        // Descending lerp.
        assert_eq!(lerp_q16(0x10_0000, 0, 0x8000), 0x8_0000);
    }

    /// Q16.16 dot of two 3-vectors.
    fn dot(a: [i32; 3], b: [i32; 3]) -> i64 {
        (0..3)
            .map(|i| (i64::from(a[i]) * i64::from(b[i])) >> 16)
            .sum()
    }

    /// The standard right-handed basis (right +X, up +Y, forward +Z) plus a
    /// translation, as the game's camera world matrix would hold it.
    fn basis_with_translation(t: [i32; 3]) -> [i32; 16] {
        [
            0x1_0000, 0, 0, 0, // right +X
            0, 0x1_0000, 0, 0, // up +Y
            0, 0, 0x1_0000, 0, // forward +Z
            t[0], t[1], t[2], 0x1_0000,
        ]
    }

    #[test]
    fn blending_identical_poses_returns_that_pose() {
        // The standard basis satisfies forward = right x up and right = up x
        // forward, so the Gram-Schmidt rebuild is a fixed point: a == b yields a.
        let a = basis_with_translation([0x5_0000, 0x2_0000, -0x3_0000]);
        for &frac in &[0, 0x4000, 0x8000, 0xc000, 0x1_0000] {
            let out = blend_pose(&a, &a, frac);
            assert_eq!(
                out, a,
                "identical blend at frac {frac:#x} must be a fixed point"
            );
        }
    }

    #[test]
    fn blended_translation_is_a_plain_lerp() {
        let a = basis_with_translation([0, 0, 0]);
        let b = basis_with_translation([0x8_0000, 0x4_0000, 0]);
        let out = blend_pose(&a, &b, 0x8000); // halfway
        assert_eq!([out[12], out[13], out[14]], [0x4_0000, 0x2_0000, 0]);
    }

    #[test]
    fn blended_orientation_is_orthonormal() {
        // Blend the identity basis with one yawed 90 degrees (right -> +Z,
        // forward -> -X). At any weight the rebuilt frame must stay orthonormal.
        let a = basis_with_translation([0, 0, 0]);
        let b = [
            0, 0, 0x1_0000, 0, // right +Z
            0, 0x1_0000, 0, 0, // up +Y
            -0x1_0000, 0, 0, 0, // forward -X
            0, 0, 0, 0x1_0000,
        ];
        for &frac in &[0x4000, 0x8000, 0xc000] {
            let m = blend_pose(&a, &b, frac);
            let right = [m[0], m[1], m[2]];
            let up = [m[4], m[5], m[6]];
            let forward = [m[8], m[9], m[10]];
            // Unit length (Q16.16 one = 0x1_0000), within a small rounding slop.
            for v in [right, up, forward] {
                let len2 = dot(v, v);
                assert!(
                    (len2 - 0x1_0000).abs() <= 4,
                    "axis not unit: len^2 {len2:#x}"
                );
            }
            // Mutually orthogonal.
            assert!(dot(right, up).abs() <= 4, "right.up {:#x}", dot(right, up));
            assert!(
                dot(right, forward).abs() <= 4,
                "right.fwd {:#x}",
                dot(right, forward)
            );
            assert!(
                dot(up, forward).abs() <= 4,
                "up.fwd {:#x}",
                dot(up, forward)
            );
            assert_eq!(m[15], 0x1_0000);
        }
    }

    #[test]
    fn scalars_blend_componentwise() {
        let a = CameraScalars {
            p48: 0,
            p4c: 0x1_0000,
            p50: 0,
            p54: 0,
            p58: 0,
        };
        let b = CameraScalars {
            p48: 0x4_0000,
            p4c: 0x3_0000,
            p50: -0x2_0000,
            p54: 0xa0_0000,
            p58: 0x1_0000,
        };
        // At frac 0 the result is A; at full frac it is B.
        assert_eq!(a.blend(b, 0), a);
        assert_eq!(a.blend(b, 0x1_0000), b);
        // Halfway each field is the midpoint.
        let mid = a.blend(b, 0x8000);
        assert_eq!(mid.p48, 0x2_0000);
        assert_eq!(mid.p50, -0x1_0000);
        assert_eq!(mid.p54, 0x50_0000);
    }
}
