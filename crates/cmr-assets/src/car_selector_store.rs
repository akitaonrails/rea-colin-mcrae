use std::fmt;
use std::fs;
use std::io::Read;
use std::path::Path;

use cmr_formats::pe_car_selector::{
    CAR_SELECTOR_COUNT, CAR_SELECTOR_ENCODED_LEN, CAR_SELECTOR_FORMAT_VERSION, CarSelectorError,
    CarSelectorMap, decode_car_selectors,
};

use crate::VerificationError;
use crate::car_selector_import::{
    CAR_SELECTOR_DATA_FILE, CAR_SELECTOR_IMPORT_SCHEMA_VERSION, CAR_SELECTOR_STATE_FILE,
    CarSelectorImportSummary,
};
use crate::car_selector_profile::{CarSelectorEncoding, ReviewedCarSelectorProfile};
use crate::verify::{require_real_directory, sha256};

const MAX_STATE_BYTES: u64 = 64 * 1024;
#[derive(Debug)]
pub struct CarSelectorStore {
    map: CarSelectorMap,
}
#[derive(Debug)]
pub enum CarSelectorStoreError {
    Verification(VerificationError),
    StateTooLarge(u64),
    StateJson(serde_json::Error),
    InvalidState { field: &'static str },
    Data(CarSelectorError),
}
impl fmt::Display for CarSelectorStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Verification(e) => write!(f, "car-selector store verification failed: {e}"),
            Self::StateTooLarge(s) => write!(
                f,
                "car-selector state is {s} bytes; limit is {MAX_STATE_BYTES}"
            ),
            Self::StateJson(e) => write!(f, "invalid car-selector state: {e}"),
            Self::InvalidState { field } => {
                write!(f, "car-selector state does not match profile field {field}")
            }
            Self::Data(e) => write!(f, "invalid car-selector data: {e}"),
        }
    }
}
impl std::error::Error for CarSelectorStoreError {}
impl From<VerificationError> for CarSelectorStoreError {
    fn from(e: VerificationError) -> Self {
        Self::Verification(e)
    }
}
impl From<CarSelectorError> for CarSelectorStoreError {
    fn from(e: CarSelectorError) -> Self {
        Self::Data(e)
    }
}

