use std::fmt;

use cmr_formats::pe_stage_weather_tables::{
    OFFICIAL_EU_STAGE_WEATHER_TABLES_LAYOUT, SCHOOL_WEATHER_ENTRIES, STAGE_WEATHER_ENTRIES,
    StageWeatherTablesLayout,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::normalize_relative_path;

pub const STAGE_WEATHER_TABLES_PROFILE_SCHEMA_VERSION: u32 = 1;
const MAX_EXECUTABLE_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StageWeatherTablesProfile {
    pub schema_version: u32,
    pub id: String,
    pub executable: StageWeatherTablesExecutable,
    pub tables: StageWeatherTablesSpec,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StageWeatherTablesExecutable {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}
/// Virtual addresses of every reviewed table, the reviewed entry counts, and
/// the SHA-256 of the encoded payload the reviewed executable yields (a
/// digest only; the payload itself is never committed).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StageWeatherTablesSpec {
    pub image_base: u32,
    pub school_weather_type_va: u32,
    pub stage_weather_type_va: u32,
    pub school_weather_detail_va: u32,
    pub stage_weather_detail_va: u32,
    pub school_lighting_va: u32,
    pub stage_lighting_va: u32,
    pub school_entries: u32,
    pub stage_entries: u32,
    pub payload_sha256: String,
    pub output_encoding: StageWeatherTablesEncoding,
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum StageWeatherTablesEncoding {
    CmrStageWeatherTablesV1,
}

impl StageWeatherTablesSpec {
    pub fn layout(&self) -> StageWeatherTablesLayout {
        StageWeatherTablesLayout {
            image_base: self.image_base,
            school_weather_type_va: self.school_weather_type_va,
            stage_weather_type_va: self.stage_weather_type_va,
            school_weather_detail_va: self.school_weather_detail_va,
            stage_weather_detail_va: self.stage_weather_detail_va,
            school_lighting_va: self.school_lighting_va,
            stage_lighting_va: self.stage_lighting_va,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ReviewedStageWeatherTablesProfile {
    profile: StageWeatherTablesProfile,
    sha256: String,
}
#[derive(Debug)]
pub enum ReviewedStageWeatherTablesProfileError {
    Json(serde_json::Error),
    Profile(StageWeatherTablesProfileError),
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StageWeatherTablesProfileError {
    UnsupportedSchema(u32),
    EmptyId,
    InvalidExecutablePath(String),
    InvalidExecutableSize(u64),
    InvalidExecutableSha256,
    InvalidPayloadSha256,
    UnsupportedLayout,
}

impl fmt::Display for StageWeatherTablesProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchema(v) => write!(f, "unsupported schema {v}"),
            Self::EmptyId => write!(f, "profile ID is empty"),
            Self::InvalidExecutablePath(p) => write!(f, "invalid executable path {p:?}"),
            Self::InvalidExecutableSize(s) => write!(f, "invalid executable size {s}"),
            Self::InvalidExecutableSha256 => write!(f, "invalid executable SHA-256"),
            Self::InvalidPayloadSha256 => write!(f, "invalid payload SHA-256"),
            Self::UnsupportedLayout => write!(f, "unsupported stage-weather-tables layout"),
        }
    }
}
impl std::error::Error for StageWeatherTablesProfileError {}
impl fmt::Display for ReviewedStageWeatherTablesProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(e) => write!(f, "invalid stage-weather-tables profile JSON: {e}"),
            Self::Profile(e) => write!(f, "invalid stage-weather-tables profile: {e}"),
        }
    }
}
impl std::error::Error for ReviewedStageWeatherTablesProfileError {}

