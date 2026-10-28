//! Scoped compatibility assembly for the software championship setup screen.
//!
//! The decoded car objects own geometry borrowed by their prepared topologies,
//! while the batch executor borrows its binding catalog. Keeping those owners
//! as stack locals and exposing a facade only during a callback avoids a
//! self-referential session type. Mutable preview, transform, route, palette,
//! and binding-cache state is retained for the complete callback scope.

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
    championship_bitmap_raster::{
        ChampionshipLogoSurfaceError, ChampionshipLogoUploadError, UploadedChampionshipLogoPool,
        prepare_championship_logo_surfaces, upload_championship_logos,
    },
    championship_car_execute::{ChampionshipCarBatchExecutor, ChampionshipCarPreDrawState},
    championship_car_transform::{ChampionshipCarInput, ChampionshipCarState},
    championship_car_visibility::ChampionshipCarVisibilityLatches,
    championship_frame_assets::{
        ChampionshipFrameAssetLoadError, ChampionshipFrameSurfaceError,
        ChampionshipFrameUploadError, UploadedChampionshipFramePool,
        load_championship_frame_assets, prepare_championship_frame_surfaces,
        upload_championship_frame_surfaces,
    },
    championship_setup::{ActivePreview, CanonicalCar},
    championship_setup_frame::{ChampionshipSetupFrameResources, ChampionshipSetupFrameState},
    championship_setup_visual::{ChampionshipPreviewCache, ChampionshipSetupVisualInput},
    championship_terminal_frame::{
        ChampionshipCarFrameResources, ChampionshipTerminalFrameError,
        ChampionshipTerminalFrameOutcome, ChampionshipTerminalFrameResources,
        ChampionshipTerminalFrameState, ChampionshipTerminalPaletteResources,
        execute_championship_terminal_frame,
    },
    deferred_palette::{DeferredPaletteState, PaletteGroupBases},
    display::{IndexedFrameTarget, StartupCopyOutcome},
    font_assets::{FontLoadError, load_font_set},
    font_raster::{FontPoolUploadError, UploadedFontPool},
    frontend_c3d_resources::{
        FrontendC3dCarMaterializationError, FrontendC3dPreparationError, FrontendC3dPrepared,
        prepare_frontend_c3d_resources,
    },
    frontend_c3d_route::FrontendC3dRouteState,
    frontend_c3d_scene::C3dDepthClear,
    localization::{IndexedTextTable, Language},
    localization_assets::{LocalizationLoadError, load_frontend_text_table},
    root_menu_assets::VisualVariant,
    root_menu_c3d_assets::{RootMenuC3dLoadError, RootMenuC3dSet, load_root_menu_c3d},
    root_menu_c3d_execute::C3dBindingBuildError,
    root_menu_c3d_resources::RootMenuC3dResourceDiagnostic,
    root_menu_c3d_runtime::C3dResourceScaledObject,
    root_menu_c3d_visibility::RootMenuC3dVisibilityLatches,
    source_store::UploadedSourceStore,
    startup_display::StartupDisplay,
    surface::PaletteMapping,
};
use cmr_formats::{
    bitmap::BitmapLimits,
    c3d::C3dLimits,
    font::{FontLimits, TextLayoutLimits},
    riff_palette::{LegacyPalette256, PaletteEntry},
};

use crate::{
    SoftwareC3dBackendError, SoftwareIndexedBackend, SoftwarePresentationError, SoftwareVideoError,
    SurfaceHandle,
    c3d_software::{
        SoftwareC3dSampledTexture, SoftwareC3dSamplingError, prepare_software_frontend_c3d_bindings,
    },
    c3d_software_raster::SoftwareChampionshipCarRasterProfile,
};

const CHEQUERED_PALETTE_ARENA_INDEX: i32 = CHAMPIONSHIP_CAR_COUNT as i32;
const NEXT_PREVIEW_PALETTE_ITEM: i32 = 9;

/// Provenance for the source-owned palette indices retained by this session.
///
/// This is deliberately not a claim about the process-global indices used by
/// the shipped executable. It packs the fourteen canonical car palettes first
/// and the loaded FCHE palette last while preserving the recovered mapper's
/// group/item arithmetic for the only requests emitted by this screen.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SoftwareChampionshipPaletteLayout {
    CanonicalCarsThenChequered,
}

