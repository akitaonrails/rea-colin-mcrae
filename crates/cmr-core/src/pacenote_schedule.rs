//! Reconstruction of the co-driver call *scheduling* and playback eventing (the
//! confidently-recovered G07 pieces): the audio event ring, the incremental
//! per-frame sequencer, the modifier swap, the into-link decision, the submit
//! routing, and the on-screen pacenote-strip arithmetic.
//!
//! The co-driver has **two independent queues**:
//!
//! - the **audio ring** `DAT_005590c8`: 8 entries of one packed note-id `u32`
//!   each (stride 4 bytes), with a tail `DAT_00559108` and head `DAT_0055910c`;
//! - the **visual FIFO** `DAT_004f2120` (depth 2, two interleaved tracks) plus a
//!   display table `DAT_0057c178` (stride `0x14`, `0x12` records per track) that
//!   drives the on-screen pacenote strip.
//!
//! Submit (`FUN_00444380`) optionally swaps the direction modifier (bits 7-8,
//! `1<->2`, when the mirror-stage config bit is set), then either enqueues the
//! whole note id into the audio ring or routes it to the visual FIFO only. The
//! sequencer tick (`FUN_00444450`, run once per frame before Mechanics, gated by
//! `QSWaveMixIsChannelDone` on the single shared speech channel) plays **one**
//! bitfield group per tick - the lowest-set field first, using the shift/mask
//! table [`crate::audio_pacenote::PACENOTE_WORD_FIELDS`] - clears that field and
//! writes the remainder back; the ring head only advances once an entry reaches
//! `0` (the phrase is complete). When the next entry carries a word/corner field
//! (`& 0x78`) the sequencer first plays the "INTO" link
//! ([`crate::audio_pacenote::INTO_LINK_SLOT_OFFSET`]), giving the "corner-A INTO
//! corner-B" cadence.
//!
//! The visual FIFO push places a word at the current depth slot and a second
//! "accumulate" call adds the display-row bump `9*(modifier-1)` and commits the
//! entry; this resolves the previously-ambiguous `9*(n-1)` term as display-list
//! index arithmetic, not an audio ring stride.
//!
//! See evidence `rea.official-eu.sim-pacenote-schedule`.
//!
//! Observations: the ring stride/indices/wrap, the per-field incremental decode
//! and write-back, the modifier swap, the `& 0x78` into-link test, the submit
//! routing predicate, and the `9*(mod-1)` / `0x14`-stride / `0x12`-per-track
//! display arithmetic. Inference: scheduling is **event-driven** (enqueue on
//! trigger-volume crossing, de-duplicated per player by a fire-once bit; drain
//! gated only by channel-free status) with **no stage-clock or distance
//! sampling** - the look-ahead is authored into the track's trigger-volume
//! placement. Unknowns (not reproduced): the 20-byte display-record field
//! layout, the exact semantics of the `DAT_00550c08`/`DAT_00550e90` mode gates,
//! and the depth-counter behaviour past 2 in the full-eviction branch.

use crate::audio_pacenote::{PACENOTE_EVENT_RING_SIZE, PACENOTE_WORD_FIELDS};

/// Bit mask of the word/corner field (bits 3-6); a non-zero value in the *next*
/// ring entry triggers the "INTO" link before it (`FUN_00444450`).
pub const INTO_LINK_TRIGGER_MASK: u32 = 0x78;

/// Shift of the direction-modifier field (bits 7-8) swapped by the mirror-stage
/// option.
pub const MODIFIER_SHIFT: u32 = 7;

/// Mask of the direction-modifier field after shifting.
pub const MODIFIER_MASK: u32 = 0x3;

/// Display-record stride in the on-screen pacenote table `DAT_0057c178`.
pub const DISPLAY_RECORD_STRIDE: usize = 0x14;

/// Number of display records per track in `DAT_0057c178`.
pub const DISPLAY_RECORDS_PER_TRACK: usize = 0x12;

/// Swap the direction modifier `1<->2` in a packed note id (`0` and `3` are
/// unchanged), reproducing the mirror-stage remap in `FUN_00444380`:
/// `if swap { m = (id>>7)&3; m = {1->2, 2->1}; id = (id & ~(3<<7)) | (m<<7) }`.
pub fn swap_modifier(id: u32, swap: bool) -> u32 {
    if !swap {
        return id;
    }
    let modifier = (id >> MODIFIER_SHIFT) & MODIFIER_MASK;
    let swapped = match modifier {
        1 => 2,
        2 => 1,
        other => other,
    };
    (id & !(MODIFIER_MASK << MODIFIER_SHIFT)) | (swapped << MODIFIER_SHIFT)
}

