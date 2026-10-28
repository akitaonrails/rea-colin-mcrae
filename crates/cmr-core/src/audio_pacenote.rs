//! Reconstruction of the co-driver pacenote (speech) sound subsystem.
//!
//! The co-driver speaks the route ahead with short samples (`CAUTION`,
//! `HAIRPIN`, `LEFT`, `RIGHT`, the number calls `1..5`, ...). They are loaded
//! once into a dedicated bank and played one component at a time on a single
//! shared speech channel, strictly sequentially.
//!
//! Recovered functions (`Rally.exe`, base `0x400000`):
//!
//! - **Loader** `FUN_00444160` ("CoDriver_Create"): walks a 40-entry pointer
//!   table at `0x4ee924..0x4ee9c0` and opens each wave with
//!   `FUN_004958e0(path, 0x2b11, 0x10, 0)` - 11025 Hz / 16-bit / mono. The
//!   per-wave path is `<root>` + `<language dir>` + `codriver\` + `<filename>`.
//!   The bank's first slot id is captured before the loop, so a wave's slot
//!   offset equals its load index. The speech channel volume is set once to
//!   `DAT_004eea3c` (`0x7d00` = 32000).
//! - **Teardown** `FUN_004440f0` ("CoDriver_Destroy"): zeroes the ring/FIFO
//!   state.
//! - **Submit** `FUN_00444380(word, track)`: rewrites the direction field on a
//!   mirrored stage (swaps 1<->2) and either enqueues the packed word into the
//!   8-entry event ring (normal in-game path) or pushes it through the depth-2
//!   per-track FIFO immediately.
//! - **Sequencer** `FUN_00444450`: emits one slot per call, gated on the speech
//!   channel being done (`FUN_00495d10`), clearing each field as it plays it,
//!   and linking consecutive queued notes with `INTO.WAV` (slot offset `0x27`).
//! - **Play helper** `FUN_00444730` -> `FUN_00495a30` with play flags `0`
//!   (one-shot; never looped).
//!
//! Observations: the wave inventory and load order, the open parameters and
//! channel volume, the six-entry language-directory table and its selection,
//! the `codriver\` path shape, the one-shot shared-channel model, and the
//! per-word field-extraction arithmetic (shift/mask -> slot offset). Inferences:
//! slot offset == load index (sequential bank allocation); `helium` is the
//! developer "chipmunk" voice (paired with the `0x4100` pitch-shift path);
//! direction value 1 = `RIGHT`, 2 = `LEFT`. Unknowns (left in the evidence
//! ledger, not encoded as semantics): what the higher packed-word fields
//! (`>>9 & 7`, `>>0xc & 3`, `>>0xe & 1`, `>>0xf & 0x1f`) each mean, the FIFO
//! `9*(n-1)` accumulation, and the gating globals. See evidence
//! `rea.official-eu.audio-pacenote`.

use crate::audio_mixer::PLAY_ONE_SHOT;

/// Recovered sample rate (`0x2b11`), bit depth (`0x10`) and channel layout
/// (mono) every pacenote wave is opened with.
pub const PACENOTE_WAVE_RATE_HZ: u32 = 11_025;
pub const PACENOTE_WAVE_BITS: u16 = 16;

/// Volume the speech channel is set to once at load (`DAT_004eea3c` = `0x7d00`).
pub const PACENOTE_CHANNEL_VOLUME: i32 = 0x7d00;

/// Size of the scheduled-note event ring (`DAT_005590c8`, index masked `& 7`).
pub const PACENOTE_EVENT_RING_SIZE: usize = 8;

/// Depth of the per-track corner/distance FIFO (`DAT_00559100[track]` caps at 2).
pub const PACENOTE_TRACK_FIFO_DEPTH: u32 = 2;

/// Slot offset of `INTO.WAV`, played as the link between consecutive queued
/// notes (sequencer plays `base + 0x27`).
pub const INTO_LINK_SLOT_OFFSET: usize = 0x27;

