//! The in-race control map `FUN_00449580` (reconstruction item B10/G02 boundary):
//! a device-input snapshot -> the per-car control block, reconstructed
//! backend-neutrally.
//!
//! In the driving tick (state 5, `FUN_00437ac0`) the shipped build calls
//! `FUN_00449580(car_control_block, scheme, player)` once per player to turn the
//! polled device state into the car's control fields. The recovery
//! (`rea.official-eu.in-race-control-map`, asm) shows:
//!
//! - The device layer polls up to 18 devices (`FUN_0049d180`), each a record
//!   with a **level mask** (`+0x04`, keys/buttons held), a **rising-edge mask**
//!   (`+0x08`, just-pressed), and, for analog devices, two dead-zoned/rescaled
//!   **axes** (`+0x78` X, `+0x7c` Y). Devices 0/1 are the P1/P2 keyboards; 2..17
//!   are joysticks. Keyboard bit `n` reads `GetAsyncKeyState(bindings[n])`. These
//!   poll-layer transforms live in [`crate::input_device`], which this map reuses.
//! - The map merges the player's primary device with the keyboard (unless the
//!   primary *is* the keyboard), then writes the control block:
//!   - **steer** `+0x2fc` (left) / `+0x300` (right), flag `+0x410`: digital bits
//!     0/1 -> full-scale `0xffbe`, or the analog X axis sign-split; swapped when
//!     the invert-steer config bit is set.
//!   - **throttle** `+0x304` (flag `+0x414`): analog Y<0, or digital mask bit.
//!   - **brake** `+0x308` (flag `+0x418`): analog Y>0, or digital mask bit.
//!   - **handbrake** `+0x30c` (or `+0x310`): digital mask bit.
//!   - **gear command** `+0x314`: edge-triggered, gear-up (`+1`) / gear-down
//!     (`0xff`, i.e. down), suppressed while the car's `+0x3f0 == 2`.
//!   - **change view**: an edge-armed latch firing `FUN_00434210(player)` once
//!     per press; that routine cycles the camera view (`FUN_00434060`).
//! - A digital press writes the un-quantized `0xffbe`; an analog magnitude is
//!   clamped to `0xffbe` **then** quantized by `& 0xfffffc00` (low 10 bits
//!   cleared, so analog saturates at `0xfc00`). The poll-layer axis pre-scale is
//!   [`crate::input_device::apply_axis_deadzone`], with the dead-zones `0x7d0`
//!   (X/steer) and `0x8fc` (Y/pedals).
//!
//! **Correction (recovered):** the earlier `G02`/stage notes that labelled
//! `+0x2fc` throttle / `+0x300` brake / `+0x304`-`+0x308` steer are **inverted** -
//! the analog routing (one horizontal axis with one flag -> steering; one
//! vertical axis split into two independent magnitudes -> throttle/brake) and the
//! default keyboard bindings (LEFT/RIGHT steer, UP throttle, DOWN brake) prove the
//! labels used here. `+0x304` is throttle (negative pedal axis), `+0x308` is brake
//! (positive pedal axis).
//!
//! This module reconstructs the pure transform (the device poll - `joyGetPosEx`/
//! `GetAsyncKeyState` - stays the backend's job;
//! [`crate::input_device::apply_axis_deadzone`] and [`KeyBindings::held_mask`] let
//! a backend build the snapshot). The per-player change-view latch and the alternate
//! throttle ramp are per-player state, so the map is a small stateful
//! [`InRaceControlMapper`].

use crate::input_device::{self, DeviceType, clamp_demand};

/// Full-scale control magnitude (`0xffbe`): a digital full-lock / full-pedal
/// writes this value un-quantized, and it is the analog clamp ceiling (before
/// quantization). Aliased to the poll layer's [`input_device::DEMAND_SATURATION`].
pub const FULL_SCALE: i32 = input_device::DEMAND_SATURATION;

/// Steering-axis (X) dead-zone, out of `0x8000` (`DAT_00504cc0`).
pub const STEER_DEADZONE: i32 = 0x7d0;

/// Pedal-axis (Y) dead-zone, out of `0x8000` (`DAT_00504cc4`).
pub const PEDAL_DEADZONE: i32 = 0x8fc;

