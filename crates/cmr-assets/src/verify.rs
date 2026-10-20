use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use cmr_formats::stored_asset::{DecodeLimits, StoredAsset, decode_stored_asset};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{AssetFile, AssetProfile, ProfileError, StorageEncoding};

const MAX_DECODED_ASSET_BYTES: usize = 512 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct VerificationSummary {
    pub files: usize,
    pub stored_bytes: u64,
    pub raw_files: usize,
    pub gzip_cmpr_files: usize,
}

#[derive(Debug)]
pub enum VerificationError {
    Profile(ProfileError),
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    SourceNotDirectory(PathBuf),
    Symlink(PathBuf),
    SpecialFile(PathBuf),
    NonUtf8Path(PathBuf),
    UnexpectedFile(String),
    MissingFile(String),
    SizeMismatch {
        path: String,
        expected: u64,
        actual: u64,
    },
    HashMismatch {
        path: String,
        expected: String,
        actual: String,
    },
    EncodingMismatch {
        path: String,
        expected: StorageEncoding,
        actual: StorageEncoding,
    },
    InvalidEncoding {
        path: String,
        reason: String,
    },
    CaseCollision {
        first: String,
        second: String,
    },
}

impl fmt::Display for VerificationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Profile(error) => write!(formatter, "invalid asset profile: {error}"),
            Self::Io { path, source } => write!(formatter, "{}: {source}", path.display()),
            Self::SourceNotDirectory(path) => {
                write!(
                    formatter,
                    "source is not a real directory: {}",
                    path.display()
                )
            }
            Self::Symlink(path) => write!(formatter, "refusing symbolic link: {}", path.display()),
            Self::SpecialFile(path) => {
                write!(formatter, "refusing special file: {}", path.display())
            }
            Self::NonUtf8Path(path) => write!(formatter, "path is not UTF-8: {}", path.display()),
            Self::UnexpectedFile(path) => write!(formatter, "unexpected source file: {path}"),
            Self::MissingFile(path) => write!(formatter, "missing source file: {path}"),
            Self::SizeMismatch {
                path,
                expected,
                actual,
            } => {
                write!(
                    formatter,
                    "size mismatch for {path}: expected {expected}, got {actual}"
                )
            }
            Self::HashMismatch {
                path,
                expected,
                actual,
            } => {
                write!(
                    formatter,
                    "SHA-256 mismatch for {path}: expected {expected}, got {actual}"
                )
            }
            Self::EncodingMismatch {
                path,
                expected,
                actual,
            } => {
                write!(
                    formatter,
                    "storage encoding mismatch for {path}: expected {expected:?}, got {actual:?}"
                )
            }
            Self::InvalidEncoding { path, reason } => {
                write!(formatter, "invalid stored encoding for {path}: {reason}")
            }
            Self::CaseCollision { first, second } => {
                write!(
                    formatter,
                    "source case-fold collision between {first:?} and {second:?}"
                )
            }
        }
    }
}

impl std::error::Error for VerificationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Profile(error) => Some(error),
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl From<ProfileError> for VerificationError {
    fn from(error: ProfileError) -> Self {
        Self::Profile(error)
    }
}

pub fn verify_source(
    source: &Path,
    profile: &AssetProfile,
) -> Result<VerificationSummary, VerificationError> {
    profile.validate()?;
    require_real_directory(source)?;

    let actual_paths = collect_root_files(source, &profile.roots)?;
    let expected_paths: BTreeSet<&str> = profile
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect();

    if let Some(path) = actual_paths
        .iter()
        .find(|path| !expected_paths.contains(path.as_str()))
    {
        return Err(VerificationError::UnexpectedFile(path.clone()));
    }
    if let Some(path) = expected_paths
        .iter()
        .find(|path| !actual_paths.contains(**path))
    {
        return Err(VerificationError::MissingFile((*path).to_owned()));
    }

    for sentinel in &profile.sentinels {
        verify_file(source, sentinel, false)?;
    }

    let mut summary = VerificationSummary {
        files: 0,
        stored_bytes: 0,
        raw_files: 0,
        gzip_cmpr_files: 0,
    };
    for expected in &profile.files {
        verify_file(source, expected, true)?;
        summary.files += 1;
        summary.stored_bytes += expected.size;
        match expected.encoding {
            StorageEncoding::Raw => summary.raw_files += 1,
            StorageEncoding::GzipCmpr => summary.gzip_cmpr_files += 1,
        }
    }

    Ok(summary)
}

