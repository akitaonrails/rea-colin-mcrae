use std::fmt;
use std::fs;
use std::io::Read;
use std::path::Path;

use cmr_formats::pe_stage_weather_tables::{
    STAGE_WEATHER_TABLES_ENCODED_LEN, StageWeatherTables, StageWeatherTablesError,
    decode_stage_weather_tables,
};

use crate::VerificationError;
use crate::stage_weather_tables_import::{
    STAGE_WEATHER_TABLES_DATA_FILE, STAGE_WEATHER_TABLES_IMPORT_SCHEMA_VERSION,
    STAGE_WEATHER_TABLES_STATE_FILE, StageWeatherTablesImportShape,
    StageWeatherTablesImportSummary,
};
use crate::stage_weather_tables_profile::ReviewedStageWeatherTablesProfile;
use crate::verify::{require_real_directory, sha256};

const MAX_STATE_BYTES: u64 = 64 * 1024;
#[derive(Debug)]
pub struct StageWeatherTablesStore {
    tables: StageWeatherTables,
}
#[derive(Debug)]
pub enum StageWeatherTablesStoreError {
    Verification(VerificationError),
    StateTooLarge(u64),
    StateJson(serde_json::Error),
    InvalidState { field: &'static str },
    Data(StageWeatherTablesError),
}
impl fmt::Display for StageWeatherTablesStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Verification(e) => {
                write!(f, "stage-weather-tables store verification failed: {e}")
            }
            Self::StateTooLarge(s) => write!(
                f,
                "stage-weather-tables state is {s} bytes; limit is {MAX_STATE_BYTES}"
            ),
            Self::StateJson(e) => write!(f, "invalid stage-weather-tables state: {e}"),
            Self::InvalidState { field } => {
                write!(
                    f,
                    "stage-weather-tables state does not match profile field {field}"
                )
            }
            Self::Data(e) => write!(f, "invalid stage-weather-tables data: {e}"),
        }
    }
}
impl std::error::Error for StageWeatherTablesStoreError {}
impl From<VerificationError> for StageWeatherTablesStoreError {
    fn from(e: VerificationError) -> Self {
        Self::Verification(e)
    }
}
impl From<StageWeatherTablesError> for StageWeatherTablesStoreError {
    fn from(e: StageWeatherTablesError) -> Self {
        Self::Data(e)
    }
}

