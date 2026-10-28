use std::fmt;

use cmr_formats::pe_rival_records::{
    OFFICIAL_EU_RIVAL_RECORDS_LAYOUT, RIVAL_RECORD_COUNT, RIVAL_RECORD_STRIDE, RivalRecordsLayout,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::normalize_relative_path;

pub const RIVAL_RECORDS_PROFILE_SCHEMA_VERSION: u32 = 1;
const MAX_EXECUTABLE_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RivalRecordsProfile {
    pub schema_version: u32,
    pub id: String,
    pub executable: RivalRecordsExecutable,
    pub records: RivalRecordsSpec,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RivalRecordsExecutable {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}
/// Placement of the reviewed record array, and the SHA-256 of the encoded
/// payload the reviewed executable yields (a digest only; the payload itself
/// is never committed).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RivalRecordsSpec {
    pub image_base: u32,
    pub records_va: u32,
    pub record_count: u32,
    pub stride: u32,
    pub payload_sha256: String,
    pub output_encoding: RivalRecordsEncoding,
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RivalRecordsEncoding {
    CmrRivalRecordsV1,
}

impl RivalRecordsSpec {
    pub fn layout(&self) -> RivalRecordsLayout {
        RivalRecordsLayout {
            image_base: self.image_base,
            records_va: self.records_va,
            record_count: self.record_count,
            stride: self.stride,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ReviewedRivalRecordsProfile {
    profile: RivalRecordsProfile,
    sha256: String,
}
#[derive(Debug)]
pub enum ReviewedRivalRecordsProfileError {
    Json(serde_json::Error),
    Profile(RivalRecordsProfileError),
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RivalRecordsProfileError {
    UnsupportedSchema(u32),
    EmptyId,
    InvalidExecutablePath(String),
    InvalidExecutableSize(u64),
    InvalidExecutableSha256,
    InvalidPayloadSha256,
    UnsupportedLayout,
}

impl fmt::Display for RivalRecordsProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchema(v) => write!(f, "unsupported schema {v}"),
            Self::EmptyId => write!(f, "profile ID is empty"),
            Self::InvalidExecutablePath(p) => write!(f, "invalid executable path {p:?}"),
            Self::InvalidExecutableSize(s) => write!(f, "invalid executable size {s}"),
            Self::InvalidExecutableSha256 => write!(f, "invalid executable SHA-256"),
            Self::InvalidPayloadSha256 => write!(f, "invalid payload SHA-256"),
            Self::UnsupportedLayout => write!(f, "unsupported rival-records layout"),
        }
    }
}
impl std::error::Error for RivalRecordsProfileError {}
impl fmt::Display for ReviewedRivalRecordsProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(e) => write!(f, "invalid rival-records profile JSON: {e}"),
            Self::Profile(e) => write!(f, "invalid rival-records profile: {e}"),
        }
    }
}
impl std::error::Error for ReviewedRivalRecordsProfileError {}

