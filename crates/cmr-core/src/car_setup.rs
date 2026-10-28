//! The per-player car-setup block at `DAT_0050d950 + slot * 0xcd0`.
//!
//! Three routines own the physics scalars the four-wheel step reads from this
//! block (offsets below are relative to the slot base):
//!
//! - `FUN_0041e3f0` (car setup) resets them: the thirteen damage fractions
//!   `+0x800..+0x830` to zero, `+0x834` and `+0x838` to `1.0`, the four wheel
//!   constants `+0x83c..+0x848` to zero, `+0x84c..+0x864` to `1.0`, and the
//!   base angle `+0xbac` to zero.
//! - `FUN_00420c70` (race start) converts the player's thirteen carried
//!   damage bytes into the fractions (`(byte << 16) / 100`), after clearing
//!   the damage of each repair part whose condition reads 100, and zeroes the
//!   per-impact table `+0x864`; it then calls `FUN_00420740`.
//! - `FUN_00420740` runs when the car record's `+0x6f0` word is nonzero, at
//!   race start and after every impact (`FUN_0041f630`):
//!   it adds damage contributions, clamps, and derives the steering factor
//!   `+0x838`, wheel constants `+0x83c..`, engine response `+0x85c`, ratio
//!   factor `+0x860`, base angle `+0xbac`, and rescales the control record's
//!   gear tables `+0x1d0/+0x1f0` from the copies at `+0x210/+0x230`.
//!
//! The other code addresses these through their absolute names: `DAT_0050e170`
//! is `+0x820` (the gear-2 skip word), `DAT_0050e188` is `+0x838`,
//! `DAT_0050e18c` is `+0x83c`, `DAT_0050e1ac` is `+0x85c`, and `DAT_0050e4fc`
//! is `+0xbac` (all for slot 0).
//!
//! Observations are at pseudocode level (`FUN_00420740`, `FUN_00420c70`,
//! `FUN_0041e3f0`). The thirteen fractions are the damage the stages carry
//! ([`crate::car_damage`]); `+0x864` is the last impact's intensity per
//! part, set by the mesh deformation `FUN_0041f880`.

use crate::fixed_point::Fixed16;

/// The physics words of one slot of the setup block.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CarSetupBlock {
    /// `+0x800..+0x830`: the thirteen damage fractions.
    pub setup_800: [i32; 13],
    /// `+0x834`.
    pub word_834: i32,
    /// `+0x838` (`DAT_0050e188`): the steering factor.
    pub steering_factor_838: i32,
    /// `+0x83c..+0x848` (`DAT_0050e18c`): the four wheel constants.
    pub wheel_constants_83c: [i32; 4],
    /// `+0x84c..+0x858`.
    pub words_84c: [i32; 4],
    /// `+0x85c` (`DAT_0050e1ac`): the engine response.
    pub engine_response_85c: i32,
    /// `+0x860`: the gear-ratio factor.
    pub ratio_factor_860: i32,
    /// `+0xbac` (`DAT_0050e4fc`): the base steer angle.
    pub base_angle_bac: i16,
}

impl CarSetupBlock {
    /// The values `FUN_0041e3f0` leaves before race start.
    pub const RESET: Self = Self {
        setup_800: [0; 13],
        word_834: 0x1_0000,
        steering_factor_838: 0x1_0000,
        wheel_constants_83c: [0; 4],
        words_84c: [0x1_0000; 4],
        engine_response_85c: 0x1_0000,
        ratio_factor_860: 0x1_0000,
        base_angle_bac: 0,
    };

    /// `DAT_0050e170[slot] > 0xb333`: the gearbox's gear-2 skip.
    #[must_use]
    pub fn gear_two_skip(&self) -> bool {
        self.setup_800[8] > 0xb333
    }
}

/// The bytes of one player's config record from `+0x9a`: the repair parts'
/// cost/condition/start triples and the carried damage bytes `0x0f..`.
pub type SetupConfigBytes = [u8; 0x1c];

/// `FUN_00420c70`: clear the carried damage of each repair part whose
/// condition reads 100 (`& 0x7f`), returning the updated record and the
/// thirteen fractions `(byte << 16) / 100`.
#[must_use]
pub fn setup_fractions_from_config(record: SetupConfigBytes) -> (SetupConfigBytes, [i32; 13]) {
    let mut record = record;
    let selected = |byte: u8| byte & 0x7f == 100;
    if selected(record[1]) {
        record[0x13] = 0;
    }
    if selected(record[4]) {
        record[0x14] = 0;
        record[0x0f..0x13].fill(0);
    }
    if selected(record[7]) {
        record[0x15] = 0;
        record[0x18..0x1c].fill(0);
    }
    if selected(record[10]) {
        record[0x16] = 0;
    }
    if selected(record[13]) {
        record[0x17] = 0;
    }
    let fractions =
        std::array::from_fn(|index| ((u32::from(record[0x0f + index]) << 16) / 100) as i32);
    (record, fractions)
}

