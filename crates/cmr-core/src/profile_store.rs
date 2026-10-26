//! Save-slot / profile record model and create/load/save/delete flows
//! (reconstruction item F03).
//!
//! # Recovered behavior
//!
//! A save game is a single fixed [`crate::settings_store::SAVE_SLOT_LEN`]-byte
//! record written whole (truncate + overwrite) to `SAVE\SAVESLOT<n>.cms`; there
//! are [`crate::settings_store::SAVE_SLOT_COUNT`] slots. The record is a header
//! wrapping a championship-state payload:
//!
//! | offset | size | role |
//! |--------|------|------|
//! | `0x000` | 4 | seed cookie (generated with the `0xAA` top-byte tag) |
//! | `0x004` | 4 | magic [`SLOT_MAGIC`] (`0x12348765`) |
//! | `0x008` | 4 | format marker [`SLOT_FORMAT_MARKER`] (`0x690`, the payload length) |
//! | `0x00c` | `0x690` | championship-state payload (copied from the live game struct) |
//! | `0x01c` | `0x54` | within the payload: a volatile window always zeroed at save |
//! | `0x69c` | 4 | trailing scalar 0 |
//! | `0x6a0` | 4 | trailing scalar 1 |
//! | `0x6a4` | 2 | trailing scalar 2 |
//! | `0x6a6` | 1 | mode/difficulty, a 2-bit field |
//! | `0x6a7` | 1 | padding (never written) |
//! | `0x6a8` | 4 | `~seed` complement check |
//!
//! A slot is **valid/occupied** (`FUN_0044eed0`) iff the magic, the format
//! marker, and the complement (`== !seed`) all match; the loader
//! (`FUN_0044ed90`) additionally requires the file to be exactly the record
//! length or the slot is treated as empty. The validity triple is an
//! anti-tamper/anti-corruption gate over the 4-byte seed, **not** a payload
//! checksum. Creating a profile is the same code path as saving: a fresh record
//! is serialized over any slot (the menu only asks to confirm an overwrite when
//! the target slot is already occupied).
//!
//! # Negative result: there is no delete
//!
//! The shipped build has no slot-delete primitive - no `DeleteFileA`/`_unlink`/
//! `remove`, and no in-place zero-then-rewrite. A slot only ever becomes empty
//! by failing the validity/size gate (e.g. its file is absent). "Deleting" a
//! slot in a portable host therefore means removing its file (an app-layer I/O
//! action), after which [`classify_slot`] reports it [`SlotState::Empty`].
//!
//! # Scope
//!
//! This module reconstructs the record's structural header and the
//! validity/occupancy gate - all game-data-independent. The `0x690` payload is
//! live championship state (game data) and its field semantics are not modeled
//! or vendored here; only its offset and length are structural.
//!
//! See evidence `rea.official-eu.save-slots`.

use crate::settings_store::SAVE_SLOT_LEN;

/// Offset of the seed cookie.
pub const SLOT_SEED_OFFSET: usize = 0x00;
/// Offset of the magic word.
pub const SLOT_MAGIC_OFFSET: usize = 0x04;
/// Offset of the format marker.
pub const SLOT_FORMAT_OFFSET: usize = 0x08;
/// Offset of the championship-state payload.
pub const SLOT_PAYLOAD_OFFSET: usize = 0x0c;
/// Length of the championship-state payload.
pub const SLOT_PAYLOAD_LEN: usize = 0x690;
/// Offset of the volatile window zeroed at save (within the payload).
pub const SLOT_VOLATILE_ZERO_OFFSET: usize = 0x1c;
/// Length of the volatile window zeroed at save.
pub const SLOT_VOLATILE_ZERO_LEN: usize = 0x54;
/// Offset of the mode/difficulty byte (a 2-bit field).
pub const SLOT_MODE_OFFSET: usize = 0x6a6;
/// Offset of the `~seed` complement check.
pub const SLOT_COMPLEMENT_OFFSET: usize = 0x6a8;

/// The record magic word.
pub const SLOT_MAGIC: u32 = 0x1234_8765;
/// The format marker, equal to the payload length (`0x690`); a different
/// payload size is a different format and fails the validity gate.
pub const SLOT_FORMAT_MARKER: u32 = 0x690;
/// The bits OR-ed into the top byte of a generated seed.
pub const SLOT_SEED_TAG: u32 = 0xaa00_0000;
/// The two-bit mask applied to the mode/difficulty byte.
pub const SLOT_MODE_MASK: u8 = 0x03;

