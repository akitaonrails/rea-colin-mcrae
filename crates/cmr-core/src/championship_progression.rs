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

/// Bounds-checked byte range of a championship record in the persisted
/// `cmRally.cfg` image. Both record tables share this settings allocation;
/// there is no separate record file.
#[must_use]
pub fn championship_record_range(country: usize, stage: usize) -> Option<std::ops::Range<usize>> {
    if country >= crate::championship_model::COUNTRY_COUNT || stage >= STAGES_PER_RECORD_COUNTRY {
        return None;
    }
    let index = country
        .checked_mul(STAGES_PER_RECORD_COUNTRY)?
        .checked_add(stage)?;
    settings_record_range(index, CHAMPIONSHIP_RECORD_BASE)
}

/// Bounds-checked byte range of an alternate-mode record in `cmRally.cfg`.
#[must_use]
pub fn alt_record_range(stage: usize) -> Option<std::ops::Range<usize>> {
    settings_record_range(stage, ALT_RECORD_BASE)
}

fn settings_record_range(index: usize, base: usize) -> Option<std::ops::Range<usize>> {
    let start = index.checked_mul(RECORD_STRIDE)?.checked_add(base)?;
    let end = start.checked_add(RECORD_STRIDE)?;
    (end <= crate::settings_store::SETTINGS_BLOB_LEN).then_some(start..end)
}

/// Return the forced rival car for a country in the four-entry native table,
/// or `None` when the country has no forced entry (`FUN_0044da00`).
#[must_use]
pub fn forced_country_car(country: u8, entries: &[(u8, u8); 4]) -> Option<u8> {
    entries
        .iter()
        .find_map(|&(entry_country, car)| (entry_country == country).then_some(car))
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

/// Result of the ordinary post-stage commit in `FUN_00436f00`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StageCommit {
    /// Updated `+0x167`: the low six-bit progress counter advances even when
    /// the event is complete; the upper two bits are preserved.
    pub progress: u8,
    /// Updated `+0x168`: the stage nibble advances and the country is retained.
    pub cursor: u8,
    /// The ordinary three-stage event has no following stage.
    pub event_complete: bool,
}

/// Commit one finished stage in the ordinary (`+0x166 & 3 == 0`) event path.
///
/// The native writer does not restore either counter when the event completes.
/// Country selection is a separate decision made by `FUN_00437090`.
#[must_use]
pub fn commit_ordinary_stage(progress: u8, cursor: u8) -> StageCommit {
    let progress = (progress & 0xc0) | (progress.wrapping_add(1) & 0x3f);
    let cursor = (cursor & 0xf0).wrapping_add(0x10) | (cursor & 0x0f);
    StageCommit {
        progress,
        cursor,
        event_complete: cursor & 0xf0 >= 0x30,
    }
}

/// Commit the sub-mode-1 stage path (`FUN_00436ff0`), including the parent's
/// progress-counter increment. `stage_count` is the low nibble of `+0x165`;
/// `difficulty_flags` is live `+0x18`. With [`MODE_SUBFLAG`] clear, the limit is
/// three stages when the difficulty bits are zero and `stage_count` otherwise.
/// With it set, the advanced progress counter selects a forced country and any
/// value above three completes the event without changing the cursor.
#[must_use]
pub fn commit_submode_one_stage(
    progress: u8,
    cursor: u8,
    mode: u8,
    difficulty_flags: u8,
    stage_count: u8,
    forced_countries: &[u8; 4],
) -> StageCommit {
    let progress = (progress & 0xc0) | (progress.wrapping_add(1) & 0x3f);
    if mode & MODE_SUBFLAG != 0 {
        let index = usize::from(progress & 0x3f);
        if index > 3 {
            return StageCommit {
                progress,
                cursor,
                event_complete: true,
            };
        }
        return StageCommit {
            progress,
            cursor: (cursor & 0xf0) | (forced_countries[index] & 0x0f),
            event_complete: false,
        };
    }
    let cursor = (cursor & 0xf0).wrapping_add(0x10) | (cursor & 0x0f);
    let variable_limit = difficulty_flags & 0x18 != 0;
    let event_complete = if variable_limit {
        stage_index(cursor) >= stage_count
    } else {
        cursor & 0xf0 >= 0x30
    };
    StageCommit {
        progress,
        cursor,
        event_complete,
    }
}

