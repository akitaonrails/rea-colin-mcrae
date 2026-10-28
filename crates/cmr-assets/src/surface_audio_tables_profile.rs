use std::fmt;

use cmr_formats::pe_surface_audio_tables::{
    OFFICIAL_EU_SURFACE_AUDIO_TABLES_LAYOUT, PRANG_NAME_COUNT, SURFACE_AUDIO_GROUP_COUNT,
    SURFACE_WET_SURFACE_COUNT, SurfaceAudioTablesLayout,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::normalize_relative_path;

pub const SURFACE_AUDIO_TABLES_PROFILE_SCHEMA_VERSION: u32 = 1;
const MAX_EXECUTABLE_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceAudioTablesProfile {
    pub schema_version: u32,
    pub id: String,
    pub executable: SurfaceAudioTablesExecutable,
    pub tables: SurfaceAudioTablesSpec,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceAudioTablesExecutable {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}
/// Virtual addresses of every reviewed table, plus the reviewed element
/// counts of the variable-looking ones.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceAudioTablesSpec {
    pub image_base: u32,
    pub variant_counts_va: u32,
    pub class_volume_va: u32,
    pub surface_group_va: u32,
    pub wet_surfaces_va: u32,
    pub surface_volume_k_va: u32,
    pub wet_volume_k_va: u32,
    pub crowd_volume_k_va: u32,
    pub prang_full_volume_va: u32,
    pub initial_last_state_va: u32,
    pub prang_names_va: u32,
    pub group_entries: u32,
    pub wet_surface_entries: u32,
    pub prang_name_entries: u32,
    pub output_encoding: SurfaceAudioTablesEncoding,
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SurfaceAudioTablesEncoding {
    CmrSurfaceAudioTablesV1,
}

impl SurfaceAudioTablesSpec {
    pub fn layout(&self) -> SurfaceAudioTablesLayout {
        SurfaceAudioTablesLayout {
            image_base: self.image_base,
            variant_counts_va: self.variant_counts_va,
            class_volume_va: self.class_volume_va,
            surface_group_va: self.surface_group_va,
            wet_surfaces_va: self.wet_surfaces_va,
            surface_volume_k_va: self.surface_volume_k_va,
            wet_volume_k_va: self.wet_volume_k_va,
            crowd_volume_k_va: self.crowd_volume_k_va,
            prang_full_volume_va: self.prang_full_volume_va,
            initial_last_state_va: self.initial_last_state_va,
            prang_names_va: self.prang_names_va,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ReviewedSurfaceAudioTablesProfile {
    profile: SurfaceAudioTablesProfile,
    sha256: String,
}
#[derive(Debug)]
pub enum ReviewedSurfaceAudioTablesProfileError {
    Json(serde_json::Error),
    Profile(SurfaceAudioTablesProfileError),
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SurfaceAudioTablesProfileError {
    UnsupportedSchema(u32),
    EmptyId,
    InvalidExecutablePath(String),
    InvalidExecutableSize(u64),
    InvalidExecutableSha256,
    UnsupportedLayout,
}

impl fmt::Display for SurfaceAudioTablesProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchema(v) => write!(f, "unsupported schema {v}"),
            Self::EmptyId => write!(f, "profile ID is empty"),
            Self::InvalidExecutablePath(p) => write!(f, "invalid executable path {p:?}"),
            Self::InvalidExecutableSize(s) => write!(f, "invalid executable size {s}"),
            Self::InvalidExecutableSha256 => write!(f, "invalid executable SHA-256"),
            Self::UnsupportedLayout => write!(f, "unsupported surface-audio-tables layout"),
        }
    }
}
impl std::error::Error for SurfaceAudioTablesProfileError {}
impl fmt::Display for ReviewedSurfaceAudioTablesProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(e) => write!(f, "invalid surface-audio-tables profile JSON: {e}"),
            Self::Profile(e) => write!(f, "invalid surface-audio-tables profile: {e}"),
        }
    }
}
impl std::error::Error for ReviewedSurfaceAudioTablesProfileError {}

