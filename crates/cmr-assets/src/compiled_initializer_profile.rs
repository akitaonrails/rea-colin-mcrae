use std::fmt;

use cmr_formats::pe_compiled_initializer::{
    COMPILED_CASE_COUNT, COMPILED_CONTROL_WORD_COUNT, COMPILED_STATE_WORD_COUNT,
    CompiledInitializerLayout, OFFICIAL_EU_COMPILED_INITIALIZER_LAYOUT,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::normalize_relative_path;

pub const COMPILED_INITIALIZER_PROFILE_SCHEMA_VERSION: u32 = 1;
const MAX_EXECUTABLE_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompiledInitializerProfile {
    pub schema_version: u32,
    pub id: String,
    pub executable: CompiledInitializerExecutable,
    pub initializer: CompiledInitializerSpec,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompiledInitializerExecutable {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompiledInitializerSpec {
    pub image_base: u32,
    pub zero_seed_va: u32,
    pub esi_seed_va: u32,
    pub edi_seed_va: u32,
    pub seed_call_targets: [u32; 6],
    pub dispatch_va: u32,
    pub jump_table_va: u32,
    pub first_case_va: u32,
    pub common_join_va: u32,
    pub state_pointer_va: u32,
    pub control_pointer_va: u32,
    pub selectors: u32,
    pub state_offsets: [u16; COMPILED_STATE_WORD_COUNT],
    pub control_offsets: [u16; COMPILED_CONTROL_WORD_COUNT],
    pub output_encoding: CompiledInitializerEncoding,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompiledInitializerEncoding {
    CmrCcaseV1,
}

impl CompiledInitializerSpec {
    pub fn layout(&self) -> CompiledInitializerLayout {
        CompiledInitializerLayout {
            image_base: self.image_base,
            zero_seed_va: self.zero_seed_va,
            esi_seed_va: self.esi_seed_va,
            edi_seed_va: self.edi_seed_va,
            seed_call_targets: self.seed_call_targets,
            dispatch_va: self.dispatch_va,
            jump_table_va: self.jump_table_va,
            first_case_va: self.first_case_va,
            common_join_va: self.common_join_va,
            state_pointer_va: self.state_pointer_va,
            control_pointer_va: self.control_pointer_va,
            state_offsets: self.state_offsets,
            control_offsets: self.control_offsets,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ReviewedCompiledInitializerProfile {
    profile: CompiledInitializerProfile,
    sha256: String,
}

#[derive(Debug)]
pub enum ReviewedCompiledInitializerProfileError {
    Json(serde_json::Error),
    Profile(CompiledInitializerProfileError),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompiledInitializerProfileError {
    UnsupportedSchema(u32),
    EmptyId,
    InvalidExecutablePath(String),
    InvalidExecutableSize(u64),
    InvalidExecutableSha256,
    UnsupportedLayout,
    UnsupportedSelectorCount(u32),
}

impl fmt::Display for ReviewedCompiledInitializerProfileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(
                formatter,
                "invalid compiled initializer profile JSON: {error}"
            ),
            Self::Profile(error) => {
                write!(formatter, "invalid compiled initializer profile: {error}")
            }
        }
    }
}

impl std::error::Error for ReviewedCompiledInitializerProfileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            Self::Profile(error) => Some(error),
        }
    }
}

impl fmt::Display for CompiledInitializerProfileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchema(value) => write!(formatter, "unsupported schema {value}"),
            Self::EmptyId => write!(formatter, "profile ID is empty"),
            Self::InvalidExecutablePath(path) => {
                write!(formatter, "invalid executable path {path:?}")
            }
            Self::InvalidExecutableSize(size) => {
                write!(formatter, "invalid executable size {size}")
            }
            Self::InvalidExecutableSha256 => write!(formatter, "invalid executable SHA-256"),
            Self::UnsupportedLayout => write!(
                formatter,
                "unsupported compiled initializer address or offset layout"
            ),
            Self::UnsupportedSelectorCount(count) => {
                write!(formatter, "unsupported selector count {count}")
            }
        }
    }
}

impl std::error::Error for CompiledInitializerProfileError {}

