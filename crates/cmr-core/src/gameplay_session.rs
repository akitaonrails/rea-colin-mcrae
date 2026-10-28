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

use crate::car_reset::ResetDispatchInput;
use crate::fixed_point::{Fixed16, Vec3};
use crate::game_menu_flow::MenuFlowInputs;
use crate::game_race_flow::{InRaceInputs, race_flow_next};
use crate::game_session::{GameSession, SessionBand};
use crate::game_state::GameState;
use crate::host_loop::HostClock;
use crate::live_static_collision_car_frame::PreviousCarXz;
use crate::race_state::{
    CarState, Controls, FourWheelIterationParams, LiveStaticCollisionStepError, RaceState,
    ResetOutcome, SimParams, WheelPostprocessError,
};
use crate::render_route::{RenderRoute, render_route};
use crate::stage_checkpoints::StageProgress;
use crate::stage_triggers::{RuntimeTrigger, initialize_runtime_triggers};
use crate::static_collision_response::StaticDamageOwner;
use crate::static_collision_scene_builder::ParsedStaticCollisionScene;
use crate::trigger_walk::{
    TriggerCellOutOfRange, TriggerCells, TriggerEvent, TriggerWalkConfig, link_trigger_cells,
};

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
    /// The script for a player's own race (`DAT_00d3ca88 == 0`): the stage
    /// world is ready, the stage-boundary director and the results page are
    /// treated as done and the fade as complete, nothing is quit or restarted,
    /// and no next championship stage is armed, so a finished stage runs
    /// `5 -> 8 -> 0xb -> 3` back to the menu band (the `FUN_0048df30` "show
    /// menu" predicate is not recovered and stays clear). Harness stand-ins.
    #[must_use]
    pub const fn gameplay_harness() -> Self {
        InRaceScript {
            frontend_active: false,
            stage_world_ready: true,
            replay_ready: false,
            replay_intro_done: false,
            flyby_cinematic_done: false,
            quit_requested: false,
            fade_done: true,
        }
    }

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StaticGameplayAdvanceError<E> {
    Wheel(WheelPostprocessError),
    Collision(LiveStaticCollisionStepError<E>),
}

/// The driven session: the recovered FSM plus the deterministic race sim.
pub struct GameplaySession<'a> {
    session: GameSession,
    race: RaceState,
    params: SimParams,
    script: InRaceScript,
    wheel: Option<FourWheelIterationParams<'a>>,
    ground_geometry: Option<&'a crate::stage_compiled_geometry::StageCompiledGeometry>,
    reset_environment: ResetEnvironment,
    last_reset: ResetOutcome,
    stage_triggers: Option<StageTriggers<'a>>,
    last_trigger_events: Vec<TriggerEvent>,
    /// The car as the stage setup left it, restored when a new stage starts.
    stage_start: CarState,
    /// The sim has stepped since the stage started.
    race_started: bool,
    /// The FSM left the gameplay band after a race; the next drive re-inits.
    stage_dirty: bool,
    sim_ticks: u32,
}

/// The stage's `.dat` records with their per-player state and cell lists, and
/// the cell centres `FUN_0048fd20` selects the car's cell from.
struct StageTriggers<'a> {
    runtime: Vec<RuntimeTrigger>,
    /// The freshly loaded state (`FUN_00469630` clears `+0x2e` at load).
    initial: Vec<RuntimeTrigger>,
    cells: TriggerCells,
    cell_centers: &'a [Vec3],
    config: TriggerWalkConfig,
}

/// The words `FUN_00467210` reads besides the car and the `.cat` cursor.
///
/// [`ResetEnvironment::single_player_harness`] is the harness choice: the
/// race-start init `FUN_00437510` sets `DAT_0057b0ac[player] = 100` on both
/// of its branches, so a running race passes the gate; the mode bits
/// `DAT_00d3ca80 +0x166/+0x168` and the loop offset `DAT_0057b030` are not
/// recovered and are zero (the plain mode); the comparison car is car record
/// 1, which the setup clear at `0x404431` zeroes and a single-player race is
/// inferred to leave at the origin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResetEnvironment {
    pub race_state_57b0ac: i32,
    pub replay_d3ca88: bool,
    pub config_166: u8,
    pub config_168: u8,
    pub loop_offset_57b030: i32,
    pub comparison_xz: [i32; 2],
}