/// Pacenotes always play one-shot; re-exported for callers building commands.
pub const PACENOTE_PLAY_FLAGS: u32 = PLAY_ONE_SHOT;

/// The pacenote wave bank in **load order**, so the index is the slot offset
/// relative to the bank base (`DAT_005590f8`). Recovered from the pointer table
/// `0x4ee924..0x4ee9c0` (reverse of the `.rdata` string-block order, with the
/// five number calls spliced in after `LONG`).
pub const PACENOTE_WAVES: [&str; 40] = [
    "CAUTION.WAV",  // 0x00
    "CARE.WAV",     // 0x01
    "LONG.WAV",     // 0x02
    "1.WAV",        // 0x03
    "2.WAV",        // 0x04
    "3.WAV",        // 0x05
    "4.WAV",        // 0x06
    "5.WAV",        // 0x07
    "SQUARE.WAV",   // 0x08
    "HAIRPIN.WAV",  // 0x09
    "TIGHTHAI.WAV", // 0x0a
    "RIGHT.WAV",    // 0x0b
    "LEFT.WAV",     // 0x0c
    "OVERCRES.WAV", // 0x0d
    "OVERJUMP.WAV", // 0x0e
    "OVERBR.WAV",   // 0x0f
    "UNDERBR.WAV",  // 0x10
    "OVERFIN.WAV",  // 0x11
    "OVERBUMP.WAV", // 0x12
    "TIGHTENS.WAV", // 0x13
    "OPENS.WAV",    // 0x14
    "DONTCUT.WAV",  // 0x15
    "ROCKS.WAV",    // 0x16
    "CONES.WAV",    // 0x17
    "BAILS.WAV",    // 0x18
    "TREES.WAV",    // 0x19
    "TREE.WAV",     // 0x1a
    "GATE.WAV",     // 0x1b
    "THRUGATE.WAV", // 0x1c
    "POST.WAV",     // 0x1d
    "FENCE.WAV",    // 0x1e
    "WALL.WAV",     // 0x1f
    "DITCH.WAV",    // 0x20
    "BANK.WAV",     // 0x21
    "TYRES.WAV",    // 0x22
    "WATERSP.WAV",  // 0x23
    "LOGS.WAV",     // 0x24
    "HOLEINS.WAV",  // 0x25
    "HOLEOUT.WAV",  // 0x26
    "INTO.WAV",     // 0x27 (see INTO_LINK_SLOT_OFFSET)
];

/// Co-driver speech language, in the recovered directory-table order
/// (`0x4ee9c4`). The index is `DAT_00d3ca80[0x18] & 7`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeechLanguage {
    English = 0,
    French = 1,
    German = 2,
    Spanish = 3,
    Italian = 4,
    /// The pitched-up developer voice; also the default when the language bits
    /// cannot be honoured.
    Helium = 5,
}

impl SpeechLanguage {
    /// The recovered `sounds\speech\<lang>\` directory, with its trailing
    /// backslash as stored.
    pub fn dir(self) -> &'static str {
        match self {
            SpeechLanguage::English => "sounds\\speech\\english\\",
            SpeechLanguage::French => "sounds\\speech\\french\\",
            SpeechLanguage::German => "sounds\\speech\\german\\",
            SpeechLanguage::Spanish => "sounds\\speech\\spanish\\",
            SpeechLanguage::Italian => "sounds\\speech\\italian\\",
            SpeechLanguage::Helium => "sounds\\speech\\helium\\",
        }
    }
}