/// Hardcoded steer-left action bit (device bit 0).
pub const BIT_STEER_LEFT: u32 = 1 << 0;
/// Hardcoded steer-right action bit (device bit 1).
pub const BIT_STEER_RIGHT: u32 = 1 << 1;

/// The remappable action-bit masks (`DAT_0055aea8/ac/b0`), defaulting to the
/// recovered hardcoded set used when the live device does not match a configured
/// custom binding. These decompose the poll layer's packed
/// [`input_device::default_button_mask`] into one bit per action.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActionMasks {
    /// Throttle bit (default bit 2, `0x4`).
    pub throttle: u32,
    /// Brake bit (default bit 3, `0x8`).
    pub brake: u32,
    /// Handbrake bit (default bit 8, `0x100`).
    pub handbrake: u32,
    /// Gear-up bit (default bit 9, `0x200`).
    pub gear_up: u32,
    /// Gear-down bit (default bit 7, `0x80`).
    pub gear_down: u32,
    /// Change-view bit (default bit 6, `0x40`): `FUN_00434210` cycles the
    /// camera view.
    pub change_view: u32,
}

impl Default for ActionMasks {
    fn default() -> Self {
        // DAT_0055aea8 = 0x00080004, DAT_0055aeac = 0x02000100,
        // DAT_0055aeb0 = 0x00400080: lo = (&0xffff), hi = (>>16).
        ActionMasks {
            throttle: 0x0004,
            brake: 0x0008,
            handbrake: 0x0100,
            gear_up: 0x0200,
            gear_down: 0x0080,
            change_view: 0x0040,
        }
    }
}

/// One device's polled state for a frame (the recovered record `+0x04`/`+0x08`/
/// `+0x78`/`+0x7c`). Axes are already dead-zoned/rescaled by the poll layer
/// ([`crate::input_device::apply_axis_deadzone`]); for non-analog devices they
/// are ignored.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DeviceSnapshot {
    /// The device kind (`None` is treated as a non-keyboard, non-analog record).
    pub kind: Option<DeviceType>,
    /// Level mask: action bits currently held (`+0x04`).
    pub held: u32,
    /// Rising-edge mask: action bits just pressed this frame (`+0x08`).
    pub edge: u32,
    /// Steering axis X, pre-scaled (`+0x78`).
    pub axis_x: i32,
    /// Pedal axis Y, pre-scaled (`+0x7c`).
    pub axis_y: i32,
}

/// The per-player control-config bits the map reads (`DAT_00d3ca80 + player*0x9c`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ControlConfig {
    /// Analog steering enabled (`cfg+0x3c & 4`).
    pub analog_steer: bool,
    /// Analog throttle enabled (`cfg+0x3c & 2`).
    pub analog_throttle: bool,
    /// Analog brake enabled (`cfg+0x3c & 1`).
    pub analog_brake: bool,
    /// Invert steering (`cfg+0x25 & 0x30`): swap left/right.
    pub invert_steer: bool,
    /// Route the handbrake to `+0x310` instead of `+0x30c` (`cfg+0x24 & 0x40`).
    pub handbrake_alt: bool,
    /// Alternate throttle ramp mode (`cfg+0x26 & 8`); also forces handbrake off.
    pub ramp_throttle: bool,
    /// The action-bit masks for this player's device.
    pub masks: ActionMasks,
}

/// The gear/discrete command the map writes to `+0x314`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum GearCommand {
    /// No shift this frame (`+0x314 == 0`).
    #[default]
    None,
    /// Shift up (`+0x314 == 1`).
    Up,
    /// Shift down (`+0x314 == 0xff`).
    Down,
}

/// The recovered per-car control block fields the map writes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CarControls {
    /// Steer-left magnitude (`+0x2fc`).
    pub steer_left: i32,
    /// Steer-right magnitude (`+0x300`).
    pub steer_right: i32,
    /// Throttle magnitude (`+0x304`).
    pub throttle: i32,
    /// Brake magnitude (`+0x308`).
    pub brake: i32,
    /// Handbrake engaged (`+0x30c`, or `+0x310` when `handbrake_alt`).
    pub handbrake: bool,
    /// Whether the handbrake was routed to the alternate slot `+0x310`.
    pub handbrake_alt: bool,
    /// Steering came from the analog path (`+0x410`).
    pub steer_analog: bool,
    /// Throttle came from the analog path (`+0x414`).
    pub throttle_analog: bool,
    /// Brake came from the analog path (`+0x418`).
    pub brake_analog: bool,
    /// The gear/discrete command (`+0x314`).
    pub gear_command: GearCommand,
    /// The change-view action fired this frame (`FUN_00434210`).
    pub change_view_fired: bool,
}