impl ResetEnvironment {
    /// The harness defaults described on the type.
    #[must_use]
    pub const fn single_player_harness() -> Self {
        Self {
            race_state_57b0ac: crate::car_reset::RACE_RUNNING_57B0AC,
            replay_d3ca88: false,
            config_166: 0,
            config_168: 0,
            loop_offset_57b030: 0,
            comparison_xz: [0, 0],
        }
    }
}

impl<'a> GameplaySession<'a> {
    /// A fresh simplified session at the boot state with the car at rest.
    #[must_use]
    pub fn new(params: SimParams, script: InRaceScript) -> Self {
        Self::with_optional_wheel(params, script, None)
    }

    /// A simplified session with an explicit stage start.
    #[must_use]
    pub fn new_with_car(params: SimParams, script: InRaceScript, car: CarState) -> Self {
        let mut session = Self::new(params, script);
        session.race = RaceState::new(car);
        session.stage_start = car;
        session
    }

    /// A fresh session driven by the complete four-wheel producer.
    #[must_use]
    pub fn new_with_four_wheel(
        params: SimParams,
        script: InRaceScript,
        wheel: FourWheelIterationParams<'a>,
    ) -> Self {
        Self::with_optional_wheel(params, script, Some(wheel))
    }

    /// A complete four-wheel session with an explicit recovered/harness start.
    #[must_use]
    pub fn new_with_four_wheel_and_car(
        params: SimParams,
        script: InRaceScript,
        wheel: FourWheelIterationParams<'a>,
        car: CarState,
    ) -> Self {
        let mut session = Self::with_optional_wheel(params, script, Some(wheel));
        session.race = RaceState::new(car);
        session.stage_start = car;
        session
    }

    fn with_optional_wheel(
        params: SimParams,
        script: InRaceScript,
        wheel: Option<FourWheelIterationParams<'a>>,
    ) -> Self {
        GameplaySession {
            session: GameSession::new(),
            race: RaceState::new(CarState::at_rest()),
            params,
            script,
            wheel,
            ground_geometry: None,
            reset_environment: ResetEnvironment::single_player_harness(),
            last_reset: ResetOutcome::default(),
            stage_triggers: None,
            last_trigger_events: Vec::new(),
            stage_start: CarState::at_rest(),
            race_started: false,
            stage_dirty: false,
            sim_ticks: 0,
        }
    }

    /// Whether this session steps the complete four-wheel producer, whose car
    /// pose lives in `MotionState::basis` rather than the planar heading.
    #[must_use]
    pub fn is_four_wheel(&self) -> bool {
        self.wheel.is_some()
    }

    /// Run the per-car ground pass `FUN_00410a60` against this stage geometry
    /// after every four-wheel step whose car carries live ground contact.
    #[must_use]
    pub fn with_ground_geometry(
        mut self,
        geometry: &'a crate::stage_compiled_geometry::StageCompiledGeometry,
    ) -> Self {
        self.ground_geometry = Some(geometry);
        self
    }

    /// Replace the reset dispatcher's environment words.
    #[must_use]
    pub fn with_reset_environment(mut self, environment: ResetEnvironment) -> Self {
        self.reset_environment = environment;
        self
    }

    /// The HUD words `FUN_00464130` prepares from the four-wheel car, or
    /// `None` for the planar harness (which has no engine or gearbox).
    #[must_use]
    pub fn prepared_hud_values(
        &self,
        metric_units: bool,
        classic_gauge: bool,
    ) -> Option<crate::race_hud_prepare::PreparedHudValues> {
        let wheel = self.wheel.as_ref()?;
        let car = self.race.car();
        Some(crate::race_hud_prepare::prepare_hud_values(
            crate::race_hud_prepare::HudPrepareInput {
                engine_speed_2e8: car.four_wheel.engine_speed_2e8,
                redline_15c: wheel.engine_smoothing.redline_15c,
                speed_ec: car.four_wheel.speed_ec,
                gear_43a: car.four_wheel.gearbox.gear_43a,
                metric_units,
                classic_gauge,
            },
        ))
    }

