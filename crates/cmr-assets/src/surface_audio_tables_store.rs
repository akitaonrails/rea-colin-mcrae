use std::fmt;
use std::fs;
use std::io::Read;
use std::path::Path;

use cmr_formats::pe_surface_audio_tables::{
    SURFACE_AUDIO_TABLES_ENCODED_LEN, SurfaceAudioTables, SurfaceAudioTablesError,
    decode_surface_audio_tables,
};

use crate::VerificationError;
use crate::surface_audio_tables_import::{
    SURFACE_AUDIO_TABLES_DATA_FILE, SURFACE_AUDIO_TABLES_IMPORT_SCHEMA_VERSION,
    SURFACE_AUDIO_TABLES_STATE_FILE, SurfaceAudioTablesImportShape,
    SurfaceAudioTablesImportSummary,
};
use crate::surface_audio_tables_profile::ReviewedSurfaceAudioTablesProfile;
use crate::verify::{require_real_directory, sha256};

const MAX_STATE_BYTES: u64 = 64 * 1024;
#[derive(Debug)]
pub struct SurfaceAudioTablesStore {
    tables: SurfaceAudioTables,
}
#[derive(Debug)]
pub enum SurfaceAudioTablesStoreError {
    Verification(VerificationError),
    StateTooLarge(u64),
    StateJson(serde_json::Error),
    InvalidState { field: &'static str },
    Data(SurfaceAudioTablesError),
}
impl fmt::Display for SurfaceAudioTablesStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Verification(e) => {
                write!(f, "surface-audio-tables store verification failed: {e}")
            }
            Self::StateTooLarge(s) => write!(
                f,
                "surface-audio-tables state is {s} bytes; limit is {MAX_STATE_BYTES}"
            ),
            Self::StateJson(e) => write!(f, "invalid surface-audio-tables state: {e}"),
            Self::InvalidState { field } => {
                write!(
                    f,
                    "surface-audio-tables state does not match profile field {field}"
                )
            }
            Self::Data(e) => write!(f, "invalid surface-audio-tables data: {e}"),
        }
    }
}
impl std::error::Error for SurfaceAudioTablesStoreError {}
impl From<VerificationError> for SurfaceAudioTablesStoreError {
    fn from(e: VerificationError) -> Self {
        Self::Verification(e)
    }
}
impl From<SurfaceAudioTablesError> for SurfaceAudioTablesStoreError {
    fn from(e: SurfaceAudioTablesError) -> Self {
        Self::Data(e)
    }
}

