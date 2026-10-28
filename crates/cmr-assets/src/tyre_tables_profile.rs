use std::fmt;

use cmr_formats::pe_tyre_tables::{
    OFFICIAL_EU_TYRE_TABLES_LAYOUT, TYRE_SURFACE_COUNT, TYRE_VARIANT_COUNT, TyreTablesLayout,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::normalize_relative_path;

pub const TYRE_TABLES_PROFILE_SCHEMA_VERSION: u32 = 1;
const MAX_EXECUTABLE_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TyreTablesProfile {
    pub schema_version: u32,
    pub id: String,
    pub executable: TyreTablesExecutable,
    pub tables: TyreTablesSpec,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TyreTablesExecutable {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TyreTablesSpec {
    pub image_base: u32,
    pub candidate_base_6b50_va: u32,
    pub candidate_base_6f10_va: u32,
    pub wheel_terms_777c_va: u32,
    pub surfaces: u32,
    pub variants: u32,
    pub output_encoding: TyreTablesEncoding,
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TyreTablesEncoding {
    CmrTyreTablesV2,
}

impl TyreTablesSpec {
    pub fn layout(&self) -> TyreTablesLayout {
        TyreTablesLayout {
            image_base: self.image_base,
            candidate_base_6b50_va: self.candidate_base_6b50_va,
            candidate_base_6f10_va: self.candidate_base_6f10_va,
            wheel_terms_777c_va: self.wheel_terms_777c_va,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ReviewedTyreTablesProfile {
    profile: TyreTablesProfile,
    sha256: String,
}
#[derive(Debug)]
pub enum ReviewedTyreTablesProfileError {
    Json(serde_json::Error),
    Profile(TyreTablesProfileError),
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TyreTablesProfileError {
    UnsupportedSchema(u32),
    EmptyId,
    InvalidExecutablePath(String),
    InvalidExecutableSize(u64),
    InvalidExecutableSha256,
    UnsupportedLayout,
}

impl fmt::Display for TyreTablesProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchema(v) => write!(f, "unsupported schema {v}"),
            Self::EmptyId => write!(f, "profile ID is empty"),
            Self::InvalidExecutablePath(p) => write!(f, "invalid executable path {p:?}"),
            Self::InvalidExecutableSize(s) => write!(f, "invalid executable size {s}"),
            Self::InvalidExecutableSha256 => write!(f, "invalid executable SHA-256"),
            Self::UnsupportedLayout => write!(f, "unsupported tyre-surface-tables layout"),
        }
    }
}
impl std::error::Error for TyreTablesProfileError {}
impl fmt::Display for ReviewedTyreTablesProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(e) => write!(f, "invalid tyre-surface-tables profile JSON: {e}"),
            Self::Profile(e) => write!(f, "invalid tyre-surface-tables profile: {e}"),
        }
    }
}
impl std::error::Error for ReviewedTyreTablesProfileError {}

impl TyreTablesProfile {
    pub fn validate(&self) -> Result<(), TyreTablesProfileError> {
        if self.schema_version != TYRE_TABLES_PROFILE_SCHEMA_VERSION {
            return Err(TyreTablesProfileError::UnsupportedSchema(
                self.schema_version,
            ));
        }
        if self.id.trim().is_empty() {
            return Err(TyreTablesProfileError::EmptyId);
        }
        if normalize_relative_path(&self.executable.path).as_deref()
            != Some(self.executable.path.as_str())
        {
            return Err(TyreTablesProfileError::InvalidExecutablePath(
                self.executable.path.clone(),
            ));
        }
        if self.executable.size == 0 || self.executable.size > MAX_EXECUTABLE_BYTES {
            return Err(TyreTablesProfileError::InvalidExecutableSize(
                self.executable.size,
            ));
        }
        if self.executable.sha256.len() != 64
            || !self
                .executable
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(TyreTablesProfileError::InvalidExecutableSha256);
        }
        if self.tables.layout() != OFFICIAL_EU_TYRE_TABLES_LAYOUT
            || self.tables.surfaces != TYRE_SURFACE_COUNT as u32
            || self.tables.variants != TYRE_VARIANT_COUNT as u32
            || self.tables.output_encoding != TyreTablesEncoding::CmrTyreTablesV2
        {
            return Err(TyreTablesProfileError::UnsupportedLayout);
        }
        Ok(())
    }
}
impl ReviewedTyreTablesProfile {
    pub fn from_json(document: &[u8]) -> Result<Self, ReviewedTyreTablesProfileError> {
        let profile: TyreTablesProfile =
            serde_json::from_slice(document).map_err(ReviewedTyreTablesProfileError::Json)?;
        profile
            .validate()
            .map_err(ReviewedTyreTablesProfileError::Profile)?;
        Ok(Self {
            profile,
            sha256: Sha256::digest(document)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
        })
    }
    pub fn profile(&self) -> &TyreTablesProfile {
        &self.profile
    }
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub(crate) fn profile(size: u64, hash: String) -> TyreTablesProfile {
        let l = OFFICIAL_EU_TYRE_TABLES_LAYOUT;
        TyreTablesProfile {
            schema_version: 1,
            id: "synthetic-tyre-surface-tabless".into(),
            executable: TyreTablesExecutable {
                path: "Rally.exe".into(),
                size,
                sha256: hash,
            },
            tables: TyreTablesSpec {
                image_base: l.image_base,
                candidate_base_6b50_va: l.candidate_base_6b50_va,
                candidate_base_6f10_va: l.candidate_base_6f10_va,
                wheel_terms_777c_va: l.wheel_terms_777c_va,
                surfaces: TYRE_SURFACE_COUNT as u32,
                variants: TYRE_VARIANT_COUNT as u32,
                output_encoding: TyreTablesEncoding::CmrTyreTablesV2,
            },
        }
    }
    #[test]
    fn validates_every_trust_boundary() {
        let base = profile(10, "00".repeat(32));
        assert!(base.validate().is_ok());
        let mut candidates = Vec::new();
        let mut p = base.clone();
        p.schema_version = 2;
        candidates.push((p, TyreTablesProfileError::UnsupportedSchema(2)));
        let mut p = base.clone();
        p.id = " \t".into();
        candidates.push((p, TyreTablesProfileError::EmptyId));
        for path in ["../Rally.exe", "/Rally.exe", "dir\\Rally.exe"] {
            let mut p = base.clone();
            p.executable.path = path.into();
            candidates.push((
                p,
                TyreTablesProfileError::InvalidExecutablePath(path.into()),
            ));
        }
        for size in [0, MAX_EXECUTABLE_BYTES + 1] {
            let mut p = base.clone();
            p.executable.size = size;
            candidates.push((p, TyreTablesProfileError::InvalidExecutableSize(size)));
        }
        for hash in ["00".to_owned(), "AA".repeat(32), "gg".repeat(32)] {
            let mut p = base.clone();
            p.executable.sha256 = hash;
            candidates.push((p, TyreTablesProfileError::InvalidExecutableSha256));
        }
        for field in 0..6 {
            let mut p = base.clone();
            match field {
                0 => p.tables.image_base += 1,
                1 => p.tables.candidate_base_6b50_va += 4,
                2 => p.tables.candidate_base_6f10_va += 4,
                3 => p.tables.wheel_terms_777c_va += 2,
                4 => p.tables.surfaces -= 1,
                _ => p.tables.variants -= 1,
            };
            candidates.push((p, TyreTablesProfileError::UnsupportedLayout));
        }
        for (candidate, expected) in candidates {
            assert_eq!(candidate.validate(), Err(expected));
        }

        let mut value = serde_json::to_value(&base).unwrap();
        value["tables"]["output_encoding"] = serde_json::Value::String("unknown".into());
        assert!(matches!(
            ReviewedTyreTablesProfile::from_json(&serde_json::to_vec(&value).unwrap()),
            Err(ReviewedTyreTablesProfileError::Json(_))
        ));
        let mut value = serde_json::to_value(&base).unwrap();
        value["payload"] = serde_json::json!([1, 2, 3]);
        assert!(matches!(
            ReviewedTyreTablesProfile::from_json(&serde_json::to_vec(&value).unwrap()),
            Err(ReviewedTyreTablesProfileError::Json(_))
        ));
    }

    #[test]
    fn committed_official_profile_is_metadata_only_and_reviewable() {
        let document = include_bytes!("../../../profiles/official-eu-tyre-surface-tables-v1.json");
        let reviewed = ReviewedTyreTablesProfile::from_json(document).unwrap();
        assert_eq!(reviewed.profile().executable.path, "Rally.exe");
        assert_eq!(reviewed.profile().tables.surfaces, 120);
        assert_eq!(reviewed.profile().tables.variants, 8);
        let text = std::str::from_utf8(document).unwrap();
        assert!(!text.contains("/mnt/"));
        assert!(!text.contains("runtime-data"));
    }
}
