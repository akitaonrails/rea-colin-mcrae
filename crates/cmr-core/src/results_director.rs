//! The post-stage results director `FUN_00430770`, which FSM state 8 runs
//! every tick until it switches itself off (`DAT_00550024`), and its init
//! `FUN_0042f860` at the finish.
//!
//! Single-player pages, by their native ids: 1 stage results, 2 rally
//! results, 3 championship standings, 4 the post-stage menu (restart,
//! continue, quit), 6 the super-special result, 8 GAME OVER, 9/10 the quit
//! and restart confirmations, 0x12 the bonus car, 0x14 championship complete,
//! 0x15 rally complete. Every page change waits five ticks (`DAT_0054fc5c`)
//! and plays the menu cue. Return advances every page; left/right move the
//! menu cursor; the confirmations are two-item vertical menus defaulting to
//! "no" (both shipped dialog records hold 0 at `+0x58`).
//!
//! Pages 1, 2 and 3 confirm through the stage, rally and championship
//! record checks when the host supplies the record image ([`RecordRun`]):
//! a record goes to its notice page (0xc, 0xd, 0xe), a CAC competition
//! entry first to its code page (0xf, 0x10, 0x11), and each then to the
//! usual continuation. Without the image the checks are skipped (harness).
//! Page 5 runs the save dialog ([`crate::save_dialog`]) every tick until it
//! ends. Not reconstructed: network and head-to-head pages. A description
//! of disassembly, not original source.
//! Evidence: `rea.official-eu.results-director`.

use crate::config::LEGACY_CONFIG_SIZE;
use crate::config_commit::{live, live_mut};
use crate::event_flow::{EventState, SPECIAL_COUNTRIES};
use crate::hiscore_codes::{RecordKind, record_code};
use crate::standings::last_stage_of_rally;

/// One player's finish as the record checks read it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlayerFinish {
    /// The player's result slot (`DAT_0057af94[p]`).
    pub slot: usize,
    /// The stage time (`DAT_0054ff88[slot]`, hundredths).
    pub stage_time: i32,
    /// The seven split clocks (`0x5ffa6c + 0x24 p`; the stage record keeps
    /// their low halves).
    pub splits: [u32; 7],
}

/// What the record checks need from the host: the cmRally.cfg image (the
/// live block inside it is the race's), the code key schedule, each
/// player's finish, and the record salt `DAT_0055002c` the init draws
/// ([`record_salt`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordRun {
    pub image: Box<[u8; LEGACY_CONFIG_SIZE]>,
    pub schedule: Vec<u8>,
    pub players: Vec<PlayerFinish>,
    pub salt: u8,
}

/// `FUN_0042f860`'s salt from one `rand()` draw scaled to Q16
/// ([`crate::game_rng::GameLcg::next_q16`]): `(q16 * 63) >> 16`, 0..=62.
pub fn record_salt(q16: i32) -> u8 {
    (q16.wrapping_mul(63) >> 16) as u8
}

/// The record pages' globals, which outlive one director (none is reset by
/// the init): carry them with [`ResultsDirector::with_record_globals`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecordGlobals {
    /// `DAT_0054fdc9`: the stage records set; `DAT_0054fdc8` the last
    /// player who set one.
    pub stage_count: u8,
    pub stage_player: u8,
    /// `DAT_0054fe54`: the rally records set; `DAT_0054fe4c` the players
    /// (`[0]` the one shown); `DAT_0054fc44` their places (-1 for none).
    pub rally_count: u8,
    pub rally_shown: [u8; 2],
    pub rally_places: [i32; 2],
    /// `DAT_0054fc4c` and `DAT_0054fee0`: the championship place and
    /// whether it was a record.
    pub championship_place: i32,
    pub championship_recorded: bool,
    /// The six code buffers: the stage record `0x54fc04` and CAC `0x54ffd8`,
    /// the rally `0x54fb44` and `0x54fc20`, the championship `0x54fbdc` and
    /// `0x54fbf0`.
    pub stage_code: Vec<u8>,
    pub stage_cac: Vec<u8>,
    pub rally_code: Vec<u8>,
    pub rally_cac: Vec<u8>,
    pub championship_code: Vec<u8>,
    pub championship_cac: Vec<u8>,
}

/// `FUN_004ba5b0(live + 0x20)`: no cheat is on (helium, night and weather
/// are allowed).
fn no_cheats(live: &[u8]) -> bool {
    live[0x24] & 0x40 == 0 && live[0x25] & 0x3f == 0 && live[0x26] & 0x9f == 0
}

/// The CAC competition number `live.u16[0x20]`.
fn cac_number(live: &[u8]) -> u16 {
    u16::from_le_bytes([live[0x20], live[0x21]])
}

/// Player `p`'s name in the live block (`live + 0x2c + 0x9c p`, to its NUL).
pub fn player_name(live: &[u8], player: usize) -> Vec<u8> {
    let name = &live[0x2c + 0x9c * player..0x2c + 0x9c * (player + 1)];
    name[..name.iter().position(|&c| c == 0).unwrap_or(name.len())].to_vec()
}

/// One tick's input (`FUN_00430980`): fresh presses, and Up/Down held
/// (`DAT_0054fff0`/`DAT_0054fff4`) for the scrolling tables.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DirectorInput {
    pub confirm: bool,
    pub left: bool,
    pub right: bool,
    pub up: bool,
    pub down: bool,
    pub up_held: bool,
    pub down_held: bool,
    /// The player's held keys as descriptor bits, which the save dialog's
    /// change filter `FUN_0043baa0` reads.
    pub keys: u32,
    /// `DAT_00551af0`: slow motion doubles the dialog's message times.
    pub slow_motion: bool,
}

/// What the race was.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DirectorContext {
    /// `FUN_00485bb0`.
    pub super_special: bool,
    /// `DAT_00550014`.
    pub disqualified: bool,
    /// Each player's stage, rally and championship position (1-based).
    pub stage_position: usize,
    pub rally_position: usize,
    pub championship_position: usize,
    /// The save schedule `0x4ed8e4` (one byte per country), when extracted.
    pub save_schedule: Option<[u8; 9]>,
    /// The display height (`*(DAT_00502518 + 4)`), which sets the table's
    /// row pitch.
    pub screen_height: u16,
}

