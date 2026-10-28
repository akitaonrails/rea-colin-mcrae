//! Scoped compatibility assembly for the software-rendered root menu.
//!
//! Several recovered resource layers intentionally borrow their source arena or
//! backend owner. This module keeps those owners as ordinary stack locals and
//! exposes a borrowing facade only for the duration of a caller callback. It
//! therefore needs neither a self-referential owner nor a native host backend.

use std::fmt;

use cmr_assets::{AssetStore, AssetStoreError};
use cmr_core::deferred_palette::{DeferredPaletteState, PaletteArenaIndex, PaletteRequestOutcome};
use cmr_core::display::{BackendFailure, IndexedFrameTarget};
use cmr_core::font_assets::{FontLoadError, load_font_set};
use cmr_core::font_raster::{FontPoolUploadError, UploadedFontPool};
use cmr_core::frontend_c3d_route::FrontendC3dRouteState;
use cmr_core::localization::Language;
use cmr_core::localization_assets::{LocalizationLoadError, load_frontend_text_table};
pub use cmr_core::root_menu_assets::ROOT_MENU_DISK_PALETTE_PATH;
use cmr_core::root_menu_assets::VisualVariant;
use cmr_core::root_menu_bitmap_assets::{
    RootMenuBitmapLoadError, RootMenuBitmapSlot, load_root_menu_bitmaps,
};
use cmr_core::root_menu_bitmap_raster::{
    RootMenuBitmapUploadDiagnostic, RootMenuBitmapUploadError, RootMenuSurfaceError,
    UploadedRootMenuBitmapPool, prepare_root_menu_surfaces, upload_root_menu_bitmaps,
};
use cmr_core::root_menu_c3d_assets::{
    RootMenuC3dAngles, RootMenuC3dLoadError, RootMenuC3dSet, load_root_menu_c3d,
};
use cmr_core::root_menu_c3d_execute::{
    RootMenuC3dBatchExecutor, RootMenuC3dBindingBuildError, RootMenuC3dExecuteError,
};
use cmr_core::root_menu_c3d_frame_plan::RootMenuC3dFramePlanError;
use cmr_core::root_menu_c3d_runtime::{
    RootMenuC3dMode2LoadStates, RootMenuC3dMode2MaterializationError,
    RootMenuC3dMode2ResourceScaledRuntime, materialize_root_menu_mode2_resource_scaled,
};
use cmr_core::root_menu_c3d_scene::{
    PreparedRootMenuC3dPipeline, PreparedRootMenuC3dPipelineError, RootMenuC3dDepthClear,
    RootMenuC3dSceneError,
};
use cmr_core::root_menu_c3d_topology::{
    RootMenuC3dTopology, RootMenuC3dTopologyError, prepare_root_menu_c3d_topology,
};
use cmr_core::root_menu_c3d_visibility::RootMenuC3dVisibilityLatches;
use cmr_core::root_menu_frame::{
    RootFrameError, RootFrameOutcome, RootFramePaletteResources, RootFrameResources,
    execute_root_frame,
};
use cmr_core::root_menu_visual::RootRenderInput;
use cmr_core::source_store::UploadedSourceStore;
use cmr_core::startup_display::StartupDisplay;
use cmr_formats::bitmap::BitmapLimits;
use cmr_formats::c3d::C3dLimits;
use cmr_formats::font::{FontLimits, TextLayoutLimits};
use cmr_formats::riff_palette::{PaletteEntry, PaletteError, parse_legacy_palette_256};

use crate::c3d_software::{
    SoftwareC3dSampledTexture, SoftwareC3dSamplingError, prepare_software_root_c3d_bindings,
};
use crate::{
    SoftwareC3dBackendError, SoftwareIndexedBackend, SoftwarePresentationError, SoftwareVideoError,
    SurfaceHandle,
};