/// Inputs to the sparse custom-stage scheduler `FUN_00436e20`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CustomStageState {
    /// Live `+0x167`, before the parent's progress increment.
    pub progress: u8,
    /// Live `+0x168` country/stage cursor.
    pub cursor: u8,
    /// Live `+0x166` mode flags.
    pub mode: u8,
    /// Enabled-stage mask from `+0x164`.
    pub stage_mask: u16,
    /// Number of stages from the low nibble of `+0x165`.
    pub stage_count: u8,
}

/// Advance through an enabled custom-stage mask, counting only enabled stages
/// against the incremented progress counter. `two_player` represents the native
/// `DAT_00d3ca94` reset branch. Exhaustion reports completion without a cursor
/// write; an out-of-table forced-country request is rejected rather than
/// reading beyond the supplied four-entry table.
#[must_use]
pub fn commit_custom_stage(
    state: CustomStageState,
    two_player: bool,
    forced_countries: &[u8; 4],
) -> StageCommit {
    let mut result = StageCommit {
        progress: (state.progress & 0xc0) | (state.progress.wrapping_add(1) & 0x3f),
        cursor: state.cursor,
        event_complete: true,
    };
    let mut enabled_index = 0u8;
    for stage in 0..state.stage_count.min(16) {
        if state.stage_mask & (1u16 << stage) == 0 {
            continue;
        }
        if two_player {
            result.progress &= 0xc0;
            result.cursor = (result.cursor & 0x0f) | (stage << 4);
            result.event_complete = false;
            break;
        }
        if enabled_index == result.progress & 0x3f {
            if state.mode & MODE_SUBFLAG != 0 {
                if state.mode & CHAMPIONSHIP_TYPE_FLAG == 0 {
                    let Some(&country) = forced_countries.get(usize::from(stage)) else {
                        break;
                    };
                    result.cursor = 0x60 | (country & 0x0f);
                    result.event_complete = false;
                    break;
                }
                result.cursor = (result.cursor & 0xf8) | 8;
            }
            result.cursor = (result.cursor & 0x0f) | (stage << 4);
            result.event_complete = false;
            break;
        }
        enabled_index += 1;
    }
    result
}

/// Advance to the next country at its first stage (`FUN_00437090`).
///
/// `can_continue` is the native country-range and external-availability gate.
/// A rejected transition leaves the cursor untouched.
#[must_use]
pub fn advance_country(cursor: u8, can_continue: bool) -> Option<u8> {
    can_continue.then(|| (cursor & 0x0f).wrapping_add(1) & 0x0f)
}

/// Stage-count value written to the low nibble of `+0x165` after a country
/// advance: six normally, seven when the next country is in the native
/// four-entry special-country table (`FUN_00485b80`).
#[must_use]
pub fn next_country_stage_count(next_country: u8, special_countries: &[u8; 4]) -> u8 {
    if special_countries.contains(&next_country) {
        7
    } else {
        6
    }
}

