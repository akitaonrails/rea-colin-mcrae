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

pub struct SoftwareRootMenuSession<'session, 'asset, 'catalog, 'backend> {
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
    palette_arena: &'session [[PaletteEntry; 256]; 1],
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
    /// backdrop and selection bitmaps and palette state.
    pub fn render_screen(
        &mut self,
        commands: &[cmr_core::frontend_screen_frame::ScreenCommand],
        screen_bitmaps: &cmr_core::frontend_screen_bitmaps::ScreenBitmapPool,
    ) -> Result<(), String> {
        cmr_core::frontend_screen_frame::execute_screen_frame(
            commands,
            self.fonts,
            self.bitmaps,
            screen_bitmaps,
            RootFramePaletteResources {
                bits_per_pixel: self.target.mode.bits_per_pixel,
                arena: self.palette_arena,
                state: self.palette_state,
                renderer_cache: self.renderer_palette_cache,
                global_cache: self.global_palette_cache,
            },
            self.sources,
            self.target.back,
            self.target.mode.extent,
            self.text_layout_limits,
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
pub fn with_software_root_menu_session<R>(
    store: &AssetStore,
    startup: StartupDisplay<SoftwareIndexedBackend>,
    request: SoftwareRootMenuSessionRequest,
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
    let mut sources = UploadedSourceStore::new(&mut backend);
    let uploaded_fonts = UploadedFontPool::upload(&mut sources, fonts)
        .map_err(SoftwareRootMenuSessionBuildError::FontUpload)?;
    let uploaded_bitmaps = upload_root_menu_bitmaps(&mut sources, prepared_bitmaps)
        .map_err(SoftwareRootMenuSessionBuildError::BitmapUpload)?;
    let bitmap_diagnostics = uploaded_bitmaps.optional_diagnostics;
    let uploaded_bitmaps = uploaded_bitmaps.pool;

    let palette_arena = [disk_palette.entries];
    let mut palette_state = DeferredPaletteState::new();
    let palette_request = palette_state.request(ROOT_MENU_DISK_PALETTE_INDEX);
    debug_assert_eq!(palette_request, PaletteRequestOutcome::PendingUpdated);
    let mut renderer_palette_cache = bootstrap_palette;
    let mut global_palette_cache = bootstrap_palette;

    let output = {
        let mut session = SoftwareRootMenuSession {
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
