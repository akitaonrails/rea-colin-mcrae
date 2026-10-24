//! In-race camera **placement transients** (`rea.official-eu.camera-modes`): the
//! near-chase velocity lead and the post-reset look-ahead timer that modulate the
//! steady chase placement ([`super::gameplay_camera`]).
//!
//! Both live in the chase chain `FUN_00419e50` (eval) -> `FUN_0041a3e0`
//! (placement). The velocity lead pulls the near-chase camera along the car's
//! heading by a projection of its motion; the reset transient briefly dips the
//! camera after a car reset/respawn. Only the fully-pinned arithmetic is
//! reconstructed here; the uncertain pieces are left documented (see the module
//! tail notes), not invented.

/// A Q16.16 3-vector dot product (`sum (a[i]*b[i]) >> 16`).
#[must_use]
pub fn dot3_q16(a: [i32; 3], b: [i32; 3]) -> i32 {
    let mut acc = 0i64;
    for i in 0..3 {
        acc += (i64::from(a[i]) * i64::from(b[i])) >> 16;
    }
    acc as i32
}

/// The near-chase longitudinal lead (`FUN_0041a3e0`, chase variant `0` only): the
/// Q16.16 dot of the car's forward basis row (`DAT_007003d4/d8/dc[car]`) with the
/// car motion vector the evaluator supplies (`FUN_0041a820`). The placement
/// **subtracts** this from the longitudinal offset, so a car moving forward pulls
/// the near chase a little further back along its heading.
#[must_use]
pub fn velocity_longitudinal_lead(forward_basis: [i32; 3], car_vec: [i32; 3]) -> i32 {
    dot3_q16(forward_basis, car_vec)
}

/// The base reset-transient length in frames before the PAL doubling (`0x19`,
/// `FUN_00419e50`).
const RESET_FRAMES: i32 = 0x19;

/// The damping scale applied to the reset transient's cosine term (`0x4000` =
/// 0.25 Q16.16, `FUN_0041a3e0`).
pub const RESET_DAMPING_Q16: i32 = 0x4000;

/// The post-reset look-ahead transient timer (`DAT_0050c60c`[car] / amplitude
/// `DAT_0050c618`[car], `FUN_00419e50`). Idle at `timer < 0`; a reset impulse
/// arms it for `(pal+1)*0x19` frames and latches the impulse as the amplitude,
/// otherwise it counts down (floored at the idle sentinel `-1`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResetTransient {
    timer: i32,
    amplitude: i32,
}

impl Default for ResetTransient {
    fn default() -> Self {
        Self::idle()
    }
}

impl ResetTransient {
    /// An idle transient (`timer = -1`).
    #[must_use]
    pub const fn idle() -> Self {
        Self {
            timer: -1,
            amplitude: 0,
        }
    }

    /// One frame of `FUN_00419e50`'s arm/tick: while idle, a positive `impulse`
    /// (the runtime reset magnitude `DAT_0050c620`) arms the transient for
    /// `(pal+1)*0x19` frames and latches it as the amplitude; otherwise the timer
    /// counts down, held at the `-1` idle sentinel.
    pub fn tick(&mut self, impulse: i32, pal: bool) {
        if self.timer < 0 && impulse > 0 {
            self.timer = RESET_FRAMES * (i32::from(pal) + 1);
            self.amplitude = impulse;
        } else if self.timer >= 0 {
            self.timer -= 1;
        }
    }

    /// Whether the transient is running this frame (`timer >= 0`).
    #[must_use]
    pub const fn is_active(self) -> bool {
        self.timer >= 0
    }

    /// The remaining timer (`DAT_0050c60c`), `-1` when idle.
    #[must_use]
    pub const fn timer(self) -> i32 {
        self.timer
    }

    /// The latched reset amplitude (`DAT_0050c618`).
    #[must_use]
    pub const fn amplitude(self) -> i32 {
        self.amplitude
    }
}

/// The BAM cosine table length (`DAT_0082d380`, 4096 Q16.16 entries); the table
/// is runtime-built so it is injected into [`reset_vertical_drop`].
pub const BAM_TABLE_LEN: usize = 0x1000;

