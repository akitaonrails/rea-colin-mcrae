use std::collections::BTreeMap;
use std::fmt;

use crate::{AssetProfile, ProfileError};

#[derive(Debug)]
pub struct AssetIndex {
    by_folded_path: BTreeMap<String, String>,
}

#[derive(Debug, Eq, PartialEq)]
pub enum ResolveError {
    Profile(ProfileError),
    InvalidPath(String),
    NotFound(String),
}

impl fmt::Display for ResolveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Profile(error) => write!(formatter, "invalid asset profile: {error}"),
            Self::InvalidPath(path) => write!(formatter, "invalid logical asset path: {path:?}"),
            Self::NotFound(path) => {
                write!(formatter, "asset is not present in the profile: {path:?}")
            }
        }
    }
}

impl std::error::Error for ResolveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Profile(error) => Some(error),
            _ => None,
        }
    }
}

impl From<ProfileError> for ResolveError {
    fn from(error: ProfileError) -> Self {
        Self::Profile(error)
    }
}

impl AssetIndex {
    pub fn from_profile(profile: &AssetProfile) -> Result<Self, ResolveError> {
        profile.validate()?;
        let by_folded_path = profile
            .files
            .iter()
            .map(|file| (file.path.to_ascii_lowercase(), file.path.clone()))
            .collect();
        Ok(Self { by_folded_path })
    }

    /// Resolves Windows-style separators and ASCII casing to the exact stored
    /// relative path from the reviewed profile.
    pub fn resolve(&self, requested: &str) -> Result<&str, ResolveError> {
        let normalized = normalize_request(requested)?;
        self.by_folded_path
            .get(&normalized.to_ascii_lowercase())
            .map(String::as_str)
            .ok_or_else(|| ResolveError::NotFound(requested.to_owned()))
    }
}

fn normalize_request(requested: &str) -> Result<String, ResolveError> {
    crate::path::normalize_relative_path(requested)
        .ok_or_else(|| ResolveError::InvalidPath(requested.to_owned()))
}

#[cfg(test)]
mod tests {
    use crate::{AssetFile, StorageEncoding};

    use super::*;

    fn profile() -> AssetProfile {
        AssetProfile {
            schema_version: 1,
            id: "synthetic".to_owned(),
            roots: vec!["Game".to_owned()],
            empty_directories: Vec::new(),
            sentinels: Vec::new(),
            files: vec![AssetFile {
                path: "Game/Objects/Cars/Lancia.c3d".to_owned(),
                size: 1,
                sha256: "00".repeat(32),
                encoding: StorageEncoding::Raw,
            }],
        }
    }

    #[test]
    fn resolves_windows_separators_and_ascii_case() {
        let index = AssetIndex::from_profile(&profile()).unwrap();
        assert_eq!(
            index.resolve("GAME\\OBJECTS\\CARS\\LANCIA.C3D").unwrap(),
            "Game/Objects/Cars/Lancia.c3d"
        );
    }

    #[test]
    fn rejects_absolute_drive_traversal_and_ambiguous_components() {
        let index = AssetIndex::from_profile(&profile()).unwrap();
        for invalid in [
            "/Game/file",
            "\\Game\\file",
            "C:\\Game\\file",
            "Game/../file",
            "Game/./file",
            "Game//file",
            "Game/file/",
            "Game\0file",
            // Windows-shaped variants (game paths use backslashes): backslash
            // traversal, UNC, and a drive-relative name.
            "Game\\..\\file",
            "\\\\server\\share\\file",
            "C:file",
        ] {
            assert!(matches!(
                index.resolve(invalid),
                Err(ResolveError::InvalidPath(_))
            ));
        }
    }

    #[test]
    fn reports_missing_path_without_filesystem_fallback() {
        let index = AssetIndex::from_profile(&profile()).unwrap();
        assert!(matches!(
            index.resolve("Game/missing.bin"),
            Err(ResolveError::NotFound(_))
        ));
    }
}
