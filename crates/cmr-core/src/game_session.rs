//! The whole-program session loop: the per-frame driver `FUN_004c1e00` around
//! the top-level FSM.
//!
//! While `DAT_00506228` is set the descriptor front end runs and the FSM is not
//! ticked ([`SessionBand::FrontEnd`]). The front end's race or rally-school
//! hand-off ([`EventState::start_race_session`],
//! [`EventState::start_school_session`]) clears it, and from then on every frame
//! runs the tick driver `FUN_00443e90` ([`FrameDriver`]). The first tick of a
//! session seeds the FSM: `DAT_004ed7d8 = 1` and the state-1 handler. When the
//! post-race hub ends the session ([`EventState::end_session`]), the init-done
//! flag is cleared, so the next session seeds again, and the front end resumes.
//!
//! The FSM's handlers are routed by band:
//!
//! - **`0` / `0xc`** (terminal): the shipped handlers are `xor eax,eax; ret`, so
//!   both return `0`.
//! - **`1..=4`**: [`crate::game_menu_flow::menu_flow_next`] over the
//!   [`EventState`] and a per-frame [`MenuFlowInputs`].
//! - **`5..=0xb`**: a caller-supplied handler, given the [`EventState`] too.
//!
//! See `rea.official-eu.game-fsm`, `rea.official-eu.game-fsm-menu`,
//! `rea.official-eu.event-flow`.

use crate::event_flow::EventState;
use crate::game_driver::{FrameDriver, FramePlan};
use crate::game_menu_flow::{MenuFlowInputs, menu_flow_next};
use crate::game_state::GameState;
use crate::host_loop::HostClock;

/// What ran this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionBand {
    /// No session: the descriptor front end ran instead of the FSM.
    FrontEnd,
    /// A terminal state (`0` stopped, or `0xc` the quit funnel).
    Terminal,
    /// The pre-race band (`1..=4`).
    Menu,
    /// The in-race / post-race band (`5..=0xb`), driven by the supplied handler.
    Gameplay,
}

impl SessionBand {
    fn of(state: GameState) -> Self {
        match state.id() {
            0 | 0xC => SessionBand::Terminal,
            1..=4 => SessionBand::Menu,
            _ => SessionBand::Gameplay,
        }
    }
}

/// What one [`GameSession::run_frame`] produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionFrame {
    /// The driver's plan; `None` when the front end ran.
    pub plan: Option<FramePlan>,
    /// The band of the state the frame dispatched *from* (whose handler ran).
    pub band: SessionBand,
}

/// The top-level game session.
#[derive(Debug, Clone)]
pub struct GameSession {
    driver: FrameDriver,
    event: EventState,
}

impl Default for GameSession {
    fn default() -> Self {
        Self::new()
    }
}

impl GameSession {
    /// The session at process start: the front end is up (`DAT_00506228 = 1`)
    /// and the FSM is not yet seeded.
    pub fn new() -> Self {
        let mut event = EventState::default();
        event.globals.frontend_506228 = true;
        Self::with_event(event)
    }

    /// A session over a prepared config block and globals.
    pub fn with_event(event: EventState) -> Self {
        GameSession {
            driver: FrameDriver::new(),
            event,
        }
    }

    /// The live FSM state.
    pub const fn current(&self) -> GameState {
        self.driver.current()
    }

    /// The event bookkeeping and session flags.
    pub const fn event(&self) -> &EventState {
        &self.event
    }

    /// The event bookkeeping and session flags.
    pub fn event_mut(&mut self) -> &mut EventState {
        &mut self.event
    }

    /// Whether the FSM has reached the stopped state (`0`).
    pub fn is_stopped(&self) -> bool {
        self.driver.current().is_terminal()
    }

    /// Run one frame. With no session the front end owns the frame and the
    /// FSM is untouched. Otherwise the tick driver runs: `menu_inputs` feeds the
    /// pre-race band and `in_race` produces each gameplay state's next state.
    pub fn run_frame<C, H>(
        &mut self,
        clock: &C,
        menu_inputs: MenuFlowInputs,
        mut in_race: H,
    ) -> SessionFrame
    where
        C: HostClock,
        H: FnMut(GameState, &mut EventState) -> GameState,
    {
        if !self.event.in_session() {
            return SessionFrame {
                plan: None,
                band: SessionBand::FrontEnd,
            };
        }
        if !self.driver.is_initialized() {
            self.event.globals.continues_4ed7d8 = true;
        }
        let dispatched_from = self.driver.current();
        let seeding = !self.driver.is_initialized();
        let event = &mut self.event;
        let plan = self
            .driver
            .run_frame(clock, |state| match SessionBand::of(state) {
                SessionBand::Terminal => GameState::stopped(),
                SessionBand::Menu => menu_flow_next(state, menu_inputs, event)
                    .expect("states 1..=4 are the menu band"),
                SessionBand::Gameplay | SessionBand::FrontEnd => in_race(state, event),
            });
        if std::mem::take(&mut self.event.globals.reseed_requested) {
            self.driver.clear_init();
        }
        SessionFrame {
            plan: Some(plan),
            band: if seeding {
                SessionBand::Menu
            } else {
                SessionBand::of(dispatched_from)
            },
        }
    }