/// The reset transient's damped-cosine drop subtracted from the camera's
/// vertical (up) offset (`FUN_0041a3e0`, `+0x4` offset component), for a running
/// [`ResetTransient`] with `timer` frames left and latched `amplitude`.
///
/// The phase `s = (timer << 16) / ((pal+1)*25)` is the remaining fraction in
/// Q16.16 (`1.0` just after the arm, falling to `0`); the cosine index is
/// `(s * 360 / 5760) & 0xfff` (`= (s >> 4) & 0xfff`, since `360/5760 = 1/16`);
/// the drop is `((cos[idx] * s >> 16) * 0x4000 >> 16) * amplitude >> 16` - the
/// cosine modulated by the linearly decaying `s` and the `0x4000` (0.25) damping.
/// A non-positive `timer` yields `0`.
#[must_use]
pub fn reset_vertical_drop(
    timer: i16,
    pal: bool,
    amplitude: i32,
    cos_lut: &[i32; BAM_TABLE_LEN],
) -> i32 {
    if timer <= 0 {
        return 0;
    }
    let denom = (i32::from(pal) + 1) * 25;
    let s = (i32::from(timer) << 16) / denom; // remaining fraction, Q16.16
    let idx = ((i64::from(s) * 360 / 5760) as usize) & 0xfff;
    let v = (i64::from(cos_lut[idx]) * i64::from(s)) >> 16;
    let v = (v * i64::from(RESET_DAMPING_Q16)) >> 16;
    ((v * i64::from(amplitude)) >> 16) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dot_and_velocity_lead_are_q16() {
        // 1.0 . 1.0 over one axis = 1.0.
        assert_eq!(dot3_q16([0x1_0000, 0, 0], [0x1_0000, 0, 0]), 0x1_0000);
        // 2.0 forward . 3.0 motion = 6.0.
        assert_eq!(
            velocity_longitudinal_lead([0x2_0000, 0, 0], [0x3_0000, 0, 0]),
            0x6_0000
        );
        // A still car produces no lead.
        assert_eq!(velocity_longitudinal_lead([0x1_0000, 0, 0], [0, 0, 0]), 0);
    }

    #[test]
    fn reset_transient_arms_latches_and_counts_down() {
        let mut t = ResetTransient::idle();
        assert!(!t.is_active());
        assert_eq!(t.timer(), -1);

        // A reset impulse arms it (NTSC) for 25 frames and latches the amplitude.
        t.tick(0x3_0000, false);
        assert_eq!(t.timer(), 25);
        assert_eq!(t.amplitude(), 0x3_0000);
        assert!(t.is_active());

        // While active, a further impulse does not re-arm; the timer counts down.
        t.tick(0x9_0000, false);
        assert_eq!(t.timer(), 24);
        assert_eq!(t.amplitude(), 0x3_0000);

        // Run it to the idle sentinel and hold there.
        for _ in 0..24 {
            t.tick(0, false);
        }
        assert_eq!(t.timer(), 0);
        t.tick(0, false);
        assert_eq!(t.timer(), -1);
        assert!(!t.is_active());
        t.tick(0, false);
        assert_eq!(t.timer(), -1, "held at the idle sentinel");
    }

    #[test]
    fn reset_transient_pal_doubles_the_duration() {
        let mut t = ResetTransient::idle();
        t.tick(0x1_0000, true);
        assert_eq!(t.timer(), 50);
    }

    #[test]
    fn idle_transient_ignores_a_zero_impulse() {
        let mut t = ResetTransient::idle();
        t.tick(0, false);
        // No arm, stays idle (floored at -1).
        assert_eq!(t.timer(), -1);
        assert!(!t.is_active());
    }

    /// A generated BAM cosine table (`lut[a] = cos(2*pi*a/4096)` Q16.16), standing
    /// in for the runtime-built `DAT_0082d380`.
    fn cos_table() -> [i32; BAM_TABLE_LEN] {
        let mut lut = [0i32; BAM_TABLE_LEN];
        for (a, slot) in lut.iter_mut().enumerate() {
            let theta = std::f64::consts::TAU * (a as f64) / (BAM_TABLE_LEN as f64);
            *slot = (theta.cos() * 65536.0).round() as i32;
        }
        lut
    }

    #[test]
    fn reset_vertical_drop_follows_the_pinned_chain() {
        let lut = cos_table();
        // Idle / spent transient contributes nothing.
        assert_eq!(reset_vertical_drop(0, false, 0x2_0000, &lut), 0);
        assert_eq!(reset_vertical_drop(-1, false, 0x2_0000, &lut), 0);

        // Just after the NTSC arm: timer == 25 -> s == 1.0 (0x10000) -> index
        // (0x10000 >> 4) & 0xfff == 0 -> cos[0] == 1.0, so
        // drop = ((1.0 * 1.0) * 0.25) * amplitude = 0.25 * amplitude.
        let amp = 0x4_0000; // 4.0
        let drop = reset_vertical_drop(25, false, amp, &lut);
        assert!(
            (drop - 0x1_0000).abs() <= 4,
            "expected ~1.0 (0.25*4.0), got {drop:#x}"
        );

        // The index is derived from the phase: timer 25 and the PAL-doubled timer
        // 50 both map to s == 1.0, so they agree.
        assert_eq!(
            reset_vertical_drop(25, false, amp, &lut),
            reset_vertical_drop(50, true, amp, &lut)
        );
    }
}
