//! Opt-in smoke for a user-owned official executable.
//!
//! Run explicitly with `CMR_ORIGINAL_DIR` set and `--ignored`. No extracted
//! values or caller-selected paths are logged.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use cmr_assets::{
    ReviewedSurfaceCapabilityProfile, SurfaceCapabilityStore, extract_surface_capabilities,
};
use cmr_formats::pe_surface::{SURFACE_CAPABILITY_BYTES, SURFACE_CAPABILITY_COUNT};

const OFFICIAL_PROFILE: &[u8] =
    include_bytes!("../../../profiles/official-eu-surface-capabilities-v1.json");
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct Temp(PathBuf);

impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "cmr-official-capability-smoke-{}-{}",
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
fn reviewed_official_capabilities_extract_and_reopen_without_exposing_values() {
    let Some(source) = std::env::var_os("CMR_ORIGINAL_DIR") else {
        panic!("CMR_ORIGINAL_DIR must select the user-owned source root");
    };
    let reviewed = ReviewedSurfaceCapabilityProfile::from_json(OFFICIAL_PROFILE)
        .expect("repository capability profile must remain valid");
    let temp = Temp::new();
    let output = temp.0.join("store");
    let result = extract_surface_capabilities(PathBuf::from(source).as_path(), &output, &reviewed);
    assert!(
        result.is_ok(),
        "official capability extraction failed: {:?}",
        result.as_ref().err().map(std::mem::discriminant)
    );
    assert_eq!(
        result.expect("checked successful extraction").output.size,
        SURFACE_CAPABILITY_BYTES as u64
    );
    let store = SurfaceCapabilityStore::open(&output, &reviewed);
    assert!(store.is_ok(), "official capability store reopen failed");
    let store = store.expect("checked successful reopen");
    assert_eq!(store.tables().values.len(), SURFACE_CAPABILITY_COUNT);
    assert!(store.table(SURFACE_CAPABILITY_COUNT - 1).is_some());
    assert!(store.table(SURFACE_CAPABILITY_COUNT).is_none());
}
