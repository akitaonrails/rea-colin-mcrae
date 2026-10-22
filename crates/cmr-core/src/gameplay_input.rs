//! The live in-race driving-input composition: one per-frame source that runs
//! the whole recovered keyboard chain - acquisition -> control map -> sim bridge
//! - end to end.
//!
//! This ties together [`input_poll::DevicePoll`](crate::input_poll::DevicePoll)
//! (the per-record poll that turns raw host key state into a
//! [`DeviceSnapshot`]), [`input_map::InRaceControlMapper`] (the recovered
//! `FUN_00449580` device-to-car mapper), and
//! [`drive_controls::car_controls_to_sim`](crate::drive_controls::car_controls_to_sim)
//! (the routing bridge onto the race-state sim). A [`KeyboardDriver`] owns the
//! per-frame state of each stage - the poll's previous-held mask (for rising
//! edges), the mapper's reset latch and throttle-ramp counter, and the player's
//! bindings and config - so a host backend only supplies a `is_down(vk)` probe
//! and the full `CarControls` + sim `Controls` fall out.
//!
//! Only the raw per-VK boolean is the backend's to supply (shipped:
//! `GetAsyncKeyState`; an SDL host: a held-key set). Everything here is the
//! recovered transform, so the windowed and headless paths drive the identical
//! logic.

use crate::drive_controls::{DriveResponse, car_controls_to_sim};
use crate::input_map::{CarControls, ControlConfig, InRaceControlMapper, KeyBindings};
use crate::input_poll::DevicePoll;
use crate::race_state::Controls;

/// One frame's driving input: the recovered per-car [`CarControls`] block and
/// the sim [`Controls`] it maps to under the caller's [`DriveResponse`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriveFrame {
    /// The recovered control-map output for this frame.
    pub car: CarControls,
    /// The race-state sim controls (net force / steer rate / handbrake).
    pub controls: Controls,
}

/// A keyboard-backed in-race driving-input source composing the recovered poll,
/// control map, and sim bridge, carrying their per-frame state across frames.
#[derive(Clone, Debug)]
pub struct KeyboardDriver {
    bindings: KeyBindings,
    config: ControlConfig,
    poll: DevicePoll,
    mapper: InRaceControlMapper,
}

impl KeyboardDriver {
    /// A driver for the given keyboard bindings and control config (fresh poll
    /// and mapper state, matching the zeroed per-player globals).
    #[must_use]
    pub fn new(bindings: KeyBindings, config: ControlConfig) -> Self {
        KeyboardDriver {
            bindings,
            config,
            poll: DevicePoll::new(),
            mapper: InRaceControlMapper::new(),
        }
    }

    /// Player 1's default keyboard bindings (arrows + numpad) with the recovered
    /// default control config (digital, no invert, no ramp).
    #[must_use]
    pub fn player_one() -> Self {
        KeyboardDriver::new(KeyBindings::PLAYER1, ControlConfig::default())
    }

    /// Player 2's default keyboard bindings with the recovered default config.
    #[must_use]
    pub fn player_two() -> Self {
        KeyboardDriver::new(KeyBindings::PLAYER2, ControlConfig::default())
    }

    /// Poll one frame: `is_down(vk)` reports whether the host key bound to a
    /// Windows VK code is held. Builds the keyboard [`DeviceSnapshot`], maps it
    /// to [`CarControls`], and scales the net demands to sim [`Controls`] by
    /// `response`. `gear_enabled` is the recovered gate (`car+0x3f0 != 2`).
    pub fn poll(
        &mut self,
        is_down: impl Fn(u8) -> bool,
        response: &DriveResponse,
        gear_enabled: bool,
    ) -> DriveFrame {
        let snapshot = self.poll.poll_keyboard(&self.bindings, is_down);
        // A keyboard device is its own primary and keyboard record; the mapper's
        // keyboard-merge path reads only the device snapshot in that case.
        let car = self
            .mapper
            .map(&snapshot, &snapshot, &self.config, gear_enabled);
        let controls = car_controls_to_sim(&car, response);
        DriveFrame { car, controls }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixed_point::Fixed16;

    fn response() -> DriveResponse {
        DriveResponse {
            max_forward_force: Fixed16::from_raw(8 << 16),
            max_steer_rate: 0x200,
        }
    }

    // VK codes from KeyBindings::PLAYER1.
    const VK_LEFT: u8 = 0x25;
    const VK_UP: u8 = 0x26;
    const VK_RIGHT: u8 = 0x27;
    const VK_DOWN: u8 = 0x28;
    const VK_RESET: u8 = 0x66; // numpad 6

    #[test]
    fn throttle_key_drives_forward_and_brake_key_drives_back() {
        let mut driver = KeyboardDriver::player_one();
        let up = driver.poll(|vk| vk == VK_UP, &response(), true);
        assert!(up.car.throttle > 0);
        assert!(up.controls.forward_force > Fixed16::ZERO);

        let down = driver.poll(|vk| vk == VK_DOWN, &response(), true);
        assert!(down.car.brake > 0);
        assert!(down.controls.forward_force < Fixed16::ZERO);
    }

    #[test]
    fn steer_keys_give_opposite_rates_and_cancel_together() {
        let mut driver = KeyboardDriver::player_one();
        let left = driver.poll(|vk| vk == VK_LEFT, &response(), true);
        assert!(left.controls.steer_rate < 0);
        let right = driver.poll(|vk| vk == VK_RIGHT, &response(), true);
        assert!(right.controls.steer_rate > 0);
        // Both arrows held: the net steer cancels.
        let both = driver.poll(|vk| vk == VK_LEFT || vk == VK_RIGHT, &response(), true);
        assert_eq!(both.controls.steer_rate, 0);
    }

    #[test]
    fn no_key_held_is_the_neutral_control_frame() {
        let mut driver = KeyboardDriver::player_one();
        let frame = driver.poll(|_| false, &response(), true);
        assert_eq!(frame.controls.steer_rate, 0);
        assert_eq!(frame.controls.forward_force, Fixed16::ZERO);
        assert!(!frame.controls.handbrake);
        assert!(!frame.car.reset_fired);
    }

    #[test]
    fn reset_fires_once_per_press_across_frames() {
        let mut driver = KeyboardDriver::player_one();
        // The recovered latch starts disarmed (zeroed BSS); a clear frame arms it.
        assert!(!driver.poll(|_| false, &response(), true).car.reset_fired);
        // Next press edge (while armed) fires once.
        assert!(driver.poll(|vk| vk == VK_RESET, &response(), true).car.reset_fired);
        // Held again, no new edge: it does not re-fire.
        assert!(!driver.poll(|vk| vk == VK_RESET, &response(), true).car.reset_fired);
        // Release re-arms; the next press fires again.
        let _ = driver.poll(|_| false, &response(), true);
        assert!(driver.poll(|vk| vk == VK_RESET, &response(), true).car.reset_fired);
    }
}
