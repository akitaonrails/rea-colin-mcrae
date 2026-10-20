//! Terminal owner for one recovered championship-setup frame.
//!
//! All software commands, source handles, and the optional car draw catalog
//! are preflighted before the first destination effect. Deferred commands are
//! then observed inline in visual-command order. The common frontend scene is
//! always run, including `Next` previews with no car submission.

use std::fmt;

use cmr_formats::{font::TextLayoutLimits, riff_palette::PaletteEntry};

use crate::{
    c3d_topology::C3dObjectTopology,
    championship_car_execute::{
        ChampionshipCarBatchBackend, ChampionshipCarBatchExecutor,
        ChampionshipCarBatchPreflightError,
    },
    championship_car_frame_plan::{ChampionshipCarFramePlanError, plan_championship_car_frame},
    championship_car_scene::{
        ChampionshipCarSceneError, ChampionshipCarSceneOutcome,
        execute_optional_championship_car_scene, preflight_optional_championship_car_scene,
    },
    championship_car_transform::{ChampionshipCarInput, ChampionshipCarState},
    championship_car_visibility::ChampionshipCarVisibilityLatches,
    championship_setup::CanonicalCar,
    championship_setup_frame::{
        ChampionshipSetupFrameExecutionError, ChampionshipSetupFrameOutcome,
        ChampionshipSetupFramePrepareError, ChampionshipSetupFrameResources,
        ChampionshipSetupFrameState, DeferredChampionshipCommand,
        PreparedChampionshipSetupFrameExecutionError, execute_prepared_championship_setup_frame,
        prepare_championship_setup_frame, validate_championship_setup_frame_sources,
    },
    championship_setup_visual::{
        CAR_TRANSFORM_STEPS, ChampionshipPreviewCache, ChampionshipSetupVisualCommand,
        ChampionshipSetupVisualInput, PixelPoint,
    },
    deferred_palette::{
        DeferredPaletteAdapter, DeferredPaletteCommitError, DeferredPaletteCommitOutcome,
        DeferredPaletteState, PaletteGroupBases, commit_deferred_palette, resolve_palette_index,
    },
    display::IndexedVideoBackend,
    frontend_c3d_route::{FrontendC3dRoute, FrontendC3dRouteState},
    frontend_c3d_scene::{C3dDepthClear, C3dViewportError, FrontendC3dSceneBackend},
    root_menu_c3d_runtime::C3dResourceScaledObject,
    root_menu_c3d_transform::root_menu_c3d_viewport,
    root_menu_c3d_visibility::RootMenuC3dVisibilityLatches,
    root_menu_frame::RootFrameHooks,
    source_store::UploadedSourceStore,
};

pub struct ChampionshipCarFrameResources<'a> {
    pub car: CanonicalCar,
    pub topology: &'a C3dObjectTopology<'a>,
    pub runtime: &'a C3dResourceScaledObject,
    pub base_basis: [i32; 9],
}

pub struct ChampionshipTerminalPaletteResources<'a> {
    pub bits_per_pixel: u8,
    pub group_bases: PaletteGroupBases,
    pub arena: &'a [[PaletteEntry; 256]],
    pub state: &'a mut DeferredPaletteState,
    pub renderer_cache: &'a mut [PaletteEntry; 256],
    pub global_cache: &'a mut [PaletteEntry; 256],
}

pub struct ChampionshipTerminalFrameState<'a> {
    pub preview_cache: &'a mut ChampionshipPreviewCache,
    pub car_transform: &'a mut ChampionshipCarState,
    pub car_visibility: &'a mut ChampionshipCarVisibilityLatches,
    pub root_visibility: &'a mut RootMenuC3dVisibilityLatches,
    pub route: &'a FrontendC3dRouteState,
}

pub struct ChampionshipTerminalFrameResources<'a, 'car> {
    pub setup: ChampionshipSetupFrameResources<'a>,
    /// Required exactly when the visual transcript contains a car preview.
    pub car: Option<ChampionshipCarFrameResources<'car>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChampionshipCarContractError {
    pub command_index: usize,
    pub kind: ChampionshipCarContractErrorKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChampionshipCarContractErrorKind {
    MissingResources {
        car: CanonicalCar,
    },
    UnexpectedResources {
        car: CanonicalCar,
    },
    ResourceCarMismatch {
        command: CanonicalCar,
        resource: CanonicalCar,
    },
    TransformShape,
    DuplicateTransform,
    LatchWithoutTransform,
    LatchCarMismatch {
        transform: CanonicalCar,
        latch: CanonicalCar,
    },
    LatchPosition {
        transform_index: usize,
    },
    DuplicateLatch,
    MissingLatch,
}

impl fmt::Display for ChampionshipCarContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "championship car contract at command {}: {:?}",
            self.command_index, self.kind
        )
    }
}
impl std::error::Error for ChampionshipCarContractError {}

#[derive(Debug, Eq, PartialEq)]
pub enum ChampionshipTerminalFrameError<E, S, P> {
    Viewport(C3dViewportError),
    SetupPrepare(ChampionshipSetupFramePrepareError),
    SetupSource(ChampionshipSetupFrameExecutionError<E>),
    SetupExecution(ChampionshipSetupFrameExecutionError<E>),
    CarContract(ChampionshipCarContractError),
    CarPlan {
        command_index: usize,
        source: ChampionshipCarFramePlanError,
    },
    CarCatalog {
        command_index: usize,
        source: ChampionshipCarBatchPreflightError,
    },
    Scene {
        terminal_index: usize,
        source: ChampionshipCarSceneError<S>,
    },
    Palette {
        terminal_index: usize,
        source: DeferredPaletteCommitError<E>,
    },
    Presentation {
        terminal_index: usize,
        source: P,
    },
}

impl<E: fmt::Display, S: fmt::Display, P: fmt::Display> fmt::Display
    for ChampionshipTerminalFrameError<E, S, P>
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Viewport(e) => write!(f, "championship terminal viewport preflight failed: {e}"),
            Self::SetupPrepare(e) => write!(f, "championship setup preflight failed: {e}"),
            Self::SetupSource(e) => write!(f, "championship setup source preflight failed: {e}"),
            Self::SetupExecution(e) => write!(f, "championship setup execution failed: {e}"),
            Self::CarContract(e) => e.fmt(f),
            Self::CarPlan {
                command_index,
                source,
            } => write!(
                f,
                "championship car plan at command {command_index} failed: {source}"
            ),
            Self::CarCatalog {
                command_index,
                source,
            } => write!(
                f,
                "championship car catalog at command {command_index} failed: {source}"
            ),
            Self::Scene {
                terminal_index,
                source,
            } => write!(
                f,
                "championship scene at terminal {terminal_index} failed: {source}"
            ),
            Self::Palette {
                terminal_index,
                source,
            } => write!(
                f,
                "championship palette at terminal {terminal_index} failed: {source}"
            ),
            Self::Presentation {
                terminal_index,
                source,
            } => write!(
                f,
                "championship presentation at terminal {terminal_index} failed: {source}"
            ),
        }
    }
}