/// Damage inputs to `FUN_00420740`: the table `+0x864` and the six index
/// bytes `+0xcc1..+0xcc6`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SetupDamage<'a> {
    pub table_864: &'a [i32],
    pub indices_cc1: [u8; 6],
}

/// The control-record gear tables `FUN_00420740` rewrites.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RescaledGearTables {
    /// Control `+0x1d0`.
    pub ratio_1d0: [i32; 8],
    /// Control `+0x1f0`.
    pub inverse_1f0: [i32; 8],
}

/// A damage index byte that reads outside the supplied table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CarSetupError {
    DamageIndex {
        index: u8,
        table_len: usize,
    },
    /// `2^32 / +0x860` does not fit the shipped 32-bit `IDIV` result.
    RatioReciprocalOverflow {
        ratio_factor_860: i32,
    },
}

impl std::fmt::Display for CarSetupError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DamageIndex { index, table_len } => write!(
                formatter,
                "setup damage index {index} is outside the {table_len}-entry table"
            ),
            Self::RatioReciprocalOverflow { ratio_factor_860 } => write!(
                formatter,
                "setup ratio factor {ratio_factor_860:#x} has no 32-bit reciprocal"
            ),
        }
    }
}

impl std::error::Error for CarSetupError {}

fn qmul(left: i32, right: i32) -> i32 {
    Fixed16::from_raw(left).mul(Fixed16::from_raw(right)).raw()
}

