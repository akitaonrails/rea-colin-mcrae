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
        execute_optional_championship_car_scene,
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
    software_championship_setup_frame::{
        SoftwareChampionshipSetupCommandOutcome, SoftwareChampionshipSetupDeferredCommand,
        SoftwareChampionshipSetupFrameError, SoftwareChampionshipSetupFrameOutcome,
        execute_preflighted_software_championship_setup_frame,
        preflight_software_championship_setup_frame,
    },
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
pub struct SoftwareChampionshipAlternateTerminalOutcome {
    pub setup: SoftwareChampionshipSetupFrameOutcome,
    pub scene: ChampionshipCarSceneOutcome,
    /// The alternate RGB16 terminal reaches the recovered palette boundary,
    /// where non-indexed depth skips upload and preserves any pending request.
    pub palette_commit: DeferredPaletteCommitOutcome,
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
/// Text, Selection stretching, info text, and next-preview work remain typed
/// deferred commands. Palette requests, car transforms, and visibility latches
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
    backend: &mut SoftwareSplitC3dOracleBackend,
    destination: SoftwareSplitC3dTargetHandle,
    depth_clear: C3dDepthClear,
) -> Result<SoftwareChampionshipAlternateTerminalOutcome, SoftwareChampionshipAlternateTerminalError>
{
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

    preflight_software_championship_setup_frame(&prepared_setup, sources, backend, destination)
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

    let scene = execute_optional_championship_car_scene(
        executor,
        backend,
        destination,
        extent,
        depth_clear,
        prepared_car,
    );
    state.car_visibility.clear_all();
    state.root_visibility.clear_all();
    let scene = match scene {
        Ok(scene) => scene,
        Err(source) => {
            return Err(SoftwareChampionshipAlternateTerminalError::Scene {
                terminal_index,
                source: Box::new(source),
            });
        }
    };

    Ok(SoftwareChampionshipAlternateTerminalOutcome {
        setup: SoftwareChampionshipSetupFrameOutcome {
            commands: setup_commands,
        },
        scene,
        palette_commit: DeferredPaletteCommitOutcome::SkippedNonIndexedDepth,
    })
}