impl<E, S, P> std::error::Error for ChampionshipTerminalFrameError<E, S, P>
where
    E: std::error::Error + 'static,
    S: std::error::Error + 'static,
    P: std::error::Error + 'static,
{
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Viewport(source) => Some(source),
            Self::SetupPrepare(source) => Some(source),
            Self::SetupSource(source) | Self::SetupExecution(source) => Some(source),
            Self::CarContract(source) => Some(source),
            Self::CarPlan { source, .. } => Some(source),
            Self::CarCatalog { source, .. } => Some(source),
            Self::Scene { source, .. } => Some(source),
            Self::Palette { source, .. } => Some(source),
            Self::Presentation { source, .. } => Some(source),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct ChampionshipTerminalFrameOutcome<E> {
    pub setup: ChampionshipSetupFrameOutcome<E>,
    pub scene: ChampionshipCarSceneOutcome,
    pub palette_commit: DeferredPaletteCommitOutcome,
    pub presentation_completed: bool,
}

pub type ChampionshipTerminalFrameResult<E, S, P> =
    Result<ChampionshipTerminalFrameOutcome<E>, ChampionshipTerminalFrameError<E, S, P>>;

#[derive(Debug)]
struct CarDeferredContract {
    command_index: usize,
    car: CanonicalCar,
    logo_origin: PixelPoint,
}

#[allow(clippy::too_many_arguments)]
pub fn execute_championship_terminal_frame<'catalog, B, H>(
    input: ChampionshipSetupVisualInput<'_>,
    setup_state: ChampionshipSetupFrameState,
    car_input: ChampionshipCarInput,
    resources: ChampionshipTerminalFrameResources<'_, '_>,
    state: ChampionshipTerminalFrameState<'_>,
    palette: ChampionshipTerminalPaletteResources<'_>,
    executor: &mut ChampionshipCarBatchExecutor<'catalog, H>,
    sources: &mut UploadedSourceStore<'_, B>,
    destination: <B as IndexedVideoBackend>::Surface,
    text_limits: TextLayoutLimits,
    depth_clear: C3dDepthClear,
) -> ChampionshipTerminalFrameResult<
    <B as IndexedVideoBackend>::Error,
    <B as FrontendC3dSceneBackend>::SceneError,
    B::FinalError,
>
where
    B: RootFrameHooks
        + DeferredPaletteAdapter<Error = <B as IndexedVideoBackend>::Error>
        + FrontendC3dSceneBackend<Surface = <B as IndexedVideoBackend>::Surface>
        + ChampionshipCarBatchBackend<H, Error = <B as FrontendC3dSceneBackend>::SceneError>,
{
    root_menu_c3d_viewport(input.viewport).map_err(ChampionshipTerminalFrameError::Viewport)?;
    let prepared_setup = prepare_championship_setup_frame(
        input,
        setup_state,
        resources.setup,
        *state.preview_cache,
        text_limits,
    )
    .map_err(ChampionshipTerminalFrameError::SetupPrepare)?;
    validate_championship_setup_frame_sources(&prepared_setup, sources)
        .map_err(ChampionshipTerminalFrameError::SetupSource)?;

    let contract = scan_car_contract(&prepared_setup, resources.car.as_ref())
        .map_err(ChampionshipTerminalFrameError::CarContract)?;
    let mut next_car_state = *state.car_transform;
    let prepared_car = match (contract.as_ref(), resources.car.as_ref()) {
        (Some(contract), Some(car)) => {
            let plan = plan_championship_car_frame(
                car.topology,
                car.runtime,
                &mut next_car_state,
                car.car,
                car_input,
                car.base_basis,
                contract.logo_origin,
                input.viewport,
            )
            .map_err(|source| ChampionshipTerminalFrameError::CarPlan {
                command_index: contract.command_index,
                source,
            })?;
            Some(executor.prepare(plan).map_err(|source| {
                ChampionshipTerminalFrameError::CarCatalog {
                    command_index: contract.command_index,
                    source,
                }
            })?)
        }
        (None, None) => None,
        _ => unreachable!("contract scan reconciles car resources"),
    };

    let visible = prepared_car
        .as_ref()
        .is_some_and(|prepared| prepared.selected_parent_visible());
    let selected_car = contract.as_ref().map(|c| c.car);
    let terminal_index = prepared_setup.command_len();
    preflight_optional_championship_car_scene(
        executor,
        sources.backend_mut(),
        prepared_car.as_ref(),
    )
    .map_err(|source| ChampionshipTerminalFrameError::Scene {
        terminal_index,
        source,
    })?;
    let mut transform_committed = false;
    let setup_result = execute_prepared_championship_setup_frame(
        prepared_setup,
        state.preview_cache,
        sources,
        destination,
        |_, command| {
            match command {
                DeferredChampionshipCommand::CarTransform(_) => {
                    *state.car_transform = next_car_state;
                    transform_committed = true;
                }
                DeferredChampionshipCommand::CarVisibilityLatch(_) => {
                    debug_assert!(transform_committed);
                    if visible {
                        state
                            .car_visibility
                            .show_all_layers(selected_car.expect("car contract"));
                    }
                }
                DeferredChampionshipCommand::PaletteRequest { group, item } => {
                    palette.state.request(resolve_palette_index(
                        i32::from(item),
                        i32::from(group),
                        palette.group_bases,
                    ));
                }
                DeferredChampionshipCommand::Selection(_)
                | DeferredChampionshipCommand::InfoText(_)
                | DeferredChampionshipCommand::NextPreview(_) => {}
            }
            Ok::<_, std::convert::Infallible>(())
        },
    );
    let setup = match setup_result {
        Ok(outcome) => outcome,
        Err(error) => {
            // A later bitmap can fail after the inline visibility callback.
            // No scene will run in that case, so this owner still consumes all
            // one-frame latches before returning.
            state.car_visibility.clear_all();
            state.root_visibility.clear_all();
            return Err(match error {
                PreparedChampionshipSetupFrameExecutionError::Frame(source) => {
                    ChampionshipTerminalFrameError::SetupExecution(source)
                }
                PreparedChampionshipSetupFrameExecutionError::Deferred { source, .. } => {
                    match source {}
                }
            });
        }
    };

    if state.route.enter(FrontendC3dRoute::ChampionshipCar) {
        executor.invalidate_bindings();
    }
    // Validate again at the immediate scene boundary because setup operations
    // are allowed to mutate an arbitrary backend after the early transaction
    // preflight above.
    let scene_result = execute_optional_championship_car_scene(
        executor,
        sources.backend_mut(),
        destination,
        input.viewport,
        depth_clear,
        prepared_car,
    );
    state.car_visibility.clear_all();
    state.root_visibility.clear_all();
    let scene = scene_result.map_err(|source| ChampionshipTerminalFrameError::Scene {
        terminal_index,
        source,
    })?;

    let backend = sources.backend_mut();
    let palette_commit = commit_deferred_palette(
        palette.state,
        palette.bits_per_pixel,
        palette.arena,
        palette.renderer_cache,
        palette.global_cache,
        backend,
    )
    .map_err(|source| ChampionshipTerminalFrameError::Palette {
        terminal_index,
        source,
    })?;
    backend
        .present_root_frame(destination, input.viewport)
        .map_err(|source| ChampionshipTerminalFrameError::Presentation {
            terminal_index,
            source,
        })?;
    Ok(ChampionshipTerminalFrameOutcome {
        setup,
        scene,
        palette_commit,
        presentation_completed: true,
    })
}

