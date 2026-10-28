//! The championship save dialog `FUN_0043bce0` the results director's page 5
//! (`0x431ae0`) runs every tick until it returns 0, its slot writer
//! `FUN_0044ef50`, and the slot list's labels (`FUN_0044f210`,
//! `FUN_0044f3d0`).
//!
//! The dialog is a state machine over `DAT_00551b58`:
//!
//! | state | does |
//! |---|---|
//! | 0 | opens the "save?" prompt `0x4ee4f0` (yes → 5, no → 1) |
//! | 1 | resets to 0, clears the draw latch `DAT_004ee4e8` and returns 0 |
//! | 2 | shows the message `DAT_00551b48` for `DAT_00551b60` ticks, or until a confirm when -1, then goes to `DAT_00551b50` |
//! | 3 | steps the open yes/no prompt (`FUN_004aae00`); a confirm goes to `DAT_00551b4c` on "yes", `DAT_00551b54` on "no" |
//! | 4 | the message `0x51` for four ticks, then 5 |
//! | 5 | loads the twelve slots (`FUN_0044ed90`), then 6 |
//! | 6 | the slot cursor `DAT_00551b5c` (0..=12, 12 cancels): an occupied slot asks to overwrite (`0x4ee5b0`: yes → 8, no → 9), a free one goes to 8, cancel to 9 |
//! | 7 | writes the slot, then the message `0x49` (two seconds), then 1 |
//! | 8 | the message `0x59` for four ticks, then 7 |
//! | 9 | the message `0x4f` (two seconds), then 0 (asks again) |
//!
//! The input is `FUN_0043baa0`'s change filter over the player's keys (a
//! mask is reported only when it differs from the previous one). The slot
//! cursor `DAT_00551b5c` is never reset, so it starts where the last save
//! left it. The host performs the slot I/O: it answers
//! [`SaveEffect::LoadSlots`] with [`SaveDialog::provide_slots`] before the
//! next tick and writes [`build_save_record`] on [`SaveEffect::Write`].
//! A description of disassembly, not original source. Evidence:
//! `rea.official-eu.save-dialog`.

use std::cell::Cell;

use crate::c_format::{FormatArg, c_format};
use crate::descriptor_menu::bits;
use crate::profile_store::{
    SLOT_COMPLEMENT_OFFSET, SLOT_FORMAT_MARKER, SLOT_FORMAT_OFFSET, SLOT_MAGIC, SLOT_MAGIC_OFFSET,
    SLOT_MODE_OFFSET, SLOT_PAYLOAD_LEN, SLOT_PAYLOAD_OFFSET, SLOT_SEED_OFFSET,
    SLOT_VOLATILE_ZERO_LEN, SLOT_VOLATILE_ZERO_OFFSET, slot_is_valid,
};
use crate::settings_store::{SAVE_SLOT_COUNT, SAVE_SLOT_LEN};

/// The cancel row of the slot list.
pub const CANCEL_ROW: i8 = SAVE_SLOT_COUNT as i8;

/// A yes/no prompt record (`FUN_004aabd0`): the question's text id, its Q16
/// y and flags, and whether "yes" starts selected (`+0x58 != 0`). Both
/// answers are the record's `0xe`/`0xf` at `(0x8000, 0x8000)` and
/// `(0x8000, 0x8ccc)`, flags `0x12`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Prompt {
    pub question: i32,
    pub y: i32,
    pub flags: u8,
    pub default_yes: bool,
}

/// `0x4ee4f0`: save the game? ("yes" first).
pub const SAVE_PROMPT: Prompt = Prompt {
    question: 0x4a,
    y: 0x7333,
    flags: 0x32,
    default_yes: true,
};
/// `0x4ee5b0`: overwrite the slot? ("no" first).
pub const OVERWRITE_PROMPT: Prompt = Prompt {
    question: 0x58,
    y: 0x7333,
    flags: 0x32,
    default_yes: false,
};

/// A request to the host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveEffect {
    /// `FUN_0044ed90`: read the twelve slot files.
    LoadSlots,
    /// `FUN_0044ef50(slot)`: write the live block to the slot.
    Write { slot: u8 },
}