/// Whether a save slot is occupied.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SlotState {
    /// No valid record (absent file, wrong size, or failed validity gate).
    Empty,
    /// A valid, occupied record.
    Occupied,
}

/// Why a record buffer could not be finalized for saving.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfileError {
    /// The buffer was not exactly [`SAVE_SLOT_LEN`] bytes.
    WrongLength {
        /// The required length.
        expected: usize,
        /// The length supplied.
        actual: usize,
    },
    /// The size matched but the native validity triple did not.
    InvalidRecord,
}

/// Generate a save-slot seed from three RNG words, reproducing `FUN_0044ef50`
/// (`((r0 << 10) ^ r1) << 10 ^ r2`, OR-ed with the [`SLOT_SEED_TAG`]).
pub fn make_seed(r0: u32, r1: u32, r2: u32) -> u32 {
    ((((r0 << 10) ^ r1) << 10) ^ r2) | SLOT_SEED_TAG
}

/// The complement check stored for a seed (`~seed`).
pub fn seed_complement(seed: u32) -> u32 {
    !seed
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

/// Whether a record buffer passes the recovered validity gate: the correct
/// length, magic, format marker, and a complement that matches `!seed`.
pub fn slot_is_valid(bytes: &[u8]) -> bool {
    bytes.len() == SAVE_SLOT_LEN
        && read_u32(bytes, SLOT_MAGIC_OFFSET) == SLOT_MAGIC
        && read_u32(bytes, SLOT_FORMAT_OFFSET) == SLOT_FORMAT_MARKER
        && read_u32(bytes, SLOT_COMPLEMENT_OFFSET) == !read_u32(bytes, SLOT_SEED_OFFSET)
}

/// Classify a slot from its file bytes (`None` when the file is absent),
/// reproducing the load gate: absent, wrong-sized, or invalid records are
/// [`SlotState::Empty`]; a valid record is [`SlotState::Occupied`].
pub fn classify_slot(file_bytes: Option<&[u8]>) -> SlotState {
    match file_bytes {
        Some(bytes) if slot_is_valid(bytes) => SlotState::Occupied,
        _ => SlotState::Empty,
    }
}

/// The mode/difficulty 2-bit field of a valid record.
pub fn slot_mode(bytes: &[u8]) -> u8 {
    bytes[SLOT_MODE_OFFSET] & SLOT_MODE_MASK
}

/// Finalize a record buffer for saving, reproducing the header stamp in
/// `FUN_0044ef50`: zero the volatile window, write the magic and format marker,
/// and store the seed with its complement. The caller supplies the record with
/// its championship payload already filled; this stamps only the structural
/// header that the validity gate checks. After this call [`slot_is_valid`]
/// holds.
pub fn finalize_slot_for_save(record: &mut [u8], seed: u32) -> Result<(), ProfileError> {
    if record.len() != SAVE_SLOT_LEN {
        return Err(ProfileError::WrongLength {
            expected: SAVE_SLOT_LEN,
            actual: record.len(),
        });
    }
    record[SLOT_VOLATILE_ZERO_OFFSET..SLOT_VOLATILE_ZERO_OFFSET + SLOT_VOLATILE_ZERO_LEN].fill(0);
    write_u32(record, SLOT_SEED_OFFSET, seed);
    write_u32(record, SLOT_MAGIC_OFFSET, SLOT_MAGIC);
    write_u32(record, SLOT_FORMAT_OFFSET, SLOT_FORMAT_MARKER);
    write_u32(record, SLOT_COMPLEMENT_OFFSET, seed_complement(seed));
    Ok(())
}

/// Number of bytes restored to the live results region at `+0x16c`.
pub const LIVE_RESULTS_LEN: usize = 0x100;
/// Record offset of that results region (`live +0x16c` -> record `+0x14c`).
pub const SLOT_RESULTS_OFFSET: usize = 0x14c;
/// Record offset of the six-bit progress counter.
pub const SLOT_PROGRESS_OFFSET: usize = 0x147;

/// Live fields written by the recovered load-apply callback.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveProfileState {
    pub results: [u8; LIVE_RESULTS_LEN],
    pub progress: u8,
    pub mode: u8,
    pub difficulty: u8,
}

