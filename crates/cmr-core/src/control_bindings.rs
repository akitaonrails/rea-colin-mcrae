//! Control-binding record layout and blob->live translation (item F02).
//!
//! # Scope
//!
//! This models the *structure* of the per-player control configuration the
//! controls screens edit and the fixup `FUN_004ba060` translates from the
//! persisted config blob into the live input-slot array: the binding-table
//! layout (key masks + assigned physical codes), the device-select fields, the
//! single-bit key mask -> live-slot index mapping (`FUN_0049d3f0`), and the live
//! action bitfield shape. It does not vendor the built-in default key tables
//! (`FUN_0049d6e0`/`FUN_004b9f70`, Windows `VK_*` codes assigned to actions) nor
//! the device backends (keyboard via `GetAsyncKeyState`, WinMM `joyGetPosEx`,
//! DirectInput enumeration only) - those are input-backend glue.
//!
//! Blob offsets are per-player-record-relative (add [`player_record_offset`]);
//! the per-player record stride is [`crate::settings_store::PLAYER_STRIDE`].
//!
//! See evidence `rea.official-eu.control-bindings`.

use crate::settings_store::PLAYER_STRIDE;

/// Number of key/button bindings per player.
pub const BINDING_COUNT: usize = 8;

/// Record-relative offset of the first binding key-mask (`u16`, stride 2).
pub const KEYMASK_BASE_OFFSET: usize = 0x64;
/// Stride between binding key-masks.
pub const KEYMASK_STRIDE: usize = 2;

/// Record-relative offset of the first assigned physical code (`u32`, stride 4).
pub const PHYSCODE_BASE_OFFSET: usize = 0x78;
/// Stride between assigned physical codes.
pub const PHYSCODE_STRIDE: usize = 4;

/// Record-relative offset of the control-toggle bits byte (bits 0-3).
pub const CONTROL_BITS_OFFSET: usize = 0x44;
/// Record-relative offset of the selected device index (`u16`).
pub const DEVICE_INDEX_OFFSET: usize = 0x46;
/// Record-relative offset of the device-type selector (`i32`).
pub const DEVICE_TYPE_OFFSET: usize = 0x48;

/// Byte offset of a player's configuration record within the config blob.
/// (The same `PLAYER_STRIDE` the settings loader and `car_resources` use.)
pub fn player_record_offset(player: usize) -> usize {
    player * PLAYER_STRIDE
}

/// Record-relative offset of binding `n`'s key-mask (`n < BINDING_COUNT`).
pub fn keymask_offset(n: usize) -> usize {
    KEYMASK_BASE_OFFSET + n * KEYMASK_STRIDE
}

/// Record-relative offset of binding `n`'s assigned physical code.
pub fn physcode_offset(n: usize) -> usize {
    PHYSCODE_BASE_OFFSET + n * PHYSCODE_STRIDE
}

// --- Device-type selector (FUN_004ccbf0) --------------------------------------

/// The device-type selector maps raw blob values `{0, 2, 4}` to dense indices
/// `{0, 1, 2}` (keyboard is a separate live type). Returns `None` for an
/// unrecognized raw value.
pub fn device_type_index(raw: i32) -> Option<u8> {
    match raw {
        0 => Some(0),
        2 => Some(1),
        4 => Some(2),
        _ => None,
    }
}

/// The inverse of [`device_type_index`]: the raw blob value for a dense index.
pub fn device_type_raw(index: u8) -> Option<i32> {
    match index {
        0 => Some(0),
        1 => Some(2),
        2 => Some(4),
        _ => None,
    }
}

// --- Live input-slot array (DAT_00b62b48) -------------------------------------

/// Stride of one live input slot.
pub const LIVE_SLOT_STRIDE: usize = 0x8c;
/// Number of live input slots.
pub const LIVE_SLOT_COUNT: usize = 18;
/// Record-relative offset of the key-code table within a live slot
/// (`DAT_00b62bc0 = DAT_00b62b48 + 0x78`), indexed by key-mask bit position.
pub const LIVE_KEYCODE_BASE_OFFSET: usize = 0x78;
/// Live-slot offset of the current action bitfield.
pub const LIVE_ACTION_OFFSET: usize = 0x04;

