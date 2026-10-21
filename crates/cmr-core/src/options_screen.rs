//! Options / settings screen field encoding (reconstruction item F02).
//!
//! # Scope
//!
//! This models the *encoding* of the persisted settings header the options
//! screens edit - the byte offsets, bit fields, value masks, and ranges read by
//! the init functions (`FUN_004ca520`/`600`/`6f0`, audio `FUN_004cac90`) and
//! written by their commit functions - as pure get/set helpers that preserve
//! unknown bits. It does not model the menu engine itself (`FUN_004c20f0` /
//! `FUN_004aee60`, a widget state machine over screen-descriptor objects, which
//! is frontend/display glue) nor the feature *labels* of individual flag bits,
//! which are unlabeled game content.
//!
//! All offsets are byte offsets into the `0x1e20` config blob (see
//! [`crate::settings_store`]); the blob is validated to full length before these
//! helpers run, so they operate on `u8`/`u16`/`u32` field values rather than
//! re-reading the slice. Setters mask only their target bits so that unknown or
//! reserved bits round-trip unchanged.
//!
//! See evidence `rea.official-eu.options-screen`.

/// Byte offset of the global flags byte (bit 0 = save-disable; see
/// [`crate::settings_store`]).
pub const FLAGS_OFFSET: usize = 0x08;

/// Byte offset of the first audio volume channel.
pub const VOLUME_BASE_OFFSET: usize = 0x0c;
/// Number of audio volume channels (`0x0c..=0x0f`).
pub const VOLUME_COUNT: usize = 4;
/// Mask of the 7-bit volume level within a volume byte.
pub const VOLUME_LEVEL_MASK: u8 = 0x7f;
/// Maximum volume level.
pub const VOLUME_MAX: u8 = 127;

/// Byte offset of the display-flags byte.
pub const DISPLAY_FLAGS_OFFSET: usize = 0x10;

/// Byte offset of the first detail byte (`0x11..=0x18`, eight full bytes).
pub const DETAIL_BASE_OFFSET: usize = 0x11;
/// Number of detail bytes.
pub const DETAIL_COUNT: usize = 8;

/// Byte offset of the difficulty/mode selector dword.
pub const DIFFICULTY_OFFSET: usize = 0x1c;

/// Byte offset of the 3-bit option enum (`& 7`, with `4` folded to `0`).
pub const OPTION_ENUM_OFFSET: usize = 0x20;

/// Byte offset of the first gameplay-assist flag byte.
pub const ASSIST_BASE_OFFSET: usize = 0x2b;
/// Number of gameplay-assist flag bytes (`0x2b..=0x30`).
pub const ASSIST_COUNT: usize = 6;
/// Live simulation-settings offset the assist bytes map onto
/// (`DAT_00d3ca80 + 0x23..0x28`, via `FUN_004b8cb0`).
pub const ASSIST_LIVE_BASE_OFFSET: usize = 0x23;

// --- Volume channels (0x0c..0x0f) ---------------------------------------------

/// The 7-bit volume level stored in a volume byte (`& 0x7f`, `0..=127`).
pub fn volume_level(raw: u8) -> u8 {
    raw & VOLUME_LEVEL_MASK
}

/// Set the 7-bit volume level, preserving the high flag bit (channel 0's bit 7
/// is the master enable/mute flag).
pub fn with_volume_level(raw: u8, level: u8) -> u8 {
    (raw & !VOLUME_LEVEL_MASK) | (level & VOLUME_LEVEL_MASK)
}

/// The high flag bit of a volume byte (bit 7 of channel 0 = master flag).
pub fn volume_flag(raw: u8) -> bool {
    raw & 0x80 != 0
}

// --- Display flags byte (0x10) ------------------------------------------------

/// The 2-bit display-mode enum packed in display bits 3-4 (`(byte >> 3) & 3`).
pub fn display_mode(display_flags: u8) -> u8 {
    (display_flags >> 3) & 0x03
}

