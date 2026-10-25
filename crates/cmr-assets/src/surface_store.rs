use std::fmt;
use std::fs;
use std::io::Read;
use std::path::Path;

use cmr_formats::pe_surface::{
    SURFACE_COEFFICIENT_COUNT, SurfaceCoefficientFamilies, SurfaceCoefficientFamily,
};

use crate::verify::{require_real_directory, sha256};
use crate::{
    ReviewedSurfaceProfile, SURFACE_DATA_FILE, SURFACE_IMPORT_SCHEMA_VERSION, SURFACE_STATE_FILE,
    SurfaceImportSummary, VerificationError,
};

const MAX_SURFACE_STATE_BYTES: u64 = 64 * 1024;
const SURFACE_FAMILY_COUNT: usize = 2;
const SURFACE_DATA_BYTES: usize = SURFACE_FAMILY_COUNT * SURFACE_COEFFICIENT_COUNT * 4;

/// Structural failure while decoding the project-owned extracted coefficient file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SurfaceDataError {
    Length { expected: usize, actual: usize },
}

impl fmt::Display for SurfaceDataError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Length { expected, actual } => write!(
                formatter,
                "surface coefficient data has {actual} bytes; expected exactly {expected}"
            ),
        }
    }
}

impl std::error::Error for SurfaceDataError {}

/// Decode the project-owned two-family extraction format.
///
/// The input must contain exactly two consecutive 120-word signed
/// little-endian families. It is not an original game file format.
pub fn decode_surface_table_data(
    input: &[u8],
) -> Result<SurfaceCoefficientFamilies, SurfaceDataError> {
    if input.len() != SURFACE_DATA_BYTES {
        return Err(SurfaceDataError::Length {
            expected: SURFACE_DATA_BYTES,
            actual: input.len(),
        });
    }
    let mut first = [0; SURFACE_COEFFICIENT_COUNT];
    let mut second = [0; SURFACE_COEFFICIENT_COUNT];
    for (index, value) in first.iter_mut().chain(&mut second).enumerate() {
        let offset = index * 4;
        *value = i32::from_le_bytes(
            input[offset..offset + 4]
                .try_into()
                .expect("four-byte word"),
        );
    }
    Ok(SurfaceCoefficientFamilies {
        first: SurfaceCoefficientFamily { values: first },
        second: SurfaceCoefficientFamily { values: second },
    })
}

/// A verified, immutable view of one completed local surface-table extraction.
///
/// The store retains only the decoded coefficient values: it does not retain
/// the supplied root or either file path. `open` expects a stable,
/// caller-owned local runtime-data directory for the duration of the call. It
/// rejects symlinks that are present when the root, state, and data paths are
/// checked and verifies the opened files and data digest, but does not claim
/// race-free resolution against concurrent adversarial filesystem mutation.
#[derive(Debug)]
pub struct SurfaceTableStore {
    families: SurfaceCoefficientFamilies,
}

#[derive(Debug)]
pub enum SurfaceTableStoreError {
    Verification(VerificationError),
    StateTooLarge(u64),
    StateJson(serde_json::Error),
    InvalidState { field: &'static str },
    Data(SurfaceDataError),
}

impl fmt::Display for SurfaceTableStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Verification(error) => {
                write!(formatter, "surface store verification failed: {error}")
            }
            Self::StateTooLarge(size) => write!(
                formatter,
                "surface import state is {size} bytes; limit is {MAX_SURFACE_STATE_BYTES}"
            ),
            Self::StateJson(error) => write!(formatter, "invalid surface import state: {error}"),
            Self::InvalidState { field } => {
                write!(
                    formatter,
                    "surface import state does not match profile field {field}"
                )
            }
            Self::Data(error) => write!(formatter, "invalid surface coefficient data: {error}"),
        }
    }
}

impl std::error::Error for SurfaceTableStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Verification(error) => Some(error),
            Self::StateJson(error) => Some(error),
            Self::Data(error) => Some(error),
            _ => None,
        }
    }
}

