//! Reconstruction of the in-race input acquisition and the device-to-control
//! mapping (B10): the pure transforms the recovered WinMM/keyboard poll and the
//! control mapper apply.
//!
//! In-race driving input is acquired through **WinMM `joyGetPosEx` plus raw
//! `GetAsyncKeyState`** - DirectInput is not used on this path. Each frame
//! `FUN_0049d180` polls up to 18 logical device records (`DAT_00b62b48`, stride
//! `0x8c`), dispatching by record type to a digital-joystick reader
//! (`FUN_0049cca0`), a keyboard reader (`FUN_0049cfb0`), or an analog
//! joystick/wheel reader (`FUN_0049cdf0`). `FUN_00449580` then maps the selected
//! record into the per-car control struct's `+0x2fc` command region (steer
//! left/right, brake, throttle, handbrake, reverse), from which the drivetrain
//! ramp stage produces the physical actuator scalars.
//!
//! This module reconstructs the cleanly-transcribable numeric transforms: the
//! rising-edge button mask, the 8-way digital direction from a WinMM axis, the
//! analog dead-zone rescale, the command quantize/saturate, and the analog
//! axis -> demand split. Device acquisition itself (the WinMM/Win32 calls) is
//! backend glue for the SDL host (B11/B12).
//!
//! See evidence `rea.official-eu.host-input-mapping`.
//!
//! Observations: the record count/stride, the device-type ids, the edge-mask
//! formula, the digital axis thresholds, the dead-zone rescale, the command
//! offsets and quantize/saturate, the analog-enable caps bits, and the default
//! button masks. Inference: the pedal-axis sign convention (+throttle / -brake)
//! and the steer split (-left / +right). Unknowns (not reproduced): the source /
//! configurability of the per-axis dead-zone values `DAT_00504cc0/cc4`, the
//! `DINPUT.dll` string's purpose (not on the poll path), and the `+0x2b8[4]`
//! actuator slot order (a drivetrain, not input, question).

/// Logical device records polled per frame (`for i in 0..0x12`).
pub const DEVICE_RECORD_COUNT: usize = 0x12;

/// Byte stride of one device record in the poll array `DAT_00b62b48`.
pub const DEVICE_RECORD_STRIDE: usize = 0x8c;

/// Centre of a WinMM axis (`0x8000`; the 0..0xffff range centres here).
pub const AXIS_CENTER: i32 = 0x8000;

/// Low digital threshold on a WinMM axis (`10000` = `0x2710`): below this reads
/// as the negative direction.
pub const JOY_AXIS_LOW_THRESHOLD: u32 = 10_000;

/// High digital threshold on a WinMM axis (`0xd8f0`): above this reads as the
/// positive direction.
pub const JOY_AXIS_HIGH_THRESHOLD: u32 = 0xd8f0;

/// Command demands quantize to clear the low 10 bits (`& 0xfffffc00`).
pub const DEMAND_QUANTIZE_MASK: i32 = !0x3ff;

/// Command demands saturate at `0xffbe` (~0.9988 of full scale).
pub const DEMAND_SATURATION: i32 = 0xffbe;

/// Control-struct command-region offsets written by `FUN_00449580`.
pub mod command_offset {
    /// Steer-left demand.
    pub const STEER_LEFT: usize = 0x2fc;
    /// Steer-right demand.
    pub const STEER_RIGHT: usize = 0x300;
    /// Brake demand.
    pub const BRAKE: usize = 0x304;
    /// Throttle demand.
    pub const THROTTLE: usize = 0x308;
    /// Handbrake (clutch) demand.
    pub const HANDBRAKE: usize = 0x30c;
    /// Alternate handbrake slot (config `[+0x24] & 0x40`).
    pub const HANDBRAKE_ALT: usize = 0x310;
    /// Reverse toggle.
    pub const REVERSE: usize = 0x314;
}

/// Per-axis analog-enable bits in the device caps byte `config[player*0x9c + 0x3c]`.
pub mod analog_caps {
    /// Steering axis is analog.
    pub const STEER: u8 = 0x4;
    /// Brake axis is analog.
    pub const BRAKE: u8 = 0x2;
    /// Throttle axis is analog.
    pub const THROTTLE: u8 = 0x1;
}

