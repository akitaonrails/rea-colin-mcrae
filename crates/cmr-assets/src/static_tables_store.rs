use std::fmt;
use std::fs;
use std::io::Read;
use std::path::Path;

use cmr_formats::pe_static_tables::{
    STATIC_TABLES_MAX_ENCODED_LEN, StaticTables, StaticTablesError, decode_static_tables,
};

use crate::VerificationError;
use crate::static_tables_import::{
    STATIC_TABLES_DATA_FILE, STATIC_TABLES_IMPORT_SCHEMA_VERSION, STATIC_TABLES_STATE_FILE,
    StaticTablesImportShape, StaticTablesImportSummary,
};
use crate::static_tables_profile::ReviewedStaticTablesProfile;
use crate::verify::{require_real_directory, sha256};

const MAX_STATE_BYTES: u64 = 64 * 1024;

/// An opened, re-verified store written by `extract-static-tables`.
#[derive(Debug)]
pub struct StaticTablesStore {
    tables: StaticTables,
}
#[derive(Debug)]
pub enum StaticTablesStoreError {
    Verification(VerificationError),
    StateTooLarge(u64),
    StateJson(serde_json::Error),
    InvalidState { field: &'static str },
    Data(StaticTablesError),
}
impl fmt::Display for StaticTablesStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Verification(e) => {
                write!(f, "static-tables store verification failed: {e}")
            }
            Self::StateTooLarge(s) => write!(
                f,
                "static-tables state is {s} bytes; limit is {MAX_STATE_BYTES}"
            ),
            Self::StateJson(e) => write!(f, "invalid static-tables state: {e}"),
            Self::InvalidState { field } => {
                write!(
                    f,
                    "static-tables state does not match profile field {field}"
                )
            }
            Self::Data(e) => write!(f, "invalid static-tables data: {e}"),
        }
    }
}
impl std::error::Error for StaticTablesStoreError {}
impl From<VerificationError> for StaticTablesStoreError {
    fn from(e: VerificationError) -> Self {
        Self::Verification(e)
    }
}
impl From<StaticTablesError> for StaticTablesStoreError {
    fn from(e: StaticTablesError) -> Self {
        Self::Data(e)
    }
}