impl StageWeatherTablesStore {
    /// Open a store written by `extract-stage-weather-tables`, re-hashing the
    /// payload and requiring it to be the one the reviewed profile names.
    pub fn open(
        root: &Path,
        reviewed: &ReviewedStageWeatherTablesProfile,
    ) -> Result<Self, StageWeatherTablesStoreError> {
        require_real_directory(root)?;
        let sp = root.join(STAGE_WEATHER_TABLES_STATE_FILE);
        reject(&sp)?;
        let sb = read_bounded(&sp, MAX_STATE_BYTES)?;
        let state: StageWeatherTablesImportSummary =
            serde_json::from_slice(&sb).map_err(StageWeatherTablesStoreError::StateJson)?;
        validate(&state, reviewed)?;
        let dp = root.join(STAGE_WEATHER_TABLES_DATA_FILE);
        reject(&dp)?;
        let data = read_exact(&dp, STAGE_WEATHER_TABLES_ENCODED_LEN as u64)?;
        if sha256(&data) != state.output.sha256 {
            return Err(StageWeatherTablesStoreError::InvalidState {
                field: "output.sha256",
            });
        }
        Ok(Self {
            tables: decode_stage_weather_tables(&data)?,
        })
    }
    pub fn tables(&self) -> &StageWeatherTables {
        &self.tables
    }
}
fn validate(
    s: &StageWeatherTablesImportSummary,
    r: &ReviewedStageWeatherTablesProfile,
) -> Result<(), StageWeatherTablesStoreError> {
    let p = r.profile();
    let expected = StageWeatherTablesImportShape::expected();
    let checks = [
        (
            s.schema_version == STAGE_WEATHER_TABLES_IMPORT_SCHEMA_VERSION,
            "schema_version",
        ),
        (s.profile == p.id, "profile"),
        (s.profile_sha256 == r.sha256(), "profile_sha256"),
        (s.source.path == p.executable.path, "source.path"),
        (s.source.size == p.executable.size, "source.size"),
        (s.source.sha256 == p.executable.sha256, "source.sha256"),
        (
            s.output.path == STAGE_WEATHER_TABLES_DATA_FILE,
            "output.path",
        ),
        (
            s.output.size == STAGE_WEATHER_TABLES_ENCODED_LEN as u64,
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
            s.shape.school_entries == expected.school_entries,
            "shape.school_entries",
        ),
        (
            s.shape.stage_entries == expected.stage_entries,
            "shape.stage_entries",
        ),
        (s.shape.tables == expected.tables, "shape.tables"),
        (s.shape.encoding == expected.encoding, "shape.encoding"),
    ];
    for (ok, field) in checks {
        if !ok {
            return Err(StageWeatherTablesStoreError::InvalidState { field });
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
fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, StageWeatherTablesStoreError> {
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
        return Err(StageWeatherTablesStoreError::StateTooLarge(size));
    }
    let mut b = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut b)
        .map_err(|source| VerificationError::Io {
            path: path.into(),
            source,
        })?;
    if b.len() as u64 > limit {
        return Err(StageWeatherTablesStoreError::StateTooLarge(b.len() as u64));
    }
    Ok(b)
}

fn read_exact(path: &Path, expected: u64) -> Result<Vec<u8>, StageWeatherTablesStoreError> {
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
        return Err(StageWeatherTablesError::WrongLength {
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
        return Err(StageWeatherTablesError::WrongLength {
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
    use crate::stage_weather_tables_import::StageWeatherTablesImportFile;
    use crate::stage_weather_tables_profile::tests::{profile, synthetic_tables};
    use cmr_formats::pe_stage_weather_tables::encode_stage_weather_tables;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "cmr-stage-weather-store-{}-{}",
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
        ReviewedStageWeatherTablesProfile,
        StageWeatherTablesImportSummary,
    ) {
        let mut p = profile(9, "00".repeat(32));
        p.tables.payload_sha256 = sha256(data);
        let reviewed =
            ReviewedStageWeatherTablesProfile::from_json(&serde_json::to_vec(&p).unwrap()).unwrap();
        let root = temp.0.join("store");
        fs::create_dir(&root).unwrap();
        fs::write(root.join(STAGE_WEATHER_TABLES_DATA_FILE), data).unwrap();
        let state = StageWeatherTablesImportSummary {
            schema_version: STAGE_WEATHER_TABLES_IMPORT_SCHEMA_VERSION,
            profile: reviewed.profile().id.clone(),
            profile_sha256: reviewed.sha256().into(),
            source: StageWeatherTablesImportFile {
                path: "Rally.exe".into(),
                size: 9,
                sha256: "00".repeat(32),
            },
            output: StageWeatherTablesImportFile {
                path: STAGE_WEATHER_TABLES_DATA_FILE.into(),
                size: STAGE_WEATHER_TABLES_ENCODED_LEN as u64,
                sha256: sha256(data),
            },
            shape: StageWeatherTablesImportShape::expected(),
        };
        write_state(&root, &state);
        (root, reviewed, state)
    }
    fn fixture(
        temp: &Temp,
    ) -> (
        PathBuf,
        ReviewedStageWeatherTablesProfile,
        StageWeatherTablesImportSummary,
    ) {
        fixture_with(temp, &encode_stage_weather_tables(&synthetic_tables()))
    }
    fn write_state(root: &Path, state: &StageWeatherTablesImportSummary) {
        fs::write(
            root.join(STAGE_WEATHER_TABLES_STATE_FILE),
            serde_json::to_vec(state).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn rejects_every_state_identity_field_and_unknown_json() {
        let t = Temp::new();
        for field in 0..14 {
            let (root, reviewed, mut state) = fixture(&t);
            match field {
                0 => state.schema_version += 1,
                1 => state.profile.push('x'),
                2 => state.profile_sha256.push('x'),
                3 => state.source.path.push('x'),
                4 => state.source.size += 1,
                5 => state.source.sha256.push('x'),
                6 => state.output.path.push('x'),
                7 => state.output.size += 1,
                8 => state.shape.format_version += 1,
                9 => state.shape.school_entries -= 1,
                10 => state.shape.stage_entries += 1,
                11 => state.shape.tables -= 1,
                12 => state.output.sha256 = "11".repeat(32),
                _ => state.output.sha256.push('x'),
            }
            write_state(&root, &state);
            assert!(matches!(
                StageWeatherTablesStore::open(&root, &reviewed),
                Err(StageWeatherTablesStoreError::InvalidState { .. })
            ));
            fs::remove_dir_all(root).unwrap();
        }
        let (root, reviewed, _) = fixture(&t);
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join(STAGE_WEATHER_TABLES_STATE_FILE)).unwrap())
                .unwrap();
        value["extra"] = serde_json::json!(true);
        fs::write(
            root.join(STAGE_WEATHER_TABLES_STATE_FILE),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            StageWeatherTablesStore::open(&root, &reviewed),
            Err(StageWeatherTablesStoreError::StateJson(_))
        ));
        fs::remove_dir_all(&root).unwrap();
        let (root, reviewed, _) = fixture(&t);
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join(STAGE_WEATHER_TABLES_STATE_FILE)).unwrap())
                .unwrap();
        value["shape"]["encoding"] = serde_json::json!("unknown");
        fs::write(
            root.join(STAGE_WEATHER_TABLES_STATE_FILE),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            StageWeatherTablesStore::open(&root, &reviewed),
            Err(StageWeatherTablesStoreError::StateJson(_))
        ));
    }

    #[test]
    fn rejects_a_self_consistent_store_the_profile_did_not_review() {
        let t = Temp::new();
        let (root, reviewed, mut state) = fixture(&t);
        // Re-hash a different (valid) payload so the state matches its data.
        let mut other = synthetic_tables();
        other.school_lighting[0] ^= 1;
        let data = encode_stage_weather_tables(&other);
        fs::write(root.join(STAGE_WEATHER_TABLES_DATA_FILE), &data).unwrap();
        state.output.sha256 = sha256(&data);
        write_state(&root, &state);
        assert!(matches!(
            StageWeatherTablesStore::open(&root, &reviewed),
            Err(StageWeatherTablesStoreError::InvalidState {
                field: "tables.payload_sha256"
            })
        ));
    }

    #[test]
    fn rejects_oversized_malformed_truncated_and_trailing_files() {
        let t = Temp::new();
        let (root, reviewed, state) = fixture(&t);
        fs::write(
            root.join(STAGE_WEATHER_TABLES_STATE_FILE),
            vec![b' '; (MAX_STATE_BYTES + 1) as usize],
        )
        .unwrap();
        assert!(matches!(
            StageWeatherTablesStore::open(&root, &reviewed),
            Err(StageWeatherTablesStoreError::StateTooLarge(_))
        ));
        fs::write(root.join(STAGE_WEATHER_TABLES_STATE_FILE), b"{").unwrap();
        assert!(matches!(
            StageWeatherTablesStore::open(&root, &reviewed),
            Err(StageWeatherTablesStoreError::StateJson(_))
        ));
        write_state(&root, &state);
        let data_path = root.join(STAGE_WEATHER_TABLES_DATA_FILE);
        let data = fs::read(&data_path).unwrap();
        fs::write(&data_path, &data[..data.len() - 1]).unwrap();
        assert!(matches!(
            StageWeatherTablesStore::open(&root, &reviewed),
            Err(StageWeatherTablesStoreError::Data(
                StageWeatherTablesError::WrongLength { .. }
            ))
        ));
        let mut trailing = data;
        trailing.push(0);
        fs::write(&data_path, trailing).unwrap();
        assert!(matches!(
            StageWeatherTablesStore::open(&root, &reviewed),
            Err(StageWeatherTablesStoreError::Data(
                StageWeatherTablesError::WrongLength { .. }
            ))
        ));
    }

    #[test]
    fn rehashes_data_then_rejects_codec_corruption() {
        let t = Temp::new();
        let (root, reviewed, _) = fixture(&t);
        let path = root.join(STAGE_WEATHER_TABLES_DATA_FILE);
        let mut data = fs::read(&path).unwrap();
        data[0] ^= 1;
        fs::write(&path, &data).unwrap();
        assert!(matches!(
            StageWeatherTablesStore::open(&root, &reviewed),
            Err(StageWeatherTablesStoreError::InvalidState {
                field: "output.sha256"
            })
        ));
        // A corrupt header that the profile nonetheless "reviewed".
        let t = Temp::new();
        let (root, reviewed, _) = fixture_with(&t, &data);
        assert!(matches!(
            StageWeatherTablesStore::open(&root, &reviewed),
            Err(StageWeatherTablesStoreError::Data(
                StageWeatherTablesError::InvalidHeader { offset: 0 }
            ))
        ));
        let mut counts = encode_stage_weather_tables(&synthetic_tables());
        counts[14] ^= 1;
        let t = Temp::new();
        let (root, reviewed, _) = fixture_with(&t, &counts);
        assert!(matches!(
            StageWeatherTablesStore::open(&root, &reviewed),
            Err(StageWeatherTablesStoreError::Data(
                StageWeatherTablesError::InvalidHeader { offset: 14 }
            ))
        ));
    }

    #[test]
    fn opens_a_valid_store_and_exposes_the_decoded_tables() {
        let t = Temp::new();
        let (root, reviewed, _) = fixture(&t);
        let store = StageWeatherTablesStore::open(&root, &reviewed).unwrap();
        assert_eq!(store.tables(), &synthetic_tables());
        assert_eq!(store.tables().stage(8, 6).unwrap().lighting, u32::MAX - 62);
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
            StageWeatherTablesStore::open(&root_link, &reviewed),
            Err(StageWeatherTablesStoreError::Verification(_))
        ));
        let state = root.join(STAGE_WEATHER_TABLES_STATE_FILE);
        let state_real = root.join("state-real");
        fs::rename(&state, &state_real).unwrap();
        symlink(&state_real, &state).unwrap();
        assert!(matches!(
            StageWeatherTablesStore::open(&root, &reviewed),
            Err(StageWeatherTablesStoreError::Verification(_))
        ));
        fs::remove_file(&state).unwrap();
        fs::rename(&state_real, &state).unwrap();
        let data = root.join(STAGE_WEATHER_TABLES_DATA_FILE);
        let data_real = root.join("data-real");
        fs::rename(&data, &data_real).unwrap();
        symlink(&data_real, &data).unwrap();
        assert!(matches!(
            StageWeatherTablesStore::open(&root, &reviewed),
            Err(StageWeatherTablesStoreError::Verification(_))
        ));
    }

    #[test]
    fn rejects_non_directory_root_and_special_data_file() {
        let t = Temp::new();
        let root_file = t.0.join("root-file");
        fs::write(&root_file, b"x").unwrap();
        let reviewed = ReviewedStageWeatherTablesProfile::from_json(
            &serde_json::to_vec(&profile(9, "00".repeat(32))).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            StageWeatherTablesStore::open(&root_file, &reviewed),
            Err(StageWeatherTablesStoreError::Verification(_))
        ));
        let (root, reviewed, _) = fixture(&t);
        let data = root.join(STAGE_WEATHER_TABLES_DATA_FILE);
        fs::remove_file(&data).unwrap();
        fs::create_dir(&data).unwrap();
        assert!(matches!(
            StageWeatherTablesStore::open(&root, &reviewed),
            Err(StageWeatherTablesStoreError::Verification(_))
        ));
    }
}