pub(crate) fn require_real_directory(path: &Path) -> Result<(), VerificationError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| VerificationError::Io {
        path: path.to_owned(),
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(VerificationError::SourceNotDirectory(path.to_owned()));
    }
    Ok(())
}

fn collect_root_files(
    source: &Path,
    roots: &[String],
) -> Result<BTreeSet<String>, VerificationError> {
    let mut files = BTreeSet::new();
    let mut folded = BTreeMap::<String, String>::new();

    for root in roots {
        let root_path = join_portable(source, root);
        require_real_directory(&root_path)?;
        let mut pending = vec![root_path];
        while let Some(path) = pending.pop() {
            for entry in fs::read_dir(&path).map_err(|source| VerificationError::Io {
                path: path.clone(),
                source,
            })? {
                let entry = entry.map_err(|source| VerificationError::Io {
                    path: path.clone(),
                    source,
                })?;
                let child = entry.path();
                let metadata =
                    fs::symlink_metadata(&child).map_err(|source| VerificationError::Io {
                        path: child.clone(),
                        source,
                    })?;
                if metadata.file_type().is_symlink() {
                    return Err(VerificationError::Symlink(child));
                }
                let relative = portable_relative(source, &child)?;
                let key = relative.to_ascii_lowercase();
                if let Some(first) = folded.insert(key, relative.clone())
                    && first != relative
                {
                    return Err(VerificationError::CaseCollision {
                        first,
                        second: relative,
                    });
                }
                if metadata.is_dir() {
                    pending.push(child);
                    continue;
                }
                if !metadata.is_file() {
                    return Err(VerificationError::SpecialFile(child));
                }
                files.insert(relative);
            }
        }
    }
    Ok(files)
}

pub(crate) fn verify_file(
    source: &Path,
    expected: &AssetFile,
    validate_encoding: bool,
) -> Result<Vec<u8>, VerificationError> {
    let path = join_portable(source, &expected.path);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(VerificationError::MissingFile(expected.path.clone()));
        }
        Err(source) => return Err(VerificationError::Io { path, source }),
    };
    if metadata.file_type().is_symlink() {
        return Err(VerificationError::Symlink(path));
    }
    if !metadata.is_file() {
        return Err(VerificationError::SpecialFile(path));
    }
    if metadata.len() != expected.size {
        return Err(VerificationError::SizeMismatch {
            path: expected.path.clone(),
            expected: expected.size,
            actual: metadata.len(),
        });
    }

    let bytes = fs::read(&path).map_err(|source| VerificationError::Io {
        path: path.clone(),
        source,
    })?;
    let actual_hash = sha256(&bytes);
    if actual_hash != expected.sha256 {
        return Err(VerificationError::HashMismatch {
            path: expected.path.clone(),
            expected: expected.sha256.clone(),
            actual: actual_hash,
        });
    }

    if !validate_encoding {
        return Ok(bytes);
    }
    let limits = DecodeLimits {
        max_stored_bytes: bytes.len(),
        max_payload_bytes: MAX_DECODED_ASSET_BYTES,
    };
    let actual_encoding = match decode_stored_asset(&bytes, limits) {
        Ok(StoredAsset::Raw(_)) => StorageEncoding::Raw,
        Ok(StoredAsset::Cmpr(_)) => StorageEncoding::GzipCmpr,
        Err(error) => {
            return Err(VerificationError::InvalidEncoding {
                path: expected.path.clone(),
                reason: error.to_string(),
            });
        }
    };
    if actual_encoding != expected.encoding {
        return Err(VerificationError::EncodingMismatch {
            path: expected.path.clone(),
            expected: expected.encoding,
            actual: actual_encoding,
        });
    }
    Ok(bytes)
}

pub(crate) fn join_portable(root: &Path, relative: &str) -> PathBuf {
    relative
        .split('/')
        .fold(root.to_owned(), |path, part| path.join(part))
}

pub(crate) fn portable_relative(root: &Path, path: &Path) -> Result<String, VerificationError> {
    let relative = path
        .strip_prefix(root)
        .expect("walked path stays under source");
    let mut parts = Vec::new();
    for component in relative.components() {
        let part = component
            .as_os_str()
            .to_str()
            .ok_or_else(|| VerificationError::NonUtf8Path(path.to_owned()))?;
        parts.push(part);
    }
    Ok(parts.join("/"))
}

