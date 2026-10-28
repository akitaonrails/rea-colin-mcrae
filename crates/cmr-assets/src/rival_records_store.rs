use std::fmt;
use std::fs;
use std::io::Read;
use std::path::Path;

use cmr_formats::pe_rival_records::{
    RIVAL_RECORDS_MAX_ENCODED_LEN, RivalRecords, RivalRecordsError, decode_rival_records,
};

use crate::VerificationError;
use crate::rival_records_import::{
    RIVAL_RECORDS_DATA_FILE, RIVAL_RECORDS_IMPORT_SCHEMA_VERSION, RIVAL_RECORDS_STATE_FILE,
    RivalRecordsImportShape, RivalRecordsImportSummary,
};
use crate::rival_records_profile::ReviewedRivalRecordsProfile;
use crate::verify::{require_real_directory, sha256};

const MAX_STATE_BYTES: u64 = 64 * 1024;
#[derive(Debug)]
pub struct RivalRecordsStore {
    records: RivalRecords,
}
#[derive(Debug)]
pub enum RivalRecordsStoreError {
    Verification(VerificationError),
    StateTooLarge(u64),
    StateJson(serde_json::Error),
    InvalidState { field: &'static str },
    Data(RivalRecordsError),
}
impl fmt::Display for RivalRecordsStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Verification(e) => {
                write!(f, "rival-records store verification failed: {e}")
            }
            Self::StateTooLarge(s) => write!(
                f,
                "rival-records state is {s} bytes; limit is {MAX_STATE_BYTES}"
            ),
            Self::StateJson(e) => write!(f, "invalid rival-records state: {e}"),
            Self::InvalidState { field } => {
                write!(
                    f,
                    "rival-records state does not match profile field {field}"
                )
            }
            Self::Data(e) => write!(f, "invalid rival-records data: {e}"),
        }
    }
}
impl std::error::Error for RivalRecordsStoreError {}
impl From<VerificationError> for RivalRecordsStoreError {
    fn from(e: VerificationError) -> Self {
        Self::Verification(e)
    }
}
impl From<RivalRecordsError> for RivalRecordsStoreError {
    fn from(e: RivalRecordsError) -> Self {
        Self::Data(e)
    }
}

impl RivalRecordsStore {
    /// Open a store written by `extract-rival-records`, re-hashing the
    /// payload and requiring it to be the one the reviewed profile names.
    pub fn open(
        root: &Path,
        reviewed: &ReviewedRivalRecordsProfile,
    ) -> Result<Self, RivalRecordsStoreError> {
        require_real_directory(root)?;
        let sp = root.join(RIVAL_RECORDS_STATE_FILE);
        reject(&sp)?;
        let sb = read_bounded(&sp, MAX_STATE_BYTES)?;
        let state: RivalRecordsImportSummary =
            serde_json::from_slice(&sb).map_err(RivalRecordsStoreError::StateJson)?;
        validate(&state, reviewed)?;
        let dp = root.join(RIVAL_RECORDS_DATA_FILE);
        reject(&dp)?;
        let data = read_exact(&dp, state.output.size)?;
        if sha256(&data) != state.output.sha256 {
            return Err(RivalRecordsStoreError::InvalidState {
                field: "output.sha256",
            });
        }
        Ok(Self {
            records: decode_rival_records(&data)?,
        })
    }
    pub fn records(&self) -> &RivalRecords {
        &self.records
    }
}
fn validate(
    s: &RivalRecordsImportSummary,
    r: &ReviewedRivalRecordsProfile,
) -> Result<(), RivalRecordsStoreError> {
    let p = r.profile();
    let expected = RivalRecordsImportShape::expected();
    let checks = [
        (
            s.schema_version == RIVAL_RECORDS_IMPORT_SCHEMA_VERSION,
            "schema_version",
        ),
        (s.profile == p.id, "profile"),
        (s.profile_sha256 == r.sha256(), "profile_sha256"),
        (s.source.path == p.executable.path, "source.path"),
        (s.source.size == p.executable.size, "source.size"),
        (s.source.sha256 == p.executable.sha256, "source.sha256"),
        (s.output.path == RIVAL_RECORDS_DATA_FILE, "output.path"),
        (
            s.output.size > 0 && s.output.size <= RIVAL_RECORDS_MAX_ENCODED_LEN as u64,
            "output.size",
        ),
        (
            s.output.sha256 == p.records.payload_sha256,
            "records.payload_sha256",
        ),
        (
            s.shape.format_version == expected.format_version,
            "shape.format_version",
        ),
        (
            s.shape.record_count == expected.record_count,
            "shape.record_count",
        ),
        (s.shape.stride == expected.stride, "shape.stride"),
        (s.shape.encoding == expected.encoding, "shape.encoding"),
    ];
    for (ok, field) in checks {
        if !ok {
            return Err(RivalRecordsStoreError::InvalidState { field });
        }
    }
    Ok(())
}
fn reject(path: &Path) -> Result<(), VerificationError> {
    let m = fs::symlink_metadata(path).map_err(|source| VerificationError::Io {
        path: path.into(),
        source,
    })?;
    if m.file_type().is_symlink() {
        return Err(VerificationError::Symlink(path.into()));
    }
    if !m.is_file() {
        return Err(VerificationError::SpecialFile(path.into()));
    }
    Ok(())
}
fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, RivalRecordsStoreError> {
    let file = fs::File::open(path).map_err(|source| VerificationError::Io {
        path: path.into(),
        source,
    })?;
    let size = file
        .metadata()
        .map_err(|source| VerificationError::Io {
            path: path.into(),
            source,
        })?
        .len();
    if size > limit {
        return Err(RivalRecordsStoreError::StateTooLarge(size));
    }
    let mut b = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut b)
        .map_err(|source| VerificationError::Io {
            path: path.into(),
            source,
        })?;
    if b.len() as u64 > limit {
        return Err(RivalRecordsStoreError::StateTooLarge(b.len() as u64));
    }
    Ok(b)
}

