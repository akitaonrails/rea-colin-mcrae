//! Payload-free contract for the compiled per-selector fields in
//! official-EU `FUN_00404440`.
//!
//! The shipped routine selects one of fourteen immediate-store bodies. Each
//! body writes the same eight state offsets and twenty-three control offsets.
//! The values themselves are game data and are deliberately not present here:
//! callers provide them, and tests use synthetic records only.
//!
//! After intervening initializer calls, the routine derives reciprocal and
//! copy families from eight control words. That later value transformation is
//! exposed separately so this module does not imply that the omitted callees
//! have been reconstructed. Offset names are neutral; physical meanings and
//! selector-to-car names remain unknown.

use std::fmt;

use cmr_assets::CompiledInitializerStore;
use cmr_formats::pe_compiled_initializer::{
    COMPILED_CASE_COUNT as FORMAT_CASE_COUNT,
    COMPILED_CONTROL_WORD_OFFSETS as FORMAT_CONTROL_WORD_OFFSETS,
    COMPILED_STATE_WORD_OFFSETS as FORMAT_STATE_WORD_OFFSETS,
    CompiledCaseWords as FormatCompiledCaseWords,
};

/// Number of compiled selector bodies accepted by the unsigned dispatch.
pub const COMPILED_CASE_COUNT: usize = 14;

/// Number of direct state-word stores in each compiled body.
pub const COMPILED_STATE_WORD_COUNT: usize = 8;

/// Number of direct control-word stores in each compiled body.
pub const COMPILED_CONTROL_WORD_COUNT: usize = 23;

/// Number of words in the later primary/reciprocal control families.
pub const COMPILED_CONTROL_FAMILY_WORD_COUNT: usize = 8;

/// Number of middle control-family words rewritten by the recovered transform.
pub const COMPILED_CONTROL_RUN_WORD_COUNT: usize = 6;

/// State destinations in their observed case-body store order.
pub const COMPILED_STATE_WORD_OFFSETS: [u16; COMPILED_STATE_WORD_COUNT] =
    [0x020, 0x024, 0x028, 0x638, 0x63c, 0x640, 0x648, 0x644];

/// Control destinations in the neutral schema order used by
/// [`CompiledCaseWords::control_words`].
///
/// The eight-word family later copied and reciprocated occupies indices
/// `14..22` (`+0x1d0..+0x1ec`).
pub const COMPILED_CONTROL_WORD_OFFSETS: [u16; COMPILED_CONTROL_WORD_COUNT] = [
    0x148, 0x158, 0x16c, 0x15c, 0x194, 0x198, 0x19c, 0x1a0, 0x1a4, 0x1b8, 0x1bc, 0x1c0, 0x1c4,
    0x1c8, 0x1d0, 0x1d4, 0x1d8, 0x1dc, 0x1e0, 0x1e4, 0x1e8, 0x1ec, 0x140,
];

const CONTROL_FAMILY_START: usize = 14;

/// Caller-supplied words for one compiled selector body.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CompiledCaseWords {
    pub state_words: [i32; COMPILED_STATE_WORD_COUNT],
    pub control_words: [i32; COMPILED_CONTROL_WORD_COUNT],
}

/// An owned set of the fourteen direct case-body writes.
///
/// Construction proves that the format/store boundary and core contract agree
/// on selector count and neutral offset order. The set contains no result from
/// the omitted common-path helpers and must not be treated as post-processed
/// input to [`derive_compiled_control_families`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompiledDirectCaseSet {
    cases: [CompiledCaseWords; COMPILED_CASE_COUNT],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompiledDirectCaseSetError {
    SelectorCount {
        core: usize,
        format: usize,
    },
    OffsetOrder {
        object: &'static str,
        index: usize,
        core_offset: u16,
        format_offset: u16,
    },
}

impl fmt::Display for CompiledDirectCaseSetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "compiled direct-case store schema does not match the core contract: {self:?}"
        )
    }
}

impl std::error::Error for CompiledDirectCaseSetError {}

impl CompiledDirectCaseSet {
    /// Copy the verified format-owned direct cases into a core-owned set.
    pub fn from_store(
        store: &CompiledInitializerStore,
    ) -> Result<Self, CompiledDirectCaseSetError> {
        Self::from_format_cases(store.cases())
    }