impl SoftwareChampionshipPaletteLayout {
    #[must_use]
    pub const fn group_bases(self) -> PaletteGroupBases {
        match self {
            Self::CanonicalCarsThenChequered => PaletteGroupBases {
                group_1: CHEQUERED_PALETTE_ARENA_INDEX - NEXT_PREVIEW_PALETTE_ITEM,
                // These groups are not emitted by the recovered championship
                // setup transcript. Values remain explicit rather than being
                // inferred from unrelated frontend screens.
                group_2: 0,
                group_3: 0,
                group_4: 0,
                group_5: CHEQUERED_PALETTE_ARENA_INDEX,
            },
        }
    }

    #[must_use]
    pub const fn palette_count(self) -> usize {
        match self {
            Self::CanonicalCarsThenChequered => CHAMPIONSHIP_CAR_COUNT + 1,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SoftwareChampionshipRasterSelectionProvenance {
    /// Raster tuple asserted by the caller; this does not claim to import the
    /// standalone session's preview, transform, route, or palette state.
    CallerAssertedRasterProfile,
    /// Narrow implementation policy for the deterministic software oracle.
    /// It is not presented as an original-game startup state.
    SupportedSoftwareOraclePolicy,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SoftwareChampionshipActorRgbSelection {
    Disabled,
    /// One source-ordered runtime actor RGB table for each canonical car.
    Enabled(Box<[Box<[[u8; 3]]>; CHAMPIONSHIP_CAR_COUNT]>),
}

impl SoftwareChampionshipActorRgbSelection {
    #[must_use]
    pub const fn enabled(&self) -> bool {
        matches!(self, Self::Enabled(_))
    }

    #[must_use]
    pub fn for_car(&self, car: CanonicalCar) -> C3dActorRgbState<'_> {
        match self {
            Self::Disabled => C3dActorRgbState::Disabled,
            Self::Enabled(cars) => C3dActorRgbState::Enabled(&cars[car.index() as usize]),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SoftwareChampionshipRasterSelection {
    pub raster_profile: SoftwareChampionshipCarRasterProfile,
    pub type2_fog: C3dType2FogState,
    pub actor_rgb: SoftwareChampionshipActorRgbSelection,
    pub provenance: SoftwareChampionshipRasterSelectionProvenance,
}

impl SoftwareChampionshipRasterSelection {
    #[must_use]
    pub fn caller_asserted(
        raster_profile: SoftwareChampionshipCarRasterProfile,
        type2_fog: C3dType2FogState,
    ) -> Self {
        Self {
            raster_profile,
            type2_fog,
            actor_rgb: SoftwareChampionshipActorRgbSelection::Disabled,
            provenance: SoftwareChampionshipRasterSelectionProvenance::CallerAssertedRasterProfile,
        }
    }

    #[must_use]
    pub fn caller_asserted_with_actor_rgb(
        raster_profile: SoftwareChampionshipCarRasterProfile,
        type2_fog: C3dType2FogState,
        actor_rgb_by_car: [Box<[[u8; 3]]>; CHAMPIONSHIP_CAR_COUNT],
    ) -> Self {
        Self {
            raster_profile,
            type2_fog,
            actor_rgb: SoftwareChampionshipActorRgbSelection::Enabled(Box::new(actor_rgb_by_car)),
            provenance: SoftwareChampionshipRasterSelectionProvenance::CallerAssertedRasterProfile,
        }
    }

    /// Selects the default mutation-disabled deterministic software policy.
    #[must_use]
    pub fn supported_software_oracle_policy() -> Self {
        Self {
            raster_profile: SoftwareChampionshipCarRasterProfile::MUTATION_DISABLED_ACTOR_RGB_DISABLED_UNIFIED_INDEXED_COPY_NO_BLEND,
            type2_fog: C3dType2FogState::DISABLED,
            actor_rgb: SoftwareChampionshipActorRgbSelection::Disabled,
            provenance: SoftwareChampionshipRasterSelectionProvenance::SupportedSoftwareOraclePolicy,
        }
    }

    pub(crate) fn pre_draw_state_for_car(
        &self,
        car: CanonicalCar,
    ) -> ChampionshipCarPreDrawState<'_> {
        ChampionshipCarPreDrawState {
            type2_fog: self.type2_fog,
            actor_rgb: self.actor_rgb.for_car(car),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SoftwareChampionshipSessionRequest {
    pub language: Language,
    pub variant: VisualVariant,
    pub bitmap_limits: BitmapLimits,
    pub font_limits: FontLimits,
    pub c3d_limits: C3dLimits,
    pub text_layout_limits: TextLayoutLimits,
    pub depth_clear: C3dDepthClear,
    /// Caller-selected retained renderer state with explicit evidence scope.
    /// No frontend-entry default is inferred because static evidence does not
    /// prove the fog, alpha, actor-RGB, or device-mode gates.
    pub raster_selection: SoftwareChampionshipRasterSelection,
    /// Explicit unresolved 2D conversion choices. Identity is the bounded
    /// index-preserving software path, but is never inferred by the session.
    pub logo_mapping: PaletteMapping,
    pub info_mapping: PaletteMapping,
    pub palette_layout: SoftwareChampionshipPaletteLayout,
}

impl SoftwareChampionshipSessionRequest {
    #[must_use]
    pub fn new(
        language: Language,
        variant: VisualVariant,
        raster_selection: SoftwareChampionshipRasterSelection,
        logo_mapping: PaletteMapping,
        info_mapping: PaletteMapping,
    ) -> Self {
        Self {
            language,
            variant,
            bitmap_limits: BitmapLimits::default(),
            font_limits: FontLimits::default(),
            c3d_limits: C3dLimits::default(),
            text_layout_limits: TextLayoutLimits::default(),
            depth_clear: C3dDepthClear::Clear,
            raster_selection,
            logo_mapping,
            info_mapping,
            palette_layout: SoftwareChampionshipPaletteLayout::CanonicalCarsThenChequered,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SoftwareChampionshipFrameRequest<'a> {
    pub visual: ChampionshipSetupVisualInput<'a>,
    pub setup: ChampionshipSetupFrameState,
    pub car_input: ChampionshipCarInput,
    pub base_basis: [i32; 9],
}

fn validate_actor_rgb_counts(
    selection: &SoftwareChampionshipActorRgbSelection,
    expected_count: impl Fn(CanonicalCar) -> usize,
) -> Result<(), SoftwareChampionshipSessionBuildError> {
    let SoftwareChampionshipActorRgbSelection::Enabled(cars) = selection else {
        return Ok(());
    };

    for car in CanonicalCar::ALL {
        let expected = expected_count(car);
        let actual = cars[car.index() as usize].len();
        if actual != expected {
            return Err(SoftwareChampionshipSessionBuildError::ActorRgbCount {
                car,
                expected,
                actual,
            });
        }
    }
    Ok(())
}

fn validate_raster_selection(
    selection: &SoftwareChampionshipRasterSelection,
) -> Result<(), SoftwareChampionshipSessionBuildError> {
    if selection.raster_profile.fog_mutation != selection.type2_fog.enabled {
        return Err(SoftwareChampionshipSessionBuildError::RasterProfile(
            SoftwareC3dBackendError::ChampionshipCarFogStateMismatch {
                profile_enabled: selection.raster_profile.fog_mutation,
                prepared_enabled: selection.type2_fog.enabled,
            },
        ));
    }
    if selection.raster_profile.actor_rgb_override != selection.actor_rgb.enabled() {
        return Err(SoftwareChampionshipSessionBuildError::RasterProfile(
            SoftwareC3dBackendError::ChampionshipCarActorRgbStateMismatch {
                profile_enabled: selection.raster_profile.actor_rgb_override,
                prepared_enabled: selection.actor_rgb.enabled(),
            },
        ));
    }
    Ok(())
}

#[derive(Debug)]
pub enum SoftwareChampionshipSessionBuildError {
    FrontendText(LocalizationLoadError),
    Fonts(FontLoadError),
    ChampionshipAssets(ChampionshipAssetLoadError),
    FrameAssets(ChampionshipFrameAssetLoadError),
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
    C3dBindings(C3dBindingBuildError<SoftwareC3dSamplingError>),
    LogoSurfaces(ChampionshipLogoSurfaceError),
    FrameSurfaces(ChampionshipFrameSurfaceError),
    MissingBootstrapPalette,
    RasterProfile(SoftwareC3dBackendError),
    FontUpload(FontPoolUploadError<SoftwareVideoError>),
    LogoUpload(ChampionshipLogoUploadError<SoftwareVideoError>),
    FrameUpload(ChampionshipFrameUploadError<SoftwareVideoError>),
}

impl fmt::Display for SoftwareChampionshipSessionBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FrontendText(source) => source.fmt(formatter),
            Self::Fonts(source) => source.fmt(formatter),
            Self::ChampionshipAssets(source) => source.fmt(formatter),
            Self::FrameAssets(source) => source.fmt(formatter),
            Self::FrontendC3d(source) => source.fmt(formatter),
            Self::CarTopology { car, source } => {
                write!(
                    formatter,
                    "cannot build championship topology for {car:?}: {source}"
                )
            }
            Self::CarRuntime { car, source } => {
                write!(
                    formatter,
                    "cannot materialize championship runtime for {car:?}: {source}"
                )
            }
            Self::ActorRgbCount {
                car,
                expected,
                actual,
            } => write!(
                formatter,
                "actor-RGB state for {car:?} has {actual} actors; loaded topology requires {expected}"
            ),
            Self::C3dBindings(source) => source.fmt(formatter),
            Self::LogoSurfaces(source) => source.fmt(formatter),
            Self::FrameSurfaces(source) => source.fmt(formatter),
            Self::MissingBootstrapPalette => {
                write!(formatter, "startup display has no active palette")
            }
            Self::RasterProfile(source) => {
                write!(
                    formatter,
                    "cannot configure championship raster profile: {source}"
                )
            }
            Self::FontUpload(source) => source.fmt(formatter),
            Self::LogoUpload(source) => source.fmt(formatter),
            Self::FrameUpload(source) => source.fmt(formatter),
        }
    }
}

impl std::error::Error for SoftwareChampionshipSessionBuildError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::FrontendText(source) => Some(source),
            Self::Fonts(source) => Some(source),
            Self::ChampionshipAssets(source) => Some(source),
            Self::FrameAssets(source) => Some(source),
            Self::FrontendC3d(source) => Some(source),
            Self::CarTopology { source, .. } => Some(source),
            Self::CarRuntime { source, .. } => Some(source),
            Self::ActorRgbCount { .. } => None,
            Self::C3dBindings(source) => Some(source),
            Self::LogoSurfaces(source) => Some(source),
            Self::FrameSurfaces(source) => Some(source),
            Self::RasterProfile(source) => Some(source),
            Self::FontUpload(source) => Some(source),
            Self::LogoUpload(source) => Some(source),
            Self::FrameUpload(source) => Some(source),
            Self::MissingBootstrapPalette => None,
        }
    }
}

pub type SoftwareChampionshipTerminalError = ChampionshipTerminalFrameError<
    SoftwareVideoError,
    SoftwareC3dBackendError,
    SoftwarePresentationError,
>;

#[derive(Debug)]
pub enum SoftwareChampionshipRenderError {
    ExtentMismatch {
        expected: cmr_core::display::DisplayExtent,
        actual: cmr_core::display::DisplayExtent,
    },
    Frame(Box<SoftwareChampionshipTerminalError>),
}

impl fmt::Display for SoftwareChampionshipRenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExtentMismatch { expected, actual } => write!(
                formatter,
                "championship frame extent {actual:?} does not match session extent {expected:?}"
            ),
            Self::Frame(source) => source.fmt(formatter),
        }
    }
}

impl std::error::Error for SoftwareChampionshipRenderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Frame(source) => Some(source),
            Self::ExtentMismatch { .. } => None,
        }
    }
}

