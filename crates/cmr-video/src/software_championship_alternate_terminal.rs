//! Combined alternate-RGB16 championship setup and terminal C3D scene.
//!
//! The complete ordinary-bitmap stream and optional car scene are preflighted
//! before the first target write. Setup commands then execute in recovered
//! transcript order, with transform/latch boundaries coordinated inline, and
//! the common C3D scene runs only after the complete setup stream.

use std::fmt;

use cmr_core::{
    c3d_topology::C3dObjectTopology,
    championship_car_execute::{
        ChampionshipCarBatchExecutor, ChampionshipCarBatchPreflightError,
        ChampionshipCarPreDrawState,
    },
    championship_car_frame_plan::{ChampionshipCarFramePlanError, plan_championship_car_frame},
    championship_car_scene::{
        ChampionshipCarSceneError, ChampionshipCarSceneOutcome,
        execute_optional_championship_car_scene_with_post_reset,
        preflight_optional_championship_car_terminal_scene,
    },
    championship_car_transform::{ChampionshipCarInput, ChampionshipCarState},
    championship_car_visibility::ChampionshipCarVisibilityLatches,
    championship_setup::CanonicalCar,
    championship_setup_frame::{
        DeferredChampionshipCommand, PreparedChampionshipAlternateSetupFrame,
    },
    championship_setup_visual::ChampionshipPreviewCache,
    championship_terminal_frame::{
        ChampionshipCarContractError, scan_championship_car_contract_for_resource,
    },
    deferred_palette::{
        DeferredPaletteCommitOutcome, DeferredPaletteState, PaletteGroupBases,
        resolve_palette_index,
    },
    display::DisplayExtent,
    frontend_c3d_route::{FrontendC3dRoute, FrontendC3dRouteState},
    frontend_c3d_scene::C3dDepthClear,
    root_menu_c3d_runtime::C3dResourceScaledObject,
    root_menu_c3d_visibility::RootMenuC3dVisibilityLatches,
};

use crate::{
    c3d_software_split_backend::{
        SoftwareSplitC3dBackendError, SoftwareSplitC3dBinding, SoftwareSplitC3dOracleBackend,
        SoftwareSplitC3dTargetHandle,
    },
    software_championship_ordinary_rgb16::SoftwareChampionshipOrdinaryRgb16Pool,
    software_championship_selection_rgb16::SoftwareChampionshipSelectionRgb16Source,
    software_championship_setup_frame::{
        SoftwareChampionshipSetupCommandOutcome, SoftwareChampionshipSetupDeferredCommand,
        SoftwareChampionshipSetupFrameError, SoftwareChampionshipSetupFrameOutcome,
        execute_preflighted_software_championship_setup_frame,
        preflight_software_championship_setup_frame,
    },
    software_queued_font_flush::{
        PreflightedSoftwareQueuedFontFlushPlan, SoftwareQueuedFontFlushOutcome,
        SoftwareQueuedFontFlushSink, dispatch_preflighted_software_queued_font_flush_plan,
    },
    software_queued_font_queue::RoutedChampionshipQueuedText,
};

pub struct SoftwareChampionshipAlternateCarResources<'a, 'actors> {
    pub car: CanonicalCar,
    pub topology: &'a C3dObjectTopology<'a>,
    pub runtime: &'a C3dResourceScaledObject,
    pub base_basis: [i32; 9],
    pub pre_draw_state: ChampionshipCarPreDrawState<'actors>,
}

pub struct SoftwareChampionshipAlternateTerminalState<'a> {
    pub preview_cache: &'a mut ChampionshipPreviewCache,
    pub car_transform: &'a mut ChampionshipCarState,
    pub car_visibility: &'a mut ChampionshipCarVisibilityLatches,
    pub root_visibility: &'a mut RootMenuC3dVisibilityLatches,
    pub palette_state: &'a mut DeferredPaletteState,
    pub palette_group_bases: PaletteGroupBases,
    pub route: &'a FrontendC3dRouteState,
}