    fn from_format_cases(
        cases: &[FormatCompiledCaseWords; FORMAT_CASE_COUNT],
    ) -> Result<Self, CompiledDirectCaseSetError> {
        verify_format_schema()?;
        Ok(Self {
            cases: std::array::from_fn(|selector| CompiledCaseWords {
                state_words: cases[selector].state_words,
                control_words: cases[selector].control_words,
            }),
        })
    }

    #[must_use]
    pub const fn cases(&self) -> &[CompiledCaseWords; COMPILED_CASE_COUNT] {
        &self.cases
    }

    /// Apply only the observed unsigned direct-case dispatch.
    #[must_use]
    pub fn apply(&self, prior: CompiledCaseWords, selector: u32) -> CompiledCaseOutcome {
        apply_compiled_case(prior, &self.cases, selector)
    }
}

impl TryFrom<&CompiledInitializerStore> for CompiledDirectCaseSet {
    type Error = CompiledDirectCaseSetError;

    fn try_from(store: &CompiledInitializerStore) -> Result<Self, Self::Error> {
        Self::from_store(store)
    }
}

fn verify_format_schema() -> Result<(), CompiledDirectCaseSetError> {
    if FORMAT_CASE_COUNT != COMPILED_CASE_COUNT {
        return Err(CompiledDirectCaseSetError::SelectorCount {
            core: COMPILED_CASE_COUNT,
            format: FORMAT_CASE_COUNT,
        });
    }
    for (object, core_offsets, format_offsets) in [
        (
            "state",
            COMPILED_STATE_WORD_OFFSETS.as_slice(),
            FORMAT_STATE_WORD_OFFSETS.as_slice(),
        ),
        (
            "control",
            COMPILED_CONTROL_WORD_OFFSETS.as_slice(),
            FORMAT_CONTROL_WORD_OFFSETS.as_slice(),
        ),
    ] {
        for (index, (&core_offset, &format_offset)) in
            core_offsets.iter().zip(format_offsets).enumerate()
        {
            if core_offset != format_offset {
                return Err(CompiledDirectCaseSetError::OffsetOrder {
                    object,
                    index,
                    core_offset,
                    format_offset,
                });
            }
        }
    }
    Ok(())
}

impl CompiledCaseWords {
    /// The eight direct case-body words at `+0x1d0..+0x1ec`, before common-path
    /// helpers such as `FUN_00404160` may transform them.
    #[must_use]
    pub fn control_family(self) -> [i32; COMPILED_CONTROL_FAMILY_WORD_COUNT] {
        std::array::from_fn(|index| self.control_words[CONTROL_FAMILY_START + index])
    }
}

/// Result of the unsigned selector dispatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompiledCaseOutcome {
    /// Selected words, or the prior destination words when the selector was
    /// outside the observed unsigned range.
    pub words: CompiledCaseWords,
    /// The selected body. `None` means the case stores were skipped.
    pub selected_case: Option<u8>,
}

/// Apply the observed unsigned fourteen-way case dispatch.
///
/// Values `0..=13` replace all thirty-one case destinations. Every other
/// `u32` preserves the prior destination words; there is no clamp or fallback
/// case. This function covers only the direct case stores, not the common tail.
#[must_use]
pub fn apply_compiled_case(
    prior: CompiledCaseWords,
    cases: &[CompiledCaseWords; COMPILED_CASE_COUNT],
    selector: u32,
) -> CompiledCaseOutcome {
    let selected_case = (selector < COMPILED_CASE_COUNT as u32).then_some(selector as u8);
    let words = selected_case
        .map(|index| cases[index as usize])
        .unwrap_or(prior);
    CompiledCaseOutcome {
        words,
        selected_case,
    }
}

/// Select the Q16 center-drive-split word from the low three option bits.
///
/// This is the total value policy recovered from `FUN_00404100`. Resolving the
/// option byte through the shipped signed config-record index is deliberately
/// outside this pure boundary.
#[must_use]
pub const fn compiled_center_split_q16(option_byte: u8) -> i32 {
    match option_byte & 7 {
        0 => 0x4000,
        1 => 0x8000,
        _ => 0xc000,
    }
}

