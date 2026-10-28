use std::fmt;
use std::fs;
use std::io::Read;
use std::path::Path;

use cmr_formats::pe_tyre_tables::{
    TYRE_SURFACE_COUNT, TYRE_TABLES_ENCODED_LEN, TYRE_TABLES_FORMAT_VERSION, TYRE_VARIANT_COUNT,
    TyreSurfaceTables, TyreTablesError, decode_tyre_surface_tables,
};

use crate::VerificationError;
use crate::tyre_tables_import::{
    TYRE_TABLES_DATA_FILE, TYRE_TABLES_IMPORT_SCHEMA_VERSION, TYRE_TABLES_STATE_FILE,
    TyreTablesImportSummary,
};
use crate::tyre_tables_profile::{ReviewedTyreTablesProfile, TyreTablesEncoding};
use crate::verify::{require_real_directory, sha256};

const MAX_STATE_BYTES: u64 = 64 * 1024;
#[derive(Debug)]
pub struct TyreTablesStore {
    tables: TyreSurfaceTables,
}
#[derive(Debug)]
pub enum TyreTablesStoreError {
    Verification(VerificationError),
    StateTooLarge(u64),
    StateJson(serde_json::Error),
    InvalidState { field: &'static str },
    Data(TyreTablesError),
}
impl fmt::Display for TyreTablesStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Verification(e) => {
                write!(f, "tyre-surface-tables store verification failed: {e}")
            }
            Self::StateTooLarge(s) => write!(
                f,
                "tyre-surface-tables state is {s} bytes; limit is {MAX_STATE_BYTES}"
            ),
            Self::StateJson(e) => write!(f, "invalid tyre-surface-tables state: {e}"),
            Self::InvalidState { field } => {
                write!(
                    f,
                    "tyre-surface-tables state does not match profile field {field}"
                )
            }
            Self::Data(e) => write!(f, "invalid tyre-surface-tables data: {e}"),
        }
    }
}
impl std::error::Error for TyreTablesStoreError {}
impl From<VerificationError> for TyreTablesStoreError {
    fn from(e: VerificationError) -> Self {
        Self::Verification(e)
    }
}
impl From<TyreTablesError> for TyreTablesStoreError {
    fn from(e: TyreTablesError) -> Self {
        Self::Data(e)
    }
}

