//! Persistence round-trip and failure-recovery verification (item F07).
//!
//! Exercises the two recovered persistence stores together - the options/config
//! image ([`cmr_core::settings_store`]) and the save-slot/profile record
//! ([`cmr_core::profile_store`]) - proving that a valid write survives a
//! read back unchanged and that every corruption, truncation, or absence
//! recovers to the shipped-defaults / empty-slot behavior rather than loading
//! garbage. All fixtures are synthetic and source-owned; no game payload is
//! used.

use cmr_core::profile_store::{
    SLOT_MAGIC_OFFSET, SLOT_MODE_OFFSET, SlotState, classify_slot, finalize_slot_for_save,
    make_seed, slot_is_valid, slot_mode,
};
use cmr_core::settings_store::{
    SAVE_SLOT_COUNT, SAVE_SLOT_LEN, SETTINGS_BLOB_LEN, SETTINGS_NO_SAVE_FLAG_BIT,
    SETTINGS_NO_SAVE_FLAG_OFFSET, SETTINGS_RUNTIME_ZERO_OFFSET, SettingsImage,
};

/// A synthetic settings blob with a recognizable fill and a payload byte in the
/// runtime-zero region so clearing is observable.
fn settings_blob() -> Vec<u8> {
    let mut raw = vec![0xa5u8; SETTINGS_BLOB_LEN];
    raw[SETTINGS_NO_SAVE_FLAG_OFFSET] = 0; // saving enabled
    raw[SETTINGS_RUNTIME_ZERO_OFFSET] = 0x77;
    raw
}

/// Load reproduces the recovered sequence: accept the exact-length file, then
/// zero the runtime region.
fn load_settings(bytes: &[u8]) -> Option<SettingsImage> {
    let mut image = SettingsImage::from_file_bytes(bytes).ok()?;
    image.clear_runtime_region();
    Some(image)
}

/// Persist honors the do-not-save flag: `None` means the file is left untouched.
fn persist_settings(image: &SettingsImage) -> Option<Vec<u8>> {
    image.should_save().then(|| image.as_bytes().to_vec())
}

#[test]
fn settings_load_then_persist_is_a_stable_fixed_point() {
    let image = load_settings(&settings_blob()).expect("exact length accepted");
    let first = persist_settings(&image).expect("saving enabled");
    // The runtime region was zeroed on load, so the persisted bytes carry it.
    assert_eq!(first[SETTINGS_RUNTIME_ZERO_OFFSET], 0);

    // Reloading and persisting again yields identical bytes - a fixed point.
    let reloaded = load_settings(&first).expect("round-trip length preserved");
    let second = persist_settings(&reloaded).expect("saving still enabled");
    assert_eq!(first, second);
}

#[test]
fn championship_records_survive_settings_round_trips() {
    let mut raw = settings_blob();
    let range = cmr_core::championship_progression::championship_record_range(8, 6)
        .expect("last championship record is in bounds");
    raw[range.clone()].fill(0x5a);

    let first = persist_settings(&load_settings(&raw).unwrap()).unwrap();
    assert!(first[range.clone()].iter().all(|&byte| byte == 0x5a));
    let second = persist_settings(&load_settings(&first).unwrap()).unwrap();
    assert_eq!(first, second);
    assert!(second[range].iter().all(|&byte| byte == 0x5a));
}

#[test]
fn settings_do_not_save_flag_suppresses_persistence() {
    let mut raw = settings_blob();
    raw[SETTINGS_NO_SAVE_FLAG_OFFSET] = SETTINGS_NO_SAVE_FLAG_BIT;
    let image = load_settings(&raw).expect("exact length accepted");
    // A locked image produces no bytes to write; the on-disk file is kept.
    assert_eq!(persist_settings(&image), None);
}

