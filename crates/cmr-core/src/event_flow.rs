//! The event (championship / rally / selected-stages) bookkeeping the
//! top-level FSM runs between stages, over the live config block
//! `DAT_00d3ca80`:
//!
//! - [`EventState::boot`] is the state-1 handler `FUN_00436be0`, which also
//!   chooses the first event of a session;
//! - [`EventState::advance`] is `FUN_00436f00`, the step to the next stage the
//!   post-race hub calls, with its per-mode helpers `FUN_00436ff0`,
//!   `FUN_00436e20` and `FUN_00437090`;
//! - [`EventState::rewind_for_restart`] is the hub's restart block
//!   (`0x438c48..0x438d09`);
//! - [`EventState::show_welcome`], [`EventState::split_screen`] and
//!   [`EventState::head_to_head`] are `FUN_0048df30`, `FUN_00401000` and
//!   `FUN_00401030`.
//!
//! Every write is the native byte operation (nibble splices included). The
//! four-entry country table `0x506dc8` is not game content but a list of
//! country indices the code tests against; its static value is reproduced as
//! [`SPECIAL_COUNTRIES`]. The 16-entry table at `0x506e48` the boot handler's
//! slot pass reads is static data and is passed in by the caller.
//! A description of disassembly, not original source. Evidence:
//! `rea.official-eu.event-flow`.

use crate::game_state::GameState;

/// `0x506dc8`: the countries whose events carry the extra special stage
/// (`FUN_00485b80`), in the order the special series visits them.
pub const SPECIAL_COUNTRIES: [u8; 4] = [1, 3, 5, 7];

/// The live config bytes the event flow touches (`DAT_00d3ca80 + offset`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EventConfig {
    /// `+0x00`: bit 0 is the attract demo.
    pub flags_000: u8,
    /// `+0x18`: bits `0x18` are the difficulty.
    pub difficulty_018: u8,
    /// `+0x25`: bit 7 forces a night stage, bit 6 weather type 5
    /// ([`crate::stage_weather`]).
    pub options_025: u8,
    /// `+0x164`: the selected-stages mask.
    pub stage_mask_164: u8,
    /// `+0x165`: low nibble is the stage count of the event.
    pub stage_count_165: u8,
    /// `+0x166`: mode (`& 3`), arrangement (`& 0x1c`), special series
    /// (`0x20`), secret stages (`0x40`), advance pending (`0x80`).
    pub mode_166: u8,
    /// `+0x167`: low six bits count the events run.
    pub progress_167: u8,
    /// `+0x168`: low nibble country, high nibble stage.
    pub cursor_168: u8,
    /// `+0x16c`: the 16 result slots of 16 bytes.
    pub results_16c: [[u8; 16]; 16],
    /// `+0x6c0`: the rally-school status, level (bits 3-4) and task
    /// (bits 5-7).
    pub school_6c0: u8,
}

impl EventConfig {
    /// Read the fields from the live block (`DAT_00d3ca80`,
    /// [`crate::config_commit::live`]).
    ///
    /// # Panics
    ///
    /// If `live` is shorter than `0x6c1` bytes.
    pub fn from_live(live: &[u8]) -> Self {
        let (slots, _) = live[0x16c..0x26c].as_chunks::<16>();
        let results_16c = std::array::from_fn(|slot| slots[slot]);
        Self {
            flags_000: live[0x00],
            difficulty_018: live[0x18],
            options_025: live[0x25],
            stage_mask_164: live[0x164],
            stage_count_165: live[0x165],
            mode_166: live[0x166],
            progress_167: live[0x167],
            cursor_168: live[0x168],
            results_16c,
            school_6c0: live[0x6c0],
        }
    }

