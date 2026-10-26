//! Platform-neutral process configuration for the native application.
//!
//! This crate deliberately does not choose a renderer or inspect the current
//! directory. The native host supplies process arguments and the optional
//! environment fallback explicitly, which keeps data selection deterministic
//! and straightforward to exercise in contract tests.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::{Path, PathBuf};

use cmr_assets::{
    AssetStore, AssetStoreError, ReviewedProfile, ReviewedProfileError,
    ReviewedSurfaceCapabilityProfile, ReviewedSurfaceProfile, ReviewedSurfaceProfileError,
    SurfaceCapabilityStore, SurfaceTableStore, SurfaceTableStoreError,
};
use cmr_core::c3d_camera::MATRIX_IDENTITY_Q16;
use cmr_core::c3d_render::VERTEXTYPE_LVERTEX;
use cmr_core::c3d_stage_scene::{StageScene, StageSceneError};
use cmr_core::camera_mode::{CameraMode, ViewGate};
use cmr_core::camera_view::ViewController;
use cmr_core::compiled_car_initializer::{CompiledControlFamilies, CompiledHalfSignState};
use cmr_core::deferred_palette::DeferredPaletteCommitOutcome;
use cmr_core::display::{CooperativeMode, DisplayExtent, IndexedMode};
use cmr_core::drive_controls::DriveResponse;
use cmr_core::drivetrain::{
    SURFACE_CAPABILITY_COUNT, SURFACE_ID_COUNT, ZERO_INITIALIZED_RUNTIME_FIFTH_CAPABILITY_TABLE,
    accumulate_effect_enables_from_extracted,
};
use cmr_core::fixed_point::{Fixed16, Vec3};
use cmr_core::frontend::{FrontendPhase, RootSelection};
use cmr_core::game_menu_flow::MenuFlowInputs;
use cmr_core::game_rng::GameLcg;
use cmr_core::game_session::SessionBand;
use cmr_core::gameplay_input::{AnalogDriver, KeyboardDriver};
use cmr_core::gameplay_session::{GameplaySession, InRaceScript};
use cmr_core::host_loop::HostClock;
use cmr_core::localization::Language;
use cmr_core::race_state::{
    CarState, Controls, FirstWheelParams, FourWheelIterationParams, FourWheelPostprocessParams,
    SecondWheelParams, SimParams, WheelPostprocessError, populate_compiled_contact_offsets,
    populate_compiled_gear_ratio, populate_force_pass_scales, populate_surface_table_factor,
};
use cmr_core::render_compose::{fade_duration_frames, fade_level_to_modulation_byte, fade_out};
use cmr_core::render_route::{RenderRoute, composes_frontend_root, presents_world_scene};
use cmr_core::root_menu_assets::VisualVariant;
use cmr_core::root_menu_interactive::RootInteraction;
use cmr_core::root_menu_visual::{
    ROOT_INITIAL_HORIZONTAL_TRANSFORM_Q16, ROOT_INITIAL_VERTICAL_TRANSFORM_Q16, RootRenderInput,
};
use cmr_core::scalar_tween::ScalarTween;
use cmr_core::settings_store::{DataPaths, SettingsError, SettingsImage};
use cmr_core::stage_checkpoints::StageProgress;
use cmr_core::startup::{StartupLanguage, StartupScreen};
use cmr_core::startup_display::{
    StartupDisplay, StartupDisplayError, StartupDisplayRequest, initialize_startup_display,
};
use cmr_core::surface::PaletteMapping;
use cmr_core::vehicle_dynamics::{
    AngularMomentumStepInput, AngularOrientationInput, ControlAxisFlags, GeometricPostprocessLane,
    SOLVER_GLOBAL_CONSTANTS, SurfaceCoefficientIndexError,
};
use cmr_core::weather::{StageWeatherEffects, particles};
use cmr_formats::bitmap::BitmapLimits;
use cmr_formats::c3d::C3dLimits;
use cmr_formats::riff_palette::PaletteEntry;
use cmr_formats::stage::CatNode;
use cmr_formats::stage::{
    C3D_EXTENSION, CAT_EXTENSION, StageBundle, StageLoadError, StageSectionSource, load_stage,
};
use cmr_formats::stored_asset::{DecodeError, DecodeLimits, decode_stored_asset};
use cmr_gpu_contract::{PersistentRawC3dGpuBackend, PersistentRawC3dIndexedBackend};
use cmr_gpu_stage::{PreparedRawStage, PreparedRawStageError, PreparedRawStageStats};
use cmr_platform_sdl3::{
    InteractiveRootError, SdlHostError, WindowedGameFrame, WindowedInput, present_software_frame,
    run_interactive_root_frame, run_windowed_game_session as run_windowed_game_session_loop,
};
use cmr_video::c3d_software_raster::SoftwareC3dIndexedQuantization;
use cmr_video::software_root_menu_session::{
    SoftwareRootMenuRenderError, SoftwareRootMenuSessionBuildError, SoftwareRootMenuSessionRequest,
    with_software_root_menu_session,
};
use cmr_video::software_stage_scene::{
    StageC3dTextureError, StageC3dTextures, StageSceneComposeError, StageSceneFrameStats,
    compose_stage_scene_frame,
};
use cmr_video::{
    SoftwareIndexedBackend, SoftwarePresentedFrame, SoftwarePresentedFrameDigest,
    SoftwareRgba8Error, SoftwareVideoError,
};
use cmr_video_wgpu::{WgpuBackendConfig, WgpuIndexedPaletteBackend};

/// Environment variable consumed by the `cmr-app` binary as a runtime-data
/// fallback.
pub const RUNTIME_DATA_ENV: &str = "CMR_RUNTIME_DATA";

/// Environment variable naming the read-only original-media (`--source`) root,
/// the portable replacement for the shipped registry `CDPath`/`HDPath` lookup.
pub const SOURCE_ENV: &str = "CMR_ORIGINAL_DIR";

const REVIEWED_PROFILE: &[u8] =
    include_bytes!("../../../profiles/installed-eu-derived-assets-v2.json");
const REVIEWED_SURFACE_CAPABILITY_PROFILE: &[u8] =
    include_bytes!("../../../profiles/official-eu-surface-capabilities-v1.json");
const SURFACE_CAPABILITY_STORE_DIRECTORY: &str = "official-eu-surface-capabilities";
const REVIEWED_SURFACE_TABLE_PROFILE: &[u8] =
    include_bytes!("../../../profiles/official-eu-surface-tables-v1.json");
const SURFACE_TABLE_STORE_DIRECTORY: &str = "official-eu-surface-coefficients";
const ROOT_EXTENT: DisplayExtent = DisplayExtent {
    width: 640,
    height: 480,
};

/// An executable operation selected on the command line.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppCommand {
    HeadlessRootFrame,
    WindowedRootFrame,
    /// Compose one software frame of a named stage from the `--source` root.
    HeadlessStageFrame,
    /// Compose and present a named stage frame in a native window.
    WindowedStageFrame,
    /// Run the recovered top-level game FSM headless (boot -> menu -> load ->
    /// in-race -> quit) with the recovered race sim as the in-race handler.
    HeadlessGameSession,
    /// Run the recovered top-level game FSM in a native window, driven by live
    /// keyboard input and presenting a named stage per the recovered render
    /// route.
    WindowedGameSession,
}

/// Which stage renderer a stage command uses.
///
/// The selected native GPU backend (wgpu, see I08/I09) is the default when a
/// device can be initialized; the established software compositor remains the
/// oracle and the fallback, and stays available explicitly for differential
/// runs and headless hosts without a GPU.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StageRendererChoice {
    /// Use the GPU backend when a device is available, otherwise software.
    #[default]
    Auto,
    /// Force the established software stage compositor.
    Software,
    /// Require the GPU backend; initialization or frame failure is an error.
    Gpu,
}

impl StageRendererChoice {
    /// The stable name used by the CLI and the path-free summaries.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Software => "software",
            Self::Gpu => "gpu",
        }
    }
}

/// Which renderer actually produced a stage frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StageRendererKind {
    Software,
    Gpu,
}

impl StageRendererKind {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Software => "software",
            Self::Gpu => "gpu",
        }
    }
}

/// Parsed command-line options before environment fallback resolution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppOptions {
    command: AppCommand,
    runtime_data: Option<PathBuf>,
    source: Option<PathBuf>,
    stage: Option<String>,
    renderer: StageRendererChoice,
}

impl AppOptions {
    /// Parses the arguments after the executable name.
    ///
    /// Runtime-data paths remain platform-native and are never converted to
    /// UTF-8. No positional arguments or implicit directories are accepted.
    pub fn parse(arguments: impl IntoIterator<Item = OsString>) -> Result<Self, ConfigError> {
        let mut arguments = arguments.into_iter();
        let mut command = None;
        let mut runtime_data = None;
        let mut source = None;
        let mut stage = None;
        let mut renderer = None;

        while let Some(argument) = arguments.next() {
            if argument == OsStr::new("--headless-root-frame") {
                if command.replace(AppCommand::HeadlessRootFrame).is_some() {
                    return Err(ConfigError::DuplicateOption("--headless-root-frame"));
                }
                continue;
            }
            if argument == OsStr::new("--windowed-root-frame") {
                if command.replace(AppCommand::WindowedRootFrame).is_some() {
                    return Err(ConfigError::DuplicateOption("--windowed-root-frame"));
                }
                continue;
            }
            if argument == OsStr::new("--headless-stage-frame") {
                if command.replace(AppCommand::HeadlessStageFrame).is_some() {
                    return Err(ConfigError::DuplicateOption("--headless-stage-frame"));
                }
                continue;
            }
            if argument == OsStr::new("--windowed-stage-frame") {
                if command.replace(AppCommand::WindowedStageFrame).is_some() {
                    return Err(ConfigError::DuplicateOption("--windowed-stage-frame"));
                }
                continue;
            }
            if argument == OsStr::new("--headless-game-session") {
                if command.replace(AppCommand::HeadlessGameSession).is_some() {
                    return Err(ConfigError::DuplicateOption("--headless-game-session"));
                }
                continue;
            }
            if argument == OsStr::new("--windowed-game-session") {
                if command.replace(AppCommand::WindowedGameSession).is_some() {
                    return Err(ConfigError::DuplicateOption("--windowed-game-session"));
                }
                continue;
            }
            if argument == OsStr::new("--renderer") {
                if renderer.is_some() {
                    return Err(ConfigError::DuplicateOption("--renderer"));
                }
                let value = arguments
                    .next()
                    .ok_or(ConfigError::MissingOptionValue("--renderer"))?;
                let value = value.into_string().map_err(ConfigError::NonUtf8Renderer)?;
                renderer = Some(match value.as_str() {
                    "auto" => StageRendererChoice::Auto,
                    "software" => StageRendererChoice::Software,
                    "gpu" => StageRendererChoice::Gpu,
                    _ => return Err(ConfigError::InvalidRenderer(value)),
                });
                continue;
            }
            if argument == OsStr::new("--stage") {
                if stage.is_some() {
                    return Err(ConfigError::DuplicateOption("--stage"));
                }
                let value = arguments
                    .next()
                    .ok_or(ConfigError::MissingOptionValue("--stage"))?;
                stage = Some(value.into_string().map_err(ConfigError::NonUtf8Stage)?);
                continue;
            }
            if argument == OsStr::new("--runtime-data") {
                if runtime_data.is_some() {
                    return Err(ConfigError::DuplicateOption("--runtime-data"));
                }
                let value = arguments
                    .next()
                    .ok_or(ConfigError::MissingOptionValue("--runtime-data"))?;
                runtime_data = Some(PathBuf::from(value));
                continue;
            }
            if argument == OsStr::new("--source") {
                if source.is_some() {
                    return Err(ConfigError::DuplicateOption("--source"));
                }
                let value = arguments
                    .next()
                    .ok_or(ConfigError::MissingOptionValue("--source"))?;
                source = Some(PathBuf::from(value));
                continue;
            }
            return Err(ConfigError::UnknownArgument(argument));
        }

        Ok(Self {
            command: command.ok_or(ConfigError::MissingCommand)?,
            runtime_data,
            source,
            stage,
            renderer: renderer.unwrap_or_default(),
        })
    }

    /// Resolves the explicit option against an injected environment fallback.
    ///
    /// An explicit option always wins, including when it is invalid. This
    /// prevents a malformed command from silently selecting unrelated data.
    pub fn resolve(
        self,
        runtime_data_environment: Option<OsString>,
        source_environment: Option<OsString>,
    ) -> Result<RuntimeConfig, ConfigError> {
        let (runtime_data, source) = if let Some(path) = self.runtime_data {
            (path, RuntimeDataSource::CommandLine)
        } else if let Some(path) = runtime_data_environment {
            (PathBuf::from(path), RuntimeDataSource::Environment)
        } else {
            return Err(ConfigError::MissingRuntimeData);
        };

        if runtime_data.as_os_str().is_empty() {
            return Err(ConfigError::EmptyRuntimeData(source));
        }

        // The media (original-data) root is optional: the headless root-frame
        // demo does not need it. An explicit option wins over the environment.
        let media = if let Some(path) = self.source {
            Some((path, RuntimeDataSource::CommandLine))
        } else {
            source_environment.map(|path| (PathBuf::from(path), RuntimeDataSource::Environment))
        };
        let media = match media {
            Some((path, media_source)) if path.as_os_str().is_empty() => {
                return Err(ConfigError::EmptySource(media_source));
            }
            other => other,
        };

        // A stage name, when supplied, must be non-empty regardless of command.
        if let Some(name) = &self.stage
            && name.is_empty()
        {
            return Err(ConfigError::EmptyStage);
        }

        // The stage commands need both a stage name and a media root to read the
        // sibling section files from; the root-frame commands need neither.
        if matches!(
            self.command,
            AppCommand::HeadlessStageFrame
                | AppCommand::WindowedStageFrame
                | AppCommand::WindowedGameSession
        ) {
            if self.stage.is_none() {
                return Err(ConfigError::MissingStage);
            }
            if media.is_none() {
                return Err(ConfigError::MissingSourceForStage);
            }
        }

        Ok(RuntimeConfig {
            command: self.command,
            runtime_data,
            source,
            media,
            stage: self.stage,
            renderer: self.renderer,
        })
    }
}