/// Transform the six-word middle run selected by the low three option bits.
///
/// All five differences are taken from `original` before rebuilding the
/// output. Products use signed Q16 arithmetic and retain the low 32 bits after
/// the arithmetic right shift. Subtraction, the first-word bias, and the
/// cumulative rebuild use 32-bit wrapping arithmetic.
#[must_use]
pub fn transform_compiled_control_run(
    original: [i32; COMPILED_CONTROL_RUN_WORD_COUNT],
    option_byte: u8,
) -> [i32; COMPILED_CONTROL_RUN_WORD_COUNT] {
    let (scale, bias) = match option_byte & 7 {
        0 => (0x1_1999_i32, -0x3333_i32),
        1 => (0x1_0ccc, -0x1999),
        2 => (0x1_0000, 0),
        3 => (0x0_f334, 0x1999),
        _ => (0x0_e667, 0x3333),
    };
    let deltas: [i32; COMPILED_CONTROL_RUN_WORD_COUNT - 1] =
        std::array::from_fn(|index| original[index + 1].wrapping_sub(original[index]));
    let scaled = deltas.map(|delta| ((i64::from(delta) * i64::from(scale)) >> 16) as i32);

    let mut output = [0; COMPILED_CONTROL_RUN_WORD_COUNT];
    output[0] = original[0].wrapping_add(bias);
    for index in 0..scaled.len() {
        output[index + 1] = output[index].wrapping_add(scaled[index]);
    }
    output
}

/// Apply [`transform_compiled_control_run`] to indices `1..=6` of the
/// eight-word family, preserving endpoints zero and seven exactly.
///
/// This adapter performs neither case selection nor reciprocal/copy-tail
/// derivation.
#[must_use]
pub fn transform_compiled_control_family(
    original: [i32; COMPILED_CONTROL_FAMILY_WORD_COUNT],
    option_byte: u8,
) -> [i32; COMPILED_CONTROL_FAMILY_WORD_COUNT] {
    let run = std::array::from_fn(|index| original[index + 1]);
    let transformed = transform_compiled_control_run(run, option_byte);
    let mut output = original;
    output[1..=COMPILED_CONTROL_RUN_WORD_COUNT].copy_from_slice(&transformed);
    output
}

/// The four eight-word families after the observed reciprocal/copy tail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompiledControlFamilies {
    pub post_processed_raw: [i32; COMPILED_CONTROL_FAMILY_WORD_COUNT],
    pub reciprocal: [i32; COMPILED_CONTROL_FAMILY_WORD_COUNT],
    pub raw_copy: [i32; COMPILED_CONTROL_FAMILY_WORD_COUNT],
    pub reciprocal_copy: [i32; COMPILED_CONTROL_FAMILY_WORD_COUNT],
}

/// A nonzero primary word whose signed `2^32 / word` quotient does not fit in
/// the 32-bit destination used by x86 `IDIV`.
///
/// The shipped instruction would raise a divide exception. The pure contract
/// returns an error before publishing any derived families; this atomic error
/// is a reconstruction safety decision, not observed recovery behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompiledReciprocalOverflow {
    pub slot: usize,
    pub raw: i32,
}

impl fmt::Display for CompiledReciprocalOverflow {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "compiled reciprocal at slot {} overflows i32 for raw value {}",
            self.slot, self.raw
        )
    }
}

impl std::error::Error for CompiledReciprocalOverflow {}

