//! The driven top-level session: [`GameSession`] plus the fixed-step race sim,
//! advanced from **injected per-frame controls** and reporting the per-frame
//! render route.
//!
//! [`GameSession`] already runs the recovered FSM from boot to quit, deferring
//! the gameplay band (`5..=0xb`) to a caller handler. This driver supplies that
//! handler: in state `5` (the main driving tick) it steps the deterministic
//! [`RaceState`] with the frame's [`Controls`] and feeds the car position to a
//! caller-owned [`StageProgress`] (the recovered checkpoint tracker) that raises
//! the finish; every in-race transition is the recovered
//! [`race_flow_next`] table. The same driver backs both the headless path (fixed
//! controls each frame) and the windowed path (live controls from the recovered
//! keyboard chain), so they drive identical logic.
//!
//! Presentation is not performed here: each [`advance`](GameplaySession::advance)
//! reports the [`RenderRoute`] of the state whose render handler ran this frame
//! ([`FramePlan::render`]), and the backend presents accordingly. The in-race
//! sequencer flags that are not the player's doing (replay/flyby/fade/quit) are a
//! documented [`InRaceScript`] harness stand-in until those subsystems are wired;
//! only `stage_finished` comes from the live tracker.

use crate::fixed_point::Fixed16;
use crate::game_menu_flow::MenuFlowInputs;
use crate::game_race_flow::{InRaceInputs, race_flow_next};
use crate::game_session::{GameSession, SessionBand};
use crate::game_state::GameState;
use crate::host_loop::HostClock;
use crate::race_state::{CarState, Controls, RaceState, SimParams};
use crate::render_route::{RenderRoute, render_route};
use crate::stage_checkpoints::StageProgress;

/// The in-race sequencer inputs that are not the player's live controls: the
/// replay/flyby/fade/quit flags and the front-end/world-ready gates the recovered
/// in-race graph branches on. A **harness stand-in** for the real subsystems
/// (`race_flow` documents each); only `stage_finished` is supplied live, from the
/// checkpoint tracker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InRaceScript {
    /// Running under the front-end sequencer (`frontend_active`).
    pub frontend_active: bool,
    /// The stage world has finished loading (`stage_world_ready`).
    pub stage_world_ready: bool,
    /// The replay camera is ready (`replay_ready`).
    pub replay_ready: bool,
    /// The replay intro has played out (`replay_intro_done`).
    pub replay_intro_done: bool,
    /// The finish fly-by cinematic has played out (`flyby_cinematic_done`).
    pub flyby_cinematic_done: bool,
    /// A quit was requested from the post-race hub (`quit_requested`).
    pub quit_requested: bool,
    /// The quit fade has completed (`fade_done`).
    pub fade_done: bool,
}

impl InRaceScript {
    /// The headless/front-end harness script: every sequencer gate satisfied so
    /// the recovered front-end path (`5 -> 9 -> 0xb -> 0xc`) runs to completion
    /// once the tracker raises the finish. Matches the headless game session.
    #[must_use]
    pub const fn frontend_harness() -> Self {
        InRaceScript {
            frontend_active: true,
            stage_world_ready: true,
            replay_ready: true,
            replay_intro_done: true,
            flyby_cinematic_done: true,
            quit_requested: true,
            fade_done: true,
        }
    }

    fn with_finish(self, stage_finished: bool) -> InRaceInputs {
        InRaceInputs {
            frontend_active: self.frontend_active,
            stage_world_ready: self.stage_world_ready,
            stage_finished,
            replay_ready: self.replay_ready,
            replay_intro_done: self.replay_intro_done,
            flyby_cinematic_done: self.flyby_cinematic_done,
            quit_requested: self.quit_requested,
            fade_done: self.fade_done,
            ..InRaceInputs::default()
        }
    }
}

/// What one [`GameplaySession::advance`] produced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GameplayFrame {
    /// The FSM band the frame dispatched from.
    pub band: SessionBand,
    /// The render route of the state whose render handler ran this frame, or
    /// `None` when the render-skip gate dropped this frame's render.
    pub route: Option<RenderRoute>,
    /// Whether the driving sim stepped this frame (the live state was `5`).
    pub stepped: bool,
}

/// The driven session: the recovered FSM plus the deterministic race sim.
pub struct GameplaySession {
    session: GameSession,
    race: RaceState,
    params: SimParams,
    script: InRaceScript,
    sim_ticks: u32,
}

impl GameplaySession {
    /// A fresh session at the boot state with the car at rest.
    #[must_use]
    pub fn new(params: SimParams, script: InRaceScript) -> Self {
        GameplaySession {
            session: GameSession::new(),
            race: RaceState::new(CarState::at_rest()),
            params,
            script,
            sim_ticks: 0,
        }
    }

