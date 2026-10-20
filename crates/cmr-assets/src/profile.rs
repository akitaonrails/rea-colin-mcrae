use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AssetProfile {
    pub schema_version: u32,
    pub id: String,
    pub roots: Vec<String>,
    #[serde(default)]
    pub empty_directories: Vec<String>,
    pub sentinels: Vec<AssetFile>,
    pub files: Vec<AssetFile>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AssetFile {
    pub path: String,
    pub size: u64,
    pub sha256: String,
    pub encoding: StorageEncoding,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum StorageEncoding {
    Raw,
    GzipCmpr,
}

#[derive(Debug, Eq, PartialEq)]
pub enum ProfileError {
    UnsupportedSchema(u32),
    EmptyId,
    InvalidPath { field: &'static str, path: String },
    UnsortedOrDuplicate { field: &'static str, path: String },
    CaseCollision { first: String, second: String },
    FileOutsideRoots(String),
    InvalidSha256 { path: String },
}

impl fmt::Display for ProfileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchema(version) => {
                write!(formatter, "unsupported asset profile schema {version}")
            }
            Self::EmptyId => write!(formatter, "asset profile ID is empty"),
            Self::InvalidPath { field, path } => {
                write!(formatter, "invalid {field} path: {path:?}")
            }
            Self::UnsortedOrDuplicate { field, path } => {
                write!(
                    formatter,
                    "{field} paths are unsorted or duplicate at {path:?}"
                )
            }
            Self::CaseCollision { first, second } => {
                write!(
                    formatter,
                    "ASCII case-fold collision between {first:?} and {second:?}"
                )
            }
            Self::FileOutsideRoots(path) => {
                write!(
                    formatter,
                    "profile file is outside declared roots: {path:?}"
                )
            }
            Self::InvalidSha256 { path } => {
                write!(formatter, "invalid SHA-256 for {path:?}")
            }
        }
    }
}

impl std::error::Error for ProfileError {}

impl AssetProfile {
    pub fn validate(&self) -> Result<(), ProfileError> {
        if self.schema_version != 1 {
            return Err(ProfileError::UnsupportedSchema(self.schema_version));
        }
        if self.id.trim().is_empty() {
            return Err(ProfileError::EmptyId);
        }

        validate_sorted_paths("roots", &self.roots)?;
        validate_sorted_paths("empty_directories", &self.empty_directories)?;
        validate_sorted_files("sentinels", &self.sentinels)?;
        validate_sorted_files("files", &self.files)?;

        let mut folded = std::collections::BTreeMap::<String, String>::new();
        for path in self
            .roots
            .iter()
            .chain(&self.empty_directories)
            .chain(self.sentinels.iter().map(|file| &file.path))
            .chain(self.files.iter().map(|file| &file.path))
        {
            for boundary in path
                .match_indices('/')
                .map(|(index, _)| index)
                .chain(std::iter::once(path.len()))
            {
                let component_path = &path[..boundary];
                let key = component_path.to_ascii_lowercase();
                if let Some(first) = folded.insert(key, component_path.to_owned())
                    && first != component_path
                {
                    return Err(ProfileError::CaseCollision {
                        first,
                        second: component_path.to_owned(),
                    });
                }
            }
        }

        for file in self.sentinels.iter().chain(&self.files) {
            if file.sha256.len() != 64
                || !file
                    .sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            {
                return Err(ProfileError::InvalidSha256 {
                    path: file.path.clone(),
                });
            }
        }

        for file in &self.files {
            if !self.roots.iter().any(|root| {
                file.path
                    .strip_prefix(root)
                    .is_some_and(|suffix| suffix.starts_with('/'))
            }) {
                return Err(ProfileError::FileOutsideRoots(file.path.clone()));
            }
        }

        Ok(())
    }
}

fn validate_sorted_paths(field: &'static str, paths: &[String]) -> Result<(), ProfileError> {
    let mut previous: Option<&str> = None;
    for path in paths {
        if !is_portable_relative_path(path) {
            return Err(ProfileError::InvalidPath {
                field,
                path: path.clone(),
            });
        }
        if previous.is_some_and(|previous| previous >= path.as_str()) {
            return Err(ProfileError::UnsortedOrDuplicate {
                field,
                path: path.clone(),
            });
        }
        previous = Some(path);
    }
    Ok(())
}

fn validate_sorted_files(field: &'static str, files: &[AssetFile]) -> Result<(), ProfileError> {
    let paths: BTreeSet<&str> = files.iter().map(|file| file.path.as_str()).collect();
    if paths.len() != files.len() {
        let duplicate = files
            .windows(2)
            .find(|pair| pair[0].path == pair[1].path)
            .map_or("<duplicate>", |pair| pair[1].path.as_str());
        return Err(ProfileError::UnsortedOrDuplicate {
            field,
            path: duplicate.to_owned(),
        });
    }

    let path_list: Vec<String> = files.iter().map(|file| file.path.clone()).collect();
    validate_sorted_paths(field, &path_list)
}

fn is_portable_relative_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.ends_with('/')
        && !path.contains('\\')
        && !path.contains('\0')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str) -> AssetFile {
        AssetFile {
            path: path.to_owned(),
            size: 1,
            sha256: "00".repeat(32),
            encoding: StorageEncoding::Raw,
        }
    }

    fn profile() -> AssetProfile {
        AssetProfile {
            schema_version: 1,
            id: "synthetic".to_owned(),
            roots: vec!["Game".to_owned(), "New_Fe".to_owned()],
            empty_directories: vec!["New_Fe/Empty".to_owned()],
            sentinels: vec![file("Rally.exe")],
            files: vec![file("Game/a.bin"), file("New_Fe/b.bin")],
        }
    }

    #[test]
    fn accepts_well_formed_profile() {
        profile().validate().unwrap();
    }

    #[test]
    fn rejects_parent_traversal_and_backslashes() {
        for invalid in ["Game/../secret", "Game\\file", "/absolute", "Game//file"] {
            let mut candidate = profile();
            candidate.files = vec![file(invalid)];
            assert!(matches!(
                candidate.validate(),
                Err(ProfileError::InvalidPath { .. })
            ));
        }
    }

    #[test]
    fn rejects_unsorted_paths() {
        let mut candidate = profile();
        candidate.files.reverse();
        assert!(matches!(
            candidate.validate(),
            Err(ProfileError::UnsortedOrDuplicate { field: "files", .. })
        ));
    }

    #[test]
    fn rejects_ascii_case_collisions() {
        let mut candidate = profile();
        candidate.files = vec![file("Game/A.bin"), file("Game/a.bin")];
        assert!(matches!(
            candidate.validate(),
            Err(ProfileError::CaseCollision { .. })
        ));

        let mut candidate = profile();
        candidate.files = vec![file("Game/Folder/a.bin"), file("Game/folder/b.bin")];
        assert!(matches!(
            candidate.validate(),
            Err(ProfileError::CaseCollision { .. })
        ));
    }

    #[test]
    fn rejects_file_outside_declared_roots() {
        let mut candidate = profile();
        candidate.files = vec![file("ini/file.bin")];
        assert!(matches!(
            candidate.validate(),
            Err(ProfileError::FileOutsideRoots(_))
        ));
    }

    #[test]
    fn rejects_uppercase_or_malformed_hashes() {
        for hash in ["AA".repeat(32), "0".repeat(63), "zz".repeat(32)] {
            let mut candidate = profile();
            candidate.files[0].sha256 = hash;
            assert!(matches!(
                candidate.validate(),
                Err(ProfileError::InvalidSha256 { .. })
            ));
        }
    }
}
