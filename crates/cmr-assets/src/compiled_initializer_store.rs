use std::fmt;
use std::fs;
use std::io::Read;
use std::path::Path;

use cmr_formats::pe_compiled_initializer::{
    COMPILED_CASE_COUNT, COMPILED_CASE_ENCODED_LEN, COMPILED_CASE_FORMAT_VERSION,
    COMPILED_CONTROL_WORD_COUNT, COMPILED_STATE_WORD_COUNT, CompiledCaseCodecError,
    CompiledCaseWords, decode_compiled_cases,
};

use crate::VerificationError;
use crate::compiled_initializer_import::{
    COMPILED_INITIALIZER_DATA_FILE, COMPILED_INITIALIZER_IMPORT_SCHEMA_VERSION,
    COMPILED_INITIALIZER_STATE_FILE, CompiledInitializerImportSummary,
};
use crate::compiled_initializer_profile::{
    CompiledInitializerEncoding, ReviewedCompiledInitializerProfile,
};
use crate::verify::{require_real_directory, sha256};

const MAX_COMPILED_INITIALIZER_STATE_BYTES: u64 = 64 * 1024;

#[derive(Debug)]
pub struct CompiledInitializerStore {
    cases: [CompiledCaseWords; COMPILED_CASE_COUNT],
}

#[derive(Debug)]
pub enum CompiledInitializerStoreError {
    Verification(VerificationError),
    StateTooLarge(u64),
    StateJson(serde_json::Error),
    InvalidState { field: &'static str },
    Data(CompiledCaseCodecError),
}

impl fmt::Display for CompiledInitializerStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Verification(error) => write!(
                formatter,
                "compiled initializer store verification failed: {error}"
            ),
            Self::StateTooLarge(size) => write!(
                formatter,
                "compiled initializer state is {size} bytes; limit is {MAX_COMPILED_INITIALIZER_STATE_BYTES}"
            ),
            Self::StateJson(error) => {
                write!(formatter, "invalid compiled initializer state: {error}")
            }
            Self::InvalidState { field } => write!(
                formatter,
                "compiled initializer state does not match profile field {field}"
            ),
            Self::Data(error) => write!(formatter, "invalid compiled initializer data: {error}"),
        }
    }
}

impl std::error::Error for CompiledInitializerStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Verification(error) => Some(error),
            Self::StateJson(error) => Some(error),
            Self::Data(error) => Some(error),
            Self::StateTooLarge(_) | Self::InvalidState { .. } => None,
        }
    }
}

impl From<VerificationError> for CompiledInitializerStoreError {
    fn from(error: VerificationError) -> Self {
        Self::Verification(error)
    }
}

impl From<CompiledCaseCodecError> for CompiledInitializerStoreError {
    fn from(error: CompiledCaseCodecError) -> Self {
        Self::Data(error)
    }
}

impl CompiledInitializerStore {
    /// Open a completed extraction from a stable caller-owned directory.
    /// Persistent links at the root and fixed flat child paths are rejected;
    /// this is not a race-free sandbox against concurrent mutation.
    pub fn open(
        root: &Path,
        reviewed: &ReviewedCompiledInitializerProfile,
    ) -> Result<Self, CompiledInitializerStoreError> {
        require_real_directory(root)?;
        let state_path = root.join(COMPILED_INITIALIZER_STATE_FILE);
        reject_link_or_special(&state_path)?;
        let state_bytes = read_bounded(&state_path, MAX_COMPILED_INITIALIZER_STATE_BYTES)?;
        let state: CompiledInitializerImportSummary = serde_json::from_slice(&state_bytes)
            .map_err(CompiledInitializerStoreError::StateJson)?;
        validate_state(&state, reviewed)?;

        let data_path = root.join(COMPILED_INITIALIZER_DATA_FILE);
        reject_link_or_special(&data_path)?;
        let data = read_exact(&data_path, COMPILED_CASE_ENCODED_LEN as u64)?;
        if sha256(&data) != state.output.sha256 {
            return Err(CompiledInitializerStoreError::InvalidState {
                field: "output.sha256",
            });
        }
        let cases = decode_compiled_cases(&data)?;
        Ok(Self { cases })
    }

