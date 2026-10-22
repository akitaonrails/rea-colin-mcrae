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

    /// A faithful BAM cosine table for exercising the weight arithmetic:
    /// `lut[a] = cos(2*pi*a/4096)` in Q16.16, matching the engine's runtime build
    /// (the real table is zero in the static image, so a generated one is used).
    fn cos_table() -> [i32; BAM_TABLE_LEN] {
        let mut lut = [0i32; BAM_TABLE_LEN];
        for (a, slot) in lut.iter_mut().enumerate() {
            let theta = std::f64::consts::TAU * (a as f64) / (BAM_TABLE_LEN as f64);
            *slot = (theta.cos() * 65536.0).round() as i32;
        }
        lut
    }

    #[test]
    fn lut_index_sweeps_0x400_to_0xc00() {
        // Start of the blend (counter == duration): elapsed 0 -> index 0x400.
        assert_eq!(blend_lut_index(25, 25), 0x400);
        // End of the blend (counter == 0): elapsed 1.0 -> index 0x400 + 0x800.
        assert_eq!(blend_lut_index(0, 25), 0xc00);
        // Halfway.
        assert_eq!(blend_lut_index(12, 25), 0x400 + ((0x1_0000 - 12 * 0x1_0000 / 25) * 180 / 5760) as usize);
        // Degenerate duration -> start index, no panic.
        assert_eq!(blend_lut_index(0, 0), 0x400);
    }

    #[test]
    fn blend_weight_follows_one_minus_half_squared() {
        let lut = cos_table();
        // Start: cos at index 0x400 (quarter table) = cos(pi/2) = 0, half = 0.5,
        // weight = 1 - 0.25 = 0.75 (0xc000).
        let w_start = blend_weight_q16(25, 25, &lut);
        assert!((w_start - 0xc000).abs() <= 2, "start weight ~0.75, got {w_start:#x}");
        // End: index 0xc00 (three-quarter table) = cos(3pi/2) = 0, same 0.75.
        let w_end = blend_weight_q16(0, 25, &lut);
        assert!((w_end - 0xc000).abs() <= 2, "end weight ~0.75, got {w_end:#x}");
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

    #[test]
    fn scalars_blend_componentwise() {
        let a = CameraScalars { p48: 0, p4c: 0x1_0000, p50: 0, p54: 0, p58: 0 };
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