#[derive(Debug, Eq, PartialEq)]
pub struct SoftwareChampionshipAlternateTerminalOutcome<Q = ()> {
    pub setup: SoftwareChampionshipSetupFrameOutcome,
    pub scene: ChampionshipCarSceneOutcome,
    /// The alternate RGB16 terminal reaches the recovered palette boundary,
    /// where non-indexed depth skips upload and preserves any pending request.
    pub palette_commit: DeferredPaletteCommitOutcome,
    /// Semantic queued-font submissions performed inside the common C3D scene
    /// after world reset and before its opaque tail. These counts do not claim
    /// rasterized pixels.
    pub font_dispatch: SoftwareQueuedFontFlushOutcome,
    /// Complete caller-owned publication attached only after terminal success.
    pub queued_text: Q,
}

#[derive(Debug, Eq, PartialEq)]
pub enum SoftwareChampionshipAlternateTerminalError {
    Setup(SoftwareChampionshipSetupFrameError),
    CarContract(ChampionshipCarContractError),
    CarPlan {
        command_index: usize,
        source: ChampionshipCarFramePlanError,
    },
    CarPrepare {
        command_index: usize,
        source: ChampionshipCarBatchPreflightError,
    },
    Scene {
        terminal_index: usize,
        source: Box<ChampionshipCarSceneError<SoftwareSplitC3dBackendError>>,
    },
}

impl fmt::Display for SoftwareChampionshipAlternateTerminalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Setup(source) => source.fmt(formatter),
            Self::CarContract(source) => source.fmt(formatter),
            Self::CarPlan {
                command_index,
                source,
            } => write!(
                formatter,
                "alternate championship car plan at command {command_index} failed: {source}"
            ),
            Self::CarPrepare {
                command_index,
                source,
            } => write!(
                formatter,
                "alternate championship car preparation at command {command_index} failed: {source}"
            ),
            Self::Scene {
                terminal_index,
                source,
            } => write!(
                formatter,
                "alternate championship scene at terminal {terminal_index} failed: {source}"
            ),
        }
    }
}

impl std::error::Error for SoftwareChampionshipAlternateTerminalError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Setup(source) => Some(source),
            Self::CarContract(source) => Some(source),
            Self::CarPlan { source, .. } => Some(source),
            Self::CarPrepare { source, .. } => Some(source),
            Self::Scene { source, .. } => Some(source),
        }
    }
}

/// Executes one prepared setup stream and its selected-car terminal scene on
/// the same owner-scoped logical target.
///
/// Text, info text, and next-preview work remain typed deferred commands.
/// A supplied converted Selection executes through the bounded source-owned
/// Stretch policy. Selection omitted during preparation and future unsupported
/// Stretch identities remain deferred; a prepared Selection bitmap with a
/// missing execution source is rejected during whole-frame preflight.
/// Palette requests, car transforms, and visibility latches
/// are coordinated inline; the terminal reports the non-indexed palette skip
/// while retaining any pending request. Setup cache, transform, and pending
/// palette state remain committed if the later scene fails, matching the
/// indexed terminal owner; all latches are consumed after every scene attempt.
#[allow(clippy::too_many_arguments)]
pub fn execute_software_championship_alternate_terminal_frame<'catalog>(
    prepared_setup: PreparedChampionshipAlternateSetupFrame,
    extent: DisplayExtent,
    car_input: ChampionshipCarInput,
    car_resources: Option<SoftwareChampionshipAlternateCarResources<'_, '_>>,
    state: SoftwareChampionshipAlternateTerminalState<'_>,
    executor: &mut ChampionshipCarBatchExecutor<'catalog, SoftwareSplitC3dBinding>,
    sources: &SoftwareChampionshipOrdinaryRgb16Pool,
    selection: Option<&SoftwareChampionshipSelectionRgb16Source>,
    backend: &mut SoftwareSplitC3dOracleBackend,
    destination: SoftwareSplitC3dTargetHandle,
    depth_clear: C3dDepthClear,
) -> Result<SoftwareChampionshipAlternateTerminalOutcome, SoftwareChampionshipAlternateTerminalError>
{
    let mut font_sink = ();
    execute_software_championship_alternate_terminal_frame_with_publication(
        prepared_setup,
        extent,
        car_input,
        car_resources,
        state,
        executor,
        sources,
        selection,
        backend,
        destination,
        depth_clear,
        (),
        None,
        &mut font_sink,
    )
}