pub(crate) fn sha256(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(encoded, "{byte:02x}").expect("writing to a String cannot fail");
    }
    encoded
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    struct TestTree(PathBuf);

    impl TestTree {
        fn new() -> Self {
            let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!("cmr-assets-{}-{id}", std::process::id()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn write(&self, relative: &str, bytes: &[u8]) {
            let path = join_portable(&self.0, relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }
    }

    impl Drop for TestTree {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn asset(path: &str, bytes: &[u8]) -> AssetFile {
        AssetFile {
            path: path.to_owned(),
            size: bytes.len() as u64,
            sha256: sha256(bytes),
            encoding: StorageEncoding::Raw,
        }
    }

    fn fixture(tree: &TestTree) -> AssetProfile {
        tree.write("Rally.exe", b"sentinel");
        tree.write("Game/a.bin", b"a");
        tree.write("New_Fe/b.bin", b"bb");
        AssetProfile {
            schema_version: 1,
            id: "synthetic".to_owned(),
            roots: vec!["Game".to_owned(), "New_Fe".to_owned()],
            empty_directories: Vec::new(),
            sentinels: vec![asset("Rally.exe", b"sentinel")],
            files: vec![asset("Game/a.bin", b"a"), asset("New_Fe/b.bin", b"bb")],
        }
    }

    #[test]
    fn verifies_complete_source() {
        let tree = TestTree::new();
        let profile = fixture(&tree);
        assert_eq!(
            verify_source(&tree.0, &profile).unwrap(),
            VerificationSummary {
                files: 2,
                stored_bytes: 3,
                raw_files: 2,
                gzip_cmpr_files: 0,
            }
        );
    }

    #[test]
    fn rejects_missing_and_unexpected_files() {
        let tree = TestTree::new();
        let profile = fixture(&tree);
        fs::remove_file(tree.0.join("Game/a.bin")).unwrap();
        assert!(matches!(
            verify_source(&tree.0, &profile),
            Err(VerificationError::MissingFile(path)) if path == "Game/a.bin"
        ));

        tree.write("Game/a.bin", b"a");
        tree.write("Game/extra.bin", b"extra");
        assert!(matches!(
            verify_source(&tree.0, &profile),
            Err(VerificationError::UnexpectedFile(path)) if path == "Game/extra.bin"
        ));
    }

    #[test]
    fn rejects_changed_sentinel_and_asset_bytes() {
        let tree = TestTree::new();
        let profile = fixture(&tree);
        tree.write("Rally.exe", b"changed!");
        assert!(matches!(
            verify_source(&tree.0, &profile),
            Err(VerificationError::HashMismatch { path, .. }) if path == "Rally.exe"
        ));

        tree.write("Rally.exe", b"sentinel");
        tree.write("Game/a.bin", b"b");
        assert!(matches!(
            verify_source(&tree.0, &profile),
            Err(VerificationError::HashMismatch { path, .. }) if path == "Game/a.bin"
        ));
    }

    #[test]
    fn rejects_declared_encoding_mismatch() {
        let tree = TestTree::new();
        let mut profile = fixture(&tree);
        profile.files[0].encoding = StorageEncoding::GzipCmpr;
        assert!(matches!(
            verify_source(&tree.0, &profile),
            Err(VerificationError::EncodingMismatch { path, .. }) if path == "Game/a.bin"
        ));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinks_inside_roots() {
        use std::os::unix::fs::symlink;

        let tree = TestTree::new();
        let profile = fixture(&tree);
        symlink("a.bin", tree.0.join("Game/link.bin")).unwrap();
        assert!(matches!(
            verify_source(&tree.0, &profile),
            Err(VerificationError::Symlink(_))
        ));
    }

    #[test]
    fn rejects_case_colliding_directories_with_distinct_files() {
        let tree = TestTree::new();
        let mut profile = fixture(&tree);
        tree.write("Game/Foo/x.bin", b"x");
        tree.write("Game/foo/y.bin", b"y");
        profile.files.push(asset("Game/Foo/x.bin", b"x"));
        profile.files.push(asset("Game/foo/y.bin", b"y"));
        profile
            .files
            .sort_by(|left, right| left.path.cmp(&right.path));
        assert!(matches!(
            verify_source(&tree.0, &profile),
            Err(VerificationError::Profile(
                ProfileError::CaseCollision { .. }
            ))
        ));
    }
}
