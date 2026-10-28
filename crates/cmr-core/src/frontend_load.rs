//! The front end's Load screen `0x00d40da0` (root item 5): twelve save-slot
//! actions and Back, built by `FUN_004be540` (no wrap, the setups' sound
//! table `0xd90060`, camera target `0x507aa0`).
//!
//! - enter `0x4c74e0` reads the slots (`FUN_0044ed90`), keeps each occupied
//!   slot's record, summary (`FUN_0044f210`) and file-time label
//!   (`FUN_0044f2f0`: the file's modification time through `ctime` and
//!   `_strupr`, or `"EMPTY SLOT"`), starts the info slide `0xd8d75f` (15
//!   steps) and the panel slide `0xd8d794` (20 steps), and mutes the table's
//!   move sound when no slot is occupied;
//! - commit `0x4c76c0` (every step) requests the selected slot's car palette
//!   (`FUN_004c4920(car, 4)`), or for Back drops the current palette and
//!   requests the chequered flag's (`FUN_004c4920(9, 1)`), moves the info
//!   anchor to `(-0xe083 - info, 0xa000)` and the panel to
//!   `(0xf5c - panel, 0x51e)`, and enables the navigation;
//! - post `0x4c7770` slides the panel out and restores the move sound;
//! - activate `0x4c7b40` loads an occupied slot into the record, commits it,
//!   writes the championship state into the live block and starts the race
//!   (`FUN_004af3a0(0xd437c8)`).
//!
//! A description of disassembly, not original source. Evidence:
//! `rea.official-eu.frontend-load`.

use crate::config::LEGACY_CONFIG_SIZE;
use crate::descriptor_menu::{ItemKind, Menu, MenuId, MenuItem, MenuNav};
use crate::profile_store::slot_is_valid;
use crate::save_dialog::slot_summary;
use crate::scalar_tween::ScalarTween;
use crate::settings_store::SAVE_SLOT_COUNT;

type Record = [u8; LEGACY_CONFIG_SIZE];

/// The first slot's key (its draw also draws the panel).
pub const FIRST_SLOT_KEY: i32 = 0x63;
/// Back's key.
pub const BACK_KEY: i32 = 8;
/// The info anchor's x once the slide has ended (`0xffff1f7d`).
pub const INFO_X: i32 = -0xe083;

/// A broken-down file time (`localtime` of the slot's modification time).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileTime {
    pub year: i32,
    /// 1..=12.
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    /// 0 is Sunday.
    pub weekday: u8,
}

impl FileTime {
    /// The civil time of `secs` seconds after the Unix epoch.
    pub fn from_unix(secs: i64) -> Self {
        let days = secs.div_euclid(86_400);
        let rem = secs.rem_euclid(86_400);
        // Howard Hinnant's civil-from-days.
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + i64::from(month <= 2);
        Self {
            year: year as i32,
            month: month as u8,
            day: day as u8,
            hour: (rem / 3600) as u8,
            minute: (rem / 60 % 60) as u8,
            second: (rem % 60) as u8,
            weekday: (days + 4).rem_euclid(7) as u8,
        }
    }
}

/// `FUN_0044f2f0`: `ctime` of the file time, upper-cased (newline kept), or
/// `"EMPTY SLOT"` when the file cannot be opened.
pub fn file_time_label(time: Option<FileTime>) -> Vec<u8> {
    const DAYS: [&str; 7] = ["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"];
    const MONTHS: [&str; 12] = [
        "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
    ];
    let Some(t) = time else {
        return b"EMPTY SLOT".to_vec();
    };
    format!(
        "{} {}{:3} {:02}:{:02}:{:02} {}\n",
        DAYS[usize::from(t.weekday % 7)],
        MONTHS[usize::from(t.month.clamp(1, 12) - 1)],
        t.day,
        t.hour,
        t.minute,
        t.second,
        t.year
    )
    .into_bytes()
}

/// One slot file as the host reads it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SlotFile {
    /// The file's bytes, `None` when absent.
    pub bytes: Option<Vec<u8>>,
    /// Its modification time, `None` when it cannot be opened.
    pub modified: Option<FileTime>,
}

/// An occupied slot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadSlot {
    pub record: Vec<u8>,
    /// `DAT_00d8cf00 + 0x64 i`: the file-time label, passed through
    /// `sprintf` as its own format.
    pub label: Vec<u8>,
}