/// A host effect of one tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectorEffect {
    /// The menu cue `FUN_00467ef0(0, 0, 1)` on a page change.
    PageCue,
    /// `FUN_00467dd0`: the cursor moved (`true`) or hit an end (`false`).
    CursorCue(bool),
    /// The bonus car was awarded (`FUN_00404310(2, car 0)`).
    BonusCar,
    /// The save dialog's slot I/O.
    Save(crate::save_dialog::SaveEffect),
}

/// The results director's state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResultsDirector {
    /// `DAT_0054fc40`.
    page: u8,
    /// `DAT_0054fc5c`.
    settle: u8,
    /// `DAT_0054ff81..83` and `DAT_0054ff84` (cursor high nibble, count low).
    menu_items: [u8; 3],
    menu: u8,
    /// The open yes/no confirmation's (`FUN_004aabd0`) selected item: 0 is
    /// "yes".
    dialog: Option<u8>,
    context: DirectorContext,
    /// `DAT_0054fc3c`: the table rows, 16 with a difficulty set and 8 without.
    rows: u8,
    scroll: Scroll,
    /// The save dialog (its globals outlive one director: carry it with
    /// [`ResultsDirector::with_save_dialog`]).
    save: crate::save_dialog::SaveDialog,
    records: Option<RecordRun>,
    record_globals: RecordGlobals,
}

/// The results tables' scroll: the first shown row `DAT_0054ffcc`, the
/// pixel offset `DAT_0054fc3e` (animating towards `-pitch * index`), the
/// tick's starting offset `DAT_0054ffc8` and the down/up animations
/// `DAT_0055000c`/`DAT_00550010`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Scroll {
    pub index: i32,
    pub offset: i16,
    pub start: i16,
    down: bool,
    up: bool,
}

/// `-(H * 37 / 480)`: one row up.
fn negative_pitch(height: i32) -> i32 {
    -height.wrapping_mul(37) / 480
}

impl Scroll {
    /// `FUN_0042fa40(position)`: show the row five above `position`, within
    /// the table.
    fn centre_on(&mut self, position: i32, rows: u8, height: i32) {
        let mut index = position - 5;
        if index < 0 {
            index = 0;
        } else if index > i32::from(rows) - 8 {
            index = i32::from(rows) - 8;
        }
        self.index = index;
        self.offset = negative_pitch(height).wrapping_mul(index) as i16;
        self.start = self.offset;
    }

    /// `FUN_00430b30(max)`: held Down/Up start a one-row scroll, which moves
    /// `5 * H / 480` pixels a tick until it lands.
    fn step(&mut self, max: i32, input: DirectorInput, height: i32) {
        self.start = self.offset;
        let step = height.wrapping_mul(5) / 480;
        let target = |index: i32| negative_pitch(height).wrapping_mul(index);
        let mut down = self.down;
        if !self.down && !self.up {
            if self.index < max && input.down_held {
                down = true;
                self.down = true;
                self.index += 1;
            }
            if self.index > 0 && input.up_held {
                self.index -= 1;
                self.up = true;
            }
        }
        if down {
            self.offset = self.offset.wrapping_sub(step as i16);
            if i32::from(self.offset) <= target(self.index) {
                self.offset = target(self.index) as i16;
                self.down = false;
            }
        }
        if self.up {
            self.offset = self.offset.wrapping_add(step as i16);
            if i32::from(self.offset) >= target(self.index) {
                self.offset = target(self.index) as i16;
                self.up = false;
            }
        }
    }

    /// `FUN_00430480(t)`: the drawn offset `DAT_0054f9f0` between this
    /// tick's start and end, `t` in Q16, rounded half up.
    #[must_use]
    pub fn drawn(&self, t_q16: i32) -> i16 {
        let start = i32::from(self.start) << 16;
        let delta = (i32::from(self.offset) << 16).wrapping_sub(start);
        let value = ((i64::from(t_q16) * i64::from(delta)) >> 16) as i32 + start;
        let whole = value >> 16;
        (if value - (whole << 16) >= 0x8000 {
            whole + 1
        } else {
            whole
        }) as i16
    }
}

/// The post-stage menu items.
pub mod item {
    pub const RESTART: u8 = 0;
    pub const CONTINUE: u8 = 1;
    pub const QUIT: u8 = 2;
}

impl ResultsDirector {
    /// `FUN_0042f860` (single player, not network or head-to-head): the event
    /// may continue (`DAT_004ed7d8 = 1`), the first page is the stage results
    /// (the super-special result outside a time trial), and the menu offers
    /// restart, continue and quit with continue selected; a disqualification
    /// drops quit and opens on GAME OVER.
    pub fn start(event: &mut EventState, context: DirectorContext) -> Self {
        event.globals.continues_4ed7d8 = true;
        let mut page = if context.super_special && event.cfg.mode_166 & 3 != 2 {
            6
        } else {
            1
        };
        let mut menu = 0x13;
        if context.disqualified {
            menu = 0x12;
            page = 8;
        }
        event.globals.restart_550018 = false;
        event.globals.results_550024 = true;
        let rows = if event.cfg.difficulty_018 & 0x18 != 0 {
            16
        } else {
            8
        };
        let mut scroll = Scroll::default();
        scroll.centre_on(
            context.stage_position as i32,
            rows,
            i32::from(context.screen_height),
        );
        Self {
            page,
            settle: 0,
            menu_items: [item::RESTART, item::CONTINUE, item::QUIT],
            menu,
            dialog: None,
            context,
            rows,
            scroll,
            save: crate::save_dialog::SaveDialog::default(),
            records: None,
            record_globals: RecordGlobals::default(),
        }
    }

    /// This director with the record image and finishes the checks read.
    #[must_use]
    pub fn with_records(mut self, records: RecordRun) -> Self {
        self.records = Some(records);
        self
    }

    /// This director with the record globals the last one left.
    #[must_use]
    pub fn with_record_globals(mut self, globals: RecordGlobals) -> Self {
        self.record_globals = globals;
        self
    }

    /// The record pages' globals.
    pub const fn record_globals(&self) -> &RecordGlobals {
        &self.record_globals
    }

    /// The record image, with any record the checks wrote (the host keeps
    /// it as its cmRally.cfg image).
    pub fn records(&self) -> Option<&RecordRun> {
        self.records.as_ref()
    }

    /// Hand the record image and globals on (the director is done).
    pub fn take_records(&mut self) -> (Option<RecordRun>, RecordGlobals) {
        (
            self.records.take(),
            std::mem::take(&mut self.record_globals),
        )
    }

