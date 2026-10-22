//! The menu/load transition logic of the top-level FSM states `1..=4`
//! (reconstruction item F04/G01 boundary): the decision tables the boot,
//! front-end, pre-load, and load/arm handlers use to pick their next state.
//!
//! The dispatch ([`crate::game_state`]) makes each state's update handler return
//! the next state. For the four pre-gameplay handlers those returns are pure
//! branches over a handful of session-struct fields and controller return
//! values (`rea.official-eu.game-fsm-menu`, asm-recovered):
//!
//! - **State 1** boot seed (`FUN_00436be0`): `frontend_active` -> `2`; else a
//!   "show menu" predicate (`FUN_0048df30`) -> `2`; else -> `3`.
//! - **State 2** front-end loop (`FUN_00437160`): with no interactive session,
//!   an intro-sequence advance (`FUN_0048dde0`) -> `3` else stay `2`; with an
//!   interactive session, the root-menu controller (`FUN_00470e20`) not ready ->
//!   stay `2`, else the screen-graph stepper (`FUN_0046f650`) failing or a
//!   menu-status of quit (`struct[0x6c0] & 7 == 6`) -> `0xc` (calling the app
//!   shutdown service), else confirmed -> `3`.
//! - **State 3** pre-load gate (`FUN_004371c0`): no pending interstitial
//!   (`FUN_0047a190`) -> `4`; else the pre-load screen finished (`FUN_00476f70`)
//!   -> `4`; else stay `3`.
//! - **State 4** load/arm (`FUN_00437200`): `frontend_active` -> `7` (skips the
//!   real loaders); else a resume/championship init pending (`DAT_00514478`) ->
//!   `6`; else the race-type bit (`struct[0] & 1`) -> `8`, else -> `5`.
//!
//! This module reconstructs those four decision tables verbatim over
//! [`MenuFlowInputs`] - the external predicate results (the uncarved controller
//! callees and the session-struct bits) are **inputs**, not reimplemented here.
//! Every edge is checked against the recovered transition graph
//! ([`crate::game_state::GameState::is_known_transition`]).
//!
//! It also bridges the recovered front-end screen-graph controller
//! ([`crate::frontend::FrontendState`]) to state 2: the recovery shows that
//! controller *is* state 2's interactive branch (`FUN_00470e20` + `FUN_0046f650`),
//! so [`frontend_menu_signal`] maps the controller's own outcome (stay in the
//! menu / leave toward the race / request exit) onto the state-2 next state. That
//! correspondence is an inference drawn from the matching call sites, not a
//! proven identity of the uncarved callees.

use crate::frontend::{FrontendContinuation, FrontendState};
use crate::game_state::GameState;

/// The external predicate results the menu/load handlers (`1..=4`) branch on,
/// one tick's worth. Each field names the recovered controller callee or
/// session-struct bit it stands in for; the uncarved callees compute these in
/// the shipped build, so here they are inputs.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MenuFlowInputs {
    /// An interactive front-end session is active (`DAT_00d3ca88 != 0`): menus
    /// run and the real stage/car loading is suppressed.
    pub frontend_active: bool,
    /// State 1 only: the "show the front-end menu" predicate (`FUN_0048df30`);
    /// when set with no interactive session, boot routes to the menu (`2`)
    /// instead of the pre-load gate (`3`).
    pub show_menu: bool,
    /// State 2, non-interactive path: the intro/attract sequencer advances
    /// (`FUN_0048dde0 != 0`), routing to the pre-load gate (`3`).
    pub intro_advance: bool,
    /// State 2, interactive path: the root-menu controller produced a result
    /// (`FUN_00470e20 != 0`); while clear the menu stays on `2`.
    pub root_menu_ready: bool,
    /// State 2, interactive path: the screen-graph stepper is healthy
    /// (`FUN_0046f650 != 0`); clear drives the quit funnel (`0xc`).
    pub screen_graph_ready: bool,
    /// State 2, interactive path: the packed menu-status byte selected quit
    /// (`struct[0x6c0] & 7 == 6`), driving the quit funnel (`0xc`).
    pub menu_status_quit: bool,
    /// State 3: a pre-load interstitial is still pending (`FUN_0047a190 != 0`);
    /// clear advances straight to the load state (`4`).
    pub preload_pending: bool,
    /// State 3: the pending pre-load screen has finished (`FUN_00476f70 != 0`),
    /// advancing to the load state (`4`).
    pub preload_done: bool,
    /// State 4: a resume/championship init is pending (`DAT_00514478 != 0`),
    /// selecting in-race entry `6`.
    pub resume_pending: bool,
    /// State 4: the top-level race-type bit (`struct[0] & 1`), selecting in-race
    /// entry `8` when set, `5` when clear.
    pub race_type_bit: bool,
}

fn menu_state(id: u8) -> GameState {
    GameState::new(id).expect("recovered menu-flow state id is in range")
}

/// State 1 boot seed (`FUN_00436be0`): the initial live state.
pub fn boot_seed_next(inputs: MenuFlowInputs) -> GameState {
    if inputs.frontend_active || inputs.show_menu {
        menu_state(2)
    } else {
        menu_state(3)
    }
}