/// Whether the audio ring should receive a submitted note, reproducing the
/// `FUN_00444380` routing gate `DAT_00550c08 == 1 && (DAT_00550e90 == 1 ||
/// track == 0)`: a single shared speech channel means only the owning mode, and
/// only track 0 unless all cars are audible, reaches the audio ring; otherwise
/// the note is visual-only.
pub fn routes_to_audio(audio_mode_active: bool, all_cars_audible: bool, track: u32) -> bool {
    audio_mode_active && (all_cars_audible || track == 0)
}

/// Whether the next ring entry carries a word/corner field, so the sequencer
/// inserts the INTO link ahead of it (`next & 0x78 != 0`).
pub fn needs_into_link(next_entry: u32) -> bool {
    next_entry & INTO_LINK_TRIGGER_MASK != 0
}

/// The on-screen display-row bump a modifier contributes, resolving the
/// recovered `value += 9*(modifier-1)` accumulate (`FUN_00444320`).
pub fn modifier_row_bump(modifier: u32) -> i32 {
    modifier as i32 * 9 - 9
}

/// Index of a display record in `DAT_0057c178` for a note `value` on `track`,
/// reproducing the `FUN_004619e0` guarded index `(value + track*0x12)` (each
/// record is [`DISPLAY_RECORD_STRIDE`] bytes). `None` when `value` is outside the
/// recovered `0..0x12` range.
pub fn display_record_index(value: i32, track: usize) -> Option<usize> {
    if (0..DISPLAY_RECORDS_PER_TRACK as i32).contains(&value) {
        Some(value as usize + track * DISPLAY_RECORDS_PER_TRACK)
    } else {
        None
    }
}

/// One component the sequencer speaks on a tick: the bank slot offset played and
/// the remaining packed id after that field's bits are cleared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpokenComponent {
    /// Bank-relative slot offset played (`slot_delta + value`).
    pub bank_offset: i32,
    /// The packed id with the spoken field's bits cleared (written back to the
    /// ring entry; the head advances only when this reaches `0`).
    pub remainder: u32,
}

/// The next component the sequencer plays for a packed ring entry, reproducing
/// the lowest-set-field-first decode in `FUN_00444450`: scan
/// [`PACENOTE_WORD_FIELDS`] in order, play the first non-zero field as slot
/// `slot_delta + value`, and clear that field's bits. `None` when the entry is
/// `0` (the phrase is complete) or carries no recognised field.
pub fn next_component(entry: u32) -> Option<SpokenComponent> {
    for field in PACENOTE_WORD_FIELDS {
        let value = (entry >> field.shift) & field.mask;
        if value != 0 {
            return Some(SpokenComponent {
                bank_offset: field.slot_delta + value as i32,
                remainder: entry & !(field.mask << field.shift),
            });
        }
    }
    None
}

/// The co-driver audio event ring: 8 packed note-id entries drained one
/// component per frame (`DAT_005590c8` + head/tail indices).
///
/// Enqueue reproduces the shipped blind write + tail advance with no full check
/// (`FUN_00444380`): on overflow the oldest unread entry is overwritten, which
/// can collapse the ring to empty - a faithfully-preserved quirk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PacenoteRing {
    entries: [u32; PACENOTE_EVENT_RING_SIZE],
    head: usize,
    tail: usize,
}

impl Default for PacenoteRing {
    fn default() -> Self {
        Self::new()
    }
}

impl PacenoteRing {
    /// An empty ring (`FUN_004440f0` zeroes both indices).
    pub const fn new() -> Self {
        PacenoteRing {
            entries: [0; PACENOTE_EVENT_RING_SIZE],
            head: 0,
            tail: 0,
        }
    }

    /// Whether the ring holds no entries (`head == tail`).
    pub fn is_empty(&self) -> bool {
        self.head == self.tail
    }

    /// Enqueue a packed note id at the tail and advance it (mod 8), as the
    /// shipped submit does - a blind write with no full check.
    pub fn enqueue(&mut self, id: u32) {
        self.entries[self.tail] = id;
        self.tail = (self.tail + 1) % PACENOTE_EVENT_RING_SIZE;
    }

    /// The current head entry (the packed id being drained), or `None` if empty.
    pub fn head(&self) -> Option<u32> {
        if self.is_empty() {
            None
        } else {
            Some(self.entries[self.head])
        }
    }

    /// The entry after the head (used for the into-link look-ahead), or `None`
    /// when the head is the only / last entry.
    pub fn following(&self) -> Option<u32> {
        let next = (self.head + 1) % PACENOTE_EVENT_RING_SIZE;
        if next == self.tail {
            None
        } else {
            Some(self.entries[next])
        }
    }

    /// Write the drained remainder back into the head entry.
    pub fn set_head(&mut self, remainder: u32) {
        if !self.is_empty() {
            self.entries[self.head] = remainder;
        }
    }

