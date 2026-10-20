use std::fs;
use std::path::{Path, PathBuf};

use cmr_formats::stored_asset::{DecodeLimits, StoredAsset, decode_stored_asset};

use crate::verify::{join_portable, portable_relative, require_real_directory, sha256};
use crate::{AssetFile, AssetProfile, StorageEncoding, VerificationError};

const ROOTS: [&str; 2] = ["Game", "New_Fe"];
const SENTINELS: [&str; 3] = ["Game.exe", "Rally.exe", "qmixer.dll"];
const MAX_DECODED_ASSET_BYTES: usize = 512 * 1024 * 1024;

/// Builds a metadata-only profile for review and later checked-in verification.
///
/// Cataloging does not make an untrusted source authoritative. Import accepts a
/// profile only after that profile has been reviewed and distributed with the
/// source project.
pub fn catalog_standard_source(
    source: &Path,
    id: impl Into<String>,
) -> Result<AssetProfile, VerificationError> {
    require_real_directory(source)?;
    let mut files = Vec::new();
    let mut empty_directories = Vec::new();

    for root in ROOTS {
        let root_path = join_portable(source, root);
        require_real_directory(&root_path)?;
        catalog_directory(source, &root_path, &mut files, &mut empty_directories)?;
    }

    let mut sentinels = Vec::new();
    for path in SENTINELS {
        sentinels.push(catalog_file(source, path, false)?);
    }

    files.sort_by(|left, right| left.path.cmp(&right.path));
    sentinels.sort_by(|left, right| left.path.cmp(&right.path));
    empty_directories.sort();
    let profile = AssetProfile {
        schema_version: 1,
        id: id.into(),
        roots: ROOTS.into_iter().map(str::to_owned).collect(),
        empty_directories,
        sentinels,
        files,
    };
    profile.validate()?;
    Ok(profile)
}

fn catalog_directory(
    source: &Path,
    directory: &Path,
    files: &mut Vec<AssetFile>,
    empty_directories: &mut Vec<String>,
) -> Result<(), VerificationError> {
    let mut children: Vec<PathBuf> = fs::read_dir(directory)
        .map_err(|source| VerificationError::Io {
            path: directory.to_owned(),
            source,
        })?
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|source| VerificationError::Io {
                    path: directory.to_owned(),
                    source,
                })
        })
        .collect::<Result<_, _>>()?;
    children.sort();

    if children.is_empty() {
        empty_directories.push(portable_relative(source, directory)?);
        return Ok(());
    }

    for child in children {
        let metadata = fs::symlink_metadata(&child).map_err(|source| VerificationError::Io {
            path: child.clone(),
            source,
        })?;
        if metadata.file_type().is_symlink() {
            return Err(VerificationError::Symlink(child));
        }
        if metadata.is_dir() {
            catalog_directory(source, &child, files, empty_directories)?;
        } else if metadata.is_file() {
            let relative = portable_relative(source, &child)?;
            files.push(catalog_file(source, &relative, true)?);
        } else {
            return Err(VerificationError::SpecialFile(child));
        }
    }
    Ok(())
}

fn catalog_file(
    source: &Path,
    relative: &str,
    validate_encoding: bool,
) -> Result<AssetFile, VerificationError> {
    let path = join_portable(source, relative);
    let metadata = fs::symlink_metadata(&path).map_err(|source| VerificationError::Io {
        path: path.clone(),
        source,
    })?;
    if metadata.file_type().is_symlink() {
        return Err(VerificationError::Symlink(path));
    }
    if !metadata.is_file() {
        return Err(VerificationError::SpecialFile(path));
    }
    let bytes = fs::read(&path).map_err(|source| VerificationError::Io {
        path: path.clone(),
        source,
    })?;
    let encoding = if validate_encoding {
        match decode_stored_asset(
            &bytes,
            DecodeLimits {
                max_stored_bytes: bytes.len(),
                max_payload_bytes: MAX_DECODED_ASSET_BYTES,
            },
        ) {
            Ok(StoredAsset::Raw(_)) => StorageEncoding::Raw,
            Ok(StoredAsset::Cmpr(_)) => StorageEncoding::GzipCmpr,
            Err(error) => {
                return Err(VerificationError::InvalidEncoding {
                    path: relative.to_owned(),
                    reason: error.to_string(),
                });
            }
        }
    } else {
        StorageEncoding::Raw
    };

    Ok(AssetFile {
        path: relative.to_owned(),
        size: metadata.len(),
        sha256: sha256(&bytes),
        encoding,
    })
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
            let path =
                std::env::temp_dir().join(format!("cmr-catalog-{}-{id}", std::process::id()));
            fs::create_dir_all(path.join("Game/empty")).unwrap();
            fs::create_dir_all(path.join("New_Fe")).unwrap();
            fs::write(path.join("Game/a.bin"), b"a").unwrap();
            fs::write(path.join("New_Fe/b.bin"), b"bb").unwrap();
            for sentinel in SENTINELS {
                fs::write(path.join(sentinel), sentinel.as_bytes()).unwrap();
            }
            Self(path)
        }
    }

    impl Drop for TestTree {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn catalogs_sorted_metadata_without_source_path() {
        let tree = TestTree::new();
        let profile = catalog_standard_source(&tree.0, "synthetic").unwrap();
        assert_eq!(profile.id, "synthetic");
        assert_eq!(profile.roots, ["Game", "New_Fe"]);
        assert_eq!(profile.empty_directories, ["Game/empty"]);
        assert_eq!(
            profile
                .files
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            ["Game/a.bin", "New_Fe/b.bin"]
        );
        assert_eq!(profile.sentinels.len(), 3);
        let serialized = serde_json::to_string(&profile).unwrap();
        assert!(!serialized.contains(tree.0.to_str().unwrap()));
    }
}
