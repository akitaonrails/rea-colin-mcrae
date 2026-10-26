//! Reconstruction of the surface and collision impact sound banks.
//!
//! Two independent mixer banks, each with its own slot/channel base distinct
//! from the engine bank:
//!
//! - **Collision / "prang" bank** - loader `FUN_00469ac0` (slot base
//!   `DAT_00592ec8`, channel base `DAT_00592ecc`), 5 channels each set to the
//!   full volume `DAT_004f41a8` (`0x7d00` = 32000); 48 named waves across the
//!   `sounds\prangs\` and `sounds\accident\` groups, all 11025 Hz / 16-bit mono.
//!   Impacts play one-shot via `FUN_00469e80(speed, freq, objType)`.
//! - **Surface / skid bank** - loader `FUN_004892b0` (`sounds\newsurfs\`, slot
//!   base `DAT_0082a300`, channel base `DAT_0082a304`), 4 looping channels; per
//!   object type (0..0x77) it loads up to four variant groups named
//!   `<type>_o<n>.wav` / `_i` / `_sko` / `_ski` (surface outer/inner,
//!   skid outer/inner). Surface roll plays via `FUN_0046a030`.
//!
//! This module encodes the recovered arithmetic (impact sub-wave selection and
//! volume, roll-volume ramp, the variant filename shape) and the bank constants.
//! See evidence `rea.official-eu.audio-surface-collision`.
//!
//! Observations: the formulas and constants below, from the play functions.
//! Inferences: `o`/`i`/`sko`/`ski` are surface outer/inner and skid outer/inner
//! variants; impact speed drives both the sub-wave pick and the volume. Unknowns
//! (left as data, not reproduced): the per-group wave order, the material->wave
//! pointer table `(&PTR_s_COLLIS6_WAV_004f4104)[objType]`, and the per-type
//! variant-count byte tables.

/// Collision/prang bank: number of reserved channels.
pub const COLLISION_CHANNEL_COUNT: u32 = 5;

/// Full channel volume the collision bank sets (`DAT_004f41a8` = `0x7d00`).
pub const COLLISION_FULL_VOLUME: i32 = 0x7d00;

/// Surface/skid bank: number of reserved (looping) channels.
pub const SURFACE_CHANNEL_COUNT: u32 = 4;

/// Number of ROLL surface-rumble waves, at bank slot offset
/// [`ROLL_BANK_OFFSET`].
pub const ROLL_WAVE_COUNT: u32 = 5;

/// Slot offset of the first ROLL wave within the collision bank.
pub const ROLL_BANK_OFFSET: u32 = 0x1b;

/// Object-type count the surface loader iterates (`0..0x78`).
pub const SURFACE_OBJECT_TYPE_COUNT: u32 = 0x78;

/// Recovered sample rate (`0x2b11`) and bit depth (`0x10`, mono) shared by both
/// banks.
pub const SURFACE_WAVE_RATE_HZ: u32 = 11_025;
pub const SURFACE_WAVE_BITS: u16 = 16;

/// Exact impact-speed substitution from `FUN_00469e80`.
#[must_use]
pub fn normalized_impact_speed(speed: i32) -> i32 {
    if speed == 0x10000 { 0xfd70 } else { speed }
}

fn impact_q16(speed: i32, scale: i32) -> i32 {
    crate::fixed_point::Fixed16::from_raw(speed)
        .mul(crate::fixed_point::Fixed16::from_raw(scale))
        .raw()
}

/// The signed `(speed * 100 >> 16) % 10` impact-strength bucket.
#[must_use]
pub fn impact_speed_bucket(speed: i32) -> i32 {
    (impact_q16(normalized_impact_speed(speed), 0x640000) >> 16) % 10
}

/// The one-shot sub-wave slot offset `(speed * 10 >> 16) >> 16`.
#[must_use]
pub fn impact_subwave_offset(speed: i32) -> i32 {
    impact_q16(normalized_impact_speed(speed), 0xa0000) >> 16 >> 16
}

/// Primary impact volume: below bucket 11, half volume plus `bucket/20`;
/// otherwise full volume. Signed remainder by 10 makes the first arm the only
/// reachable branch for every input, including negative values.
#[must_use]
pub fn impact_primary_volume(speed: i32, full_volume: i32) -> i32 {
    let bucket = impact_speed_bucket(speed);
    if bucket < 11 {
        full_volume * bucket / 20 + full_volume / 2
    } else {
        full_volume
    }
}

