use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

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

#[derive(Clone, Debug)]
pub struct ReviewedProfile {
    profile: AssetProfile,
    sha256: String,
}

#[derive(Debug)]
pub enum ReviewedProfileError {
    Json(serde_json::Error),
    Profile(ProfileError),
}

impl fmt::Display for ReviewedProfileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(formatter, "invalid profile JSON: {error}"),
            Self::Profile(error) => write!(formatter, "invalid asset profile: {error}"),
        }
    }
}

impl std::error::Error for ReviewedProfileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            Self::Profile(error) => Some(error),
        }
    }
}

impl ReviewedProfile {
    /// Parses and validates a profile while binding it to the exact distributed
    /// JSON bytes. Whitespace changes intentionally produce a different digest.
    pub fn from_json(document: &[u8]) -> Result<Self, ReviewedProfileError> {
        let profile: AssetProfile =
            serde_json::from_slice(document).map_err(ReviewedProfileError::Json)?;
        profile.validate().map_err(ReviewedProfileError::Profile)?;
        let digest = Sha256::digest(document);
        let mut sha256 = String::with_capacity(digest.len() * 2);
        use std::fmt::Write as _;
        for byte in digest {
            write!(sha256, "{byte:02x}").expect("writing to a String cannot fail");
        }
        Ok(Self { profile, sha256 })
    }

    pub fn profile(&self) -> &AssetProfile {
        &self.profile
    }

    pub fn sha256(&self) -> &str {
        &self.sha256
    }
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
        && path.split('/').all(is_windows_portable_component)
}

fn is_windows_portable_component(part: &str) -> bool {
    if part.is_empty()
        || part == "."
        || part == ".."
        || part.ends_with([' ', '.'])
        || part
            .chars()
            .any(|character| character <= '\u{1f}' || "<>:\"|?*".contains(character))
    {
        return false;
    }

    let stem = part.split('.').next().unwrap_or(part).to_ascii_uppercase();
    !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !is_numbered_device(&stem, "COM")
        && !is_numbered_device(&stem, "LPT")
}

fn is_numbered_device(stem: &str, prefix: &str) -> bool {
    stem.strip_prefix(prefix)
        .is_some_and(|number| matches!(number.as_bytes(), [b'1'..=b'9']))
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
        for invalid in [
            "Game/../secret",
            "Game\\file",
            "/absolute",
            "Game//file",
            "Game/C:escape",
            "Game/foo.",
            "Game/foo ",
            "Game/CON",
            "Game/nul.txt",
            "Game/Com1.bin",
            "Game/LPT9",
        ] {
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

    #[test]
    fn reviewed_profile_digest_is_bound_to_exact_json() {
        let compact = serde_json::to_vec(&profile()).unwrap();
        let pretty = serde_json::to_vec_pretty(&profile()).unwrap();
        let compact = ReviewedProfile::from_json(&compact).unwrap();
        let pretty = ReviewedProfile::from_json(&pretty).unwrap();
        assert_eq!(compact.profile().id, "synthetic");
        assert_ne!(compact.sha256(), pretty.sha256());
    }
}
