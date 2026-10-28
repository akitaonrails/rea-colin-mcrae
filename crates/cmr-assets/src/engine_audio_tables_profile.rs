use std::fmt;

use cmr_formats::pe_engine_audio_tables::{
    ENGINE_UPSHIFT_DIP_ENTRIES, EngineAudioTablesLayout, EngineSoundBandLayout,
    OFFICIAL_EU_ENGINE_AUDIO_TABLES_LAYOUT,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::normalize_relative_path;

pub const ENGINE_AUDIO_TABLES_PROFILE_SCHEMA_VERSION: u32 = 1;
const MAX_EXECUTABLE_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EngineAudioTablesProfile {
    pub schema_version: u32,
    pub id: String,
    pub executable: EngineAudioTablesExecutable,
    pub tables: EngineAudioTablesSpec,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EngineAudioTablesExecutable {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}
/// Virtual addresses of one rev-band record (`count, lo, hi, pitch[N], vol[N]`).
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EngineSoundBandSpec {
    pub base_va: u32,
    pub pitch_va: u32,
    pub vol_va: u32,
    pub entries: u32,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EngineAudioTablesSpec {
    pub image_base: u32,
    pub slow: EngineSoundBandSpec,
    pub mid: EngineSoundBandSpec,
    pub top: EngineSoundBandSpec,
    pub whine: EngineSoundBandSpec,
    pub upshift_dip_va: u32,
    pub upshift_dip_entries: u32,
    pub output_encoding: EngineAudioTablesEncoding,
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum EngineAudioTablesEncoding {
    CmrEngineAudioTablesV1,
}

impl EngineSoundBandSpec {
    fn layout(self) -> EngineSoundBandLayout {
        EngineSoundBandLayout {
            base_va: self.base_va,
            pitch_va: self.pitch_va,
            vol_va: self.vol_va,
            entries: self.entries,
        }
    }
}

impl EngineAudioTablesSpec {
    pub fn layout(&self) -> EngineAudioTablesLayout {
        EngineAudioTablesLayout {
            image_base: self.image_base,
            slow: self.slow.layout(),
            mid: self.mid.layout(),
            top: self.top.layout(),
            whine: self.whine.layout(),
            upshift_dip_va: self.upshift_dip_va,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ReviewedEngineAudioTablesProfile {
    profile: EngineAudioTablesProfile,
    sha256: String,
}
#[derive(Debug)]
pub enum ReviewedEngineAudioTablesProfileError {
    Json(serde_json::Error),
    Profile(EngineAudioTablesProfileError),
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EngineAudioTablesProfileError {
    UnsupportedSchema(u32),
    EmptyId,
    InvalidExecutablePath(String),
    InvalidExecutableSize(u64),
    InvalidExecutableSha256,
    UnsupportedLayout,
}

impl fmt::Display for EngineAudioTablesProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchema(v) => write!(f, "unsupported schema {v}"),
            Self::EmptyId => write!(f, "profile ID is empty"),
            Self::InvalidExecutablePath(p) => write!(f, "invalid executable path {p:?}"),
            Self::InvalidExecutableSize(s) => write!(f, "invalid executable size {s}"),
            Self::InvalidExecutableSha256 => write!(f, "invalid executable SHA-256"),
            Self::UnsupportedLayout => write!(f, "unsupported engine-audio-tables layout"),
        }
    }
}
impl std::error::Error for EngineAudioTablesProfileError {}
impl fmt::Display for ReviewedEngineAudioTablesProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(e) => write!(f, "invalid engine-audio-tables profile JSON: {e}"),
            Self::Profile(e) => write!(f, "invalid engine-audio-tables profile: {e}"),
        }
    }
}
impl std::error::Error for ReviewedEngineAudioTablesProfileError {}

impl EngineAudioTablesProfile {
    pub fn validate(&self) -> Result<(), EngineAudioTablesProfileError> {
        if self.schema_version != ENGINE_AUDIO_TABLES_PROFILE_SCHEMA_VERSION {
            return Err(EngineAudioTablesProfileError::UnsupportedSchema(
                self.schema_version,
            ));
        }
        if self.id.trim().is_empty() {
            return Err(EngineAudioTablesProfileError::EmptyId);
        }
        if normalize_relative_path(&self.executable.path).as_deref()
            != Some(self.executable.path.as_str())
        {
            return Err(EngineAudioTablesProfileError::InvalidExecutablePath(
                self.executable.path.clone(),
            ));
        }
        if self.executable.size == 0 || self.executable.size > MAX_EXECUTABLE_BYTES {
            return Err(EngineAudioTablesProfileError::InvalidExecutableSize(
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
            return Err(EngineAudioTablesProfileError::InvalidExecutableSha256);
        }
        if self.tables.layout() != OFFICIAL_EU_ENGINE_AUDIO_TABLES_LAYOUT
            || self.tables.upshift_dip_entries != ENGINE_UPSHIFT_DIP_ENTRIES as u32
            || self.tables.output_encoding != EngineAudioTablesEncoding::CmrEngineAudioTablesV1
        {
            return Err(EngineAudioTablesProfileError::UnsupportedLayout);
        }
        Ok(())
    }
}
impl ReviewedEngineAudioTablesProfile {
    pub fn from_json(document: &[u8]) -> Result<Self, ReviewedEngineAudioTablesProfileError> {
        let profile: EngineAudioTablesProfile = serde_json::from_slice(document)
            .map_err(ReviewedEngineAudioTablesProfileError::Json)?;
        profile
            .validate()
            .map_err(ReviewedEngineAudioTablesProfileError::Profile)?;
        Ok(Self {
            profile,
            sha256: Sha256::digest(document)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
        })
    }
    pub fn profile(&self) -> &EngineAudioTablesProfile {
        &self.profile
    }
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use cmr_formats::pe_engine_audio_tables::{
        EngineSoundBand, EngineSoundBandId, EngineSoundTables,
    };
    fn band(l: EngineSoundBandLayout) -> EngineSoundBandSpec {
        EngineSoundBandSpec {
            base_va: l.base_va,
            pitch_va: l.pitch_va,
            vol_va: l.vol_va,
            entries: l.entries,
        }
    }
    /// Synthetic tables that satisfy every band invariant; not game data.
    pub(crate) fn synthetic_tables() -> EngineSoundTables {
        let band = |id: EngineSoundBandId, seed: u16| {
            let n = id.entries();
            EngineSoundBand {
                count: n as i32 - 1,
                lo: -i32::from(seed),
                hi: i32::from(seed) * 1000,
                pitch: (0..n).map(|i| seed ^ i as u16).collect(),
                vol: (0..n).map(|i| (i as u8) ^ seed as u8).collect(),
            }
        };
        EngineSoundTables {
            slow: band(EngineSoundBandId::Slow, 1),
            mid: band(EngineSoundBandId::Mid, 2),
            top: band(EngineSoundBandId::Top, 3),
            whine: band(EngineSoundBandId::Whine, 4),
            upshift_dip: std::array::from_fn(|i| i as i32 - 12),
        }
    }
    pub(crate) fn profile(size: u64, hash: String) -> EngineAudioTablesProfile {
        let l = OFFICIAL_EU_ENGINE_AUDIO_TABLES_LAYOUT;
        EngineAudioTablesProfile {
            schema_version: 1,
            id: "synthetic-engine-audio-tables".into(),
            executable: EngineAudioTablesExecutable {
                path: "Rally.exe".into(),
                size,
                sha256: hash,
            },
            tables: EngineAudioTablesSpec {
                image_base: l.image_base,
                slow: band(l.slow),
                mid: band(l.mid),
                top: band(l.top),
                whine: band(l.whine),
                upshift_dip_va: l.upshift_dip_va,
                upshift_dip_entries: ENGINE_UPSHIFT_DIP_ENTRIES as u32,
                output_encoding: EngineAudioTablesEncoding::CmrEngineAudioTablesV1,
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
        candidates.push((p, EngineAudioTablesProfileError::UnsupportedSchema(2)));
        let mut p = base.clone();
        p.id = " \t".into();
        candidates.push((p, EngineAudioTablesProfileError::EmptyId));
        for path in ["../Rally.exe", "/Rally.exe", "dir\\Rally.exe"] {
            let mut p = base.clone();
            p.executable.path = path.into();
            candidates.push((
                p,
                EngineAudioTablesProfileError::InvalidExecutablePath(path.into()),
            ));
        }
        for size in [0, MAX_EXECUTABLE_BYTES + 1] {
            let mut p = base.clone();
            p.executable.size = size;
            candidates.push((
                p,
                EngineAudioTablesProfileError::InvalidExecutableSize(size),
            ));
        }
        for hash in ["00".to_owned(), "AA".repeat(32), "gg".repeat(32)] {
            let mut p = base.clone();
            p.executable.sha256 = hash;
            candidates.push((p, EngineAudioTablesProfileError::InvalidExecutableSha256));
        }
        for field in 0..10 {
            let mut p = base.clone();
            match field {
                0 => p.tables.image_base += 1,
                1 => p.tables.slow.base_va += 4,
                2 => p.tables.mid.pitch_va += 2,
                3 => p.tables.top.vol_va += 1,
                4 => p.tables.whine.entries -= 1,
                5 => p.tables.top.base_va -= 4,
                6 => p.tables.whine.vol_va += 2,
                7 => p.tables.upshift_dip_va += 4,
                8 => p.tables.upshift_dip_entries -= 1,
                _ => p.tables.slow.entries += 1,
            };
            candidates.push((p, EngineAudioTablesProfileError::UnsupportedLayout));
        }
        for (candidate, expected) in candidates {
            assert_eq!(candidate.validate(), Err(expected));
        }

        let mut value = serde_json::to_value(&base).unwrap();
        value["tables"]["output_encoding"] = serde_json::Value::String("unknown".into());
        assert!(matches!(
            ReviewedEngineAudioTablesProfile::from_json(&serde_json::to_vec(&value).unwrap()),
            Err(ReviewedEngineAudioTablesProfileError::Json(_))
        ));
        let mut value = serde_json::to_value(&base).unwrap();
        value["payload"] = serde_json::json!([1, 2, 3]);
        assert!(matches!(
            ReviewedEngineAudioTablesProfile::from_json(&serde_json::to_vec(&value).unwrap()),
            Err(ReviewedEngineAudioTablesProfileError::Json(_))
        ));
        let mut value = serde_json::to_value(&base).unwrap();
        value["tables"]["mid"]["pitch"] = serde_json::json!([1]);
        assert!(matches!(
            ReviewedEngineAudioTablesProfile::from_json(&serde_json::to_vec(&value).unwrap()),
            Err(ReviewedEngineAudioTablesProfileError::Json(_))
        ));
        ReviewedEngineAudioTablesProfile::from_json(&serde_json::to_vec(&base).unwrap()).unwrap();
    }

    #[test]
    fn committed_official_profile_is_metadata_only_and_reviewable() {
        let document = include_bytes!("../../../profiles/official-eu-engine-audio-tables-v1.json");
        let reviewed = ReviewedEngineAudioTablesProfile::from_json(document).unwrap();
        assert_eq!(reviewed.profile().id, "official-eu-engine-audio-tables-v1");
        assert_eq!(reviewed.profile().executable.path, "Rally.exe");
        assert_eq!(reviewed.profile().tables.whine.entries, 194);
        // Same executable identity as the reviewed tyre-table profile.
        let tyre = crate::ReviewedTyreTablesProfile::from_json(include_bytes!(
            "../../../profiles/official-eu-tyre-surface-tables-v1.json"
        ))
        .unwrap();
        assert_eq!(
            serde_json::to_value(&reviewed.profile().executable).unwrap(),
            serde_json::to_value(&tyre.profile().executable).unwrap()
        );
        let text = std::str::from_utf8(document).unwrap();
        assert!(!text.contains("/mnt/"));
        assert!(!text.contains("runtime-data"));
    }
}
