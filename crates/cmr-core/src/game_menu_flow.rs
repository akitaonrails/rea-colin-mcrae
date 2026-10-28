//! The pre-race states `1..=4` of the top-level FSM (`rea.official-eu.game-fsm-menu`,
//! corrected by `rea.official-eu.event-flow`), instruction level:
//!
//! - **State 1** (`FUN_00436be0`): choose or continue the event, then the
//!   country welcome screen (`2`) or loading (`3`); a rally-school session
//!   goes to `2` ([`crate::event_flow::EventState::boot`]).
//! - **State 2** (`FUN_00437160`): in a race, the welcome screen sequencer
//!   `FUN_0048dde0` finishing -> `3`, else stay `2`; in the rally school, its
//!   menu `FUN_00470e20` returning 0 -> stay `2`, else the quit latch
//!   `FUN_0046f650` nonzero or the school status `cfg[0x6c0] & 7 == 6` -> `0xc`
//!   (with `FUN_0049daf0(1)`), else `3`. Its render handler `FUN_00421cd0`
//!   ends a school session on the same quit test and, in a race, copies the
//!   special-event flag `DAT_0055001c` to `DAT_00514478`.
//! - **State 3** (`FUN_004371c0`): no pending interstitial (`FUN_0047a190`)
//!   -> `4`; else its screen finished (`FUN_00476f70`) -> `4`, else stay `3`.
//! - **State 4** (`FUN_00437200`): arm the race. Outside the school it clears
//!   the restart request `DAT_00550018` and copies the special-event flag
//!   `DAT_0055001c` to `DAT_00514478`; it returns `7` for the school, `6` for
//!   the special event, `8` for the attract demo (`cfg[0] & 1`), else `5`.
//!
//! The front end itself is not one of these states: the descriptor menus run
//! while `DAT_00506228` is set and the FSM is not ticked at all
//! ([`crate::game_session::GameSession`]).
//!
//! The welcome sequencer, the school menu and the interstitial are separate
//! subsystems; their results are the [`MenuFlowInputs`].

use crate::event_flow::EventState;
use crate::game_state::GameState;

/// The results of the subsystems states 2 and 3 wait on, one tick's worth.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MenuFlowInputs {
    /// State 2, race: the country welcome screen sequencer finished
    /// (`FUN_0048dde0 != 0`).
    pub welcome_done: bool,
    /// State 2, rally school: its menu produced a result (`FUN_00470e20 != 0`).
    pub school_menu_done: bool,
    /// State 2, rally school: the quit latch (`FUN_0046f650 != 0`).
    pub school_quit: bool,
    /// State 2, rally school: the status `cfg[0x6c0] & 7 == 6`.
    pub school_status_quit: bool,
    /// State 3: an interstitial is pending (`FUN_0047a190 != 0`).
    pub preload_pending: bool,
    /// State 3: the interstitial finished (`FUN_00476f70 != 0`).
    pub preload_done: bool,
}

fn menu_state(id: u8) -> GameState {
    GameState::new(id).expect("recovered menu-flow state id is in range")
}

/// State 2 (`FUN_00437160`).
pub fn welcome_or_school_next(inputs: MenuFlowInputs, school: bool) -> GameState {
    if !school {
        return menu_state(if inputs.welcome_done { 3 } else { 2 });
    }
    if !inputs.school_menu_done {
        return menu_state(2);
    }
    if inputs.school_quit || inputs.school_status_quit {
        menu_state(0xC)
    } else {
        menu_state(3)
    }
}

/// State 3 (`FUN_004371c0`).
pub fn preload_gate_next(inputs: MenuFlowInputs) -> GameState {
    if !inputs.preload_pending || inputs.preload_done {
        menu_state(4)
    } else {
        menu_state(3)
    }
}

/// State 4 (`FUN_00437200`): the routing and the two flag writes it makes.
pub fn load_arm_next(event: &mut EventState) -> GameState {
    let globals = &mut event.globals;
    if globals.school_d3ca88 {
        return menu_state(7);
    }
    globals.restart_550018 = false;
    globals.special_514478 = globals.special_55001c;
    if globals.special_514478 {
        menu_state(6)
    } else if event.cfg.flags_000 & 1 != 0 {
        menu_state(8)
    } else {
        menu_state(5)
    }
}