impl LoadSlot {
    /// The player name at record `+0xc`.
    pub fn name(&self) -> &[u8] {
        crate::save_dialog::record_name(&self.record)
    }
}

/// The Load screen's state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadScreen {
    pub menu: Menu,
    /// `DAT_00d8d6d0`/`DAT_00d8d704`: the occupied slots.
    slots: [Option<LoadSlot>; SAVE_SLOT_COUNT],
    /// `DAT_00d8d738`: each slot's summary, kept from earlier entries for a
    /// free one.
    summaries: [[u8; 3]; SAVE_SLOT_COUNT],
    /// `DAT_00d8d75f`, `DAT_00d8d794`.
    info_slide: Option<ScalarTween>,
    panel_slide: Option<ScalarTween>,
    /// `DAT_00d90038/c`: the info anchor.
    pub info: (i32, i32),
    /// `DAT_00d8cef8/c`: the panel.
    pub panel: (i32, i32),
    /// `DAT_00d90060[0]`: the move sound plays (some slot is occupied).
    pub move_sound: bool,
}

impl LoadScreen {
    /// The menu as `FUN_004be540` builds it: slots 1..=12 (actions with
    /// activate `0x4c7b40`; the first keyed `0x63`), then Back (text 10, key
    /// 8), all drawn by `0x4c77d0`; no wrap.
    pub fn build(root: MenuId) -> Self {
        let mut menu = Menu::new(5, Some(root), Some(crate::frontend_sounds::SETUP_SOUNDS));
        menu.wrap = false;
        menu.on_enter = Some(0x004c_74e0);
        menu.on_commit = Some(0x004c_76c0);
        menu.on_predraw = Some(0x004c_77b0);
        menu.on_post = Some(0x004c_7770);
        menu.on_leave = Some(0x004c_7600);
        for slot in 0..SAVE_SLOT_COUNT {
            let mut item = MenuItem::new(ItemKind::Action, -1, 0x004c_77d0);
            item.key = if slot == 0 { FIRST_SLOT_KEY } else { 0 };
            item.activate = Some(0x004c_7b40);
            menu.items.push(item);
        }
        let mut back = MenuItem::new(ItemKind::Back, 10, 0x004c_77d0);
        back.key = BACK_KEY;
        menu.items.push(back);
        Self {
            menu,
            slots: Default::default(),
            summaries: [[0; 3]; SAVE_SLOT_COUNT],
            info_slide: None,
            panel_slide: None,
            info: (0, 0),
            panel: (0, 0),
            move_sound: false,
        }
    }

    /// The slot behind item `index`, when occupied.
    pub fn slot(&self, index: usize) -> Option<&LoadSlot> {
        self.slots.get(index).and_then(Option::as_ref)
    }

    /// The summary of slot `index` (`DAT_00d8d738 + 3 i`).
    pub fn summary(&self, index: usize) -> [u8; 3] {
        self.summaries.get(index).copied().unwrap_or([0; 3])
    }

    /// `0x4c74e0`. `no_animation` is `DAT_00bbabec`.
    pub fn enter(&mut self, files: &[SlotFile], no_animation: bool) {
        let mut count = 0;
        for slot in 0..SAVE_SLOT_COUNT {
            let file = files.get(slot);
            let record = file
                .and_then(|file| file.bytes.as_deref())
                .filter(|bytes| slot_is_valid(bytes));
            self.slots[slot] = record.map(|record| {
                count += 1;
                self.summaries[slot] = slot_summary(record);
                let label = file_time_label(file.and_then(|file| file.modified));
                LoadSlot {
                    record: record.to_vec(),
                    label: crate::c_format::c_format(&label, &[]),
                }
            });
        }
        let slide = |from, to, steps| {
            if no_animation {
                ScalarTween::ease_out(to, to, steps, 0)
            } else {
                ScalarTween::ease_out(from, to, steps, 0)
            }
        };
        self.info_slide = Some(slide(0x10000, 0, 15));
        self.panel_slide = Some(slide(0x10000, 0, 20));
        self.move_sound = count > 0;
        self.menu.nav = MenuNav::NONE;
    }