/// `FUN_00420740`'s body, run only when the car record's `+0x6c0` is nonzero.
///
/// `control_148` is the control record's full throttle response; `raw_copy`
/// and `reciprocal_copy` are its `+0x210/+0x230` families.
///
/// # Errors
///
/// Returns [`CarSetupError`] before changing anything when a damage index
/// reads outside the table or the ratio factor has no 32-bit reciprocal.
pub fn derive_car_setup(
    block: CarSetupBlock,
    damage: SetupDamage<'_>,
    control_148: i32,
    raw_copy: [i32; 8],
    reciprocal_copy: [i32; 8],
) -> Result<(CarSetupBlock, RescaledGearTables), CarSetupError> {
    let damage_value = |slot: usize| {
        let index = damage.indices_cc1[slot];
        damage
            .table_864
            .get(usize::from(index))
            .copied()
            .ok_or(CarSetupError::DamageIndex {
                index,
                table_len: damage.table_864.len(),
            })
    };
    let raw: [i32; 6] = [
        damage_value(0)?,
        damage_value(1)?,
        damage_value(2)?,
        damage_value(3)?,
        damage_value(4)?,
        damage_value(5)?,
    ];
    let [d1, d2, d3, d4] = [raw[0], raw[1], raw[2], raw[3]].map(|value| qmul(value, 0x4ccc));

    let mut block = block;
    let s = &mut block.setup_800;
    // Offsets +0x800, +0x804, +0x808, +0x80c and +0x824..+0x830.
    s[0] = s[0].wrapping_add(d3.wrapping_add(d1));
    s[1] = s[1].wrapping_add(d4.wrapping_add(d1));
    s[2] = s[2].wrapping_add(d3.wrapping_add(d2));
    s[3] = s[3].wrapping_add(d4.wrapping_add(d2));
    s[10] = s[10].wrapping_add(d3.wrapping_add(d1));
    s[9] = s[9].wrapping_add(d1.wrapping_add(d4));
    s[12] = s[12].wrapping_add(d3.wrapping_add(d2));
    s[11] = s[11].wrapping_add(d4.wrapping_add(d2));
    let front_sum = s[0..4]
        .iter()
        .fold(0_i32, |sum, word| sum.wrapping_add(*word));
    s[5] = qmul(front_sum, 0x4000);
    let rear_sum = s[9..13]
        .iter()
        .fold(0_i32, |sum, word| sum.wrapping_add(*word));
    s[6] = qmul(rear_sum, 0x4000);
    s[7] = s[7].wrapping_add(qmul(raw[0], 0x9999));
    s[8] = s[8].wrapping_add(qmul(qmul(raw[5].wrapping_add(raw[4]), 0x8000), 0x9999));
    for word in s.iter_mut() {
        if *word > 0x1_0000 {
            *word = 0x1_0000;
        }
    }
    let mut constants: [i32; 4] = [s[0], s[1], s[2], s[3]];

    // The balance term uses the unclamped front sum.
    let average = qmul(front_sum, 0x4000);
    let gain = if average < 0x6666 {
        0
    } else {
        qmul(average.wrapping_sub(0x6666), 0x1_aac0).min(0x1_0000)
    };
    let balance = constants[3]
        .wrapping_sub(constants[2])
        .wrapping_sub(constants[0])
        .wrapping_add(constants[1]);
    let mut angle = qmul(balance, qmul(gain, 0xa_0000));
    if angle.wrapping_abs() > 0x1_0000 {
        angle = if angle < 1 { -0x1_0000 } else { 0x1_0000 };
    }
    block.base_angle_bac = qmul(angle, 0x44) as i16;
    block.steering_factor_838 = 0x1_0000_i32.wrapping_sub(qmul(
        qmul(constants[1].wrapping_add(constants[0]), 0x8000),
        0x9999,
    ));
    let s = block.setup_800;
    block.words_84c[0] =
        0x1_0000_i32.wrapping_sub(qmul(qmul(s[9].wrapping_add(s[10]), 0x8000), 0x3333));
    block.words_84c[1] =
        0x1_0000_i32.wrapping_sub(qmul(qmul(s[12].wrapping_add(s[11]), 0x8000), 0x3333));
    block.words_84c[2] = qmul(block.words_84c[0], block.word_834);
    block.words_84c[3] = qmul(block.words_84c[1], block.word_834);
    for constant in &mut constants {
        *constant = qmul(*constant, 0xccc);
    }
    block.wheel_constants_83c = constants;

    let throttle_scale = qmul(control_148, 0x2_f0a3).max(0x1_0000);
    block.engine_response_85c = 0x1_0000_i32.wrapping_sub(qmul(s[7], qmul(throttle_scale, 0x3333)));
    block.ratio_factor_860 = 0x1_0000_i32.wrapping_sub(qmul(s[8], 0xccc));
    let reciprocal = if block.ratio_factor_860 == 0 {
        -1
    } else {
        i32::try_from((1_i64 << 32) / i64::from(block.ratio_factor_860)).map_err(|_| {
            CarSetupError::RatioReciprocalOverflow {
                ratio_factor_860: block.ratio_factor_860,
            }
        })?
    };
    let tables = RescaledGearTables {
        ratio_1d0: raw_copy.map(|word| qmul(word, block.ratio_factor_860)),
        inverse_1f0: reciprocal_copy.map(|word| qmul(word, reciprocal)),
    };
    Ok((block, tables))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_values_match_the_setup_routine() {
        let block = CarSetupBlock::RESET;
        assert_eq!(block.setup_800, [0; 13]);
        assert_eq!(block.engine_response_85c, 0x1_0000);
        assert_eq!(block.steering_factor_838, 0x1_0000);
        assert_eq!(block.wheel_constants_83c, [0; 4]);
        assert_eq!(block.ratio_factor_860, 0x1_0000);
        assert_eq!(block.base_angle_bac, 0);
        assert!(!block.gear_two_skip());
    }

    #[test]
    fn config_bytes_become_percent_fractions_after_the_selector_clears() {
        let mut record = [0_u8; 0x1c];
        for (index, byte) in record[0x0f..0x1c].iter_mut().enumerate() {
            *byte = 10 * (index as u8 + 1);
        }
        let (_, plain) = setup_fractions_from_config(record);
        assert_eq!(plain[0], (10 << 16) / 100);
        assert_eq!(plain[9], (100 << 16) / 100);
        assert_eq!(plain[12], (130 << 16) / 100);

        // Selector bytes reading 100 under the 0x7f mask clear their pairs.
        record[1] = 100 | 0x80;
        record[4] = 100;
        record[7] = 100;
        let (cleared_record, cleared) = setup_fractions_from_config(record);
        assert_eq!(cleared[4], 0); // +0x13 via selector 1
        assert_eq!(cleared[0..4], [0; 4]); // +0x0f..+0x12 via selector 4
        assert_eq!(cleared[5], 0); // +0x14 via selector 4
        assert_eq!(cleared[6], 0); // +0x15 via selector 7
        assert_eq!(cleared[9..13], [0; 4]); // +0x18..+0x1b via selector 7
        assert_eq!(cleared[7], plain[7]);
        assert_eq!(cleared_record[0x0f], 0);
    }

    #[test]
    fn undamaged_derivation_yields_the_reset_outputs_for_zero_setup() {
        let table = [0; 4];
        let (block, tables) = derive_car_setup(
            CarSetupBlock::RESET,
            SetupDamage {
                table_864: &table,
                indices_cc1: [0; 6],
            },
            0x51ec,
            [0x2_0000; 8],
            [0x8000; 8],
        )
        .unwrap();
        assert_eq!(block.engine_response_85c, 0x1_0000);
        assert_eq!(block.steering_factor_838, 0x1_0000);
        assert_eq!(block.ratio_factor_860, 0x1_0000);
        assert_eq!(block.base_angle_bac, 0);
        assert_eq!(tables.ratio_1d0, [0x2_0000; 8]);
        assert_eq!(tables.inverse_1f0, [0x8000; 8]);
    }

    #[test]
    fn setup_fractions_drive_steering_engine_ratio_and_constants() {
        let mut block = CarSetupBlock::RESET;
        block.setup_800 = [0x8000; 13];
        block.setup_800[7] = 0x4000;
        block.setup_800[8] = 0xc000;
        let table = [0];
        let (derived, tables) = derive_car_setup(
            block,
            SetupDamage {
                table_864: &table,
                indices_cc1: [0; 6],
            },
            0x2_0000,
            [0x2_0000; 8],
            [0x8000; 8],
        )
        .unwrap();
        // Steering: 1 - qmul(qmul(0.5 + 0.5, 0.5), 0.6).
        assert_eq!(
            derived.steering_factor_838,
            0x1_0000 - qmul(qmul(0x1_0000, 0x8000), 0x9999)
        );
        assert_eq!(derived.wheel_constants_83c, [qmul(0x8000, 0xccc); 4]);
        let scale = qmul(0x2_0000, 0x2_f0a3).max(0x1_0000);
        assert_eq!(
            derived.engine_response_85c,
            0x1_0000 - qmul(0x4000, qmul(scale, 0x3333))
        );
        assert_eq!(derived.ratio_factor_860, 0x1_0000 - qmul(0xc000, 0xccc));
        assert!(derived.gear_two_skip());
        assert_eq!(
            tables.ratio_1d0[0],
            qmul(0x2_0000, derived.ratio_factor_860)
        );
        // Words 5 and 6 are rebuilt as quarter sums.
        assert_eq!(derived.setup_800[5], qmul(4 * 0x8000, 0x4000));
        // A balanced front gives no base angle.
        assert_eq!(derived.base_angle_bac, 0);
    }

    #[test]
    fn unbalanced_front_setup_with_a_high_average_turns_the_base_angle() {
        let mut block = CarSetupBlock::RESET;
        block.setup_800[0] = 0x2000;
        block.setup_800[1] = 0xe000;
        block.setup_800[2] = 0x2000;
        block.setup_800[3] = 0xe000;
        let table = [0];
        let (derived, _) = derive_car_setup(
            block,
            SetupDamage {
                table_864: &table,
                indices_cc1: [0; 6],
            },
            0,
            [0; 8],
            [0; 8],
        )
        .unwrap();
        // Average 0.5 > 0x6666; balance = (0xe000 - 0x2000) - 0x2000 + 0xe000.
        let gain = qmul(0x8000 - 0x6666, 0x1_aac0);
        let angle = qmul(0x1_8000, qmul(gain, 0xa_0000)).clamp(-0x1_0000, 0x1_0000);
        assert_eq!(derived.base_angle_bac, qmul(angle, 0x44) as i16);
        assert_ne!(derived.base_angle_bac, 0);
    }

    #[test]
    fn damage_adds_to_the_paired_setup_words_and_clamps() {
        let table = [0, 0x1_0000];
        let damaged = derive_car_setup(
            CarSetupBlock::RESET,
            SetupDamage {
                table_864: &table,
                indices_cc1: [1, 0, 0, 0, 0, 0],
            },
            0,
            [0; 8],
            [0; 8],
        )
        .unwrap()
        .0;
        let d1 = qmul(0x1_0000, 0x4ccc);
        assert_eq!(derived_word(&damaged, 0), d1);
        assert_eq!(derived_word(&damaged, 1), d1);
        assert_eq!(derived_word(&damaged, 2), 0);
        assert_eq!(derived_word(&damaged, 7), qmul(0x1_0000, 0x9999));
        assert!(damaged.engine_response_85c < 0x1_0000);
    }

    fn derived_word(block: &CarSetupBlock, index: usize) -> i32 {
        block.setup_800[index]
    }

    #[test]
    fn a_damage_index_outside_the_table_is_reported() {
        let table = [0];
        assert_eq!(
            derive_car_setup(
                CarSetupBlock::RESET,
                SetupDamage {
                    table_864: &table,
                    indices_cc1: [0, 0, 0, 0, 0, 3],
                },
                0,
                [0; 8],
                [0; 8],
            ),
            Err(CarSetupError::DamageIndex {
                index: 3,
                table_len: 1
            })
        );
    }
}