pub struct SoftwareChampionshipSessionRun<R> {
    pub output: R,
    pub backend: SoftwareIndexedBackend,
    pub startup_copy: StartupCopyOutcome<SoftwareVideoError>,
}

pub struct SoftwareChampionshipSession<'session, 'asset, 'catalog, 'backend> {
    request: SoftwareChampionshipSessionRequest,
    target: IndexedFrameTarget<SurfaceHandle>,
    sources: &'session mut UploadedSourceStore<'backend, SoftwareIndexedBackend>,
    text: &'session IndexedTextTable,
    fonts: &'session UploadedFontPool,
    logos: &'session UploadedChampionshipLogoPool,
    frames: &'session UploadedChampionshipFramePool,
    championship: &'session ChampionshipAssets,
    root: &'session RootMenuC3dSet,
    frontend: &'session FrontendC3dPrepared,
    topologies: &'session [C3dObjectTopology<'asset>; CHAMPIONSHIP_CAR_COUNT],
    runtimes: &'session [C3dResourceScaledObject; CHAMPIONSHIP_CAR_COUNT],
    executor: &'session mut ChampionshipCarBatchExecutor<'catalog, SoftwareC3dSampledTexture>,
    preview_cache: &'session mut ChampionshipPreviewCache,
    car_transform: &'session mut ChampionshipCarState,
    car_visibility: &'session mut ChampionshipCarVisibilityLatches,
    root_visibility: &'session mut RootMenuC3dVisibilityLatches,
    route: &'session FrontendC3dRouteState,
    palette_arena: &'session [[PaletteEntry; 256]],
    palette_state: &'session mut DeferredPaletteState,
    renderer_palette_cache: &'session mut [PaletteEntry; 256],
    global_palette_cache: &'session mut [PaletteEntry; 256],
}

