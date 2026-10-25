use std::fmt;

use cmr_formats::pe_aux_control_initializer::{
    AUX_CONTROL_WORD_OFFSETS, AuxControlInitializerLayout,
    OFFICIAL_EU_AUX_CONTROL_INITIALIZER_LAYOUT,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::normalize_relative_path;

pub const AUX_CONTROL_PROFILE_SCHEMA_VERSION: u32 = 1;
const MAX_EXECUTABLE_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuxControlProfile {
    pub schema_version: u32,
    pub id: String,
    pub executable: AuxControlExecutable,
    pub initializer: AuxControlSpec,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuxControlExecutable {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuxControlSpec {
    pub image_base: u32,
    pub function_start_va: u32,
    pub function_end_va: u32,
    pub lookup_va: u32,
    pub select_va: u32,
    pub default_policy_va: u32,
    pub policy_two_va: u32,
    pub policy_one_va: u32,
    pub common_va: u32,
    pub first_divide_va: u32,
    pub first_divide_join_va: u32,
    pub second_divide_va: u32,
    pub second_divide_join_va: u32,
    pub third_divide_va: u32,
    pub third_divide_join_va: u32,
    pub config_pointer_va: u32,
    pub state_pointer_va: u32,
    pub control_pointer_va: u32,
    pub global_word_va: u32,
    pub config_flag_offset: u16,
    pub state_index_offset: u16,
    pub config_option_offset: u16,
    pub config_record_stride: u16,
    pub state_word_offset: u16,
    pub output_offsets: [u16; AUX_CONTROL_WORD_OFFSETS.len()],
    pub output_encoding: AuxControlEncoding,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuxControlEncoding {
    CmrAuxcV1,
}

impl AuxControlSpec {
    #[must_use]
    pub fn layout(&self) -> AuxControlInitializerLayout {
        AuxControlInitializerLayout {
            image_base: self.image_base,
            function_start_va: self.function_start_va,
            function_end_va: self.function_end_va,
            lookup_va: self.lookup_va,
            select_va: self.select_va,
            default_policy_va: self.default_policy_va,
            policy_two_va: self.policy_two_va,
            policy_one_va: self.policy_one_va,
            common_va: self.common_va,
            first_divide_va: self.first_divide_va,
            first_divide_join_va: self.first_divide_join_va,
            second_divide_va: self.second_divide_va,
            second_divide_join_va: self.second_divide_join_va,
            third_divide_va: self.third_divide_va,
            third_divide_join_va: self.third_divide_join_va,
            config_pointer_va: self.config_pointer_va,
            state_pointer_va: self.state_pointer_va,
            control_pointer_va: self.control_pointer_va,
            global_word_va: self.global_word_va,
            config_flag_offset: self.config_flag_offset,
            state_index_offset: self.state_index_offset,
            config_option_offset: self.config_option_offset,
            config_record_stride: self.config_record_stride,
            state_word_offset: self.state_word_offset,
            output_offsets: self.output_offsets,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ReviewedAuxControlProfile {
    profile: AuxControlProfile,
    sha256: String,
}

#[derive(Debug)]
pub enum ReviewedAuxControlProfileError {
    Json(serde_json::Error),
    Profile(AuxControlProfileError),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AuxControlProfileError {
    UnsupportedSchema(u32),
    EmptyId,
    InvalidExecutablePath(String),
    InvalidExecutableSize(u64),
    InvalidExecutableSha256,
    UnsupportedLayout,
}

impl fmt::Display for ReviewedAuxControlProfileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(formatter, "invalid aux-control profile JSON: {error}"),
            Self::Profile(error) => write!(formatter, "invalid aux-control profile: {error}"),
        }
    }
}

impl std::error::Error for ReviewedAuxControlProfileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            Self::Profile(error) => Some(error),
        }
    }
}

impl fmt::Display for AuxControlProfileError {
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
                "unsupported aux-control initializer address or offset layout"
            ),
        }
    }
}

impl std::error::Error for AuxControlProfileError {}

impl ReviewedAuxControlProfile {
    pub fn from_json(document: &[u8]) -> Result<Self, ReviewedAuxControlProfileError> {
        let profile: AuxControlProfile =
            serde_json::from_slice(document).map_err(ReviewedAuxControlProfileError::Json)?;
        profile
            .validate()
            .map_err(ReviewedAuxControlProfileError::Profile)?;
        Ok(Self {
            profile,
            sha256: encode_digest(Sha256::digest(document)),
        })
    }

    #[must_use]
    pub fn profile(&self) -> &AuxControlProfile {
        &self.profile
    }