    /// Write the fields back into the live block.
    ///
    /// # Panics
    ///
    /// If `live` is shorter than `0x6c1` bytes.
    pub fn store_live(&self, live: &mut [u8]) {
        live[0x00] = self.flags_000;
        live[0x18] = self.difficulty_018;
        live[0x25] = self.options_025;
        live[0x164] = self.stage_mask_164;
        live[0x165] = self.stage_count_165;
        live[0x166] = self.mode_166;
        live[0x167] = self.progress_167;
        live[0x168] = self.cursor_168;
        let (slots, _) = live[0x16c..0x26c].as_chunks_mut::<16>();
        slots.copy_from_slice(&self.results_16c);
        live[0x6c0] = self.school_6c0;
    }
}

/// The globals the event flow reads and writes besides the config block.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EventGlobals {
    /// `DAT_00d3ca88`: a rally-school session.
    pub school_d3ca88: bool,
    /// `DAT_00d3ca94`: a network session.
    pub network_d3ca94: bool,
    /// `DAT_00550018`: a restart was requested from the pause menu.
    pub restart_550018: bool,
    /// `DAT_0055001c`: the special event is armed.
    pub special_55001c: bool,
    /// `DAT_004ed7d8`: the championship may continue to the next country (set
    /// to 1 when the FSM is seeded, cleared by the results screens).
    pub continues_4ed7d8: bool,
    /// `DAT_00551ad8`: the head-to-head sequence byte.
    pub sequence_551ad8: u8,
    /// `DAT_00514478`: the armed race is the special event (state 4 copies
    /// `DAT_0055001c`).
    pub special_514478: bool,
    /// `DAT_004f6848`.
    pub word_4f6848: u32,
    /// `DAT_00506228`: the descriptor front end runs instead of the FSM.
    pub frontend_506228: bool,
    /// `DAT_00d3ca84`: a race session is running.
    pub race_session_d3ca84: bool,
    /// The FSM init-done flag `DAT_005590b0` was cleared (`FUN_004440a0(0)`),
    /// so the next session re-seeds the FSM from state 1.
    pub reseed_requested: bool,
}

/// The config block plus the globals.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EventState {
    pub cfg: EventConfig,
    pub globals: EventGlobals,
}

fn splice_low(byte: u8, low: u8) -> u8 {
    ((byte ^ low) & 0xf) ^ byte
}

fn splice_counter(byte: u8, counter: u8) -> u8 {
    ((byte ^ counter) & 0x3f) ^ byte
}

fn special_country(country: u8) -> bool {
    SPECIAL_COUNTRIES.contains(&country)
}

impl EventState {
    fn mode(&self) -> u8 {
        self.cfg.mode_166 & 3
    }

    fn difficulty(&self) -> u8 {
        self.cfg.difficulty_018 & 0x18
    }

    fn counter(&self) -> u8 {
        self.cfg.progress_167 & 0x3f
    }

    fn stage(&self) -> u8 {
        self.cfg.cursor_168 >> 4
    }

    fn clear_times(&mut self) {
        for slot in &mut self.cfg.results_16c {
            slot[..4].fill(0);
        }
    }

    /// `FUN_00401000`: the arrangement is a two-player split (`4` or `0xc`)
    /// outside the rally school.
    pub fn split_screen(&self) -> bool {
        matches!(self.cfg.mode_166 & 0x1c, 4 | 0xc) && !self.globals.school_d3ca88
    }

    /// `FUN_00401030`: the arrangement `8` (head to head) outside the rally
    /// school.
    pub fn head_to_head(&self) -> bool {
        self.cfg.mode_166 & 0x1c == 8 && !self.globals.school_d3ca88
    }

    /// `FUN_0048df30`: the country welcome screen shows before the stage, for
    /// modes 0 and 1 when the special event is armed, in the special series,
    /// or on an event's first stage.
    pub fn show_welcome(&self) -> bool {
        self.mode() <= 1
            && (self.globals.special_55001c
                || self.cfg.mode_166 & 0x20 != 0
                || self.cfg.cursor_168 & 0xf0 == 0)
    }