/// Complete process configuration required before a native backend can start.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeConfig {
    command: AppCommand,
    runtime_data: PathBuf,
    source: RuntimeDataSource,
    media: Option<(PathBuf, RuntimeDataSource)>,
    stage: Option<String>,
    renderer: StageRendererChoice,
}

impl RuntimeConfig {
    pub fn command(&self) -> AppCommand {
        self.command
    }

    /// The selected stage base name, if one was supplied.
    pub fn stage(&self) -> Option<&str> {
        self.stage.as_deref()
    }

    /// The selected stage renderer (`--renderer`).
    pub fn stage_renderer(&self) -> StageRendererChoice {
        self.renderer
    }

    pub fn runtime_data(&self) -> &Path {
        &self.runtime_data
    }

    pub fn runtime_data_source(&self) -> RuntimeDataSource {
        self.source
    }

    /// The read-only original-media root, if one was supplied.
    pub fn media_root(&self) -> Option<&Path> {
        self.media.as_ref().map(|(path, _)| path.as_path())
    }

    /// Where the media root came from, if one was supplied.
    pub fn media_source(&self) -> Option<RuntimeDataSource> {
        self.media.as_ref().map(|(_, source)| *source)
    }

    /// Build the portable [`DataPaths`] when a media root is available, pairing
    /// the writable runtime-data root with the read-only media root.
    pub fn data_paths(&self) -> Option<DataPaths> {
        self.media
            .as_ref()
            .map(|(media, _)| DataPaths::new(self.runtime_data.clone(), media.clone()))
    }
}

/// Where the selected runtime-data path came from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeDataSource {
    CommandLine,
    Environment,
}

/// Typed failures from parsing or resolving application configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConfigError {
    UnknownArgument(OsString),
    DuplicateOption(&'static str),
    MissingOptionValue(&'static str),
    MissingCommand,
    MissingRuntimeData,
    EmptyRuntimeData(RuntimeDataSource),
    EmptySource(RuntimeDataSource),
    /// `--stage` carried a non-UTF-8 value (stage base names are identifiers).
    NonUtf8Stage(OsString),
    /// `--stage` was supplied but empty.
    EmptyStage,
    /// The stage command was selected without a `--stage` name.
    MissingStage,
    /// The stage command was selected without a `--source` media root.
    MissingSourceForStage,
    /// `--renderer` carried a non-UTF-8 value.
    NonUtf8Renderer(OsString),
    /// `--renderer` named an unknown renderer.
    InvalidRenderer(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownArgument(argument) => {
                write!(formatter, "unknown argument {argument:?}")
            }
            Self::DuplicateOption(option) => write!(formatter, "duplicate option {option}"),
            Self::MissingOptionValue(option) => write!(formatter, "missing value for {option}"),
            Self::MissingCommand => write!(
                formatter,
                "missing command; pass --headless-root-frame, --windowed-root-frame, --headless-stage-frame, --windowed-stage-frame, or --headless-game-session"
            ),
            Self::MissingRuntimeData => write!(
                formatter,
                "missing runtime data; pass --runtime-data PATH or set {RUNTIME_DATA_ENV}"
            ),
            Self::EmptyRuntimeData(RuntimeDataSource::CommandLine) => {
                write!(formatter, "--runtime-data must not be empty")
            }
            Self::EmptyRuntimeData(RuntimeDataSource::Environment) => {
                write!(formatter, "{RUNTIME_DATA_ENV} must not be empty")
            }
            Self::EmptySource(RuntimeDataSource::CommandLine) => {
                write!(formatter, "--source must not be empty")
            }
            Self::EmptySource(RuntimeDataSource::Environment) => {
                write!(formatter, "{SOURCE_ENV} must not be empty")
            }
            Self::NonUtf8Stage(value) => {
                write!(formatter, "--stage must be valid UTF-8, got {value:?}")
            }
            Self::EmptyStage => write!(formatter, "--stage must not be empty"),
            Self::MissingStage => write!(
                formatter,
                "a stage command requires a stage; pass --stage NAME"
            ),
            Self::MissingSourceForStage => write!(
                formatter,
                "a stage command requires a media root; pass --source PATH or set {SOURCE_ENV}"
            ),
            Self::NonUtf8Renderer(value) => {
                write!(formatter, "--renderer must be valid UTF-8, got {value:?}")
            }
            Self::InvalidRenderer(value) => write!(
                formatter,
                "invalid renderer {value:?}; expected auto, software, or gpu"
            ),
        }
    }
}

impl std::error::Error for ConfigError {}

/// Parses application arguments and resolves their injected environment
/// fallback in one call.
pub fn runtime_config(
    arguments: impl IntoIterator<Item = OsString>,
    runtime_data_environment: Option<OsString>,
    source_environment: Option<OsString>,
) -> Result<RuntimeConfig, ConfigError> {
    AppOptions::parse(arguments)?.resolve(runtime_data_environment, source_environment)
}

/// The outcome of the recovered startup settings-load gate.
///
/// The loader accepts `save/cmRally.cfg` only at exactly
/// [`cmr_core::settings_store::SETTINGS_BLOB_LEN`] bytes; otherwise the shipped
/// defaults stand. This reproduces that decision over injected file bytes (the
/// actual read is the native host's), returning the validated image with its
/// runtime region cleared, or the reason the defaults are used.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StartupSettings {
    /// A valid settings image was loaded (runtime region already cleared).
    Loaded(SettingsImage),
    /// No settings file was present; the shipped defaults stand.
    Defaults,
    /// A settings file was present but rejected by the exact-length gate.
    Rejected(SettingsError),
}

/// Apply the recovered startup settings-load gate to the bytes of the config
/// file (`None` when the file is absent).
pub fn resolve_startup_settings(settings_file_bytes: Option<&[u8]>) -> StartupSettings {
    match settings_file_bytes {
        None => StartupSettings::Defaults,
        Some(bytes) => match SettingsImage::from_file_bytes(bytes) {
            Ok(mut image) => {
                image.clear_runtime_region();
                StartupSettings::Loaded(image)
            }
            Err(error) => StartupSettings::Rejected(error),
        },
    }
}

/// Stable measurements from one complete headless root-menu frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HeadlessRootFrameOutput {
    pub profile_id: String,
    pub frame_digest: SoftwarePresentedFrameDigest,
    pub bitmap_blits: usize,
    pub bitmap_no_ops: usize,
    pub footer_rendered: bool,
    pub c3d_commands: usize,
    pub scene_completed: bool,
    pub palette_committed: bool,
    pub presentation_completed: bool,
}

/// One completed compatibility frame plus the recovered frame outcome.
///
/// Native hosts consume this owned value after the platform-neutral software
/// session has finished. This keeps window-system code out of the recovered
/// render path and gives headless and interactive presenters the same source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderedRootFrameOutput {
    pub profile_id: String,
    pub frame: SoftwarePresentedFrame,
    pub bitmap_blits: usize,
    pub bitmap_no_ops: usize,
    pub footer_rendered: bool,
    pub c3d_commands: usize,
    pub scene_completed: bool,
    pub palette_committed: bool,
    pub presentation_completed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RootFrameObservations {
    bitmap_blits: usize,
    bitmap_no_ops: usize,
    footer_rendered: bool,
    c3d_commands: usize,
    scene_completed: bool,
    palette_committed: bool,
    presentation_completed: bool,
}

impl From<RenderedRootFrameOutput> for HeadlessRootFrameOutput {
    fn from(rendered: RenderedRootFrameOutput) -> Self {
        Self {
            profile_id: rendered.profile_id,
            frame_digest: rendered.frame.digest_summary(),
            bitmap_blits: rendered.bitmap_blits,
            bitmap_no_ops: rendered.bitmap_no_ops,
            footer_rendered: rendered.footer_rendered,
            c3d_commands: rendered.c3d_commands,
            scene_completed: rendered.scene_completed,
            palette_committed: rendered.palette_committed,
            presentation_completed: rendered.presentation_completed,
        }
    }
}

impl HeadlessRootFrameOutput {
    /// Produces a stable, path-free representation suitable for the CLI.
    #[must_use]
    pub fn stable_summary(&self) -> String {
        format!(
            "profile={} extent={}x{} indexed_pixels={} frame_sha256={} pixels_sha256={} palette_sha256={} bitmap_blits={} bitmap_no_ops={} footer_rendered={} c3d_commands={} scene_completed={} palette_committed={} presentation_completed={}",
            self.profile_id,
            self.frame_digest.extent.width,
            self.frame_digest.extent.height,
            self.frame_digest.indexed_pixel_count,
            self.frame_digest.canonical_frame_sha256,
            self.frame_digest.indexed_pixels_sha256,
            self.frame_digest.palette_rgb_flags_sha256,
            self.bitmap_blits,
            self.bitmap_no_ops,
            self.footer_rendered,
            self.c3d_commands,
            self.scene_completed,
            self.palette_committed,
            self.presentation_completed,
        )
    }
}

/// Which startup copy failed before the retained root session was entered.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartupCopyTarget {
    VisibleFront,
    Back,
}

/// Typed failures from the production headless root-frame boundary.
#[derive(Debug)]
pub enum HeadlessRootFrameError {
    Profile(ReviewedProfileError),
    Store(AssetStoreError),
    Startup(StartupDisplayError<SoftwareVideoError>),
    StartupCopy {
        target: StartupCopyTarget,
        source: SoftwareVideoError,
    },
    Session(SoftwareRootMenuSessionBuildError),
    Render(SoftwareRootMenuRenderError),
    MissingPresentedFrame,
}

impl HeadlessRootFrameError {
    /// Stable error category for path-free process diagnostics.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Profile(_) => "profile",
            Self::Store(_) => "store",
            Self::Startup(_) => "startup",
            Self::StartupCopy { .. } => "startup-copy",
            Self::Session(_) => "session",
            Self::Render(_) => "render",
            Self::MissingPresentedFrame => "missing-presented-frame",
        }
    }
}

impl fmt::Display for HeadlessRootFrameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Profile(source) => {
                write!(formatter, "invalid embedded reviewed profile: {source}")
            }
            Self::Store(source) => write!(formatter, "cannot open runtime-data store: {source}"),
            Self::Startup(source) => {
                write!(formatter, "cannot initialize startup display: {source}")
            }
            Self::StartupCopy { target, source } => {
                write!(formatter, "startup copy to {target:?} failed: {source}")
            }
            Self::Session(source) => write!(formatter, "cannot build root-menu session: {source}"),
            Self::Render(source) => write!(formatter, "cannot render root-menu frame: {source}"),
            Self::MissingPresentedFrame => {
                write!(
                    formatter,
                    "root-menu render completed without a presented frame"
                )
            }
        }
    }
}

impl std::error::Error for HeadlessRootFrameError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Profile(source) => Some(source),
            Self::Store(source) => Some(source),
            Self::Startup(source) => Some(source),
            Self::StartupCopy { source, .. } => Some(source),
            Self::Session(source) => Some(source),
            Self::Render(source) => Some(source),
            Self::MissingPresentedFrame => None,
        }
    }
}

/// A failure while running the native windowed root menu.
#[derive(Debug)]
pub enum WindowedRootFrameError {
    /// Session setup (profile, store, startup display, or session build).
    Setup(HeadlessRootFrameError),
    /// A failure from the interactive host loop (SDL host or per-frame render).
    Interactive(InteractiveRootError<HeadlessRootFrameError>),
}

impl WindowedRootFrameError {
    /// Stable error category for path-free process diagnostics.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Setup(source) => source.kind(),
            Self::Interactive(InteractiveRootError::Host(_)) => "host",
            Self::Interactive(InteractiveRootError::Render(source)) => source.kind(),
        }
    }
}

impl From<HeadlessRootFrameError> for WindowedRootFrameError {
    fn from(source: HeadlessRootFrameError) -> Self {
        Self::Setup(source)
    }
}

impl fmt::Display for WindowedRootFrameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Setup(source) => source.fmt(formatter),
            Self::Interactive(source) => source.fmt(formatter),
        }
    }
}

impl std::error::Error for WindowedRootFrameError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Setup(source) => Some(source),
            Self::Interactive(source) => Some(source),
        }
    }
}

/// Runs the fixed official-EU, English, normal-variant initial root frame.
pub fn run_headless_root_frame(
    runtime_data: &Path,
) -> Result<HeadlessRootFrameOutput, HeadlessRootFrameError> {
    render_root_frame(runtime_data).map(HeadlessRootFrameOutput::from)
}

/// Renders the fixed official-EU, English, normal-variant initial root frame
/// for a caller-owned presenter.
pub fn render_root_frame(
    runtime_data: &Path,
) -> Result<RenderedRootFrameOutput, HeadlessRootFrameError> {
    let reviewed =
        ReviewedProfile::from_json(REVIEWED_PROFILE).map_err(HeadlessRootFrameError::Profile)?;
    let request = SoftwareRootMenuSessionRequest::official_eu_initial_software_default_config(
        Language::English,
        VisualVariant::Normal,
    );
    render_root_frame_with_profile(runtime_data, &reviewed, request)
}

/// Test seam for an injected reviewed profile and compatible session request.
///
/// Display mode, startup art, root selection, language, and transforms remain
/// fixed to the production one-frame contract.
pub fn run_headless_root_frame_with_profile(
    runtime_data: &Path,
    reviewed: &ReviewedProfile,
    request: SoftwareRootMenuSessionRequest,
) -> Result<HeadlessRootFrameOutput, HeadlessRootFrameError> {
    render_root_frame_with_profile(runtime_data, reviewed, request)
        .map(HeadlessRootFrameOutput::from)
}

