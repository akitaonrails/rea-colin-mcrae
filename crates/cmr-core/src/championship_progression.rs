//! Championship progression, standings, and records structure (item F06).
//!
//! # Scope
//!
//! This reconstructs the *behavior structure* layered over the [F05 data model]
//! ([`crate::championship_model`]): the per-car-slot results table, the
//! standings-ranking formulas, the best-time record addressing, and the
//! mode/flag decode. It does **not** vendor content: opponent AI time tables,
//! record times, points values (there are none - see below), stage names, and
//! the per-stage `.TIM`/pace resources are game data and are modeled only by
//! their addressing (offset, stride, element shape).
//!
//! # Recovered facts
//!
//! - **Standings are by elapsed time, not points.** There is decisively no
//!   points/scoring table in the executable (the `points.dat` string has zero
//!   code references). Two parallel rankings coexist: within-event ranking by
//!   cumulative stage time (`FUN_00430590`, live `+0x16c + slot*0x10`), and
//!   overall ranking by accumulated per-stage finishing positions
//!   (`FUN_004304d0`, live `+0x170 + slot*0x10`).
//! - The live championship holds a 16-slot, `0x10`-stride results table keyed by
//!   car-type slot. Each slot carries a cumulative-time dword, a run of per-stage
//!   finishing-position bytes, and a flag byte (bit `0x10` = "entered this
//!   stage's field", low nibble = provisional order / finishing position). This
//!   is the same array [`crate::championship_model`] addresses as event progress
//!   at live `+0x170`.
//! - Best/record times live in the mutable game-data image (`PTR_DAT_0050622c`),
//!   addressed by `(country * 7 + stage) * 0x1e` for championship records and by
//!   `stage * 0x1e` for the alternate mode, each off a fixed base. Records are
//!   **not** part of the `0x690` save payload and have no standalone file.
//! - Mode/flags: `+0x166` packs the game-mode enum (`& 0x1c`), a championship
//!   -type flag (`0x40`), a sub-flag (`0x20`), and a sub-mode (`& 3`, `== 2`
//!   selects the record-time path). `+0x168` packs country (`& 0xf`) and stage
//!   (`>> 4`). Difficulty is `+0x18 (>> 3) & 3` (see
//!   [`crate::championship_model::mode_tier`]).
//!
//! See evidence `rea.official-eu.championship-progression`.
//!
//! [F05 data model]: crate::championship_model

/// Number of car-type slots in the standings/results table.
pub const RESULT_SLOT_COUNT: usize = 16;
/// Stride of one slot in the standings/results table.
pub const RESULT_SLOT_STRIDE: usize = 0x10;

/// Live-struct offset of the cumulative-time dword of slot 0
/// (`FUN_00430590`: `+0x16c + slot * 0x10`).
pub const SLOT_TIME_OFFSET: usize = 0x16c;
/// Live-struct offset of the per-stage finishing-position byte run of slot 0
/// (`FUN_004304d0`: `+0x170 + slot * 0x10`).
pub const SLOT_POSITIONS_OFFSET: usize = 0x170;
/// Live-struct offset of the state-flag byte of slot 0
/// (`FUN_00466350`: `+0x178 + slot * 0x10`).
pub const SLOT_FLAG_OFFSET: usize = 0x178;

/// Flag bit meaning the car entered the current stage's field.
pub const SLOT_FLAG_ENTERED: u8 = 0x10;
/// Low-nibble mask of the provisional order / finishing position.
pub const SLOT_ORDER_MASK: u8 = 0x0f;

/// Live-struct offset of the cumulative-time dword for `slot`.
pub fn slot_time_offset(slot: usize) -> usize {
    SLOT_TIME_OFFSET + slot * RESULT_SLOT_STRIDE
}

/// Live-struct offset of the per-stage finishing-position run for `slot`.
pub fn slot_positions_offset(slot: usize) -> usize {
    SLOT_POSITIONS_OFFSET + slot * RESULT_SLOT_STRIDE
}

/// Live-struct offset of the state-flag byte for `slot`.
pub fn slot_flag_offset(slot: usize) -> usize {
    SLOT_FLAG_OFFSET + slot * RESULT_SLOT_STRIDE
}