pub const ROOT_MENU_DISK_PALETTE_INDEX: PaletteArenaIndex = PaletteArenaIndex::new(0);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SoftwareRootMenuSessionRequest {
    pub language: Language,
    pub variant: VisualVariant,
    pub bitmap_limits: BitmapLimits,
    pub font_limits: FontLimits,
    pub c3d_limits: C3dLimits,
    pub text_layout_limits: TextLayoutLimits,
    pub c3d_load_states: RootMenuC3dMode2LoadStates,
    pub depth_clear: RootMenuC3dDepthClear,
}

impl SoftwareRootMenuSessionRequest {
    #[must_use]
    pub fn new(
        language: Language,
        variant: VisualVariant,
        c3d_load_states: RootMenuC3dMode2LoadStates,
    ) -> Self {
        Self {
            language,
            variant,
            bitmap_limits: BitmapLimits::default(),
            font_limits: FontLimits::default(),
            c3d_limits: C3dLimits::default(),
            text_layout_limits: TextLayoutLimits::default(),
            c3d_load_states,
            depth_clear: RootMenuC3dDepthClear::Clear,
        }
    }

    /// Builds the narrowly scoped initial-session request recovered for the
    /// reviewed official-EU data, software renderer, and fresh default mutable
    /// configuration.
    #[must_use]
    pub fn official_eu_initial_software_default_config(
        language: Language,
        variant: VisualVariant,
    ) -> Self {
        Self::new(
            language,
            variant,
            RootMenuC3dMode2LoadStates::official_eu_initial_software_default_config(),
        )
    }
}

#[cfg(test)]
mod request_tests {
    use cmr_core::root_menu_c3d_assets::RootMenuC3dSlot;

    use super::*;

    #[test]
    fn official_eu_initial_software_request_selects_recovered_load_states() {
        let request = SoftwareRootMenuSessionRequest::official_eu_initial_software_default_config(
            Language::French,
            VisualVariant::GPrefixed,
        );

        assert_eq!(request.language, Language::French);
        assert_eq!(request.variant, VisualVariant::GPrefixed);
        assert_eq!(
            RootMenuC3dSlot::ALL
                .map(|slot| { request.c3d_load_states.get(slot).prepared_resource_count }),
            [55, 63, 63, 69, 73, 79],
        );
    }
}

#[derive(Debug)]
pub enum SoftwareRootMenuSessionBuildError {
    PaletteAsset(AssetStoreError),
    PaletteDecode(PaletteError),
    FrontendText(LocalizationLoadError),
    Fonts(FontLoadError),
    Bitmaps(RootMenuBitmapLoadError),
    BitmapSurfaces(RootMenuSurfaceError),
    C3dTopology(RootMenuC3dTopologyError),
    C3dRuntime(RootMenuC3dMode2MaterializationError),
    C3dBindings(RootMenuC3dBindingBuildError<SoftwareC3dSamplingError>),
    BootstrapPalette(BackendFailure<SoftwareVideoError>),
    FontUpload(FontPoolUploadError<SoftwareVideoError>),
    BitmapUpload(RootMenuBitmapUploadError<SoftwareVideoError>),
}

impl fmt::Display for SoftwareRootMenuSessionBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PaletteAsset(source) => write!(
                formatter,
                "cannot load root-menu palette {ROOT_MENU_DISK_PALETTE_PATH}: {source}"
            ),
            Self::PaletteDecode(source) => write!(
                formatter,
                "cannot decode root-menu palette {ROOT_MENU_DISK_PALETTE_PATH}: {source}"
            ),
            Self::FrontendText(source) => source.fmt(formatter),
            Self::Fonts(source) => source.fmt(formatter),
            Self::Bitmaps(source) => source.fmt(formatter),
            Self::BitmapSurfaces(source) => source.fmt(formatter),
            Self::C3dTopology(source) => source.fmt(formatter),
            Self::C3dRuntime(source) => source.fmt(formatter),
            Self::C3dBindings(source) => source.fmt(formatter),
            Self::BootstrapPalette(source) => {
                write!(
                    formatter,
                    "cannot upload root-menu bootstrap palette: {source}"
                )
            }
            Self::FontUpload(source) => source.fmt(formatter),
            Self::BitmapUpload(source) => source.fmt(formatter),
        }
    }
}