    /// What the reset dispatcher did on the last four-wheel tick.
    #[must_use]
    pub fn last_reset(&self) -> ResetOutcome {
        self.last_reset
    }

    /// Run the trigger walk `FUN_00403a00` each four-wheel tick over these
    /// `.dat` records (linked into cells by `FUN_004904e0`) with the harness
    /// [`TriggerWalkConfig::single_player_harness`] words.
    ///
    /// # Errors
    ///
    /// A record names a cell outside `cell_centers`.
    pub fn with_stage_triggers(
        mut self,
        volumes: &[cmr_formats::stage::TriggerVolume],
        cell_centers: &'a [Vec3],
    ) -> Result<Self, TriggerCellOutOfRange> {
        let runtime = initialize_runtime_triggers(volumes);
        self.stage_triggers = Some(StageTriggers {
            initial: runtime.clone(),
            runtime,
            cells: link_trigger_cells(volumes, cell_centers.len())?,
            cell_centers,
            config: TriggerWalkConfig::single_player_harness(),
        });
        Ok(self)
    }

    /// The trigger walk's effects on the last four-wheel tick.
    #[must_use]
    pub fn last_trigger_events(&self) -> &[TriggerEvent] {
        &self.last_trigger_events
    }

    /// The driver and integrator of one four-wheel mechanics tick.
    fn four_wheel_drive(
        &mut self,
        controls: &Controls,
    ) -> Result<crate::race_state::TraceSample, WheelPostprocessError> {
        let wheel = self.wheel.as_ref().expect("a four-wheel session");
        self.race.step_with_all_four_wheels(controls, wheel)
    }

    /// The rest of the mechanics tick after the driver and the static-scene
    /// collision: the trigger walk `FUN_00403a00` (`0x43b327`), then per car
    /// the ground pass `FUN_00410a60` and the reset `FUN_00467210`
    /// (`0x43b50a`, `0x43b517`).
    fn four_wheel_tail(
        &mut self,
        progress: &mut StageProgress,
        sample: &mut crate::race_state::TraceSample,
    ) -> Result<(), WheelPostprocessError> {
        let wheel = self.wheel.as_ref().expect("a four-wheel session");
        self.last_reset = ResetOutcome::default();
        self.last_trigger_events.clear();
        if let Some(triggers) = self.stage_triggers.as_mut() {
            let events = self
                .race
                .run_trigger_walk(
                    &mut triggers.runtime,
                    &triggers.cells,
                    triggers.cell_centers,
                    &triggers.config,
                )
                .map_err(WheelPostprocessError::TriggerWalk)?;
            for event in &events {
                if let TriggerEvent::NodeCursor { payload, .. } = *event
                    && let Ok(cursor) = usize::try_from(payload)
                {
                    progress.set_cursor(cursor);
                }
            }
            self.last_trigger_events = events;
        }
        if let Some(geometry) = self.ground_geometry {
            let mut config = |_lane: usize| Ok::<u8, std::convert::Infallible>(0);
            self.race.run_ground_pass(
                wheel.lane_time_step,
                &mut geometry.eight_lane_providers(),
                &mut config,
            )?;
            let environment = self.reset_environment;
            let dispatch = ResetDispatchInput {
                nodes: progress.nodes(),
                cursor: i32::try_from(progress.cursor()).unwrap_or(i32::MAX),
                loop_offset_57b030: environment.loop_offset_57b030,
                race_state_57b0ac: environment.race_state_57b0ac,
                replay_d3ca88: environment.replay_d3ca88,
                config_166: environment.config_166,
                config_168: environment.config_168,
                comparison_xz: environment.comparison_xz,
                position: Vec3::ZERO,
                row0: Vec3::ZERO,
            };
            let outcome = self.race.run_reset(
                &dispatch,
                &mut geometry.eight_lane_providers(),
                &mut config,
            )?;
            if let Some(cursor) = outcome.cursor.and_then(|c| usize::try_from(c).ok()) {
                progress.set_cursor(cursor);
            }
            self.last_reset = outcome;
        }
        sample.car = self.race.car();
        Ok(())
    }