    /// This director with the dialog the last one left.
    #[must_use]
    pub fn with_save_dialog(mut self, save: crate::save_dialog::SaveDialog) -> Self {
        self.save = save;
        self
    }

    /// The save dialog.
    pub const fn save_dialog(&self) -> &crate::save_dialog::SaveDialog {
        &self.save
    }

    /// The host's answer to [`crate::save_dialog::SaveEffect::LoadSlots`].
    pub fn provide_save_slots(&mut self, files: &[Option<Vec<u8>>]) {
        self.save.provide_slots(files);
    }

    /// Hand the save dialog on (the director is done).
    pub fn take_save_dialog(&mut self) -> crate::save_dialog::SaveDialog {
        std::mem::take(&mut self.save)
    }

    /// The five-tick settle after a page change (`DAT_0054fc5c`); the pages
    /// draw nothing while it runs.
    pub const fn settle(&self) -> u8 {
        self.settle
    }

    /// `DAT_0054fc3c`.
    pub const fn rows(&self) -> u8 {
        self.rows
    }

    /// The tables' scroll.
    pub const fn scroll(&self) -> &Scroll {
        &self.scroll
    }

    /// What the race was.
    pub const fn context(&self) -> &DirectorContext {
        &self.context
    }

    /// The current page id.
    pub const fn page(&self) -> u8 {
        self.page
    }

    /// The menu's cursor and item count.
    pub fn menu(&self) -> (usize, usize) {
        (usize::from(self.menu >> 4), usize::from(self.menu & 0xf))
    }

    /// The menu items in order.
    pub fn menu_items(&self) -> &[u8] {
        &self.menu_items[..usize::from(self.menu & 0xf).min(3)]
    }

    /// The open confirmation's selected item (0 is "yes").
    pub fn dialog_selection(&self) -> Option<u8> {
        self.dialog
    }

    /// `FUN_00430770`: one tick.
    pub fn tick(&mut self, event: &mut EventState, input: DirectorInput) -> Vec<DirectorEffect> {
        let mut effects = Vec::new();
        if !event.globals.results_550024 {
            return effects;
        }
        if self.settle > 0 {
            self.settle -= 1;
            return effects;
        }
        let old = self.page;
        self.page = match self.page {
            4 => self.post_stage_menu(event, input, &mut effects),
            9 => self.confirmation(event, input, 9, quit),
            10 => self.confirmation(event, input, 10, restart),
            1..=3 => self.scrolling_page(event, input, &mut effects),
            5 => {
                let mut save = Vec::new();
                let open = self.save.tick(input.keys, input.slow_motion, &mut save);
                effects.extend(save.into_iter().map(DirectorEffect::Save));
                if open { 5 } else { self.after_save(event) }
            }
            page if !input.confirm => page,
            0xf if self.record_globals.stage_count > 0 => 0xc,
            0xf | 0xc => self.after_stage_results(event, &mut effects),
            0x10 if self.record_globals.rally_count > 0 => 0xd,
            0x10 | 0xd => self.after_rally_results(event),
            0x11 if self.record_globals.championship_recorded => 0xe,
            0x11 | 0xe => self.after_championship(event),
            6 => 1,
            8 => {
                event.globals.continues_4ed7d8 = false;
                4
            }
            0x12 => 2,
            0x14 | 0x15 => 4,
            other => other,
        };
        if self.page != old {
            effects.push(DirectorEffect::PageCue);
            self.settle = 5;
        }
        effects
    }

    /// Pages 1, 2 and 3 (`0x430af0`, `0x4313f0`, `0x431730`): the table
    /// scrolls (the stage results not in a time trial), then Return moves on.
    fn scrolling_page(
        &mut self,
        event: &mut EventState,
        input: DirectorInput,
        effects: &mut Vec<DirectorEffect>,
    ) -> u8 {
        if self.page != 1 || event.cfg.mode_166 & 3 != 2 {
            let max = i32::from(self.rows) - 8;
            self.scroll
                .step(max, input, i32::from(self.context.screen_height));
        }
        if !input.confirm {
            return self.page;
        }
        if self.records.is_none() {
            return match self.page {
                1 => self.after_stage_results(event, effects),
                2 => self.after_rally_results(event),
                _ => self.after_championship(event),
            };
        }
        match self.page {
            1 => self.stage_check(event, effects),
            2 => self.rally_check(event),
            _ => self.championship_check(event),
        }
    }

    fn stage_check(&mut self, event: &mut EventState, effects: &mut Vec<DirectorEffect>) -> u8 {
        let mut run = self.records.take().expect("the checks run with the image");
        let page = stage_records(&mut run, &mut self.record_globals, event);
        self.records = Some(run);
        page.unwrap_or_else(|| self.after_stage_results(event, effects))
    }

    fn rally_check(&mut self, event: &mut EventState) -> u8 {
        let mut run = self.records.take().expect("the checks run with the image");
        let page = rally_records(&mut run, &mut self.record_globals, event);
        self.records = Some(run);
        page.unwrap_or_else(|| self.after_rally_results(event))
    }

    fn championship_check(&mut self, event: &mut EventState) -> u8 {
        let mut run = self.records.take().expect("the checks run with the image");
        let page = championship_records(&mut run, &mut self.record_globals, event);
        self.records = Some(run);
        page.unwrap_or_else(|| self.after_championship(event))
    }

    fn centre_on(&mut self, position: usize) {
        let height = i32::from(self.context.screen_height);
        self.scroll.centre_on(position as i32, self.rows, height);
    }

    /// `0x431ae0`: the save dialog has returned; mid-rally the menu, at a rally's end the menu with the bonus
    /// event armed, else the rally-complete page.
    fn after_save(&self, event: &EventState) -> u8 {
        if !last_stage_of_rally(event) || event.globals.special_55001c {
            4
        } else {
            0x15
        }
    }

    /// `FUN_004311b0`: a time trial goes to the menu; a championship's
    /// super-special stage won outright awards that special country's bonus
    /// car once; otherwise the rally results.
    fn after_stage_results(
        &mut self,
        event: &mut EventState,
        effects: &mut Vec<DirectorEffect>,
    ) -> u8 {
        self.centre_on(self.context.rally_position);
        let mode = event.cfg.mode_166 & 3;
        if mode == 2 {
            return 4;
        }
        if mode == 0 && self.context.super_special && self.context.stage_position == 1 {
            let country = event.cfg.cursor_168 & 0xf;
            for (index, &special) in SPECIAL_COUNTRIES.iter().enumerate() {
                let bit = 1u8 << index;
                if special == country && event.cfg.unlocks_024 & bit == 0 {
                    event.cfg.unlocks_024 |= bit;
                    event.cfg.difficulty_018 |= 0x40;
                    event.cfg.progress_01b = event.cfg.progress_01b.wrapping_add(1);
                    effects.push(DirectorEffect::BonusCar);
                    return 0x12;
                }
            }
        }
        2
    }