    /// `FUN_00436e20`: make the `counter`-th enabled stage of the mask the
    /// current one (a network session takes the first enabled stage and
    /// restarts the count). Returns 0 when the mask holds no such stage.
    pub fn pick_selected_stage(&mut self) -> bool {
        let count = self.cfg.stage_count_165 & 0xf;
        let mut enabled = 0;
        for stage in 0..count {
            if self.cfg.stage_mask_164 & (1 << stage) == 0 {
                continue;
            }
            if self.globals.network_d3ca94 {
                self.cfg.progress_167 &= 0xc0;
                self.set_stage(stage);
                return true;
            }
            if enabled == self.counter() {
                if self.cfg.mode_166 & 0x20 == 0 {
                    self.set_stage(stage);
                } else if self.cfg.mode_166 & 0x40 != 0 {
                    self.cfg.cursor_168 = (self.cfg.cursor_168 & 0xf8) | 8;
                    self.set_stage(stage);
                } else {
                    // The native reads `0x506dc8 + stage`, past the four
                    // entries for a stage above 3.
                    let country = SPECIAL_COUNTRIES
                        .get(usize::from(stage))
                        .copied()
                        .unwrap_or(0);
                    self.cfg.cursor_168 = splice_low(self.cfg.cursor_168, country);
                    self.cfg.cursor_168 = (self.cfg.cursor_168 & 0xf) | 0x60;
                }
                return true;
            }
            enabled += 1;
        }
        false
    }

    fn set_stage(&mut self, stage: u8) {
        self.cfg.cursor_168 = (stage << 4) | (self.cfg.cursor_168 & 0xf);
    }

    /// Whether the stage nibble has run past the event: past the configured
    /// count at a set difficulty, past stage 3 otherwise.
    fn stages_exhausted(&self) -> bool {
        if self.difficulty() != 0 {
            self.stage() >= self.cfg.stage_count_165 & 0xf
        } else {
            self.cfg.cursor_168 & 0xf0 >= 0x30
        }
    }

    fn next_stage_nibble(&mut self) {
        let cursor = self.cfg.cursor_168;
        self.cfg.cursor_168 = ((cursor & 0xf0).wrapping_add(0x10)) ^ (cursor & 0xf);
    }

    /// `FUN_00437090`: clear the result times and move to the next country's
    /// first stage, setting its stage count to 6, or 7 for a special country.
    /// Refused in the rally school, past the last country (7 at a set
    /// difficulty, 5 otherwise) or when `DAT_004ed7d8` is clear.
    pub fn next_country(&mut self) -> bool {
        self.clear_times();
        if self.globals.school_d3ca88 {
            return false;
        }
        let country = self.cfg.cursor_168 & 0xf;
        let limit = if self.difficulty() != 0 { 7 } else { 5 };
        if country >= limit || !self.globals.continues_4ed7d8 {
            return false;
        }
        self.cfg.cursor_168 &= 0xf;
        let cursor = self.cfg.cursor_168;
        self.cfg.cursor_168 = splice_low(cursor, cursor.wrapping_add(1));
        let count = 6 + u8::from(special_country(self.cfg.cursor_168 & 0xf));
        self.cfg.stage_count_165 = splice_low(self.cfg.stage_count_165, count);
        true
    }

    /// `FUN_00436ff0`: the single-rally step. In the special series the
    /// counter picks the next special country (four of them); otherwise the
    /// next stage of the same country.
    fn advance_single(&mut self) -> bool {
        if self.cfg.mode_166 & 0x20 != 0 {
            let counter = self.counter();
            if counter >= 4 {
                return false;
            }
            self.cfg.cursor_168 =
                splice_low(self.cfg.cursor_168, SPECIAL_COUNTRIES[usize::from(counter)]);
            return true;
        }
        self.next_stage_nibble();
        !self.stages_exhausted()
    }

