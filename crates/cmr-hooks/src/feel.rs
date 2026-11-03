//! Driving feel for pads with trigger motors (Xbox One / Series / Elite):
//! besides the impacts (`xinput::Rumble`), the right trigger answers
//! wheelspin, the left one hard braking and locked wheels (pulsing like
//! ABS), the heavy motor gives a knock at each gear change and the light
//! one a faint hum that grows with speed.
//!
//! The game's car block (`cmr_pedals`, player 1, about 25 updates a
//! second) holds the speeds of the wheels (eight Q16 values from `+0x2a8`,
//! units of about 16 mph), the gear (`+0x43a`) and the pedals (`+0x304`
//! throttle, `+0x308` brake, `0..=0xffbe`). Found by recording the block in
//! a race and comparing it with the speedometer.

/// One update of the car, as read from its block.
#[derive(Debug, Clone, Copy, Default)]
pub struct Car {
    pub wheels: [f32; 8],
    pub gear: i16,
    /// 0.0..=1.0.
    pub throttle: f32,
    pub brake: f32,
    /// The load on each wheel (`+0x35c`, Q16): about 36 standing on the
    /// ground, 0 off it, up to about 70 landing.
    pub loads: [f32; 4],
    /// Engine speed in thousands (`+0x2e0`, Q16); the limiter is at 8.946.
    pub rpm: f32,
}

impl Car {
    /// Reads a car block's fields.
    ///
    /// # Safety
    /// `block` must point at the game's car block (0x440 bytes or more).
    pub unsafe fn read(block: *const u8) -> Car {
        let i32_at = |o: usize| unsafe { (block.add(o) as *const i32).read_unaligned() };
        let pedal = |o: usize| (i32_at(o).clamp(0, 0xffbe) as f32 / 0xffbe as f32).min(1.0);
        Car {
            wheels: core::array::from_fn(|i| i32_at(0x2a8 + 4 * i) as f32 / 65536.0),
            gear: unsafe { (block.add(0x43a) as *const i16).read_unaligned() },
            throttle: pedal(0x304),
            brake: pedal(0x308),
            loads: core::array::from_fn(|i| i32_at(0x35c + 4 * i) as f32 / 65536.0),
            rpm: i32_at(0x2e0) as f32 / 65536.0,
        }
    }

    /// All four wheels off the ground.
    pub fn airborne(&self) -> bool {
        self.loads.iter().all(|&l| l <= 0.0)
    }

    /// Two or more wheels off the ground (small jumps lift one end).
    pub fn lifted(&self) -> bool {
        self.loads.iter().filter(|&&l| l <= 0.0).count() >= 2
    }

    pub fn speed(&self) -> f32 {
        self.wheels.iter().sum::<f32>() / 8.0
    }

    /// How far apart the wheels turn, relative to the speed.
    fn spread(&self) -> f32 {
        let (lo, hi) = self
            .wheels
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), &w| (lo.min(w), hi.max(w)));
        (hi - lo) / self.speed().max(0.5)
    }
}

/// The player's choices (`trigger_rumble`, `road_rumble`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Choices {
    pub triggers: bool,
    pub road: bool,
}

impl Choices {
    pub const ALL: Choices = Choices {
        triggers: true,
        road: true,
    };

    /// From the launcher's rumble word (F17 settings): bit 8 the trigger
    /// feel off, bit 9 the road feel off.
    pub fn of_word(word: u32) -> Choices {
        Choices {
            triggers: word & 0x100 == 0,
            road: word & 0x200 == 0,
        }
    }
}

/// What the pad is asked to do, each 0.0..=1.0.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Output {
    pub heavy: f32,
    pub light: f32,
    pub left_trigger: f32,
    pub right_trigger: f32,
}