    /// Report the planned render's outcome to the driver's render-skip gate.
    pub fn report_render(&mut self, overran: bool) {
        self.driver.report_render(overran);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn state(id: u8) -> GameState {
        GameState::new(id).expect("valid state id")
    }

    struct FakeClock {
        now: Cell<u32>,
    }
    impl HostClock for FakeClock {
        fn millis(&self) -> u32 {
            self.now.get()
        }
    }
    fn clock() -> FakeClock {
        FakeClock { now: Cell::new(0) }
    }

    /// A one-stage selected-stages event (mode 2, stage 2 of the mask).
    fn one_stage() -> EventState {
        let mut event = EventState::default();
        event.cfg.mode_166 = 2;
        event.cfg.stage_mask_164 = 0b100;
        event.cfg.stage_count_165 = 6;
        event.globals.frontend_506228 = true;
        event
    }

    #[test]
    fn the_front_end_owns_the_frame_until_a_session_starts() {
        let clock = clock();
        let mut session = GameSession::with_event(one_stage());
        let frame = session.run_frame(&clock, MenuFlowInputs::default(), |_, _| {
            panic!("no session")
        });
        assert_eq!(frame.band, SessionBand::FrontEnd);
        assert!(frame.plan.is_none());
        assert!(!session.driver.is_initialized());
    }

    #[test]
    fn a_race_session_seeds_from_state_one_and_runs_to_the_race() {
        let clock = clock();
        let mut session = GameSession::with_event(one_stage());
        session.event_mut().start_race_session();
        // The seed frame runs the state-1 handler only: the first selected
        // stage is chosen and mode 2 skips the welcome screen.
        let frame = session.run_frame(&clock, MenuFlowInputs::default(), |s, _| s);
        assert_eq!(frame.band, SessionBand::Menu);
        assert_eq!(session.current(), state(3));
        assert_eq!(session.event().cfg.cursor_168, 0x20);
        assert!(session.event().globals.continues_4ed7d8);
        session.run_frame(&clock, MenuFlowInputs::default(), |s, _| s); // -> 4
        session.run_frame(&clock, MenuFlowInputs::default(), |s, _| s); // -> 5
        assert_eq!(session.current(), state(5));
    }

    #[test]
    fn the_hub_ends_the_session_and_the_next_one_seeds_again() {
        let clock = clock();
        let mut session = GameSession::with_event(one_stage());
        session.event_mut().start_race_session();
        for _ in 0..3 {
            session.run_frame(&clock, MenuFlowInputs::default(), |s, _| s);
        }
        assert_eq!(session.current(), state(5));
        // The handler plays the hub's end of the event.
        let frame = session.run_frame(&clock, MenuFlowInputs::default(), |_, event| {
            event.end_session();
            state(0xC)
        });
        assert_eq!(frame.band, SessionBand::Gameplay);
        assert!(!session.driver.is_initialized());
        let frame = session.run_frame(&clock, MenuFlowInputs::default(), |_, _| {
            panic!("back in the front end")
        });
        assert_eq!(frame.band, SessionBand::FrontEnd);

        // A new session seeds from state 1 again.
        session.event_mut().cfg.progress_167 = 0;
        session.event_mut().start_race_session();
        let frame = session.run_frame(&clock, MenuFlowInputs::default(), |s, _| s);
        assert_eq!(frame.band, SessionBand::Menu);
        assert_eq!(session.current(), state(3));
    }

    #[test]
    fn the_terminal_states_drain_to_stopped() {
        let clock = clock();
        let mut session = GameSession::with_event(one_stage());
        session.event_mut().start_race_session();
        for _ in 0..3 {
            session.run_frame(&clock, MenuFlowInputs::default(), |s, _| s);
        }
        session.run_frame(&clock, MenuFlowInputs::default(), |_, _| state(9));
        session.run_frame(&clock, MenuFlowInputs::default(), |_, _| state(0xB));
        session.run_frame(&clock, MenuFlowInputs::default(), |_, _| state(0xC));
        let frame = session.run_frame(&clock, MenuFlowInputs::default(), |_, _| {
            panic!("terminal state must not consult the in-race handler")
        });
        assert_eq!(frame.band, SessionBand::Terminal);
        assert!(session.is_stopped());
    }
}
