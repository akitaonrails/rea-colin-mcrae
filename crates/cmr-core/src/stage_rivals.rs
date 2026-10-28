//! The rivals' stage times the CatsEye load `FUN_00466350` makes for the
//! results: each player takes a result slot by car, the rivals get
//! performance bytes ([`crate::rival_times`]) and, from the stage's pace file
//! (`<stage>n.cjp` / `i.cjp` / `e.cjp` by difficulty: the fastest, slowest and
//! middle totals and nine cumulative split references), a time at every split.
//!
//! Per rival and split of reference length `d`: `u = d*max/min`,
//! `extra = u - (u - d)*(255 - perf)/255 - d`, a jitter
//! `(d*(min/100)/min) * n^2 >> 16` from `n = (q*257 >> 16) - 256`, and one
//! more draw that advances the generator again when `(q*150) & 0xffff0000`
//! is `0x10000`. A novice's slots 0..7 are excluded (`0xffffffff`). The
//! super-special stage draws one total instead. Each split's sixteen entries
//! are then bubble-sorted by time, and the fastest rival's splits become the
//! HUD reference (`DAT_005ffa1c`). A description of disassembly, not original
//! source. Evidence: `rea.official-eu.rival-times`.

use crate::event_flow::EventState;
use crate::game_rng::GameLcg;
use crate::rival_times::{RivalTimeInput, rival_performance};

/// The time an excluded or unplayed slot carries.
pub const NO_TIME: u32 = 0xffff_ffff;

/// A pace file's thirteen words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StagePace {
    pub words: [u32; 13],
}

impl StagePace {
    /// Parse a decoded `.cjp` (at least 52 bytes).
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let words = bytes.get(..52)?;
        Some(Self {
            words: std::array::from_fn(|i| {
                u32::from_le_bytes([
                    words[i * 4],
                    words[i * 4 + 1],
                    words[i * 4 + 2],
                    words[i * 4 + 3],
                ])
            }),
        })
    }
}

/// The pace file suffix for the difficulty (`0x4f3d64`, `0x4f3d5c`,
/// `0x4f3d54`), replacing the `.cat` extension.
pub fn pace_suffix(difficulty_018: u8) -> Option<&'static str> {
    match (difficulty_018 & 0x18) >> 3 {
        0 => Some("n.cjp"),
        1 => Some("i.cjp"),
        2 => Some("e.cjp"),
        _ => None,
    }
}

/// One entry of a split row (`DAT_0057af74`, 8 bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SplitEntry {
    pub time: u32,
    /// 2 for a rival; for a player the native writes `p != 0`.
    pub kind: u16,
    /// The rival's slot, or the player's car nibble.
    pub slot: u16,
}

/// What the stage load produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StageRivals {
    /// `DAT_0057af00`: each slot's stage time; a player's slot holds `-1 - p`
    /// until the finish replaces it.
    pub times: [u32; 16],
    /// `DAT_0057af94`: each player's slot.
    pub player_slots: Vec<u8>,
    /// Each split's sixteen entries, sorted by time.
    pub splits: Vec<[SplitEntry; 16]>,
    /// `DAT_005ffa1c`: the fastest rival's split times.
    pub hud_reference: [u32; 9],
}

/// The stage-load inputs besides the event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StageRivalInput<'a> {
    pub pace: Option<StagePace>,
    /// Which `.cat` nodes carry a split (`+0x18 != 0`), in order.
    pub split_nodes: &'a [bool],
    /// Each slot's rival record `+9 + country` tier.
    pub tiers: [u8; 16],
    /// `DAT_00550c08`, or 2 for a head-to-head.
    pub players: u32,
    /// Each player's car nibble (`cfg[0xb6 + p*0x9c] & 0xf`).
    pub player_cars: [u8; 2],
    /// `FUN_00485bb0`: the super-special stage.
    pub super_special: bool,
    /// `cfg[0x25] & 8` outside the school and the super-special stage.
    pub reversed: bool,
}

/// The slot a player's car takes (`0x466a9d` switch): the higher of its pair,
/// or the lower when that is taken.
fn player_slot(car: u8, times: &[u32; 16]) -> u8 {
    let (high, low): (u8, u8) = match car & 0xf {
        1 => (0xb, 0xa),
        2 | 5 => (9, 8),
        6 => (3, 2),
        7 => (0xd, 0xc),
        9 => (7, 6),
        10 => (5, 4),
        0xb => (0xf, 0xe),
        _ => (1, 0),
    };
    if times[usize::from(high)] != 0 {
        low
    } else {
        high
    }
}