/// Wheel acceleration (speed units a second) above which the wheels are
/// spinning up faster than the car can: wheelspin. With the wheels' spread
/// over `SPIN_SPREAD` this flags about 5 % of the time on the throttle in
/// a recorded stage (looser limits flagged 13 %, felt as always on;
/// 5 % was too seldom).
const SPIN_ACCEL: f32 = 1.2;
const SPIN_SPREAD: f32 = 0.18;
/// Wheel deceleration under braking above which they are locking.
const LOCK_DECEL: f32 = 2.5;
/// Below this speed the car counts as standing.
const MOVING: f32 = 0.3;
/// The gear-change knock: strength and length (seconds).
const KICK: f32 = 0.45;
const KICK_TIME: f32 = 0.09;
/// The ABS pulse rate (Hz).
const ABS_RATE: f32 = 14.0;
/// A landing counts after this long off the ground (seconds).
const FLIGHT: f32 = 0.08;
/// A wheel's load rising this much in one update is a bump (normal
/// driving: half the updates under 1.6, nine in ten under 6.3; at 18 a
/// recorded stage had about 7 bumps a minute, at 12 a tiring 22).
const BUMP_RISE: f32 = 18.0;
/// Thumps (landings, bumps) fade out over this long (seconds).
const THUMP_FADE: f32 = 0.25;
/// The rev limiter's buzz starts here (thousands of rpm).
const REDLINE: f32 = 8.6;
/// The road's feel: from this smoothed load change (a recorded stage:
/// median 1.2, nine in ten under 4.9) to full at `ROUGH_FULL`, at most
/// `ROAD_MAX` of the light motor, starting at a felt `ROAD_MIN` (rough
/// for about 9 % of a recorded stage).
const ROAD_MIN: f32 = 0.06;
/// The faint road feel everywhere: `ROAD_BASE` plus `ROAD_SPEED` at speed.
const ROAD_BASE: f32 = 0.02;
const ROAD_SPEED: f32 = 0.035;
const ROUGH_FROM: f32 = 4.0;
const ROUGH_FULL: f32 = 14.0;
const ROAD_MAX: f32 = 0.18;

/// The driving feel's state between updates.
#[derive(Debug, Clone, Copy, Default)]
pub struct Feel {
    car: Car,
    seen: bool,
    last: i64,
    accel: f32,
    spin: f32,
    lock: f32,
    kick_at: Option<i64>,
    /// When two or more wheels left the ground, and whether all four
    /// were off at some point since.
    air_since: Option<i64>,
    flew: bool,
    /// The last thump's strength and time.
    thump: f32,
    thump_at: i64,
    /// The wheels' load changes, smoothed: how rough the road is.
    rough: f32,
}

impl Feel {
    pub const fn new() -> Feel {
        Feel {
            car: Car {
                wheels: [0.0; 8],
                gear: 0,
                throttle: 0.0,
                brake: 0.0,
                loads: [0.0; 4],
                rpm: 0.0,
            },
            seen: false,
            last: 0,
            accel: 0.0,
            spin: 0.0,
            lock: 0.0,
            kick_at: None,
            air_since: None,
            flew: false,
            thump: 0.0,
            thump_at: 0,
            rough: 0.0,
        }
    }

    /// A thump of `level` at `now`, unless a stronger one is still felt.
    fn thump(&mut self, level: f32, now: i64, freq: i64) {
        if level > self.thump_level(now, freq) {
            self.thump = level.clamp(0.0, 1.0);
            self.thump_at = now;
        }
    }

    fn thump_level(&self, now: i64, freq: i64) -> f32 {
        let dt = (now - self.thump_at) as f32 / freq.max(1) as f32;
        (self.thump * (1.0 - dt / THUMP_FADE)).max(0.0)
    }

    /// Forgets the car (out of a race).
    pub fn reset(&mut self) {
        *self = Feel::new();
    }

