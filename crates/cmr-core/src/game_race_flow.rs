//! The in-race transition logic of the top-level FSM states `5..=0xb`
//! (reconstruction item G01, in-race band): the decision tables the seven
//! gameplay handlers use to pick their next state.
//!
//! The dispatch ([`crate::game_state`]) makes each state's update handler return
//! the next state. For the in-race handlers those returns are branches over a
//! handful of recovered sub-state globals and per-frame probe results
//! (`rea.official-eu.game-fsm`, the in-race handler recovery, asm):
//!
//! - **State 5** main driving tick (`FUN_00437ac0`) -> `{5,8,9,0xa}`: not
//!   world-ready -> stay `5`; a pause request -> `0xa`; the stage-finished flag
//!   (`DAT_00514468`, set by the checkpoint tracker [`crate::stage_checkpoints`])
//!   -> `8` in normal play, or `9` in front-end/replay when the replay is armed
//!   (`DAT_005fbdec`); else keep driving `5`.
//! - **State 6** pre-stage / interstitial overlay (`FUN_00437690`) ->
//!   `{6,8,0xb}`: a front-end session or a missing loader handle -> `0xb`; not
//!   world-ready -> stay `6`; fresh input or finished -> `8`; not yet ready -> `6`;
//!   once ready, the "congratulations" scroller completing -> `0xb` else `6`.
//! - **State 7** stage-entry / replay-camera transition (`FUN_00437ef0`) ->
//!   `{5,7,0xa}`: pause -> `0xa` (remembering state `7`); normal play -> straight
//!   to driving `5`; front-end intro/replay still running -> hold `7`, done -> `5`.
//! - **State 8** stage-boundary grid/countdown & settle (`FUN_00438100`) ->
//!   `{8,0xb}`: stay `8` while in normal play, not skipping, and the pre-stage
//!   camera-intro director / input gate is still running; otherwise -> `0xb`.
//! - **State 9** front-end finish-flyby / replay (`FUN_004384a0`) -> `{9,0xa,0xb}`:
//!   pause -> `0xa`; normal play loops at `9`; in front-end mode the results
//!   cinematic completing -> `0xb` else `9`.
//! - **State 0xa** in-race pause / overlay (`FUN_00438970`) ->
//!   `{5,7,8,9,0xa,0xb}`: quit, or a normal-play restart, -> `0xb`; the
//!   resume-alt item -> `9` (front-end) / `8` (normal); the resume item -> `7`
//!   (front-end, paused from `7`) / `5`; nothing chosen -> stay `0xa`.
//! - **State 0xb** post-race / inter-stage hub (`FUN_00438b90`) ->
//!   `{2,3,5,0xb,0xc}`: fade not done or results page up -> hold `0xb`; a
//!   front-end quit-event -> `0xc`; a restart or an allowed next stage -> `5`
//!   (re-drive); else exit to the menu band (`2` front-end or show-menu, else `3`).
//!
//! This module reconstructs those seven decision tables over [`InRaceInputs`] -
//! the recovered globals and probe results are **inputs**, not reimplemented
//! here, exactly as [`crate::game_menu_flow`] treats the menu band `1..=4`. The
//! per-frame subsystem spine the handlers run (control-map -> pause-probe ->
//! Mechanics physics -> audio -> particles -> camera -> sound events) is driven
//! by the recovered subsystem modules at the call site, not by this pure table.
//! Every produced edge is checked against the recovered transition graph
//! ([`crate::game_state::GameState::is_known_transition`]).

use crate::game_state::GameState;

