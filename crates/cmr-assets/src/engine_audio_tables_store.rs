use std::fmt;
use std::fs;
use std::io::Read;
use std::path::Path;

use cmr_formats::pe_engine_audio_tables::{
    ENGINE_AUDIO_TABLES_ENCODED_LEN, EngineAudioTablesError, EngineSoundTables,
    decode_engine_audio_tables,
};

use crate::VerificationError;
use crate::engine_audio_tables_import::{
    ENGINE_AUDIO_TABLES_DATA_FILE, ENGINE_AUDIO_TABLES_IMPORT_SCHEMA_VERSION,
    ENGINE_AUDIO_TABLES_STATE_FILE, EngineAudioTablesImportShape, EngineAudioTablesImportSummary,
};
use crate::engine_audio_tables_profile::ReviewedEngineAudioTablesProfile;
use crate::verify::{require_real_directory, sha256};

const MAX_STATE_BYTES: u64 = 64 * 1024;
#[derive(Debug)]
pub struct EngineAudioTablesStore {
    tables: EngineSoundTables,
}
#[derive(Debug)]
pub enum EngineAudioTablesStoreError {
    Verification(VerificationError),
    StateTooLarge(u64),
    StateJson(serde_json::Error),
    InvalidState { field: &'static str },
    Data(EngineAudioTablesError),
}
impl fmt::Display for EngineAudioTablesStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Verification(e) => {
                write!(f, "engine-audio-tables store verification failed: {e}")
            }
            Self::StateTooLarge(s) => write!(
                f,
                "engine-audio-tables state is {s} bytes; limit is {MAX_STATE_BYTES}"
            ),
            Self::StateJson(e) => write!(f, "invalid engine-audio-tables state: {e}"),
            Self::InvalidState { field } => {
                write!(
                    f,
                    "engine-audio-tables state does not match profile field {field}"
                )
            }
            Self::Data(e) => write!(f, "invalid engine-audio-tables data: {e}"),
        }
    }
}
impl std::error::Error for EngineAudioTablesStoreError {}
impl From<VerificationError> for EngineAudioTablesStoreError {
    fn from(e: VerificationError) -> Self {
        Self::Verification(e)
    }
}
impl From<EngineAudioTablesError> for EngineAudioTablesStoreError {
    fn from(e: EngineAudioTablesError) -> Self {
        Self::Data(e)
    }
}

