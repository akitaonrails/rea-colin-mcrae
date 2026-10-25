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

use cmr_assets::{AuxControlConstantsStore, CompiledInitializerStore};
use cmr_formats::pe_aux_control_initializer::{
    AUX_CONTROL_COMMON_WORD_OFFSETS as FORMAT_AUX_COMMON_WORD_OFFSETS,
    AUX_CONTROL_POLICY_WORD_OFFSETS as FORMAT_AUX_POLICY_WORD_OFFSETS,
    AUX_CONTROL_WORD_OFFSETS as FORMAT_AUX_WORD_OFFSETS,
    AuxControlConstantWords as FormatAuxControlConstantWords,
    AuxControlPolicyWords as FormatAuxControlPolicyWords,
};
use cmr_formats::pe_compiled_initializer::{
    COMPILED_CASE_COUNT as FORMAT_CASE_COUNT,
    COMPILED_CONTROL_WORD_OFFSETS as FORMAT_CONTROL_WORD_OFFSETS,
    COMPILED_STATE_WORD_OFFSETS as FORMAT_STATE_WORD_OFFSETS,
    CompiledCaseWords as FormatCompiledCaseWords,
};

use crate::legacy_math::sqrt_q16;

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

/// Number of dword destinations written by the auxiliary control patch.
pub const AUX_CONTROL_WORD_COUNT: usize = 22;

/// Stride applied to the live configuration block by the compiled helpers.
pub const COMPILED_CONFIG_RECORD_STRIDE: usize = 0x09c;

/// State-slot domain supplied by every resolved explicit constructor.
///
/// This does not describe eight homogeneous or authored configuration
/// records. Only the lookup index domain is established; later slots alias
/// other live configuration-block fields at the observed stride.
pub const COMPILED_CONFIG_SLOT_COUNT: i8 = 8;

/// Unindexed configuration flag whose bit one forces the auxiliary default.
pub const COMPILED_CONFIG_AUX_FORCE_FLAG_OFFSET: usize = 0x026;

/// Unindexed configuration byte controlling the common-prefix scale branches.
pub const COMPILED_CONFIG_COMMON_FLAGS_OFFSET: usize = 0x025;

/// Record-relative auxiliary-policy option byte.
pub const COMPILED_CONFIG_AUX_OPTION_OFFSET: usize = 0x093;

/// Record-relative center-drive-split option byte.
pub const COMPILED_CONFIG_CENTER_SPLIT_OPTION_OFFSET: usize = 0x095;

/// Record-relative six-word-run option byte.
pub const COMPILED_CONFIG_CONTROL_RUN_OPTION_OFFSET: usize = 0x099;

/// Record-relative fallback byte consumed by `FUN_00486090`.
pub const COMPILED_CONFIG_STATE_758_OPTION_OFFSET: usize = 0x091;
pub const COMPILED_CONFIG_STATE_758_SELECTION_OFFSET: usize = 0x6c0;

const COMPILED_CONFIG_AUX_FORCE_DEFAULT_MASK: u8 = 0x02;

/// Auxiliary control destinations in ascending offset order.
pub const AUX_CONTROL_WORD_OFFSETS: [u16; AUX_CONTROL_WORD_COUNT] = [
    0x0d8, 0x0dc, 0x0e0, 0x0e4, 0x0e8, 0x0ec, 0x0f0, 0x0f4, 0x0f8, 0x0fc, 0x100, 0x104, 0x108,
    0x10c, 0x110, 0x118, 0x11c, 0x120, 0x124, 0x128, 0x12c, 0x13c,
];

/// Policy-owned destinations in [`AuxControlPolicyConstants::words`] order.
pub const AUX_CONTROL_POLICY_WORD_OFFSETS: [u16; 7] =
    [0x0f8, 0x10c, 0x110, 0x120, 0x124, 0x128, 0x13c];

/// Common immediate destinations in [`AuxControlConstants::common_words`] order.
pub const AUX_CONTROL_COMMON_WORD_OFFSETS: [u16; 9] = [
    0x0d8, 0x0dc, 0x0e0, 0x0e4, 0x0e8, 0x0ec, 0x0f0, 0x0fc, 0x104,
];

const _: [(); AUX_CONTROL_WORD_OFFSETS.len()] = [(); FORMAT_AUX_WORD_OFFSETS.len()];
const _: [(); AUX_CONTROL_COMMON_WORD_OFFSETS.len()] = [(); FORMAT_AUX_COMMON_WORD_OFFSETS.len()];
const _: [(); AUX_CONTROL_POLICY_WORD_OFFSETS.len()] = [(); FORMAT_AUX_POLICY_WORD_OFFSETS.len()];

/// State destinations in their observed case-body store order.
pub const COMPILED_STATE_WORD_OFFSETS: [u16; COMPILED_STATE_WORD_COUNT] =
    [0x020, 0x024, 0x028, 0x638, 0x63c, 0x640, 0x648, 0x644];

/// Complete state-destination universe of the recovered common prefix.
pub const COMPILED_COMMON_PREFIX_STATE_OFFSETS: [u16; COMPILED_STATE_WORD_COUNT + 2] = [
    0x020, 0x024, 0x028, 0x638, 0x63c, 0x640, 0x648, 0x644, 0x048, 0x04c,
];

/// Complete control-destination universe of the recovered common prefix.
pub const COMPILED_COMMON_PREFIX_CONTROL_OFFSETS: [u16; 2] = [0x148, 0x15c];

/// Control dword destinations in canonical ascending offset order.
pub const COMPILED_POST_HELPER_CONTROL_WORD_OFFSETS: [u16; 15] = [
    0x144, 0x14c, 0x150, 0x154, 0x160, 0x174, 0x178, 0x17c, 0x180, 0x184, 0x188, 0x18c, 0x1a8,
    0x1b4, 0x408,
];

pub const COMPILED_POST_HELPER_STATE_WORD_OFFSETS: [u16; 1] = [0x0ec];
pub const COMPILED_POST_HELPER_CONTROL_BYTE_OFFSETS: [u16; 1] = [0x440];
pub const COMPILED_POST_HELPER_CONTROL_U16_OFFSETS: [u16; 1] = [0x3e4];

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

/// Unindexed configuration flag that scales two direct control destinations.
pub const COMPILED_COMMON_CONTROL_SCALE_MASK: u8 = 0x02;

/// Unindexed configuration flag that scales all eight direct state words.
pub const COMPILED_COMMON_STATE_SCALE_MASK: u8 = 0x04;

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

/// Exact writes performed by the observed common prefix.
///
/// `None` means the corresponding flag-controlled destination was not written.
/// This is a bounded patch, not a complete state/control object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompiledCommonPrefixEffects {
    pub control_word_148: i32,
    pub control_word_15c: Option<i32>,
    pub state_words: Option<[i32; COMPILED_STATE_WORD_COUNT]>,
    pub state_word_48: i32,
    pub state_word_4c: i32,
}

/// A derived state `+0x48` whose signed `2^32 / value` quotient would make the
/// shipped x86 `IDIV` fault.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompiledCommonPrefixReciprocalOverflow {
    pub state_word_48: i32,
}

impl fmt::Display for CompiledCommonPrefixReciprocalOverflow {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "compiled common-prefix reciprocal overflows i32 for state +0x48 value {}",
            self.state_word_48
        )
    }
}

impl std::error::Error for CompiledCommonPrefixReciprocalOverflow {}

/// Apply the common prefix reached after both valid and invalid selectors.
///
/// The raw unindexed config byte controls two independent scaling branches.
/// Control `+0x148` is always adjusted before optional scaling; state `+0x48`
/// is then derived from control `+0x140`, and state `+0x4c` is its guarded
/// signed Q16 reciprocal. Every arithmetic store uses the observed 32-bit
/// wrapping/low-word behavior. On reciprocal overflow this pure contract
/// returns no partial effect bundle, an explicit reconstruction-safety policy;
/// the shipped in-place routine had already performed earlier writes.
pub fn derive_compiled_common_prefix(
    words: CompiledCaseWords,
    config_flags: u8,
) -> Result<CompiledCommonPrefixEffects, CompiledCommonPrefixReciprocalOverflow> {
    let adjusted_word_148 = words.control_words[0].wrapping_sub(0x051e);
    let control_scale_enabled = config_flags & COMPILED_COMMON_CONTROL_SCALE_MASK != 0;
    let control_word_148 = if control_scale_enabled {
        common_prefix_q16_mul(adjusted_word_148, 0x2_0000)
    } else {
        adjusted_word_148
    };
    let control_word_15c =
        control_scale_enabled.then(|| common_prefix_q16_mul(words.control_words[3], 0x1_3333));
    let state_words = (config_flags & COMPILED_COMMON_STATE_SCALE_MASK != 0).then(|| {
        words
            .state_words
            .map(|word| common_prefix_q16_mul(word, 0x8000))
    });

    let state_word_48 = words.control_words[22].wrapping_add(0x78_0000);
    let state_word_4c = if state_word_48 == 0 {
        -1
    } else {
        let quotient = (1_i64 << 32) / i64::from(state_word_48);
        i32::try_from(quotient)
            .map_err(|_| CompiledCommonPrefixReciprocalOverflow { state_word_48 })?
    };

    Ok(CompiledCommonPrefixEffects {
        control_word_148,
        control_word_15c,
        state_words,
        state_word_48,
        state_word_4c,
    })
}

fn common_prefix_q16_mul(left: i32, right: i32) -> i32 {
    ((i64::from(left) * i64::from(right)) >> 16) as i32
}

pub const COMPILED_CONTROL_PRODUCT_RECIPROCAL_OFFSET: u16 = 0x164;
pub const COMPILED_CONTROL_PRODUCT_OFFSET: u16 = 0x168;
pub const COMPILED_CONTROL_PRODUCT_OFFSETS: [u16; 2] = [
    COMPILED_CONTROL_PRODUCT_RECIPROCAL_OFFSET,
    COMPILED_CONTROL_PRODUCT_OFFSET,
];

/// State destinations written by the bounded suffix-default interval.
pub const COMPILED_SUFFIX_DEFAULT_STATE_OFFSETS: [u16; 4] = [0x044, 0x050, 0x054, 0x058];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompiledSuffixDefaultEffects {
    pub state_word_44: i32,
    pub state_words_50_54_58: [i32; 3],
}

/// Reconstruct initializer interval `[0x4066dd,0x406831)`.
///
/// The first three effective state words are arithmetic-halved into stack
/// temporaries, squared with wrapping signed Q16 projection, and accumulated
/// with wrapping adds before the shared generated-table square root. The
/// halves are not written back. Raw integer seeds `[1, 2, 1]` are written to
/// `+0x50/+0x54/+0x58`; config `+0x25 & 0x04` doubles them through the shipped
/// Q16 multiply.
#[must_use]
pub fn derive_compiled_suffix_defaults(
    effective_state_words_20_24_28: [i32; 3],
    config_flags: u8,
) -> CompiledSuffixDefaultEffects {
    let halves = effective_state_words_20_24_28.map(|word| word >> 1);
    let square_sum = halves.into_iter().fold(0_i32, |sum, word| {
        sum.wrapping_add(common_prefix_q16_mul(word, word))
    });
    let state_word_44 = sqrt_q16(square_sum as u32) as i32;

    let mut state_words_50_54_58 = [1, 2, 1];
    if config_flags & COMPILED_COMMON_STATE_SCALE_MASK != 0 {
        state_words_50_54_58 =
            state_words_50_54_58.map(|word| common_prefix_q16_mul(word, 0x2_0000));
    }

    CompiledSuffixDefaultEffects {
        state_word_44,
        state_words_50_54_58,
    }
}

/// Two recovered control writes immediately after the reciprocal/copy families.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompiledControlProductPair {
    pub reciprocal_word_164: i32,
    pub product_word_168: i32,
}

/// A quarter-product whose signed `2^32 / value` quotient would fault `IDIV`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompiledControlProductReciprocalOverflow {
    pub divisor: i32,
}

impl fmt::Display for CompiledControlProductReciprocalOverflow {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "compiled control +0x164 reciprocal overflows i32 for divisor {}",
            self.divisor
        )
    }
}