    /// `0x4c76c0`'s slides and navigation.
    pub fn commit(&mut self) {
        if let Some(slide) = self.info_slide.as_mut()
            && !slide.is_finished()
        {
            self.info = (INFO_X.wrapping_sub(slide.evaluate_and_advance()), 0xa000);
        }
        if let Some(slide) = self.panel_slide.as_mut()
            && !slide.is_finished()
        {
            self.panel = (0xf5c - slide.evaluate_and_advance(), 0x51e);
        }
        self.menu.nav = MenuNav::ALL;
    }

    /// `0x4c76c0`'s palette request for the selected item: the arena index
    /// and whether the current palette is dropped first (Back).
    pub fn palette(&self) -> (i32, bool) {
        use crate::frontend_screen_bitmaps::palette;
        if self.menu.items.get(self.menu.selected).map(|item| item.key) == Some(BACK_KEY) {
            (palette::FLAGS + 9, true)
        } else {
            let car = self.summary(self.menu.selected)[2] & 0xf;
            (palette::CARS + i32::from(car), false)
        }
    }

    /// `0x4c7770`: the panel slides out (an ease-out over 15 steps), the
    /// move sound returns and the navigation stops.
    pub fn post(&mut self, no_animation: bool) {
        self.panel_slide = Some(if no_animation {
            ScalarTween::ease_out(0x10000, 0x10000, 15, 0)
        } else {
            ScalarTween::ease_out(0, 0x10000, 15, 0)
        });
        self.move_sound = true;
        self.menu.nav = MenuNav::NONE;
    }

    /// The item draw's side effect: a free slot is disabled (Back stays
    /// enabled).
    pub fn draw_effects(&mut self) {
        for (index, item) in self.menu.items.iter_mut().enumerate() {
            item.enabled =
                item.key == BACK_KEY || self.slots.get(index).is_some_and(Option::is_some);
        }
    }

    /// `0x4c7b40`: load slot `index` into the record (the front end's part;
    /// the host then commits it and applies [`apply_to_live`]). `false` for a
    /// free slot, which does nothing.
    pub fn activate(&self, index: usize, record: &mut Record) -> bool {
        let Some(slot) = self.slot(index) else {
            return false;
        };
        let rec = &slot.record;
        let controls: [u8; 0x54] = record[0x44..0x98].try_into().expect("0x54 bytes");
        record[0x34..0x16c].copy_from_slice(&rec[0x0c..0x144]);
        record[0x44..0x98].copy_from_slice(&controls);
        let at = 0x16c + usize::from(rec[0x148] & 0xf) * 2;
        record[at..at + 2].copy_from_slice(&rec[0x144..0x146]);
        record[0x180] = (record[0x180] & !3) | (rec[0x146] & 3);
        record[0x180] = (record[0x180] & 3) | (rec[0x147] << 2);
        record[0x181] = (record[0x181] & 0xf0) | (rec[0x148] & 0xf);
        record[0x181] = (rec[0x148] & 0xf0) | (record[0x181] & 0xf);
        record[0x182] = (record[0x182] & 0xf8) | ((rec[0x146] >> 2) & 7);
        record[0x182] = (record[0x182] & 0xf7) | ((rec[0x146] >> 3) & 8);
        record[0x182] = (record[0x182] & 0xef) | ((rec[0x146] >> 1) & 0x10);
        true
    }
}

