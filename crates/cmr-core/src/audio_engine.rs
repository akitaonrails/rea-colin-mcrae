//! Reconstruction of the engine-sound rev-band crossfade.
//!
//! The per-frame engine tick `FUN_00487e00` (with the whine sub-tick
//! `FUN_00488ab0`) drives the car's looping engine layers: it keeps each layer
//! on its own mixer channel and updates only per-channel volume and pitch each
//! frame, muting a layer by setting its volume to zero. Pitch is the recovered
//! frequency control `FUN_00495bd0`; volume goes through the master-scaled
//! setter `FUN_0047eb70`. See evidence `rea.official-eu.audio-engine-crossfade`.
//!
//! The three rev bands are front/rear emitter pairs of slots - Slow
//! `0`(FS)/`1`(RS), Mid `2`(FM)/`3`(RM), Top `5`(FT)/`6`(RT). The "F" layer takes
//! the front emitter's proximity and position, the "R" layer the rear's; the
//! `±20%` term the volume helpers subtract for F and add for R is a front/rear
//! balance from the camera heading (this corrects an earlier left/right stereo
//! reading). Slot `4` is WHINE and slot `7` carries the overrun/one-shot
//! chatter. The full tick is reconstructed in [`crate::engine_sound`].
//!
//! Observations encoded here: the band-index crossfade formula and its
//! out-of-window muting, the idle pitch, the per-layer channel arithmetic, and
//! the slot/band layout. Inferences: F/R is a front/rear emitter split (not
//! on/off power - on/off power is the separate T-band throttle gate
//! `DAT_00829548`); Slow/Mid/Top are rev bands. The per-band tables are static
//! `.data`, extracted from the user's copy (never committed).

/// Idle / out-of-range pitch, equal to the car waves' native rate (`0x2b11` =
/// 11025): the engine tick resets a layer to this pitch when its band is out of
/// the active RPM window.
pub const ENGINE_IDLE_PITCH: u32 = 0x2b11;

/// The WHINE layer slot offset (turbo/transmission whine, speed-driven).
pub const WHINE_SLOT: u32 = 4;

/// The overrun/chatter one-shot slot offset.
pub const CHATTER_SLOT: u32 = 7;

/// An engine rev band, each a front/rear pair of looping layer slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineRevBand {
    /// Low revs: slots 0 (FS) / 1 (RS). Volume-only crossfade (fixed pitch).
    Slow,
    /// Mid revs: slots 2 (FM) / 3 (RM).
    Mid,
    /// High revs: slots 5 (FT) / 6 (RT). Gated by the throttle on/off state.
    Top,
}

impl EngineRevBand {
    /// The `(front, rear)` slot offsets of the band's emitter pair: the first is
    /// the "F" layer (the recovered pan helper subtracts the pan term), the
    /// second the "R" layer (adds it). The pair is the discriminator - the slot
    /// numbers are not simply even/odd (the Top band is 5/6).
    pub fn slot_pair(self) -> (u32, u32) {
        match self {
            EngineRevBand::Slow => (0, 1),
            EngineRevBand::Mid => (2, 3),
            EngineRevBand::Top => (5, 6),
        }
    }

    /// The front ("F") layer slot offset.
    pub fn front_slot(self) -> u32 {
        self.slot_pair().0
    }

    /// The rear ("R") layer slot offset.
    pub fn rear_slot(self) -> u32 {
        self.slot_pair().1
    }
}

/// The mixer channel offset (from the car channel base) for a layer slot of a
/// given player: `player_index * 8 + slot_offset`, reproducing the recovered
/// `DAT_0082955c + slotOffset + carIdx*8` channel arithmetic (the second block
/// of eight is the split-screen player enabled when `DAT_00550c08 == 2`).
pub fn engine_channel_offset(player_index: u32, slot_offset: u32) -> u32 {
    player_index * 8 + slot_offset
}

/// The recovered rev-band position index for an RPM value:
/// `((rpm - lo) * count) / (hi - lo)`, or `None` when `rpm` is outside the
/// `[lo, hi]` window (the tick then mutes the band and resets it to
/// [`ENGINE_IDLE_PITCH`]). Overlapping band windows produce the band-to-band
/// crossfade. Reproduced verbatim, including the boundary where `rpm == hi`
/// yields `count` (the index tables carry the corresponding entry).
pub fn rev_band_index(rpm: i32, lo: i32, hi: i32, count: i32) -> Option<u32> {
    if hi <= lo || count <= 0 || rpm < lo || rpm > hi {
        return None;
    }
    Some((((rpm - lo) * count) / (hi - lo)) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rev_bands_map_to_the_recovered_slot_pairs() {
        assert_eq!(EngineRevBand::Slow.slot_pair(), (0, 1));
        assert_eq!(EngineRevBand::Mid.slot_pair(), (2, 3));
        assert_eq!(EngineRevBand::Top.slot_pair(), (5, 6));
        // front_slot/rear_slot agree with the pair; the Top band (5/6) shows the
        // slots are not a clean even/odd split.
        for band in [EngineRevBand::Slow, EngineRevBand::Mid, EngineRevBand::Top] {
            assert_eq!((band.front_slot(), band.rear_slot()), band.slot_pair());
        }
        assert_eq!(EngineRevBand::Top.front_slot(), 5);
    }

    #[test]
    fn idle_pitch_is_the_native_car_rate() {
        assert_eq!(ENGINE_IDLE_PITCH, 11_025);
    }

    #[test]
    fn channel_offsets_separate_the_two_players() {
        assert_eq!(engine_channel_offset(0, 5), 5);
        assert_eq!(engine_channel_offset(1, 5), 13);
        assert_eq!(engine_channel_offset(1, 0), 8);
    }

    #[test]
    fn rev_band_index_is_none_outside_the_window() {
        assert_eq!(rev_band_index(999, 1000, 2000, 4), None);
        assert_eq!(rev_band_index(2001, 1000, 2000, 4), None);
        // Degenerate windows are muted, not divided by zero.
        assert_eq!(rev_band_index(1500, 2000, 1000, 4), None);
        assert_eq!(rev_band_index(1500, 1000, 2000, 0), None);
    }

    #[test]
    fn rev_band_index_matches_the_recovered_formula() {
        // ((rpm - lo) * count) / (hi - lo)
        assert_eq!(rev_band_index(1000, 1000, 2000, 4), Some(0));
        assert_eq!(rev_band_index(1500, 1000, 2000, 4), Some(2));
        assert_eq!(rev_band_index(1250, 1000, 2000, 4), Some(1));
        // Boundary: rpm == hi yields `count` (one past the first `count` indices).
        assert_eq!(rev_band_index(2000, 1000, 2000, 4), Some(4));
    }

    #[test]
    fn overlapping_windows_crossfade_between_adjacent_bands() {
        // An RPM inside both the Slow [1000,1600] and Mid [1400,2200] windows
        // yields a position in each band - the recovered band-to-band crossfade.
        let rpm = 1500;
        assert!(rev_band_index(rpm, 1000, 1600, 4).is_some());
        assert!(rev_band_index(rpm, 1400, 2200, 4).is_some());
        // Above both overlaps only the higher band.
        assert!(rev_band_index(1800, 1000, 1600, 4).is_none());
        assert!(rev_band_index(1800, 1400, 2200, 4).is_some());
    }
}