impl SoftwareChampionshipSession<'_, '_, '_, '_> {
    pub fn render(
        &mut self,
        frame: SoftwareChampionshipFrameRequest<'_>,
    ) -> Result<ChampionshipTerminalFrameOutcome<SoftwareVideoError>, SoftwareChampionshipRenderError>
    {
        if frame.visual.viewport != self.target.mode.extent {
            return Err(SoftwareChampionshipRenderError::ExtentMismatch {
                expected: self.target.mode.extent,
                actual: frame.visual.viewport,
            });
        }
        let selected = match frame.visual.transcript.preview {
            ActivePreview::Car { canonical_car, .. }
            | ActivePreview::Gear { canonical_car, .. } => Some(canonical_car),
            ActivePreview::Next => None,
        };
        let car = selected.map(|car| ChampionshipCarFrameResources {
            car,
            topology: &self.topologies[car.index() as usize],
            runtime: &self.runtimes[car.index() as usize],
            base_basis: frame.base_basis,
            pre_draw_state: self.request.raster_selection.pre_draw_state_for_car(car),
        });

        execute_championship_terminal_frame(
            frame.visual,
            frame.setup,
            frame.car_input,
            ChampionshipTerminalFrameResources {
                setup: ChampionshipSetupFrameResources {
                    text: self.text,
                    fonts: self.fonts,
                    // The recovered 256x256-to-row stretch remains outside the
                    // deterministic software backend's copy-only contract.
                    selection: None,
                    logos: self.logos,
                    frames: self.frames,
                },
                car,
            },
            ChampionshipTerminalFrameState {
                preview_cache: self.preview_cache,
                car_transform: self.car_transform,
                car_visibility: self.car_visibility,
                root_visibility: self.root_visibility,
                route: self.route,
            },
            ChampionshipTerminalPaletteResources {
                bits_per_pixel: self.target.mode.bits_per_pixel,
                group_bases: self.request.palette_layout.group_bases(),
                arena: self.palette_arena,
                state: self.palette_state,
                renderer_cache: self.renderer_palette_cache,
                global_cache: self.global_palette_cache,
            },
            self.executor,
            self.sources,
            self.target.back,
            self.request.text_layout_limits,
            self.request.depth_clear,
        )
        .map_err(|source| SoftwareChampionshipRenderError::Frame(Box::new(source)))
    }

