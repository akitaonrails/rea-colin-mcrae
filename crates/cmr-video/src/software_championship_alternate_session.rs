//! Scoped standalone ownership for the alternate championship oracle.
//!
//! This assembler consumes decoded assets from a caller-selected verified
//! import and retains source-owned logical RGB resources. Selection is decoded
//! and converted once, then sampled by an explicitly source-owned Stretch
//! policy. The session deliberately does not create an indexed backend,
//! upload surfaces, load fonts or localization, or claim native presentation
//! behavior.

use std::fmt;

use cmr_assets::AssetStore;
use cmr_core::{
    c3d_actor_rgb::C3dActorRgbState,
    c3d_topology::{C3dObjectTopology, C3dObjectTopologyError},
    c3d_type2_fog::C3dType2FogState,
    championship_assets::{
        CHAMPIONSHIP_CAR_COUNT, ChampionshipAssetLoadError, ChampionshipAssets,
        load_championship_assets,
    },
    championship_car_execute::{ChampionshipCarBatchExecutor, ChampionshipCarPreDrawState},
    championship_car_transform::{ChampionshipCarInput, ChampionshipCarState},
    championship_car_visibility::ChampionshipCarVisibilityLatches,
    championship_frame_assets::{ChampionshipFrameAssetLoadError, load_championship_frame_assets},
    championship_setup::{ActivePreview, CanonicalCar},
    championship_setup_frame::{ChampionshipSetupFramePrepareError, ChampionshipSetupFrameState},
    championship_setup_visual::{ChampionshipPreviewCache, ChampionshipSetupVisualInput},
    deferred_palette::DeferredPaletteState,
    display::DisplayExtent,
    frontend_c3d_resources::{
        FrontendC3dCarMaterializationError, FrontendC3dPreparationError, FrontendC3dPrepared,
        prepare_frontend_c3d_resources,
    },
    frontend_c3d_route::FrontendC3dRouteState,
    frontend_c3d_scene::C3dDepthClear,
    root_menu_assets::VisualVariant,
    root_menu_bitmap_assets::{RootMenuBitmapLoadError, load_root_menu_selection_bitmap},
    root_menu_c3d_assets::{RootMenuC3dLoadError, RootMenuC3dSet, load_root_menu_c3d},
    root_menu_c3d_execute::{C3dBindingBuildError, C3dBindingCatalog},
    root_menu_c3d_resources::RootMenuC3dResourceDiagnostic,
    root_menu_c3d_runtime::C3dResourceScaledObject,
    root_menu_c3d_visibility::RootMenuC3dVisibilityLatches,
};
use cmr_formats::{bitmap::BitmapLimits, c3d::C3dLimits};

use crate::{
    c3d_software_raster::SoftwareChampionshipCarSplitOracleProfile,
    c3d_software_split_backend::{
        SoftwareSplitC3dBackendError, SoftwareSplitC3dBinding, SoftwareSplitC3dBindingBuildError,
        SoftwareSplitC3dOracleBackend, SoftwareSplitC3dOracleConfiguration,
        SoftwareSplitC3dTargetHandle, prepare_software_split_frontend_c3d_bindings,
    },
    legacy_c3d_pixel_format::{KnownRgbLayout, SelectedNonAlphaTextureFormat},
    packed_rgb_target::FramebufferRgbFormat,
    selected_texture_conversion_oracle::SourceOwnedTextureConversionOraclePolicy,
    software_championship_alternate_terminal::{
        SoftwareChampionshipAlternateCarResources, SoftwareChampionshipAlternateTerminalError,
        SoftwareChampionshipAlternateTerminalOutcome, SoftwareChampionshipAlternateTerminalState,
        execute_software_championship_alternate_terminal_frame,
    },
    software_championship_ordinary_rgb16::{
        SoftwareChampionshipOrdinaryRgb16Pool, SoftwareChampionshipOrdinaryRgb16PoolError,
        prepare_software_championship_ordinary_rgb16_pool,
    },
    software_championship_selection_rgb16::{
        SoftwareChampionshipSelectionRgb16Error, SoftwareChampionshipSelectionRgb16Source,
        prepare_software_championship_selection_rgb16_source,
    },
    software_championship_session::{
        SoftwareChampionshipActorRgbSelection, SoftwareChampionshipPaletteLayout,
    },
    software_championship_setup_frame::prepare_software_championship_alternate_setup_frame,
};