    /// Advance one frame: run the FSM under `menu_inputs`, step the race sim with
    /// `controls` when the live state is the driving tick (`5`), advance the
    /// caller's checkpoint `progress`, and report the frame's render route.
    pub fn advance<C: HostClock>(
        &mut self,
        clock: &C,
        menu_inputs: MenuFlowInputs,
        controls: Controls,
        progress: &mut StageProgress,
    ) -> GameplayFrame {
        // Disjoint field borrows so the in-race closure can touch the sim while
        // the session borrows itself to run the frame.
        let race = &mut self.race;
        let params = &self.params;
        let script = self.script;
        let sim_ticks = &mut self.sim_ticks;
        let mut stepped = false;

        let frame = self.session.run_frame(clock, menu_inputs, |state| {
            if state.id() == 5 {
                let sample = race.step(&controls, params);
                *sim_ticks += 1;
                progress.update(sample.car.position, sample.centiseconds);
                stepped = true;
            }
            let inputs = script.with_finish(progress.finished());
            race_flow_next(state, inputs).unwrap_or(state)
        });

        GameplayFrame {
            band: frame.band,
            route: frame.plan.render.map(render_route),
            stepped,
        }
    }

    /// Report the planned render's overrun to the driver's render-skip gate.
    pub fn report_render(&mut self, overran: bool) {
        self.session.report_render(overran);
    }

    /// Whether the session has drained to the terminal stopped state.
    #[must_use]
    pub fn is_stopped(&self) -> bool {
        self.session.is_stopped()
    }

    /// The live FSM state id.
    #[must_use]
    pub fn state(&self) -> GameState {
        self.session.current()
    }

    /// The current car state (position/heading/velocity), for a follow camera.
    #[must_use]
    pub fn car(&self) -> CarState {
        self.race.car()
    }

    /// The stage clock (Q16.16 seconds) the race sim has accumulated.
    #[must_use]
    pub fn stage_clock(&self) -> Fixed16 {
        self.race.clock()
    }

    /// Total driving-sim ticks stepped so far.
    #[must_use]
    pub fn sim_ticks(&self) -> u32 {
        self.sim_ticks
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixed_point::Vec3;
    use crate::host_loop::HostClock;
    use cmr_formats::stage::CatNode;

    struct StationaryClock;
    impl HostClock for StationaryClock {
        fn millis(&self) -> u32 {
            0
        }
    }

    fn line() -> Vec<CatNode> {
        [10i32, 20, 40]
            .into_iter()
            .map(|x| CatNode {
                position: [x << 16, 0, 0],
                direction: [1 << 16, 0, 0],
                split_flag: 0b1,
                reserved: 0,
                seq_index: 0,
            })
            .collect()
    }

    fn params() -> SimParams {
        SimParams {
            inverse_mass: Fixed16::ONE,
            gravity: Vec3::ZERO,
        }
    }

    fn frontend_menu() -> MenuFlowInputs {
        MenuFlowInputs {
            frontend_active: true,
            root_menu_ready: true,
            screen_graph_ready: true,
            ..MenuFlowInputs::default()
        }
    }

    fn forward() -> Controls {
        Controls {
            steer_rate: 0,
            forward_force: Fixed16::from_raw(8 << 16),
            handbrake: false,
        }
    }

    #[test]
    fn a_forward_drive_runs_the_fsm_from_boot_to_stopped() {
        let mut session = GameplaySession::new(params(), InRaceScript::frontend_harness());
        let nodes = line();
        let mut progress = StageProgress::new(&nodes, 0);

        let mut frames = 0u32;
        while !session.is_stopped() && frames < 100_000 {
            frames += 1;
            session.advance(&StationaryClock, frontend_menu(), forward(), &mut progress);
        }
        assert!(session.is_stopped());
        assert_eq!(session.state().id(), 0);
        assert!(progress.finished());
        assert!(session.sim_ticks() > 0);
        // The car advanced in +x under the forward force.
        assert!(session.car().position.x.raw() > 0);
    }

    #[test]
    fn the_in_race_band_presents_the_stage_world_route() {
        let mut session = GameplaySession::new(params(), InRaceScript::frontend_harness());
        let nodes = line();
        let mut progress = StageProgress::new(&nodes, 0);

        // Run until the driving tick (state 5) has stepped, and confirm that
        // frame's render route is the in-race stage-world composition.
        let mut saw_stage_world = false;
        let mut frames = 0u32;
        while !session.is_stopped() && frames < 100_000 {
            frames += 1;
            let frame =
                session.advance(&StationaryClock, frontend_menu(), forward(), &mut progress);
            if frame.stepped {
                assert_eq!(frame.band, SessionBand::Gameplay);
                if matches!(frame.route, Some(RenderRoute::StageWorld(_))) {
                    saw_stage_world = true;
                }
            }
        }
        assert!(saw_stage_world, "the driving tick presents the stage world");
    }

    #[test]
    fn a_still_car_never_reaches_the_finish_line() {
        // With no forward force the car never crosses a gate, so the FSM stays in
        // the gameplay band (never funnels through the finish).
        let mut session = GameplaySession::new(params(), InRaceScript::frontend_harness());
        let nodes = line();
        let mut progress = StageProgress::new(&nodes, 0);
        let still = Controls {
            steer_rate: 0,
            forward_force: Fixed16::ZERO,
            handbrake: false,
        };
        for _ in 0..1_000 {
            session.advance(&StationaryClock, frontend_menu(), still, &mut progress);
        }
        assert!(!progress.finished());
        assert!(!session.is_stopped());
    }
}