    /// `FUN_004315e0`: before the rally's last stage the menu (the save page
    /// when offered); at its end the championship standings, except that a
    /// championship ends with GAME OVER outside the top six.
    fn after_rally_results(&mut self, event: &mut EventState) -> u8 {
        self.centre_on(self.context.championship_position);
        if !last_stage_of_rally(event) {
            return if self.save_offered(event) { 5 } else { 4 };
        }
        if event.cfg.mode_166 & 3 != 0 {
            return 4;
        }
        if self.context.rally_position > 6 {
            event.globals.continues_4ed7d8 = false;
            self.menu_items[0] = item::RESTART;
            self.menu_items[1] = item::CONTINUE;
            self.menu = 0x12;
            return 8;
        }
        3
    }

    /// `FUN_004318b0`: the menu offers only continue; the final rally ends
    /// the championship (or arms the bonus event for a top-three finish);
    /// otherwise the reach and the progress counters rise and the bonus event
    /// is armed for a top-three rally.
    fn after_championship(&mut self, event: &mut EventState) -> u8 {
        self.centre_on(self.context.stage_position);
        self.menu_items[0] = item::CONTINUE;
        self.menu = 0x01;
        let cfg = &mut event.cfg;
        let difficulty = cfg.difficulty_018 & 0x18 != 0;
        let country = cfg.cursor_168 & 0xf;
        if difficulty && country >= 7 {
            cfg.unlocks_023 = (cfg.unlocks_023 & 0xfb) | 0x0a;
            if self.context.rally_position > 3 && self.context.championship_position > 3 {
                return 0x14;
            }
            event.globals.special_55001c = true;
            return 4;
        }
        if !difficulty && country >= 5 {
            return 0x14;
        }
        let reach = country + 2;
        let bonus_count = u8::from(country + 1 > 3);
        if !difficulty {
            if country + 1 >= cfg.unlocks_022 & 0xf {
                cfg.unlocks_022 = (cfg.unlocks_022 & 0xf0) | (reach & 0xf);
                cfg.difficulty_018 |= 0x40;
                cfg.progress_01a = cfg.progress_01a.wrapping_add(2 + bonus_count);
            }
        } else {
            if country + 1 >= cfg.unlocks_022 >> 4 {
                cfg.unlocks_022 = (reach << 4) | (reach & 0xf);
                cfg.difficulty_018 |= 0x40;
                cfg.progress_01a = cfg.progress_01a.wrapping_add(5 + bonus_count);
            }
            let tier = i32::from(reach) / 2;
            if tier > i32::from((cfg.unlocks_023 >> 1) & 7) {
                cfg.unlocks_023 = (cfg.unlocks_023 & 0xf1) | (((tier & 7) as u8) << 1);
                cfg.difficulty_018 |= 0x40;
                cfg.progress_01a = cfg.progress_01a.wrapping_add(1);
            }
            event.globals.special_55001c = self.context.rally_position <= 3;
        }
        if self.save_offered(event) {
            5
        } else if event.globals.special_55001c {
            4
        } else {
            0x15
        }
    }

    /// `0x4321f0`.
    fn post_stage_menu(
        &mut self,
        event: &mut EventState,
        input: DirectorInput,
        effects: &mut Vec<DirectorEffect>,
    ) -> u8 {
        let cursor = self.menu >> 4;
        let count = self.menu & 0xf;
        if input.right {
            if cursor + 1 < count {
                effects.push(DirectorEffect::CursorCue(true));
                self.menu = ((cursor + 1) << 4) | count;
            } else {
                effects.push(DirectorEffect::CursorCue(false));
            }
            return 4;
        }
        if input.left {
            if cursor > 0 {
                effects.push(DirectorEffect::CursorCue(true));
                self.menu = ((cursor - 1) << 4) | count;
            } else {
                effects.push(DirectorEffect::CursorCue(false));
            }
            return 4;
        }
        if !input.confirm {
            return 4;
        }
        match self.menu_items.get(usize::from(cursor)).copied() {
            Some(item::RESTART) => {
                self.dialog = Some(1);
                10
            }
            Some(item::CONTINUE) => {
                finish(event);
                0xb
            }
            Some(item::QUIT) => {
                self.dialog = Some(1);
                9
            }
            // An unknown item leaves without switching the director off.
            _ => 0xb,
        }
    }

    /// Pages 9 and 10: the dialog menu steps until an item is confirmed;
    /// "yes" runs `yes`, "no" returns to the menu.
    fn confirmation(
        &mut self,
        event: &mut EventState,
        input: DirectorInput,
        stay: u8,
        yes: fn(&mut EventState),
    ) -> u8 {
        let Some(mut selected) = self.dialog else {
            return 4;
        };
        if input.up && selected > 0 {
            selected -= 1;
        } else if input.down && selected < 1 {
            selected += 1;
        }
        self.dialog = Some(selected);
        if !input.confirm {
            return stay;
        }
        self.dialog = None;
        if selected == 0 {
            yes(event);
            0xb
        } else {
            4
        }
    }

    /// `FUN_00431680`: the save is offered only in a championship; a novice
    /// is offered it at every stage except the last country's later ones;
    /// with a difficulty set it follows bit `1 << stage` of the country's
    /// byte in the static schedule `0x4ed8e4`, which the host supplies from
    /// the user's executable (none supplied: never offered).
    fn save_offered(&self, event: &EventState) -> bool {
        if event.cfg.mode_166 & 3 != 0 {
            return false;
        }
        let country = event.cfg.cursor_168 & 0xf;
        let stage = event.cfg.cursor_168 >> 4;
        if event.cfg.difficulty_018 & 0x18 == 0 {
            return !(country == 5 && stage >= 2);
        }
        self.context
            .save_schedule
            .and_then(|schedule| schedule.get(usize::from(country)).copied())
            .is_some_and(|bits| u32::from(bits) & (1u32 << stage) != 0)
    }
}