impl StaticTablesStore {
    /// Open a store written by `extract-static-tables`, re-hashing the
    /// payload, requiring it to be the one the reviewed profile names, and
    /// requiring the decoded tables to have the profile's names and shapes.
    pub fn open(
        root: &Path,
        reviewed: &ReviewedStaticTablesProfile,
    ) -> Result<Self, StaticTablesStoreError> {
        require_real_directory(root)?;
        let sp = root.join(STATIC_TABLES_STATE_FILE);
        reject(&sp)?;
        let sb = read_bounded(&sp, MAX_STATE_BYTES)?;
        let state: StaticTablesImportSummary =
            serde_json::from_slice(&sb).map_err(StaticTablesStoreError::StateJson)?;
        validate(&state, reviewed)?;
        let dp = root.join(STATIC_TABLES_DATA_FILE);
        reject(&dp)?;
        let data = read_exact(&dp, state.output.size)?;
        if sha256(&data) != state.output.sha256 {
            return Err(StaticTablesStoreError::InvalidState {
                field: "output.sha256",
            });
        }
        let tables = decode_static_tables(&data)?;
        if !tables.matches_layout(reviewed.layout()) {
            return Err(StaticTablesStoreError::InvalidState {
                field: "tables.entries",
            });
        }
        Ok(Self { tables })
    }
    pub fn tables(&self) -> &StaticTables {
        &self.tables
    }
}
fn validate(
    s: &StaticTablesImportSummary,
    r: &ReviewedStaticTablesProfile,
) -> Result<(), StaticTablesStoreError> {
    let p = r.profile();
    let expected = StaticTablesImportShape::expected(r.layout());
    let checks = [
        (
            s.schema_version == STATIC_TABLES_IMPORT_SCHEMA_VERSION,
            "schema_version",
        ),
        (s.profile == p.id, "profile"),
        (s.profile_sha256 == r.sha256(), "profile_sha256"),
        (s.source.path == p.executable.path, "source.path"),
        (s.source.size == p.executable.size, "source.size"),
        (s.source.sha256 == p.executable.sha256, "source.sha256"),
        (s.output.path == STATIC_TABLES_DATA_FILE, "output.path"),
        (
            s.output.size > 0 && s.output.size <= STATIC_TABLES_MAX_ENCODED_LEN as u64,
            "output.size",
        ),
        (
            s.output.sha256 == p.tables.payload_sha256,
            "tables.payload_sha256",
        ),
        (
            s.shape.format_version == expected.format_version,
            "shape.format_version",
        ),
        (
            s.shape.entry_count == expected.entry_count,
            "shape.entry_count",
        ),
        (s.shape.encoding == expected.encoding, "shape.encoding"),
    ];
    for (ok, field) in checks {
        if !ok {
            return Err(StaticTablesStoreError::InvalidState { field });
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
fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, StaticTablesStoreError> {
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
        return Err(StaticTablesStoreError::StateTooLarge(size));
    }
    let mut b = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut b)
        .map_err(|source| VerificationError::Io {
            path: path.into(),
            source,
        })?;
    if b.len() as u64 > limit {
        return Err(StaticTablesStoreError::StateTooLarge(b.len() as u64));
    }
    Ok(b)
}

fn read_exact(path: &Path, expected: u64) -> Result<Vec<u8>, StaticTablesStoreError> {
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
        return Err(StaticTablesError::WrongLength {
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
        return Err(StaticTablesError::WrongLength {
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
    use crate::static_tables_import::StaticTablesImportFile;
    use crate::static_tables_profile::tests::{profile, synthetic_tables};
    use cmr_formats::pe_static_tables::{StaticTableData, encode_static_tables};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "cmr-static-tables-store-{}-{}",
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
        ReviewedStaticTablesProfile,
        StaticTablesImportSummary,
    ) {
        let mut p = profile(9, "00".repeat(32));
        p.tables.payload_sha256 = sha256(data);
        let reviewed =
            ReviewedStaticTablesProfile::from_json(&serde_json::to_vec(&p).unwrap()).unwrap();
        let root = temp.0.join("store");
        fs::create_dir(&root).unwrap();
        fs::write(root.join(STATIC_TABLES_DATA_FILE), data).unwrap();
        let state = StaticTablesImportSummary {
            schema_version: STATIC_TABLES_IMPORT_SCHEMA_VERSION,
            profile: reviewed.profile().id.clone(),
            profile_sha256: reviewed.sha256().into(),
            source: StaticTablesImportFile {
                path: "Rally.exe".into(),
                size: 9,
                sha256: "00".repeat(32),
            },
            output: StaticTablesImportFile {
                path: STATIC_TABLES_DATA_FILE.into(),
                size: data.len() as u64,
                sha256: sha256(data),
            },
            shape: StaticTablesImportShape::expected(reviewed.layout()),
        };
        write_state(&root, &state);
        (root, reviewed, state)
    }
    fn fixture(
        temp: &Temp,
    ) -> (
        PathBuf,
        ReviewedStaticTablesProfile,
        StaticTablesImportSummary,
    ) {
        fixture_with(temp, &encode_static_tables(&synthetic_tables()))
    }
    fn write_state(root: &Path, state: &StaticTablesImportSummary) {
        fs::write(
            root.join(STATIC_TABLES_STATE_FILE),
            serde_json::to_vec(state).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn rejects_every_state_identity_field_and_unknown_json() {
        let t = Temp::new();
        for field in 0..12 {
            let (root, reviewed, mut state) = fixture(&t);
            match field {
                0 => state.schema_version += 1,
                1 => state.profile.push('x'),
                2 => state.profile_sha256.push('x'),
                3 => state.source.path.push('x'),
                4 => state.source.size += 1,
                5 => state.source.sha256.push('x'),
                6 => state.output.path.push('x'),
                7 => state.output.size = STATIC_TABLES_MAX_ENCODED_LEN as u64 + 1,
                8 => state.shape.format_version += 1,
                9 => state.shape.entry_count -= 1,
                10 => state.output.sha256 = "11".repeat(32),
                _ => state.output.sha256.push('x'),
            }
            write_state(&root, &state);
            assert!(matches!(
                StaticTablesStore::open(&root, &reviewed),
                Err(StaticTablesStoreError::InvalidState { .. })
            ));
            fs::remove_dir_all(root).unwrap();
        }
        let (root, reviewed, _) = fixture(&t);
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join(STATIC_TABLES_STATE_FILE)).unwrap())
                .unwrap();
        value["extra"] = serde_json::json!(true);
        fs::write(
            root.join(STATIC_TABLES_STATE_FILE),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            StaticTablesStore::open(&root, &reviewed),
            Err(StaticTablesStoreError::StateJson(_))
        ));
        fs::remove_dir_all(&root).unwrap();
        let (root, reviewed, _) = fixture(&t);
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join(STATIC_TABLES_STATE_FILE)).unwrap())
                .unwrap();
        value["shape"]["encoding"] = serde_json::json!("unknown");
        fs::write(
            root.join(STATIC_TABLES_STATE_FILE),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            StaticTablesStore::open(&root, &reviewed),
            Err(StaticTablesStoreError::StateJson(_))
        ));
    }

    #[test]
    fn rejects_a_self_consistent_store_the_profile_did_not_review() {
        let t = Temp::new();
        let (root, reviewed, mut state) = fixture(&t);
        // Re-hash a different (valid) payload so the state matches its data.
        let mut entries = synthetic_tables().entries().to_vec();
        entries[0].data = StaticTableData::Block(vec![9, 9, 9, 9]);
        let data = encode_static_tables(&StaticTables::new(entries).unwrap());
        fs::write(root.join(STATIC_TABLES_DATA_FILE), &data).unwrap();
        state.output.size = data.len() as u64;
        state.output.sha256 = sha256(&data);
        write_state(&root, &state);
        assert!(matches!(
            StaticTablesStore::open(&root, &reviewed),
            Err(StaticTablesStoreError::InvalidState {
                field: "tables.payload_sha256"
            })
        ));
    }

    #[test]
    fn rejects_a_reviewed_payload_whose_entries_differ_from_the_profile() {
        // A payload the profile's digest names, but with a renamed entry.
        let mut entries = synthetic_tables().entries().to_vec();
        entries[2].name = "renamed".into();
        let data = encode_static_tables(&StaticTables::new(entries).unwrap());
        let t = Temp::new();
        let (root, reviewed, _) = fixture_with(&t, &data);
        assert!(matches!(
            StaticTablesStore::open(&root, &reviewed),
            Err(StaticTablesStoreError::InvalidState {
                field: "tables.entries"
            })
        ));
    }

    #[test]
    fn rejects_oversized_malformed_truncated_and_trailing_files() {
        let t = Temp::new();
        let (root, reviewed, state) = fixture(&t);
        fs::write(
            root.join(STATIC_TABLES_STATE_FILE),
            vec![b' '; (MAX_STATE_BYTES + 1) as usize],
        )
        .unwrap();
        assert!(matches!(
            StaticTablesStore::open(&root, &reviewed),
            Err(StaticTablesStoreError::StateTooLarge(_))
        ));
        fs::write(root.join(STATIC_TABLES_STATE_FILE), b"{").unwrap();
        assert!(matches!(
            StaticTablesStore::open(&root, &reviewed),
            Err(StaticTablesStoreError::StateJson(_))
        ));
        write_state(&root, &state);
        let data_path = root.join(STATIC_TABLES_DATA_FILE);
        let data = fs::read(&data_path).unwrap();
        fs::write(&data_path, &data[..data.len() - 1]).unwrap();
        assert!(matches!(
            StaticTablesStore::open(&root, &reviewed),
            Err(StaticTablesStoreError::Data(
                StaticTablesError::WrongLength { .. }
            ))
        ));
        let mut trailing = data;
        trailing.push(0);
        fs::write(&data_path, trailing).unwrap();
        assert!(matches!(
            StaticTablesStore::open(&root, &reviewed),
            Err(StaticTablesStoreError::Data(
                StaticTablesError::WrongLength { .. }
            ))
        ));
    }

    #[test]
    fn rehashes_data_then_rejects_codec_corruption() {
        let t = Temp::new();
        let (root, reviewed, _) = fixture(&t);
        let path = root.join(STATIC_TABLES_DATA_FILE);
        let mut data = fs::read(&path).unwrap();
        data[0] ^= 1;
        fs::write(&path, &data).unwrap();
        assert!(matches!(
            StaticTablesStore::open(&root, &reviewed),
            Err(StaticTablesStoreError::InvalidState {
                field: "output.sha256"
            })
        ));
        // A corrupt header that the profile nonetheless "reviewed".
        let t = Temp::new();
        let (root, reviewed, _) = fixture_with(&t, &data);
        assert!(matches!(
            StaticTablesStore::open(&root, &reviewed),
            Err(StaticTablesStoreError::Data(
                StaticTablesError::InvalidHeader { offset: 0 }
            ))
        ));
        // A truncated payload that the profile nonetheless "reviewed".
        let full = encode_static_tables(&synthetic_tables());
        let t = Temp::new();
        let (root, reviewed, _) = fixture_with(&t, &full[..full.len() - 2]);
        assert!(matches!(
            StaticTablesStore::open(&root, &reviewed),
            Err(StaticTablesStoreError::Data(
                StaticTablesError::Truncated { .. }
            ))
        ));
    }

    #[test]
    fn opens_a_valid_store_and_exposes_the_decoded_tables() {
        let t = Temp::new();
        let (root, reviewed, _) = fixture(&t);
        let store = StaticTablesStore::open(&root, &reviewed).unwrap();
        let tables = store.tables();
        assert_eq!(tables, &synthetic_tables());
        assert_eq!(tables.block("synthetic_block"), Some(&[1, 2, 3, 4][..]));
        assert_eq!(
            tables.strings("synthetic_names"),
            Some(&[b"Alpha".to_vec(), b"Beta".to_vec()][..])
        );
        assert_eq!(tables.string("synthetic_name"), Some(&b"Gamma"[..]));
        assert_eq!(tables.block("missing"), None);
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
            StaticTablesStore::open(&root_link, &reviewed),
            Err(StaticTablesStoreError::Verification(_))
        ));
        let state = root.join(STATIC_TABLES_STATE_FILE);
        let state_real = root.join("state-real");
        fs::rename(&state, &state_real).unwrap();
        symlink(&state_real, &state).unwrap();
        assert!(matches!(
            StaticTablesStore::open(&root, &reviewed),
            Err(StaticTablesStoreError::Verification(_))
        ));
        fs::remove_file(&state).unwrap();
        fs::rename(&state_real, &state).unwrap();
        let data = root.join(STATIC_TABLES_DATA_FILE);
        let data_real = root.join("data-real");
        fs::rename(&data, &data_real).unwrap();
        symlink(&data_real, &data).unwrap();
        assert!(matches!(
            StaticTablesStore::open(&root, &reviewed),
            Err(StaticTablesStoreError::Verification(_))
        ));
    }

    #[test]
    fn rejects_non_directory_root_and_special_data_file() {
        let t = Temp::new();
        let root_file = t.0.join("root-file");
        fs::write(&root_file, b"x").unwrap();
        let reviewed = ReviewedStaticTablesProfile::from_json(
            &serde_json::to_vec(&profile(9, "00".repeat(32))).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            StaticTablesStore::open(&root_file, &reviewed),
            Err(StaticTablesStoreError::Verification(_))
        ));
        let (root, reviewed, _) = fixture(&t);
        let data = root.join(STATIC_TABLES_DATA_FILE);
        fs::remove_file(&data).unwrap();
        fs::create_dir(&data).unwrap();
        assert!(matches!(
            StaticTablesStore::open(&root, &reviewed),
            Err(StaticTablesStoreError::Verification(_))
        ));
    }
}