/// Session-only boundary which publishes a complete queued-text value iff the
/// terminal frame succeeds. A later terminal failure drops the unpublished
/// value; it does not make existing bitmap/scene effects transactional.
#[allow(clippy::too_many_arguments)]
pub(crate) fn execute_software_championship_alternate_terminal_frame_with_queued_text<
    'catalog,
    S: SoftwareQueuedFontFlushSink,
>(
    prepared_setup: PreparedChampionshipAlternateSetupFrame,
    extent: DisplayExtent,
    car_input: ChampionshipCarInput,
    car_resources: Option<SoftwareChampionshipAlternateCarResources<'_, '_>>,
    state: SoftwareChampionshipAlternateTerminalState<'_>,
    executor: &mut ChampionshipCarBatchExecutor<'catalog, SoftwareSplitC3dBinding>,
    sources: &SoftwareChampionshipOrdinaryRgb16Pool,
    selection: Option<&SoftwareChampionshipSelectionRgb16Source>,
    backend: &mut SoftwareSplitC3dOracleBackend,
    destination: SoftwareSplitC3dTargetHandle,
    depth_clear: C3dDepthClear,
    queued_text: RoutedChampionshipQueuedText,
    font_plan: PreflightedSoftwareQueuedFontFlushPlan<'_>,
    font_sink: &mut S,
) -> Result<
    SoftwareChampionshipAlternateTerminalOutcome<RoutedChampionshipQueuedText>,
    SoftwareChampionshipAlternateTerminalError,
> {
    execute_software_championship_alternate_terminal_frame_with_publication(
        prepared_setup,
        extent,
        car_input,
        car_resources,
        state,
        executor,
        sources,
        selection,
        backend,
        destination,
        depth_clear,
        queued_text,
        Some(font_plan),
        font_sink,
    )
}

#[allow(clippy::too_many_arguments)]
fn execute_software_championship_alternate_terminal_frame_with_publication<
    'catalog,
    Q,
    S: SoftwareQueuedFontFlushSink,
>(
    prepared_setup: PreparedChampionshipAlternateSetupFrame,
    extent: DisplayExtent,
    car_input: ChampionshipCarInput,
    car_resources: Option<SoftwareChampionshipAlternateCarResources<'_, '_>>,
    state: SoftwareChampionshipAlternateTerminalState<'_>,
    executor: &mut ChampionshipCarBatchExecutor<'catalog, SoftwareSplitC3dBinding>,
    sources: &SoftwareChampionshipOrdinaryRgb16Pool,
    selection: Option<&SoftwareChampionshipSelectionRgb16Source>,
    backend: &mut SoftwareSplitC3dOracleBackend,
    destination: SoftwareSplitC3dTargetHandle,
    depth_clear: C3dDepthClear,
    publication: Q,
    font_plan: Option<PreflightedSoftwareQueuedFontFlushPlan<'_>>,
    font_sink: &mut S,
) -> Result<
    SoftwareChampionshipAlternateTerminalOutcome<Q>,
    SoftwareChampionshipAlternateTerminalError,