/// Set the 2-bit display-mode enum (bits 3-4), preserving all other bits.
pub fn with_display_mode(display_flags: u8, mode: u8) -> u8 {
    (display_flags & !0x18) | ((mode & 0x03) << 3)
}

/// Read a single display flag bit by mask.
pub fn display_bit(display_flags: u8, mask: u8) -> bool {
    display_flags & mask != 0
}

/// Set or clear a single display flag bit by mask, preserving the others.
pub fn with_display_bit(display_flags: u8, mask: u8, on: bool) -> u8 {
    if on {
        display_flags | mask
    } else {
        display_flags & !mask
    }
}

// --- Option enum (0x20) -------------------------------------------------------

/// The 3-bit option enum at `0x20`: `byte & 7`, with the out-of-range value `4`
/// folded to `0`, reproducing the `FUN_004c20f0` masking.
pub fn option_enum(raw: u8) -> u8 {
    let value = raw & 0x07;
    if value == 4 { 0 } else { value }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_level_masks_to_seven_bits_and_round_trips() {
        assert_eq!(volume_level(0xff), VOLUME_MAX);
        assert_eq!(volume_level(0x40), 0x40);
        // Setting the level preserves the master flag bit.
        let raw = 0x80 | 0x20; // flag set, level 0x20
        assert!(volume_flag(raw));
        let updated = with_volume_level(raw, 100);
        assert_eq!(volume_level(updated), 100);
        assert!(volume_flag(updated)); // high bit preserved
        // Over-range level is masked.
        assert_eq!(volume_level(with_volume_level(0, 200)), 200 & 0x7f);
    }

    #[test]
    fn display_mode_is_bits_three_and_four() {
        // mode 2 -> bits 3-4 = 0b10000.
        let flags = with_display_mode(0, 2);
        assert_eq!(flags, 0x10);
        assert_eq!(display_mode(flags), 2);
        // Setting the mode preserves neighbouring bits 0,2,5.
        let base = 0x01 | 0x04 | 0x20;
        let flags = with_display_mode(base, 3);
        assert_eq!(display_mode(flags), 3);
        assert!(display_bit(flags, 0x01));
        assert!(display_bit(flags, 0x04));
        assert!(display_bit(flags, 0x20));
    }

    #[test]
    fn display_bit_set_clear_preserves_others() {
        let flags = with_display_bit(0x00, 0x40, true);
        assert!(display_bit(flags, 0x40));
        let flags = with_display_bit(0xff, 0x40, false);
        assert!(!display_bit(flags, 0x40));
        assert_eq!(flags, 0xbf); // 0xff with bit 0x40 cleared
    }

    #[test]
    fn option_enum_folds_four_to_zero() {
        assert_eq!(option_enum(0), 0);
        assert_eq!(option_enum(3), 3);
        assert_eq!(option_enum(4), 0); // out-of-range folded
        assert_eq!(option_enum(0xf8 | 5), 5); // upper bits ignored
    }

    #[test]
    fn header_offsets_are_non_overlapping_and_ordered() {
        // The four volume bytes end exactly where the display byte begins.
        assert_eq!(VOLUME_BASE_OFFSET + VOLUME_COUNT, DISPLAY_FLAGS_OFFSET);
        // The eight detail bytes 0x11..=0x18 sit after the display byte.
        assert_eq!(DETAIL_BASE_OFFSET, DISPLAY_FLAGS_OFFSET + 1);
        assert_eq!(DETAIL_BASE_OFFSET + DETAIL_COUNT - 1, 0x18);
        // Difficulty is a dword; the option enum sits one byte past its end.
        assert_eq!(OPTION_ENUM_OFFSET, DIFFICULTY_OFFSET + 4);
        // Assist bytes form a contiguous run ending at 0x30.
        assert_eq!(ASSIST_BASE_OFFSET + ASSIST_COUNT - 1, 0x30);
    }
}
