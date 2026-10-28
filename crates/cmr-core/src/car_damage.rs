//! The car damage the stages carry between them: the thirteen damage
//! fractions of the setup block (`+0x800..+0x830`), filled at race start
//! from the player's record bytes (`FUN_00420c70`), raised in the race by
//! the impacts (`FUN_00420740`) and by water (`FUN_00420260`), and written
//! back at the stage's end (`FUN_00420db0`) as the record's damage bytes and
//! the service screen's repair conditions and costs.
//!
//! The record is the player block from `+0x9a` (`rec`): the repair bytes
//! `A_i = rec[3i]` (cost, low 6 bits), `B_i = rec[3i + 1]` (condition %,
//! low 7 bits), `C_i = rec[3i + 2]`, and the carried damage bytes
//! `rec[0x0f..0x1c]`. A description of disassembly, not original source.
//! Evidence: `rea.official-eu.car-damage`.

use crate::car_setup::{
    CarSetupBlock, CarSetupError, RescaledGearTables, SetupConfigBytes, SetupDamage,
    derive_car_setup, setup_fractions_from_config,
};
use crate::legacy_math::sqrt_q16;

/// The repair parts' cost scales (minutes for a part at 0%): electrics,
/// steering, brakes, engine, gearbox.
pub const REPAIR_COST_SCALE: [i32; 5] = [30, 45, 30, 60, 60];

/// The high-detail part indices `+0xcc1..+0xcc6` (`FUN_0041efe0`).
pub const HIGH_DETAIL_INDICES: [u8; 6] = [7, 0xa, 1, 2, 6, 3];

/// The water surfaces (`FUN_00420260`'s `0x50..=0x53` test).
pub const WATER_SURFACES: std::ops::RangeInclusive<i16> = 0x50..=0x53;

fn qmul(a: i32, b: i32) -> i32 {
    ((i64::from(a) * i64::from(b)) >> 16) as i32
}

fn qdiv(a: i32, b: i32) -> i32 {
    ((i64::from(a) << 16) / i64::from(b)) as i32
}

/// `FUN_00420c70` for one player: clear the damage of the repaired parts in
/// the record, turn its damage bytes into the fractions (`(b << 16) / 100`)
/// and derive the handling (`FUN_00420740` with this impact table zeroed).
/// `control_148` is the control record's throttle scale and the ratios are
/// the gearbox's unscaled copies (`ctl +0x210/+0x230`).
///
/// # Errors
///
/// [`CarSetupError`] when the ratio factor has no reciprocal.
pub fn race_start(
    record: &mut SetupConfigBytes,
    control_148: i32,
    ratios: [i32; 8],
    inverses: [i32; 8],
) -> Result<(CarSetupBlock, RescaledGearTables), CarSetupError> {
    let (cleared, fractions) = setup_fractions_from_config(*record);
    *record = cleared;
    let block = CarSetupBlock {
        setup_800: fractions,
        ..CarSetupBlock::RESET
    };
    let zero = [0i32; 16];
    derive_car_setup(
        block,
        SetupDamage {
            table_864: &zero,
            indices_cc1: HIGH_DETAIL_INDICES,
        },
        control_148,
        ratios,
        inverses,
    )
}

/// The words `FUN_00420260` reads for one car.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WetInputs {
    /// `DAT_00d3ca88 == 0 && FUN_00485bb0()`: off on the super special.
    pub super_special: bool,
    /// `car +0x6f0`: the car is set up.
    pub initialised: bool,
    /// `car +0x674`: the wheels in contact.
    pub contact_count: i32,
    /// `ctl +0x42a`: the wheel surfaces.
    pub surfaces: [i16; 4],
    /// `car +0x264` (up row), `+0x324` (ground normal), `+0xd4` (velocity).
    pub up: [i32; 3],
    pub normal: [i32; 3],
    pub velocity: [i32; 3],
    /// `ctl +0x308`: the brake pedal.
    pub brake_308: i32,
}