> {
    prepared_setup
        .validate_preview_cache(state.preview_cache)
        .map_err(|stale| {
            SoftwareChampionshipAlternateTerminalError::Setup(
                SoftwareChampionshipSetupFrameError::StalePreviewCache {
                    expected: stale.expected,
                    actual: stale.actual,
                },
            )
        })?;
    let resource_car = car_resources.as_ref().map(|resources| resources.car);
    let contract = scan_championship_car_contract_for_resource(&prepared_setup, resource_car)
        .map_err(SoftwareChampionshipAlternateTerminalError::CarContract)?;
    let terminal_index = prepared_setup.command_len();

    let mut next_transform = *state.car_transform;
    let prepared_car = match (contract.as_ref(), car_resources.as_ref()) {
        (Some(contract), Some(resources)) => {
            let plan = plan_championship_car_frame(
                resources.topology,
                resources.runtime,
                &mut next_transform,
                resources.car,
                car_input,
                resources.base_basis,
                contract.logo_origin,
                extent,
            )
            .map_err(
                |source| SoftwareChampionshipAlternateTerminalError::CarPlan {
                    command_index: contract.command_index,
                    source,
                },
            )?;
            Some(
                executor
                    .prepare(plan, resources.pre_draw_state)
                    .map_err(
                        |source| SoftwareChampionshipAlternateTerminalError::CarPrepare {
                            command_index: contract.command_index,
                            source,
                        },
                    )?,
            )
        }
        (None, None) => None,
        _ => unreachable!("contract scan reconciles car resources"),
    };
    let selected_visible = prepared_car
        .as_ref()
        .is_some_and(|prepared| prepared.selected_parent_visible());

    preflight_software_championship_setup_frame(
        &prepared_setup,
        sources,
        selection,
        backend,
        destination,
    )
    .map_err(SoftwareChampionshipAlternateTerminalError::Setup)?;

    // Route entry precedes whole-frame C3D preflight so retained-binding plans
    // exactly match the subsequent terminal execution.
    if state.route.enter(FrontendC3dRoute::ChampionshipCar) {
        executor.invalidate_bindings();
    }
    preflight_optional_championship_car_terminal_scene(
        executor,
        backend,
        destination,
        extent,
        depth_clear,
        prepared_car.as_ref(),
    )
    .map_err(|source| SoftwareChampionshipAlternateTerminalError::Scene {
        terminal_index,
        source: Box::new(source),
    })?;

    let selected_car = contract.as_ref().map(|contract| contract.car);
    let mut transform_seen = false;
    let setup_commands = execute_preflighted_software_championship_setup_frame(
        prepared_setup,
        state.preview_cache,
        sources,
        selection,
        backend,
        destination,
        |command_index, command| match command {
            DeferredChampionshipCommand::CarTransform(_) => {
                *state.car_transform = next_transform;
                transform_seen = true;
                Ok(
                    SoftwareChampionshipSetupCommandOutcome::CoordinatedChampionship {
                        command_index,
                        command,
                    },
                )
            }
            DeferredChampionshipCommand::CarVisibilityLatch(_) => {
                debug_assert!(transform_seen);
                if selected_visible {
                    state.car_visibility.show_all_layers(
                        selected_car.expect("validated car latch has selected car"),
                    );
                }
                Ok(
                    SoftwareChampionshipSetupCommandOutcome::CoordinatedChampionship {
                        command_index,
                        command,
                    },
                )
            }
            DeferredChampionshipCommand::PaletteRequest { .. } => {
                let DeferredChampionshipCommand::PaletteRequest { group, item } = command else {
                    unreachable!()
                };
                state.palette_state.request(resolve_palette_index(
                    i32::from(item),
                    i32::from(group),
                    state.palette_group_bases,
                ));
                Ok(
                    SoftwareChampionshipSetupCommandOutcome::CoordinatedChampionship {
                        command_index,
                        command,
                    },
                )
            }
            DeferredChampionshipCommand::Selection(_)
            | DeferredChampionshipCommand::InfoText(_)
            | DeferredChampionshipCommand::NextPreview(_) => {
                Ok(SoftwareChampionshipSetupCommandOutcome::Deferred {
                    command_index,
                    command: SoftwareChampionshipSetupDeferredCommand::Championship(command),
                })
            }
        },
    );
    let setup_commands = match setup_commands {
        Ok(commands) => commands,
        Err(source) => {
            state.car_visibility.clear_all();
            state.root_visibility.clear_all();
            return Err(SoftwareChampionshipAlternateTerminalError::Setup(source));
        }
    };

    let (scene, font_dispatch) =
        execute_scene_with_font_dispatch(font_plan, font_sink, |dispatch| {
            execute_optional_championship_car_scene_with_post_reset(
                executor,
                backend,
                destination,
                extent,
                depth_clear,
                prepared_car,
                |_| dispatch(),
            )
            .map_err(Box::new)
        });
    state.car_visibility.clear_all();
    state.root_visibility.clear_all();
    let scene = match scene {
        Ok(scene) => scene,
        Err(source) => {
            return Err(SoftwareChampionshipAlternateTerminalError::Scene {
                terminal_index,
                source,
            });
        }
    };

    Ok(SoftwareChampionshipAlternateTerminalOutcome {
        setup: SoftwareChampionshipSetupFrameOutcome {
            commands: setup_commands,
        },
        scene,
        palette_commit: DeferredPaletteCommitOutcome::SkippedNonIndexedDepth,
        font_dispatch,
        queued_text: publication,
    })
}