impl From<VerificationError> for SurfaceTableStoreError {
    fn from(error: VerificationError) -> Self {
        Self::Verification(error)
    }
}

impl From<SurfaceDataError> for SurfaceTableStoreError {
    fn from(error: SurfaceDataError) -> Self {
        Self::Data(error)
    }
}

impl SurfaceTableStore {
    /// Open and verify a completed extraction before exposing its values.
    ///
    /// The caller must keep `root` stable for the duration of this call. This
    /// method rejects persistent symlinks and replacements; it is not a
    /// race-free filesystem sandbox for a directory being mutated by an
    /// adversary concurrently with `open`.
    pub fn open(
        root: &Path,
        reviewed: &ReviewedSurfaceProfile,
    ) -> Result<Self, SurfaceTableStoreError> {
        require_real_directory(root)?;
        let state_path = root.join(SURFACE_STATE_FILE);
        reject_link_or_special(&state_path)?;
        let state_bytes = read_bounded(&state_path, MAX_SURFACE_STATE_BYTES)?;
        let state: SurfaceImportSummary =
            serde_json::from_slice(&state_bytes).map_err(SurfaceTableStoreError::StateJson)?;
        validate_state(&state, reviewed)?;

        let data_path = root.join(SURFACE_DATA_FILE);
        reject_link_or_special(&data_path)?;
        let data = read_exact(&data_path, SURFACE_DATA_BYTES as u64)?;
        let actual_hash = sha256(&data);
        if actual_hash != state.output.sha256 {
            return Err(SurfaceTableStoreError::InvalidState {
                field: "output.sha256",
            });
        }
        let families = decode_surface_table_data(&data)?;
        Ok(Self { families })
    }

    pub fn families(&self) -> &SurfaceCoefficientFamilies {
        &self.families
    }

    pub fn family(&self, index: usize) -> Option<&[i32; SURFACE_COEFFICIENT_COUNT]> {
        match index {
            0 => Some(&self.families.first.values),
            1 => Some(&self.families.second.values),
            _ => None,
        }
    }
}