/// A writer's mark `live[0x18] |= 0x40`, `live[0x19] += 1`, mirrored into
/// the event.
fn mark(run: &mut RecordRun, event: &mut EventState) {
    let live = live_mut(&mut run.image);
    crate::hiscore_records::mark_live(live);
    event.cfg.difficulty_018 = live[0x18];
}

/// `FUN_00430ca0`, page 1's confirm: each player's stage time against
/// the stage record (`FUN_004ba370`), the CAC stage competition (numbers
/// `1..=3000`, alone and not head to head), then the notice or
/// [`Self::after_stage_results`].
fn stage_records(run: &mut RecordRun, g: &mut RecordGlobals, event: &mut EventState) -> Option<u8> {
    event.cfg.store_live(live_mut(&mut run.image));
    if !no_cheats(live(&run.image)) {
        return None;
    }
    g.stage_count = 0;
    let cursor = event.cfg.cursor_168;
    let secret = event.cfg.mode_166 & 0x40 != 0;
    for (p, finish) in run.players.clone().iter().enumerate() {
        let live = live(&run.image);
        let car = live[0xb6 + 0x9c * p];
        let name = player_name(live, p);
        let record = crate::hiscore_records::StageRecord {
            name: &name,
            car: car & 0xf,
            splits: finish.splits.map(|split| split as u16),
            time: finish.stage_time,
            manual: (car >> 4) & 1 != 0,
            extra: run.salt,
        };
        if crate::hiscore_records::best_stage(
            &mut run.image[..],
            secret,
            cursor & 0xf,
            cursor >> 4,
            &record,
        ) {
            mark(run, event);
            g.stage_code = record_code(
                RecordKind::Stage,
                live_of(run),
                p,
                finish.slot,
                finish.stage_time,
                run.salt,
                0,
                &run.schedule,
            );
            g.stage_player = p as u8;
            g.stage_count = g.stage_count.wrapping_add(1);
        }
    }
    if !event.split_screen() && !event.head_to_head() {
        let n = cac_number(live(&run.image));
        if (1..=3000).contains(&n) {
            let first = run.players.first().copied().unwrap_or_default();
            g.stage_cac = record_code(
                RecordKind::Stage,
                live_of(run),
                0,
                first.slot,
                first.stage_time,
                run.salt,
                u32::from(n),
                &run.schedule,
            );
            return Some(0xf);
        }
    }
    if g.stage_count > 0 { Some(0xc) } else { None }
}

/// `FUN_004313f0`'s confirm: at the rally's stage 5 (6 in a special
/// country), outside a single special-series rally, each player's rally
/// time against the rally's top five (`FUN_004ba260`); with two records
/// the better place is shown (a tie shows player 2); then the CAC rally
/// competition (`3001..=6000`), the notice or
/// [`Self::after_rally_results`].
fn rally_records(run: &mut RecordRun, g: &mut RecordGlobals, event: &mut EventState) -> Option<u8> {
    let country = event.cfg.cursor_168 & 0xf;
    let last = if SPECIAL_COUNTRIES.contains(&country) {
        6
    } else {
        5
    };
    let mode = event.cfg.mode_166;
    if (mode & 3 == 1 && mode & 0x20 != 0) || event.cfg.cursor_168 >> 4 != last {
        return None;
    }
    event.cfg.store_live(live_mut(&mut run.image));
    if !no_cheats(live(&run.image)) {
        return None;
    }
    g.rally_count = 0;
    let mut times = [0i32; 2];
    for (p, finish) in run.players.clone().iter().enumerate().take(2) {
        let live = live(&run.image);
        let car = live[0xb6 + 0x9c * p];
        let at = 0x16c + 16 * finish.slot;
        times[p] = i32::from_le_bytes([live[at], live[at + 1], live[at + 2], live[at + 3]]);
        let name = player_name(live, p);
        let place = crate::hiscore_records::best_rally(
            &mut run.image[..],
            country,
            &name,
            car & 0xf,
            times[p],
            (car >> 4) & 1 != 0,
            run.salt,
        );
        g.rally_places[p] = place.map_or(-1, |place| place as i32);
        if place.is_some() {
            mark(run, event);
            g.rally_shown[p] = p as u8;
            g.rally_count = g.rally_count.wrapping_add(1);
        }
    }
    if g.rally_count == 2 && g.rally_places[1] <= g.rally_places[0] {
        g.rally_shown[0] = g.rally_shown[1];
    }
    let slot = |p: usize| run.players.get(p).map_or(0, |finish| finish.slot);
    if g.rally_count > 0 {
        let q = usize::from(g.rally_shown[0]).min(1);
        g.rally_code = record_code(
            RecordKind::Rally,
            live_of(run),
            q,
            slot(q),
            times[q],
            run.salt,
            0,
            &run.schedule,
        );
    }
    if !event.split_screen() && !event.head_to_head() {
        let n = cac_number(live(&run.image));
        if (3001..=6000).contains(&n) {
            g.rally_cac = record_code(
                RecordKind::Rally,
                live_of(run),
                0,
                slot(0),
                times[0],
                run.salt,
                u32::from(n),
                &run.schedule,
            );
            return Some(0x10);
        }
    }
    if g.rally_count > 0 { Some(0xd) } else { None }
}

/// `FUN_00431730`'s confirm: at the championship's last stage, player
/// 1's points (the sum of the slot's first six bytes, eight with a
/// difficulty) and rally wins (`+0xd`) against the difficulty's top five
/// (`FUN_004ba120`); then the CAC championship competition
/// (`6001..=9999`), the notice or [`Self::after_championship`].
fn championship_records(
    run: &mut RecordRun,
    g: &mut RecordGlobals,
    event: &mut EventState,
) -> Option<u8> {
    event.cfg.store_live(live_mut(&mut run.image));
    let slot = run.players.first().map_or(0, |finish| finish.slot);
    let live = live(&run.image);
    let wins = i32::from(live[0x179 + 16 * slot] as i8);
    let difficulty = live[0x18] & 0x18 != 0;
    let last = if difficulty { 7 } else { 5 };
    let points: u32 = live[0x170 + 16 * slot..=0x170 + 16 * slot + last]
        .iter()
        .map(|&b| u32::from(b))
        .sum();
    let final_stage = if difficulty {
        i32::from(live[0x165] & 0xf) - 1
    } else {
        2
    };
    if usize::from(live[0x168] & 0xf) < last
        || i32::from(live[0x168] >> 4) < final_stage
        || !no_cheats(live)
    {
        return None;
    }
    let car = live[0xb6];
    let name = player_name(live, 0);
    let table = (live[0x18] >> 3) & 3;
    let place = crate::hiscore_records::championship(
        &mut run.image[..],
        &name,
        car & 0xf,
        wins,
        points as i32,
        table,
        (car >> 4) & 1 != 0,
        run.salt,
    );
    g.championship_place = place.map_or(-1, |place| place as i32);
    g.championship_recorded = place.is_some();
    if place.is_some() {
        mark(run, event);
        g.championship_code = record_code(
            RecordKind::Championship,
            live_of(run),
            0,
            slot,
            points as i32,
            run.salt,
            0,
            &run.schedule,
        );
    }
    let n = cac_number(live_of(run));
    if (6001..=9999).contains(&n) {
        g.championship_cac = record_code(
            RecordKind::Championship,
            live_of(run),
            0,
            slot,
            points as i32,
            run.salt,
            u32::from(n),
            &run.schedule,
        );
        return Some(0x11);
    }
    if g.championship_recorded {
        Some(0xe)
    } else {
        None
    }
}