impl TyreTablesStore {
    pub fn open(
        root: &Path,
        reviewed: &ReviewedTyreTablesProfile,
    ) -> Result<Self, TyreTablesStoreError> {
        require_real_directory(root)?;
        let sp = root.join(TYRE_TABLES_STATE_FILE);
        reject(&sp)?;
        let sb = read_bounded(&sp, MAX_STATE_BYTES)?;
        let state: TyreTablesImportSummary =
            serde_json::from_slice(&sb).map_err(TyreTablesStoreError::StateJson)?;
        validate(&state, reviewed)?;
        let dp = root.join(TYRE_TABLES_DATA_FILE);
        reject(&dp)?;
        let data = read_exact(&dp, TYRE_TABLES_ENCODED_LEN as u64)?;
        if sha256(&data) != state.output.sha256 {
            return Err(TyreTablesStoreError::InvalidState {
                field: "output.sha256",
            });
        }
        Ok(Self {
            tables: decode_tyre_surface_tables(&data)?,
        })
    }
    pub fn tables(&self) -> &TyreSurfaceTables {
        &self.tables
    }
}
fn validate(
    s: &TyreTablesImportSummary,
    r: &ReviewedTyreTablesProfile,
) -> Result<(), TyreTablesStoreError> {
    let p = r.profile();
    let checks = [
        (
            s.schema_version == TYRE_TABLES_IMPORT_SCHEMA_VERSION,
            "schema_version",
        ),
        (s.profile == p.id, "profile"),
        (s.profile_sha256 == r.sha256(), "profile_sha256"),
        (s.source.path == p.executable.path, "source.path"),
        (s.source.size == p.executable.size, "source.size"),
        (s.source.sha256 == p.executable.sha256, "source.sha256"),
        (s.output.path == TYRE_TABLES_DATA_FILE, "output.path"),
        (
            s.output.size == TYRE_TABLES_ENCODED_LEN as u64,
            "output.size",
        ),
        (
            s.shape.format_version == TYRE_TABLES_FORMAT_VERSION,
            "shape.format_version",
        ),
        (
            s.shape.surfaces == TYRE_SURFACE_COUNT as u32,
            "shape.surfaces",
        ),
        (
            s.shape.variants == TYRE_VARIANT_COUNT as u32,
            "shape.variants",
        ),
        (
            s.shape.encoding == TyreTablesEncoding::CmrTyreTablesV1,
            "shape.encoding",
        ),
    ];
    for (ok, field) in checks {
        if !ok {
            return Err(TyreTablesStoreError::InvalidState { field });
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
fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, TyreTablesStoreError> {
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
        return Err(TyreTablesStoreError::StateTooLarge(size));
    }
    let mut b = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut b)
        .map_err(|source| VerificationError::Io {
            path: path.into(),
            source,
        })?;
    if b.len() as u64 > limit {
        return Err(TyreTablesStoreError::StateTooLarge(b.len() as u64));
    }
    Ok(b)
}

fn read_exact(path: &Path, expected: u64) -> Result<Vec<u8>, TyreTablesStoreError> {
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
        return Err(TyreTablesError::WrongLength {
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
        return Err(TyreTablesError::WrongLength {
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
    use crate::tyre_tables_import::{TyreTablesImportFile, TyreTablesImportShape};
    use crate::tyre_tables_profile::tests::profile;
    use cmr_formats::pe_tyre_tables::{TyreSurfaceTables, encode_tyre_surface_tables};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "cmr-tyre-store-{}-{}",
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

    fn synthetic_tables() -> TyreSurfaceTables {
        TyreSurfaceTables {
            candidate_base_6b50: std::array::from_fn(|i| i as i32 - 60),
            candidate_base_6f10: std::array::from_fn(|i| i32::MIN + i as i32),
            wheel_terms_777c: std::array::from_fn(|v| {
                std::array::from_fn(|s| [v as u16, s as u16, u16::MAX - s as u16])
            }),
        }
    }

    fn fixture(temp: &Temp) -> (PathBuf, ReviewedTyreTablesProfile, TyreTablesImportSummary) {
        let reviewed = ReviewedTyreTablesProfile::from_json(
            &serde_json::to_vec(&profile(9, "00".repeat(32))).unwrap(),
        )
        .unwrap();
        let root = temp.0.join("store");
        fs::create_dir(&root).unwrap();
        let data = encode_tyre_surface_tables(&synthetic_tables());
        fs::write(root.join(TYRE_TABLES_DATA_FILE), &data).unwrap();
        let state = TyreTablesImportSummary {
            schema_version: TYRE_TABLES_IMPORT_SCHEMA_VERSION,
            profile: reviewed.profile().id.clone(),
            profile_sha256: reviewed.sha256().into(),
            source: TyreTablesImportFile {
                path: "Rally.exe".into(),
                size: 9,
                sha256: "00".repeat(32),
            },
            output: TyreTablesImportFile {
                path: TYRE_TABLES_DATA_FILE.into(),
                size: TYRE_TABLES_ENCODED_LEN as u64,
                sha256: sha256(&data),
            },
            shape: TyreTablesImportShape {
                format_version: TYRE_TABLES_FORMAT_VERSION,
                surfaces: TYRE_SURFACE_COUNT as u32,
                variants: TYRE_VARIANT_COUNT as u32,
                encoding: TyreTablesEncoding::CmrTyreTablesV1,
            },
        };
        fs::write(
            root.join(TYRE_TABLES_STATE_FILE),
            serde_json::to_vec(&state).unwrap(),
        )
        .unwrap();
        (root, reviewed, state)
    }
    fn write_state(root: &Path, state: &TyreTablesImportSummary) {
        fs::write(
            root.join(TYRE_TABLES_STATE_FILE),
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
                7 => state.output.size += 1,
                8 => state.shape.format_version += 1,
                9 => state.shape.surfaces -= 1,
                10 => state.shape.variants -= 1,
                _ => state.output.sha256.push('x'),
            }
            write_state(&root, &state);
            assert!(matches!(
                TyreTablesStore::open(&root, &reviewed),
                Err(TyreTablesStoreError::InvalidState { .. })
            ));
            fs::remove_dir_all(root).unwrap();
        }
        let (root, reviewed, _) = fixture(&t);
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join(TYRE_TABLES_STATE_FILE)).unwrap()).unwrap();
        value["extra"] = serde_json::json!(true);
        fs::write(
            root.join(TYRE_TABLES_STATE_FILE),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            TyreTablesStore::open(&root, &reviewed),
            Err(TyreTablesStoreError::StateJson(_))
        ));
        fs::remove_dir_all(&root).unwrap();
        let (root, reviewed, _) = fixture(&t);
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join(TYRE_TABLES_STATE_FILE)).unwrap()).unwrap();
        value["shape"]["encoding"] = serde_json::json!("unknown");
        fs::write(
            root.join(TYRE_TABLES_STATE_FILE),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            TyreTablesStore::open(&root, &reviewed),
            Err(TyreTablesStoreError::StateJson(_))
        ));
    }

    #[test]
    fn rejects_oversized_malformed_truncated_and_trailing_files() {
        let t = Temp::new();
        let (root, reviewed, state) = fixture(&t);
        fs::write(
            root.join(TYRE_TABLES_STATE_FILE),
            vec![b' '; (MAX_STATE_BYTES + 1) as usize],
        )
        .unwrap();
        assert!(matches!(
            TyreTablesStore::open(&root, &reviewed),
            Err(TyreTablesStoreError::StateTooLarge(_))
        ));
        fs::write(root.join(TYRE_TABLES_STATE_FILE), b"{").unwrap();
        assert!(matches!(
            TyreTablesStore::open(&root, &reviewed),
            Err(TyreTablesStoreError::StateJson(_))
        ));
        write_state(&root, &state);
        let data_path = root.join(TYRE_TABLES_DATA_FILE);
        let data = fs::read(&data_path).unwrap();
        fs::write(&data_path, &data[..data.len() - 1]).unwrap();
        assert!(matches!(
            TyreTablesStore::open(&root, &reviewed),
            Err(TyreTablesStoreError::Data(
                TyreTablesError::WrongLength { .. }
            ))
        ));
        let mut trailing = data;
        trailing.push(0);
        fs::write(&data_path, trailing).unwrap();
        assert!(matches!(
            TyreTablesStore::open(&root, &reviewed),
            Err(TyreTablesStoreError::Data(
                TyreTablesError::WrongLength { .. }
            ))
        ));
    }

    #[test]
    fn rehashes_data_then_rejects_codec_corruption() {
        let t = Temp::new();
        let (root, reviewed, mut state) = fixture(&t);
        let path = root.join(TYRE_TABLES_DATA_FILE);
        let mut data = fs::read(&path).unwrap();
        data[0] ^= 1;
        fs::write(&path, &data).unwrap();
        assert!(matches!(
            TyreTablesStore::open(&root, &reviewed),
            Err(TyreTablesStoreError::InvalidState {
                field: "output.sha256"
            })
        ));
        state.output.sha256 = sha256(&data);
        write_state(&root, &state);
        assert!(matches!(
            TyreTablesStore::open(&root, &reviewed),
            Err(TyreTablesStoreError::Data(TyreTablesError::InvalidHeader))
        ));
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
            TyreTablesStore::open(&root_link, &reviewed),
            Err(TyreTablesStoreError::Verification(_))
        ));
        let state = root.join(TYRE_TABLES_STATE_FILE);
        let state_real = root.join("state-real");
        fs::rename(&state, &state_real).unwrap();
        symlink(&state_real, &state).unwrap();
        assert!(matches!(
            TyreTablesStore::open(&root, &reviewed),
            Err(TyreTablesStoreError::Verification(_))
        ));
        fs::remove_file(&state).unwrap();
        fs::rename(&state_real, &state).unwrap();
        let data = root.join(TYRE_TABLES_DATA_FILE);
        let data_real = root.join("data-real");
        fs::rename(&data, &data_real).unwrap();
        symlink(&data_real, &data).unwrap();
        assert!(matches!(
            TyreTablesStore::open(&root, &reviewed),
            Err(TyreTablesStoreError::Verification(_))
        ));
    }

    #[test]
    fn rejects_non_directory_root_and_special_data_file() {
        let t = Temp::new();
        let root_file = t.0.join("root-file");
        fs::write(&root_file, b"x").unwrap();
        let reviewed = ReviewedTyreTablesProfile::from_json(
            &serde_json::to_vec(&profile(9, "00".repeat(32))).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            TyreTablesStore::open(&root_file, &reviewed),
            Err(TyreTablesStoreError::Verification(_))
        ));
        let (root, reviewed, _) = fixture(&t);
        let data = root.join(TYRE_TABLES_DATA_FILE);
        fs::remove_file(&data).unwrap();
        fs::create_dir(&data).unwrap();
        assert!(matches!(
            TyreTablesStore::open(&root, &reviewed),
            Err(TyreTablesStoreError::Verification(_))
        ));
    }
}