/// A default keyboard binding table: the VK code bound to each action bit index
/// (`record+0x78+n`), or `None` when the bit has no key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeyBindings {
    /// VK code per action-bit index `0..16`.
    pub codes: [Option<u8>; 16],
}

impl KeyBindings {
    /// The recovered Player-1 default keyboard bindings (`FUN_004b9f70`,
    /// device 0): arrows to steer/throttle/brake, numpad to handbrake/gears/
    /// change view, ESC to pause.
    pub const PLAYER1: KeyBindings = KeyBindings {
        codes: {
            let mut c = [None; 16];
            c[0] = Some(0x25); // VK_LEFT  - steer left
            c[1] = Some(0x27); // VK_RIGHT - steer right
            c[2] = Some(0x26); // VK_UP    - throttle
            c[3] = Some(0x28); // VK_DOWN  - brake
            c[6] = Some(0x66); // VK_NUMPAD6 - change view
            c[7] = Some(0x62); // VK_NUMPAD2 - gear down
            c[8] = Some(0x64); // VK_NUMPAD4 - handbrake
            c[9] = Some(0x68); // VK_NUMPAD8 - gear up
            c[10] = Some(0x1b); // VK_ESCAPE - pause
            c
        },
    };

    /// The recovered Player-2 default keyboard bindings (device 1): A/S steer,
    /// R throttle, D brake, T handbrake, TAB gear up, Q gear down, ESC pause.
    pub const PLAYER2: KeyBindings = KeyBindings {
        codes: {
            let mut c = [None; 16];
            c[0] = Some(0x41); // 'A' - steer left
            c[1] = Some(0x53); // 'S' - steer right
            c[2] = Some(0x52); // 'R' - throttle
            c[3] = Some(0x44); // 'D' - brake
            c[6] = Some(0x5c); // raw 0x5c - change view
            c[7] = Some(0x51); // 'Q' - gear down
            c[8] = Some(0x54); // 'T' - handbrake
            c[9] = Some(0x09); // VK_TAB - gear up
            c[10] = Some(0x1b); // VK_ESCAPE - pause
            c
        },
    };

    /// `FUN_0049d3f0(dev, mask, code)` on a keyboard record: a single-bit
    /// `mask` binds its action bit to `code`'s low byte (`+0x78 + bit`); any
    /// other mask is ignored.
    pub fn install(&mut self, mask: u16, code: u32) {
        if mask.count_ones() == 1 {
            let byte = code as u8;
            self.codes[mask.trailing_zeros() as usize] = (byte != 0).then_some(byte);
        }
    }

    /// `FUN_004ba060` for one player at the config load: the eight bindings
    /// of the record's player block (`+0x64` masks, `+0x78` codes) installed
    /// over the keyboard's defaults.
    #[must_use]
    pub fn with_record(mut self, record: &[u8], player: usize) -> Self {
        let base = player * crate::settings_store::PLAYER_STRIDE;
        for n in 0..crate::control_bindings::BINDING_COUNT {
            let at = base + crate::control_bindings::keymask_offset(n);
            let mask = u16::from_le_bytes([record[at], record[at + 1]]);
            let at = base + crate::control_bindings::physcode_offset(n);
            let code =
                u32::from_le_bytes([record[at], record[at + 1], record[at + 2], record[at + 3]]);
            self.install(mask, code);
        }
        self
    }

    /// Build the level mask for a keyboard from a key-down predicate: bit `n` is
    /// set when its bound VK code is down (`FUN_0049cfb0`).
    #[must_use]
    pub fn held_mask(&self, is_down: impl Fn(u8) -> bool) -> u32 {
        let mut mask = 0u32;
        for (bit, code) in self.codes.iter().enumerate() {
            if let Some(vk) = code
                && is_down(*vk)
            {
                mask |= 1u32 << bit;
            }
        }
        mask
    }
}