/// Secondary material volume: below bucket 11, `full*bucket/10`; otherwise full.
#[must_use]
pub fn impact_secondary_volume(speed: i32, full_volume: i32) -> i32 {
    let bucket = impact_speed_bucket(speed);
    if bucket < 11 {
        full_volume * bucket / 10
    } else {
        full_volume
    }
}

/// Apply `FUN_0047f9c0`: above listener value 333, scale by listener/1000;
/// otherwise return one third, preserving signed integer division.
#[must_use]
pub fn scaled_volume_curve(listener_value: i32, volume: i32) -> i32 {
    if listener_value > 0x14d {
        listener_value * volume / 1000
    } else {
        volume / 3
    }
}

/// The surface-roll volume for a wheel's contact-sample counter, reproducing
/// `FUN_0046a030`: it ramps as `base/3 + (count-5)*base/0x2d` until `count`
/// reaches `0x24`, then holds at full `base`.
pub fn roll_volume(base: i32, contact_count: i32) -> i32 {
    if contact_count >= 0x24 {
        base
    } else {
        base / 3 + (contact_count - 5) * base / 0x2d
    }
}

/// A surface wave variant, matching the recovered `sprintf` tags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceVariant {
    /// `%d_o%d.wav` - surface, outer wheel.
    Outer,
    /// `%d_i%d.wav` - surface, inner wheel.
    Inner,
    /// `%d_sko%d.wav` - skid, outer wheel.
    SkidOuter,
    /// `%d_ski%d.wav` - skid, inner wheel.
    SkidInner,
}

impl SurfaceVariant {
    /// The recovered filename tag between the type id and the variant index.
    pub fn tag(self) -> &'static str {
        match self {
            SurfaceVariant::Outer => "o",
            SurfaceVariant::Inner => "i",
            SurfaceVariant::SkidOuter => "sko",
            SurfaceVariant::SkidInner => "ski",
        }
    }
}

/// Build the recovered surface wave filename `<type_id>_<tag><index>.wav`
/// (e.g. `5_sko2.wav`), the shape the loader's `sprintf` produces under
/// `sounds\newsurfs\`.
pub fn surface_wave_name(type_id: u32, variant: SurfaceVariant, index: u32) -> String {
    format!("{type_id}_{}{index}.wav", variant.tag())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bank_constants_match_recovery() {
        assert_eq!(COLLISION_CHANNEL_COUNT, 5);
        assert_eq!(COLLISION_FULL_VOLUME, 32_000);
        assert_eq!(SURFACE_CHANNEL_COUNT, 4);
        assert_eq!(ROLL_WAVE_COUNT, 5);
        assert_eq!(ROLL_BANK_OFFSET, 0x1b);
        assert_eq!(SURFACE_OBJECT_TYPE_COUNT, 120);
        assert_eq!(SURFACE_WAVE_RATE_HZ, 11_025);
    }

    #[test]
    fn impact_speed_volume_and_curve_match_native_integer_order() {
        assert_eq!(impact_speed_bucket(0x10000), 8);
        assert_eq!(impact_primary_volume(0x10000, 32_000), 28_800);
        assert_eq!(impact_secondary_volume(0x10000, 32_000), 25_600);
        assert_eq!(impact_subwave_offset(0x10000), 0);
        assert_eq!(impact_speed_bucket(0), 0);
        assert_eq!(impact_primary_volume(0, 32_000), 16_000);
        assert_eq!(scaled_volume_curve(334, 30_000), 10_020);
        assert_eq!(scaled_volume_curve(333, 30_000), 10_000);
    }

    #[test]
    fn roll_volume_ramps_then_holds_full() {
        let base = 32_000;
        assert_eq!(roll_volume(base, 0x24), base); // held at full
        assert_eq!(roll_volume(base, 100), base); // beyond threshold
        // At the start of the ramp (count 5) it is base/3.
        assert_eq!(roll_volume(base, 5), base / 3);
        // Midway it is below full and above the start.
        let mid = roll_volume(base, 0x18);
        assert!(mid > base / 3 && mid < base);
    }

    #[test]
    fn surface_wave_names_match_the_sprintf_shape() {
        assert_eq!(surface_wave_name(5, SurfaceVariant::Outer, 2), "5_o2.wav");
        assert_eq!(surface_wave_name(5, SurfaceVariant::Inner, 0), "5_i0.wav");
        assert_eq!(
            surface_wave_name(12, SurfaceVariant::SkidOuter, 1),
            "12_sko1.wav"
        );
        assert_eq!(
            surface_wave_name(12, SurfaceVariant::SkidInner, 3),
            "12_ski3.wav"
        );
    }
}
