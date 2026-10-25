//! I11 packaging audit: no tracked file may be a catalogued game payload.
//!
//! The reviewed profile records the exact size and SHA-256 of every file in the
//! supported edition. The repository must contain independently written source,
//! documentation, and synthetic fixtures only, so a tracked file that matches a
//! catalogued size and digest is a distribution bug. The audit hashes only
//! tracked files whose size appears in the profile, which keeps it cheap while
//! still covering every payload candidate.
//!
//! `CMR_ORIGINAL_DIR` and `runtime-data/` are Git-ignored user data; this audit
//! reads no game bytes, only the repository's own tracked files.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use cmr_assets::{
    AUX_CONTROL_DATA_FILE, AUX_CONTROL_STATE_FILE, COMPILED_INITIALIZER_DATA_FILE,
    COMPILED_INITIALIZER_STATE_FILE, ReviewedAuxControlProfile, ReviewedCompiledInitializerProfile,
    ReviewedProfile,
};
use sha2::{Digest, Sha256};

/// The reviewed profile embedded in the application build.
const REVIEWED_PROFILE: &str =
    include_str!("../../../profiles/installed-eu-derived-assets-v2.json");
const COMPILED_INITIALIZER_PROFILE: &[u8] =
    include_bytes!("../../../profiles/official-eu-compiled-car-initializer-v1.json");
const AUX_CONTROL_PROFILE: &[u8] =
    include_bytes!("../../../profiles/official-eu-aux-control-constants-v1.json");

#[test]
fn aux_control_profile_is_metadata_only_and_generated_store_is_untracked() {
    let reviewed = ReviewedAuxControlProfile::from_json(AUX_CONTROL_PROFILE)
        .expect("the aux-control profile is valid metadata");
    assert_eq!(reviewed.profile().executable.path, "Rally.exe");

    let document: serde_json::Value = serde_json::from_slice(AUX_CONTROL_PROFILE).unwrap();
    for field in ["values", "constants", "immediates", "bytes"] {
        assert!(document.get(field).is_none());
        assert!(document["initializer"].get(field).is_none());
    }

    let root = repository_root();
    let Some(tracked) = tracked_files(&root) else {
        eprintln!("skipping generated-store audit: no Git metadata in this checkout");
        return;
    };
    for relative in tracked {
        let name = relative.file_name().and_then(|name| name.to_str());
        assert_ne!(name, Some(AUX_CONTROL_DATA_FILE));
        assert_ne!(name, Some(AUX_CONTROL_STATE_FILE));
    }
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/cmr-assets has a repository root")
        .to_path_buf()
}

#[test]
fn compiled_initializer_profile_is_metadata_only_and_generated_store_is_untracked() {
    let reviewed = ReviewedCompiledInitializerProfile::from_json(COMPILED_INITIALIZER_PROFILE)
        .expect("the compiled-initializer profile is valid metadata");
    let profile = reviewed.profile();
    assert_eq!(profile.executable.path, "Rally.exe");
    assert_eq!(profile.initializer.selectors, 14);

    let document: serde_json::Value = serde_json::from_slice(COMPILED_INITIALIZER_PROFILE).unwrap();
    assert!(document.get("cases").is_none());
    assert!(document["initializer"].get("cases").is_none());
    assert!(document["initializer"].get("values").is_none());

    let root = repository_root();
    let Some(tracked) = tracked_files(&root) else {
        eprintln!("skipping generated-store audit: no Git metadata in this checkout");
        return;
    };
    for relative in tracked {
        let name = relative.file_name().and_then(|name| name.to_str());
        assert_ne!(name, Some(COMPILED_INITIALIZER_DATA_FILE));
        assert_ne!(name, Some(COMPILED_INITIALIZER_STATE_FILE));
    }
}

/// The tracked file list, or `None` when this checkout has no Git metadata.
fn tracked_files(root: &Path) -> Option<Vec<PathBuf>> {
    let output = match Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(root)
        .output()
    {
        Ok(output) => output,
        // No git binary: a source tarball can still run the rest of the suite.
        Err(_) => return None,
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("not a git repository"),
            "git ls-files failed: {stderr}"
        );
        return None;
    }
    Some(
        output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|path| !path.is_empty())
            .map(|path| PathBuf::from(std::str::from_utf8(path).expect("tracked paths are UTF-8")))
            .collect(),
    )
}

#[test]
fn no_tracked_file_matches_a_catalogued_game_payload() {
    let profile = ReviewedProfile::from_json(REVIEWED_PROFILE.as_bytes())
        .expect("the embedded reviewed profile is valid");
    let profile = profile.profile();

    let mut payloads = BTreeSet::new();
    let mut payload_sizes = BTreeSet::new();
    for entry in profile.sentinels.iter().chain(&profile.files) {
        payloads.insert(entry.sha256.as_str());
        payload_sizes.insert(entry.size);
    }

    let root = repository_root();
    let Some(tracked) = tracked_files(&root) else {
        eprintln!("skipping the payload audit: no Git metadata in this checkout");
        return;
    };
    assert!(!tracked.is_empty(), "the repository must track files");
    let tracked_count = tracked.len();

    let mut hashed = 0usize;
    for relative in tracked {
        let path = root.join(&relative);
        let metadata = match std::fs::metadata(&path) {
            Ok(metadata) => metadata,
            // A tracked path that is not checked out (sparse checkout) is not a
            // distribution payload; skip it.
            Err(_) => continue,
        };
        if !metadata.is_file() || !payload_sizes.contains(&metadata.len()) {
            continue;
        }
        let bytes = std::fs::read(&path).expect("tracked file is readable");
        hashed += 1;
        let digest = Sha256::digest(&bytes);
        let mut sha256 = String::with_capacity(digest.len() * 2);
        use std::fmt::Write as _;
        for byte in digest {
            write!(sha256, "{byte:02x}").expect("writing to a String cannot fail");
        }
        assert!(
            !payloads.contains(sha256.as_str()),
            "tracked file {} is a catalogued game payload",
            relative.display()
        );
    }

    // Sanity check: the profile really does catalogue payloads, so a broken
    // profile cannot make the audit vacuous.
    assert!(!payloads.is_empty());
    eprintln!(
        "payload audit: {tracked_count} tracked files, {hashed} size-matched candidates, none catalogued"
    );
}