/// One occupied slot's list entry: the player name at record `+0xc` and the
/// three-byte summary `FUN_0044f210`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlotEntry {
    pub name: Vec<u8>,
    pub summary: [u8; 3],
}

/// What the draw shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveView<'a> {
    /// The message and whether it waits for a confirm (`FUN_0044f1a0`).
    Message { text: i32, waits: bool },
    /// The open prompt and its selected answer (0 is "yes").
    Prompt { prompt: Prompt, selected: u8 },
    /// The slot list and its cursor (`FUN_0044f070`).
    Slots {
        cursor: i8,
        slots: &'a [Option<SlotEntry>; SAVE_SLOT_COUNT],
    },
}

/// The dialog's state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveDialog {
    /// `DAT_00551b58`.
    state: u8,
    /// `DAT_00551b50`: the state after the message.
    resume: u8,
    /// `DAT_00551b60`: the message's ticks left, -1 to wait for a confirm.
    countdown: i32,
    /// `DAT_00551b48`: the message's text id.
    message: i32,
    /// `DAT_00551b4c`/`DAT_00551b54`: the prompt's "yes" and "no" states.
    on_yes: u8,
    on_no: u8,
    /// The open prompt (`DAT_00bb3440`) and its selection (`DAT_00bb348c`).
    prompt: Option<(Prompt, u8)>,
    /// `DAT_00551b5c`.
    cursor: i8,
    /// `DAT_0055c248`/`DAT_0055c298`: each slot's list entry.
    entries: [Option<SlotEntry>; SAVE_SLOT_COUNT],
    /// `DAT_0055d74c`: the loaded slot records' validity, while loaded.
    loaded: Option<[bool; SAVE_SLOT_COUNT]>,
    /// `DAT_00550c20`: the change filter's last mask (a global the pause
    /// menu's filter shares; kept per dialog here).
    previous_mask: u32,
    /// `DAT_004ee4e8`: the state the draw saw last; the draw skips the
    /// frame a state changes.
    drawn: Cell<i32>,
}

impl Default for SaveDialog {
    fn default() -> Self {
        Self {
            state: 0,
            resume: 0,
            countdown: 0,
            message: 0,
            on_yes: 0,
            on_no: 0,
            prompt: None,
            cursor: 0,
            entries: Default::default(),
            loaded: None,
            previous_mask: 0,
            drawn: Cell::new(-1),
        }
    }
}

impl SaveDialog {
    /// `FUN_0043bfe0(record, yes, no)`.
    fn open(&mut self, prompt: Prompt, on_yes: u8, on_no: u8) {
        self.on_yes = on_yes;
        self.on_no = on_no;
        self.state = 3;
        self.prompt = Some((prompt, u8::from(!prompt.default_yes)));
    }

    /// `FUN_0043bf70(next, seconds, text)`: a message for `seconds * 12`
    /// ticks (24 in slow motion), four ticks for 0, or until a confirm for a
    /// negative count.
    fn show(&mut self, next: u8, seconds: i32, text: i32, slow_motion: bool) {
        self.resume = next;
        self.message = text;
        self.countdown = match seconds {
            n if n > 0 => (i32::from(slow_motion) + 1) * 25 / 2 * n,
            0 => 4,
            n => n,
        };
        self.state = 2;
    }

    /// `FUN_0044eeb0`: free the loaded slots.
    fn free(&mut self) {
        self.loaded = None;
    }

    /// The host's answer to [`SaveEffect::LoadSlots`]: each slot file's
    /// bytes (`None` when absent). A file that is not exactly one record
    /// long, or fails the validity check, is a free slot.
    pub fn provide_slots(&mut self, files: &[Option<Vec<u8>>]) {
        let mut valid = [false; SAVE_SLOT_COUNT];
        for (slot, valid) in valid.iter_mut().enumerate() {
            let record = files.get(slot).and_then(Option::as_deref);
            *valid = record.is_some_and(slot_is_valid);
            self.entries[slot] = record.filter(|_| *valid).map(slot_entry);
        }
        self.loaded = Some(valid);
    }