impl SurfaceAudioTablesProfile {
    pub fn validate(&self) -> Result<(), SurfaceAudioTablesProfileError> {
        if self.schema_version != SURFACE_AUDIO_TABLES_PROFILE_SCHEMA_VERSION {
            return Err(SurfaceAudioTablesProfileError::UnsupportedSchema(
                self.schema_version,
            ));
        }
        if self.id.trim().is_empty() {
            return Err(SurfaceAudioTablesProfileError::EmptyId);
        }
        if normalize_relative_path(&self.executable.path).as_deref()
            != Some(self.executable.path.as_str())
        {
            return Err(SurfaceAudioTablesProfileError::InvalidExecutablePath(
                self.executable.path.clone(),
            ));
        }
        if self.executable.size == 0 || self.executable.size > MAX_EXECUTABLE_BYTES {
            return Err(SurfaceAudioTablesProfileError::InvalidExecutableSize(
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
            return Err(SurfaceAudioTablesProfileError::InvalidExecutableSha256);
        }
        if self.tables.layout() != OFFICIAL_EU_SURFACE_AUDIO_TABLES_LAYOUT
            || self.tables.group_entries != SURFACE_AUDIO_GROUP_COUNT as u32
            || self.tables.wet_surface_entries != SURFACE_WET_SURFACE_COUNT as u32
            || self.tables.prang_name_entries != PRANG_NAME_COUNT as u32
            || self.tables.output_encoding != SurfaceAudioTablesEncoding::CmrSurfaceAudioTablesV1
        {
            return Err(SurfaceAudioTablesProfileError::UnsupportedLayout);
        }
        Ok(())
    }
}
impl ReviewedSurfaceAudioTablesProfile {
    pub fn from_json(document: &[u8]) -> Result<Self, ReviewedSurfaceAudioTablesProfileError> {
        let profile: SurfaceAudioTablesProfile = serde_json::from_slice(document)
            .map_err(ReviewedSurfaceAudioTablesProfileError::Json)?;
        profile
            .validate()
            .map_err(ReviewedSurfaceAudioTablesProfileError::Profile)?;
        Ok(Self {
            profile,
            sha256: Sha256::digest(document)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
        })
    }
    pub fn profile(&self) -> &SurfaceAudioTablesProfile {
        &self.profile
    }
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use cmr_formats::pe_surface_audio_tables::{SurfaceAudioTables, SurfaceVariantCounts};

    /// Synthetic tables that satisfy every invariant; not game data.
    pub(crate) fn synthetic_tables() -> SurfaceAudioTables {
        SurfaceAudioTables {
            counts: std::array::from_fn(|g| SurfaceVariantCounts {
                ski: (g % 7) as u8,
                i: (g % 3) as u8,
                sko: (g % 5) as u8,
                o: 6 - (g % 7) as u8,
            }),
            class_volume_percent: std::array::from_fn(|g| {
                std::array::from_fn(|c| (g as i8).wrapping_mul(3).wrapping_sub(c as i8))
            }),
            surface_group: std::array::from_fn(|s| (119 - s) as i32),
            wet_surfaces: std::array::from_fn(|i| i as i32 - 7),
            surface_volume_k: 1,
            wet_volume_k: -2,
            crowd_volume_k: 3,
            prang_full_volume: -4,
            initial_last_state: [5, 6, -7, -8],
            prang_names: (0..PRANG_NAME_COUNT)
                .map(|i| format!("SYN{i}.WAV"))
                .collect(),
        }
    }
    pub(crate) fn profile(size: u64, hash: String) -> SurfaceAudioTablesProfile {
        let l = OFFICIAL_EU_SURFACE_AUDIO_TABLES_LAYOUT;
        SurfaceAudioTablesProfile {
            schema_version: 1,
            id: "synthetic-surface-audio-tables".into(),
            executable: SurfaceAudioTablesExecutable {
                path: "Rally.exe".into(),
                size,
                sha256: hash,
            },
            tables: SurfaceAudioTablesSpec {
                image_base: l.image_base,
                variant_counts_va: l.variant_counts_va,
                class_volume_va: l.class_volume_va,
                surface_group_va: l.surface_group_va,
                wet_surfaces_va: l.wet_surfaces_va,
                surface_volume_k_va: l.surface_volume_k_va,
                wet_volume_k_va: l.wet_volume_k_va,
                crowd_volume_k_va: l.crowd_volume_k_va,
                prang_full_volume_va: l.prang_full_volume_va,
                initial_last_state_va: l.initial_last_state_va,
                prang_names_va: l.prang_names_va,
                group_entries: SURFACE_AUDIO_GROUP_COUNT as u32,
                wet_surface_entries: SURFACE_WET_SURFACE_COUNT as u32,
                prang_name_entries: PRANG_NAME_COUNT as u32,
                output_encoding: SurfaceAudioTablesEncoding::CmrSurfaceAudioTablesV1,
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
        candidates.push((p, SurfaceAudioTablesProfileError::UnsupportedSchema(2)));
        let mut p = base.clone();
        p.id = " \t".into();
        candidates.push((p, SurfaceAudioTablesProfileError::EmptyId));
        for path in ["../Rally.exe", "/Rally.exe", "dir\\Rally.exe"] {
            let mut p = base.clone();
            p.executable.path = path.into();
            candidates.push((
                p,
                SurfaceAudioTablesProfileError::InvalidExecutablePath(path.into()),
            ));
        }
        for size in [0, MAX_EXECUTABLE_BYTES + 1] {
            let mut p = base.clone();
            p.executable.size = size;
            candidates.push((
                p,
                SurfaceAudioTablesProfileError::InvalidExecutableSize(size),
            ));
        }
        for hash in ["00".to_owned(), "AA".repeat(32), "gg".repeat(32)] {
            let mut p = base.clone();
            p.executable.sha256 = hash;
            candidates.push((p, SurfaceAudioTablesProfileError::InvalidExecutableSha256));
        }
        for field in 0..14 {
            let mut p = base.clone();
            let t = &mut p.tables;
            match field {
                0 => t.image_base += 1,
                1 => t.variant_counts_va += 4,
                2 => t.class_volume_va += 1,
                3 => t.surface_group_va -= 4,
                4 => t.wet_surfaces_va += 2,
                5 => t.surface_volume_k_va += 4,
                6 => t.wet_volume_k_va -= 4,
                7 => t.crowd_volume_k_va += 4,
                8 => t.prang_full_volume_va += 4,
                9 => t.initial_last_state_va += 4,
                10 => t.prang_names_va -= 4,
                11 => t.group_entries -= 1,
                12 => t.wet_surface_entries += 1,
                _ => t.prang_name_entries -= 1,
            };
            candidates.push((p, SurfaceAudioTablesProfileError::UnsupportedLayout));
        }
        for (candidate, expected) in candidates {
            assert_eq!(candidate.validate(), Err(expected));
        }

        let mut value = serde_json::to_value(&base).unwrap();
        value["tables"]["output_encoding"] = serde_json::Value::String("unknown".into());
        assert!(matches!(
            ReviewedSurfaceAudioTablesProfile::from_json(&serde_json::to_vec(&value).unwrap()),
            Err(ReviewedSurfaceAudioTablesProfileError::Json(_))
        ));
        let mut value = serde_json::to_value(&base).unwrap();
        value["payload"] = serde_json::json!([1, 2, 3]);
        assert!(matches!(
            ReviewedSurfaceAudioTablesProfile::from_json(&serde_json::to_vec(&value).unwrap()),
            Err(ReviewedSurfaceAudioTablesProfileError::Json(_))
        ));
        let mut value = serde_json::to_value(&base).unwrap();
        value["tables"]["prang_names"] = serde_json::json!(["SWIPE1.WAV"]);
        assert!(matches!(
            ReviewedSurfaceAudioTablesProfile::from_json(&serde_json::to_vec(&value).unwrap()),
            Err(ReviewedSurfaceAudioTablesProfileError::Json(_))
        ));
        ReviewedSurfaceAudioTablesProfile::from_json(&serde_json::to_vec(&base).unwrap()).unwrap();
    }

    #[test]
    fn committed_official_profile_is_metadata_only_and_reviewable() {
        let document = include_bytes!("../../../profiles/official-eu-surface-audio-tables-v1.json");
        let reviewed = ReviewedSurfaceAudioTablesProfile::from_json(document).unwrap();
        assert_eq!(reviewed.profile().id, "official-eu-surface-audio-tables-v1");
        assert_eq!(reviewed.profile().executable.path, "Rally.exe");
        assert_eq!(reviewed.profile().tables.prang_name_entries, 48);
        // Same executable identity as the reviewed engine-audio profile.
        let engine = crate::ReviewedEngineAudioTablesProfile::from_json(include_bytes!(
            "../../../profiles/official-eu-engine-audio-tables-v1.json"
        ))
        .unwrap();
        assert_eq!(
            serde_json::to_value(&reviewed.profile().executable).unwrap(),
            serde_json::to_value(&engine.profile().executable).unwrap()
        );
        let text = std::str::from_utf8(document).unwrap();
        assert!(!text.contains("/mnt/"));
        assert!(!text.contains("runtime-data"));
    }
}
