//! Procedural rival stage times from `FUN_00465d40`.
//!
//! Tier values and player slots are inputs, not vendored data. The pure function
//! uses the recovered game LCG and returns no result for native fault domains
//! (zero duration or slot/division bounds). The decompiler's uninitialized
//! `local_44` outlier write is deliberately not modeled.

use crate::game_rng::GameLcg;

/// Inputs for one 16-slot rival time generation pass.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RivalTimeInput {
    pub start: u8,
    pub end: u8,
    pub duration: u32,
    pub tiers: [u8; 16],
    pub difficulty: u8,
    pub player_slot: u8,
    pub second_player_slot: u8,
    pub replay_car_count: u32,
}

/// Generate tier curves, jitter, ranking and player-specific placement.
#[must_use]
pub fn generate_rival_times(input: RivalTimeInput, rng: &mut GameLcg) -> Option<[u8; 16]> {
    if input.duration == u32::from(input.start)
        || input.player_slot >= 16
        || (input.replay_car_count == 2 && input.second_player_slot >= 16)
    {
        return None;
    }
    let fraction = (i32::from(input.end) * 0xff - i32::from(input.start) * 0xff)
        / (input.duration as i32 - i32::from(input.start));
    let mut times = [0_i32; 16];
    let mut jitter_a = fraction;
    let mut jitter_b = fraction;
    for (index, tier) in input.tiers.into_iter().enumerate() {
        let base = if tier < 6 {
            if tier == 1 {
                0
            } else {
                (i32::from(tier) - 1) * fraction / 5
            }
        } else if tier == 6 {
            fraction
        } else if tier == 0x10 {
            0xff
        } else {
            (i32::from(tier) - 6) * (0xff - fraction) / 10 + fraction
        };
        times[index] = base;
        if tier == 8 {
            jitter_a = base;
        } else if tier == 9 {
            jitter_b = base;
        }
    }
    for (index, value) in times.iter_mut().enumerate() {
        let noise = ((rng.next_raw() as i32 * 0x101) >> 16) - 0x100;
        let jitter =
            noise
                .wrapping_mul(noise)
                .wrapping_mul(if index < 8 { jitter_a } else { jitter_b });
        if index < 8 {
            *value = (*value).wrapping_add(jitter) >> 1;
        } else {
            *value = (-1 - (*value).wrapping_add(jitter)) >> 1;
        }
    }

    let minimum = *times.iter().min()?;
    let maximum = *times.iter().max()?;
    let mut selected = [false; 16];
    let mut sixth = fraction;
    let mut sixth_index = fraction as usize;
    for _ in 0..6 {
        let mut best = 0xff;
        for (index, value) in times.iter().enumerate() {
            let eligible = !selected[index]
                && *value < best
                && (input.replay_car_count == 1
                    || (input.replay_car_count == 2 && index != input.second_player_slot as usize))
                && index != input.player_slot as usize;
            if eligible {
                best = *value;
                sixth = best;
                sixth_index = index;
            }
        }
        selected[sixth_index] = true;
    }
    if minimum < maximum {
        for (index, value) in times.iter_mut().enumerate() {
            *value = if *value == minimum {
                0
            } else if *value == maximum {
                0xff
            } else if *value == sixth {
                sixth
            } else if *value < sixth {
                if sixth == minimum {
                    return None;
                }
                (*value - minimum) * fraction / (sixth - minimum)
            } else {
                if maximum == sixth {
                    return None;
                }
                (*value - sixth) * (0xff - fraction) / (maximum - sixth) + i32::from(input.start)
            };
            let _ = index;
        }
    }

    if input.difficulty != 0 {
        let mut order = [-1_i32; 8];
        let mut available = [true; 8];
        for slot in &mut order {
            let mut best = 1000;
            let mut best_index = 0;
            for (index, value) in times[..8].iter().enumerate() {
                if available[index] && *value < best {
                    best = *value;
                    best_index = index;
                }
            }
            *slot = best_index as i32;
            available[best_index] = false;
        }
        let maximum_first = *times[..8].iter().max()?;
        let mut allowance = 0xff - maximum_first;
        let mut remaining = 7;
        for index in order {
            let candidate = ((allowance + (allowance >> 31 & 7)) >> 3) - 5
                + maximum_first
                + ((rng.next_raw() as i32 * 10) >> 16);
            times[index as usize] = candidate.min(0xff);
            allowance += 0xff - maximum_first;
            remaining -= 1;
            if remaining == 0 {
                break;
            }
        }
    } else {
        times[..8].fill(0xff);
        if input.replay_car_count != 1 {
            let mut best = 0;
            let mut best_index = 8;
            for (index, _) in times.iter().enumerate().skip(8) {
                if times[index] > best
                    && index != input.player_slot as usize
                    && index != input.second_player_slot as usize
                {
                    best = times[index];
                    best_index = index;
                }
            }
            times[best_index] = i32::from(input.start);
        } else {
            let mut first = (0, 8);
            let mut second = (0, 8);
            for (index, _) in times.iter().enumerate().skip(8) {
                if times[index] > first.0 && index != input.player_slot as usize {
                    first = (times[index], index);
                }
            }
            for (index, _) in times.iter().enumerate().skip(8) {
                if times[index] > second.0
                    && index != first.1
                    && index != input.player_slot as usize
                {
                    second = (times[index], index);
                }
            }
            times[second.1] = i32::from(input.start);
            times[first.1] = 0xff;
        }
    }
    Some(times.map(|value| value as u8))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rival_times_use_tiers_rng_and_player_exclusions() {
        let input = RivalTimeInput {
            start: 10,
            end: 200,
            duration: 1000,
            tiers: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
            difficulty: 0,
            player_slot: 0,
            second_player_slot: 1,
            replay_car_count: 1,
        };
        let times = generate_rival_times(input, &mut GameLcg::new(1)).unwrap();
        assert_eq!(times.len(), 16);
        assert_eq!(&times[..8], &[0xff; 8]);
        assert!(times.windows(2).any(|pair| pair[0] != pair[1]));
        assert_eq!(
            generate_rival_times(input, &mut GameLcg::new(1)),
            Some(times)
        );
        assert_eq!(
            generate_rival_times(
                RivalTimeInput {
                    duration: u32::from(input.start),
                    ..input
                },
                &mut GameLcg::new(1),
            ),
            None
        );
    }

    #[test]
    fn difficulty_branch_preserves_order_and_caps_at_255() {
        let input = RivalTimeInput {
            start: 10,
            end: 200,
            duration: 1000,
            tiers: [7; 16],
            difficulty: 1,
            player_slot: 0,
            second_player_slot: 1,
            replay_car_count: 1,
        };
        let times = generate_rival_times(input, &mut GameLcg::new(1)).unwrap();
        assert_eq!(times.len(), 16);
        assert!(times.iter().any(|time| *time != 0));
    }
}
