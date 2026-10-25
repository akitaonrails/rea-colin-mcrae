use std::fmt;
use std::fs;
use std::io::Read;
use std::path::Path;

use cmr_formats::pe_aux_control_initializer::{
    AUX_CONTROL_COMMON_WORD_COUNT, AUX_CONTROL_ENCODED_LEN, AUX_CONTROL_FORMAT_VERSION,
    AUX_CONTROL_MULTIPLIER_COUNT, AUX_CONTROL_POLICY_COUNT, AUX_CONTROL_POLICY_DIVISOR_COUNT,
    AUX_CONTROL_POLICY_WORD_COUNT, AuxControlCodecError, AuxControlConstantWords,
    decode_aux_control_constants,
};

use crate::VerificationError;
use crate::aux_control_import::{
    AUX_CONTROL_DATA_FILE, AUX_CONTROL_IMPORT_SCHEMA_VERSION, AUX_CONTROL_STATE_FILE,
    AuxControlImportSummary,
};
use crate::aux_control_profile::{AuxControlEncoding, ReviewedAuxControlProfile};
use crate::verify::{require_real_directory, sha256};

const MAX_AUX_CONTROL_STATE_BYTES: u64 = 64 * 1024;

#[derive(Debug)]
pub struct AuxControlConstantsStore {
    constants: AuxControlConstantWords,
}

#[derive(Debug)]
pub enum AuxControlStoreError {
    Verification(VerificationError),
    StateTooLarge(u64),
    StateJson(serde_json::Error),
    InvalidState { field: &'static str },
    Data(AuxControlCodecError),
}

impl fmt::Display for AuxControlStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Verification(error) => {
                write!(formatter, "aux-control store verification failed: {error}")
            }
            Self::StateTooLarge(size) => write!(
                formatter,
                "aux-control state is {size} bytes; limit is {MAX_AUX_CONTROL_STATE_BYTES}"
            ),
            Self::StateJson(error) => write!(formatter, "invalid aux-control state: {error}"),
            Self::InvalidState { field } => {
                write!(
                    formatter,
                    "aux-control state does not match profile field {field}"
                )
            }
            Self::Data(error) => write!(formatter, "invalid aux-control data: {error}"),
        }
    }
}

impl std::error::Error for AuxControlStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Verification(error) => Some(error),
            Self::StateJson(error) => Some(error),
            Self::Data(error) => Some(error),
            Self::StateTooLarge(_) | Self::InvalidState { .. } => None,
        }
    }
}

impl From<VerificationError> for AuxControlStoreError {
    fn from(error: VerificationError) -> Self {
        Self::Verification(error)
    }
}

impl From<AuxControlCodecError> for AuxControlStoreError {
    fn from(error: AuxControlCodecError) -> Self {
        Self::Data(error)
    }
}

impl AuxControlConstantsStore {
    /// Open a completed extraction from a stable caller-owned directory.
    /// Persistent links at the root and fixed flat children are rejected; this
    /// is not a race-free sandbox against concurrent directory mutation.
    pub fn open(
        root: &Path,
        reviewed: &ReviewedAuxControlProfile,
    ) -> Result<Self, AuxControlStoreError> {
        require_real_directory(root)?;
        let state_path = root.join(AUX_CONTROL_STATE_FILE);
        reject_link_or_special(&state_path)?;
        let state_bytes = read_bounded(&state_path, MAX_AUX_CONTROL_STATE_BYTES)?;
        let state: AuxControlImportSummary =
            serde_json::from_slice(&state_bytes).map_err(AuxControlStoreError::StateJson)?;
        validate_state(&state, reviewed)?;

        let data_path = root.join(AUX_CONTROL_DATA_FILE);
        reject_link_or_special(&data_path)?;
        let data = read_exact(&data_path, AUX_CONTROL_ENCODED_LEN as u64)?;
        if sha256(&data) != state.output.sha256 {
            return Err(AuxControlStoreError::InvalidState {
                field: "output.sha256",
            });
        }
        let constants = decode_aux_control_constants(&data)?;
        Ok(Self { constants })
    }

    #[must_use]
    pub fn constants(&self) -> &AuxControlConstantWords {
        &self.constants
    }
}