/// The next state for a pre-race state (`1..=4`), or `None` for any other.
pub fn menu_flow_next(
    state: GameState,
    inputs: MenuFlowInputs,
    event: &mut EventState,
) -> Option<GameState> {
    match state.id() {
        1 => Some(event.boot(None)),
        2 => {
            let school = event.globals.school_d3ca88;
            let next = welcome_or_school_next(inputs, school);
            // State 2's render handler `FUN_00421cd0`: the school's quit
            // block ends the session; a race arms the special-event flag.
            if school {
                if inputs.school_quit || inputs.school_status_quit {
                    event.end_session();
                }
            } else {
                event.globals.special_514478 = event.globals.special_55001c;
            }
            Some(next)
        }
        3 => Some(preload_gate_next(inputs)),
        4 => Some(load_arm_next(event)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(id: u8) -> GameState {
        GameState::new(id).expect("valid state id")
    }

    #[test]
    fn state_two_waits_on_the_welcome_screen_in_a_race() {
        let done = MenuFlowInputs {
            welcome_done: true,
            ..Default::default()
        };
        assert_eq!(welcome_or_school_next(done, false), state(3));
        assert_eq!(
            welcome_or_school_next(MenuFlowInputs::default(), false),
            state(2)
        );
    }

    #[test]
    fn state_two_runs_the_school_menu_and_its_quit() {
        assert_eq!(
            welcome_or_school_next(MenuFlowInputs::default(), true),
            state(2)
        );
        let chosen = MenuFlowInputs {
            school_menu_done: true,
            ..Default::default()
        };
        assert_eq!(welcome_or_school_next(chosen, true), state(3));
        let quit = MenuFlowInputs {
            school_quit: true,
            ..chosen
        };
        assert_eq!(welcome_or_school_next(quit, true), state(0xC));
        let status = MenuFlowInputs {
            school_status_quit: true,
            ..chosen
        };
        assert_eq!(welcome_or_school_next(status, true), state(0xC));
    }

    #[test]
    fn state_two_s_render_handler_ends_a_quit_school_and_arms_the_special() {
        let mut school = EventState::default();
        school.start_school_session();
        let quit = MenuFlowInputs {
            school_quit: true,
            ..Default::default()
        };
        menu_flow_next(state(2), quit, &mut school);
        assert!(!school.in_session() && school.globals.reseed_requested);
        let mut race = EventState::default();
        race.start_race_session();
        race.globals.special_55001c = true;
        menu_flow_next(state(2), MenuFlowInputs::default(), &mut race);
        assert!(race.globals.special_514478);
    }

    #[test]
    fn preload_gate_matches_the_recovered_table() {
        assert_eq!(preload_gate_next(MenuFlowInputs::default()), state(4));
        let waiting = MenuFlowInputs {
            preload_pending: true,
            ..Default::default()
        };
        assert_eq!(preload_gate_next(waiting), state(3));
        let done = MenuFlowInputs {
            preload_done: true,
            ..waiting
        };
        assert_eq!(preload_gate_next(done), state(4));
    }

    #[test]
    fn load_arm_routes_by_session_kind_and_clears_the_restart() {
        let mut event = EventState::default();
        event.globals.restart_550018 = true;
        assert_eq!(load_arm_next(&mut event), state(5));
        assert!(!event.globals.restart_550018);
        event.cfg.flags_000 = 1;
        assert_eq!(load_arm_next(&mut event), state(8));
        event.globals.special_55001c = true;
        assert_eq!(load_arm_next(&mut event), state(6));
        assert!(event.globals.special_514478);
        let mut school = EventState::default();
        school.globals.school_d3ca88 = true;
        school.globals.restart_550018 = true;
        assert_eq!(load_arm_next(&mut school), state(7));
        assert!(school.globals.restart_550018, "the school skips the clear");
    }

    #[test]
    fn every_menu_flow_edge_is_a_recovered_transition() {
        for bits in 0u16..(1 << 9) {
            let inputs = MenuFlowInputs {
                welcome_done: bits & 1 != 0,
                school_menu_done: bits & 2 != 0,
                school_quit: bits & 4 != 0,
                school_status_quit: bits & 8 != 0,
                preload_pending: bits & 0x10 != 0,
                preload_done: bits & 0x20 != 0,
            };
            for id in 1u8..=4 {
                let mut event = EventState::default();
                event.globals.school_d3ca88 = bits & 0x40 != 0;
                event.globals.special_55001c = bits & 0x80 != 0;
                event.cfg.flags_000 = u8::from(bits & 0x100 != 0);
                let from = state(id);
                let next = menu_flow_next(from, inputs, &mut event).expect("1..=4 is covered");
                assert!(
                    from.is_known_transition(next),
                    "state {id} -> {} is not a recovered edge",
                    next.id()
                );
            }
        }
    }

    #[test]
    fn other_states_are_not_covered() {
        let mut event = EventState::default();
        for id in [0u8, 5, 6, 9, 0xB, 0xC] {
            assert_eq!(
                menu_flow_next(state(id), MenuFlowInputs::default(), &mut event),
                None
            );
        }
    }
}