/// The live block of a run's image.
fn live_of(run: &RecordRun) -> &[u8] {
    live(&run.image)
}

/// `FUN_0042f560` (time trial: `FUN_00432460`): restart the rally; outside a
/// time trial the bonus event is disarmed.
fn restart(event: &mut EventState) {
    event.globals.restart_550018 = true;
    if event.cfg.mode_166 & 3 != 2 {
        event.globals.special_55001c = false;
    }
    finish(event);
}

/// `FUN_0042f590`: quit ends the event.
fn quit(event: &mut EventState) {
    event.globals.continues_4ed7d8 = false;
    event.globals.special_55001c = false;
    finish(event);
}

/// `FUN_0042f540`: the director switches off.
fn finish(event: &mut EventState) {
    event.globals.results_550024 = false;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(mode: u8, cursor: u8, difficulty: u8) -> EventState {
        let mut event = EventState::default();
        event.cfg.mode_166 = mode;
        event.cfg.cursor_168 = cursor;
        event.cfg.difficulty_018 = difficulty;
        event.cfg.stage_count_165 = 6;
        event
    }

    fn confirm() -> DirectorInput {
        DirectorInput {
            confirm: true,
            ..Default::default()
        }
    }

    /// Advance past the settle and act once.
    fn press(director: &mut ResultsDirector, event: &mut EventState, input: DirectorInput) {
        while director.settle > 0 {
            director.tick(event, DirectorInput::default());
        }
        director.tick(event, input);
    }

    fn context(rally: usize) -> DirectorContext {
        DirectorContext {
            stage_position: 2,
            rally_position: rally,
            championship_position: 4,
            ..Default::default()
        }
    }

    #[test]
    fn mid_rally_pages_go_stage_rally_menu_and_continue() {
        let mut e = event(0, 0x10, 0x08);
        e.globals.continues_4ed7d8 = false;
        let mut d = ResultsDirector::start(&mut e, context(2));
        assert!(e.globals.continues_4ed7d8);
        assert_eq!(d.page(), 1);
        assert_eq!(d.menu(), (1, 3));
        press(&mut d, &mut e, confirm());
        assert_eq!(d.page(), 2);
        assert_eq!(d.settle, 5);
        press(&mut d, &mut e, confirm());
        assert_eq!(d.page(), 4);
        press(&mut d, &mut e, confirm());
        assert_eq!(d.page(), 0xb);
        assert!(!e.globals.results_550024);
    }

    #[test]
    fn the_rally_end_shows_the_standings_or_game_over() {
        let mut e = event(0, 0x51, 0x08); // country 1, last stage
        let mut d = ResultsDirector::start(&mut e, context(7));
        press(&mut d, &mut e, confirm());
        press(&mut d, &mut e, confirm());
        assert_eq!(d.page(), 8, "seventh in a championship rally");
        assert!(!e.globals.continues_4ed7d8);
        assert_eq!(d.menu(), (1, 2));
        press(&mut d, &mut e, confirm());
        assert_eq!(d.page(), 4);

        let mut e = event(0, 0x51, 0x08);
        let mut d = ResultsDirector::start(&mut e, context(3));
        press(&mut d, &mut e, confirm());
        press(&mut d, &mut e, confirm());
        assert_eq!(d.page(), 3);
        press(&mut d, &mut e, confirm());
        // Difficulty set, country 1: reach 3 in both nibbles, tier 1, bonus armed.
        assert_eq!(e.cfg.unlocks_022, 0x33);
        assert_eq!((e.cfg.unlocks_023 >> 1) & 7, 1);
        assert_eq!(e.cfg.progress_01a, 5 + 1);
        assert!(e.globals.special_55001c);
        assert_eq!(d.page(), 4);
        assert_eq!(d.menu_items(), &[item::CONTINUE]);
    }

    #[test]
    fn the_final_rally_ends_the_championship() {
        let mut e = event(0, 0x57, 0x08); // country 7
        let mut d = ResultsDirector::start(&mut e, context(5));
        for _ in 0..3 {
            press(&mut d, &mut e, confirm());
        }
        assert_eq!(d.page(), 0x14);
        assert_eq!(e.cfg.unlocks_023 & 0x0e, 0x0a);
        let mut novice = event(0, 0x25, 0);
        let mut d = ResultsDirector::start(&mut novice, context(1));
        for _ in 0..3 {
            press(&mut d, &mut novice, confirm());
        }
        assert_eq!(d.page(), 0x14);
    }

    #[test]
    fn the_menu_moves_and_the_confirmations_default_to_no() {
        let mut e = event(1, 0x10, 0x08);
        let mut d = ResultsDirector::start(&mut e, context(1));
        press(&mut d, &mut e, confirm());
        press(&mut d, &mut e, confirm());
        assert_eq!(d.page(), 4);
        let right = DirectorInput {
            right: true,
            ..Default::default()
        };
        press(&mut d, &mut e, right);
        assert_eq!(d.menu(), (2, 3));
        let effects = d.tick(&mut e, right);
        assert_eq!(effects, vec![DirectorEffect::CursorCue(false)]);
        // Quit: "no" by default returns to the menu.
        press(&mut d, &mut e, confirm());
        assert_eq!((d.page(), d.dialog_selection()), (9, Some(1)));
        press(&mut d, &mut e, confirm());
        assert_eq!(d.page(), 4);
        // Quit again and say yes.
        press(&mut d, &mut e, confirm());
        press(
            &mut d,
            &mut e,
            DirectorInput {
                up: true,
                ..Default::default()
            },
        );
        assert_eq!(d.dialog_selection(), Some(0));
        press(&mut d, &mut e, confirm());
        assert_eq!(d.page(), 0xb);
        assert!(!e.globals.results_550024 && !e.globals.continues_4ed7d8);
    }

    #[test]
    fn restart_arms_the_rewind() {
        let mut e = event(2, 0x10, 0x08);
        e.globals.special_55001c = true;
        let mut d = ResultsDirector::start(&mut e, context(1));
        press(&mut d, &mut e, confirm()); // a time trial goes straight to the menu
        assert_eq!(d.page(), 4);
        let left = DirectorInput {
            left: true,
            ..Default::default()
        };
        press(&mut d, &mut e, left);
        press(&mut d, &mut e, confirm());
        assert_eq!(d.page(), 10);
        press(
            &mut d,
            &mut e,
            DirectorInput {
                up: true,
                ..Default::default()
            },
        );
        press(&mut d, &mut e, confirm());
        assert!(e.globals.restart_550018 && !e.globals.results_550024);
        assert!(e.globals.special_55001c, "a time trial's restart keeps it");
    }

    #[test]
    fn the_super_special_win_awards_a_bonus_car_once() {
        let mut e = event(0, 0x63, 0x08); // country 3 is special index 1
        let ctx = DirectorContext {
            super_special: true,
            stage_position: 1,
            rally_position: 1,
            championship_position: 1,
            ..Default::default()
        };
        let mut d = ResultsDirector::start(&mut e, ctx);
        assert_eq!(d.page(), 6);
        press(&mut d, &mut e, confirm());
        assert_eq!(d.page(), 1);
        press(&mut d, &mut e, confirm());
        assert_eq!(d.page(), 0x12);
        assert_eq!(e.cfg.unlocks_024, 0b10);
        assert_eq!(e.cfg.progress_01b, 1);
        let mut again = ResultsDirector::start(&mut e, ctx);
        press(&mut again, &mut e, confirm());
        press(&mut again, &mut e, confirm());
        assert_eq!(again.page(), 2, "already awarded");
    }

    #[test]
    fn a_championship_offers_the_save_by_its_schedule() {
        // Novice, mid-rally: offered, then the dialog returns to the menu.
        let mut e = event(0, 0x02, 0);
        let mut d = ResultsDirector::start(&mut e, context(1));
        press(&mut d, &mut e, confirm());
        press(&mut d, &mut e, confirm());
        assert_eq!(d.page(), 5);
        // The dialog: Down to "no", Return, then it ends.
        let keys = |keys| DirectorInput {
            keys,
            ..Default::default()
        };
        press(&mut d, &mut e, keys(0));
        assert_eq!(d.page(), 5);
        d.tick(&mut e, keys(crate::descriptor_menu::bits::DOWN));
        d.tick(&mut e, keys(crate::descriptor_menu::bits::CONFIRM));
        d.tick(&mut e, keys(0));
        assert_eq!(d.page(), 4);
        // Novice, the last country's later stages: not offered.
        let mut e = event(0, 0x25, 0);
        let mut d = ResultsDirector::start(&mut e, context(1));
        assert!(!d.save_offered(&e));
        // A rally does not offer it.
        let e = event(1, 0x02, 0);
        assert!(!d.save_offered(&e));
        // Difficulty set: the schedule's stage bit, never without a schedule.
        let e = event(0, 0x12, 0x08);
        assert!(!d.save_offered(&e));
        let mut schedule = [0u8; 9];
        schedule[2] = 0b10;
        d.context.save_schedule = Some(schedule);
        assert!(d.save_offered(&e));
        let e = event(0, 0x02, 0x08);
        assert!(!d.save_offered(&e));
    }

    #[test]
    fn after_the_save_a_rally_end_completes_the_rally() {
        let mut e = event(0, 0x51, 0);
        let d = ResultsDirector::start(&mut e, context(1));
        assert_eq!(d.after_save(&e), 0x15);
        e.globals.special_55001c = true;
        assert_eq!(d.after_save(&e), 4);
        let e = event(0, 0x01, 0);
        assert_eq!(d.after_save(&e), 4);
    }

    #[test]
    fn the_tables_scroll_a_row_at_a_time() {
        let mut e = event(0, 0x10, 0x08);
        let mut d = ResultsDirector::start(
            &mut e,
            DirectorContext {
                stage_position: 12,
                screen_height: 480,
                ..context(1)
            },
        );
        // Sixteen rows; row 12 shows from row 7 (pitch 37 at 480 high).
        assert_eq!(d.rows(), 16);
        assert_eq!((d.scroll().index, d.scroll().offset), (7, -259));
        let down = DirectorInput {
            down_held: true,
            ..Default::default()
        };
        d.tick(&mut e, down);
        assert_eq!((d.scroll().index, d.scroll().offset), (8, -264));
        for _ in 0..7 {
            d.tick(&mut e, DirectorInput::default());
        }
        assert_eq!((d.scroll().index, d.scroll().offset), (8, -296));
        // The last row is the limit.
        for _ in 0..20 {
            d.tick(&mut e, down);
        }
        assert_eq!(d.scroll().index, 8);
        // Halfway through a tick the drawn offset is the rounded midpoint.
        let scroll = Scroll {
            start: -10,
            offset: -15,
            ..Scroll::default()
        };
        assert_eq!(scroll.drawn(0x8000), -12);
        assert_eq!(scroll.drawn(0x10000), -15);
        // A novice's eight rows never scroll.
        let mut novice = event(0, 0x10, 0);
        let d = ResultsDirector::start(
            &mut novice,
            DirectorContext {
                stage_position: 8,
                screen_height: 480,
                ..context(1)
            },
        );
        assert_eq!((d.rows(), d.scroll().index), (8, 0));
    }

    #[test]
    fn disqualification_opens_on_game_over() {
        let mut e = event(0, 0x10, 0x08);
        let d = ResultsDirector::start(
            &mut e,
            DirectorContext {
                disqualified: true,
                ..context(1)
            },
        );
        assert_eq!(d.page(), 8);
        assert_eq!(d.menu(), (1, 2));
    }

    /// A record image whose tables every time beats, with the player named
    /// and the CAC number `cac`.
    fn run(cac: u16, time: i32) -> RecordRun {
        let mut image = Box::new([0u8; LEGACY_CONFIG_SIZE]);
        for record in 0..(7 * 9) {
            let at = 0xffc + record * 30 + 0x1a;
            image[at..at + 2].copy_from_slice(&0xffffu16.to_le_bytes());
        }
        for record in 0..(5 * 9) {
            let at = 0xd7c + record * 16 + 0xc;
            image[at..at + 4].copy_from_slice(&0x7ffffu32.to_le_bytes());
        }
        let live = live_mut(&mut image);
        live[0x2c..0x31].copy_from_slice(b"COLIN");
        live[0x20..0x22].copy_from_slice(&cac.to_le_bytes());
        RecordRun {
            image,
            schedule: vec![7; crate::hiscore_codes::SCHEDULE_LEN],
            players: vec![PlayerFinish {
                slot: 3,
                stage_time: time,
                splits: [1, 2, 3, 4, 5, 6, 7],
            }],
            salt: 9,
        }
    }

    #[test]
    fn a_stage_record_shows_its_notice_before_the_rally_results() {
        let mut event = event(0, 0x21, 0);
        event.cfg.car_0b6 = 0x12;
        let mut director =
            ResultsDirector::start(&mut event, context(1)).with_records(run(0, 4321));
        press(&mut director, &mut event, confirm());
        assert_eq!(director.page(), 0xc);
        let g = director.record_globals();
        assert_eq!((g.stage_count, g.stage_player), (1, 0));
        assert!(!g.stage_code.is_empty());
        let image = &director.records().unwrap().image;
        let at = 0xffc + (7 + 2) * 30;
        assert_eq!(&image[at..at + 5], b"COLIN");
        assert_eq!(
            u16::from_le_bytes([image[at + 0x1a], image[at + 0x1b]]),
            4321
        );
        assert_eq!(image[at + 0x1c] & 0xf, 2, "the car");
        assert_eq!(live(image)[0x19], 1);
        assert_eq!(event.cfg.difficulty_018 & 0x40, 0x40);
        press(&mut director, &mut event, confirm());
        assert_eq!(director.page(), 2);
    }

    #[test]
    fn a_cac_entry_shows_its_code_page_first() {
        let mut event = event(0, 0x21, 0);
        let mut director =
            ResultsDirector::start(&mut event, context(1)).with_records(run(1234, 4321));
        press(&mut director, &mut event, confirm());
        assert_eq!(director.page(), 0xf);
        assert!(!director.record_globals().stage_cac.is_empty());
        press(&mut director, &mut event, confirm());
        assert_eq!(director.page(), 0xc);
        press(&mut director, &mut event, confirm());
        assert_eq!(director.page(), 2);
        // A rally competition number does not stop at the stage.
        let mut event = self::event(0, 0x21, 0);
        let mut director =
            ResultsDirector::start(&mut event, context(1)).with_records(run(4000, 0x10000));
        press(&mut director, &mut event, confirm());
        assert_eq!(director.page(), 2, "65536 hundredths cannot record");
    }

    #[test]
    fn a_cheat_skips_the_checks() {
        let mut event = event(0, 0x21, 0);
        event.cfg.unlocks_024 = 0x40;
        let mut director =
            ResultsDirector::start(&mut event, context(1)).with_records(run(1234, 4321));
        press(&mut director, &mut event, confirm());
        assert_eq!(director.page(), 2);
        assert_eq!(director.record_globals().stage_count, 0);
    }

    #[test]
    fn a_rally_record_needs_stage_five() {
        // Stage index 5 of country 0 (not special): the rally's end.
        let mut event = event(1, 0x50, 0);
        let mut records = run(0, 0x10000);
        live_mut(&mut records.image)[0x16c + 16 * 3..0x170 + 16 * 3]
            .copy_from_slice(&90_000i32.to_le_bytes());
        event.cfg.results_16c[3][..4].copy_from_slice(&90_000i32.to_le_bytes());
        let mut director = ResultsDirector::start(&mut event, context(1)).with_records(records);
        press(&mut director, &mut event, confirm());
        assert_eq!(director.page(), 2);
        press(&mut director, &mut event, confirm());
        assert_eq!(director.page(), 0xd);
        let g = director.record_globals();
        assert_eq!((g.rally_count, g.rally_places[0]), (1, 0));
        let image = &director.records().unwrap().image;
        assert_eq!(&image[0xd7c..0xd81], b"COLIN");
        press(&mut director, &mut event, confirm());
        assert_eq!(director.page(), 4, "a single rally's end goes to the menu");
        // Stage 4 is not checked.
        let mut event = self::event(1, 0x40, 0);
        let mut director =
            ResultsDirector::start(&mut event, context(1)).with_records(run(0, 0x10000));
        press(&mut director, &mut event, confirm());
        press(&mut director, &mut event, confirm());
        assert_ne!(director.page(), 0xd);
    }

    #[test]
    fn a_championship_record_at_the_last_stage() {
        // Novice: country 5, stage 2; points 6 + 5 (bytes +0x170..=+0x175).
        let mut event = event(0, 0x25, 0);
        event.cfg.results_16c[3][4..10].copy_from_slice(&[1, 2, 3, 0, 5, 0]);
        event.cfg.results_16c[3][0xd] = 2;
        let mut director =
            ResultsDirector::start(&mut event, context(1)).with_records(run(0, 0x10000));
        director.page = 3;
        press(&mut director, &mut event, confirm());
        assert_eq!(director.page(), 0xe);
        let g = director.record_globals();
        assert_eq!((g.championship_place, g.championship_recorded), (0, true));
        let image = &director.records().unwrap().image;
        assert_eq!(image[0xc98 + 0xc], 0x80 | 11);
        assert_eq!(image[0xc98 + 0xd] & 0xf, 2, "the rally wins");
        press(&mut director, &mut event, confirm());
        assert_eq!(director.page(), 0x14, "the championship is complete");
    }

    #[test]
    fn the_salt_spans_zero_to_sixty_two() {
        assert_eq!(record_salt(0), 0);
        assert_eq!(record_salt(0x8000), 31);
        // The largest draw scales just under 1 (`0x38000100` rounds below
        // 1 / 32767).
        let top = (f64::from(0x7fff) * f64::from(f32::from_bits(0x3800_0100)) * 65536.0) as i32;
        assert_eq!(record_salt(top), 62);
    }
}