impl std::error::Error for SoftwareRootMenuSessionBuildError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::PaletteAsset(source) => Some(source),
            Self::PaletteDecode(source) => Some(source),
            Self::FrontendText(source) => Some(source),
            Self::Fonts(source) => Some(source),
            Self::Bitmaps(source) => Some(source),
            Self::BitmapSurfaces(source) => Some(source),
            Self::C3dTopology(source) => Some(source),
            Self::C3dRuntime(source) => Some(source),
            Self::C3dBindings(source) => Some(source),
            Self::BootstrapPalette(source) => Some(source),
            Self::FontUpload(source) => Some(source),
            Self::BitmapUpload(source) => Some(source),
        }
    }
}

pub type SoftwareRootFrameError = RootFrameError<
    SoftwareVideoError,
    RootMenuC3dFramePlanError,
    RootMenuC3dSceneError<
        SoftwareC3dBackendError,
        RootMenuC3dExecuteError<SoftwareC3dBackendError>,
    >,
    SoftwarePresentationError,
>;

#[derive(Debug)]
pub enum SoftwareRootMenuRenderError {
    LanguageMismatch {
        expected: Language,
        actual: Language,
    },
    ExtentMismatch {
        expected: cmr_core::display::DisplayExtent,
        actual: cmr_core::display::DisplayExtent,
    },
    Pipeline(PreparedRootMenuC3dPipelineError),
    Frame(Box<SoftwareRootFrameError>),
}

impl fmt::Display for SoftwareRootMenuRenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LanguageMismatch { expected, actual } => write!(
                formatter,
                "root frame language {actual:?} does not match session language {expected:?}"
            ),
            Self::ExtentMismatch { expected, actual } => write!(
                formatter,
                "root frame extent {actual:?} does not match session extent {expected:?}"
            ),
            Self::Pipeline(source) => source.fmt(formatter),
            Self::Frame(source) => source.fmt(formatter),
        }
    }
}

impl std::error::Error for SoftwareRootMenuRenderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Pipeline(source) => Some(source),
            Self::Frame(source) => Some(source),
            Self::LanguageMismatch { .. } | Self::ExtentMismatch { .. } => None,
        }
    }
}

pub struct SoftwareRootMenuSessionRun<R> {
    pub output: R,
    pub backend: SoftwareIndexedBackend,
    pub startup_copy: cmr_core::display::StartupCopyOutcome<SoftwareVideoError>,
}

/// The front end's 3D cars (`DAT_00d3cec4[0..14)`, `FUN_004c4d30`): the
/// fourteen car objects, their batch executor and the turning state.
struct FrontendCarLayer<'session, 'asset, 'catalog> {
    topologies: &'session [cmr_core::c3d_topology::C3dObjectTopology<'asset>;
                  cmr_core::championship_assets::CHAMPIONSHIP_CAR_COUNT],
    runtimes: &'session [cmr_core::root_menu_c3d_runtime::C3dResourceScaledObject;
                  cmr_core::championship_assets::CHAMPIONSHIP_CAR_COUNT],
    executor: &'session mut cmr_core::championship_car_execute::ChampionshipCarBatchExecutor<
        'catalog,
        SoftwareC3dSampledTexture,
    >,
    state: &'session mut cmr_core::championship_car_transform::ChampionshipCarState,
    raster: crate::software_championship_session::SoftwareChampionshipRasterSelection,
}