    /// `FUN_00436f00`: step the event to its next stage. Returns 0 when the
    /// event is over (the hub then ends the session).
    pub fn advance(&mut self) -> bool {
        if !self.globals.school_d3ca88 {
            let head_to_head_done = self.head_to_head() && self.globals.sequence_551ad8 == 1;
            if !head_to_head_done && (self.globals.special_55001c || self.globals.restart_550018) {
                return true;
            }
            if !self.globals.continues_4ed7d8 {
                return false;
            }
        }
        self.cfg.progress_167 = splice_counter(self.cfg.progress_167, self.counter() + 1);
        match self.mode() {
            0 => {
                self.next_stage_nibble();
                if self.stages_exhausted() {
                    self.next_country()
                } else {
                    true
                }
            }
            1 => self.advance_single(),
            2 => self.pick_selected_stage(),
            _ => false,
        }
    }

    /// The first event of a session (`FUN_00436be0`, counter 0): championship
    /// from the first country, single rally from its first stage (or the
    /// first special country's stage 6), or the first selected stage; then
    /// every result slot is reset to its own index.
    fn pick_first_event(&mut self) {
        match self.mode() {
            0 => self.cfg.cursor_168 = 0,
            1 if self.cfg.mode_166 & 0x20 != 0 => {
                let cursor = splice_low(self.cfg.cursor_168, SPECIAL_COUNTRIES[0]);
                self.cfg.cursor_168 = (cursor & 0xf) | 0x60;
            }
            1 => self.cfg.cursor_168 &= 0xf,
            2 => {
                self.pick_selected_stage();
            }
            _ => {}
        }
        for (index, slot) in self.cfg.results_16c.iter_mut().enumerate() {
            slot[..4].fill(0);
            slot[0xc] = splice_low(slot[0xc], index as u8) & 0xef;
            slot[0xd] = 0;
            slot[4..0xc].fill(0);
        }
    }

    /// The state-1 handler `FUN_00436be0`: choose or continue the event, then
    /// route to the welcome screen (`2`) or straight to loading (`3`). A rally
    /// school session goes to `2` untouched. A pending restart rewinds the
    /// stage only. `slot_kinds` is byte 0 of each `0x506e48` record; at
    /// difficulty 0 the kinds 0, 6, 9 and 10 mark their slot `+0xd = 0xff`.
    pub fn boot(&mut self, slot_kinds: Option<&[u8; 16]>) -> GameState {
        if self.globals.school_d3ca88 {
            return state(2);
        }
        if self.globals.restart_550018 {
            self.cfg.cursor_168 &= 0xf;
            self.cfg.progress_167 &= 0xc0;
            self.globals.restart_550018 = false;
        } else {
            if self.cfg.flags_000 & 1 == 0 {
                if self.cfg.mode_166 & 0x80 != 0 {
                    self.cfg.mode_166 &= 0x7f;
                    self.advance();
                    self.globals.word_4f6848 = 0;
                } else if self.counter() == 0 {
                    self.pick_first_event();
                }
            }
            if let Some(kinds) = slot_kinds {
                for (slot, &kind) in self.cfg.results_16c.iter_mut().zip(kinds) {
                    if self.cfg.difficulty_018 & 0x18 == 0 && matches!(kind, 0 | 6 | 9 | 10) {
                        slot[0xd] = 0xff;
                    }
                }
            }
        }
        if self.show_welcome() {
            state(2)
        } else {
            state(3)
        }
    }

    /// The front end's race hand-off (`FUN_004c20f0` phase 2, result 3):
    /// `DAT_00506228 = 0`, `DAT_00d3ca84 = 1`.
    pub fn start_race_session(&mut self) {
        self.globals.frontend_506228 = false;
        self.globals.race_session_d3ca84 = true;
    }

    /// The front end's rally-school hand-off (result 4): `DAT_00506228 = 0`,
    /// `DAT_00d3ca88 = 1`.
    pub fn start_school_session(&mut self) {
        self.globals.frontend_506228 = false;
        self.globals.school_d3ca88 = true;
    }