/// Select the speech language the loader uses, reproducing `FUN_00444160`:
/// `Helium` is forced when either config flag is set (`cfg[0x26] & 0x20` or
/// `cfg[0x25] & 4`); otherwise the language index is `cfg[0x18] & 7`. Indices
/// `6`/`7` fall past the recovered six-entry table, so they are clamped to
/// `Helium` here (the binary reads adjacent memory there - an unknown, not
/// reproduced).
pub fn select_speech_language(language_index: u8, force_helium: bool) -> SpeechLanguage {
    if force_helium {
        return SpeechLanguage::Helium;
    }
    match language_index & 7 {
        0 => SpeechLanguage::English,
        1 => SpeechLanguage::French,
        2 => SpeechLanguage::German,
        3 => SpeechLanguage::Spanish,
        4 => SpeechLanguage::Italian,
        _ => SpeechLanguage::Helium,
    }
}

/// Build the pacenote wave path relative to the source root, reproducing the
/// loader's concatenation `<language dir>` + `codriver\` + `<filename>`
/// (e.g. `sounds\speech\english\codriver\INTO.WAV`). The absolute path the game
/// forms additionally prepends the installation root from `FUN_00496730`.
pub fn codriver_relative_path(language: SpeechLanguage, filename: &str) -> String {
    format!("{}codriver\\{filename}", language.dir())
}

/// The pacenote wave path relative to the media source root: the in-game
/// root is `<CDPath>\game\` (`FUN_0043bb60`), so the loader's path sits under
/// `game\`.
pub fn codriver_source_path(language: SpeechLanguage, filename: &str) -> String {
    format!("game\\{}", codriver_relative_path(language, filename))
}

/// One extracted component of a packed pacenote word: the observed field (its
/// right shift and mask) and the bank slot offset it plays
/// (`slot_delta + value`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacenoteField {
    /// Right shift applied to the packed word before masking.
    pub shift: u32,
    /// Mask applied after the shift to extract the field value.
    pub mask: u32,
    /// Added to the field value to get the bank-relative slot offset.
    pub slot_delta: i32,
}

/// The eight packed-word fields the sequencer `FUN_00444450` extracts, in the
/// order it checks them. Each non-zero field plays slot `slot_delta + value`.
/// The arithmetic (shift/mask/delta) is observed directly; the higher fields'
/// *meaning* is an unknown recorded in the evidence ledger.
pub const PACENOTE_WORD_FIELDS: [PacenoteField; 8] = [
    PacenoteField {
        shift: 0,
        mask: 0x3,
        slot_delta: -1,
    }, // CAUTION/CARE/LONG
    PacenoteField {
        shift: 2,
        mask: 0x1,
        slot_delta: 1,
    },
    PacenoteField {
        shift: 3,
        mask: 0xf,
        slot_delta: 2,
    }, // number / severity
    PacenoteField {
        shift: 7,
        mask: 0x3,
        slot_delta: 0xa,
    }, // direction: 1=RIGHT 2=LEFT
    PacenoteField {
        shift: 9,
        mask: 0x7,
        slot_delta: 0xc,
    },
    PacenoteField {
        shift: 0xc,
        mask: 0x3,
        slot_delta: 0x12,
    },
    PacenoteField {
        shift: 0xe,
        mask: 0x1,
        slot_delta: 0x14,
    },
    PacenoteField {
        shift: 0xf,
        mask: 0x1f,
        slot_delta: 0x15,
    },
];

