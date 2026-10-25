//! Opt-in smoke for a user-owned official executable.
//!
//! Run explicitly with `CMR_ORIGINAL_DIR` set and `--ignored`. The test never
//! logs extracted words or retains the caller's source path.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use cmr_assets::{
    AuxControlConstantsStore, ReviewedAuxControlProfile, extract_aux_control_constants,
};
use cmr_formats::pe_aux_control_initializer::{
    AUX_CONTROL_COMMON_WORD_COUNT, AUX_CONTROL_ENCODED_LEN, AUX_CONTROL_FORMAT_VERSION,
    AUX_CONTROL_MULTIPLIER_COUNT, AUX_CONTROL_POLICY_COUNT, AUX_CONTROL_POLICY_DIVISOR_COUNT,
    AUX_CONTROL_POLICY_WORD_COUNT,
};

const OFFICIAL_PROFILE: &[u8] =
    include_bytes!("../../../profiles/official-eu-aux-control-constants-v1.json");
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct Temp(PathBuf);

impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "cmr-official-aux-smoke-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).expect("fresh smoke root can be created");
        Self(path)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
#[ignore = "requires user-owned media selected only by CMR_ORIGINAL_DIR"]
fn reviewed_official_aux_control_extracts_and_reopens_without_exposing_values() {
    let Some(source) = std::env::var_os("CMR_ORIGINAL_DIR") else {
        panic!("CMR_ORIGINAL_DIR must select the user-owned source root");
    };
    let reviewed = ReviewedAuxControlProfile::from_json(OFFICIAL_PROFILE)
        .expect("repository aux-control profile must remain valid");
    let temp = Temp::new();
    let output = temp.0.join("store");

    let result = extract_aux_control_constants(PathBuf::from(source).as_path(), &output, &reviewed);
    assert!(
        result.is_ok(),
        "official aux-control extraction failed: {:?}",
        result.as_ref().err().map(std::mem::discriminant)
    );
    let summary = result.expect("checked successful extraction");
    assert_eq!(summary.output.size, AUX_CONTROL_ENCODED_LEN as u64);
    assert_eq!(summary.shape.format_version, AUX_CONTROL_FORMAT_VERSION);
    assert_eq!(
        summary.shape.common_word_count,
        AUX_CONTROL_COMMON_WORD_COUNT as u32
    );
    assert_eq!(
        summary.shape.multiplier_count,
        AUX_CONTROL_MULTIPLIER_COUNT as u32
    );
    assert_eq!(summary.shape.policy_count, AUX_CONTROL_POLICY_COUNT as u32);
    assert_eq!(
        summary.shape.policy_word_count,
        AUX_CONTROL_POLICY_WORD_COUNT as u32
    );
    assert_eq!(
        summary.shape.policy_divisor_count,
        AUX_CONTROL_POLICY_DIVISOR_COUNT as u32
    );

    let store = AuxControlConstantsStore::open(&output, &reviewed);
    assert!(store.is_ok(), "official aux-control store reopen failed");
    let constants = store.expect("checked successful reopen");
    assert_eq!(
        constants.constants().common_words.len(),
        AUX_CONTROL_COMMON_WORD_COUNT
    );
    assert_eq!(
        constants.constants().q16_multipliers.len(),
        AUX_CONTROL_MULTIPLIER_COUNT
    );
}