impl std::error::Error for CompiledControlProductReciprocalOverflow {}

/// Reconstruct the paired control writes at `+0x164` and `+0x168`.
///
/// The product keeps bits 16..47 of the signed full-width multiplication,
/// matching `IMUL` followed by `SHRD 16`. Its arithmetic quarter is the
/// reciprocal divisor. Zero maps to `-1`; a quotient outside signed 32-bit is
/// reported instead of reproducing the shipped x86 divide fault. The pure
/// result is pair-atomic, although the shipped initializer had already made
/// earlier writes before reaching this interval. Computing the product once is
/// equivalent to the shipped reload/recompute only for stable, non-volatile
/// object memory; concurrent mutation is outside this pure contract.
pub fn derive_compiled_control_product_pair(
    effective_control_word_15c: i32,
    reciprocal_word_208: i32,
) -> Result<CompiledControlProductPair, CompiledControlProductReciprocalOverflow> {
    let product_word_168 =
        ((i64::from(effective_control_word_15c) * i64::from(reciprocal_word_208)) >> 16) as i32;
    let divisor = product_word_168 >> 2;
    let reciprocal_word_164 = if divisor == 0 {
        -1
    } else {
        let quotient = (1_i64 << 32) / i64::from(divisor);
        i32::try_from(quotient).map_err(|_| CompiledControlProductReciprocalOverflow { divisor })?
    };

    Ok(CompiledControlProductPair {
        reciprocal_word_164,
        product_word_168,
    })
}

/// Sole state destination written by `FUN_00486090`.
pub const COMPILED_STATE_WORD_758_OFFSET: u16 = 0x758;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompiledStateWord758Effect {
    pub state_word_758: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompiledStateWord758SelectionError {
    TruncatedSelectionByte {
        required_len: usize,
        actual_len: usize,
    },
    TableTruncated {
        index: usize,
        table_len: usize,
    },
}

impl fmt::Display for CompiledStateWord758SelectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TruncatedSelectionByte {
                required_len,
                actual_len,
            } => write!(
                formatter,
                "compiled state +0x758 selection needs {required_len} config bytes, got {actual_len}"
            ),
            Self::TableTruncated { index, table_len } => write!(
                formatter,
                "compiled state +0x758 selector needs table index {index}, but length is {table_len}"
            ),
        }
    }
}

impl std::error::Error for CompiledStateWord758SelectionError {}

/// Reconstruct the caller's requested-value selection before `FUN_00486090`.
///
/// The table remains caller-owned. Context-zero and the gated-default region
/// return the `-1` sentinel without inspecting it. A short table on the table
/// route is a typed safe-policy error; the shipped fixed-address read was
/// unchecked.
pub fn select_compiled_state_word_758_argument(
    context_nonzero: bool,
    selection_byte: u8,
    table: &[i32],
) -> Result<i32, CompiledStateWord758SelectionError> {
    if !context_nonzero || (selection_byte & 0x18) >= 0x10 && (selection_byte & 0xe0) >= 0x80 {
        return Ok(-1);
    }

    let row = usize::from((selection_byte >> 3) & 3);
    let column = usize::from(selection_byte >> 5);
    let index = row * 5 + column;
    table
        .get(index)
        .copied()
        .ok_or(CompiledStateWord758SelectionError::TableTruncated {
            index,
            table_len: table.len(),
        })
}

/// Apply the exact `FUN_00486090` sentinel normalization.
///
/// Only `-1` selects the record-relative config fallback. Every other 32-bit
/// value is written verbatim; the fallback is masked to its low three bits.
#[must_use]
pub const fn derive_compiled_state_word_758(
    requested_value: i32,
    config_fallback: u8,
) -> CompiledStateWord758Effect {
    CompiledStateWord758Effect {
        state_word_758: if requested_value == -1 {
            (config_fallback & 7) as i32
        } else {
            requested_value
        },
    }
}

/// Exact writes performed after the recovered helpers and before the family
/// reciprocal/copy tail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompiledPostHelperEffects {
    /// Values corresponding to [`COMPILED_POST_HELPER_CONTROL_WORD_OFFSETS`].
    pub control_words: [i32; COMPILED_POST_HELPER_CONTROL_WORD_OFFSETS.len()],
    pub state_words: [i32; COMPILED_POST_HELPER_STATE_WORD_OFFSETS.len()],
    pub control_bytes: [u8; COMPILED_POST_HELPER_CONTROL_BYTE_OFFSETS.len()],
    pub control_u16: [u16; COMPILED_POST_HELPER_CONTROL_U16_OFFSETS.len()],
}

/// A control `+0x15c` value whose signed `2^32 / value` quotient would make
/// the shipped x86 `IDIV` fault before the family tail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompiledPostHelperReciprocalOverflow {
    pub control_word_15c: i32,
}

impl fmt::Display for CompiledPostHelperReciprocalOverflow {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "compiled post-helper reciprocal overflows i32 for control +0x15c value {}",
            self.control_word_15c
        )
    }
}

impl std::error::Error for CompiledPostHelperReciprocalOverflow {}

/// Derive the exact writes in `[0x406324,0x40645f)`.
///
/// The two inputs are the effective values after the recovered common prefix.
/// Zero at `+0x15c` maps to `-1`; another value uses signed `2^32 / value`
/// with truncation toward zero. A quotient overflow returns no patch, an
/// explicit safe policy; the shipped in-place routine had already performed
/// earlier stores before its unguarded `IDIV`.
pub fn derive_compiled_post_helper_effects(
    control_word_148: i32,
    control_word_15c: i32,
) -> Result<CompiledPostHelperEffects, CompiledPostHelperReciprocalOverflow> {
    let reciprocal_160 = if control_word_15c == 0 {
        -1
    } else {
        let quotient = (1_i64 << 32) / i64::from(control_word_15c);
        i32::try_from(quotient)
            .map_err(|_| CompiledPostHelperReciprocalOverflow { control_word_15c })?
    };

    Ok(CompiledPostHelperEffects {
        control_words: [
            0x83,
            control_word_148,
            0x0ccc,
            0x0ccc,
            reciprocal_160,
            0x0ccc,
            0x0ccc,
            0x0ccc,
            0x0ccc,
            0x8000,
            0x3333,
            0x3333,
            0x0_fae1,
            0x8000,
            0,
        ],
        state_words: [0],
        control_bytes: [3],
        control_u16: [0x02aa],
    })
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

/// Raw option/flag bytes resolved for the recovered common path.
///
/// The existing arithmetic helpers own low-three-bit masking. Keeping the raw
/// bytes here makes this resolver only the checked replacement for the
/// observed configuration reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompiledConfigOptions {
    pub common_prefix_flags: u8,
    pub state_word_758_fallback: u8,
    pub center_split: u8,
    pub control_run: u8,
    pub aux_control: AuxControlOption,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompiledConfigResolveError {
    NegativeIndex {
        index: i8,
    },
    IndexOutOfRange {
        index: i8,
        slot_count: i8,
    },
    TruncatedConfig {
        index: i8,
        required_len: usize,
        actual_len: usize,
    },
}

impl fmt::Display for CompiledConfigResolveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NegativeIndex { index } => {
                write!(
                    formatter,
                    "compiled configuration index is negative: {index}"
                )
            }
            Self::IndexOutOfRange { index, slot_count } => write!(
                formatter,
                "compiled configuration index {index} is outside {slot_count} constructed slots"
            ),
            Self::TruncatedConfig {
                index,
                required_len,
                actual_len,
            } => write!(
                formatter,
                "compiled configuration index {index} requires {required_len} bytes, got {actual_len}"
            ),
        }
    }
}

impl std::error::Error for CompiledConfigResolveError {}

/// Resolve the option and flag bytes read by the recovered common path.
///
/// The shipped code sign-extends a state byte, multiplies it by `0x9c`, and
/// performs unchecked reads. This safe reconstruction rejects a negative
/// index, applies a fail-closed rejection outside the eight-slot domain of the
/// resolved explicit constructors, and proves the caller-owned slice contains
/// the highest addressed byte before reading anything. The eight slots are not
/// claimed to be homogeneous authored records, and the enclosing image need
/// not end on a stride boundary.
pub fn resolve_compiled_config_options(
    config: &[u8],
    signed_index: i8,
) -> Result<CompiledConfigOptions, CompiledConfigResolveError> {
    let index =
        usize::try_from(signed_index).map_err(|_| CompiledConfigResolveError::NegativeIndex {
            index: signed_index,
        })?;
    if signed_index >= COMPILED_CONFIG_SLOT_COUNT {
        return Err(CompiledConfigResolveError::IndexOutOfRange {
            index: signed_index,
            slot_count: COMPILED_CONFIG_SLOT_COUNT,
        });
    }
    let record_base = index * COMPILED_CONFIG_RECORD_STRIDE;
    let required_len = record_base + COMPILED_CONFIG_CONTROL_RUN_OPTION_OFFSET + 1;
    if config.len() < required_len {
        return Err(CompiledConfigResolveError::TruncatedConfig {
            index: signed_index,
            required_len,
            actual_len: config.len(),
        });
    }

    let aux_control = if config[COMPILED_CONFIG_AUX_FORCE_FLAG_OFFSET]
        & COMPILED_CONFIG_AUX_FORCE_DEFAULT_MASK
        != 0
    {
        AuxControlOption::ForcedDefault
    } else {
        AuxControlOption::Resolved(config[record_base + COMPILED_CONFIG_AUX_OPTION_OFFSET])
    };

    Ok(CompiledConfigOptions {
        common_prefix_flags: config[COMPILED_CONFIG_COMMON_FLAGS_OFFSET],
        state_word_758_fallback: config[record_base + COMPILED_CONFIG_STATE_758_OPTION_OFFSET],
        center_split: config[record_base + COMPILED_CONFIG_CENTER_SPLIT_OPTION_OFFSET],
        control_run: config[record_base + COMPILED_CONFIG_CONTROL_RUN_OPTION_OFFSET],
        aux_control,
    })
}

/// Caller-owned literals for one auxiliary-control policy.
///
/// `words` follows [`AUX_CONTROL_POLICY_WORD_OFFSETS`]. The first divisor feeds
/// control `+0x11c`; the reciprocal of the second is Q16-multiplied into
/// control `+0x12c`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuxControlPolicyConstants {
    pub words: [i32; 7],
    pub reciprocal_divisor: i32,
    pub scaled_reciprocal_divisor: i32,
}

/// Caller-owned literal set for the auxiliary 22-word control patch.
///
/// No shipped tuning payload is embedded here. `common_words` follows
/// [`AUX_CONTROL_COMMON_WORD_OFFSETS`]. The Q16 multipliers feed `+0xf4` and
/// `+0x100`, respectively.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuxControlConstants {
    pub common_words: [i32; 9],
    pub q16_multipliers: [i32; 2],
    pub policy_one: AuxControlPolicyConstants,
    pub policy_two: AuxControlPolicyConstants,
    pub default_policy: AuxControlPolicyConstants,
}

/// Owned auxiliary constants copied from a verified user-data store.
///
/// Construction checks exact format/core offset order. The policy order is
/// copied explicitly as one, two, then default; fixed array types prove each
/// record's word count. Codec shape is authenticated by the input store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuxControlConstantSet {
    constants: AuxControlConstants,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuxControlConstantSetError {
    OffsetOrder {
        group: &'static str,
        index: usize,
        core_offset: u16,
        format_offset: u16,
    },
}

impl fmt::Display for AuxControlConstantSetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "auxiliary-control store schema does not match the core contract: {self:?}"
        )
    }
}

impl std::error::Error for AuxControlConstantSetError {}

impl AuxControlConstantSet {
    pub fn from_store(
        store: &AuxControlConstantsStore,
    ) -> Result<Self, AuxControlConstantSetError> {
        Self::from_format(store.constants())
    }

    fn from_format(
        constants: &FormatAuxControlConstantWords,
    ) -> Result<Self, AuxControlConstantSetError> {
        verify_aux_control_format_schema()?;
        Ok(Self {
            constants: AuxControlConstants {
                common_words: constants.common_words,
                q16_multipliers: constants.q16_multipliers,
                policy_one: copy_aux_policy(constants.policy_one),
                policy_two: copy_aux_policy(constants.policy_two),
                default_policy: copy_aux_policy(constants.default_policy),
            },
        })
    }