/// The live-slot byte offset a `FUN_0049d3f0` binding writes to: the key-code
/// table base plus the key-mask's single-bit position. Returns `None` unless
/// exactly one bit is set (the setter is only ever called with a single mask
/// bit).
pub fn live_keycode_offset(key_mask: u16) -> Option<usize> {
    if key_mask.count_ones() != 1 {
        return None;
    }
    Some(LIVE_KEYCODE_BASE_OFFSET + key_mask.trailing_zeros() as usize)
}

/// Base byte offset of live slot `slot` within the input-slot array.
pub fn live_slot_offset(slot: usize) -> usize {
    slot * LIVE_SLOT_STRIDE
}

// --- Action bitfield (live slot +0x04) ----------------------------------------

/// Mask of the four direction bits (up/down/left/right) in the action bitfield.
pub const ACTION_DIRECTION_MASK: u32 = 0x0000_000f;
/// Mask of the twelve button bits (`0x10..=0x8000`).
pub const ACTION_BUTTON_MASK: u32 = 0x0000_fff0;
/// The "any button pressed" bit.
pub const ACTION_ANY_BUTTON: u32 = 0x0001_0000;

/// Whether any of the four direction bits are set.
pub fn action_has_direction(action: u32) -> bool {
    action & ACTION_DIRECTION_MASK != 0
}

/// Whether any of the twelve button bits are set.
pub fn action_has_button(action: u32) -> bool {
    action & ACTION_BUTTON_MASK != 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings_store::PLAYER_COUNT;

    #[test]
    fn binding_table_is_eight_contiguous_pairs() {
        assert_eq!(keymask_offset(0), 0x64);
        assert_eq!(keymask_offset(BINDING_COUNT - 1), 0x72);
        assert_eq!(physcode_offset(0), 0x78);
        assert_eq!(physcode_offset(BINDING_COUNT - 1), 0x94);
        // Key-masks (u16) end before the physical codes (u32) begin.
        assert!(keymask_offset(BINDING_COUNT - 1) + KEYMASK_STRIDE <= PHYSCODE_BASE_OFFSET);
    }

    #[test]
    fn player_records_use_the_shared_stride() {
        assert_eq!(player_record_offset(0), 0);
        assert_eq!(player_record_offset(1), PLAYER_STRIDE);
        // Both players' records fit the recovered 0x138 span.
        let span = PLAYER_COUNT * PLAYER_STRIDE;
        assert_eq!(span, 0x138);
    }

    #[test]
    fn device_type_enum_round_trips() {
        for (raw, index) in [(0, 0), (2, 1), (4, 2)] {
            assert_eq!(device_type_index(raw), Some(index));
            assert_eq!(device_type_raw(index), Some(raw));
        }
        assert_eq!(device_type_index(1), None);
        assert_eq!(device_type_index(3), None);
        assert_eq!(device_type_raw(3), None);
    }

    #[test]
    fn keymask_maps_to_live_keycode_slot_by_bit_position() {
        assert_eq!(live_keycode_offset(0x0001), Some(0x78));
        assert_eq!(live_keycode_offset(0x0002), Some(0x79));
        assert_eq!(live_keycode_offset(0x8000), Some(0x78 + 15));
        // Only single-bit masks are valid.
        assert_eq!(live_keycode_offset(0x0003), None);
        assert_eq!(live_keycode_offset(0x0000), None);
    }

    #[test]
    fn live_slots_do_not_overlap() {
        assert_eq!(live_slot_offset(0), 0);
        assert_eq!(live_slot_offset(1), 0x8c);
        assert_eq!(live_slot_offset(LIVE_SLOT_COUNT - 1), 17 * 0x8c);
    }

    #[test]
    fn action_bitfield_classifies_directions_and_buttons() {
        assert!(action_has_direction(0x0001));
        assert!(action_has_direction(0x0008));
        assert!(!action_has_direction(0x0010));
        assert!(action_has_button(0x0010));
        assert!(action_has_button(0x8000));
        assert!(!action_has_button(0x0008));
        // Direction, button and any-button masks are disjoint.
        assert_eq!(ACTION_DIRECTION_MASK & ACTION_BUTTON_MASK, 0);
        assert_eq!(
            (ACTION_DIRECTION_MASK | ACTION_BUTTON_MASK) & ACTION_ANY_BUTTON,
            0
        );
    }
}
