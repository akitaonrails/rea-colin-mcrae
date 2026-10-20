//! Platform-neutral reconstruction of the first frontend's input rules.
//!
//! Host backends provide snapshots. This module deliberately contains no
//! Win32, DirectInput, or WinMM calls.

pub const ACTION_LEFT: u32 = 0x0001;
pub const ACTION_RIGHT: u32 = 0x0002;
pub const ACTION_UP: u32 = 0x0004;
pub const ACTION_DOWN: u32 = 0x0008;
pub const ACTION_FIRST_BUTTON: u32 = 0x0010;
pub const ACTION_ANY_BUTTON: u32 = 0x1_0000;

pub const KEYBOARD_MAPPING_COUNT: usize = 18;
pub const JOYSTICK_BUTTON_COUNT: u32 = 12;
pub const ANALOG_X_DEAD_ZONE: i32 = 2_000;
pub const ANALOG_Y_DEAD_ZONE: i32 = 2_300;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JoystickSnapshot {
    pub x: u16,
    pub y: u16,
    pub buttons: u32,
}

impl Default for JoystickSnapshot {
    fn default() -> Self {
        Self {
            x: 32_768,
            y: 32_768,
            buttons: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DeviceSample {
    pub current: u32,
    pub pressed: u32,
    pub analog_x: i32,
    pub analog_y: i32,
}

impl DeviceSample {
    pub fn sample_keyboard(
        &mut self,
        mappings: &[u8; KEYBOARD_MAPPING_COUNT],
        keys_down: &[bool; 256],
    ) {
        self.finish(keyboard_mask(mappings, keys_down));
    }

    /// `None` models a failed `joyGetPosEx`; digital mode clears current input.
    pub fn sample_digital_joystick(&mut self, snapshot: Option<JoystickSnapshot>) {
        self.finish(snapshot.map_or(0, digital_joystick_mask));
    }

    /// A failed analog sample preserves the previous mask and axis values.
    pub fn sample_analog_joystick(&mut self, snapshot: Option<JoystickSnapshot>) {
        let Some(snapshot) = snapshot else {
            self.finish(self.current);
            return;
        };

        self.analog_x = scale_analog_axis(snapshot.x, ANALOG_X_DEAD_ZONE);
        self.analog_y = scale_analog_axis(snapshot.y, ANALOG_Y_DEAD_ZONE);

        let mut current = joystick_button_mask(snapshot.buttons);
        if self.analog_x < -32_768 {
            current |= ACTION_LEFT;
        } else if self.analog_x > 32_768 {
            current |= ACTION_RIGHT;
        }
        if self.analog_y < -32_768 {
            current |= ACTION_UP;
        } else if self.analog_y > 32_768 {
            current |= ACTION_DOWN;
        }
        self.finish(current);
    }

    fn finish(&mut self, current: u32) {
        let previous = self.current;
        self.current = current;
        self.pressed = rising_edges(previous, current);
    }
}

pub const fn rising_edges(previous: u32, current: u32) -> u32 {
    (current ^ previous) & current
}

pub fn keyboard_mask(mappings: &[u8; KEYBOARD_MAPPING_COUNT], keys_down: &[bool; 256]) -> u32 {
    let mut current = 0;

    // The original uses an if/else-if pair: left wins when both are held.
    if keys_down[usize::from(mappings[0])] {
        current |= ACTION_LEFT;
    } else if keys_down[usize::from(mappings[1])] {
        current |= ACTION_RIGHT;
    }

    for action_index in 2..16 {
        if keys_down[usize::from(mappings[action_index])] {
            current |= 1 << action_index;
        }
    }

    // Two additional mapping bytes alias the final two action bits.
    if keys_down[usize::from(mappings[16])] {
        current |= 0x4000;
    }
    if keys_down[usize::from(mappings[17])] {
        current |= 0x8000;
    }

    with_any_button(current)
}

pub fn digital_joystick_mask(snapshot: JoystickSnapshot) -> u32 {
    let mut current = joystick_button_mask(snapshot.buttons);
    if snapshot.x < 10_000 {
        current |= ACTION_LEFT;
    } else if snapshot.x > 55_536 {
        current |= ACTION_RIGHT;
    }
    if snapshot.y < 10_000 {
        current |= ACTION_UP;
    } else if snapshot.y > 55_536 {
        current |= ACTION_DOWN;
    }
    current
}

fn joystick_button_mask(buttons: u32) -> u32 {
    let mut current = 0;
    for button in 0..JOYSTICK_BUTTON_COUNT {
        if buttons & (1 << button) != 0 {
            current |= ACTION_FIRST_BUTTON << button;
        }
    }
    with_any_button(current)
}

fn with_any_button(mut current: u32) -> u32 {
    if current & !0x0f != 0 {
        current |= ACTION_ANY_BUTTON;
    }
    current
}

fn scale_analog_axis(raw: u16, dead_zone: i32) -> i32 {
    let centered = i32::from(raw) - 32_768;
    if centered.abs() <= dead_zone {
        return 0;
    }

    let adjusted = if centered > 0 {
        centered - dead_zone
    } else {
        centered + dead_zone
    };
    let denominator = 32_768 - dead_zone;
    ((i64::from(adjusted) * 32_768 / i64::from(denominator)) * 2) as i32
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawKey {
    pub virtual_key: u8,
    pub shift: bool,
}

pub fn lowest_raw_key(keys_down: &[bool; 256]) -> Option<RawKey> {
    (5u8..=254)
        .find(|&virtual_key| virtual_key != 0x10 && keys_down[usize::from(virtual_key)])
        .map(|virtual_key| RawKey {
            virtual_key,
            shift: keys_down[0x10],
        })
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct KeyRepeat {
    key: Option<u8>,
    started: bool,
    repeating: bool,
    timestamp_ms: u32,
}

impl KeyRepeat {
    pub fn update(&mut self, key: Option<RawKey>, now_ms: u32) -> Option<RawKey> {
        let Some(key) = key else {
            self.key = None;
            self.started = false;
            self.repeating = false;
            return None;
        };

        if self.key != Some(key.virtual_key) {
            self.key = Some(key.virtual_key);
            self.started = false;
            self.repeating = false;
        }

        if !self.started {
            self.started = true;
            self.timestamp_ms = now_ms;
            return Some(key);
        }

        let elapsed = now_ms.wrapping_sub(self.timestamp_ms);
        if !self.repeating {
            if elapsed > 500 {
                self.repeating = true;
                self.timestamp_ms = now_ms;
                return Some(key);
            }
        } else if elapsed > 130 {
            self.timestamp_ms = now_ms;
            return Some(key);
        }
        None
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendDeviceKind {
    Keyboard,
    Other,
}

pub fn frontend_actions(
    selected_kind: FrontendDeviceKind,
    device_pressed: u32,
    repeated_key: Option<RawKey>,
) -> u32 {
    let device_actions = match selected_kind {
        FrontendDeviceKind::Keyboard => 0,
        FrontendDeviceKind::Other => device_pressed,
    };
    device_actions | repeated_key.map_or(0, |key| frontend_key_action(key.virtual_key))
}

pub const fn frontend_key_action(virtual_key: u8) -> u32 {
    match virtual_key {
        0x25 => ACTION_LEFT,
        0x27 => ACTION_RIGHT,
        0x26 => ACTION_UP,
        0x28 => ACTION_DOWN,
        0x0d => 0x10,
        0x1b => 0x20,
        _ => 0,
    }
}

/// Runs a sampling transaction only while the application is focused.
pub fn sample_when_focused<T>(focused: bool, sample: impl FnOnce() -> T) -> Option<T> {
    focused.then(sample)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mappings() -> [u8; KEYBOARD_MAPPING_COUNT] {
        [
            0x25, 0x27, 0x26, 0x28, 0x0d, 0x1b, 0x21, 0x2e, 0x60, 0x58, 0x70, 0x71, 0x72, 0x73,
            0x74, 0x75, 0x76, 0x77,
        ]
    }

    #[test]
    fn rising_edges_only_report_new_bits() {
        assert_eq!(rising_edges(0b0110, 0b1010), 0b1000);
    }

    #[test]
    fn keyboard_left_precedes_right_and_sets_button_summary() {
        let mut keys = [false; 256];
        keys[0x25] = true;
        keys[0x27] = true;
        keys[0x0d] = true;
        keys[0x76] = true;

        assert_eq!(
            keyboard_mask(&mappings(), &keys),
            ACTION_LEFT | 0x10 | 0x4000 | ACTION_ANY_BUTTON
        );
    }

    #[test]
    fn keyboard_extra_mappings_alias_the_last_two_actions() {
        let mut keys = [false; 256];
        keys[0x74] = true;
        keys[0x75] = true;
        keys[0x76] = true;
        keys[0x77] = true;

        assert_eq!(
            keyboard_mask(&mappings(), &keys),
            0x4000 | 0x8000 | ACTION_ANY_BUTTON
        );
    }

    #[test]
    fn digital_joystick_thresholds_are_strict() {
        let neutral_low = JoystickSnapshot {
            x: 10_000,
            y: 10_000,
            buttons: 0,
        };
        let neutral_high = JoystickSnapshot {
            x: 55_536,
            y: 55_536,
            buttons: 0,
        };
        assert_eq!(digital_joystick_mask(neutral_low), 0);
        assert_eq!(digital_joystick_mask(neutral_high), 0);
        assert_eq!(
            digital_joystick_mask(JoystickSnapshot {
                x: 9_999,
                y: 55_537,
                buttons: 0,
            }),
            ACTION_LEFT | ACTION_DOWN
        );
    }

    #[test]
    fn joystick_maps_twelve_buttons_and_ignores_higher_bits() {
        let snapshot = JoystickSnapshot {
            x: 32_768,
            y: 32_768,
            buttons: 1 | (1 << 11) | (1 << 12),
        };
        assert_eq!(
            digital_joystick_mask(snapshot),
            0x10 | 0x8000 | ACTION_ANY_BUTTON
        );
    }

    #[test]
    fn digital_failure_clears_current_state() {
        let mut state = DeviceSample {
            current: ACTION_LEFT | 0x10,
            pressed: u32::MAX,
            analog_x: 123,
            analog_y: 456,
        };
        state.sample_digital_joystick(None);
        assert_eq!(state.current, 0);
        assert_eq!(state.pressed, 0);
        assert_eq!((state.analog_x, state.analog_y), (123, 456));
    }

    #[test]
    fn analog_dead_zones_and_direction_thresholds_are_strict() {
        assert_eq!(scale_analog_axis(32_768 - 2_000, ANALOG_X_DEAD_ZONE), 0);
        assert_eq!(scale_analog_axis(32_768 + 2_000, ANALOG_X_DEAD_ZONE), 0);
        assert_eq!(scale_analog_axis(32_768 - 2_300, ANALOG_Y_DEAD_ZONE), 0);
        assert_eq!(scale_analog_axis(32_768 + 2_300, ANALOG_Y_DEAD_ZONE), 0);

        let exact_x = (0..=u16::MAX)
            .find(|&raw| scale_analog_axis(raw, ANALOG_X_DEAD_ZONE) == -32_768)
            .unwrap();
        let exact_y = (0..=u16::MAX)
            .find(|&raw| scale_analog_axis(raw, ANALOG_Y_DEAD_ZONE) == 32_768)
            .unwrap();
        let mut state = DeviceSample::default();
        state.sample_analog_joystick(Some(JoystickSnapshot {
            x: exact_x,
            y: exact_y,
            buttons: 0,
        }));
        assert_eq!(state.current & 0x0f, 0);

        let left = (0..=u16::MAX)
            .find(|&raw| scale_analog_axis(raw, ANALOG_X_DEAD_ZONE) < -32_768)
            .unwrap();
        let down = (0..=u16::MAX)
            .find(|&raw| scale_analog_axis(raw, ANALOG_Y_DEAD_ZONE) > 32_768)
            .unwrap();
        state.sample_analog_joystick(Some(JoystickSnapshot {
            x: left,
            y: down,
            buttons: 0,
        }));
        assert_eq!(state.current, ACTION_LEFT | ACTION_DOWN);
    }

    #[test]
    fn analog_failure_preserves_current_axes_and_clears_edges() {
        let mut state = DeviceSample {
            current: ACTION_RIGHT | 0x20 | ACTION_ANY_BUTTON,
            pressed: u32::MAX,
            analog_x: 40_000,
            analog_y: -12_000,
        };
        state.sample_analog_joystick(None);
        assert_eq!(state.current, ACTION_RIGHT | 0x20 | ACTION_ANY_BUTTON);
        assert_eq!(state.pressed, 0);
        assert_eq!((state.analog_x, state.analog_y), (40_000, -12_000));
    }

    #[test]
    fn raw_key_scan_uses_lowest_non_shift_key_and_reports_shift() {
        let mut keys = [false; 256];
        keys[0x10] = true;
        keys[0x41] = true;
        keys[0x25] = true;
        assert_eq!(
            lowest_raw_key(&keys),
            Some(RawKey {
                virtual_key: 0x25,
                shift: true,
            })
        );

        keys[0x25] = false;
        keys[0x41] = false;
        assert_eq!(lowest_raw_key(&keys), None);
    }

    #[test]
    fn key_repeat_uses_exact_recovered_boundaries() {
        let key = RawKey {
            virtual_key: 0x25,
            shift: false,
        };
        let mut repeat = KeyRepeat::default();
        assert_eq!(repeat.update(Some(key), 1_000), Some(key));
        assert_eq!(repeat.update(Some(key), 1_500), None);
        assert_eq!(repeat.update(Some(key), 1_501), Some(key));
        assert_eq!(repeat.update(Some(key), 1_631), None);
        assert_eq!(repeat.update(Some(key), 1_632), Some(key));

        let other = RawKey {
            virtual_key: 0x27,
            shift: true,
        };
        assert_eq!(repeat.update(Some(other), 1_633), Some(other));
        assert_eq!(repeat.update(None, 2_000), None);
        assert_eq!(repeat.update(Some(key), 2_001), Some(key));
    }

    #[test]
    fn key_repeat_wraps_time_and_tracks_shift_outside_key_identity() {
        let plain = RawKey {
            virtual_key: 0x25,
            shift: false,
        };
        let shifted = RawKey {
            shift: true,
            ..plain
        };
        let mut repeat = KeyRepeat::default();
        assert_eq!(repeat.update(Some(plain), u32::MAX - 250), Some(plain));
        assert_eq!(repeat.update(Some(shifted), 249), None);
        assert_eq!(repeat.update(Some(shifted), 250), Some(shifted));
        assert_eq!(repeat.update(Some(plain), 380), None);
        assert_eq!(repeat.update(Some(plain), 381), Some(plain));
    }

    #[test]
    fn frontend_combines_raw_navigation_only_for_keyboard_device() {
        let enter = Some(RawKey {
            virtual_key: 0x0d,
            shift: false,
        });
        assert_eq!(
            frontend_actions(FrontendDeviceKind::Keyboard, ACTION_LEFT, enter),
            0x10
        );
        assert_eq!(
            frontend_actions(FrontendDeviceKind::Other, ACTION_LEFT, enter),
            ACTION_LEFT | 0x10
        );
        assert_eq!(frontend_key_action(0x41), 0);

        for (virtual_key, action) in [
            (0x25, ACTION_LEFT),
            (0x27, ACTION_RIGHT),
            (0x26, ACTION_UP),
            (0x28, ACTION_DOWN),
            (0x0d, 0x10),
            (0x1b, 0x20),
            (0x41, 0),
        ] {
            assert_eq!(frontend_key_action(virtual_key), action);
        }
    }

    #[test]
    fn joystick_default_is_neutral() {
        assert_eq!(digital_joystick_mask(JoystickSnapshot::default()), 0);
    }

    #[test]
    fn inactive_frame_does_not_sample() {
        let mut calls = 0;
        let result = sample_when_focused(false, || {
            calls += 1;
            7
        });
        assert_eq!(result, None);
        assert_eq!(calls, 0);

        assert_eq!(
            sample_when_focused(true, || {
                calls += 1;
                7
            }),
            Some(7)
        );
        assert_eq!(calls, 1);
    }
}