    pub fn cases(&self) -> &[CompiledCaseWords; COMPILED_CASE_COUNT] {
        &self.cases
    }

    pub fn case(&self, selector: usize) -> Option<&CompiledCaseWords> {
        self.cases.get(selector)
    }
}

fn validate_state(
    state: &CompiledInitializerImportSummary,
    reviewed: &ReviewedCompiledInitializerProfile,
) -> Result<(), CompiledInitializerStoreError> {
    let profile = reviewed.profile();
    let checks = [
        (
            state.schema_version == COMPILED_INITIALIZER_IMPORT_SCHEMA_VERSION,
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
        (
            state.output.path == COMPILED_INITIALIZER_DATA_FILE,
            "output.path",
        ),
        (
            state.output.size == COMPILED_CASE_ENCODED_LEN as u64,
            "output.size",
        ),
        (
            state.shape.format_version == COMPILED_CASE_FORMAT_VERSION,
            "shape.format_version",
        ),
        (
            state.shape.selectors == COMPILED_CASE_COUNT as u32,
            "shape.selectors",
        ),
        (
            state.shape.state_words_per_selector == COMPILED_STATE_WORD_COUNT as u32,
            "shape.state_words_per_selector",
        ),
        (
            state.shape.control_words_per_selector == COMPILED_CONTROL_WORD_COUNT as u32,
            "shape.control_words_per_selector",
        ),
        (
            state.shape.encoding == CompiledInitializerEncoding::CmrCcaseV1,
            "shape.encoding",
        ),
    ];
    for (matches, field) in checks {
        if !matches {
            return Err(CompiledInitializerStoreError::InvalidState { field });
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

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, CompiledInitializerStoreError> {
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
        return Err(CompiledInitializerStoreError::StateTooLarge(metadata.len()));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| VerificationError::Io {
            path: path.to_owned(),
            source,
        })?;
    if bytes.len() as u64 > limit {
        return Err(CompiledInitializerStoreError::StateTooLarge(
            bytes.len() as u64
        ));
    }
    Ok(bytes)
}

fn read_exact(path: &Path, expected: u64) -> Result<Vec<u8>, CompiledInitializerStoreError> {
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
        return Err(CompiledCaseCodecError::WrongLength {
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
        return Err(CompiledCaseCodecError::WrongLength {
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
    use crate::compiled_initializer_import::{
        CompiledInitializerImportFile, CompiledInitializerImportShape,
    };
    use crate::compiled_initializer_profile::tests::profile;
    use cmr_formats::pe_compiled_initializer::encode_compiled_cases;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "cmr-compiled-store-{}-{}",
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

    fn cases() -> [CompiledCaseWords; COMPILED_CASE_COUNT] {
        let mut cases = std::array::from_fn(|selector| CompiledCaseWords {
            state_words: std::array::from_fn(|index| selector as i32 * 100 + index as i32),
            control_words: std::array::from_fn(|index| -(selector as i32 * 100 + index as i32)),
        });
        cases[0].state_words[0] = i32::MIN;
        cases[13].control_words[22] = i32::MAX;
        cases
    }

    fn prepared() -> (
        Temp,
        ReviewedCompiledInitializerProfile,
        [CompiledCaseWords; COMPILED_CASE_COUNT],
    ) {
        let temp = Temp::new();
        let root = temp.0.join("store");
        fs::create_dir(&root).unwrap();
        let executable = b"synthetic";
        let reviewed = ReviewedCompiledInitializerProfile::from_json(
            &serde_json::to_vec(&profile(executable.len() as u64, sha256(executable))).unwrap(),
        )
        .unwrap();
        let cases = cases();
        let data = encode_compiled_cases(&cases);
        fs::write(root.join(COMPILED_INITIALIZER_DATA_FILE), data).unwrap();
        let summary = CompiledInitializerImportSummary {
            schema_version: 1,
            profile: reviewed.profile().id.clone(),
            profile_sha256: reviewed.sha256().into(),
            source: CompiledInitializerImportFile {
                path: "Rally.exe".into(),
                size: executable.len() as u64,
                sha256: sha256(executable),
            },
            output: CompiledInitializerImportFile {
                path: COMPILED_INITIALIZER_DATA_FILE.into(),
                size: COMPILED_CASE_ENCODED_LEN as u64,
                sha256: sha256(&data),
            },
            shape: CompiledInitializerImportShape {
                format_version: 1,
                selectors: 14,
                state_words_per_selector: 8,
                control_words_per_selector: 23,
                encoding: CompiledInitializerEncoding::CmrCcaseV1,
            },
        };
        fs::write(
            root.join(COMPILED_INITIALIZER_STATE_FILE),
            serde_json::to_vec(&summary).unwrap(),
        )
        .unwrap();
        (temp, reviewed, cases)
    }

    fn assert_state_rejected(
        expected_field: &'static str,
        mutate: impl FnOnce(&mut CompiledInitializerImportSummary),
    ) {
        let (temp, reviewed, _) = prepared();
        let root = temp.0.join("store");
        let state_path = root.join(COMPILED_INITIALIZER_STATE_FILE);
        let mut state: CompiledInitializerImportSummary =
            serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
        mutate(&mut state);
        fs::write(&state_path, serde_json::to_vec(&state).unwrap()).unwrap();
        assert!(matches!(
            CompiledInitializerStore::open(&root, &reviewed),
            Err(CompiledInitializerStoreError::InvalidState { field })
                if field == expected_field
        ));
    }

    #[test]
    fn opens_all_owned_cases_and_preserves_signed_endpoints() {
        let (temp, reviewed, expected) = prepared();
        let store = CompiledInitializerStore::open(&temp.0.join("store"), &reviewed).unwrap();
        assert_eq!(store.cases(), &expected);
        assert_eq!(store.case(0).unwrap().state_words[0], i32::MIN);
        assert_eq!(store.case(13).unwrap().control_words[22], i32::MAX);
        assert!(store.case(14).is_none());
        assert!(!format!("{store:?}").contains(temp.0.to_str().unwrap()));
    }

    #[test]
    fn rehashes_data_and_rejects_state_identity_or_length_changes() {
        let (temp, reviewed, _) = prepared();
        let root = temp.0.join("store");
        let data_path = root.join(COMPILED_INITIALIZER_DATA_FILE);
        let mut data = fs::read(&data_path).unwrap();
        data[24] ^= 1;
        fs::write(&data_path, data).unwrap();
        assert!(matches!(
            CompiledInitializerStore::open(&root, &reviewed),
            Err(CompiledInitializerStoreError::InvalidState {
                field: "output.sha256"
            })
        ));

        let (temp, reviewed, _) = prepared();
        let root = temp.0.join("store");
        let data_path = root.join(COMPILED_INITIALIZER_DATA_FILE);
        let mut data = fs::read(&data_path).unwrap();
        data.push(0);
        fs::write(&data_path, data).unwrap();
        assert!(matches!(
            CompiledInitializerStore::open(&root, &reviewed),
            Err(CompiledInitializerStoreError::Data(
                CompiledCaseCodecError::WrongLength { .. }
            ))
        ));

        let (temp, reviewed, _) = prepared();
        let root = temp.0.join("store");
        let data_path = root.join(COMPILED_INITIALIZER_DATA_FILE);
        let mut data = fs::read(&data_path).unwrap();
        data.pop();
        fs::write(&data_path, data).unwrap();
        assert!(matches!(
            CompiledInitializerStore::open(&root, &reviewed),
            Err(CompiledInitializerStoreError::Data(
                CompiledCaseCodecError::WrongLength { .. }
            ))
        ));
    }

    #[test]
    fn rejects_every_declared_state_identity_and_shape_mismatch() {
        assert_state_rejected("schema_version", |state| state.schema_version += 1);
        assert_state_rejected("profile", |state| state.profile.push('x'));
        assert_state_rejected("profile_sha256", |state| state.profile_sha256.push('0'));
        assert_state_rejected("source.path", |state| state.source.path.push('x'));
        assert_state_rejected("source.size", |state| state.source.size += 1);
        assert_state_rejected("source.sha256", |state| state.source.sha256.push('0'));
        assert_state_rejected("output.path", |state| state.output.path.push('x'));
        assert_state_rejected("output.size", |state| state.output.size += 1);
        assert_state_rejected("output.sha256", |state| state.output.sha256.push('0'));
        assert_state_rejected("shape.format_version", |state| {
            state.shape.format_version += 1;
        });
        assert_state_rejected("shape.selectors", |state| state.shape.selectors -= 1);
        assert_state_rejected("shape.state_words_per_selector", |state| {
            state.shape.state_words_per_selector -= 1;
        });
        assert_state_rejected("shape.control_words_per_selector", |state| {
            state.shape.control_words_per_selector -= 1;
        });

        let (temp, reviewed, _) = prepared();
        let root = temp.0.join("store");
        let state_path = root.join(COMPILED_INITIALIZER_STATE_FILE);
        let mut state: serde_json::Value =
            serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
        state["shape"]["encoding"] = "unknown-encoding".into();
        fs::write(&state_path, serde_json::to_vec(&state).unwrap()).unwrap();
        assert!(matches!(
            CompiledInitializerStore::open(&root, &reviewed),
            Err(CompiledInitializerStoreError::StateJson(_))
        ));
    }

    #[test]
    fn rejects_malformed_and_oversized_state_before_exposing_data() {
        let (temp, reviewed, _) = prepared();
        let root = temp.0.join("store");
        fs::write(root.join(COMPILED_INITIALIZER_STATE_FILE), b"not json").unwrap();
        assert!(matches!(
            CompiledInitializerStore::open(&root, &reviewed),
            Err(CompiledInitializerStoreError::StateJson(_))
        ));

        let (temp, reviewed, _) = prepared();
        let root = temp.0.join("store");
        fs::write(
            root.join(COMPILED_INITIALIZER_STATE_FILE),
            vec![b' '; MAX_COMPILED_INITIALIZER_STATE_BYTES as usize + 1],
        )
        .unwrap();
        assert!(matches!(
            CompiledInitializerStore::open(&root, &reviewed),
            Err(CompiledInitializerStoreError::StateTooLarge(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_linked_root_state_and_data() {
        use std::os::unix::fs::symlink;
        let (temp, reviewed, _) = prepared();
        let root = temp.0.join("store");
        let linked_root = temp.0.join("linked-root");
        symlink(&root, &linked_root).unwrap();
        assert!(CompiledInitializerStore::open(&linked_root, &reviewed).is_err());

        for name in [
            COMPILED_INITIALIZER_STATE_FILE,
            COMPILED_INITIALIZER_DATA_FILE,
        ] {
            let (temp, reviewed, _) = prepared();
            let root = temp.0.join("store");
            let path = root.join(name);
            let actual = root.join(format!("actual-{name}"));
            fs::rename(&path, &actual).unwrap();
            symlink(&actual, &path).unwrap();
            assert!(CompiledInitializerStore::open(&root, &reviewed).is_err());
        }
    }

    #[test]
    fn rejects_non_regular_state_and_data() {
        for name in [
            COMPILED_INITIALIZER_STATE_FILE,
            COMPILED_INITIALIZER_DATA_FILE,
        ] {
            let (temp, reviewed, _) = prepared();
            let root = temp.0.join("store");
            let path = root.join(name);
            fs::remove_file(&path).unwrap();
            fs::create_dir(&path).unwrap();
            assert!(matches!(
                CompiledInitializerStore::open(&root, &reviewed),
                Err(CompiledInitializerStoreError::Verification(
                    VerificationError::SpecialFile(actual)
                )) if actual == path
            ));
        }
    }
}