/// Test seam for producing an owned completed frame from an injected profile.
pub fn render_root_frame_with_profile(
    runtime_data: &Path,
    reviewed: &ReviewedProfile,
    request: SoftwareRootMenuSessionRequest,
) -> Result<RenderedRootFrameOutput, HeadlessRootFrameError> {
    let profile_id = reviewed.profile().id.clone();
    let (store, startup) = open_root_session_inputs(runtime_data, reviewed)?;

    let mut run = with_software_root_menu_session(&store, startup, request, |session| {
        let outcome = session
            .render(initial_root_input())
            .map_err(HeadlessRootFrameError::Render)?;
        Ok(RootFrameObservations {
            bitmap_blits: outcome.bitmap_blits,
            bitmap_no_ops: outcome.bitmap_no_ops,
            footer_rendered: outcome.footer.is_some(),
            c3d_commands: outcome.c3d_commands,
            scene_completed: outcome.scene_completed,
            palette_committed: matches!(
                outcome.palette_commit,
                DeferredPaletteCommitOutcome::Committed(_)
            ),
            presentation_completed: outcome.presentation_completed,
        })
    })
    .map_err(HeadlessRootFrameError::Session)?;

    let observations = run.output?;
    let frame = run
        .backend
        .take_presented_frame()
        .ok_or(HeadlessRootFrameError::MissingPresentedFrame)?;
    Ok(RenderedRootFrameOutput {
        profile_id,
        frame,
        bitmap_blits: observations.bitmap_blits,
        bitmap_no_ops: observations.bitmap_no_ops,
        footer_rendered: observations.footer_rendered,
        c3d_commands: observations.c3d_commands,
        scene_completed: observations.scene_completed,
        palette_committed: observations.palette_committed,
        presentation_completed: observations.presentation_completed,
    })
}

/// Opens the verified asset store and the initialized loading display shared by
/// the headless and interactive root runners, rejecting either startup-copy
/// failure with a path-free boundary error.
fn open_root_session_inputs(
    runtime_data: &Path,
    reviewed: &ReviewedProfile,
) -> Result<(AssetStore, StartupDisplay<SoftwareIndexedBackend>), HeadlessRootFrameError> {
    let store = AssetStore::open(
        runtime_data,
        reviewed,
        DecodeLimits {
            max_stored_bytes: 64 * 1024 * 1024,
            max_payload_bytes: 256 * 1024 * 1024,
        },
    )
    .map_err(HeadlessRootFrameError::Store)?;
    let startup = initialize_startup_display(
        &store,
        SoftwareIndexedBackend::default(),
        StartupDisplayRequest {
            mode: IndexedMode {
                extent: ROOT_EXTENT,
                bits_per_pixel: 8,
            },
            cooperative: CooperativeMode::ExclusiveFullscreen,
            screen: StartupScreen::Loading,
            language: StartupLanguage::English,
            mapping: PaletteMapping::Identity,
            bitmap_limits: BitmapLimits::default(),
        },
    )
    .map_err(HeadlessRootFrameError::Startup)?;
    if let Err(source) = &startup.copy.front {
        return Err(HeadlessRootFrameError::StartupCopy {
            target: StartupCopyTarget::VisibleFront,
            source: source.clone(),
        });
    }
    if let Err(source) = &startup.copy.back {
        return Err(HeadlessRootFrameError::StartupCopy {
            target: StartupCopyTarget::Back,
            source: source.clone(),
        });
    }
    Ok((store, startup))
}

/// Runs the native, windowed, interactive root menu for the fixed official-EU,
/// English, normal-variant initial state.
///
/// The compatibility session stays live for the whole loop: each distinct
/// recovered render input is drawn by the software path and presented by the
/// SDL host, which owns the window, input, and gamepads.
pub fn run_windowed_root_frame(
    runtime_data: &Path,
    source_root: Option<&Path>,
) -> Result<(), WindowedRootFrameError> {
    let reviewed =
        ReviewedProfile::from_json(REVIEWED_PROFILE).map_err(HeadlessRootFrameError::Profile)?;
    let request = SoftwareRootMenuSessionRequest::official_eu_initial_software_default_config(
        Language::English,
        VisualVariant::Normal,
    );
    let (store, startup) = open_root_session_inputs(runtime_data, &reviewed)?;

    let run = with_software_root_menu_session(&store, startup, request, |session| {
        let interaction =
            RootInteraction::from_saved_selection(0, Language::English, ROOT_EXTENT, 0)
                .expect("zero is a valid recovered root selection");
        run_interactive_root_frame(interaction, true, source_root, |input| {
            session
                .render(*input)
                .map_err(HeadlessRootFrameError::Render)?;
            session
                .backend()
                .presented_frame()
                .cloned()
                .ok_or(HeadlessRootFrameError::MissingPresentedFrame)
        })
    })
    .map_err(|source| WindowedRootFrameError::Setup(HeadlessRootFrameError::Session(source)))?;

    run.output.map_err(WindowedRootFrameError::Interactive)
}

fn initial_root_input() -> RootRenderInput {
    RootRenderInput {
        phase: FrontendPhase::Interactive,
        selection: RootSelection::new(0).expect("zero is a valid recovered root selection"),
        animation_position_q16: 0,
        language: Language::English,
        viewport_extent: ROOT_EXTENT,
        horizontal_transform_q16: ROOT_INITIAL_HORIZONTAL_TRANSFORM_Q16,
        vertical_transform_q16: ROOT_INITIAL_VERTICAL_TRANSFORM_Q16,
        entry_visuals_suppressed: false,
    }
}

/// Reads a stage's sibling section files from a filesystem media root.
///
/// The recovered master loader (`FUN_004228d0`) walks a 14-extension table and
/// loads `<stagebase><ext>` for each present sibling through the shared,
/// CMPR-capable loader. This source reproduces that lookup over a real directory
/// for the sections the bundle loader consumes (`.c3d`, `.cat`): it reads
/// `<media_root>/<stage><ext>`, runs each file through [`decode_stored_asset`]
/// (so a CMPR-wrapped sibling is transparently inflated), and keeps the decoded
/// bytes. A missing sibling is simply absent; only an I/O or decode failure is an
/// error. Original media is read in place and never copied into the repository.
pub struct FsStageSectionSource {
    c3d: Option<Vec<u8>>,
    cat: Option<Vec<u8>>,
}

impl FsStageSectionSource {
    /// Reads the `.c3d` and `.cat` siblings of `stage` under `media_root`.
    pub fn read(
        media_root: &Path,
        stage: &str,
        limits: DecodeLimits,
    ) -> Result<Self, StageFrameError> {
        Ok(Self {
            c3d: read_decoded_section(media_root, stage, C3D_EXTENSION, limits)?,
            cat: read_decoded_section(media_root, stage, CAT_EXTENSION, limits)?,
        })
    }
}

impl StageSectionSource for FsStageSectionSource {
    fn section(&self, extension: &str) -> Option<&[u8]> {
        match extension {
            C3D_EXTENSION => self.c3d.as_deref(),
            CAT_EXTENSION => self.cat.as_deref(),
            _ => None,
        }
    }
}

/// Reads `<media_root>/<stage><extension>` and decodes its storage wrapper,
/// returning `None` when the sibling file is absent.
fn read_decoded_section(
    media_root: &Path,
    stage: &str,
    extension: &'static str,
    limits: DecodeLimits,
) -> Result<Option<Vec<u8>>, StageFrameError> {
    let path = media_root.join(format!("{stage}{extension}"));
    let raw = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(StageFrameError::Io { extension, error }),
    };
    let decoded = decode_stored_asset(&raw, limits)
        .map_err(|error| StageFrameError::Decode { extension, error })?;
    Ok(Some(decoded.bytes().to_vec()))
}

/// Renderer-specific measurements for one composed stage frame.
///
/// The software compositor reports its full raster statistics. The GPU path
/// reports the prepared-scene conversion counts instead: rasterization happens
/// on the device and its correctness is the shared raw-C3D contract's concern
/// (the same oracle comparison the conformance suite performs), not a per-frame
/// host counter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StageRenderStats {
    Software(StageSceneFrameStats),
    Gpu(PreparedRawStageStats),
}

/// Stable measurements from one complete headless stage frame.
///
/// The composition uses a fixed placeholder camera (world origin) and a
/// deterministic grayscale display palette: the recovered gameplay camera and
/// active palette are later-milestone concerns. This output therefore proves the
/// file -> scene -> texture-binding -> compose -> present wiring end to end and
/// reports the scene and frame statistics, with a path-free digest for
/// differential checks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HeadlessStageFrameOutput {
    pub stage: String,
    pub renderer: StageRendererKind,
    pub node_count: usize,
    pub draw_ops: usize,
    pub textures: usize,
    pub stats: StageRenderStats,
    pub frame_digest: SoftwarePresentedFrameDigest,
}

impl HeadlessStageFrameOutput {
    /// Produces a stable, path-free representation suitable for the CLI.
    #[must_use]
    pub fn stable_summary(&self) -> String {
        let stats = match self.stats {
            StageRenderStats::Software(stats) => format!(
                "prims_composed={} unresolved_prims={} empty_lod_nodes={} covered={} depth_passed={} depth_rejected={}",
                stats.prims_composed,
                stats.unresolved_prims,
                stats.empty_lod_nodes,
                stats.covered,
                stats.depth_passed,
                stats.depth_rejected,
            ),
            StageRenderStats::Gpu(stats) => format!(
                "prepared_draws={} unique_textures={} unresolved_prims={} empty_lod_nodes={}",
                stats.draws, stats.unique_textures, stats.unresolved_prims, stats.empty_lod_nodes,
            ),
        };
        format!(
            "stage={} renderer={} extent={}x{} nodes={} draw_ops={} textures={} {stats} frame_sha256={} pixels_sha256={} palette_sha256={}",
            self.stage,
            self.renderer.name(),
            self.frame_digest.extent.width,
            self.frame_digest.extent.height,
            self.node_count,
            self.draw_ops,
            self.textures,
            self.frame_digest.canonical_frame_sha256,
            self.frame_digest.indexed_pixels_sha256,
            self.frame_digest.palette_rgb_flags_sha256,
        )
    }
}

/// Typed failures from the headless stage-frame boundary.
#[derive(Debug)]
pub enum StageFrameError {
    /// A sibling section file could not be read.
    Io {
        extension: &'static str,
        error: std::io::Error,
    },
    /// A sibling section file's storage wrapper could not be decoded.
    Decode {
        extension: &'static str,
        error: DecodeError,
    },
    /// The stage bundle (its `.c3d` scene or `.cat` nodes) failed to load.
    Load(StageLoadError),
    /// The parsed scene could not be walked into a drawable stage scene.
    Scene(StageSceneError),
    /// The scene's group-2 textures could not be prepared.
    Texture(StageC3dTextureError),
    /// The stage scene could not be composed into a frame.
    Compose(StageSceneComposeError),
    /// The composed frame could not be bridged to the presentation type.
    Present(SoftwareRgba8Error),
    /// The raw-GPU stage adapter rejected the scene.
    GpuPrepare(PreparedRawStageError),
    /// The selected GPU backend could not be initialized (or its persistent
    /// scene could not be uploaded).
    GpuInit(String),
    /// The selected GPU backend failed to render or read back a frame.
    GpuCompose(String),
}

impl StageFrameError {
    /// Stable error category for path-free process diagnostics.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Io { .. } => "io",
            Self::Decode { .. } => "decode",
            Self::Load(_) => "load",
            Self::Scene(_) => "scene",
            Self::Texture(_) => "texture",
            Self::Compose(_) => "compose",
            Self::Present(_) => "present",
            Self::GpuPrepare(_) => "gpu-prepare",
            Self::GpuInit(_) => "gpu-init",
            Self::GpuCompose(_) => "gpu-compose",
        }
    }
}

impl fmt::Display for StageFrameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { extension, error } => {
                write!(formatter, "cannot read stage {extension} section: {error}")
            }
            Self::Decode { extension, error } => {
                write!(
                    formatter,
                    "cannot decode stage {extension} section: {error}"
                )
            }
            Self::Load(error) => write!(formatter, "cannot load stage bundle: {error}"),
            Self::Scene(error) => write!(formatter, "cannot walk stage scene: {error}"),
            Self::Texture(error) => write!(formatter, "cannot prepare stage textures: {error}"),
            Self::Compose(error) => write!(formatter, "cannot compose stage frame: {error}"),
            Self::Present(error) => write!(formatter, "cannot present stage frame: {error}"),
            Self::GpuPrepare(error) => write!(formatter, "cannot prepare raw GPU stage: {error}"),
            Self::GpuInit(detail) => {
                write!(
                    formatter,
                    "cannot initialize the GPU stage renderer: {detail}"
                )
            }
            Self::GpuCompose(detail) => {
                write!(formatter, "cannot compose the GPU stage frame: {detail}")
            }
        }
    }
}

impl std::error::Error for StageFrameError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { error, .. } => Some(error),
            Self::Decode { error, .. } => Some(error),
            Self::Load(error) => Some(error),
            Self::Scene(error) => Some(error),
            Self::Texture(error) => Some(error),
            Self::Compose(error) => Some(error),
            Self::Present(error) => Some(error),
            Self::GpuPrepare(error) => Some(error),
            Self::GpuInit(_) | Self::GpuCompose(_) => None,
        }
    }
}

/// The placeholder stage camera is backed this far along `-Z` from the world
/// origin (Q16.16), so origin-centred geometry projects into the viewport until
/// the recovered gameplay camera (driven by the `.cat` start node) is wired.
const STAGE_PLACEHOLDER_CAMERA_BACKOFF_Q16: i32 = 6 << 16;

/// A fixed placeholder camera world transform: the identity basis translated
/// back along `-Z` so it looks toward geometry at the world origin.
fn stage_placeholder_camera() -> [i32; 16] {
    let mut camera = MATRIX_IDENTITY_Q16;
    camera[14] = -STAGE_PLACEHOLDER_CAMERA_BACKOFF_Q16;
    camera
}

/// A deterministic grayscale display palette used as a placeholder until the
/// recovered gameplay active palette is wired.
fn stage_placeholder_palette() -> [PaletteEntry; 256] {
    let mut palette = [PaletteEntry {
        red: 0,
        green: 0,
        blue: 0,
        flags: 0,
    }; 256];
    for (index, entry) in palette.iter_mut().enumerate() {
        let level = index as u8;
        *entry = PaletteEntry {
            red: level,
            green: level,
            blue: level,
            flags: 0,
        };
    }
    palette
}