    #[must_use]
    pub const fn constants(&self) -> &AuxControlConstants {
        &self.constants
    }

    pub fn derive(
        &self,
        option: AuxControlOption,
        state_word_48: i32,
        global_word: i32,
    ) -> Result<AuxControlPatch, AuxControlReciprocalOverflow> {
        derive_aux_control_patch(option, state_word_48, global_word, &self.constants)
    }
}

impl TryFrom<&AuxControlConstantsStore> for AuxControlConstantSet {
    type Error = AuxControlConstantSetError;

    fn try_from(store: &AuxControlConstantsStore) -> Result<Self, Self::Error> {
        Self::from_store(store)
    }
}

const fn copy_aux_policy(policy: FormatAuxControlPolicyWords) -> AuxControlPolicyConstants {
    AuxControlPolicyConstants {
        words: policy.words,
        reciprocal_divisor: policy.reciprocal_divisor,
        scaled_reciprocal_divisor: policy.scaled_reciprocal_divisor,
    }
}

fn verify_aux_control_format_schema() -> Result<(), AuxControlConstantSetError> {
    for (group, core_offsets, format_offsets) in [
        (
            "all outputs",
            AUX_CONTROL_WORD_OFFSETS.as_slice(),
            FORMAT_AUX_WORD_OFFSETS.as_slice(),
        ),
        (
            "common words",
            AUX_CONTROL_COMMON_WORD_OFFSETS.as_slice(),
            FORMAT_AUX_COMMON_WORD_OFFSETS.as_slice(),
        ),
        (
            "policy words",
            AUX_CONTROL_POLICY_WORD_OFFSETS.as_slice(),
            FORMAT_AUX_POLICY_WORD_OFFSETS.as_slice(),
        ),
    ] {
        for (index, (&core_offset, &format_offset)) in
            core_offsets.iter().zip(format_offsets).enumerate()
        {
            if core_offset != format_offset {
                return Err(AuxControlConstantSetError::OffsetOrder {
                    group,
                    index,
                    core_offset,
                    format_offset,
                });
            }
        }
    }

    Ok(())
}

/// Option source for the auxiliary policy selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuxControlOption {
    /// The controlling flag bypasses the option lookup and selects the default.
    ForcedDefault,
    /// A safely resolved option byte; only its low three bits are observed.
    Resolved(u8),
}

/// The complete auxiliary control patch in [`AUX_CONTROL_WORD_OFFSETS`] order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuxControlPatch {
    pub words: [i32; AUX_CONTROL_WORD_COUNT],
}

/// Reciprocal input whose signed quotient does not fit the x86 destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuxControlReciprocalInput {
    CommonWord,
    PolicyDivisor,
    ScaledPolicyDivisor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuxControlReciprocalOverflow {
    pub input: AuxControlReciprocalInput,
    pub divisor: i32,
}

impl fmt::Display for AuxControlReciprocalOverflow {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "auxiliary control reciprocal {:?} overflows i32 for divisor {}",
            self.input, self.divisor
        )
    }
}

impl std::error::Error for AuxControlReciprocalOverflow {}

/// Derive the standalone auxiliary 22-word control patch.
///
/// Policy one is selected by masked option `1`, policy two by `2`, and options
/// `0,3..=7` select the default. [`AuxControlOption::ForcedDefault`] represents
/// the observed bypass without requiring an option lookup. The result is
/// assembled only after every signed reciprocal has been validated.
pub fn derive_aux_control_patch(
    option: AuxControlOption,
    state_word_48: i32,
    global_word: i32,
    constants: &AuxControlConstants,
) -> Result<AuxControlPatch, AuxControlReciprocalOverflow> {
    let policy = match option {
        AuxControlOption::Resolved(option_byte) => match option_byte & 7 {
            1 => &constants.policy_one,
            2 => &constants.policy_two,
            _ => &constants.default_policy,
        },
        AuxControlOption::ForcedDefault => &constants.default_policy,
    };

    let common_reciprocal = aux_control_reciprocal(
        constants.common_words[8],
        AuxControlReciprocalInput::CommonWord,
    )?;
    let policy_reciprocal = aux_control_reciprocal(
        policy.reciprocal_divisor,
        AuxControlReciprocalInput::PolicyDivisor,
    )?;
    let scaled_policy_reciprocal = aux_control_reciprocal(
        policy.scaled_reciprocal_divisor,
        AuxControlReciprocalInput::ScaledPolicyDivisor,
    )?;

    Ok(AuxControlPatch {
        words: [
            constants.common_words[0],
            constants.common_words[1],
            constants.common_words[2],
            constants.common_words[3],
            constants.common_words[4],
            constants.common_words[5],
            constants.common_words[6],
            aux_control_q16_mul(global_word, constants.q16_multipliers[0]),
            policy.words[0],
            constants.common_words[7],
            aux_control_q16_mul(global_word, constants.q16_multipliers[1]),
            constants.common_words[8],
            common_reciprocal,
            policy.words[1],
            policy.words[2],
            state_word_48 >> 3,
            policy_reciprocal,
            policy.words[3],
            policy.words[4],
            policy.words[5],
            aux_control_q16_mul(scaled_policy_reciprocal, global_word),
            policy.words[6],
        ],
    })
}

fn aux_control_q16_mul(left: i32, right: i32) -> i32 {
    ((i64::from(left) * i64::from(right)) >> 16) as i32
}

fn aux_control_reciprocal(
    divisor: i32,
    input: AuxControlReciprocalInput,
) -> Result<i32, AuxControlReciprocalOverflow> {
    if divisor == 0 {
        return Ok(-1);
    }
    let quotient = (1_i64 << 32) / i64::from(divisor);
    i32::try_from(quotient).map_err(|_| AuxControlReciprocalOverflow { input, divisor })
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

/// Caller-owned inputs for composing only the recovered initializer stages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecoveredCompiledStageInputs<'a> {
    pub prior_case_words: CompiledCaseWords,
    pub selector: u32,
    pub config: &'a [u8],
    pub state_slot: i8,
    pub global_word: i32,
    pub state_758_context_nonzero: bool,
    pub state_758_table: &'a [i32],
}

/// Separate effects of the recovered post-dispatch initializer stages.
///
/// This is deliberately not a complete state/control snapshot. In particular,
/// `direct_case_stores` remains the raw case-body outcome before omitted
/// common-prefix mutations. Only `control_families.post_processed_raw`
/// supersedes its eight family words; the other three arrays are the later
/// reciprocal/copy-tail effects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecoveredCompiledStageEffects {
    pub direct_case_stores: CompiledCaseOutcome,
    pub common_prefix: CompiledCommonPrefixEffects,
    pub center_split_q16: i32,
    pub aux_control_patch: AuxControlPatch,
    pub state_word_758: CompiledStateWord758Effect,
    pub post_helper: CompiledPostHelperEffects,
    pub control_families: CompiledControlFamilies,
    pub control_product_pair: CompiledControlProductPair,
    pub suffix_defaults: CompiledSuffixDefaultEffects,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveredCompiledStageError {
    Config(CompiledConfigResolveError),
    Prefix(CompiledCommonPrefixReciprocalOverflow),
    Aux(AuxControlReciprocalOverflow),
    State758Selection(CompiledStateWord758SelectionError),
    PostHelper(CompiledPostHelperReciprocalOverflow),
    Family(CompiledReciprocalOverflow),
    ProductPair(CompiledControlProductReciprocalOverflow),
}

impl fmt::Display for RecoveredCompiledStageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(source) => write!(formatter, "cannot resolve compiled options: {source}"),
            Self::Prefix(source) => write!(formatter, "cannot derive common prefix: {source}"),
            Self::Aux(source) => write!(formatter, "cannot derive auxiliary patch: {source}"),
            Self::State758Selection(source) => {
                write!(formatter, "cannot select state +0x758 value: {source}")
            }
            Self::PostHelper(source) => {
                write!(formatter, "cannot derive post-helper patch: {source}")
            }
            Self::Family(source) => {
                write!(
                    formatter,
                    "cannot derive compiled control families: {source}"
                )
            }
            Self::ProductPair(source) => {
                write!(
                    formatter,
                    "cannot derive compiled control product pair: {source}"
                )
            }
        }
    }
}

impl std::error::Error for RecoveredCompiledStageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Config(source) => Some(source),
            Self::Prefix(source) => Some(source),
            Self::Aux(source) => Some(source),
            Self::State758Selection(source) => Some(source),
            Self::PostHelper(source) => Some(source),
            Self::Family(source) => Some(source),
            Self::ProductPair(source) => Some(source),
        }
    }
}

