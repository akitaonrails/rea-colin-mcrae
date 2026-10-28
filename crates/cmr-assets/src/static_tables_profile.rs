use std::fmt;

use cmr_formats::pe_static_tables::{
    OFFICIAL_EU_STATIC_TABLES_IMAGE_BASE, StaticTableEntry, StaticTableKind, StaticTablesError,
    StaticTablesLayout, check_static_tables_layout,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::normalize_relative_path;

pub const STATIC_TABLES_PROFILE_SCHEMA_VERSION: u32 = 1;
const MAX_EXECUTABLE_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StaticTablesProfile {
    pub schema_version: u32,
    pub id: String,
    pub executable: StaticTablesExecutable,
    pub tables: StaticTablesSpec,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StaticTablesExecutable {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}
/// The reviewed named spans, and the SHA-256 of the encoded payload the
/// reviewed executable yields (a digest only; the payload itself is never
/// committed).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StaticTablesSpec {
    pub image_base: u32,
    pub entries: Vec<StaticTablesEntrySpec>,
    pub payload_sha256: String,
    pub output_encoding: StaticTablesEncoding,
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum StaticTablesEntryKind {
    Block,
    StringPointers,
    String,
}
/// One reviewed entry. `len` is required for (and only for) a block, `count`
/// for (and only for) a string-pointer table.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StaticTablesEntrySpec {
    pub name: String,
    pub kind: StaticTablesEntryKind,
    pub va: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub len: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum StaticTablesEncoding {
    CmrStaticTablesV1,
}

impl StaticTablesEntrySpec {
    fn kind(&self) -> Option<StaticTableKind> {
        match (self.kind, self.len, self.count) {
            (StaticTablesEntryKind::Block, Some(len), None) => Some(StaticTableKind::Block { len }),
            (StaticTablesEntryKind::StringPointers, None, Some(count)) => {
                Some(StaticTableKind::StringPointers { count })
            }
            (StaticTablesEntryKind::String, None, None) => Some(StaticTableKind::String),
            _ => None,
        }
    }
}

impl StaticTablesSpec {
    /// The format-level layout, or `None` when an entry's `len`/`count` do
    /// not fit its kind.
    pub fn layout(&self) -> Option<StaticTablesLayout> {
        let entries = self
            .entries
            .iter()
            .map(|e| {
                Some(StaticTableEntry {
                    name: e.name.clone(),
                    va: e.va,
                    kind: e.kind()?,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(StaticTablesLayout {
            image_base: self.image_base,
            entries,
        })
    }
}

#[derive(Clone, Debug)]
pub struct ReviewedStaticTablesProfile {
    profile: StaticTablesProfile,
    layout: StaticTablesLayout,
    sha256: String,
}
#[derive(Debug)]
pub enum ReviewedStaticTablesProfileError {
    Json(serde_json::Error),
    Profile(StaticTablesProfileError),
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StaticTablesProfileError {
    UnsupportedSchema(u32),
    EmptyId,
    InvalidExecutablePath(String),
    InvalidExecutableSize(u64),
    InvalidExecutableSha256,
    InvalidPayloadSha256,
    /// The image base is not the reviewed one.
    UnsupportedImageBase(u32),
    /// Entry `name` gives `len`/`count` that do not fit its kind.
    EntryShape(String),
    /// The entry list is structurally invalid (names, sizes, alignment).
    Layout(StaticTablesError),
}

impl fmt::Display for StaticTablesProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchema(v) => write!(f, "unsupported schema {v}"),
            Self::EmptyId => write!(f, "profile ID is empty"),
            Self::InvalidExecutablePath(p) => write!(f, "invalid executable path {p:?}"),
            Self::InvalidExecutableSize(s) => write!(f, "invalid executable size {s}"),
            Self::InvalidExecutableSha256 => write!(f, "invalid executable SHA-256"),
            Self::InvalidPayloadSha256 => write!(f, "invalid payload SHA-256"),
            Self::UnsupportedImageBase(b) => write!(f, "unsupported image base {b:#x}"),
            Self::EntryShape(n) => {
                write!(f, "entry {n:?}: len/count do not match its kind")
            }
            Self::Layout(e) => write!(f, "invalid static-tables layout: {e}"),
        }
    }
}
impl std::error::Error for StaticTablesProfileError {}
impl fmt::Display for ReviewedStaticTablesProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(e) => write!(f, "invalid static-tables profile JSON: {e}"),
            Self::Profile(e) => write!(f, "invalid static-tables profile: {e}"),
        }
    }
}
impl std::error::Error for ReviewedStaticTablesProfileError {}

fn is_lower_sha256(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl StaticTablesProfile {
    /// Validate the profile and return its format-level layout.
    pub fn validate(&self) -> Result<StaticTablesLayout, StaticTablesProfileError> {
        if self.schema_version != STATIC_TABLES_PROFILE_SCHEMA_VERSION {
            return Err(StaticTablesProfileError::UnsupportedSchema(
                self.schema_version,
            ));
        }
        if self.id.trim().is_empty() {
            return Err(StaticTablesProfileError::EmptyId);
        }
        if normalize_relative_path(&self.executable.path).as_deref()
            != Some(self.executable.path.as_str())
        {
            return Err(StaticTablesProfileError::InvalidExecutablePath(
                self.executable.path.clone(),
            ));
        }
        if self.executable.size == 0 || self.executable.size > MAX_EXECUTABLE_BYTES {
            return Err(StaticTablesProfileError::InvalidExecutableSize(
                self.executable.size,
            ));
        }
        if !is_lower_sha256(&self.executable.sha256) {
            return Err(StaticTablesProfileError::InvalidExecutableSha256);
        }
        if !is_lower_sha256(&self.tables.payload_sha256) {
            return Err(StaticTablesProfileError::InvalidPayloadSha256);
        }
        if self.tables.image_base != OFFICIAL_EU_STATIC_TABLES_IMAGE_BASE {
            return Err(StaticTablesProfileError::UnsupportedImageBase(
                self.tables.image_base,
            ));
        }
        if let Some(bad) = self.tables.entries.iter().find(|e| e.kind().is_none()) {
            return Err(StaticTablesProfileError::EntryShape(bad.name.clone()));
        }
        let layout = self.tables.layout().expect("entry shapes checked above");
        check_static_tables_layout(&layout).map_err(StaticTablesProfileError::Layout)?;
        Ok(layout)
    }
}
impl ReviewedStaticTablesProfile {
    pub fn from_json(document: &[u8]) -> Result<Self, ReviewedStaticTablesProfileError> {
        let profile: StaticTablesProfile =
            serde_json::from_slice(document).map_err(ReviewedStaticTablesProfileError::Json)?;
        let layout = profile
            .validate()
            .map_err(ReviewedStaticTablesProfileError::Profile)?;
        Ok(Self {
            profile,
            layout,
            sha256: Sha256::digest(document)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
        })
    }
    pub fn profile(&self) -> &StaticTablesProfile {
        &self.profile
    }
    /// The validated format-level layout of the profile's entries.
    pub fn layout(&self) -> &StaticTablesLayout {
        &self.layout
    }
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use cmr_formats::pe_static_tables::{
        StaticStringTable, StaticTable, StaticTableData, StaticTables, encode_static_tables,
    };

    /// Synthetic tables matching [`profile`]'s entries; not game data.
    pub(crate) fn synthetic_tables() -> StaticTables {
        StaticTables::new(vec![
            StaticTable {
                name: "synthetic_block".into(),
                va: 0x0041_0010,
                data: StaticTableData::Block(vec![1, 2, 3, 4]),
            },
            StaticTable {
                name: "synthetic_names".into(),
                va: 0x0041_0100,
                data: StaticTableData::StringPointers(StaticStringTable {
                    pointers: vec![0x0041_0200, 0x0041_0210],
                    strings: vec![b"Alpha".to_vec(), b"Beta".to_vec()],
                }),
            },
            StaticTable {
                name: "synthetic_name".into(),
                va: 0x0041_0300,
                data: StaticTableData::String(b"Gamma".to_vec()),
            },
        ])
        .unwrap()
    }
    pub(crate) fn synthetic_payload_sha256() -> String {
        crate::verify::sha256(&encode_static_tables(&synthetic_tables()))
    }
    /// A profile whose entries and payload digest match [`synthetic_tables`].
    pub(crate) fn profile(size: u64, hash: String) -> StaticTablesProfile {
        let entry = |name: &str, kind, va, len, count| StaticTablesEntrySpec {
            name: name.into(),
            kind,
            va,
            len,
            count,
        };
        StaticTablesProfile {
            schema_version: 1,
            id: "synthetic-static-tables".into(),
            executable: StaticTablesExecutable {
                path: "Rally.exe".into(),
                size,
                sha256: hash,
            },
            tables: StaticTablesSpec {
                image_base: OFFICIAL_EU_STATIC_TABLES_IMAGE_BASE,
                entries: vec![
                    entry(
                        "synthetic_block",
                        StaticTablesEntryKind::Block,
                        0x0041_0010,
                        Some(4),
                        None,
                    ),
                    entry(
                        "synthetic_names",
                        StaticTablesEntryKind::StringPointers,
                        0x0041_0100,
                        None,
                        Some(2),
                    ),
                    entry(
                        "synthetic_name",
                        StaticTablesEntryKind::String,
                        0x0041_0300,
                        None,
                        None,
                    ),
                ],
                payload_sha256: synthetic_payload_sha256(),
                output_encoding: StaticTablesEncoding::CmrStaticTablesV1,
            },
        }
    }

    #[test]
    fn synthetic_profile_layout_matches_synthetic_tables() {
        let layout = profile(10, "00".repeat(32)).validate().unwrap();
        assert!(synthetic_tables().matches_layout(&layout));
    }

    #[test]
    fn validates_every_trust_boundary() {
        let base = profile(10, "00".repeat(32));
        assert!(base.validate().is_ok());
        let mut candidates = Vec::new();
        let mut p = base.clone();
        p.schema_version = 2;
        candidates.push((p, StaticTablesProfileError::UnsupportedSchema(2)));
        let mut p = base.clone();
        p.id = " \t".into();
        candidates.push((p, StaticTablesProfileError::EmptyId));
        for path in ["../Rally.exe", "/Rally.exe", "dir\\Rally.exe"] {
            let mut p = base.clone();
            p.executable.path = path.into();
            candidates.push((
                p,
                StaticTablesProfileError::InvalidExecutablePath(path.into()),
            ));
        }
        for size in [0, MAX_EXECUTABLE_BYTES + 1] {
            let mut p = base.clone();
            p.executable.size = size;
            candidates.push((p, StaticTablesProfileError::InvalidExecutableSize(size)));
        }
        for hash in ["00".to_owned(), "AA".repeat(32), "gg".repeat(32)] {
            let mut p = base.clone();
            p.executable.sha256 = hash.clone();
            candidates.push((p, StaticTablesProfileError::InvalidExecutableSha256));
            let mut p = base.clone();
            p.tables.payload_sha256 = hash;
            candidates.push((p, StaticTablesProfileError::InvalidPayloadSha256));
        }
        let mut p = base.clone();
        p.tables.image_base += 0x1000;
        candidates.push((
            p,
            StaticTablesProfileError::UnsupportedImageBase(0x0040_1000),
        ));
        for (index, len, count) in [
            (0, None, None),
            (0, Some(4), Some(1)),
            (1, Some(4), Some(2)),
            (1, None, None),
            (2, Some(1), None),
            (2, None, Some(1)),
        ] {
            let mut p = base.clone();
            p.tables.entries[index].len = len;
            p.tables.entries[index].count = count;
            let name = p.tables.entries[index].name.clone();
            candidates.push((p, StaticTablesProfileError::EntryShape(name)));
        }
        let mut p = base.clone();
        p.tables.entries[2].name = "synthetic_block".into();
        candidates.push((
            p,
            StaticTablesProfileError::Layout(StaticTablesError::DuplicateName(
                "synthetic_block".into(),
            )),
        ));
        let mut p = base.clone();
        p.tables.entries[0].len = Some(0);
        candidates.push((
            p,
            StaticTablesProfileError::Layout(StaticTablesError::InvalidSize(
                "synthetic_block".into(),
            )),
        ));
        let mut p = base.clone();
        p.tables.entries.clear();
        candidates.push((
            p,
            StaticTablesProfileError::Layout(StaticTablesError::EntryCount(0)),
        ));
        for (candidate, expected) in candidates {
            assert_eq!(candidate.validate(), Err(expected));
        }

        let mut value = serde_json::to_value(&base).unwrap();
        value["tables"]["output_encoding"] = serde_json::Value::String("unknown".into());
        assert!(matches!(
            ReviewedStaticTablesProfile::from_json(&serde_json::to_vec(&value).unwrap()),
            Err(ReviewedStaticTablesProfileError::Json(_))
        ));
        let mut value = serde_json::to_value(&base).unwrap();
        value["tables"]["entries"][0]["bytes"] = serde_json::json!([1, 2, 3]);
        assert!(matches!(
            ReviewedStaticTablesProfile::from_json(&serde_json::to_vec(&value).unwrap()),
            Err(ReviewedStaticTablesProfileError::Json(_))
        ));
        let mut value = serde_json::to_value(&base).unwrap();
        value["tables"]["entries"][0]["kind"] = serde_json::json!("words");
        assert!(matches!(
            ReviewedStaticTablesProfile::from_json(&serde_json::to_vec(&value).unwrap()),
            Err(ReviewedStaticTablesProfileError::Json(_))
        ));
        let mut value = serde_json::to_value(&base).unwrap();
        value["tables"]
            .as_object_mut()
            .unwrap()
            .remove("payload_sha256");
        assert!(matches!(
            ReviewedStaticTablesProfile::from_json(&serde_json::to_vec(&value).unwrap()),
            Err(ReviewedStaticTablesProfileError::Json(_))
        ));
        let reviewed =
            ReviewedStaticTablesProfile::from_json(&serde_json::to_vec(&base).unwrap()).unwrap();
        assert_eq!(reviewed.layout().entries.len(), 3);
    }

    #[test]
    fn committed_official_profile_is_metadata_only_and_reviewable() {
        let document = include_bytes!("../../../profiles/official-eu-static-tables-v1.json");
        let reviewed = ReviewedStaticTablesProfile::from_json(document).unwrap();
        assert_eq!(reviewed.profile().id, "official-eu-static-tables-v1");
        assert_eq!(reviewed.profile().executable.path, "Rally.exe");
        let entries: Vec<_> = reviewed
            .layout()
            .entries
            .iter()
            .map(|e| (e.name.as_str(), e.va, e.kind))
            .collect();
        use StaticTableKind::{Block, String, StringPointers};
        assert_eq!(
            entries,
            [
                (
                    "frontend_setup_layout_championship",
                    0x0050_92e8,
                    Block { len: 0x50 }
                ),
                (
                    "frontend_setup_layout_rally",
                    0x0050_9338,
                    Block { len: 0x50 }
                ),
                (
                    "frontend_setup_layout_time_trial",
                    0x0050_9388,
                    Block { len: 0x50 }
                ),
                (
                    "frontend_camera_targets_and_records",
                    0x0050_7a78,
                    Block { len: 0x128 }
                ),
                ("frontend_country_table", 0x0050_8850, Block { len: 0x50 }),
                ("results_save_schedule", 0x004e_d8e4, Block { len: 9 }),
                ("bonus_car_countries", 0x0050_6dc8, Block { len: 8 }),
                ("car_names", 0x0050_6dd0, StringPointers { count: 16 }),
                ("stage_names", 0x0050_6f80, StringPointers { count: 54 }),
                ("school_stage_name", 0x0050_7520, String),
                (
                    "frontend_arrow_oscillation",
                    0x0050_8818,
                    Block { len: 0x10 }
                ),
                ("frontend_arrow_left", 0x0050_8834, String),
                ("frontend_arrow_right", 0x0050_8838, String),
                ("car_short_names", 0x0050_6e08, StringPointers { count: 14 }),
                ("car_stats", 0x0050_8e18, Block { len: 0xa8 }),
            ]
        );
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
        // Metadata only: no entry carries payload bytes or strings.
        let value: serde_json::Value = serde_json::from_slice(document).unwrap();
        for entry in value["tables"]["entries"].as_array().unwrap() {
            for (_, field) in entry.as_object().unwrap() {
                assert!(field.is_string() || field.is_u64());
            }
        }
    }
}
