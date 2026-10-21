//! Championship content data-model structure (reconstruction item F05).
//!
//! # Scope
//!
//! The actual content tables - country/stage/car names, car codes, BHP/stat
//! figures, difficulty labels, points - are game data and are **not** vendored.
//! This module reconstructs only the *structure* that addresses them: the model
//! dimensions, the save-record <-> live-struct offset mapping, the event-progress
//! element decode, the per-slot display descriptor packing, and the
//! Rally-School / difficulty selector bit layout. Display names are
//! runtime-loaded localized strings (`FUN_0049b010(id)`), not static tables, so
//! only the string-ID base offsets are structural.
//!
//! # Recovered facts
//!
//! - Countries: 9 slots, selected by live `+0x168 & 0xf` (`0..=8`), each a
//!   `.TIM` definition file reached through a pointer sub-table; the per-stage
//!   list/order/surface live in those external files, not the executable.
//! - Cars: 14 slots, selected by a 4-bit nibble (see [`crate::car_resources`]),
//!   with five parallel 14-entry tables (name / asset-dir / code / make-model
//!   pointers, and a numeric stats table of stride `0x0c`).
//! - Difficulty: 3 usable levels (easy/medium/hard) plus a NONE; the Rally-School
//!   selector byte `cfg[0x6c0]` packs a 3-bit task (`>>5`) and a 2-bit difficulty
//!   (`>>3 & 3`), and the persisted mode/difficulty proxy is live `+0x18 >>3 & 3`.
//! - The save record mirrors the live struct: `record_off = live_off - 0x20`
//!   and `payload_off = live_off - 0x2c` (verified against the car nibble, the
//!   country index, and the event-progress array).
//! - Event progress is a `0x10`-stride array (<= 16 events) at record `+0x150`
//!   (live `+0x170`); each element's flag byte at `+0x08` carries bit `0x10`
//!   (started) and a low-nibble completed-stage count.
//!
//! See evidence `rea.official-eu.championship-model`.

use crate::car_resources::CAR_MODEL_COUNT;

/// Number of country/rally slots (`+0x168 & 0xf`, `0..=8`).
pub const COUNTRY_COUNT: usize = 9;

/// Number of car slots (shared with [`crate::car_resources::CAR_MODEL_COUNT`]).
pub const CAR_COUNT: usize = CAR_MODEL_COUNT;

/// Number of usable difficulty levels (easy / medium / hard).
pub const DIFFICULTY_COUNT: usize = 3;

/// Stride of one entry in the numeric car-stats table (`0x508e18`).
pub const CAR_STATS_STRIDE: usize = 0x0c;

// --- Save-record <-> live-struct offset mapping -------------------------------

/// `record_off = live_off - SAVE_RECORD_LIVE_DELTA`.
pub const SAVE_RECORD_LIVE_DELTA: usize = 0x20;

/// `payload_off = live_off - PAYLOAD_LIVE_DELTA` (the payload starts at record
/// `+0x0c`, i.e. live `+0x2c`).
pub const PAYLOAD_LIVE_DELTA: usize = 0x2c;

/// Live-struct offset of the selection car nibble base (`+ player * 0x9c`).
pub const LIVE_CAR_NIBBLE_OFFSET: usize = 0xb6;
/// Live-struct offset of the active/instantiated car nibble base.
pub const LIVE_ACTIVE_CAR_NIBBLE_OFFSET: usize = 0x2fe;
/// Live-struct offset of the country index.
pub const LIVE_COUNTRY_OFFSET: usize = 0x168;
/// Live-struct offset of the event-progress array.
pub const LIVE_PROGRESS_OFFSET: usize = 0x170;
/// Live-struct offset of the mode/difficulty tier byte.
pub const LIVE_MODE_TIER_OFFSET: usize = 0x18;

/// Map a live-struct offset to its save-record offset, or `None` when the live
/// offset is below the record window.
pub fn record_offset(live_offset: usize) -> Option<usize> {
    live_offset.checked_sub(SAVE_RECORD_LIVE_DELTA)
}

/// Map a live-struct offset to its payload offset (record `+0x0c`-relative), or
/// `None` when the live offset is below the payload window.
pub fn payload_offset(live_offset: usize) -> Option<usize> {
    live_offset.checked_sub(PAYLOAD_LIVE_DELTA)
}

// --- Event-progress array -----------------------------------------------------