    /// `FUN_0043bce0`: one tick over the player's held keys (descriptor
    /// bits). `false` when the dialog has ended.
    pub fn tick(&mut self, keys: u32, slow_motion: bool, effects: &mut Vec<SaveEffect>) -> bool {
        let input = if keys == self.previous_mask {
            0
        } else {
            self.previous_mask = keys;
            keys
        };
        let confirm = input & bits::CONFIRM != 0;
        match self.state {
            0 => self.open(SAVE_PROMPT, 5, 1),
            1 => {
                self.state = 0;
                self.drawn.set(-1);
                return false;
            }
            2 => {
                if self.countdown == -1 {
                    if confirm {
                        self.state = self.resume;
                    }
                } else {
                    self.countdown -= 1;
                    if self.countdown == 0 {
                        self.state = self.resume;
                    }
                }
            }
            3 => {
                if let Some((_, selected)) = &mut self.prompt {
                    if input & bits::UP != 0 && *selected > 0 {
                        *selected -= 1;
                    } else if input & bits::DOWN != 0 && *selected < 1 {
                        *selected += 1;
                    }
                    if confirm {
                        self.state = if *selected == 0 {
                            self.on_yes
                        } else {
                            self.on_no
                        };
                    }
                }
            }
            4 => self.show(5, 0, 0x51, slow_motion),
            5 => {
                effects.push(SaveEffect::LoadSlots);
                self.state = 6;
            }
            6 => {
                if input & bits::UP != 0 && self.cursor >= 1 {
                    self.cursor -= 1;
                } else if input & bits::DOWN != 0 && self.cursor < CANCEL_ROW {
                    self.cursor += 1;
                }
                if confirm {
                    let occupied = self
                        .loaded
                        .and_then(|valid| valid.get(self.cursor as usize).copied())
                        .unwrap_or(false);
                    self.free();
                    if self.cursor >= CANCEL_ROW {
                        self.show(0, 2, 0x4f, slow_motion);
                    } else if occupied {
                        self.open(OVERWRITE_PROMPT, 8, 9);
                    } else {
                        self.show(7, 0, 0x59, slow_motion);
                    }
                }
            }
            7 => {
                effects.push(SaveEffect::Write {
                    slot: self.cursor as u8,
                });
                self.show(1, 2, 0x49, slow_motion);
            }
            8 => self.show(7, 0, 0x59, slow_motion),
            9 => self.show(0, 2, 0x4f, slow_motion),
            _ => {}
        }
        true
    }

    /// `FUN_0043bc90`: what the draw shows this frame, or nothing on the
    /// frame the state changed.
    pub fn view(&self) -> Option<SaveView<'_>> {
        let state = i32::from(self.state);
        if self.drawn.replace(state) != state {
            return None;
        }
        match self.state {
            2 => Some(SaveView::Message {
                text: self.message,
                waits: self.countdown == -1,
            }),
            3 => self
                .prompt
                .map(|(prompt, selected)| SaveView::Prompt { prompt, selected }),
            6 => Some(SaveView::Slots {
                cursor: self.cursor,
                slots: &self.entries,
            }),
            _ => None,
        }
    }
}

/// `FUN_0044f210`: a record's summary — byte 0 the player's points over the
/// rallies run (the row of the 16-row table at `+0x150` whose flag byte
/// `+8` has bit 4 and a zero low nibble; `0xff` without one), byte 1 the
/// next rally (bits 0-2), the next stage (bits 3-5) and the difficulty (bits
/// 6-7), byte 2 the player's car (`+0x96`, low nibble).
pub fn slot_summary(record: &[u8]) -> [u8; 3] {
    let cursor = record[0x148];
    let mut rally = i32::from(cursor & 0xf);
    let mut stage = i32::from(cursor >> 4) + 1;
    let per_rally = if record[SLOT_MODE_OFFSET] == 0 {
        3
    } else {
        i32::from(record[0x145] & 0xf)
    };
    if stage >= per_rally {
        stage = 1;
        rally += 1;
    } else {
        stage += 1;
    }
    let row = (0..16).find(|row| {
        let flags = record[0x158 + row * 16];
        flags & 0x10 != 0 && flags & 0xf == 0
    });
    let points = row.map_or(0xff, |row| {
        (0..rally.max(0) as usize).fold(0u8, |sum, column| {
            sum.wrapping_add(record[0x150 + row * 16 + column])
        })
    });
    [
        points,
        (record[SLOT_MODE_OFFSET] << 6) | ((stage as u8 & 7) << 3) | (rally as u8 & 7),
        record[0x96] & 0xf,
    ]
}

