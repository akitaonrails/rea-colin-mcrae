//! The per-frame device-poll record build - the readers `FUN_0049d180` runs per
//! device record (`FUN_0049cca0` digital joystick, `FUN_0049cfb0` keyboard,
//! `FUN_0049cdf0` analog) - turning raw host device state into an
//! [`input_map::DeviceSnapshot`] by composing the [`crate::input_device`]
//! primitives. This is the acquisition layer between the backend and the control
//! map [`crate::input_map::InRaceControlMapper`].
//!
//! The raw per-key / per-button / per-axis state is the backend's to supply: on
//! the shipped build it is `GetAsyncKeyState` (keyboard) and `joyGetPosEx`
//! (joystick); an SDL backend supplies the same booleans/positions. Everything
//! downstream - the level mask from the bound keys or the axis thresholds, the
//! rising-edge mask `(prev ^ cur) & cur`, and the centred axis dead-zone - is the
//! recovered transform.
//!
//! Edges are per device: each [`DevicePoll`] remembers the previous frame's level
//! mask, exactly as the shipped per-record edge computation `FUN_0049cbe0` does.
//! The joystick button-bit packing (which physical button maps to which action
//! bit) is the backend/config's concern, so the pollers take the already-assembled
//! `buttons` level mask and only add the recovered direction bits / axes.

use crate::input_device::{DeviceType, apply_axis_deadzone, digital_direction_bits, rising_edge};
use crate::input_map::{DeviceSnapshot, KeyBindings, PEDAL_DEADZONE, STEER_DEADZONE};

/// One device's per-frame poll, tracking the previous level mask so the
/// rising-edge mask can be computed across frames (the per-record `+0x08` edge
/// field, `FUN_0049cbe0`).
#[derive(Clone, Copy, Debug, Default)]
pub struct DevicePoll {
    /// The previous frame's level (held) mask for this device.
    prev_held: u32,
}

impl DevicePoll {
    /// A fresh poll (nothing held yet, matching the zeroed record).
    #[must_use]
    pub fn new() -> Self {
        DevicePoll::default()
    }

    /// Build a keyboard record (`FUN_0049cfb0`): the level mask is bit `n` set
    /// when the VK code bound to action bit `n` is down (`GetAsyncKeyState(vk) &
    /// 0x8000`); the edge mask is the rising edge across frames; a keyboard has no
    /// analog axes.
    pub fn poll_keyboard(
        &mut self,
        bindings: &KeyBindings,
        is_down: impl Fn(u8) -> bool,
    ) -> DeviceSnapshot {
        let held = bindings.held_mask(is_down);
        self.finish(DeviceType::Keyboard, held, 0, 0)
    }

    /// Build a digital-joystick record (`FUN_0049cca0`): the level mask is the
    /// 8-way direction bits from the X/Y axis thresholds OR'd with the already
    /// assembled `buttons` mask (bits `0x10..`); no analog axes are carried.
    pub fn poll_digital(&mut self, axis_x: u32, axis_y: u32, buttons: u32) -> DeviceSnapshot {
        let held = digital_direction_bits(axis_x, axis_y) | buttons;
        self.finish(DeviceType::DigitalJoystick, held, 0, 0)
    }

    /// Build an analog-joystick/wheel record (`FUN_0049cdf0`): the level mask is
    /// the `buttons` mask; the steering (X) and pedal (Y) axes are centred and
    /// dead-zone-rescaled by [`apply_axis_deadzone`] with the recovered dead-zones.
    pub fn poll_analog(&mut self, raw_x: u32, raw_y: u32, buttons: u32) -> DeviceSnapshot {
        let axis_x = apply_axis_deadzone(raw_x, STEER_DEADZONE);
        let axis_y = apply_axis_deadzone(raw_y, PEDAL_DEADZONE);
        self.finish(DeviceType::Analog, buttons, axis_x, axis_y)
    }

    fn finish(&mut self, kind: DeviceType, held: u32, axis_x: i32, axis_y: i32) -> DeviceSnapshot {
        let edge = rising_edge(self.prev_held, held);
        self.prev_held = held;
        DeviceSnapshot {
            kind: Some(kind),
            held,
            edge,
            axis_x,
            axis_y,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input_device::AXIS_CENTER;
    use crate::input_map::{BIT_STEER_LEFT, BIT_STEER_RIGHT};

    #[test]
    fn keyboard_poll_builds_level_and_edge_masks() {
        let mut poll = DevicePoll::new();
        // LEFT (0x25) + UP (0x26) down -> steer-left + throttle (bit 2) held.
        let down = |vk: u8| vk == 0x25 || vk == 0x26;
        let first = poll.poll_keyboard(&KeyBindings::PLAYER1, down);
        assert_eq!(first.kind, Some(DeviceType::Keyboard));
        assert_eq!(first.held & BIT_STEER_LEFT, BIT_STEER_LEFT);
        assert_eq!(first.held & (1 << 2), 1 << 2);
        // Everything held is a fresh edge on the first frame.
        assert_eq!(first.edge, first.held);
        // Held again: no new edge.
        let second = poll.poll_keyboard(&KeyBindings::PLAYER1, down);
        assert_eq!(second.held, first.held);
        assert_eq!(second.edge, 0);
        // Add RIGHT: only the right bit is a new edge.
        let third = poll.poll_keyboard(&KeyBindings::PLAYER1, |vk| {
            vk == 0x25 || vk == 0x26 || vk == 0x27
        });
        assert_eq!(third.edge, BIT_STEER_RIGHT);
    }

    #[test]
    fn digital_poll_combines_direction_bits_with_buttons() {
        let mut poll = DevicePoll::new();
        // X below the low threshold -> left (bit 0); Y centred -> nothing; plus a
        // button at 0x10.
        let snap = poll.poll_digital(0, AXIS_CENTER as u32, 0x10);
        assert_eq!(snap.kind, Some(DeviceType::DigitalJoystick));
        assert_eq!(snap.held & BIT_STEER_LEFT, BIT_STEER_LEFT);
        assert_eq!(snap.held & 0x10, 0x10);
        assert_eq!(snap.axis_x, 0);
        assert_eq!(snap.axis_y, 0);
        // The direction bit and the button are both fresh edges.
        assert_eq!(snap.edge, snap.held);
    }

    #[test]
    fn analog_poll_deadzones_the_axes_and_carries_buttons() {
        let mut poll = DevicePoll::new();
        let center = AXIS_CENTER as u32;
        // Centred axes -> zero; a button bit held.
        let centered = poll.poll_analog(center, center, 0x20);
        assert_eq!(centered.kind, Some(DeviceType::Analog));
        assert_eq!(centered.axis_x, 0);
        assert_eq!(centered.axis_y, 0);
        assert_eq!(centered.held & 0x20, 0x20);
        // Full deflection -> signed axes (X right positive, Y down positive).
        let deflected = poll.poll_analog(0xffff, 0, 0);
        assert!(deflected.axis_x > 0);
        assert!(deflected.axis_y < 0);
    }
}