/// The recovered sub-state globals and per-frame probe results the in-race
/// handlers (`5..=0xb`) branch on, one tick's worth. Each field names the
/// recovered global or callee it stands for; the shipped build computes these
/// inside the handlers and their callees, so here they are inputs.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct InRaceInputs {
    /// An interactive front-end/attract/replay session is active
    /// (`DAT_00d3ca88 != 0`): gates nearly every in-race branch.
    pub frontend_active: bool,
    /// The stage world has finished loading (`DAT_005144a0`, via `FUN_00421ca0`);
    /// states 5/6 stay put until it is set.
    pub stage_world_ready: bool,
    /// The stage-finished / race-over flag (`DAT_00514468`), raised by the
    /// checkpoint tracker on the finish gate and cleared by the hub (`0xb`).
    pub stage_finished: bool,
    /// A pause was requested this frame (`FUN_00468200`: `DAT_0057e66c` /
    /// `DAT_00b63748`), diverting the driving states to the pause overlay.
    pub pause_requested: bool,
    /// State 5, front-end finish: the replay is armed (`DAT_005fbdec != 0`),
    /// selecting the finish-flyby state `9` instead of the normal results `8`.
    pub replay_ready: bool,
    /// State 6: a required loader handle is null (`DAT_0057e6e4` / `DAT_0057e6e0`),
    /// so the overlay bails to the hub (and raises the finished flag).
    pub loader_missing: bool,
    /// State 6: fresh input arrived this frame (`in != 0 && changed`), starting
    /// the stage (`-> 8`).
    pub input_pressed: bool,
    /// State 6: the pre-stage screen is ready to advance (`DAT_0054f99c`, via
    /// `FUN_0042a840`); clear keeps the overlay up.
    pub prestage_ready: bool,
    /// State 6: the "congratulations" / stage-info scroller has completed
    /// (`FUN_0042a860 != 0`), dropping to the hub (`0xb`).
    pub scroller_done: bool,
    /// State 7, front-end: the intro / replay-camera sequence has finished
    /// (`DAT_005ef448` or `FUN_00495d10 == 0`), handing off to driving (`5`).
    pub replay_intro_done: bool,
    /// State 8: the boundary "skip body" flag (`DAT_00514478 != 0`), forcing the
    /// handler straight to the hub (`0xb`).
    pub boundary_skip: bool,
    /// State 8: the pre-stage camera-intro director / input gate is still running
    /// (`FUN_00430750 != 0`, i.e. `local_c == 0`); clear funnels to the hub.
    pub boundary_active: bool,
    /// State 9, front-end: the finish/results cinematic chain has completed
    /// (`DAT_005ef44c` path), exiting the flyby to the hub (`0xb`).
    pub flyby_cinematic_done: bool,
    /// Pause menu / hub: quit was requested (`DAT_0057e678`, `FUN_00468d70`, or
    /// the SHIFT+Q hotkey).
    pub quit_requested: bool,
    /// Pause menu / hub: restart was chosen (`DAT_0057e680` -> `DAT_00550018`);
    /// in the pause overlay it funnels to the hub, in the hub it re-drives (`5`).
    pub restart_requested: bool,
    /// State 0xa: the resume-alt item fired (`DAT_0057e67c`, `FUN_00468d90`) ->
    /// `9` (front-end) or `8` (normal, committing telemetry + stopping audio).
    pub resume_alt: bool,
    /// State 0xa: the resume item fired (`DAT_0057e670`, `FUN_00468d20`) -> `5`,
    /// or `7` when paused from state 7.
    pub resume: bool,
    /// State 0xa: the pre-pause state was 7 (`DAT_00551b28 == 7`), so a front-end
    /// resume returns to the entry/replay state `7`.
    pub prepause_was_7: bool,
    /// State 0xb: the results page is still showing (`sess+0x428 && sess+0x10`),
    /// holding the hub.
    pub results_page_up: bool,
    /// State 0xb: the screen fade-to-black has completed (`FUN_0048dfd0`); the hub
    /// holds until it is set.
    pub fade_done: bool,
    /// State 0xb: a next/first stage is allowed to start (`FUN_00401030 != 0`);
    /// clear routes to the menu-band exit.
    pub next_stage_allowed: bool,
    /// State 0xb: the championship/sequence is exhausted (`DAT_00551ad8 != 0`),
    /// routing to the menu-band exit instead of the next stage.
    pub championship_exhausted: bool,
    /// State 0xb, normal-play exit: the "show the front-end menu" predicate
    /// (`FUN_0048df30 != 0`) selects `2` over `3`.
    pub show_menu: bool,
}

