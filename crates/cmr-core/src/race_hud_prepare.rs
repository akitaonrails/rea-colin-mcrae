//! The per-player HUD value preparation `FUN_00464130`, run before the primary
//! HUD `FUN_00464930` in the normal single-player finalizer.
//!
//! Observations (pseudocode level): for the common gauge it stores the rev
//! fraction `(ctl +0x2e8 << 16) / ctl +0x15c` (`-1` when the redline word is
//! zero), the speed `qmul(car +0xec, 0x431168)`, scaled by `0x19c28` when the
//! units bits `DAT_00d3ca80 +0x18 & 7` are set and truncated to whole units
//! (zero below one), and the gear `ctl +0x43a`. When the car-type nibble
//! `DAT_00d3ca80 +0xb6 & 0xf` is 3..5 or 8 (outside replays) it prepares the
//! classic gauge instead: the rev becomes `(fraction * 20 >> 16) - 18` and the
//! speed `speed * 180 / 100`. The progress and split-table words it also
//! prepares are not reconstructed here.

use crate::fixed_point::Fixed16;

/// `ctl +0x2e8 / ctl +0x15c` in Q16, `-1` for a zero redline.
#[must_use]
pub fn hud_rev_fraction(engine_speed_2e8: i32, redline_15c: i32) -> i32 {
    if redline_15c == 0 {
        return -1;
    }
    ((i64::from(engine_speed_2e8) << 16) / i64::from(redline_15c)) as i32
}

/// The displayed speed from `car +0xec`.
#[must_use]
pub fn hud_speed(speed_ec: i32, metric_units: bool) -> i32 {
    let mut speed = qmul(speed_ec, 0x43_1168);
    if metric_units {
        speed = qmul(speed, 0x1_9c28);
    }
    if speed < 0x1_0000 { 0 } else { speed >> 16 }
}

/// The values `FUN_00464130` hands to the HUD.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreparedHudValues {
    pub rev_q16: i32,
    pub speed: i32,
    pub gear: i8,
}

/// The car words the preparation reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HudPrepareInput {
    pub engine_speed_2e8: i32,
    pub redline_15c: i32,
    pub speed_ec: i32,
    pub gear_43a: i8,
    /// `DAT_00d3ca80 +0x18 & 7 != 0`.
    pub metric_units: bool,
    /// The car-type nibble selects the classic gauge.
    pub classic_gauge: bool,
}

/// `FUN_00464130`'s rev, speed and gear words.
#[must_use]
pub fn prepare_hud_values(input: HudPrepareInput) -> PreparedHudValues {
    let fraction = hud_rev_fraction(input.engine_speed_2e8, input.redline_15c);
    let speed = hud_speed(input.speed_ec, input.metric_units);
    if input.classic_gauge {
        PreparedHudValues {
            rev_q16: (fraction.wrapping_mul(0x14) >> 16) - 0x12,
            speed: speed.wrapping_mul(0xb4) / 100,
            gear: input.gear_43a,
        }
    } else {
        PreparedHudValues {
            rev_q16: fraction,
            speed,
            gear: input.gear_43a,
        }
    }
}

/// Whether the car-type nibble selects the classic gauge (outside replays).
#[must_use]
pub fn classic_gauge_selected(car_type_nibble: u8, replay_d3ca88: bool) -> bool {
    let nibble = car_type_nibble & 0xf;
    !replay_d3ca88 && nibble > 2 && (nibble < 6 || nibble == 8)
}

fn qmul(left: i32, right: i32) -> i32 {
    Fixed16::from_raw(left).mul(Fixed16::from_raw(right)).raw()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> HudPrepareInput {
        HudPrepareInput {
            engine_speed_2e8: 0x2_8000,
            redline_15c: 0x5_0000,
            speed_ec: 0x1_0000,
            gear_43a: 3,
            metric_units: false,
            classic_gauge: false,
        }
    }

    #[test]
    fn the_rev_fraction_divides_by_the_redline() {
        assert_eq!(hud_rev_fraction(0x2_8000, 0x5_0000), 0x8000);
        assert_eq!(hud_rev_fraction(0x5_0000, 0x5_0000), 0x1_0000);
        assert_eq!(hud_rev_fraction(-0x2_8000, 0x5_0000), -0x8000);
        assert_eq!(hud_rev_fraction(0x1234, 0), -1);
    }

    #[test]
    fn the_speed_scales_and_truncates_to_whole_units() {
        // 1.0 units per tick shows as 67.
        assert_eq!(hud_speed(0x1_0000, false), 67);
        assert_eq!(hud_speed(0x1_0000, true), 107);
        // `qmul(0x3d1, 0x431168)` is just under one unit.
        assert_eq!(hud_speed(0x3d1, false), 0);
        assert_eq!(hud_speed(0x3d2, false), 1);
        assert_eq!(hud_speed(-0x1_0000, false), 0);
    }

    #[test]
    fn the_common_gauge_passes_the_words_through() {
        assert_eq!(
            prepare_hud_values(input()),
            PreparedHudValues {
                rev_q16: 0x8000,
                speed: 67,
                gear: 3,
            }
        );
    }

    #[test]
    fn the_classic_gauge_rescales_rev_and_speed() {
        let classic = prepare_hud_values(HudPrepareInput {
            classic_gauge: true,
            ..input()
        });
        assert_eq!(classic.rev_q16, ((0x8000 * 20) >> 16) - 18);
        assert_eq!(classic.speed, 67 * 180 / 100);
        assert_eq!(classic.gear, 3);
    }

    #[test]
    fn the_classic_gauge_follows_the_car_type_nibble() {
        for nibble in 0..16 {
            let expected = matches!(nibble, 3..=5 | 8);
            assert_eq!(classic_gauge_selected(nibble, false), expected, "{nibble}");
            assert!(!classic_gauge_selected(nibble, true));
        }
        assert!(classic_gauge_selected(0xf3, false));
    }
}