/// The stateful in-race control map, reproducing `FUN_00449580` for one player
/// (the change-view latch and the throttle ramp are per-player state across frames).
#[derive(Clone, Copy, Debug, Default)]
pub struct InRaceControlMapper {
    /// The change-view latch arm flag (`DAT_0055ae7c[player]`), so it fires once per
    /// press.
    change_view_armed: bool,
    /// The alternate-ramp throttle counter (`DAT_0055ae74[player]`).
    ramp_counter: i32,
    /// Previous frame's throttle-held state, to detect the ramp's re-trigger.
    ramp_prev_held: bool,
}

impl InRaceControlMapper {
    /// A fresh mapper (matches the zeroed BSS globals: change view not yet armed).
    #[must_use]
    pub fn new() -> Self {
        InRaceControlMapper::default()
    }

    /// Map one frame's device + keyboard snapshot to the car control block.
    ///
    /// `gear_enabled` is the recovered gate `car+0x3f0 != 2` (the discrete gear
    /// command is suppressed while it is `false`).
    pub fn map(
        &mut self,
        device: &DeviceSnapshot,
        keyboard: &DeviceSnapshot,
        config: &ControlConfig,
        gear_enabled: bool,
    ) -> CarControls {
        let is_keyboard = device.kind == Some(DeviceType::Keyboard);
        let (held, edge) = if is_keyboard {
            (device.held, device.edge)
        } else {
            (device.held | keyboard.held, device.edge | keyboard.edge)
        };
        let analog = device.kind == Some(DeviceType::Analog);
        let m = &config.masks;

        let (steer_left, steer_right, steer_analog) =
            self.map_steering(device, held, config, analog);
        let (throttle, throttle_analog) = self.map_throttle(device, held, config, analog, m);
        let (brake, brake_analog) = map_brake(device, held, config, analog, m);

        // Handbrake: a plain mask bit, forced off in the ramp mode.
        let handbrake = !config.ramp_throttle && (held & m.handbrake != 0);

        let gear_command = if gear_enabled {
            map_gear(edge, m)
        } else {
            GearCommand::None
        };

        CarControls {
            steer_left,
            steer_right,
            throttle,
            brake,
            handbrake,
            handbrake_alt: config.handbrake_alt,
            steer_analog,
            throttle_analog,
            brake_analog,
            gear_command,
            change_view_fired: self.update_change_view(edge & m.change_view != 0),
        }
    }

    fn map_steering(
        &self,
        device: &DeviceSnapshot,
        held: u32,
        config: &ControlConfig,
        analog: bool,
    ) -> (i32, i32, bool) {
        if !analog || !config.analog_steer {
            let left = held & BIT_STEER_LEFT != 0;
            let right = held & BIT_STEER_RIGHT != 0;
            let (left, right) = maybe_swap(left, right, config.invert_steer);
            let scale = |on: bool| if on { FULL_SCALE } else { 0 };
            (scale(left), scale(right), false)
        } else {
            // One horizontal axis, sign-split into the two half-channels.
            let x = device.axis_x;
            let left = if x < 0 { clamp_demand(-x) } else { 0 };
            let right = if x > 0 { clamp_demand(x) } else { 0 };
            let (left, right) = maybe_swap(left, right, config.invert_steer);
            (left, right, true)
        }
    }

    fn map_throttle(
        &mut self,
        device: &DeviceSnapshot,
        held: u32,
        config: &ControlConfig,
        analog: bool,
        masks: &ActionMasks,
    ) -> (i32, bool) {
        if analog && config.analog_throttle {
            let y = device.axis_y;
            let value = if y < 0 { clamp_demand(-y) } else { 0 };
            (value, true)
        } else if !config.ramp_throttle {
            let value = if held & masks.throttle != 0 {
                FULL_SCALE
            } else {
                0
            };
            (value, false)
        } else {
            (self.ramp_throttle(held & masks.throttle != 0), false)
        }
    }

    /// The alternate throttle ramp (`cfg+0x26 & 8`): a counter seeded to `0x64`
    /// when the throttle bit re-engages and decremented each frame, yielding a
    /// brief decaying pulse while above `0x60`. The arithmetic is observed; its
    /// gameplay purpose is unrecovered, so the re-trigger (the throttle bit's
    /// rising edge) is an inference.
    fn ramp_throttle(&mut self, held: bool) -> i32 {
        if held && !self.ramp_prev_held {
            self.ramp_counter = 0x64;
        }
        self.ramp_prev_held = held;
        if self.ramp_counter > 0 {
            self.ramp_counter -= 1;
        }
        if self.ramp_counter <= 0x60 {
            return 0;
        }
        let stage = ((i64::from(self.ramp_counter - 0x60) << 16) * 0x553f) >> 16;
        ((stage * i64::from(FULL_SCALE)) >> 16) as i32
    }