    /// Advance past a completed head entry (mod 8).
    pub fn advance(&mut self) {
        if !self.is_empty() {
            self.head = (self.head + 1) % PACENOTE_EVENT_RING_SIZE;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_pacenote::INTO_LINK_SLOT_OFFSET;

    #[test]
    fn modifier_swap_only_exchanges_one_and_two() {
        // modifier 1 (RIGHT) <-> 2 (LEFT) in bits 7-8.
        assert_eq!(swap_modifier(1 << 7, true), 2 << 7);
        assert_eq!(swap_modifier(2 << 7, true), 1 << 7);
        // 0 and 3 are unchanged; other bits are preserved.
        assert_eq!(swap_modifier(0x1f, true), 0x1f);
        assert_eq!(swap_modifier(3 << 7, true), 3 << 7);
        // No swap when the option is off.
        assert_eq!(swap_modifier(1 << 7, false), 1 << 7);
    }

    #[test]
    fn audio_routing_honours_the_mode_gate() {
        // Active mode, all cars audible: any track reaches audio.
        assert!(routes_to_audio(true, true, 1));
        // Active mode, only car 0 audible.
        assert!(routes_to_audio(true, false, 0));
        assert!(!routes_to_audio(true, false, 1));
        // Inactive mode: never audio (visual only).
        assert!(!routes_to_audio(false, true, 0));
    }

    #[test]
    fn into_link_fires_only_for_a_following_word_field() {
        // A following entry with a corner/word field (bits 3-6) links.
        assert!(needs_into_link(0x3 << 3));
        // One with only non-word fields does not.
        assert!(!needs_into_link(0x1 << 7));
        assert!(!needs_into_link(0));
        // The link plays the documented INTO bank slot.
        assert_eq!(INTO_LINK_SLOT_OFFSET, 0x27);
    }

    #[test]
    fn display_arithmetic_matches_recovery() {
        // 9*(modifier-1): modifier 1 -> 0, 2 -> 9, 3 -> 18.
        assert_eq!(modifier_row_bump(1), 0);
        assert_eq!(modifier_row_bump(2), 9);
        assert_eq!(modifier_row_bump(3), 18);
        // Display index: value + track*0x12, guarded to 0..0x12.
        assert_eq!(display_record_index(5, 0), Some(5));
        assert_eq!(display_record_index(5, 1), Some(5 + 0x12));
        assert_eq!(display_record_index(0x12, 0), None);
        assert_eq!(display_record_index(-1, 0), None);
    }

    #[test]
    fn sequencer_drains_one_field_per_tick_lowest_first() {
        // A compound note: field0 value 1 (CAUTION), word field (bits 3-6) = 3,
        // modifier (bits 7-8) = 2 (LEFT).
        let note = 1 | (3 << 3) | (2 << 7);
        // First tick: lowest field (bits 0-1, value 1) -> slot_delta -1 + 1 = 0.
        let first = next_component(note).expect("field present");
        assert_eq!(first.bank_offset, 0);
        assert_eq!(first.remainder, note & !0x3);
        // Next: the word field (bits 3-6, value 3) -> slot_delta 2 + 3 = 5.
        let second = next_component(first.remainder).expect("word field present");
        assert_eq!(second.bank_offset, 5);
        assert_eq!(second.remainder, first.remainder & !(0xf << 3));
        // Next: the modifier (bits 7-8, value 2) -> slot_delta 0xa + 2 = 0xc (LEFT).
        let third = next_component(second.remainder).expect("modifier present");
        assert_eq!(third.bank_offset, 0xc);
        // The remainder is now zero: the phrase is complete.
        assert_eq!(third.remainder, 0);
        assert_eq!(next_component(third.remainder), None);
    }

    #[test]
    fn ring_enqueues_drains_and_looks_ahead() {
        let mut ring = PacenoteRing::new();
        assert!(ring.is_empty());
        assert_eq!(ring.head(), None);

        let a = 1 | (3 << 3);
        let b = 2 << 7;
        ring.enqueue(a);
        ring.enqueue(b);
        assert_eq!(ring.head(), Some(a));
        // The following entry (b) is visible for the into-link look-ahead.
        assert_eq!(ring.following(), Some(b));

        // Drain entry a one component at a time, writing back the remainder.
        let mut cur = ring.head().unwrap();
        while let Some(component) = next_component(cur) {
            cur = component.remainder;
            ring.set_head(cur);
        }
        assert_eq!(ring.head(), Some(0));
        // Entry complete: advance to b, which is now the only entry.
        ring.advance();
        assert_eq!(ring.head(), Some(b));
        assert_eq!(ring.following(), None);
        ring.advance();
        assert!(ring.is_empty());
    }

    #[test]
    fn ring_overflow_overwrites_blindly() {
        // The shipped ring has no full check: enqueuing a full ring's worth plus
        // one advances the tail into the head (the faithfully-preserved quirk).
        let mut ring = PacenoteRing::new();
        for i in 0..PACENOTE_EVENT_RING_SIZE as u32 {
            ring.enqueue(i + 1);
        }
        // Tail wrapped back onto the head: the ring reads empty despite writes.
        assert!(ring.is_empty());
    }
}