// --- Slot flag transitions (FUN_00466350) -------------------------------------

/// Stage-load flag reset: replace the low nibble with the slot index and clear
/// the "entered" bit, preserving the upper flag bits
/// (`flag = ((flag ^ i) & 0xf) ^ flag; flag &= 0xef`).
pub fn reset_slot_flag(prev: u8, slot_index: u8) -> u8 {
    (prev & 0xe0) | (slot_index & SLOT_ORDER_MASK)
}

/// Field-assignment: mark the slot entered for `entrant`, setting the low nibble
/// to the entrant index and the "entered" bit, preserving the upper nibble.
pub fn enter_slot_flag(prev: u8, entrant: u8) -> u8 {
    (prev & 0xf0) | SLOT_FLAG_ENTERED | (entrant & SLOT_ORDER_MASK)
}

/// Whether a slot's flag marks it as entered in the current stage.
pub fn slot_entered(flag: u8) -> bool {
    flag & SLOT_FLAG_ENTERED != 0
}

/// The provisional order / finishing position encoded in a slot flag.
pub fn slot_order(flag: u8) -> u8 {
    flag & SLOT_ORDER_MASK
}

// --- Standings ranking --------------------------------------------------------

/// 1-based rank of `slot` by ascending cumulative time, reproducing
/// `FUN_00430590` (lowest time ranks first). Ties break by slot index, matching
/// the stable sort the original performs before locating the slot.
pub fn rank_by_time(times: &[u32], slot: usize) -> Option<usize> {
    rank_ascending(times, slot)
}

/// The inverse of [`rank_by_time`]: the slot at 1-based `rank` by time
/// (`FUN_004306f0`).
pub fn slot_at_time_rank(times: &[u32], rank: usize) -> Option<usize> {
    slot_at_rank(times, rank)
}

/// Weight applied to the accumulated per-stage position sum before the
/// tiebreak term is added (`FUN_004304d0`).
pub const CHAMPIONSHIP_POSITION_WEIGHT: u32 = 10;

/// The overall championship score of one slot: the sum of its per-stage
/// finishing positions times [`CHAMPIONSHIP_POSITION_WEIGHT`] plus a tiebreak
/// term, reproducing the key `FUN_004304d0` sorts by (lower is better).
pub fn championship_score(stage_positions: &[u8], tiebreak: u8) -> u32 {
    let sum: u32 = stage_positions.iter().map(|&p| u32::from(p)).sum();
    sum * CHAMPIONSHIP_POSITION_WEIGHT + u32::from(tiebreak)
}

/// 1-based rank of `slot` by ascending championship score (`FUN_004304d0`).
pub fn rank_by_championship_score(scores: &[u32], slot: usize) -> Option<usize> {
    rank_ascending(scores, slot)
}

/// The inverse of [`rank_by_championship_score`] (`FUN_004306c0`).
pub fn slot_at_championship_rank(scores: &[u32], rank: usize) -> Option<usize> {
    slot_at_rank(scores, rank)
}

/// 1-based rank of `index` among `keys` by ascending value, breaking ties by
/// original index (stable).
fn rank_ascending(keys: &[u32], index: usize) -> Option<usize> {
    let target = *keys.get(index)?;
    let rank = keys
        .iter()
        .enumerate()
        .filter(|&(i, &key)| key < target || (key == target && i < index))
        .count();
    Some(rank + 1)
}

/// The index occupying 1-based `rank` under [`rank_ascending`]'s ordering.
fn slot_at_rank(keys: &[u32], rank: usize) -> Option<usize> {
    if rank == 0 || rank > keys.len() {
        return None;
    }
    let mut order: Vec<usize> = (0..keys.len()).collect();
    order.sort_by_key(|&i| (keys[i], i));
    order.get(rank - 1).copied()
}

// --- Best-time records (PTR_DAT_0050622c image) -------------------------------