/// Sole placement seam for alternate-terminal font dispatch.
///
/// The production terminal calls this boundary exactly once around its scene
/// transaction. This owner retains and consumes the preflighted plan only when
/// that transaction reaches its post-world-reset hook.
fn execute_scene_with_font_dispatch<T, E, S: SoftwareQueuedFontFlushSink>(
    mut font_plan: Option<PreflightedSoftwareQueuedFontFlushPlan<'_>>,
    font_sink: &mut S,
    execute_scene: impl FnOnce(&mut dyn FnMut()) -> Result<T, E>,
) -> (Result<T, E>, SoftwareQueuedFontFlushOutcome) {
    let mut outcome = SoftwareQueuedFontFlushOutcome::default();
    let mut dispatch = || {
        if let Some(plan) = font_plan.take() {
            outcome = dispatch_preflighted_software_queued_font_flush_plan(plan, font_sink);
        }
    };
    let scene = execute_scene(&mut dispatch);
    (scene, outcome)
}

#[cfg(test)]
mod font_terminal_wiring_tests {
    use std::{cell::RefCell, rc::Rc};

    use cmr_core::{
        display::DisplayExtent,
        font_assets::{FontSet, FontStyle},
        frontend_c3d_scene::{
            C3dBindingState, C3dDepthClear, C3dMatrixWords, C3dSceneStage, C3dViewportWords,
            FrontendC3dSceneBackend, execute_frontend_c3d_scene_with_post_reset,
        },
    };
    use cmr_formats::font::{FontLimits, RectI16, RectI32, parse_legacy_font};

    use super::*;
    use crate::{
        software_queued_font_argb4444::{
            SoftwareQueuedFontArgb4444Source, prepare_software_queued_font_argb4444_pool,
        },
        software_queued_font_flush::{
            SoftwareQueuedFontFlushCommand, SoftwareQueuedFontFlushPlan,
            preflight_software_queued_font_flush_plan,
        },
        software_queued_font_glyphs::{LaneNeutralQueuedFontGlyph, SoftwareQueuedFontTerminal},
        software_queued_font_quad::SoftwareQueuedFontQuad,
        software_queued_font_queue::{
            SoftwareQueuedFontByte3Policy, SoftwareQueuedFontLane, SoftwareQueuedFontLaneQueues,
        },
    };

    #[test]
    fn actual_terminal_font_wrapper_places_car_and_empty_dispatch_inside_scene() {
        for car in [Some(7_u8), None] {
            let pool = pool();
            let plan = plan(&pool);
            let events = Rc::new(RefCell::new(Vec::new()));
            let mut backend = Backend::new(Rc::clone(&events), None);
            let mut sink = Sink(Rc::clone(&events));

            let (scene, outcome) =
                execute_scene_with_font_dispatch(Some(plan), &mut sink, |dispatch| {
                    execute_frontend_c3d_scene_with_post_reset(
                        &mut backend,
                        1,
                        EXTENT,
                        C3dDepthClear::Skip,
                        car,
                        |backend, marker| {
                            backend.event(format!("car-{marker}"));
                            Ok::<_, SubmitError>(())
                        },
                        |_| dispatch(),
                    )
                });

            assert!(scene.is_ok());
            assert_eq!(outcome.submitted, 1);
            let mut expected = vec!["preflight", "view", "viewport", "begin"];
            if car.is_some() {
                expected.push("car-7");
            }
            expected.extend(["reset", "font-30", "tail", "end"]);
            assert_eq!(*events.borrow(), expected);
        }
    }