/// State 2 front-end loop (`FUN_00437160`): stay in the menu, advance to the
/// pre-load gate, or take the quit funnel.
pub fn frontend_loop_next(inputs: MenuFlowInputs) -> GameState {
    if !inputs.frontend_active {
        return if inputs.intro_advance {
            menu_state(3)
        } else {
            menu_state(2)
        };
    }
    if !inputs.root_menu_ready {
        return menu_state(2);
    }
    if !inputs.screen_graph_ready || inputs.menu_status_quit {
        return menu_state(0xC);
    }
    menu_state(3)
}

/// State 3 pre-load gate (`FUN_004371c0`): hold on the interstitial or advance
/// to the load state.
pub fn preload_gate_next(inputs: MenuFlowInputs) -> GameState {
    if !inputs.preload_pending || inputs.preload_done {
        menu_state(4)
    } else {
        menu_state(3)
    }
}

/// State 4 load/arm (`FUN_00437200`): the in-race entry state the race is armed
/// into (or `7` when a front-end session suppressed the real load).
pub fn load_arm_next(inputs: MenuFlowInputs) -> GameState {
    if inputs.frontend_active {
        menu_state(7)
    } else if inputs.resume_pending {
        menu_state(6)
    } else if inputs.race_type_bit {
        menu_state(8)
    } else {
        menu_state(5)
    }
}

/// The next state for a menu/load state (`1..=4`), or `None` for any other
/// state (whose handler is a different reconstruction item). This is the
/// `handler` oracle a [`crate::game_driver::FrameDriver`] uses while the FSM is
/// in the pre-gameplay band.
pub fn menu_flow_next(state: GameState, inputs: MenuFlowInputs) -> Option<GameState> {
    match state.id() {
        1 => Some(boot_seed_next(inputs)),
        2 => Some(frontend_loop_next(inputs)),
        3 => Some(preload_gate_next(inputs)),
        4 => Some(load_arm_next(inputs)),
        _ => None,
    }
}

/// The outcome the recovered front-end controller drives state 2 to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendMenuSignal {
    /// The user is still in the menu - state 2 self-loops.
    Stay,
    /// The user committed to leaving the front-end toward the race - state 2
    /// advances to the pre-load gate (`3`).
    Advance,
    /// The user requested exit - state 2 takes the quit funnel (`0xc`).
    Quit,
}

impl FrontendMenuSignal {
    /// The state-2 next state this signal drives, per the recovered
    /// front-end-loop outcomes (`Stay -> 2`, `Advance -> 3`, `Quit -> 0xc`).
    pub fn menu_next_state(self) -> GameState {
        match self {
            FrontendMenuSignal::Stay => menu_state(2),
            FrontendMenuSignal::Advance => menu_state(3),
            FrontendMenuSignal::Quit => menu_state(0xC),
        }
    }
}