fn scan_car_contract(
    prepared: &crate::championship_setup_frame::PreparedChampionshipSetupFrame,
    resources: Option<&ChampionshipCarFrameResources<'_>>,
) -> Result<Option<CarDeferredContract>, ChampionshipCarContractError> {
    scan_deferred_car_contract(
        prepared.deferred_commands(),
        resources.map(|resources| resources.car),
    )
}

fn scan_deferred_car_contract(
    commands: impl IntoIterator<Item = (usize, DeferredChampionshipCommand)>,
    resource_car: Option<CanonicalCar>,
) -> Result<Option<CarDeferredContract>, ChampionshipCarContractError> {
    let mut transform: Option<(usize, CanonicalCar)> = None;
    let mut contract = None;
    for (command_index, command) in commands {
        match command {
            DeferredChampionshipCommand::CarTransform(
                ChampionshipSetupVisualCommand::DeferredCarTransform {
                    car,
                    angle_delta,
                    steps,
                },
            ) => {
                if angle_delta != crate::championship_setup_visual::CAR_ANGLE_DELTA
                    || steps != CAR_TRANSFORM_STEPS
                {
                    return Err(contract_error(
                        command_index,
                        ChampionshipCarContractErrorKind::TransformShape,
                    ));
                }
                if transform.replace((command_index, car)).is_some() {
                    return Err(contract_error(
                        command_index,
                        ChampionshipCarContractErrorKind::DuplicateTransform,
                    ));
                }
            }
            DeferredChampionshipCommand::CarVisibilityLatch(
                ChampionshipSetupVisualCommand::DeferredCarVisibilityLatch {
                    car,
                    tested_rectangle,
                },
            ) => {
                let Some((transform_index, transform_car)) = transform else {
                    return Err(contract_error(
                        command_index,
                        ChampionshipCarContractErrorKind::LatchWithoutTransform,
                    ));
                };
                if transform_car != car {
                    return Err(contract_error(
                        command_index,
                        ChampionshipCarContractErrorKind::LatchCarMismatch {
                            transform: transform_car,
                            latch: car,
                        },
                    ));
                }
                if command_index != transform_index + 1 {
                    return Err(contract_error(
                        command_index,
                        ChampionshipCarContractErrorKind::LatchPosition { transform_index },
                    ));
                }
                if contract.is_some() {
                    return Err(contract_error(
                        command_index,
                        ChampionshipCarContractErrorKind::DuplicateLatch,
                    ));
                }
                contract = Some(CarDeferredContract {
                    command_index: transform_index,
                    car,
                    logo_origin: PixelPoint::new(tested_rectangle.x, tested_rectangle.y),
                });
            }
            DeferredChampionshipCommand::CarTransform(_) => {
                return Err(contract_error(
                    command_index,
                    ChampionshipCarContractErrorKind::TransformShape,
                ));
            }
            DeferredChampionshipCommand::CarVisibilityLatch(_) => {
                return Err(contract_error(
                    command_index,
                    ChampionshipCarContractErrorKind::TransformShape,
                ));
            }
            _ => {}
        }
    }
    if let Some((command_index, _)) = transform
        && contract.is_none()
    {
        return Err(contract_error(
            command_index,
            ChampionshipCarContractErrorKind::MissingLatch,
        ));
    }
    match (&contract, resource_car) {
        (Some(contract), None) => Err(contract_error(
            contract.command_index,
            ChampionshipCarContractErrorKind::MissingResources { car: contract.car },
        )),
        (None, Some(resource_car)) => Err(contract_error(
            0,
            ChampionshipCarContractErrorKind::UnexpectedResources { car: resource_car },
        )),
        (Some(contract), Some(resource_car)) if contract.car != resource_car => {
            Err(contract_error(
                contract.command_index,
                ChampionshipCarContractErrorKind::ResourceCarMismatch {
                    command: contract.car,
                    resource: resource_car,
                },
            ))
        }
        _ => Ok(contract),
    }
}