fn race_state(id: u8) -> GameState {
    GameState::new(id).expect("recovered race-flow state id is in range")
}

/// State 5 main driving tick (`FUN_00437ac0`): keep driving, divert to pause, or
/// hand off on the stage-finished flag.
pub fn drive_next(inputs: InRaceInputs) -> GameState {
    if !inputs.stage_world_ready {
        return race_state(5);
    }
    if inputs.pause_requested {
        return race_state(0xA);
    }
    if inputs.stage_finished {
        if !inputs.frontend_active {
            return race_state(8);
        }
        if inputs.replay_ready {
            return race_state(9);
        }
    }
    race_state(5)
}

/// State 6 pre-stage / interstitial overlay (`FUN_00437690`): hold the overlay,
/// start the stage, or drop to the hub.
pub fn prestage_next(inputs: InRaceInputs) -> GameState {
    if inputs.frontend_active || inputs.loader_missing {
        return race_state(0xB);
    }
    if !inputs.stage_world_ready {
        return race_state(6);
    }
    if inputs.input_pressed || inputs.stage_finished {
        return race_state(8);
    }
    if !inputs.prestage_ready {
        return race_state(6);
    }
    if inputs.scroller_done {
        race_state(0xB)
    } else {
        race_state(6)
    }
}

/// State 7 stage-entry / replay-camera transition (`FUN_00437ef0`): pause, the
/// normal one-tick hand-off to driving, or the front-end intro hold.
pub fn entry_next(inputs: InRaceInputs) -> GameState {
    if inputs.pause_requested {
        return race_state(0xA);
    }
    if !inputs.frontend_active || inputs.replay_intro_done {
        race_state(5)
    } else {
        race_state(7)
    }
}

/// State 8 stage-boundary grid/countdown & settle (`FUN_00438100`): hold while
/// the director/gate runs, else funnel to the hub.
pub fn boundary_next(inputs: InRaceInputs) -> GameState {
    if !inputs.frontend_active && !inputs.boundary_skip && inputs.boundary_active {
        race_state(8)
    } else {
        race_state(0xB)
    }
}

/// State 9 front-end finish-flyby / replay (`FUN_004384a0`): pause, loop, or exit
/// to the hub once the cinematic completes.
pub fn flyby_next(inputs: InRaceInputs) -> GameState {
    if inputs.pause_requested {
        return race_state(0xA);
    }
    if inputs.frontend_active && inputs.flyby_cinematic_done {
        race_state(0xB)
    } else {
        race_state(9)
    }
}

/// State 0xa in-race pause / overlay (`FUN_00438970`): resume to the remembered
/// state, restart/quit to the hub, or stay paused.
pub fn pause_next(inputs: InRaceInputs) -> GameState {
    if inputs.quit_requested || (!inputs.frontend_active && inputs.restart_requested) {
        return race_state(0xB);
    }
    if inputs.resume_alt {
        return if inputs.frontend_active {
            race_state(9)
        } else {
            race_state(8)
        };
    }
    if inputs.resume {
        return if inputs.frontend_active && inputs.prepause_was_7 {
            race_state(7)
        } else {
            race_state(5)
        };
    }
    race_state(0xA)
}

/// State 0xb post-race / inter-stage hub (`FUN_00438b90`): hold on the results,
/// re-drive the next/restarted stage, quit the event, or exit to the menu band.
pub fn hub_next(inputs: InRaceInputs) -> GameState {
    if !inputs.fade_done || inputs.results_page_up {
        return race_state(0xB);
    }
    if inputs.frontend_active && inputs.quit_requested {
        return race_state(0xC);
    }
    // Restart re-drives the current stage; otherwise an allowed, non-quit next
    // stage starts driving. (The restart -> 5 edge is inferred from the restart
    // flag's intent; the explicit next-stage guard is observed.)
    if inputs.restart_requested {
        return race_state(5);
    }
    if !inputs.quit_requested && inputs.next_stage_allowed && !inputs.championship_exhausted {
        return race_state(5);
    }
    if inputs.frontend_active || inputs.show_menu {
        race_state(2)
    } else {
        race_state(3)
    }
}

