//! Compatibility model for the legacy `save\\cmRally.cfg` record.
//!
//! The shipped loader treats the file as an opaque, exact-size memory image.
//! Consequently this module exposes typed accessors only for recovered fields
//! and preserves every other byte without assigning it a meaning.

use std::{error::Error, fmt};

use crate::localization::Language;

pub const LEGACY_CONFIG_SIZE: usize = 0x1e20;
/// Subkey passed with the separately selected `HKEY_LOCAL_MACHINE` hive.
pub const LEGACY_REGISTRY_SUBKEY: &str = r"SOFTWARE\Codemasters\Rally\1.00";

const SAVE_FLAGS_OFFSET: usize = 0x08;
const HARDWARE_INDEX_OFFSET: usize = 0x12;
const LANGUAGE_FLAGS_OFFSET: usize = 0x20;
const LANGUAGE_MASK: u8 = 0x07;
const VOLATILE_START: usize = 0x184;
const VOLATILE_END: usize = 0x5d4;

const MAGIC_OFFSET: usize = 0;
const TOKEN_OFFSET: usize = 4;
const DECLARED_SIZE_OFFSET: usize = 0x1e18;
const TOKEN_COMPLEMENT_OFFSET: usize = 0x1e1c;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LegacyConfig {
    bytes: [u8; LEGACY_CONFIG_SIZE],
}

impl LegacyConfig {
    /// Copies an exact legacy record without validating its internal fields.
    ///
    /// This intentionally accepts arbitrary bytes when the length is correct:
    /// the recovered shipped loader performs the same size-only admission.
    pub fn from_exact_bytes(bytes: &[u8]) -> Result<Self, ConfigSizeError> {
        if bytes.len() != LEGACY_CONFIG_SIZE {
            return Err(ConfigSizeError {
                actual: bytes.len(),
            });
        }

        let mut record = [0; LEGACY_CONFIG_SIZE];
        record.copy_from_slice(bytes);
        Ok(Self { bytes: record })
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8; LEGACY_CONFIG_SIZE] {
        &self.bytes
    }

    #[must_use]
    pub fn into_bytes(self) -> [u8; LEGACY_CONFIG_SIZE] {
        self.bytes
    }

    #[must_use]
    pub fn header_magic(&self) -> u32 {
        self.read_u32(MAGIC_OFFSET)
    }

    #[must_use]
    pub fn token(&self) -> u32 {
        self.read_u32(TOKEN_OFFSET)
    }

    #[must_use]
    pub fn declared_size(&self) -> u32 {
        self.read_u32(DECLARED_SIZE_OFFSET)
    }

    #[must_use]
    pub fn token_complement(&self) -> u32 {
        self.read_u32(TOKEN_COMPLEMENT_OFFSET)
    }

    /// Returns whether the recovered save wrapper would write this record.
    #[must_use]
    pub fn should_save(&self) -> bool {
        self.bytes[SAVE_FLAGS_OFFSET] & 1 == 0
    }

    #[must_use]
    pub fn hardware_index(&self) -> u8 {
        self.bytes[HARDWARE_INDEX_OFFSET]
    }

    pub fn set_hardware_index(&mut self, hardware_index: u8) {
        self.bytes[HARDWARE_INDEX_OFFSET] = hardware_index;
    }

    #[must_use]
    pub fn language(&self) -> Option<Language> {
        Language::from_index(usize::from(
            self.bytes[LANGUAGE_FLAGS_OFFSET] & LANGUAGE_MASK,
        ))
    }

    pub fn set_language(&mut self, language: Language) {
        let flags = &mut self.bytes[LANGUAGE_FLAGS_OFFSET];
        *flags = (*flags & !LANGUAGE_MASK) | language.index() as u8;
    }

    fn clear_volatile_runtime_region(&mut self) {
        self.bytes[VOLATILE_START..VOLATILE_END].fill(0);
    }