impl ReviewedCompiledInitializerProfile {
    pub fn from_json(document: &[u8]) -> Result<Self, ReviewedCompiledInitializerProfileError> {
        let profile: CompiledInitializerProfile = serde_json::from_slice(document)
            .map_err(ReviewedCompiledInitializerProfileError::Json)?;
        profile
            .validate()
            .map_err(ReviewedCompiledInitializerProfileError::Profile)?;
        Ok(Self {
            profile,
            sha256: encode_digest(Sha256::digest(document)),
        })
    }

    pub fn profile(&self) -> &CompiledInitializerProfile {
        &self.profile
    }

    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

impl CompiledInitializerProfile {
    pub fn validate(&self) -> Result<(), CompiledInitializerProfileError> {
        if self.schema_version != COMPILED_INITIALIZER_PROFILE_SCHEMA_VERSION {
            return Err(CompiledInitializerProfileError::UnsupportedSchema(
                self.schema_version,
            ));
        }
        if self.id.trim().is_empty() {
            return Err(CompiledInitializerProfileError::EmptyId);
        }
        if normalize_relative_path(&self.executable.path).as_deref()
            != Some(self.executable.path.as_str())
        {
            return Err(CompiledInitializerProfileError::InvalidExecutablePath(
                self.executable.path.clone(),
            ));
        }
        if self.executable.size == 0 || self.executable.size > MAX_EXECUTABLE_BYTES {
            return Err(CompiledInitializerProfileError::InvalidExecutableSize(
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
            return Err(CompiledInitializerProfileError::InvalidExecutableSha256);
        }
        if self.initializer.selectors != COMPILED_CASE_COUNT as u32 {
            return Err(CompiledInitializerProfileError::UnsupportedSelectorCount(
                self.initializer.selectors,
            ));
        }
        if self.initializer.layout() != OFFICIAL_EU_COMPILED_INITIALIZER_LAYOUT {
            return Err(CompiledInitializerProfileError::UnsupportedLayout);
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
pub(crate) mod tests {
    use super::*;

    pub(crate) fn profile(size: u64, sha256: String) -> CompiledInitializerProfile {
        let layout = OFFICIAL_EU_COMPILED_INITIALIZER_LAYOUT;
        CompiledInitializerProfile {
            schema_version: 1,
            id: "synthetic-compiled-cases".into(),
            executable: CompiledInitializerExecutable {
                path: "Rally.exe".into(),
                size,
                sha256,
            },
            initializer: CompiledInitializerSpec {
                image_base: layout.image_base,
                zero_seed_va: layout.zero_seed_va,
                esi_seed_va: layout.esi_seed_va,
                edi_seed_va: layout.edi_seed_va,
                seed_call_targets: layout.seed_call_targets,
                dispatch_va: layout.dispatch_va,
                jump_table_va: layout.jump_table_va,
                first_case_va: layout.first_case_va,
                common_join_va: layout.common_join_va,
                state_pointer_va: layout.state_pointer_va,
                control_pointer_va: layout.control_pointer_va,
                selectors: COMPILED_CASE_COUNT as u32,
                state_offsets: layout.state_offsets,
                control_offsets: layout.control_offsets,
                output_encoding: CompiledInitializerEncoding::CmrCcaseV1,
            },
        }
    }

    #[test]
    fn exact_json_digest_and_every_profile_boundary_are_checked() {
        let base = profile(1234, "00".repeat(32));
        let compact = serde_json::to_vec(&base).unwrap();
        let pretty = serde_json::to_vec_pretty(&base).unwrap();
        assert_ne!(
            ReviewedCompiledInitializerProfile::from_json(&compact)
                .unwrap()
                .sha256(),
            ReviewedCompiledInitializerProfile::from_json(&pretty)
                .unwrap()
                .sha256()
        );

        let mut candidate = base.clone();
        candidate.executable.path = "../Rally.exe".into();
        assert!(candidate.validate().is_err());
        for path in [
            "/Rally.exe",
            "C:/Rally.exe",
            "bin\\Rally.exe",
            "./Rally.exe",
        ] {
            candidate = base.clone();
            candidate.executable.path = path.into();
            assert!(candidate.validate().is_err());
        }
        candidate = base.clone();
        candidate.executable.size = 0;
        assert!(candidate.validate().is_err());
        candidate = base.clone();
        candidate.executable.size = MAX_EXECUTABLE_BYTES + 1;
        assert!(candidate.validate().is_err());
        candidate = base.clone();
        candidate.executable.sha256 = "AA".repeat(32);
        assert!(candidate.validate().is_err());
        for invalid_hash in ["0".repeat(63), format!("{}g", "0".repeat(63))] {
            candidate = base.clone();
            candidate.executable.sha256 = invalid_hash;
            assert_eq!(
                candidate.validate(),
                Err(CompiledInitializerProfileError::InvalidExecutableSha256)
            );
        }
        candidate = base.clone();
        candidate.schema_version += 1;
        assert!(matches!(
            candidate.validate(),
            Err(CompiledInitializerProfileError::UnsupportedSchema(_))
        ));
        candidate = base.clone();
        candidate.id = "  ".into();
        assert_eq!(
            candidate.validate(),
            Err(CompiledInitializerProfileError::EmptyId)
        );

        let layout_mutations: [fn(&mut CompiledInitializerSpec); 13] = [
            |spec| spec.image_base += 1,
            |spec| spec.zero_seed_va += 1,
            |spec| spec.esi_seed_va += 1,
            |spec| spec.edi_seed_va += 1,
            |spec| spec.seed_call_targets[0] += 1,
            |spec| spec.seed_call_targets.swap(0, 1),
            |spec| spec.dispatch_va += 1,
            |spec| spec.jump_table_va += 1,
            |spec| spec.first_case_va += 1,
            |spec| spec.common_join_va += 1,
            |spec| spec.state_pointer_va += 1,
            |spec| spec.control_pointer_va += 1,
            |spec| spec.state_offsets.swap(6, 7),
        ];
        for mutate in layout_mutations {
            candidate = base.clone();
            mutate(&mut candidate.initializer);
            assert_eq!(
                candidate.validate(),
                Err(CompiledInitializerProfileError::UnsupportedLayout)
            );
        }
        candidate = base.clone();
        candidate.initializer.control_offsets.swap(0, 1);
        assert_eq!(
            candidate.validate(),
            Err(CompiledInitializerProfileError::UnsupportedLayout)
        );

        candidate = base.clone();
        candidate.initializer.selectors = 13;
        assert_eq!(
            candidate.validate(),
            Err(CompiledInitializerProfileError::UnsupportedSelectorCount(
                13
            ))
        );

        let mut document = serde_json::to_value(&base).unwrap();
        document["initializer"]["output_encoding"] = "not-cmr-ccase".into();
        assert!(matches!(
            ReviewedCompiledInitializerProfile::from_json(&serde_json::to_vec(&document).unwrap()),
            Err(ReviewedCompiledInitializerProfileError::Json(_))
        ));

        let mut document = serde_json::to_value(&base).unwrap();
        document["initializer"]["cases"] = serde_json::json!([[1, 2, 3]]);
        assert!(matches!(
            ReviewedCompiledInitializerProfile::from_json(&serde_json::to_vec(&document).unwrap()),
            Err(ReviewedCompiledInitializerProfileError::Json(_))
        ));
    }

    #[test]
    fn official_profile_is_exact_metadata_only_reviewed_document() {
        let document =
            include_bytes!("../../../profiles/official-eu-compiled-car-initializer-v1.json");
        let reviewed = ReviewedCompiledInitializerProfile::from_json(document).unwrap();
        assert_eq!(
            reviewed.sha256(),
            "f02dec474458174e5c1b9738667d7d9523ab6794cf79bbb8ebdf693604a933a9"
        );
        assert_eq!(
            reviewed.profile().initializer.layout(),
            OFFICIAL_EU_COMPILED_INITIALIZER_LAYOUT
        );
        assert_eq!(
            reviewed.profile().initializer.selectors,
            COMPILED_CASE_COUNT as u32
        );
    }
}