fn slot_entry(record: &[u8]) -> SlotEntry {
    let name = &record[SLOT_PAYLOAD_OFFSET..SLOT_PAYLOAD_OFFSET + SLOT_PAYLOAD_LEN];
    let end = name.iter().position(|&b| b == 0).unwrap_or(name.len());
    SlotEntry {
        name: name[..end].to_vec(),
        summary: slot_summary(record),
    }
}

/// `strncpy(dst, src, n)` then `dst[n] = 0`.
fn prefix(text: &[u8], n: usize) -> &[u8] {
    &text[..text.len().min(n)]
}

/// `FUN_0044f3d0`: an occupied slot's line in the list — at 512 wide or
/// more `"%s - %s - %s - %s %s %d - "` (`0x4f12c4`) of the name, the car's
/// short name (`0x506e08`), the difficulty (`0x2f + d`), the rally
/// (`0x3c + r`), `0x25` and the stage; narrower `"%s - %s - %s - %s - %d - "`
/// (`0x4f12e0`) of the same cut to 3, 6, 6 and 11 bytes. Then the text
/// `0xdf` formatted with the points.
pub fn slot_label(
    entry: &SlotEntry,
    wide: bool,
    text: &dyn Fn(i32) -> Vec<u8>,
    car_short_names: &[Vec<u8>],
) -> Vec<u8> {
    let [points, progress, car] = entry.summary;
    let car = car_short_names
        .get(usize::from(car & 0xf))
        .cloned()
        .unwrap_or_default();
    let difficulty = text(0x2f + i32::from(progress >> 6));
    let rally = text(0x3c + i32::from(progress & 7));
    let stage = i64::from((progress >> 3) & 7);
    let mut line = if wide {
        c_format(
            b"%s - %s - %s - %s %s %d - ",
            &[
                FormatArg::Bytes(&entry.name),
                FormatArg::Bytes(&car),
                FormatArg::Bytes(&difficulty),
                FormatArg::Bytes(&rally),
                FormatArg::Bytes(&text(0x25)),
                FormatArg::Int(stage),
            ],
        )
    } else {
        c_format(
            b"%s - %s - %s - %s - %d - ",
            &[
                FormatArg::Bytes(prefix(&entry.name, 3)),
                FormatArg::Bytes(prefix(&car, 6)),
                FormatArg::Bytes(prefix(&difficulty, 6)),
                FormatArg::Bytes(prefix(&rally, 11)),
                FormatArg::Int(stage),
            ],
        )
    };
    line.extend(c_format(&text(0xdf), &[FormatArg::Int(i64::from(points))]));
    line
}