/// Maps the recovered front-end controller's own outcome onto the state-2
/// signal (the inferred correspondence in the module docs): a requested exit is
/// a quit; a committed leave (`LeaveFrontendA`/`B`) advances toward the race;
/// otherwise the controller is still running and the menu stays put.
pub fn frontend_menu_signal(frontend: &FrontendState) -> FrontendMenuSignal {
    if frontend.exit_requested() {
        return FrontendMenuSignal::Quit;
    }
    match frontend.continuation() {
        FrontendContinuation::Running => FrontendMenuSignal::Stay,
        FrontendContinuation::LeaveFrontendA | FrontendContinuation::LeaveFrontendB => {
            FrontendMenuSignal::Advance
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend::{FrontendInput, TransitionResult};

    fn state(id: u8) -> GameState {
        GameState::new(id).expect("valid state id")
    }

    #[test]
    fn boot_seed_matches_the_recovered_table() {
        // frontend_active -> 2
        let active = MenuFlowInputs {
            frontend_active: true,
            ..Default::default()
        };
        assert_eq!(boot_seed_next(active), state(2));
        // no session, show_menu -> 2
        let show = MenuFlowInputs {
            show_menu: true,
            ..Default::default()
        };
        assert_eq!(boot_seed_next(show), state(2));
        // no session, no menu -> 3
        assert_eq!(boot_seed_next(MenuFlowInputs::default()), state(3));
    }

    #[test]
    fn frontend_loop_matches_the_recovered_table() {
        // Non-interactive: intro advance -> 3, else stay 2.
        let intro = MenuFlowInputs {
            intro_advance: true,
            ..Default::default()
        };
        assert_eq!(frontend_loop_next(intro), state(3));
        assert_eq!(frontend_loop_next(MenuFlowInputs::default()), state(2));

        // Interactive, menu controller not ready -> stay 2.
        let base = MenuFlowInputs {
            frontend_active: true,
            ..Default::default()
        };
        assert_eq!(frontend_loop_next(base), state(2));

        // Interactive, ready but screen graph failed -> quit 0xc.
        let quit_graph = MenuFlowInputs {
            frontend_active: true,
            root_menu_ready: true,
            screen_graph_ready: false,
            ..Default::default()
        };
        assert_eq!(frontend_loop_next(quit_graph), state(0xC));

        // Interactive, ready + healthy but menu status says quit -> 0xc.
        let quit_status = MenuFlowInputs {
            frontend_active: true,
            root_menu_ready: true,
            screen_graph_ready: true,
            menu_status_quit: true,
            ..Default::default()
        };
        assert_eq!(frontend_loop_next(quit_status), state(0xC));

        // Interactive, ready + healthy + not quit -> confirmed, advance 3.
        let confirm = MenuFlowInputs {
            frontend_active: true,
            root_menu_ready: true,
            screen_graph_ready: true,
            menu_status_quit: false,
            ..Default::default()
        };
        assert_eq!(frontend_loop_next(confirm), state(3));
    }

    #[test]
    fn preload_gate_matches_the_recovered_table() {
        // No pending interstitial -> advance 4.
        assert_eq!(preload_gate_next(MenuFlowInputs::default()), state(4));
        // Pending but done -> 4.
        let done = MenuFlowInputs {
            preload_pending: true,
            preload_done: true,
            ..Default::default()
        };
        assert_eq!(preload_gate_next(done), state(4));
        // Pending, not done -> stay 3.
        let waiting = MenuFlowInputs {
            preload_pending: true,
            preload_done: false,
            ..Default::default()
        };
        assert_eq!(preload_gate_next(waiting), state(3));
    }

    #[test]
    fn load_arm_matches_the_recovered_table() {
        // frontend_active -> 7 (loaders skipped).
        let active = MenuFlowInputs {
            frontend_active: true,
            ..Default::default()
        };
        assert_eq!(load_arm_next(active), state(7));
        // resume pending -> 6.
        let resume = MenuFlowInputs {
            resume_pending: true,
            ..Default::default()
        };
        assert_eq!(load_arm_next(resume), state(6));
        // race-type bit set -> 8.
        let type_bit = MenuFlowInputs {
            race_type_bit: true,
            ..Default::default()
        };
        assert_eq!(load_arm_next(type_bit), state(8));
        // default -> 5.
        assert_eq!(load_arm_next(MenuFlowInputs::default()), state(5));
    }

    #[test]
    fn menu_flow_next_only_covers_the_pre_gameplay_band() {
        let inputs = MenuFlowInputs::default();
        assert!(menu_flow_next(state(1), inputs).is_some());
        assert!(menu_flow_next(state(4), inputs).is_some());
        for id in [0u8, 5, 6, 9, 0xB, 0xC] {
            assert_eq!(menu_flow_next(state(id), inputs), None);
        }
    }

    #[test]
    fn every_menu_flow_edge_is_a_recovered_transition() {
        // Exhaustively exercise all input combinations for states 1..=4 and
        // confirm each produced next state is a recovered edge of that state.
        for bits in 0u16..(1 << 10) {
            let inputs = MenuFlowInputs {
                frontend_active: bits & 1 != 0,
                show_menu: bits & 2 != 0,
                intro_advance: bits & 4 != 0,
                root_menu_ready: bits & 8 != 0,
                screen_graph_ready: bits & 0x10 != 0,
                menu_status_quit: bits & 0x20 != 0,
                preload_pending: bits & 0x40 != 0,
                preload_done: bits & 0x80 != 0,
                resume_pending: bits & 0x100 != 0,
                race_type_bit: bits & 0x200 != 0,
            };
            for id in 1u8..=4 {
                let from = state(id);
                let next = menu_flow_next(from, inputs).expect("1..=4 is covered");
                assert!(
                    from.is_known_transition(next),
                    "state {id} -> {} is not a recovered edge",
                    next.id()
                );
            }
        }
    }

    #[test]
    fn frontend_signal_maps_to_the_state_2_outcomes() {
        assert_eq!(FrontendMenuSignal::Stay.menu_next_state(), state(2));
        assert_eq!(FrontendMenuSignal::Advance.menu_next_state(), state(3));
        assert_eq!(FrontendMenuSignal::Quit.menu_next_state(), state(0xC));
    }

    #[test]
    fn frontend_controller_outcome_drives_state_2() {
        // A fresh running controller keeps the menu on state 2.
        let mut frontend = FrontendState::after_startup_art(0, 0).expect("valid selection");
        assert_eq!(frontend_menu_signal(&frontend), FrontendMenuSignal::Stay);
        assert_eq!(frontend_menu_signal(&frontend).menu_next_state(), state(2));

        // A committed leave (the transition driver reports LeaveFrontendA while
        // the controller is advancing a pending screen) advances to state 3.
        let _ = frontend.step(
            FrontendInput::default(),
            1,
            TransitionResult::LeaveFrontendA,
            true,
        );
        assert_eq!(frontend_menu_signal(&frontend), FrontendMenuSignal::Advance);
        assert_eq!(frontend_menu_signal(&frontend).menu_next_state(), state(3));
    }
}
