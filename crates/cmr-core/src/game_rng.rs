//! The game's pseudo-random generator (`FUN_004cfac0`, state `DAT_0050b690`): a
//! standard 32-bit linear congruential generator used for camera framing, the
//! trackside director's exits, and other cinematic jitter.
//!
//! `state = state * 0x343fd + 0x269ec3`; each draw returns the middle 15 bits
//! `(state >> 16) & 0x7fff` (the classic MSVC `rand()` sequence, range
//! `0..=0x7fff`). Callers that want a Q16.16 fraction scale a raw draw by the
//! recovered float `0x38000100` (~`1/0x7fff`) times `65536.0` ([`GameLcg::next_q16`]).

/// The LCG multiplier (`0x343fd`).
const LCG_MULT: u32 = 0x3_43fd;
/// The LCG increment (`0x269ec3`).
const LCG_INC: u32 = 0x26_9ec3;

/// The bit pattern of the `f32` scale (`DAT_004e6310`) that maps a raw draw
/// (`0..=0x7fff`) onto `[0, 1)` (`~1/0x7fff`).
pub const RAND_UNIT_SCALE_BITS: u32 = 0x3800_0100;

/// The game's LCG (`FUN_004cfac0`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GameLcg {
    state: u32,
}

impl GameLcg {
    /// A generator seeded with `seed` (the raw 32-bit state `DAT_0050b690`).
    #[must_use]
    pub const fn new(seed: u32) -> Self {
        Self { state: seed }
    }

    /// Advance the state and return the next raw draw in `0..=0x7fff`.
    pub fn next_raw(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(LCG_MULT).wrapping_add(LCG_INC);
        (self.state >> 16) & 0x7fff
    }

    /// The next draw as a Q16.16 fraction in `[0, 0x10000)`: `ftol(scale * raw *
    /// 65536.0)` with `scale = f32::from_bits(0x38000100)`, matching the engine's
    /// float scaling exactly.
    pub fn next_q16(&mut self) -> i32 {
        let raw = self.next_raw();
        let scale = f64::from(f32::from_bits(RAND_UNIT_SCALE_BITS));
        (scale * f64::from(raw) * 65536.0) as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_draws_match_the_msvc_sequence() {
        // The canonical rand() sequence from seed 1: 41, 18467, 6334, ...
        let mut rng = GameLcg::new(1);
        assert_eq!(rng.next_raw(), 41);
        assert_eq!(rng.next_raw(), 18467);
        assert_eq!(rng.next_raw(), 6334);
    }

    #[test]
    fn raw_draws_stay_in_range() {
        let mut rng = GameLcg::new(0x1234_5678);
        for _ in 0..1000 {
            assert!(rng.next_raw() <= 0x7fff);
        }
    }

    #[test]
    fn q16_fraction_spans_zero_to_one() {
        let scale = f64::from(f32::from_bits(RAND_UNIT_SCALE_BITS));
        // The scale is ~1/0x7fff.
        assert!((scale - 1.0 / 32767.0).abs() < 1e-8);
        let mut rng = GameLcg::new(99);
        for _ in 0..1000 {
            let q = rng.next_q16();
            assert!((0..=0x1_0001).contains(&q), "q16 {q:#x} out of range");
        }
    }
}