pub struct SoftwareRootMenuSession<'session, 'asset, 'catalog, 'backend> {
    car: Option<FrontendCarLayer<'session, 'asset, 'catalog>>,
    language: Language,
    target: IndexedFrameTarget<SurfaceHandle>,
    text_layout_limits: TextLayoutLimits,
    depth_clear: RootMenuC3dDepthClear,
    sources: &'session mut UploadedSourceStore<'backend, SoftwareIndexedBackend>,
    text: &'session cmr_core::localization::IndexedTextTable,
    fonts: &'session UploadedFontPool,
    bitmaps: &'session UploadedRootMenuBitmapPool,
    bitmap_diagnostics: &'session [RootMenuBitmapUploadDiagnostic<SoftwareVideoError>],
    c3d_assets: &'session RootMenuC3dSet,
    topology: &'session RootMenuC3dTopology<'asset>,
    runtime: &'session RootMenuC3dMode2ResourceScaledRuntime,
    angles: &'session mut RootMenuC3dAngles,
    visibility: &'session mut RootMenuC3dVisibilityLatches,
    executor: &'session mut RootMenuC3dBatchExecutor<'catalog, SoftwareC3dSampledTexture>,
    route_state: &'session FrontendC3dRouteState,
    palette_arena: &'session [[PaletteEntry; 256]],
    palette_state: &'session mut DeferredPaletteState,
    renderer_palette_cache: &'session mut [PaletteEntry; 256],
    global_palette_cache: &'session mut [PaletteEntry; 256],
}

impl SoftwareRootMenuSession<'_, '_, '_, '_> {
    pub fn render(
        &mut self,
        input: RootRenderInput,
    ) -> Result<RootFrameOutcome<SoftwareVideoError>, SoftwareRootMenuRenderError> {
        if input.language != self.language {
            return Err(SoftwareRootMenuRenderError::LanguageMismatch {
                expected: self.language,
                actual: input.language,
            });
        }
        if input.viewport_extent != self.target.mode.extent {
            return Err(SoftwareRootMenuRenderError::ExtentMismatch {
                expected: self.target.mode.extent,
                actual: input.viewport_extent,
            });
        }
        let pipeline = PreparedRootMenuC3dPipeline::new(
            self.topology,
            self.runtime,
            self.angles,
            self.visibility,
            self.executor,
            self.route_state,
            self.depth_clear,
        )
        .map_err(SoftwareRootMenuRenderError::Pipeline)?;
        execute_root_frame(
            input,
            RootFrameResources {
                text: self.text,
                fonts: self.fonts,
                bitmaps: self.bitmaps,
                c3d: pipeline,
            },
            RootFramePaletteResources {
                bits_per_pixel: self.target.mode.bits_per_pixel,
                arena: self.palette_arena,
                state: self.palette_state,
                renderer_cache: self.renderer_palette_cache,
                global_cache: self.global_palette_cache,
            },
            self.sources,
            self.target.back,
            self.text_layout_limits,
        )
        .map_err(|source| SoftwareRootMenuRenderError::Frame(Box::new(source)))
    }