fn validate_state(
    state: &SurfaceImportSummary,
    reviewed: &ReviewedSurfaceProfile,
) -> Result<(), SurfaceTableStoreError> {
    let profile = reviewed.profile();
    let checks = [
        (
            state.schema_version == SURFACE_IMPORT_SCHEMA_VERSION,
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
        (state.output.path == SURFACE_DATA_FILE, "output.path"),
        (
            state.output.size == SURFACE_DATA_BYTES as u64,
            "output.size",
        ),
    ];
    for (matches, field) in checks {
        if !matches {
            return Err(SurfaceTableStoreError::InvalidState { field });
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

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, SurfaceTableStoreError> {
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
        return Err(SurfaceTableStoreError::StateTooLarge(metadata.len()));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| VerificationError::Io {
            path: path.to_owned(),
            source,
        })?;
    if bytes.len() as u64 > limit {
        return Err(SurfaceTableStoreError::StateTooLarge(bytes.len() as u64));
    }
    Ok(bytes)
}

fn read_exact(path: &Path, expected: u64) -> Result<Vec<u8>, SurfaceTableStoreError> {
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
        return Err(SurfaceTableStoreError::Data(SurfaceDataError::Length {
            expected: expected as usize,
            actual: usize::try_from(metadata.len()).unwrap_or(usize::MAX),
        }));
    }
    let mut bytes = Vec::with_capacity(expected as usize);
    file.take(expected + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| VerificationError::Io {
            path: path.to_owned(),
            source,
        })?;
    if bytes.len() as u64 != expected {
        return Err(SurfaceTableStoreError::Data(SurfaceDataError::Length {
            expected: expected as usize,
            actual: bytes.len(),
        }));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verify::sha256;
    use crate::{
        ReviewedSurfaceProfile, SurfaceExecutableProfile, SurfaceTableEncoding,
        SurfaceTableProfile, SurfaceTableSpec, extract_surface_tables,
    };
    use cmr_formats::pe_surface::OFFICIAL_EU_SURFACE_FAMILY_VA;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    type StateMutation = (&'static str, fn(&mut SurfaceImportSummary));

    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct Temp(PathBuf);

    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "cmr-surface-store-{}-{}",
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

    fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn synthetic_pe() -> Vec<u8> {
        let mut bytes = vec![0; 0xa00];
        let pe = 0x80;
        let optional = pe + 24;
        let sections = optional + 0xe0;
        let raw = 0x200;
        let target = raw + 0x2cc;
        bytes[..2].copy_from_slice(b"MZ");
        put_u32(&mut bytes, 0x3c, pe as u32);
        bytes[pe..pe + 4].copy_from_slice(b"PE\0\0");
        put_u16(&mut bytes, pe + 4, 0x14c);
        put_u16(&mut bytes, pe + 6, 1);
        put_u16(&mut bytes, pe + 20, 0xe0);
        put_u16(&mut bytes, optional, 0x10b);
        put_u32(&mut bytes, optional + 28, 0x400000);
        put_u32(&mut bytes, sections + 8, 0x800);
        put_u32(&mut bytes, sections + 12, 0xf7000);
        put_u32(&mut bytes, sections + 16, 0x800);
        put_u32(&mut bytes, sections + 20, raw as u32);
        for index in 0..240 {
            let value = match index {
                0 => i32::MIN,
                1 => i32::MAX,
                _ => index as i32 - 120,
            };
            put_u32(&mut bytes, target + index * 4, value as u32);
        }
        bytes
    }

    fn reviewed_document(bytes: &[u8], id: &str, pretty: bool) -> ReviewedSurfaceProfile {
        let profile = SurfaceTableProfile {
            schema_version: 1,
            id: id.to_owned(),
            executable: SurfaceExecutableProfile {
                path: "Rally.exe".to_owned(),
                size: bytes.len() as u64,
                sha256: sha256(bytes),
            },
            table: SurfaceTableSpec {
                virtual_address: OFFICIAL_EU_SURFACE_FAMILY_VA,
                families: 2,
                entries_per_family: 120,
                entry_encoding: SurfaceTableEncoding::SignedI32LittleEndian,
            },
        };
        let document = if pretty {
            serde_json::to_vec_pretty(&profile).unwrap()
        } else {
            serde_json::to_vec(&profile).unwrap()
        };
        ReviewedSurfaceProfile::from_json(&document).unwrap()
    }

    fn extracted(temp: &Temp) -> (PathBuf, Vec<u8>, ReviewedSurfaceProfile) {
        let source = temp.0.join("source");
        fs::create_dir(&source).unwrap();
        let bytes = synthetic_pe();
        fs::write(source.join("Rally.exe"), &bytes).unwrap();
        let reviewed = reviewed_document(&bytes, "synthetic", false);
        let output = temp.0.join("surface-store");
        extract_surface_tables(&source, &output, &reviewed).unwrap();
        (output, bytes, reviewed)
    }

    fn write_state(root: &Path, state: &SurfaceImportSummary) {
        fs::write(
            root.join(SURFACE_STATE_FILE),
            serde_json::to_vec_pretty(state).unwrap(),
        )
        .unwrap();
    }

    fn read_state(root: &Path) -> SurfaceImportSummary {
        serde_json::from_slice(&fs::read(root.join(SURFACE_STATE_FILE)).unwrap()).unwrap()
    }

    #[test]
    fn pure_decoder_requires_exact_length_and_preserves_signed_words() {
        let mut data = vec![0; SURFACE_DATA_BYTES];
        data[..4].copy_from_slice(&i32::MIN.to_le_bytes());
        data[4..8].copy_from_slice(&i32::MAX.to_le_bytes());
        data[SURFACE_COEFFICIENT_COUNT * 4..SURFACE_COEFFICIENT_COUNT * 4 + 4]
            .copy_from_slice(&(-7i32).to_le_bytes());
        let decoded = decode_surface_table_data(&data).unwrap();
        assert_eq!(decoded.first.values[0], i32::MIN);
        assert_eq!(decoded.first.values[1], i32::MAX);
        assert_eq!(decoded.second.values[0], -7);
        assert_eq!(
            decode_surface_table_data(&data[..data.len() - 1]),
            Err(SurfaceDataError::Length {
                expected: SURFACE_DATA_BYTES,
                actual: SURFACE_DATA_BYTES - 1,
            })
        );
        data.push(0);
        assert!(matches!(
            decode_surface_table_data(&data),
            Err(SurfaceDataError::Length { actual, .. }) if actual == SURFACE_DATA_BYTES + 1
        ));
    }

    #[test]
    fn opens_extractor_output_and_exposes_two_immutable_families() {
        let temp = Temp::new();
        let (root, _, reviewed) = extracted(&temp);
        let store = SurfaceTableStore::open(&root, &reviewed).unwrap();
        assert_eq!(store.family(0).unwrap()[0], i32::MIN);
        assert_eq!(store.family(0).unwrap()[1], i32::MAX);
        assert_eq!(store.family(0).unwrap()[119], -1);
        assert_eq!(store.family(1).unwrap()[0], 0);
        assert_eq!(store.family(1).unwrap()[119], 119);
        assert!(store.family(2).is_none());
        assert_eq!(store.families().first.values, *store.family(0).unwrap());
        assert_eq!(store.families().second.values, *store.family(1).unwrap());

        let debug = format!("{store:?}");
        assert_eq!(
            debug,
            format!("SurfaceTableStore {{ families: {:?} }}", store.families())
        );
        assert!(!debug.contains(&root.display().to_string()));
    }

    #[test]
    fn rejects_malformed_state_json() {
        let temp = Temp::new();
        let (root, _, reviewed) = extracted(&temp);
        fs::write(root.join(SURFACE_STATE_FILE), b"{not-json").unwrap();
        assert!(matches!(
            SurfaceTableStore::open(&root, &reviewed),
            Err(SurfaceTableStoreError::StateJson(_))
        ));
    }

    #[test]
    fn rejects_every_profile_and_state_identity_mismatch() {
        let temp = Temp::new();
        let (root, bytes, reviewed) = extracted(&temp);
        let original = read_state(&root);

        let cases: [StateMutation; 8] = [
            ("schema_version", |state| state.schema_version += 1),
            ("profile", |state| state.profile.push('x')),
            ("profile_sha256", |state| {
                state.profile_sha256 = "00".repeat(32)
            }),
            ("source.path", |state| {
                state.source.path = "Other.exe".to_owned()
            }),
            ("source.size", |state| state.source.size += 1),
            ("source.sha256", |state| {
                state.source.sha256 = "11".repeat(32)
            }),
            ("output.path", |state| {
                state.output.path = "other.bin".to_owned()
            }),
            ("output.sha256", |state| {
                state.output.sha256 = "22".repeat(32)
            }),
        ];
        for (field, mutate) in cases {
            let mut state = original.clone();
            mutate(&mut state);
            write_state(&root, &state);
            assert!(matches!(
                SurfaceTableStore::open(&root, &reviewed),
                Err(SurfaceTableStoreError::InvalidState { field: actual }) if actual == field
            ));
        }

        write_state(&root, &original);
        let same_profile_different_document = reviewed_document(&bytes, "synthetic", true);
        assert!(matches!(
            SurfaceTableStore::open(&root, &same_profile_different_document),
            Err(SurfaceTableStoreError::InvalidState {
                field: "profile_sha256"
            })
        ));
        let other_profile = reviewed_document(&bytes, "other", false);
        assert!(matches!(
            SurfaceTableStore::open(&root, &other_profile),
            Err(SurfaceTableStoreError::InvalidState { field: "profile" })
        ));
    }

    #[test]
    fn rejects_declared_output_size_and_rehashes_data_on_every_open() {
        let temp = Temp::new();
        let (root, _, reviewed) = extracted(&temp);
        let original = read_state(&root);
        let mut state = original.clone();
        state.output.size -= 1;
        write_state(&root, &state);
        assert!(matches!(
            SurfaceTableStore::open(&root, &reviewed),
            Err(SurfaceTableStoreError::InvalidState {
                field: "output.size"
            })
        ));

        write_state(&root, &original);
        let data_path = root.join(SURFACE_DATA_FILE);
        let mut data = fs::read(&data_path).unwrap();
        data[0] ^= 1;
        fs::write(&data_path, data).unwrap();
        assert!(matches!(
            SurfaceTableStore::open(&root, &reviewed),
            Err(SurfaceTableStoreError::InvalidState {
                field: "output.sha256"
            })
        ));
    }

    #[test]
    fn rejects_truncated_and_trailing_data_before_decoding() {
        for trailing in [false, true] {
            let temp = Temp::new();
            let (root, _, reviewed) = extracted(&temp);
            let data_path = root.join(SURFACE_DATA_FILE);
            let mut data = fs::read(&data_path).unwrap();
            if trailing {
                data.push(0);
            } else {
                data.pop();
            }
            fs::write(data_path, data).unwrap();
            assert!(matches!(
                SurfaceTableStore::open(&root, &reviewed),
                Err(SurfaceTableStoreError::Data(
                    SurfaceDataError::Length { .. }
                ))
            ));
        }
    }

    #[test]
    fn rejects_oversized_state_before_json_parsing() {
        let temp = Temp::new();
        let (root, _, reviewed) = extracted(&temp);
        fs::write(
            root.join(SURFACE_STATE_FILE),
            vec![b' '; MAX_SURFACE_STATE_BYTES as usize + 1],
        )
        .unwrap();
        assert!(matches!(
            SurfaceTableStore::open(&root, &reviewed),
            Err(SurfaceTableStoreError::StateTooLarge(size))
                if size == MAX_SURFACE_STATE_BYTES + 1
        ));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_state_data_and_root_replacements() {
        use std::os::unix::fs::symlink;

        let temp = Temp::new();
        let (root, _, reviewed) = extracted(&temp);
        let real_state = temp.0.join("real-state.json");
        fs::rename(root.join(SURFACE_STATE_FILE), &real_state).unwrap();
        symlink(&real_state, root.join(SURFACE_STATE_FILE)).unwrap();
        assert!(matches!(
            SurfaceTableStore::open(&root, &reviewed),
            Err(SurfaceTableStoreError::Verification(
                VerificationError::Symlink(_)
            ))
        ));

        fs::remove_file(root.join(SURFACE_STATE_FILE)).unwrap();
        fs::rename(&real_state, root.join(SURFACE_STATE_FILE)).unwrap();
        let real_data = temp.0.join("real-data.bin");
        fs::rename(root.join(SURFACE_DATA_FILE), &real_data).unwrap();
        symlink(&real_data, root.join(SURFACE_DATA_FILE)).unwrap();
        assert!(matches!(
            SurfaceTableStore::open(&root, &reviewed),
            Err(SurfaceTableStoreError::Verification(
                VerificationError::Symlink(_)
            ))
        ));

        fs::remove_file(root.join(SURFACE_DATA_FILE)).unwrap();
        fs::rename(&real_data, root.join(SURFACE_DATA_FILE)).unwrap();
        let root_link = temp.0.join("root-link");
        symlink(&root, &root_link).unwrap();
        assert!(matches!(
            SurfaceTableStore::open(&root_link, &reviewed),
            Err(SurfaceTableStoreError::Verification(
                VerificationError::SourceNotDirectory(_)
            ))
        ));
    }
}