/// A composed stage frame plus its scene statistics, for a caller-owned
/// presenter (the native window host or the headless digest path).
#[derive(Clone, Debug)]
pub struct RenderedStageFrameOutput {
    pub stage: String,
    pub renderer: StageRendererKind,
    pub frame: SoftwarePresentedFrame,
    pub node_count: usize,
    pub draw_ops: usize,
    pub textures: usize,
    pub stats: StageRenderStats,
}

impl From<RenderedStageFrameOutput> for HeadlessStageFrameOutput {
    fn from(rendered: RenderedStageFrameOutput) -> Self {
        Self {
            stage: rendered.stage,
            renderer: rendered.renderer,
            node_count: rendered.node_count,
            draw_ops: rendered.draw_ops,
            textures: rendered.textures,
            stats: rendered.stats,
            frame_digest: rendered.frame.digest_summary(),
        }
    }
}

/// The persistent wgpu stage renderer: the selected native backend behind the
/// shared raw-C3D contracts.
///
/// One-time preparation converts the walkable scene into [`PreparedRawStage`]
/// (validated raw resources + the recovered draw list) and uploads it to a
/// persistent GPU scene. Each frame only supplies the camera VIEW matrix,
/// rasterizes into the 8-bit palette-index target, and reads that index plane
/// back, so the host presents it through the same recovered indexed display
/// path the software compositor feeds. The software composer remains the
/// oracle; the GPU frame is the same rendering contract, not a replacement
/// semantics.
struct WgpuStageRenderer {
    backend: WgpuIndexedPaletteBackend,
    prepared: PreparedRawStage,
    scene: <WgpuIndexedPaletteBackend as PersistentRawC3dGpuBackend>::PreparedScene,
    palette: [PaletteEntry; 256],
}

impl WgpuStageRenderer {
    fn new(
        scene: &StageScene<'_>,
        textures: &StageC3dTextures,
        extent: DisplayExtent,
        palette: &[PaletteEntry; 256],
    ) -> Result<Self, StageFrameError> {
        let prepared = PreparedRawStage::prepare(scene, textures, extent, palette)
            .map_err(StageFrameError::GpuPrepare)?;
        let mut backend = WgpuIndexedPaletteBackend::new(&WgpuBackendConfig::default())
            .map_err(|error| StageFrameError::GpuInit(error.to_string()))?;
        let prepared_scene = backend
            .prepare_c3d_scene(prepared.upload())
            .map_err(|error| StageFrameError::GpuInit(error.to_string()))?;
        Ok(Self {
            backend,
            prepared,
            scene: prepared_scene,
            palette: *palette,
        })
    }

    fn stats(&self) -> PreparedRawStageStats {
        self.prepared.stats()
    }

    /// Rasterizes one camera frame and returns it as the recovered indexed
    /// presentation value (active-palette slots + the active palette).
    fn compose(&mut self, camera: &[i32; 16]) -> Result<SoftwarePresentedFrame, StageFrameError> {
        let frame = self.prepared.prepared_frame_for(camera);
        let indexed = self
            .backend
            .render_prepared_indexed_readback(&mut self.scene, frame)
            .map_err(|error| StageFrameError::GpuCompose(error.to_string()))?;
        SoftwarePresentedFrame::from_indexed(
            indexed.extent(),
            indexed.indices().to_vec().into_boxed_slice(),
            Box::new(self.palette),
        )
        .map_err(StageFrameError::Present)
    }
}

/// A loaded stage's retained render inputs: the owned bundle and prepared
/// textures, so a caller can recompose the scene every frame with a new camera
/// (the windowed session's follow camera) without reloading.
///
/// The GPU path prepares and uploads its persistent scene once at load. When no
/// device is available, or a GPU frame fails under [`StageRendererChoice::Auto`],
/// the retained bundle keeps the software compositor available as the oracle
/// fallback. [`StageScene`] borrows the bundle, so it is rebuilt per software
/// compose from the retained `bundle` (cheap: it collects the group records);
/// the textures and palette are prepared once.
pub struct StageRenderResources {
    stage: String,
    bundle: StageBundle,
    textures: StageC3dTextures,
    palette: [PaletteEntry; 256],
    node_count: usize,
    draw_ops: usize,
    choice: StageRendererChoice,
    gpu: Option<WgpuStageRenderer>,
}

impl StageRenderResources {
    /// Loads the named stage with the default renderer policy
    /// ([`StageRendererChoice::Auto`]: GPU when available, else software).
    pub fn load(media_root: &Path, stage: &str) -> Result<Self, StageFrameError> {
        Self::load_with(media_root, stage, StageRendererChoice::Auto)
    }

    /// Loads the named stage and selects its renderer.
    ///
    /// `--source` threading is unchanged: it reads the stage's sibling sections
    /// ([`FsStageSectionSource`]), loads the bundle ([`load_stage`]), binds its
    /// group-2 textures ([`StageC3dTextures`]), and prepares the placeholder
    /// palette. The GPU path then converts the already-walked scene and uploads
    /// the persistent scene once. `Gpu` fails closed when the device or scene is
    /// unavailable; `Auto` falls back to the software compositor.
    pub fn load_with(
        media_root: &Path,
        stage: &str,
        choice: StageRendererChoice,
    ) -> Result<Self, StageFrameError> {
        let source = FsStageSectionSource::read(media_root, stage, DecodeLimits::default())?;
        let bundle = load_stage(&source, C3dLimits::default()).map_err(StageFrameError::Load)?;
        let textures = StageC3dTextures::prepare(bundle.scene(), BitmapLimits::default())
            .map_err(StageFrameError::Texture)?;
        let palette = stage_placeholder_palette();
        let scene = StageScene::from_c3d(bundle.scene(), &MATRIX_IDENTITY_Q16, VERTEXTYPE_LVERTEX)
            .map_err(StageFrameError::Scene)?;
        let node_count = scene.node_count();
        let draw_ops = scene.draw_order().len();

        let gpu = match choice {
            StageRendererChoice::Software => None,
            StageRendererChoice::Gpu => Some(WgpuStageRenderer::new(
                &scene,
                &textures,
                ROOT_EXTENT,
                &palette,
            )?),
            StageRendererChoice::Auto => {
                WgpuStageRenderer::new(&scene, &textures, ROOT_EXTENT, &palette).ok()
            }
        };

        Ok(StageRenderResources {
            stage: stage.to_owned(),
            bundle,
            textures,
            palette,
            node_count,
            draw_ops,
            choice,
            gpu,
        })
    }

    /// Which renderer will compose the next frame.
    #[must_use]
    pub fn renderer(&self) -> StageRendererKind {
        if self.gpu.is_some() {
            StageRendererKind::Gpu
        } else {
            StageRendererKind::Software
        }
    }

    /// Composes one frame of the retained stage from the given camera world
    /// transform through the selected renderer.
    ///
    /// A GPU frame failure under `Auto` disables the GPU path for the rest of the
    /// session and composes the software oracle frame instead; `Gpu` surfaces the
    /// failure.
    pub fn compose(
        &mut self,
        camera: &[i32; 16],
    ) -> Result<RenderedStageFrameOutput, StageFrameError> {
        if let Some(gpu) = self.gpu.as_mut() {
            match gpu.compose(camera) {
                Ok(frame) => {
                    return Ok(RenderedStageFrameOutput {
                        stage: self.stage.clone(),
                        renderer: StageRendererKind::Gpu,
                        frame,
                        node_count: self.node_count,
                        draw_ops: self.draw_ops,
                        textures: self.textures.len(),
                        stats: StageRenderStats::Gpu(gpu.stats()),
                    });
                }
                Err(error) => {
                    if self.choice == StageRendererChoice::Gpu {
                        return Err(error);
                    }
                    // Auto: fall back to the software oracle for this and every
                    // later frame rather than failing the session.
                    self.gpu = None;
                }
            }
        }
        self.compose_software(camera)
    }

    fn compose_software(
        &self,
        camera: &[i32; 16],
    ) -> Result<RenderedStageFrameOutput, StageFrameError> {
        let scene = StageScene::from_c3d(
            self.bundle.scene(),
            &MATRIX_IDENTITY_Q16,
            VERTEXTYPE_LVERTEX,
        )
        .map_err(StageFrameError::Scene)?;
        let frame = compose_stage_scene_frame(
            &scene,
            camera,
            [ROOT_EXTENT.width, ROOT_EXTENT.height],
            &self.palette,
            SoftwareC3dIndexedQuantization::NearestRgbL1LowestIndexNoDither,
            |ordinal| self.textures.texture_for_ordinal(ordinal),
        )
        .map_err(StageFrameError::Compose)?;

        let presented = frame
            .to_presented_frame()
            .map_err(StageFrameError::Present)?;

        Ok(RenderedStageFrameOutput {
            stage: self.stage.clone(),
            renderer: StageRendererKind::Software,
            frame: presented,
            node_count: self.node_count,
            draw_ops: self.draw_ops,
            textures: self.textures.len(),
            stats: StageRenderStats::Software(*frame.stats()),
        })
    }
}

/// Loads the named stage from the media root and composes one frame with the
/// fixed placeholder camera. The owned frame lets a headless caller take a
/// digest or a native host present it in a window.
pub fn render_stage_frame(
    media_root: &Path,
    stage: &str,
    renderer: StageRendererChoice,
) -> Result<RenderedStageFrameOutput, StageFrameError> {
    StageRenderResources::load_with(media_root, stage, renderer)?
        .compose(&stage_placeholder_camera())
}

/// Composes one stage frame headlessly and returns its path-free measurements.
pub fn run_headless_stage_frame(
    media_root: &Path,
    stage: &str,
    renderer: StageRendererChoice,
) -> Result<HeadlessStageFrameOutput, StageFrameError> {
    render_stage_frame(media_root, stage, renderer).map(HeadlessStageFrameOutput::from)
}

/// A failure while running the native windowed stage frame.
#[derive(Debug)]
pub enum WindowedStageFrameError {
    /// Loading or composing the stage frame failed.
    Setup(StageFrameError),
    /// Authenticated session input or four-wheel production failed.
    Session(SessionFourWheelError),
    /// The SDL host failed to present the composed frame.
    Present(SdlHostError),
}

impl WindowedStageFrameError {
    /// Stable error category for path-free process diagnostics.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Setup(source) => source.kind(),
            Self::Session(_) => "four-wheel-session",
            Self::Present(_) => "present-host",
        }
    }
}

impl From<StageFrameError> for WindowedStageFrameError {
    fn from(source: StageFrameError) -> Self {
        Self::Setup(source)
    }
}

impl From<SessionFourWheelError> for WindowedStageFrameError {
    fn from(source: SessionFourWheelError) -> Self {
        Self::Session(source)
    }
}

impl fmt::Display for WindowedStageFrameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Setup(source) => source.fmt(formatter),
            Self::Session(source) => write!(formatter, "four-wheel session failed: {source}"),
            Self::Present(source) => write!(formatter, "cannot present stage frame: {source}"),
        }
    }
}

impl std::error::Error for WindowedStageFrameError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Setup(source) => Some(source),
            Self::Session(source) => Some(source),
            Self::Present(source) => Some(source),
        }
    }
}

/// Composes one stage frame and presents it in a native SDL window until the
/// user closes it, reusing the same composition path as the headless command.
pub fn run_windowed_stage_frame(
    media_root: &Path,
    stage: &str,
    renderer: StageRendererChoice,
) -> Result<(), WindowedStageFrameError> {
    let rendered = render_stage_frame(media_root, stage, renderer)?;
    present_software_frame(&rendered.frame).map_err(WindowedStageFrameError::Present)
}

/// A real wall clock for the windowed session: milliseconds since the loop
/// started, so the [`FrameDriver`](cmr_core::game_driver::FrameDriver)'s 40 ms
/// catch-up buckets advance the sim in real time (lock-step 25 Hz against the
/// host's frame pace), unlike the headless [`StationaryClock`].
struct WallClock {
    start: std::time::Instant,
}

impl WallClock {
    fn new() -> Self {
        WallClock {
            start: std::time::Instant::now(),
        }
    }
}

impl HostClock for WallClock {
    fn millis(&self) -> u32 {
        self.start.elapsed().as_millis() as u32
    }
}

/// Composes the named stage once and runs the recovered top-level game session
/// in a native SDL window, driven by **live keyboard input** and presenting per
/// the recovered render route.
///
/// This threads the whole live chain end to end: SDL key events feed a
/// [`KeyboardVkState`](cmr_platform_sdl3::KeyboardVkState) that the recovered
/// keyboard driver ([`KeyboardDriver`]) turns into sim [`Controls`], which drive
/// the fixed-step race sim inside the recovered FSM ([`GameplaySession`]); the
/// loop presents the composed stage frame whenever the state's render route
/// composes the in-race world and clears otherwise. The drive-force and
/// steer-rate magnitudes are the documented harness [`DriveResponse`] (the real
/// values are runtime car/surface-table quantities), the checkpoint line is the
/// synthetic [`headless_checkpoint_line`], and the per-route recovered
/// compositors + a live follow camera remain to be wired - but the FSM, the
/// input chain, and the render-route routing are all the recovered behavior.
/// The windowed session's camera state: the recovered multi-mode view
/// controller plus the harness view-cycle binding.
///
/// The view modes, their placement math, and the cycle order are all recovered
/// behavior ([`ViewController`]/[`cmr_core::camera_view`]). What is *not*
/// recovered is the shipped camera-change input binding, so this harness cycles
/// the modes on the rising edge of a documented key (VK `0x43` / `C`, matching
/// `cmr_platform_sdl3::input::sdl_keycode_to_vk`) seeded from a fixed harness
/// seed - the same documented-stand-in pattern as the loop's synthetic drive
/// force and checkpoint line. Only the trigger key and seed are harness policy.
struct ViewHarness {
    controller: ViewController,
    rng: GameLcg,
    change_view_down: bool,
}

impl ViewHarness {
    /// VK of the harness change-view key (`C`).
    const CHANGE_VIEW_VK: u8 = 0x43;
    /// In-race view gate: both interior cockpit poses are reachable; the
    /// showroom pose is a front-end mode, not an in-race one.
    const GATE: ViewGate = ViewGate {
        cockpit_enabled: true,
        showroom_allowed: false,
    };
    /// Fixed harness seed for the cinematic view-offset RNG draws.
    const VIEW_SEED: u32 = 1;

