//! The rivals' performance bytes `FUN_00465d40(min, mid, max)`: one byte per
//! result slot (`DAT_0057af44`), 0 the fastest and `0xff` the slowest, from
//! which the stage load builds each rival's stage time.
//!
//! Re-derived from the instructions (`0x465d40..0x466299`). The stage's
//! fastest, middle and slowest totals give `F = (mid*255 - min*255) /
//! (max - min)`, which reuses the first argument's stack slot; each slot's
//! tier (rival record `+9 + country`) sets a base value; random jitter is
//! added; the values are rescaled around the sixth-fastest rival; and the
//! difficulty decides how the second eight slots spread. The jitter draws are
//! `ftol(rand() / 32767 * 65536)` ([`GameLcg::next_q16`]). Native reads past
//! the 16 slots (a stale selection index) are refused with `None`. A
//! description of disassembly, not original source. Evidence:
//! `rea.official-eu.rival-times`.

use crate::game_rng::GameLcg;

/// One generation pass's inputs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RivalTimeInput {
    /// The stage's fastest, middle and slowest totals (the pace file's words
    /// 0, 12 and 1, each at least 1).
    pub min: u32,
    pub mid: u32,
    pub max: u32,
    /// Each slot's tier, rival record `+9 + country`.
    pub tiers: [u8; 16],
    /// `cfg[0x18] & 0x18 != 0`.
    pub difficulty_set: bool,
    /// `DAT_00550c08`: the cars driven by players.
    pub players: u32,
    /// `DAT_0057af94`/`DAT_0057af98`: their slots.
    pub player_slots: [u32; 2],
}

fn ratio_byte(numerator: i32, denominator: i32) -> Option<u8> {
    (denominator != 0).then(|| numerator.wrapping_div(denominator) as u8)
}

fn q16_floor_to_zero(value: i32) -> i32 {
    value.wrapping_add((value >> 31) & 0xffff) >> 16
}

fn noise(rng: &mut GameLcg) -> i32 {
    let draw = rng.next_q16();
    (draw.wrapping_add(draw << 8) >> 16) - 0x100
}

/// `FUN_00465d40`. `None` when the stage's totals divide by zero or a native
/// stale index would leave the 16 slots.
#[must_use]
pub fn rival_performance(input: RivalTimeInput, rng: &mut GameLcg) -> Option<[u8; 16]> {
    let spread = input.max.wrapping_sub(input.min);
    if spread == 0 {
        return None;
    }
    let fraction = input
        .mid
        .wrapping_mul(255)
        .wrapping_sub(input.min.wrapping_mul(255))
        / spread;
    let f = fraction as i32;
    let mut perf = [0u8; 16];
    let mut jitter_low = input.max as i32;
    let mut jitter_high = f;
    for (slot, &tier) in input.tiers.iter().enumerate() {
        perf[slot] = if tier < 6 {
            if tier == 1 {
                0
            } else {
                ((i32::from(tier) - 1).wrapping_mul(f) / 5) as u8
            }
        } else if tier == 6 {
            f as u8
        } else if tier == 0x10 {
            0xff
        } else {
            let value =
                (((i32::from(tier) - 6).wrapping_mul(0xff - f) / 10) as u8).wrapping_add(f as u8);
            if tier == 8 {
                jitter_low = i32::from(value);
            } else if tier == 9 {
                jitter_high = i32::from(value);
            }
            value
        };
    }
    for (slot, value) in perf.iter_mut().enumerate() {
        if slot < 8 {
            let n = noise(rng);
            let jitter = q16_floor_to_zero(n.wrapping_mul(n).wrapping_mul(jitter_low));
            *value = (jitter as u8).wrapping_add(*value) >> 1;
        } else {
            let flipped = 0xff - *value;
            let n = noise(rng);
            let jitter = q16_floor_to_zero((0xff - jitter_high).wrapping_mul(n).wrapping_mul(n));
            *value = 0xff - ((jitter as u8).wrapping_add(flipped) >> 1);
        }
    }
    let highest = perf.iter().copied().max()?;
    let lowest = perf.iter().copied().min()?;
    let mut marked = [false; 16];
    let mut selected = fraction as usize;
    for _ in 0..6 {
        let mut best = 0xff;
        for (slot, &value) in perf.iter().enumerate() {
            let eligible = match input.players {
                1 => slot as u32 != input.player_slots[0],
                2 => !input.player_slots.contains(&(slot as u32)),
                _ => false,
            };
            if !marked[slot] && value < best && eligible {
                selected = slot;
                best = value;
            }
        }
        *marked.get_mut(selected)? = true;
    }
    let sixth = *perf.get(selected)?;
    if highest > lowest {
        for value in &mut perf {
            let p = *value;
            *value = if p == lowest {
                0
            } else if p == highest {
                0xff
            } else if p == sixth {
                f as u8
            } else if p < sixth {
                ratio_byte(
                    i32::from(p.wrapping_sub(lowest)).wrapping_mul(f),
                    i32::from(sixth) - i32::from(lowest),
                )?
            } else {
                ratio_byte(
                    i32::from(p.wrapping_sub(sixth)).wrapping_mul(0xff - f),
                    i32::from(highest) - i32::from(sixth),
                )?
                .wrapping_add(f as u8)
            };
        }
    }
    if input.difficulty_set {
        let top = i32::from(perf[..8].iter().copied().max()?);
        let mut taken = [false; 8];
        let order: [usize; 8] = std::array::from_fn(|_| {
            let mut best = 1000;
            let mut pick = 0;
            for offset in 0..8 {
                if !taken[offset] && i32::from(perf[8 + offset]) < best {
                    best = i32::from(perf[8 + offset]);
                    pick = offset;
                }
            }
            taken[pick] = true;
            8 + pick
        });
        let step = 0xff - top;
        let mut acc = step;
        for &slot in &order[..7] {
            let draw = rng.next_q16();
            let wobble = draw.wrapping_mul(10) >> 16;
            let value = (acc / 8 + top + wobble - 5).clamp(top, 0xff);
            perf[slot] = value as u8;
            acc += step;
        }
        perf[order[7]] = 0xff;
    } else {
        for slot in 0..8 {
            perf[slot + 8] = perf[slot];
            perf[slot] = 0xff;
        }
        let excluded = |slot: usize, players: &[u32]| players.contains(&(slot as u32));
        if input.players == 1 {
            let player = [input.player_slots[0]];
            let slowest = slowest_rival(&perf, fraction as usize, |slot| excluded(slot, &player));
            let next = slowest_rival(&perf, fraction as usize, |slot| {
                slot == slowest || excluded(slot, &player)
            });
            *perf.get_mut(next)? = f as u8;
            *perf.get_mut(slowest)? = 0xff;
        } else {
            let slowest = if input.players == 2 {
                slowest_rival(&perf, fraction as usize, |slot| {
                    excluded(slot, &input.player_slots)
                })
            } else {
                fraction as usize
            };
            *perf.get_mut(slowest)? = f as u8;
        }
    }
    Some(perf)
}