/// Compose only the recovered post-dispatch stages in their observed order.
///
/// The returned bundle keeps raw direct stores separate from the exact prefix,
/// center-split, auxiliary, and family/tail effects. The family/tail is derived
/// from and supersedes only the raw direct-case family, which the prefix and
/// post-helper patch do not touch. Config, prefix, auxiliary, state-`+0x758`
/// selection, post-helper, family, then product-pair validation order applies
/// only to the represented fallible stages. The infallible suffix-default
/// effect follows the product pair. Success does not prove a complete shipped
/// initializer snapshot. Any represented failure returns no partial bundle.
/// That atomic error boundary is a safe
/// reconstruction policy; the shipped in-place code could have performed
/// earlier writes before an `IDIV` fault.
pub fn compose_recovered_compiled_stage_effects(
    direct_cases: &CompiledDirectCaseSet,
    aux_constants: &AuxControlConstantSet,
    input: RecoveredCompiledStageInputs<'_>,
) -> Result<RecoveredCompiledStageEffects, RecoveredCompiledStageError> {
    let options = resolve_compiled_config_options(input.config, input.state_slot)
        .map_err(RecoveredCompiledStageError::Config)?;
    let direct_case_stores = direct_cases.apply(input.prior_case_words, input.selector);
    let common_prefix =
        derive_compiled_common_prefix(direct_case_stores.words, options.common_prefix_flags)
            .map_err(RecoveredCompiledStageError::Prefix)?;
    let center_split_q16 = compiled_center_split_q16(options.center_split);
    let transformed_control_family = transform_compiled_control_family(
        direct_case_stores.words.control_family(),
        options.control_run,
    );
    let aux_control_patch = aux_constants
        .derive(
            options.aux_control,
            common_prefix.state_word_48,
            input.global_word,
        )
        .map_err(RecoveredCompiledStageError::Aux)?;
    let selection_byte = if input.state_758_context_nonzero {
        *input
            .config
            .get(COMPILED_CONFIG_STATE_758_SELECTION_OFFSET)
            .ok_or(RecoveredCompiledStageError::State758Selection(
                CompiledStateWord758SelectionError::TruncatedSelectionByte {
                    required_len: COMPILED_CONFIG_STATE_758_SELECTION_OFFSET + 1,
                    actual_len: input.config.len(),
                },
            ))?
    } else {
        0
    };
    let state_word_758_argument = select_compiled_state_word_758_argument(
        input.state_758_context_nonzero,
        selection_byte,
        input.state_758_table,
    )
    .map_err(RecoveredCompiledStageError::State758Selection)?;
    let state_word_758 =
        derive_compiled_state_word_758(state_word_758_argument, options.state_word_758_fallback);
    let effective_control_word_15c = common_prefix
        .control_word_15c
        .unwrap_or(direct_case_stores.words.control_words[3]);
    let post_helper = derive_compiled_post_helper_effects(
        common_prefix.control_word_148,
        effective_control_word_15c,
    )
    .map_err(RecoveredCompiledStageError::PostHelper)?;
    let control_families = derive_compiled_control_families(transformed_control_family)
        .map_err(RecoveredCompiledStageError::Family)?;
    let control_product_pair = derive_compiled_control_product_pair(
        effective_control_word_15c,
        control_families.reciprocal[6],
    )
    .map_err(RecoveredCompiledStageError::ProductPair)?;
    let effective_state_words = common_prefix
        .state_words
        .unwrap_or(direct_case_stores.words.state_words);
    let suffix_defaults = derive_compiled_suffix_defaults(
        [
            effective_state_words[0],
            effective_state_words[1],
            effective_state_words[2],
        ],
        options.common_prefix_flags,
    );

    Ok(RecoveredCompiledStageEffects {
        direct_case_stores,
        common_prefix,
        center_split_q16,
        aux_control_patch,
        state_word_758,
        post_helper,
        control_families,
        control_product_pair,
        suffix_defaults,
    })
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
        AUX_CONTROL_DATA_FILE, AUX_CONTROL_IMPORT_SCHEMA_VERSION,
        AUX_CONTROL_PROFILE_SCHEMA_VERSION, AUX_CONTROL_STATE_FILE, AuxControlEncoding,
        AuxControlExecutable, AuxControlImportFile, AuxControlImportShape, AuxControlImportSummary,
        AuxControlProfile, AuxControlSpec, COMPILED_INITIALIZER_DATA_FILE,
        COMPILED_INITIALIZER_IMPORT_SCHEMA_VERSION, COMPILED_INITIALIZER_STATE_FILE,
        CompiledInitializerEncoding, CompiledInitializerExecutable, CompiledInitializerImportFile,
        CompiledInitializerImportShape, CompiledInitializerImportSummary,
        CompiledInitializerProfile, CompiledInitializerSpec, ReviewedAuxControlProfile,
        ReviewedCompiledInitializerProfile,
    };
    use cmr_formats::pe_aux_control_initializer::{
        AUX_CONTROL_COMMON_WORD_COUNT as FORMAT_AUX_COMMON_WORD_COUNT,
        AUX_CONTROL_ENCODED_LEN as FORMAT_AUX_ENCODED_LEN,
        AUX_CONTROL_FORMAT_VERSION as FORMAT_AUX_FORMAT_VERSION,
        AUX_CONTROL_MULTIPLIER_COUNT as FORMAT_AUX_MULTIPLIER_COUNT,
        AUX_CONTROL_POLICY_COUNT as FORMAT_AUX_POLICY_COUNT,
        AUX_CONTROL_POLICY_DIVISOR_COUNT as FORMAT_AUX_POLICY_DIVISOR_COUNT,
        AUX_CONTROL_POLICY_WORD_COUNT as FORMAT_AUX_POLICY_WORD_COUNT,
        AUX_CONTROL_TOTAL_WORD_COUNT as FORMAT_AUX_TOTAL_WORD_COUNT,
        AuxControlConstantWords as FormatAuxControlConstantWords,
        AuxControlPolicyWords as FormatAuxControlPolicyWords,
        OFFICIAL_EU_AUX_CONTROL_INITIALIZER_LAYOUT, encode_aux_control_constants,
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

    fn aux_policy(tag: i32) -> AuxControlPolicyConstants {
        AuxControlPolicyConstants {
            words: std::array::from_fn(|index| tag + index as i32 + 1),
            reciprocal_divisor: 0x1_0000,
            scaled_reciprocal_divisor: 0x2_0000,
        }
    }

    fn aux_constants() -> AuxControlConstants {
        AuxControlConstants {
            common_words: [10, 11, 12, 13, 14, 15, 16, 17, 0x1_0000],
            q16_multipliers: [0x1_0000, 0x2_0000],
            policy_one: aux_policy(100),
            policy_two: aux_policy(200),
            default_policy: aux_policy(300),
        }
    }

    fn format_aux_constants() -> FormatAuxControlConstantWords {
        FormatAuxControlConstantWords {
            common_words: [i32::MIN, i32::MAX, 0, -1, -2, 3, -4, 5, 0x1_0000],
            q16_multipliers: [i32::MIN, i32::MAX],
            policy_one: FormatAuxControlPolicyWords {
                words: [i32::MAX, i32::MIN, 0, -1, 11, -12, 13],
                reciprocal_divisor: 0x1_0000,
                scaled_reciprocal_divisor: -0x3_0000,
            },
            policy_two: FormatAuxControlPolicyWords {
                words: [-20, 21, -22, 23, -24, 25, -26],
                reciprocal_divisor: -0x2_0000,
                scaled_reciprocal_divisor: 0,
            },
            default_policy: FormatAuxControlPolicyWords {
                words: [30, -31, 32, -33, 34, -35, 36],
                reciprocal_divisor: i32::MIN,
                scaled_reciprocal_divisor: i32::MAX,
            },
        }
    }

    fn core_aux_constants(format: &FormatAuxControlConstantWords) -> AuxControlConstants {
        AuxControlConstants {
            common_words: format.common_words,
            q16_multipliers: format.q16_multipliers,
            policy_one: copy_aux_policy(format.policy_one),
            policy_two: copy_aux_policy(format.policy_two),
            default_policy: copy_aux_policy(format.default_policy),
        }
    }

    fn open_synthetic_aux_store(
        constants: &FormatAuxControlConstantWords,
    ) -> (TempDirectory, AuxControlConstantsStore) {
        let temp = TempDirectory::new();
        let root = temp.0.join("aux-store");
        fs::create_dir(&root).unwrap();
        let source = b"synthetic aux-control source identity";
        let layout = OFFICIAL_EU_AUX_CONTROL_INITIALIZER_LAYOUT;
        let profile = AuxControlProfile {
            schema_version: AUX_CONTROL_PROFILE_SCHEMA_VERSION,
            id: "synthetic-core-aux-adapter".into(),
            executable: AuxControlExecutable {
                path: "Rally.exe".into(),
                size: source.len() as u64,
                sha256: digest(source),
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
        };
        let reviewed =
            ReviewedAuxControlProfile::from_json(&serde_json::to_vec(&profile).unwrap()).unwrap();
        let data = encode_aux_control_constants(constants);
        fs::write(root.join(AUX_CONTROL_DATA_FILE), data).unwrap();
        let summary = AuxControlImportSummary {
            schema_version: AUX_CONTROL_IMPORT_SCHEMA_VERSION,
            profile: reviewed.profile().id.clone(),
            profile_sha256: reviewed.sha256().into(),
            source: AuxControlImportFile {
                path: reviewed.profile().executable.path.clone(),
                size: reviewed.profile().executable.size,
                sha256: reviewed.profile().executable.sha256.clone(),
            },
            output: AuxControlImportFile {
                path: AUX_CONTROL_DATA_FILE.into(),
                size: FORMAT_AUX_ENCODED_LEN as u64,
                sha256: digest(&data),
            },
            shape: AuxControlImportShape {
                format_version: FORMAT_AUX_FORMAT_VERSION,
                common_word_count: FORMAT_AUX_COMMON_WORD_COUNT as u32,
                multiplier_count: FORMAT_AUX_MULTIPLIER_COUNT as u32,
                policy_count: FORMAT_AUX_POLICY_COUNT as u32,
                policy_word_count: FORMAT_AUX_POLICY_WORD_COUNT as u32,
                policy_divisor_count: FORMAT_AUX_POLICY_DIVISOR_COUNT as u32,
                encoding: AuxControlEncoding::CmrAuxcV1,
            },
        };
        fs::write(
            root.join(AUX_CONTROL_STATE_FILE),
            serde_json::to_vec(&summary).unwrap(),
        )
        .unwrap();
        let store = AuxControlConstantsStore::open(&root, &reviewed).unwrap();
        (temp, store)
    }

    fn compiled_stage_config(
        slot: i8,
        common_prefix_flags: u8,
        aux_option: u8,
        center_option: u8,
        run_option: u8,
        force_default: bool,
    ) -> Vec<u8> {
        let record_base = slot as usize * COMPILED_CONFIG_RECORD_STRIDE;
        let mut config = vec![0; record_base + COMPILED_CONFIG_CONTROL_RUN_OPTION_OFFSET + 1];
        config[COMPILED_CONFIG_COMMON_FLAGS_OFFSET] = common_prefix_flags;
        config[record_base + COMPILED_CONFIG_AUX_OPTION_OFFSET] = aux_option;
        config[record_base + COMPILED_CONFIG_CENTER_SPLIT_OPTION_OFFSET] = center_option;
        config[record_base + COMPILED_CONFIG_CONTROL_RUN_OPTION_OFFSET] = run_option;
        if force_default {
            config[COMPILED_CONFIG_AUX_FORCE_FLAG_OFFSET] = COMPILED_CONFIG_AUX_FORCE_DEFAULT_MASK;
        }
        config
    }

    fn aux_value(patch: AuxControlPatch, offset: u16) -> i32 {
        let index = AUX_CONTROL_WORD_OFFSETS
            .iter()
            .position(|candidate| *candidate == offset)
            .unwrap();
        patch.words[index]
    }

    fn post_helper_control_value(effects: CompiledPostHelperEffects, offset: u16) -> i32 {
        let index = COMPILED_POST_HELPER_CONTROL_WORD_OFFSETS
            .iter()
            .position(|candidate| *candidate == offset)
            .unwrap();
        effects.control_words[index]
    }

    fn aux_policy_words(patch: AuxControlPatch) -> [i32; 7] {
        AUX_CONTROL_POLICY_WORD_OFFSETS.map(|offset| aux_value(patch, offset))
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
        for offset in COMPILED_CONTROL_PRODUCT_OFFSETS {
            assert!(!COMPILED_CONTROL_WORD_OFFSETS.contains(&offset));
            assert!(!COMPILED_COMMON_PREFIX_CONTROL_OFFSETS.contains(&offset));
            assert!(!AUX_CONTROL_WORD_OFFSETS.contains(&offset));
            assert!(!COMPILED_POST_HELPER_CONTROL_WORD_OFFSETS.contains(&offset));
            assert_ne!(offset, 0x170);
            for family_base in [0x1f0_u16, 0x210, 0x230] {
                for family_index in 0..COMPILED_CONTROL_FAMILY_WORD_COUNT as u16 {
                    assert_ne!(offset, family_base + family_index * 4);
                }
            }
        }
        for offset in COMPILED_SUFFIX_DEFAULT_STATE_OFFSETS {
            assert!(!COMPILED_STATE_WORD_OFFSETS.contains(&offset));
            assert!(!COMPILED_COMMON_PREFIX_STATE_OFFSETS.contains(&offset));
            assert!(!COMPILED_POST_HELPER_STATE_WORD_OFFSETS.contains(&offset));
            assert_ne!(offset, COMPILED_STATE_WORD_758_OFFSET);
        }
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
    fn common_prefix_always_adjusts_control_and_derives_zero_reciprocal() {
        let mut words = synthetic_case(3);
        words.control_words[0] = 0x051e;
        words.control_words[22] = -0x78_0000;
        let original_state = words.state_words;
        let original_control_15c = words.control_words[3];

        let effects = derive_compiled_common_prefix(words, 0).unwrap();

        assert_eq!(effects.control_word_148, 0);
        assert_eq!(effects.control_word_15c, None);
        assert_eq!(effects.state_words, None);
        assert_eq!(words.control_words[3], original_control_15c);
        assert_eq!(words.state_words, original_state);
        assert_eq!(effects.state_word_48, 0);
        assert_eq!(effects.state_word_4c, -1);
    }

    #[test]
    fn common_prefix_control_flag_scales_adjusted_signed_words() {
        let mut words = synthetic_case(4);
        words.control_words[0] = i32::MAX;
        words.control_words[3] = -0x1_0001;
        words.control_words[22] = -0x77_fffc;
        let adjusted = i32::MAX.wrapping_sub(0x051e);

        let effects =
            derive_compiled_common_prefix(words, COMPILED_COMMON_CONTROL_SCALE_MASK).unwrap();

        assert_eq!(
            effects.control_word_148,
            ((i64::from(adjusted) * 0x2_0000_i64) >> 16) as i32
        );
        assert_eq!(effects.control_word_15c, Some(-0x1_3335));
        assert_eq!(effects.state_words, None);
        assert_eq!(effects.state_word_48, 4);
        assert_eq!(effects.state_word_4c, 0x4000_0000);
    }

    #[test]
    fn common_prefix_state_flag_halves_all_eight_words_with_arithmetic_shift() {
        let mut words = synthetic_case(5);
        words.state_words = [-3, -2, -1, 0, 1, 2, 3, i32::MAX];
        words.control_words[22] = -0x77_fffd;

        let effects =
            derive_compiled_common_prefix(words, COMPILED_COMMON_STATE_SCALE_MASK).unwrap();

        assert_eq!(
            effects.state_words,
            Some([-2, -1, -1, 0, 0, 1, 1, 0x3fff_ffff])
        );
        assert_eq!(effects.control_word_15c, None);
        assert_eq!(effects.state_word_48, 3);
        assert_eq!(effects.state_word_4c, 1_431_655_765);
    }

    #[test]
    fn common_prefix_combines_both_masks_and_ignores_other_bits() {
        let mut words = synthetic_case(9);
        words.state_words = [2, 4, 6, 8, 10, 12, 14, 16];
        words.control_words[0] = 0x151e;
        words.control_words[3] = 0x2_0000;
        words.control_words[22] = -0x77_fffc;
        let flags = COMPILED_COMMON_CONTROL_SCALE_MASK | COMPILED_COMMON_STATE_SCALE_MASK;

        let combined = derive_compiled_common_prefix(words, flags).unwrap();
        let with_other_bits = derive_compiled_common_prefix(words, flags | 0xf9).unwrap();

        assert_eq!(combined, with_other_bits);
        assert_eq!(combined.control_word_148, 0x2000);
        assert_eq!(combined.control_word_15c, Some(0x2_6666));
        assert_eq!(combined.state_words, Some([1, 2, 3, 4, 5, 6, 7, 8]));
    }

    #[test]
    fn common_prefix_ignores_unobserved_flag_bits_and_wraps_state_derivation() {
        let mut words = synthetic_case(6);
        words.control_words[22] = i32::MAX;

        let baseline = derive_compiled_common_prefix(words, 0).unwrap();
        let unknown_bits = derive_compiled_common_prefix(words, 0xf9).unwrap();

        assert_eq!(unknown_bits, baseline);
        assert_eq!(baseline.state_word_48, i32::MIN + 0x77_ffff);
    }

    #[test]
    fn common_prefix_reports_every_signed_reciprocal_overflow_boundary() {
        for state_word_48 in [-1_i32, 1, 2] {
            let mut words = synthetic_case(7);
            words.control_words[22] = state_word_48.wrapping_sub(0x78_0000);
            assert_eq!(
                derive_compiled_common_prefix(words, 0),
                Err(CompiledCommonPrefixReciprocalOverflow { state_word_48 })
            );
        }

        let mut words = synthetic_case(8);
        words.control_words[22] = (-2_i32).wrapping_sub(0x78_0000);
        let effects = derive_compiled_common_prefix(words, 0).unwrap();
        assert_eq!(effects.state_word_4c, i32::MIN);
    }

    #[test]
    fn post_helper_patch_pins_exact_mixed_width_schema_and_values() {
        assert_eq!(
            COMPILED_POST_HELPER_CONTROL_WORD_OFFSETS,
            [
                0x144, 0x14c, 0x150, 0x154, 0x160, 0x174, 0x178, 0x17c, 0x180, 0x184, 0x188, 0x18c,
                0x1a8, 0x1b4, 0x408
            ]
        );
        assert_eq!(COMPILED_POST_HELPER_STATE_WORD_OFFSETS, [0x0ec]);
        assert_eq!(COMPILED_POST_HELPER_CONTROL_BYTE_OFFSETS, [0x440]);
        assert_eq!(COMPILED_POST_HELPER_CONTROL_U16_OFFSETS, [0x3e4]);

        let effects = derive_compiled_post_helper_effects(-77, 0x2_0000).unwrap();
        assert_eq!(post_helper_control_value(effects, 0x144), 0x83);
        assert_eq!(post_helper_control_value(effects, 0x14c), -77);
        assert_eq!(post_helper_control_value(effects, 0x150), 0x0ccc);
        assert_eq!(post_helper_control_value(effects, 0x154), 0x0ccc);
        assert_eq!(post_helper_control_value(effects, 0x160), 0x8000);
        for offset in [0x174, 0x178, 0x17c, 0x180] {
            assert_eq!(post_helper_control_value(effects, offset), 0x0ccc);
        }
        assert_eq!(post_helper_control_value(effects, 0x184), 0x8000);
        assert_eq!(post_helper_control_value(effects, 0x188), 0x3333);
        assert_eq!(post_helper_control_value(effects, 0x18c), 0x3333);
        assert_eq!(post_helper_control_value(effects, 0x1a8), 0x0fae1);
        assert_eq!(post_helper_control_value(effects, 0x1b4), 0x8000);
        assert_eq!(post_helper_control_value(effects, 0x408), 0);
        assert_eq!(effects.state_words, [0]);
        assert_eq!(effects.control_bytes, [3]);
        assert_eq!(effects.control_u16, [0x02aa]);
    }

    #[test]
    fn post_helper_reciprocal_pins_zero_signed_truncation_and_boundaries() {
        for (divisor, expected) in [
            (0, -1),
            (0x3_0000, 0x5555),
            (-0x3_0000, -0x5555),
            (-2, i32::MIN),
            (3, 1_431_655_765),
            (i32::MIN, -2),
            (i32::MAX, 2),
        ] {
            let effects = derive_compiled_post_helper_effects(0, divisor).unwrap();
            assert_eq!(post_helper_control_value(effects, 0x160), expected);
        }
    }

    #[test]
    fn post_helper_reciprocal_overflow_is_typed_and_atomic() {
        for control_word_15c in [-1, 1, 2] {
            assert_eq!(
                derive_compiled_post_helper_effects(123, control_word_15c),
                Err(CompiledPostHelperReciprocalOverflow { control_word_15c })
            );
        }
    }

    #[test]
    fn state_758_helper_preserves_every_non_sentinel_and_masks_fallback() {
        for value in [i32::MIN, -2, 0, 1, i32::MAX] {
            assert_eq!(
                derive_compiled_state_word_758(value, 0xff),
                CompiledStateWord758Effect {
                    state_word_758: value
                }
            );
        }
        for fallback in u8::MIN..=u8::MAX {
            assert_eq!(
                derive_compiled_state_word_758(-1, fallback).state_word_758,
                i32::from(fallback & 7)
            );
        }
    }

    #[test]
    fn control_product_pair_pins_offsets_zero_quarter_and_nominal_values() {
        assert_eq!(COMPILED_CONTROL_PRODUCT_OFFSETS, [0x164, 0x168]);
        assert_eq!(
            derive_compiled_control_product_pair(0x1_0000, 0x8000),
            Ok(CompiledControlProductPair {
                reciprocal_word_164: 0x8_0000,
                product_word_168: 0x8000,
            })
        );
        assert_eq!(
            derive_compiled_control_product_pair(1, 0x1_0000),
            Ok(CompiledControlProductPair {
                reciprocal_word_164: -1,
                product_word_168: 1,
            })
        );
        assert_eq!(
            derive_compiled_control_product_pair(3, 0x1_0000),
            Ok(CompiledControlProductPair {
                reciprocal_word_164: -1,
                product_word_168: 3,
            })
        );
        assert_eq!(
            derive_compiled_control_product_pair(-5, 0x1_0000),
            Ok(CompiledControlProductPair {
                reciprocal_word_164: i32::MIN,
                product_word_168: -5,
            })
        );
        assert_eq!(
            derive_compiled_control_product_pair(0x2_0000, 0x4000_0000),
            Ok(CompiledControlProductPair {
                reciprocal_word_164: -8,
                product_word_168: i32::MIN,
            })
        );
    }

    #[test]
    fn control_product_pair_pins_exact_idiv_overflow_and_safe_boundaries() {
        for (product, divisor) in [(-4, -1), (4, 1), (8, 2)] {
            assert_eq!(
                derive_compiled_control_product_pair(product, 0x1_0000),
                Err(CompiledControlProductReciprocalOverflow { divisor })
            );
        }
        assert_eq!(
            derive_compiled_control_product_pair(-8, 0x1_0000),
            Ok(CompiledControlProductPair {
                reciprocal_word_164: i32::MIN,
                product_word_168: -8,
            })
        );
        assert_eq!(
            derive_compiled_control_product_pair(12, 0x1_0000),
            Ok(CompiledControlProductPair {
                reciprocal_word_164: 0x5555_5555,
                product_word_168: 12,
            })
        );
        assert_eq!(
            derive_compiled_control_product_pair(-12, 0x1_0000),
            Ok(CompiledControlProductPair {
                reciprocal_word_164: -0x5555_5555,
                product_word_168: -12,
            })
        );
    }

    #[test]
    fn control_product_pair_keeps_wrapped_bits_16_through_47() {
        for (left, right) in [
            (i32::MIN, i32::MIN),
            (i32::MAX, i32::MAX),
            (i32::MIN, i32::MAX),
            (-0x0345_6789, 0x0765_4321),
        ] {
            let expected_product = ((i64::from(left) * i64::from(right)) >> 16) as i32;
            let expected_divisor = expected_product >> 2;
            let result = derive_compiled_control_product_pair(left, right);
            if [-1, 1, 2].contains(&expected_divisor) {
                assert_eq!(
                    result,
                    Err(CompiledControlProductReciprocalOverflow {
                        divisor: expected_divisor,
                    })
                );
            } else {
                assert_eq!(result.unwrap().product_word_168, expected_product);
            }
        }
    }

    #[test]
    fn suffix_defaults_pin_offsets_norm_anchor_and_raw_seed_values() {
        assert_eq!(
            COMPILED_SUFFIX_DEFAULT_STATE_OFFSETS,
            [0x044, 0x050, 0x054, 0x058]
        );
        assert_eq!(
            derive_compiled_suffix_defaults([0, 0, 0], 0),
            CompiledSuffixDefaultEffects {
                state_word_44: 0,
                state_words_50_54_58: [1, 2, 1],
            }
        );
        assert_eq!(
            derive_compiled_suffix_defaults([0x4_0000, 0, 0], 0).state_word_44,
            131_100
        );
    }

    #[test]
    fn suffix_defaults_exhaustively_use_only_config_mask_04() {
        for config_flags in u8::MIN..=u8::MAX {
            assert_eq!(
                derive_compiled_suffix_defaults([0; 3], config_flags).state_words_50_54_58,
                if config_flags & COMPILED_COMMON_STATE_SCALE_MASK == 0 {
                    [1, 2, 1]
                } else {
                    [2, 4, 2]
                }
            );
        }
    }

    #[test]
    fn suffix_defaults_use_arithmetic_halves_and_wrapping_q16_square_sum() {
        assert_eq!(
            derive_compiled_suffix_defaults([-0x2_0001, 0, 0], 0).state_word_44,
            sqrt_q16(0x1_0002) as i32
        );
        assert_eq!(
            derive_compiled_suffix_defaults([0x0100_0000; 3], 0).state_word_44,
            sqrt_q16(0xc000_0000) as i32
        );
        for words in [
            [i32::MIN, i32::MAX, -3],
            [-1, -5, 7],
            [0x4000_0000, 0x4000_0000, 0x4000_0000],
        ] {
            let halves = words.map(|word| word >> 1);
            let expected_sum = halves.into_iter().fold(0_i32, |sum, word| {
                sum.wrapping_add(((i64::from(word) * i64::from(word)) >> 16) as i32)
            });
            assert_eq!(
                derive_compiled_suffix_defaults(words, 0).state_word_44,
                sqrt_q16(expected_sum as u32) as i32
            );
        }
    }

    #[test]
    fn state_758_caller_selection_exhaustively_pins_gate_and_indices() {
        let table: Vec<i32> = (0..19).map(|index| index + 100).collect();
        assert_eq!(
            select_compiled_state_word_758_argument(false, 0xff, &[]),
            Ok(-1)
        );

        let mut reached = [false; 19];
        for selection_byte in u8::MIN..=u8::MAX {
            let row = usize::from((selection_byte >> 3) & 3);
            let column = usize::from(selection_byte >> 5);
            let gated = row >= 2 && column >= 4;
            let selected =
                select_compiled_state_word_758_argument(true, selection_byte, &table).unwrap();
            if gated {
                assert_eq!(selected, -1);
            } else {
                let index = row * 5 + column;
                reached[index] = true;
                assert_eq!(selected, table[index]);
            }
        }
        assert_eq!(
            reached
                .iter()
                .enumerate()
                .filter_map(|(index, reached)| reached.then_some(index))
                .collect::<Vec<_>>(),
            (0..=13).chain(15..=18).collect::<Vec<_>>()
        );
    }

    #[test]
    fn state_758_selection_checks_only_the_live_table_route() {
        assert_eq!(
            select_compiled_state_word_758_argument(true, 0x00, &[]),
            Err(CompiledStateWord758SelectionError::TableTruncated {
                index: 0,
                table_len: 0,
            })
        );
        assert_eq!(
            select_compiled_state_word_758_argument(true, 0x90, &[]),
            Ok(-1)
        );
        assert_eq!(
            select_compiled_state_word_758_argument(true, 0x78, &[0; 18]),
            Err(CompiledStateWord758SelectionError::TableTruncated {
                index: 18,
                table_len: 18,
            })
        );
        assert_eq!(
            select_compiled_state_word_758_argument(true, 0x78, &[0; 19]),
            Ok(0)
        );
        assert_eq!(COMPILED_STATE_WORD_758_OFFSET, 0x758);
        assert!(!COMPILED_STATE_WORD_OFFSETS.contains(&COMPILED_STATE_WORD_758_OFFSET));
        assert!(!COMPILED_COMMON_PREFIX_STATE_OFFSETS.contains(&COMPILED_STATE_WORD_758_OFFSET));
        assert!(!COMPILED_POST_HELPER_STATE_WORD_OFFSETS.contains(&COMPILED_STATE_WORD_758_OFFSET));
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
    fn config_option_resolver_rejects_the_signed_negative_domain_first() {
        for index in [-1, i8::MIN] {
            assert_eq!(
                resolve_compiled_config_options(&[], index),
                Err(CompiledConfigResolveError::NegativeIndex { index })
            );
        }
    }

    #[test]
    fn config_option_resolver_requires_only_the_highest_observed_read() {
        let required_len = COMPILED_CONFIG_CONTROL_RUN_OPTION_OFFSET + 1;
        for actual_len in [0, required_len - 1] {
            assert_eq!(
                resolve_compiled_config_options(&vec![0; actual_len], 0),
                Err(CompiledConfigResolveError::TruncatedConfig {
                    index: 0,
                    required_len,
                    actual_len,
                })
            );
        }

        assert_eq!(
            resolve_compiled_config_options(&vec![0; required_len], 0),
            Ok(CompiledConfigOptions {
                common_prefix_flags: 0,
                state_word_758_fallback: 0,
                center_split: 0,
                control_run: 0,
                aux_control: AuxControlOption::Resolved(0),
            })
        );
        assert!(resolve_compiled_config_options(&vec![0; required_len + 7], 0).is_ok());
    }

    #[test]
    fn config_option_resolver_selects_one_record_and_preserves_raw_bytes() {
        let record_base = COMPILED_CONFIG_RECORD_STRIDE;
        let required_len = record_base + COMPILED_CONFIG_CONTROL_RUN_OPTION_OFFSET + 1;
        let mut config = vec![0; required_len];
        config[COMPILED_CONFIG_AUX_OPTION_OFFSET] = 0x11;
        config[COMPILED_CONFIG_CENTER_SPLIT_OPTION_OFFSET] = 0x22;
        config[COMPILED_CONFIG_CONTROL_RUN_OPTION_OFFSET] = 0x33;
        config[COMPILED_CONFIG_STATE_758_OPTION_OFFSET] = 0x44;
        config[record_base + COMPILED_CONFIG_STATE_758_OPTION_OFFSET] = 0x7b;
        config[record_base + COMPILED_CONFIG_AUX_OPTION_OFFSET] = 0xe5;
        config[record_base + COMPILED_CONFIG_CENTER_SPLIT_OPTION_OFFSET] = 0xfa;
        config[record_base + COMPILED_CONFIG_CONTROL_RUN_OPTION_OFFSET] = 0xf9;
        let original = config.clone();

        assert_eq!(
            resolve_compiled_config_options(&config, 1),
            Ok(CompiledConfigOptions {
                common_prefix_flags: 0,
                state_word_758_fallback: 0x7b,
                center_split: 0xfa,
                control_run: 0xf9,
                aux_control: AuxControlOption::Resolved(0xe5),
            })
        );
        assert_eq!(config, original);

        assert_eq!(
            resolve_compiled_config_options(&config[..required_len - 1], 1),
            Err(CompiledConfigResolveError::TruncatedConfig {
                index: 1,
                required_len,
                actual_len: required_len - 1,
            })
        );
    }

    #[test]
    fn config_option_resolver_pins_the_constructed_slot_boundary() {
        for index in [COMPILED_CONFIG_SLOT_COUNT, i8::MAX] {
            assert_eq!(
                resolve_compiled_config_options(&[], index),
                Err(CompiledConfigResolveError::IndexOutOfRange {
                    index,
                    slot_count: COMPILED_CONFIG_SLOT_COUNT,
                })
            );
        }

        let index = COMPILED_CONFIG_SLOT_COUNT - 1;
        let record_base = index as usize * COMPILED_CONFIG_RECORD_STRIDE;
        let required_len = record_base + COMPILED_CONFIG_CONTROL_RUN_OPTION_OFFSET + 1;
        assert_eq!(
            resolve_compiled_config_options(&vec![0; required_len - 1], index),
            Err(CompiledConfigResolveError::TruncatedConfig {
                index,
                required_len,
                actual_len: required_len - 1,
            })
        );

        let mut config = vec![0; required_len];
        config[record_base + COMPILED_CONFIG_AUX_OPTION_OFFSET] = 0xa3;
        config[record_base + COMPILED_CONFIG_CENTER_SPLIT_OPTION_OFFSET] = 0xa5;
        config[record_base + COMPILED_CONFIG_CONTROL_RUN_OPTION_OFFSET] = 0xa9;
        assert_eq!(
            resolve_compiled_config_options(&config, index),
            Ok(CompiledConfigOptions {
                common_prefix_flags: 0,
                state_word_758_fallback: 0,
                center_split: 0xa5,
                control_run: 0xa9,
                aux_control: AuxControlOption::Resolved(0xa3),
            })
        );
    }

    #[test]
    fn config_option_resolver_uses_only_the_unindexed_force_default_flag() {
        let record_base = COMPILED_CONFIG_RECORD_STRIDE;
        let required_len = record_base + COMPILED_CONFIG_CONTROL_RUN_OPTION_OFFSET + 1;
        let mut config = vec![0; required_len];
        config[record_base + COMPILED_CONFIG_AUX_OPTION_OFFSET] = 0xa7;
        config[record_base + COMPILED_CONFIG_AUX_FORCE_FLAG_OFFSET] =
            COMPILED_CONFIG_AUX_FORCE_DEFAULT_MASK;

        assert_eq!(
            resolve_compiled_config_options(&config, 1)
                .unwrap()
                .aux_control,
            AuxControlOption::Resolved(0xa7)
        );

        config[COMPILED_CONFIG_AUX_FORCE_FLAG_OFFSET] = 0xfd;
        assert_eq!(
            resolve_compiled_config_options(&config, 1)
                .unwrap()
                .aux_control,
            AuxControlOption::Resolved(0xa7)
        );

        config[COMPILED_CONFIG_AUX_FORCE_FLAG_OFFSET] = COMPILED_CONFIG_AUX_FORCE_DEFAULT_MASK;
        assert_eq!(
            resolve_compiled_config_options(&config, 1)
                .unwrap()
                .aux_control,
            AuxControlOption::ForcedDefault
        );
    }

    #[test]
    fn config_option_resolver_preserves_unindexed_common_prefix_flags() {
        let mut config = compiled_stage_config(1, 0xa6, 1, 2, 3, false);
        config[COMPILED_CONFIG_COMMON_FLAGS_OFFSET + COMPILED_CONFIG_RECORD_STRIDE] = 0x19;

        assert_eq!(
            resolve_compiled_config_options(&config, 1)
                .unwrap()
                .common_prefix_flags,
            0xa6
        );
    }

    #[test]
    fn recovered_stage_bundle_routes_selected_case_options_and_tail_in_order() {
        let cases = synthetic_cases();
        let direct_cases = CompiledDirectCaseSet { cases };
        let constants = aux_constants();
        let aux_constants = AuxControlConstantSet { constants };
        let config = compiled_stage_config(1, 0, 1, 1, 0, false);
        let prior = synthetic_case(-7);

        let effects = compose_recovered_compiled_stage_effects(
            &direct_cases,
            &aux_constants,
            RecoveredCompiledStageInputs {
                prior_case_words: prior,
                selector: 3,
                config: &config,
                state_slot: 1,
                global_word: 0x1_0000,
                state_758_context_nonzero: false,
                state_758_table: &[],
            },
        )
        .unwrap();

        let selected = CompiledCaseOutcome {
            words: cases[3],
            selected_case: Some(3),
        };
        let transformed = transform_compiled_control_family(cases[3].control_family(), 0);
        let prefix = derive_compiled_common_prefix(cases[3], 0).unwrap();
        assert_eq!(effects.direct_case_stores, selected);
        assert_eq!(effects.common_prefix, prefix);
        assert_eq!(
            effects.post_helper,
            derive_compiled_post_helper_effects(
                prefix.control_word_148,
                cases[3].control_words[3],
            )
            .unwrap()
        );
        assert_ne!(
            effects.direct_case_stores.words.control_family(),
            transformed
        );
        assert_eq!(effects.center_split_q16, 0x8000);
        assert_eq!(
            effects.suffix_defaults,
            derive_compiled_suffix_defaults(
                [
                    cases[3].state_words[0],
                    cases[3].state_words[1],
                    cases[3].state_words[2],
                ],
                0,
            )
        );
        assert_eq!(
            effects.aux_control_patch,
            derive_aux_control_patch(
                AuxControlOption::Resolved(1),
                prefix.state_word_48,
                0x1_0000,
                &constants,
            )
            .unwrap()
        );
        assert_eq!(
            effects.control_families,
            derive_compiled_control_families(transformed).unwrap()
        );
    }

    #[test]
    fn recovered_stage_bundle_pins_separate_destination_relationships() {
        let family_offsets = &COMPILED_CONTROL_WORD_OFFSETS
            [CONTROL_FAMILY_START..CONTROL_FAMILY_START + COMPILED_CONTROL_FAMILY_WORD_COUNT];
        assert_eq!(
            family_offsets,
            &[0x1d0, 0x1d4, 0x1d8, 0x1dc, 0x1e0, 0x1e4, 0x1e8, 0x1ec]
        );
        assert!(!COMPILED_CONTROL_WORD_OFFSETS.contains(&0x170));
        assert!(!AUX_CONTROL_WORD_OFFSETS.contains(&0x170));
        assert_eq!(COMPILED_COMMON_PREFIX_CONTROL_OFFSETS, [0x148, 0x15c]);
        assert_eq!(
            COMPILED_COMMON_PREFIX_STATE_OFFSETS,
            [
                0x020, 0x024, 0x028, 0x638, 0x63c, 0x640, 0x648, 0x644, 0x048, 0x04c
            ]
        );
        for offset in COMPILED_COMMON_PREFIX_CONTROL_OFFSETS {
            assert_ne!(offset, 0x170);
            assert!(!family_offsets.contains(&offset));
            assert!(!AUX_CONTROL_WORD_OFFSETS.contains(&offset));
        }
        for offset in COMPILED_POST_HELPER_CONTROL_WORD_OFFSETS {
            assert_ne!(offset, 0x170);
            assert!(!COMPILED_CONTROL_WORD_OFFSETS.contains(&offset));
            assert!(!AUX_CONTROL_WORD_OFFSETS.contains(&offset));
            assert!(!family_offsets.contains(&offset));
            assert!(!COMPILED_COMMON_PREFIX_CONTROL_OFFSETS.contains(&offset));
        }
        for offset in family_offsets {
            assert!(COMPILED_CONTROL_WORD_OFFSETS.contains(offset));
            assert!(!AUX_CONTROL_WORD_OFFSETS.contains(offset));
        }
    }

    #[test]
    fn recovered_stage_bundle_runs_common_effects_for_invalid_selectors() {
        let direct_cases = CompiledDirectCaseSet {
            cases: synthetic_cases(),
        };
        let constants = aux_constants();
        let aux_constants = AuxControlConstantSet { constants };
        let config = compiled_stage_config(0, COMPILED_COMMON_STATE_SCALE_MASK, 2, 0, 0, true);
        let prior = synthetic_case(40);
        let transformed = transform_compiled_control_family(prior.control_family(), 0);
        assert_ne!(transformed, prior.control_family());

        for selector in [14, u32::MAX] {
            let effects = compose_recovered_compiled_stage_effects(
                &direct_cases,
                &aux_constants,
                RecoveredCompiledStageInputs {
                    prior_case_words: prior,
                    selector,
                    config: &config,
                    state_slot: 0,
                    global_word: 0x8000,
                    state_758_context_nonzero: false,
                    state_758_table: &[],
                },
            )
            .unwrap();

            assert_eq!(
                effects.direct_case_stores,
                CompiledCaseOutcome {
                    words: prior,
                    selected_case: None,
                }
            );
            assert_eq!(effects.center_split_q16, 0x4000);
            assert_eq!(
                effects.common_prefix,
                derive_compiled_common_prefix(prior, COMPILED_COMMON_STATE_SCALE_MASK).unwrap()
            );
            assert_eq!(effects.control_families.post_processed_raw, transformed);
            assert_eq!(
                effects.post_helper,
                derive_compiled_post_helper_effects(
                    effects.common_prefix.control_word_148,
                    prior.control_words[3],
                )
                .unwrap()
            );
            assert_eq!(
                effects.aux_control_patch,
                derive_aux_control_patch(
                    AuxControlOption::ForcedDefault,
                    effects.common_prefix.state_word_48,
                    0x8000,
                    &constants,
                )
                .unwrap()
            );
            assert_eq!(
                effects.control_product_pair,
                derive_compiled_control_product_pair(
                    prior.control_words[3],
                    effects.control_families.reciprocal[6],
                )
                .unwrap()
            );
            assert_eq!(
                effects.suffix_defaults,
                derive_compiled_suffix_defaults(
                    [
                        effects.common_prefix.state_words.unwrap()[0],
                        effects.common_prefix.state_words.unwrap()[1],
                        effects.common_prefix.state_words.unwrap()[2],
                    ],
                    COMPILED_COMMON_STATE_SCALE_MASK,
                )
            );
        }
    }

    #[test]
    fn recovered_stage_bundle_feeds_scaled_prefix_words_to_post_helper() {
        let mut cases = synthetic_cases();
        cases[0].control_words[0] = 0x151e;
        cases[0].control_words[3] = 0x2_0000;
        cases[0].control_words[CONTROL_FAMILY_START + 6] = 0x2_0000;
        let direct_cases = CompiledDirectCaseSet { cases };
        let aux_constants = AuxControlConstantSet {
            constants: aux_constants(),
        };
        let config = compiled_stage_config(
            0,
            COMPILED_COMMON_CONTROL_SCALE_MASK | COMPILED_COMMON_STATE_SCALE_MASK,
            0,
            0,
            2,
            false,
        );

        let effects = compose_recovered_compiled_stage_effects(
            &direct_cases,
            &aux_constants,
            RecoveredCompiledStageInputs {
                prior_case_words: synthetic_case(-1),
                selector: 0,
                config: &config,
                state_slot: 0,
                global_word: 0,
                state_758_context_nonzero: false,
                state_758_table: &[],
            },
        )
        .unwrap();

        assert_eq!(effects.common_prefix.control_word_148, 0x2000);
        assert_eq!(effects.common_prefix.control_word_15c, Some(0x2_6666));
        assert_eq!(
            post_helper_control_value(effects.post_helper, 0x14c),
            0x2000
        );
        assert_eq!(
            effects.post_helper,
            derive_compiled_post_helper_effects(0x2000, 0x2_6666).unwrap()
        );
        assert_eq!(effects.control_families.reciprocal[6], 0x8000);
        assert_eq!(
            effects.control_product_pair,
            derive_compiled_control_product_pair(0x2_6666, 0x8000).unwrap()
        );
        assert_eq!(
            effects.suffix_defaults,
            derive_compiled_suffix_defaults(
                [
                    effects.common_prefix.state_words.unwrap()[0],
                    effects.common_prefix.state_words.unwrap()[1],
                    effects.common_prefix.state_words.unwrap()[2],
                ],
                COMPILED_COMMON_CONTROL_SCALE_MASK | COMPILED_COMMON_STATE_SCALE_MASK,
            )
        );
    }

    #[test]
    fn recovered_stage_bundle_reports_product_pair_after_family() {
        let mut cases = synthetic_cases();
        cases[0].control_words[3] = 0x1_0000;
        cases[0].control_words[CONTROL_FAMILY_START + 6] = 0;
        let direct_cases = CompiledDirectCaseSet { cases };
        let aux_constants = AuxControlConstantSet {
            constants: aux_constants(),
        };
        let config = compiled_stage_config(0, 0, 0, 0, 2, false);

        assert_eq!(
            compose_recovered_compiled_stage_effects(
                &direct_cases,
                &aux_constants,
                RecoveredCompiledStageInputs {
                    prior_case_words: synthetic_case(-1),
                    selector: 0,
                    config: &config,
                    state_slot: 0,
                    global_word: 0,
                    state_758_context_nonzero: false,
                    state_758_table: &[],
                },
            ),
            Err(RecoveredCompiledStageError::ProductPair(
                CompiledControlProductReciprocalOverflow { divisor: -1 }
            ))
        );

        cases[0].control_words[CONTROL_FAMILY_START + 1] = 1;
        let family_fails_first = CompiledDirectCaseSet { cases };
        assert!(matches!(
            compose_recovered_compiled_stage_effects(
                &family_fails_first,
                &aux_constants,
                RecoveredCompiledStageInputs {
                    prior_case_words: synthetic_case(-1),
                    selector: 0,
                    config: &config,
                    state_slot: 0,
                    global_word: 0,
                    state_758_context_nonzero: false,
                    state_758_table: &[],
                },
            ),
            Err(RecoveredCompiledStageError::Family(_))
        ));
    }

    #[test]
    fn recovered_stage_bundle_routes_state_758_table_and_fallback_values() {
        let direct_cases = CompiledDirectCaseSet {
            cases: synthetic_cases(),
        };
        let aux_constants = AuxControlConstantSet {
            constants: aux_constants(),
        };
        let mut config = compiled_stage_config(1, 0, 0, 0, 2, false);
        config[COMPILED_CONFIG_RECORD_STRIDE + COMPILED_CONFIG_STATE_758_OPTION_OFFSET] = 0xfe;
        config.resize(
            COMPILED_CONFIG_STATE_758_SELECTION_OFFSET + COMPILED_CONFIG_RECORD_STRIDE + 1,
            0,
        );
        config[COMPILED_CONFIG_STATE_758_SELECTION_OFFSET] = 0;
        config[COMPILED_CONFIG_STATE_758_SELECTION_OFFSET + COMPILED_CONFIG_RECORD_STRIDE] = 0x90;
        let prior = synthetic_case(-3);

        for (selector, table_value, expected) in [(0, 77, 77), (u32::MAX, -1, 6)] {
            let table = [table_value];
            let effects = compose_recovered_compiled_stage_effects(
                &direct_cases,
                &aux_constants,
                RecoveredCompiledStageInputs {
                    prior_case_words: prior,
                    selector,
                    config: &config,
                    state_slot: 1,
                    global_word: 0,
                    state_758_context_nonzero: true,
                    state_758_table: &table,
                },
            )
            .unwrap();
            assert_eq!(effects.state_word_758.state_word_758, expected);
            let selected_words = if selector == 0 {
                direct_cases.cases()[0]
            } else {
                prior
            };
            assert_eq!(
                effects.suffix_defaults,
                derive_compiled_suffix_defaults(
                    [
                        selected_words.state_words[0],
                        selected_words.state_words[1],
                        selected_words.state_words[2],
                    ],
                    0,
                )
            );
        }
    }

    #[test]
    fn recovered_stage_bundle_reports_config_before_auxiliary_failure() {
        let mut cases = synthetic_cases();
        cases[0].control_words[22] = 1_i32.wrapping_sub(0x78_0000);
        let direct_cases = CompiledDirectCaseSet { cases };
        let mut constants = aux_constants();
        constants.policy_one.reciprocal_divisor = 1;
        let aux_constants = AuxControlConstantSet { constants };
        let prior = synthetic_case(9);

        for (config, state_slot, expected) in [
            (
                vec![],
                -1,
                CompiledConfigResolveError::NegativeIndex { index: -1 },
            ),
            (
                vec![],
                COMPILED_CONFIG_SLOT_COUNT,
                CompiledConfigResolveError::IndexOutOfRange {
                    index: COMPILED_CONFIG_SLOT_COUNT,
                    slot_count: COMPILED_CONFIG_SLOT_COUNT,
                },
            ),
            (
                vec![0; COMPILED_CONFIG_CONTROL_RUN_OPTION_OFFSET],
                0,
                CompiledConfigResolveError::TruncatedConfig {
                    index: 0,
                    required_len: COMPILED_CONFIG_CONTROL_RUN_OPTION_OFFSET + 1,
                    actual_len: COMPILED_CONFIG_CONTROL_RUN_OPTION_OFFSET,
                },
            ),
        ] {
            assert_eq!(
                compose_recovered_compiled_stage_effects(
                    &direct_cases,
                    &aux_constants,
                    RecoveredCompiledStageInputs {
                        prior_case_words: prior,
                        selector: 0,
                        config: &config,
                        state_slot,
                        global_word: 0,
                        state_758_context_nonzero: false,
                        state_758_table: &[],
                    },
                ),
                Err(RecoveredCompiledStageError::Config(expected))
            );
        }

        let config = compiled_stage_config(0, 0, 1, 0, 2, false);
        assert_eq!(
            compose_recovered_compiled_stage_effects(
                &direct_cases,
                &aux_constants,
                RecoveredCompiledStageInputs {
                    prior_case_words: prior,
                    selector: 1,
                    config: &config,
                    state_slot: 0,
                    global_word: 0,
                    state_758_context_nonzero: false,
                    state_758_table: &[],
                },
            ),
            Err(RecoveredCompiledStageError::Aux(
                AuxControlReciprocalOverflow {
                    input: AuxControlReciprocalInput::PolicyDivisor,
                    divisor: 1,
                }
            ))
        );
    }

    #[test]
    fn recovered_stage_bundle_reports_prefix_before_auxiliary_and_family_failure() {
        let mut cases = synthetic_cases();
        cases[0].control_words[22] = 1_i32.wrapping_sub(0x78_0000);
        cases[0].control_words[CONTROL_FAMILY_START + 1] = 1;
        let direct_cases = CompiledDirectCaseSet { cases };
        let mut constants = aux_constants();
        constants.policy_one.reciprocal_divisor = 1;
        let aux_constants = AuxControlConstantSet { constants };
        let config = compiled_stage_config(0, 0, 1, 0, 2, false);

        assert_eq!(
            compose_recovered_compiled_stage_effects(
                &direct_cases,
                &aux_constants,
                RecoveredCompiledStageInputs {
                    prior_case_words: synthetic_case(-1),
                    selector: 0,
                    config: &config,
                    state_slot: 0,
                    global_word: 0,
                    state_758_context_nonzero: false,
                    state_758_table: &[],
                },
            ),
            Err(RecoveredCompiledStageError::Prefix(
                CompiledCommonPrefixReciprocalOverflow { state_word_48: 1 }
            ))
        );
    }

    #[test]
    fn recovered_stage_bundle_orders_aux_post_helper_and_family_failures() {
        let mut cases = synthetic_cases();
        cases[0].control_words[3] = 1;
        cases[0].control_words[CONTROL_FAMILY_START + 1] = 1;
        let direct_cases = CompiledDirectCaseSet { cases };
        let config = compiled_stage_config(0, 0, 1, 0, 2, false);
        let input = RecoveredCompiledStageInputs {
            prior_case_words: synthetic_case(-1),
            selector: 0,
            config: &config,
            state_slot: 0,
            global_word: 0,
            state_758_context_nonzero: false,
            state_758_table: &[],
        };

        let mut failing_aux_constants = aux_constants();
        failing_aux_constants.policy_one.reciprocal_divisor = 1;
        let selection_input = RecoveredCompiledStageInputs {
            state_758_context_nonzero: true,
            ..input
        };
        assert_eq!(
            compose_recovered_compiled_stage_effects(
                &direct_cases,
                &AuxControlConstantSet {
                    constants: failing_aux_constants,
                },
                selection_input,
            ),
            Err(RecoveredCompiledStageError::Aux(
                AuxControlReciprocalOverflow {
                    input: AuxControlReciprocalInput::PolicyDivisor,
                    divisor: 1,
                }
            ))
        );

        assert_eq!(
            compose_recovered_compiled_stage_effects(
                &direct_cases,
                &AuxControlConstantSet {
                    constants: aux_constants(),
                },
                selection_input,
            ),
            Err(RecoveredCompiledStageError::State758Selection(
                CompiledStateWord758SelectionError::TruncatedSelectionByte {
                    required_len: COMPILED_CONFIG_STATE_758_SELECTION_OFFSET + 1,
                    actual_len: config.len(),
                }
            ))
        );

        assert_eq!(
            compose_recovered_compiled_stage_effects(
                &direct_cases,
                &AuxControlConstantSet {
                    constants: aux_constants(),
                },
                input,
            ),
            Err(RecoveredCompiledStageError::PostHelper(
                CompiledPostHelperReciprocalOverflow {
                    control_word_15c: 1,
                }
            ))
        );
    }

    #[test]
    fn recovered_stage_bundle_reports_auxiliary_before_family_overflow() {
        let mut cases = synthetic_cases();
        cases[0].control_words[CONTROL_FAMILY_START + 1] = 1;
        let direct_cases = CompiledDirectCaseSet { cases };
        let config = compiled_stage_config(0, 0, 1, 0, 2, false);
        let input = RecoveredCompiledStageInputs {
            prior_case_words: synthetic_case(-2),
            selector: 0,
            config: &config,
            state_slot: 0,
            global_word: 0,
            state_758_context_nonzero: false,
            state_758_table: &[],
        };

        let safe_aux = AuxControlConstantSet {
            constants: aux_constants(),
        };
        assert_eq!(
            compose_recovered_compiled_stage_effects(&direct_cases, &safe_aux, input),
            Err(RecoveredCompiledStageError::Family(
                CompiledReciprocalOverflow { slot: 1, raw: 1 }
            ))
        );

        let mut overflowing_constants = aux_constants();
        overflowing_constants.policy_one.reciprocal_divisor = 1;
        let overflowing_aux = AuxControlConstantSet {
            constants: overflowing_constants,
        };
        assert_eq!(
            compose_recovered_compiled_stage_effects(&direct_cases, &overflowing_aux, input),
            Err(RecoveredCompiledStageError::Aux(
                AuxControlReciprocalOverflow {
                    input: AuxControlReciprocalInput::PolicyDivisor,
                    divisor: 1,
                }
            ))
        );
    }

    #[test]
    fn aux_cross_layer_regression_pins_offset_and_codec_invariants() {
        assert_eq!(AUX_CONTROL_WORD_OFFSETS, FORMAT_AUX_WORD_OFFSETS);
        assert_eq!(
            AUX_CONTROL_COMMON_WORD_OFFSETS,
            FORMAT_AUX_COMMON_WORD_OFFSETS
        );
        assert_eq!(
            AUX_CONTROL_POLICY_WORD_OFFSETS,
            FORMAT_AUX_POLICY_WORD_OFFSETS
        );
        assert_eq!(FORMAT_AUX_FORMAT_VERSION, 1);
        assert_eq!(FORMAT_AUX_COMMON_WORD_COUNT, 9);
        assert_eq!(FORMAT_AUX_MULTIPLIER_COUNT, 2);
        assert_eq!(FORMAT_AUX_POLICY_COUNT, 3);
        assert_eq!(FORMAT_AUX_POLICY_WORD_COUNT, 7);
        assert_eq!(FORMAT_AUX_POLICY_DIVISOR_COUNT, 2);
        assert_eq!(FORMAT_AUX_TOTAL_WORD_COUNT, 38);
        assert_eq!(FORMAT_AUX_ENCODED_LEN, 184);
        assert_eq!(verify_aux_control_format_schema(), Ok(()));
    }

    #[test]
    fn verified_aux_store_becomes_owned_and_derives_each_policy_after_drop() {
        let format = format_aux_constants();
        let expected = core_aux_constants(&format);
        let (temp, owned) = {
            let (temp, store) = open_synthetic_aux_store(&format);
            let owned = AuxControlConstantSet::try_from(&store).unwrap();
            (temp, owned)
        };
        drop(temp);

        assert_eq!(owned.constants(), &expected);
        assert_eq!(owned.constants().common_words[0], i32::MIN);
        assert_eq!(owned.constants().common_words[1], i32::MAX);
        assert_eq!(owned.constants().common_words[2], 0);
        assert_eq!(owned.constants().common_words[3], -1);
        assert_eq!(
            owned.constants().policy_one.words[..4],
            [i32::MAX, i32::MIN, 0, -1]
        );

        for option in [
            AuxControlOption::Resolved(1),
            AuxControlOption::Resolved(2),
            AuxControlOption::Resolved(0),
            AuxControlOption::Resolved(7),
            AuxControlOption::ForcedDefault,
        ] {
            assert_eq!(
                owned.derive(option, -9, 0x1_0000),
                derive_aux_control_patch(option, -9, 0x1_0000, &expected)
            );
        }
    }

    #[test]
    fn verified_aux_store_preserves_typed_overflow_through_owned_adapter() {
        let mut format = format_aux_constants();
        format.policy_one.reciprocal_divisor = 1;
        let (temp, owned) = {
            let (temp, store) = open_synthetic_aux_store(&format);
            let owned = AuxControlConstantSet::from_store(&store).unwrap();
            (temp, owned)
        };
        drop(temp);

        assert_eq!(
            owned.derive(AuxControlOption::Resolved(1), 0, 0),
            Err(AuxControlReciprocalOverflow {
                input: AuxControlReciprocalInput::PolicyDivisor,
                divisor: 1,
            })
        );
    }

    #[test]
    fn auxiliary_patch_pins_policy_partition_masking_and_forced_default() {
        let constants = aux_constants();
        for option in [1_u8, 9, 0xf9] {
            let patch =
                derive_aux_control_patch(AuxControlOption::Resolved(option), 0, 0, &constants)
                    .unwrap();
            assert_eq!(aux_policy_words(patch), constants.policy_one.words);
        }
        for option in [2_u8, 10, 0xfa] {
            let patch =
                derive_aux_control_patch(AuxControlOption::Resolved(option), 0, 0, &constants)
                    .unwrap();
            assert_eq!(aux_policy_words(patch), constants.policy_two.words);
        }
        for option in [0_u8, 3, 4, 5, 6, 7, 8, 0xfb, 0xff] {
            let patch =
                derive_aux_control_patch(AuxControlOption::Resolved(option), 0, 0, &constants)
                    .unwrap();
            assert_eq!(aux_policy_words(patch), constants.default_policy.words);
        }

        let forced =
            derive_aux_control_patch(AuxControlOption::ForcedDefault, 0, 0, &constants).unwrap();
        assert_eq!(aux_policy_words(forced), constants.default_policy.words);
    }

    #[test]
    fn auxiliary_patch_maps_every_literal_and_derived_word_to_its_exact_offset() {
        let patch = derive_aux_control_patch(
            AuxControlOption::Resolved(1),
            80,
            0x1_0000,
            &aux_constants(),
        )
        .unwrap();
        assert_eq!(
            patch.words,
            [
                10, 11, 12, 13, 14, 15, 16, 0x1_0000, 101, 17, 0x2_0000, 0x1_0000, 0x1_0000, 102,
                103, 10, 0x1_0000, 104, 105, 106, 0x8000, 107,
            ]
        );
    }

    #[test]
    fn auxiliary_patch_schema_is_exact_unique_and_disjoint_from_direct_cases() {
        assert_eq!(
            AUX_CONTROL_WORD_OFFSETS,
            [
                0x0d8, 0x0dc, 0x0e0, 0x0e4, 0x0e8, 0x0ec, 0x0f0, 0x0f4, 0x0f8, 0x0fc, 0x100, 0x104,
                0x108, 0x10c, 0x110, 0x118, 0x11c, 0x120, 0x124, 0x128, 0x12c, 0x13c,
            ]
        );
        assert!(
            AUX_CONTROL_WORD_OFFSETS
                .windows(2)
                .all(|pair| pair[0] < pair[1])
        );
        for omitted in [0x114, 0x130, 0x134, 0x138] {
            assert!(!AUX_CONTROL_WORD_OFFSETS.contains(&omitted));
        }
        for direct_offset in COMPILED_CONTROL_WORD_OFFSETS {
            assert!(!AUX_CONTROL_WORD_OFFSETS.contains(&direct_offset));
        }
    }

    #[test]
    fn auxiliary_patch_uses_signed_q16_bits_and_arithmetic_shift() {
        let mut constants = aux_constants();
        constants.q16_multipliers = [0x1_8000, -1];
        let positive =
            derive_aux_control_patch(AuxControlOption::Resolved(1), -9, 0x2_0000, &constants)
                .unwrap();
        assert_eq!(aux_value(positive, 0x0f4), 0x3_0000);
        assert_eq!(aux_value(positive, 0x100), -2);
        assert_eq!(aux_value(positive, 0x118), -2);

        let negative_odd =
            derive_aux_control_patch(AuxControlOption::Resolved(1), 0, 1, &constants).unwrap();
        assert_eq!(aux_value(negative_odd, 0x100), -1);

        constants.q16_multipliers[0] = i32::MAX;
        let low_32 =
            derive_aux_control_patch(AuxControlOption::Resolved(1), 0, i32::MIN, &constants)
                .unwrap();
        assert_eq!(aux_value(low_32, 0x0f4), 32_768);
    }

    #[test]
    fn auxiliary_patch_reciprocals_pin_zero_and_signed_truncation() {
        let mut constants = aux_constants();
        constants.common_words[8] = 0;
        constants.policy_one.reciprocal_divisor = 0x3_0000;
        constants.policy_one.scaled_reciprocal_divisor = -0x3_0000;
        let patch =
            derive_aux_control_patch(AuxControlOption::Resolved(1), 0, 0x1_0000, &constants)
                .unwrap();
        assert_eq!(aux_value(patch, 0x108), -1);
        assert_eq!(aux_value(patch, 0x11c), 0x5555);
        assert_eq!(aux_value(patch, 0x12c), -0x5555);
    }

    #[test]
    fn auxiliary_patch_reciprocal_overflow_is_typed_and_atomic() {
        let mut constants = aux_constants();
        for (input, divisor) in [
            (AuxControlReciprocalInput::CommonWord, 1),
            (AuxControlReciprocalInput::PolicyDivisor, 2),
            (AuxControlReciprocalInput::ScaledPolicyDivisor, -1),
        ] {
            constants.common_words[8] = 0x1_0000;
            constants.policy_one.reciprocal_divisor = 0x1_0000;
            constants.policy_one.scaled_reciprocal_divisor = 0x1_0000;
            match input {
                AuxControlReciprocalInput::CommonWord => constants.common_words[8] = divisor,
                AuxControlReciprocalInput::PolicyDivisor => {
                    constants.policy_one.reciprocal_divisor = divisor;
                }
                AuxControlReciprocalInput::ScaledPolicyDivisor => {
                    constants.policy_one.scaled_reciprocal_divisor = divisor;
                }
            }
            assert_eq!(
                derive_aux_control_patch(AuxControlOption::Resolved(1), 0, 0, &constants,),
                Err(AuxControlReciprocalOverflow { input, divisor })
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