    /// Advance a simplified session. Four-wheel sessions use [`Self::try_advance`]
    /// to preserve checked producer failures.
    pub fn advance<C: HostClock>(
        &mut self,
        clock: &C,
        menu_inputs: MenuFlowInputs,
        controls: Controls,
        progress: &mut StageProgress,
    ) -> GameplayFrame {
        assert!(
            self.wheel.is_none(),
            "four-wheel sessions must use try_advance"
        );
        self.try_advance(clock, menu_inputs, controls, progress)
            .expect("the simplified race step is infallible")
    }

    /// A new stage starts when driving resumes after the FSM left the gameplay
    /// band: the stage load re-creates the car from its setup, reloads the
    /// trigger records and restarts the checkpoint tracker.
    fn begin_stage_if_reentered(&mut self, progress: &mut StageProgress) {
        if self.session.current().id() != 5 || !self.stage_dirty {
            return;
        }
        self.race = RaceState::new(self.stage_start);
        if let Some(triggers) = self.stage_triggers.as_mut() {
            triggers.runtime.clone_from(&triggers.initial);
        }
        progress.restart();
        self.last_reset = ResetOutcome::default();
        self.last_trigger_events.clear();
        self.stage_dirty = false;
    }

    /// Note when a race hands back to the menu band.
    fn track_stage_end(&mut self, stepped: bool) {
        self.race_started |= stepped;
        if self.race_started && (1..=4).contains(&self.session.current().id()) {
            self.stage_dirty = true;
            self.race_started = false;
        }
    }

    /// Advance one frame, preserving checked four-wheel producer failures. When
    /// already in state 5, the driving step completes before the FSM frame, so an
    /// error does not advance the FSM, stage progress, or simulation tick count.
    ///
    /// # Errors
    ///
    /// Returns contact-index, candidate lookup, or geometric postprocess failures.
    pub fn try_advance<C: HostClock>(
        &mut self,
        clock: &C,
        menu_inputs: MenuFlowInputs,
        controls: Controls,
        progress: &mut StageProgress,
    ) -> Result<GameplayFrame, WheelPostprocessError> {
        self.begin_stage_if_reentered(progress);
        let driving_sample = if self.session.current().id() == 5 {
            Some(if self.wheel.is_some() {
                let race_before = self.race.clone();
                let mut sample = self.four_wheel_drive(&controls)?;
                if let Err(error) = self.four_wheel_tail(progress, &mut sample) {
                    self.race = race_before;
                    return Err(error);
                }
                sample
            } else {
                self.race.step(&controls, &self.params)
            })
        } else {
            None
        };
        let script = self.script;
        let sim_ticks = &mut self.sim_ticks;
        let mut stepped = false;

        let frame = self.session.run_frame(clock, menu_inputs, |state| {
            if state.id() == 5 {
                let sample = driving_sample.expect("state-5 sample was produced");
                *sim_ticks += 1;
                progress.update(sample.car.position, sample.centiseconds);
                stepped = true;
            }
            let inputs = script.with_finish(progress.finished());
            race_flow_next(state, inputs).unwrap_or(state)
        });
        self.track_stage_end(stepped);

        Ok(GameplayFrame {
            band: frame.band,
            route: frame.plan.render.map(render_route),
            stepped,
        })
    }

