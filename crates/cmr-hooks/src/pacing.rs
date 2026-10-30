//! The frame-rate cap (E7): when each presented frame may be shown.
//!
//! Frames are spaced one period apart on a steady cadence; a frame that is a
//! little late keeps the cadence, one more than a period late starts it
//! again from now (after a loading pause, say) instead of rushing to catch
//! up. Only presentation is paced: the race's logic runs on its own 25 Hz
//! timer thread and is not slowed.

#[derive(Debug, Default)]
pub struct Pacer {
    /// The time the next frame may be shown (performance-counter ticks), or
    /// 0 before the first one.
    next: i64,
}

impl Pacer {
    pub const fn new() -> Pacer {
        Pacer { next: 0 }
    }

    /// How long to wait from `now` before showing a frame, at `limit`
    /// frames per second (0: no cap) with a counter of `freq` ticks per
    /// second.
    pub fn wait(&mut self, now: i64, freq: i64, limit: u32) -> i64 {
        if limit == 0 || freq <= 0 {
            self.next = 0;
            return 0;
        }
        let period = (freq / limit as i64).max(1);
        if self.next == 0 || now - self.next > period {
            self.next = now;
        }
        let wait = (self.next - now).max(0);
        self.next += period;
        wait
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FREQ: i64 = 10_000_000; // 100 ns ticks, as on Windows

    #[test]
    fn no_cap_never_waits() {
        let mut p = Pacer::new();
        assert_eq!(p.wait(123, FREQ, 0), 0);
        assert_eq!(p.wait(124, FREQ, 0), 0);
    }

    #[test]
    fn fast_frames_wait_for_a_steady_cadence() {
        let mut p = Pacer::new();
        let period = FREQ / 60;
        assert_eq!(p.wait(0, FREQ, 60), 0, "the first frame goes at once");
        // A frame drawn in 2 ms waits for the rest of its 16.7 ms.
        assert_eq!(p.wait(20_000, FREQ, 60), period - 20_000);
        assert_eq!(p.wait(period + 20_000, FREQ, 60), period - 20_000);
    }

    #[test]
    fn a_slightly_late_frame_keeps_the_cadence() {
        let mut p = Pacer::new();
        let period = FREQ / 100;
        p.wait(0, FREQ, 100);
        assert_eq!(p.wait(period + period / 2, FREQ, 100), 0, "late: no wait");
        // The next frame is still due at 2 periods, not 2.5.
        assert_eq!(p.wait(period * 3 / 2 + 1, FREQ, 100), period / 2 - 1);
    }

    #[test]
    fn a_long_pause_restarts_instead_of_rushing() {
        let mut p = Pacer::new();
        let period = FREQ / 60;
        p.wait(0, FREQ, 60);
        let later = FREQ * 3; // three seconds of loading
        assert_eq!(p.wait(later, FREQ, 60), 0);
        assert_eq!(p.wait(later + 1, FREQ, 60), period - 1, "then paced again");
    }
}