impl SurfaceAudioTablesStore {
    pub fn open(
        root: &Path,
        reviewed: &ReviewedSurfaceAudioTablesProfile,
    ) -> Result<Self, SurfaceAudioTablesStoreError> {
        require_real_directory(root)?;
        let sp = root.join(SURFACE_AUDIO_TABLES_STATE_FILE);
        reject(&sp)?;
        let sb = read_bounded(&sp, MAX_STATE_BYTES)?;
        let state: SurfaceAudioTablesImportSummary =
            serde_json::from_slice(&sb).map_err(SurfaceAudioTablesStoreError::StateJson)?;
        validate(&state, reviewed)?;
        let dp = root.join(SURFACE_AUDIO_TABLES_DATA_FILE);
        reject(&dp)?;
        let data = read_exact(&dp, SURFACE_AUDIO_TABLES_ENCODED_LEN as u64)?;
        if sha256(&data) != state.output.sha256 {
            return Err(SurfaceAudioTablesStoreError::InvalidState {
                field: "output.sha256",
            });
        }
        Ok(Self {
            tables: decode_surface_audio_tables(&data)?,
        })
    }
    pub fn tables(&self) -> &SurfaceAudioTables {
        &self.tables
    }
}
fn validate(
    s: &SurfaceAudioTablesImportSummary,
    r: &ReviewedSurfaceAudioTablesProfile,
) -> Result<(), SurfaceAudioTablesStoreError> {
    let p = r.profile();
    let expected = SurfaceAudioTablesImportShape::expected();
    let checks = [
        (
            s.schema_version == SURFACE_AUDIO_TABLES_IMPORT_SCHEMA_VERSION,
            "schema_version",
        ),
        (s.profile == p.id, "profile"),
        (s.profile_sha256 == r.sha256(), "profile_sha256"),
        (s.source.path == p.executable.path, "source.path"),
        (s.source.size == p.executable.size, "source.size"),
        (s.source.sha256 == p.executable.sha256, "source.sha256"),
        (
            s.output.path == SURFACE_AUDIO_TABLES_DATA_FILE,
            "output.path",
        ),
        (
            s.output.size == SURFACE_AUDIO_TABLES_ENCODED_LEN as u64,
            "output.size",
        ),
        (
            s.shape.format_version == expected.format_version,
            "shape.format_version",
        ),
        (s.shape.groups == expected.groups, "shape.groups"),
        (
            s.shape.camera_classes == expected.camera_classes,
            "shape.camera_classes",
        ),
        (
            s.shape.wet_surfaces == expected.wet_surfaces,
            "shape.wet_surfaces",
        ),
        (
            s.shape.initial_last_state_entries == expected.initial_last_state_entries,
            "shape.initial_last_state_entries",
        ),
        (
            s.shape.prang_names == expected.prang_names,
            "shape.prang_names",
        ),
        (s.shape.encoding == expected.encoding, "shape.encoding"),
    ];
    for (ok, field) in checks {
        if !ok {
            return Err(SurfaceAudioTablesStoreError::InvalidState { field });
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
fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, SurfaceAudioTablesStoreError> {
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
        return Err(SurfaceAudioTablesStoreError::StateTooLarge(size));
    }
    let mut b = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut b)
        .map_err(|source| VerificationError::Io {
            path: path.into(),
            source,
        })?;
    if b.len() as u64 > limit {
        return Err(SurfaceAudioTablesStoreError::StateTooLarge(b.len() as u64));
    }
    Ok(b)
}

fn read_exact(path: &Path, expected: u64) -> Result<Vec<u8>, SurfaceAudioTablesStoreError> {
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
        return Err(SurfaceAudioTablesError::WrongLength {
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
        return Err(SurfaceAudioTablesError::WrongLength {
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
    use crate::surface_audio_tables_import::SurfaceAudioTablesImportFile;
    use crate::surface_audio_tables_profile::tests::{profile, synthetic_tables};
    use cmr_formats::pe_surface_audio_tables::encode_surface_audio_tables;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "cmr-surface-audio-store-{}-{}",
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
        ReviewedSurfaceAudioTablesProfile,
        SurfaceAudioTablesImportSummary,
    ) {
        let reviewed = ReviewedSurfaceAudioTablesProfile::from_json(
            &serde_json::to_vec(&profile(9, "00".repeat(32))).unwrap(),
        )
        .unwrap();
        let root = temp.0.join("store");
        fs::create_dir(&root).unwrap();
        let data = encode_surface_audio_tables(&synthetic_tables()).unwrap();
        fs::write(root.join(SURFACE_AUDIO_TABLES_DATA_FILE), &data).unwrap();
        let state = SurfaceAudioTablesImportSummary {
            schema_version: SURFACE_AUDIO_TABLES_IMPORT_SCHEMA_VERSION,
            profile: reviewed.profile().id.clone(),
            profile_sha256: reviewed.sha256().into(),
            source: SurfaceAudioTablesImportFile {
                path: "Rally.exe".into(),
                size: 9,
                sha256: "00".repeat(32),
            },
            output: SurfaceAudioTablesImportFile {
                path: SURFACE_AUDIO_TABLES_DATA_FILE.into(),
                size: SURFACE_AUDIO_TABLES_ENCODED_LEN as u64,
                sha256: sha256(&data),
            },
            shape: SurfaceAudioTablesImportShape::expected(),
        };
        fs::write(
            root.join(SURFACE_AUDIO_TABLES_STATE_FILE),
            serde_json::to_vec(&state).unwrap(),
        )
        .unwrap();
        (root, reviewed, state)
    }
    fn write_state(root: &Path, state: &SurfaceAudioTablesImportSummary) {
        fs::write(
            root.join(SURFACE_AUDIO_TABLES_STATE_FILE),
            serde_json::to_vec(state).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn rejects_every_state_identity_field_and_unknown_json() {
        let t = Temp::new();
        for field in 0..15 {
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
                9 => state.shape.groups -= 1,
                10 => state.shape.camera_classes += 1,
                11 => state.shape.wet_surfaces -= 1,
                12 => state.shape.initial_last_state_entries += 1,
                13 => state.shape.prang_names -= 1,
                _ => state.output.sha256.push('x'),
            }
            write_state(&root, &state);
            assert!(matches!(
                SurfaceAudioTablesStore::open(&root, &reviewed),
                Err(SurfaceAudioTablesStoreError::InvalidState { .. })
            ));
            fs::remove_dir_all(root).unwrap();
        }
        let (root, reviewed, _) = fixture(&t);
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join(SURFACE_AUDIO_TABLES_STATE_FILE)).unwrap())
                .unwrap();
        value["extra"] = serde_json::json!(true);
        fs::write(
            root.join(SURFACE_AUDIO_TABLES_STATE_FILE),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            SurfaceAudioTablesStore::open(&root, &reviewed),
            Err(SurfaceAudioTablesStoreError::StateJson(_))
        ));
        fs::remove_dir_all(&root).unwrap();
        let (root, reviewed, _) = fixture(&t);
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join(SURFACE_AUDIO_TABLES_STATE_FILE)).unwrap())
                .unwrap();
        value["shape"]["encoding"] = serde_json::json!("unknown");
        fs::write(
            root.join(SURFACE_AUDIO_TABLES_STATE_FILE),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            SurfaceAudioTablesStore::open(&root, &reviewed),
            Err(SurfaceAudioTablesStoreError::StateJson(_))
        ));
    }

    #[test]
    fn rejects_oversized_malformed_truncated_and_trailing_files() {
        let t = Temp::new();
        let (root, reviewed, state) = fixture(&t);
        fs::write(
            root.join(SURFACE_AUDIO_TABLES_STATE_FILE),
            vec![b' '; (MAX_STATE_BYTES + 1) as usize],
        )
        .unwrap();
        assert!(matches!(
            SurfaceAudioTablesStore::open(&root, &reviewed),
            Err(SurfaceAudioTablesStoreError::StateTooLarge(_))
        ));
        fs::write(root.join(SURFACE_AUDIO_TABLES_STATE_FILE), b"{").unwrap();
        assert!(matches!(
            SurfaceAudioTablesStore::open(&root, &reviewed),
            Err(SurfaceAudioTablesStoreError::StateJson(_))
        ));
        write_state(&root, &state);
        let data_path = root.join(SURFACE_AUDIO_TABLES_DATA_FILE);
        let data = fs::read(&data_path).unwrap();
        fs::write(&data_path, &data[..data.len() - 1]).unwrap();
        assert!(matches!(
            SurfaceAudioTablesStore::open(&root, &reviewed),
            Err(SurfaceAudioTablesStoreError::Data(
                SurfaceAudioTablesError::WrongLength { .. }
            ))
        ));
        let mut trailing = data;
        trailing.push(0);
        fs::write(&data_path, trailing).unwrap();
        assert!(matches!(
            SurfaceAudioTablesStore::open(&root, &reviewed),
            Err(SurfaceAudioTablesStoreError::Data(
                SurfaceAudioTablesError::WrongLength { .. }
            ))
        ));
    }

    #[test]
    fn rehashes_data_then_rejects_codec_corruption() {
        let t = Temp::new();
        let (root, reviewed, mut state) = fixture(&t);
        let path = root.join(SURFACE_AUDIO_TABLES_DATA_FILE);
        let mut data = fs::read(&path).unwrap();
        data[0] ^= 1;
        fs::write(&path, &data).unwrap();
        assert!(matches!(
            SurfaceAudioTablesStore::open(&root, &reviewed),
            Err(SurfaceAudioTablesStoreError::InvalidState {
                field: "output.sha256"
            })
        ));
        state.output.sha256 = sha256(&data);
        write_state(&root, &state);
        assert!(matches!(
            SurfaceAudioTablesStore::open(&root, &reviewed),
            Err(SurfaceAudioTablesStoreError::Data(
                SurfaceAudioTablesError::InvalidHeader { offset: 0 }
            ))
        ));
        // A re-hashed variant count beyond the 6-wide slot map (group 0, ski).
        data[0] ^= 1;
        data[16] = 7;
        fs::write(&path, &data).unwrap();
        state.output.sha256 = sha256(&data);
        write_state(&root, &state);
        assert!(matches!(
            SurfaceAudioTablesStore::open(&root, &reviewed),
            Err(SurfaceAudioTablesStoreError::Data(
                SurfaceAudioTablesError::VariantCount {
                    group: 0,
                    offset: 16,
                    ..
                }
            ))
        ));
    }

    #[test]
    fn opens_a_valid_store_and_exposes_the_decoded_tables() {
        let t = Temp::new();
        let (root, reviewed, _) = fixture(&t);
        let store = SurfaceAudioTablesStore::open(&root, &reviewed).unwrap();
        assert_eq!(store.tables(), &synthetic_tables());
        assert_eq!(store.tables().prang_names.len(), 48);
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
            SurfaceAudioTablesStore::open(&root_link, &reviewed),
            Err(SurfaceAudioTablesStoreError::Verification(_))
        ));
        let state = root.join(SURFACE_AUDIO_TABLES_STATE_FILE);
        let state_real = root.join("state-real");
        fs::rename(&state, &state_real).unwrap();
        symlink(&state_real, &state).unwrap();
        assert!(matches!(
            SurfaceAudioTablesStore::open(&root, &reviewed),
            Err(SurfaceAudioTablesStoreError::Verification(_))
        ));
        fs::remove_file(&state).unwrap();
        fs::rename(&state_real, &state).unwrap();
        let data = root.join(SURFACE_AUDIO_TABLES_DATA_FILE);
        let data_real = root.join("data-real");
        fs::rename(&data, &data_real).unwrap();
        symlink(&data_real, &data).unwrap();
        assert!(matches!(
            SurfaceAudioTablesStore::open(&root, &reviewed),
            Err(SurfaceAudioTablesStoreError::Verification(_))
        ));
    }

    #[test]
    fn rejects_non_directory_root_and_special_data_file() {
        let t = Temp::new();
        let root_file = t.0.join("root-file");
        fs::write(&root_file, b"x").unwrap();
        let reviewed = ReviewedSurfaceAudioTablesProfile::from_json(
            &serde_json::to_vec(&profile(9, "00".repeat(32))).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            SurfaceAudioTablesStore::open(&root_file, &reviewed),
            Err(SurfaceAudioTablesStoreError::Verification(_))
        ));
        let (root, reviewed, _) = fixture(&t);
        let data = root.join(SURFACE_AUDIO_TABLES_DATA_FILE);
        fs::remove_file(&data).unwrap();
        fs::create_dir(&data).unwrap();
        assert!(matches!(
            SurfaceAudioTablesStore::open(&root, &reviewed),
            Err(SurfaceAudioTablesStoreError::Verification(_))
        ));
    }
}