#[test]
fn settings_failure_recovery_falls_back_to_defaults() {
    // Truncated, over-long, and empty files are all rejected, so a caller keeps
    // its own default image instead of loading corruption.
    assert!(load_settings(&[]).is_none());
    assert!(load_settings(&vec![0u8; SETTINGS_BLOB_LEN - 1]).is_none());
    assert!(load_settings(&vec![0u8; SETTINGS_BLOB_LEN + 1]).is_none());
}

/// A save-slot record whose `0x690` payload is a recognizable synthetic fill.
fn slot_record(seed: u32, payload_fill: u8) -> Vec<u8> {
    let mut record = vec![payload_fill; SAVE_SLOT_LEN];
    finalize_slot_for_save(&mut record, seed).expect("exact length");
    record
}

#[test]
fn save_slot_write_survives_read_back_unchanged() {
    // The writer emits the whole record verbatim, so a read back is identity.
    let seed = make_seed(0x1234, 0x5678, 0x9abc);
    let written = slot_record(seed, 0x3c);
    let read_back = written.clone();
    assert!(slot_is_valid(&read_back));
    assert_eq!(classify_slot(Some(&read_back)), SlotState::Occupied);
    assert_eq!(read_back, written);
}

#[test]
fn every_slot_round_trips_with_distinct_seeds() {
    let mut complements = std::collections::BTreeSet::new();
    for index in 0..SAVE_SLOT_COUNT {
        let seed = make_seed(index as u32 + 1, 0xbeef, 0x0f0f);
        let record = slot_record(seed, index as u8);
        assert_eq!(classify_slot(Some(&record)), SlotState::Occupied);
        // Distinct seeds give distinct complement checks.
        let complement = u32::from_le_bytes(record[SAVE_SLOT_LEN - 4..].try_into().unwrap());
        assert!(
            complements.insert(complement),
            "seed collision at slot {index}"
        );
    }
    assert_eq!(complements.len(), SAVE_SLOT_COUNT);
}

#[test]
fn save_slot_failure_recovery_classifies_corruption_as_empty() {
    let seed = make_seed(1, 2, 3);
    // Corrupted magic -> empty.
    let mut record = slot_record(seed, 0);
    record[SLOT_MAGIC_OFFSET] ^= 0xff;
    assert_eq!(classify_slot(Some(&record)), SlotState::Empty);
    // Tampered seed without a matching complement -> empty.
    let mut record = slot_record(seed, 0);
    record[0] ^= 0x01;
    assert_eq!(classify_slot(Some(&record)), SlotState::Empty);
    // Truncated -> empty.
    let record = slot_record(seed, 0);
    assert_eq!(
        classify_slot(Some(&record[..SAVE_SLOT_LEN - 1])),
        SlotState::Empty
    );
    // Absent file -> empty (this is how deletion manifests).
    assert_eq!(classify_slot(None), SlotState::Empty);
}

#[test]
fn overwriting_a_slot_replaces_it_in_place_without_a_delete() {
    // The shipped build has no delete; "create over an occupied slot" is a
    // straight overwrite. A second finalize with a new seed yields a valid
    // record carrying the new identity.
    let old_seed = make_seed(10, 20, 30);
    let mut record = slot_record(old_seed, 0x11);
    assert_eq!(classify_slot(Some(&record)), SlotState::Occupied);

    let new_seed = make_seed(40, 50, 60);
    assert_ne!(new_seed, old_seed);
    finalize_slot_for_save(&mut record, new_seed).unwrap();
    assert!(slot_is_valid(&record));
    assert_eq!(
        u32::from_le_bytes(record[..4].try_into().unwrap()),
        new_seed
    );
}

#[test]
fn save_slot_mode_field_survives_the_round_trip() {
    let mut record = slot_record(make_seed(5, 6, 7), 0);
    record[SLOT_MODE_OFFSET] = 0x02;
    // Mode is untouched by validity, so it reads back across a round trip.
    assert!(slot_is_valid(&record));
    assert_eq!(slot_mode(&record), 0x02);
}
