//! Analog-axis dead-zone scaling (reconstruction item F02).
//!
//! # Scope
//!
//! The shipped game has **no interactive calibration screen** and no persisted
//! per-axis min/center/max or invert state: the recovered analog path
//! (`FUN_0049cdf0`, a WinMM `joyGetPosEx` backend) uses a fixed center of
//! `0x8000`, a raw range of `0..=0xffff`, and two fixed dead-zone constants read
//! from `.data` (`DAT_00504cc0` = 2000 for X, `DAT_00504cc4` = 2300 for Y) that
//! have no writers. This module reconstructs that fixed-dead-zone scaling as the
//! compatibility baseline, with the dead-zone passed in so a later input backend
//! can parameterize it without changing the baseline.
//!
//! See evidence `rea.official-eu.input-calibration`.

/// The fixed center of the raw axis range.
pub const AXIS_CENTER: i32 = 0x8000;
/// The maximum raw axis value.
pub const AXIS_RAW_MAX: u16 = 0xffff;

/// Fixed dead-zone for the X axis (`DAT_00504cc0`).
pub const DEADZONE_X: i32 = 2000;
/// Fixed dead-zone for the Y axis (`DAT_00504cc4`).
pub const DEADZONE_Y: i32 = 2300;

/// The threshold at which a scaled axis registers as a digital direction press.
pub const DIRECTION_THRESHOLD: i32 = 0x8000;

/// Scale a raw axis sample through a fixed dead-zone, reproducing
/// `FUN_0049cdf0`: center on `0x8000`, subtract the dead-zone from the deflection
/// (clamping the dead band to zero), rescale the remaining `0x8000 - deadzone`
/// span back to `0x8000`, then double. Full deflection yields roughly
/// `±0x10000`, so half deflection crosses [`DIRECTION_THRESHOLD`].
///
/// A `deadzone` of zero or more than the half-range leaves the span degenerate;
/// such inputs return `0` rather than dividing by a non-positive span.
pub fn scale_axis(raw: u16, deadzone: i32) -> i32 {
    let span = AXIS_CENTER - deadzone;
    if span <= 0 {
        return 0;
    }
    let centered = i32::from(raw) - AXIS_CENTER;
    let adjusted = if centered > deadzone {
        centered - deadzone
    } else if centered < -deadzone {
        centered + deadzone
    } else {
        0
    };
    (adjusted * AXIS_CENTER / span) * 2
}

/// The digital direction a scaled axis represents: `(negative, positive)`, each
/// set when the scaled value passes [`DIRECTION_THRESHOLD`] in that direction.
pub fn axis_direction(scaled: i32) -> (bool, bool) {
    (scaled < -DIRECTION_THRESHOLD, scaled > DIRECTION_THRESHOLD)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centered_sample_is_zero() {
        assert_eq!(scale_axis(AXIS_CENTER as u16, DEADZONE_X), 0);
        // Anything inside the dead band is zero.
        assert_eq!(
            scale_axis((AXIS_CENTER + DEADZONE_X - 1) as u16, DEADZONE_X),
            0
        );
        assert_eq!(
            scale_axis((AXIS_CENTER - DEADZONE_X + 1) as u16, DEADZONE_X),
            0
        );
    }

    #[test]
    fn full_deflection_passes_the_direction_threshold() {
        let high = scale_axis(AXIS_RAW_MAX, DEADZONE_X);
        assert!(high > DIRECTION_THRESHOLD, "scaled={high:#x}");
        assert_eq!(axis_direction(high), (false, true));

        let low = scale_axis(0, DEADZONE_X);
        assert!(low < -DIRECTION_THRESHOLD, "scaled={low:#x}");
        assert_eq!(axis_direction(low), (true, false));
    }

    #[test]
    fn just_past_the_deadzone_is_small_and_not_a_direction() {
        let scaled = scale_axis((AXIS_CENTER + DEADZONE_X + 1) as u16, DEADZONE_X);
        assert!(scaled > 0);
        assert_eq!(axis_direction(scaled), (false, false));
    }

    #[test]
    fn x_and_y_deadzones_differ() {
        // The wider Y dead band yields a slightly smaller value for the same raw.
        let raw = (AXIS_CENTER + 10000) as u16;
        let x = scale_axis(raw, DEADZONE_X);
        let y = scale_axis(raw, DEADZONE_Y);
        assert!(x > y, "x={x:#x} y={y:#x}");
    }

    #[test]
    fn degenerate_deadzone_is_safe() {
        assert_eq!(scale_axis(0xffff, AXIS_CENTER), 0); // span == 0
        assert_eq!(scale_axis(0xffff, AXIS_CENTER + 1), 0); // span < 0
    }
}
