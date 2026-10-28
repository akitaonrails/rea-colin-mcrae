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
//! Not reconstructed: the stage, rally and championship record checks
//! (`FUN_004ba370`/`FUN_004ba260`/`FUN_004ba120`) and their notice and code
//! pages, which the confirm skips; network and head-to-head pages. Page 5
//! runs the save dialog ([`crate::save_dialog`]) every tick until it ends. A description of disassembly, not original source.
//! Evidence: `rea.official-eu.results-director`.

use crate::event_flow::{EventState, SPECIAL_COUNTRIES};
use crate::standings::last_stage_of_rally;

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
        }
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
        match self.page {
            1 => self.after_stage_results(event, effects),
            2 => self.after_rally_results(event),
            _ => self.after_championship(event),
        }
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
}