    /// Draw one front-end sub-screen frame (`FUN_004c1bc0` with a setup
    /// menu current) from its planned commands, with this session's fonts,
    /// backdrop and selection bitmaps and palette state; a planned car
    /// ([`cmr_core::frontend_screen_frame::ScreenCommand::Car`]) turns one
    /// step and draws in its viewport between the blits and the text, as the
    /// frame's C3D pass does. `keys` are F9 and F10 held (they tilt the car).
    pub fn render_screen(
        &mut self,
        commands: &[cmr_core::frontend_screen_frame::ScreenCommand],
        screen_bitmaps: &cmr_core::frontend_screen_bitmaps::ScreenBitmapPool,
        images: &cmr_core::frontend_screen_bitmaps::ScreenImagePool,
        keys: cmr_core::championship_car_transform::ChampionshipCarInput,
    ) -> Result<(), String> {
        let extent = self.target.mode.extent;
        let depth_clear = self.depth_clear;
        let route = self.route_state;
        let planned = commands.iter().find_map(|command| match command {
            cmr_core::frontend_screen_frame::ScreenCommand::Car {
                car,
                logo,
                viewport,
            } => Some((*car, *logo, *viewport)),
            _ => None,
        });
        let car_layer = self.car.as_mut();
        cmr_core::frontend_screen_frame::execute_screen_frame_with(
            commands,
            self.fonts,
            self.bitmaps,
            screen_bitmaps,
            images,
            RootFramePaletteResources {
                bits_per_pixel: self.target.mode.bits_per_pixel,
                arena: self.palette_arena,
                state: self.palette_state,
                renderer_cache: self.renderer_palette_cache,
                global_cache: self.global_palette_cache,
            },
            self.sources,
            self.target.back,
            extent,
            self.text_layout_limits,
            |sources, destination| {
                let (Some(layer), Some((car, logo, viewport))) = (car_layer, planned) else {
                    return Ok(());
                };
                draw_frontend_car(
                    layer,
                    route,
                    sources,
                    destination,
                    extent,
                    depth_clear,
                    car,
                    logo,
                    viewport,
                    keys,
                )
            },
        )
        .map_err(|error| error.to_string())
    }

    /// Load and upload the sub-screens' bitmaps over the disk palette.
    pub fn load_screen_bitmaps(
        &mut self,
        store: &AssetStore,
    ) -> cmr_core::frontend_screen_bitmaps::ScreenBitmapPool {
        let palette = cmr_formats::riff_palette::LegacyPalette256 {
            entries: self.palette_arena[0],
        };
        cmr_core::frontend_screen_bitmaps::ScreenBitmapPool::load(store, &palette, self.sources)
    }

    /// Load and upload the flag frames (`FUN_004c4ad0`) and the car logos
    /// over the disk palette (identity mapping: each is drawn while its own
    /// palette is current).
    pub fn load_screen_images(
        &mut self,
        store: &AssetStore,
        flag_prefix: &[u8],
        flag_groups: &[Vec<u8>],
        flag_format: &[u8],
    ) -> cmr_core::frontend_screen_bitmaps::ScreenImagePool {
        let palette = cmr_formats::riff_palette::LegacyPalette256 {
            entries: self.palette_arena[0],
        };
        cmr_core::frontend_screen_bitmaps::ScreenImagePool::load(
            store,
            &palette,
            self.sources,
            flag_prefix,
            flag_groups,
            flag_format,
        )
    }

    /// The size of the palette arena.
    #[must_use]
    pub fn palette_count(&self) -> usize {
        self.palette_arena.len()
    }

    /// The fonts the session uploaded.
    #[must_use]
    pub fn fonts(&self) -> &cmr_core::font_raster::UploadedFontPool {
        self.fonts
    }