    #[test]
    fn actual_terminal_font_wrapper_obeys_scene_failure_boundaries() {
        for (failure, dispatched) in [
            (Failure::Submit, false),
            (Failure::Backend(C3dSceneStage::ResetWorld), false),
            (Failure::Backend(C3dSceneStage::Tail), true),
            (Failure::Backend(C3dSceneStage::End), true),
        ] {
            let pool = pool();
            let plan = plan(&pool);
            let events = Rc::new(RefCell::new(Vec::new()));
            let backend_failure = match failure {
                Failure::Submit => None,
                Failure::Backend(stage) => Some(stage),
            };
            let mut backend = Backend::new(Rc::clone(&events), backend_failure);
            let mut sink = Sink(Rc::clone(&events));

            let (scene, outcome) =
                execute_scene_with_font_dispatch(Some(plan), &mut sink, |dispatch| {
                    execute_frontend_c3d_scene_with_post_reset(
                        &mut backend,
                        1,
                        EXTENT,
                        C3dDepthClear::Skip,
                        Some(7_u8),
                        |backend, marker| {
                            backend.event(format!("car-{marker}"));
                            if failure == Failure::Submit {
                                Err(SubmitError)
                            } else {
                                Ok(())
                            }
                        },
                        |_| dispatch(),
                    )
                });

            assert!(scene.is_err());
            assert_eq!(outcome.submitted, usize::from(dispatched));
            assert_eq!(
                events.borrow().iter().any(|event| event == "font-30"),
                dispatched,
                "{failure:?}"
            );
            let expected = match failure {
                Failure::Submit => {
                    vec!["preflight", "view", "viewport", "begin", "car-7", "end"]
                }
                Failure::Backend(C3dSceneStage::ResetWorld) => vec![
                    "preflight",
                    "view",
                    "viewport",
                    "begin",
                    "car-7",
                    "reset",
                    "end",
                ],
                Failure::Backend(C3dSceneStage::Tail) => vec![
                    "preflight",
                    "view",
                    "viewport",
                    "begin",
                    "car-7",
                    "reset",
                    "font-30",
                    "tail",
                    "end",
                ],
                Failure::Backend(C3dSceneStage::End) => vec![
                    "preflight",
                    "view",
                    "viewport",
                    "begin",
                    "car-7",
                    "reset",
                    "font-30",
                    "tail",
                    "end",
                ],
                Failure::Backend(stage) => panic!("uncovered failure stage {stage:?}"),
            };
            assert_eq!(*events.borrow(), expected, "{failure:?}");
        }
    }

