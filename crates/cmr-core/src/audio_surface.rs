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

// NOTE: the impact sub-wave selection and the impact volume scaling in
// `FUN_00469e80` are observed (speed selects a sub-wave within the collision
// bank and scales the one-shot volume between ~half and ~full, with a named
// material wave layered on `channel+1` for hard objects `0 < objType < 0x11`),
// but the exact integer bit-expression is not yet transcribed with confidence
// from the delegated decompile, so it is documented in the evidence ledger
// (`rea.official-eu.audio-surface-collision`) rather than encoded here. Only the
// relations recovered unambiguously are implemented below.

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