/// Stride of one best-time record (`0x1e` bytes, ~7 split shorts + a total).
pub const RECORD_STRIDE: usize = 0x1e;
/// Number of record-bearing stages per country in the championship table.
pub const STAGES_PER_RECORD_COUNTRY: usize = 7;
/// Base offset of the championship best-time record table in the data image.
pub const CHAMPIONSHIP_RECORD_BASE: usize = 0xffc;
/// Base offset of the alternate-mode best-time record table.
pub const ALT_RECORD_BASE: usize = 0x175e;

/// Offset (into the `PTR_DAT_0050622c` data image) of the championship best-time
/// record for `(country, stage)` (`FUN_004ba4b0`).
pub fn championship_record_offset(country: usize, stage: usize) -> usize {
    (country * STAGES_PER_RECORD_COUNTRY + stage) * RECORD_STRIDE + CHAMPIONSHIP_RECORD_BASE
}

/// Offset of the alternate-mode best-time record for `stage`.
pub fn alt_record_offset(stage: usize) -> usize {
    stage * RECORD_STRIDE + ALT_RECORD_BASE
}

// --- Mode / flag decode -------------------------------------------------------

/// Mask selecting the game-mode enum within the `+0x166` flag byte.
pub const MODE_ENUM_MASK: u8 = 0x1c;
/// Flag bit selecting the championship variant within `+0x166`.
pub const CHAMPIONSHIP_TYPE_FLAG: u8 = 0x40;
/// Sub-flag bit within `+0x166`.
pub const MODE_SUBFLAG: u8 = 0x20;
/// Sub-mode mask within `+0x166`; value `2` selects the record-time path.
pub const MODE_SUBMODE_MASK: u8 = 0x03;
/// Sub-mode value selecting the record/best-time read path.
pub const SUBMODE_RECORD: u8 = 0x02;

/// The country index packed in the `+0x168` byte (`& 0xf`, `0..=8`).
pub fn country_index(byte_168: u8) -> u8 {
    byte_168 & 0x0f
}

/// The current stage index within the event packed in `+0x168` (`>> 4`).
pub fn stage_index(byte_168: u8) -> u8 {
    byte_168 >> 4
}

/// The game-mode enum nibble within `+0x166` (`& 0x1c`).
pub fn game_mode(byte_166: u8) -> u8 {
    byte_166 & MODE_ENUM_MASK
}

/// Whether the sub-mode selects the record/best-time path (`& 3 == 2`).
pub fn uses_record_path(byte_166: u8) -> bool {
    byte_166 & MODE_SUBMODE_MASK == SUBMODE_RECORD
}

/// The championship predicate (`FUN_00485bb0`): with the championship-type flag
/// set, any high nibble of `+0x168` except `0x20` is a championship; otherwise a
/// championship requires either the sub-flag set or a `0x60` high nibble.
pub fn is_championship(byte_166: u8, byte_168: u8) -> bool {
    let high = byte_168 & 0xf0;
    if byte_166 & CHAMPIONSHIP_TYPE_FLAG != 0 {
        return high != 0x20;
    }
    byte_166 & MODE_SUBFLAG != 0 || high == 0x60
}