    /// Advance state 5 through integration and the recovered static-scene pass
    /// before checkpoint progress observes the car. A collision failure restores
    /// the complete pre-step race snapshot and leaves the FSM/progress/tick count
    /// unchanged.
    pub fn try_advance_with_static_collision<C: HostClock, D: StaticDamageOwner>(
        &mut self,
        clock: &C,
        menu_inputs: MenuFlowInputs,
        controls: Controls,
        progress: &mut StageProgress,
        parsed: &ParsedStaticCollisionScene,
        damage_owner: &mut D,
    ) -> Result<GameplayFrame, StaticGameplayAdvanceError<D::Error>> {
        self.begin_stage_if_reentered(progress);
        let driving_sample = if self.session.current().id() == 5 {
            let race_before = self.race.clone();
            let previous_center_xz = PreviousCarXz::capture(race_before.car().position);
            let four_wheel = self.wheel.is_some();
            let mut sample = if four_wheel {
                self.four_wheel_drive(&controls)
                    .map_err(StaticGameplayAdvanceError::Wheel)?
            } else {
                self.race.step(&controls, &self.params)
            };
            if let Err(error) = self.race.resolve_static_collision_after_step(
                previous_center_xz,
                parsed,
                damage_owner,
            ) {
                self.race = race_before;
                return Err(StaticGameplayAdvanceError::Collision(error));
            }
            sample.car = self.race.car();
            if four_wheel && let Err(error) = self.four_wheel_tail(progress, &mut sample) {
                self.race = race_before;
                return Err(StaticGameplayAdvanceError::Wheel(error));
            }
            Some(sample)
        } else {
            None
        };
        let script = self.script;
        let sim_ticks = &mut self.sim_ticks;
        let mut stepped = false;

        let frame = self.session.run_frame(clock, menu_inputs, |state| {
            if state.id() == 5 {
                let sample = driving_sample.expect("state-5 sample was produced");
                *sim_ticks += 1;
                progress.update(sample.car.position, sample.centiseconds);
                stepped = true;
            }
            let inputs = script.with_finish(progress.finished());
            race_flow_next(state, inputs).unwrap_or(state)
        });
        self.track_stage_end(stepped);

        Ok(GameplayFrame {
            band: frame.band,
            route: frame.plan.render.map(render_route),
            stepped,
        })
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
    use crate::c3d_terrain_cull::CellGrid;
    use crate::fixed_point::Vec3;
    use crate::host_loop::HostClock;
    use crate::live_static_collision_car_frame::ActiveCellRecordBuildError;
    use crate::static_collision_kernel::NoStaticDamageOwner;
    use crate::static_world_collision::{StaticCollisionScene, StaticNarrowObb};
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

    fn empty_collision_scene(cell_count: i32) -> ParsedStaticCollisionScene {
        ParsedStaticCollisionScene {
            grid: CellGrid {
                width: cell_count.max(1),
                height: 1,
                cell_count,
                cell_size: 0x20_0000,
                min_x: 0,
                half_cell: 0x10_0000,
                max_z: 0,
            },
            main_cell_centers: vec![Vec3::ZERO],
            scene: StaticCollisionScene::<StaticNarrowObb>::new(Vec::new(), []).unwrap(),
        }
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
            raw_steer: Default::default(),
            raw_pedals: Default::default(),
            raw_gear_command_314: 0,
        }
    }

