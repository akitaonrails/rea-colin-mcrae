//! The standings `FUN_0042fab0` computes once at the finish, for the results
//! pages: the stage order by stage time, the rally order by cumulative time
//! (adding the stage time into each result slot), the rally points on the
//! rally's last stage (10, 6, 4, 3, 2, 1 for the first six), and the
//! championship order by `points * 100 + rally wins`.
//!
//! Every order is a selection: sixteen passes, each taking the strictly
//! best unmarked slot, so ties go to the lower slot. Times compare unsigned
//! (`FUN_0047fe20`), smallest first from `0xffffffff`; scores compare signed,
//! largest first from `-1`. A pass that finds nothing better keeps the
//! previous pass's slot, as the native does. A description of disassembly,
//! not original source. Evidence: `rea.official-eu.standings`.

use crate::event_flow::EventState;

/// The time a disqualified player and a novice's excluded slots carry.
pub const EXCLUDED_TIME: u32 = 0xffff_fffe;

/// The rally points by position (jump table `0x4300e8`).
pub const RALLY_POINTS: [u8; 6] = [10, 6, 4, 3, 2, 1];

/// One stage's results going into the standings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StageFinish {
    /// `DAT_0057af00`: every slot's stage time from the stage load.
    pub rival_times: [u32; 16],
    /// `DAT_0057af94[p]`, `DAT_005ffbf0[p]`: each player's slot and stage time
    /// (`DAT_004ed7dc` players).
    pub players: Vec<(u8, u32)>,
    /// `DAT_0054fc38`: the super-special opponent's slot.
    pub opponent_slot: u8,
    /// `DAT_00550014`.
    pub disqualified: bool,
    /// `FUN_00401030() && DAT_00551ad8 == 0`: a head-to-head still under way,
    /// which skips the standings.
    pub head_to_head_pending: bool,
}

/// One placed slot of an order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placed {
    pub slot: u8,
    /// The stage or cumulative time, or the championship score.
    pub value: u32,
}

/// The three orders and each player's position in them (1-based).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Standings {
    /// `DAT_0054fdcc`, positions `DAT_0054fc6c/70`.
    pub stage: [Placed; 16],
    pub stage_positions: Vec<usize>,
    /// `DAT_0054fe58`, positions `DAT_0054fb38/3c`.
    pub rally: [Placed; 16],
    pub rally_positions: Vec<usize>,
    /// `DAT_0054fee1`, positions `DAT_0054fc54/58`; the value is the score.
    pub championship: [Placed; 16],
    pub championship_positions: Vec<usize>,
}

fn select_smallest(values: &[u32; 16]) -> [Placed; 16] {
    let mut marked = [false; 16];
    let mut slot = 0usize;
    std::array::from_fn(|_| {
        let mut best = u32::MAX;
        for (candidate, &value) in values.iter().enumerate() {
            if !marked[candidate] && value < best {
                best = value;
                slot = candidate;
            }
        }
        marked[slot] = true;
        Placed {
            slot: slot as u8,
            value: values[slot],
        }
    })
}

fn select_largest(scores: &[i32; 16]) -> [Placed; 16] {
    let mut marked = [false; 16];
    let mut slot = 0usize;
    std::array::from_fn(|_| {
        let mut best = -1;
        for (candidate, &score) in scores.iter().enumerate() {
            if !marked[candidate] && score > best {
                best = score;
                slot = candidate;
            }
        }
        marked[slot] = true;
        Placed {
            slot: slot as u8,
            value: scores[slot] as u32,
        }
    })
}

fn positions(order: &[Placed; 16], players: &[(u8, u32)]) -> Vec<usize> {
    players
        .iter()
        .map(|&(slot, _)| {
            order
                .iter()
                .position(|placed| placed.slot == slot)
                .map_or(0, |index| index + 1)
        })
        .collect()
}

/// Whether the event's current stage is its rally's last (`FUN_0042fab0`'s
/// points test): the last of the configured stages with a difficulty set,
/// stage 3 or later without.
pub fn last_stage_of_rally(event: &EventState) -> bool {
    let stage = event.cfg.cursor_168 >> 4;
    if event.cfg.difficulty_018 & 0x18 != 0 {
        i32::from(stage) >= i32::from(event.cfg.stage_count_165 & 0xf) - 1
    } else {
        event.cfg.cursor_168 & 0xf0 >= 0x20
    }
}