    fn new() -> Self {
        Self {
            controller: ViewController::new(CameraMode::ChaseNear),
            rng: GameLcg::new(Self::VIEW_SEED),
            change_view_down: false,
        }
    }

    /// Cycles to the next recovered view on the rising edge of the change-view
    /// key, then returns the world matrix for the car's current placement.
    fn update(
        &mut self,
        change_view_down: bool,
        position: Vec3,
        heading: u16,
        pal: bool,
    ) -> [i32; 16] {
        if change_view_down && !self.change_view_down {
            self.controller.cycle(Self::GATE, &mut self.rng);
        }
        self.change_view_down = change_view_down;
        self.controller.update(position, heading, pal)
    }
}

/// One windowed-game-session frame of the input/advance/world-compose work the
/// two loop variants share.
///
/// It maps the live input through the recovered control map (the analog driver
/// when a gamepad is connected, the keyboard driver otherwise), advances the FSM
/// one frame stepping the race sim in the driving state, and - only when the
/// route is a `StageWorld`-family route - composes the stage from the follow
/// camera. A compose failure drops the world frame rather than killing the loop.
/// It returns the frame's render route (so the caller can decide any non-world
/// presentation), the composed world frame (if any), and whether the FSM has
/// reached the stopped state.
#[allow(clippy::too_many_arguments)]
fn advance_game_frame<C: HostClock>(
    session: &mut GameplaySession,
    keyboard: &mut KeyboardDriver,
    analog: &mut AnalogDriver,
    response: &DriveResponse,
    menu_inputs: MenuFlowInputs,
    progress: &mut StageProgress,
    clock: &C,
    resources: &mut StageRenderResources,
    camera: &mut ViewHarness,
    fade: &mut Option<ScalarTween>,
    input: WindowedInput,
) -> Result<GameFrameOutcome, WheelPostprocessError> {
    let controls = if let Some(pad) = input.gamepad {
        analog
            .poll(
                pad.steer_axis(),
                pad.pedal_axis(),
                pad.buttons(),
                response,
                true,
            )
            .controls
    } else {
        keyboard
            .poll(|vk| input.keys.is_down(vk), response, true)
            .controls
    };
    let advanced = session.try_advance(clock, menu_inputs, controls, progress)?;
    let world = if advanced.route.is_some_and(presents_world_scene) {
        // Compose the live world for every route that reaches the shared viewport
        // compositor 0x422ca0: the in-race StageWorld states and the overlay
        // (0xa) / fade (0xb) routes, which composite the same world frame (the
        // overlay layer and fade modulation are runtime/asset-gated effects over
        // it, not yet reproduced here). The recovered in-race chase camera places
        // behind + above the car along its heading and eases the orientation
        // (`gameplay_camera`), NTSC base rate (`pal = false`). The harness
        // change-view key cycles the recovered view modes (chase/cockpit/
        // cinematic) on its rising edge.
        let car = session.car();
        let change_view_down = input.keys.is_down(ViewHarness::CHANGE_VIEW_VK);
        let camera_world = camera.update(change_view_down, car.position, car.heading, false);
        resources
            .compose(&camera_world)
            .ok()
            .map(|rendered| rendered.frame)
    } else {
        None
    };
    // Race-end fade (state 0xb, FadeComposite): the recovered linear brightness
    // tween drives a fullscreen modulate of the composed world toward black. The
    // shipped fade is triggered by the in-race transition handlers; this windowed
    // harness starts the recovered fade-out when the FSM reaches the fade route
    // and advances it one step per frame (a documented harness stand-in, like the
    // synthetic checkpoint line), presenting the modulated world so the race-end
    // transition is visible. Any other route clears the fade.
    let world = if advanced.route == Some(RenderRoute::FadeComposite) {
        let tween = fade.get_or_insert_with(|| fade_out(fade_duration_frames(false)));
        let level = tween.evaluate_and_advance();
        world.map(|frame| frame.modulated(fade_level_to_modulation_byte(level)))
    } else {
        *fade = None;
        world
    };
    Ok(GameFrameOutcome {
        route: advanced.route,
        world,
        stopped: session.is_stopped(),
        // The recovered front-end music plays during the menu band (states 1..4).
        music_active: matches!(advanced.band, SessionBand::Menu),
    })
}

/// What one [`advance_game_frame`] produced for the windowed loop.
struct GameFrameOutcome {
    route: Option<RenderRoute>,
    world: Option<SoftwarePresentedFrame>,
    stopped: bool,
    music_active: bool,
}

/// Reviewed surface inputs owned by a game session.
#[derive(Debug)]
pub struct SessionSurfaceInputs {
    store: SurfaceCapabilityStore,
    runtime_fifth: [u8; SURFACE_ID_COUNT],
    stage_surface_ids: Vec<u8>,
}

#[derive(Debug)]
pub enum SessionSurfaceInputsError {
    Profile(ReviewedSurfaceProfileError),
    Store(SurfaceTableStoreError),
    Stage(StageFrameError),
    MissingStageSurfaceData,
}

impl fmt::Display for SessionSurfaceInputsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Profile(error) => write!(formatter, "invalid reviewed surface profile: {error}"),
            Self::Store(error) => write!(formatter, "cannot open surface inputs: {error}"),
            Self::Stage(error) => write!(formatter, "cannot read stage surface data: {error}"),
            Self::MissingStageSurfaceData => write!(formatter, "stage .srf sibling is missing"),
        }
    }
}

impl std::error::Error for SessionSurfaceInputsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Profile(error) => Some(error),
            Self::Store(error) => Some(error),
            Self::Stage(error) => Some(error),
            Self::MissingStageSurfaceData => None,
        }
    }
}

impl From<StageFrameError> for SessionSurfaceInputsError {
    fn from(error: StageFrameError) -> Self {
        Self::Stage(error)
    }
}

impl SessionSurfaceInputs {
    pub fn open(runtime_data: &Path) -> Result<Self, SessionSurfaceInputsError> {
        let reviewed =
            ReviewedSurfaceCapabilityProfile::from_json(REVIEWED_SURFACE_CAPABILITY_PROFILE)
                .map_err(SessionSurfaceInputsError::Profile)?;
        let store = SurfaceCapabilityStore::open(
            &runtime_data.join(SURFACE_CAPABILITY_STORE_DIRECTORY),
            &reviewed,
        )
        .map_err(SessionSurfaceInputsError::Store)?;
        Ok(Self {
            store,
            runtime_fifth: ZERO_INITIALIZED_RUNTIME_FIFTH_CAPABILITY_TABLE,
            stage_surface_ids: Vec::new(),
        })
    }

    /// Open the reviewed tables and the stage's CMPR-aware `.srf` sibling.
    pub fn open_stage(
        media_root: &Path,
        runtime_data: &Path,
        stage: &str,
        limits: DecodeLimits,
    ) -> Result<Self, SessionSurfaceInputsError> {
        let mut inputs = Self::open(runtime_data)?;
        inputs.stage_surface_ids = read_decoded_section(media_root, stage, ".srf", limits)?
            .ok_or(SessionSurfaceInputsError::MissingStageSurfaceData)?;
        Ok(inputs)
    }

    /// Compute effect enables for stage-authored surface IDs.
    #[must_use]
    pub fn effect_enables(
        &self,
        stage_surface_ids: &[u8],
    ) -> Option<[u32; SURFACE_CAPABILITY_COUNT]> {
        accumulate_effect_enables_from_extracted(
            stage_surface_ids,
            self.store.tables(),
            &self.runtime_fifth,
        )
    }

    #[must_use]
    pub fn stage_surface_ids(&self) -> &[u8] {
        &self.stage_surface_ids
    }

    #[must_use]
    pub fn stage_effect_enables(&self) -> Option<[u32; SURFACE_CAPABILITY_COUNT]> {
        self.effect_enables(&self.stage_surface_ids)
    }

    #[must_use]
    pub fn stage_weather_effects(&self) -> Option<StageWeatherEffects> {
        self.stage_effect_enables()
            .map(|counters| StageWeatherEffects::from_counters(&counters))
    }

    #[must_use]
    pub fn stage_particle_effect(&self, type_byte: u8) -> Option<particles::ParticleEffect> {
        self.stage_weather_effects()
            .and_then(|effects| particles::gated_particle_effect(type_byte, effects))
    }

    #[must_use]
    pub fn stage_spawn_particle<'a>(
        &self,
        pools: &'a mut particles::ParticlePools,
        pool: usize,
        input: particles::ParticleSpawn,
    ) -> Option<(bool, &'a particles::ParticleRecord)> {
        self.stage_particle_effect(input.type_byte)?;
        pools.spawn(pool, input)
    }
}

/// Authenticated coefficient inputs for the second-wheel force producer.
#[derive(Debug)]
pub struct SessionWheelInputs {
    store: SurfaceTableStore,
}

impl SessionWheelInputs {
    pub fn open(runtime_data: &Path) -> Result<Self, SessionSurfaceInputsError> {
        let reviewed = ReviewedSurfaceProfile::from_json(REVIEWED_SURFACE_TABLE_PROFILE)
            .map_err(SessionSurfaceInputsError::Profile)?;
        let store =
            SurfaceTableStore::open(&runtime_data.join(SURFACE_TABLE_STORE_DIRECTORY), &reviewed)
                .map_err(SessionSurfaceInputsError::Store)?;
        Ok(Self { store })
    }

    /// Populate a second-wheel coefficient from the authenticated first family.
    ///
    /// # Errors
    ///
    /// Returns the shared error when the stage surface ID is out of range.
    pub fn populate_second_wheel(
        &self,
        second: &mut SecondWheelParams<'_>,
        surface_index: usize,
    ) -> Result<(), SurfaceCoefficientIndexError> {
        populate_surface_table_factor(second, self.store.families(), surface_index)
    }

    /// Populate all second-wheel coefficients atomically from the reviewed store.
    ///
    /// # Errors
    ///
    /// Returns the first invalid surface index without modifying the wheel inputs.
    pub fn populate_four_wheel(
        &self,
        wheel: &mut FourWheelIterationParams<'_>,
        surface_indices: [usize; 4],
    ) -> Result<(), SurfaceCoefficientIndexError> {
        let mut second = wheel.second.clone();
        for (index, surface_index) in surface_indices.into_iter().enumerate() {
            self.populate_second_wheel(&mut second[index], surface_index)?;
        }
        wheel.second = second;
        Ok(())
    }