fn validate_state(
    state: &AuxControlImportSummary,
    reviewed: &ReviewedAuxControlProfile,
) -> Result<(), AuxControlStoreError> {
    let profile = reviewed.profile();
    let checks = [
        (
            state.schema_version == AUX_CONTROL_IMPORT_SCHEMA_VERSION,
            "schema_version",
        ),
        (state.profile == profile.id, "profile"),
        (state.profile_sha256 == reviewed.sha256(), "profile_sha256"),
        (state.source.path == profile.executable.path, "source.path"),
        (state.source.size == profile.executable.size, "source.size"),
        (
            state.source.sha256 == profile.executable.sha256,
            "source.sha256",
        ),
        (state.output.path == AUX_CONTROL_DATA_FILE, "output.path"),
        (
            state.output.size == AUX_CONTROL_ENCODED_LEN as u64,
            "output.size",
        ),
        (
            state.shape.format_version == AUX_CONTROL_FORMAT_VERSION,
            "shape.format_version",
        ),
        (
            state.shape.common_word_count == AUX_CONTROL_COMMON_WORD_COUNT as u32,
            "shape.common_word_count",
        ),
        (
            state.shape.multiplier_count == AUX_CONTROL_MULTIPLIER_COUNT as u32,
            "shape.multiplier_count",
        ),
        (
            state.shape.policy_count == AUX_CONTROL_POLICY_COUNT as u32,
            "shape.policy_count",
        ),
        (
            state.shape.policy_word_count == AUX_CONTROL_POLICY_WORD_COUNT as u32,
            "shape.policy_word_count",
        ),
        (
            state.shape.policy_divisor_count == AUX_CONTROL_POLICY_DIVISOR_COUNT as u32,
            "shape.policy_divisor_count",
        ),
        (
            state.shape.encoding == AuxControlEncoding::CmrAuxcV1,
            "shape.encoding",
        ),
    ];
    for (matches, field) in checks {
        if !matches {
            return Err(AuxControlStoreError::InvalidState { field });
        }
    }
    Ok(())
}

fn reject_link_or_special(path: &Path) -> Result<(), VerificationError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| VerificationError::Io {
        path: path.to_owned(),
        source,
    })?;
    if metadata.file_type().is_symlink() {
        return Err(VerificationError::Symlink(path.to_owned()));
    }
    if !metadata.is_file() {
        return Err(VerificationError::SpecialFile(path.to_owned()));
    }
    Ok(())
}

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, AuxControlStoreError> {
    let file = fs::File::open(path).map_err(|source| VerificationError::Io {
        path: path.to_owned(),
        source,
    })?;
    let metadata = file.metadata().map_err(|source| VerificationError::Io {
        path: path.to_owned(),
        source,
    })?;
    if !metadata.is_file() {
        return Err(VerificationError::SpecialFile(path.to_owned()).into());
    }
    if metadata.len() > limit {
        return Err(AuxControlStoreError::StateTooLarge(metadata.len()));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| VerificationError::Io {
            path: path.to_owned(),
            source,
        })?;
    if bytes.len() as u64 > limit {
        return Err(AuxControlStoreError::StateTooLarge(bytes.len() as u64));
    }
    Ok(bytes)
}