/// `FUN_0042fab0`: `None` for a time trial (which shows its record instead)
/// and for a head-to-head still under way.
pub fn compute_standings(event: &mut EventState, finish: &StageFinish) -> Option<Standings> {
    if finish.head_to_head_pending || event.cfg.mode_166 & 3 == 2 {
        return None;
    }
    let mut stage_times = finish.rival_times;
    for &(slot, time) in &finish.players {
        stage_times[usize::from(slot & 0xf)] = time;
    }
    if finish.disqualified
        && let Some(&(slot, _)) = finish.players.first()
    {
        stage_times[usize::from(slot & 0xf)] = EXCLUDED_TIME;
        stage_times[usize::from(finish.opponent_slot & 0xf)] = EXCLUDED_TIME;
    }
    let novice = event.cfg.difficulty_018 & 0x18 == 0;
    for (index, slot) in event.cfg.results_16c.iter_mut().enumerate() {
        let cumulative = u32::from_le_bytes([slot[0], slot[1], slot[2], slot[3]]);
        let cumulative = if novice && index < 8 {
            EXCLUDED_TIME
        } else {
            cumulative.wrapping_add(stage_times[index])
        };
        slot[..4].copy_from_slice(&cumulative.to_le_bytes());
    }
    let stage = select_smallest(&stage_times);
    let cumulative: [u32; 16] = std::array::from_fn(|index| {
        let slot = &event.cfg.results_16c[index];
        u32::from_le_bytes([slot[0], slot[1], slot[2], slot[3]])
    });
    let rally = select_smallest(&cumulative);
    let country = usize::from(event.cfg.cursor_168 & 0xf);
    if last_stage_of_rally(event) {
        for (rank, placed) in rally.iter().enumerate() {
            let points = RALLY_POINTS.get(rank).copied().unwrap_or(0);
            if let Some(byte) = event.cfg.results_16c[usize::from(placed.slot)].get_mut(4 + country)
            {
                *byte = points;
            }
        }
    }
    let scores: [i32; 16] = std::array::from_fn(|index| {
        let slot = &mut event.cfg.results_16c[index];
        let mut total = 0i32;
        let mut wins = 0i32;
        for country_index in 0..=country {
            let points = slot.get(4 + country_index).copied().unwrap_or(0);
            total += i32::from(points);
            if points == 10 {
                wins += 1;
            }
        }
        if (slot[0xd] as i8) < 0 {
            wins = -1;
        } else {
            slot[0xd] = wins as u8;
        }
        total * 100 + wins
    });
    let championship = select_largest(&scores);
    Some(Standings {
        stage_positions: positions(&stage, &finish.players),
        rally_positions: positions(&rally, &finish.players),
        championship_positions: positions(&championship, &finish.players),
        stage,
        rally,
        championship,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(mode: u8, cursor: u8, difficulty: u8) -> EventState {
        let mut event = EventState::default();
        event.cfg.mode_166 = mode;
        event.cfg.cursor_168 = cursor;
        event.cfg.difficulty_018 = difficulty;
        event.cfg.stage_count_165 = 6;
        event
    }

    fn finish(player_time: u32) -> StageFinish {
        StageFinish {
            rival_times: std::array::from_fn(|slot| 10_000 + slot as u32 * 100),
            players: vec![(3, player_time)],
            opponent_slot: 0,
            disqualified: false,
            head_to_head_pending: false,
        }
    }

    fn cumulative(event: &EventState, slot: usize) -> u32 {
        let bytes = &event.cfg.results_16c[slot];
        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
    }

    #[test]
    fn the_stage_and_rally_orders_take_the_fastest_first() {
        let mut e = event(0, 0x00, 0x08);
        let standings = compute_standings(&mut e, &finish(9_000)).unwrap();
        assert_eq!(
            standings.stage[0],
            Placed {
                slot: 3,
                value: 9_000
            }
        );
        assert_eq!(standings.stage[1].slot, 0);
        assert_eq!(standings.stage_positions, vec![1]);
        assert_eq!(cumulative(&e, 3), 9_000);
        assert_eq!(cumulative(&e, 5), 10_500);
        // Not the rally's last stage: no points yet.
        assert!(e.cfg.results_16c.iter().all(|slot| slot[4] == 0));
        // The second stage adds to the cumulative times.
        let mut second = finish(12_000);
        second.rival_times = [10_000; 16];
        e.cfg.cursor_168 = 0x10;
        let standings = compute_standings(&mut e, &second).unwrap();
        assert_eq!(cumulative(&e, 3), 21_000);
        assert_eq!(standings.rally[0].slot, 0, "20,000 beats 21,000");
        // Slots 0..2 and 4..9 are ahead; slot 10 ties at 21,000 and loses.
        assert_eq!(standings.rally_positions, vec![10]);
        assert_eq!(standings.rally[10].slot, 10);
    }

    #[test]
    fn ties_go_to_the_lower_slot() {
        let mut e = event(0, 0x00, 0x08);
        let mut tie = finish(10_000);
        tie.rival_times = [10_000; 16];
        let standings = compute_standings(&mut e, &tie).unwrap();
        let slots: Vec<u8> = standings.stage.iter().map(|placed| placed.slot).collect();
        assert_eq!(slots, (0..16).collect::<Vec<u8>>());
    }

    #[test]
    fn the_last_stage_scores_the_rally_and_the_championship() {
        let mut e = event(0, 0x51, 0x08); // country 1, stage 5 of 6
        assert!(last_stage_of_rally(&e));
        e.cfg.results_16c[0][4] = 10; // country 0 won by slot 0
        let standings = compute_standings(&mut e, &finish(5_000)).unwrap();
        // Slot 3 wins the rally: 10 points in country 1.
        assert_eq!(e.cfg.results_16c[3][4 + 1], 10);
        assert_eq!(e.cfg.results_16c[0][4 + 1], 6);
        assert_eq!(e.cfg.results_16c[1][4 + 1], 4);
        assert_eq!(e.cfg.results_16c[6][4 + 1], 0, "seventh scores nothing");
        // Slot 0: 10 + 6 = 16 points and 1 win -> 1601; slot 3: 1001.
        assert_eq!(
            standings.championship[0],
            Placed {
                slot: 0,
                value: 1601
            }
        );
        assert_eq!(
            standings.championship[1],
            Placed {
                slot: 3,
                value: 1001
            }
        );
        assert_eq!(e.cfg.results_16c[0][0xd], 1);
        assert_eq!(standings.championship_positions, vec![2]);
    }

    #[test]
    fn a_novice_excludes_the_first_eight_slots() {
        let mut e = event(0, 0x20, 0);
        e.cfg.results_16c[2][0xd] = 0xff;
        let standings = compute_standings(&mut e, &finish(20_000)).unwrap();
        assert_eq!(cumulative(&e, 0), EXCLUDED_TIME);
        assert_eq!(cumulative(&e, 3), EXCLUDED_TIME, "even the player's slot");
        assert_eq!(standings.rally[0].slot, 8);
        // An excluded slot keeps its 0xff and scores with -1 wins.
        assert_eq!(e.cfg.results_16c[2][0xd], 0xff);
    }

    #[test]
    fn disqualification_time_trials_and_head_to_heads() {
        let mut e = event(0, 0x00, 0x08);
        let mut dq = finish(5_000);
        dq.disqualified = true;
        dq.opponent_slot = 7;
        let standings = compute_standings(&mut e, &dq).unwrap();
        assert_eq!(standings.stage[15].value, EXCLUDED_TIME);
        assert_eq!(standings.stage_positions, vec![15]);
        let mut trial = event(2, 0x00, 0x08);
        assert_eq!(compute_standings(&mut trial, &finish(5_000)), None);
        let mut h2h = finish(5_000);
        h2h.head_to_head_pending = true;
        assert_eq!(compute_standings(&mut event(0, 0, 8), &h2h), None);
    }
}
