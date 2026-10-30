//! Eased view distance (E10, F20).
//!
//! Stage sections switch the far plane at once (say from 75 to 150 m), so a
//! band of scenery pops in or out in one frame. The eased value follows the
//! game's far plane, closing half the gap every third of a second of real
//! time; it snaps on the first frame and after a stall (a load, a pause),
//! so a new stage starts at its own distance.

/// Half of the gap closes in this many seconds.
const HALF_LIFE: f32 = 0.33;
/// A longer gap between frames than this snaps instead of easing.
const STALL: f32 = 0.25;

#[derive(Debug, Default)]
pub struct Easer {
    /// The eased far plane (Q16 metres) and when it was last shown
    /// (performance-counter ticks); `last` 0 before the first frame.
    value: i64,
    last: i64,
}

impl Easer {
    pub const fn new() -> Easer {
        Easer { value: 0, last: 0 }
    }

    /// The far plane to use now for the game's `target` (Q16), at `now` on
    /// a counter of `freq` ticks per second.
    pub fn step(&mut self, target: i32, now: i64, freq: i64) -> i32 {
        let dt = if self.last == 0 || freq <= 0 {
            f32::INFINITY
        } else {
            (now - self.last) as f32 / freq as f32
        };
        self.last = now;
        if !(0.0..=STALL).contains(&dt) {
            self.value = target.into();
            return target;
        }
        let gap = target as f32 - self.value as f32;
        self.value += (gap * closed(dt / HALF_LIFE)) as i64;
        // Within a centimetre: there.
        if (i64::from(target) - self.value).abs() < 655 {
            self.value = target.into();
        }
        self.value as i32
    }
}

/// The share of the gap closed in `x` half-lives: 1 - 2^-x, for x up to
/// about 1 (a 4th-order series of e^-y, y = x ln 2, no `std` needed).
fn closed(x: f32) -> f32 {
    let y = x * core::f32::consts::LN_2;
    let e = 1.0 - y + y * y / 2.0 - y * y * y / 6.0 + y * y * y * y / 24.0;
    (1.0 - e).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FREQ: i64 = 10_000_000;
    const M: i32 = 0x10000;

    fn frames(e: &mut Easer, target: i32, start: i64, n: i64, step: i64) -> i32 {
        let mut v = 0;
        for i in 1..=n {
            v = e.step(target, start + i * step, FREQ);
        }
        v
    }

    #[test]
    fn the_first_frame_and_a_stall_snap() {
        let mut e = Easer::new();
        assert_eq!(e.step(150 * M, 1, FREQ), 150 * M);
        // A load: half a second without frames.
        assert_eq!(e.step(75 * M, 1 + FREQ / 2, FREQ), 75 * M);
    }

    #[test]
    fn a_section_change_eases_with_a_third_of_a_second_half_life() {
        let mut e = Easer::new();
        e.step(75 * M, 1, FREQ);
        // 0.33 s of 60 fps frames: about half way from 75 to 150 m.
        let half = frames(&mut e, 150 * M, 1, 20, FREQ / 60);
        let metres = half as f32 / M as f32;
        assert!((110.0..=115.0).contains(&metres), "{metres}");
        // Two seconds later: there.
        assert_eq!(frames(&mut e, 150 * M, FREQ, 120, FREQ / 60), 150 * M);
    }

    #[test]
    fn the_frame_rate_does_not_change_the_pace() {
        let (mut a, mut b) = (Easer::new(), Easer::new());
        a.step(75 * M, 1, FREQ);
        b.step(75 * M, 1, FREQ);
        let at30 = frames(&mut a, 150 * M, 1, 15, FREQ / 30) as f32 / M as f32;
        let at240 = frames(&mut b, 150 * M, 1, 120, FREQ / 240) as f32 / M as f32;
        assert!((at30 - at240).abs() < 1.5, "{at30} vs {at240}");
    }

    #[test]
    fn closing_share() {
        assert!((closed(1.0) - 0.5).abs() < 0.002);
        assert_eq!(closed(0.0), 0.0);
    }
}