/// Retained pre-draw state paired with the split raster profile.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SoftwareChampionshipAlternateRasterState {
    pub type2_fog: C3dType2FogState,
    pub actor_rgb: SoftwareChampionshipActorRgbSelection,
}

impl SoftwareChampionshipAlternateRasterState {
    #[must_use]
    pub const fn disabled() -> Self {
        Self {
            type2_fog: C3dType2FogState::DISABLED,
            actor_rgb: SoftwareChampionshipActorRgbSelection::Disabled,
        }
    }

    fn pre_draw_state_for_car(&self, car: CanonicalCar) -> ChampionshipCarPreDrawState<'_> {
        ChampionshipCarPreDrawState {
            type2_fog: self.type2_fog,
            actor_rgb: self.actor_rgb.for_car(car),
        }
    }
}

/// Explicit source-owned choices needed to assemble the alternate session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SoftwareChampionshipAlternateSessionRequest {
    pub variant: VisualVariant,
    pub bitmap_limits: BitmapLimits,
    pub c3d_limits: C3dLimits,
    pub extent: DisplayExtent,
    pub framebuffer_format: FramebufferRgbFormat,
    pub display_format_discriminator: u8,
    pub selected_texture_format: SelectedNonAlphaTextureFormat,
    pub texture_conversion_policy: SourceOwnedTextureConversionOraclePolicy,
    pub backend_configuration: SoftwareSplitC3dOracleConfiguration,
    pub raster_state: SoftwareChampionshipAlternateRasterState,
    pub depth_clear: C3dDepthClear,
    pub palette_layout: SoftwareChampionshipPaletteLayout,
}

impl SoftwareChampionshipAlternateSessionRequest {
    #[must_use]
    pub fn new(
        variant: VisualVariant,
        extent: DisplayExtent,
        framebuffer_format: FramebufferRgbFormat,
        display_format_discriminator: u8,
        selected_texture_format: SelectedNonAlphaTextureFormat,
        backend_configuration: SoftwareSplitC3dOracleConfiguration,
        raster_state: SoftwareChampionshipAlternateRasterState,
    ) -> Self {
        Self {
            variant,
            bitmap_limits: BitmapLimits::default(),
            c3d_limits: C3dLimits::default(),
            extent,
            framebuffer_format,
            display_format_discriminator,
            selected_texture_format,
            texture_conversion_policy:
                SourceOwnedTextureConversionOraclePolicy::ExactPaletteOrNearestMaskNoDither,
            backend_configuration,
            raster_state,
            depth_clear: C3dDepthClear::Clear,
            palette_layout: SoftwareChampionshipPaletteLayout::CanonicalCarsThenChequered,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SoftwareChampionshipAlternateFrameRequest<'a> {
    pub visual: ChampionshipSetupVisualInput<'a>,
    pub setup: ChampionshipSetupFrameState,
    pub car_input: ChampionshipCarInput,
    pub base_basis: [i32; 9],
}

#[derive(Debug)]
pub enum SoftwareChampionshipAlternateSessionBuildError {
    ChampionshipAssets(ChampionshipAssetLoadError),
    FrameAssets(ChampionshipFrameAssetLoadError),
    Selection(RootMenuBitmapLoadError),
    SelectionRgb16(SoftwareChampionshipSelectionRgb16Error),
    FrontendC3d(FrontendC3dPreparationError),
    CarTopology {
        car: CanonicalCar,
        source: C3dObjectTopologyError,
    },
    CarRuntime {
        car: CanonicalCar,
        source: FrontendC3dCarMaterializationError,
    },
    ActorRgbCount {
        car: CanonicalCar,
        expected: usize,
        actual: usize,
    },
    RasterProfile(SoftwareSplitC3dBackendError),
    OrdinaryFramebufferLayoutMismatch {
        expected: KnownRgbLayout,
        actual: Option<KnownRgbLayout>,
    },
    C3dBindings(C3dBindingBuildError<SoftwareSplitC3dBindingBuildError>),
    OrdinarySources(SoftwareChampionshipOrdinaryRgb16PoolError),
    Backend(SoftwareSplitC3dBackendError),
}

impl fmt::Display for SoftwareChampionshipAlternateSessionBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ChampionshipAssets(source) => source.fmt(formatter),
            Self::FrameAssets(source) => source.fmt(formatter),
            Self::Selection(source) => source.fmt(formatter),
            Self::SelectionRgb16(source) => source.fmt(formatter),
            Self::FrontendC3d(source) => source.fmt(formatter),
            Self::CarTopology { car, source } => {
                write!(
                    formatter,
                    "cannot build alternate championship topology for {car:?}: {source}"
                )
            }
            Self::CarRuntime { car, source } => {
                write!(
                    formatter,
                    "cannot materialize alternate championship runtime for {car:?}: {source}"
                )
            }
            Self::ActorRgbCount {
                car,
                expected,
                actual,
            } => write!(
                formatter,
                "alternate actor-RGB state for {car:?} has {actual} actors; loaded topology requires {expected}"
            ),
            Self::RasterProfile(source) => {
                write!(
                    formatter,
                    "cannot configure alternate championship raster profile: {source}"
                )
            }
            Self::OrdinaryFramebufferLayoutMismatch { expected, actual } => write!(
                formatter,
                "ordinary championship sources use {expected:?}, but alternate framebuffer layout is {actual:?}"
            ),
            Self::C3dBindings(source) => source.fmt(formatter),
            Self::OrdinarySources(source) => source.fmt(formatter),
            Self::Backend(source) => source.fmt(formatter),
        }
    }
}