/// Recovered default button masks (used when no device-specific config matches).
pub mod default_button_mask {
    /// `DAT_0055aea8`: low 16 = brake button, high 16 = throttle button.
    pub const BRAKE_THROTTLE: u32 = 0x8_0004;
    /// `DAT_0055aeac`: low 16 = handbrake, high 16 = reverse.
    pub const HANDBRAKE_REVERSE: u32 = 0x200_0100;
    /// `DAT_0055aeb0`.
    pub const EXTRA_0: u32 = 0x400;
    /// `DAT_0055aeb4`.
    pub const EXTRA_1: u32 = 0x400;
}

/// A polled device record's type (`record+0x00`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceType {
    /// Type 0: a WinMM joystick used digitally (8-way from the axes).
    DigitalJoystick,
    /// Type 1: the keyboard (VK codes at `record+0x78..`).
    Keyboard,
    /// Types 2 and 4: an analog joystick / wheel.
    Analog,
    /// `0xffffffff`: an inactive record.
    Inactive,
}

impl DeviceType {
    /// Classify a record's type id.
    pub fn from_raw(type_id: u32) -> Option<Self> {
        match type_id {
            0 => Some(DeviceType::DigitalJoystick),
            1 => Some(DeviceType::Keyboard),
            2 | 4 => Some(DeviceType::Analog),
            0xffff_ffff => Some(DeviceType::Inactive),
            _ => None,
        }
    }
}

/// The per-frame rising-edge button mask, reproducing `FUN_0049cbe0`:
/// `(previous ^ current) & current` (bits newly pressed this frame).
pub fn rising_edge(previous: u32, current: u32) -> u32 {
    (previous ^ current) & current
}

/// The 8-way digital direction bits from a WinMM joystick's X/Y axes,
/// reproducing `FUN_0049cca0`: bit0 left / bit1 right from X, bit2 up / bit3
/// down from Y, using the low/high thresholds.
pub fn digital_direction_bits(x: u32, y: u32) -> u32 {
    let mut bits = 0u32;
    if x < JOY_AXIS_LOW_THRESHOLD {
        bits |= 1;
    } else if x > JOY_AXIS_HIGH_THRESHOLD {
        bits |= 2;
    }
    if y < JOY_AXIS_LOW_THRESHOLD {
        bits |= 4;
    } else if y > JOY_AXIS_HIGH_THRESHOLD {
        bits |= 8;
    }
    bits
}

/// Rescale a raw WinMM axis `position` (0..0xffff) through a centred dead-zone,
/// reproducing `FUN_0049cdf0`: centre at `0x8000`, zero inside `±deadzone`, and
/// outside it subtract the dead-zone and rescale so the surviving range still
/// spans full scale, then double. The result is a signed value ~`±0x10000`.
pub fn apply_axis_deadzone(position: u32, deadzone: i32) -> i32 {
    let dx = position as i32 - AXIS_CENTER;
    let span = AXIS_CENTER - deadzone;
    if dx.abs() <= deadzone || span <= 0 {
        return 0;
    }
    let adjusted = if dx > 0 { dx - deadzone } else { dx + deadzone };
    (adjusted * AXIS_CENTER / span) * 2
}

/// Quantize and saturate a command demand, reproducing the `& 0xfffffc00` /
/// clamp-to-`0xffbe` applied in `FUN_00449580` (non-negative demands).
pub fn clamp_demand(value: i32) -> i32 {
    (value & DEMAND_QUANTIZE_MASK).clamp(0, DEMAND_SATURATION)
}

/// The driving demands mapped from the command region (all non-negative,
/// quantized/saturated).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DrivingDemands {
    /// Steer-left demand (`+0x2fc`).
    pub steer_left: i32,
    /// Steer-right demand (`+0x300`).
    pub steer_right: i32,
    /// Brake demand (`+0x304`).
    pub brake: i32,
    /// Throttle demand (`+0x308`).
    pub throttle: i32,
}