    /// The front-end text table.
    #[must_use]
    pub fn text(&self) -> &cmr_core::localization::IndexedTextTable {
        self.text
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
    pub fn bitmap_diagnostics(&self) -> &[RootMenuBitmapUploadDiagnostic<SoftwareVideoError>] {
        self.bitmap_diagnostics
    }

    #[must_use]
    pub fn bitmap_extent(&self, slot: RootMenuBitmapSlot) -> Option<(u32, u32)> {
        self.bitmaps.extent(slot)
    }

    pub fn c3d_load_diagnostics(&self) -> impl ExactSizeIterator<Item = &RootMenuC3dLoadError> {
        self.c3d_assets.diagnostics()
    }

    #[must_use]
    pub fn c3d_preparation_diagnostic_count(&self) -> usize {
        self.topology.diagnostics().len()
    }

    #[must_use]
    pub const fn target(&self) -> IndexedFrameTarget<SurfaceHandle> {
        self.target
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
    pub fn angles(&self) -> &RootMenuC3dAngles {
        self.angles
    }

    #[must_use]
    pub fn visibility(&self) -> &RootMenuC3dVisibilityLatches {
        self.visibility
    }
}

/// Loads and retains every software root-menu resource for one callback scope.
///
/// Recovered bootstrap behavior is applied before persistent source uploads:
/// disk.pal occupies arena index zero, current starts at the native `-1`
/// sentinel, index zero is pending, and the immediate display upload uses the
/// all-zero contents of the executable's zero-filled global buffer. The first
/// terminal frame then commits the complete disk palette.
/// `FUN_004c4d30` for `car`: one turning step (F9/F10 tilt it), then the
/// frame's C3D pass in the screen's viewport; the car shows once its logo
/// has stopped moving (the visibility latch). An unknown car index draws
/// nothing.
#[allow(clippy::too_many_arguments)]
fn draw_frontend_car(
    layer: &mut FrontendCarLayer<'_, '_, '_>,
    route: &FrontendC3dRouteState,
    sources: &mut UploadedSourceStore<'_, SoftwareIndexedBackend>,
    destination: SurfaceHandle,
    extent: cmr_core::display::DisplayExtent,
    depth_clear: RootMenuC3dDepthClear,
    car: u8,
    logo: (i32, i32),
    viewport: cmr_core::root_menu_c3d_transform::C3dViewportRect,
    keys: cmr_core::championship_car_transform::ChampionshipCarInput,
) -> Result<(), String> {
    use cmr_core::championship_setup::CanonicalCar;
    let Some(&canonical) = CanonicalCar::ALL.get(usize::from(car)) else {
        return Ok(());
    };
    // The rectangles are coded for 640 x 480; one that does not fit the
    // display fails `SetViewport2`, leaving the display's viewport from the
    // last scene (inference: the result is not checked).
    let fits = viewport.x >= 0
        && viewport.y >= 0
        && i32::from(viewport.x) + i32::from(viewport.width) <= i32::from(extent.width)
        && i32::from(viewport.y) + i32::from(viewport.height) <= i32::from(extent.height);
    let viewport = if fits {
        viewport
    } else {
        cmr_core::root_menu_c3d_transform::C3dViewportRect {
            x: 0,
            y: 0,
            width: extent.width,
            height: extent.height,
        }
    };
    let index = usize::from(car);
    let plan = cmr_core::championship_car_frame_plan::plan_championship_car_frame(
        &layer.topologies[index],
        &layer.runtimes[index],
        layer.state,
        canonical,
        keys,
        [0x10000, 0, 0, 0, 0x10000, 0, 0, 0, 0x10000],
        cmr_core::championship_setup_visual::PixelPoint::new(logo.0, logo.1),
        extent,
    )
    .map_err(|error| format!("{error:?}"))?;
    let prepared = layer
        .executor
        .prepare(plan, layer.raster.pre_draw_state_for_car(canonical))
        .map_err(|error| format!("{error:?}"))?;
    if route.enter(cmr_core::frontend_c3d_route::FrontendC3dRoute::ChampionshipCar) {
        layer.executor.invalidate_bindings();
    }
    cmr_core::championship_car_scene::execute_championship_car_scene_on(
        layer.executor,
        sources,
        destination,
        extent,
        viewport,
        depth_clear,
        prepared,
    )
    .map(|_| ())
    .map_err(|error| format!("{error:?}"))
}

pub fn with_software_root_menu_session<R>(
    store: &AssetStore,
    startup: StartupDisplay<SoftwareIndexedBackend>,
    request: SoftwareRootMenuSessionRequest,
    run: impl FnOnce(&mut SoftwareRootMenuSession<'_, '_, '_, '_>) -> R,
) -> Result<SoftwareRootMenuSessionRun<R>, SoftwareRootMenuSessionBuildError> {
    with_software_root_menu_session_palettes(store, startup, request, &[], run)
}

/// [`with_software_root_menu_session`] with the rest of the front end's
/// palette arena (`FUN_004c46f0`, indices 1.. after disk.pal): each path is
/// loaded in order; one that cannot be loaded keeps disk.pal at its index so
/// the indices stay in place.
pub fn with_software_root_menu_session_palettes<R>(
    store: &AssetStore,
    startup: StartupDisplay<SoftwareIndexedBackend>,
    request: SoftwareRootMenuSessionRequest,
    palette_paths: &[String],
    run: impl FnOnce(&mut SoftwareRootMenuSession<'_, '_, '_, '_>) -> R,
) -> Result<SoftwareRootMenuSessionRun<R>, SoftwareRootMenuSessionBuildError> {
    let palette_bytes = store
        .load(ROOT_MENU_DISK_PALETTE_PATH)
        .map_err(SoftwareRootMenuSessionBuildError::PaletteAsset)?;
    let disk_palette = parse_legacy_palette_256(&palette_bytes)
        .map_err(SoftwareRootMenuSessionBuildError::PaletteDecode)?;
    let bootstrap_palette = disk_palette.entries.map(|_| PaletteEntry {
        red: 0,
        green: 0,
        blue: 0,
        flags: 0,
    });

    let text = load_frontend_text_table(store, request.language)
        .map_err(SoftwareRootMenuSessionBuildError::FrontendText)?;
    let fonts = load_font_set(store, request.font_limits)
        .map_err(SoftwareRootMenuSessionBuildError::Fonts)?;
    let bitmaps = load_root_menu_bitmaps(
        store,
        request.language,
        request.variant,
        request.bitmap_limits,
    )
    .map_err(SoftwareRootMenuSessionBuildError::Bitmaps)?;
    let prepared_bitmaps = prepare_root_menu_surfaces(bitmaps, &disk_palette)
        .map_err(SoftwareRootMenuSessionBuildError::BitmapSurfaces)?;

    let c3d_assets = load_root_menu_c3d(store, request.variant, request.c3d_limits);
    let topology = prepare_root_menu_c3d_topology(&c3d_assets, request.bitmap_limits)
        .map_err(SoftwareRootMenuSessionBuildError::C3dTopology)?;
    let runtime = materialize_root_menu_mode2_resource_scaled(&topology, request.c3d_load_states)
        .map_err(SoftwareRootMenuSessionBuildError::C3dRuntime)?;
    let catalog = prepare_software_root_c3d_bindings(&topology, request.c3d_load_states)
        .map_err(SoftwareRootMenuSessionBuildError::C3dBindings)?;
    let mut executor = RootMenuC3dBatchExecutor::new(&catalog);
    // The fourteen car objects (`DAT_00d3cec4`), when the user's data has
    // them; without them the setup screens draw no 3D car.
    let championship = cmr_core::championship_assets::load_championship_assets(
        store,
        request.c3d_limits,
        request.bitmap_limits,
    )
    .ok();
    let frontend = championship.as_ref().and_then(|championship| {
        cmr_core::frontend_c3d_resources::prepare_frontend_c3d_resources(
            championship,
            &c3d_assets,
            request.bitmap_limits,
        )
        .ok()
    });
    let car_topologies =
        championship
            .as_ref()
            .zip(frontend.as_ref())
            .and_then(|(championship, frontend)| {
                cmr_core::championship_setup::CanonicalCar::ALL
                    .into_iter()
                    .map(|car| frontend.car_topology(championship, car).ok())
                    .collect::<Option<Vec<_>>>()
                    .and_then(|topologies| topologies.try_into().ok())
            });
    let car_runtimes = frontend.as_ref().zip(car_topologies.as_ref()).and_then(
        |(frontend, topologies): (_, &[cmr_core::c3d_topology::C3dObjectTopology<'_>; 14])| {
            cmr_core::championship_setup::CanonicalCar::ALL
                .into_iter()
                .map(|car| {
                    frontend
                        .materialize_car(car, &topologies[car.index() as usize])
                        .ok()
                })
                .collect::<Option<Vec<_>>>()
                .and_then(|runtimes| runtimes.try_into().ok())
        },
    );
    let car_catalog = frontend.as_ref().and_then(|frontend| {
        crate::c3d_software::prepare_software_frontend_c3d_bindings(frontend).ok()
    });
    let mut car_executor = car_catalog
        .as_ref()
        .map(cmr_core::championship_car_execute::ChampionshipCarBatchExecutor::new);
    let mut car_state = cmr_core::championship_car_transform::ChampionshipCarState::new();
    let car_raster =
        crate::software_championship_session::SoftwareChampionshipRasterSelection::supported_software_oracle_policy();
    let mut angles = RootMenuC3dAngles::new();
    let mut visibility = RootMenuC3dVisibilityLatches::new();
    let route_state = FrontendC3dRouteState::new();

    let StartupDisplay {
        mut display,
        copy: startup_copy,
    } = startup;
    display
        .upload_palette(&bootstrap_palette)
        .map_err(SoftwareRootMenuSessionBuildError::BootstrapPalette)?;
    let target = display.frame_target();
    let mut backend = display.into_backend();
    // Surface creation resets the backend's C3D state, so the car raster
    // profile is configured after it (as the championship session does).
    let cars_ready = car_runtimes.is_some()
        && car_executor.is_some()
        && backend
            .configure_championship_car_raster_profile(car_raster.raster_profile)
            .is_ok();
    let mut sources = UploadedSourceStore::new(&mut backend);
    let uploaded_fonts = UploadedFontPool::upload(&mut sources, fonts)
        .map_err(SoftwareRootMenuSessionBuildError::FontUpload)?;
    let uploaded_bitmaps = upload_root_menu_bitmaps(&mut sources, prepared_bitmaps)
        .map_err(SoftwareRootMenuSessionBuildError::BitmapUpload)?;
    let bitmap_diagnostics = uploaded_bitmaps.optional_diagnostics;
    let uploaded_bitmaps = uploaded_bitmaps.pool;

    let mut palette_arena = vec![disk_palette.entries];
    palette_arena.extend(palette_paths.iter().map(|path| {
        store
            .load(path)
            .ok()
            .and_then(|bytes| parse_legacy_palette_256(&bytes).ok())
            .map_or(disk_palette.entries, |palette| palette.entries)
    }));
    let mut palette_state = DeferredPaletteState::new();
    let palette_request = palette_state.request(ROOT_MENU_DISK_PALETTE_INDEX);
    debug_assert_eq!(palette_request, PaletteRequestOutcome::PendingUpdated);
    let mut renderer_palette_cache = bootstrap_palette;
    let mut global_palette_cache = bootstrap_palette;

    let output = {
        let car = match (
            cars_ready,
            &car_topologies,
            &car_runtimes,
            car_executor.as_mut(),
        ) {
            (true, Some(topologies), Some(runtimes), Some(executor)) => Some(FrontendCarLayer {
                topologies,
                runtimes,
                executor,
                state: &mut car_state,
                raster: car_raster.clone(),
            }),
            _ => None,
        };
        let mut session = SoftwareRootMenuSession {
            car,
            language: request.language,
            target,
            text_layout_limits: request.text_layout_limits,
            depth_clear: request.depth_clear,
            sources: &mut sources,
            text: &text,
            fonts: &uploaded_fonts,
            bitmaps: &uploaded_bitmaps,
            bitmap_diagnostics: &bitmap_diagnostics,
            c3d_assets: &c3d_assets,
            topology: &topology,
            runtime: &runtime,
            angles: &mut angles,
            visibility: &mut visibility,
            executor: &mut executor,
            route_state: &route_state,
            palette_arena: &palette_arena,
            palette_state: &mut palette_state,
            renderer_palette_cache: &mut renderer_palette_cache,
            global_palette_cache: &mut global_palette_cache,
        };
        run(&mut session)
    };
    drop(sources);

    Ok(SoftwareRootMenuSessionRun {
        output,
        backend,
        startup_copy,
    })
}
