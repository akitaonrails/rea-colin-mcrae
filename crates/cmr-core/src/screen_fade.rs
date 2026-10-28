//! The screen fade channel `DAT_0082b37c`: one linear tween record of the
//! tween engine (`0x5046d8`), started by `FUN_0048df80(n)` (in, 0 to
//! `0x10000`) and `FUN_0048dfa0(n)` (out), read by `FUN_0048dfc0` (which
//! advances it) and tested by `FUN_0048dfd0` (which does not). The service
//! screen and the race share it, so a fade left by one is what the next
//! sees.
//!
//! In the race its only pixels are the compositor's black quad
//! (`FUN_00423170`): alpha `0xff - ((level * 255) >> 16)` over the frame
//! while the fade is in progress. State `0xb` holds until it is idle.
//!
//! A description of disassembly, not original source. Evidence:
//! `rea.official-eu.race-fade`.

use crate::scalar_tween::ScalarTween;

/// Q16 full level.
pub const FULL: i32 = 0x1_0000;

/// The channel's record: its tween (`None` until the first trigger, when
/// the handle owns no record) and the active flag (`+0x14`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScreenFade {
    tween: Option<ScalarTween>,
    active: bool,
}

impl ScreenFade {
    /// `FUN_004ad720(&h, 0, n, 0, start, end, 1)`: a new linear tween that
    /// replaces any running one. `instant` is `DAT_00bbabec` (the front end's
    /// `!(record[0x10] & 0x20)`): the value is the end at once and the tween
    /// is in progress for exactly `n` evaluations.
    pub fn start(&mut self, n: u32, start: i32, end: i32, instant: bool) {
        let start = if instant { end } else { start };
        *self = Self {
            tween: Some(ScalarTween::linear(start, end, n as i32, 0)),
            active: !instant,
        };
    }

    /// `FUN_0048df80(n)`: fade in, black to full.
    pub fn fade_in(&mut self, n: u32, instant: bool) {
        self.start(n, 0, FULL, instant);
    }

    /// `FUN_0048dfa0(n)`: fade out, full to black.
    pub fn fade_out(&mut self, n: u32, instant: bool) {
        self.start(n, FULL, 0, instant);
    }

    /// `FUN_004ad5b0` via `FUN_0048dfc0`: the level, advancing the counter.
    /// A channel never started reads its zeroed record's end, 0.
    pub fn evaluate(&mut self) -> i32 {
        self.tween
            .as_mut()
            .map_or(0, ScalarTween::evaluate_and_advance)
    }

    /// `FUN_0048dfd0`: no fade in progress (`!FUN_004ad560`).
    pub fn idle(&self) -> bool {
        self.tween
            .as_ref()
            .is_none_or(|tween| !tween.in_progress(self.active))
    }
}

/// `FUN_00423170`'s black-quad alpha for a level.
pub const fn overlay_alpha(level: i32) -> u8 {
    0xff - ((level.wrapping_mul(255) >> 16) as u8)
}

/// The race fades' count: `(((DAT_00551af0 != 0) + 1) * 25 << 16) / 2 >> 16`
/// = 12, or 25 with the flag.
pub const fn race_fade_count(flag_551af0: bool) -> u32 {
    let k = if flag_551af0 { 2 } else { 1 };
    ((((k * 25) << 16) / 2) >> 16) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alphas(fade: &mut ScreenFade) -> Vec<u8> {
        let mut out = Vec::new();
        while !fade.idle() {
            out.push(overlay_alpha(fade.evaluate()));
        }
        out
    }

    #[test]
    fn the_counts_are_12_and_25() {
        assert_eq!((race_fade_count(false), race_fade_count(true)), (12, 25));
    }

    #[test]
    fn a_fade_out_ramps_then_holds_black_for_the_tail() {
        let mut fade = ScreenFade::default();
        fade.fade_out(12, false);
        assert_eq!(
            alphas(&mut fade),
            [
                0, 22, 43, 64, 85, 107, 128, 149, 170, 192, 213, 255, 255, 255, 255
            ]
        );
        assert_eq!(fade.evaluate(), 0, "a finished fade keeps its end");
    }

    #[test]
    fn a_fade_in_starts_black() {
        let mut fade = ScreenFade::default();
        fade.fade_in(12, false);
        assert_eq!(
            alphas(&mut fade),
            [
                255, 234, 213, 192, 171, 149, 128, 107, 86, 64, 43, 0, 0, 0, 0
            ]
        );
        let mut one = ScreenFade::default();
        one.fade_out(1, false);
        assert_eq!(alphas(&mut one), [255; 4], "the load's one-frame fade out");
    }

    #[test]
    fn instant_mode_is_the_end_for_n_evaluations() {
        let mut fade = ScreenFade::default();
        fade.fade_out(12, true);
        assert_eq!(alphas(&mut fade), [255; 12]);
        fade.fade_in(3, true);
        assert_eq!(alphas(&mut fade), [0; 3]);
    }

    #[test]
    fn idle_never_advances_and_a_restart_replaces() {
        let mut fade = ScreenFade::default();
        assert!(fade.idle(), "never started");
        fade.fade_out(12, false);
        for _ in 0..5 {
            assert!(!fade.idle());
        }
        fade.evaluate();
        fade.fade_in(12, false);
        assert_eq!(
            overlay_alpha(fade.evaluate()),
            255,
            "the new fade from its start"
        );
    }
}