    fn read_u32(&self, offset: usize) -> u32 {
        u32::from_le_bytes(
            self.bytes[offset..offset + 4]
                .try_into()
                .expect("fixed field"),
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConfigSizeError {
    pub actual: usize,
}

impl fmt::Display for ConfigSizeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "legacy configuration must be exactly {LEGACY_CONFIG_SIZE} bytes, got {}",
            self.actual
        )
    }
}

impl Error for ConfigSizeError {}

/// Parses the ANSI `Language` registry value used by the shipped game.
///
/// Exact uppercase values are compatible with the observed comparisons, and
/// an unrecognized present value resolves to English as observed. The shipped
/// missing-value path reads an uninitialized buffer, so resolving a missing
/// value to English is an explicit safe reconstruction policy derived from
/// the observed unknown-value fallback, not an observed original behavior.
#[must_use]
pub fn parse_registry_language(value: Option<&[u8]>) -> Language {
    Language::from_registry_value(value)
}

/// Parses the registry's decimal `Hardware` value and returns the byte stored
/// in the legacy record.
///
/// The parser follows the recovered `atoi`-style behavior: ASCII whitespace
/// and one sign are accepted, parsing stops at the first non-digit, and the
/// low byte is retained. The shipped query-failure path consumed an
/// uninitialized buffer, so empty or missing data resolving to zero is an
/// explicit safe reconstruction policy rather than observed behavior.
#[must_use]
pub fn parse_registry_hardware(value: Option<&[u8]>) -> u8 {
    let bytes = c_string_bytes(value.unwrap_or_default());
    let mut cursor = 0;
    while bytes
        .get(cursor)
        .is_some_and(|byte| byte.is_ascii_whitespace())
    {
        cursor += 1;
    }

    let negative = match bytes.get(cursor) {
        Some(b'-') => {
            cursor += 1;
            true
        }
        Some(b'+') => {
            cursor += 1;
            false
        }
        _ => false,
    };

    let mut value = 0_u32;
    while let Some(byte @ b'0'..=b'9') = bytes.get(cursor) {
        value = value.wrapping_mul(10).wrapping_add(u32::from(*byte - b'0'));
        cursor += 1;
    }

    if negative {
        value = value.wrapping_neg();
    }
    value as u8
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigLoadDisposition {
    Loaded,
    Missing,
    WrongSize { actual: usize },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigLoadResult {
    pub config: LegacyConfig,
    pub disposition: ConfigLoadDisposition,
}

/// Models the record-selection and byte-mutation portion of startup loading.
///
/// An exact-sized stored record replaces the defaults and has its runtime-only
/// region cleared. A missing or wrongly sized record leaves the defaults in
/// place and applies the registry-selected hardware byte. Input-binding
/// rebuilding and display-field application occur after this step and are not
/// modeled here.
#[must_use]
pub fn load_config_or_defaults(
    mut defaults: LegacyConfig,
    stored: Option<&[u8]>,
    registry_hardware: Option<&[u8]>,
) -> ConfigLoadResult {
    match stored {
        Some(bytes) if bytes.len() == LEGACY_CONFIG_SIZE => {
            let mut config = LegacyConfig::from_exact_bytes(bytes)
                .expect("length was checked against the exact legacy size");
            config.clear_volatile_runtime_region();
            ConfigLoadResult {
                config,
                disposition: ConfigLoadDisposition::Loaded,
            }
        }
        Some(bytes) => {
            defaults.set_hardware_index(parse_registry_hardware(registry_hardware));
            ConfigLoadResult {
                config: defaults,
                disposition: ConfigLoadDisposition::WrongSize {
                    actual: bytes.len(),
                },
            }
        }
        None => {
            defaults.set_hardware_index(parse_registry_hardware(registry_hardware));
            ConfigLoadResult {
                config: defaults,
                disposition: ConfigLoadDisposition::Missing,
            }
        }
    }
}

fn c_string_bytes(bytes: &[u8]) -> &[u8] {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    &bytes[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_with(fill: u8) -> LegacyConfig {
        LegacyConfig::from_exact_bytes(&[fill; LEGACY_CONFIG_SIZE]).unwrap()
    }

    #[test]
    fn exact_size_parser_preserves_every_byte() {
        let bytes = std::array::from_fn::<_, LEGACY_CONFIG_SIZE, _>(|index| index as u8);
        let config = LegacyConfig::from_exact_bytes(&bytes).unwrap();
        assert_eq!(config.as_bytes(), &bytes);
        assert_eq!(config.into_bytes(), bytes);
    }

    #[test]
    fn parser_rejects_both_wrong_size_boundaries() {
        for actual in [0, LEGACY_CONFIG_SIZE - 1, LEGACY_CONFIG_SIZE + 1] {
            assert_eq!(
                LegacyConfig::from_exact_bytes(&vec![0; actual]).unwrap_err(),
                ConfigSizeError { actual }
            );
        }
    }

    #[test]
    fn parser_deliberately_accepts_invalid_header_fields() {
        let config = config_with(0);
        assert_eq!(config.header_magic(), 0);
        assert_eq!(config.declared_size(), 0);
        assert_eq!(config.token(), 0);
        assert_eq!(config.token_complement(), 0);
    }

    #[test]
    fn header_accessors_are_little_endian_and_independent() {
        let mut bytes = [0; LEGACY_CONFIG_SIZE];
        bytes[MAGIC_OFFSET..MAGIC_OFFSET + 4].copy_from_slice(&0x8765_1234_u32.to_le_bytes());
        bytes[TOKEN_OFFSET..TOKEN_OFFSET + 4].copy_from_slice(&0x1122_3344_u32.to_le_bytes());
        bytes[DECLARED_SIZE_OFFSET..DECLARED_SIZE_OFFSET + 4]
            .copy_from_slice(&(LEGACY_CONFIG_SIZE as u32).to_le_bytes());
        bytes[TOKEN_COMPLEMENT_OFFSET..TOKEN_COMPLEMENT_OFFSET + 4]
            .copy_from_slice(&(!0x1122_3344_u32).to_le_bytes());
        let config = LegacyConfig::from_exact_bytes(&bytes).unwrap();

        assert_eq!(config.header_magic(), 0x8765_1234);
        assert_eq!(config.token(), 0x1122_3344);
        assert_eq!(config.declared_size(), LEGACY_CONFIG_SIZE as u32);
        assert_eq!(config.token_complement(), !0x1122_3344);
    }

    #[test]
    fn save_gate_depends_only_on_byte_eight_bit_zero() {
        let mut bytes = [0; LEGACY_CONFIG_SIZE];
        assert!(
            LegacyConfig::from_exact_bytes(&bytes)
                .unwrap()
                .should_save()
        );

        bytes[SAVE_FLAGS_OFFSET] = 1;
        assert!(
            !LegacyConfig::from_exact_bytes(&bytes)
                .unwrap()
                .should_save()
        );

        bytes[SAVE_FLAGS_OFFSET] = 0xfe;
        assert!(
            LegacyConfig::from_exact_bytes(&bytes)
                .unwrap()
                .should_save()
        );

        bytes[SAVE_FLAGS_OFFSET] = 0xff;
        assert!(
            !LegacyConfig::from_exact_bytes(&bytes)
                .unwrap()
                .should_save()
        );
    }

    #[test]
    fn typed_field_mutators_preserve_unrelated_bits_and_bytes() {
        let mut config = config_with(0xa8);
        config.set_hardware_index(7);
        config.set_language(Language::Spanish);

        assert_eq!(config.hardware_index(), 7);
        assert_eq!(config.language(), Some(Language::Spanish));
        assert_eq!(config.as_bytes()[LANGUAGE_FLAGS_OFFSET], 0xab);
        assert_eq!(config.as_bytes()[HARDWARE_INDEX_OFFSET - 1], 0xa8);
        assert_eq!(config.as_bytes()[HARDWARE_INDEX_OFFSET + 1], 0xa8);
    }

    #[test]
    fn unknown_stored_language_code_is_not_invented() {
        let mut bytes = [0; LEGACY_CONFIG_SIZE];
        bytes[LANGUAGE_FLAGS_OFFSET] = 0xff;
        assert_eq!(
            LegacyConfig::from_exact_bytes(&bytes).unwrap().language(),
            None
        );
    }

    #[test]
    fn exact_size_load_replaces_defaults_and_clears_only_volatile_range() {
        let defaults = config_with(0x11);
        let stored = [0xa5; LEGACY_CONFIG_SIZE];
        let result = load_config_or_defaults(defaults, Some(&stored), Some(b"93"));
        let mut expected = stored;
        expected[VOLATILE_START..VOLATILE_END].fill(0);

        assert_eq!(result.disposition, ConfigLoadDisposition::Loaded);
        assert_eq!(result.config.as_bytes(), &expected);
        assert_eq!(result.config.hardware_index(), 0xa5);
    }

    #[test]
    fn missing_load_retains_defaults_and_applies_hardware() {
        let defaults = config_with(0x44);
        let mut expected = *defaults.as_bytes();
        expected[HARDWARE_INDEX_OFFSET] = 2;
        let result = load_config_or_defaults(defaults, None, Some(b"  +258 ignored"));

        assert_eq!(result.disposition, ConfigLoadDisposition::Missing);
        assert_eq!(result.config.as_bytes(), &expected);
    }

    #[test]
    fn wrong_size_load_retains_defaults_and_reports_actual_size() {
        let defaults = config_with(0x66);
        for actual in [LEGACY_CONFIG_SIZE - 1, LEGACY_CONFIG_SIZE + 1] {
            let mut expected = *defaults.as_bytes();
            expected[HARDWARE_INDEX_OFFSET] = 9;
            let result =
                load_config_or_defaults(defaults.clone(), Some(&vec![0; actual]), Some(b"9"));
            assert_eq!(
                result.disposition,
                ConfigLoadDisposition::WrongSize { actual }
            );
            assert_eq!(result.config.as_bytes(), &expected);
        }
    }

    #[test]
    fn registry_language_accepts_only_observed_uppercase_names() {
        let cases = [
            (b"ENGLISH".as_slice(), Language::English),
            (b"FRENCH".as_slice(), Language::French),
            (b"GERMAN".as_slice(), Language::German),
            (b"SPANISH".as_slice(), Language::Spanish),
            (b"ITALIAN".as_slice(), Language::Italian),
        ];
        for (value, expected) in cases {
            assert_eq!(parse_registry_language(Some(value)), expected);
        }

        assert_eq!(
            parse_registry_language(Some(b"FRENCH\0ignored")),
            Language::French
        );
        assert_eq!(parse_registry_language(Some(b"french")), Language::English);
        assert_eq!(parse_registry_language(Some(b" FRENCH")), Language::English);
        assert_eq!(parse_registry_language(Some(b"UNKNOWN")), Language::English);
        assert_eq!(parse_registry_language(None), Language::English);
    }

    #[test]
    fn registry_hardware_matches_low_byte_atoi_behavior() {
        let cases: &[(Option<&[u8]>, u8)] = &[
            (None, 0),
            (Some(b""), 0),
            (Some(b"no digits"), 0),
            (Some(b"7"), 7),
            (Some(b"\t\r\n +258 trailing"), 2),
            (Some(b"-1"), 255),
            (Some(b"513\0ignored"), 1),
            (Some(b"4294967296"), 0),
        ];
        for (value, expected) in cases {
            assert_eq!(parse_registry_hardware(*value), *expected);
        }
    }

    #[test]
    fn registry_subkey_is_separate_from_the_machine_wide_hive() {
        assert_eq!(LEGACY_REGISTRY_SUBKEY, r"SOFTWARE\Codemasters\Rally\1.00");
    }
}