    /// Build complete four-wheel inputs from authenticated coefficients and the
    /// caller's recovered compiled state. Remaining scalar tuning is explicit
    /// harness input, not a claim of trajectory-equivalent car data.
    ///
    /// # Errors
    ///
    /// Returns an invalid stage surface index before publishing the parameters.
    pub fn four_wheel_params<'a>(
        &self,
        trig: &'a [i32],
        families: &CompiledControlFamilies,
        half_state: &CompiledHalfSignState,
        factor_c860: Fixed16,
        surface_indices: [usize; 4],
    ) -> Result<FourWheelIterationParams<'a>, SurfaceCoefficientIndexError> {
        let inactive = GeometricPostprocessLane {
            active: false,
            vector_a: Vec3::ZERO,
            vector_b: Vec3::ZERO,
            vector_c: Vec3::ZERO,
            accumulator: Vec3::ZERO,
        };
        let mut wheel = FourWheelIterationParams {
            first: [FirstWheelParams {
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
                flag_43b: 10,
                config_158: 1,
                gear_ratio: 1,
                is_front_wheel: true,
            }; 4],
            second: std::array::from_fn(|_| SecondWheelParams {
                axis_dot: 0,
                local_68: [0x10000, 0, 0],
                local_98: [0; 3],
                wheel_e8: 0,
                scale_c5c4: 0x10000,
                residual_flag_147a: false,
                config_2ec: 1,
                gear_ratio: 1,
                coefficient_34: 0x10000,
                arctan_table: &cmr_core::legacy_math::SECOND_WHEEL_ARCTAN_TABLE,
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
            contact_update_inputs: [cmr_core::vehicle_dynamics::ContactVectorUpdateInput {
                update_gate: 0,
                enabled: false,
                vector: Vec3::ZERO,
                basis: Vec3::ZERO,
                scale_528: Fixed16::ONE,
                extra_vector: Vec3::ZERO,
                limit: Fixed16::ONE,
                threshold: Fixed16::ONE,
            }; 8],
            lane_threshold_inputs:
                cmr_core::compiled_car_initializer::CompiledLaneThresholdInputs {
                    lane_words: [0; 8],
                    source_y: [0x199a; 8],
                },
            lane_response_2d8: Fixed16::ZERO,
            lane_time_step: SOLVER_GLOBAL_CONSTANTS.time_step,
            lane_direction_angle: 0,
            lane_rebuild: false,
            lane_exchange: false,
            postprocess: FourWheelPostprocessParams {
                lanes: [inactive; 4],
                current_lane: 0,
                argument_scale: Fixed16::ONE,
                base_force: Vec3::ZERO,
                non_wheel_forces: &[],
                wheel_contacts: [true; 4],
            },
            angular: AngularOrientationInput {
                momentum: AngularMomentumStepInput {
                    prior_moment: Vec3::ZERO,
                    contact_moment: Vec3::ZERO,
                    moment_scale: Fixed16::ONE,
                    inertia_scale: Vec3::new(Fixed16::ONE, Fixed16::ONE, Fixed16::ONE),
                    angular_velocity: Vec3::ZERO,
                    time_step: SOLVER_GLOBAL_CONSTANTS.time_step,
                },
                clamp_enabled: false,
                control: ControlAxisFlags {
                    mode_3ec: 0,
                    axis_3f4: 0,
                    axis_3f8: 0,
                },
                basis: [Vec3::ZERO; 3],
            },
            engine_smoothing: cmr_core::drivetrain::SmoothedEngineSpeedInput {
                wheel_speeds: [0; 4],
                config_158: 1,
                gear_ratio: 0x10000,
                engine_2e0: 0,
                redline_15c: 0x10000,
                slip_gain: 0,
                engine_load: 0x10000,
            },
            driven_smoothing: cmr_core::race_state::DrivenWheelUpdateParams::default(),
            engine_force: cmr_core::drivetrain::EngineForceInput {
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
        };
        for index in 0..4 {
            populate_compiled_gear_ratio(
                &mut wheel.first[index],
                &mut wheel.second[index],
                families,
                factor_c860,
            )
            .expect("gear one is valid in the compiled family");
            let _ = populate_force_pass_scales(
                &mut wheel.first[index],
                &mut wheel.second[index],
                Fixed16::from_raw(0x30000),
                SOLVER_GLOBAL_CONSTANTS.force_scale,
                Fixed16::from_raw(0x40000),
            );
        }
        self.populate_four_wheel(&mut wheel, surface_indices)?;
        populate_compiled_contact_offsets(&mut wheel, half_state);
        cmr_core::race_state::populate_compiled_contact_update_inputs(
            &mut wheel,
            &cmr_core::compiled_car_initializer::derive_compiled_control_clear_loop(),
        );
        Ok(wheel)
    }
}

pub fn run_windowed_game_session(
    media_root: &Path,
    runtime_data: &Path,
    stage: &str,
    renderer: StageRendererChoice,
) -> Result<(), WindowedStageFrameError> {
    let mut resources = StageRenderResources::load_with(media_root, stage, renderer)?;
    let initial = resources.compose(&stage_placeholder_camera())?;

    let inputs = SessionWheelInputs::open(runtime_data).map_err(SessionFourWheelError::Inputs)?;
    let trig = cmr_core::fixed_point::build_sine_table();
    let families = CompiledControlFamilies {
        post_processed_raw: [0; 8],
        reciprocal: [0; 8],
        raw_copy: [0x10000; 8],
        reciprocal_copy: [0; 8],
    };
    let half_state = cmr_core::compiled_car_initializer::derive_compiled_half_sign_state([
        0x40000, -0x60000, 0x80000,
    ]);
    let wheel = inputs
        .four_wheel_params(&trig, &families, &half_state, Fixed16::ONE, [0; 4])
        .map_err(SessionFourWheelError::Surface)?;
    let params = SimParams {
        inverse_mass: Fixed16::from_raw(0x8000),
        gravity: Vec3::ZERO,
    };
    let mut start = CarState::at_rest();
    start.motion.angular_velocity = Vec3::new(Fixed16::ONE, Fixed16::ONE, Fixed16::ONE);
    let mut session = GameplaySession::new_with_four_wheel_and_car(
        params,
        InRaceScript::frontend_harness(),
        wheel,
        start,
    );
    let mut keyboard = KeyboardDriver::player_one();
    let mut analog = AnalogDriver::new();
    let response = DriveResponse {
        max_forward_force: Fixed16::from_raw(GAME_SESSION_DRIVE_FORCE_Q16),
        max_steer_rate: GAME_SESSION_MAX_STEER_RATE,
    };
    let menu_inputs = MenuFlowInputs {
        frontend_active: true,
        root_menu_ready: true,
        screen_graph_ready: true,
        ..MenuFlowInputs::default()
    };
    let nodes = headless_checkpoint_line();
    let mut progress = StageProgress::new(&nodes, 0);
    let clock = WallClock::new();
    let mut camera = ViewHarness::new();
    let mut fade: Option<ScalarTween> = None;
    let mut producer_error = None;

    // Best-effort: open the recovered front-end root-menu compositor from the
    // runtime-data root so the interactive front-end route (state 2) presents the
    // real recovered menu frame instead of a cleared one. If the front-end assets
    // are unavailable or the session cannot be built, the front-end route falls
    // back to a cleared frame and the world render path is unaffected.
    let front_end = ReviewedProfile::from_json(REVIEWED_PROFILE)
        .ok()
        .and_then(|reviewed| open_root_session_inputs(runtime_data, &reviewed).ok());

    if let Some((store, startup)) = front_end {
        let request = SoftwareRootMenuSessionRequest::official_eu_initial_software_default_config(
            Language::English,
            VisualVariant::Normal,
        );
        let built = with_software_root_menu_session(&store, startup, request, |root| {
            run_windowed_game_session_loop(
                &initial.frame,
                Some(media_root),
                |input: WindowedInput| {
                    match advance_game_frame(
                        &mut session,
                        &mut keyboard,
                        &mut analog,
                        &response,
                        menu_inputs,
                        &mut progress,
                        &clock,
                        &mut resources,
                        &mut camera,
                        &mut fade,
                        input,
                    ) {
                        Ok(outcome) => {
                            // A composed world frame wins; otherwise the interactive
                            // front-end route presents the recovered root-menu frame.
                            let present = if outcome.world.is_some() {
                                outcome.world
                            } else if outcome.route.is_some_and(composes_frontend_root) {
                                root.render(initial_root_input())
                                    .ok()
                                    .and_then(|_| root.backend().presented_frame().cloned())
                            } else {
                                None
                            };
                            WindowedGameFrame {
                                present,
                                stopped: outcome.stopped,
                                music_active: outcome.music_active,
                            }
                        }
                        Err(error) => {
                            producer_error = Some(SessionFourWheelError::Producer(error));
                            WindowedGameFrame {
                                present: None,
                                stopped: true,
                                music_active: false,
                            }
                        }
                    }
                },
            )
        });
        if let Ok(run) = built {
            run.output.map_err(WindowedStageFrameError::Present)?;
            if let Some(error) = producer_error {
                return Err(error.into());
            }
            return Ok(());
        }
        // The front-end session could not be built after its inputs opened; fall
        // through to the world-only loop below rather than failing the session.
    }

    run_windowed_game_session_loop(&initial.frame, Some(media_root), |input: WindowedInput| {
        match advance_game_frame(
            &mut session,
            &mut keyboard,
            &mut analog,
            &response,
            menu_inputs,
            &mut progress,
            &clock,
            &mut resources,
            &mut camera,
            &mut fade,
            input,
        ) {
            Ok(outcome) => WindowedGameFrame {
                present: outcome.world,
                stopped: outcome.stopped,
                music_active: outcome.music_active,
            },
            Err(error) => {
                producer_error = Some(SessionFourWheelError::Producer(error));
                WindowedGameFrame {
                    present: None,
                    stopped: true,
                    music_active: false,
                }
            }
        }
    })
    .map_err(WindowedStageFrameError::Present)?;
    if let Some(error) = producer_error {
        return Err(error.into());
    }
    Ok(())
}

/// A stationary host clock: it never advances within a frame, so the
/// [`FrameDriver`](cmr_core::game_driver::FrameDriver)'s in-race catch-up loop
/// (`do { tick } while (the tick crossed a 40 ms bucket)`) runs exactly one sim
/// tick per in-race frame - deterministic lock-step for a headless trace. Real
/// wall-clock pacing is the windowed backend's concern.
struct StationaryClock;

impl HostClock for StationaryClock {
    fn millis(&self) -> u32 {
        0
    }
}

/// The forward drive force (Q16.16) the headless session applies each in-race
/// tick. A **harness constant** for the demonstration drive, not a recovered
/// value.
const GAME_SESSION_DRIVE_FORCE_Q16: i32 = 8 << 16;
/// The heading rate (signed 16-bit BAM per tick) at full steer for the windowed
/// session's live-input drive. A **harness constant**, not a recovered value:
/// the shipped steer authority is a runtime car/surface-table quantity
/// (`rea.official-eu.drivetrain-actuator-chain`).
const GAME_SESSION_MAX_STEER_RATE: i16 = 0x200;
/// A safety cap on frames so a mis-tuned drive cannot loop forever.
const GAME_SESSION_MAX_FRAMES: u32 = 100_000;

/// The synthetic `.cat` checkpoint line the headless drive runs along: split
/// nodes at +x = 10/20/40 world units (Q16.16), each facing +x. This is a
/// **harness stand-in** for a real stage's `.cat` spline - the +x auto-drive
/// follows this straight line, whereas a real 3D spline needs real steering
/// (input/replay), a later milestone. The crossing predicate and the finish
/// detection are the recovered [`StageProgress`] semantics, not a harness.
fn headless_checkpoint_line() -> Vec<CatNode> {
    [10i32, 20, 40]
        .into_iter()
        .map(|x| CatNode {
            position: [x << 16, 0, 0],
            direction: [1 << 16, 0, 0],
            split_flag: 0b1, // a split for car 0
            reserved: 0,
            seq_index: 0,
        })
        .collect()
}

/// The result of a headless game-session run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HeadlessGameSessionOutput {
    /// Frames driven until the FSM reached the stopped state (or the cap).
    pub frames: u32,
    /// Sim ticks the in-race handler stepped the race state.
    pub sim_ticks: u32,
    /// The final FSM state id (`0` when the session stopped cleanly).
    pub final_state: u8,
    /// Whether the drive crossed the finish line.
    pub finished: bool,
    /// The finish time in centiseconds, if it finished.
    pub finish_centiseconds: Option<i32>,
    /// The car's final +x position (Q16.16 raw).
    pub final_position_x_q16: i32,
    /// The stage clock at the end (Q16.16 raw).
    pub stage_clock_q16: i32,
}

impl HeadlessGameSessionOutput {
    /// A deterministic, path-free one-line digest for the CLI.
    pub fn stable_summary(&self) -> String {
        format!(
            "game-session frames={} sim_ticks={} final_state={} finished={} finish_cs={} pos_x_q16={} clock_q16={}",
            self.frames,
            self.sim_ticks,
            self.final_state,
            self.finished,
            self.finish_centiseconds
                .map_or_else(|| "none".to_string(), |cs| cs.to_string()),
            self.final_position_x_q16,
            self.stage_clock_q16,
        )
    }
}

/// Runs the recovered top-level game FSM headless from boot to quit, driving the
/// in-race band through the recovered in-race decision tables
/// ([`race_flow_next`]) with the recovered checkpoint tracker
/// ([`StageProgress`]) raising the finish.
///
/// The session runs in **front-end mode** (`frontend_active`) so the recovered
/// graph terminates cleanly through the real tables: the menu band routes
/// `1 -> 2 -> 3 -> 4 -> 7` (`frontend_loop` confirmed, `load_arm` -> entry `7`),
/// then the gameplay band runs `7 -> 5 -> 9 -> 0xb -> 0xc -> 0`. In state `5`
/// (the main driving tick) the deterministic race sim steps one fixed tick per
/// frame under a stationary clock and feeds the car's world position to a
/// [`StageProgress`] walking the synthetic [`headless_checkpoint_line`]; when
/// the tracker crosses the final gate it raises `stage_finished`, and the
/// recovered [`drive_next`](cmr_core::game_race_flow::drive_next) table hands off
/// to the front-end finish-flyby (`9`). Every in-race transition is
/// [`race_flow_next`], not an ad-hoc choice; the drive force, the synthetic
/// checkpoint line, and the scripted front-end sequencer inputs (the replay/
/// fade/quit flags) are documented **harness** stand-ins for the real subsystems.
pub fn run_headless_game_session() -> HeadlessGameSessionOutput {
    let params = SimParams {
        inverse_mass: Fixed16::ONE,
        gravity: Vec3::ZERO,
    };
    let mut session = GameplaySession::new(params, InRaceScript::frontend_harness());
    let controls = Controls {
        steer_rate: 0,
        forward_force: Fixed16::from_raw(GAME_SESSION_DRIVE_FORCE_Q16),
        handbrake: false,
    };
    drive_headless_session(&mut session, controls).expect("the simplified session is infallible")
}

fn drive_headless_session(
    session: &mut GameplaySession<'_>,
    controls: Controls,
) -> Result<HeadlessGameSessionOutput, WheelPostprocessError> {
    let clock = StationaryClock;
    let menu_inputs = MenuFlowInputs {
        frontend_active: true,
        root_menu_ready: true,
        screen_graph_ready: true,
        ..MenuFlowInputs::default()
    };
    let nodes = headless_checkpoint_line();
    let mut progress = StageProgress::new(&nodes, 0);
    let mut frames = 0u32;
    while !session.is_stopped() && frames < GAME_SESSION_MAX_FRAMES {
        frames += 1;
        session.try_advance(&clock, menu_inputs, controls, &mut progress)?;
    }
    Ok(HeadlessGameSessionOutput {
        frames,
        sim_ticks: session.sim_ticks(),
        final_state: session.state().id(),
        finished: progress.finished(),
        finish_centiseconds: progress.finish_centiseconds(),
        final_position_x_q16: session.car().position.x.raw(),
        stage_clock_q16: session.stage_clock().raw(),
    })
}

#[derive(Debug)]
pub enum SessionFourWheelError {
    Inputs(SessionSurfaceInputsError),
    Surface(SurfaceCoefficientIndexError),
    Producer(WheelPostprocessError),
}

impl fmt::Display for SessionFourWheelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Inputs(error) => write!(formatter, "session wheel inputs failed: {error}"),
            Self::Surface(error) => write!(formatter, "invalid wheel surface index: {error:?}"),
            Self::Producer(error) => write!(formatter, "four-wheel producer failed: {error:?}"),
        }
    }
}

impl std::error::Error for SessionFourWheelError {}

impl From<SessionSurfaceInputsError> for SessionFourWheelError {
    fn from(error: SessionSurfaceInputsError) -> Self {
        Self::Inputs(error)
    }
}

impl From<SurfaceCoefficientIndexError> for SessionFourWheelError {
    fn from(error: SurfaceCoefficientIndexError) -> Self {
        Self::Surface(error)
    }
}

impl From<WheelPostprocessError> for SessionFourWheelError {
    fn from(error: WheelPostprocessError) -> Self {
        Self::Producer(error)
    }
}

