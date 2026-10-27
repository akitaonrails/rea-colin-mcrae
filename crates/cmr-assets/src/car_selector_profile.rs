use std::fmt;

use cmr_formats::pe_car_selector::{
    CAR_SELECTOR_COUNT, CarSelectorLayout, OFFICIAL_EU_CAR_SELECTOR_LAYOUT,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::normalize_relative_path;

pub const CAR_SELECTOR_PROFILE_SCHEMA_VERSION: u32 = 1;
const MAX_EXECUTABLE_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CarSelectorProfile {
    pub schema_version: u32,
    pub id: String,
    pub executable: CarSelectorExecutable,
    pub selector_map: CarSelectorSpec,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CarSelectorExecutable {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CarSelectorSpec {
    pub image_base: u32,
    pub consumer_va: u32,
    pub pointer_table_va: u32,
    pub selectors: u32,
    pub max_code_bytes: u32,
    pub output_encoding: CarSelectorEncoding,
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CarSelectorEncoding {
    CmrCselectorV1,
}

impl CarSelectorSpec {
    pub fn layout(&self) -> CarSelectorLayout {
        CarSelectorLayout {
            image_base: self.image_base,
            consumer_va: self.consumer_va,
            pointer_table_va: self.pointer_table_va,
            selector_count: self.selectors as usize,
            max_code_bytes: self.max_code_bytes as usize,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ReviewedCarSelectorProfile {
    profile: CarSelectorProfile,
    sha256: String,
}
#[derive(Debug)]
pub enum ReviewedCarSelectorProfileError {
    Json(serde_json::Error),
    Profile(CarSelectorProfileError),
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CarSelectorProfileError {
    UnsupportedSchema(u32),
    EmptyId,
    InvalidExecutablePath(String),
    InvalidExecutableSize(u64),
    InvalidExecutableSha256,
    UnsupportedLayout,
}

impl fmt::Display for CarSelectorProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchema(v) => write!(f, "unsupported schema {v}"),
            Self::EmptyId => write!(f, "profile ID is empty"),
            Self::InvalidExecutablePath(p) => write!(f, "invalid executable path {p:?}"),
            Self::InvalidExecutableSize(s) => write!(f, "invalid executable size {s}"),
            Self::InvalidExecutableSha256 => write!(f, "invalid executable SHA-256"),
            Self::UnsupportedLayout => write!(f, "unsupported car-selector layout"),
        }
    }
}
impl std::error::Error for CarSelectorProfileError {}
impl fmt::Display for ReviewedCarSelectorProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(e) => write!(f, "invalid car-selector profile JSON: {e}"),
            Self::Profile(e) => write!(f, "invalid car-selector profile: {e}"),
        }
    }
}
impl std::error::Error for ReviewedCarSelectorProfileError {}

impl CarSelectorProfile {
    pub fn validate(&self) -> Result<(), CarSelectorProfileError> {
        if self.schema_version != CAR_SELECTOR_PROFILE_SCHEMA_VERSION {
            return Err(CarSelectorProfileError::UnsupportedSchema(
                self.schema_version,
            ));
        }
        if self.id.trim().is_empty() {
            return Err(CarSelectorProfileError::EmptyId);
        }
        if normalize_relative_path(&self.executable.path).as_deref()
            != Some(self.executable.path.as_str())
        {
            return Err(CarSelectorProfileError::InvalidExecutablePath(
                self.executable.path.clone(),
            ));
        }
        if self.executable.size == 0 || self.executable.size > MAX_EXECUTABLE_BYTES {
            return Err(CarSelectorProfileError::InvalidExecutableSize(
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
            return Err(CarSelectorProfileError::InvalidExecutableSha256);
        }
        if self.selector_map.layout() != OFFICIAL_EU_CAR_SELECTOR_LAYOUT
            || self.selector_map.selectors != CAR_SELECTOR_COUNT as u32
            || self.selector_map.output_encoding != CarSelectorEncoding::CmrCselectorV1
        {
            return Err(CarSelectorProfileError::UnsupportedLayout);
        }
        Ok(())
    }
}
impl ReviewedCarSelectorProfile {
    pub fn from_json(document: &[u8]) -> Result<Self, ReviewedCarSelectorProfileError> {
        let profile: CarSelectorProfile =
            serde_json::from_slice(document).map_err(ReviewedCarSelectorProfileError::Json)?;
        profile
            .validate()
            .map_err(ReviewedCarSelectorProfileError::Profile)?;
        Ok(Self {
            profile,
            sha256: Sha256::digest(document)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
        })
    }
    pub fn profile(&self) -> &CarSelectorProfile {
        &self.profile
    }
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub(crate) fn profile(size: u64, hash: String) -> CarSelectorProfile {
        let l = OFFICIAL_EU_CAR_SELECTOR_LAYOUT;
        CarSelectorProfile {
            schema_version: 1,
            id: "synthetic-car-selectors".into(),
            executable: CarSelectorExecutable {
                path: "Rally.exe".into(),
                size,
                sha256: hash,
            },
            selector_map: CarSelectorSpec {
                image_base: l.image_base,
                consumer_va: l.consumer_va,
                pointer_table_va: l.pointer_table_va,
                selectors: CAR_SELECTOR_COUNT as u32,
                max_code_bytes: l.max_code_bytes as u32,
                output_encoding: CarSelectorEncoding::CmrCselectorV1,
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
        candidates.push((p, CarSelectorProfileError::UnsupportedSchema(2)));
        let mut p = base.clone();
        p.id = " \t".into();
        candidates.push((p, CarSelectorProfileError::EmptyId));
        for path in ["../Rally.exe", "/Rally.exe", "dir\\Rally.exe"] {
            let mut p = base.clone();
            p.executable.path = path.into();
            candidates.push((
                p,
                CarSelectorProfileError::InvalidExecutablePath(path.into()),
            ));
        }
        for size in [0, MAX_EXECUTABLE_BYTES + 1] {
            let mut p = base.clone();
            p.executable.size = size;
            candidates.push((p, CarSelectorProfileError::InvalidExecutableSize(size)));
        }
        for hash in ["00".to_owned(), "AA".repeat(32), "gg".repeat(32)] {
            let mut p = base.clone();
            p.executable.sha256 = hash;
            candidates.push((p, CarSelectorProfileError::InvalidExecutableSha256));
        }
        for field in 0..5 {
            let mut p = base.clone();
            match field {
                0 => p.selector_map.image_base += 1,
                1 => p.selector_map.consumer_va += 1,
                2 => p.selector_map.pointer_table_va += 1,
                3 => p.selector_map.selectors -= 1,
                _ => p.selector_map.max_code_bytes -= 1,
            };
            candidates.push((p, CarSelectorProfileError::UnsupportedLayout));
        }
        for (candidate, expected) in candidates {
            assert_eq!(candidate.validate(), Err(expected));
        }

        let mut value = serde_json::to_value(&base).unwrap();
        value["selector_map"]["output_encoding"] = serde_json::Value::String("unknown".into());
        assert!(matches!(
            ReviewedCarSelectorProfile::from_json(&serde_json::to_vec(&value).unwrap()),
            Err(ReviewedCarSelectorProfileError::Json(_))
        ));
        let mut value = serde_json::to_value(&base).unwrap();
        value["payload"] = serde_json::json!([1, 2, 3]);
        assert!(matches!(
            ReviewedCarSelectorProfile::from_json(&serde_json::to_vec(&value).unwrap()),
            Err(ReviewedCarSelectorProfileError::Json(_))
        ));
    }

    #[test]
    fn committed_official_profile_is_metadata_only_and_reviewable() {
        let document = include_bytes!("../../../profiles/official-eu-car-selector-map-v1.json");
        let reviewed = ReviewedCarSelectorProfile::from_json(document).unwrap();
        assert_eq!(reviewed.profile().executable.path, "Rally.exe");
        assert_eq!(reviewed.profile().selector_map.selectors, 14);
        let text = std::str::from_utf8(document).unwrap();
        assert!(!text.contains("/mnt/"));
        assert!(!text.contains("runtime-data"));
    }
}