    /// The hub's quit block `0x438c2e`: `DAT_004f6848 = 1`, `FUN_00439880`
    /// (back to the front end, both sessions over) and `FUN_004440a0(0)`.
    pub fn end_session(&mut self) {
        self.globals.word_4f6848 = 1;
        self.globals.frontend_506228 = true;
        self.globals.race_session_d3ca84 = false;
        self.globals.school_d3ca88 = false;
        self.globals.reseed_requested = true;
    }

    /// The FSM is ticked only during a race or rally-school session
    /// (`FUN_004c1e00`).
    pub fn in_session(&self) -> bool {
        !self.globals.frontend_506228
            && (self.globals.race_session_d3ca84 || self.globals.school_d3ca88)
    }

    /// The post-race hub's restart block: rewind the event to the stage that
    /// was restarted and clear the result times.
    pub fn rewind_for_restart(&mut self) {
        match self.mode() {
            0 => {
                let counter = self.counter().wrapping_sub(self.stage());
                self.cfg.progress_167 = splice_counter(self.cfg.progress_167, counter);
                self.cfg.cursor_168 &= 0xf;
            }
            1 if self.cfg.mode_166 & 0x20 != 0 => {
                self.cfg.progress_167 &= 0xc0;
                self.cfg.cursor_168 = splice_low(self.cfg.cursor_168, SPECIAL_COUNTRIES[0]);
            }
            1 => {
                self.cfg.progress_167 &= 0xc0;
                self.cfg.cursor_168 &= 0xf;
            }
            _ => {}
        }
        self.clear_times();
    }
}