    #[must_use]
    pub const fn request(&self) -> &SoftwareChampionshipSessionRequest {
        &self.request
    }

    #[must_use]
    pub const fn target(&self) -> IndexedFrameTarget<SurfaceHandle> {
        self.target
    }

    #[must_use]
    pub fn backend(&self) -> &SoftwareIndexedBackend {
        self.sources.backend()
    }

    #[must_use]
    pub fn uploaded_source_count(&self) -> usize {
        self.sources.uploaded_source_count()
    }

    #[must_use]
    pub const fn palette_state(&self) -> DeferredPaletteState {
        *self.palette_state
    }

    #[must_use]
    pub fn renderer_palette_cache(&self) -> &[PaletteEntry; 256] {
        self.renderer_palette_cache
    }

    #[must_use]
    pub fn global_palette_cache(&self) -> &[PaletteEntry; 256] {
        self.global_palette_cache
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
    pub fn active_c3d_route(&self) -> Option<cmr_core::frontend_c3d_route::FrontendC3dRoute> {
        self.route.active()
    }

    #[must_use]
    pub const fn selection_is_deferred(&self) -> bool {
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
    pub fn championship_assets(&self) -> &ChampionshipAssets {
        self.championship
    }
}

/// Loads and retains one software championship setup session for a callback.
///
/// The caller-selected raster profile is configured exactly once after the
/// startup display has created its surfaces. Surface creation resets backend
/// C3D state, so configuring it earlier would not retain the requested state.
pub fn with_software_championship_session<R>(
    store: &AssetStore,
    startup: StartupDisplay<SoftwareIndexedBackend>,
    request: SoftwareChampionshipSessionRequest,
    run: impl FnOnce(&mut SoftwareChampionshipSession<'_, '_, '_, '_>) -> R,
) -> Result<SoftwareChampionshipSessionRun<R>, SoftwareChampionshipSessionBuildError> {
    let text = load_frontend_text_table(store, request.language)
        .map_err(SoftwareChampionshipSessionBuildError::FrontendText)?;
    let fonts = load_font_set(store, request.font_limits)
        .map_err(SoftwareChampionshipSessionBuildError::Fonts)?;
    let championship = load_championship_assets(store, request.c3d_limits, request.bitmap_limits)
        .map_err(SoftwareChampionshipSessionBuildError::ChampionshipAssets)?;
    let frame_assets = load_championship_frame_assets(store, request.bitmap_limits)
        .map_err(SoftwareChampionshipSessionBuildError::FrameAssets)?;
    let root = load_root_menu_c3d(store, request.variant, request.c3d_limits);
    let frontend = prepare_frontend_c3d_resources(&championship, &root, request.bitmap_limits)
        .map_err(SoftwareChampionshipSessionBuildError::FrontendC3d)?;

    let mut topology_values = Vec::with_capacity(CHAMPIONSHIP_CAR_COUNT);
    for car in CanonicalCar::ALL {
        topology_values.push(
            frontend
                .car_topology(&championship, car)
                .map_err(
                    |source| SoftwareChampionshipSessionBuildError::CarTopology { car, source },
                )?,
        );
    }
    let topologies: [C3dObjectTopology<'_>; CHAMPIONSHIP_CAR_COUNT] = topology_values
        .try_into()
        .expect("one topology was built for every canonical car");
    validate_actor_rgb_counts(&request.raster_selection.actor_rgb, |car| {
        topologies[car.index() as usize].group0_records().count()
    })?;
    let mut runtime_values = Vec::with_capacity(CHAMPIONSHIP_CAR_COUNT);
    for car in CanonicalCar::ALL {
        runtime_values.push(
            frontend
                .materialize_car(car, &topologies[car.index() as usize])
                .map_err(|source| SoftwareChampionshipSessionBuildError::CarRuntime {
                    car,
                    source,
                })?,
        );
    }
    let runtimes: [C3dResourceScaledObject; CHAMPIONSHIP_CAR_COUNT] = runtime_values
        .try_into()
        .expect("one runtime was built for every canonical car");
    let catalog = prepare_software_frontend_c3d_bindings(&frontend)
        .map_err(SoftwareChampionshipSessionBuildError::C3dBindings)?;
    let mut executor = ChampionshipCarBatchExecutor::new(&catalog);

    let StartupDisplay {
        display,
        copy: startup_copy,
    } = startup;
    let target = display.frame_target();
    let mut backend = display.into_backend();
    let bootstrap_palette = *backend
        .active_palette()
        .ok_or(SoftwareChampionshipSessionBuildError::MissingBootstrapPalette)?;
    let bootstrap_legacy_palette = LegacyPalette256 {
        entries: bootstrap_palette,
    };
    let logo_surfaces = prepare_championship_logo_surfaces(
        &championship,
        &bootstrap_legacy_palette,
        request.logo_mapping,
    )
    .map_err(SoftwareChampionshipSessionBuildError::LogoSurfaces)?;
    let frame_surfaces = prepare_championship_frame_surfaces(
        &frame_assets,
        &bootstrap_legacy_palette,
        request.info_mapping,
    )
    .map_err(SoftwareChampionshipSessionBuildError::FrameSurfaces)?;
    validate_raster_selection(&request.raster_selection)?;
    backend
        .configure_championship_car_raster_profile(request.raster_selection.raster_profile)
        .map_err(SoftwareChampionshipSessionBuildError::RasterProfile)?;
    let mut sources = UploadedSourceStore::new(&mut backend);
    let uploaded_fonts = UploadedFontPool::upload(&mut sources, fonts)
        .map_err(SoftwareChampionshipSessionBuildError::FontUpload)?;
    let uploaded_logos = upload_championship_logos(&mut sources, logo_surfaces)
        .map_err(SoftwareChampionshipSessionBuildError::LogoUpload)?;
    let uploaded_frames = upload_championship_frame_surfaces(&mut sources, frame_surfaces)
        .map_err(SoftwareChampionshipSessionBuildError::FrameUpload)?;

    let mut palette_arena = Vec::with_capacity(request.palette_layout.palette_count());
    palette_arena.extend(CanonicalCar::ALL.map(|car| championship.car(car).palette.entries));
    palette_arena.push(frame_assets.chequered_palette.entries);
    debug_assert_eq!(palette_arena.len(), request.palette_layout.palette_count());
    let mut palette_state = DeferredPaletteState::new();
    let mut renderer_palette_cache = bootstrap_palette;
    let mut global_palette_cache = bootstrap_palette;
    let mut preview_cache = ChampionshipPreviewCache::new();
    let mut car_transform = ChampionshipCarState::new();
    let mut car_visibility = ChampionshipCarVisibilityLatches::new();
    let mut root_visibility = RootMenuC3dVisibilityLatches::new();
    let route = FrontendC3dRouteState::new();

    let output = {
        let mut session = SoftwareChampionshipSession {
            request,
            target,
            sources: &mut sources,
            text: &text,
            fonts: &uploaded_fonts,
            logos: &uploaded_logos,
            frames: &uploaded_frames,
            championship: &championship,
            root: &root,
            frontend: &frontend,
            topologies: &topologies,
            runtimes: &runtimes,
            executor: &mut executor,
            preview_cache: &mut preview_cache,
            car_transform: &mut car_transform,
            car_visibility: &mut car_visibility,
            root_visibility: &mut root_visibility,
            route: &route,
            palette_arena: &palette_arena,
            palette_state: &mut palette_state,
            renderer_palette_cache: &mut renderer_palette_cache,
            global_palette_cache: &mut global_palette_cache,
        };
        run(&mut session)
    };
    drop(sources);

    Ok(SoftwareChampionshipSessionRun {
        output,
        backend,
        startup_copy,
    })
}

#[cfg(test)]
mod tests {
    use cmr_core::deferred_palette::resolve_palette_index;

    use super::*;

    const SUPPORTED: SoftwareChampionshipCarRasterProfile =
        SoftwareChampionshipCarRasterProfile::MUTATION_DISABLED_ACTOR_RGB_DISABLED_UNIFIED_INDEXED_COPY_NO_BLEND;
    const ACTOR_RGB_SUPPORTED: SoftwareChampionshipCarRasterProfile =
        SoftwareChampionshipCarRasterProfile::MUTATION_DISABLED_ACTOR_RGB_ENABLED_UNIFIED_INDEXED_COPY_NO_BLEND;

    fn one_actor_rgb_per_car() -> [Box<[[u8; 3]]>; CHAMPIONSHIP_CAR_COUNT] {
        std::array::from_fn(|index| {
            vec![[index as u8, index as u8 + 1, index as u8 + 2]].into_boxed_slice()
        })
    }

    #[test]
    fn request_retains_explicit_profile_and_source_owned_palette_provenance() {
        let request = SoftwareChampionshipSessionRequest::new(
            Language::French,
            VisualVariant::GPrefixed,
            SoftwareChampionshipRasterSelection::caller_asserted(
                SUPPORTED,
                C3dType2FogState::DISABLED,
            ),
            PaletteMapping::Identity,
            PaletteMapping::NearestNonzero,
        );

        assert_eq!(request.language, Language::French);
        assert_eq!(request.variant, VisualVariant::GPrefixed);
        assert_eq!(request.raster_selection.raster_profile, SUPPORTED);
        assert_eq!(
            request.raster_selection.provenance,
            SoftwareChampionshipRasterSelectionProvenance::CallerAssertedRasterProfile
        );
        assert_eq!(request.logo_mapping, PaletteMapping::Identity);
        assert_eq!(request.info_mapping, PaletteMapping::NearestNonzero);
        assert_eq!(
            request.palette_layout,
            SoftwareChampionshipPaletteLayout::CanonicalCarsThenChequered
        );
        assert_eq!(request.palette_layout.palette_count(), 15);
    }

    #[test]
    fn request_retains_explicit_per_car_actor_rgb_tables_and_provenance() {
        let actor_rgb_by_car = one_actor_rgb_per_car();
        let expected = actor_rgb_by_car.clone();
        let request = SoftwareChampionshipSessionRequest::new(
            Language::English,
            VisualVariant::Normal,
            SoftwareChampionshipRasterSelection::caller_asserted_with_actor_rgb(
                ACTOR_RGB_SUPPORTED,
                C3dType2FogState::DISABLED,
                actor_rgb_by_car,
            ),
            PaletteMapping::Identity,
            PaletteMapping::Identity,
        );

        assert_eq!(request.raster_selection.raster_profile, ACTOR_RGB_SUPPORTED);
        assert_eq!(
            request.raster_selection.provenance,
            SoftwareChampionshipRasterSelectionProvenance::CallerAssertedRasterProfile
        );
        assert_eq!(
            request.raster_selection.actor_rgb,
            SoftwareChampionshipActorRgbSelection::Enabled(Box::new(expected))
        );
        assert_eq!(
            request
                .raster_selection
                .actor_rgb
                .for_car(CanonicalCar::Ibiza)
                .rgb(0),
            Some([1, 2, 3])
        );
        let selected = request
            .raster_selection
            .pre_draw_state_for_car(CanonicalCar::Ibiza);
        assert_eq!(selected.type2_fog, C3dType2FogState::DISABLED);
        assert_eq!(selected.actor_rgb.rgb(0), Some([1, 2, 3]));
    }

    #[test]
    fn actor_rgb_count_validation_accepts_matching_tables_and_reports_first_car_mismatch() {
        let matching =
            SoftwareChampionshipActorRgbSelection::Enabled(Box::new(one_actor_rgb_per_car()));
        assert!(validate_actor_rgb_counts(&matching, |_| 1).is_ok());

        let mut mismatched = one_actor_rgb_per_car();
        mismatched[CanonicalCar::Ibiza.index() as usize] = Vec::new().into_boxed_slice();
        let error = validate_actor_rgb_counts(
            &SoftwareChampionshipActorRgbSelection::Enabled(Box::new(mismatched)),
            |_| 1,
        )
        .unwrap_err();

        assert!(matches!(
            error,
            SoftwareChampionshipSessionBuildError::ActorRgbCount {
                car: CanonicalCar::Ibiza,
                expected: 1,
                actual: 0,
            }
        ));
    }

    #[test]
    fn raster_selection_validation_accepts_matching_actor_rgb_and_rejects_both_mismatch_directions()
    {
        let enabled = SoftwareChampionshipRasterSelection::caller_asserted_with_actor_rgb(
            ACTOR_RGB_SUPPORTED,
            C3dType2FogState::DISABLED,
            one_actor_rgb_per_car(),
        );
        assert!(validate_raster_selection(&enabled).is_ok());

        let profile_enabled_state_disabled = SoftwareChampionshipRasterSelection::caller_asserted(
            ACTOR_RGB_SUPPORTED,
            C3dType2FogState::DISABLED,
        );
        assert!(matches!(
            validate_raster_selection(&profile_enabled_state_disabled),
            Err(SoftwareChampionshipSessionBuildError::RasterProfile(
                SoftwareC3dBackendError::ChampionshipCarActorRgbStateMismatch {
                    profile_enabled: true,
                    prepared_enabled: false,
                }
            ))
        ));

        let profile_disabled_state_enabled =
            SoftwareChampionshipRasterSelection::caller_asserted_with_actor_rgb(
                SUPPORTED,
                C3dType2FogState::DISABLED,
                one_actor_rgb_per_car(),
            );
        assert!(matches!(
            validate_raster_selection(&profile_disabled_state_enabled),
            Err(SoftwareChampionshipSessionBuildError::RasterProfile(
                SoftwareC3dBackendError::ChampionshipCarActorRgbStateMismatch {
                    profile_enabled: false,
                    prepared_enabled: true,
                }
            ))
        ));
    }

    #[test]
    fn software_oracle_policy_is_named_and_does_not_masquerade_as_observed_state() {
        let retained = SoftwareChampionshipRasterSelection::supported_software_oracle_policy();

        assert_eq!(retained.raster_profile, SUPPORTED);
        assert_eq!(
            retained.provenance,
            SoftwareChampionshipRasterSelectionProvenance::SupportedSoftwareOraclePolicy
        );
    }

    #[test]
    fn compact_palette_layout_maps_every_emitted_request_inside_its_arena() {
        let layout = SoftwareChampionshipPaletteLayout::CanonicalCarsThenChequered;
        let bases = layout.group_bases();

        for car in CanonicalCar::ALL {
            assert_eq!(
                resolve_palette_index(i32::from(car.index()), 4, bases).get(),
                i32::from(car.index())
            );
        }
        assert_eq!(
            resolve_palette_index(NEXT_PREVIEW_PALETTE_ITEM, 1, bases).get(),
            CHEQUERED_PALETTE_ARENA_INDEX
        );
        assert!(usize::try_from(CHEQUERED_PALETTE_ARENA_INDEX).unwrap() < layout.palette_count());
    }
}