    #[must_use]
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

impl AuxControlProfile {
    pub fn validate(&self) -> Result<(), AuxControlProfileError> {
        if self.schema_version != AUX_CONTROL_PROFILE_SCHEMA_VERSION {
            return Err(AuxControlProfileError::UnsupportedSchema(
                self.schema_version,
            ));
        }
        if self.id.trim().is_empty() {
            return Err(AuxControlProfileError::EmptyId);
        }
        if normalize_relative_path(&self.executable.path).as_deref()
            != Some(self.executable.path.as_str())
        {
            return Err(AuxControlProfileError::InvalidExecutablePath(
                self.executable.path.clone(),
            ));
        }
        if self.executable.size == 0 || self.executable.size > MAX_EXECUTABLE_BYTES {
            return Err(AuxControlProfileError::InvalidExecutableSize(
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
            return Err(AuxControlProfileError::InvalidExecutableSha256);
        }
        if self.initializer.layout() != OFFICIAL_EU_AUX_CONTROL_INITIALIZER_LAYOUT {
            return Err(AuxControlProfileError::UnsupportedLayout);
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

    pub(crate) fn profile(size: u64, sha256: String) -> AuxControlProfile {
        let layout = OFFICIAL_EU_AUX_CONTROL_INITIALIZER_LAYOUT;
        AuxControlProfile {
            schema_version: AUX_CONTROL_PROFILE_SCHEMA_VERSION,
            id: "synthetic-aux-control".into(),
            executable: AuxControlExecutable {
                path: "Rally.exe".into(),
                size,
                sha256,
            },
            initializer: AuxControlSpec {
                image_base: layout.image_base,
                function_start_va: layout.function_start_va,
                function_end_va: layout.function_end_va,
                lookup_va: layout.lookup_va,
                select_va: layout.select_va,
                default_policy_va: layout.default_policy_va,
                policy_two_va: layout.policy_two_va,
                policy_one_va: layout.policy_one_va,
                common_va: layout.common_va,
                first_divide_va: layout.first_divide_va,
                first_divide_join_va: layout.first_divide_join_va,
                second_divide_va: layout.second_divide_va,
                second_divide_join_va: layout.second_divide_join_va,
                third_divide_va: layout.third_divide_va,
                third_divide_join_va: layout.third_divide_join_va,
                config_pointer_va: layout.config_pointer_va,
                state_pointer_va: layout.state_pointer_va,
                control_pointer_va: layout.control_pointer_va,
                global_word_va: layout.global_word_va,
                config_flag_offset: layout.config_flag_offset,
                state_index_offset: layout.state_index_offset,
                config_option_offset: layout.config_option_offset,
                config_record_stride: layout.config_record_stride,
                state_word_offset: layout.state_word_offset,
                output_offsets: layout.output_offsets,
                output_encoding: AuxControlEncoding::CmrAuxcV1,
            },
        }
    }

    #[test]
    fn exact_digest_paths_hashes_and_canonical_layout_are_checked() {
        let base = profile(1234, "00".repeat(32));
        let compact = serde_json::to_vec(&base).unwrap();
        let pretty = serde_json::to_vec_pretty(&base).unwrap();
        assert_ne!(
            ReviewedAuxControlProfile::from_json(&compact)
                .unwrap()
                .sha256(),
            ReviewedAuxControlProfile::from_json(&pretty)
                .unwrap()
                .sha256()
        );

        for path in [
            "../Rally.exe",
            "/Rally.exe",
            "C:/Rally.exe",
            "bin\\Rally.exe",
            "./Rally.exe",
        ] {
            let mut candidate = base.clone();
            candidate.executable.path = path.into();
            assert!(candidate.validate().is_err());
        }
        for size in [0, MAX_EXECUTABLE_BYTES + 1] {
            let mut candidate = base.clone();
            candidate.executable.size = size;
            assert!(candidate.validate().is_err());
        }
        for hash in [
            "AA".repeat(32),
            "0".repeat(63),
            format!("{}g", "0".repeat(63)),
        ] {
            let mut candidate = base.clone();
            candidate.executable.sha256 = hash;
            assert_eq!(
                candidate.validate(),
                Err(AuxControlProfileError::InvalidExecutableSha256)
            );
        }
        let mut candidate = base.clone();
        candidate.initializer.function_start_va += 1;
        assert_eq!(
            candidate.validate(),
            Err(AuxControlProfileError::UnsupportedLayout)
        );
        candidate = base.clone();
        candidate.initializer.output_offsets.swap(0, 1);
        assert_eq!(
            candidate.validate(),
            Err(AuxControlProfileError::UnsupportedLayout)
        );
    }

    #[test]
    fn unknown_fields_and_payload_fields_are_rejected_at_every_level() {
        let base = profile(1234, "00".repeat(32));
        for pointer in ["", "/executable", "/initializer"] {
            let mut document = serde_json::to_value(&base).unwrap();
            document.pointer_mut(pointer).unwrap()["unexpected"] = serde_json::json!(1);
            assert!(matches!(
                ReviewedAuxControlProfile::from_json(&serde_json::to_vec(&document).unwrap()),
                Err(ReviewedAuxControlProfileError::Json(_))
            ));
        }
        for field in ["values", "constants", "immediates", "bytes"] {
            let mut document = serde_json::to_value(&base).unwrap();
            document["initializer"][field] = serde_json::json!([1, 2, 3]);
            assert!(
                ReviewedAuxControlProfile::from_json(&serde_json::to_vec(&document).unwrap())
                    .is_err()
            );
        }
    }

    #[test]
    fn official_profile_is_exact_reviewed_metadata() {
        let document =
            include_bytes!("../../../profiles/official-eu-aux-control-constants-v1.json");
        let reviewed = ReviewedAuxControlProfile::from_json(document).unwrap();
        assert_eq!(
            reviewed.profile().initializer.layout(),
            OFFICIAL_EU_AUX_CONTROL_INITIALIZER_LAYOUT
        );
        assert_eq!(reviewed.profile().executable.path, "Rally.exe");
        assert_eq!(
            reviewed.sha256(),
            "f56b25238ad4bf2caaf75e1984aeffabe6507e56e50c3e2e8fd2970051f1a057"
        );
    }
}
