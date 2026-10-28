//! The front end's keyboard input: the held key `FUN_0049d1c0`, its repeat
//! `FUN_004c2970` (into `DAT_00d4cd70` each stepper frame) and the menu bits
//! `FUN_004c25f0` built from it.
//!
//! `FUN_0049d1c0` scans `GetAsyncKeyState` from VK 5 to 0xfe, skipping Shift
//! (0x10), and returns the first held key (the lowest code), or none.
//! `FUN_004c2970` reports that key on its first frame, again once more than
//! 500 ms have passed, then whenever more than 130 ms have passed since the
//! last report (`FUN_004cf750` milliseconds); a different key or no key
//! starts over. `FUN_004c25f0` turns the reported key into one menu bit:
//! Escape back (`0x20`), Up `4`, Down `8`, Left `1`, Right `2`, Return confirm
//! (`0x10`), ORed onto the pad bits. A description of disassembly, not
//! original source. Evidence: `rea.official-eu.frontend-keys`.

/// `FUN_0049d1c0`: the lowest held key in `5..0xff` other than Shift.
pub fn held_key(is_down: impl Fn(u8) -> bool) -> Option<u8> {
    (5..0xff).filter(|&vk| vk != 0x10).find(|&vk| is_down(vk))
}

/// `FUN_004c2970`'s state: the key last seen (`DAT_00d4cdf4`), whether it
/// has been reported (`DAT_00d4ce0c`) and is repeating (`DAT_00d4ce08`), and
/// the time of the last report (`DAT_00d4ce04`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyRepeat {
    key: Option<u8>,
    pressed: bool,
    repeating: bool,
    reported_ms: u32,
    now_ms: u32,
}

impl KeyRepeat {
    /// The key to report this frame (0 for none), given the held key.
    pub fn poll(&mut self, held: Option<u8>, now_ms: u32) -> u8 {
        let Some(key) = held else {
            self.pressed = false;
            self.repeating = false;
            self.reported_ms = self.now_ms;
            return 0;
        };
        if Some(key) != self.key {
            self.pressed = false;
            self.repeating = false;
            self.key = Some(key);
        }
        self.now_ms = now_ms;
        let since = now_ms.wrapping_sub(self.reported_ms);
        if self.repeating {
            if since > 130 {
                self.reported_ms = now_ms;
                return key;
            }
            return 0;
        }
        if !self.pressed {
            self.pressed = true;
            self.reported_ms = now_ms;
            return key;
        }
        if since > 500 {
            self.reported_ms = now_ms;
            self.repeating = true;
            return key;
        }
        0
    }
}

/// `FUN_004c25f0`: the menu bit of the reported key, ORed onto `pad`.
pub const fn menu_bits(key: u8, pad: u32) -> u32 {
    pad | match key {
        0x1b => 0x20,
        0x26 => 4,
        0x28 => 8,
        0x25 => 1,
        0x27 => 2,
        0x0d => 0x10,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lowest_held_key_wins_and_shift_is_skipped() {
        assert_eq!(held_key(|vk| vk == 0x10), None);
        assert_eq!(held_key(|vk| vk == 0x25 || vk == 0x0d), Some(0x0d));
        assert_eq!(held_key(|vk| vk == 4), None, "below 5");
        assert_eq!(held_key(|_| false), None);
    }

    #[test]
    fn a_held_key_repeats_after_half_a_second_then_every_130_ms() {
        let mut repeat = KeyRepeat::default();
        assert_eq!(repeat.poll(Some(b'A'), 1000), b'A');
        assert_eq!(repeat.poll(Some(b'A'), 1500), 0, "not past 500 ms");
        assert_eq!(repeat.poll(Some(b'A'), 1501), b'A');
        assert_eq!(repeat.poll(Some(b'A'), 1631), 0);
        assert_eq!(repeat.poll(Some(b'A'), 1632), b'A');
        // A different key starts over.
        assert_eq!(repeat.poll(Some(b'B'), 1640), b'B');
        assert_eq!(repeat.poll(Some(b'B'), 1700), 0);
        // Released, then pressed again: reported at once.
        assert_eq!(repeat.poll(None, 1710), 0);
        assert_eq!(repeat.poll(Some(b'B'), 1720), b'B');
    }

    #[test]
    fn the_menu_bits() {
        assert_eq!(menu_bits(0x1b, 0), 0x20);
        assert_eq!(menu_bits(0x26, 0), 4);
        assert_eq!(menu_bits(0x28, 0), 8);
        assert_eq!(menu_bits(0x25, 0), 1);
        assert_eq!(menu_bits(0x27, 0), 2);
        assert_eq!(menu_bits(0x0d, 0x08), 0x18);
        assert_eq!(menu_bits(b'A', 0), 0);
    }
}