/// The two-player head-to-head predicate (`FUN_00401030`): game-mode `8` in a
/// championship.
pub fn is_two_player_head_to_head(byte_166: u8, byte_168: u8) -> bool {
    game_mode(byte_166) == 0x08 && is_championship(byte_166, byte_168)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::championship_model::COUNTRY_COUNT;

    #[test]
    fn slot_offsets_follow_the_recovered_stride() {
        assert_eq!(slot_time_offset(0), 0x16c);
        assert_eq!(slot_positions_offset(0), 0x170);
        assert_eq!(slot_flag_offset(0), 0x178);
        // Slot 3 is three strides on.
        assert_eq!(slot_time_offset(3), 0x16c + 3 * 0x10);
        assert_eq!(slot_flag_offset(RESULT_SLOT_COUNT - 1), 0x178 + 15 * 0x10);
    }

    #[test]
    fn slot_flag_reset_sets_low_nibble_and_clears_entered() {
        // Upper bits preserved, entered bit cleared, low nibble := index.
        let reset = reset_slot_flag(0xff, 5);
        assert_eq!(slot_order(reset), 5);
        assert!(!slot_entered(reset));
        assert_eq!(reset & 0xe0, 0xe0); // bits 7..5 preserved
    }

    #[test]
    fn slot_flag_enter_marks_entered_with_entrant_order() {
        let entered = enter_slot_flag(0x00, 3);
        assert!(slot_entered(entered));
        assert_eq!(slot_order(entered), 3);
        // Entering preserves the upper nibble of the previous flag.
        let entered = enter_slot_flag(0xa0, 2);
        assert_eq!(entered & 0xf0, 0xb0); // 0xa0 | 0x10
        assert_eq!(slot_order(entered), 2);
    }

    #[test]
    fn rank_by_time_is_ascending_with_stable_ties() {
        let times = [300u32, 100, 200, 100];
        // Fastest (100) at index 1 ranks first; the tie at index 3 ranks after it.
        assert_eq!(rank_by_time(&times, 1), Some(1));
        assert_eq!(rank_by_time(&times, 3), Some(2));
        assert_eq!(rank_by_time(&times, 2), Some(3));
        assert_eq!(rank_by_time(&times, 0), Some(4));
        // Inverse round-trips.
        assert_eq!(slot_at_time_rank(&times, 1), Some(1));
        assert_eq!(slot_at_time_rank(&times, 2), Some(3));
        assert_eq!(slot_at_time_rank(&times, 4), Some(0));
        assert_eq!(slot_at_time_rank(&times, 5), None);
        assert_eq!(rank_by_time(&times, 9), None);
    }

    #[test]
    fn championship_score_weights_positions_then_adds_tiebreak() {
        // positions 1,2,3 -> (1+2+3)*10 + tiebreak 4 = 64.
        assert_eq!(championship_score(&[1, 2, 3], 4), 64);
        assert_eq!(championship_score(&[], 0), 0);
        // Lower score ranks first.
        let scores = [64u32, 30, 64];
        assert_eq!(rank_by_championship_score(&scores, 1), Some(1));
        assert_eq!(rank_by_championship_score(&scores, 0), Some(2));
        assert_eq!(rank_by_championship_score(&scores, 2), Some(3));
        assert_eq!(slot_at_championship_rank(&scores, 1), Some(1));
    }

    #[test]
    fn record_offsets_match_the_recovered_formulas() {
        // Championship: (country*7 + stage)*0x1e + 0xffc.
        assert_eq!(championship_record_offset(0, 0), 0xffc);
        assert_eq!(championship_record_offset(0, 1), 0x1e + 0xffc);
        assert_eq!(championship_record_offset(2, 3), (2 * 7 + 3) * 0x1e + 0xffc);
        // Alternate mode: stage*0x1e + 0x175e.
        assert_eq!(alt_record_offset(0), 0x175e);
        assert_eq!(alt_record_offset(4), 4 * 0x1e + 0x175e);
        // Record tables address all countries.
        assert!(championship_record_offset(COUNTRY_COUNT - 1, 6) > CHAMPIONSHIP_RECORD_BASE);
    }

    #[test]
    fn mode_and_country_decode() {
        // +0x168: low nibble country, high nibble stage.
        assert_eq!(country_index(0x53), 3);
        assert_eq!(stage_index(0x53), 5);
        // +0x166 sub-mode == 2 selects the record path.
        assert!(uses_record_path(0x02));
        assert!(!uses_record_path(0x01));
        assert_eq!(game_mode(0xff), 0x1c);
    }

    #[test]
    fn championship_predicate_branches() {
        // Type flag set: championship unless high nibble is 0x20.
        assert!(is_championship(CHAMPIONSHIP_TYPE_FLAG, 0x10));
        assert!(!is_championship(CHAMPIONSHIP_TYPE_FLAG, 0x20));
        // Type flag clear: needs the sub-flag or a 0x60 high nibble.
        assert!(is_championship(MODE_SUBFLAG, 0x00));
        assert!(is_championship(0x00, 0x60));
        assert!(!is_championship(0x00, 0x10));
        // Two-player head-to-head: game-mode 8 in a championship.
        assert!(is_two_player_head_to_head(0x08 | MODE_SUBFLAG, 0x00));
        assert!(!is_two_player_head_to_head(0x04 | MODE_SUBFLAG, 0x00));
    }
}