/// Record offset of the event-progress array.
pub const PROGRESS_RECORD_OFFSET: usize = 0x150;
/// Stride of one event-progress element.
pub const PROGRESS_ELEMENT_STRIDE: usize = 0x10;
/// Maximum number of event-progress elements scanned.
pub const PROGRESS_ELEMENT_MAX: usize = 16;
/// Offset of the state flag within an event-progress element.
pub const PROGRESS_FLAG_OFFSET: usize = 0x08;
/// Flag bit meaning the event has been started/entered.
pub const PROGRESS_FLAG_STARTED: u8 = 0x10;
/// Low-nibble mask of the completed-stage count within the flag byte.
pub const PROGRESS_COMPLETED_MASK: u8 = 0x0f;

/// Whether an event-progress flag marks the event as started.
pub fn event_started(flag: u8) -> bool {
    flag & PROGRESS_FLAG_STARTED != 0
}

/// The completed-stage count encoded in an event-progress flag.
pub fn event_completed_stages(flag: u8) -> u8 {
    flag & PROGRESS_COMPLETED_MASK
}

/// The current/next event cursor: the first started-but-not-completed event,
/// reproducing the scan in `FUN_0044f210`.
pub fn current_event_index(flags: &[u8]) -> Option<usize> {
    flags
        .iter()
        .take(PROGRESS_ELEMENT_MAX)
        .position(|&flag| event_started(flag) && event_completed_stages(flag) == 0)
}

// --- Display descriptor packing (FUN_0044f210) --------------------------------

/// String-ID base for a localized country/location name (`(byte1 & 7) + 0x3c`).
pub const STRING_ID_LOCATION_BASE: u16 = 0x3c;
/// String-ID base for a localized mode label (`(byte1 >> 6) + 0x2f`).
pub const STRING_ID_MODE_BASE: u16 = 0x2f;

/// Pack the descriptor byte combining country, stage index, and mode:
/// `(country & 7) | ((stage & 7) << 3) | (mode << 6)`.
pub fn pack_descriptor(country: u8, stage: u8, mode: u8) -> u8 {
    (country & 7) | ((stage & 7) << 3) | (mode << 6)
}

/// The country field of a packed descriptor byte.
pub fn descriptor_country(byte1: u8) -> u8 {
    byte1 & 7
}

/// The stage index of a packed descriptor byte.
pub fn descriptor_stage(byte1: u8) -> u8 {
    (byte1 >> 3) & 7
}

/// The mode field of a packed descriptor byte.
pub fn descriptor_mode(byte1: u8) -> u8 {
    byte1 >> 6
}

/// The localized string id for a descriptor's country name.
pub fn location_string_id(byte1: u8) -> u16 {
    u16::from(descriptor_country(byte1)) + STRING_ID_LOCATION_BASE
}

/// The localized string id for a descriptor's mode label.
pub fn mode_string_id(byte1: u8) -> u16 {
    u16::from(descriptor_mode(byte1)) + STRING_ID_MODE_BASE
}

// --- Difficulty / Rally-School selectors --------------------------------------

/// The Rally-School task/section index packed in `cfg[0x6c0]` (`>> 5`, `0..=7`).
pub fn rally_school_task(selector: u8) -> u8 {
    selector >> 5
}

/// The Rally-School difficulty index packed in `cfg[0x6c0]` (`>> 3 & 3`).
pub fn rally_school_difficulty(selector: u8) -> u8 {
    (selector >> 3) & 3
}

/// The persisted mode/difficulty tier proxy from live `+0x18` (`>> 3 & 3`), the
/// 2-bit field stored at save-record `+0x6a6`.
pub fn mode_tier(mode_byte: u8) -> u8 {
    (mode_byte >> 3) & 3
}

// --- Car numeric stats entry (structure only) ---------------------------------

/// One parsed entry of the numeric car-stats table (`0x508e18`, stride `0x0c`).
/// The figures are game data; this models only the entry shape.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CarStatsEntry {
    /// Primary figure at `+0x00` (paired with a "BHP" label).
    pub primary: u32,
    /// Secondary figure at `+0x04` (paired with a "MM" label).
    pub secondary: u32,
    /// Class/drivetrain selector at `+0x08` (observed `0..=2`).
    pub class_index: u8,
}