fn is_lower_sha256(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl StageWeatherTablesProfile {
    pub fn validate(&self) -> Result<(), StageWeatherTablesProfileError> {
        if self.schema_version != STAGE_WEATHER_TABLES_PROFILE_SCHEMA_VERSION {
            return Err(StageWeatherTablesProfileError::UnsupportedSchema(
                self.schema_version,
            ));
        }
        if self.id.trim().is_empty() {
            return Err(StageWeatherTablesProfileError::EmptyId);
        }
        if normalize_relative_path(&self.executable.path).as_deref()
            != Some(self.executable.path.as_str())
        {
            return Err(StageWeatherTablesProfileError::InvalidExecutablePath(
                self.executable.path.clone(),
            ));
        }
        if self.executable.size == 0 || self.executable.size > MAX_EXECUTABLE_BYTES {
            return Err(StageWeatherTablesProfileError::InvalidExecutableSize(
                self.executable.size,
            ));
        }
        if !is_lower_sha256(&self.executable.sha256) {
            return Err(StageWeatherTablesProfileError::InvalidExecutableSha256);
        }
        if !is_lower_sha256(&self.tables.payload_sha256) {
            return Err(StageWeatherTablesProfileError::InvalidPayloadSha256);
        }
        if self.tables.layout() != OFFICIAL_EU_STAGE_WEATHER_TABLES_LAYOUT
            || self.tables.school_entries != SCHOOL_WEATHER_ENTRIES as u32
            || self.tables.stage_entries != STAGE_WEATHER_ENTRIES as u32
            || self.tables.output_encoding != StageWeatherTablesEncoding::CmrStageWeatherTablesV1
        {
            return Err(StageWeatherTablesProfileError::UnsupportedLayout);
        }
        Ok(())
    }
}
impl ReviewedStageWeatherTablesProfile {
    pub fn from_json(document: &[u8]) -> Result<Self, ReviewedStageWeatherTablesProfileError> {
        let profile: StageWeatherTablesProfile = serde_json::from_slice(document)
            .map_err(ReviewedStageWeatherTablesProfileError::Json)?;
        profile
            .validate()
            .map_err(ReviewedStageWeatherTablesProfileError::Profile)?;
        Ok(Self {
            profile,
            sha256: Sha256::digest(document)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
        })
    }
    pub fn profile(&self) -> &StageWeatherTablesProfile {
        &self.profile
    }
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use cmr_formats::pe_stage_weather_tables::{StageWeatherTables, encode_stage_weather_tables};

    /// Synthetic tables; not game data.
    pub(crate) fn synthetic_tables() -> StageWeatherTables {
        StageWeatherTables {
            school_type: std::array::from_fn(|i| i as u32),
            stage_type: std::array::from_fn(|i| 100 + i as u32),
            school_detail: std::array::from_fn(|i| 200 + i as u32),
            stage_detail: std::array::from_fn(|i| 300 + i as u32),
            school_lighting: std::array::from_fn(|i| 400 + i as u32),
            stage_lighting: std::array::from_fn(|i| u32::MAX - i as u32),
        }
    }
    pub(crate) fn synthetic_payload_sha256() -> String {
        crate::verify::sha256(&encode_stage_weather_tables(&synthetic_tables()))
    }
    /// A profile whose payload digest matches [`synthetic_tables`].
    pub(crate) fn profile(size: u64, hash: String) -> StageWeatherTablesProfile {
        let l = OFFICIAL_EU_STAGE_WEATHER_TABLES_LAYOUT;
        StageWeatherTablesProfile {
            schema_version: 1,
            id: "synthetic-stage-weather-tables".into(),
            executable: StageWeatherTablesExecutable {
                path: "Rally.exe".into(),
                size,
                sha256: hash,
            },
            tables: StageWeatherTablesSpec {
                image_base: l.image_base,
                school_weather_type_va: l.school_weather_type_va,
                stage_weather_type_va: l.stage_weather_type_va,
                school_weather_detail_va: l.school_weather_detail_va,
                stage_weather_detail_va: l.stage_weather_detail_va,
                school_lighting_va: l.school_lighting_va,
                stage_lighting_va: l.stage_lighting_va,
                school_entries: SCHOOL_WEATHER_ENTRIES as u32,
                stage_entries: STAGE_WEATHER_ENTRIES as u32,
                payload_sha256: synthetic_payload_sha256(),
                output_encoding: StageWeatherTablesEncoding::CmrStageWeatherTablesV1,
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
        candidates.push((p, StageWeatherTablesProfileError::UnsupportedSchema(2)));
        let mut p = base.clone();
        p.id = " \t".into();
        candidates.push((p, StageWeatherTablesProfileError::EmptyId));
        for path in ["../Rally.exe", "/Rally.exe", "dir\\Rally.exe"] {
            let mut p = base.clone();
            p.executable.path = path.into();
            candidates.push((
                p,
                StageWeatherTablesProfileError::InvalidExecutablePath(path.into()),
            ));
        }
        for size in [0, MAX_EXECUTABLE_BYTES + 1] {
            let mut p = base.clone();
            p.executable.size = size;
            candidates.push((
                p,
                StageWeatherTablesProfileError::InvalidExecutableSize(size),
            ));
        }
        for hash in ["00".to_owned(), "AA".repeat(32), "gg".repeat(32)] {
            let mut p = base.clone();
            p.executable.sha256 = hash.clone();
            candidates.push((p, StageWeatherTablesProfileError::InvalidExecutableSha256));
            let mut p = base.clone();
            p.tables.payload_sha256 = hash;
            candidates.push((p, StageWeatherTablesProfileError::InvalidPayloadSha256));
        }
        for field in 0..9 {
            let mut p = base.clone();
            let t = &mut p.tables;
            match field {
                0 => t.image_base += 1,
                1 => t.school_weather_type_va += 4,
                2 => t.stage_weather_type_va -= 4,
                3 => t.school_weather_detail_va += 1,
                4 => t.stage_weather_detail_va += 4,
                5 => t.school_lighting_va -= 4,
                6 => t.stage_lighting_va += 4,
                7 => t.school_entries -= 1,
                _ => t.stage_entries += 1,
            };
            candidates.push((p, StageWeatherTablesProfileError::UnsupportedLayout));
        }
        for (candidate, expected) in candidates {
            assert_eq!(candidate.validate(), Err(expected));
        }

        let mut value = serde_json::to_value(&base).unwrap();
        value["tables"]["output_encoding"] = serde_json::Value::String("unknown".into());
        assert!(matches!(
            ReviewedStageWeatherTablesProfile::from_json(&serde_json::to_vec(&value).unwrap()),
            Err(ReviewedStageWeatherTablesProfileError::Json(_))
        ));
        let mut value = serde_json::to_value(&base).unwrap();
        value["payload"] = serde_json::json!([1, 2, 3]);
        assert!(matches!(
            ReviewedStageWeatherTablesProfile::from_json(&serde_json::to_vec(&value).unwrap()),
            Err(ReviewedStageWeatherTablesProfileError::Json(_))
        ));
        let mut value = serde_json::to_value(&base).unwrap();
        value["tables"]["stage_type"] = serde_json::json!([0, 1]);
        assert!(matches!(
            ReviewedStageWeatherTablesProfile::from_json(&serde_json::to_vec(&value).unwrap()),
            Err(ReviewedStageWeatherTablesProfileError::Json(_))
        ));
        let mut value = serde_json::to_value(&base).unwrap();
        value["tables"]
            .as_object_mut()
            .unwrap()
            .remove("payload_sha256");
        assert!(matches!(
            ReviewedStageWeatherTablesProfile::from_json(&serde_json::to_vec(&value).unwrap()),
            Err(ReviewedStageWeatherTablesProfileError::Json(_))
        ));
        ReviewedStageWeatherTablesProfile::from_json(&serde_json::to_vec(&base).unwrap()).unwrap();
    }

    #[test]
    fn committed_official_profile_is_metadata_only_and_reviewable() {
        let document = include_bytes!("../../../profiles/official-eu-stage-weather-tables-v1.json");
        let reviewed = ReviewedStageWeatherTablesProfile::from_json(document).unwrap();
        assert_eq!(reviewed.profile().id, "official-eu-stage-weather-tables-v1");
        assert_eq!(reviewed.profile().executable.path, "Rally.exe");
        assert_eq!(reviewed.profile().tables.school_entries, 15);
        assert_eq!(reviewed.profile().tables.stage_entries, 63);
        // Same executable identity as the reviewed surface-audio profile.
        let surface = crate::ReviewedSurfaceAudioTablesProfile::from_json(include_bytes!(
            "../../../profiles/official-eu-surface-audio-tables-v1.json"
        ))
        .unwrap();
        assert_eq!(
            serde_json::to_value(&reviewed.profile().executable).unwrap(),
            serde_json::to_value(&surface.profile().executable).unwrap()
        );
        let text = std::str::from_utf8(document).unwrap();
        assert!(!text.contains("/mnt/"));
        assert!(!text.contains("runtime-data"));
        // Metadata only: no array of table words.
        assert!(!text.contains('['));
    }
}