    const EXTENT: DisplayExtent = DisplayExtent {
        width: 640,
        height: 480,
    };

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Failure {
        Submit,
        Backend(C3dSceneStage),
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct BackendError(C3dSceneStage);

    impl std::fmt::Display for BackendError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{:?}", self.0)
        }
    }
    impl std::error::Error for BackendError {}

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct SubmitError;

    impl std::fmt::Display for SubmitError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("submit")
        }
    }
    impl std::error::Error for SubmitError {}

    struct Backend {
        events: Rc<RefCell<Vec<String>>>,
        failure: Option<C3dSceneStage>,
    }

    impl Backend {
        fn new(events: Rc<RefCell<Vec<String>>>, failure: Option<C3dSceneStage>) -> Self {
            Self { events, failure }
        }
        fn event(&mut self, event: impl Into<String>) {
            self.events.borrow_mut().push(event.into());
        }
        fn effect(
            &mut self,
            stage: C3dSceneStage,
            event: &'static str,
        ) -> Result<(), BackendError> {
            self.event(event);
            (self.failure != Some(stage))
                .then_some(())
                .ok_or(BackendError(stage))
        }
    }

    impl FrontendC3dSceneBackend for Backend {
        type Surface = u8;
        type SceneError = BackendError;
        fn preflight_c3d_scene(
            &mut self,
            _: u8,
            _: DisplayExtent,
            _: C3dDepthClear,
        ) -> Result<(), BackendError> {
            self.effect(C3dSceneStage::Preflight, "preflight")
        }
        fn clear_c3d_depth(&mut self, _: u8, _: DisplayExtent) -> Result<(), BackendError> {
            self.effect(C3dSceneStage::ClearDepth, "clear")
        }
        fn set_c3d_view(&mut self, _: C3dMatrixWords) -> Result<(), BackendError> {
            self.effect(C3dSceneStage::View, "view")
        }
        fn set_c3d_viewport(&mut self, _: C3dViewportWords) -> Result<(), BackendError> {
            self.effect(C3dSceneStage::Viewport, "viewport")
        }
        fn begin_c3d_scene(&mut self, _: u8) -> Result<(), BackendError> {
            self.effect(C3dSceneStage::Begin, "begin")
        }
        fn reset_c3d_world(&mut self) -> Result<(), BackendError> {
            self.effect(C3dSceneStage::ResetWorld, "reset")
        }
        fn finish_c3d_scene_tail(&mut self) -> Result<C3dBindingState, BackendError> {
            self.effect(C3dSceneStage::Tail, "tail")?;
            Ok(C3dBindingState::Preserved)
        }
        fn end_c3d_scene(&mut self) -> Result<(), BackendError> {
            self.effect(C3dSceneStage::End, "end")
        }
    }

    struct Sink(Rc<RefCell<Vec<String>>>);
    impl SoftwareQueuedFontFlushSink for Sink {
        fn submit_textured_quad(
            &mut self,
            command: SoftwareQueuedFontFlushCommand,
            _: &SoftwareQueuedFontArgb4444Source,
            _: SoftwareQueuedFontQuad,
        ) {
            self.0
                .borrow_mut()
                .push(format!("font-{}", command.record.producer.code));
        }
        fn submit_fallback_blitter(
            &mut self,
            command: SoftwareQueuedFontFlushCommand,
            _: &SoftwareQueuedFontArgb4444Source,
        ) {
            self.0
                .borrow_mut()
                .push(format!("fallback-{}", command.record.producer.code));
        }
    }

    fn plan(
        pool: &crate::software_queued_font_argb4444::SoftwareQueuedFontArgb4444Pool,
    ) -> PreflightedSoftwareQueuedFontFlushPlan<'_> {
        let mut queues = SoftwareQueuedFontLaneQueues::new();
        let glyph = LaneNeutralQueuedFontGlyph {
            command_index: 0,
            blit_index: 0,
            code: 30,
            glyph_index: 0,
            style: FontStyle::Title,
            source: RectI16 {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
            destination: RectI32 {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
            caller_tuple: [1, 2, 3, 4],
            terminal: SoftwareQueuedFontTerminal::TexturedQuad,
        };
        assert!(matches!(
            queues.push(
                SoftwareQueuedFontLane::Three,
                SoftwareQueuedFontByte3Policy::PreserveCaller,
                glyph,
            ),
            crate::software_queued_font_queue::SoftwareQueuedFontPushOutcome::Accepted { .. }
        ));
        preflight_software_queued_font_flush_plan(
            SoftwareQueuedFontFlushPlan::from_queues(&queues, pool),
            0,
        )
        .unwrap()
    }

    fn pool() -> crate::software_queued_font_argb4444::SoftwareQueuedFontArgb4444Pool {
        const PIXEL_OFFSET: usize = 0x436;
        let trailer = PIXEL_OFFSET + 4;
        let mut bytes = vec![0; trailer + 24 + 16];
        bytes[0..2].copy_from_slice(b"BM");
        bytes[18..22].copy_from_slice(&1i32.to_le_bytes());
        bytes[22..26].copy_from_slice(&1i32.to_le_bytes());
        bytes[28..30].copy_from_slice(&8u16.to_le_bytes());
        for index in 0..256usize {
            let offset = 54 + index * 4;
            bytes[offset..offset + 4].copy_from_slice(&[index as u8, index as u8, index as u8, 0]);
        }
        bytes[PIXEL_OFFSET] = 1;
        bytes[trailer + 1] = 1;
        bytes[trailer + 4..trailer + 8].copy_from_slice(&1u32.to_le_bytes());
        bytes[trailer + 8..trailer + 12].copy_from_slice(&1u32.to_le_bytes());
        let glyph = trailer + 24;
        bytes[glyph] = b'A';
        bytes[glyph + 6..glyph + 8].copy_from_slice(&1i16.to_le_bytes());
        bytes[glyph + 8..glyph + 10].copy_from_slice(&1i16.to_le_bytes());
        let fonts = FontSet::from_ordered_fonts(std::array::from_fn(|_| {
            parse_legacy_font(&bytes, FontLimits::default()).unwrap()
        }));
        prepare_software_queued_font_argb4444_pool(fonts).unwrap()
    }
}