    fn four_wheel_params(trig: &[i32]) -> FourWheelIterationParams<'_> {
        let inactive = crate::vehicle_dynamics::GeometricPostprocessLane {
            active: false,
            vector_a: Vec3::ZERO,
            vector_b: Vec3::ZERO,
            vector_c: Vec3::ZERO,
            accumulator: Vec3::ZERO,
        };
        FourWheelIterationParams {
            first: [crate::race_state::FirstWheelParams {
                split: 0x10000,
                wheel_constant: 0,
                wheel_vector: [0; 3],
                state_vector: [0; 3],
                scale_c600: 0x10000,
                limit: 0x10000,
                shift: 0,
                factor_c5c0: 0x10000,
                gear: 1,
                config_40c: 0,
                flag_43b: 0,
                config_158: 1,
                gear_ratio: 1,
                is_front_wheel: true,
            }; 4],
            second: std::array::from_fn(|_| crate::race_state::SecondWheelParams {
                axis_dot: 0,
                local_68: [0x10000, 0, 0],
                local_98: [0; 3],
                wheel_e8: 0,
                scale_c5c4: 0x10000,
                residual_flag_147a: false,
                config_2ec: 1,
                gear_ratio: 1,
                coefficient_34: 0x10000,
                arctan_table: &crate::legacy_math::SECOND_WHEEL_ARCTAN_TABLE,
                cos_table: trig,
                config_scale: 1,
                transition_gain: 0,
                transition_base: 0,
                alternate_gain: 0,
                alternate_base: 0,
                correction_gate: 1,
                accumulation_limit: 0x10000,
                correction_alternate: 0x10000,
                surface_vector: [0; 3],
                table_factor: None,
            }),
            contact_vector_indices: [2, 3, 6, 7],
            contact_offsets: [Vec3::ZERO; 4],
            contact_update_inputs: [crate::vehicle_dynamics::ContactVectorUpdateInput {
                update_gate: 0,
                enabled: false,
                vector: Vec3::ZERO,
                basis: Vec3::ZERO,
                scale_528: Fixed16::ONE,
                extra_vector: Vec3::ZERO,
                limit: Fixed16::ONE,
                threshold: Fixed16::ONE,
            }; 8],
            lane_threshold_inputs: crate::compiled_car_initializer::CompiledLaneThresholdInputs {
                lane_words: [0; 8],
                source_y: [0x199a; 8],
            },
            pedal_response: crate::pedal_response::PedalResponseParams::default(),
            lane_time_step: Fixed16::ZERO,
            lane_direction_angle: 0,
            steering: crate::steering_response::SteeringParams::default(),
            steering_car_factor_e188: 0x1_0000,
            steering_surface_factors: [0; 2],
            gearbox: None,
            brake_balance_e1a4: [0x1_0000; 2],
            tyre: None,
            hull_contact_offsets: [Vec3::ZERO; 4],
            // Harness value: never crest.
            crest_128_12c: [i32::MAX, 0],
            lane_rebuild: false,
            lane_exchange: false,
            postprocess: crate::race_state::FourWheelPostprocessParams {
                lanes: [inactive; 4],
                current_lane: 0,
                argument_scale: Fixed16::ONE,
                base_force: Vec3::new(Fixed16::ONE, Fixed16::ZERO, Fixed16::ZERO),
                non_wheel_forces: &[],
                wheel_contacts: [true; 4],
            },
            inverse_mass_4c: Fixed16::ONE,
            angular: crate::vehicle_dynamics::AngularOrientationInput {
                momentum: crate::vehicle_dynamics::AngularMomentumStepInput {
                    prior_moment: Vec3::ZERO,
                    contact_moment: Vec3::ZERO,
                    moment_scale: Fixed16::ONE,
                    inertia_scale: Vec3::new(Fixed16::ONE, Fixed16::ONE, Fixed16::ONE),
                    angular_velocity: Vec3::ZERO,
                    time_step: Fixed16::ZERO,
                },
                clamp_enabled: false,
                control: crate::vehicle_dynamics::ControlAxisFlags {
                    mode_3ec: 0,
                    axis_3f4: 0,
                    axis_3f8: 0,
                },
                basis: [Vec3::ZERO; 3],
            },
            engine_smoothing: crate::drivetrain::SmoothedEngineSpeedInput {
                wheel_speeds: [0; 4],
                config_158: 1,
                gear_ratio: 0x10000,
                engine_2e0: 0,
                redline_15c: 0x10000,
                slip_gain: 0,
                engine_load: 0x10000,
            },
            driven_smoothing: crate::race_state::DrivenWheelUpdateParams::default(),
            engine_force: crate::drivetrain::EngineForceInput {
                limiter_444: false,
                cut_448: 0,
                engine_2e0: 0,
                redline_15c: 0x10000,
                gear: 1,
                config_40c: 0,
                response_2d8: 0,
                response_e1ac: 0,
                engine_brake_144: 0,
                throttle_304: 0,
            },
        }
    }

    #[test]
    fn four_wheel_session_steps_and_preserves_checked_errors() {
        let trig = crate::fixed_point::build_sine_table();
        let wheel = four_wheel_params(&trig);
        let mut session =
            GameplaySession::new_with_four_wheel(params(), InRaceScript::frontend_harness(), wheel);
        let nodes = line();
        let mut progress = StageProgress::new(&nodes, 0);
        for _ in 0..1000 {
            session
                .try_advance(&StationaryClock, frontend_menu(), forward(), &mut progress)
                .unwrap();
            if session.sim_ticks() != 0 {
                break;
            }
        }
        assert_eq!(session.sim_ticks(), 1);

        let mut invalid = four_wheel_params(&trig);
        invalid.contact_vector_indices[0] = 8;
        let mut session = GameplaySession::new_with_four_wheel(
            params(),
            InRaceScript::frontend_harness(),
            invalid,
        );
        let mut progress = StageProgress::new(&nodes, 0);
        let mut failure = None;
        for _ in 0..1000 {
            if let Err(error) =
                session.try_advance(&StationaryClock, frontend_menu(), forward(), &mut progress)
            {
                failure = Some(error);
                break;
            }
        }
        assert_eq!(
            failure,
            Some(WheelPostprocessError::ContactVectorIndex {
                lane: 0,
                vector_index: 8,
            })
        );
        assert_eq!(session.sim_ticks(), 0);
        assert!(!progress.finished());
    }

    #[test]
    fn static_collision_advance_is_atomic_before_progress_and_fsm_commit() {
        let mut session = GameplaySession::new(params(), InRaceScript::frontend_harness());
        let nodes = line();
        let mut progress = StageProgress::new(&nodes, 0);
        let invalid = empty_collision_scene(2);
        let mut failure = None;
        for _ in 0..1000 {
            let before = session.car();
            match session.try_advance_with_static_collision(
                &StationaryClock,
                frontend_menu(),
                forward(),
                &mut progress,
                &invalid,
                &mut NoStaticDamageOwner,
            ) {
                Ok(_) => {}
                Err(error) => {
                    assert_eq!(session.car(), before);
                    failure = Some(error);
                    break;
                }
            }
        }
        assert!(matches!(
            failure,
            Some(StaticGameplayAdvanceError::Collision(
                LiveStaticCollisionStepError::ActiveCell(
                    ActiveCellRecordBuildError::CellCountMismatch { .. }
                )
            ))
        ));
        assert_eq!(session.sim_ticks(), 0);
        assert!(!progress.finished());

        let mut session = GameplaySession::new(params(), InRaceScript::frontend_harness());
        let mut progress = StageProgress::new(&nodes, 0);
        let valid = empty_collision_scene(1);
        for _ in 0..1000 {
            let frame = session
                .try_advance_with_static_collision(
                    &StationaryClock,
                    frontend_menu(),
                    forward(),
                    &mut progress,
                    &valid,
                    &mut NoStaticDamageOwner,
                )
                .unwrap();
            if frame.stepped {
                break;
            }
        }
        assert_eq!(session.sim_ticks(), 1);
    }

    #[test]
    fn compiled_constants_fill_mass_inertia_pedals_and_mode() {
        let trig = crate::fixed_point::build_sine_table();
        let mut wheel = four_wheel_params(&trig);
        let constants = crate::compiled_car_initializer::CompiledFourWheelConstants {
            state_word_48: 0x7a_0000,
            state_word_4c: 0x218,
            inertia_50_54_58: [1, 2, 1],
            control_word_148: 0x3_0000,
            control_word_16c: 0x2_0000,
            control_word_158: 1,
            control_word_150: 0x0ccc,
            control_word_174: 0x0ccd,
            control_word_17c: 0x0cce,
            control_word_184: 0x8000,
            control_word_408: 1,
            control_word_188: 0x3333,
            control_u16_3e4: 0x02aa,
            control_byte_440: 3,
            control_word_15c: 0x5_0000,
            gravity_c5fc: 0x9999,
            control_word_420: 1,
        };
        crate::race_state::populate_compiled_four_wheel_constants(&mut wheel, &constants);
        assert_eq!(wheel.inverse_mass_4c.raw(), 0x218);
        assert_eq!(wheel.angular.momentum.moment_scale.raw(), 0x7a_0000);
        assert_eq!(
            [
                wheel.angular.momentum.inertia_scale.x.raw(),
                wheel.angular.momentum.inertia_scale.y.raw(),
                wheel.angular.momentum.inertia_scale.z.raw(),
            ],
            [1, 2, 1]
        );
        assert_eq!(
            wheel.pedal_response,
            crate::pedal_response::PedalResponseParams {
                throttle_max_148: 0x3_0000,
                throttle_rate_150: 0x0ccc,
                brake_max_16c: 0x2_0000,
                brake_rate_174: 0x0ccd,
                handbrake_rate_17c: 0x0cce,
                handbrake_max_184: 0x8000,
            }
        );
        assert_eq!(wheel.driven_smoothing.mode_408, 1);
        assert_eq!(
            (
                wheel.steering.centering_188,
                wheel.steering.max_angle_3e4,
                wheel.steering.flags_440,
            ),
            (0x3333, 0x02aa, 3)
        );
        assert_eq!(wheel.engine_smoothing.redline_15c, 0x5_0000);
        assert_eq!(wheel.engine_force.redline_15c, 0x5_0000);
        assert_eq!(
            wheel.postprocess.base_force,
            crate::race_state::gravity_force_51c(
                crate::fixed_point::Fixed16::from_raw(0x7a_0000),
                crate::fixed_point::Fixed16::from_raw(0x9999),
            )
        );
        assert!(
            wheel
                .second
                .iter()
                .all(|second| second.correction_gate == 1)
        );
    }

    #[test]
    fn setup_block_fills_engine_steering_wheel_and_gearbox_words() {
        let trig = crate::fixed_point::build_sine_table();
        let mut wheel = four_wheel_params(&trig);
        wheel.gearbox = Some(crate::gearbox::GearboxParams::default());
        let mut setup = crate::car_setup::CarSetupBlock::RESET;
        setup.engine_response_85c = 0xe000;
        setup.steering_factor_838 = 0xd000;
        setup.wheel_constants_83c = [1, 2, 3, 4];
        setup.base_angle_bac = -5;
        setup.setup_800[8] = 0xb334;
        crate::race_state::populate_car_setup(&mut wheel, &setup);
        assert_eq!(wheel.engine_force.response_e1ac, 0xe000);
        assert_eq!(wheel.steering_car_factor_e188, 0xd000);
        assert_eq!(wheel.lane_direction_angle, -5);
        assert_eq!(wheel.first.map(|first| first.wheel_constant), [1, 2, 3, 4]);
        assert!(wheel.gearbox.unwrap().gear_two_skip);
    }

    #[test]
    fn four_wheel_base_force_produces_world_motion() {
        let trig = crate::fixed_point::build_sine_table();
        let wheel = four_wheel_params(&trig);
        let mut session =
            GameplaySession::new_with_four_wheel(params(), InRaceScript::frontend_harness(), wheel);
        let nodes = line();
        let mut progress = StageProgress::new(&nodes, 0);
        for _ in 0..100 {
            session
                .try_advance(&StationaryClock, frontend_menu(), forward(), &mut progress)
                .unwrap();
        }
        assert!(session.sim_ticks() > 0);
        assert!(session.car().position.x.raw() > 0);
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
    fn a_finished_race_returns_to_the_menu_and_the_next_stage_starts_fresh() {
        let mut session = GameplaySession::new(params(), InRaceScript::gameplay_harness());
        let nodes = line();
        let mut progress = StageProgress::new(&nodes, 0);
        let mut frames = 0u32;
        let mut left_gameplay = false;
        let mut finish_position = Vec3::ZERO;
        while frames < 100_000 {
            frames += 1;
            let frame =
                session.advance(&StationaryClock, frontend_menu(), forward(), &mut progress);
            if progress.finished() && !frame.stepped && session.state().id() <= 4 {
                left_gameplay = true;
                finish_position = session.car().position;
                break;
            }
        }
        assert!(
            left_gameplay,
            "a normal finish runs 5 -> 8 -> 0xb back to the menu band"
        );
        assert!(!session.is_stopped());
        assert!(finish_position.x.raw() > 0);
        // The menu flow starts the stage again; the first drive starts over.
        let mut restarted = false;
        while frames < 200_000 {
            frames += 1;
            let frame =
                session.advance(&StationaryClock, frontend_menu(), forward(), &mut progress);
            if frame.stepped {
                restarted = true;
                assert!(session.car().position.x.raw() < finish_position.x.raw());
                assert!(!progress.finished());
                break;
            }
        }
        assert!(restarted);
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
            raw_steer: Default::default(),
            raw_pedals: Default::default(),
            raw_gear_command_314: 0,
        };
        for _ in 0..1_000 {
            session.advance(&StationaryClock, frontend_menu(), still, &mut progress);
        }
        assert!(!progress.finished());
        assert!(!session.is_stopped());
    }
}