fn read_exact(path: &Path, expected: u64) -> Result<Vec<u8>, RivalRecordsStoreError> {
    let file = fs::File::open(path).map_err(|source| VerificationError::Io {
        path: path.into(),
        source,
    })?;
    let actual = file
        .metadata()
        .map_err(|source| VerificationError::Io {
            path: path.into(),
            source,
        })?
        .len();
    if actual != expected {
        return Err(RivalRecordsError::WrongLength {
            expected: expected as usize,
            actual: usize::try_from(actual).unwrap_or(usize::MAX),
        }
        .into());
    }
    let mut bytes = Vec::with_capacity(expected as usize);
    file.take(expected + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| VerificationError::Io {
            path: path.into(),
            source,
        })?;
    if bytes.len() as u64 != expected {
        return Err(RivalRecordsError::WrongLength {
            expected: expected as usize,
            actual: bytes.len(),
        }
        .into());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rival_records_import::RivalRecordsImportFile;
    use crate::rival_records_profile::tests::{profile, synthetic_records};
    use cmr_formats::pe_rival_records::encode_rival_records;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "cmr-rival-records-store-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    /// A store holding `data`, under a profile that reviewed exactly `data`.
    fn fixture_with(
        temp: &Temp,
        data: &[u8],
    ) -> (
        PathBuf,
        ReviewedRivalRecordsProfile,
        RivalRecordsImportSummary,
    ) {
        let mut p = profile(9, "00".repeat(32));
        p.records.payload_sha256 = sha256(data);
        let reviewed =
            ReviewedRivalRecordsProfile::from_json(&serde_json::to_vec(&p).unwrap()).unwrap();
        let root = temp.0.join("store");
        fs::create_dir(&root).unwrap();
        fs::write(root.join(RIVAL_RECORDS_DATA_FILE), data).unwrap();
        let state = RivalRecordsImportSummary {
            schema_version: RIVAL_RECORDS_IMPORT_SCHEMA_VERSION,
            profile: reviewed.profile().id.clone(),
            profile_sha256: reviewed.sha256().into(),
            source: RivalRecordsImportFile {
                path: "Rally.exe".into(),
                size: 9,
                sha256: "00".repeat(32),
            },
            output: RivalRecordsImportFile {
                path: RIVAL_RECORDS_DATA_FILE.into(),
                size: data.len() as u64,
                sha256: sha256(data),
            },
            shape: RivalRecordsImportShape::expected(),
        };
        write_state(&root, &state);
        (root, reviewed, state)
    }
    fn fixture(
        temp: &Temp,
    ) -> (
        PathBuf,
        ReviewedRivalRecordsProfile,
        RivalRecordsImportSummary,
    ) {
        fixture_with(temp, &encode_rival_records(&synthetic_records()))
    }
    fn write_state(root: &Path, state: &RivalRecordsImportSummary) {
        fs::write(
            root.join(RIVAL_RECORDS_STATE_FILE),
            serde_json::to_vec(state).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn rejects_every_state_identity_field_and_unknown_json() {
        let t = Temp::new();
        for field in 0..13 {
            let (root, reviewed, mut state) = fixture(&t);
            match field {
                0 => state.schema_version += 1,
                1 => state.profile.push('x'),
                2 => state.profile_sha256.push('x'),
                3 => state.source.path.push('x'),
                4 => state.source.size += 1,
                5 => state.source.sha256.push('x'),
                6 => state.output.path.push('x'),
                7 => state.output.size = RIVAL_RECORDS_MAX_ENCODED_LEN as u64 + 1,
                8 => state.shape.format_version += 1,
                9 => state.shape.record_count -= 1,
                10 => state.shape.stride += 1,
                11 => state.output.sha256 = "11".repeat(32),
                _ => state.output.sha256.push('x'),
            }
            write_state(&root, &state);
            assert!(matches!(
                RivalRecordsStore::open(&root, &reviewed),
                Err(RivalRecordsStoreError::InvalidState { .. })
            ));
            fs::remove_dir_all(root).unwrap();
        }
        let (root, reviewed, _) = fixture(&t);
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join(RIVAL_RECORDS_STATE_FILE)).unwrap())
                .unwrap();
        value["extra"] = serde_json::json!(true);
        fs::write(
            root.join(RIVAL_RECORDS_STATE_FILE),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            RivalRecordsStore::open(&root, &reviewed),
            Err(RivalRecordsStoreError::StateJson(_))
        ));
        fs::remove_dir_all(&root).unwrap();
        let (root, reviewed, _) = fixture(&t);
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join(RIVAL_RECORDS_STATE_FILE)).unwrap())
                .unwrap();
        value["shape"]["encoding"] = serde_json::json!("unknown");
        fs::write(
            root.join(RIVAL_RECORDS_STATE_FILE),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            RivalRecordsStore::open(&root, &reviewed),
            Err(RivalRecordsStoreError::StateJson(_))
        ));
    }

    #[test]
    fn rejects_a_self_consistent_store_the_profile_did_not_review() {
        let t = Temp::new();
        let (root, reviewed, mut state) = fixture(&t);
        // Re-hash a different (valid) payload so the state matches its data.
        let mut other = synthetic_records();
        other.records[0].raw[12] ^= 1;
        let data = encode_rival_records(&other);
        fs::write(root.join(RIVAL_RECORDS_DATA_FILE), &data).unwrap();
        state.output.sha256 = sha256(&data);
        write_state(&root, &state);
        assert!(matches!(
            RivalRecordsStore::open(&root, &reviewed),
            Err(RivalRecordsStoreError::InvalidState {
                field: "records.payload_sha256"
            })
        ));
    }

    #[test]
    fn rejects_oversized_malformed_truncated_and_trailing_files() {
        let t = Temp::new();
        let (root, reviewed, state) = fixture(&t);
        fs::write(
            root.join(RIVAL_RECORDS_STATE_FILE),
            vec![b' '; (MAX_STATE_BYTES + 1) as usize],
        )
        .unwrap();
        assert!(matches!(
            RivalRecordsStore::open(&root, &reviewed),
            Err(RivalRecordsStoreError::StateTooLarge(_))
        ));
        fs::write(root.join(RIVAL_RECORDS_STATE_FILE), b"{").unwrap();
        assert!(matches!(
            RivalRecordsStore::open(&root, &reviewed),
            Err(RivalRecordsStoreError::StateJson(_))
        ));
        write_state(&root, &state);
        let data_path = root.join(RIVAL_RECORDS_DATA_FILE);
        let data = fs::read(&data_path).unwrap();
        fs::write(&data_path, &data[..data.len() - 1]).unwrap();
        assert!(matches!(
            RivalRecordsStore::open(&root, &reviewed),
            Err(RivalRecordsStoreError::Data(
                RivalRecordsError::WrongLength { .. }
            ))
        ));
        let mut trailing = data;
        trailing.push(0);
        fs::write(&data_path, trailing).unwrap();
        assert!(matches!(
            RivalRecordsStore::open(&root, &reviewed),
            Err(RivalRecordsStoreError::Data(
                RivalRecordsError::WrongLength { .. }
            ))
        ));
    }

    #[test]
    fn rehashes_data_then_rejects_codec_corruption() {
        let t = Temp::new();
        let (root, reviewed, _) = fixture(&t);
        let path = root.join(RIVAL_RECORDS_DATA_FILE);
        let mut data = fs::read(&path).unwrap();
        data[0] ^= 1;
        fs::write(&path, &data).unwrap();
        assert!(matches!(
            RivalRecordsStore::open(&root, &reviewed),
            Err(RivalRecordsStoreError::InvalidState {
                field: "output.sha256"
            })
        ));
        // A corrupt header that the profile nonetheless "reviewed".
        let t = Temp::new();
        let (root, reviewed, _) = fixture_with(&t, &data);
        assert!(matches!(
            RivalRecordsStore::open(&root, &reviewed),
            Err(RivalRecordsStoreError::Data(
                RivalRecordsError::InvalidHeader { offset: 0 }
            ))
        ));
        let mut counts = encode_rival_records(&synthetic_records());
        counts[14] ^= 1;
        let t = Temp::new();
        let (root, reviewed, _) = fixture_with(&t, &counts);
        assert!(matches!(
            RivalRecordsStore::open(&root, &reviewed),
            Err(RivalRecordsStoreError::Data(
                RivalRecordsError::InvalidHeader { offset: 14 }
            ))
        ));
    }

    #[test]
    fn opens_a_valid_store_and_exposes_the_decoded_records() {
        let t = Temp::new();
        let (root, reviewed, _) = fixture(&t);
        let store = RivalRecordsStore::open(&root, &reviewed).unwrap();
        assert_eq!(store.records(), &synthetic_records());
        let last = store.records().get(15).unwrap();
        assert_eq!(last.name, b"Synthetic15");
        assert_eq!(last.kind, last.raw[8]);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_root_state_and_data() {
        use std::os::unix::fs::symlink;
        let t = Temp::new();
        let (root, reviewed, _) = fixture(&t);
        let root_link = t.0.join("root-link");
        symlink(&root, &root_link).unwrap();
        assert!(matches!(
            RivalRecordsStore::open(&root_link, &reviewed),
            Err(RivalRecordsStoreError::Verification(_))
        ));
        let state = root.join(RIVAL_RECORDS_STATE_FILE);
        let state_real = root.join("state-real");
        fs::rename(&state, &state_real).unwrap();
        symlink(&state_real, &state).unwrap();
        assert!(matches!(
            RivalRecordsStore::open(&root, &reviewed),
            Err(RivalRecordsStoreError::Verification(_))
        ));
        fs::remove_file(&state).unwrap();
        fs::rename(&state_real, &state).unwrap();
        let data = root.join(RIVAL_RECORDS_DATA_FILE);
        let data_real = root.join("data-real");
        fs::rename(&data, &data_real).unwrap();
        symlink(&data_real, &data).unwrap();
        assert!(matches!(
            RivalRecordsStore::open(&root, &reviewed),
            Err(RivalRecordsStoreError::Verification(_))
        ));
    }

    #[test]
    fn rejects_non_directory_root_and_special_data_file() {
        let t = Temp::new();
        let root_file = t.0.join("root-file");
        fs::write(&root_file, b"x").unwrap();
        let reviewed = ReviewedRivalRecordsProfile::from_json(
            &serde_json::to_vec(&profile(9, "00".repeat(32))).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            RivalRecordsStore::open(&root_file, &reviewed),
            Err(RivalRecordsStoreError::Verification(_))
        ));
        let (root, reviewed, _) = fixture(&t);
        let data = root.join(RIVAL_RECORDS_DATA_FILE);
        fs::remove_file(&data).unwrap();
        fs::create_dir(&data).unwrap();
        assert!(matches!(
            RivalRecordsStore::open(&root, &reviewed),
            Err(RivalRecordsStoreError::Verification(_))
        ));
    }
}
