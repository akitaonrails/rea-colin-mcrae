use std::fmt;

use cmr_formats::pe_surface::{OFFICIAL_EU_SURFACE_FAMILY_VA, SURFACE_COEFFICIENT_COUNT};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::normalize_relative_path;

pub const SURFACE_PROFILE_SCHEMA_VERSION: u32 = 1;
pub const SURFACE_FAMILY_COUNT: u32 = 2;
const MAX_EXECUTABLE_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SurfaceTableProfile {
    pub schema_version: u32,
    pub id: String,
    pub executable: SurfaceExecutableProfile,
    pub table: SurfaceTableSpec,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SurfaceExecutableProfile {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SurfaceTableSpec {
    pub virtual_address: u32,
    pub families: u32,
    pub entries_per_family: u32,
    pub entry_encoding: SurfaceTableEncoding,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SurfaceTableEncoding {
    SignedI32LittleEndian,
}

#[derive(Clone, Debug)]
pub struct ReviewedSurfaceProfile {
    profile: SurfaceTableProfile,
    sha256: String,
}

#[derive(Debug)]
pub enum ReviewedSurfaceProfileError {
    Json(serde_json::Error),
    Profile(SurfaceProfileError),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SurfaceProfileError {
    UnsupportedSchema(u32),
    EmptyId,
    InvalidExecutablePath(String),
    InvalidExecutableSize(u64),
    InvalidExecutableSha256,
    UnsupportedVirtualAddress(u32),
    UnsupportedFamilyCount(u32),
    UnsupportedEntryCount(u32),
}

impl fmt::Display for ReviewedSurfaceProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(f, "invalid surface profile JSON: {error}"),
            Self::Profile(error) => write!(f, "invalid surface profile: {error}"),
        }
    }
}

impl std::error::Error for ReviewedSurfaceProfileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            Self::Profile(error) => Some(error),
        }
    }
}

impl fmt::Display for SurfaceProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchema(value) => write!(f, "unsupported schema {value}"),
            Self::EmptyId => write!(f, "profile ID is empty"),
            Self::InvalidExecutablePath(path) => write!(f, "invalid executable path {path:?}"),
            Self::InvalidExecutableSize(size) => write!(f, "invalid executable size {size}"),
            Self::InvalidExecutableSha256 => write!(f, "invalid executable SHA-256"),
            Self::UnsupportedVirtualAddress(value) => {
                write!(f, "unsupported table virtual address {value:#010x}")
            }
            Self::UnsupportedFamilyCount(value) => {
                write!(f, "unsupported surface family count {value}")
            }
            Self::UnsupportedEntryCount(value) => {
                write!(f, "unsupported surface entry count {value}")
            }
        }
    }
}

impl std::error::Error for SurfaceProfileError {}

impl ReviewedSurfaceProfile {
    pub fn from_json(document: &[u8]) -> Result<Self, ReviewedSurfaceProfileError> {
        let profile: SurfaceTableProfile =
            serde_json::from_slice(document).map_err(ReviewedSurfaceProfileError::Json)?;
        profile
            .validate()
            .map_err(ReviewedSurfaceProfileError::Profile)?;
        let sha256 = encode_digest(Sha256::digest(document));
        Ok(Self { profile, sha256 })
    }

    pub fn profile(&self) -> &SurfaceTableProfile {
        &self.profile
    }

    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

impl SurfaceTableProfile {
    pub fn validate(&self) -> Result<(), SurfaceProfileError> {
        if self.schema_version != SURFACE_PROFILE_SCHEMA_VERSION {
            return Err(SurfaceProfileError::UnsupportedSchema(self.schema_version));
        }
        if self.id.trim().is_empty() {
            return Err(SurfaceProfileError::EmptyId);
        }
        if normalize_relative_path(&self.executable.path).as_deref()
            != Some(self.executable.path.as_str())
        {
            return Err(SurfaceProfileError::InvalidExecutablePath(
                self.executable.path.clone(),
            ));
        }
        if self.executable.size == 0 || self.executable.size > MAX_EXECUTABLE_BYTES {
            return Err(SurfaceProfileError::InvalidExecutableSize(
                self.executable.size,
            ));
        }
        if self.executable.sha256.len() != 64
            || !self
                .executable
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(SurfaceProfileError::InvalidExecutableSha256);
        }
        if self.table.virtual_address != OFFICIAL_EU_SURFACE_FAMILY_VA {
            return Err(SurfaceProfileError::UnsupportedVirtualAddress(
                self.table.virtual_address,
            ));
        }
        if self.table.families != SURFACE_FAMILY_COUNT {
            return Err(SurfaceProfileError::UnsupportedFamilyCount(
                self.table.families,
            ));
        }
        if self.table.entries_per_family != SURFACE_COEFFICIENT_COUNT as u32 {
            return Err(SurfaceProfileError::UnsupportedEntryCount(
                self.table.entries_per_family,
            ));
        }
        Ok(())
    }
}

fn encode_digest(digest: impl IntoIterator<Item = u8>) -> String {
    use std::fmt::Write as _;
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        write!(encoded, "{byte:02x}").expect("writing to String cannot fail");
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> SurfaceTableProfile {
        SurfaceTableProfile {
            schema_version: 1,
            id: "synthetic".to_owned(),
            executable: SurfaceExecutableProfile {
                path: "Rally.exe".to_owned(),
                size: 1234,
                sha256: "00".repeat(32),
            },
            table: SurfaceTableSpec {
                virtual_address: OFFICIAL_EU_SURFACE_FAMILY_VA,
                families: 2,
                entries_per_family: 120,
                entry_encoding: SurfaceTableEncoding::SignedI32LittleEndian,
            },
        }
    }

    #[test]
    fn reviewed_profile_is_bound_to_exact_json_bytes() {
        let document = serde_json::to_vec(&profile()).unwrap();
        let reviewed = ReviewedSurfaceProfile::from_json(&document).unwrap();
        assert_eq!(reviewed.profile(), &profile());
        assert_eq!(reviewed.sha256().len(), 64);
        let pretty = serde_json::to_vec_pretty(&profile()).unwrap();
        assert_ne!(
            reviewed.sha256(),
            ReviewedSurfaceProfile::from_json(&pretty).unwrap().sha256()
        );
    }

    #[test]
    fn rejects_unsafe_paths_hashes_sizes_and_shapes() {
        let mut candidate = profile();
        candidate.executable.path = "../Rally.exe".to_owned();
        assert!(matches!(
            candidate.validate(),
            Err(SurfaceProfileError::InvalidExecutablePath(_))
        ));
        candidate = profile();
        candidate.executable.sha256 = "A".repeat(64);
        assert_eq!(
            candidate.validate(),
            Err(SurfaceProfileError::InvalidExecutableSha256)
        );
        candidate = profile();
        candidate.executable.size = 0;
        assert_eq!(
            candidate.validate(),
            Err(SurfaceProfileError::InvalidExecutableSize(0))
        );
        candidate = profile();
        candidate.table.virtual_address += 4;
        assert!(matches!(
            candidate.validate(),
            Err(SurfaceProfileError::UnsupportedVirtualAddress(_))
        ));
        candidate = profile();
        candidate.table.families = 1;
        assert_eq!(
            candidate.validate(),
            Err(SurfaceProfileError::UnsupportedFamilyCount(1))
        );
        candidate = profile();
        candidate.table.entries_per_family = 119;
        assert_eq!(
            candidate.validate(),
            Err(SurfaceProfileError::UnsupportedEntryCount(119))
        );
    }
}