/// `FUN_0044ef50`'s record: the live block's `+0x2c..+0x6bc` as the payload,
/// `+0x20..+0x2a` after it, the player-0 controls window zeroed, the magic,
/// the length, the difficulty (`(live[0x18] >> 3) & 3`) and the seed with its
/// complement. `seed` is [`crate::profile_store::make_seed`] of three
/// `rand()` draws. The never-written byte `0x6a7` is 0 here (the native
/// allocation is not cleared).
///
/// # Panics
///
/// If `live` is shorter than `0x6bc` bytes.
pub fn build_save_record(live: &[u8], seed: u32) -> Vec<u8> {
    let mut record = vec![0u8; SAVE_SLOT_LEN];
    record[SLOT_PAYLOAD_OFFSET..SLOT_PAYLOAD_OFFSET + SLOT_PAYLOAD_LEN]
        .copy_from_slice(&live[0x2c..0x2c + SLOT_PAYLOAD_LEN]);
    record[0x69c..0x6a6].copy_from_slice(&live[0x20..0x2a]);
    record[SLOT_VOLATILE_ZERO_OFFSET..SLOT_VOLATILE_ZERO_OFFSET + SLOT_VOLATILE_ZERO_LEN].fill(0);
    record[SLOT_MAGIC_OFFSET..SLOT_MAGIC_OFFSET + 4].copy_from_slice(&SLOT_MAGIC.to_le_bytes());
    record[SLOT_FORMAT_OFFSET..SLOT_FORMAT_OFFSET + 4]
        .copy_from_slice(&SLOT_FORMAT_MARKER.to_le_bytes());
    record[SLOT_MODE_OFFSET] = (live[0x18] >> 3) & 3;
    record[SLOT_SEED_OFFSET..SLOT_SEED_OFFSET + 4].copy_from_slice(&seed.to_le_bytes());
    record[SLOT_COMPLEMENT_OFFSET..SLOT_COMPLEMENT_OFFSET + 4]
        .copy_from_slice(&(!seed).to_le_bytes());
    record
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(dialog: &mut SaveDialog, keys: &[u32]) -> Vec<SaveEffect> {
        let mut effects = Vec::new();
        for &mask in keys {
            assert!(dialog.tick(mask, false, &mut effects));
        }
        effects
    }

    fn live() -> Vec<u8> {
        let mut live = vec![0u8; 0x6c4];
        for (i, b) in live.iter_mut().enumerate() {
            *b = i as u8;
        }
        live[0x18] = 0x10;
        live
    }

    #[test]
    fn yes_then_a_free_slot_saves_and_returns() {
        let mut dialog = SaveDialog::default();
        // 0 opens the prompt with "yes" selected; confirm.
        let effects = run(&mut dialog, &[0, bits::CONFIRM]);
        assert!(effects.is_empty());
        assert_eq!(dialog.state, 5);
        let effects = run(&mut dialog, &[0]);
        assert_eq!(effects, vec![SaveEffect::LoadSlots]);
        dialog.provide_slots(&vec![None; 12]);
        // Down twice, confirm: slot 2 is free, so "saving" for four ticks.
        run(&mut dialog, &[bits::DOWN, 0, bits::DOWN, bits::CONFIRM]);
        assert_eq!((dialog.state, dialog.cursor, dialog.countdown), (2, 2, 4));
        assert!(dialog.loaded.is_none(), "the slots were freed");
        let effects = run(&mut dialog, &[0, 0, 0, 0, 0]);
        assert_eq!(effects, vec![SaveEffect::Write { slot: 2 }]);
        // "Saved" for 24 ticks, then the dialog ends.
        assert_eq!(
            (dialog.state, dialog.countdown, dialog.message),
            (2, 24, 0x49)
        );
        run(&mut dialog, &[0; 24]);
        assert_eq!(dialog.state, 1);
        assert!(!dialog.tick(0, false, &mut Vec::new()));
        assert_eq!(dialog.state, 0);
        assert_eq!(dialog.cursor, 2, "the cursor is kept for the next save");
    }

    #[test]
    fn no_ends_and_an_occupied_slot_asks_to_overwrite() {
        let mut dialog = SaveDialog::default();
        run(&mut dialog, &[0, bits::DOWN, bits::CONFIRM]);
        assert_eq!(dialog.state, 1, "no");
        let mut dialog = SaveDialog::default();
        run(&mut dialog, &[0, bits::CONFIRM, 0]);
        let mut record = build_save_record(&live(), 0xaa12_3456);
        record[0x148] = 0x12;
        let mut files = vec![None; 12];
        files[0] = Some(record);
        dialog.provide_slots(&files);
        assert!(dialog.entries[0].is_some() && dialog.entries[1].is_none());
        run(&mut dialog, &[bits::CONFIRM]);
        assert_eq!(dialog.prompt, Some((OVERWRITE_PROMPT, 1)), "no first");
        run(&mut dialog, &[0, bits::CONFIRM]);
        assert_eq!(dialog.state, 9);
        run(&mut dialog, &[0]);
        // "Not saved" for 24 ticks, then the save prompt again.
        assert_eq!((dialog.state, dialog.message, dialog.resume), (2, 0x4f, 0));
    }

    #[test]
    fn the_cursor_stops_at_the_ends_and_cancel_asks_again() {
        let mut dialog = SaveDialog::default();
        run(&mut dialog, &[0, bits::CONFIRM, 0]);
        dialog.provide_slots(&vec![None; 12]);
        run(&mut dialog, &[bits::UP]);
        assert_eq!(dialog.cursor, 0);
        for _ in 0..20 {
            run(&mut dialog, &[bits::DOWN, 0]);
        }
        assert_eq!(dialog.cursor, CANCEL_ROW);
        run(&mut dialog, &[bits::CONFIRM]);
        assert_eq!((dialog.state, dialog.message), (2, 0x4f));
    }

    #[test]
    fn held_keys_repeat_only_on_change_and_the_draw_skips_a_changed_state() {
        let mut dialog = SaveDialog::default();
        run(&mut dialog, &[0]);
        assert!(dialog.view().is_none(), "the frame the state changed");
        assert!(matches!(
            dialog.view(),
            Some(SaveView::Prompt { selected: 0, .. })
        ));
        run(&mut dialog, &[bits::DOWN, bits::DOWN]);
        assert_eq!(dialog.prompt.unwrap().1, 1);
        // A slow-motion message lasts twice as long.
        dialog.show(0, 2, 0x4f, true);
        assert_eq!(dialog.countdown, 50);
        dialog.show(0, -1, 0x4f, false);
        assert_eq!(dialog.countdown, -1);
    }

    #[test]
    fn the_record_and_its_summary() {
        let record = build_save_record(&live(), 0xaa00_0001);
        assert!(slot_is_valid(&record));
        assert_eq!(record[0x0c], 0x2c);
        assert_eq!(record[0x1c..0x70], [0; 0x54]);
        assert_eq!(record[0x70], (0x2c + 0x64) as u8);
        assert_eq!(record[0x69c..0x6a6], live()[0x20..0x2a]);
        assert_eq!(record[0x6a6], 2);
        let mut record = record;
        // Rally 2, stage 0 done of 3 (difficulty set: count at 0x145).
        record[0x148] = 0x02;
        record[0x145] = 3;
        for row in 0..16 {
            record[0x158 + row * 16] = 0;
        }
        record[0x158 + 5 * 16] = 0x10;
        record[0x150 + 5 * 16..0x150 + 5 * 16 + 3].copy_from_slice(&[10, 6, 4]);
        record[0x96] = 0x37;
        assert_eq!(slot_summary(&record), [16, (2 << 6) | (2 << 3) | 2, 7]);
        // The last stage moves to the next rally; no player row → 0xff.
        record[0x148] = 0x22;
        record[0x158 + 5 * 16] = 0x11;
        assert_eq!(slot_summary(&record), [0xff, (2 << 6) | (1 << 3) | 3, 7]);
    }

    #[test]
    fn the_labels() {
        let entry = SlotEntry {
            name: b"COLIN".to_vec(),
            summary: [16, (1 << 6) | (2 << 3) | 3, 1],
        };
        let text = |id: i32| match id {
            0xdf => b"%d PTS".to_vec(),
            id => format!("T{id:X}").into_bytes(),
        };
        let cars = vec![b"IMPREZA".to_vec(), b"CELICA GT".to_vec()];
        assert_eq!(
            slot_label(&entry, true, &text, &cars),
            b"COLIN - CELICA GT - T30 - T3F T25 2 - 16 PTS".to_vec()
        );
        assert_eq!(
            slot_label(&entry, false, &text, &cars),
            b"COL - CELICA - T30 - T3F - 2 - 16 PTS".to_vec()
        );
    }
}