/// Parse one `0x0c`-byte car-stats entry (little-endian), reproducing the field
/// shape read by `FUN_004c54a0`.
pub fn parse_car_stats_entry(entry: &[u8; CAR_STATS_STRIDE]) -> CarStatsEntry {
    CarStatsEntry {
        primary: u32::from_le_bytes([entry[0], entry[1], entry[2], entry[3]]),
        secondary: u32::from_le_bytes([entry[4], entry[5], entry[6], entry[7]]),
        class_index: entry[8],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::car_resources::is_valid_car_model;

    #[test]
    fn model_dimensions_are_consistent() {
        assert_eq!(COUNTRY_COUNT, 9);
        assert_eq!(DIFFICULTY_COUNT, 3);
        // CAR_COUNT tracks the car_resources validity bound.
        assert_eq!(CAR_COUNT, 14);
        assert!(is_valid_car_model((CAR_COUNT - 1) as u8));
        assert!(!is_valid_car_model(CAR_COUNT as u8));
    }

    #[test]
    fn offset_mapping_matches_the_verified_anchors() {
        // Car nibble: live +0xb6 -> record +0x96.
        assert_eq!(record_offset(LIVE_CAR_NIBBLE_OFFSET), Some(0x96));
        // Country: live +0x168 -> record +0x148.
        assert_eq!(record_offset(LIVE_COUNTRY_OFFSET), Some(0x148));
        // Progress array: live +0x170 -> record +0x150.
        assert_eq!(
            record_offset(LIVE_PROGRESS_OFFSET),
            Some(PROGRESS_RECORD_OFFSET)
        );
        // Payload starts at live +0x2c.
        assert_eq!(payload_offset(PAYLOAD_LIVE_DELTA), Some(0));
        // Below the window -> None.
        assert_eq!(record_offset(0x10), None);
        assert_eq!(payload_offset(0x10), None);
    }

    #[test]
    fn event_progress_flag_decode() {
        // Started with 2 completed stages.
        let flag = PROGRESS_FLAG_STARTED | 0x02;
        assert!(event_started(flag));
        assert_eq!(event_completed_stages(flag), 2);
        // Not started.
        assert!(!event_started(0x02));
    }

    #[test]
    fn current_event_cursor_is_first_started_but_incomplete() {
        // Event 0 done (started + completed), event 1 started + 0 completed.
        let flags = [
            PROGRESS_FLAG_STARTED | 0x03,
            PROGRESS_FLAG_STARTED,
            PROGRESS_FLAG_STARTED | 0x01,
        ];
        assert_eq!(current_event_index(&flags), Some(1));
        // Nothing started -> no cursor.
        assert_eq!(current_event_index(&[0, 0, 0]), None);
        // All completed -> no cursor.
        assert_eq!(
            current_event_index(&[PROGRESS_FLAG_STARTED | 1, PROGRESS_FLAG_STARTED | 2]),
            None
        );
    }

    #[test]
    fn descriptor_pack_round_trips_and_maps_string_ids() {
        let byte1 = pack_descriptor(5, 4, 2);
        assert_eq!(descriptor_country(byte1), 5);
        assert_eq!(descriptor_stage(byte1), 4);
        assert_eq!(descriptor_mode(byte1), 2);
        // String-id bases.
        assert_eq!(location_string_id(byte1), 5 + STRING_ID_LOCATION_BASE);
        assert_eq!(mode_string_id(byte1), 2 + STRING_ID_MODE_BASE);
        // Fields are masked to their widths.
        let packed = pack_descriptor(0xff, 0xff, 0x3);
        assert_eq!(descriptor_country(packed), 7);
        assert_eq!(descriptor_stage(packed), 7);
        assert_eq!(descriptor_mode(packed), 3);
    }

    #[test]
    fn rally_school_and_mode_tier_selectors() {
        // selector bits: task = >>5, difficulty = >>3 & 3.
        let selector = (0b101 << 5) | (0b10 << 3);
        assert_eq!(rally_school_task(selector), 0b101);
        assert_eq!(rally_school_difficulty(selector), 0b10);
        // Mode tier from live +0x18.
        assert_eq!(mode_tier(0b11 << 3), 3);
        assert_eq!(mode_tier(0), 0);
    }

    #[test]
    fn car_stats_entry_parses_the_recovered_shape() {
        let entry = [
            0x2c, 0x01, 0x00, 0x00, // primary = 300
            0x10, 0x00, 0x00, 0x00, // secondary = 16
            0x02, // class index
            0xff, 0xff, 0xff, // trailing bytes ignored
        ];
        let stats = parse_car_stats_entry(&entry);
        assert_eq!(stats.primary, 300);
        assert_eq!(stats.secondary, 16);
        assert_eq!(stats.class_index, 2);
    }
}
