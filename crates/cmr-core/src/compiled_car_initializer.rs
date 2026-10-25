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

/// Number of compiled selector bodies accepted by the unsigned dispatch.
pub const COMPILED_CASE_COUNT: usize = 14;

/// Number of direct state-word stores in each compiled body.
pub const COMPILED_STATE_WORD_COUNT: usize = 8;

/// Number of direct control-word stores in each compiled body.
pub const COMPILED_CONTROL_WORD_COUNT: usize = 23;

/// Number of words in the later primary/reciprocal control families.
pub const COMPILED_CONTROL_FAMILY_WORD_COUNT: usize = 8;

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