/// Clear every cumulative-time slot, as done on a country advance.
pub fn clear_cumulative_times(times: &mut [u32; RESULT_SLOT_COUNT]) {
    times.fill(0);
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
    fn record_ranges_stay_inside_the_persisted_settings_image() {
        let last = championship_record_range(COUNTRY_COUNT - 1, 6).unwrap();
        assert_eq!(last.end, ALT_RECORD_BASE);
        assert!(championship_record_range(COUNTRY_COUNT, 0).is_none());
        assert!(alt_record_range(56).is_some());
        assert!(alt_record_range(57).is_none());

        let mut raw = vec![0; crate::settings_store::SETTINGS_BLOB_LEN];
        raw[last.clone()].fill(0x5a);
        let image = crate::settings_store::SettingsImage::from_file_bytes(&raw).unwrap();
        assert!(image.as_bytes()[last].iter().all(|&byte| byte == 0x5a));
    }

    #[test]
    fn forced_country_car_requires_a_matching_table_entry() {
        let entries = [(1, 9), (3, 10), (6, 11), (8, 12)];
        assert_eq!(forced_country_car(3, &entries), Some(10));
        assert_eq!(forced_country_car(4, &entries), None);
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
    fn ordinary_stage_commit_preserves_country_and_completion_cursor() {
        let next = commit_ordinary_stage(0x80, 0x13);
        assert_eq!(next.progress, 0x81);
        assert_eq!(next.cursor, 0x23);
        assert!(!next.event_complete);

        let complete = commit_ordinary_stage(0xbf, 0x23);
        assert_eq!(complete.progress, 0x80);
        assert_eq!(complete.cursor, 0x33);
        assert!(complete.event_complete);
    }

    #[test]
    fn country_advance_rejects_or_restarts_and_clears_times() {
        assert_eq!(advance_country(0x23, false), None);
        assert_eq!(advance_country(0x23, true), Some(0x04));
        assert_eq!(next_country_stage_count(3, &[1, 3, 6, 8]), 7);
        assert_eq!(next_country_stage_count(4, &[1, 3, 6, 8]), 6);

        let mut times = [7; RESULT_SLOT_COUNT];
        clear_cumulative_times(&mut times);
        assert_eq!(times, [0; RESULT_SLOT_COUNT]);
    }

    #[test]
    fn submode_one_commit_honors_limits_and_forced_countries() {
        let next = commit_submode_one_stage(0, 0x13, 0, 0, 6, &[1, 3, 6, 8]);
        assert_eq!(
            (next.progress, next.cursor, next.event_complete),
            (1, 0x23, false)
        );
        let complete = commit_submode_one_stage(0, 0x13, 0, 8, 2, &[1, 3, 6, 8]);
        assert_eq!((complete.cursor, complete.event_complete), (0x23, true));

        let forced = commit_submode_one_stage(0, 0x13, MODE_SUBFLAG, 0, 6, &[1, 3, 6, 8]);
        assert_eq!((forced.progress, forced.cursor), (1, 0x13));
        let exhausted = commit_submode_one_stage(3, 0x13, MODE_SUBFLAG, 0, 6, &[1, 3, 6, 8]);
        assert_eq!((exhausted.progress, exhausted.cursor), (4, 0x13));
        assert!(exhausted.event_complete);
    }

    #[test]
    fn custom_commit_selects_sparse_stages_and_resets_two_player_progress() {
        let state = CustomStageState {
            progress: 0,
            cursor: 0x23,
            mode: 0,
            stage_mask: 0b1010,
            stage_count: 4,
        };
        let next = commit_custom_stage(state, false, &[1, 3, 6, 8]);
        assert_eq!(
            (next.progress, next.cursor, next.event_complete),
            (1, 0x33, false)
        );
        let two_player = commit_custom_stage(state, true, &[1, 3, 6, 8]);
        assert_eq!((two_player.progress, two_player.cursor), (0, 0x13));
        let exhausted = commit_custom_stage(
            CustomStageState {
                stage_mask: 0,
                ..state
            },
            false,
            &[1, 3, 6, 8],
        );
        assert_eq!((exhausted.progress, exhausted.cursor), (1, 0x23));
        assert!(exhausted.event_complete);
    }

    #[test]
    fn custom_commit_applies_special_country_rules_safely() {
        let state = CustomStageState {
            progress: 0,
            cursor: 0x23,
            mode: MODE_SUBFLAG,
            stage_mask: 0b101,
            stage_count: 5,
        };
        let special = commit_custom_stage(state, false, &[1, 3, 6, 8]);
        assert_eq!((special.cursor, special.event_complete), (0x66, false));

        let invalid = commit_custom_stage(
            CustomStageState {
                stage_mask: 0b1_0001,
                ..state
            },
            false,
            &[1, 3, 6, 8],
        );
        assert_eq!((invalid.cursor, invalid.event_complete), (0x23, true));
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