/// The next state for an in-race state (`5..=0xb`), or `None` for any other
/// state (whose handler is a different reconstruction item). This is the
/// `handler` oracle a [`crate::game_driver::FrameDriver`] /
/// [`crate::game_session::GameSession`] uses while the FSM is in the gameplay
/// band, the in-race counterpart of [`crate::game_menu_flow::menu_flow_next`].
pub fn race_flow_next(state: GameState, inputs: InRaceInputs) -> Option<GameState> {
    match state.id() {
        5 => Some(drive_next(inputs)),
        6 => Some(prestage_next(inputs)),
        7 => Some(entry_next(inputs)),
        8 => Some(boundary_next(inputs)),
        9 => Some(flyby_next(inputs)),
        0xA => Some(pause_next(inputs)),
        0xB => Some(hub_next(inputs)),
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
    fn drive_matches_the_recovered_table() {
        // Not world-ready -> stay 5 (even with other flags set).
        let not_ready = InRaceInputs {
            stage_world_ready: false,
            pause_requested: true,
            stage_finished: true,
            ..Default::default()
        };
        assert_eq!(drive_next(not_ready), state(5));

        let ready = InRaceInputs {
            stage_world_ready: true,
            ..Default::default()
        };
        // Ready, nothing else -> keep driving.
        assert_eq!(drive_next(ready), state(5));
        // Pause wins over finish.
        assert_eq!(
            drive_next(InRaceInputs {
                pause_requested: true,
                stage_finished: true,
                ..ready
            }),
            state(0xA)
        );
        // Normal finish -> 8.
        assert_eq!(
            drive_next(InRaceInputs {
                stage_finished: true,
                ..ready
            }),
            state(8)
        );
        // Front-end finish with replay armed -> 9.
        assert_eq!(
            drive_next(InRaceInputs {
                stage_finished: true,
                frontend_active: true,
                replay_ready: true,
                ..ready
            }),
            state(9)
        );
        // Front-end finish, replay NOT armed -> keep driving 5.
        assert_eq!(
            drive_next(InRaceInputs {
                stage_finished: true,
                frontend_active: true,
                replay_ready: false,
                ..ready
            }),
            state(5)
        );
    }

    #[test]
    fn prestage_matches_the_recovered_table() {
        // Front-end or missing loader -> hub.
        assert_eq!(
            prestage_next(InRaceInputs {
                frontend_active: true,
                ..Default::default()
            }),
            state(0xB)
        );
        assert_eq!(
            prestage_next(InRaceInputs {
                loader_missing: true,
                ..Default::default()
            }),
            state(0xB)
        );
        // Not world-ready -> stay 6.
        assert_eq!(prestage_next(InRaceInputs::default()), state(6));

        let ready = InRaceInputs {
            stage_world_ready: true,
            ..Default::default()
        };
        // Fresh input -> start stage (8).
        assert_eq!(
            prestage_next(InRaceInputs {
                input_pressed: true,
                ..ready
            }),
            state(8)
        );
        // Finished/abort -> 8.
        assert_eq!(
            prestage_next(InRaceInputs {
                stage_finished: true,
                ..ready
            }),
            state(8)
        );
        // Ready-flag clear -> stay 6.
        assert_eq!(prestage_next(ready), state(6));
        // Ready + scroller done -> hub.
        assert_eq!(
            prestage_next(InRaceInputs {
                prestage_ready: true,
                scroller_done: true,
                ..ready
            }),
            state(0xB)
        );
        // Ready + scroller not done -> stay 6.
        assert_eq!(
            prestage_next(InRaceInputs {
                prestage_ready: true,
                ..ready
            }),
            state(6)
        );
    }

    #[test]
    fn entry_matches_the_recovered_table() {
        // Pause -> 0xa.
        assert_eq!(
            entry_next(InRaceInputs {
                pause_requested: true,
                ..Default::default()
            }),
            state(0xA)
        );
        // Normal play -> straight to driving 5.
        assert_eq!(entry_next(InRaceInputs::default()), state(5));
        // Front-end, intro running -> hold 7.
        assert_eq!(
            entry_next(InRaceInputs {
                frontend_active: true,
                ..Default::default()
            }),
            state(7)
        );
        // Front-end, intro done -> 5.
        assert_eq!(
            entry_next(InRaceInputs {
                frontend_active: true,
                replay_intro_done: true,
                ..Default::default()
            }),
            state(5)
        );
    }

    #[test]
    fn boundary_matches_the_recovered_table() {
        // Normal play, not skipping, director running -> stay 8.
        assert_eq!(
            boundary_next(InRaceInputs {
                boundary_active: true,
                ..Default::default()
            }),
            state(8)
        );
        // Director finished -> hub.
        assert_eq!(boundary_next(InRaceInputs::default()), state(0xB));
        // Front-end -> hub.
        assert_eq!(
            boundary_next(InRaceInputs {
                frontend_active: true,
                boundary_active: true,
                ..Default::default()
            }),
            state(0xB)
        );
        // Skip flag -> hub.
        assert_eq!(
            boundary_next(InRaceInputs {
                boundary_skip: true,
                boundary_active: true,
                ..Default::default()
            }),
            state(0xB)
        );
    }

    #[test]
    fn flyby_matches_the_recovered_table() {
        // Pause -> 0xa.
        assert_eq!(
            flyby_next(InRaceInputs {
                pause_requested: true,
                ..Default::default()
            }),
            state(0xA)
        );
        // Normal play -> loop 9.
        assert_eq!(flyby_next(InRaceInputs::default()), state(9));
        // Front-end, cinematic running -> 9.
        assert_eq!(
            flyby_next(InRaceInputs {
                frontend_active: true,
                ..Default::default()
            }),
            state(9)
        );
        // Front-end, cinematic done -> hub.
        assert_eq!(
            flyby_next(InRaceInputs {
                frontend_active: true,
                flyby_cinematic_done: true,
                ..Default::default()
            }),
            state(0xB)
        );
    }

    #[test]
    fn pause_matches_the_recovered_table() {
        // Nothing chosen -> stay paused.
        assert_eq!(pause_next(InRaceInputs::default()), state(0xA));
        // Quit -> hub.
        assert_eq!(
            pause_next(InRaceInputs {
                quit_requested: true,
                ..Default::default()
            }),
            state(0xB)
        );
        // Normal restart -> hub.
        assert_eq!(
            pause_next(InRaceInputs {
                restart_requested: true,
                ..Default::default()
            }),
            state(0xB)
        );
        // Resume-alt, normal -> 8; front-end -> 9.
        assert_eq!(
            pause_next(InRaceInputs {
                resume_alt: true,
                ..Default::default()
            }),
            state(8)
        );
        assert_eq!(
            pause_next(InRaceInputs {
                resume_alt: true,
                frontend_active: true,
                ..Default::default()
            }),
            state(9)
        );
        // Resume, normal -> 5.
        assert_eq!(
            pause_next(InRaceInputs {
                resume: true,
                ..Default::default()
            }),
            state(5)
        );
        // Resume, front-end paused from 7 -> 7.
        assert_eq!(
            pause_next(InRaceInputs {
                resume: true,
                frontend_active: true,
                prepause_was_7: true,
                ..Default::default()
            }),
            state(7)
        );
        // Resume, front-end but not from 7 -> 5.
        assert_eq!(
            pause_next(InRaceInputs {
                resume: true,
                frontend_active: true,
                ..Default::default()
            }),
            state(5)
        );
    }

    #[test]
    fn hub_matches_the_recovered_table() {
        // Fade not done -> hold.
        assert_eq!(hub_next(InRaceInputs::default()), state(0xB));
        let faded = InRaceInputs {
            fade_done: true,
            ..Default::default()
        };
        // Results page up -> hold.
        assert_eq!(
            hub_next(InRaceInputs {
                results_page_up: true,
                ..faded
            }),
            state(0xB)
        );
        // Front-end quit -> 0xc.
        assert_eq!(
            hub_next(InRaceInputs {
                frontend_active: true,
                quit_requested: true,
                ..faded
            }),
            state(0xC)
        );
        // Restart -> re-drive 5.
        assert_eq!(
            hub_next(InRaceInputs {
                restart_requested: true,
                ..faded
            }),
            state(5)
        );
        // Allowed next stage -> drive 5.
        assert_eq!(
            hub_next(InRaceInputs {
                next_stage_allowed: true,
                ..faded
            }),
            state(5)
        );
        // Not allowed, normal, show-menu -> 2; else 3.
        assert_eq!(
            hub_next(InRaceInputs {
                show_menu: true,
                ..faded
            }),
            state(2)
        );
        assert_eq!(hub_next(faded), state(3));
        // Front-end exit (not quit, no next stage) -> 2.
        assert_eq!(
            hub_next(InRaceInputs {
                frontend_active: true,
                ..faded
            }),
            state(2)
        );
        // Normal quit (no next stage) -> menu band 3.
        assert_eq!(
            hub_next(InRaceInputs {
                quit_requested: true,
                ..faded
            }),
            state(3)
        );
    }

    #[test]
    fn race_flow_next_only_covers_the_gameplay_band() {
        let inputs = InRaceInputs::default();
        for id in 5u8..=0xB {
            assert!(race_flow_next(state(id), inputs).is_some(), "{id} covered");
        }
        for id in [0u8, 1, 2, 3, 4, 0xC] {
            assert_eq!(race_flow_next(state(id), inputs), None);
        }
    }

    #[test]
    fn every_race_flow_edge_is_a_recovered_transition() {
        // A deterministic seeded sweep over the 23-bit input space (a full 2^23
        // enumeration is wasteful; this LCG walk covers a large, reproducible
        // sample) confirms every produced next state is a recovered edge.
        let mut seed: u32 = 0x1234_5678;
        for _ in 0..300_000 {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let bits = seed;
            let inputs = InRaceInputs {
                frontend_active: bits & (1 << 0) != 0,
                stage_world_ready: bits & (1 << 1) != 0,
                stage_finished: bits & (1 << 2) != 0,
                pause_requested: bits & (1 << 3) != 0,
                replay_ready: bits & (1 << 4) != 0,
                loader_missing: bits & (1 << 5) != 0,
                input_pressed: bits & (1 << 6) != 0,
                prestage_ready: bits & (1 << 7) != 0,
                scroller_done: bits & (1 << 8) != 0,
                replay_intro_done: bits & (1 << 9) != 0,
                boundary_skip: bits & (1 << 10) != 0,
                boundary_active: bits & (1 << 11) != 0,
                flyby_cinematic_done: bits & (1 << 12) != 0,
                quit_requested: bits & (1 << 13) != 0,
                restart_requested: bits & (1 << 14) != 0,
                resume_alt: bits & (1 << 15) != 0,
                resume: bits & (1 << 16) != 0,
                prepause_was_7: bits & (1 << 17) != 0,
                results_page_up: bits & (1 << 18) != 0,
                fade_done: bits & (1 << 19) != 0,
                next_stage_allowed: bits & (1 << 20) != 0,
                championship_exhausted: bits & (1 << 21) != 0,
                show_menu: bits & (1 << 22) != 0,
            };
            for id in 5u8..=0xB {
                let from = state(id);
                let next = race_flow_next(from, inputs).expect("5..=0xb is covered");
                assert!(
                    from.is_known_transition(next),
                    "state {id} -> {} is not a recovered edge",
                    next.id()
                );
            }
        }
    }
}