fn read_exact(path: &Path, expected: u64) -> Result<Vec<u8>, AuxControlStoreError> {
    let file = fs::File::open(path).map_err(|source| VerificationError::Io {
        path: path.to_owned(),
        source,
    })?;
    let metadata = file.metadata().map_err(|source| VerificationError::Io {
        path: path.to_owned(),
        source,
    })?;
    if !metadata.is_file() {
        return Err(VerificationError::SpecialFile(path.to_owned()).into());
    }
    if metadata.len() != expected {
        return Err(AuxControlCodecError::WrongLength {
            expected: expected as usize,
            actual: usize::try_from(metadata.len()).unwrap_or(usize::MAX),
        }
        .into());
    }
    let mut bytes = Vec::with_capacity(expected as usize);
    file.take(expected + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| VerificationError::Io {
            path: path.to_owned(),
            source,
        })?;
    if bytes.len() as u64 != expected {
        return Err(AuxControlCodecError::WrongLength {
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
    use crate::aux_control_import::tests::constants;
    use crate::aux_control_import::{AuxControlImportFile, AuxControlImportShape};
    use crate::aux_control_profile::tests::profile;
    use cmr_formats::pe_aux_control_initializer::encode_aux_control_constants;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "cmr-aux-store-{}-{}",
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

    fn prepared() -> (Temp, ReviewedAuxControlProfile, AuxControlConstantWords) {
        let temp = Temp::new();
        let root = temp.0.join("store");
        fs::create_dir(&root).unwrap();
        let executable = b"synthetic";
        let reviewed = ReviewedAuxControlProfile::from_json(
            &serde_json::to_vec(&profile(executable.len() as u64, sha256(executable))).unwrap(),
        )
        .unwrap();
        let constants = constants();
        let data = encode_aux_control_constants(&constants);
        fs::write(root.join(AUX_CONTROL_DATA_FILE), data).unwrap();
        let summary = AuxControlImportSummary {
            schema_version: AUX_CONTROL_IMPORT_SCHEMA_VERSION,
            profile: reviewed.profile().id.clone(),
            profile_sha256: reviewed.sha256().into(),
            source: AuxControlImportFile {
                path: "Rally.exe".into(),
                size: executable.len() as u64,
                sha256: sha256(executable),
            },
            output: AuxControlImportFile {
                path: AUX_CONTROL_DATA_FILE.into(),
                size: AUX_CONTROL_ENCODED_LEN as u64,
                sha256: sha256(&data),
            },
            shape: AuxControlImportShape {
                format_version: AUX_CONTROL_FORMAT_VERSION,
                common_word_count: AUX_CONTROL_COMMON_WORD_COUNT as u32,
                multiplier_count: AUX_CONTROL_MULTIPLIER_COUNT as u32,
                policy_count: AUX_CONTROL_POLICY_COUNT as u32,
                policy_word_count: AUX_CONTROL_POLICY_WORD_COUNT as u32,
                policy_divisor_count: AUX_CONTROL_POLICY_DIVISOR_COUNT as u32,
                encoding: AuxControlEncoding::CmrAuxcV1,
            },
        };
        fs::write(
            root.join(AUX_CONTROL_STATE_FILE),
            serde_json::to_vec(&summary).unwrap(),
        )
        .unwrap();
        (temp, reviewed, constants)
    }

    fn assert_state_rejected(
        expected_field: &'static str,
        mutate: impl FnOnce(&mut AuxControlImportSummary),
    ) {
        let (temp, reviewed, _) = prepared();
        let root = temp.0.join("store");
        let state_path = root.join(AUX_CONTROL_STATE_FILE);
        let mut state: AuxControlImportSummary =
            serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
        mutate(&mut state);
        fs::write(&state_path, serde_json::to_vec(&state).unwrap()).unwrap();
        assert!(matches!(
            AuxControlConstantsStore::open(&root, &reviewed),
            Err(AuxControlStoreError::InvalidState { field }) if field == expected_field
        ));
    }

    #[test]
    fn opens_owned_constants_and_survives_backing_removal() {
        let (temp, reviewed, expected) = prepared();
        let root = temp.0.join("store");
        let store = AuxControlConstantsStore::open(&root, &reviewed).unwrap();
        fs::remove_dir_all(&root).unwrap();
        assert_eq!(store.constants(), &expected);
        assert!(!format!("{store:?}").contains(temp.0.to_str().unwrap()));
    }

    #[test]
    fn rejects_mutation_truncation_trailing_and_bad_codec_header() {
        for change in [0_i8, -1, 1] {
            let (temp, reviewed, _) = prepared();
            let root = temp.0.join("store");
            let path = root.join(AUX_CONTROL_DATA_FILE);
            let mut data = fs::read(&path).unwrap();
            match change {
                0 => data[40] ^= 1,
                -1 => {
                    data.pop();
                }
                1 => data.push(0),
                _ => unreachable!(),
            }
            fs::write(&path, data).unwrap();
            assert!(AuxControlConstantsStore::open(&root, &reviewed).is_err());
        }

        let (temp, reviewed, _) = prepared();
        let root = temp.0.join("store");
        let path = root.join(AUX_CONTROL_DATA_FILE);
        let mut data = fs::read(&path).unwrap();
        data[0] ^= 1;
        let state_path = root.join(AUX_CONTROL_STATE_FILE);
        let mut state: AuxControlImportSummary =
            serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
        state.output.sha256 = sha256(&data);
        fs::write(&path, data).unwrap();
        fs::write(&state_path, serde_json::to_vec(&state).unwrap()).unwrap();
        assert!(matches!(
            AuxControlConstantsStore::open(&root, &reviewed),
            Err(AuxControlStoreError::Data(AuxControlCodecError::WrongMagic))
        ));
    }

    #[test]
    fn rejects_every_state_identity_shape_and_unknown_field() {
        assert_state_rejected("schema_version", |s| s.schema_version += 1);
        assert_state_rejected("profile", |s| s.profile.push('x'));
        assert_state_rejected("profile_sha256", |s| s.profile_sha256.push('0'));
        assert_state_rejected("source.path", |s| s.source.path.push('x'));
        assert_state_rejected("source.size", |s| s.source.size += 1);
        assert_state_rejected("source.sha256", |s| s.source.sha256.push('0'));
        assert_state_rejected("output.path", |s| s.output.path.push('x'));
        assert_state_rejected("output.size", |s| s.output.size += 1);
        assert_state_rejected("output.sha256", |s| s.output.sha256.push('0'));
        assert_state_rejected("shape.format_version", |s| s.shape.format_version += 1);
        assert_state_rejected("shape.common_word_count", |s| {
            s.shape.common_word_count += 1
        });
        assert_state_rejected("shape.multiplier_count", |s| s.shape.multiplier_count += 1);
        assert_state_rejected("shape.policy_count", |s| s.shape.policy_count += 1);
        assert_state_rejected("shape.policy_word_count", |s| {
            s.shape.policy_word_count += 1
        });
        assert_state_rejected("shape.policy_divisor_count", |s| {
            s.shape.policy_divisor_count += 1
        });

        for pointer in ["", "/source", "/output", "/shape"] {
            let (temp, reviewed, _) = prepared();
            let root = temp.0.join("store");
            let state_path = root.join(AUX_CONTROL_STATE_FILE);
            let mut state: serde_json::Value =
                serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
            state.pointer_mut(pointer).unwrap()["unknown"] = serde_json::json!(1);
            fs::write(&state_path, serde_json::to_vec(&state).unwrap()).unwrap();
            assert!(matches!(
                AuxControlConstantsStore::open(&root, &reviewed),
                Err(AuxControlStoreError::StateJson(_))
            ));
        }
    }

    #[test]
    fn rejects_malformed_oversized_and_non_regular_state_or_data() {
        let (temp, reviewed, _) = prepared();
        let root = temp.0.join("store");
        fs::write(root.join(AUX_CONTROL_STATE_FILE), b"not json").unwrap();
        assert!(matches!(
            AuxControlConstantsStore::open(&root, &reviewed),
            Err(AuxControlStoreError::StateJson(_))
        ));

        let (temp, reviewed, _) = prepared();
        let root = temp.0.join("store");
        fs::write(
            root.join(AUX_CONTROL_STATE_FILE),
            vec![b' '; MAX_AUX_CONTROL_STATE_BYTES as usize + 1],
        )
        .unwrap();
        assert!(matches!(
            AuxControlConstantsStore::open(&root, &reviewed),
            Err(AuxControlStoreError::StateTooLarge(_))
        ));

        for name in [AUX_CONTROL_STATE_FILE, AUX_CONTROL_DATA_FILE] {
            let (temp, reviewed, _) = prepared();
            let root = temp.0.join("store");
            let path = root.join(name);
            fs::remove_file(&path).unwrap();
            fs::create_dir(&path).unwrap();
            assert!(AuxControlConstantsStore::open(&root, &reviewed).is_err());
        }
    }

    #[cfg(unix)]
    #[test]
    fn rejects_linked_or_special_root_state_and_data() {
        use std::os::unix::fs::symlink;
        use std::os::unix::net::UnixListener;
        let (temp, reviewed, _) = prepared();
        let root = temp.0.join("store");
        let linked_root = temp.0.join("linked-root");
        symlink(&root, &linked_root).unwrap();
        assert!(AuxControlConstantsStore::open(&linked_root, &reviewed).is_err());

        for name in [AUX_CONTROL_STATE_FILE, AUX_CONTROL_DATA_FILE] {
            let (temp, reviewed, _) = prepared();
            let root = temp.0.join("store");
            let path = root.join(name);
            let actual = root.join(format!("actual-{name}"));
            fs::rename(&path, &actual).unwrap();
            symlink(&actual, &path).unwrap();
            assert!(AuxControlConstantsStore::open(&root, &reviewed).is_err());
        }

        for name in [AUX_CONTROL_STATE_FILE, AUX_CONTROL_DATA_FILE] {
            let (temp, reviewed, _) = prepared();
            let root = temp.0.join("store");
            let path = root.join(name);
            fs::remove_file(&path).unwrap();
            let _listener = UnixListener::bind(&path).unwrap();
            assert!(matches!(
                AuxControlConstantsStore::open(&root, &reviewed),
                Err(AuxControlStoreError::Verification(
                    VerificationError::SpecialFile(actual)
                )) if actual == path
            ));
        }
    }
}