fn contract_error(
    command_index: usize,
    kind: ChampionshipCarContractErrorKind,
) -> ChampionshipCarContractError {
    ChampionshipCarContractError {
        command_index,
        kind,
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use super::*;
    use crate::{
        blit::BlitPlan,
        championship_bitmap_raster::UploadedChampionshipLogoPool,
        championship_car_frame_plan::tests::{
            terminal_scene_test_fixture, terminal_scene_test_fixture_with_selectors,
        },
        championship_frame_assets::UploadedChampionshipFramePool,
        championship_setup::{
            ActivePreview, CAR_ROW_TAG, ChampionshipSetupRenderTranscript, GEAR_ROW_TAG,
            GearChoice, NEXT_ROW_TAG, RenderRow, RowValue, SetupRowText,
        },
        championship_setup_visual::{
            CAR_ANGLE_DELTA, NextPreviewContext, PixelRect, Q16Point, RowPresentation,
        },
        display::{CooperativeMode, DisplayExtent, DisplaySurfaces, IndexedMode, SurfaceTopology},
        font_assets::FontSet,
        font_raster::UploadedFontPool,
        frontend_c3d_scene::{C3dBindingState, C3dMatrixWords, C3dViewportWords},
        root_menu_c3d_execute::bind_prepared_c3d_resources,
        surface::IndexedSurface,
    };
    use cmr_formats::{
        font::{FontLimits, parse_legacy_font},
        riff_palette::PaletteEntry,
    };

    #[derive(Clone, Debug, Eq, PartialEq)]
    enum Event {
        Blit,
        Clear,
        View,
        Viewport,
        Begin,
        World(usize),
        Resource,
        Draw,
        Default,
        Reset,
        Tail,
        End,
        Palette,
        Present,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Error {
        Blit,
        Profile,
        Scene,
        Palette,
        Present,
    }
    impl fmt::Display for Error {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{self:?}")
        }
    }
    impl std::error::Error for Error {}

    struct Backend {
        events: Rc<RefCell<Vec<Event>>>,
        next: u16,
        blit_attempts: usize,
        fail_blit_at: Option<usize>,
        invalidate_profile_on_blit: bool,
        preflight_attempts: usize,
        fail_preflight: bool,
        fail_scene: bool,
        fail_palette: bool,
        fail_present: bool,
    }
    impl Backend {
        fn new(events: Rc<RefCell<Vec<Event>>>) -> Self {
            Self {
                events,
                next: 1,
                blit_attempts: 0,
                fail_blit_at: None,
                invalidate_profile_on_blit: false,
                preflight_attempts: 0,
                fail_preflight: false,
                fail_scene: false,
                fail_palette: false,
                fail_present: false,
            }
        }
    }
    impl IndexedVideoBackend for Backend {
        type Error = Error;
        type Surface = u16;
        type Palette = u16;
        fn create_device(&mut self) -> Result<(), Error> {
            unreachable!()
        }
        fn set_cooperative_mode(&mut self, _: CooperativeMode) -> Result<(), Error> {
            unreachable!()
        }
        fn set_display_mode(&mut self, _: IndexedMode) -> Result<(), Error> {
            unreachable!()
        }
        fn create_surfaces(
            &mut self,
            _: SurfaceTopology,
            _: IndexedMode,
        ) -> Result<DisplaySurfaces<u16>, Error> {
            unreachable!()
        }
        fn create_palette(&mut self, _: &[PaletteEntry; 256]) -> Result<u16, Error> {
            unreachable!()
        }
        fn attach_palette(&mut self, _: u16, _: u16) -> Result<(), Error> {
            unreachable!()
        }
        fn upload_palette(&mut self, _: u16, _: &[PaletteEntry; 256]) -> Result<(), Error> {
            unreachable!()
        }
        fn create_source(&mut self, _: &IndexedSurface) -> Result<u16, Error> {
            let v = self.next;
            self.next += 1;
            Ok(v)
        }
        fn blit(&mut self, _: u16, _: u16, _: BlitPlan) -> Result<(), Error> {
            self.blit_attempts += 1;
            self.events.borrow_mut().push(Event::Blit);
            if self.invalidate_profile_on_blit {
                self.fail_preflight = true;
            }
            if self.fail_blit_at == Some(self.blit_attempts) {
                Err(Error::Blit)
            } else {
                Ok(())
            }
        }
        fn destroy_source(&mut self, _: u16) {}
    }
    impl RootFrameHooks for Backend {
        type FinalError = Error;
        fn present_root_frame(&mut self, _: u16, _: DisplayExtent) -> Result<(), Error> {
            self.events.borrow_mut().push(Event::Present);
            if self.fail_present {
                Err(Error::Present)
            } else {
                Ok(())
            }
        }
    }
    impl DeferredPaletteAdapter for Backend {
        type Error = Error;
        fn upload_palette(&mut self, _: &[PaletteEntry; 256]) -> Result<(), Error> {
            self.events.borrow_mut().push(Event::Palette);
            if self.fail_palette {
                Err(Error::Palette)
            } else {
                Ok(())
            }
        }
    }
    impl FrontendC3dSceneBackend for Backend {
        type Surface = u16;
        type SceneError = Error;
        fn clear_c3d_depth(&mut self, _: u16, _: DisplayExtent) -> Result<(), Error> {
            self.events.borrow_mut().push(Event::Clear);
            Ok(())
        }
        fn set_c3d_view(&mut self, _: C3dMatrixWords) -> Result<(), Error> {
            self.events.borrow_mut().push(Event::View);
            Ok(())
        }
        fn set_c3d_viewport(&mut self, _: C3dViewportWords) -> Result<(), Error> {
            self.events.borrow_mut().push(Event::Viewport);
            Ok(())
        }
        fn begin_c3d_scene(&mut self, _: u16) -> Result<(), Error> {
            self.events.borrow_mut().push(Event::Begin);
            if self.fail_scene {
                Err(Error::Scene)
            } else {
                Ok(())
            }
        }
        fn reset_c3d_world(&mut self) -> Result<(), Error> {
            self.events.borrow_mut().push(Event::Reset);
            Ok(())
        }
        fn finish_c3d_scene_tail(&mut self) -> Result<C3dBindingState, Error> {
            self.events.borrow_mut().push(Event::Tail);
            Ok(C3dBindingState::Preserved)
        }
        fn end_c3d_scene(&mut self) -> Result<(), Error> {
            self.events.borrow_mut().push(Event::End);
            Ok(())
        }
    }
    impl ChampionshipCarBatchBackend<u8> for Backend {
        type Error = Error;
        fn preflight_championship_car_scene(&mut self) -> Result<(), Error> {
            self.preflight_attempts += 1;
            if self.fail_preflight {
                Err(Error::Profile)
            } else {
                Ok(())
            }
        }
        fn set_championship_car_actor_world(
            &mut self,
            index: usize,
            _: C3dMatrixWords,
        ) -> Result<(), Error> {
            self.events.borrow_mut().push(Event::World(index));
            Ok(())
        }
        fn bind_championship_car_resource(&mut self, _: &u8) -> Result<(), Error> {
            self.events.borrow_mut().push(Event::Resource);
            Ok(())
        }
        fn bind_championship_car_default(&mut self) -> Result<(), Error> {
            self.events.borrow_mut().push(Event::Default);
            Ok(())
        }
        fn draw_championship_car_batch(
            &mut self,
            _: &crate::c3d_batch::C3dDrawCommand,
        ) -> Result<(), Error> {
            self.events.borrow_mut().push(Event::Draw);
            Ok(())
        }
    }

    fn surface(width: u32, height: u32) -> IndexedSurface {
        IndexedSurface {
            width,
            height,
            pitch: width,
            pixels: vec![1; (width * height) as usize],
            palette: [PaletteEntry {
                red: 0,
                green: 0,
                blue: 0,
                flags: 0,
            }; 256],
        }
    }
    fn synthetic_font(identity: u8) -> Vec<u8> {
        const O: usize = 0x436;
        const H: usize = 24;
        let mut v = vec![0; O + 4 + H + 16];
        v[0..2].copy_from_slice(b"BM");
        v[18..22].copy_from_slice(&1i32.to_le_bytes());
        v[22..26].copy_from_slice(&1i32.to_le_bytes());
        v[28..30].copy_from_slice(&8u16.to_le_bytes());
        v[O] = identity + 1;
        let t = O + 4;
        v[t] = identity;
        v[t + 1] = 1;
        v[t + 4..t + 8].copy_from_slice(&1u32.to_le_bytes());
        v[t + 12..t + 16].copy_from_slice(&1u32.to_le_bytes());
        v[t + H] = b'A';
        v[t + H + 6..t + H + 8].copy_from_slice(&1i16.to_le_bytes());
        v[t + H + 8..t + H + 10].copy_from_slice(&1i16.to_le_bytes());
        v
    }
    fn fonts() -> FontSet {
        FontSet::from_ordered_fonts(std::array::from_fn(|i| {
            parse_legacy_font(&synthetic_font(i as u8), FontLimits::default()).unwrap()
        }))
    }
    fn transcript(selected: usize) -> ChampionshipSetupRenderTranscript {
        let car = CanonicalCar::Subaru;
        ChampionshipSetupRenderTranscript {
            title_text_index: 0x13,
            rows: [
                RenderRow {
                    tag: CAR_ROW_TAG,
                    text: SetupRowText::ChooseCar,
                    selected: selected == 0,
                    value: RowValue::Car(car),
                },
                RenderRow {
                    tag: GEAR_ROW_TAG,
                    text: SetupRowText::ChooseGear,
                    selected: selected == 1,
                    value: RowValue::Gear(GearChoice::BitClear),
                },
                RenderRow {
                    tag: NEXT_ROW_TAG,
                    text: SetupRowText::Localized(0x18),
                    selected: selected == 2,
                    value: RowValue::None,
                },
            ],
            highlight_row: selected,
            preview: if selected == 2 {
                ActivePreview::Next
            } else {
                ActivePreview::Car {
                    canonical_car: car,
                    object_name: car.object_name(),
                    logo_name: car.logo_name(),
                }
            },
        }
    }
    fn input(t: &ChampionshipSetupRenderTranscript) -> ChampionshipSetupVisualInput<'_> {
        ChampionshipSetupVisualInput {
            viewport: DisplayExtent {
                width: 640,
                height: 480,
            },
            frontend_offset_q16: Q16Point::new(-0x1_6666, 0x1_0000),
            row_anchors_q16: [
                Q16Point::new(0x1_a000, -0x8000),
                Q16Point::new(0x1_a000, -0x7000),
                Q16Point::new(0x1_a000, -0x6000),
            ],
            preview_anchor_q16: Q16Point::new(0x1_8000, -0xc000),
            rows: [RowPresentation {
                visible: true,
                enabled: true,
            }; 3],
            selected_text_widths: [100, 80, 60],
            transcript: t,
            next_context: NextPreviewContext {
                state_byte: 0,
                mode_byte: 0,
            },
        }
    }

    struct Harness<'a> {
        text: crate::localization::IndexedTextTable,
        fonts: UploadedFontPool,
        logos: UploadedChampionshipLogoPool,
        frames: UploadedChampionshipFramePool,
        selection: crate::source_store::UploadedIndexedBitmap,
        store: UploadedSourceStore<'a, Backend>,
    }
    impl Harness<'_> {
        fn new(backend: &mut Backend) -> Harness<'_> {
            let mut store = UploadedSourceStore::new(backend);
            let fonts = UploadedFontPool::upload(&mut store, fonts()).unwrap();
            let bmp = store.upload_bitmap(&surface(320, 256)).unwrap();
            Harness {
                text: crate::localization::IndexedTextTable::parse(&b"A\n".repeat(0xfe), 0xfe)
                    .unwrap(),
                fonts,
                logos: UploadedChampionshipLogoPool::test_repeated(bmp),
                frames: UploadedChampionshipFramePool::test_repeated(bmp),
                selection: bmp,
                store,
            }
        }
    }

    #[test]
    fn terminal_frame_orders_scene_palette_and_present_and_stabilizes_visibility() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut backend = Backend::new(Rc::clone(&events));
        let mut h = Harness::new(&mut backend);
        let fixture = terminal_scene_test_fixture();
        let catalog = bind_prepared_c3d_resources(
            fixture.arena.provenance(),
            fixture.arena.resources(),
            &mut |id, _| Ok::<_, std::convert::Infallible>(id.index() as u8),
        )
        .unwrap();
        let mut executor = ChampionshipCarBatchExecutor::new(&catalog);
        let mut preview = ChampionshipPreviewCache::new();
        let mut car_state = ChampionshipCarState::new();
        let mut car_latches = ChampionshipCarVisibilityLatches::new();
        let mut root_latches = RootMenuC3dVisibilityLatches::new();
        let route = FrontendC3dRouteState::new();
        let mut pal = DeferredPaletteState::new();
        let zero = PaletteEntry {
            red: 0,
            green: 0,
            blue: 0,
            flags: 0,
        };
        let mut palette_arena = [[zero; 256]; 10];
        palette_arena[0] = [PaletteEntry { red: 7, ..zero }; 256];
        palette_arena[1] = [PaletteEntry { red: 9, ..zero }; 256];
        let mut rcache = [zero; 256];
        let mut gcache = [zero; 256];
        let t = transcript(0);
        let run = |h: &mut Harness<'_>,
                   executor: &mut ChampionshipCarBatchExecutor<'_, u8>,
                   preview: &mut ChampionshipPreviewCache,
                   car_state: &mut ChampionshipCarState,
                   car_latches: &mut ChampionshipCarVisibilityLatches,
                   root_latches: &mut RootMenuC3dVisibilityLatches,
                   pal: &mut DeferredPaletteState,
                   rcache: &mut [PaletteEntry; 256],
                   gcache: &mut [PaletteEntry; 256]| {
            execute_championship_terminal_frame(
                input(&t),
                ChampionshipSetupFrameState::default(),
                ChampionshipCarInput {
                    f9: false,
                    f10: false,
                },
                ChampionshipTerminalFrameResources {
                    setup: ChampionshipSetupFrameResources {
                        text: &h.text,
                        fonts: &h.fonts,
                        selection: Some(h.selection),
                        logos: &h.logos,
                        frames: &h.frames,
                    },
                    car: Some(ChampionshipCarFrameResources {
                        car: CanonicalCar::Subaru,
                        topology: &fixture.topology,
                        runtime: &fixture.runtime,
                        base_basis: [0x10000, 0, 0, 0, 0x10000, 0, 0, 0, 0x10000],
                    }),
                },
                ChampionshipTerminalFrameState {
                    preview_cache: preview,
                    car_transform: car_state,
                    car_visibility: car_latches,
                    root_visibility: root_latches,
                    route: &route,
                },
                ChampionshipTerminalPaletteResources {
                    bits_per_pixel: 8,
                    group_bases: PaletteGroupBases {
                        group_1: 0,
                        group_2: 0,
                        group_3: 0,
                        group_4: 0,
                        group_5: 0,
                    },
                    arena: &palette_arena,
                    state: pal,
                    renderer_cache: rcache,
                    global_cache: gcache,
                },
                executor,
                &mut h.store,
                999,
                TextLayoutLimits::default(),
                C3dDepthClear::Clear,
            )
        };
        let first = run(
            &mut h,
            &mut executor,
            &mut preview,
            &mut car_state,
            &mut car_latches,
            &mut root_latches,
            &mut pal,
            &mut rcache,
            &mut gcache,
        )
        .unwrap();
        assert_eq!(first.scene.submission.unwrap().draws, 0);
        let first_events = events.borrow().clone();
        let first_end = first_events.iter().rposition(|x| *x == Event::End).unwrap();
        let first_palette = first_events
            .iter()
            .position(|x| *x == Event::Palette)
            .unwrap();
        let first_present = first_events
            .iter()
            .position(|x| *x == Event::Present)
            .unwrap();
        assert!(first_end < first_palette && first_palette < first_present);
        events.borrow_mut().clear();
        let second = run(
            &mut h,
            &mut executor,
            &mut preview,
            &mut car_state,
            &mut car_latches,
            &mut root_latches,
            &mut pal,
            &mut rcache,
            &mut gcache,
        )
        .unwrap();
        assert_eq!(second.scene.submission.unwrap().draws, 2);
        let e = events.borrow();
        let begin = e.iter().position(|x| *x == Event::Begin).unwrap();
        let present = e.iter().position(|x| *x == Event::Present).unwrap();
        assert!(begin < e.iter().position(|x| *x == Event::World(0)).unwrap());
        assert!(
            e.iter().position(|x| *x == Event::Resource).unwrap()
                < e.iter().position(|x| *x == Event::Default).unwrap()
        );
        assert!(e.iter().rposition(|x| *x == Event::End).unwrap() < present);
        assert!(!e.contains(&Event::Palette));
        assert!(
            CanonicalCar::ALL
                .into_iter()
                .all(|car| car_latches.mask(car).raw() == 0)
        );
        assert!(
            crate::root_menu_c3d_assets::RootMenuC3dSlot::ALL
                .into_iter()
                .all(|slot| root_latches.mask(slot).raw() == 0)
        );
        drop(e);

        // A generic backend may invalidate its own C3D profile while setup
        // pixels execute. The second guard must stop before depth or scene
        // effects even though the early transaction preflight succeeded.
        {
            let backend = h.store.backend_mut();
            backend.preflight_attempts = 0;
            backend.fail_preflight = false;
            backend.invalidate_profile_on_blit = true;
        }
        events.borrow_mut().clear();
        let stale_profile = run(
            &mut h,
            &mut executor,
            &mut preview,
            &mut car_state,
            &mut car_latches,
            &mut root_latches,
            &mut pal,
            &mut rcache,
            &mut gcache,
        )
        .unwrap_err();
        assert!(matches!(
            stale_profile,
            ChampionshipTerminalFrameError::Scene {
                source: ChampionshipCarSceneError::BackendPreflight {
                    source: Error::Profile,
                    ..
                },
                ..
            }
        ));
        assert_eq!(h.store.backend_mut().preflight_attempts, 2);
        let stale_events = events.borrow();
        assert!(stale_events.contains(&Event::Blit));
        assert!(!stale_events.contains(&Event::Clear));
        assert!(!stale_events.contains(&Event::Begin));
        drop(stale_events);
        {
            let backend = h.store.backend_mut();
            backend.invalidate_profile_on_blit = false;
            backend.fail_preflight = false;
        }

        // Moving to Next still submits the common empty scene so a prior car
        // cannot remain displayed.
        h.store.backend_mut().fail_blit_at = None;
        events.borrow_mut().clear();
        let next = transcript(2);
        let next_outcome = execute_championship_terminal_frame(
            input(&next),
            ChampionshipSetupFrameState::default(),
            ChampionshipCarInput {
                f9: false,
                f10: false,
            },
            ChampionshipTerminalFrameResources {
                setup: ChampionshipSetupFrameResources {
                    text: &h.text,
                    fonts: &h.fonts,
                    selection: Some(h.selection),
                    logos: &h.logos,
                    frames: &h.frames,
                },
                car: None,
            },
            ChampionshipTerminalFrameState {
                preview_cache: &mut preview,
                car_transform: &mut car_state,
                car_visibility: &mut car_latches,
                root_visibility: &mut root_latches,
                route: &route,
            },
            ChampionshipTerminalPaletteResources {
                bits_per_pixel: 8,
                group_bases: PaletteGroupBases {
                    group_1: 0,
                    group_2: 0,
                    group_3: 0,
                    group_4: 0,
                    group_5: 0,
                },
                arena: &palette_arena,
                state: &mut pal,
                renderer_cache: &mut rcache,
                global_cache: &mut gcache,
            },
            &mut executor,
            &mut h.store,
            999,
            TextLayoutLimits::default(),
            C3dDepthClear::Clear,
        )
        .unwrap();
        assert_eq!(next_outcome.scene.submission, None);
        let next_events = events.borrow();
        assert!(next_events.contains(&Event::Begin));
        assert!(next_events.contains(&Event::End));
        assert!(!next_events.iter().any(|event| matches!(
            event,
            Event::World(_) | Event::Draw | Event::Resource | Event::Default
        )));
        drop(next_events);

        // A scene failure consumes both latch banks and stops terminal work.
        root_latches.show_all_layers(crate::root_menu_c3d_assets::RootMenuC3dSlot::Championship);
        h.store.backend_mut().fail_scene = true;
        events.borrow_mut().clear();
        let scene_error = run(
            &mut h,
            &mut executor,
            &mut preview,
            &mut car_state,
            &mut car_latches,
            &mut root_latches,
            &mut pal,
            &mut rcache,
            &mut gcache,
        )
        .unwrap_err();
        assert!(matches!(
            scene_error,
            ChampionshipTerminalFrameError::Scene { .. }
        ));
        assert!(!events.borrow().contains(&Event::Palette));
        assert!(!events.borrow().contains(&Event::Present));
        assert!(
            CanonicalCar::ALL
                .into_iter()
                .all(|car| car_latches.mask(car).raw() == 0)
        );
        assert!(
            crate::root_menu_c3d_assets::RootMenuC3dSlot::ALL
                .into_iter()
                .all(|slot| root_latches.mask(slot).raw() == 0)
        );

        // Palette failure happens after latch consumption and suppresses presentation.
        h.store.backend_mut().fail_scene = false;
        h.store.backend_mut().fail_palette = true;
        pal.request(crate::deferred_palette::PaletteArenaIndex::new(1));
        events.borrow_mut().clear();
        let palette_error = run(
            &mut h,
            &mut executor,
            &mut preview,
            &mut car_state,
            &mut car_latches,
            &mut root_latches,
            &mut pal,
            &mut rcache,
            &mut gcache,
        )
        .unwrap_err();
        assert!(matches!(
            palette_error,
            ChampionshipTerminalFrameError::Palette { .. }
        ));
        assert!(events.borrow().contains(&Event::Palette));
        assert!(!events.borrow().contains(&Event::Present));
        assert_eq!(
            pal.pending(),
            Some(crate::deferred_palette::PaletteArenaIndex::new(1))
        );

        // Presentation failure leaves the already committed palette committed.
        h.store.backend_mut().fail_palette = false;
        h.store.backend_mut().fail_present = true;
        events.borrow_mut().clear();
        let present_error = run(
            &mut h,
            &mut executor,
            &mut preview,
            &mut car_state,
            &mut car_latches,
            &mut root_latches,
            &mut pal,
            &mut rcache,
            &mut gcache,
        )
        .unwrap_err();
        assert!(matches!(
            present_error,
            ChampionshipTerminalFrameError::Presentation { .. }
        ));
        assert_eq!(
            pal.current(),
            Some(crate::deferred_palette::PaletteArenaIndex::new(1))
        );
        assert_eq!(pal.pending(), None);
        assert_eq!(rcache[0].red, 9);
        assert_eq!(gcache[0].red, 9);

        // The logo bitmap follows the inline transform/latch callbacks. A
        // failure there still consumes both one-frame latch banks and never
        // enters the scene or terminal palette/presentation stages.
        preview = ChampionshipPreviewCache::new();
        car_state = ChampionshipCarState::new();
        pal = DeferredPaletteState::new();
        let pre_failure_car_state = car_state;
        let pre_failure_preview = preview;
        root_latches.show_all_layers(crate::root_menu_c3d_assets::RootMenuC3dSlot::Options);
        let backend = h.store.backend_mut();
        backend.fail_present = false;
        backend.blit_attempts = 0;
        backend.fail_blit_at = Some(6);
        events.borrow_mut().clear();
        let bitmap_error = run(
            &mut h,
            &mut executor,
            &mut preview,
            &mut car_state,
            &mut car_latches,
            &mut root_latches,
            &mut pal,
            &mut rcache,
            &mut gcache,
        )
        .unwrap_err();
        assert!(matches!(
            bitmap_error,
            ChampionshipTerminalFrameError::SetupExecution(_)
        ));
        assert_ne!(car_state, pre_failure_car_state);
        assert_eq!(preview, pre_failure_preview);
        assert_eq!(pal.pending(), None);
        let failed_events = events.borrow();
        assert!(!failed_events.contains(&Event::Begin));
        assert!(!failed_events.contains(&Event::Palette));
        assert!(!failed_events.contains(&Event::Present));
        assert!(
            CanonicalCar::ALL
                .into_iter()
                .all(|car| car_latches.mask(car).raw() == 0)
        );
        assert!(
            crate::root_menu_c3d_assets::RootMenuC3dSlot::ALL
                .into_iter()
                .all(|slot| root_latches.mask(slot).raw() == 0)
        );
    }

    #[test]
    fn invalid_next_viewport_is_effect_free_and_preserves_all_state() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut backend = Backend::new(Rc::clone(&events));
        let mut h = Harness::new(&mut backend);
        let fixture = terminal_scene_test_fixture();
        let catalog = bind_prepared_c3d_resources(
            fixture.arena.provenance(),
            fixture.arena.resources(),
            &mut |id, _| Ok::<_, std::convert::Infallible>(id.index() as u8),
        )
        .unwrap();
        let mut executor = ChampionshipCarBatchExecutor::new(&catalog);
        let mut preview = ChampionshipPreviewCache::new();
        let mut car_state = ChampionshipCarState::new();
        let mut car_latches = ChampionshipCarVisibilityLatches::new();
        car_latches.show_all_layers(CanonicalCar::Subaru);
        let mut root_latches = RootMenuC3dVisibilityLatches::new();
        let route = FrontendC3dRouteState::new();
        let mut pal = DeferredPaletteState::new();
        pal.request(crate::deferred_palette::PaletteArenaIndex::new(0));
        let before_preview = preview;
        let before_car = car_state;
        let before_car_latches = car_latches.clone();
        let before_root_latches = root_latches.clone();
        let before_palette = pal;
        let zero = PaletteEntry {
            red: 0,
            green: 0,
            blue: 0,
            flags: 0,
        };
        let palette_arena = [[zero; 256]];
        let mut rcache = [zero; 256];
        let mut gcache = [zero; 256];
        let t = transcript(2);
        let mut bad = input(&t);
        bad.viewport.width = 0;
        events.borrow_mut().clear();
        let result = execute_championship_terminal_frame(
            bad,
            ChampionshipSetupFrameState::default(),
            ChampionshipCarInput {
                f9: false,
                f10: false,
            },
            ChampionshipTerminalFrameResources {
                setup: ChampionshipSetupFrameResources {
                    text: &h.text,
                    fonts: &h.fonts,
                    selection: Some(h.selection),
                    logos: &h.logos,
                    frames: &h.frames,
                },
                car: None,
            },
            ChampionshipTerminalFrameState {
                preview_cache: &mut preview,
                car_transform: &mut car_state,
                car_visibility: &mut car_latches,
                root_visibility: &mut root_latches,
                route: &route,
            },
            ChampionshipTerminalPaletteResources {
                bits_per_pixel: 8,
                group_bases: PaletteGroupBases {
                    group_1: 0,
                    group_2: 0,
                    group_3: 0,
                    group_4: 0,
                    group_5: 0,
                },
                arena: &palette_arena,
                state: &mut pal,
                renderer_cache: &mut rcache,
                global_cache: &mut gcache,
            },
            &mut executor,
            &mut h.store,
            999,
            TextLayoutLimits::default(),
            C3dDepthClear::Clear,
        );
        assert!(matches!(
            result,
            Err(ChampionshipTerminalFrameError::Viewport(_))
        ));
        assert!(events.borrow().is_empty());
        assert_eq!(preview, before_preview);
        assert_eq!(car_state, before_car);
        assert_eq!(car_latches, before_car_latches);
        assert_eq!(root_latches, before_root_latches);
        assert_eq!(pal, before_palette);
    }

    #[test]
    fn backend_profile_rejection_precedes_terminal_pixels_and_state_changes() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut backend = Backend::new(Rc::clone(&events));
        let mut h = Harness::new(&mut backend);
        let fixture = terminal_scene_test_fixture();
        let catalog = bind_prepared_c3d_resources(
            fixture.arena.provenance(),
            fixture.arena.resources(),
            &mut |id, _| Ok::<_, std::convert::Infallible>(id.index() as u8),
        )
        .unwrap();
        let mut executor = ChampionshipCarBatchExecutor::new(&catalog);
        let mut preview = ChampionshipPreviewCache::new();
        let mut car_state = ChampionshipCarState::new();
        let mut car_latches = ChampionshipCarVisibilityLatches::new();
        car_latches.show_all_layers(CanonicalCar::Escort);
        let mut root_latches = RootMenuC3dVisibilityLatches::new();
        root_latches.show_all_layers(crate::root_menu_c3d_assets::RootMenuC3dSlot::Options);
        let route = FrontendC3dRouteState::new();
        let mut pal = DeferredPaletteState::new();
        pal.request(crate::deferred_palette::PaletteArenaIndex::new(0));
        let zero = PaletteEntry {
            red: 0,
            green: 0,
            blue: 0,
            flags: 0,
        };
        let palette_arena = [[zero; 256]];
        let mut rcache = [PaletteEntry { red: 3, ..zero }; 256];
        let mut gcache = [PaletteEntry { red: 5, ..zero }; 256];
        let before_preview = preview;
        let before_car = car_state;
        let before_car_latches = car_latches.clone();
        let before_root_latches = root_latches.clone();
        let before_palette = pal;
        let before_renderer = rcache;
        let before_global = gcache;
        h.store.backend_mut().fail_preflight = true;
        events.borrow_mut().clear();
        let t = transcript(0);

        let result = execute_championship_terminal_frame(
            input(&t),
            ChampionshipSetupFrameState::default(),
            ChampionshipCarInput {
                f9: false,
                f10: false,
            },
            ChampionshipTerminalFrameResources {
                setup: ChampionshipSetupFrameResources {
                    text: &h.text,
                    fonts: &h.fonts,
                    selection: Some(h.selection),
                    logos: &h.logos,
                    frames: &h.frames,
                },
                car: Some(ChampionshipCarFrameResources {
                    car: CanonicalCar::Subaru,
                    topology: &fixture.topology,
                    runtime: &fixture.runtime,
                    base_basis: [0x10000, 0, 0, 0, 0x10000, 0, 0, 0, 0x10000],
                }),
            },
            ChampionshipTerminalFrameState {
                preview_cache: &mut preview,
                car_transform: &mut car_state,
                car_visibility: &mut car_latches,
                root_visibility: &mut root_latches,
                route: &route,
            },
            ChampionshipTerminalPaletteResources {
                bits_per_pixel: 8,
                group_bases: PaletteGroupBases {
                    group_1: 0,
                    group_2: 0,
                    group_3: 0,
                    group_4: 0,
                    group_5: 0,
                },
                arena: &palette_arena,
                state: &mut pal,
                renderer_cache: &mut rcache,
                global_cache: &mut gcache,
            },
            &mut executor,
            &mut h.store,
            999,
            TextLayoutLimits::default(),
            C3dDepthClear::Clear,
        );

        assert!(matches!(
            result,
            Err(ChampionshipTerminalFrameError::Scene {
                source: ChampionshipCarSceneError::BackendPreflight {
                    source: Error::Profile,
                    ..
                },
                ..
            })
        ));
        assert!(events.borrow().is_empty());
        assert_eq!(preview, before_preview);
        assert_eq!(car_state, before_car);
        assert_eq!(car_latches, before_car_latches);
        assert_eq!(root_latches, before_root_latches);
        assert_eq!(route.active(), None);
        assert_eq!(pal, before_palette);
        assert_eq!(rcache, before_renderer);
        assert_eq!(gcache, before_global);
    }

    fn transform(car: CanonicalCar) -> DeferredChampionshipCommand {
        DeferredChampionshipCommand::CarTransform(
            ChampionshipSetupVisualCommand::DeferredCarTransform {
                car,
                angle_delta: CAR_ANGLE_DELTA,
                steps: CAR_TRANSFORM_STEPS,
            },
        )
    }

    fn latch(car: CanonicalCar) -> DeferredChampionshipCommand {
        DeferredChampionshipCommand::CarVisibilityLatch(
            ChampionshipSetupVisualCommand::DeferredCarVisibilityLatch {
                car,
                tested_rectangle: PixelRect::new(12, 34, 182, 128),
            },
        )
    }

    #[test]
    fn car_contract_requires_exact_transform_latch_adjacency_and_identity() {
        let contract = scan_deferred_car_contract(
            [
                (5, transform(CanonicalCar::Subaru)),
                (6, latch(CanonicalCar::Subaru)),
            ],
            Some(CanonicalCar::Subaru),
        )
        .unwrap()
        .unwrap();
        assert_eq!(contract.command_index, 5);
        assert_eq!(contract.car, CanonicalCar::Subaru);
        assert_eq!(contract.logo_origin, PixelPoint::new(12, 34));

        assert_eq!(
            scan_deferred_car_contract(
                [
                    (5, transform(CanonicalCar::Subaru)),
                    (7, latch(CanonicalCar::Subaru))
                ],
                Some(CanonicalCar::Subaru),
            )
            .unwrap_err(),
            ChampionshipCarContractError {
                command_index: 7,
                kind: ChampionshipCarContractErrorKind::LatchPosition { transform_index: 5 },
            }
        );
        assert!(matches!(
            scan_deferred_car_contract(
                [
                    (5, transform(CanonicalCar::Subaru)),
                    (6, latch(CanonicalCar::Ibiza))
                ],
                Some(CanonicalCar::Subaru),
            ),
            Err(ChampionshipCarContractError {
                kind: ChampionshipCarContractErrorKind::LatchCarMismatch { .. },
                ..
            })
        ));
    }

    #[test]
    fn car_contract_reconciles_preview_presence_with_supplied_resources() {
        assert!(scan_deferred_car_contract([], None).unwrap().is_none());
        assert!(matches!(
            scan_deferred_car_contract([], Some(CanonicalCar::Subaru)),
            Err(ChampionshipCarContractError {
                kind: ChampionshipCarContractErrorKind::UnexpectedResources { .. },
                ..
            })
        ));
        assert!(matches!(
            scan_deferred_car_contract(
                [
                    (2, transform(CanonicalCar::Subaru)),
                    (3, latch(CanonicalCar::Subaru))
                ],
                None,
            ),
            Err(ChampionshipCarContractError {
                command_index: 2,
                kind: ChampionshipCarContractErrorKind::MissingResources {
                    car: CanonicalCar::Subaru
                },
            })
        ));
    }

    #[test]
    fn terminal_route_switch_invalidates_an_otherwise_reused_selector_binding() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut backend = Backend::new(Rc::clone(&events));
        let mut h = Harness::new(&mut backend);
        let fixture = terminal_scene_test_fixture_with_selectors(&[0]);
        let catalog = bind_prepared_c3d_resources(
            fixture.arena.provenance(),
            fixture.arena.resources(),
            &mut |id, _| Ok::<_, std::convert::Infallible>(id.index() as u8),
        )
        .unwrap();
        let mut executor = ChampionshipCarBatchExecutor::new(&catalog);
        let mut preview = ChampionshipPreviewCache::new();
        let mut car_state = ChampionshipCarState::new();
        let mut car_latches = ChampionshipCarVisibilityLatches::new();
        let mut root_latches = RootMenuC3dVisibilityLatches::new();
        let route = FrontendC3dRouteState::new();
        let mut pal = DeferredPaletteState::new();
        let zero = PaletteEntry {
            red: 0,
            green: 0,
            blue: 0,
            flags: 0,
        };
        let palette_arena = [[zero; 256]];
        let mut rcache = [zero; 256];
        let mut gcache = [zero; 256];
        let t = transcript(0);
        let mut run = || {
            execute_championship_terminal_frame(
                input(&t),
                ChampionshipSetupFrameState::default(),
                ChampionshipCarInput {
                    f9: false,
                    f10: false,
                },
                ChampionshipTerminalFrameResources {
                    setup: ChampionshipSetupFrameResources {
                        text: &h.text,
                        fonts: &h.fonts,
                        selection: Some(h.selection),
                        logos: &h.logos,
                        frames: &h.frames,
                    },
                    car: Some(ChampionshipCarFrameResources {
                        car: CanonicalCar::Subaru,
                        topology: &fixture.topology,
                        runtime: &fixture.runtime,
                        base_basis: [0x10000, 0, 0, 0, 0x10000, 0, 0, 0, 0x10000],
                    }),
                },
                ChampionshipTerminalFrameState {
                    preview_cache: &mut preview,
                    car_transform: &mut car_state,
                    car_visibility: &mut car_latches,
                    root_visibility: &mut root_latches,
                    route: &route,
                },
                ChampionshipTerminalPaletteResources {
                    bits_per_pixel: 32,
                    group_bases: PaletteGroupBases {
                        group_1: 0,
                        group_2: 0,
                        group_3: 0,
                        group_4: 0,
                        group_5: 0,
                    },
                    arena: &palette_arena,
                    state: &mut pal,
                    renderer_cache: &mut rcache,
                    global_cache: &mut gcache,
                },
                &mut executor,
                &mut h.store,
                999,
                TextLayoutLimits::default(),
                C3dDepthClear::Clear,
            )
        };
        run().unwrap(); // recovered first frame is hidden
        events.borrow_mut().clear();
        run().unwrap();
        assert_eq!(
            events
                .borrow()
                .iter()
                .filter(|e| **e == Event::Resource)
                .count(),
            1
        );
        events.borrow_mut().clear();
        run().unwrap();
        assert_eq!(
            events
                .borrow()
                .iter()
                .filter(|e| **e == Event::Resource)
                .count(),
            0
        );
        assert!(route.enter(FrontendC3dRoute::RootMenu));
        events.borrow_mut().clear();
        run().unwrap();
        assert_eq!(
            events
                .borrow()
                .iter()
                .filter(|e| **e == Event::Resource)
                .count(),
            1
        );
    }
}