/// Decode a packed pacenote word into the ordered bank slot offsets the
/// sequencer would speak, reproducing the observed per-field extraction. A
/// field with value 0 contributes nothing; a field whose computed offset falls
/// outside the recovered bank is skipped (the real words never set those bits
/// that high - the wide masks are the raw instruction operands, not the live
/// value range).
pub fn pacenote_word_components(word: u32) -> Vec<usize> {
    let mut slots = Vec::new();
    for field in PACENOTE_WORD_FIELDS {
        let value = (word >> field.shift) & field.mask;
        if value == 0 {
            continue;
        }
        let offset = field.slot_delta + value as i32;
        if (0..PACENOTE_WAVES.len() as i32).contains(&offset) {
            slots.push(offset as usize);
        }
    }
    slots
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wave_bank_layout_matches_recovery() {
        assert_eq!(PACENOTE_WAVES.len(), 40);
        assert_eq!(PACENOTE_WAVES[0], "CAUTION.WAV");
        assert_eq!(PACENOTE_WAVES[2], "LONG.WAV");
        // The five number calls are spliced in after LONG, slots 3..7.
        assert_eq!(
            &PACENOTE_WAVES[3..8],
            &["1.WAV", "2.WAV", "3.WAV", "4.WAV", "5.WAV"]
        );
        assert_eq!(PACENOTE_WAVES[0x0b], "RIGHT.WAV");
        assert_eq!(PACENOTE_WAVES[0x0c], "LEFT.WAV");
        // INTO is the link call at the documented offset.
        assert_eq!(PACENOTE_WAVES[INTO_LINK_SLOT_OFFSET], "INTO.WAV");
    }

    #[test]
    fn format_and_channel_constants_match_recovery() {
        assert_eq!(PACENOTE_WAVE_RATE_HZ, 11_025);
        assert_eq!(PACENOTE_WAVE_BITS, 16);
        assert_eq!(PACENOTE_CHANNEL_VOLUME, 32_000);
        assert_eq!(PACENOTE_EVENT_RING_SIZE, 8);
        assert_eq!(PACENOTE_TRACK_FIFO_DEPTH, 2);
        assert_eq!(PACENOTE_PLAY_FLAGS, PLAY_ONE_SHOT);
    }

    #[test]
    fn language_selection_reproduces_the_config_gate() {
        assert_eq!(select_speech_language(0, false), SpeechLanguage::English);
        assert_eq!(select_speech_language(4, false), SpeechLanguage::Italian);
        assert_eq!(select_speech_language(5, false), SpeechLanguage::Helium);
        // Forced helium ignores the index.
        assert_eq!(select_speech_language(0, true), SpeechLanguage::Helium);
        // Out-of-table indices clamp to helium.
        assert_eq!(select_speech_language(6, false), SpeechLanguage::Helium);
        assert_eq!(select_speech_language(7, false), SpeechLanguage::Helium);
    }

    #[test]
    fn relative_path_matches_the_codriver_shape() {
        assert_eq!(
            codriver_relative_path(SpeechLanguage::English, "INTO.WAV"),
            "sounds\\speech\\english\\codriver\\INTO.WAV"
        );
        assert_eq!(
            codriver_relative_path(SpeechLanguage::Helium, "HAIRPIN.WAV"),
            "sounds\\speech\\helium\\codriver\\HAIRPIN.WAV"
        );
    }

    #[test]
    fn word_components_extract_observed_fields() {
        // Empty word plays nothing.
        assert_eq!(pacenote_word_components(0), Vec::<usize>::new());
        // Direction field (shift 7, mask 3): value 1 -> RIGHT (0xa+1=0xb).
        assert_eq!(pacenote_word_components(1 << 7), vec![0x0b]);
        // value 2 -> LEFT (0xa+2=0xc).
        assert_eq!(pacenote_word_components(2 << 7), vec![0x0c]);
        // Number field (shift 3, mask 0xf): value 1 -> slot 0x02+1 = LONG-area.
        assert_eq!(pacenote_word_components(1 << 3), vec![0x03]);
        // Low field (shift 0, mask 3): value 1 -> slot -1+1 = CAUTION.
        assert_eq!(pacenote_word_components(1), vec![0x00]);
        // A combined word emits each set field once, in field order.
        let word = 1 | (1 << 3) | (2 << 7);
        assert_eq!(pacenote_word_components(word), vec![0x00, 0x03, 0x0c]);
    }

    #[test]
    fn out_of_range_field_values_are_skipped() {
        // Top field (shift 0xf, mask 0x1f) with a large value computes an
        // offset past the 40-slot bank and is dropped rather than invented.
        assert_eq!(pacenote_word_components(0x1f << 0xf), Vec::<usize>::new());
    }
}