/// Run the headless FSM with authenticated coefficients and compiled geometry.
///
/// # Errors
///
/// Returns store/profile, surface-index, or checked producer failures.
pub fn run_headless_four_wheel_game_session(
    runtime_data: &Path,
    families: &CompiledControlFamilies,
    half_state: &CompiledHalfSignState,
) -> Result<HeadlessGameSessionOutput, SessionFourWheelError> {
    let inputs = SessionWheelInputs::open(runtime_data)?;
    let trig = cmr_core::fixed_point::build_sine_table();
    let wheel = inputs.four_wheel_params(&trig, families, half_state, Fixed16::ONE, [0; 4])?;
    let params = SimParams {
        inverse_mass: Fixed16::from_raw(0x8000),
        gravity: Vec3::ZERO,
    };
    let mut start = CarState::at_rest();
    start.motion.angular_velocity = Vec3::new(Fixed16::ONE, Fixed16::ONE, Fixed16::ONE);
    let mut session = GameplaySession::new_with_four_wheel_and_car(
        params,
        InRaceScript::frontend_harness(),
        wheel,
        start,
    );
    let controls = Controls {
        steer_rate: 0,
        forward_force: Fixed16::ONE,
        handbrake: false,
    };
    Ok(drive_headless_session(&mut session, controls)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(values: &[&str]) -> impl Iterator<Item = OsString> {
        values.iter().map(OsString::from)
    }

    static NEXT_SURFACE_TEMP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    struct SurfaceTemp(PathBuf);

    impl Drop for SurfaceTemp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn surface_runtime() -> SurfaceTemp {
        use cmr_assets::{
            SURFACE_CAPABILITY_DATA_FILE, SURFACE_CAPABILITY_STATE_FILE,
            SurfaceCapabilityImportSummary, SurfaceImportFile,
        };
        use sha2::{Digest, Sha256};

        let root = std::env::temp_dir().join(format!(
            "cmr-app-surface-inputs-{}-{}",
            std::process::id(),
            NEXT_SURFACE_TEMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let store = root.join(SURFACE_CAPABILITY_STORE_DIRECTORY);
        std::fs::create_dir_all(&store).unwrap();
        let reviewed =
            ReviewedSurfaceCapabilityProfile::from_json(REVIEWED_SURFACE_CAPABILITY_PROFILE)
                .unwrap();
        let data: Vec<u8> = (0..16)
            .flat_map(|table| vec![(table + 1) as u8; SURFACE_ID_COUNT])
            .collect();
        std::fs::write(store.join(SURFACE_CAPABILITY_DATA_FILE), &data).unwrap();
        let digest = |bytes: &[u8]| {
            Sha256::digest(bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        };
        let summary = SurfaceCapabilityImportSummary {
            schema_version: 1,
            profile: reviewed.profile().id.clone(),
            profile_sha256: reviewed.sha256().into(),
            source: SurfaceImportFile {
                path: reviewed.profile().executable.path.clone(),
                size: reviewed.profile().executable.size,
                sha256: reviewed.profile().executable.sha256.clone(),
            },
            output: SurfaceImportFile {
                path: SURFACE_CAPABILITY_DATA_FILE.into(),
                size: data.len() as u64,
                sha256: digest(&data),
            },
        };
        std::fs::write(
            store.join(SURFACE_CAPABILITY_STATE_FILE),
            serde_json::to_vec(&summary).unwrap(),
        )
        .unwrap();
        SurfaceTemp(root)
    }

    fn wheel_runtime() -> SurfaceTemp {
        use cmr_assets::{
            SURFACE_DATA_FILE, SURFACE_STATE_FILE, SurfaceImportFile, SurfaceImportSummary,
        };
        use sha2::{Digest, Sha256};

        let root = std::env::temp_dir().join(format!(
            "cmr-app-wheel-inputs-{}-{}",
            std::process::id(),
            NEXT_SURFACE_TEMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let store = root.join(SURFACE_TABLE_STORE_DIRECTORY);
        std::fs::create_dir_all(&store).unwrap();
        let reviewed = ReviewedSurfaceProfile::from_json(REVIEWED_SURFACE_TABLE_PROFILE).unwrap();
        let data: Vec<u8> = (0..240).flat_map(|_| 0x20000i32.to_le_bytes()).collect();
        std::fs::write(store.join(SURFACE_DATA_FILE), &data).unwrap();
        let digest = |bytes: &[u8]| {
            Sha256::digest(bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        };
        let summary = SurfaceImportSummary {
            schema_version: 1,
            profile: reviewed.profile().id.clone(),
            profile_sha256: reviewed.sha256().into(),
            source: SurfaceImportFile {
                path: reviewed.profile().executable.path.clone(),
                size: reviewed.profile().executable.size,
                sha256: reviewed.profile().executable.sha256.clone(),
            },
            output: SurfaceImportFile {
                path: SURFACE_DATA_FILE.into(),
                size: data.len() as u64,
                sha256: digest(&data),
            },
        };
        std::fs::write(
            store.join(SURFACE_STATE_FILE),
            serde_json::to_vec(&summary).unwrap(),
        )
        .unwrap();
        SurfaceTemp(root)
    }

    #[test]
    fn session_surface_inputs_load_reviewed_tables_and_compute_counters() {
        let temp = surface_runtime();
        let inputs = SessionSurfaceInputs::open(&temp.0).unwrap();
        assert_eq!(
            inputs.effect_enables(&[3, 3]),
            Some([2, 1, 3, 4, 0, 5, 6, 7, 8, 9, 10, 11, 12, 14, 13, 15])
        );
        assert_eq!(inputs.effect_enables(&[120]), None);
    }

    #[test]
    fn session_stage_surface_ids_come_from_the_srf_bytes() {
        let temp = surface_runtime();
        let media = temp.0.join("media");
        std::fs::create_dir(&media).unwrap();
        std::fs::write(media.join("AU1.srf"), [3, 3]).unwrap();
        let inputs =
            SessionSurfaceInputs::open_stage(&media, &temp.0, "AU1", DecodeLimits::default())
                .unwrap();
        assert_eq!(inputs.stage_surface_ids(), &[3, 3]);
        assert_eq!(
            inputs.stage_effect_enables(),
            Some([2, 1, 3, 4, 0, 5, 6, 7, 8, 9, 10, 11, 12, 14, 13, 15])
        );
        let weather = inputs.stage_weather_effects().unwrap();
        assert!(weather.splash && weather.dust && weather.snow);
        assert_eq!(
            inputs.stage_particle_effect(0x01),
            Some(particles::ParticleEffect::Dust)
        );
        let spawn = particles::ParticleSpawn {
            position: [1, 2, 3],
            vector_24: [4, 5, 6],
            word_30: 7,
            word_34: 8,
            word_46: 9,
            type_byte: 1,
            tuning: [10, 11],
            random: 0x090603,
            low_detail: false,
            global_flag: true,
        };
        let mut pools = particles::ParticlePools::default();
        let (_, record) = inputs.stage_spawn_particle(&mut pools, 0, spawn).unwrap();
        assert_eq!(record.position, [1, 2, 3]);
        assert_eq!(record.flags, 3);
        std::fs::write(media.join("AU1.srf"), [120]).unwrap();
        let inputs =
            SessionSurfaceInputs::open_stage(&media, &temp.0, "AU1", DecodeLimits::default())
                .unwrap();
        assert_eq!(inputs.stage_effect_enables(), None);
        assert_eq!(inputs.stage_weather_effects(), None);
        assert_eq!(inputs.stage_particle_effect(0x01), None);
        assert!(inputs.stage_spawn_particle(&mut pools, 0, spawn).is_none());
        assert!(matches!(
            SessionSurfaceInputs::open_stage(&media, &temp.0, "MISSING", DecodeLimits::default()),
            Err(SessionSurfaceInputsError::MissingStageSurfaceData)
        ));
    }

    #[test]
    fn session_surface_inputs_reject_tampered_data() {
        let temp = surface_runtime();
        let path = temp
            .0
            .join(SURFACE_CAPABILITY_STORE_DIRECTORY)
            .join(cmr_assets::SURFACE_CAPABILITY_DATA_FILE);
        let mut data = std::fs::read(&path).unwrap();
        data[0] ^= 1;
        std::fs::write(path, data).unwrap();
        assert!(SessionSurfaceInputs::open(&temp.0).is_err());
    }

    #[test]
    #[ignore = "requires reviewed user-derived data selected only by CMR_RUNTIME_DATA"]
    fn official_session_surface_inputs_open_without_exposing_values() {
        let root = std::env::var_os("CMR_RUNTIME_DATA")
            .expect("CMR_RUNTIME_DATA must select the reviewed runtime-data root");
        let inputs = SessionSurfaceInputs::open(Path::new(&root)).unwrap();
        assert_eq!(
            inputs.effect_enables(&[]),
            Some([0; SURFACE_CAPABILITY_COUNT])
        );
    }

    #[test]
    fn authenticated_four_wheel_session_finishes_deterministically() {
        let temp = wheel_runtime();
        let families = CompiledControlFamilies {
            post_processed_raw: [0; 8],
            reciprocal: [0; 8],
            raw_copy: [0x10000; 8],
            reciprocal_copy: [0; 8],
        };
        let half_state = cmr_core::compiled_car_initializer::derive_compiled_half_sign_state([
            0x40000, -0x60000, 0x80000,
        ]);
        let inputs = SessionWheelInputs::open(&temp.0).unwrap();
        let trig = cmr_core::fixed_point::build_sine_table();
        assert_eq!(
            inputs
                .four_wheel_params(&trig, &families, &half_state, Fixed16::ONE, [0, 0, 0, 120])
                .err(),
            Some(SurfaceCoefficientIndexError { index: 120 })
        );
        let wheel = inputs
            .four_wheel_params(&trig, &families, &half_state, Fixed16::ONE, [0; 4])
            .unwrap();
        assert!(wheel.first.iter().all(|first| first.flag_43b == 10));

        let first = run_headless_four_wheel_game_session(&temp.0, &families, &half_state).unwrap();
        let second = run_headless_four_wheel_game_session(&temp.0, &families, &half_state).unwrap();
        assert_eq!(first, second);
        assert!(first.finished);
        assert!(first.sim_ticks > 0);
    }

    #[test]
    #[ignore = "requires an authenticated coefficient store under CMR_RUNTIME_DATA"]
    fn official_session_wheel_inputs_populate_second_core() {
        let root = std::env::var_os("CMR_RUNTIME_DATA").expect("CMR_RUNTIME_DATA");
        let inputs = SessionWheelInputs::open(Path::new(&root)).unwrap();
        let trig = cmr_core::fixed_point::build_sine_table();
        let mut second = SecondWheelParams {
            axis_dot: 0,
            local_68: [0x10000, 0, 0],
            local_98: [0; 3],
            wheel_e8: 0,
            scale_c5c4: 0x10000,
            residual_flag_147a: false,
            config_2ec: 1,
            gear_ratio: 1,
            coefficient_34: 0x10000,
            arctan_table: &cmr_core::legacy_math::SECOND_WHEEL_ARCTAN_TABLE,
            cos_table: &trig,
            config_scale: 1,
            transition_gain: 0,
            transition_base: 0,
            alternate_gain: 0,
            alternate_base: 0,
            correction_gate: 1,
            accumulation_limit: 0x10000,
            correction_alternate: 0x10000,
            surface_vector: [0x10000, 0, 0],
            table_factor: None,
        };
        inputs.populate_second_wheel(&mut second, 0).unwrap();
        assert_eq!(
            second.table_factor,
            Some(inputs.store.families().first.values[0])
        );
        assert_eq!(
            inputs.populate_second_wheel(&mut second, 120),
            Err(SurfaceCoefficientIndexError { index: 120 })
        );
    }

    #[test]
    #[ignore = "requires an authenticated coefficient store under CMR_RUNTIME_DATA"]
    fn official_four_wheel_session_uses_authenticated_store() {
        let root = std::env::var_os("CMR_RUNTIME_DATA").expect("CMR_RUNTIME_DATA");
        let families = CompiledControlFamilies {
            post_processed_raw: [0; 8],
            reciprocal: [0; 8],
            raw_copy: [0x10000; 8],
            reciprocal_copy: [0; 8],
        };
        let half_state = cmr_core::compiled_car_initializer::derive_compiled_half_sign_state([
            0x40000, -0x60000, 0x80000,
        ]);
        let output =
            run_headless_four_wheel_game_session(Path::new(&root), &families, &half_state).unwrap();
        assert!(output.sim_ticks > 0);
    }

    #[test]
    #[ignore = "requires user media and reviewed data selected by CMR_ORIGINAL_DIR/CMR_RUNTIME_DATA"]
    fn official_stage_surface_inputs_compute_without_exposing_values() {
        let media = std::env::var_os("CMR_ORIGINAL_DIR")
            .expect("CMR_ORIGINAL_DIR must select a stage media directory");
        let runtime = std::env::var_os("CMR_RUNTIME_DATA")
            .expect("CMR_RUNTIME_DATA must select the reviewed runtime-data root");
        let stage = std::env::var("CMR_STAGE").unwrap_or_else(|_| "AU1".into());
        let inputs = SessionSurfaceInputs::open_stage(
            Path::new(&media),
            Path::new(&runtime),
            &stage,
            DecodeLimits::default(),
        )
        .unwrap();
        assert!(!inputs.stage_surface_ids().is_empty());
        assert!(inputs.stage_effect_enables().is_some());
        assert!(inputs.stage_weather_effects().is_some());
    }

    #[test]
    fn explicit_runtime_data_wins_over_environment_fallback() {
        let config = runtime_config(
            arguments(&["--headless-root-frame", "--runtime-data", "explicit-data"]),
            Some(OsString::from("environment-data")),
            None,
        )
        .unwrap();

        assert_eq!(config.runtime_data(), Path::new("explicit-data"));
        assert_eq!(config.command(), AppCommand::HeadlessRootFrame);
        assert_eq!(config.runtime_data_source(), RuntimeDataSource::CommandLine);
    }

    #[test]
    fn environment_is_used_only_as_a_fallback() {
        let config = runtime_config(
            arguments(&["--headless-root-frame"]),
            Some(OsString::from("environment-data")),
            None,
        )
        .unwrap();

        assert_eq!(config.runtime_data(), Path::new("environment-data"));
        assert_eq!(config.runtime_data_source(), RuntimeDataSource::Environment);
    }

    #[test]
    fn windowed_command_uses_the_same_explicit_runtime_data_contract() {
        let config = runtime_config(
            arguments(&["--windowed-root-frame", "--runtime-data", "native-data"]),
            None,
            None,
        )
        .unwrap();

        assert_eq!(config.command(), AppCommand::WindowedRootFrame);
        assert_eq!(config.runtime_data(), Path::new("native-data"));
    }

    #[test]
    fn headless_game_session_command_parses_like_a_root_frame() {
        let config = runtime_config(
            arguments(&["--headless-game-session", "--runtime-data", "data"]),
            None,
            None,
        )
        .unwrap();
        assert_eq!(config.command(), AppCommand::HeadlessGameSession);
        // It needs neither a stage nor a media root.
        assert_eq!(config.stage(), None);
        assert_eq!(config.media_root(), None);
    }

    #[test]
    fn stage_renderer_option_defaults_to_auto_and_accepts_each_name() {
        let config = runtime_config(
            arguments(&[
                "--headless-stage-frame",
                "--runtime-data",
                "data",
                "--source",
                "media",
                "--stage",
                "AU1",
            ]),
            None,
            None,
        )
        .unwrap();
        assert_eq!(config.stage_renderer(), StageRendererChoice::Auto);

        for (value, expected) in [
            ("auto", StageRendererChoice::Auto),
            ("software", StageRendererChoice::Software),
            ("gpu", StageRendererChoice::Gpu),
        ] {
            let config = runtime_config(
                arguments(&[
                    "--headless-stage-frame",
                    "--runtime-data",
                    "data",
                    "--source",
                    "media",
                    "--stage",
                    "AU1",
                    "--renderer",
                    value,
                ]),
                None,
                None,
            )
            .unwrap();
            assert_eq!(config.stage_renderer(), expected);
        }

        assert_eq!(
            runtime_config(
                arguments(&[
                    "--headless-stage-frame",
                    "--runtime-data",
                    "data",
                    "--source",
                    "media",
                    "--stage",
                    "AU1",
                    "--renderer",
                    "gpu",
                    "--renderer",
                    "software",
                ]),
                None,
                None,
            ),
            Err(ConfigError::DuplicateOption("--renderer"))
        );
        assert_eq!(
            runtime_config(
                arguments(&[
                    "--headless-stage-frame",
                    "--runtime-data",
                    "data",
                    "--source",
                    "media",
                    "--stage",
                    "AU1",
                    "--renderer",
                    "vulkan",
                ]),
                None,
                None,
            ),
            Err(ConfigError::InvalidRenderer("vulkan".to_owned()))
        );
        assert_eq!(
            runtime_config(
                arguments(&[
                    "--headless-stage-frame",
                    "--runtime-data",
                    "data",
                    "--source",
                    "media",
                    "--stage",
                    "AU1",
                    "--renderer",
                ]),
                None,
                None,
            ),
            Err(ConfigError::MissingOptionValue("--renderer"))
        );
    }

    #[test]
    fn headless_game_session_runs_the_fsm_from_boot_to_stopped_deterministically() {
        let first = run_headless_game_session();
        let second = run_headless_game_session();
        assert_eq!(first, second, "the session must be deterministic");

        // It drives the FSM to the terminal stopped state, having stepped the
        // in-race sim and crossed the recovered checkpoint tracker's final gate.
        assert_eq!(first.final_state, 0);
        assert!(first.finished);
        assert!(first.finish_centiseconds.is_some());
        assert!(first.sim_ticks > 0);
        // It stopped well inside the safety cap (did not spin).
        assert!(first.frames < GAME_SESSION_MAX_FRAMES);
        // The car reached at least the last checkpoint node on +x (40.0 Q16.16).
        assert!(first.final_position_x_q16 >= 40 << 16);
        // The summary is a single path-free line.
        let summary = first.stable_summary();
        assert!(summary.starts_with("game-session frames="));
        assert!(!summary.contains('/') && !summary.contains('\\'));
    }

    #[test]
    fn missing_and_empty_runtime_data_are_distinct_errors() {
        assert_eq!(
            runtime_config(arguments(&["--headless-root-frame"]), None, None),
            Err(ConfigError::MissingRuntimeData)
        );
        assert_eq!(
            runtime_config(
                arguments(&["--headless-root-frame"]),
                Some(OsString::new()),
                None,
            ),
            Err(ConfigError::EmptyRuntimeData(
                RuntimeDataSource::Environment
            ))
        );
        assert_eq!(
            runtime_config(
                arguments(&["--headless-root-frame", "--runtime-data", ""]),
                Some(OsString::from("valid-fallback")),
                None,
            ),
            Err(ConfigError::EmptyRuntimeData(
                RuntimeDataSource::CommandLine
            ))
        );
    }

    #[test]
    fn rejects_unknown_duplicate_and_incomplete_options() {
        assert_eq!(
            AppOptions::parse(arguments(&["runtime-data"])),
            Err(ConfigError::UnknownArgument(OsString::from("runtime-data")))
        );
        assert_eq!(
            AppOptions::parse(arguments(&[
                "--headless-root-frame",
                "--runtime-data",
                "one",
                "--runtime-data",
                "two",
            ])),
            Err(ConfigError::DuplicateOption("--runtime-data"))
        );
        assert_eq!(
            AppOptions::parse(arguments(&["--headless-root-frame", "--runtime-data"])),
            Err(ConfigError::MissingOptionValue("--runtime-data"))
        );
        assert_eq!(
            AppOptions::parse(arguments(&[])),
            Err(ConfigError::MissingCommand)
        );
        assert_eq!(
            AppOptions::parse(arguments(&[
                "--headless-root-frame",
                "--headless-root-frame",
            ])),
            Err(ConfigError::DuplicateOption("--headless-root-frame"))
        );
        assert_eq!(
            AppOptions::parse(arguments(&[
                "--headless-root-frame",
                "--windowed-root-frame",
            ])),
            Err(ConfigError::DuplicateOption("--windowed-root-frame"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn preserves_non_utf8_runtime_data_paths() {
        use std::os::unix::ffi::OsStringExt;

        let path = OsString::from_vec(b"runtime-\xff-data".to_vec());
        let config = runtime_config(
            [
                OsString::from("--headless-root-frame"),
                OsString::from("--runtime-data"),
                path.clone(),
            ],
            None,
            None,
        )
        .unwrap();

        assert_eq!(config.runtime_data().as_os_str(), path.as_os_str());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_non_utf8_stage_names() {
        use std::os::unix::ffi::OsStringExt;

        let stage = OsString::from_vec(b"stage-\xff".to_vec());
        assert_eq!(
            AppOptions::parse([
                OsString::from("--headless-stage-frame"),
                OsString::from("--stage"),
                stage.clone(),
            ]),
            Err(ConfigError::NonUtf8Stage(stage))
        );
    }

    #[test]
    fn source_option_wins_over_its_environment_fallback_and_builds_data_paths() {
        let config = runtime_config(
            arguments(&[
                "--windowed-root-frame",
                "--runtime-data",
                "/user",
                "--source",
                "/media/explicit",
            ]),
            None,
            Some(OsString::from("/media/env")),
        )
        .unwrap();

        assert_eq!(config.media_root(), Some(Path::new("/media/explicit")));
        assert_eq!(config.media_source(), Some(RuntimeDataSource::CommandLine));
        let paths = config.data_paths().unwrap();
        assert_eq!(paths.settings_file(), Path::new("/user/save/cmRally.cfg"));
        assert_eq!(paths.game_root(), Path::new("/media/explicit/game"));
    }

    #[test]
    fn source_environment_is_used_only_as_a_fallback() {
        let config = runtime_config(
            arguments(&["--windowed-root-frame", "--runtime-data", "/user"]),
            None,
            Some(OsString::from("/media/env")),
        )
        .unwrap();
        assert_eq!(config.media_root(), Some(Path::new("/media/env")));
        assert_eq!(config.media_source(), Some(RuntimeDataSource::Environment));
    }

    #[test]
    fn media_root_is_optional_and_empty_source_is_rejected() {
        // No source at all: data paths cannot be built, but config is valid.
        let config = runtime_config(
            arguments(&["--headless-root-frame", "--runtime-data", "/user"]),
            None,
            None,
        )
        .unwrap();
        assert_eq!(config.media_root(), None);
        assert!(config.data_paths().is_none());
        // An explicitly empty source is an error.
        assert_eq!(
            runtime_config(
                arguments(&[
                    "--windowed-root-frame",
                    "--runtime-data",
                    "/user",
                    "--source",
                    "",
                ]),
                None,
                None,
            ),
            Err(ConfigError::EmptySource(RuntimeDataSource::CommandLine))
        );
        assert_eq!(
            runtime_config(
                arguments(&["--windowed-root-frame", "--runtime-data", "/user"]),
                None,
                Some(OsString::new()),
            ),
            Err(ConfigError::EmptySource(RuntimeDataSource::Environment))
        );
    }

    #[test]
    fn duplicate_source_option_is_rejected() {
        assert_eq!(
            AppOptions::parse(arguments(&[
                "--windowed-root-frame",
                "--source",
                "one",
                "--source",
                "two",
            ])),
            Err(ConfigError::DuplicateOption("--source"))
        );
    }

    #[test]
    fn stage_command_resolves_with_a_stage_name_and_media_root() {
        let config = runtime_config(
            arguments(&[
                "--headless-stage-frame",
                "--runtime-data",
                "/user",
                "--source",
                "/media",
                "--stage",
                "MONACO1",
            ]),
            None,
            None,
        )
        .unwrap();

        assert_eq!(config.command(), AppCommand::HeadlessStageFrame);
        assert_eq!(config.stage(), Some("MONACO1"));
        assert_eq!(config.media_root(), Some(Path::new("/media")));
    }

    #[test]
    fn windowed_stage_command_uses_the_same_stage_contract() {
        let config = runtime_config(
            arguments(&[
                "--windowed-stage-frame",
                "--runtime-data",
                "/user",
                "--source",
                "/media",
                "--stage",
                "MONACO1",
            ]),
            None,
            None,
        )
        .unwrap();
        assert_eq!(config.command(), AppCommand::WindowedStageFrame);
        assert_eq!(config.stage(), Some("MONACO1"));
        assert_eq!(config.media_root(), Some(Path::new("/media")));

        // It requires a media root just like the headless stage command.
        assert_eq!(
            runtime_config(
                arguments(&[
                    "--windowed-stage-frame",
                    "--runtime-data",
                    "/user",
                    "--stage",
                    "MONACO1",
                ]),
                None,
                None,
            ),
            Err(ConfigError::MissingSourceForStage)
        );
    }

    #[test]
    fn windowed_game_session_command_uses_the_stage_contract() {
        let config = runtime_config(
            arguments(&[
                "--windowed-game-session",
                "--runtime-data",
                "/user",
                "--source",
                "/media",
                "--stage",
                "MONACO1",
            ]),
            None,
            None,
        )
        .unwrap();
        assert_eq!(config.command(), AppCommand::WindowedGameSession);
        assert_eq!(config.stage(), Some("MONACO1"));
        assert_eq!(config.media_root(), Some(Path::new("/media")));

        // Like the stage commands, it needs both a stage name and a media root.
        assert_eq!(
            runtime_config(
                arguments(&[
                    "--windowed-game-session",
                    "--runtime-data",
                    "/user",
                    "--source",
                    "/media"
                ]),
                None,
                None,
            ),
            Err(ConfigError::MissingStage)
        );
        assert_eq!(
            runtime_config(
                arguments(&[
                    "--windowed-game-session",
                    "--runtime-data",
                    "/user",
                    "--stage",
                    "MONACO1",
                ]),
                None,
                None,
            ),
            Err(ConfigError::MissingSourceForStage)
        );
    }

    #[test]
    fn stage_command_takes_its_media_root_from_the_environment_fallback() {
        let config = runtime_config(
            arguments(&[
                "--headless-stage-frame",
                "--runtime-data",
                "/user",
                "--stage",
                "MONACO1",
            ]),
            None,
            Some(OsString::from("/media/env")),
        )
        .unwrap();

        assert_eq!(config.media_root(), Some(Path::new("/media/env")));
        assert_eq!(config.media_source(), Some(RuntimeDataSource::Environment));
    }

    #[test]
    fn stage_command_requires_a_stage_and_a_media_root() {
        assert_eq!(
            runtime_config(
                arguments(&[
                    "--headless-stage-frame",
                    "--runtime-data",
                    "/user",
                    "--source",
                    "/media",
                ]),
                None,
                None,
            ),
            Err(ConfigError::MissingStage)
        );
        assert_eq!(
            runtime_config(
                arguments(&[
                    "--headless-stage-frame",
                    "--runtime-data",
                    "/user",
                    "--stage",
                    "MONACO1",
                ]),
                None,
                None,
            ),
            Err(ConfigError::MissingSourceForStage)
        );
    }

    #[test]
    fn empty_and_duplicate_stage_options_are_rejected() {
        assert_eq!(
            runtime_config(
                arguments(&[
                    "--headless-stage-frame",
                    "--runtime-data",
                    "/user",
                    "--source",
                    "/media",
                    "--stage",
                    "",
                ]),
                None,
                None,
            ),
            Err(ConfigError::EmptyStage)
        );
        assert_eq!(
            AppOptions::parse(arguments(&[
                "--headless-stage-frame",
                "--stage",
                "one",
                "--stage",
                "two",
            ])),
            Err(ConfigError::DuplicateOption("--stage"))
        );
        assert_eq!(
            AppOptions::parse(arguments(&["--headless-stage-frame", "--stage"])),
            Err(ConfigError::MissingOptionValue("--stage"))
        );
    }

    #[test]
    fn startup_settings_gate_matches_the_recovered_loader() {
        use cmr_core::settings_store::{SETTINGS_BLOB_LEN, SettingsError};

        // Absent file -> shipped defaults.
        assert_eq!(resolve_startup_settings(None), StartupSettings::Defaults);
        // Exact length -> loaded, runtime region cleared.
        let mut raw = vec![0xabu8; SETTINGS_BLOB_LEN];
        raw[0x184] = 0x55;
        match resolve_startup_settings(Some(&raw)) {
            StartupSettings::Loaded(image) => assert_eq!(image.as_bytes()[0x184], 0),
            other => panic!("expected Loaded, got {other:?}"),
        }
        // Wrong length -> rejected, defaults stand.
        assert_eq!(
            resolve_startup_settings(Some(&[0u8; 8])),
            StartupSettings::Rejected(SettingsError::WrongLength {
                expected: SETTINGS_BLOB_LEN,
                actual: 8,
            })
        );
    }
}