impl CarSelectorStore {
    pub fn open(
        root: &Path,
        reviewed: &ReviewedCarSelectorProfile,
    ) -> Result<Self, CarSelectorStoreError> {
        require_real_directory(root)?;
        let sp = root.join(CAR_SELECTOR_STATE_FILE);
        reject(&sp)?;
        let sb = read_bounded(&sp, MAX_STATE_BYTES)?;
        let state: CarSelectorImportSummary =
            serde_json::from_slice(&sb).map_err(CarSelectorStoreError::StateJson)?;
        validate(&state, reviewed)?;
        let dp = root.join(CAR_SELECTOR_DATA_FILE);
        reject(&dp)?;
        let data = read_exact(&dp, CAR_SELECTOR_ENCODED_LEN as u64)?;
        if sha256(&data) != state.output.sha256 {
            return Err(CarSelectorStoreError::InvalidState {
                field: "output.sha256",
            });
        }
        Ok(Self {
            map: decode_car_selectors(&data)?,
        })
    }
    pub fn map(&self) -> &CarSelectorMap {
        &self.map
    }
    pub fn selector_for_code(&self, code: &str) -> Option<u8> {
        self.map.selector_for_code(code)
    }
}
fn validate(
    s: &CarSelectorImportSummary,
    r: &ReviewedCarSelectorProfile,
) -> Result<(), CarSelectorStoreError> {
    let p = r.profile();
    let checks = [
        (
            s.schema_version == CAR_SELECTOR_IMPORT_SCHEMA_VERSION,
            "schema_version",
        ),
        (s.profile == p.id, "profile"),
        (s.profile_sha256 == r.sha256(), "profile_sha256"),
        (s.source.path == p.executable.path, "source.path"),
        (s.source.size == p.executable.size, "source.size"),
        (s.source.sha256 == p.executable.sha256, "source.sha256"),
        (s.output.path == CAR_SELECTOR_DATA_FILE, "output.path"),
        (
            s.output.size == CAR_SELECTOR_ENCODED_LEN as u64,
            "output.size",
        ),
        (
            s.shape.format_version == CAR_SELECTOR_FORMAT_VERSION,
            "shape.format_version",
        ),
        (
            s.shape.selectors == CAR_SELECTOR_COUNT as u32,
            "shape.selectors",
        ),
        (s.shape.code_bytes == 32, "shape.code_bytes"),
        (
            s.shape.encoding == CarSelectorEncoding::CmrCselectorV1,
            "shape.encoding",
        ),
    ];
    for (ok, field) in checks {
        if !ok {
            return Err(CarSelectorStoreError::InvalidState { field });
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
fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, CarSelectorStoreError> {
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
        return Err(CarSelectorStoreError::StateTooLarge(size));
    }
    let mut b = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut b)
        .map_err(|source| VerificationError::Io {
            path: path.into(),
            source,
        })?;
    if b.len() as u64 > limit {
        return Err(CarSelectorStoreError::StateTooLarge(b.len() as u64));
    }
    Ok(b)
}

fn read_exact(path: &Path, expected: u64) -> Result<Vec<u8>, CarSelectorStoreError> {
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
        return Err(CarSelectorError::WrongLength {
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
        return Err(CarSelectorError::WrongLength {
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
    use crate::car_selector_import::{CarSelectorImportFile, CarSelectorImportShape};
    use crate::car_selector_profile::tests::profile;
    use cmr_formats::pe_car_selector::{CarSelectorMap, encode_car_selectors};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "cmr-selector-store-{}-{}",
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
        ReviewedCarSelectorProfile,
        CarSelectorImportSummary,
    ) {
        let reviewed = ReviewedCarSelectorProfile::from_json(
            &serde_json::to_vec(&profile(9, "00".repeat(32))).unwrap(),
        )
        .unwrap();
        let root = temp.0.join("store");
        fs::create_dir(&root).unwrap();
        let map = CarSelectorMap::new(std::array::from_fn(|i| format!("CAR{i:02}"))).unwrap();
        let data = encode_car_selectors(&map);
        fs::write(root.join(CAR_SELECTOR_DATA_FILE), data).unwrap();
        let state = CarSelectorImportSummary {
            schema_version: CAR_SELECTOR_IMPORT_SCHEMA_VERSION,
            profile: reviewed.profile().id.clone(),
            profile_sha256: reviewed.sha256().into(),
            source: CarSelectorImportFile {
                path: "Rally.exe".into(),
                size: 9,
                sha256: "00".repeat(32),
            },
            output: CarSelectorImportFile {
                path: CAR_SELECTOR_DATA_FILE.into(),
                size: CAR_SELECTOR_ENCODED_LEN as u64,
                sha256: sha256(&data),
            },
            shape: CarSelectorImportShape {
                format_version: CAR_SELECTOR_FORMAT_VERSION,
                selectors: CAR_SELECTOR_COUNT as u32,
                code_bytes: 32,
                encoding: CarSelectorEncoding::CmrCselectorV1,
            },
        };
        fs::write(
            root.join(CAR_SELECTOR_STATE_FILE),
            serde_json::to_vec(&state).unwrap(),
        )
        .unwrap();
        (root, reviewed, state)
    }
    fn write_state(root: &Path, state: &CarSelectorImportSummary) {
        fs::write(
            root.join(CAR_SELECTOR_STATE_FILE),
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
                9 => state.shape.selectors -= 1,
                10 => state.shape.code_bytes -= 1,
                _ => state.output.sha256.push('x'),
            }
            write_state(&root, &state);
            assert!(matches!(
                CarSelectorStore::open(&root, &reviewed),
                Err(CarSelectorStoreError::InvalidState { .. })
            ));
            fs::remove_dir_all(root).unwrap();
        }
        let (root, reviewed, _) = fixture(&t);
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join(CAR_SELECTOR_STATE_FILE)).unwrap()).unwrap();
        value["extra"] = serde_json::json!(true);
        fs::write(
            root.join(CAR_SELECTOR_STATE_FILE),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            CarSelectorStore::open(&root, &reviewed),
            Err(CarSelectorStoreError::StateJson(_))
        ));
        fs::remove_dir_all(&root).unwrap();
        let (root, reviewed, _) = fixture(&t);
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join(CAR_SELECTOR_STATE_FILE)).unwrap()).unwrap();
        value["shape"]["encoding"] = serde_json::json!("unknown");
        fs::write(
            root.join(CAR_SELECTOR_STATE_FILE),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            CarSelectorStore::open(&root, &reviewed),
            Err(CarSelectorStoreError::StateJson(_))
        ));
    }

    #[test]
    fn rejects_oversized_malformed_truncated_and_trailing_files() {
        let t = Temp::new();
        let (root, reviewed, state) = fixture(&t);
        fs::write(
            root.join(CAR_SELECTOR_STATE_FILE),
            vec![b' '; (MAX_STATE_BYTES + 1) as usize],
        )
        .unwrap();
        assert!(matches!(
            CarSelectorStore::open(&root, &reviewed),
            Err(CarSelectorStoreError::StateTooLarge(_))
        ));
        fs::write(root.join(CAR_SELECTOR_STATE_FILE), b"{").unwrap();
        assert!(matches!(
            CarSelectorStore::open(&root, &reviewed),
            Err(CarSelectorStoreError::StateJson(_))
        ));
        write_state(&root, &state);
        let data_path = root.join(CAR_SELECTOR_DATA_FILE);
        let data = fs::read(&data_path).unwrap();
        fs::write(&data_path, &data[..data.len() - 1]).unwrap();
        assert!(matches!(
            CarSelectorStore::open(&root, &reviewed),
            Err(CarSelectorStoreError::Data(
                CarSelectorError::WrongLength { .. }
            ))
        ));
        let mut trailing = data;
        trailing.push(0);
        fs::write(&data_path, trailing).unwrap();
        assert!(matches!(
            CarSelectorStore::open(&root, &reviewed),
            Err(CarSelectorStoreError::Data(
                CarSelectorError::WrongLength { .. }
            ))
        ));
    }

    #[test]
    fn rehashes_data_then_rejects_codec_corruption() {
        let t = Temp::new();
        let (root, reviewed, mut state) = fixture(&t);
        let path = root.join(CAR_SELECTOR_DATA_FILE);
        let mut data = fs::read(&path).unwrap();
        data[0] ^= 1;
        fs::write(&path, &data).unwrap();
        assert!(matches!(
            CarSelectorStore::open(&root, &reviewed),
            Err(CarSelectorStoreError::InvalidState {
                field: "output.sha256"
            })
        ));
        state.output.sha256 = sha256(&data);
        write_state(&root, &state);
        assert!(matches!(
            CarSelectorStore::open(&root, &reviewed),
            Err(CarSelectorStoreError::Data(CarSelectorError::InvalidHeader))
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
            CarSelectorStore::open(&root_link, &reviewed),
            Err(CarSelectorStoreError::Verification(_))
        ));
        let state = root.join(CAR_SELECTOR_STATE_FILE);
        let state_real = root.join("state-real");
        fs::rename(&state, &state_real).unwrap();
        symlink(&state_real, &state).unwrap();
        assert!(matches!(
            CarSelectorStore::open(&root, &reviewed),
            Err(CarSelectorStoreError::Verification(_))
        ));
        fs::remove_file(&state).unwrap();
        fs::rename(&state_real, &state).unwrap();
        let data = root.join(CAR_SELECTOR_DATA_FILE);
        let data_real = root.join("data-real");
        fs::rename(&data, &data_real).unwrap();
        symlink(&data_real, &data).unwrap();
        assert!(matches!(
            CarSelectorStore::open(&root, &reviewed),
            Err(CarSelectorStoreError::Verification(_))
        ));
    }

    #[test]
    fn rejects_non_directory_root_and_special_data_file() {
        let t = Temp::new();
        let root_file = t.0.join("root-file");
        fs::write(&root_file, b"x").unwrap();
        let reviewed = ReviewedCarSelectorProfile::from_json(
            &serde_json::to_vec(&profile(9, "00".repeat(32))).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            CarSelectorStore::open(&root_file, &reviewed),
            Err(CarSelectorStoreError::Verification(_))
        ));
        let (root, reviewed, _) = fixture(&t);
        let data = root.join(CAR_SELECTOR_DATA_FILE);
        fs::remove_file(&data).unwrap();
        fs::create_dir(&data).unwrap();
        assert!(matches!(
            CarSelectorStore::open(&root, &reviewed),
            Err(CarSelectorStoreError::Verification(_))
        ));
    }
}