impl EngineAudioTablesStore {
    pub fn open(
        root: &Path,
        reviewed: &ReviewedEngineAudioTablesProfile,
    ) -> Result<Self, EngineAudioTablesStoreError> {
        require_real_directory(root)?;
        let sp = root.join(ENGINE_AUDIO_TABLES_STATE_FILE);
        reject(&sp)?;
        let sb = read_bounded(&sp, MAX_STATE_BYTES)?;
        let state: EngineAudioTablesImportSummary =
            serde_json::from_slice(&sb).map_err(EngineAudioTablesStoreError::StateJson)?;
        validate(&state, reviewed)?;
        let dp = root.join(ENGINE_AUDIO_TABLES_DATA_FILE);
        reject(&dp)?;
        let data = read_exact(&dp, ENGINE_AUDIO_TABLES_ENCODED_LEN as u64)?;
        if sha256(&data) != state.output.sha256 {
            return Err(EngineAudioTablesStoreError::InvalidState {
                field: "output.sha256",
            });
        }
        Ok(Self {
            tables: decode_engine_audio_tables(&data)?,
        })
    }
    pub fn tables(&self) -> &EngineSoundTables {
        &self.tables
    }
}
fn validate(
    s: &EngineAudioTablesImportSummary,
    r: &ReviewedEngineAudioTablesProfile,
) -> Result<(), EngineAudioTablesStoreError> {
    let p = r.profile();
    let expected = EngineAudioTablesImportShape::expected();
    let checks = [
        (
            s.schema_version == ENGINE_AUDIO_TABLES_IMPORT_SCHEMA_VERSION,
            "schema_version",
        ),
        (s.profile == p.id, "profile"),
        (s.profile_sha256 == r.sha256(), "profile_sha256"),
        (s.source.path == p.executable.path, "source.path"),
        (s.source.size == p.executable.size, "source.size"),
        (s.source.sha256 == p.executable.sha256, "source.sha256"),
        (
            s.output.path == ENGINE_AUDIO_TABLES_DATA_FILE,
            "output.path",
        ),
        (
            s.output.size == ENGINE_AUDIO_TABLES_ENCODED_LEN as u64,
            "output.size",
        ),
        (
            s.shape.format_version == expected.format_version,
            "shape.format_version",
        ),
        (
            s.shape.band_entries == expected.band_entries,
            "shape.band_entries",
        ),
        (
            s.shape.upshift_dip_entries == expected.upshift_dip_entries,
            "shape.upshift_dip_entries",
        ),
        (s.shape.encoding == expected.encoding, "shape.encoding"),
    ];
    for (ok, field) in checks {
        if !ok {
            return Err(EngineAudioTablesStoreError::InvalidState { field });
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
fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, EngineAudioTablesStoreError> {
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
        return Err(EngineAudioTablesStoreError::StateTooLarge(size));
    }
    let mut b = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut b)
        .map_err(|source| VerificationError::Io {
            path: path.into(),
            source,
        })?;
    if b.len() as u64 > limit {
        return Err(EngineAudioTablesStoreError::StateTooLarge(b.len() as u64));
    }
    Ok(b)
}

fn read_exact(path: &Path, expected: u64) -> Result<Vec<u8>, EngineAudioTablesStoreError> {
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
        return Err(EngineAudioTablesError::WrongLength {
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
        return Err(EngineAudioTablesError::WrongLength {
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
    use crate::engine_audio_tables_import::EngineAudioTablesImportFile;
    use crate::engine_audio_tables_profile::tests::{profile, synthetic_tables};
    use cmr_formats::pe_engine_audio_tables::encode_engine_audio_tables;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "cmr-engine-audio-store-{}-{}",
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

    fn fixture(
        temp: &Temp,
    ) -> (
        PathBuf,
        ReviewedEngineAudioTablesProfile,
        EngineAudioTablesImportSummary,
    ) {
        let reviewed = ReviewedEngineAudioTablesProfile::from_json(
            &serde_json::to_vec(&profile(9, "00".repeat(32))).unwrap(),
        )
        .unwrap();
        let root = temp.0.join("store");
        fs::create_dir(&root).unwrap();
        let data = encode_engine_audio_tables(&synthetic_tables()).unwrap();
        fs::write(root.join(ENGINE_AUDIO_TABLES_DATA_FILE), &data).unwrap();
        let state = EngineAudioTablesImportSummary {
            schema_version: ENGINE_AUDIO_TABLES_IMPORT_SCHEMA_VERSION,
            profile: reviewed.profile().id.clone(),
            profile_sha256: reviewed.sha256().into(),
            source: EngineAudioTablesImportFile {
                path: "Rally.exe".into(),
                size: 9,
                sha256: "00".repeat(32),
            },
            output: EngineAudioTablesImportFile {
                path: ENGINE_AUDIO_TABLES_DATA_FILE.into(),
                size: ENGINE_AUDIO_TABLES_ENCODED_LEN as u64,
                sha256: sha256(&data),
            },
            shape: EngineAudioTablesImportShape::expected(),
        };
        fs::write(
            root.join(ENGINE_AUDIO_TABLES_STATE_FILE),
            serde_json::to_vec(&state).unwrap(),
        )
        .unwrap();
        (root, reviewed, state)
    }
    fn write_state(root: &Path, state: &EngineAudioTablesImportSummary) {
        fs::write(
            root.join(ENGINE_AUDIO_TABLES_STATE_FILE),
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
                7 => state.output.size += 1,
                8 => state.shape.format_version += 1,
                9 => state.shape.band_entries[0] -= 1,
                10 => state.shape.band_entries[3] += 1,
                11 => state.shape.upshift_dip_entries -= 1,
                _ => state.output.sha256.push('x'),
            }
            write_state(&root, &state);
            assert!(matches!(
                EngineAudioTablesStore::open(&root, &reviewed),
                Err(EngineAudioTablesStoreError::InvalidState { .. })
            ));
            fs::remove_dir_all(root).unwrap();
        }
        let (root, reviewed, _) = fixture(&t);
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join(ENGINE_AUDIO_TABLES_STATE_FILE)).unwrap())
                .unwrap();
        value["extra"] = serde_json::json!(true);
        fs::write(
            root.join(ENGINE_AUDIO_TABLES_STATE_FILE),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            EngineAudioTablesStore::open(&root, &reviewed),
            Err(EngineAudioTablesStoreError::StateJson(_))
        ));
        fs::remove_dir_all(&root).unwrap();
        let (root, reviewed, _) = fixture(&t);
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join(ENGINE_AUDIO_TABLES_STATE_FILE)).unwrap())
                .unwrap();
        value["shape"]["encoding"] = serde_json::json!("unknown");
        fs::write(
            root.join(ENGINE_AUDIO_TABLES_STATE_FILE),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            EngineAudioTablesStore::open(&root, &reviewed),
            Err(EngineAudioTablesStoreError::StateJson(_))
        ));
    }

    #[test]
    fn rejects_oversized_malformed_truncated_and_trailing_files() {
        let t = Temp::new();
        let (root, reviewed, state) = fixture(&t);
        fs::write(
            root.join(ENGINE_AUDIO_TABLES_STATE_FILE),
            vec![b' '; (MAX_STATE_BYTES + 1) as usize],
        )
        .unwrap();
        assert!(matches!(
            EngineAudioTablesStore::open(&root, &reviewed),
            Err(EngineAudioTablesStoreError::StateTooLarge(_))
        ));
        fs::write(root.join(ENGINE_AUDIO_TABLES_STATE_FILE), b"{").unwrap();
        assert!(matches!(
            EngineAudioTablesStore::open(&root, &reviewed),
            Err(EngineAudioTablesStoreError::StateJson(_))
        ));
        write_state(&root, &state);
        let data_path = root.join(ENGINE_AUDIO_TABLES_DATA_FILE);
        let data = fs::read(&data_path).unwrap();
        fs::write(&data_path, &data[..data.len() - 1]).unwrap();
        assert!(matches!(
            EngineAudioTablesStore::open(&root, &reviewed),
            Err(EngineAudioTablesStoreError::Data(
                EngineAudioTablesError::WrongLength { .. }
            ))
        ));
        let mut trailing = data;
        trailing.push(0);
        fs::write(&data_path, trailing).unwrap();
        assert!(matches!(
            EngineAudioTablesStore::open(&root, &reviewed),
            Err(EngineAudioTablesStoreError::Data(
                EngineAudioTablesError::WrongLength { .. }
            ))
        ));
    }

    #[test]
    fn rehashes_data_then_rejects_codec_corruption() {
        let t = Temp::new();
        let (root, reviewed, mut state) = fixture(&t);
        let path = root.join(ENGINE_AUDIO_TABLES_DATA_FILE);
        let mut data = fs::read(&path).unwrap();
        data[0] ^= 1;
        fs::write(&path, &data).unwrap();
        assert!(matches!(
            EngineAudioTablesStore::open(&root, &reviewed),
            Err(EngineAudioTablesStoreError::InvalidState {
                field: "output.sha256"
            })
        ));
        state.output.sha256 = sha256(&data);
        write_state(&root, &state);
        assert!(matches!(
            EngineAudioTablesStore::open(&root, &reviewed),
            Err(EngineAudioTablesStoreError::Data(
                EngineAudioTablesError::InvalidHeader { offset: 0 }
            ))
        ));
        // A re-hashed band whose header count no longer matches its length.
        data[0] ^= 1;
        data[16 + 4] ^= 1;
        fs::write(&path, &data).unwrap();
        state.output.sha256 = sha256(&data);
        write_state(&root, &state);
        assert!(matches!(
            EngineAudioTablesStore::open(&root, &reviewed),
            Err(EngineAudioTablesStoreError::Data(
                EngineAudioTablesError::BandCount {
                    band: cmr_formats::pe_engine_audio_tables::EngineSoundBandId::Slow,
                    ..
                }
            ))
        ));
    }

    #[test]
    fn opens_a_valid_store_and_exposes_the_decoded_tables() {
        let t = Temp::new();
        let (root, reviewed, _) = fixture(&t);
        let store = EngineAudioTablesStore::open(&root, &reviewed).unwrap();
        assert_eq!(store.tables(), &synthetic_tables());
        assert_eq!(store.tables().top_pitch_at_minus_one(), 0);
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
            EngineAudioTablesStore::open(&root_link, &reviewed),
            Err(EngineAudioTablesStoreError::Verification(_))
        ));
        let state = root.join(ENGINE_AUDIO_TABLES_STATE_FILE);
        let state_real = root.join("state-real");
        fs::rename(&state, &state_real).unwrap();
        symlink(&state_real, &state).unwrap();
        assert!(matches!(
            EngineAudioTablesStore::open(&root, &reviewed),
            Err(EngineAudioTablesStoreError::Verification(_))
        ));
        fs::remove_file(&state).unwrap();
        fs::rename(&state_real, &state).unwrap();
        let data = root.join(ENGINE_AUDIO_TABLES_DATA_FILE);
        let data_real = root.join("data-real");
        fs::rename(&data, &data_real).unwrap();
        symlink(&data_real, &data).unwrap();
        assert!(matches!(
            EngineAudioTablesStore::open(&root, &reviewed),
            Err(EngineAudioTablesStoreError::Verification(_))
        ));
    }

    #[test]
    fn rejects_non_directory_root_and_special_data_file() {
        let t = Temp::new();
        let root_file = t.0.join("root-file");
        fs::write(&root_file, b"x").unwrap();
        let reviewed = ReviewedEngineAudioTablesProfile::from_json(
            &serde_json::to_vec(&profile(9, "00".repeat(32))).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            EngineAudioTablesStore::open(&root_file, &reviewed),
            Err(EngineAudioTablesStoreError::Verification(_))
        ));
        let (root, reviewed, _) = fixture(&t);
        let data = root.join(ENGINE_AUDIO_TABLES_DATA_FILE);
        fs::remove_file(&data).unwrap();
        fs::create_dir(&data).unwrap();
        assert!(matches!(
            EngineAudioTablesStore::open(&root, &reviewed),
            Err(EngineAudioTablesStoreError::Verification(_))
        ));
    }
}