/// The slot among 8..15 with the strictly largest byte above 0, or the
/// native's starting index.
fn slowest_rival(perf: &[u8; 16], start: usize, skip: impl Fn(usize) -> bool) -> usize {
    let mut best = 0;
    let mut pick = start;
    for (slot, &value) in perf.iter().enumerate().skip(8) {
        if value > best && !skip(slot) {
            best = value;
            pick = slot;
        }
    }
    pick
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(difficulty_set: bool) -> RivalTimeInput {
        RivalTimeInput {
            min: 6000,
            mid: 8000,
            max: 12000,
            tiers: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
            difficulty_set,
            players: 1,
            player_slots: [3, 0],
        }
    }

    #[test]
    fn a_set_difficulty_spreads_the_second_eight_above_the_first() {
        let perf = rival_performance(input(true), &mut GameLcg::new(1)).unwrap();
        let top = *perf[..8].iter().max().unwrap();
        assert!(perf[8..].iter().all(|&value| value >= top));
        assert_eq!(perf[8..].iter().filter(|&&value| value == 0xff).count(), 1);
        // The rescale pins the extremes: some slot is the fastest.
        assert!(perf.contains(&0));
    }

    #[test]
    fn a_novice_moves_the_rivals_up_and_excludes_the_first_eight() {
        let perf = rival_performance(input(false), &mut GameLcg::new(7)).unwrap();
        assert_eq!(&perf[..8], &[0xff; 8]);
        // F = (8000*255 - 6000*255) / 6000 = 85.
        assert!(perf[8..].contains(&85));
        assert!(perf[8..].contains(&0xff));
    }

    #[test]
    fn the_generation_is_deterministic_and_rejects_bad_totals() {
        let a = rival_performance(input(true), &mut GameLcg::new(42));
        let b = rival_performance(input(true), &mut GameLcg::new(42));
        assert_eq!(a, b);
        let mut flat = input(true);
        flat.max = flat.min;
        assert_eq!(rival_performance(flat, &mut GameLcg::new(1)), None);
    }

    #[test]
    fn the_tier_bases() {
        // With no jitter (noise 0 needs a draw of 0x8000-ish), check the base
        // arithmetic directly through a single-tier pass would be fragile, so
        // check the helper rules instead.
        assert_eq!(q16_floor_to_zero(-0x1_0001), -1);
        assert_eq!(q16_floor_to_zero(0x1_ffff), 1);
        assert_eq!(ratio_byte(10, 0), None);
        assert_eq!(ratio_byte(-10, 3), Some((-3i32) as u8));
    }
}