impl std::error::Error for SoftwareChampionshipAlternateSessionBuildError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ChampionshipAssets(source) => Some(source),
            Self::FrameAssets(source) => Some(source),
            Self::Selection(source) => Some(source),
            Self::SelectionRgb16(source) => Some(source),
            Self::FrontendC3d(source) => Some(source),
            Self::CarTopology { source, .. } => Some(source),
            Self::CarRuntime { source, .. } => Some(source),
            Self::ActorRgbCount { .. } | Self::OrdinaryFramebufferLayoutMismatch { .. } => None,
            Self::RasterProfile(source) | Self::Backend(source) => Some(source),
            Self::C3dBindings(source) => Some(source),
            Self::OrdinarySources(source) => Some(source),
        }
    }
}

#[derive(Debug)]
pub enum SoftwareChampionshipAlternateRenderError {
    ExtentMismatch {
        expected: DisplayExtent,
        actual: DisplayExtent,
    },
    Prepare(ChampionshipSetupFramePrepareError),
    Frame(Box<SoftwareChampionshipAlternateTerminalError>),
}

impl fmt::Display for SoftwareChampionshipAlternateRenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExtentMismatch { expected, actual } => write!(
                formatter,
                "alternate championship frame extent {actual:?} does not match session extent {expected:?}"
            ),
            Self::Prepare(source) => source.fmt(formatter),
            Self::Frame(source) => source.fmt(formatter),
        }
    }
}

impl std::error::Error for SoftwareChampionshipAlternateRenderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ExtentMismatch { .. } => None,
            Self::Prepare(source) => Some(source),
            Self::Frame(source) => Some(source),
        }
    }
}

pub struct SoftwareChampionshipAlternateSessionRun<R> {
    pub output: R,
    pub backend: SoftwareSplitC3dOracleBackend,
}

pub struct SoftwareChampionshipAlternateSession<'session, 'asset, 'catalog> {
    request: SoftwareChampionshipAlternateSessionRequest,
    championship: &'session ChampionshipAssets,
    root: &'session RootMenuC3dSet,
    frontend: &'session FrontendC3dPrepared,
    topologies: &'session [C3dObjectTopology<'asset>; CHAMPIONSHIP_CAR_COUNT],
    runtimes: &'session [C3dResourceScaledObject; CHAMPIONSHIP_CAR_COUNT],
    sources: &'session SoftwareChampionshipOrdinaryRgb16Pool,
    selection: &'session SoftwareChampionshipSelectionRgb16Source,
    executor: &'session mut ChampionshipCarBatchExecutor<'catalog, SoftwareSplitC3dBinding>,
    backend: &'session mut SoftwareSplitC3dOracleBackend,
    preview_cache: &'session mut ChampionshipPreviewCache,
    car_transform: &'session mut ChampionshipCarState,
    car_visibility: &'session mut ChampionshipCarVisibilityLatches,
    root_visibility: &'session mut RootMenuC3dVisibilityLatches,
    route: &'session FrontendC3dRouteState,
    palette_state: &'session mut DeferredPaletteState,
}