fn state(id: u8) -> GameState {
    GameState::new(id).expect("an event-flow state id is in range")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(mode: u8, cursor: u8) -> EventState {
        EventState {
            cfg: EventConfig {
                mode_166: mode,
                cursor_168: cursor,
                ..EventConfig::default()
            },
            globals: EventGlobals {
                continues_4ed7d8: true,
                ..EventGlobals::default()
            },
        }
    }

    #[test]
    fn a_championship_starts_at_the_first_country_and_shows_its_welcome() {
        let mut e = event(0, 0x53);
        e.cfg.results_16c[2][0] = 9;
        e.cfg.results_16c[2][0xc] = 0x1f;
        assert_eq!(e.boot(None).id(), 2);
        assert_eq!(e.cfg.cursor_168, 0);
        assert_eq!(e.cfg.results_16c[2][0], 0);
        assert_eq!(e.cfg.results_16c[2][0xc], 0x02);
    }

    #[test]
    fn a_championship_runs_three_stages_then_the_next_country() {
        let mut e = event(0, 0x00);
        assert!(e.advance());
        assert_eq!((e.cfg.cursor_168, e.counter()), (0x10, 1));
        assert!(!e.show_welcome());
        assert!(e.advance());
        assert_eq!(e.cfg.cursor_168, 0x20);
        assert!(e.advance());
        assert_eq!(e.cfg.cursor_168, 0x01);
        assert_eq!(e.cfg.stage_count_165 & 0xf, 7);
        assert!(e.show_welcome());
        // At difficulty 0 the last country is 5.
        let mut fourth = event(0, 0x24);
        assert!(fourth.advance());
        assert_eq!(fourth.cfg.cursor_168, 0x05);
        let mut last = event(0, 0x25);
        assert!(!last.advance());
        // The results screens can stop the championship.
        let mut stopped = event(0, 0x21);
        stopped.globals.continues_4ed7d8 = false;
        assert!(!stopped.advance());
    }

    #[test]
    fn a_set_difficulty_uses_the_stage_count_and_seven_countries() {
        let mut e = event(0, 0x50);
        e.cfg.difficulty_018 = 0x08;
        e.cfg.stage_count_165 = 6;
        assert!(e.advance());
        assert_eq!(e.cfg.cursor_168, 0x01);
        assert_eq!(e.cfg.stage_count_165, 6 + 1);
        let mut end = event(0, 0x56);
        end.cfg.difficulty_018 = 0x08;
        end.cfg.stage_count_165 = 6;
        assert!(end.advance());
        assert_eq!(end.cfg.cursor_168, 0x07);
        let mut past = event(0, 0x57);
        past.cfg.difficulty_018 = 0x08;
        past.cfg.stage_count_165 = 6;
        assert!(!past.advance());
    }

    #[test]
    fn a_single_rally_stays_in_its_country() {
        let mut e = event(1, 0x23);
        assert_eq!(e.boot(None).id(), 2);
        assert_eq!(e.cfg.cursor_168, 0x03);
        assert!(e.advance() && e.advance());
        assert_eq!(e.cfg.cursor_168, 0x23);
        assert!(!e.advance());
        assert_eq!(e.cfg.cursor_168, 0x33);
    }

    #[test]
    fn the_special_series_visits_the_four_special_countries() {
        let mut e = event(1 | 0x20, 0x00);
        e.boot(None);
        assert_eq!(e.cfg.cursor_168, 0x61);
        for country in [3, 5, 7] {
            assert!(e.advance());
            assert_eq!(e.cfg.cursor_168, 0x60 | country);
        }
        assert!(!e.advance());
    }

    #[test]
    fn selected_stages_run_in_mask_order_and_then_end() {
        let mut e = event(2, 0x03);
        e.cfg.stage_mask_164 = 0b0010_0100;
        e.cfg.stage_count_165 = 6;
        assert_eq!(e.boot(None).id(), 3);
        assert_eq!(e.cfg.cursor_168, 0x23);
        assert!(e.advance());
        assert_eq!(e.cfg.cursor_168, 0x53);
        assert!(!e.advance());
        // A network session always takes the first enabled stage.
        let mut net = event(2, 0x03);
        net.cfg.stage_mask_164 = 0b0010_0100;
        net.cfg.stage_count_165 = 6;
        net.cfg.progress_167 = 0x45;
        net.globals.network_d3ca94 = true;
        assert!(net.pick_selected_stage());
        assert_eq!((net.cfg.cursor_168, net.cfg.progress_167), (0x23, 0x40));
    }

    #[test]
    fn selected_secret_and_special_stages_rewrite_the_country() {
        let mut secret = event(2 | 0x60, 0x03);
        secret.cfg.stage_mask_164 = 0b10;
        secret.cfg.stage_count_165 = 4;
        assert!(secret.pick_selected_stage());
        assert_eq!(secret.cfg.cursor_168, 0x18);
        let mut special = event(2 | 0x20, 0x00);
        special.cfg.stage_mask_164 = 0b100;
        special.cfg.stage_count_165 = 4;
        assert!(special.pick_selected_stage());
        assert_eq!(special.cfg.cursor_168, 0x65);
    }

    #[test]
    fn a_pending_advance_or_restart_takes_its_own_path() {
        let mut pending = event(0x80, 0x10);
        pending.cfg.progress_167 = 1;
        pending.globals.word_4f6848 = 7;
        pending.boot(None);
        assert_eq!(pending.cfg.mode_166, 0);
        assert_eq!(pending.cfg.cursor_168, 0x20);
        assert_eq!(pending.globals.word_4f6848, 0);
        let mut restart = event(0, 0x24);
        restart.cfg.progress_167 = 0xc5;
        restart.globals.restart_550018 = true;
        assert_eq!(restart.boot(None).id(), 2);
        assert_eq!(
            (restart.cfg.cursor_168, restart.cfg.progress_167),
            (0x04, 0xc0)
        );
        assert!(!restart.globals.restart_550018);
        // The demo keeps its preset event.
        let mut demo = event(2, 0x13);
        demo.cfg.flags_000 = 1;
        demo.boot(None);
        assert_eq!(demo.cfg.cursor_168, 0x13);
    }

    #[test]
    fn the_slot_pass_marks_four_kinds_at_difficulty_zero() {
        let mut kinds = [1u8; 16];
        kinds[0] = 0;
        kinds[1] = 6;
        kinds[2] = 9;
        kinds[3] = 10;
        kinds[4] = 11;
        let mut e = event(0, 0x10);
        e.cfg.progress_167 = 1;
        e.boot(Some(&kinds));
        let marked: Vec<_> = e.cfg.results_16c.iter().map(|s| s[0xd] == 0xff).collect();
        assert_eq!(&marked[..6], &[true, true, true, true, false, false]);
        let mut hard = event(0, 0x10);
        hard.cfg.progress_167 = 1;
        hard.cfg.difficulty_018 = 0x10;
        hard.boot(Some(&kinds));
        assert!(hard.cfg.results_16c.iter().all(|s| s[0xd] == 0));
    }

    #[test]
    fn the_advance_gate_short_circuits_on_special_and_restart() {
        let mut special = event(0, 0x10);
        special.globals.special_55001c = true;
        assert!(special.advance());
        assert_eq!(special.cfg.cursor_168, 0x10, "nothing advances");
        // A finished head-to-head sequence skips that short-cut.
        let mut h2h = event(8, 0x10);
        h2h.globals.special_55001c = true;
        h2h.globals.sequence_551ad8 = 1;
        assert!(h2h.advance());
        assert_eq!(h2h.cfg.cursor_168, 0x20);
        // The school always advances, and only by its own mode.
        let mut school = event(3, 0x10);
        school.globals.school_d3ca88 = true;
        school.globals.continues_4ed7d8 = false;
        assert!(!school.advance());
        assert_eq!(school.counter(), 1);
    }

    #[test]
    fn the_restart_rewind_returns_to_the_event_start() {
        let mut champ = event(0, 0x23);
        champ.cfg.progress_167 = 0x47;
        champ.cfg.results_16c[0][1] = 5;
        champ.rewind_for_restart();
        assert_eq!((champ.cfg.progress_167, champ.cfg.cursor_168), (0x45, 0x03));
        assert_eq!(champ.cfg.results_16c[0][1], 0);
        let mut special = event(1 | 0x20, 0x67);
        special.cfg.progress_167 = 3;
        special.rewind_for_restart();
        assert_eq!(
            (special.cfg.progress_167, special.cfg.cursor_168),
            (0, 0x61)
        );
        let mut stages = event(2, 0x23);
        stages.rewind_for_restart();
        assert_eq!(stages.cfg.cursor_168, 0x23);
    }

    #[test]
    fn the_config_round_trips_through_the_live_block() {
        let mut live = vec![0u8; 0x6c4];
        let mut e = event(0x42, 0x35);
        e.cfg.flags_000 = 1;
        e.cfg.difficulty_018 = 0x18;
        e.cfg.stage_mask_164 = 0x0f;
        e.cfg.stage_count_165 = 7;
        e.cfg.progress_167 = 0x83;
        e.cfg.results_16c[15][15] = 9;
        e.cfg.options_025 = 0x80;
        e.cfg.school_6c0 = 0x2b;
        e.cfg.store_live(&mut live);
        assert_eq!((live[0x166], live[0x168], live[0x26b]), (0x42, 0x35, 9));
        assert_eq!(EventConfig::from_live(&live), e.cfg);
    }

    #[test]
    fn the_arrangement_predicates() {
        assert!(event(4, 0).split_screen() && event(0xc, 0).split_screen());
        assert!(!event(8, 0).split_screen() && event(8, 0).head_to_head());
        let mut school = event(8, 0);
        school.globals.school_d3ca88 = true;
        assert!(!school.head_to_head());
    }
}