/// `0x4c7b40` after the commit `FUN_004b93f0`: the championship table
/// `live[0x16c..0x26c)` from the record, the advance flag `0x80` of
/// `live[0x166]`, the events run (`live[0x167]` low six bits) and the
/// difficulty (`live[0x18]` bits 3-4).
pub fn apply_to_live(live: &mut [u8], record: &[u8]) {
    live[0x16c..0x26c].copy_from_slice(&record[0x14c..0x24c]);
    live[0x166] |= 0x80;
    live[0x167] = (live[0x167] & 0xc0) | (record[0x147] & 0x3f);
    live[0x18] = (live[0x18] & 0xe7) | ((record[0x6a6] & 3) << 3);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::save_dialog::build_save_record;

    fn record() -> Vec<u8> {
        let mut live = vec![0u8; 0x6c4];
        live[0x2c..0x31].copy_from_slice(b"COLIN");
        live[0x18] = 0x10;
        let mut record = build_save_record(&live, 0xaa00_0007);
        record[0x96] = 0x35;
        record[0x144..0x146].copy_from_slice(&[0x34, 0x12]);
        record[0x146] = 0x65; // mode 1, players 1, bit 5 and bit 6
        record[0x147] = 0x0a;
        record[0x148] = 0x23;
        record[0x14c] = 0x77;
        record
    }

    #[test]
    fn ctime_labels() {
        assert_eq!(file_time_label(None), b"EMPTY SLOT");
        // 2026-10-28 08:14:34 UTC, a Wednesday.
        let t = FileTime::from_unix(1_793_175_274);
        assert_eq!(
            (
                t.year, t.month, t.day, t.hour, t.minute, t.second, t.weekday
            ),
            (2026, 10, 28, 8, 14, 34, 3)
        );
        assert_eq!(file_time_label(Some(t)), b"WED OCT 28 08:14:34 2026\n");
        let t = FileTime::from_unix(0);
        assert_eq!(file_time_label(Some(t)), b"THU JAN  1 00:00:00 1970\n");
    }

    #[test]
    fn enter_keeps_the_occupied_slots_and_their_summaries() {
        let mut screen = LoadScreen::build(MenuId(0));
        assert_eq!(screen.menu.items.len(), 13);
        assert!(!screen.menu.wrap);
        let mut files = vec![SlotFile::default(); 12];
        files[4] = SlotFile {
            bytes: Some(record()),
            modified: Some(FileTime::from_unix(0)),
        };
        files[5].bytes = Some(vec![0; 3]);
        screen.enter(&files, false);
        assert!(screen.move_sound);
        assert!(screen.slot(4).is_some() && screen.slot(5).is_none());
        assert_eq!(screen.slot(4).unwrap().name(), b"COLIN");
        assert_eq!(screen.summary(4)[2], 5);
        screen.draw_effects();
        assert!(screen.menu.items[4].enabled && !screen.menu.items[0].enabled);
        assert!(screen.menu.items[12].enabled, "back");
        // The commit slides the anchors in; the info box ends at -0xe083.
        screen.commit();
        assert_eq!(screen.menu.nav, MenuNav::ALL);
        for _ in 0..30 {
            screen.commit();
        }
        assert_eq!(
            (screen.info, screen.panel),
            ((INFO_X, 0xa000), (0xf5c, 0x51e))
        );
        screen.menu.selected = 4;
        assert_eq!(
            screen.palette(),
            (crate::frontend_screen_bitmaps::palette::CARS + 5, false)
        );
        screen.menu.selected = 12;
        assert_eq!(
            screen.palette(),
            (crate::frontend_screen_bitmaps::palette::FLAGS + 9, true)
        );
        // No occupied slot mutes the move sound.
        screen.enter(&vec![SlotFile::default(); 12], true);
        assert!(!screen.move_sound);
        assert_eq!(screen.summary(4)[2], 5, "kept from the last entry");
    }

    #[test]
    fn activate_loads_the_record_and_the_live_block() {
        let mut screen = LoadScreen::build(MenuId(0));
        let mut files = vec![SlotFile::default(); 12];
        files[0].bytes = Some(record());
        screen.enter(&files, true);
        let mut img = [0u8; LEGACY_CONFIG_SIZE];
        img[0x44] = 0x99; // player-0 controls survive
        img[0x180] = 0xfc;
        img[0x182] = 0xff;
        assert!(!screen.activate(1, &mut img));
        assert!(screen.activate(0, &mut img));
        assert_eq!(&img[0x34..0x39], b"COLIN");
        assert_eq!(img[0x44], 0x99);
        assert_eq!(img[0x16c + 3 * 2..0x16c + 3 * 2 + 2], [0x34, 0x12]);
        assert_eq!(img[0x180], (0x0a << 2) | 1);
        assert_eq!(img[0x181], 0x23, "the whole cursor byte");
        assert_eq!(img[0x182], 0xe0 | 1 | 8 | 0x10);
        let mut live = vec![0u8; 0x6c4];
        live[0x167] = 0xc5;
        live[0x18] = 0xff;
        apply_to_live(&mut live, &record());
        assert_eq!(live[0x16c], 0x77);
        assert_eq!(live[0x166], 0x80);
        assert_eq!(live[0x167], 0xca);
        assert_eq!(live[0x18], 0xe7 | (2 << 3));
    }
}