impl SoftwareChampionshipAlternateSession<'_, '_, '_> {
    pub fn render(
        &mut self,
        frame: SoftwareChampionshipAlternateFrameRequest<'_>,
    ) -> Result<
        SoftwareChampionshipAlternateTerminalOutcome,
        SoftwareChampionshipAlternateRenderError,
    > {
        if frame.visual.viewport != self.request.extent {
            return Err(SoftwareChampionshipAlternateRenderError::ExtentMismatch {
                expected: self.request.extent,
                actual: frame.visual.viewport,
            });
        }

        let prepared = prepare_software_championship_alternate_setup_frame(
            frame.visual,
            frame.setup,
            *self.preview_cache,
            self.sources,
            Some(self.selection),
        )
        .map_err(SoftwareChampionshipAlternateRenderError::Prepare)?;
        let selected = match frame.visual.transcript.preview {
            ActivePreview::Car { canonical_car, .. }
            | ActivePreview::Gear { canonical_car, .. } => Some(canonical_car),
            ActivePreview::Next => None,
        };
        let car = selected.map(|car| SoftwareChampionshipAlternateCarResources {
            car,
            topology: &self.topologies[car.index() as usize],
            runtime: &self.runtimes[car.index() as usize],
            base_basis: frame.base_basis,
            pre_draw_state: self.request.raster_state.pre_draw_state_for_car(car),
        });
        let destination = self.backend.target_handle();

        execute_software_championship_alternate_terminal_frame(
            prepared,
            self.request.extent,
            frame.car_input,
            car,
            SoftwareChampionshipAlternateTerminalState {
                preview_cache: self.preview_cache,
                car_transform: self.car_transform,
                car_visibility: self.car_visibility,
                root_visibility: self.root_visibility,
                palette_state: self.palette_state,
                palette_group_bases: self.request.palette_layout.group_bases(),
                route: self.route,
            },
            self.executor,
            self.sources,
            Some(self.selection),
            self.backend,
            destination,
            self.request.depth_clear,
        )
        .map_err(|source| SoftwareChampionshipAlternateRenderError::Frame(Box::new(source)))
    }

    #[must_use]
    pub const fn request(&self) -> &SoftwareChampionshipAlternateSessionRequest {
        &self.request
    }

    #[must_use]
    pub fn backend(&self) -> &SoftwareSplitC3dOracleBackend {
        self.backend
    }

    #[must_use]
    pub fn target(&self) -> SoftwareSplitC3dTargetHandle {
        self.backend.target_handle()
    }

    #[must_use]
    pub const fn preview_cache(&self) -> ChampionshipPreviewCache {
        *self.preview_cache
    }

    #[must_use]
    pub const fn car_transform(&self) -> ChampionshipCarState {
        *self.car_transform
    }

    #[must_use]
    pub fn car_visibility(&self) -> &ChampionshipCarVisibilityLatches {
        self.car_visibility
    }

    #[must_use]
    pub fn root_visibility(&self) -> &RootMenuC3dVisibilityLatches {
        self.root_visibility
    }

    #[must_use]
    pub const fn palette_state(&self) -> DeferredPaletteState {
        *self.palette_state
    }

    #[must_use]
    pub fn active_c3d_route(&self) -> Option<cmr_core::frontend_c3d_route::FrontendC3dRoute> {
        self.route.active()
    }

    #[must_use]
    pub const fn ordinary_source_count(&self) -> usize {
        self.sources.len()
    }

    #[must_use]
    /// Returns false because the retained converted Selection now executes at
    /// its recovered bitmap position through the source-owned Stretch policy.
    pub const fn selection_is_deferred(&self) -> bool {
        false
    }

    #[must_use]
    pub const fn selection_is_loaded(&self) -> bool {
        true
    }

    pub fn root_c3d_load_diagnostics(
        &self,
    ) -> impl ExactSizeIterator<Item = &RootMenuC3dLoadError> {
        self.root.diagnostics()
    }

    pub fn root_c3d_preparation_diagnostics(
        &self,
    ) -> impl ExactSizeIterator<Item = &RootMenuC3dResourceDiagnostic> {
        self.frontend.root_diagnostics()
    }

    #[must_use]
    pub const fn championship_assets(&self) -> &ChampionshipAssets {
        self.championship
    }
}