fn is_lower_sha256(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl RivalRecordsProfile {
    pub fn validate(&self) -> Result<(), RivalRecordsProfileError> {
        if self.schema_version != RIVAL_RECORDS_PROFILE_SCHEMA_VERSION {
            return Err(RivalRecordsProfileError::UnsupportedSchema(
                self.schema_version,
            ));
        }
        if self.id.trim().is_empty() {
            return Err(RivalRecordsProfileError::EmptyId);
        }
        if normalize_relative_path(&self.executable.path).as_deref()
            != Some(self.executable.path.as_str())
        {
            return Err(RivalRecordsProfileError::InvalidExecutablePath(
                self.executable.path.clone(),
            ));
        }
        if self.executable.size == 0 || self.executable.size > MAX_EXECUTABLE_BYTES {
            return Err(RivalRecordsProfileError::InvalidExecutableSize(
                self.executable.size,
            ));
        }
        if !is_lower_sha256(&self.executable.sha256) {
            return Err(RivalRecordsProfileError::InvalidExecutableSha256);
        }
        if !is_lower_sha256(&self.records.payload_sha256) {
            return Err(RivalRecordsProfileError::InvalidPayloadSha256);
        }
        if self.records.layout() != OFFICIAL_EU_RIVAL_RECORDS_LAYOUT
            || self.records.record_count != RIVAL_RECORD_COUNT as u32
            || self.records.stride != RIVAL_RECORD_STRIDE as u32
            || self.records.output_encoding != RivalRecordsEncoding::CmrRivalRecordsV1
        {
            return Err(RivalRecordsProfileError::UnsupportedLayout);
        }
        Ok(())
    }
}
impl ReviewedRivalRecordsProfile {
    pub fn from_json(document: &[u8]) -> Result<Self, ReviewedRivalRecordsProfileError> {
        let profile: RivalRecordsProfile =
            serde_json::from_slice(document).map_err(ReviewedRivalRecordsProfileError::Json)?;
        profile
            .validate()
            .map_err(ReviewedRivalRecordsProfileError::Profile)?;
        Ok(Self {
            profile,
            sha256: Sha256::digest(document)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
        })
    }
    pub fn profile(&self) -> &RivalRecordsProfile {
        &self.profile
    }
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use cmr_formats::pe_rival_records::{RivalRecord, RivalRecords, encode_rival_records};

    /// Synthetic records; not game data.
    pub(crate) fn synthetic_records() -> RivalRecords {
        RivalRecords::from_vec(
            (0..RIVAL_RECORD_COUNT)
                .map(|i| {
                    let raw = std::array::from_fn(|j| (i * 0x14 + j) as u8);
                    RivalRecord::new(raw, format!("Synthetic{i}").into_bytes())
                })
                .collect(),
        )
        .unwrap()
    }
    pub(crate) fn synthetic_payload_sha256() -> String {
        crate::verify::sha256(&encode_rival_records(&synthetic_records()))
    }
    /// A profile whose payload digest matches [`synthetic_records`].
    pub(crate) fn profile(size: u64, hash: String) -> RivalRecordsProfile {
        let l = OFFICIAL_EU_RIVAL_RECORDS_LAYOUT;
        RivalRecordsProfile {
            schema_version: 1,
            id: "synthetic-rival-records".into(),
            executable: RivalRecordsExecutable {
                path: "Rally.exe".into(),
                size,
                sha256: hash,
            },
            records: RivalRecordsSpec {
                image_base: l.image_base,
                records_va: l.records_va,
                record_count: l.record_count,
                stride: l.stride,
                payload_sha256: synthetic_payload_sha256(),
                output_encoding: RivalRecordsEncoding::CmrRivalRecordsV1,
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
        candidates.push((p, RivalRecordsProfileError::UnsupportedSchema(2)));
        let mut p = base.clone();
        p.id = " \t".into();
        candidates.push((p, RivalRecordsProfileError::EmptyId));
        for path in ["../Rally.exe", "/Rally.exe", "dir\\Rally.exe"] {
            let mut p = base.clone();
            p.executable.path = path.into();
            candidates.push((
                p,
                RivalRecordsProfileError::InvalidExecutablePath(path.into()),
            ));
        }
        for size in [0, MAX_EXECUTABLE_BYTES + 1] {
            let mut p = base.clone();
            p.executable.size = size;
            candidates.push((p, RivalRecordsProfileError::InvalidExecutableSize(size)));
        }
        for hash in ["00".to_owned(), "AA".repeat(32), "gg".repeat(32)] {
            let mut p = base.clone();
            p.executable.sha256 = hash.clone();
            candidates.push((p, RivalRecordsProfileError::InvalidExecutableSha256));
            let mut p = base.clone();
            p.records.payload_sha256 = hash;
            candidates.push((p, RivalRecordsProfileError::InvalidPayloadSha256));
        }
        for field in 0..5 {
            let mut p = base.clone();
            let t = &mut p.records;
            match field {
                0 => t.image_base += 1,
                1 => t.records_va += 4,
                2 => t.records_va -= 0x14,
                3 => t.record_count -= 1,
                _ => t.stride += 4,
            };
            candidates.push((p, RivalRecordsProfileError::UnsupportedLayout));
        }
        for (candidate, expected) in candidates {
            assert_eq!(candidate.validate(), Err(expected));
        }

        let mut value = serde_json::to_value(&base).unwrap();
        value["records"]["output_encoding"] = serde_json::Value::String("unknown".into());
        assert!(matches!(
            ReviewedRivalRecordsProfile::from_json(&serde_json::to_vec(&value).unwrap()),
            Err(ReviewedRivalRecordsProfileError::Json(_))
        ));
        let mut value = serde_json::to_value(&base).unwrap();
        value["payload"] = serde_json::json!([1, 2, 3]);
        assert!(matches!(
            ReviewedRivalRecordsProfile::from_json(&serde_json::to_vec(&value).unwrap()),
            Err(ReviewedRivalRecordsProfileError::Json(_))
        ));
        let mut value = serde_json::to_value(&base).unwrap();
        value["records"]["names"] = serde_json::json!(["x"]);
        assert!(matches!(
            ReviewedRivalRecordsProfile::from_json(&serde_json::to_vec(&value).unwrap()),
            Err(ReviewedRivalRecordsProfileError::Json(_))
        ));
        let mut value = serde_json::to_value(&base).unwrap();
        value["records"]
            .as_object_mut()
            .unwrap()
            .remove("payload_sha256");
        assert!(matches!(
            ReviewedRivalRecordsProfile::from_json(&serde_json::to_vec(&value).unwrap()),
            Err(ReviewedRivalRecordsProfileError::Json(_))
        ));
        ReviewedRivalRecordsProfile::from_json(&serde_json::to_vec(&base).unwrap()).unwrap();
    }

    #[test]
    fn committed_official_profile_is_metadata_only_and_reviewable() {
        let document = include_bytes!("../../../profiles/official-eu-rival-records-v1.json");
        let reviewed = ReviewedRivalRecordsProfile::from_json(document).unwrap();
        assert_eq!(reviewed.profile().id, "official-eu-rival-records-v1");
        assert_eq!(reviewed.profile().executable.path, "Rally.exe");
        assert_eq!(reviewed.profile().records.record_count, 16);
        assert_eq!(reviewed.profile().records.stride, 0x14);
        // Same executable identity as the reviewed stage-weather profile.
        let weather = crate::ReviewedStageWeatherTablesProfile::from_json(include_bytes!(
            "../../../profiles/official-eu-stage-weather-tables-v1.json"
        ))
        .unwrap();
        assert_eq!(
            serde_json::to_value(&reviewed.profile().executable).unwrap(),
            serde_json::to_value(&weather.profile().executable).unwrap()
        );
        let text = std::str::from_utf8(document).unwrap();
        assert!(!text.contains("/mnt/"));
        assert!(!text.contains("runtime-data"));
        // Metadata only: no array of record bytes or names.
        assert!(!text.contains('['));
    }
}