    /// A new reading of the car at `now` (ticks of `freq` a second).
    pub fn update(&mut self, car: Car, now: i64, freq: i64) {
        if !self.seen || freq <= 0 {
            self.car = car;
            self.seen = true;
            self.last = now;
            return;
        }
        let dt = (now - self.last) as f32 / freq as f32;
        if dt <= 0.0 {
            return;
        }
        let raw = (car.speed() - self.car.speed()) / dt;
        // Smoothed over about two updates.
        self.accel = if dt > 0.5 {
            0.0
        } else {
            self.accel * 0.5 + raw * 0.5
        };
        let speed = car.speed();
        let spread = car.spread();
        let airborne = car.airborne() && speed > MOVING;
        let lifted = car.lifted() && speed > MOVING;
        // Flight and landing: all four wheels off, or a small jump that
        // lifts two (a recorded stage had 5 of each kind... 3 more small).
        match (self.air_since, lifted) {
            (None, true) => {
                self.air_since = Some(now);
                self.flew = airborne;
            }
            (Some(_), true) => self.flew |= airborne,
            (Some(since), false) => {
                let flight = (now - since) as f32 / freq as f32;
                if flight >= FLIGHT {
                    // Longer flights and harder landings knock harder; a
                    // small jump less.
                    let peak = car.loads.iter().fold(0.0f32, |m, &l| m.max(l));
                    let hard = ((peak - 36.0) / 34.0).clamp(0.0, 1.0);
                    let level = if self.flew {
                        0.35 + 0.4 * (flight / 0.5).min(1.0) + 0.25 * hard
                    } else {
                        0.25 + 0.25 * (flight / 0.5).min(1.0) + 0.15 * hard
                    };
                    self.thump(level, now, freq);
                }
                self.air_since = None;
                self.flew = false;
            }
            _ => {}
        }
        // Bumps: a wheel's load jumping up without a flight.
        if !lifted && self.air_since.is_none() {
            let rise = car
                .loads
                .iter()
                .zip(self.car.loads)
                .fold(0.0f32, |m, (&l, p)| m.max(l - p));
            if rise > BUMP_RISE && !self.car.lifted() {
                self.thump((0.15 + (rise - BUMP_RISE) / 45.0).min(0.7), now, freq);
            }
        }
        let change = car
            .loads
            .iter()
            .zip(self.car.loads)
            .map(|(&l, p)| (l - p).abs())
            .sum::<f32>()
            / 4.0;
        self.rough = if airborne {
            0.0
        } else {
            self.rough * 0.7 + change.min(20.0) * 0.3
        };
        self.spin = if car.throttle > 0.5 && !airborne {
            let by_accel = ((self.accel - SPIN_ACCEL) / 2.0).clamp(0.0, 1.0);
            let by_spread = ((spread - SPIN_SPREAD) * 3.0).clamp(0.0, 1.0);
            by_accel.max(by_spread)
        } else {
            0.0
        };
        self.lock = if car.brake > 0.5 && speed > 0.8 && !airborne {
            let by_decel = ((-self.accel - LOCK_DECEL) / 2.5).clamp(0.0, 1.0);
            let by_spread = ((spread - 0.15) * 3.0).clamp(0.0, 1.0);
            by_decel.max(by_spread)
        } else {
            0.0
        };
        // A gear change while driving (not the start's count to first).
        if car.gear != self.car.gear && self.car.gear > 0 && car.gear > 0 && speed > MOVING {
            self.kick_at = Some(now);
        }
        self.car = car;
        self.last = now;
    }

    /// The pad's output at `now`, at `strength` (0.0..=1.0); `impact` is
    /// the impacts' own (heavy, light) motor speeds, 0.0..=1.0.
    pub fn output(&self, now: i64, freq: i64, impact: (f32, f32), strength: f32) -> Output {
        self.output_with(now, freq, impact, strength, Choices::ALL)
    }

    /// `output` with some of the feel switched off.
    pub fn output_with(
        &self,
        now: i64,
        freq: i64,
        impact: (f32, f32),
        strength: f32,
        choices: Choices,
    ) -> Output {
        let mut o = self.output_all(now, freq, impact, strength);
        if !choices.triggers {
            o.left_trigger = 0.0;
            o.right_trigger = 0.0;
        }
        if !choices.road {
            o.light = impact
                .1
                .max(self.thump_level(now, freq) * strength.clamp(0.0, 1.0) * 0.5);
        }
        o
    }

