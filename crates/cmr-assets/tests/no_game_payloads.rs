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

use cmr_assets::ReviewedProfile;
use sha2::{Digest, Sha256};

/// The reviewed profile embedded in the application build.
const REVIEWED_PROFILE: &str =
    include_str!("../../../profiles/installed-eu-derived-assets-v2.json");

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/cmr-assets has a repository root")
        .to_path_buf()
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