/// Apply a valid record to the live championship fields. Invalid input is
/// rejected without mutation. Other live fields and settings remain the
/// caller's responsibility, matching the callback's bounded writes.
pub fn apply_slot_to_live(record: &[u8], live: &mut LiveProfileState) -> Result<(), ProfileError> {
    if record.len() != SAVE_SLOT_LEN {
        return Err(ProfileError::WrongLength {
            expected: SAVE_SLOT_LEN,
            actual: record.len(),
        });
    }
    if !slot_is_valid(record) {
        return Err(ProfileError::InvalidRecord);
    }
    live.results
        .copy_from_slice(&record[SLOT_RESULTS_OFFSET..SLOT_RESULTS_OFFSET + LIVE_RESULTS_LEN]);
    live.progress = (live.progress & 0xc0) | (record[SLOT_PROGRESS_OFFSET] & 0x3f);
    live.mode |= 0x80;
    live.difficulty = (live.difficulty & !0x18) | (slot_mode(record) << 3);
    Ok(())
}

/// Owned equivalent of the 12-record cache built by `FUN_0044ed90`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProfileSlots {
    records: Vec<Option<Vec<u8>>>,
}

impl ProfileSlots {
    /// Retain only valid records, preserving slot positions. Wrong-sized or
    /// corrupt records and absent files all behave as empty slots.
    #[must_use]
    pub fn from_files(files: &[Option<&[u8]>]) -> Self {
        Self {
            records: files
                .iter()
                .take(crate::settings_store::SAVE_SLOT_COUNT)
                .map(|bytes| {
                    bytes
                        .filter(|bytes| slot_is_valid(bytes))
                        .map(<[u8]>::to_vec)
                })
                .collect(),
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.records.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    #[must_use]
    pub fn record(&self, slot: usize) -> Option<&[u8]> {
        self.records.get(slot)?.as_deref()
    }

    /// Release the entire cache (`FUN_0044eeb0`).
    pub fn clear(&mut self) {
        self.records.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_record(seed: u32) -> Vec<u8> {
        let mut record = vec![0u8; SAVE_SLOT_LEN];
        finalize_slot_for_save(&mut record, seed).unwrap();
        record
    }

    #[test]
    fn make_seed_sets_the_tag_bits() {
        let seed = make_seed(0x1111_1111, 0x2222_2222, 0x3333_3333);
        assert_eq!(seed & SLOT_SEED_TAG, SLOT_SEED_TAG);
        // Deterministic for the same inputs.
        assert_eq!(seed, make_seed(0x1111_1111, 0x2222_2222, 0x3333_3333));
        // Matches the recovered expression.
        let expected =
            ((((0x1111_1111u32 << 10) ^ 0x2222_2222) << 10) ^ 0x3333_3333) | SLOT_SEED_TAG;
        assert_eq!(seed, expected);
    }

    #[test]
    fn seed_complement_round_trips() {
        let seed = make_seed(7, 9, 11);
        assert_eq!(seed_complement(seed), !seed);
        assert_eq!(!seed_complement(seed), seed);
    }

    #[test]
    fn finalize_stamps_a_valid_header_and_zeroes_the_volatile_window() {
        let mut record = vec![0xcdu8; SAVE_SLOT_LEN];
        let seed = make_seed(1, 2, 3);
        finalize_slot_for_save(&mut record, seed).unwrap();

        assert!(slot_is_valid(&record));
        assert_eq!(read_u32(&record, SLOT_SEED_OFFSET), seed);
        assert_eq!(read_u32(&record, SLOT_MAGIC_OFFSET), SLOT_MAGIC);
        assert_eq!(read_u32(&record, SLOT_FORMAT_OFFSET), SLOT_FORMAT_MARKER);
        assert_eq!(read_u32(&record, SLOT_COMPLEMENT_OFFSET), !seed);
        // Volatile window zeroed; payload outside it preserved.
        let zero_end = SLOT_VOLATILE_ZERO_OFFSET + SLOT_VOLATILE_ZERO_LEN;
        assert!(
            record[SLOT_VOLATILE_ZERO_OFFSET..zero_end]
                .iter()
                .all(|&b| b == 0)
        );
        assert_eq!(record[zero_end], 0xcd);
        assert_eq!(record[SLOT_PAYLOAD_OFFSET], 0xcd);
    }

    #[test]
    fn finalize_rejects_a_wrong_length_buffer() {
        assert_eq!(
            finalize_slot_for_save(&mut [0u8; 16], make_seed(1, 1, 1)),
            Err(ProfileError::WrongLength {
                expected: SAVE_SLOT_LEN,
                actual: 16,
            })
        );
    }

    #[test]
    fn validity_gate_rejects_each_broken_field() {
        let seed = make_seed(4, 5, 6);
        // Wrong magic.
        let mut record = valid_record(seed);
        write_u32(&mut record, SLOT_MAGIC_OFFSET, 0xdead_beef);
        assert!(!slot_is_valid(&record));
        // Wrong format marker.
        let mut record = valid_record(seed);
        write_u32(&mut record, SLOT_FORMAT_OFFSET, 0x691);
        assert!(!slot_is_valid(&record));
        // Tampered seed without matching complement (anti-tamper).
        let mut record = valid_record(seed);
        write_u32(&mut record, SLOT_SEED_OFFSET, seed ^ 1);
        assert!(!slot_is_valid(&record));
        // Wrong length.
        assert!(!slot_is_valid(&record[..SAVE_SLOT_LEN - 1]));
    }

    #[test]
    fn classify_slot_reproduces_the_load_gate() {
        let record = valid_record(make_seed(1, 2, 3));
        assert_eq!(classify_slot(Some(&record)), SlotState::Occupied);
        // Absent file -> empty (this is how a "delete" manifests).
        assert_eq!(classify_slot(None), SlotState::Empty);
        // Wrong size -> empty.
        assert_eq!(classify_slot(Some(&[0u8; 8])), SlotState::Empty);
        // Right size but invalid -> empty.
        assert_eq!(
            classify_slot(Some(&vec![0u8; SAVE_SLOT_LEN])),
            SlotState::Empty
        );
    }

    #[test]
    fn slot_mode_is_a_two_bit_field() {
        let mut record = valid_record(make_seed(1, 2, 3));
        record[SLOT_MODE_OFFSET] = 0xfe;
        assert_eq!(slot_mode(&record), 0x02);
        record[SLOT_MODE_OFFSET] = 0x03;
        assert_eq!(slot_mode(&record), 0x03);
    }

    #[test]
    fn load_apply_restores_only_the_recovered_live_fields() {
        let mut record = valid_record(make_seed(1, 2, 3));
        for (index, byte) in record[SLOT_RESULTS_OFFSET..SLOT_RESULTS_OFFSET + LIVE_RESULTS_LEN]
            .iter_mut()
            .enumerate()
        {
            *byte = index as u8;
        }
        record[SLOT_PROGRESS_OFFSET] = 0x3f;
        record[SLOT_MODE_OFFSET] = 2;
        let mut live = LiveProfileState {
            results: [0xff; LIVE_RESULTS_LEN],
            progress: 0x80,
            mode: 1,
            difficulty: 0xe7,
        };
        apply_slot_to_live(&record, &mut live).unwrap();
        assert_eq!(live.results[0], 0);
        assert_eq!(live.results[255], 255);
        assert_eq!(live.progress, 0xbf);
        assert_eq!(live.mode, 0x81);
        assert_eq!(live.difficulty, 0xf7);

        let before = live.clone();
        record[SLOT_MAGIC_OFFSET] ^= 1;
        assert_eq!(
            apply_slot_to_live(&record, &mut live),
            Err(ProfileError::InvalidRecord)
        );
        assert_eq!(live, before);
    }

    #[test]
    fn slot_cache_retains_valid_positions_and_releases_together() {
        let valid = valid_record(make_seed(1, 2, 3));
        let invalid = vec![0; SAVE_SLOT_LEN];
        let files = [
            None,
            None,
            Some(valid.as_slice()),
            Some(invalid.as_slice()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        ];
        let mut slots = ProfileSlots::from_files(&files);
        assert_eq!(slots.len(), 12);
        assert_eq!(slots.record(2), Some(valid.as_slice()));
        assert_eq!(slots.record(3), None);
        slots.clear();
        assert!(slots.is_empty());
    }

    #[test]
    fn structural_offsets_are_consistent() {
        assert_eq!(SLOT_PAYLOAD_OFFSET + SLOT_PAYLOAD_LEN, 0x69c);
        assert_eq!(SLOT_COMPLEMENT_OFFSET + 4, SAVE_SLOT_LEN);
        // The volatile window lies within the payload.
        let volatile_end = SLOT_VOLATILE_ZERO_OFFSET + SLOT_VOLATILE_ZERO_LEN;
        let payload_end = SLOT_PAYLOAD_OFFSET + SLOT_PAYLOAD_LEN;
        assert!(volatile_end <= payload_end);
    }
}