fn validate_raster_state(
    profile: SoftwareChampionshipCarSplitOracleProfile,
    state: &SoftwareChampionshipAlternateRasterState,
) -> Result<(), SoftwareChampionshipAlternateSessionBuildError> {
    if profile.raster.fog_mutation != state.type2_fog.enabled {
        return Err(
            SoftwareChampionshipAlternateSessionBuildError::RasterProfile(
                SoftwareSplitC3dBackendError::ChampionshipCarFogStateMismatch {
                    profile_enabled: profile.raster.fog_mutation,
                    prepared_enabled: state.type2_fog.enabled,
                },
            ),
        );
    }
    if profile.raster.actor_rgb_override != state.actor_rgb.enabled() {
        return Err(
            SoftwareChampionshipAlternateSessionBuildError::RasterProfile(
                SoftwareSplitC3dBackendError::ChampionshipCarActorRgbStateMismatch {
                    profile_enabled: profile.raster.actor_rgb_override,
                    prepared_enabled: state.actor_rgb.enabled(),
                },
            ),
        );
    }
    Ok(())
}

fn validate_actor_rgb_counts(
    state: &SoftwareChampionshipActorRgbSelection,
    topologies: &[C3dObjectTopology<'_>; CHAMPIONSHIP_CAR_COUNT],
) -> Result<(), SoftwareChampionshipAlternateSessionBuildError> {
    if matches!(state, SoftwareChampionshipActorRgbSelection::Disabled) {
        return Ok(());
    }
    for car in CanonicalCar::ALL {
        let expected = topologies[car.index() as usize].group0_records().count();
        let actual = match state.for_car(car) {
            C3dActorRgbState::Enabled(entries) => entries.len(),
            C3dActorRgbState::Disabled => unreachable!("enabled selection has every car table"),
        };
        if actual != expected {
            return Err(
                SoftwareChampionshipAlternateSessionBuildError::ActorRgbCount {
                    car,
                    expected,
                    actual,
                },
            );
        }
    }
    Ok(())
}

fn validate_ordinary_framebuffer_layout(
    discriminator: u8,
    framebuffer: FramebufferRgbFormat,
) -> Result<(), SoftwareChampionshipAlternateSessionBuildError> {
    let expected = if discriminator == 5 {
        KnownRgbLayout::Rgb555
    } else {
        KnownRgbLayout::Rgb565
    };
    let actual = framebuffer.layout().known_layout();
    if actual == Some(expected) {
        Ok(())
    } else {
        Err(
            SoftwareChampionshipAlternateSessionBuildError::OrdinaryFramebufferLayoutMismatch {
                expected,
                actual,
            },
        )
    }
}

/// Loads and retains one source-owned alternate championship session for a
/// callback, returning its logical target backend after the callback ends.
pub fn with_software_championship_alternate_session<R>(
    store: &AssetStore,
    request: SoftwareChampionshipAlternateSessionRequest,
    run: impl FnOnce(&mut SoftwareChampionshipAlternateSession<'_, '_, '_>) -> R,
) -> Result<
    SoftwareChampionshipAlternateSessionRun<R>,
    SoftwareChampionshipAlternateSessionBuildError,
> {
    validate_ordinary_framebuffer_layout(
        request.display_format_discriminator,
        request.framebuffer_format,
    )?;
    validate_raster_state(request.backend_configuration.profile, &request.raster_state)?;

    let championship =
        load_championship_assets(store, request.c3d_limits, request.bitmap_limits)
            .map_err(SoftwareChampionshipAlternateSessionBuildError::ChampionshipAssets)?;
    let frame_assets = load_championship_frame_assets(store, request.bitmap_limits)
        .map_err(SoftwareChampionshipAlternateSessionBuildError::FrameAssets)?;
    let selection = load_root_menu_selection_bitmap(store, request.bitmap_limits)
        .map_err(SoftwareChampionshipAlternateSessionBuildError::Selection)?;
    let selection = prepare_software_championship_selection_rgb16_source(
        &selection.bitmap,
        request.display_format_discriminator,
    )
    .map_err(SoftwareChampionshipAlternateSessionBuildError::SelectionRgb16)?;
    let root = load_root_menu_c3d(store, request.variant, request.c3d_limits);
    let frontend = prepare_frontend_c3d_resources(&championship, &root, request.bitmap_limits)
        .map_err(SoftwareChampionshipAlternateSessionBuildError::FrontendC3d)?;

    let mut topology_values = Vec::with_capacity(CHAMPIONSHIP_CAR_COUNT);
    for car in CanonicalCar::ALL {
        topology_values.push(
            frontend
                .car_topology(&championship, car)
                .map_err(
                    |source| SoftwareChampionshipAlternateSessionBuildError::CarTopology {
                        car,
                        source,
                    },
                )?,
        );
    }
    let topologies: [C3dObjectTopology<'_>; CHAMPIONSHIP_CAR_COUNT] = topology_values
        .try_into()
        .expect("one topology was built for every canonical car");
    validate_actor_rgb_counts(&request.raster_state.actor_rgb, &topologies)?;

    let mut runtime_values = Vec::with_capacity(CHAMPIONSHIP_CAR_COUNT);
    for car in CanonicalCar::ALL {
        runtime_values.push(
            frontend
                .materialize_car(car, &topologies[car.index() as usize])
                .map_err(
                    |source| SoftwareChampionshipAlternateSessionBuildError::CarRuntime {
                        car,
                        source,
                    },
                )?,
        );
    }
    let runtimes: [C3dResourceScaledObject; CHAMPIONSHIP_CAR_COUNT] = runtime_values
        .try_into()
        .expect("one runtime was built for every canonical car");
    let catalog: C3dBindingCatalog<SoftwareSplitC3dBinding> =
        prepare_software_split_frontend_c3d_bindings(
            &frontend,
            request.selected_texture_format,
            request.texture_conversion_policy,
        )
        .map_err(SoftwareChampionshipAlternateSessionBuildError::C3dBindings)?;
    let mut executor = ChampionshipCarBatchExecutor::new(&catalog);
    let sources = prepare_software_championship_ordinary_rgb16_pool(
        &championship,
        &frame_assets,
        request.display_format_discriminator,
    )
    .map_err(SoftwareChampionshipAlternateSessionBuildError::OrdinarySources)?;
    let mut backend = SoftwareSplitC3dOracleBackend::new(
        request.extent,
        request.framebuffer_format,
        request.backend_configuration,
    )
    .map_err(SoftwareChampionshipAlternateSessionBuildError::Backend)?;

    let mut preview_cache = ChampionshipPreviewCache::new();
    let mut car_transform = ChampionshipCarState::new();
    let mut car_visibility = ChampionshipCarVisibilityLatches::new();
    let mut root_visibility = RootMenuC3dVisibilityLatches::new();
    let route = FrontendC3dRouteState::new();
    let mut palette_state = DeferredPaletteState::new();

    let output = {
        let mut session = SoftwareChampionshipAlternateSession {
            request,
            championship: &championship,
            root: &root,
            frontend: &frontend,
            topologies: &topologies,
            runtimes: &runtimes,
            sources: &sources,
            selection: &selection,
            executor: &mut executor,
            backend: &mut backend,
            preview_cache: &mut preview_cache,
            car_transform: &mut car_transform,
            car_visibility: &mut car_visibility,
            root_visibility: &mut root_visibility,
            route: &route,
            palette_state: &mut palette_state,
        };
        run(&mut session)
    };

    Ok(SoftwareChampionshipAlternateSessionRun { output, backend })
}