fn dot(a: [i32; 3], b: [i32; 3]) -> i32 {
    qmul(a[0], b[0])
        .wrapping_add(qmul(a[1], b[1]))
        .wrapping_add(qmul(a[2], b[2]))
}

/// `FUN_00420260`, once per car per tick: driving upright and faster than
/// 1.0 through water wets the brakes (`+0x834` down by half the excess)
/// and damages the electrics (`+0x810` up by `excess * 0.05`); wet brakes
/// dry by `0x20` a tick, `0x147` while braking. The brake words `+0x854/
/// +0x858` follow `+0x834`. `wet` is the flag `+0xbb4`.
pub fn wet_tick(block: &mut CarSetupBlock, wet: &mut bool, input: &WetInputs) {
    if input.super_special || !input.initialised {
        return;
    }
    let recompute = |block: &mut CarSetupBlock| {
        block.words_84c[2] = qmul(block.words_84c[0], block.word_834);
        block.words_84c[3] = qmul(block.words_84c[1], block.word_834);
    };
    if input.contact_count > 0 {
        let water = input.surfaces.iter().any(|s| WATER_SURFACES.contains(s));
        if water && dot(input.up, input.normal) > 0xf0a3 {
            let n = input.normal;
            let v = input.velocity;
            let s = dot(n, v);
            let t: [i32; 3] = std::array::from_fn(|i| v[i].wrapping_sub(qmul(n[i], s)));
            let length = sqrt_q16(dot(t, t) as u32) as i32;
            if length > 0x1_0000 {
                let excess = length - 0x1_0000;
                let wet_by = qmul(excess, qdiv(0x1_0000, 0x2_0000)).min(0x1_0000);
                block.word_834 = (block.word_834 - wet_by).max(0);
                let shorted = qmul(excess, qdiv(0x1_0000, 0x14_0000)).min(0x1_0000);
                block.setup_800[4] = (block.setup_800[4] + shorted).min(0x1_0000);
                *wet = true;
            }
        }
        recompute(block);
    }
    if *wet {
        block.word_834 += if input.brake_308 != 0 { 0x147 } else { 0x20 };
        if block.word_834 >= 0x1_0000 {
            block.word_834 = 0x1_0000;
            *wet = false;
        }
        recompute(block);
    }
}

/// `FUN_00420db0` for one record: the fractions back into the damage bytes
/// (`(f * 100) >> 16`, or all zero for the stage before the fifth when not
/// `carry`), then each repair part's condition `B = 100 - damage` and its
/// cost `A = max(1, (100 - B)% * scale)` for any damage at all.
pub fn stage_end(record: &mut [u8], fractions: &[i32; 13], carry: bool) {
    for (j, &f) in fractions.iter().enumerate() {
        record[0x0f + j] = if carry {
            ((i64::from(f) * 100) >> 16) as u8
        } else {
            0
        };
    }
    repair_costs(record);
}

