//! Modern pads through XInput (E8): separate trigger pedals and rumble.
//!
//! Through WinMM a pad's two triggers are one combined axis, so pressing both
//! cancels out, and the game reads one vertical axis for throttle (up) and
//! brake (down). Its control map still keeps the two pedals apart (`+0x304`
//! throttle, `+0x308` brake), so the triggers read through XInput go straight
//! into them (F21). Rumble follows the game's impacts (F22) and fades out.

/// Below this a trigger reads as released (XInput's own threshold).
pub const TRIGGER_THRESHOLD: u8 = 30;

/// The game's full pedal (`0xffbe`) and its analog quantization (low 10 bits
/// cleared, so an analog pedal tops out at `0xfc00`).
const FULL: u32 = 0xffbe;
const QUANT: u32 = 0xffff_fc00;

/// A trigger (0..255) as the game's analog pedal magnitude, 0 below the
/// threshold; rescaled so the threshold is 0 and a full press the maximum.
pub fn pedal(trigger: u8) -> u32 {
    if trigger <= TRIGGER_THRESHOLD {
        return 0;
    }
    let span = 255 - u32::from(TRIGGER_THRESHOLD);
    let v = (u32::from(trigger - TRIGGER_THRESHOLD) * FULL / span).min(FULL);
    v & QUANT
}

/// The rumble level, 0.0..=1.0: set by an impact, fading with a 150 ms
/// half-life.
#[derive(Debug, Default)]
pub struct Rumble {
    level: f32,
    last: i64,
}

const HALF_LIFE: f32 = 0.15;

impl Rumble {
    pub const fn new() -> Rumble {
        Rumble {
            level: 0.0,
            last: 0,
        }
    }

    /// An impact of `severity` (Q16, 0..=1.0) at `strength` percent; a
    /// weaker impact never cuts a stronger rumble short.
    pub fn impact(&mut self, severity: i32, strength: u32, now: i64) {
        let s = (severity.clamp(0, 0x10000) as f32 / 65536.0) * (strength.min(100) as f32 / 100.0);
        if s > self.level {
            self.level = s;
            self.last = now;
        }
    }

    /// The level now, faded since the impact.
    pub fn level(&mut self, now: i64, freq: i64) -> f32 {
        if self.level <= 0.0 || freq <= 0 {
            return 0.0;
        }
        let dt = (now - self.last) as f32 / freq as f32;
        // 2^-x by halving per whole half-life, linear within one.
        let halves = dt / HALF_LIFE;
        let mut level = self.level;
        let mut n = halves;
        while n >= 1.0 && level > 0.0 {
            level *= 0.5;
            n -= 1.0;
        }
        level *= 1.0 - n * 0.5;
        if level < 0.02 {
            self.level = 0.0;
            return 0.0;
        }
        level
    }

    pub fn stop(&mut self) {
        self.level = 0.0;
    }
}

/// Below this a pad's motors barely turn: any rumble starts here.
const FELT: f32 = 0.3;

/// The two motors' speeds for a level: the heavy (low-frequency) motor
/// carries the impact, the light one adds a little bite. Most impacts are
/// mild (a fifth to a half of the strongest), and pad motors don't spin
/// below about a third, so the speed starts at a felt minimum and grows
/// with the square root of the level.
pub fn motors(level: f32) -> (u16, u16) {
    let l = level.clamp(0.0, 1.0);
    if l <= 0.0 {
        return (0, 0);
    }
    let heavy = FELT + (1.0 - FELT) * sqrt(l);
    ((heavy * 65535.0) as u16, (heavy * 0.6 * 65535.0) as u16)
}

/// Square root without `std` (a few Newton steps; 0..=1 here).
fn sqrt(x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    let mut r = if x < 0.25 { 0.25 } else { x };
    for _ in 0..6 {
        r = 0.5 * (r + x / r);
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triggers_become_analog_pedals() {
        assert_eq!(pedal(0), 0);
        assert_eq!(pedal(TRIGGER_THRESHOLD), 0, "resting pads don't creep");
        assert_eq!(pedal(255), 0xfc00, "full press: the game's analog maximum");
        let half = pedal(142);
        assert!((0x7800..=0x8400).contains(&half), "{half:#x}");
        assert_eq!(half & 0x3ff, 0, "quantized like the game's own analog read");
    }

    #[test]
    fn rumble_fades_and_strength_scales() {
        const F: i64 = 1000;
        let mut r = Rumble::new();
        r.impact(0x10000, 50, 0);
        assert!((r.level(0, F) - 0.5).abs() < 1e-6);
        assert!((r.level(150, F) - 0.25).abs() < 1e-3, "one half-life later");
        assert_eq!(r.level(2000, F), 0.0, "gone after two seconds");
        // A weak knock doesn't cut a strong one short.
        r.impact(0x10000, 100, 3000);
        r.impact(0x1000, 100, 3010);
        assert!(r.level(3010, F) > 0.9);
        r.stop();
        assert_eq!(r.level(3020, F), 0.0);
    }

    #[test]
    fn motor_speeds() {
        assert_eq!(motors(0.0), (0, 0));
        assert_eq!(motors(1.0), (65535, 39321));
        assert_eq!(motors(2.0), (65535, 39321));
        // A mild impact (21 %) at Medium (60 %) is felt: about 55 %, not 13 %.
        let (heavy, light) = motors(0.21 * 0.6);
        assert!((0.52..=0.58).contains(&(heavy as f32 / 65535.0)), "{heavy}");
        assert!(light < heavy);
        // The faintest rumble still turns the motor.
        assert!(motors(0.001).0 as f32 / 65535.0 >= FELT);
    }

    #[test]
    fn square_root() {
        for x in [0.01f32, 0.1, 0.25, 0.5, 0.9, 1.0] {
            assert!((sqrt(x) - x.sqrt()).abs() < 1e-4, "{x}");
        }
    }
}
