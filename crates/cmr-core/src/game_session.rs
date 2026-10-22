//! The whole-program session loop: the composition that runs the top-level FSM
//! from boot through the menu and load states into the in-race band and back out
//! to the terminal quit, driving one [`crate::game_driver::FrameDriver`] with a
//! band-routed handler.
//!
//! The dispatch needs a total handler `GameState -> GameState` (the next state
//! each state's update handler returns). This module routes that handler by band
//! from the pieces already reconstructed:
//!
//! - **`0` / `0xc`** (terminal): the shipped handlers are `xor eax,eax; ret`, so
//!   both return `0` - the quit funnel `0xc -> 0` and the stopped self-loop.
//! - **`1..=4`** (boot / menu / pre-load / load-arm): the recovered decision
//!   tables ([`crate::game_menu_flow::menu_flow_next`]) over a per-frame
//!   [`MenuFlowInputs`].
//! - **`5..=0xb`** (in-race and post-race/results): a caller-supplied handler.
//!   The in-race transition triggers are per-state race logic recovered
//!   separately (the fixed-step sim in `race_state`/`vertical_path`); this loop
//!   defers them exactly as the dispatch does, so a backend plugs the race sim in
//!   without this module modelling it.
//!
//! The [`FrameDriver`] already reproduces `FUN_00443e90`: first-frame boot
//! seeding, the in-race catch-up loop vs the single menu dispatch, and the
//! render-skip gate. This session is the faithful top-level `run` around it; the
//! native per-state handler bodies for the gameplay band and the real backend
//! present remain the caller's. See `rea.official-eu.game-fsm`,
//! `rea.official-eu.game-fsm-menu`.

use crate::game_driver::{FrameDriver, FramePlan};
use crate::game_menu_flow::{MenuFlowInputs, menu_flow_next};
use crate::game_state::GameState;
use crate::host_loop::HostClock;

/// Which band the live state is in this frame, so a caller knows whether its
/// in-race handler was consulted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionBand {
    /// A terminal state (`0` stopped, or `0xc` the quit funnel).
    Terminal,
    /// The boot/menu/pre-load/load-arm band (`1..=4`), driven by the recovered
    /// menu-flow decision tables.
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
    /// The underlying driver's plan (dispatch count, live state, render target).
    pub plan: FramePlan,
    /// The band of the state the frame dispatched *from* (whose handler ran).
    pub band: SessionBand,
}

/// The top-level game session: a [`FrameDriver`] plus the band-routing that
/// turns the reconstructed pieces into one runnable loop.
#[derive(Debug, Clone)]
pub struct GameSession {
    driver: FrameDriver,
}

impl Default for GameSession {
    fn default() -> Self {
        Self::new()
    }
}

impl GameSession {
    /// A fresh session whose first frame seeds the live state from the boot
    /// handler (FSM state `1`).
    pub fn new() -> Self {
        GameSession {
            driver: FrameDriver::new(),
        }
    }

    /// The live FSM state.
    pub const fn current(&self) -> GameState {
        self.driver.current()
    }

    /// Whether the session has reached the terminal stopped state (`0`), i.e. the
    /// quit funnel has fully drained.
    pub fn is_stopped(&self) -> bool {
        self.driver.current().is_terminal()
    }