/// Derive the later reciprocal and copy families from the already
/// post-processed eight words present at control offsets `+0x1d0..+0x1ec`
/// after `FUN_00404160` and when the common tail reaches the reciprocal code.
///
/// Reciprocal slot zero is always zero. For slots `1..=7`, zero maps to `-1`;
/// otherwise the value is signed `2^32 / raw`, truncated toward zero.
/// Copies preserve every raw word exactly. Intervening initializer calls that
/// occur between case selection and this tail remain outside this contract;
/// callers must not pass [`CompiledCaseWords::control_family`] directly unless
/// they have separately applied that omitted processing.
pub fn derive_compiled_control_families(
    post_processed_raw: [i32; COMPILED_CONTROL_FAMILY_WORD_COUNT],
) -> Result<CompiledControlFamilies, CompiledReciprocalOverflow> {
    let mut reciprocal = [0; COMPILED_CONTROL_FAMILY_WORD_COUNT];
    for slot in 1..COMPILED_CONTROL_FAMILY_WORD_COUNT {
        let raw = post_processed_raw[slot];
        reciprocal[slot] = if raw == 0 {
            -1
        } else {
            let quotient = (1_i64 << 32) / i64::from(raw);
            i32::try_from(quotient).map_err(|_| CompiledReciprocalOverflow { slot, raw })?
        };
    }

    Ok(CompiledControlFamilies {
        post_processed_raw,
        reciprocal,
        raw_copy: post_processed_raw,
        reciprocal_copy: reciprocal,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cmr_assets::{
        COMPILED_INITIALIZER_DATA_FILE, COMPILED_INITIALIZER_IMPORT_SCHEMA_VERSION,
        COMPILED_INITIALIZER_STATE_FILE, CompiledInitializerEncoding,
        CompiledInitializerExecutable, CompiledInitializerImportFile,
        CompiledInitializerImportShape, CompiledInitializerImportSummary,
        CompiledInitializerProfile, CompiledInitializerSpec, ReviewedCompiledInitializerProfile,
    };
    use cmr_formats::pe_compiled_initializer::{
        COMPILED_CASE_ENCODED_LEN, COMPILED_CASE_FORMAT_VERSION,
        COMPILED_CONTROL_WORD_COUNT as FORMAT_CONTROL_WORD_COUNT,
        COMPILED_STATE_WORD_COUNT as FORMAT_STATE_WORD_COUNT,
        CompiledCaseWords as FormatCompiledCaseWords, OFFICIAL_EU_COMPILED_INITIALIZER_LAYOUT,
        encode_compiled_cases,
    };
    use sha2::{Digest, Sha256};
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    struct TempDirectory(PathBuf);

    impl TempDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "cmr-core-compiled-direct-cases-{}-{}",
                std::process::id(),
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDirectory {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn digest(bytes: &[u8]) -> String {
        use std::fmt::Write as _;
        let mut output = String::with_capacity(64);
        for byte in Sha256::digest(bytes) {
            write!(output, "{byte:02x}").unwrap();
        }
        output
    }

    fn format_cases() -> [FormatCompiledCaseWords; FORMAT_CASE_COUNT] {
        let mut cases = std::array::from_fn(|selector| FormatCompiledCaseWords {
            state_words: std::array::from_fn(|index| selector as i32 * 100 + index as i32 - 50),
            control_words: std::array::from_fn(|index| selector as i32 * -1_000 - index as i32),
        });
        cases[0].state_words[..4].copy_from_slice(&[i32::MIN, i32::MAX, 0, -1]);
        cases[13].control_words[..4].copy_from_slice(&[i32::MIN, i32::MAX, 0, -1]);
        cases
    }

    fn open_synthetic_store(
        cases: &[FormatCompiledCaseWords; FORMAT_CASE_COUNT],
    ) -> (TempDirectory, CompiledInitializerStore) {
        let temp = TempDirectory::new();
        let root = temp.0.join("store");
        fs::create_dir(&root).unwrap();
        let source = b"synthetic source identity";
        let layout = OFFICIAL_EU_COMPILED_INITIALIZER_LAYOUT;
        let profile = CompiledInitializerProfile {
            schema_version: 1,
            id: "synthetic-core-adapter".into(),
            executable: CompiledInitializerExecutable {
                path: "Rally.exe".into(),
                size: source.len() as u64,
                sha256: digest(source),
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
                selectors: FORMAT_CASE_COUNT as u32,
                state_offsets: layout.state_offsets,
                control_offsets: layout.control_offsets,
                output_encoding: CompiledInitializerEncoding::CmrCcaseV1,
            },
        };
        let reviewed =
            ReviewedCompiledInitializerProfile::from_json(&serde_json::to_vec(&profile).unwrap())
                .unwrap();
        let data = encode_compiled_cases(cases);
        fs::write(root.join(COMPILED_INITIALIZER_DATA_FILE), data).unwrap();
        let summary = CompiledInitializerImportSummary {
            schema_version: COMPILED_INITIALIZER_IMPORT_SCHEMA_VERSION,
            profile: reviewed.profile().id.clone(),
            profile_sha256: reviewed.sha256().into(),
            source: CompiledInitializerImportFile {
                path: reviewed.profile().executable.path.clone(),
                size: reviewed.profile().executable.size,
                sha256: reviewed.profile().executable.sha256.clone(),
            },
            output: CompiledInitializerImportFile {
                path: COMPILED_INITIALIZER_DATA_FILE.into(),
                size: COMPILED_CASE_ENCODED_LEN as u64,
                sha256: digest(&data),
            },
            shape: CompiledInitializerImportShape {
                format_version: COMPILED_CASE_FORMAT_VERSION,
                selectors: FORMAT_CASE_COUNT as u32,
                state_words_per_selector: FORMAT_STATE_WORD_COUNT as u32,
                control_words_per_selector: FORMAT_CONTROL_WORD_COUNT as u32,
                encoding: CompiledInitializerEncoding::CmrCcaseV1,
            },
        };
        fs::write(
            root.join(COMPILED_INITIALIZER_STATE_FILE),
            serde_json::to_vec(&summary).unwrap(),
        )
        .unwrap();
        let store = CompiledInitializerStore::open(&root, &reviewed).unwrap();
        (temp, store)
    }

    fn synthetic_case(tag: i32) -> CompiledCaseWords {
        CompiledCaseWords {
            state_words: std::array::from_fn(|index| tag * 100 + index as i32),
            control_words: std::array::from_fn(|index| tag * 1_000 + index as i32 + 100),
        }
    }

    fn synthetic_cases() -> [CompiledCaseWords; COMPILED_CASE_COUNT] {
        std::array::from_fn(|index| synthetic_case(index as i32 + 1))
    }

    #[test]
    fn format_adapter_pins_counts_offsets_and_selector_order() {
        let format = format_cases();
        let owned = CompiledDirectCaseSet::from_format_cases(&format).unwrap();
        assert_eq!(owned.cases().len(), COMPILED_CASE_COUNT);
        for (selector, format_case) in format.iter().enumerate() {
            assert_eq!(owned.cases()[selector].state_words, format_case.state_words);
            assert_eq!(
                owned.cases()[selector].control_words,
                format_case.control_words
            );
        }
    }

    #[test]
    fn verified_store_adapts_to_owned_direct_cases_and_unsigned_selection() {
        let format = format_cases();
        let (temp, owned) = {
            let (temp, store) = open_synthetic_store(&format);
            let owned = CompiledDirectCaseSet::try_from(&store).unwrap();
            (temp, owned)
        };
        drop(temp);

        for (selector, format_case) in format.iter().enumerate() {
            assert_eq!(owned.cases()[selector].state_words, format_case.state_words);
            assert_eq!(
                owned.cases()[selector].control_words,
                format_case.control_words
            );
        }
        assert_eq!(
            owned.cases()[0].state_words[..4],
            [i32::MIN, i32::MAX, 0, -1]
        );
        assert_eq!(
            owned.cases()[13].control_words[..4],
            [i32::MIN, i32::MAX, 0, -1]
        );

        let prior = synthetic_case(-77);

        for selector in [0_u32, 7, 13] {
            let outcome = owned.apply(prior, selector);
            assert_eq!(outcome.selected_case, Some(selector as u8));
            assert_eq!(
                outcome.words.state_words,
                format[selector as usize].state_words
            );
            assert_eq!(
                outcome.words.control_words,
                format[selector as usize].control_words
            );
        }
        for selector in [14, u32::MAX] {
            assert_eq!(
                owned.apply(prior, selector),
                CompiledCaseOutcome {
                    words: prior,
                    selected_case: None,
                }
            );
        }
    }

    #[test]
    fn schema_offsets_pin_every_destination_and_family_slice() {
        assert_eq!(
            COMPILED_STATE_WORD_OFFSETS,
            [0x020, 0x024, 0x028, 0x638, 0x63c, 0x640, 0x648, 0x644]
        );
        assert_eq!(
            COMPILED_CONTROL_WORD_OFFSETS,
            [
                0x148, 0x158, 0x16c, 0x15c, 0x194, 0x198, 0x19c, 0x1a0, 0x1a4, 0x1b8, 0x1bc, 0x1c0,
                0x1c4, 0x1c8, 0x1d0, 0x1d4, 0x1d8, 0x1dc, 0x1e0, 0x1e4, 0x1e8, 0x1ec, 0x140,
            ]
        );

        let words = synthetic_case(7);
        assert_eq!(words.control_family(), words.control_words[14..22]);
    }

    #[test]
    fn every_valid_selector_replaces_all_thirty_one_words() {
        let cases = synthetic_cases();
        let prior = synthetic_case(-1);
        for selector in 0..COMPILED_CASE_COUNT as u32 {
            let outcome = apply_compiled_case(prior, &cases, selector);
            assert_eq!(outcome.selected_case, Some(selector as u8));
            assert_eq!(outcome.words, cases[selector as usize]);
        }
    }

    #[test]
    fn first_and_last_selectors_keep_offset_order_intact() {
        let cases = synthetic_cases();
        for selector in [0, 13] {
            let outcome = apply_compiled_case(CompiledCaseWords::default(), &cases, selector);
            assert_eq!(
                outcome.words.state_words,
                cases[selector as usize].state_words
            );
            assert_eq!(
                outcome.words.control_words,
                cases[selector as usize].control_words
            );
        }
    }

    #[test]
    fn unsigned_invalid_selectors_preserve_every_case_destination() {
        let cases = synthetic_cases();
        let prior = synthetic_case(-9);
        for selector in [14, 15, u32::MAX, i32::MAX as u32, i32::MIN as u32] {
            assert_eq!(
                apply_compiled_case(prior, &cases, selector),
                CompiledCaseOutcome {
                    words: prior,
                    selected_case: None,
                }
            );
        }
    }

    #[test]
    fn center_split_pins_all_low_three_bit_policies() {
        assert_eq!(compiled_center_split_q16(0), 0x4000);
        assert_eq!(compiled_center_split_q16(1), 0x8000);
        for mode in 2..=7 {
            assert_eq!(compiled_center_split_q16(mode), 0xc000);
        }
    }

    #[test]
    fn center_split_exhaustively_ignores_upper_option_bits() {
        for option_byte in u8::MIN..=u8::MAX {
            assert_eq!(
                compiled_center_split_q16(option_byte),
                compiled_center_split_q16(option_byte & 7)
            );
        }
    }

    #[test]
    fn control_run_pins_all_five_mode_policies() {
        let original = [0, 0x1_0000, 0x2_0000, 0x3_0000, 0x4_0000, 0x5_0000];
        assert_eq!(
            transform_compiled_control_run(original, 0),
            [-13_107, 58_982, 131_071, 203_160, 275_249, 347_338]
        );
        assert_eq!(
            transform_compiled_control_run(original, 1),
            [-6_553, 62_259, 131_071, 199_883, 268_695, 337_507]
        );
        assert_eq!(transform_compiled_control_run(original, 2), original);
        assert_eq!(
            transform_compiled_control_run(original, 3),
            [6_553, 68_813, 131_073, 193_333, 255_593, 317_853]
        );

        let default = [13_107, 72_090, 131_073, 190_056, 249_039, 308_022];
        for mode in 4..=7 {
            assert_eq!(transform_compiled_control_run(original, mode), default);
        }
    }

    #[test]
    fn control_run_masks_option_to_its_low_three_bits() {
        let original = [-70_001, -2, 0, 9, 80_000, i32::MAX];
        for mode in 0..=7 {
            assert_eq!(
                transform_compiled_control_run(original, mode),
                transform_compiled_control_run(original, mode | 0xf8)
            );
        }
    }

    #[test]
    fn control_run_mode_two_is_identity_at_signed_boundaries() {
        for original in [
            [0, 1, 2, 3, 4, 5],
            [i32::MIN, i32::MAX, 0, -1, 1, i32::MIN],
            [i32::MAX, i32::MIN, i32::MAX, i32::MIN, -1, 0],
        ] {
            assert_eq!(transform_compiled_control_run(original, 2), original);
        }
    }

    #[test]
    fn control_run_uses_original_deltas_and_arithmetic_right_shift() {
        assert_eq!(
            transform_compiled_control_run([100, 200, 400, 800, 1_600, 3_200], 0),
            [-13_007, -12_898, -12_679, -12_240, -11_361, -9_602]
        );

        // A signed Q16 product of -1 by 0x11999 shifts to -2, not the -1
        // produced by truncating signed division toward zero.
        assert_eq!(
            transform_compiled_control_run([1, 0, 0, 0, 0, 0], 0),
            [-13_106, -13_108, -13_108, -13_108, -13_108, -13_108]
        );
    }

    #[test]
    fn control_run_wraps_delta_shift_retention_bias_and_rebuild() {
        // 0 - MIN wraps during delta formation. Its scaled signed product
        // shifts below i32::MIN, so the cast retains the low 32 bits. The
        // first bias and the first cumulative rebuild both wrap as well.
        assert_eq!(
            transform_compiled_control_run([i32::MIN, 0, 0, 0, 0, 0], 0),
            [
                2_147_470_541,
                -214_741_811,
                -214_741_811,
                -214_741_811,
                -214_741_811,
                -214_741_811,
            ]
        );
    }

    #[test]
    fn eight_word_adapter_preserves_endpoints_and_only_transforms_middle_run() {
        let original = [
            i32::MIN,
            0,
            0x1_0000,
            0x2_0000,
            0x3_0000,
            0x4_0000,
            0x5_0000,
            i32::MAX,
        ];
        let transformed = transform_compiled_control_family(original, 0);
        assert_eq!(transformed[0], i32::MIN);
        assert_eq!(transformed[7], i32::MAX);
        assert_eq!(
            transformed[1..=6],
            [-13_107, 58_982, 131_071, 203_160, 275_249, 347_338]
        );
    }

    #[test]
    fn reciprocal_tail_pins_slot_zero_zero_divisors_and_exact_copies() {
        let post_processed_raw = [
            0x1234, 0, 0x2_0000, -0x2_0000, 0x8_000, -0x8_000, 0, 0x1_0000,
        ];
        let families = derive_compiled_control_families(post_processed_raw).unwrap();
        assert_eq!(
            families.reciprocal,
            [0, -1, 0x8000, -0x8000, 0x20000, -0x20000, -1, 0x10000]
        );
        assert_eq!(families.post_processed_raw, post_processed_raw);
        assert_eq!(families.raw_copy, post_processed_raw);
        assert_eq!(families.reciprocal_copy, families.reciprocal);
    }

    #[test]
    fn reciprocal_tail_truncates_signed_quotients_toward_zero() {
        let families = derive_compiled_control_families([
            0, 0x3_0000, -0x3_0000, 0x5_0000, -0x5_0000, 0x7_0000, -0x7_0000, 0x1_0000,
        ])
        .unwrap();
        assert_eq!(families.reciprocal[1], 0x5555);
        assert_eq!(families.reciprocal[2], -0x5555);
        assert_eq!(families.reciprocal[3], 0x3333);
        assert_eq!(families.reciprocal[4], -0x3333);
        assert_eq!(families.reciprocal[5], 0x2492);
        assert_eq!(families.reciprocal[6], -0x2492);

        let near_overflow = derive_compiled_control_families([
            0, 3, -3, 0x1_0000, 0x1_0000, 0x1_0000, 0x1_0000, 0x1_0000,
        ])
        .unwrap();
        assert_eq!(near_overflow.reciprocal[1], 1_431_655_765);
        assert_eq!(near_overflow.reciprocal[2], -1_431_655_765);
    }

    #[test]
    fn reciprocal_tail_accepts_signed_integer_extremes() {
        let post_processed_raw = [
            9,
            i32::MAX,
            i32::MIN,
            0x1_0000,
            -0x1_0000,
            0,
            0x2_0000,
            -0x2_0000,
        ];
        let families = derive_compiled_control_families(post_processed_raw).unwrap();
        assert_eq!(families.reciprocal[1], 2);
        assert_eq!(families.reciprocal[2], -2);
        assert_eq!(families.raw_copy, post_processed_raw);
    }

    #[test]
    fn reciprocal_overflow_is_typed_and_reports_first_slot() {
        for raw in [1, 2, -1] {
            let mut post_processed_raw = [0x1_0000; COMPILED_CONTROL_FAMILY_WORD_COUNT];
            post_processed_raw[3] = raw;
            assert_eq!(
                derive_compiled_control_families(post_processed_raw),
                Err(CompiledReciprocalOverflow { slot: 3, raw })
            );
        }

        let post_processed_raw = [0, 0x1_0000, 2, 1, 0x1_0000, 0x1_0000, 0x1_0000, 0x1_0000];
        assert_eq!(
            derive_compiled_control_families(post_processed_raw),
            Err(CompiledReciprocalOverflow { slot: 2, raw: 2 })
        );
    }

    #[test]
    fn slot_zero_is_never_reciprocated_even_when_it_would_overflow() {
        let post_processed_raw = [
            1, 0x1_0000, 0x1_0000, 0x1_0000, 0x1_0000, 0x1_0000, 0x1_0000, 0x1_0000,
        ];
        let families = derive_compiled_control_families(post_processed_raw).unwrap();
        assert_eq!(families.reciprocal[0], 0);
        assert_eq!(families.raw_copy[0], 1);
    }
}