    fn output_all(&self, now: i64, freq: i64, impact: (f32, f32), strength: f32) -> Output {
        let s = strength.clamp(0.0, 1.0);
        if !self.seen || freq <= 0 {
            return Output {
                heavy: impact.0,
                light: impact.1,
                ..Output::default()
            };
        }
        let t = (now as f64 / freq as f64) as f32;
        let speed = self.car.speed();
        let moving = speed > MOVING;
        let kick = match self.kick_at {
            Some(at) if ((now - at) as f32 / freq as f32) < KICK_TIME => KICK * s,
            _ => 0.0,
        };
        // The road: only where it is rough (the wheels' loads changing
        // more than on a smooth stretch), and faint. A hum with the speed
        // alone was felt all the time on gravel, tiring.
        let on_ground = !self.car.airborne();
        // A faint base with the speed (half the first version's, which
        // was felt all the time and tiring; none at all was missed).
        let base = if moving && on_ground {
            ROAD_BASE + ROAD_SPEED * (speed / 7.0).min(1.0)
        } else {
            0.0
        };
        let rough = if moving && on_ground && self.rough > ROUGH_FROM {
            ROAD_MIN
                + ((self.rough - ROUGH_FROM) / (ROUGH_FULL - ROUGH_FROM)).min(1.0)
                    * (ROAD_MAX - ROAD_MIN)
        } else {
            0.0
        };
        let road = base.max(rough) * s;
        // The rev limiter.
        let rev = if self.car.rpm > REDLINE {
            (0.12 + 0.12 * ((self.car.rpm - REDLINE) / 0.35).min(1.0)) * s
        } else {
            0.0
        };
        let thump = self.thump_level(now, freq) * s;
        // The right trigger: only when the wheels spin.
        let right = if self.spin > 0.0 {
            (0.3 + 0.7 * self.spin) * s
        } else {
            0.0
        };
        // The left trigger: a light hold under hard braking, pulses when
        // the wheels lock.
        let hold = if self.car.brake > 0.3 && moving {
            0.10 * self.car.brake
        } else {
            0.0
        };
        let pulse = if self.lock > 0.0 {
            // The phase within a pulse (no `fract` without `std`).
            let cycles = t * ABS_RATE;
            let whole = cycles as i64 as f32;
            let on = cycles - whole < 0.5;
            if on { 0.5 + 0.5 * self.lock } else { 0.1 }
        } else {
            0.0
        };
        Output {
            heavy: impact.0.max(kick).max(thump),
            light: impact.1.max(road).max(rev).max(thump * 0.5),
            left_trigger: hold.max(pulse) * s,
            right_trigger: right,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const F: i64 = 1000;

    fn car(speed: f32, gear: i16, throttle: f32, brake: f32) -> Car {
        Car {
            wheels: [speed; 8],
            gear,
            throttle,
            brake,
            loads: [36.0; 4],
            rpm: 6.0,
        }
    }

    fn with_loads(mut c: Car, loads: [f32; 4]) -> Car {
        c.loads = loads;
        c
    }

    #[test]
    fn landings_thump_after_a_flight() {
        let mut f = Feel::new();
        f.update(car(5.0, 4, 1.0, 0.0), 0, F);
        // 200 ms in the air, wheels spinning up: no wheelspin felt there.
        for t in [40, 80, 120, 160, 200] {
            f.update(
                with_loads(car(5.0 + t as f32 / 100.0, 4, 1.0, 0.0), [0.0; 4]),
                t,
                F,
            );
            let o = f.output(t, F, (0.0, 0.0), 1.0);
            assert_eq!(
                (o.right_trigger, o.light),
                (0.0, 0.0),
                "quiet in the air at {t}"
            );
        }
        f.update(with_loads(car(7.0, 4, 1.0, 0.0), [70.0; 4]), 240, F);
        let o = f.output(240, F, (0.0, 0.0), 1.0);
        assert!(o.heavy > 0.6, "{o:?}");
        assert_eq!(f.output(600, F, (0.0, 0.0), 1.0).heavy, 0.0, "faded");
        // A short hop (under 80 ms) doesn't.
        let mut g = Feel::new();
        g.update(car(5.0, 4, 1.0, 0.0), 0, F);
        g.update(with_loads(car(5.0, 4, 1.0, 0.0), [0.0; 4]), 40, F);
        g.update(car(5.0, 4, 1.0, 0.0), 80, F);
        assert_eq!(g.output(80, F, (0.0, 0.0), 1.0).heavy, 0.0);
    }

    #[test]
    fn bumps_knock_and_rough_roads_buzz_faintly() {
        let mut f = Feel::new();
        f.update(car(5.0, 4, 0.6, 0.0), 0, F);
        f.update(
            with_loads(car(5.0, 4, 0.6, 0.0), [36.0, 36.0, 66.0, 36.0]),
            40,
            F,
        );
        let o = f.output(40, F, (0.0, 0.0), 1.0);
        assert!(o.heavy > 0.15 && o.heavy < 0.7, "a bump: {o:?}");
        // A rough stretch: loads swinging by 15 each update.
        let mut g = Feel::new();
        for i in 0..20 {
            let l = if i % 2 == 0 { 30.0 } else { 42.0 };
            g.update(
                with_loads(car(5.0, 4, 0.6, 0.0), [l, 72.0 - l, l, 72.0 - l]),
                i * 40,
                F,
            );
        }
        let o = g.output(800, F, (0.0, 0.0), 1.0);
        assert!(o.light > 0.05 && o.light <= ROAD_MAX, "{o:?}");
    }

    #[test]
    fn choices_switch_the_trigger_and_road_feel_off() {
        assert_eq!(Choices::of_word(60), Choices::ALL);
        assert_eq!(
            Choices::of_word(60 | 0x100),
            Choices {
                triggers: false,
                road: true
            }
        );
        assert_eq!(
            Choices::of_word(60 | 0x200),
            Choices {
                triggers: true,
                road: false
            }
        );
        let mut f = Feel::new();
        let mut c = car(3.0, 2, 1.0, 0.0);
        c.rpm = 8.946;
        f.update(c, 0, F);
        f.update(car(3.4, 2, 1.0, 0.0), 40, F);
        let all = f.output(40, F, (0.0, 0.0), 1.0);
        assert!(all.right_trigger > 0.0 && all.light > 0.0, "{all:?}");
        let quiet = f.output_with(
            40,
            F,
            (0.0, 0.0),
            1.0,
            Choices {
                triggers: false,
                road: false,
            },
        );
        assert_eq!(
            (quiet.left_trigger, quiet.right_trigger, quiet.light),
            (0.0, 0.0, 0.0)
        );
        // Impacts and gear changes stay.
        let hit = f.output_with(
            40,
            F,
            (0.8, 0.4),
            1.0,
            Choices {
                triggers: false,
                road: false,
            },
        );
        assert_eq!((hit.heavy, hit.light), (0.8, 0.4));
    }

    #[test]
    fn the_rev_limiter_buzzes() {
        let mut f = Feel::new();
        let mut c = car(3.0, 2, 1.0, 0.0);
        c.rpm = 8.946;
        f.update(c, 0, F);
        f.update(c, 40, F);
        let o = f.output(40, F, (0.0, 0.0), 1.0);
        assert!(o.light > 0.2, "{o:?}");
    }

    #[test]
    fn reads_the_block() {
        let mut b = [0u8; 0x440];
        for i in 0..8 {
            b[0x2a8 + 4 * i..][..4].copy_from_slice(&(65536 * (i as i32 + 1)).to_le_bytes());
        }
        b[0x304..0x308].copy_from_slice(&0xffbeu32.to_le_bytes());
        b[0x308..0x30c].copy_from_slice(&0u32.to_le_bytes());
        b[0x43a..0x43c].copy_from_slice(&3i16.to_le_bytes());
        let c = unsafe { Car::read(b.as_ptr()) };
        assert_eq!(c.wheels[0], 1.0);
        assert_eq!(c.wheels[7], 8.0);
        assert_eq!(c.gear, 3);
        assert_eq!(c.throttle, 1.0);
        assert_eq!(c.brake, 0.0);
        assert_eq!(c.speed(), 4.5);
    }

    #[test]
    fn standing_still_is_quiet() {
        let mut f = Feel::new();
        f.update(car(0.0, 0, 0.0, 0.0), 0, F);
        f.update(car(0.0, 0, 0.0, 0.0), 40, F);
        assert_eq!(f.output(40, F, (0.0, 0.0), 1.0), Output::default());
    }

    #[test]
    fn a_smooth_cruise_at_full_throttle_only_hums_faintly() {
        let mut f = Feel::new();
        for i in 0..10 {
            f.update(car(5.0, 4, 1.0, 0.0), i * 40, F);
        }
        let o = f.output(400, F, (0.0, 0.0), 1.0);
        assert_eq!((o.heavy, o.left_trigger, o.right_trigger), (0.0, 0.0, 0.0));
        assert!(o.light > 0.02 && o.light < 0.06, "{o:?}");
    }

    #[test]
    fn small_jumps_on_two_wheels_thump_lighter() {
        let mut f = Feel::new();
        f.update(car(5.0, 4, 1.0, 0.0), 0, F);
        for t in [40, 80, 120] {
            f.update(
                with_loads(car(5.0, 4, 1.0, 0.0), [0.0, 0.0, 40.0, 40.0]),
                t,
                F,
            );
        }
        f.update(
            with_loads(car(5.0, 4, 1.0, 0.0), [60.0, 60.0, 36.0, 36.0]),
            160,
            F,
        );
        let small = f.output(160, F, (0.0, 0.0), 1.0).heavy;
        assert!(small > 0.25 && small < 0.6, "{small}");
    }

    #[test]
    fn wheelspin_drives_the_right_trigger() {
        let mut f = Feel::new();
        f.update(car(1.0, 1, 1.0, 0.0), 0, F);
        // +0.2 a 40 ms update: 5 units a second, far beyond a real car.
        f.update(car(1.2, 1, 1.0, 0.0), 40, F);
        f.update(car(1.4, 1, 1.0, 0.0), 80, F);
        let o = f.output(80, F, (0.0, 0.0), 1.0);
        assert!(o.right_trigger > 0.5, "{o:?}");
        // Off the throttle it stops.
        f.update(car(1.6, 1, 0.0, 0.0), 120, F);
        assert_eq!(f.output(120, F, (0.0, 0.0), 1.0).right_trigger, 0.0);
    }

    #[test]
    fn locked_wheels_pulse_the_left_trigger() {
        let mut f = Feel::new();
        f.update(car(5.0, 4, 0.0, 1.0), 0, F);
        f.update(car(4.6, 4, 0.0, 1.0), 40, F);
        f.update(car(4.2, 4, 0.0, 1.0), 80, F);
        // 10 units a second down: locking. Pulses alternate strong and weak.
        let a = f.output(1000, F, (0.0, 0.0), 1.0).left_trigger;
        let b = f.output(1036, F, (0.0, 0.0), 1.0).left_trigger;
        assert!(a.max(b) > 0.7 && a.min(b) <= 0.11, "{a} {b}");
        // Gentle braking: only a light hold.
        let mut g = Feel::new();
        g.update(car(5.0, 4, 0.0, 1.0), 0, F);
        g.update(car(4.98, 4, 0.0, 1.0), 40, F);
        let o = g.output(40, F, (0.0, 0.0), 1.0);
        assert!(o.left_trigger > 0.05 && o.left_trigger < 0.2, "{o:?}");
    }

    #[test]
    fn gear_changes_knock_briefly() {
        let mut f = Feel::new();
        f.update(car(4.0, 3, 1.0, 0.0), 0, F);
        f.update(car(4.1, 4, 1.0, 0.0), 40, F);
        assert!((f.output(60, F, (0.0, 0.0), 1.0).heavy - KICK).abs() < 1e-6);
        assert_eq!(f.output(200, F, (0.0, 0.0), 1.0).heavy, 0.0);
        // The start's first gear doesn't knock.
        let mut g = Feel::new();
        g.update(car(0.0, 0, 1.0, 0.0), 0, F);
        g.update(car(0.1, 1, 1.0, 0.0), 40, F);
        assert_eq!(g.output(50, F, (0.0, 0.0), 1.0).heavy, 0.0);
    }

    #[test]
    fn impacts_and_strength() {
        let mut f = Feel::new();
        f.update(car(5.0, 4, 1.0, 0.0), 0, F);
        f.update(car(5.0, 4, 1.0, 0.0), 40, F);
        let o = f.output(40, F, (0.9, 0.5), 0.5);
        assert_eq!(
            (o.heavy, o.light),
            (0.9, 0.5),
            "impacts keep their own scale"
        );
        let full = f.output(40, F, (0.0, 0.0), 1.0);
        let half = f.output(40, F, (0.0, 0.0), 0.5);
        assert!((half.right_trigger - full.right_trigger / 2.0).abs() < 1e-6);
    }
}