/// The repair loop of `FUN_00420db0` (also its trailing head-to-head pass).
pub fn repair_costs(record: &mut [u8]) {
    for (i, &scale) in REPAIR_COST_SCALE.iter().enumerate() {
        let damage = record[0x13 + i];
        let b = &mut record[3 * i + 1];
        *b = (*b & 0x80) | (0xe4u8.wrapping_sub(damage) & 0x7f);
        let condition = i32::from(record[3 * i + 1] & 0x7f);
        let f = qdiv(0x64_0000 - (condition << 16), 0x64_0000);
        let mut m = qmul(f, scale << 16) >> 16;
        if m == 0 && f != 0 {
            m = 1;
        }
        let a = &mut record[3 * i];
        *a = (*a & 0xc0) | (m as u8 & 0x3f);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_carried_bytes_round_trip_losing_a_percent_off_the_quarters() {
        let mut record = [0u8; 0x1c];
        record[0x0f + 7] = 40; // engine damage 40%
        record[0x0f + 8] = 50;
        let (block, _) = race_start(&mut record, 0x1_0000, [0x1_0000; 8], [0x1_0000; 8]).unwrap();
        assert_eq!(block.setup_800[7], (40 << 16) / 100);
        let mut out = record;
        stage_end(&mut out, &block.setup_800, true);
        assert_eq!(out[0x0f + 7], 39, "40 is not a multiple of 25");
        assert_eq!(out[0x0f + 8], 50);
    }

    #[test]
    fn a_repaired_part_starts_the_stage_undamaged() {
        let mut record = [0u8; 0x1c];
        record[0x0f + 7] = 60;
        record[3 * 3 + 1] = 100; // engine condition repaired to 100
        let (block, _) = race_start(&mut record, 0x1_0000, [0x1_0000; 8], [0x1_0000; 8]).unwrap();
        assert_eq!(block.setup_800[7], 0);
        assert_eq!(record[0x0f + 7], 0);
    }

    #[test]
    fn the_write_back_sets_conditions_and_costs() {
        let mut record = [0u8; 0x1c];
        let mut fractions = [0i32; 13];
        fractions[4] = 0x8000; // electrics half gone
        fractions[7] = 0x199; // a trace of engine damage
        stage_end(&mut record, &fractions, true);
        // Electrics: byte 50 -> condition 50, cost 15 of 30.
        assert_eq!((record[1] & 0x7f, record[0] & 0x3f), (50, 15));
        // Engine (part 3): byte 0 -> condition 100, free.
        assert_eq!((record[10] & 0x7f, record[9] & 0x3f), (100, 0));
        // Before stage five nothing carries.
        let mut early = [0u8; 0x1c];
        stage_end(&mut early, &fractions, false);
        assert_eq!((early[1] & 0x7f, early[0] & 0x3f), (100, 0));
    }

    #[test]
    fn a_minimal_damage_still_costs_a_minute() {
        let mut record = [0u8; 0x1c];
        record[0x13] = 1; // electrics 1%
        repair_costs(&mut record);
        assert_eq!((record[1] & 0x7f, record[0] & 0x3f), (99, 1));
    }

    fn splash(brake: i32) -> WetInputs {
        WetInputs {
            super_special: false,
            initialised: true,
            contact_count: 4,
            surfaces: [0x50, 0, 0, 0],
            up: [0, 0x1_0000, 0],
            normal: [0, 0x1_0000, 0],
            velocity: [0x3_0000, 0, 0],
            brake_308: brake,
        }
    }

    #[test]
    fn water_wets_the_brakes_and_shorts_the_electrics_then_they_dry() {
        let mut block = CarSetupBlock::RESET;
        let mut wet = false;
        wet_tick(&mut block, &mut wet, &splash(0));
        // Excess 2.0: brakes down by 1.0 (capped) then dried by 0x20.
        assert!(wet);
        assert_eq!(block.word_834, 0x20);
        assert_eq!(block.setup_800[4], qmul(0x2_0000, 0xccc));
        assert_eq!(block.words_84c[2], qmul(block.words_84c[0], 0x20));
        let dry = WetInputs {
            surfaces: [0; 4],
            ..splash(0x1_0000)
        };
        let mut ticks = 0;
        while wet {
            wet_tick(&mut block, &mut wet, &dry);
            ticks += 1;
        }
        assert_eq!(
            ticks,
            (0x1_0000 - 0x20 + 0x146) / 0x147,
            "braking dries at 0x147 a tick"
        );
        assert_eq!(block.word_834, 0x1_0000);
        // Not on the super special.
        let mut block = CarSetupBlock::RESET;
        let off = WetInputs {
            super_special: true,
            ..splash(0)
        };
        wet_tick(&mut block, &mut wet, &off);
        assert_eq!(block.word_834, 0x1_0000);
    }
}