fn draw(rng: &mut GameLcg) -> i32 {
    let q = rng.next_q16();
    (q.wrapping_add(q << 8) >> 16) - 0x100
}

/// `FUN_00466350`'s rival-time part. Rewrites the result slots' flag bytes
/// (`cfg + 0x178`).
pub fn stage_rivals(
    event: &mut EventState,
    input: &StageRivalInput<'_>,
    rng: &mut GameLcg,
) -> Option<StageRivals> {
    let words = input.pace.map(|pace| pace.words);
    let (min, max, mid) = match words {
        Some(w) => (w[0].max(1), w[1].max(1), w[12].max(1)),
        None => (6000, 12000, 8000),
    };
    let mut refs: [u32; 9] = match words {
        Some(w) => std::array::from_fn(|i| w[2 + i]),
        None => [0; 9],
    };
    if input.reversed && words.is_some() {
        let last = refs[8];
        let first = refs[0];
        refs = std::array::from_fn(|k| last.wrapping_sub(refs[8 - k]).wrapping_add(first));
    }
    let segments: [u32; 9] = std::array::from_fn(|i| {
        if i == 0 {
            refs[0]
        } else {
            refs[i].wrapping_sub(refs[i - 1])
        }
    });

    let mut times = [0u32; 16];
    for (index, slot) in event.cfg.results_16c.iter_mut().enumerate() {
        slot[0xc] = (((slot[0xc] ^ index as u8) & 0xf) ^ slot[0xc]) & 0xef;
    }
    let mut player_slots = Vec::new();
    for player in 0..input.players.min(2) as usize {
        let slot = player_slot(input.player_cars[player], &times);
        times[usize::from(slot)] = !(player as u32);
        let flag = &mut event.cfg.results_16c[usize::from(slot)][0xc];
        *flag = ((((*flag) ^ player as u8) & 0xf) ^ *flag) | 0x10;
        player_slots.push(slot);
    }

    let novice = event.cfg.difficulty_018 & 0x18 == 0;
    let perf = rival_performance(
        RivalTimeInput {
            min,
            mid,
            max,
            tiers: input.tiers,
            difficulty_set: !novice,
            players: input.players,
            player_slots: [
                player_slots.first().copied().map_or(0, u32::from),
                player_slots.get(1).copied().map_or(0, u32::from),
            ],
        },
        rng,
    )?;
    let per_hundred = min / 100;
    let split_count = input.split_nodes.iter().filter(|&&split| split).count();
    let mut splits = vec![
        [SplitEntry {
            time: 0,
            kind: 0,
            slot: 0
        }; 16];
        split_count
    ];
    let mut hud_reference = [0u32; 9];
    if input.super_special {
        for (slot, time) in times.iter_mut().enumerate() {
            if *time != 0 {
                continue;
            }
            *time = if novice && slot < 8 {
                NO_TIME
            } else {
                let n = draw(rng) as u32;
                (n.wrapping_mul(n).wrapping_mul(per_hundred) >> 16)
                    .wrapping_sub(
                        max.wrapping_sub(min)
                            .wrapping_mul(0xff - u32::from(perf[slot]))
                            / 0xff,
                    )
                    .wrapping_add(max)
            };
        }
    } else {
        let mut fastest = NO_TIME;
        for slot in 0..16 {
            let marker = times[slot];
            if marker == 0 {
                let mut cumulative = 0u32;
                for (k, row) in splits.iter_mut().enumerate() {
                    let entry = &mut row[slot];
                    entry.slot = slot as u16;
                    entry.kind = 2;
                    entry.time = if novice && slot < 8 {
                        NO_TIME
                    } else {
                        let d = segments.get(k).copied().unwrap_or(0);
                        let u = d.wrapping_mul(max) / min;
                        let extra = u
                            .wrapping_sub(
                                u.wrapping_sub(d).wrapping_mul(0xff - u32::from(perf[slot])) / 0xff,
                            )
                            .wrapping_sub(d);
                        let n = draw(rng) as u32;
                        let jitter_scale = d.wrapping_mul(per_hundred) / min;
                        let second = rng.next_q16();
                        if second.wrapping_mul(150) as u32 & 0xffff_0000 == 0x1_0000 {
                            rng.next_raw();
                        }
                        cumulative
                            .wrapping_add(d)
                            .wrapping_add(extra)
                            .wrapping_add(jitter_scale.wrapping_mul(n).wrapping_mul(n) >> 16)
                    };
                    cumulative = entry.time;
                }
                if cumulative < fastest {
                    for (k, reference) in hud_reference.iter_mut().enumerate() {
                        *reference = splits.get(k).map_or(0, |row| row[slot].time);
                    }
                    fastest = cumulative;
                }
                times[slot] = cumulative;
            } else {
                let player = usize::from(marker != NO_TIME);
                let car = u16::from(input.player_cars[player] & 0xf);
                for row in &mut splits {
                    row[slot] = SplitEntry {
                        time: marker,
                        kind: u16::from(marker != NO_TIME),
                        slot: car,
                    };
                }
            }
        }
        for row in &mut splits {
            loop {
                let mut swapped = false;
                for i in 0..15 {
                    if row[i + 1].time < row[i].time {
                        row.swap(i, i + 1);
                        swapped = true;
                    }
                }
                if !swapped {
                    break;
                }
            }
        }
    }
    Some(StageRivals {
        times,
        player_slots,
        splits,
        hud_reference,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pace() -> StagePace {
        let mut words = [0u32; 13];
        words[0] = 30_000;
        words[1] = 40_000;
        words[12] = 34_000;
        for (i, word) in words[2..11].iter_mut().enumerate() {
            *word = 3_000 * (i as u32 + 1);
        }
        StagePace { words }
    }

    fn event(difficulty: u8) -> EventState {
        let mut event = EventState::default();
        event.cfg.difficulty_018 = difficulty;
        event
    }

    fn input(splits: &[bool]) -> StageRivalInput<'_> {
        StageRivalInput {
            pace: Some(pace()),
            split_nodes: splits,
            tiers: [2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 2, 3],
            players: 1,
            player_cars: [6, 0],
            super_special: false,
            reversed: false,
        }
    }

    #[test]
    fn a_player_takes_the_slot_of_its_car_and_the_flags_are_set() {
        let mut e = event(0x08);
        let splits = [true; 9];
        let rivals = stage_rivals(&mut e, &input(&splits), &mut GameLcg::new(3)).unwrap();
        assert_eq!(rivals.player_slots, vec![3]);
        assert_eq!(rivals.times[3], NO_TIME, "player 0's marker");
        assert_eq!(e.cfg.results_16c[3][0xc], 0x10);
        assert_eq!(e.cfg.results_16c[5][0xc], 5);
        // Every rival gets a time near the pace totals.
        for slot in (0..16).filter(|&slot| slot != 3) {
            assert!((25_000..60_000).contains(&rivals.times[slot]), "{slot}");
        }
        // Rows are sorted and the HUD reference is the fastest rival's.
        let row = &rivals.splits[8];
        assert!(row.windows(2).all(|pair| pair[0].time <= pair[1].time));
        let fastest = (0..16)
            .filter(|&slot| slot != 3)
            .min_by_key(|&slot| rivals.times[slot])
            .unwrap();
        assert_eq!(rivals.hud_reference[8], rivals.times[fastest]);
    }

    #[test]
    fn a_novice_excludes_the_first_eight_and_a_taken_slot_moves_down() {
        let mut e = event(0);
        let splits = [true; 9];
        let mut two = input(&splits);
        two.players = 2;
        two.player_cars = [6, 6];
        let rivals = stage_rivals(&mut e, &two, &mut GameLcg::new(9)).unwrap();
        assert_eq!(rivals.player_slots, vec![3, 2]);
        for slot in [0, 1, 4, 5, 6, 7] {
            assert_eq!(rivals.times[slot], NO_TIME);
        }
        assert!(rivals.times[8] < NO_TIME);
    }

    #[test]
    fn pace_files_and_suffixes() {
        let mut bytes = vec![0u8; 52];
        bytes[0..4].copy_from_slice(&30_000u32.to_le_bytes());
        bytes[48..52].copy_from_slice(&34_000u32.to_le_bytes());
        let parsed = StagePace::parse(&bytes).unwrap();
        assert_eq!((parsed.words[0], parsed.words[12]), (30_000, 34_000));
        assert_eq!(StagePace::parse(&bytes[..51]), None);
        assert_eq!(pace_suffix(0), Some("n.cjp"));
        assert_eq!(pace_suffix(0x10), Some("e.cjp"));
        assert_eq!(pace_suffix(0x18), None);
    }

    #[test]
    fn the_super_special_stage_draws_one_total() {
        let mut e = event(0x08);
        let splits = [true; 9];
        let mut special = input(&splits);
        special.super_special = true;
        let rivals = stage_rivals(&mut e, &special, &mut GameLcg::new(5)).unwrap();
        assert!(rivals.splits.iter().all(|row| row[0].kind == 0));
        for slot in (0..16).filter(|&slot| slot != 3) {
            assert!((25_000..45_000).contains(&rivals.times[slot]), "{slot}");
        }
    }
}