/// Map the analog steer and pedal axes into the command demands, reproducing the
/// split in `FUN_00449580`: the steer axis splits into left (negative) / right
/// (positive); the pedal axis splits into throttle (positive) / brake
/// (negative); each is quantized and saturated.
pub fn map_analog_axes(steer_axis: i32, pedal_axis: i32) -> DrivingDemands {
    DrivingDemands {
        steer_left: clamp_demand((-steer_axis).max(0)),
        steer_right: clamp_demand(steer_axis.max(0)),
        brake: clamp_demand((-pedal_axis).max(0)),
        throttle: clamp_demand(pedal_axis.max(0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_types_classify_the_recovered_ids() {
        assert_eq!(DeviceType::from_raw(0), Some(DeviceType::DigitalJoystick));
        assert_eq!(DeviceType::from_raw(1), Some(DeviceType::Keyboard));
        assert_eq!(DeviceType::from_raw(2), Some(DeviceType::Analog));
        assert_eq!(DeviceType::from_raw(4), Some(DeviceType::Analog));
        assert_eq!(
            DeviceType::from_raw(0xffff_ffff),
            Some(DeviceType::Inactive)
        );
        assert_eq!(DeviceType::from_raw(3), None);
        assert_eq!(DEVICE_RECORD_COUNT, 18);
        assert_eq!(DEVICE_RECORD_STRIDE, 0x8c);
    }

    #[test]
    fn rising_edge_reports_only_newly_pressed_bits() {
        // bit0 held, bit1 newly pressed -> only bit1 is an edge.
        assert_eq!(rising_edge(0b01, 0b11), 0b10);
        // Nothing new.
        assert_eq!(rising_edge(0b11, 0b11), 0);
        // A release is not a rising edge.
        assert_eq!(rising_edge(0b11, 0b01), 0);
    }

    #[test]
    fn digital_direction_bits_cover_all_quadrants() {
        // Centred: no direction.
        assert_eq!(digital_direction_bits(0x8000, 0x8000), 0);
        // Left + up.
        assert_eq!(digital_direction_bits(0, 0), 1 | 4);
        // Right + down.
        assert_eq!(digital_direction_bits(0xffff, 0xffff), 2 | 8);
        // Just inside the thresholds: nothing.
        assert_eq!(
            digital_direction_bits(JOY_AXIS_LOW_THRESHOLD, JOY_AXIS_HIGH_THRESHOLD),
            0
        );
    }

    #[test]
    fn axis_deadzone_zeros_the_centre_and_scales_to_full() {
        let dz = 0x800;
        // Centre and within the dead-zone: zero.
        assert_eq!(apply_axis_deadzone(0x8000, dz), 0);
        assert_eq!(apply_axis_deadzone(0x8000 + 0x400, dz), 0);
        // Full positive deflection: near +full scale (~0x10000), positive. The
        // WinMM axis range is asymmetric (-0x8000..+0x7fff), so +full and -full
        // need not be equal magnitude - only the sign and near-full scale hold.
        let full_pos = apply_axis_deadzone(0xffff, dz);
        assert!((0xf001..=0x1_0000).contains(&full_pos));
        let full_neg = apply_axis_deadzone(0, dz);
        assert!((-0x1_0000..=-0xf001).contains(&full_neg));
        assert!(full_pos > 0 && full_neg < 0);
        // A degenerate dead-zone (>= centre) yields zero, no panic.
        assert_eq!(apply_axis_deadzone(0xffff, AXIS_CENTER), 0);
    }

    #[test]
    fn clamp_demand_quantizes_and_saturates() {
        // Low 10 bits cleared.
        assert_eq!(clamp_demand(0x1fff), 0x1c00);
        // Saturates at 0xffbe (which is itself low-10-bits-clear-safe: 0xfc00).
        assert_eq!(clamp_demand(0x2_0000), DEMAND_SATURATION);
        // Negative clamps to zero.
        assert_eq!(clamp_demand(-5), 0);
    }

    #[test]
    fn analog_axes_split_into_the_command_demands() {
        // Positive pedal -> throttle only; steer right -> steer_right only.
        let d = map_analog_axes(0x4000, 0x4000);
        assert_eq!(d.throttle, clamp_demand(0x4000));
        assert_eq!(d.brake, 0);
        assert_eq!(d.steer_right, clamp_demand(0x4000));
        assert_eq!(d.steer_left, 0);
        // Negative pedal -> brake; steer left.
        let d = map_analog_axes(-0x4000, -0x4000);
        assert_eq!(d.brake, clamp_demand(0x4000));
        assert_eq!(d.throttle, 0);
        assert_eq!(d.steer_left, clamp_demand(0x4000));
        assert_eq!(d.steer_right, 0);
    }

    #[test]
    fn recovered_constants_match() {
        assert_eq!(JOY_AXIS_LOW_THRESHOLD, 0x2710);
        assert_eq!(JOY_AXIS_HIGH_THRESHOLD, 0xd8f0);
        assert_eq!(DEMAND_SATURATION, 0xffbe);
        assert_eq!(command_offset::STEER_LEFT, 0x2fc);
        assert_eq!(command_offset::REVERSE, 0x314);
        assert_eq!(analog_caps::STEER, 4);
        assert_eq!(default_button_mask::BRAKE_THROTTLE, 0x8_0004);
    }
}