    /// Run one frame. `menu_inputs` supplies the recovered predicate inputs for
    /// the menu band this frame; `in_race` is consulted for the gameplay band
    /// (`5..=0xb`) to produce that state's next state. Terminal states resolve to
    /// the stopped state without consulting either.
    ///
    /// The band reported is that of the state the frame dispatched from; on the
    /// very first frame the driver also seeds from the boot handler (state `1`),
    /// which routes through the menu band.
    pub fn run_frame<C, H>(
        &mut self,
        clock: &C,
        menu_inputs: MenuFlowInputs,
        mut in_race: H,
    ) -> SessionFrame
    where
        C: HostClock,
        H: FnMut(GameState) -> GameState,
    {
        let dispatched_from = self.driver.current();
        let plan = self
            .driver
            .run_frame(clock, |state| match SessionBand::of(state) {
                SessionBand::Terminal => GameState::stopped(),
                SessionBand::Menu => {
                    menu_flow_next(state, menu_inputs).expect("states 1..=4 are the menu band")
                }
                SessionBand::Gameplay => in_race(state),
            });
        SessionFrame {
            plan,
            band: SessionBand::of(dispatched_from),
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
    impl FakeClock {
        fn at(start: u32) -> Self {
            FakeClock {
                now: Cell::new(start),
            }
        }
    }
    impl HostClock for FakeClock {
        fn millis(&self) -> u32 {
            self.now.get()
        }
    }

    /// Menu inputs that drive the direct (non-interactive) path straight through
    /// boot -> pre-load -> load into state 5: no front-end session, nothing
    /// pending, no resume, race-type bit clear.
    fn direct_to_race() -> MenuFlowInputs {
        MenuFlowInputs::default()
    }

    #[test]
    fn boot_runs_straight_through_the_menu_band_to_the_in_race_band() {
        let clock = FakeClock::at(0);
        let mut session = GameSession::new();

        // Frame 1: boot seeds state 1 -> 3 (direct path), then dispatches 3 -> 4.
        let frame = session.run_frame(&clock, direct_to_race(), |s| s);
        assert_eq!(frame.band, SessionBand::Menu);
        assert_eq!(session.current(), state(4));

        // Frame 2: state 4 load/arm -> 5 (direct race-type path).
        let frame = session.run_frame(&clock, direct_to_race(), |s| s);
        assert_eq!(frame.band, SessionBand::Menu);
        assert_eq!(session.current(), state(5));
        assert!(session.current().is_in_race());
    }

    #[test]
    fn the_in_race_handler_drives_the_gameplay_band() {
        let clock = FakeClock::at(0);
        let mut session = GameSession::new();
        // Advance to state 5 (boot->3->4->5) with the direct path.
        session.run_frame(&clock, direct_to_race(), |s| s); // -> 4
        session.run_frame(&clock, direct_to_race(), |s| s); // -> 5
        assert_eq!(session.current(), state(5));

        // Now the gameplay band consults the in-race handler. Self-loop once,
        // then finish to the post-race state 0xb (a recovered 5 edge? 5 -> {5,8,
        // 9,0xa}; 0xb is reached from 8/9/0xa). Drive 5 -> 9 -> 0xb.
        let frame = session.run_frame(&clock, direct_to_race(), |s| {
            assert_eq!(s, state(5));
            state(9)
        });
        assert_eq!(frame.band, SessionBand::Gameplay);
        assert_eq!(session.current(), state(9));

        let frame = session.run_frame(&clock, direct_to_race(), |s| {
            assert_eq!(s, state(9));
            state(0xB)
        });
        assert_eq!(frame.band, SessionBand::Gameplay);
        assert_eq!(session.current(), state(0xB));
    }

    #[test]
    fn the_quit_funnel_drains_to_the_terminal_state() {
        let clock = FakeClock::at(0);
        let mut session = GameSession::new();
        session.run_frame(&clock, direct_to_race(), |s| s); // 1->3, ->4
        session.run_frame(&clock, direct_to_race(), |s| s); // ->5

        // From the gameplay band drive 5 -> 0xb (via 9), then 0xb -> 0xc (quit).
        session.run_frame(&clock, direct_to_race(), |_| state(9)); // ->9
        session.run_frame(&clock, direct_to_race(), |_| state(0xB)); // ->0xb
        let frame = session.run_frame(&clock, direct_to_race(), |_| state(0xC)); // ->0xc
        assert_eq!(frame.band, SessionBand::Gameplay);
        assert_eq!(session.current(), state(0xC));
        assert!(!session.is_stopped());

        // The terminal 0xc handler drains to the stopped state 0 without
        // consulting the in-race handler (it would panic if called).
        let frame = session.run_frame(&clock, direct_to_race(), |_| {
            panic!("terminal state must not consult the in-race handler")
        });
        assert_eq!(frame.band, SessionBand::Terminal);
        assert_eq!(session.current(), state(0));
        assert!(session.is_stopped());

        // The stopped state self-loops.
        session.run_frame(&clock, direct_to_race(), |_| {
            panic!("stopped state must not consult the in-race handler")
        });
        assert!(session.is_stopped());
    }

    #[test]
    fn an_interactive_menu_can_quit_before_any_race() {
        let clock = FakeClock::at(0);
        let mut session = GameSession::new();

        // An interactive front-end session seeds boot -> 2 and holds in the menu.
        let interactive = MenuFlowInputs {
            frontend_active: true,
            root_menu_ready: false,
            ..Default::default()
        };
        let frame = session.run_frame(&clock, interactive, |s| s);
        assert_eq!(frame.band, SessionBand::Menu);
        assert_eq!(session.current(), state(2));

        // The user requests quit: the front-end branch returns 0xc, which then
        // drains to the stopped state.
        let quitting = MenuFlowInputs {
            frontend_active: true,
            root_menu_ready: true,
            screen_graph_ready: false,
            ..Default::default()
        };
        session.run_frame(&clock, quitting, |s| s);
        assert_eq!(session.current(), state(0xC));
        session.run_frame(&clock, direct_to_race(), |s| s);
        assert!(session.is_stopped());
    }
}