    /// The change-view edge latch: arm when the edge is clear, fire once (and
    /// disarm) when it is set while armed.
    fn update_change_view(&mut self, view_edge: bool) -> bool {
        if !view_edge {
            self.change_view_armed = true;
            false
        } else if self.change_view_armed {
            self.change_view_armed = false;
            true
        } else {
            false
        }
    }
}

fn map_brake(
    device: &DeviceSnapshot,
    held: u32,
    config: &ControlConfig,
    analog: bool,
    masks: &ActionMasks,
) -> (i32, bool) {
    if analog && config.analog_brake {
        let y = device.axis_y;
        let value = if y > 0 { clamp_demand(y) } else { 0 };
        (value, true)
    } else {
        let value = if held & masks.brake != 0 {
            FULL_SCALE
        } else {
            0
        };
        (value, false)
    }
}

fn map_gear(edge: u32, masks: &ActionMasks) -> GearCommand {
    // Gear-down is tested second in the shipped code, so it wins a tie.
    if edge & masks.gear_down != 0 {
        GearCommand::Down
    } else if edge & masks.gear_up != 0 {
        GearCommand::Up
    } else {
        GearCommand::None
    }
}

fn maybe_swap<T>(left: T, right: T, swap: bool) -> (T, T) {
    if swap { (right, left) } else { (left, right) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input_device::{AXIS_CENTER, DEMAND_QUANTIZE_MASK, apply_axis_deadzone};

    fn keyboard(held: u32, edge: u32) -> DeviceSnapshot {
        DeviceSnapshot {
            kind: Some(DeviceType::Keyboard),
            held,
            edge,
            axis_x: 0,
            axis_y: 0,
        }
    }

    fn analog(axis_x: i32, axis_y: i32, held: u32) -> DeviceSnapshot {
        DeviceSnapshot {
            kind: Some(DeviceType::Analog),
            held,
            edge: 0,
            axis_x,
            axis_y,
        }
    }

    #[test]
    fn digital_keyboard_steering_throttle_brake() {
        let mut mapper = InRaceControlMapper::new();
        let config = ControlConfig::default();
        // Steer left + full throttle.
        let dev = keyboard(BIT_STEER_LEFT | config.masks.throttle, 0);
        let out = mapper.map(&dev, &keyboard(0, 0), &config, true);
        assert_eq!(out.steer_left, FULL_SCALE);
        assert_eq!(out.steer_right, 0);
        assert_eq!(out.throttle, FULL_SCALE);
        assert_eq!(out.brake, 0);
        assert!(!out.steer_analog && !out.throttle_analog);

        // Brake + steer right.
        let dev = keyboard(BIT_STEER_RIGHT | config.masks.brake, 0);
        let out = mapper.map(&dev, &keyboard(0, 0), &config, true);
        assert_eq!(out.steer_right, FULL_SCALE);
        assert_eq!(out.brake, FULL_SCALE);
        assert_eq!(out.throttle, 0);
    }

    #[test]
    fn invert_steer_swaps_the_half_channels() {
        let mut mapper = InRaceControlMapper::new();
        let config = ControlConfig {
            invert_steer: true,
            ..ControlConfig::default()
        };
        let out = mapper.map(&keyboard(BIT_STEER_LEFT, 0), &keyboard(0, 0), &config, true);
        // With invert, the left key drives the right channel.
        assert_eq!(out.steer_left, 0);
        assert_eq!(out.steer_right, FULL_SCALE);
    }

    #[test]
    fn handbrake_bit_and_alt_routing() {
        let mut mapper = InRaceControlMapper::new();
        let config = ControlConfig {
            handbrake_alt: true,
            ..ControlConfig::default()
        };
        let out = mapper.map(
            &keyboard(config.masks.handbrake, 0),
            &keyboard(0, 0),
            &config,
            true,
        );
        assert!(out.handbrake);
        assert!(out.handbrake_alt);
        // The ramp mode forces the handbrake off.
        let ramp = ControlConfig {
            ramp_throttle: true,
            ..ControlConfig::default()
        };
        let out = mapper.map(
            &keyboard(ramp.masks.handbrake, 0),
            &keyboard(0, 0),
            &ramp,
            true,
        );
        assert!(!out.handbrake);
    }

    #[test]
    fn gear_command_is_edge_triggered_and_gated() {
        let mut mapper = InRaceControlMapper::new();
        let config = ControlConfig::default();
        // Gear up on its edge.
        let out = mapper.map(
            &keyboard(0, config.masks.gear_up),
            &keyboard(0, 0),
            &config,
            true,
        );
        assert_eq!(out.gear_command, GearCommand::Up);
        // Down wins a tie (tested second in the shipped code).
        let both = config.masks.gear_up | config.masks.gear_down;
        let out = mapper.map(&keyboard(0, both), &keyboard(0, 0), &config, true);
        assert_eq!(out.gear_command, GearCommand::Down);
        // Held (not edged) does not shift.
        let out = mapper.map(
            &keyboard(config.masks.gear_up, 0),
            &keyboard(0, 0),
            &config,
            true,
        );
        assert_eq!(out.gear_command, GearCommand::None);
        // Suppressed when the gear gate is closed.
        let out = mapper.map(
            &keyboard(0, config.masks.gear_up),
            &keyboard(0, 0),
            &config,
            false,
        );
        assert_eq!(out.gear_command, GearCommand::None);
    }

    #[test]
    fn change_view_latch_fires_once_per_press() {
        let mut mapper = InRaceControlMapper::new();
        let config = ControlConfig::default();
        let view_edge = keyboard(0, config.masks.change_view);
        let clear = keyboard(0, 0);
        // Fresh mapper is not yet armed: a change-view edge without a prior clear frame
        // does not fire.
        assert!(
            !mapper
                .map(&view_edge, &clear, &config, true)
                .change_view_fired
        );
        // A clear frame arms it; the next change-view edge fires once.
        assert!(!mapper.map(&clear, &clear, &config, true).change_view_fired);
        assert!(
            mapper
                .map(&view_edge, &clear, &config, true)
                .change_view_fired
        );
        // A second consecutive change-view edge does not re-fire until re-armed.
        assert!(
            !mapper
                .map(&view_edge, &clear, &config, true)
                .change_view_fired
        );
    }

    #[test]
    fn keyboard_is_merged_when_the_primary_device_is_a_joystick() {
        let mut mapper = InRaceControlMapper::new();
        let config = ControlConfig::default();
        // A digital joystick with no bits, plus a keyboard holding throttle.
        let stick = DeviceSnapshot {
            kind: Some(DeviceType::DigitalJoystick),
            ..DeviceSnapshot::default()
        };
        let kb = keyboard(config.masks.throttle, 0);
        let out = mapper.map(&stick, &kb, &config, true);
        assert_eq!(out.throttle, FULL_SCALE);
    }

    #[test]
    fn analog_axes_split_clamp_and_quantize() {
        let mut mapper = InRaceControlMapper::new();
        let config = ControlConfig {
            analog_steer: true,
            analog_throttle: true,
            analog_brake: true,
            ..ControlConfig::default()
        };
        // X < 0 -> steer left (magnitude clamped to 0xffbe then quantized).
        let out = mapper.map(
            &analog(-0x4321, -0x2000, 0),
            &analog(0, 0, 0),
            &config,
            true,
        );
        assert!(out.steer_analog);
        assert_eq!(out.steer_left, clamp_demand(0x4321));
        assert_eq!(out.steer_right, 0);
        // Y < 0 -> throttle, Y > 0 -> brake.
        assert!(out.throttle_analog && out.throttle == clamp_demand(0x2000));
        assert_eq!(out.brake, 0);

        let out = mapper.map(
            &analog(0x1_0000, 0x3000, 0),
            &analog(0, 0, 0),
            &config,
            true,
        );
        // X > 0 -> steer right, clamped to the 0xffbe ceiling then quantized.
        assert_eq!(out.steer_right, clamp_demand(0x1_0000));
        assert_eq!(out.steer_right, FULL_SCALE & DEMAND_QUANTIZE_MASK);
        assert_eq!(out.brake, clamp_demand(0x3000));
    }

    #[test]
    fn poll_layer_prescale_zeroes_deadzone_and_signs_outside() {
        // DeviceSnapshot axes are filled by the poll layer's apply_axis_deadzone.
        let center = AXIS_CENTER as u32;
        assert_eq!(apply_axis_deadzone(center, STEER_DEADZONE), 0);
        assert_eq!(
            apply_axis_deadzone(center + STEER_DEADZONE as u32, STEER_DEADZONE),
            0
        );
        // Just outside -> small nonzero of the sign of the offset.
        assert!(apply_axis_deadzone(center + STEER_DEADZONE as u32 + 1, STEER_DEADZONE) > 0);
        assert!(apply_axis_deadzone(center - STEER_DEADZONE as u32 - 1, STEER_DEADZONE) < 0);
        // Full deflection on the pedal axis is near full scale, of each sign.
        assert!(apply_axis_deadzone(0xffff, PEDAL_DEADZONE) > 0);
        assert!(apply_axis_deadzone(0, PEDAL_DEADZONE) < 0);
    }

    #[test]
    fn default_keyboard_bindings_match_the_recovered_vk_codes() {
        // P1 arrows map to the steer/throttle/brake bits.
        let down = |vk: u8| vk == 0x25 || vk == 0x26; // LEFT + UP held
        let mask = KeyBindings::PLAYER1.held_mask(down);
        assert_eq!(mask & BIT_STEER_LEFT, BIT_STEER_LEFT);
        assert_eq!(mask & (1 << 2), 1 << 2); // throttle bit
        assert_eq!(mask & BIT_STEER_RIGHT, 0);
        // P2 uses A/S/R/D.
        let down = |vk: u8| vk == 0x41; // 'A' = steer left
        assert_eq!(
            KeyBindings::PLAYER2.held_mask(down) & BIT_STEER_LEFT,
            BIT_STEER_LEFT
        );
        // ESC is bound to the pause bit (10) for both players.
        assert_eq!(KeyBindings::PLAYER1.codes[10], Some(0x1b));
        assert_eq!(KeyBindings::PLAYER2.codes[10], Some(0x1b));
    }

    #[test]
    fn ramp_throttle_pulses_then_decays_to_zero() {
        let mut mapper = InRaceControlMapper::new();
        let config = ControlConfig {
            ramp_throttle: true,
            ..ControlConfig::default()
        };
        // Engaging throttle seeds the ramp; the first frame is a nonzero pulse.
        let first = mapper
            .map(
                &keyboard(config.masks.throttle, 0),
                &keyboard(0, 0),
                &config,
                true,
            )
            .throttle;
        assert!(first > 0);
        // Held continuously, the counter decays below 0x60 and the pulse ends.
        let mut last = first;
        for _ in 0..8 {
            last = mapper
                .map(
                    &keyboard(config.masks.throttle, 0),
                    &keyboard(0, 0),
                    &config,
                    true,
                )
                .throttle;
        }
        assert_eq!(last, 0);
    }
}

#[cfg(test)]
mod record_binding_tests {
    use super::*;

    #[test]
    fn the_record_bindings_install_over_the_defaults() {
        let mut record = vec![0u8; 0x200];
        let base = crate::settings_store::PLAYER_STRIDE;
        // Player 2: binding 0 maps mask 0x40 (change view) to 'V'; binding 1
        // has a two-bit mask and is ignored; a zero mask is ignored too.
        record[base + 0x64..base + 0x66].copy_from_slice(&0x40u16.to_le_bytes());
        record[base + 0x78..base + 0x7c].copy_from_slice(&0x56u32.to_le_bytes());
        record[base + 0x66..base + 0x68].copy_from_slice(&3u16.to_le_bytes());
        record[base + 0x7c..base + 0x80].copy_from_slice(&0x57u32.to_le_bytes());
        let bindings = KeyBindings::PLAYER2.with_record(&record, 1);
        assert_eq!(bindings.codes[6], Some(0x56));
        assert_eq!(bindings.codes[0], KeyBindings::PLAYER2.codes[0]);
        assert_eq!(
            KeyBindings::PLAYER1.with_record(&record, 0),
            KeyBindings::PLAYER1
        );
        let mut b = KeyBindings::PLAYER1;
        b.install(0x8000, 0x41);
        assert_eq!(b.codes[15], Some(0x41));
    }
}
