//! Platform-neutral process configuration for the native application.
//!
//! This crate deliberately does not choose a renderer or inspect the current
//! directory. The native host supplies process arguments and the optional
//! environment fallback explicitly, which keeps data selection deterministic
//! and straightforward to exercise in contract tests.

use std::convert::Infallible;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::{Path, PathBuf};

use cmr_assets::{
    AssetStore, AssetStoreError, CarSelectorStore, CarSelectorStoreError, CompiledInitializerStore,
    CompiledInitializerStoreError, EngineAudioTablesStore, EngineAudioTablesStoreError,
    ReviewedCarSelectorProfile, ReviewedCarSelectorProfileError,
    ReviewedCompiledInitializerProfile, ReviewedCompiledInitializerProfileError,
    ReviewedEngineAudioTablesProfile, ReviewedEngineAudioTablesProfileError, ReviewedProfile,
    ReviewedProfileError, ReviewedSurfaceAudioTablesProfile, ReviewedSurfaceCapabilityProfile,
    ReviewedSurfaceProfile, ReviewedSurfaceProfileError, ReviewedTyreTablesProfile,
    ReviewedTyreTablesProfileError, SurfaceAudioTablesStore, SurfaceCapabilityStore,
    SurfaceTableStore, SurfaceTableStoreError, TyreTablesStore, TyreTablesStoreError,
};
use cmr_core::audio_ambience::{AmbienceChannel, AmbienceController};
use cmr_core::audio_car_assets::{
    CAR_WAVE_BITS, CAR_WAVE_RATE_HZ, car_row_index_for_selector, car_wave_names,
    car_wave_source_path,
};
use cmr_core::audio_menu::{MenuChannel, MenuSound as MenuCue, play_menu_sound};
use cmr_core::audio_mixer::{AudioMixer, ChannelId, WaveSlotId, apply_commands};
use cmr_core::audio_render::{FileWaveSource, SoftwareMixer};
use cmr_core::audio_weather_assets::load_weather_ambience;
use cmr_core::c3d_camera::MATRIX_IDENTITY_Q16;
use cmr_core::c3d_mesh::node_mesh_vertex_type;
use cmr_core::c3d_render::VERTEXTYPE_LVERTEX;
use cmr_core::c3d_stage_scene::{StageScene, StageSceneError};
use cmr_core::c3d_transform::scene_world_transforms;
use cmr_core::camera_mode::{CameraMode, ViewGate};
use cmr_core::camera_view::ViewController;
use cmr_core::car_resources::{InRaceBodyLoadBranch, car_body_path, in_race_car_load_policy};
use cmr_core::compiled_car_initializer::{
    CompiledCaseWords, CompiledControlFamilies, CompiledDirectCaseSet, CompiledDirectCaseSetError,
    CompiledEightLaneExecutionFailure, CompiledEightLaneExecutionState, CompiledFourWheelConstants,
    CompiledHalfSignState, CompiledLaneDispatchFailurePhase, CompiledPostLaneBasisEffects,
    CompiledSetupPoseError, CompiledSetupPoseInputs, CompiledSetupPoseLaneInputs,
    DecodedCompiledLaneGeometryError, RecoveredCompiledStageError, derive_compiled_half_sign_state,
    execute_stage_compiled_setup_pose,
};
use cmr_core::crowd_sound::{crowd_cue, crowd_points};
use cmr_core::deferred_palette::DeferredPaletteCommitOutcome;
use cmr_core::descriptor_menu::MenuSound as DescriptorSound;
use cmr_core::display::{CooperativeMode, DisplayExtent, IndexedMode};
use cmr_core::drive_controls::DriveResponse;
use cmr_core::drivetrain::{
    SURFACE_CAPABILITY_COUNT, SURFACE_ID_COUNT, ZERO_INITIALIZED_RUNTIME_FIFTH_CAPABILITY_TABLE,
    accumulate_effect_enables_from_extracted,
};
use cmr_core::engine_listener::point_azimuth;
use cmr_core::engine_listener::{ListenerInput, update_listener};
use cmr_core::engine_sound::{
    EngineAudioTables, EngineSoundConfig, EngineSoundState, EngineVoices, RevBandTable,
    antilag_family_for_nibble,
};
use cmr_core::event_flow::EventState;
use cmr_core::fixed_point::{Fixed16, Vec3};
use cmr_core::frontend::{FrontendPhase, RootSelection};
use cmr_core::game_menu_flow::MenuFlowInputs;
use cmr_core::game_rng::GameLcg;
use cmr_core::game_session::SessionBand;
use cmr_core::gameplay_input::{AnalogDriver, KeyboardDriver};
use cmr_core::gameplay_session::PauseFlow;
use cmr_core::gameplay_session::{GameplaySession, InRaceScript, StaticGameplayAdvanceError};
use cmr_core::host_loop::HostClock;
use cmr_core::load_interstitial::{LOADING_BAR_BMP, LOADING_LINE_BMP, STAGE_LOADING_PROGRESS_CAP};
use cmr_core::localization::Language;
use cmr_core::pause_menu::{
    DrivingPauseInput, PauseController, PauseDecision, PauseEffect, PauseKeys, PauseMenuConditions,
    PauseOptions,
};
use cmr_core::pause_menu_draw::{PauseDisplay, PauseFont, pause_draw_plan};
use cmr_core::pause_request::PauseKeyGates;
use cmr_core::prang_sound::{
    PrangSoundState, PrangVoices, ThumpInput, barrier_object_type, impact_severity, play_impact,
    prang_wave_paths,
};
use cmr_core::race_hud_plan::{
    CommonGaugeInset, RaceHudBitmap, RaceHudFont, RaceHudPlanError, RaceHudPreparedInputs,
    plan_normal_single_player_race_hud,
};
use cmr_core::race_state::{
    CarState, Controls, FirstWheelParams, FourWheelIterationParams, FourWheelPostprocessParams,
    SecondWheelParams, SimParams, WheelPostprocessError, populate_compiled_contact_offsets,
    populate_compiled_gear_ratio, populate_force_pass_scales, populate_surface_table_factor,
};
use cmr_core::render_compose::{fade_duration_frames, fade_level_to_modulation_byte, fade_out};
use cmr_core::render_route::{RenderRoute, presents_world_scene};
use cmr_core::root_menu_assets::VisualVariant;
use cmr_core::root_menu_interactive::RootInteraction;
use cmr_core::root_menu_visual::{
    ROOT_INITIAL_HORIZONTAL_TRANSFORM_Q16, ROOT_INITIAL_VERTICAL_TRANSFORM_Q16, RootRenderInput,
};
use cmr_core::scalar_tween::ScalarTween;
use cmr_core::settings_store::{DataPaths, SettingsError, SettingsImage};
use cmr_core::stage_checkpoints::StageProgress;
use cmr_core::stage_compiled_geometry::{StageCompiledGeometry, StageCompiledGeometryLoadError};
use cmr_core::stage_timing::elapsed_centiseconds;
use cmr_core::startup::{StartupLanguage, StartupScreen};
use cmr_core::startup_display::{
    StartupDisplay, StartupDisplayError, StartupDisplayRequest, initialize_startup_display,
};
use cmr_core::static_collision_kernel::NoStaticDamageOwner;
use cmr_core::static_collision_scene_builder::{
    ParsedStaticCollisionBuildError, ParsedStaticCollisionScene,
    build_parsed_static_collision_scene,
};
use cmr_core::surface::PaletteMapping;
use cmr_core::surface_sound::{
    SurfaceAudioTables, SurfaceSoundState, SurfaceTickInput, load_surface_bank, set_surface_class,
    stop_surface_channels,
};
use cmr_core::vehicle_dynamics::{
    AngularMomentumStepInput, AngularOrientationInput, ControlAxisFlags, GeometricPostprocessLane,
    SOLVER_GLOBAL_CONSTANTS, SurfaceCoefficientIndexError, integrate_orientation,
};
use cmr_core::weather::{StageWeatherEffects, particles};
use cmr_core::wet_sound::{WetLoad, WetSoundState, WetTickInput, load_wet_bank};
use cmr_formats::bitmap::{BitmapError, BitmapLimits, IndexedBitmap, parse_indexed_bitmap};
use cmr_formats::c3d::{C3dError, C3dFile, C3dLimits, parse_c3d};
use cmr_formats::font::{BitmapFont, FontError, FontLimits, parse_legacy_font};
use cmr_formats::riff_palette::{PaletteEntry, PaletteError, parse_legacy_palette_256};
use cmr_formats::stage::CatNode;
use cmr_formats::stage::{
    ARS_EXTENSION, C3D_EXTENSION, CAT_EXTENSION, CSP_EXTENSION, CspStart, DAT_EXTENSION,
    HZN_EXTENSION, OBJ_EXTENSION, SHT_EXTENSION, StageBundle, StageLoadError, StageSectionSource,
    TSC_EXTENSION, load_stage,
};
use cmr_formats::stored_asset::{DecodeError, DecodeLimits, decode_stored_asset};
use cmr_gpu_contract::{PersistentRawC3dGpuBackend, PersistentRawC3dIndexedBackend};
use cmr_gpu_stage::{
    PreparedRawStage, PreparedRawStageError, PreparedRawStageLayer, PreparedRawStageStats,
};
use cmr_platform_sdl3::SharedSoftwareMixer;
use cmr_platform_sdl3::input::GamepadDrivingState;
use cmr_platform_sdl3::{
    InteractiveRootError, SdlHostError, WindowedGameFrame, WindowedInput, present_software_frame,
    run_interactive_root_frame, run_windowed_game_session as run_windowed_game_session_loop,
};
use cmr_video::c3d_software_raster::SoftwareC3dIndexedQuantization;
use cmr_video::software_load_interstitial::{
    LoadInterstitialComposeError, compose_load_interstitial,
};
use cmr_video::software_pause_overlay::compose_pause_overlay;
use cmr_video::software_race_hud::{RaceHudComposeError, compose_race_hud_commands};
use cmr_video::software_root_menu_session::{
    SoftwareRootMenuRenderError, SoftwareRootMenuSessionBuildError, SoftwareRootMenuSessionRequest,
    with_software_root_menu_session,
};
use cmr_video::software_stage_scene::{
    StageC3dTextureError, StageC3dTextures, StageSceneComposeError, StageSceneFrameStats,
    compose_stage_scene_frame, compose_stage_scene_frame_color_key,
    compose_stage_scene_frame_with_projection,
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
const REVIEWED_COMPILED_INITIALIZER_PROFILE: &[u8] =
    include_bytes!("../../../profiles/official-eu-compiled-car-initializer-v1.json");
const COMPILED_INITIALIZER_STORE_DIRECTORY: &str = "official-eu-compiled-car-initializer";
const REVIEWED_CAR_SELECTOR_PROFILE: &[u8] =
    include_bytes!("../../../profiles/official-eu-car-selector-map-v1.json");
const CAR_SELECTOR_STORE_DIRECTORY: &str = "official-eu-car-selectors";
const REVIEWED_TYRE_TABLES_PROFILE: &[u8] =
    include_bytes!("../../../profiles/official-eu-tyre-surface-tables-v1.json");
const TYRE_TABLES_STORE_DIRECTORY: &str = "official-eu-tyre-surface-tables";
const REVIEWED_ENGINE_AUDIO_TABLES_PROFILE: &[u8] =
    include_bytes!("../../../profiles/official-eu-engine-audio-tables-v1.json");
const ENGINE_AUDIO_TABLES_STORE_DIRECTORY: &str = "official-eu-engine-audio-tables";
const REVIEWED_SURFACE_AUDIO_TABLES_PROFILE: &[u8] =
    include_bytes!("../../../profiles/official-eu-surface-audio-tables-v1.json");
const SURFACE_AUDIO_TABLES_STORE_DIRECTORY: &str = "official-eu-surface-audio-tables";
const REVIEWED_AUX_CONTROL_PROFILE: &[u8] =
    include_bytes!("../../../profiles/official-eu-aux-control-constants-v1.json");
const AUX_CONTROL_STORE_DIRECTORY: &str = "official-eu-aux-control-constants";
const ROOT_EXTENT: DisplayExtent = DisplayExtent {
    width: 640,
    height: 480,
};
// Temporary native-host policy: the windowed harness does not yet publish the
// three recovered runtime predicates selecting the mode-5 versus mode-6 body
// branch. Keep its existing keyed-body behavior explicit; this is not a claim
// that every ordinary rally takes the native mode-5 branch.
const WINDOWED_BODY_LOAD_BRANCH: InRaceBodyLoadBranch = InRaceBodyLoadBranch::KeyedMode5;
const LOADING_BACKGROUND_EXTENSION: &str = ".loa";

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

/// Where an automatic GPU-to-software fallback occurred.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StageRendererFallbackPhase {
    /// GPU preparation, device initialization, or persistent upload.
    Setup,
    /// Per-frame GPU composition or readback.
    Frame,
}

impl StageRendererFallbackPhase {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Setup => "setup",
            Self::Frame => "frame",
        }
    }
}

/// Path-free reason retained when `auto` falls back to the software oracle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StageRendererFallback {
    pub phase: StageRendererFallbackPhase,
    pub reason: &'static str,
}

/// Stable renderer selection diagnostics for a loaded stage session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StageRendererStatus {
    pub requested: StageRendererChoice,
    pub selected: StageRendererKind,
    pub fallback: Option<StageRendererFallback>,
}

impl StageRendererStatus {
    /// A path-free line suitable for native-session startup diagnostics.
    #[must_use]
    pub fn stable_summary(self) -> String {
        let fallback = self.fallback.map_or_else(
            || "none".to_owned(),
            |fallback| format!("{}:{}", fallback.phase.name(), fallback.reason),
        );
        format!(
            "renderer requested={} selected={} fallback={fallback}",
            self.requested.name(),
            self.selected.name(),
        )
    }
}

/// Parsed command-line options before environment fallback resolution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppOptions {
    command: AppCommand,
    runtime_data: Option<PathBuf>,
    source: Option<PathBuf>,
    stage: Option<String>,
    car_code: Option<String>,
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
        let mut car_code = None;
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
            if argument == OsStr::new("--car-code") {
                if car_code.is_some() {
                    return Err(ConfigError::DuplicateOption("--car-code"));
                }
                let value = arguments
                    .next()
                    .ok_or(ConfigError::MissingOptionValue("--car-code"))?;
                car_code = Some(value.into_string().map_err(ConfigError::NonUtf8CarCode)?);
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
            car_code,
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
        if let Some(code) = &self.car_code
            && (code.is_empty()
                || !code
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'))
        {
            return Err(ConfigError::InvalidCarCode(code.clone()));
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
        if self.command == AppCommand::WindowedGameSession && self.car_code.is_none() {
            return Err(ConfigError::MissingCarCode);
        }

        Ok(RuntimeConfig {
            command: self.command,
            runtime_data,
            source,
            media,
            stage: self.stage,
            car_code: self.car_code,
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
    car_code: Option<String>,
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

    /// Caller-selected car asset code for the direct-to-stage harness.
    pub fn car_code(&self) -> Option<&str> {
        self.car_code.as_deref()
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
    /// `--car-code` carried a non-UTF-8 identifier.
    NonUtf8CarCode(OsString),
    /// Direct-to-stage play requires a caller-selected car asset code.
    MissingCarCode,
    /// Car codes are single safe path components.
    InvalidCarCode(String),
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
            Self::NonUtf8CarCode(value) => {
                write!(formatter, "--car-code must be valid UTF-8, got {value:?}")
            }
            Self::MissingCarCode => write!(
                formatter,
                "--windowed-game-session requires a car; pass --car-code CODE"
            ),
            Self::InvalidCarCode(value) => write!(
                formatter,
                "invalid car code {value:?}; expected one ASCII path component"
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
/// for the sections the bundle loader consumes (`.c3d`, `.dat`, `.cat`, ...): it reads
/// `<media_root>/<stage><ext>`, runs each file through [`decode_stored_asset`]
/// (so a CMPR-wrapped sibling is transparently inflated), and keeps the decoded
/// bytes. A missing sibling is simply absent; only an I/O or decode failure is an
/// error. Original media is read in place and never copied into the repository.
pub struct FsStageSectionSource {
    c3d: Option<Vec<u8>>,
    cat: Option<Vec<u8>>,
    hzn: Option<Vec<u8>>,
    ars: Option<Vec<u8>>,
    obj: Option<Vec<u8>>,
    sht: Option<Vec<u8>>,
    csp: Option<Vec<u8>>,
    dat: Option<Vec<u8>>,
    tsc: Option<Vec<u8>>,
}

impl FsStageSectionSource {
    /// Reads all currently decoded siblings of `stage` under `media_root`.
    pub fn read(
        media_root: &Path,
        stage: &str,
        limits: DecodeLimits,
    ) -> Result<Self, StageFrameError> {
        Ok(Self {
            c3d: read_decoded_section(media_root, stage, C3D_EXTENSION, limits)?,
            cat: read_decoded_section(media_root, stage, CAT_EXTENSION, limits)?,
            hzn: read_decoded_section(media_root, stage, HZN_EXTENSION, limits)?,
            ars: read_decoded_section(media_root, stage, ARS_EXTENSION, limits)?,
            obj: read_decoded_section(media_root, stage, OBJ_EXTENSION, limits)?,
            sht: read_decoded_section(media_root, stage, SHT_EXTENSION, limits)?,
            csp: read_decoded_section(media_root, stage, CSP_EXTENSION, limits)?,
            dat: read_decoded_section(media_root, stage, DAT_EXTENSION, limits)?,
            tsc: read_decoded_section(media_root, stage, TSC_EXTENSION, limits)?,
        })
    }
}

impl StageSectionSource for FsStageSectionSource {
    fn section(&self, extension: &str) -> Option<&[u8]> {
        match extension {
            C3D_EXTENSION => self.c3d.as_deref(),
            CAT_EXTENSION => self.cat.as_deref(),
            HZN_EXTENSION => self.hzn.as_deref(),
            ARS_EXTENSION => self.ars.as_deref(),
            OBJ_EXTENSION => self.obj.as_deref(),
            SHT_EXTENSION => self.sht.as_deref(),
            CSP_EXTENSION => self.csp.as_deref(),
            DAT_EXTENSION => self.dat.as_deref(),
            TSC_EXTENSION => self.tsc.as_deref(),
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

/// Reads the Windows stage-loading sibling while tolerating the ASCII case
/// variants found on case-sensitive installs. The shipped lookup is
/// case-insensitive; extraction tools commonly materialize `AU1.LOA` as
/// `au1.loa` on Linux.
fn read_loading_background(
    media_root: &Path,
    stage: &str,
    limits: DecodeLimits,
) -> Result<Option<Vec<u8>>, StageFrameError> {
    let lower_stage = stage.to_ascii_lowercase();
    let candidates = [
        format!("{stage}.LOA"),
        format!("{stage}{LOADING_BACKGROUND_EXTENSION}"),
        format!("{lower_stage}.LOA"),
        format!("{lower_stage}{LOADING_BACKGROUND_EXTENSION}"),
    ];
    let mut attempted = Vec::<PathBuf>::new();
    for candidate in candidates {
        let path = media_root.join(candidate);
        if attempted.contains(&path) {
            continue;
        }
        attempted.push(path.clone());
        let raw = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(StageFrameError::Io {
                    extension: LOADING_BACKGROUND_EXTENSION,
                    error,
                });
            }
        };
        let decoded =
            decode_stored_asset(&raw, limits).map_err(|error| StageFrameError::Decode {
                extension: LOADING_BACKGROUND_EXTENSION,
                error,
            })?;
        return Ok(Some(decoded.bytes().to_vec()));
    }
    Ok(None)
}

fn parse_loading_background(bytes: &[u8]) -> Result<SoftwarePresentedFrame, StageFrameError> {
    let bitmap = parse_indexed_bitmap(bytes, BitmapLimits::default())
        .map_err(StageFrameError::LoadingBitmap)?;
    if bitmap.width != u32::from(ROOT_EXTENT.width)
        || bitmap.height != u32::from(ROOT_EXTENT.height)
    {
        return Err(StageFrameError::LoadingExtent {
            width: bitmap.width,
            height: bitmap.height,
        });
    }
    let entries = bitmap.palette.len();
    let palette: [PaletteEntry; 256] = bitmap
        .palette
        .into_iter()
        .map(|entry| PaletteEntry {
            red: entry.red,
            green: entry.green,
            blue: entry.blue,
            flags: entry.reserved,
        })
        .collect::<Vec<_>>()
        .try_into()
        .map_err(|_| StageFrameError::LoadingPalette { entries })?;
    SoftwarePresentedFrame::from_indexed(
        ROOT_EXTENT,
        bitmap.pixels.into_boxed_slice(),
        Box::new(palette),
    )
    .map_err(StageFrameError::Present)
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
    /// Parsed main-C3D/OBJ data could not be translated into the recovered
    /// immutable static-collision scene.
    StaticCollision(ParsedStaticCollisionBuildError),
    /// The stage scene could not be composed into a frame.
    Compose(StageSceneComposeError),
    /// The composed frame could not be bridged to the presentation type.
    Present(SoftwareRgba8Error),
    /// A selected-stage loading background was not a valid indexed bitmap.
    LoadingBitmap(BitmapError),
    /// A selected-stage loading background did not match the recovered viewport.
    LoadingExtent {
        width: u32,
        height: u32,
    },
    /// A selected-stage loading background did not carry the full display palette.
    LoadingPalette {
        entries: usize,
    },
    /// A recovered loading-overlay source could not be loaded from the verified store.
    LoadingOverlayAsset {
        bitmap: LoadingOverlayBitmap,
        error: AssetStoreError,
    },
    /// The reviewed runtime-data store could not be opened for loading artwork.
    LoadingOverlayStore(AssetStoreError),
    /// A recovered loading-overlay source was not a valid indexed bitmap.
    LoadingOverlayBitmap {
        bitmap: LoadingOverlayBitmap,
        error: BitmapError,
    },
    /// The retained loading-overlay sources were absent at composition time.
    MissingLoadingOverlayAssets,
    /// The recovered loading-overlay commands could not be composed.
    LoadingOverlayCompose(LoadInterstitialComposeError),
    MissingNodes,
    DegenerateSpawn,
    UnknownPaletteStage(String),
    Palette(AssetStoreError),
    PaletteFormat(PaletteError),
    Profile(ReviewedProfileError),
    /// The caller-selected in-race car could not be loaded from the verified store.
    CarAsset(AssetStoreError),
    /// The selected car body is not a valid C3D scene.
    CarScene(C3dError),
    /// A race-HUD source asset could not be loaded from the verified store.
    HudAsset(AssetStoreError),
    /// A race-HUD bitmap failed bounded indexed-bitmap parsing.
    HudBitmap {
        bitmap: RaceHudBitmap,
        error: BitmapError,
    },
    /// A race-HUD font failed bounded legacy-font parsing.
    HudFont {
        font: RaceHudFont,
        error: FontError,
    },
    /// A race-HUD plan could not be produced for the live viewport.
    HudPlan(RaceHudPlanError),
    /// The recovered HUD commands could not be composed over the world frame.
    HudCompose(RaceHudComposeError),
    /// Native setup did not retain the authenticated HUD sources.
    MissingHudAssets,
    /// The raw-GPU stage adapter rejected the scene.
    GpuPrepare(PreparedRawStageError),
    /// The selected GPU backend could not be initialized (or its persistent
    /// scene could not be uploaded).
    GpuInit(String),
    /// The selected GPU backend failed to render or read back a frame.
    GpuCompose(String),
    /// The current GPU contract cannot yet express the separate horizon pass.
    GpuLayeredStageUnsupported,
    /// A horizon scene was present without the settings needed to place it.
    MissingHorizonSettings,
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
            Self::StaticCollision(_) => "static-collision",
            Self::Compose(_) => "compose",
            Self::Present(_) => "present",
            Self::LoadingBitmap(_) => "loading-bitmap",
            Self::LoadingExtent { .. } => "loading-extent",
            Self::LoadingPalette { .. } => "loading-palette",
            Self::LoadingOverlayAsset { .. } => "loading-overlay-asset",
            Self::LoadingOverlayStore(_) => "loading-overlay-store",
            Self::LoadingOverlayBitmap { .. } => "loading-overlay-bitmap",
            Self::MissingLoadingOverlayAssets => "missing-loading-overlay-assets",
            Self::LoadingOverlayCompose(_) => "loading-overlay-compose",
            Self::MissingNodes => "missing-nodes",
            Self::DegenerateSpawn => "degenerate-spawn",
            Self::UnknownPaletteStage(_) => "unknown-palette-stage",
            Self::Palette(_) => "palette",
            Self::PaletteFormat(_) => "palette-format",
            Self::Profile(_) => "profile",
            Self::CarAsset(_) => "car-asset",
            Self::CarScene(_) => "car-scene",
            Self::HudAsset(_) => "hud-asset",
            Self::HudBitmap { .. } => "hud-bitmap",
            Self::HudFont { .. } => "hud-font",
            Self::HudPlan(_) => "hud-plan",
            Self::HudCompose(_) => "hud-compose",
            Self::MissingHudAssets => "missing-hud-assets",
            Self::GpuPrepare(_) => "gpu-prepare",
            Self::GpuInit(_) => "gpu-init",
            Self::GpuCompose(_) => "gpu-compose",
            Self::GpuLayeredStageUnsupported => "gpu-layered-stage-unsupported",
            Self::MissingHorizonSettings => "missing-horizon-settings",
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
            Self::StaticCollision(error) => {
                write!(formatter, "cannot build static collision scene: {error}")
            }
            Self::Compose(error) => write!(formatter, "cannot compose stage frame: {error}"),
            Self::Present(error) => write!(formatter, "cannot present stage frame: {error}"),
            Self::LoadingBitmap(error) => {
                write!(
                    formatter,
                    "cannot parse selected-stage loading bitmap: {error}"
                )
            }
            Self::LoadingExtent { width, height } => write!(
                formatter,
                "selected-stage loading bitmap is {width}x{height}; expected {}x{}",
                ROOT_EXTENT.width, ROOT_EXTENT.height
            ),
            Self::LoadingPalette { entries } => write!(
                formatter,
                "selected-stage loading bitmap has {entries} palette entries; expected 256"
            ),
            Self::LoadingOverlayAsset { bitmap, error } => write!(
                formatter,
                "cannot load loading-overlay bitmap {}: {error}",
                bitmap.store_path()
            ),
            Self::LoadingOverlayStore(error) => {
                write!(
                    formatter,
                    "cannot open loading-overlay asset store: {error}"
                )
            }
            Self::LoadingOverlayBitmap { bitmap, error } => write!(
                formatter,
                "cannot parse loading-overlay bitmap {}: {error}",
                bitmap.store_path()
            ),
            Self::MissingLoadingOverlayAssets => {
                write!(formatter, "native loading-overlay assets were not attached")
            }
            Self::LoadingOverlayCompose(error) => {
                write!(formatter, "cannot compose loading overlay: {error}")
            }
            Self::MissingNodes => write!(formatter, "stage has no checkpoint nodes"),
            Self::DegenerateSpawn => write!(formatter, "stage spawn has no horizontal direction"),
            Self::UnknownPaletteStage(stage) => {
                write!(formatter, "no country palette mapping for stage {stage}")
            }
            Self::Palette(error) => write!(formatter, "cannot load stage palette: {error}"),
            Self::PaletteFormat(error) => write!(formatter, "cannot parse stage palette: {error}"),
            Self::Profile(error) => write!(formatter, "cannot read reviewed profile: {error}"),
            Self::CarAsset(error) => write!(formatter, "cannot load in-race car: {error}"),
            Self::CarScene(error) => write!(formatter, "cannot parse in-race car: {error}"),
            Self::HudAsset(error) => write!(formatter, "cannot load race HUD asset: {error}"),
            Self::HudBitmap { bitmap, error } => write!(
                formatter,
                "cannot parse race HUD bitmap {}: {error}",
                bitmap.relative_path()
            ),
            Self::HudFont { font, error } => write!(
                formatter,
                "cannot parse race HUD font {}: {error}",
                font.relative_path()
            ),
            Self::HudPlan(error) => write!(formatter, "cannot plan race HUD: {error:?}"),
            Self::HudCompose(error) => write!(formatter, "cannot compose race HUD: {error}"),
            Self::MissingHudAssets => write!(formatter, "native race HUD assets were not attached"),
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
            Self::GpuLayeredStageUnsupported => write!(
                formatter,
                "the GPU stage contract does not yet implement the recovered multi-graph stage passes"
            ),
            Self::MissingHorizonSettings => {
                write!(
                    formatter,
                    "stage has a horizon but no .ars placement settings"
                )
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
            Self::StaticCollision(error) => Some(error),
            Self::Compose(error) => Some(error),
            Self::Present(error) => Some(error),
            Self::LoadingBitmap(error) => Some(error),
            Self::LoadingOverlayAsset { error, .. } => Some(error),
            Self::LoadingOverlayStore(error) => Some(error),
            Self::LoadingOverlayBitmap { error, .. } => Some(error),
            Self::LoadingOverlayCompose(error) => Some(error),
            Self::Palette(error) => Some(error),
            Self::PaletteFormat(error) => Some(error),
            Self::Profile(error) => Some(error),
            Self::CarAsset(error) => Some(error),
            Self::CarScene(error) => Some(error),
            Self::HudAsset(error) => Some(error),
            Self::HudBitmap { error, .. } => Some(error),
            Self::HudFont { error, .. } => Some(error),
            Self::HudCompose(error) => Some(error),
            Self::GpuPrepare(error) => Some(error),
            Self::MissingNodes
            | Self::DegenerateSpawn
            | Self::UnknownPaletteStage(_)
            | Self::GpuInit(_)
            | Self::GpuCompose(_)
            | Self::GpuLayeredStageUnsupported
            | Self::MissingHorizonSettings
            | Self::HudPlan(_)
            | Self::MissingHudAssets => None,
            Self::LoadingExtent { .. }
            | Self::LoadingPalette { .. }
            | Self::MissingLoadingOverlayAssets => None,
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

fn native_stage_palette_path(stage: &str) -> Result<String, StageFrameError> {
    let code = stage.get(..2).unwrap_or(stage).to_ascii_uppercase();
    let country = match code.as_str() {
        "AU" => "aus",
        "CO" => "cor",
        "GR" => "gre",
        "IN" => "ind",
        "MC" => "mon",
        "NZ" => "nz",
        "SW" => "swe",
        "UK" => "uk",
        "SC" => "school",
        _ => return Err(StageFrameError::UnknownPaletteStage(stage.to_owned())),
    };
    Ok(format!("Game/Palettes/Countries/{country}.pal"))
}

fn load_native_stage_palette(
    runtime_data: &Path,
    stage: &str,
) -> Result<[PaletteEntry; 256], StageFrameError> {
    let reviewed =
        ReviewedProfile::from_json(REVIEWED_PROFILE).map_err(StageFrameError::Profile)?;
    let store = AssetStore::open(runtime_data, &reviewed, DecodeLimits::default())
        .map_err(StageFrameError::Palette)?;
    let path = native_stage_palette_path(stage)?;
    let bytes = store.load(&path).map_err(StageFrameError::Palette)?;
    Ok(parse_legacy_palette_256(&bytes)
        .map_err(StageFrameError::PaletteFormat)?
        .entries)
}

type ActorMatrixRows = [[i32; 3]; 3];
type NativeStageStart = (CarState, [i32; 16], ActorMatrixRows);
type NativeStageSetup = (
    StageRenderResources,
    Vec<CatNode>,
    CarState,
    [i32; 16],
    ActorMatrixRows,
);

fn native_stage_start(nodes: &[CatNode]) -> Result<NativeStageStart, StageFrameError> {
    let node = nodes.first().ok_or(StageFrameError::MissingNodes)?;
    let [dir_x, dir_y, dir_z] = node.direction;
    if dir_x == 0 && dir_z == 0 {
        return Err(StageFrameError::DegenerateSpawn);
    }
    let position = Vec3::new(
        Fixed16::from_raw(node.position[0]),
        Fixed16::from_raw(node.position[1]),
        Fixed16::from_raw(node.position[2]),
    );
    let heading =
        ((dir_z as f64).atan2(dir_x as f64) * (4096.0 / std::f64::consts::TAU)).round() as i64;
    let heading = heading.rem_euclid(4096) as u16;
    let trig = cmr_core::fixed_point::build_sine_table();
    let world = cmr_core::gameplay_camera::car_world_q16(&trig, position, heading);
    let column = |index: usize| {
        Vec3::new(
            Fixed16::from_raw(world[index]),
            Fixed16::from_raw(world[index + 4]),
            Fixed16::from_raw(world[index + 8]),
        )
    };
    let mut start = CarState::at_rest();
    start.position = position;
    start.heading = heading;
    start.motion.basis = [column(0), column(1), column(2)];
    start.motion.projection_axis = Vec3::new(
        Fixed16::from_raw(dir_x),
        Fixed16::from_raw(dir_y),
        Fixed16::from_raw(dir_z),
    );
    start.motion.lane_source_s = column(2);
    let mut camera = ViewHarness::new();
    let initial_camera = camera.update(false, position, heading, false);
    let harness = start.motion.basis;
    let negate = |vector: Vec3| {
        [
            vector.x.raw().wrapping_neg(),
            vector.y.raw().wrapping_neg(),
            vector.z.raw().wrapping_neg(),
        ]
    };
    let components = |vector: Vec3| [vector.x.raw(), vector.y.raw(), vector.z.raw()];
    // The native car rolls along its `+0x258` row (the tyre model's lane
    // source), so the setup row 0 is the harness forward and row 2 its left.
    let actor_rows = [
        components(harness[2]),
        components(harness[1]),
        negate(harness[0]),
    ];
    Ok((start, initial_camera, actor_rows))
}

fn native_stage_start_from_csp(start_record: CspStart) -> NativeStageStart {
    let position = Vec3::new(
        Fixed16::from_raw(start_record.position[0]),
        Fixed16::from_raw(start_record.position[1]),
        Fixed16::from_raw(start_record.position[2]),
    );
    let trig = cmr_core::fixed_point::build_sine_table();
    let identity_basis = [
        Vec3::new(Fixed16::ONE, Fixed16::ZERO, Fixed16::ZERO),
        Vec3::new(Fixed16::ZERO, Fixed16::ONE, Fixed16::ZERO),
        Vec3::new(Fixed16::ZERO, Fixed16::ZERO, Fixed16::ONE),
    ];
    let angles = start_record.rotation_bam.map(|angle| angle as i16);
    let native_basis = integrate_orientation(identity_basis, angles, &trig)
        .expect("the complete 4096-entry trig table admits every CSP angle");
    // The integrated CSP basis is the native one: the car rolls along row 0
    // (`+0x258`, the tyre model's lane source), row 1 is up and row 2 its
    // left. The planar harness publishes `[right, up, forward]` columns.
    let negate = |vector: Vec3| {
        Vec3::new(
            Fixed16::from_raw(vector.x.raw().wrapping_neg()),
            Fixed16::from_raw(vector.y.raw().wrapping_neg()),
            Fixed16::from_raw(vector.z.raw().wrapping_neg()),
        )
    };
    let basis = [negate(native_basis[2]), native_basis[1], native_basis[0]];
    let forward = native_basis[0];
    let yaw = f64::from(forward.z.raw()).atan2(f64::from(forward.x.raw()));
    let heading = ((yaw * (4096.0 / std::f64::consts::TAU)).round() as i64).rem_euclid(4096) as u16;
    let mut start = CarState::at_rest();
    start.position = position;
    start.heading = heading;
    start.motion.basis = basis;
    start.motion.projection_axis = basis[2];
    start.motion.lane_source_s = basis[2];
    let mut camera = ViewHarness::new();
    let initial_camera = camera.update(false, position, heading, false);
    let actor_rows = native_basis.map(|vector| [vector.x.raw(), vector.y.raw(), vector.z.raw()]);
    (start, initial_camera, actor_rows)
}

/// The harness render pose of a four-wheel car: the yaw of its rolling row
/// `+0x258` as a 4096-unit heading for the camera (harness forward is
/// `(cos, 0, sin)`), and a car world matrix in the harness column layout built
/// from the full basis (right = `-row2`, up = `row1`, forward = `row0`).
fn four_wheel_render_pose(car: &CarState) -> (u16, [i32; 16]) {
    let [forward, up, left] = car.motion.basis;
    let yaw = f64::from(forward.z.raw()).atan2(f64::from(forward.x.raw()));
    let heading = (yaw * (4096.0 / std::f64::consts::TAU)).round() as i64;
    let heading = heading.rem_euclid(4096) as u16;
    let one = Fixed16::ONE.raw();
    let world = [
        left.x.raw().wrapping_neg(),
        left.y.raw().wrapping_neg(),
        left.z.raw().wrapping_neg(),
        0,
        up.x.raw(),
        up.y.raw(),
        up.z.raw(),
        0,
        forward.x.raw(),
        forward.y.raw(),
        forward.z.raw(),
        0,
        car.position.x.raw(),
        car.position.y.raw(),
        car.position.z.raw(),
        one,
    ];
    (heading, world)
}

fn car_model_world_q16(car_world: &[i32; 16]) -> [i32; 16] {
    // A controlled chase-camera differential identifies this as the rear-view
    // orientation: local +X maps to harness forward and local -Z maps to
    // harness right. Negating two transverse axes preserves a proper rotation
    // (and therefore winding). Wheel-node positions alone do not establish
    // front/rear semantics and are not used by this adapter.
    [
        car_world[8],
        car_world[9],
        car_world[10],
        0,
        car_world[4],
        car_world[5],
        car_world[6],
        0,
        car_world[0].wrapping_neg(),
        car_world[1].wrapping_neg(),
        car_world[2].wrapping_neg(),
        0,
        car_world[12],
        car_world[13],
        car_world[14],
        car_world[15],
    ]
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
    horizon_prepared: Option<PreparedRawStage>,
    horizon_scene: Option<<WgpuIndexedPaletteBackend as PersistentRawC3dGpuBackend>::PreparedScene>,
    palette: [PaletteEntry; 256],
}

impl WgpuStageRenderer {
    fn from_prepared(
        prepared: PreparedRawStage,
        horizon_prepared: Option<PreparedRawStage>,
    ) -> Result<Self, StageFrameError> {
        let palette = *prepared.upload().active_palette();
        let mut backend = WgpuIndexedPaletteBackend::new(&WgpuBackendConfig::default())
            .map_err(|error| StageFrameError::GpuInit(error.to_string()))?;
        let prepared_scene = backend
            .prepare_c3d_scene(prepared.upload())
            .map_err(|error| StageFrameError::GpuInit(error.to_string()))?;
        let horizon_scene = horizon_prepared
            .as_ref()
            .map(|horizon| backend.prepare_c3d_scene(horizon.upload()))
            .transpose()
            .map_err(|error| StageFrameError::GpuInit(error.to_string()))?;
        Ok(Self {
            backend,
            prepared,
            scene: prepared_scene,
            horizon_prepared,
            horizon_scene,
            palette,
        })
    }

    fn stats(&self) -> PreparedRawStageStats {
        self.prepared.stats()
    }

    /// Rasterizes one camera frame and returns it as the recovered indexed
    /// presentation value (active-palette slots + the active palette).
    fn compose(
        &mut self,
        camera: &[i32; 16],
        dynamic_worlds: Option<&[[i32; 16]]>,
        horizon_worlds: Option<&[[i32; 16]]>,
    ) -> Result<SoftwarePresentedFrame, StageFrameError> {
        let frame = if self.prepared.dynamic_world_set_count() == 0 {
            self.prepared.prepared_frame_for(camera)
        } else {
            let dynamic_worlds = dynamic_worlds.ok_or(StageFrameError::GpuPrepare(
                PreparedRawStageError::DynamicWorldSetCount {
                    expected: 1,
                    actual: 0,
                },
            ))?;
            self.prepared
                .prepared_frame_for_worlds(camera, &[dynamic_worlds])
                .map_err(StageFrameError::GpuPrepare)?
        };
        let indexed = if let (Some(horizon_prepared), Some(horizon_scene)) =
            (self.horizon_prepared.as_mut(), self.horizon_scene.as_mut())
        {
            let horizon_worlds = horizon_worlds.ok_or(StageFrameError::GpuPrepare(
                PreparedRawStageError::DynamicWorldSetCount {
                    expected: 1,
                    actual: 0,
                },
            ))?;
            let (w, h, z_near, _) = cmr_core::c3d_camera::PROJECTION_BASELINE;
            let horizon_frame = horizon_prepared
                .prepared_frame_for_worlds_with_projection(
                    camera,
                    &[horizon_worlds],
                    (w, h, z_near, cmr_core::c3d_camera::HORIZON_Z_FAR),
                )
                .map_err(StageFrameError::GpuPrepare)?;
            self.backend
                .render_prepared_layered_indexed_readback(
                    horizon_scene,
                    horizon_frame,
                    &mut self.scene,
                    frame,
                )
                .map_err(|error| StageFrameError::GpuCompose(error.to_string()))?
        } else {
            self.backend
                .render_prepared_indexed_readback(&mut self.scene, frame)
                .map_err(|error| StageFrameError::GpuCompose(error.to_string()))?
        };
        SoftwarePresentedFrame::from_indexed(
            indexed.extent(),
            indexed.indices().to_vec().into_boxed_slice(),
            Box::new(self.palette),
        )
        .map_err(StageFrameError::Present)
    }
}

/// Identity retained on typed loading-overlay failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LoadingOverlayBitmap {
    Bar,
    Line,
}

impl LoadingOverlayBitmap {
    #[must_use]
    pub const fn legacy_relative_path(self) -> &'static str {
        match self {
            Self::Bar => LOADING_BAR_BMP,
            Self::Line => LOADING_LINE_BMP,
        }
    }

    #[must_use]
    pub fn store_path(self) -> String {
        format!("Game/{}", self.legacy_relative_path().replace('\\', "/"))
    }
}

struct LoadingOverlayAssets {
    bar: IndexedBitmap,
    line: IndexedBitmap,
}

impl LoadingOverlayAssets {
    fn load(store: &AssetStore) -> Result<Self, StageFrameError> {
        Ok(Self {
            bar: load_loading_overlay_bitmap(store, LoadingOverlayBitmap::Bar)?,
            line: load_loading_overlay_bitmap(store, LoadingOverlayBitmap::Line)?,
        })
    }
}

fn load_loading_overlay_bitmap(
    store: &AssetStore,
    bitmap: LoadingOverlayBitmap,
) -> Result<IndexedBitmap, StageFrameError> {
    let bytes = store
        .load(&bitmap.store_path())
        .map_err(|error| StageFrameError::LoadingOverlayAsset { bitmap, error })?;
    parse_loading_overlay_bitmap(bitmap, &bytes)
}

fn parse_loading_overlay_bitmap(
    bitmap: LoadingOverlayBitmap,
    bytes: &[u8],
) -> Result<IndexedBitmap, StageFrameError> {
    parse_indexed_bitmap(bytes, BitmapLimits::default())
        .map_err(|error| StageFrameError::LoadingOverlayBitmap { bitmap, error })
}

/// Authenticated, bounded-decoded sources referenced by the recovered normal
/// single-player race-HUD plan.
pub struct RaceHudAssets {
    common_gauge: IndexedBitmap,
    progress_red_spot: IndexedBitmap,
    horizontal_bar: IndexedBitmap,
    horizontal_bar_end: IndexedBitmap,
    numbers: BitmapFont,
    time_numbers: BitmapFont,
}

impl RaceHudAssets {
    fn load(store: &AssetStore) -> Result<Self, StageFrameError> {
        Ok(Self {
            common_gauge: load_race_hud_bitmap(store, RaceHudBitmap::CommonGauge640)?,
            progress_red_spot: load_race_hud_bitmap(store, RaceHudBitmap::ProgressRedSpot)?,
            horizontal_bar: load_race_hud_bitmap(store, RaceHudBitmap::HorizontalBar)?,
            horizontal_bar_end: load_race_hud_bitmap(store, RaceHudBitmap::HorizontalBarEnd)?,
            numbers: load_race_hud_font(store, RaceHudFont::Numbers640)?,
            time_numbers: load_race_hud_font(store, RaceHudFont::TimeNumbers640)?,
        })
    }

    /// Return the decoded bitmap selected by a plan command.
    #[must_use]
    pub fn bitmap(&self, bitmap: RaceHudBitmap) -> &IndexedBitmap {
        match bitmap {
            RaceHudBitmap::CommonGauge640 => &self.common_gauge,
            RaceHudBitmap::ProgressRedSpot => &self.progress_red_spot,
            RaceHudBitmap::HorizontalBar => &self.horizontal_bar,
            RaceHudBitmap::HorizontalBarEnd => &self.horizontal_bar_end,
        }
    }

    /// Return the decoded font selected by a plan command.
    #[must_use]
    pub fn font(&self, font: RaceHudFont) -> &BitmapFont {
        match font {
            RaceHudFont::Numbers640 => &self.numbers,
            RaceHudFont::TimeNumbers640 => &self.time_numbers,
        }
    }
}

fn race_hud_store_path(relative: &str) -> String {
    format!("Game/{relative}")
}

fn load_race_hud_bitmap(
    store: &AssetStore,
    bitmap: RaceHudBitmap,
) -> Result<IndexedBitmap, StageFrameError> {
    let bytes = store
        .load(&race_hud_store_path(bitmap.relative_path()))
        .map_err(StageFrameError::HudAsset)?;
    parse_race_hud_bitmap(bitmap, &bytes)
}

fn parse_race_hud_bitmap(
    bitmap: RaceHudBitmap,
    bytes: &[u8],
) -> Result<IndexedBitmap, StageFrameError> {
    parse_indexed_bitmap(bytes, BitmapLimits::default())
        .map_err(|error| StageFrameError::HudBitmap { bitmap, error })
}

fn load_race_hud_font(
    store: &AssetStore,
    font: RaceHudFont,
) -> Result<BitmapFont, StageFrameError> {
    let bytes = store
        .load(&race_hud_store_path(font.relative_path()))
        .map_err(StageFrameError::HudAsset)?;
    parse_race_hud_font(font, &bytes)
}

fn parse_race_hud_font(font: RaceHudFont, bytes: &[u8]) -> Result<BitmapFont, StageFrameError> {
    parse_legacy_font(bytes, FontLimits::default())
        .map_err(|error| StageFrameError::HudFont { font, error })
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
    static_collision_scene: Option<ParsedStaticCollisionScene>,
    loading_background: Option<SoftwarePresentedFrame>,
    loading_overlay: Option<LoadingOverlayAssets>,
    textures: StageC3dTextures,
    object_textures: Option<StageC3dTextures>,
    scenery_textures: Option<StageC3dTextures>,
    horizon_textures: Option<StageC3dTextures>,
    car_scene: Option<C3dFile>,
    car_textures: Option<StageC3dTextures>,
    race_hud: Option<RaceHudAssets>,
    palette: [PaletteEntry; 256],
    node_count: usize,
    draw_ops: usize,
    choice: StageRendererChoice,
    gpu: Option<WgpuStageRenderer>,
    gpu_fallback: Option<StageRendererFallback>,
}

fn horizon_root_world(camera: &[i32; 16], y_offset_q16: i32) -> [i32; 16] {
    let mut root = MATRIX_IDENTITY_Q16;
    root[12] = camera[12];
    root[13] = camera[13].wrapping_add(y_offset_q16);
    root[14] = camera[14];
    root
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
        Self::load_with_palette(media_root, stage, choice, stage_placeholder_palette())
    }

    /// Loads a stage with a caller-selected active palette before preparing
    /// either renderer.
    pub fn load_with_palette(
        media_root: &Path,
        stage: &str,
        choice: StageRendererChoice,
        palette: [PaletteEntry; 256],
    ) -> Result<Self, StageFrameError> {
        let decode_limits = DecodeLimits::default();
        let source = FsStageSectionSource::read(media_root, stage, decode_limits)?;
        let loading_background = read_loading_background(media_root, stage, decode_limits)?
            .map(|bytes| parse_loading_background(&bytes))
            .transpose()?;
        let bundle = load_stage(&source, C3dLimits::default()).map_err(StageFrameError::Load)?;
        let static_collision_scene = bundle
            .objects()
            .map(|objects| build_parsed_static_collision_scene(bundle.scene(), objects))
            .transpose()
            .map_err(StageFrameError::StaticCollision)?;
        let textures = StageC3dTextures::prepare(bundle.scene(), BitmapLimits::default())
            .map_err(StageFrameError::Texture)?;
        let horizon_textures = bundle
            .horizon()
            .map(|horizon| StageC3dTextures::prepare(horizon, BitmapLimits::default()))
            .transpose()
            .map_err(StageFrameError::Texture)?;
        let object_textures = bundle
            .objects()
            .map(|scene| StageC3dTextures::prepare(scene, BitmapLimits::default()))
            .transpose()
            .map_err(StageFrameError::Texture)?;
        let scenery_textures = bundle
            .scenery()
            .map(|scene| StageC3dTextures::prepare(scene, BitmapLimits::default()))
            .transpose()
            .map_err(StageFrameError::Texture)?;
        if bundle.horizon().is_some() && bundle.ars().is_none() {
            return Err(StageFrameError::MissingHorizonSettings);
        }
        let scene = StageScene::from_c3d(bundle.scene(), &MATRIX_IDENTITY_Q16, VERTEXTYPE_LVERTEX)
            .map_err(StageFrameError::Scene)?;
        let node_count = scene.node_count();
        let draw_ops = scene.draw_order().len();

        let mut resources = StageRenderResources {
            stage: stage.to_owned(),
            bundle,
            static_collision_scene,
            loading_background,
            loading_overlay: None,
            textures,
            object_textures,
            scenery_textures,
            horizon_textures,
            car_scene: None,
            car_textures: None,
            race_hud: None,
            palette,
            node_count,
            draw_ops,
            choice,
            gpu: None,
            gpu_fallback: None,
        };
        resources.rebuild_gpu_foreground()?;
        Ok(resources)
    }

    /// Selected-stage artwork shown by the recovered state-4 load route before
    /// the BAR/LINE progress overlay. It remains absent when user media does not
    /// contain the optional sibling.
    #[must_use]
    pub fn loading_background(&self) -> Option<&SoftwarePresentedFrame> {
        self.loading_background.as_ref()
    }

    /// Immutable recovered collision records built once from the retained
    /// main-C3D grid and OBJ graph. A stage without an OBJ sibling has no
    /// static collision scene; live car/session wiring is a separate step.
    #[must_use]
    pub fn static_collision_scene(&self) -> Option<&ParsedStaticCollisionScene> {
        self.static_collision_scene.as_ref()
    }

    fn prepare_gpu_foreground(&self) -> Result<PreparedRawStage, StageFrameError> {
        let car_policy = in_race_car_load_policy(WINDOWED_BODY_LOAD_BRANCH, false);
        let main = StageScene::from_c3d(
            self.bundle.scene(),
            &MATRIX_IDENTITY_Q16,
            VERTEXTYPE_LVERTEX,
        )
        .map_err(StageFrameError::Scene)?;
        let objects = self
            .bundle
            .objects()
            .map(|file| StageScene::from_c3d(file, &MATRIX_IDENTITY_Q16, VERTEXTYPE_LVERTEX))
            .transpose()
            .map_err(StageFrameError::Scene)?;
        let scenery = self
            .bundle
            .scenery()
            .map(|file| StageScene::from_c3d(file, &MATRIX_IDENTITY_Q16, VERTEXTYPE_LVERTEX))
            .transpose()
            .map_err(StageFrameError::Scene)?;
        let car = self
            .car_scene
            .as_ref()
            .map(|file| {
                StageScene::from_c3d(
                    file,
                    &MATRIX_IDENTITY_Q16,
                    node_mesh_vertex_type(car_policy.load_flags),
                )
            })
            .transpose()
            .map_err(StageFrameError::Scene)?;
        let mut layers = vec![PreparedRawStageLayer::opaque(&main, &self.textures)];
        if let (Some(scene), Some(textures)) = (objects.as_ref(), self.object_textures.as_ref()) {
            layers.push(PreparedRawStageLayer::keyed(scene, textures, 0));
        }
        if let (Some(scene), Some(textures)) = (scenery.as_ref(), self.scenery_textures.as_ref()) {
            layers.push(PreparedRawStageLayer::keyed(scene, textures, 0));
        }
        if let (Some(scene), Some(textures)) = (car.as_ref(), self.car_textures.as_ref()) {
            layers.push(PreparedRawStageLayer::keyed_dynamic(
                scene,
                textures,
                car_policy
                    .body_source_color_key
                    .expect("the recovered mode-5 body policy is source-keyed"),
                0,
            ));
        }
        PreparedRawStage::prepare_layers(layers, ROOT_EXTENT, &self.palette)
            .map_err(StageFrameError::GpuPrepare)
    }

    fn prepare_gpu_horizon(&self) -> Result<Option<PreparedRawStage>, StageFrameError> {
        let (Some(horizon), Some(textures)) =
            (self.bundle.horizon(), self.horizon_textures.as_ref())
        else {
            return Ok(None);
        };
        let scene = StageScene::from_c3d(horizon, &MATRIX_IDENTITY_Q16, VERTEXTYPE_LVERTEX)
            .map_err(StageFrameError::Scene)?;
        PreparedRawStage::prepare_layers(
            [PreparedRawStageLayer::opaque_dynamic(&scene, textures, 0)],
            ROOT_EXTENT,
            &self.palette,
        )
        .map(Some)
        .map_err(StageFrameError::GpuPrepare)
    }

    fn rebuild_gpu_foreground(&mut self) -> Result<(), StageFrameError> {
        self.gpu = None;
        self.gpu_fallback = None;
        if self.choice == StageRendererChoice::Software {
            return Ok(());
        }
        let renderer = self.prepare_gpu_foreground().and_then(|foreground| {
            let horizon = self.prepare_gpu_horizon()?;
            WgpuStageRenderer::from_prepared(foreground, horizon)
        });
        match renderer {
            Ok(renderer) => {
                self.gpu = Some(renderer);
                Ok(())
            }
            Err(error) if self.choice == StageRendererChoice::Auto => {
                self.gpu_fallback = Some(StageRendererFallback {
                    phase: StageRendererFallbackPhase::Setup,
                    reason: error.kind(),
                });
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    /// Attach the caller-selected full-detail daytime race car from the
    /// authenticated runtime-data store. The car code is data selected by the
    /// user/menu; the reconstruction does not embed the original 14-entry table.
    pub fn attach_car(
        &mut self,
        runtime_data: &Path,
        car_code: &str,
    ) -> Result<(), StageFrameError> {
        let reviewed =
            ReviewedProfile::from_json(REVIEWED_PROFILE).map_err(StageFrameError::Profile)?;
        let store = AssetStore::open(runtime_data, &reviewed, DecodeLimits::default())
            .map_err(StageFrameError::CarAsset)?;
        let relative = car_body_path(car_code, false, false).replace('\\', "/");
        let path = format!("Game/{relative}");
        let bytes = store.load(&path).map_err(StageFrameError::CarAsset)?;
        let scene = parse_c3d(&bytes, C3dLimits::default()).map_err(StageFrameError::CarScene)?;
        let textures = StageC3dTextures::prepare(&scene, BitmapLimits::default())
            .map_err(StageFrameError::Texture)?;
        self.car_scene = Some(scene);
        self.car_textures = Some(textures);
        self.rebuild_gpu_foreground()
    }

    /// Load and retain every source-owned bitmap/font referenced by the
    /// recovered normal single-player HUD plan. Pixel policy is deliberately
    /// separate: this method authenticates and parses assets only.
    pub fn attach_race_hud(&mut self, runtime_data: &Path) -> Result<(), StageFrameError> {
        let reviewed =
            ReviewedProfile::from_json(REVIEWED_PROFILE).map_err(StageFrameError::Profile)?;
        let store = AssetStore::open(runtime_data, &reviewed, DecodeLimits::default())
            .map_err(StageFrameError::HudAsset)?;
        self.race_hud = Some(RaceHudAssets::load(&store)?);
        Ok(())
    }

    /// Load the recovered BAR/LINE loading-overlay sources from authenticated
    /// user runtime data. This intentionally does not derive an installation
    /// root from the stage-local `--source` path.
    pub fn attach_loading_overlay(&mut self, runtime_data: &Path) -> Result<(), StageFrameError> {
        let reviewed =
            ReviewedProfile::from_json(REVIEWED_PROFILE).map_err(StageFrameError::Profile)?;
        let store = AssetStore::open(runtime_data, &reviewed, DecodeLimits::default())
            .map_err(StageFrameError::LoadingOverlayStore)?;
        self.loading_overlay = Some(LoadingOverlayAssets::load(&store)?);
        Ok(())
    }

    /// Retained authenticated HUD sources, once native race setup attached them.
    #[must_use]
    pub fn race_hud(&self) -> Option<&RaceHudAssets> {
        self.race_hud.as_ref()
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

    /// Requested renderer, actual renderer, and any path-free automatic fallback.
    #[must_use]
    pub fn renderer_status(&self) -> StageRendererStatus {
        StageRendererStatus {
            requested: self.choice,
            selected: self.renderer(),
            fallback: self.gpu_fallback,
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
        self.compose_with_car(camera, None)
    }

    /// Compose the stage and, when supplied, the retained player car under its
    /// current world transform into the same depth-tested frame.
    pub fn compose_with_car(
        &mut self,
        camera: &[i32; 16],
        car_world: Option<&[i32; 16]>,
    ) -> Result<RenderedStageFrameOutput, StageFrameError> {
        self.compose_with_posed_car(camera, car_world, &[])
    }

    /// [`Self::compose_with_car`] with the car's wheel nodes posed by the
    /// render interpolation (`FUN_00439ee0`): `wheels` pairs a car record index
    /// with its local matrix.
    pub fn compose_with_posed_car(
        &mut self,
        camera: &[i32; 16],
        car_world: Option<&[i32; 16]>,
        wheels: &[(usize, [i32; 16])],
    ) -> Result<RenderedStageFrameOutput, StageFrameError> {
        let car_node_worlds = self.car_scene.as_ref().zip(car_world).map(|(scene, root)| {
            cmr_core::c3d_transform::scene_world_transforms_with_locals(
                scene.group0_records(),
                root,
                wheels,
            )
        });
        let horizon_node_worlds =
            self.bundle
                .horizon()
                .zip(self.bundle.ars())
                .map(|(horizon, ars)| {
                    let root = horizon_root_world(camera, ars.horizon_y_offset_q16());
                    scene_world_transforms(horizon.group0_records(), &root)
                });
        if let Some(gpu) = self.gpu.as_mut() {
            match gpu.compose(
                camera,
                car_node_worlds.as_deref(),
                horizon_node_worlds.as_deref(),
            ) {
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
                    let fallback = StageRendererFallback {
                        phase: StageRendererFallbackPhase::Frame,
                        reason: error.kind(),
                    };
                    self.gpu_fallback = Some(fallback);
                    self.gpu = None;
                    eprintln!(
                        "cmr-app: renderer fallback phase={} reason={} selected=software",
                        fallback.phase.name(),
                        fallback.reason,
                    );
                }
            }
        }
        self.compose_software(camera, car_world, wheels)
    }

    /// The player car's wheel nodes, rest poses and radius (`FUN_00404310`,
    /// `FUN_00480e60`), or `None` without a car model or its four wheels.
    #[must_use]
    pub fn wheel_rig(&self) -> Option<WheelRig> {
        WheelRig::from_scene(self.car_scene.as_ref()?)
    }

    fn compose_software(
        &self,
        camera: &[i32; 16],
        car_world: Option<&[i32; 16]>,
        wheels: &[(usize, [i32; 16])],
    ) -> Result<RenderedStageFrameOutput, StageFrameError> {
        let scene = StageScene::from_c3d(
            self.bundle.scene(),
            &MATRIX_IDENTITY_Q16,
            VERTEXTYPE_LVERTEX,
        )
        .map_err(StageFrameError::Scene)?;
        let mut frame = compose_stage_scene_frame(
            &scene,
            camera,
            [ROOT_EXTENT.width, ROOT_EXTENT.height],
            &self.palette,
            SoftwareC3dIndexedQuantization::NearestRgbL1LowestIndexNoDither,
            |ordinal| self.textures.texture_for_ordinal(ordinal),
        )
        .map_err(StageFrameError::Compose)?;

        for (graph, graph_textures) in [
            (self.bundle.objects(), self.object_textures.as_ref()),
            (self.bundle.scenery(), self.scenery_textures.as_ref()),
        ] {
            let (Some(graph), Some(graph_textures)) = (graph, graph_textures) else {
                continue;
            };
            let graph_scene = StageScene::from_c3d(graph, &MATRIX_IDENTITY_Q16, VERTEXTYPE_LVERTEX)
                .map_err(StageFrameError::Scene)?;
            let graph_frame = compose_stage_scene_frame_color_key(
                &graph_scene,
                camera,
                [ROOT_EXTENT.width, ROOT_EXTENT.height],
                &self.palette,
                SoftwareC3dIndexedQuantization::NearestRgbL1LowestIndexNoDither,
                0,
                |ordinal| graph_textures.texture_for_ordinal(ordinal),
            )
            .map_err(StageFrameError::Compose)?;
            frame = frame
                .merge_nearest(&graph_frame)
                .map_err(StageFrameError::Compose)?;
        }

        if let (Some(car_scene), Some(car_textures), Some(car_world)) = (
            self.car_scene.as_ref(),
            self.car_textures.as_ref(),
            car_world,
        ) {
            let car_policy = in_race_car_load_policy(WINDOWED_BODY_LOAD_BRANCH, false);
            let car = StageScene::from_c3d_with_locals(
                car_scene,
                car_world,
                node_mesh_vertex_type(car_policy.load_flags),
                wheels,
            )
            .map_err(StageFrameError::Scene)?;
            let car_frame = compose_stage_scene_frame_color_key(
                &car,
                camera,
                [ROOT_EXTENT.width, ROOT_EXTENT.height],
                &self.palette,
                SoftwareC3dIndexedQuantization::NearestRgbL1LowestIndexNoDither,
                car_policy
                    .body_source_color_key
                    .expect("the recovered mode-5 body policy is source-keyed"),
                |ordinal| car_textures.texture_for_ordinal(ordinal),
            )
            .map_err(StageFrameError::Compose)?;
            frame = frame
                .merge_nearest(&car_frame)
                .map_err(StageFrameError::Compose)?;
        }

        let frame = if let (Some(horizon), Some(horizon_textures), Some(ars)) = (
            self.bundle.horizon(),
            self.horizon_textures.as_ref(),
            self.bundle.ars(),
        ) {
            // The shipped shared compositor re-centers the horizon root on the
            // current viewport camera and adds the stage-specific ARS Y offset.
            // The root basis remains identity; rendering it through the normal
            // camera view therefore preserves camera rotation but cancels
            // translation.
            let horizon_root = horizon_root_world(camera, ars.horizon_y_offset_q16());
            let horizon_scene = StageScene::from_c3d(horizon, &horizon_root, VERTEXTYPE_LVERTEX)
                .map_err(StageFrameError::Scene)?;
            let (w, h, z_near, _) = cmr_core::c3d_camera::PROJECTION_BASELINE;
            let background = compose_stage_scene_frame_with_projection(
                &horizon_scene,
                camera,
                [ROOT_EXTENT.width, ROOT_EXTENT.height],
                &self.palette,
                SoftwareC3dIndexedQuantization::NearestRgbL1LowestIndexNoDither,
                (w, h, z_near, cmr_core::c3d_camera::HORIZON_Z_FAR),
                |ordinal| horizon_textures.texture_for_ordinal(ordinal),
            )
            .map_err(StageFrameError::Compose)?;
            frame
                .over_background(&background)
                .map_err(StageFrameError::Compose)?
        } else {
            frame
        };

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
    /// Authenticated compiled setup-time pose inputs could not be applied.
    CompiledPose(SessionCompiledPoseError),
    /// Authenticated session input or four-wheel production failed.
    Session(SessionFourWheelError),
    /// Live static-scene production or response failed atomically.
    StaticCollision(StaticGameplayAdvanceError<Infallible>),
    /// The SDL host failed to present the composed frame.
    Present(SdlHostError),
    /// A stage `.dat` record names a cell outside the main-C3D grid.
    StageTriggers(cmr_core::trigger_walk::TriggerCellOutOfRange),
}

impl WindowedStageFrameError {
    /// Stable error category for path-free process diagnostics.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Setup(source) => source.kind(),
            Self::CompiledPose(source) => source.kind(),
            Self::Session(_) => "four-wheel-session",
            Self::StaticCollision(_) => "static-collision-session",
            Self::Present(_) => "present-host",
            Self::StageTriggers(_) => "stage-triggers",
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
            Self::CompiledPose(source) => write!(formatter, "compiled pose setup failed: {source}"),
            Self::Session(source) => write!(formatter, "four-wheel session failed: {source}"),
            Self::StaticCollision(source) => {
                write!(formatter, "static collision session failed: {source:?}")
            }
            Self::Present(source) => write!(formatter, "cannot present stage frame: {source}"),
            Self::StageTriggers(source) => {
                write!(
                    formatter,
                    "stage .dat record outside the cell grid: {source:?}"
                )
            }
        }
    }
}

impl std::error::Error for WindowedStageFrameError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Setup(source) => Some(source),
            Self::CompiledPose(source) => Some(source),
            Self::Session(source) => Some(source),
            Self::StaticCollision(_) | Self::StageTriggers(_) => None,
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
/// composes the in-race world and clears otherwise. The playable baseline uses
/// the existing planar dynamics and documented harness [`DriveResponse`] values
/// rather than claiming complete four-wheel trajectory fidelity. Stage checkpoints
/// come from the authored `.cat` nodes, the camera follows the live car, and the
/// recovered FSM, input chain, and render routes drive the session. Player-car
/// rendering and the in-race HUD remain separate composition work.
///
/// The player car's wheel nodes as the car setup finds them
/// (`FUN_00495050(model, 1..4)`, the first record with that id), their rest
/// rows and translations (copied to `+0x5a4` at `0x4043b1`), and the wheel
/// radius `FUN_00480e60` derives from node 6: its y scale (`1.0` from the node
/// init `FUN_00492310`) times the y bound `+0x94` of its attached mesh, halved
/// toward zero. The attached mesh is taken as the first entry of the record's
/// mesh list (an inference).
#[derive(Clone, Copy, Debug)]
pub struct WheelRig {
    records: [usize; 4],
    rest_rows: [[Vec3; 3]; 4],
    rest_translation: [[i32; 3]; 4],
    radius: i32,
}

impl WheelRig {
    fn from_scene(scene: &C3dFile) -> Option<Self> {
        let records: Vec<_> = scene.group0_records().collect();
        let find = |id: u16| records.iter().position(|record| record.node_id() == id);
        let indices = [find(1)?, find(2)?, find(3)?, find(4)?];
        let poses = indices.map(|index| {
            cmr_core::wheel_animation::matrix_rows(&records[index].affine_matrix_q16())
        });
        let radius_node = &records[find(6)?];
        let mesh = usize::from(radius_node.values().next()?);
        let bound = scene.group1_records().nth(mesh)?.collision_bounds_q16()[1];
        let radius = cmr_core::car_resources::wheel_radius(0x1_0000, bound, false);
        Some(Self {
            records: indices,
            rest_rows: poses.map(|(rows, _)| rows),
            rest_translation: poses.map(|(_, translation)| translation),
            radius,
        })
    }

    /// The four posed wheel locals for the car's current animation words.
    fn locals(
        &self,
        car: &CarState,
        setup_800: [i32; 4],
        analog_steer: bool,
        trig: &[i32],
    ) -> Vec<(usize, [i32; 16])> {
        let input = cmr_core::wheel_animation::WheelPoseInput {
            rest_rows: self.rest_rows,
            rest_translation: self.rest_translation,
            wheel_radius: self.radius,
            half_height_30: car.motion.compiled_half_words.y.raw(),
            steer_angle: if analog_steer {
                car.four_wheel.steering.target_3e2
            } else {
                car.four_wheel.steering.angle_3e0
            },
            setup_800,
            rear_steer: false,
            no_ride_offset: false,
        };
        (0..4)
            .filter_map(|wheel| {
                cmr_core::wheel_animation::wheel_pose(&car.four_wheel.wheels, &input, wheel, trig)
                    .map(|local| (self.records[wheel], local))
            })
            .collect()
    }
}

/// The windowed session's camera state is the recovered multi-mode view
/// controller. The view modes, their placement math, and the cycle order are
/// recovered behavior ([`ViewController`]/[`cmr_core::camera_view`]); the
/// cycle fires on the control map's change-view action (action bit 6, Numpad 6
/// for player one), the latch that calls `FUN_00434210`. Only the RNG seed is
/// harness policy.
struct ViewHarness {
    controller: ViewController,
    rng: GameLcg,
    trig: [i32; cmr_core::fixed_point::TRIG_TABLE_LEN],
    /// The camera record as the last camera-driver run left it.
    last: Option<[i32; 16]>,
}

impl ViewHarness {
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
            trig: cmr_core::fixed_point::build_sine_table(),
            last: None,
        }
    }

    /// Cycles to the next recovered view when the change-view action fired
    /// (`FUN_00434210`), then returns the world matrix for the car's current
    /// placement.
    fn update(&mut self, change_view: bool, position: Vec3, heading: u16, pal: bool) -> [i32; 16] {
        if change_view {
            self.controller.cycle(Self::GATE, &mut self.rng);
        }
        let world = self.controller.update(position, heading, pal);
        self.last = Some(world);
        world
    }

    /// The camera record without a driver run: the pause (`0xa`) and hub
    /// (`0xb`) handlers do not run Mechanics, which owns the camera driver
    /// `FUN_004343a0`, so the record keeps its last pose.
    fn held(&mut self, position: Vec3, heading: u16, pal: bool) -> [i32; 16] {
        match self.last {
            Some(world) => world,
            None => self.update(false, position, heading, pal),
        }
    }

    /// The reset's camera snap (`DAT_00551560` -> `FUN_00434870`): the
    /// native re-initializes the view's camera slots. This harness rebuilds the
    /// controller in its current mode, an approximation until `FUN_00436730`
    /// is reconstructed.
    fn snap(&mut self) {
        self.controller = ViewController::new(self.controller.mode());
    }

    /// The camera record's mode word `+4`: the `Blending` sentinel while a
    /// crossfade runs, else the active mode.
    fn record_mode(&self) -> i32 {
        if self.controller.is_blending() {
            CameraMode::Blending.raw() as i32
        } else {
            self.controller.mode().raw() as i32
        }
    }

    fn car_world(&self, position: Vec3, heading: u16) -> [i32; 16] {
        cmr_core::gameplay_camera::car_world_q16(&self.trig, position, heading)
    }
}

fn analog_driving_is_active(pad: &GamepadDrivingState) -> bool {
    pad.has_driving_input()
}

/// Recovered speed-display multiplier applied to the reconstructed Q16 velocity
/// magnitude by the normal HUD preparation path.
const RACE_HUD_SPEED_MULTIPLIER_Q16: i32 = 0x431168;

/// Adapt the session state to the recovered HUD input contract. A four-wheel
/// car supplies `FUN_00464130`'s rev, speed and gear words (`prepared`), with
/// the units bits and car-type nibble at their harness zero. The planar
/// harness has no engine or gearbox, so it shows gear 1, zero rev and its
/// velocity magnitude. Progress is `FUN_00464130`'s cursor fraction.
fn live_race_hud_inputs(
    car: CarState,
    prepared: Option<cmr_core::race_hud_prepare::PreparedHudValues>,
    progress: &StageProgress<'_>,
    node_count: usize,
    stage_clock: Fixed16,
) -> RaceHudPreparedInputs {
    let (rev_q16, speed, gear) = prepared.map_or_else(
        || {
            let speed = car
                .velocity
                .length()
                .mul(Fixed16::from_raw(RACE_HUD_SPEED_MULTIPLIER_Q16))
                .raw()
                .max(0)
                >> 16;
            (0, speed, 1)
        },
        |values| (values.rev_q16, values.speed.max(0), values.gear),
    );
    // `FUN_00464130`'s progress word; one active player (`DAT_005515fc`).
    let progress_q16 = cmr_core::race_hud_prepare::hud_progress(
        node_count,
        i32::try_from(progress.cursor()).unwrap_or(i32::MAX),
        progress.finished(),
        1,
    );
    RaceHudPreparedInputs {
        viewport: ROOT_EXTENT,
        rev_q16,
        speed: speed as u32,
        gear,
        progress_q16,
        elapsed_centiseconds: elapsed_centiseconds(stage_clock, Fixed16::ZERO).max(0) as u32,
        gauge_inset: CommonGaugeInset::ConfigLowThreeBitsZero,
    }
}

fn compose_live_race_hud(
    resources: &StageRenderResources,
    frame: SoftwarePresentedFrame,
    input: RaceHudPreparedInputs,
) -> Result<SoftwarePresentedFrame, StageFrameError> {
    let assets = resources
        .race_hud()
        .ok_or(StageFrameError::MissingHudAssets)?;
    let plan = plan_normal_single_player_race_hud(input).map_err(StageFrameError::HudPlan)?;
    compose_race_hud_commands(
        frame,
        plan.commands(),
        |font| assets.font(font),
        |bitmap| assets.bitmap(bitmap),
    )
    .map(|(frame, _)| frame)
    .map_err(StageFrameError::HudCompose)
}

fn compose_world_for_route<F>(
    route: Option<RenderRoute>,
    compose: F,
) -> Result<Option<SoftwarePresentedFrame>, StageFrameError>
where
    F: FnOnce() -> Result<SoftwarePresentedFrame, StageFrameError>,
{
    if route.is_some_and(presents_world_scene) {
        compose().map(Some)
    } else {
        Ok(None)
    }
}

fn loading_interstitial_for_route(
    route: Option<RenderRoute>,
    background: Option<&SoftwarePresentedFrame>,
    overlay: Option<&LoadingOverlayAssets>,
    progress: u8,
) -> Result<Option<SoftwarePresentedFrame>, StageFrameError> {
    if route != Some(RenderRoute::LoadInterstitial) {
        return Ok(None);
    }
    let Some(background) = background else {
        // Matches the native setup boundary: a failed/missing loading backdrop
        // never arms the progress compositor.
        return Ok(None);
    };
    let overlay = overlay.ok_or(StageFrameError::MissingLoadingOverlayAssets)?;
    compose_load_interstitial(background.clone(), &overlay.bar, &overlay.line, progress)
        .map(Some)
        .map_err(StageFrameError::LoadingOverlayCompose)
}

/// The pre-race waits this session satisfies at once. Harness: the country
/// welcome screen (`FUN_0048dde0`) is not reconstructed, so it counts as
/// dismissed (the native screen also ends on a key press); no interstitial is
/// pending.
const PRE_RACE_INPUTS: MenuFlowInputs = MenuFlowInputs {
    welcome_done: true,
    school_menu_done: false,
    school_quit: false,
    school_status_quit: false,
    preload_pending: false,
    preload_done: false,
};

/// The descriptor front end's frame (`FUN_004c20f0`), run only while no
/// session is: its race hand-off starts the race session (after the two
/// unconditional writes of the commit `FUN_004b93f0`), its rally-school
/// hand-off the school session.
fn front_end_frame(
    session: &mut GameplaySession,
    root_interaction: &mut RootInteraction,
    input: cmr_core::frontend::FrontendInput,
    now: u32,
) -> cmr_core::root_menu_interactive::RootTick {
    let tick = root_interaction.tick_for_game_session(input, now, true);
    for effect in &tick.effects {
        let event = session.event_mut();
        match effect {
            cmr_core::frontend::FrontendEffect::StartRace => {
                event.commit_unconditional();
                event.start_race_session();
            }
            cmr_core::frontend::FrontendEffect::StartRallySchool => {
                event.commit_unconditional();
                event.start_school_session();
            }
            _ => {}
        }
    }
    tick
}

/// One windowed-game-session frame of the input/advance/world-compose work the
/// two loop variants share.
///
/// It maps the live input through the recovered control map (the analog driver
/// when a gamepad has driving input, the keyboard driver otherwise), advances the FSM
/// one frame stepping the race sim in the driving state, and - only when the
/// route is a `StageWorld`-family route - composes the stage from the follow
/// camera. A compose failure stops the loop and is surfaced to the caller;
/// silently replacing it with a black frame made renderer regressions appear
/// to be a static game screen.
/// It returns the frame's render route (so the caller can decide any non-world
/// presentation), the composed world frame (if any), and whether the FSM has
/// reached the stopped state.
#[allow(clippy::too_many_arguments)]
fn advance_game_frame<C: HostClock>(
    session: &mut GameplaySession,
    keyboard: &mut KeyboardDriver,
    analog: &mut AnalogDriver,
    response: &DriveResponse,
    root_interaction: &mut RootInteraction,
    progress: &mut StageProgress,
    clock: &C,
    resources: &mut StageRenderResources,
    camera: &mut ViewHarness,
    fade: &mut Option<ScalarTween>,
    damage_owner: &mut NoStaticDamageOwner,
    audio: &mut RaceAudio,
    pause: &mut PauseHarness,
    input: WindowedInput,
) -> Result<GameFrameOutcome, WindowedStageFrameError> {
    let mut pause_effects = Vec::new();
    let pause_flow = pause.frame(session, progress, input.keys, &mut pause_effects);
    session.set_pause_flow(pause_flow);
    pause.apply(&pause_effects, input.audio, audio);
    let root_tick = if session.event().in_session() {
        None
    } else {
        Some(front_end_frame(
            session,
            root_interaction,
            input.frontend,
            clock.millis(),
        ))
    };
    let menu_inputs = PRE_RACE_INPUTS;
    let drive = if let Some(pad) = input.gamepad.filter(|pad| analog_driving_is_active(pad)) {
        analog.poll(
            pad.steer_axis(),
            pad.pedal_axis(),
            pad.buttons(),
            response,
            true,
        )
    } else {
        keyboard.poll(|vk| input.keys.is_down(vk), response, true)
    };
    let controls = drive.controls;
    let change_view = drive.car.change_view_fired;
    let steer_analog = drive.car.steer_analog;
    let advanced = if let Some(parsed) = resources.static_collision_scene() {
        session
            .try_advance_with_static_collision(
                clock,
                menu_inputs,
                controls,
                progress,
                parsed,
                damage_owner,
            )
            .map_err(WindowedStageFrameError::StaticCollision)?
    } else {
        session
            .try_advance(clock, menu_inputs, controls, progress)
            .map_err(SessionFourWheelError::Producer)?
    };
    if session.last_reset().camera_reset {
        camera.snap();
    }
    let reset_fade = session.car().four_wheel.reset.fade_428;
    let mut tick_camera = None;
    let world = compose_world_for_route(advanced.route, || {
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

        // A four-wheel car's pose is its evolving basis; the planar harness
        // keeps only a heading.
        let (heading, harness_car_world) = if session.is_four_wheel() {
            four_wheel_render_pose(&car)
        } else {
            (car.heading, camera.car_world(car.position, car.heading))
        };
        let camera_world = if matches!(
            advanced.route,
            Some(RenderRoute::OverlayAlt | RenderRoute::FadeComposite)
        ) {
            camera.held(car.position, heading, false)
        } else {
            camera.update(change_view, car.position, heading, false)
        };
        tick_camera = Some(camera_world);
        let car_world = car_model_world_q16(&harness_car_world);
        let hud_input = live_race_hud_inputs(
            car,
            session.prepared_hud_values(false, false),
            progress,
            resources.bundle.nodes().map_or(0, <[CatNode]>::len),
            session.stage_clock(),
        );
        // A four-wheel car's wheel nodes are posed from its animation words
        // (`FUN_00439ee0`); the planar harness has none.
        let wheels = match (session.wheel_setup_800(), resources.wheel_rig()) {
            (Some(setup_800), Some(rig)) => rig.locals(&car, setup_800, steer_analog, &camera.trig),
            _ => Vec::new(),
        };
        let world = resources
            .compose_with_posed_car(&camera_world, Some(&car_world), &wheels)
            .map(|rendered| rendered.frame)?;
        compose_live_race_hud(resources, world, hud_input)
    })?;
    // Race-end fade (state 0xb, FadeComposite): the recovered linear brightness
    // tween drives a fullscreen modulate of the composed world toward black. The
    // shipped fade is triggered by the in-race transition handlers; this windowed
    // harness starts the recovered fade-out when the FSM reaches the fade route
    // and advances it one step per frame (a documented harness stand-in, like the
    // synthetic checkpoint line), presenting the modulated world so the race-end
    // transition is visible. Any other route clears the fade.
    // State 0xa: the pause menu's panel and text over the world (B handler
    // `FUN_004233e0` -> `FUN_00468dd0`).
    let world = match (advanced.route, world) {
        (Some(RenderRoute::OverlayAlt), Some(frame)) => Some(pause.draw(frame)),
        (_, world) => world,
    };
    let world = if advanced.route == Some(RenderRoute::FadeComposite) {
        let tween = fade.get_or_insert_with(|| fade_out(fade_duration_frames(false)));
        let level = tween.evaluate_and_advance();
        world.map(|frame| frame.modulated(fade_level_to_modulation_byte(level)))
    } else {
        *fade = None;
        world
    };
    // The reset fade `ctl +0x428` is copied into the viewport record each tick
    // (`0x43b6f5`); its renderer is not recovered, so it is presented as a
    // modulate toward black, fully black at the placement tick (an inference).
    let world = if reset_fade > 0 {
        world.map(|frame| frame.modulated(0xff - reset_fade))
    } else {
        world
    };
    // The listener `FUN_0047ec20` and the engine sound `FUN_00488d60` run
    // after this tick's Mechanics (which includes the camera driver) in every
    // in-race handler 5..=9 (the pause state 0xa skips them); on the single
    // view the surface, landing, wet and crowd routines follow.
    if let (Some(mixer), Some(camera_world)) = (input.audio, tick_camera.as_ref())
        && (5..=9).contains(&session.state().id())
    {
        let car = session.car();
        let camera_mode = camera.record_mode();
        match update_listener(&ListenerInput {
            camera: *camera_world,
            camera_mode,
            forward_258: vec3_raw(car.motion.basis[0]),
            centre_c8: vec3_raw(car.position),
            half_length_2c: car.motion.compiled_half_words.x.raw(),
        }) {
            Ok(listener) => {
                let master = pause.options.effects_master_4f653c;
                if let Some(engine) = audio.engine.as_mut() {
                    engine.tick(
                        mixer,
                        session,
                        &mut camera.rng,
                        camera_mode,
                        &listener.listener,
                        master,
                    );
                }
                if let Some(effects) = audio.effects.as_mut() {
                    effects.tick(
                        mixer,
                        session,
                        &mut camera.rng,
                        camera_world,
                        camera_mode,
                        listener.centre_proximity_9e4,
                        master,
                        advanced.stepped,
                        session.state().id(),
                    );
                }
            }
            Err(error) => eprintln!("cmr-app: listener skipped: {error}"),
        }
    }
    let loading_interstitial = loading_interstitial_for_route(
        advanced.route,
        resources.loading_background(),
        resources.loading_overlay.as_ref(),
        STAGE_LOADING_PROGRESS_CAP,
    )?;
    Ok(GameFrameOutcome {
        world,
        loading_interstitial,
        exit_requested: root_tick.as_ref().is_some_and(|tick| tick.exit_requested),
        root_render_input: root_tick.and_then(|tick| tick.render_input),
        // The recovered front-end music plays while the front end runs.
        music_active: advanced.band == SessionBand::FrontEnd,
        codriver_calls: if advanced.stepped {
            session
                .last_trigger_events()
                .iter()
                .filter_map(|event| match *event {
                    cmr_core::trigger_walk::TriggerEvent::CodriverCall { payload, .. } => {
                        Some(payload as u32)
                    }
                    _ => None,
                })
                .collect()
        } else {
            Vec::new()
        },
    })
}

/// What one [`advance_game_frame`] produced for the windowed loop.
struct GameFrameOutcome {
    world: Option<SoftwarePresentedFrame>,
    loading_interstitial: Option<SoftwarePresentedFrame>,
    root_render_input: Option<RootRenderInput>,
    /// The front end's exit screen ended the process (`DAT_0096efe8 = 1`).
    exit_requested: bool,
    music_active: bool,
    /// Co-driver note words the trigger walk submitted this frame.
    codriver_calls: Vec<u32>,
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

/// The reviewed auxiliary control constants behind `FUN_00407270`.
pub struct SessionAuxControl {
    set: cmr_core::compiled_car_initializer::AuxControlConstantSet,
}

impl SessionAuxControl {
    /// # Errors
    ///
    /// Reports an invalid reviewed profile, a missing or tampered store, or a
    /// store schema that does not match the core contract.
    pub fn open(runtime_data: &Path) -> Result<Self, String> {
        let reviewed =
            cmr_assets::ReviewedAuxControlProfile::from_json(REVIEWED_AUX_CONTROL_PROFILE)
                .map_err(|error| format!("invalid reviewed aux-control profile: {error}"))?;
        let store = cmr_assets::AuxControlConstantsStore::open(
            &session_sidecar_store(runtime_data, AUX_CONTROL_STORE_DIRECTORY),
            &reviewed,
        )
        .map_err(|error| format!("cannot open aux-control constants: {error}"))?;
        let set = cmr_core::compiled_car_initializer::AuxControlConstantSet::from_store(&store)
            .map_err(|error| error.to_string())?;
        Ok(Self { set })
    }

    /// Control words of the patch for the session's zero config: its option
    /// byte selects the default policy, and the patch's global word is
    /// `DAT_004eac6c`.
    fn patch_words<const N: usize>(
        &self,
        state_word_48: i32,
        offsets: [u16; N],
    ) -> Result<[i32; N], String> {
        use cmr_core::compiled_car_initializer::{AUX_CONTROL_WORD_OFFSETS, AuxControlOption};
        let patch = self
            .set
            .derive(
                AuxControlOption::Resolved(0),
                state_word_48,
                cmr_core::vehicle_dynamics::SOLVER_GLOBAL_CONSTANTS
                    .time_step
                    .raw(),
            )
            .map_err(|error| error.to_string())?;
        let word = |offset: u16| {
            let index = AUX_CONTROL_WORD_OFFSETS
                .iter()
                .position(|candidate| *candidate == offset)
                .expect("the patch writes this word");
            patch.words[index]
        };
        Ok(offsets.map(word))
    }
}

#[derive(Debug)]
pub enum SessionTyreTablesError {
    Profile(ReviewedTyreTablesProfileError),
    Store(TyreTablesStoreError),
}

impl fmt::Display for SessionTyreTablesError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Profile(error) => {
                write!(formatter, "invalid reviewed tyre-table profile: {error}")
            }
            Self::Store(error) => write!(formatter, "cannot open tyre tables: {error}"),
        }
    }
}

impl std::error::Error for SessionTyreTablesError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Profile(error) => Some(error),
            Self::Store(error) => Some(error),
        }
    }
}

/// The windowed session's engine sound: the recovered tick
/// ([`cmr_core::engine_sound`]) and listener ([`cmr_core::engine_listener`])
/// over the authenticated static band tables, driving the software mixer.
///
/// Harness choices (the shipped values are runtime state): mixer channels
/// from [`ENGINE_CHANNEL_BASE`] (the allocator `FUN_004957c0` is runtime),
/// player 0 only, no helium mode (`cfg[0x25] & 4`), the effects master at its
/// static default `DAT_004f653c = 100`, real play (`DAT_00d3ca88 == 0`), and
/// the camera view's LCG as the shared `rand()` state.
struct EngineAudio {
    tables: EngineAudioTables,
    car_nibble: u8,
    state: EngineSoundState,
    config: Option<EngineSoundConfig>,
}

/// The windowed session's pause: the recovered request and menu
/// ([`cmr_core::pause_menu`]) and the option bytes its sound/settings
/// submenus edit.
///
/// Harness choices: player 0 on the keyboard device (pad mask 0), real play,
/// one player, no link session, an all-zero config block apart from the
/// volumes `cfg[5] = cfg[6] = 5`, which match the static master defaults
/// `DAT_004f653c = DAT_004f6538 = 100` (`5 * 20`), and `DAT_00514478 = 0`
/// (`DAT_0055001c` is clear at race start), so Escape always pauses. The menu
/// cues play on mixer channel [`PAUSE_MENU_CHANNEL`] and the speech mute
/// targets the speech bank's channel [`SPEECH_CHANNEL`]; the native channels
/// are allocated at run time. The value-change preview plays a speech or
/// collision sample whose banks this harness does not share, so it is silent.
struct PauseHarness {
    controller: PauseController,
    options: PauseOptions,
    menu_cues: Option<MenuChannel>,
    /// The fonts and in-game text, loaded for one display width.
    assets: Option<(u16, PauseAssets)>,
    runtime_data: Option<PathBuf>,
}

/// The pause menu's fonts (`FUN_00436a50`) and the in-game text table
/// (`FUN_0043bc00`, language `cfg[0x18] & 7 = 0`, English, in this harness).
struct PauseAssets {
    title: BitmapFont,
    result: BitmapFont,
    result_grey: BitmapFont,
    text: cmr_core::localization::IndexedTextTable,
}

impl PauseAssets {
    fn load(runtime_data: &Path, width: u16) -> Option<Self> {
        let reviewed = ReviewedProfile::from_json(REVIEWED_PROFILE).ok()?;
        let store = AssetStore::open(runtime_data, &reviewed, DecodeLimits::default()).ok()?;
        let font = |font: PauseFont| {
            let bytes = store.load(&format!("Game/{}", font.path(width))).ok()?;
            parse_legacy_font(&bytes, FontLimits::default()).ok()
        };
        Some(Self {
            title: font(PauseFont::Title)?,
            result: font(PauseFont::Result)?,
            result_grey: font(PauseFont::ResultGrey)?,
            text: cmr_core::localization_assets::load_game_text_table(&store, Language::English)
                .ok()?,
        })
    }

    fn font(&self, font: PauseFont) -> &BitmapFont {
        match font {
            PauseFont::Title => &self.title,
            PauseFont::Result => &self.result,
            PauseFont::ResultGrey => &self.result_grey,
        }
    }
}

/// The pause menu cue channel (`FUN_00468f50`, the first allocation).
const PAUSE_MENU_CHANNEL: u32 = 0;
/// The co-driver speech channel (`FUN_00444160`), shared with the SDL
/// speech bank.
const SPEECH_CHANNEL: u32 = cmr_platform_sdl3::SPEECH_CHANNEL;

impl PauseHarness {
    fn new() -> Self {
        Self {
            controller: PauseController::default(),
            options: PauseOptions {
                cfg_5: 5,
                cfg_6: 5,
                cfg_8: 0,
                view_toggle_4eb5f0: 0,
                effects_master_4f653c: 100,
                speech_master_4f6538: 100,
            },
            menu_cues: None,
            assets: None,
            runtime_data: None,
        }
    }

    fn with_runtime_data(runtime_data: &Path) -> Self {
        Self {
            runtime_data: Some(runtime_data.to_owned()),
            ..Self::new()
        }
    }

    /// `FUN_00468dd0`: the current pause menu's panel and text over `frame`.
    /// The display is taken as alpha-capable and not mode 6 (the D3D path
    /// the queued glyphs model). Without the fonts or text the world shows
    /// unchanged.
    fn draw(&mut self, frame: SoftwarePresentedFrame) -> SoftwarePresentedFrame {
        let extent = frame.extent();
        let (Some(menus), Some(current)) =
            (self.controller.menus.as_ref(), self.controller.current)
        else {
            return frame;
        };
        if self
            .assets
            .as_ref()
            .is_none_or(|(width, _)| *width != extent.width)
        {
            self.assets = self
                .runtime_data
                .as_deref()
                .and_then(|root| PauseAssets::load(root, extent.width))
                .map(|assets| (extent.width, assets));
        }
        let Some((_, assets)) = self.assets.as_ref() else {
            return frame;
        };
        let text = |id: i32| {
            usize::try_from(id)
                .map(|id| assets.text.lookup(id).to_vec())
                .unwrap_or_default()
        };
        let display = PauseDisplay {
            width: extent.width,
            height: extent.height,
            mode_6: false,
            alpha_capable: true,
        };
        let Ok(plan) = pause_draw_plan(menus, current, &text, &|font| assets.font(font), display)
        else {
            return frame;
        };
        match compose_pause_overlay(frame.clone(), &plan, |font| assets.font(font)) {
            Ok((drawn, _)) => drawn,
            Err(_) => frame,
        }
    }

    /// The control map's Escape poll and probe in states 5/7/9, or the paused
    /// handler in `0xa`; returns the next frame's pause words.
    fn frame(
        &mut self,
        session: &GameplaySession,
        progress: &StageProgress,
        keys: &cmr_platform_sdl3::KeyboardVkState,
        effects: &mut Vec<PauseEffect>,
    ) -> PauseFlow {
        let state_id = session.state().id();
        let conditions = PauseMenuConditions {
            real_play: true,
            link_session: false,
            two_player_mode: false,
            cfg_166_mode: 0,
            slow_motion: false,
        };
        let mut flow = PauseFlow::default();
        match state_id {
            5 | 7 | 9 => {
                let car = session.car();
                flow.enter_pause = self.controller.driving_tick(
                    DrivingPauseInput {
                        state_id,
                        escape_down: keys.is_down(0x1b),
                        player: 0,
                        any_countdown_75c: car.four_wheel.trigger_walk.penalty_timer_75c != 0,
                        gates: PauseKeyGates {
                            player_finished: progress.finished(),
                            player_active: true,
                            blocked_55ae9c: false,
                            real_play: conditions.real_play,
                            alternate_6c0: false,
                        },
                        conditions,
                        one_player: true,
                    },
                    self.options,
                    effects,
                );
            }
            0xa => {
                let pause_keys = PauseKeys {
                    escape: keys.is_down(0x1b),
                    up: keys.is_down(0x26),
                    down: keys.is_down(0x28),
                    left: keys.is_down(0x25),
                    right: keys.is_down(0x27),
                    enter: keys.is_down(0x0d),
                    shift: keys.is_down(0x10),
                    q: keys.is_down(0x51),
                    pad_mask: 0,
                };
                match self.controller.paused_tick(
                    pause_keys,
                    conditions.real_play,
                    conditions.slow_motion,
                    &mut self.options,
                    effects,
                ) {
                    PauseDecision::Stay => {}
                    PauseDecision::QuitHotkey => flow.quit = true,
                    PauseDecision::Quit => {}
                    // The handler sets `DAT_00550018` on its way to the hub.
                    PauseDecision::Restart => flow.restart = true,
                    PauseDecision::Alternate => flow.resume_alt = true,
                    PauseDecision::Resume { to_entry_state } => {
                        flow.resume = true;
                        flow.prepause_was_7 = to_entry_state;
                    }
                }
            }
            _ => {}
        }
        // The quit word persists into the hub.
        flow.quit |= self.controller.quit_678;
        flow
    }

    fn apply(
        &mut self,
        effects: &[PauseEffect],
        mixer: Option<&SharedSoftwareMixer>,
        audio: &RaceAudio,
    ) {
        let Some(mixer) = mixer else {
            return;
        };
        for effect in effects {
            match *effect {
                PauseEffect::MuteEngine { car } => {
                    if let Some(engine) = audio.engine.as_ref() {
                        engine.mute(mixer, u32::from(car));
                    }
                }
                PauseEffect::StopSurfaceLoops => {
                    if let Some(effects) = audio.effects.as_ref() {
                        effects.stop_surface(mixer);
                    }
                }
                PauseEffect::MuteSpeech => set_speech_volume(mixer, 0),
                PauseEffect::RestoreSpeech => {
                    set_speech_volume(mixer, cmr_core::audio_pacenote::PACENOTE_CHANNEL_VOLUME)
                }
                PauseEffect::MenuSound {
                    table, sound, ok, ..
                } => {
                    if !ok
                        || (table == cmr_core::pause_menu::callback::SOUNDS_PREVIEW
                            && sound == DescriptorSound::ValueChange)
                    {
                        continue;
                    }
                    let cue = match sound {
                        DescriptorSound::Move => MenuCue::UpDown,
                        DescriptorSound::ValueChange => MenuCue::LeftRight,
                        DescriptorSound::Enter => MenuCue::Forward,
                        DescriptorSound::Back => MenuCue::Backward,
                    };
                    let Ok(mut mixer) = mixer.lock() else {
                        continue;
                    };
                    let channel = match self.menu_cues {
                        Some(channel) => channel,
                        None => match load_menu_cues(&mut mixer) {
                            Some(channel) => *self.menu_cues.insert(channel),
                            None => continue,
                        },
                    };
                    let commands =
                        play_menu_sound(channel, cue, self.options.effects_master_4f653c);
                    let _ = apply_commands(&mut *mixer, &commands);
                }
                // No surface loops, force feedback or telemetry in this
                // session; the fade is the hub route's tween.
                PauseEffect::ZeroForceFeedback
                | PauseEffect::FadeOut { .. }
                | PauseEffect::CommitTelemetryAndStopAudio => {}
            }
        }
    }
}

fn set_speech_volume(mixer: &SharedSoftwareMixer, volume: i32) {
    if let Ok(mut mixer) = mixer.lock() {
        EngineVoices::set_volume(&mut *mixer, SPEECH_CHANNEL, volume);
    }
}

/// The pause menu's cue bank (`FUN_00468f50`: `sounds\menu\` FORWARD,
/// BACKWARD, LEFTRIGH, UPDOWN at 22050 Hz), under the install root `game\`.
fn load_menu_cues(mixer: &mut SoftwareMixer<FileWaveSource>) -> Option<MenuChannel> {
    let mut base = None;
    for file in cmr_core::audio_assets::MENU_WAVE_FILES {
        let slot = mixer
            .load_wave(&format!("game\\{file}"), 22_050, 16, false)
            .ok()?;
        base.get_or_insert(slot);
    }
    Some(MenuChannel {
        channel: ChannelId::new(PAUSE_MENU_CHANNEL).ok()?,
        base_slot: base?,
    })
}

/// The first mixer channel of the engine's eight. The windowed session uses
/// the native allocation order, inferred from the stage-load sequence with
/// the channel counter starting at 0: the pause menu cues (`FUN_00468f50`,
/// 0), the engine (1..=8), the prang bank (9..=13), the speech channel
/// (`FUN_00444160`, 14) and the surface bank (15..=18). So the surface tick's
/// absolute `stop(0x11)` lands on surface channel 2 and the car loader's
/// `FUN_0047eb70(7, ..)` on the engine's RT channel.
const ENGINE_CHANNEL_BASE: u32 = 1;

impl EngineAudio {
    /// Open the band tables, or `None` when the store is absent or rejected
    /// (the session then runs without engine sound).
    fn open(runtime_data: &Path, car_nibble: u8) -> Option<Self> {
        let tables = match open_engine_audio_tables(runtime_data) {
            Ok(tables) => tables,
            Err(error) => {
                eprintln!("cmr-app: engine sound disabled: {error}");
                return None;
            }
        };
        Some(Self {
            tables,
            car_nibble,
            state: EngineSoundState::new(),
            config: None,
        })
    }

    /// The car loader `FUN_004872b0`: register the row's 14 waves in slot
    /// order, the loader's stray `FUN_0047eb70(7, 0x1a0a)` on absolute
    /// channel 7, then the init `FUN_00487250`.
    fn load(
        &mut self,
        mixer: &mut SoftwareMixer<FileWaveSource>,
        master_percent: i32,
    ) -> Option<EngineSoundConfig> {
        let row = car_row_index_for_selector(self.car_nibble);
        let names = car_wave_names(row)?;
        let mut slots = Vec::with_capacity(names.len());
        for name in &names {
            let path = car_wave_source_path(name);
            slots.push(
                mixer
                    .load_wave(&path, CAR_WAVE_RATE_HZ, CAR_WAVE_BITS, false)
                    .ok()?,
            );
        }
        let config = EngineSoundConfig {
            channel_base: ENGINE_CHANNEL_BASE,
            wave_base: slots[0].ordinal(),
            antilag_family_540: antilag_family_for_nibble(self.car_nibble),
            car_nibble: self.car_nibble,
            helium: false,
            master_percent,
        };
        EngineVoices::set_volume(mixer, 7, config.master_percent * 0x1a0a / 100);
        self.state.stage_init(mixer, &config, false);
        Some(config)
    }

    /// `FUN_00487170(0, car)` on pause entry: volume 0 on each of the car's
    /// engine channels that is playing (`FUN_00487210` with level 0).
    fn mute(&self, mixer: &SharedSoftwareMixer, car: u32) {
        let (Some(config), Ok(mut mixer)) = (self.config, mixer.lock()) else {
            return;
        };
        for n in [0, 1, 2, 3, 5, 6, 7, 4] {
            let channel = config.channel_base + car * 8 + n;
            if EngineVoices::playing(&*mixer, channel) {
                EngineVoices::set_volume(&mut *mixer, channel, 0);
            }
        }
    }

    /// `FUN_00488d60(0)` with this tick's listener; `master_percent` is the
    /// live effects master `DAT_004f653c`.
    fn tick(
        &mut self,
        mixer: &SharedSoftwareMixer,
        session: &mut GameplaySession,
        rng: &mut GameLcg,
        camera_mode: i32,
        listener: &cmr_core::engine_sound::EngineListener,
        master_percent: i32,
    ) {
        let Some(car_words) = session.engine_sound_car() else {
            return;
        };
        let Ok(mut mixer) = mixer.lock() else {
            return;
        };
        let config = match self.config {
            Some(config) => config,
            None => match self.load(&mut mixer, master_percent) {
                Some(config) => *self.config.insert(config),
                None => return,
            },
        };
        let config = EngineSoundConfig {
            master_percent,
            ..config
        };
        match self.state.update(
            &mut *mixer,
            &config,
            &self.tables,
            rng,
            0,
            &car_words,
            camera_mode,
            listener,
        ) {
            Ok(outcome) => session.set_idle_countdown_6ec(outcome.idle_countdown_6ec),
            Err(error) => eprintln!("cmr-app: engine sound tick failed: {error}"),
        }
    }
}

/// The race's sound banks: the engine ([`EngineAudio`]) and the surface,
/// landing and impact effects ([`EffectsAudio`]).
#[derive(Default)]
struct RaceAudio {
    engine: Option<EngineAudio>,
    effects: Option<EffectsAudio>,
}

/// The surface roll/skid (`FUN_00489890`), landing thump (`FUN_0046a030`)
/// and impact (`FUN_00469e80`) sounds over the extracted static tables and
/// the stage's `.srf` list. The wet/rain (`FUN_0048d4c0`) and crowd
/// (`FUN_0041cfe0`) channels are not loaded in this session.
struct EffectsAudio {
    tables: SurfaceAudioTables,
    prang_names: Vec<String>,
    full_volume: i32,
    srf: Vec<u8>,
    surface: SurfaceSoundState,
    prang: PrangSoundState,
    wet: WetSoundState,
    wet_k: i32,
    wet_surfaces: [i32; 15],
    /// The stage's crowd points (`FUN_00481990`) and factor `DAT_004eb2b0`.
    crowd_points: Vec<[i32; 3]>,
    crowd_k: i32,
    ambience: AmbienceController,
    banks: Option<EffectBanks>,
    camera_mode: Option<i32>,
}

/// The first wave slot of each effects bank once loaded.
#[derive(Clone, Copy)]
struct EffectBanks {
    surface: u32,
    prang: u32,
    wet: u32,
    ambience: WaveSlotId,
}

/// The prang bank's five channels and the surface bank's four (harness:
/// the native allocation order, see [`ENGINE_CHANNEL_BASE`]).
const PRANG_CHANNEL_BASE: u32 = 9;
const SURFACE_CHANNEL_BASE: u32 = 15;
/// The wet/rain channel (`FUN_0048d340`, after the surface bank).
const WET_CHANNEL: u32 = 19;
/// The crowd/weather ambience channel (`FUN_0041cd30`, after the wet one).
const AMBIENCE_CHANNEL: u32 = 20;
/// Harness: the weather-detail level `FUN_0046b230` is not the storm level
/// 3, so the thunder waves are not loaded and the storm never runs.
const AMBIENCE_WEATHER_DETAIL: i32 = 0;

impl EffectsAudio {
    fn open(
        runtime_data: &Path,
        srf: Option<Vec<u8>>,
        crowd_points: Vec<[i32; 3]>,
    ) -> Option<Self> {
        let reviewed =
            ReviewedSurfaceAudioTablesProfile::from_json(REVIEWED_SURFACE_AUDIO_TABLES_PROFILE)
                .ok()?;
        let store = match SurfaceAudioTablesStore::open(
            &session_sidecar_store(runtime_data, SURFACE_AUDIO_TABLES_STORE_DIRECTORY),
            &reviewed,
        ) {
            Ok(store) => store,
            Err(error) => {
                eprintln!("cmr-app: surface/impact sound disabled: {error:?}");
                return None;
            }
        };
        let t = store.tables();
        let tables = SurfaceAudioTables {
            counts: t.counts.map(|c| [c.ski, c.i, c.sko, c.o]),
            class_percent: t.class_volume_percent,
            surface_group: t.surface_group,
            wet_surfaces: t.wet_surfaces,
            surface_k: t.surface_volume_k,
            initial_last_state: t.initial_last_state,
        };
        Some(Self {
            surface: SurfaceSoundState::new(&tables),
            prang: PrangSoundState::new(t.prang_full_volume),
            wet: WetSoundState::default(),
            wet_k: t.wet_volume_k,
            wet_surfaces: t.wet_surfaces,
            full_volume: t.prang_full_volume,
            prang_names: t.prang_names.clone(),
            tables,
            srf: srf.unwrap_or_default(),
            crowd_points,
            crowd_k: t.crowd_volume_k,
            ambience: AmbienceController::new(),
            banks: None,
            camera_mode: None,
        })
    }

    /// The prang loader `FUN_00469ac0` (48 waves, five channels at the full
    /// volume), the surface loader `FUN_004892b0` (the `.srf` groups, four
    /// channels at volume 0) and the wet loader `FUN_0048d340`, in the
    /// stage-load order, then the ambience loader `FUN_0041cd30` (its channel
    /// at volume 0). Harness: the weather type is 0 (not the rain type 3),
    /// so `rain_in` is not loaded.
    fn load(
        &mut self,
        mixer: &mut SoftwareMixer<FileWaveSource>,
        master_percent: i32,
        camera_mode: i32,
    ) -> Option<EffectBanks> {
        let mut prang_base = None;
        for path in prang_wave_paths(&self.prang_names) {
            let slot = mixer
                .load_wave(
                    &format!("game\\{path}"),
                    CAR_WAVE_RATE_HZ,
                    CAR_WAVE_BITS,
                    false,
                )
                .ok()?;
            prang_base.get_or_insert(slot.ordinal());
        }
        for n in 0..5 {
            EngineVoices::set_volume(
                mixer,
                PRANG_CHANNEL_BASE + n,
                master_percent.wrapping_mul(self.full_volume) / 100,
            );
        }
        self.prang = PrangSoundState::new(self.full_volume);
        let bank = match load_surface_bank(&self.tables, &self.srf, &mut self.surface) {
            Ok(bank) => bank,
            Err(error) => {
                eprintln!("cmr-app: surface bank: {error}");
                return None;
            }
        };
        let mut surface_base = 0;
        for (index, name) in bank.waves.iter().enumerate() {
            let slot = mixer
                .load_wave(
                    &format!("game\\sounds\\newsurfs\\{name}"),
                    CAR_WAVE_RATE_HZ,
                    CAR_WAVE_BITS,
                    false,
                )
                .ok()?;
            if index == 0 {
                surface_base = slot.ordinal();
            }
        }
        for n in 0..4 {
            EngineVoices::set_volume(mixer, SURFACE_CHANNEL_BASE + n, 0);
        }
        let wet_bank = load_wet_bank(
            &mut self.wet,
            WetLoad {
                rain_weather: false,
                wet_surface: bank.wet,
                camera_mode,
            },
        );
        EngineVoices::set_volume(mixer, WET_CHANNEL, 0);
        let mut wet_base = 0;
        for (index, path) in wet_bank.waves.iter().enumerate() {
            let slot = mixer
                .load_wave(
                    &format!("game\\{path}"),
                    CAR_WAVE_RATE_HZ,
                    CAR_WAVE_BITS,
                    false,
                )
                .ok()?;
            if index == 0 {
                wet_base = slot.ordinal();
            }
        }
        if wet_bank.start_rain {
            EngineVoices::play(mixer, wet_base, WET_CHANNEL, u32::MAX);
        }
        EngineVoices::set_volume(mixer, AMBIENCE_CHANNEL, 0);
        let ambience = load_weather_ambience(mixer, "game\\", AMBIENCE_WEATHER_DETAIL).ok()?;
        Some(EffectBanks {
            surface: surface_base,
            prang: prang_base?,
            wet: wet_base,
            ambience: *ambience.first()?,
        })
    }

    /// `FUN_0048b650`.
    fn stop_surface(&self, mixer: &SharedSoftwareMixer) {
        if self.banks.is_some()
            && let Ok(mut mixer) = mixer.lock()
        {
            stop_surface_channels(&mut *mixer, SURFACE_CHANNEL_BASE, true);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn tick(
        &mut self,
        mixer: &SharedSoftwareMixer,
        session: &mut GameplaySession,
        rng: &mut GameLcg,
        camera_world: &[i32; 16],
        camera_mode: i32,
        centre_proximity: i32,
        master_percent: i32,
        stepped: bool,
        state_id: u8,
    ) {
        let Ok(mut mixer) = mixer.lock() else {
            return;
        };
        let banks = match self.banks {
            Some(banks) => banks,
            None => match self.load(&mut mixer, master_percent, camera_mode) {
                Some(banks) => *self.banks.insert(banks),
                None => return,
            },
        };
        // FUN_0048b680 runs when the camera driver sets a mode.
        if self.camera_mode != Some(camera_mode) {
            self.camera_mode = Some(camera_mode);
            set_surface_class(
                &mut self.surface,
                &mut *mixer,
                SURFACE_CHANNEL_BASE,
                0,
                camera_mode,
            );
        }
        let car = session.car();
        let centre = vec3_raw(car.position);
        // Impacts fire inside Mechanics, so only on a stepped tick: static
        // scenery first (`FUN_0043d760`), then the barrier walk
        // (`FUN_00401fd0`), then the tumbling contact (`FUN_00412860`). The damage model that yields the breakage kind is
        // not reconstructed, so no breakage cue plays.
        if stepped {
            let static_impacts = session
                .last_static_impacts()
                .iter()
                .map(|impact| (impact.severity, impact.world_contact, impact.material as i8));
            let barrier_impacts = session.last_trigger_events().iter().filter_map(|event| {
                let cmr_core::trigger_walk::TriggerEvent::Barrier {
                    kind,
                    impact: Some(impact),
                    ..
                } = *event
                else {
                    return None;
                };
                let severity = impact_severity(impact.severity_vector);
                let object_type = barrier_object_type(kind).filter(|_| severity > 0)?;
                Some((severity, impact.point, object_type))
            });
            // The ground pass (and its tumbling contact `FUN_00412860`, object
            // type 0) runs after the trigger walk.
            let tumble_impact = session.last_tumble_impact().and_then(|impact| {
                let severity = impact_severity(impact.severity_vector);
                (severity > 0).then_some((severity, impact.point, 0_i8))
            });
            let impacts: Vec<_> = static_impacts
                .chain(barrier_impacts)
                .chain(tumble_impact)
                .collect();
            for (severity, point, object_type) in impacts {
                let Ok(azimuth) = point_azimuth(camera_world, camera_mode, centre, point) else {
                    continue;
                };
                let mut out = PrangVoices {
                    voices: &mut *mixer,
                    slot_base: banks.prang,
                    channel_base: PRANG_CHANNEL_BASE,
                    centre_proximity,
                    master_percent,
                };
                play_impact(&self.prang, &mut out, severity, azimuth, object_type);
            }
        }
        self.crowd_tick(
            &mut mixer,
            session,
            banks,
            centre_proximity,
            master_percent,
            state_id,
        );
        let Some(words) = session.surface_sound_words() else {
            return;
        };
        let input = SurfaceTickInput {
            surfaces: words.surfaces,
            contact: std::array::from_fn(|wheel| words.contact[wheel]),
            longitudinal: words.longitudinal,
            lateral: words.lateral,
            speed_ec: words.speed_ec,
            effects_off: false,
            centre_proximity,
            master_percent,
        };
        match self.surface.tick(
            &mut *mixer,
            &self.tables,
            rng,
            banks.surface,
            SURFACE_CHANNEL_BASE,
            &input,
        ) {
            Ok(outcome) => session.set_surface_words(outcome.surfaces),
            Err(error) => eprintln!("cmr-app: surface sound tick failed: {error}"),
        }
        let mut out = PrangVoices {
            voices: &mut *mixer,
            slot_base: banks.prang,
            channel_base: PRANG_CHANNEL_BASE,
            centre_proximity,
            master_percent,
        };
        // `car +0x704`, which the thump sets, has no reconstructed reader.
        let _ = self.prang.thump(
            &mut out,
            rng,
            &ThumpInput {
                effects_off: false,
                upright_404: words.upright_404,
                out_of_control_680: words.out_of_control_680,
                contact: words.contact,
            },
        );
        // FUN_0048d4c0 reads the surface words after the surface tick's
        // rewrite. `ctl +0x164` only feeds the rain branch, which needs
        // `rain_in` and so never runs in this session.
        let words = session.surface_sound_words().unwrap_or(words);
        self.wet.tick(
            &mut *mixer,
            banks.wet,
            WET_CHANNEL,
            &WetTickInput {
                surfaces: words.surfaces,
                contact: std::array::from_fn(|wheel| words.contact[wheel]),
                longitudinal: words.longitudinal,
                lateral: words.lateral,
                speed_ec: words.speed_ec,
                control_164: 0,
                camera_mode,
                wet_surfaces: self.wet_surfaces,
                wet_k: self.wet_k,
                centre_proximity,
                master_percent,
            },
        );
    }

    /// `FUN_0041cfe0`, from the in-race handlers 5, 6 and 8 only (not the
    /// states 7 or 9). Storm off (see [`AMBIENCE_WEATHER_DETAIL`]), so the
    /// abduct and thunder branches never run.
    fn crowd_tick(
        &mut self,
        mixer: &mut SoftwareMixer<FileWaveSource>,
        session: &GameplaySession,
        banks: EffectBanks,
        centre_proximity: i32,
        master_percent: i32,
        state_id: u8,
    ) {
        if !matches!(state_id, 5 | 6 | 8) {
            return;
        }
        let Ok(channel) = ChannelId::new(AMBIENCE_CHANNEL) else {
            return;
        };
        let cue = crowd_cue(
            &self.crowd_points,
            vec3_raw(session.car().position),
            self.crowd_k,
            centre_proximity,
        );
        let busy = EngineVoices::playing(mixer, AMBIENCE_CHANNEL);
        let commands = self.ambience.crowd_or_storm_tick(
            AmbienceChannel {
                channel,
                base_slot: banks.ambience,
            },
            cue,
            busy,
            master_percent,
        );
        if let Err(error) = apply_commands(mixer, &commands) {
            eprintln!("cmr-app: crowd sound failed: {error:?}");
        }
    }
}

#[derive(Debug)]
enum EngineAudioOpenError {
    Profile(ReviewedEngineAudioTablesProfileError),
    Store(EngineAudioTablesStoreError),
    Tables(cmr_core::engine_sound::EngineSoundError),
}

impl std::fmt::Display for EngineAudioOpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Profile(error) => write!(f, "engine audio profile: {error:?}"),
            Self::Store(error) => write!(f, "engine audio tables store: {error:?}"),
            Self::Tables(error) => write!(f, "engine audio tables: {error}"),
        }
    }
}

/// `FUN_00481990`'s crowd points over the stage `.obj` nodes (the range
/// `DAT_0051445c..DAT_00514010` brackets the `.obj` load `FUN_00480c50`).
/// No shipped `.obj` group-0 record carries id `0xe`, so this is empty on the
/// reviewed install and the crowd stays silent there, as it would natively.
fn stage_crowd_points(resources: &StageRenderResources) -> Vec<[i32; 3]> {
    let Some(objects) = resources.bundle.objects() else {
        return Vec::new();
    };
    crowd_points(
        objects
            .group0_records()
            .map(|record| (record.node_class(), record.affine_matrix_q16())),
    )
}

fn vec3_raw(v: Vec3) -> [i32; 3] {
    [v.x.raw(), v.y.raw(), v.z.raw()]
}

fn engine_band(band: &cmr_formats::pe_engine_audio_tables::EngineSoundBand) -> RevBandTable {
    RevBandTable {
        count: band.count,
        lo: band.lo,
        hi: band.hi,
        pitch: band.pitch.clone(),
        volume: band.vol.clone(),
    }
}

fn open_engine_audio_tables(
    runtime_data: &Path,
) -> Result<EngineAudioTables, EngineAudioOpenError> {
    let reviewed =
        ReviewedEngineAudioTablesProfile::from_json(REVIEWED_ENGINE_AUDIO_TABLES_PROFILE)
            .map_err(EngineAudioOpenError::Profile)?;
    let store = EngineAudioTablesStore::open(
        &session_sidecar_store(runtime_data, ENGINE_AUDIO_TABLES_STORE_DIRECTORY),
        &reviewed,
    )
    .map_err(EngineAudioOpenError::Store)?;
    let t = store.tables();
    EngineAudioTables::new(
        engine_band(&t.slow),
        engine_band(&t.mid),
        engine_band(&t.top),
        engine_band(&t.whine),
        t.upshift_dip,
    )
    .map_err(EngineAudioOpenError::Tables)
}

/// Authenticated static tyre/surface tables for the wheel-force producer.
#[derive(Debug)]
pub struct SessionTyreTables {
    store: TyreTablesStore,
}

impl SessionTyreTables {
    pub fn open(runtime_data: &Path) -> Result<Self, SessionTyreTablesError> {
        let reviewed = ReviewedTyreTablesProfile::from_json(REVIEWED_TYRE_TABLES_PROFILE)
            .map_err(SessionTyreTablesError::Profile)?;
        let store = TyreTablesStore::open(
            &session_sidecar_store(runtime_data, TYRE_TABLES_STORE_DIRECTORY),
            &reviewed,
        )
        .map_err(SessionTyreTablesError::Store)?;
        Ok(Self { store })
    }

    #[must_use]
    pub fn tables(&self) -> &cmr_formats::pe_tyre_tables::TyreSurfaceTables {
        self.store.tables()
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
        constants: &CompiledFourWheelConstants,
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
            // Both wheel loops read wheel `w`'s contact vector at `+0x4bc + 12w`.
            contact_vector_indices: [0, 1, 2, 3],
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
            // Harness scan without live ground: every probe touches, so the
            // integrator's contact sum includes all lanes (live ground
            // replaces this with the real scan).
            lane_threshold_inputs:
                cmr_core::compiled_car_initializer::CompiledLaneThresholdInputs {
                    lane_words: [0; 8],
                    source_y: [0; 8],
                },
            pedal_response: cmr_core::pedal_response::PedalResponseParams::default(),
            lane_time_step: SOLVER_GLOBAL_CONSTANTS.time_step,
            lane_direction_angle: 0,
            steering: cmr_core::steering_response::SteeringParams::default(),
            steering_car_factor_e188: 0x1_0000,
            steering_surface_factors: [0; 2],
            gearbox: None,
            brake_balance_e1a4: [0x1_0000; 2],
            tyre: None,
            hull_contact_offsets: [Vec3::ZERO; 4],
            // Harness value: never crest.
            crest_128_12c: [i32::MAX, 0],
            suspension: cmr_core::wheel_animation::SuspensionConstants::default(),
            wheel_setup_800: [0; 4],
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
            inverse_mass_4c: Fixed16::ZERO,
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
                SOLVER_GLOBAL_CONSTANTS.time_step,
            );
        }
        self.populate_four_wheel(&mut wheel, surface_indices)?;
        populate_compiled_contact_offsets(&mut wheel, half_state);
        cmr_core::race_state::populate_compiled_four_wheel_constants(&mut wheel, constants);
        cmr_core::race_state::populate_compiled_contact_update_inputs(
            &mut wheel,
            &cmr_core::compiled_car_initializer::derive_compiled_control_clear_loop(),
        );
        Ok(wheel)
    }
}

/// Loader-zeroed lane state owned for the lifetime of one native game session.
///
/// The shipped PE loader establishes these zeros once. Per-car setup only
/// re-zeroes the eight handles, so repeated setup on the same owner preserves
/// every other lane field until the recovered executor changes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FreshProcessLaneState {
    state: CompiledEightLaneExecutionState,
    gate_words: [i32; 8],
    drive_words: [i32; 8],
    first_four_subtractions: [i32; 4],
}

impl FreshProcessLaneState {
    fn new() -> Self {
        Self {
            state: CompiledEightLaneExecutionState {
                handles: [0; 8],
                type_words: [0; 8],
                output_vectors: [[0; 3]; 8],
                lane_words: [0; 8],
                accumulator_words: [0; 8],
                latch_67c: 0,
            },
            gate_words: [0; 8],
            drive_words: [0; 8],
            first_four_subtractions: [0; 4],
        }
    }

    fn begin_car_setup(&mut self) -> CompiledEightLaneExecutionState {
        self.state.handles = [0; 8];
        self.state
    }

    fn publish(&mut self, state: CompiledEightLaneExecutionState) {
        self.state = state;
    }

    fn inputs(&self) -> CompiledSetupPoseLaneInputs {
        CompiledSetupPoseLaneInputs {
            gate_words: self.gate_words,
            drive_words: self.drive_words,
            first_four_subtractions: self.first_four_subtractions,
        }
    }
}

pub type SessionCompiledPoseCoreError =
    CompiledSetupPoseError<DecodedCompiledLaneGeometryError, Infallible>;

#[derive(Debug)]
pub enum SessionCompiledPoseError {
    AssetProfile(ReviewedProfileError),
    AssetStore(AssetStoreError),
    CompiledProfile(ReviewedCompiledInitializerProfileError),
    CompiledStore(CompiledInitializerStoreError),
    DirectCases(CompiledDirectCaseSetError),
    SelectorProfile(ReviewedCarSelectorProfileError),
    SelectorStore(CarSelectorStoreError),
    UnknownCarCode(String),
    UnsupportedStage(String),
    Geometry(StageCompiledGeometryLoadError),
    Execution(SessionCompiledPoseCoreError),
    /// The selected car's case words fault a recovered initializer stage.
    CarInputs(RecoveredCompiledStageError),
}

fn compiled_setup_failure_metadata<ProviderError, ConfigError>(
    error: &CompiledSetupPoseError<ProviderError, ConfigError>,
) -> (&'static str, Option<usize>) {
    match error {
        CompiledSetupPoseError::CommonPrefix(_) => ("compiled-pose-common-prefix", None),
        CompiledSetupPoseError::Lanes(CompiledEightLaneExecutionFailure::Dispatch {
            failed_lane,
            failure,
            ..
        }) => {
            let kind = match failure.phase {
                CompiledLaneDispatchFailurePhase::Validate => "compiled-pose-lane-validate",
                CompiledLaneDispatchFailurePhase::Search => "compiled-pose-lane-search",
                CompiledLaneDispatchFailurePhase::Build => "compiled-pose-lane-build",
            };
            (kind, Some(*failed_lane))
        }
        CompiledSetupPoseError::Lanes(CompiledEightLaneExecutionFailure::Config {
            failed_lane,
            ..
        }) => ("compiled-pose-lane-config", Some(*failed_lane)),
    }
}

impl SessionCompiledPoseError {
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::AssetProfile(_) => "compiled-pose-asset-profile",
            Self::AssetStore(_) => "compiled-pose-asset-store",
            Self::CompiledProfile(_) => "compiled-pose-case-profile",
            Self::CompiledStore(_) => "compiled-pose-case-store",
            Self::DirectCases(_) => "compiled-pose-case-schema",
            Self::SelectorProfile(_) => "compiled-pose-selector-profile",
            Self::SelectorStore(_) => "compiled-pose-selector-store",
            Self::UnknownCarCode(_) => "compiled-pose-unknown-car",
            Self::UnsupportedStage(_) => "compiled-pose-unsupported-stage",
            Self::Geometry(_) => "compiled-pose-geometry",
            Self::Execution(error) => compiled_setup_failure_metadata(error).0,
            Self::CarInputs(_) => "compiled-pose-car-inputs",
        }
    }

    #[must_use]
    pub fn failed_lane(&self) -> Option<usize> {
        match self {
            Self::Execution(error) => compiled_setup_failure_metadata(error).1,
            _ => None,
        }
    }
}

impl fmt::Display for SessionCompiledPoseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AssetProfile(error) => {
                write!(formatter, "invalid reviewed asset profile: {error}")
            }
            Self::AssetStore(error) => {
                write!(formatter, "cannot open authenticated asset store: {error}")
            }
            Self::CompiledProfile(error) => {
                write!(formatter, "invalid compiled-case profile: {error}")
            }
            Self::CompiledStore(error) => {
                write!(formatter, "cannot open compiled-case store: {error}")
            }
            Self::DirectCases(error) => write!(formatter, "cannot bind compiled cases: {error}"),
            Self::SelectorProfile(error) => {
                write!(formatter, "invalid car-selector profile: {error}")
            }
            Self::SelectorStore(error) => {
                write!(formatter, "cannot open car-selector store: {error}")
            }
            Self::UnknownCarCode(code) => write!(
                formatter,
                "car code {code:?} has no recovered initializer selector"
            ),
            Self::UnsupportedStage(stage) => write!(
                formatter,
                "stage {stage:?} has no ordinary-stage authenticated geometry identity"
            ),
            Self::Geometry(error) => error.fmt(formatter),
            Self::CarInputs(error) => write!(formatter, "cannot derive car inputs: {error}"),
            Self::Execution(CompiledSetupPoseError::CommonPrefix(error)) => error.fmt(formatter),
            Self::Execution(CompiledSetupPoseError::Lanes(
                CompiledEightLaneExecutionFailure::Dispatch {
                    failed_lane,
                    failure,
                    ..
                },
            )) => write!(
                formatter,
                "compiled setup-time pose lane {failed_lane} {:?} failed: {:?}",
                failure.phase, failure.provider_error
            ),
            Self::Execution(CompiledSetupPoseError::Lanes(
                CompiledEightLaneExecutionFailure::Config { config_error, .. },
            )) => match *config_error {},
        }
    }
}

impl std::error::Error for SessionCompiledPoseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::AssetProfile(error) => Some(error),
            Self::AssetStore(error) => Some(error),
            Self::CompiledProfile(error) => Some(error),
            Self::CompiledStore(error) => Some(error),
            Self::DirectCases(error) => Some(error),
            Self::SelectorProfile(error) => Some(error),
            Self::SelectorStore(error) => Some(error),
            Self::Geometry(error) => Some(error),
            Self::CarInputs(error) => Some(error),
            Self::Execution(CompiledSetupPoseError::CommonPrefix(error)) => Some(error),
            Self::UnknownCarCode(_)
            | Self::UnsupportedStage(_)
            | Self::Execution(CompiledSetupPoseError::Lanes(_)) => None,
        }
    }
}

/// Authenticated compiled inputs and mutable loader-zero state for one window.
struct SessionCompiledPose {
    direct_cases: CompiledDirectCaseSet,
    geometry: StageCompiledGeometry,
    selector: u32,
    lanes: FreshProcessLaneState,
}

fn publish_session_compiled_pose(
    start: &mut CarState,
    effective_state_words: [i32; 6],
    adjusted_center_y: i32,
    final_published_normal: [i32; 3],
    corrected_basis: &CompiledPostLaneBasisEffects,
    lane_words: [i32; 8],
    contact_sources: [[i32; 3]; 8],
) -> cmr_core::live_static_collision_car_frame::UpperHullDeformationScalars {
    let half_state = derive_compiled_half_sign_state([
        effective_state_words[0],
        effective_state_words[1],
        effective_state_words[2],
    ]);
    start.motion.apply_compiled_half_state(&half_state);
    start
        .motion
        .apply_compiled_post_lane_basis(final_published_normal, corrected_basis);
    start.position.y = Fixed16::from_raw(adjusted_center_y);
    let (contact_distance_flags, contact_count_674) =
        cmr_core::vehicle_dynamics::classify_contact_distance_lanes(
            std::array::from_fn(|index| lane_words[index]),
            std::array::from_fn(|index| contact_sources[index][1]),
        );
    start.motion.contact_distance_flags = contact_distance_flags;
    start.motion.contact_count_674 = i32::from(contact_count_674);
    let deformation = cmr_core::live_static_collision_car_frame::UpperHullDeformationScalars {
        scale_638: Fixed16::from_raw(effective_state_words[3]),
        scale_63c: Fixed16::from_raw(effective_state_words[4]),
        scale_640: Fixed16::from_raw(effective_state_words[5]),
    };
    start.motion.upper_hull_deformation = deformation;
    deformation
}

impl SessionCompiledPose {
    fn open(
        runtime_data: &Path,
        stage: &str,
        car_code: &str,
    ) -> Result<Self, SessionCompiledPoseError> {
        let reviewed_assets = ReviewedProfile::from_json(REVIEWED_PROFILE)
            .map_err(SessionCompiledPoseError::AssetProfile)?;
        let assets = AssetStore::open(runtime_data, &reviewed_assets, DecodeLimits::default())
            .map_err(SessionCompiledPoseError::AssetStore)?;

        let reviewed_cases =
            ReviewedCompiledInitializerProfile::from_json(REVIEWED_COMPILED_INITIALIZER_PROFILE)
                .map_err(SessionCompiledPoseError::CompiledProfile)?;
        let case_store = CompiledInitializerStore::open(
            &session_sidecar_store(runtime_data, COMPILED_INITIALIZER_STORE_DIRECTORY),
            &reviewed_cases,
        )
        .map_err(SessionCompiledPoseError::CompiledStore)?;
        let direct_cases = CompiledDirectCaseSet::from_store(&case_store)
            .map_err(SessionCompiledPoseError::DirectCases)?;

        let reviewed_selectors =
            ReviewedCarSelectorProfile::from_json(REVIEWED_CAR_SELECTOR_PROFILE)
                .map_err(SessionCompiledPoseError::SelectorProfile)?;
        let selector_store = CarSelectorStore::open(
            &session_sidecar_store(runtime_data, CAR_SELECTOR_STORE_DIRECTORY),
            &reviewed_selectors,
        )
        .map_err(SessionCompiledPoseError::SelectorStore)?;
        let selector = selector_store
            .selector_for_code(car_code)
            .ok_or_else(|| SessionCompiledPoseError::UnknownCarCode(car_code.to_owned()))?;

        let stage_base = ordinary_stage_asset_base(stage)
            .ok_or_else(|| SessionCompiledPoseError::UnsupportedStage(stage.to_owned()))?;
        let geometry = StageCompiledGeometry::load(&assets, &stage_base)
            .map_err(SessionCompiledPoseError::Geometry)?;
        Ok(Self {
            direct_cases,
            geometry,
            selector: u32::from(selector),
            lanes: FreshProcessLaneState::new(),
        })
    }

    fn adjust_spawn(
        &mut self,
        start: &mut CarState,
        actor_rows: ActorMatrixRows,
    ) -> Result<
        cmr_core::live_static_collision_car_frame::UpperHullDeformationScalars,
        SessionCompiledPoseError,
    > {
        let center = [
            start.position.x.raw(),
            start.position.y.raw(),
            start.position.z.raw(),
        ];
        let input = CompiledSetupPoseInputs {
            prior_case_words: CompiledCaseWords::default(),
            selector: self.selector,
            center,
            matrix_rows: actor_rows,
            common_config_25: 0,
            lane: self.lanes.inputs(),
        };
        let state = self.lanes.begin_car_setup();
        let lane_inputs = self.lanes.inputs();
        let mut config = |_lane: usize| Ok::<u8, std::convert::Infallible>(0);
        let effects = execute_stage_compiled_setup_pose(
            &self.direct_cases,
            input,
            state,
            &self.geometry,
            &mut config,
        )
        .map_err(SessionCompiledPoseError::Execution)?;
        self.lanes.publish(effects.lanes.state);
        let probes = effects.sources.contact_basis;
        start.ground = Some(cmr_core::ground_pass::GroundContact {
            lanes: effects.lanes.state,
            lane_inputs,
            half_words: derive_compiled_half_sign_state([
                effects.effective_state_words[0],
                effects.effective_state_words[1],
                effects.effective_state_words[2],
            ])
            .words[..3]
                .try_into()
                .expect("the half-sign state starts with the three half words"),
            deformation: [
                effects.effective_state_words[3],
                effects.effective_state_words[4],
                effects.effective_state_words[5],
            ],
            probes,
            thresholds: cmr_core::compiled_car_initializer::derive_compiled_lane_thresholds(
                cmr_core::compiled_car_initializer::CompiledLaneThresholdInputs {
                    lane_words: effects.lanes.state.lane_words,
                    source_y: probes.map(|probe| probe[1]),
                },
            ),
            wheel_reach_648_644: [
                effects.effective_state_words[6],
                effects.effective_state_words[7],
            ],
            reach_20: effects.effective_state_words[0],
            radius_44: cmr_core::compiled_car_initializer::derive_compiled_suffix_defaults(
                [
                    effects.effective_state_words[0],
                    effects.effective_state_words[1],
                    effects.effective_state_words[2],
                ],
                0,
            )
            .state_word_44,
        });
        // First-frame trigger snapshot and saved centre: the native
        // initialization of `+0x15c`/`+0x3e4` is not recovered, so the spawn
        // probes and centre stand in (an inference).
        start.four_wheel.trigger_prior_15c = probes;
        start.four_wheel.saved_position_3e4 = start.position;
        Ok(publish_session_compiled_pose(
            start,
            [
                effects.effective_state_words[0],
                effects.effective_state_words[1],
                effects.effective_state_words[2],
                effects.effective_state_words[3],
                effects.effective_state_words[4],
                effects.effective_state_words[5],
            ],
            effects.adjusted_center[1],
            effects.normal.final_published_normal,
            &effects.corrected_basis,
            effects.lanes.state.lane_words,
            effects.sources.contact_basis,
        ))
    }
}

/// Everything the four-wheel step needs from one car's compiled case words,
/// under the zero config the session's pose setup also uses.
#[derive(Clone, Copy, Debug)]
struct SessionCompiledCar {
    families: CompiledControlFamilies,
    constants: CompiledFourWheelConstants,
    gearbox_tables: cmr_core::gearbox::GearboxTables,
    half_state: CompiledHalfSignState,
    /// Tyre variant `car +0x758`, indexing the `DAT_004f777c` blocks.
    variant_758: i32,
}

impl SessionCompiledPose {
    /// Derive the selected car's four-wheel inputs from its direct case words.
    fn compiled_car(&self) -> Result<SessionCompiledCar, SessionCompiledPoseError> {
        use cmr_core::compiled_car_initializer::{
            derive_compiled_control_families, derive_compiled_four_wheel_constants,
            derive_compiled_gearbox_tables, derive_compiled_state_word_758,
            transform_compiled_control_family,
        };
        let words = self
            .direct_cases
            .apply(CompiledCaseWords::default(), self.selector)
            .words;
        let families = derive_compiled_control_families(transform_compiled_control_family(
            words.control_family(),
            0,
        ))
        .map_err(|error| {
            SessionCompiledPoseError::CarInputs(RecoveredCompiledStageError::Family(error))
        })?;
        let constants = derive_compiled_four_wheel_constants(words, 0)
            .map_err(SessionCompiledPoseError::CarInputs)?;
        let gearbox_tables = derive_compiled_gearbox_tables(words, 0, &families)
            .map_err(SessionCompiledPoseError::CarInputs)?;
        // Under the zero config the prefix does not rescale the state words,
        // so these are the effective words the pose setup also derives from.
        let half_state = derive_compiled_half_sign_state([
            words.state_words[0],
            words.state_words[1],
            words.state_words[2],
        ]);
        // A zero config has a zero context, so the selector returns the `-1`
        // sentinel and the record's zero fallback byte picks variant 0.
        let variant_758 = derive_compiled_state_word_758(-1, 0).state_word_758;
        Ok(SessionCompiledCar {
            families,
            constants,
            gearbox_tables,
            half_state,
            variant_758,
        })
    }

    /// The selected car's complete four-wheel inputs: the compiled constants,
    /// gearbox and tyre variant, the authenticated surface coefficients, the
    /// car-setup reset values (no config setup bytes are read) and the static
    /// tyre tables.
    fn compiled_four_wheel_params<'a>(
        &self,
        inputs: &SessionWheelInputs,
        trig: &'a [i32],
        tyre_tables: &'a cmr_formats::pe_tyre_tables::TyreSurfaceTables,
        aux: &SessionAuxControl,
    ) -> Result<FourWheelIterationParams<'a>, SessionFourWheelError> {
        let car = self
            .compiled_car()
            .map_err(SessionFourWheelError::CompiledCar)?;
        let mut wheel = inputs.four_wheel_params(
            trig,
            &car.families,
            &car.half_state,
            Fixed16::ONE,
            &car.constants,
            [0; 4],
        )?;
        wheel.gearbox = Some(cmr_core::gearbox::GearboxParams {
            tables: car.gearbox_tables,
            drive_ratios: cmr_core::race_state::compiled_drive_ratios(&car.families, Fixed16::ONE),
            ..cmr_core::gearbox::GearboxParams::default()
        });
        cmr_core::race_state::populate_car_setup(
            &mut wheel,
            &cmr_core::car_setup::CarSetupBlock::RESET,
        );
        wheel.tyre = Some(cmr_core::race_state::TyreSurfaceInputs {
            tables: tyre_tables,
            variant_758: car.variant_758,
        });
        // `FUN_0041b160` reads the player's sensitivity option; the session's
        // zero config selects option 0.
        wheel.steering = cmr_core::steering_response::steering_sensitivity(wheel.steering, 0);
        let [crest_128, crest_12c, travel_11c, limit_120, rate_124] = aux
            .patch_words(
                car.constants.state_word_48,
                [0x128, 0x12c, 0x11c, 0x120, 0x124],
            )
            .map_err(SessionFourWheelError::AuxControl)?;
        wheel.crest_128_12c = [crest_128, crest_12c];
        wheel.suspension = cmr_core::wheel_animation::SuspensionConstants {
            travel_gain_11c: travel_11c,
            limit_120,
            rate_124,
        };
        Ok(wheel)
    }
}

fn session_sidecar_store(runtime_data: &Path, directory: &str) -> PathBuf {
    let nested = runtime_data.join(directory);
    if nested.is_dir() {
        nested
    } else {
        runtime_data
            .parent()
            .unwrap_or(runtime_data)
            .join(directory)
    }
}

fn ordinary_stage_asset_base(stage: &str) -> Option<String> {
    let stage = stage.to_ascii_uppercase();
    let bytes = stage.as_bytes();
    if bytes.len() != 3 || !bytes[2].is_ascii_digit() {
        return None;
    }
    let country = match &stage[..2] {
        "AU" => "Au",
        "CO" => "Co",
        "GR" => "Gr",
        "IN" => "In",
        "MC" => "MC",
        "NZ" => "NZ",
        "SW" => "Sw",
        "UK" => "UK",
        _ => return None,
    };
    Some(format!("Game/{country}/{}/{stage}", bytes[2] as char))
}

fn native_stage_setup(
    media_root: &Path,
    runtime_data: &Path,
    stage: &str,
    car_code: Option<&str>,
    renderer: StageRendererChoice,
) -> Result<NativeStageSetup, StageFrameError> {
    let palette = load_native_stage_palette(runtime_data, stage)?;
    let mut resources =
        StageRenderResources::load_with_palette(media_root, stage, renderer, palette)?;
    if let Some(car_code) = car_code {
        resources.attach_car(runtime_data, car_code)?;
    }
    resources.attach_loading_overlay(runtime_data)?;
    resources.attach_race_hud(runtime_data)?;
    let nodes = resources
        .bundle
        .nodes()
        .ok_or(StageFrameError::MissingNodes)?
        .to_vec();
    let (start, initial_camera, actor_rows) = if let Some(starts) = resources.bundle.starts() {
        native_stage_start_from_csp(starts.normal()[0])
    } else {
        native_stage_start(&nodes)?
    };
    Ok((resources, nodes, start, initial_camera, actor_rows))
}

/// The country directories in the native country order (`0x4f0db8`).
const COUNTRY_CODES: [&str; 8] = ["NZ", "GR", "MC", "AU", "SW", "CO", "IN", "UK"];

/// The event a session over one `--stage` runs. Harness for the unbuilt
/// setup screens and the cmRally.cfg commit `FUN_004b93f0`: a native
/// selected-stages event (mode 2) whose mask holds only that stage of its
/// country, with the country's stage count (6, or 7 for a special country),
/// as the time-trial setup commits it. State 1 then picks the stage, and the
/// hub ends the session after it. A stage name outside `<CC><n>` falls back to
/// a championship from the first country. The front end is up.
fn single_stage_event(stage: &str) -> EventState {
    let mut event = EventState::default();
    event.globals.frontend_506228 = true;
    let (code, number) = stage.split_at(stage.len().min(2));
    let country = COUNTRY_CODES
        .iter()
        .position(|candidate| candidate.eq_ignore_ascii_case(code));
    let index = number.parse::<u8>().ok().and_then(|n| n.checked_sub(1));
    if let (Some(country), Some(index)) = (country, index) {
        let country = country as u8;
        let count = 6 + u8::from(cmr_core::event_flow::SPECIAL_COUNTRIES.contains(&country));
        if index < count {
            event.cfg.mode_166 = 2;
            event.cfg.cursor_168 = country;
            event.cfg.stage_count_165 = count;
            event.cfg.stage_mask_164 = 1 << index;
        }
    }
    event
}

/// The windowed session's driving sim: the complete four-wheel producer with
/// the ground pass when the reviewed surface and tyre stores are present,
/// otherwise the planar harness.
#[allow(clippy::too_many_arguments)]
fn windowed_drive_session<'a>(
    compiled_pose: &'a SessionCompiledPose,
    trig: &'a [i32],
    wheel_inputs: &Result<SessionWheelInputs, SessionSurfaceInputsError>,
    tyre_tables: &'a Result<SessionTyreTables, SessionTyreTablesError>,
    aux: &Result<SessionAuxControl, String>,
    params: SimParams,
    mut start: CarState,
    event: EventState,
) -> GameplaySession<'a> {
    let four_wheel = match (wheel_inputs, tyre_tables, aux) {
        (Ok(inputs), Ok(tables), Ok(aux)) => compiled_pose
            .compiled_four_wheel_params(inputs, trig, tables.tables(), aux)
            .map_err(|error| error.to_string()),
        (Err(error), _, _) => Err(error.to_string()),
        (_, Err(error), _) => Err(error.to_string()),
        (_, _, Err(error)) => Err(error.clone()),
    };
    match four_wheel {
        Ok(wheel) => {
            start.four_wheel.gearbox = cmr_core::gearbox::GearboxState::at_setup(true);
            GameplaySession::new_with_four_wheel_and_car(
                params,
                InRaceScript::gameplay_harness(),
                wheel,
                start,
            )
            .with_ground_geometry(&compiled_pose.geometry)
            .with_event(event)
        }
        Err(error) => {
            eprintln!("cmr-app: four-wheel inputs unavailable ({error}); using the planar harness");
            GameplaySession::new_with_car(params, InRaceScript::gameplay_harness(), start)
                .with_event(event)
        }
    }
}

/// The stage's `.dat` records and the main-C3D cell centres the trigger walk
/// selects the car's cell from; empty when the stage has no static scene.
fn stage_trigger_inputs(
    resources: &StageRenderResources,
) -> (Vec<cmr_formats::stage::TriggerVolume>, Vec<Vec3>) {
    match resources.static_collision_scene() {
        Some(scene) => (
            resources
                .bundle
                .triggers()
                .map(<[_]>::to_vec)
                .unwrap_or_default(),
            scene.main_cell_centers.clone(),
        ),
        None => (Vec::new(), Vec::new()),
    }
}

pub fn run_windowed_game_session(
    media_root: &Path,
    runtime_data: &Path,
    stage: &str,
    car_code: &str,
    renderer: StageRendererChoice,
) -> Result<(), WindowedStageFrameError> {
    let (mut resources, nodes, mut start, _, actor_rows) =
        native_stage_setup(media_root, runtime_data, stage, Some(car_code), renderer)?;
    // This owner models one fresh process/session: its lane arrays are
    // loader-zeroed once, authenticated setup runs once, and only the recovered
    // setup-time center-Y, half state, AK11 normal, and corrected basis are fed
    // into the existing session. Deformation scalars are retained as an exact
    // setup result, but live collision does not consume or evolve them yet.
    let mut compiled_pose = SessionCompiledPose::open(runtime_data, stage, car_code)
        .map_err(WindowedStageFrameError::CompiledPose)?;
    let _compiled_deformation_scalars = compiled_pose
        .adjust_spawn(&mut start, actor_rows)
        .map_err(WindowedStageFrameError::CompiledPose)?;
    let mut initial_view = ViewHarness::new();
    let initial_camera = initial_view.update(false, start.position, start.heading, false);
    eprintln!("cmr-app: {}", resources.renderer_status().stable_summary());
    let initial_harness_world = cmr_core::gameplay_camera::car_world_q16(
        &cmr_core::fixed_point::build_sine_table(),
        start.position,
        start.heading,
    );
    let initial_car_world = car_model_world_q16(&initial_harness_world);
    let initial_world = resources
        .compose_with_car(&initial_camera, Some(&initial_car_world))?
        .frame;

    let params = SimParams {
        inverse_mass: Fixed16::from_raw(0x8000),
        gravity: Vec3::ZERO,
    };
    let trig = cmr_core::fixed_point::build_sine_table();
    let wheel_inputs = SessionWheelInputs::open(runtime_data);
    let tyre_tables = SessionTyreTables::open(runtime_data);
    let aux = SessionAuxControl::open(runtime_data);
    let (trigger_volumes, trigger_cell_centers) = stage_trigger_inputs(&resources);
    let mut session = windowed_drive_session(
        &compiled_pose,
        &trig,
        &wheel_inputs,
        &tyre_tables,
        &aux,
        params,
        start,
        single_stage_event(stage),
    )
    .with_stage_triggers(&trigger_volumes, &trigger_cell_centers)
    .map_err(WindowedStageFrameError::StageTriggers)?;
    let mut keyboard = KeyboardDriver::player_one();
    let mut analog = AnalogDriver::new();
    let response = DriveResponse {
        max_forward_force: Fixed16::from_raw(GAME_SESSION_DRIVE_FORCE_Q16),
        max_steer_rate: GAME_SESSION_MAX_STEER_RATE,
    };
    let mut progress = StageProgress::new(&nodes, 0);
    let initial = compose_live_race_hud(
        &resources,
        initial_world,
        live_race_hud_inputs(start, None, &progress, nodes.len(), Fixed16::ZERO),
    )?;
    let clock = WallClock::new();
    let mut root_interaction =
        RootInteraction::from_saved_selection(0, Language::English, ROOT_EXTENT, clock.millis())
            .expect("zero is a valid recovered root selection");
    let mut camera = ViewHarness::new();
    let mut fade: Option<ScalarTween> = None;
    let mut damage_owner = NoStaticDamageOwner;
    let srf = read_decoded_section(media_root, stage, ".srf", DecodeLimits::default())
        .ok()
        .flatten();
    let mut audio = RaceAudio {
        engine: EngineAudio::open(runtime_data, compiled_pose.selector as u8),
        effects: EffectsAudio::open(runtime_data, srf, stage_crowd_points(&resources)),
    };
    let mut pause = PauseHarness::with_runtime_data(runtime_data);
    let mut runtime_error = None;

    // Best-effort: open the recovered front-end root-menu compositor from the
    // runtime-data root so the interactive front-end route (state 2) presents the
    // real recovered menu frame instead of a cleared one. If the front-end assets
    // are unavailable or the session cannot be built, the front-end route falls
    // back to a cleared frame and the world render path is unaffected.
    let front_end = ReviewedProfile::from_json(REVIEWED_PROFILE)
        .ok()
        .and_then(|reviewed| open_root_session_inputs(runtime_data, &reviewed).ok());

    // Every game wave path (`\new_fe\sounds\...`, `game\sounds\...`) is
    // relative to the installation root, which the imported runtime data
    // mirrors; `media_root` is the stage directory, so it cannot root them.
    let audio_root = runtime_data;
    if let Some((store, startup)) = front_end {
        let request = SoftwareRootMenuSessionRequest::official_eu_initial_software_default_config(
            Language::English,
            VisualVariant::Normal,
        );
        let built = with_software_root_menu_session(&store, startup, request, |root| {
            run_windowed_game_session_loop(&initial, Some(audio_root), |input: WindowedInput| {
                match advance_game_frame(
                    &mut session,
                    &mut keyboard,
                    &mut analog,
                    &response,
                    &mut root_interaction,
                    &mut progress,
                    &clock,
                    &mut resources,
                    &mut camera,
                    &mut fade,
                    &mut damage_owner,
                    &mut audio,
                    &mut pause,
                    input,
                ) {
                    Ok(outcome) => {
                        // A composed world frame wins; otherwise the interactive
                        // front-end route presents the recovered root-menu frame.
                        let present = if outcome.world.is_some() {
                            outcome.world
                        } else if outcome.loading_interstitial.is_some() {
                            outcome.loading_interstitial
                        } else if let Some(render_input) = outcome.root_render_input {
                            root.render(render_input)
                                .ok()
                                .and_then(|_| root.backend().presented_frame().cloned())
                        } else {
                            None
                        };
                        WindowedGameFrame {
                            present,
                            stopped: outcome.exit_requested,
                            music_active: outcome.music_active,
                            codriver_calls: outcome.codriver_calls,
                        }
                    }
                    Err(error) => {
                        runtime_error = Some(error);
                        WindowedGameFrame {
                            present: None,
                            stopped: true,
                            music_active: false,
                            codriver_calls: Vec::new(),
                        }
                    }
                }
            })
        });
        if let Ok(run) = built {
            run.output.map_err(WindowedStageFrameError::Present)?;
            if let Some(error) = runtime_error {
                return Err(error);
            }
            return Ok(());
        }
        // The front-end session could not be built after its inputs opened; fall
        // through to the world-only loop below rather than failing the session.
    }

    run_windowed_game_session_loop(&initial, Some(audio_root), |input: WindowedInput| {
        match advance_game_frame(
            &mut session,
            &mut keyboard,
            &mut analog,
            &response,
            &mut root_interaction,
            &mut progress,
            &clock,
            &mut resources,
            &mut camera,
            &mut fade,
            &mut damage_owner,
            &mut audio,
            &mut pause,
            input,
        ) {
            Ok(outcome) => WindowedGameFrame {
                present: outcome.world.or(outcome.loading_interstitial),
                stopped: outcome.exit_requested,
                music_active: outcome.music_active,
                codriver_calls: outcome.codriver_calls,
            },
            Err(error) => {
                runtime_error = Some(error);
                WindowedGameFrame {
                    present: None,
                    stopped: true,
                    music_active: false,
                    codriver_calls: Vec::new(),
                }
            }
        }
    })
    .map_err(WindowedStageFrameError::Present)?;
    if let Some(error) = runtime_error {
        return Err(error);
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
const GAME_SESSION_DRIVE_FORCE_Q16: i32 = 0x8000;
/// The heading rate (signed 16-bit BAM per tick) at full steer for the windowed
/// session's live-input drive. A **harness constant**, not a recovered value:
/// the shipped steer authority is a runtime car/surface-table quantity
/// (`rea.official-eu.drivetrain-actuator-chain`).
const GAME_SESSION_MAX_STEER_RATE: i16 = 8;
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
/// The session is a **rally-school** session so the recovered graph
/// terminates cleanly through the real tables: the pre-race band routes
/// `1 -> 2 -> 3 -> 4 -> 7` (the school menu confirmed, `load_arm` -> entry
/// `7`), then the gameplay band runs `7 -> 5 -> 9 -> 0xb -> 0xc`, where the
/// hub ends the session and hands back to the front end. In state `5`
/// (the main driving tick) the deterministic race sim steps one fixed tick per
/// frame under a stationary clock and feeds the car's world position to a
/// [`StageProgress`] walking the synthetic [`headless_checkpoint_line`]; when
/// the tracker crosses the final gate it raises `stage_finished`, and the
/// recovered [`drive_next`](cmr_core::game_race_flow::drive_next) table hands off
/// to the finish-flyby (`9`). Every in-race transition is
/// [`race_flow_next`], not an ad-hoc choice; the drive force, the synthetic
/// checkpoint line, and the scripted school sequencer inputs (the replay/
/// fade/quit flags) are documented **harness** stand-ins for the real subsystems.
pub fn run_headless_game_session() -> HeadlessGameSessionOutput {
    let params = SimParams {
        inverse_mass: Fixed16::ONE,
        gravity: Vec3::ZERO,
    };
    let mut session = GameplaySession::new(params, InRaceScript::school_harness());
    let controls = Controls {
        steer_rate: 0,
        forward_force: Fixed16::from_raw(GAME_SESSION_DRIVE_FORCE_Q16),
        handbrake: false,
        raw_steer: Default::default(),
        raw_pedals: Default::default(),
        raw_gear_command_314: 0,
    };
    drive_headless_session(&mut session, controls).expect("the simplified session is infallible")
}

fn drive_headless_session(
    session: &mut GameplaySession<'_>,
    controls: Controls,
) -> Result<HeadlessGameSessionOutput, WheelPostprocessError> {
    let clock = StationaryClock;
    let menu_inputs = MenuFlowInputs {
        school_menu_done: true,
        ..MenuFlowInputs::default()
    };
    let nodes = headless_checkpoint_line();
    let mut progress = StageProgress::new(&nodes, 0);
    let mut frames = 0u32;
    while session.event().in_session() && frames < GAME_SESSION_MAX_FRAMES {
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
    CompiledCar(SessionCompiledPoseError),
    AuxControl(String),
}

impl fmt::Display for SessionFourWheelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Inputs(error) => write!(formatter, "session wheel inputs failed: {error}"),
            Self::Surface(error) => write!(formatter, "invalid wheel surface index: {error:?}"),
            Self::Producer(error) => write!(formatter, "four-wheel producer failed: {error:?}"),
            Self::CompiledCar(error) => write!(formatter, "compiled car inputs failed: {error}"),
            Self::AuxControl(error) => write!(formatter, "aux-control patch failed: {error}"),
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

/// Harness car constants for the headless four-wheel session, which has no
/// compiled case words: unit mass and inertia, the old planar inverse mass,
/// and inert pedal ramps. A session with authenticated case words derives
/// these with `derive_compiled_four_wheel_constants` instead.
const HEADLESS_HARNESS_FOUR_WHEEL_CONSTANTS: CompiledFourWheelConstants =
    CompiledFourWheelConstants {
        state_word_48: 0x1_0000,
        state_word_4c: 0x8000,
        inertia_50_54_58: [0x1_0000; 3],
        control_word_148: 0,
        control_word_16c: 0,
        control_word_158: 1,
        control_word_150: 0,
        control_word_174: 0,
        control_word_17c: 0,
        control_word_184: 0,
        control_word_408: 0,
        control_word_188: 0,
        control_u16_3e4: 0,
        control_byte_440: 0,
        control_word_15c: 0x1_0000,
        gravity_c5fc: 0,
        control_word_420: 1,
    };

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
    let wheel = inputs.four_wheel_params(
        &trig,
        families,
        half_state,
        Fixed16::ONE,
        &HEADLESS_HARNESS_FOUR_WHEEL_CONSTANTS,
        [0; 4],
    )?;
    let params = SimParams {
        inverse_mass: Fixed16::from_raw(0x8000),
        gravity: Vec3::ZERO,
    };
    let mut start = CarState::at_rest();
    start.motion.angular_velocity = Vec3::new(Fixed16::ONE, Fixed16::ONE, Fixed16::ONE);
    // A harness grounded start in the game's y-up frame: four contacting
    // lanes, an up normal and the identity basis (rolling along +x), so the
    // producer runs its grounded body and the integrator head sees level
    // ground; an at-rest car with no contacts is airborne.
    start.motion.contact_distance_flags = [true; 4];
    start.motion.contact_count_674 = 4;
    start.motion.basis = [
        Vec3::new(Fixed16::ONE, Fixed16::ZERO, Fixed16::ZERO),
        Vec3::new(Fixed16::ZERO, Fixed16::ONE, Fixed16::ZERO),
        Vec3::new(Fixed16::ZERO, Fixed16::ZERO, Fixed16::ONE),
    ];
    start.motion.projection_axis = Vec3::new(Fixed16::ZERO, Fixed16::ONE, Fixed16::ZERO);
    let mut session = GameplaySession::new_with_four_wheel_and_car(
        params,
        InRaceScript::school_harness(),
        wheel,
        start,
    );
    let controls = Controls {
        steer_rate: 0,
        forward_force: Fixed16::ONE,
        handbrake: false,
        raw_steer: Default::default(),
        raw_pedals: Default::default(),
        raw_gear_command_314: 0,
    };
    Ok(drive_headless_session(&mut session, controls)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loading_bitmap(width: u32, height: u32, palette_entries: usize) -> Vec<u8> {
        let row_stride = width.div_ceil(4) * 4;
        let pixels_len = (row_stride * height) as usize;
        let pixel_offset = 14 + 40 + palette_entries * 4;
        let file_size = pixel_offset + pixels_len;
        let mut bytes = vec![0; file_size];
        bytes[0..2].copy_from_slice(b"BM");
        bytes[2..6].copy_from_slice(&(file_size as u32).to_le_bytes());
        bytes[10..14].copy_from_slice(&(pixel_offset as u32).to_le_bytes());
        bytes[14..18].copy_from_slice(&40u32.to_le_bytes());
        bytes[18..22].copy_from_slice(&(width as i32).to_le_bytes());
        bytes[22..26].copy_from_slice(&(height as i32).to_le_bytes());
        bytes[26..28].copy_from_slice(&1u16.to_le_bytes());
        bytes[28..30].copy_from_slice(&8u16.to_le_bytes());
        bytes[34..38].copy_from_slice(&(pixels_len as u32).to_le_bytes());
        bytes[46..50].copy_from_slice(&(palette_entries as u32).to_le_bytes());
        for index in 0..palette_entries {
            let start = 54 + index * 4;
            bytes[start..start + 4].copy_from_slice(&[
                index as u8,
                (index as u8).wrapping_add(1),
                (index as u8).wrapping_add(2),
                0,
            ]);
        }
        for y in 0..height as usize {
            let row = pixel_offset + y * row_stride as usize;
            bytes[row..row + width as usize].fill((y % palette_entries) as u8);
        }
        bytes
    }

    fn gzip_cmpr(payload: &[u8]) -> Vec<u8> {
        use std::io::Write;

        use flate2::Compression;
        use flate2::write::GzEncoder;

        let mut envelope = b"CMPR".to_vec();
        envelope.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        envelope.extend_from_slice(payload);
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&envelope).unwrap();
        encoder.finish().unwrap()
    }

    fn arguments(values: &[&str]) -> impl Iterator<Item = OsString> {
        values.iter().map(OsString::from)
    }

    #[test]
    fn fresh_process_lane_state_is_zeroed_once_and_only_handles_reset_per_setup() {
        let mut owner = FreshProcessLaneState::new();
        assert_eq!(
            owner.begin_car_setup(),
            CompiledEightLaneExecutionState {
                handles: [0; 8],
                type_words: [0; 8],
                output_vectors: [[0; 3]; 8],
                lane_words: [0; 8],
                accumulator_words: [0; 8],
                latch_67c: 0,
            }
        );

        owner.publish(CompiledEightLaneExecutionState {
            handles: [7; 8],
            type_words: [0x52; 8],
            output_vectors: [[1, 2, 3]; 8],
            lane_words: [4; 8],
            accumulator_words: [5; 8],
            latch_67c: 1,
        });
        owner.gate_words = [6; 8];
        owner.drive_words = [7; 8];
        owner.first_four_subtractions = [8; 4];
        let repeated = owner.begin_car_setup();
        assert_eq!(repeated.handles, [0; 8]);
        assert_eq!(repeated.type_words, [0x52; 8]);
        assert_eq!(repeated.output_vectors, [[1, 2, 3]; 8]);
        assert_eq!(repeated.lane_words, [4; 8]);
        assert_eq!(repeated.accumulator_words, [5; 8]);
        assert_eq!(repeated.latch_67c, 1);
        assert_eq!(
            owner.inputs(),
            CompiledSetupPoseLaneInputs {
                gate_words: [6; 8],
                drive_words: [7; 8],
                first_four_subtractions: [8; 4],
            }
        );
    }

    #[test]
    fn compiled_pose_publication_updates_motion_and_retains_deformation_scalars() {
        let mut start = CarState::at_rest();
        start.position.y = Fixed16::from_raw(0x1234);
        let basis = CompiledPostLaneBasisEffects {
            vector_258: [0x10000, 0, 0],
            vector_264: [0, 0x10000, 0],
            vector_270: [0, 0, 0x10000],
        };

        let deformation = publish_session_compiled_pose(
            &mut start,
            [8, -10, 12, 13, -14, 15],
            -0x2345,
            basis.vector_264,
            &basis,
            [0, 0xcccc, 0x20000, -1, 0, 0, 0, 0],
            [
                [0, 0, 0],
                [0, 1, 0],
                [0, 0x20000 - 0xcccb, 0],
                [0, i32::MAX, 0],
                [0; 3],
                [0; 3],
                [0; 3],
                [0; 3],
            ],
        );

        assert_eq!(
            start.motion.compiled_half_words,
            Vec3::new(
                Fixed16::from_raw(4),
                Fixed16::from_raw(-5),
                Fixed16::from_raw(6),
            )
        );
        assert_eq!(start.motion.projection_axis, start.motion.basis[1]);
        assert_eq!(
            start.motion.basis,
            [
                Vec3::new(Fixed16::ONE, Fixed16::ZERO, Fixed16::ZERO),
                Vec3::new(Fixed16::ZERO, Fixed16::ONE, Fixed16::ZERO),
                Vec3::new(Fixed16::ZERO, Fixed16::ZERO, Fixed16::ONE),
            ]
        );
        assert_eq!(start.position.y, Fixed16::from_raw(-0x2345));
        assert_eq!(
            start.motion.contact_distance_flags,
            [true, true, true, true]
        );
        assert_eq!(start.motion.contact_count_674, 4);
        assert_eq!(
            deformation,
            cmr_core::live_static_collision_car_frame::UpperHullDeformationScalars {
                scale_638: Fixed16::from_raw(13),
                scale_63c: Fixed16::from_raw(-14),
                scale_640: Fixed16::from_raw(15),
            }
        );
        assert_eq!(start.motion.upper_hull_deformation, deformation);
    }

    #[test]
    fn compiled_pose_failures_retain_stable_phase_and_lane_metadata() {
        let common = SessionCompiledPoseError::Execution(CompiledSetupPoseError::CommonPrefix(
            cmr_core::compiled_car_initializer::CompiledCommonPrefixReciprocalOverflow {
                state_word_48: 1,
            },
        ));
        assert_eq!(common.kind(), "compiled-pose-common-prefix");
        assert_eq!(common.failed_lane(), None);
        assert!(common.to_string().contains("state +0x48 value 1"));
        assert!(std::error::Error::source(&common).is_some());

        let lane =
            CompiledSetupPoseError::<(), ()>::Lanes(CompiledEightLaneExecutionFailure::Dispatch {
                failed_lane: 3,
                progress: Box::new(
                    cmr_core::compiled_car_initializer::CompiledEightLaneExecutionProgress {
                        completed_lanes: 3,
                        state: FreshProcessLaneState::new().begin_car_setup(),
                        lanes: [None; 8],
                    },
                ),
                failure: cmr_core::compiled_car_initializer::CompiledLaneDispatchFailure {
                    phase: CompiledLaneDispatchFailurePhase::Search,
                    handle_writes:
                        cmr_core::compiled_car_initializer::CompiledLaneHandleWriteTrace::default(),
                    provider_error: (),
                },
            });
        assert_eq!(
            compiled_setup_failure_metadata(&lane),
            ("compiled-pose-lane-search", Some(3))
        );
    }

    #[test]
    fn ordinary_stage_geometry_identity_is_explicit_and_path_relative() {
        assert_eq!(
            ordinary_stage_asset_base("au1").as_deref(),
            Some("Game/Au/1/AU1")
        );
        assert_eq!(
            ordinary_stage_asset_base("NZ8").as_deref(),
            Some("Game/NZ/8/NZ8")
        );
        assert_eq!(ordinary_stage_asset_base("TASK1"), None);
        assert_eq!(ordinary_stage_asset_base("../AU1"), None);
    }

    #[test]
    fn session_sidecar_store_supports_canonical_sibling_and_nested_layouts() {
        let root = std::env::temp_dir().join(format!(
            "cmr-app-sidecar-layout-{}-{}",
            std::process::id(),
            NEXT_SURFACE_TEMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let temp = SurfaceTemp(root);
        let runtime_data = temp.0.join("installed-eu-derived-assets-v2");
        let sibling = temp.0.join("official-eu-car-selectors");
        std::fs::create_dir_all(&runtime_data).unwrap();
        std::fs::create_dir_all(&sibling).unwrap();

        assert_eq!(
            session_sidecar_store(&runtime_data, "official-eu-car-selectors"),
            sibling
        );

        let nested = runtime_data.join("official-eu-car-selectors");
        std::fs::create_dir_all(&nested).unwrap();
        assert_eq!(
            session_sidecar_store(&runtime_data, "official-eu-car-selectors"),
            nested
        );
    }

    #[test]
    fn selected_stage_loading_bitmap_becomes_a_full_palette_frame() {
        let frame = parse_loading_background(&loading_bitmap(640, 480, 256)).unwrap();
        assert_eq!(frame.extent(), ROOT_EXTENT);
        assert_eq!(frame.pixels().len(), 640 * 480);
        // Source rows are bottom-up; the parser normalizes them for presentation.
        assert_eq!(frame.pixels()[0], 479u16 as u8);
        assert_eq!(frame.pixels()[frame.pixels().len() - 1], 0);
        assert_eq!(frame.palette()[7].red, 9);
        assert_eq!(frame.palette()[7].green, 8);
        assert_eq!(frame.palette()[7].blue, 7);

        assert!(matches!(
            parse_loading_background(&loading_bitmap(320, 200, 256)),
            Err(StageFrameError::LoadingExtent {
                width: 320,
                height: 200
            })
        ));
        assert!(matches!(
            parse_loading_background(&loading_bitmap(640, 480, 16)),
            Err(StageFrameError::LoadingPalette { entries: 16 })
        ));
        assert!(matches!(
            parse_loading_background(b"not a bitmap"),
            Err(StageFrameError::LoadingBitmap(_))
        ));
    }

    #[test]
    fn loading_lookup_accepts_lowercase_wrapped_media_and_missing_is_optional() {
        let root = std::env::temp_dir().join(format!(
            "cmr-app-loading-background-{}-{}",
            std::process::id(),
            NEXT_SURFACE_TEMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let temp = SurfaceTemp(root);
        std::fs::create_dir_all(&temp.0).unwrap();
        let bitmap = loading_bitmap(640, 480, 256);
        std::fs::write(temp.0.join("au1.loa"), gzip_cmpr(&bitmap)).unwrap();

        let decoded = read_loading_background(&temp.0, "AU1", DecodeLimits::default())
            .unwrap()
            .unwrap();
        assert_eq!(decoded, bitmap);
        assert!(
            read_loading_background(&temp.0, "NZ2", DecodeLimits::default())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn race_hud_asset_paths_and_parse_failures_retain_source_identity() {
        assert_eq!(
            race_hud_store_path(RaceHudBitmap::CommonGauge640.relative_path()),
            "Game/images/osdnew/revs/640x480.bmp"
        );
        let bitmap = parse_race_hud_bitmap(RaceHudBitmap::ProgressRedSpot, b"bad").unwrap_err();
        assert_eq!(bitmap.kind(), "hud-bitmap");
        assert!(matches!(
            bitmap,
            StageFrameError::HudBitmap {
                bitmap: RaceHudBitmap::ProgressRedSpot,
                error: BitmapError::Truncated,
            }
        ));

        let font = parse_race_hud_font(RaceHudFont::TimeNumbers640, b"bad").unwrap_err();
        assert_eq!(font.kind(), "hud-font");
        assert!(matches!(
            font,
            StageFrameError::HudFont {
                font: RaceHudFont::TimeNumbers640,
                ..
            }
        ));
    }

    #[test]
    fn idle_gamepad_does_not_mask_keyboard_driving_input() {
        assert!(!analog_driving_is_active(&GamepadDrivingState::new()));
    }

    #[test]
    fn the_rally_entry_starts_a_race_session_that_seeds_the_fsm() {
        let mut root =
            RootInteraction::from_saved_selection(0, Language::English, ROOT_EXTENT, 0).unwrap();
        let mut game = cmr_core::game_session::GameSession::new();
        let next = cmr_core::frontend::FrontendInput {
            next: true,
            ..Default::default()
        };
        let confirm = cmr_core::frontend::FrontendInput {
            confirm: true,
            ..Default::default()
        };
        let mut pulses = vec![Default::default(), Default::default(), next, confirm];
        pulses.resize(14, Default::default());
        let mut transcript = Vec::new();
        let mut gameplay_calls = 0;
        let mut front_end_frames = 0;
        for (now, pulse) in pulses.into_iter().enumerate() {
            if !game.event().in_session() {
                front_end_frames += 1;
                let tick = root.tick_for_game_session(pulse, now as u32 + 1, true);
                if tick
                    .effects
                    .contains(&cmr_core::frontend::FrontendEffect::StartRace)
                {
                    game.event_mut().commit_unconditional();
                    game.event_mut().start_race_session();
                }
            }
            let frame = game.run_frame(&StationaryClock, PRE_RACE_INPUTS, |state, _| {
                gameplay_calls += 1;
                state
            });
            if let Some(plan) = frame.plan {
                transcript.push((
                    plan.render.map(cmr_core::game_state::GameState::id),
                    plan.state.id(),
                ));
            }
            if game.current().id() == 5 {
                break;
            }
        }
        assert!(front_end_frames > 0);
        assert_eq!(game.current().id(), 5);
        // The seed frame renders state 1; the championship's first stage
        // shows its welcome screen (state 2) before loading.
        assert_eq!(transcript[0], (Some(1), 2), "{transcript:?}");
        assert_eq!(
            transcript
                .iter()
                .filter(|entry| **entry == (Some(4), 5))
                .count(),
            1,
            "state 4 must render exactly once while transitioning to state 5: {transcript:?}"
        );
        assert_eq!(gameplay_calls, 0, "state 4->5 must not step the race sim: {transcript:?}");
    }

    fn native_test_node(position: [i32; 3], direction: [i32; 3], split_flag: u8) -> CatNode {
        CatNode {
            position,
            direction,
            split_flag,
            reserved: 0,
            seq_index: 0,
        }
    }

    #[test]
    fn native_spawn_uses_node_position_heading_basis_and_first_camera() {
        let nodes = [native_test_node(
            [10 << 16, 0, 20 << 16],
            [0, 0, 0x10000],
            1,
        )];
        let (start, camera, actor_rows) = native_stage_start(&nodes).unwrap();
        assert_eq!(start.position.x.raw(), 10 << 16);
        assert_eq!(start.position.z.raw(), 20 << 16);
        assert_eq!(start.heading, 0x400);
        assert_eq!(start.motion.basis[2].z.raw(), 0x10000);
        // Setup row 0 is the harness forward (the native rolling direction).
        assert_eq!(
            actor_rows,
            [
                [
                    start.motion.basis[2].x.raw(),
                    start.motion.basis[2].y.raw(),
                    start.motion.basis[2].z.raw(),
                ],
                [
                    start.motion.basis[1].x.raw(),
                    start.motion.basis[1].y.raw(),
                    start.motion.basis[1].z.raw(),
                ],
                [
                    start.motion.basis[0].x.raw().wrapping_neg(),
                    start.motion.basis[0].y.raw().wrapping_neg(),
                    start.motion.basis[0].z.raw().wrapping_neg(),
                ],
            ]
        );
        assert_eq!(start.motion.angular_velocity, Vec3::ZERO);
        assert_ne!(camera, stage_placeholder_camera());
        assert_ne!(&camera[12..15], &[0, 0, 0]);
    }

    #[test]
    fn csp_spawn_uses_authored_position_and_yaw() {
        let (start, camera, actor_rows) = native_stage_start_from_csp(CspStart {
            position: [10 << 16, 2 << 16, -4 << 16],
            rotation_bam: [0, 0x400, 0],
            reserved: 0xbeef,
        });
        assert_eq!(start.position.x.raw(), 10 << 16);
        assert_eq!(start.position.y.raw(), 2 << 16);
        assert_eq!(start.position.z.raw(), -4 << 16);
        assert_eq!(start.heading, 0xc00);
        assert_eq!(start.motion.projection_axis, start.motion.basis[2]);
        assert!(start.motion.basis[2].z.raw() < -0xff00);
        assert_ne!(camera, stage_placeholder_camera());
        let native = actor_rows.map(|row| {
            Vec3::new(
                Fixed16::from_raw(row[0]),
                Fixed16::from_raw(row[1]),
                Fixed16::from_raw(row[2]),
            )
        });
        // The setup rows are the harness basis with row 0 forward: the
        // integrated CSP basis turned half a turn about the up axis.
        let negated = |vector: Vec3| {
            Vec3::new(
                Fixed16::from_raw(vector.x.raw().wrapping_neg()),
                Fixed16::from_raw(vector.y.raw().wrapping_neg()),
                Fixed16::from_raw(vector.z.raw().wrapping_neg()),
            )
        };
        assert_eq!(native[0], start.motion.basis[2]);
        assert_eq!(native[1], start.motion.basis[1]);
        assert_eq!(native[2], negated(start.motion.basis[0]));
        assert_ne!(
            actor_rows,
            start
                .motion
                .basis
                .map(|vector| [vector.x.raw(), vector.y.raw(), vector.z.raw()])
        );
    }

    #[test]
    fn race_car_local_positive_x_end_faces_away_from_the_chase_camera() {
        use cmr_core::c3d_camera::{apply_affine_q16, invert_affine_q16};
        use cmr_core::gameplay_camera::{ChasePreset, chase_target_world};

        let trig = cmr_core::fixed_point::build_sine_table();
        let harness = cmr_core::gameplay_camera::car_world_q16(
            &trig,
            Vec3::new(
                Fixed16::from_int(1),
                Fixed16::from_int(2),
                Fixed16::from_int(3),
            ),
            0,
        );
        let model = car_model_world_q16(&harness);
        // Pin only the observed transform: local +X maps to harness forward and
        // local -Z maps to harness right. No wheel-node role is inferred here.
        assert_eq!(&model[0..3], &[0x10000, 0, 0]);
        assert_eq!(&model[8..11], &[0, 0, 0x10000]);
        assert_eq!(&model[12..15], &[0x10000, 0x20000, 0x30000]);

        let determinant = i64::from(model[0])
            * (i64::from(model[5]) * i64::from(model[10])
                - i64::from(model[9]) * i64::from(model[6]))
            - i64::from(model[4])
                * (i64::from(model[1]) * i64::from(model[10])
                    - i64::from(model[9]) * i64::from(model[2]))
            + i64::from(model[8])
                * (i64::from(model[1]) * i64::from(model[6])
                    - i64::from(model[5]) * i64::from(model[2]));
        assert!(determinant > 0, "the model adapter must not mirror winding");

        // The chase camera is behind the car. Under the observed mapping, the
        // local -X endpoint is nearer and local +X is farther along view Z.
        let camera = chase_target_world(&harness, ChasePreset::Near.offset());
        let view = invert_affine_q16(&camera);
        let positive_x_world = apply_affine_q16(&model, [2 << 16, 0, 0]);
        let negative_x_world = apply_affine_q16(&model, [-2 << 16, 0, 0]);
        let positive_x_view = apply_affine_q16(&view, positive_x_world);
        let negative_x_view = apply_affine_q16(&view, negative_x_world);
        assert!(negative_x_view[2] > 0);
        assert!(negative_x_view[2] < positive_x_view[2]);

        let harness_forward = [harness[8], harness[9], harness[10]];
        let model_forward = [model[0], model[1], model[2]];
        let forward_dot: i64 = model_forward
            .into_iter()
            .zip(harness_forward)
            .map(|(a, b)| i64::from(a) * i64::from(b))
            .sum();
        assert!(forward_dot > 0);
    }

    #[test]
    fn horizon_root_tracks_camera_position_with_identity_basis_and_ars_height() {
        let mut camera = MATRIX_IDENTITY_Q16;
        camera[0] = 0;
        camera[2] = -0x10000;
        camera[8] = 0x10000;
        camera[10] = 0;
        camera[12] = 0x120000;
        camera[13] = -0x30000;
        camera[14] = 0x450000;

        let root = horizon_root_world(&camera, 0x8000);
        assert_eq!(&root[..12], &MATRIX_IDENTITY_Q16[..12]);
        assert_eq!(&root[12..15], &[0x120000, -0x28000, 0x450000]);
        assert_eq!(root[15], 0x10000);
    }

    #[test]
    fn native_spawn_covers_cardinal_headings_and_rejects_bad_inputs() {
        for (direction, expected) in [
            ([0x10000, 0, 0], 0),
            ([0, 0, 0x10000], 0x400),
            ([-0x10000, 0, 0], 0x800),
            ([0, 0, -0x10000], 0xc00),
        ] {
            let (start, _, _) = native_stage_start(&[native_test_node([0; 3], direction, 0)])
                .expect("cardinal spawn");
            assert_eq!(start.heading, expected);
        }
        assert!(matches!(
            native_stage_start(&[]),
            Err(StageFrameError::MissingNodes)
        ));
        assert!(matches!(
            native_stage_start(&[native_test_node([0; 3], [0, 0x10000, 0], 0)]),
            Err(StageFrameError::DegenerateSpawn)
        ));
    }

    #[test]
    fn native_palette_mapping_is_explicit_and_fail_closed() {
        for (stage, country) in [
            ("AU1", "aus"),
            ("CO7", "cor"),
            ("GR4", "gre"),
            ("IN8", "ind"),
            ("MC5", "mon"),
            ("NZ2", "nz"),
            ("SW4", "swe"),
            ("UK2", "uk"),
            ("SC1", "school"),
        ] {
            assert_eq!(
                native_stage_palette_path(stage).unwrap(),
                format!("Game/Palettes/Countries/{country}.pal")
            );
        }
        assert!(matches!(
            native_stage_palette_path("XX1"),
            Err(StageFrameError::UnknownPaletteStage(_))
        ));
    }

    #[test]
    fn world_route_surfaces_composition_failures_instead_of_dropping_the_frame() {
        let error = compose_world_for_route(
            Some(RenderRoute::StageWorld(cmr_core::render_route::StageView {
                status_banner: true,
                consume_overlay_dirty: true,
                resource_prestage: false,
            })),
            || Err(StageFrameError::DegenerateSpawn),
        )
        .unwrap_err();
        assert!(matches!(error, StageFrameError::DegenerateSpawn));

        let skipped = compose_world_for_route(None, || {
            panic!("non-world routes must not compose a stage frame")
        })
        .unwrap();
        assert!(skipped.is_none());
    }

    #[test]
    fn loading_overlay_is_route_gated_and_missing_inputs_have_native_boundaries() {
        let background = parse_loading_background(&loading_bitmap(640, 480, 256)).unwrap();
        let overlay = LoadingOverlayAssets {
            bar: parse_indexed_bitmap(&loading_bitmap(267, 13, 256), BitmapLimits::default())
                .unwrap(),
            line: parse_indexed_bitmap(&loading_bitmap(8, 8, 256), BitmapLimits::default())
                .unwrap(),
        };
        let composed = loading_interstitial_for_route(
            Some(RenderRoute::LoadInterstitial),
            Some(&background),
            Some(&overlay),
            STAGE_LOADING_PROGRESS_CAP,
        )
        .unwrap()
        .unwrap();
        assert_ne!(composed.pixels(), background.pixels());

        // Non-load routes never consult loading assets.
        assert!(
            loading_interstitial_for_route(None, Some(&background), None, 255)
                .unwrap()
                .is_none()
        );
        // The native failed-background path never arms BAR/LINE drawing.
        assert!(
            loading_interstitial_for_route(
                Some(RenderRoute::LoadInterstitial),
                None,
                None,
                STAGE_LOADING_PROGRESS_CAP,
            )
            .unwrap()
            .is_none()
        );
        // A present background without its authenticated overlay sources fails
        // before publishing a partially composed frame.
        assert!(matches!(
            loading_interstitial_for_route(
                Some(RenderRoute::LoadInterstitial),
                Some(&background),
                None,
                STAGE_LOADING_PROGRESS_CAP,
            ),
            Err(StageFrameError::MissingLoadingOverlayAssets)
        ));
    }

    #[test]
    fn loading_overlay_paths_and_malformed_sources_retain_identity() {
        assert_eq!(
            LoadingOverlayBitmap::Bar.legacy_relative_path(),
            LOADING_BAR_BMP
        );
        assert_eq!(
            LoadingOverlayBitmap::Line.legacy_relative_path(),
            LOADING_LINE_BMP
        );
        assert_eq!(
            LoadingOverlayBitmap::Bar.store_path(),
            "Game/IMAGES/LOADING/BAR.BMP"
        );
        assert!(matches!(
            parse_loading_overlay_bitmap(LoadingOverlayBitmap::Line, b"bad"),
            Err(StageFrameError::LoadingOverlayBitmap {
                bitmap: LoadingOverlayBitmap::Line,
                error: BitmapError::Truncated,
            })
        ));
    }

    #[test]
    fn authored_curved_nodes_drive_checkpoint_progression() {
        let nodes = vec![
            native_test_node([0x10000, 0, 0], [0x10000, 0, 0], 1),
            native_test_node([0x10000, 0, 0x10000], [0, 0, 0x10000], 1),
            native_test_node([0x20000, 0, 0x10000], [0x10000, 0, 0], 1),
        ];
        let mut progress = StageProgress::new(&nodes, 0);
        let car = Vec3::new(Fixed16::from_int(2), Fixed16::ZERO, Fixed16::from_int(2));
        assert!(matches!(
            progress.update(car, 10),
            cmr_core::stage_checkpoints::StageEvent::Split { node: 0, .. }
        ));
        assert!(matches!(
            progress.update(car, 20),
            cmr_core::stage_checkpoints::StageEvent::Split { node: 1, .. }
        ));
        let car = Vec3::new(Fixed16::from_int(3), Fixed16::ZERO, Fixed16::from_int(2));
        let third = progress.update(car, 30);
        assert!(
            matches!(
                third,
                cmr_core::stage_checkpoints::StageEvent::Finished { node: 2, .. }
            ),
            "{third:?}"
        );
        assert!(matches!(
            progress.update(car, 40),
            cmr_core::stage_checkpoints::StageEvent::Idle
        ));
        assert_eq!(progress.split_times(), &[10, 20, 30]);
        assert_eq!(progress.finish_centiseconds(), Some(30));
    }

    #[test]
    fn live_hud_adapter_uses_recovered_timer_and_speed_arithmetic() {
        let nodes = [
            native_test_node([0, 0, 0], [0x10000, 0, 0], 1),
            native_test_node([0x10000, 0, 0], [0x10000, 0, 0], 1),
        ];
        let progress = StageProgress::new(&nodes, 0);
        let mut car = CarState::at_rest();
        car.velocity = Vec3::new(Fixed16::ONE, Fixed16::ZERO, Fixed16::ZERO);

        let input = live_race_hud_inputs(
            car,
            None,
            &progress,
            nodes.len(),
            Fixed16::from_raw(0x1_8000),
        );
        assert_eq!(input.speed, 0x43);
        assert_eq!(input.elapsed_centiseconds, 150);
        assert_eq!(input.progress_q16, 0);
        assert_eq!(input.rev_q16, 0);
        assert_eq!(input.gear, 1);
    }

    #[test]
    #[ignore = "requires the reviewed installed-EU runtime data"]
    fn imported_au1_loading_background_decodes_from_user_media() {
        let root = std::env::var_os("CMR_RUNTIME_DATA")
            .map(PathBuf::from)
            .expect("set CMR_RUNTIME_DATA to the reviewed installed-EU root");
        let bytes =
            read_loading_background(&root.join("Game/Au/1"), "AU1", DecodeLimits::default())
                .unwrap()
                .expect("AU1 selected-stage loading sibling");
        let frame = parse_loading_background(&bytes).unwrap();
        assert_eq!(frame.extent(), ROOT_EXTENT);
        assert_eq!(frame.pixels().len(), 640 * 480);

        let reviewed = ReviewedProfile::from_json(REVIEWED_PROFILE).unwrap();
        let store = AssetStore::open(&root, &reviewed, DecodeLimits::default()).unwrap();
        let overlay = LoadingOverlayAssets::load(&store).unwrap();
        assert_eq!((overlay.bar.width, overlay.bar.height), (267, 13));
        assert_eq!(overlay.bar.row_stride, 268);
        assert_eq!((overlay.line.width, overlay.line.height), (8, 8));
        assert_eq!(overlay.line.row_stride, 8);
        let original_pixels = frame.pixels().to_vec();
        let composed = compose_load_interstitial(
            frame,
            &overlay.bar,
            &overlay.line,
            STAGE_LOADING_PROGRESS_CAP,
        )
        .unwrap();
        assert_eq!(composed.extent(), ROOT_EXTENT);
        assert_ne!(composed.pixels(), original_pixels);
    }

    #[test]
    #[ignore = "requires the reviewed installed-EU runtime data"]
    fn imported_au1_native_setup_composes_from_the_real_spawn_and_palette() {
        let root = std::env::var_os("CMR_RUNTIME_DATA")
            .map(PathBuf::from)
            .expect("set CMR_RUNTIME_DATA to the reviewed installed-EU root");
        let (mut resources, nodes, start, initial_camera, _) = native_stage_setup(
            &root.join("Game/Au/1"),
            &root,
            "AU1",
            Some("Sub"),
            StageRendererChoice::Software,
        )
        .unwrap();
        assert!(!nodes.is_empty());
        assert_ne!(start.position, Vec3::ZERO);
        assert_ne!(initial_camera, stage_placeholder_camera());
        assert_ne!(resources.palette, stage_placeholder_palette());
        assert!(resources.bundle.horizon().is_some());
        assert!(resources.bundle.ars().is_some());
        assert!(
            resources
                .bundle
                .triggers()
                .is_some_and(|triggers| !triggers.is_empty())
        );
        assert!(
            resources
                .bundle
                .trackside_cameras()
                .is_some_and(|cameras| !cameras.is_empty())
        );
        assert!(resources.horizon_textures.is_some());
        assert!(resources.car_scene.is_some());
        assert!(resources.car_textures.is_some());
        assert_eq!(
            resources
                .loading_background()
                .expect("AU1 retains its selected-stage LOA")
                .extent(),
            ROOT_EXTENT
        );
        let hud = resources
            .race_hud()
            .expect("native setup retains HUD assets");
        let gauge = hud.bitmap(RaceHudBitmap::CommonGauge640);
        assert_eq!([gauge.width, gauge.height], [256, 256]);
        let marker = hud.bitmap(RaceHudBitmap::ProgressRedSpot);
        assert!(marker.width >= 32 && marker.height >= 32);
        let bar = hud.bitmap(RaceHudBitmap::HorizontalBar);
        let bar_end = hud.bitmap(RaceHudBitmap::HorizontalBarEnd);
        assert!(bar.width >= 4 && bar.height >= 4);
        assert!(bar_end.width >= 4 && bar_end.height >= 4);
        let numbers = hud.font(RaceHudFont::Numbers640);
        assert!(
            b"0123456789NR"
                .iter()
                .all(|code| numbers.glyph(*code).is_some())
        );
        let timer = hud.font(RaceHudFont::TimeNumbers640);
        assert!(
            b"0123456789:."
                .iter()
                .all(|code| timer.glyph(*code).is_some())
        );
        let harness_car_world = cmr_core::gameplay_camera::car_world_q16(
            &cmr_core::fixed_point::build_sine_table(),
            start.position,
            start.heading,
        );
        let car_world = car_model_world_q16(&harness_car_world);
        let without_car = resources.compose(&initial_camera).unwrap();
        let rendered = resources
            .compose_with_car(&initial_camera, Some(&car_world))
            .unwrap();
        assert_ne!(
            rendered.frame.digest_summary().canonical_frame_sha256,
            without_car.frame.digest_summary().canonical_frame_sha256
        );
        assert!(matches!(
            rendered.stats,
            StageRenderStats::Software(stats) if stats.prims_composed >= 3
        ));
        let hud_frame = compose_live_race_hud(
            &resources,
            rendered.frame.clone(),
            live_race_hud_inputs(
                start,
                None,
                &StageProgress::new(&nodes, 0),
                nodes.len(),
                Fixed16::ZERO,
            ),
        )
        .unwrap();
        assert_ne!(
            hud_frame.digest_summary().canonical_frame_sha256,
            rendered.frame.digest_summary().canonical_frame_sha256
        );
    }

    #[test]
    #[ignore = "requires reviewed owned AU1, compiled-case, and car-selector runtime data"]
    fn imported_au1_compiled_pose_adjusts_spawn_before_first_composition() {
        let root = std::env::var_os("CMR_RUNTIME_DATA")
            .map(PathBuf::from)
            .expect("set CMR_RUNTIME_DATA to the reviewed installed-EU root");
        let (mut resources, _, mut start, _, actor_rows) = native_stage_setup(
            &root.join("Game/Au/1"),
            &root,
            "AU1",
            Some("Sub"),
            StageRendererChoice::Software,
        )
        .unwrap();
        let authored_y = start.position.y;
        let authored_half_words = start.motion.compiled_half_words;
        let authored_basis = start.motion.basis;
        let authored_projection_axis = start.motion.projection_axis;
        let mut pose = SessionCompiledPose::open(&root, "AU1", "Sub").unwrap();
        let deformation = pose.adjust_spawn(&mut start, actor_rows).unwrap();
        assert_ne!(start.position.y, authored_y);
        assert_ne!(start.motion.compiled_half_words, authored_half_words);
        assert_ne!(start.motion.basis, authored_basis);
        assert_ne!(start.motion.projection_axis, authored_projection_axis);
        assert_eq!(start.motion.projection_axis, start.motion.basis[1]);
        assert_ne!(
            deformation,
            cmr_core::live_static_collision_car_frame::UpperHullDeformationScalars {
                scale_638: Fixed16::ZERO,
                scale_63c: Fixed16::ZERO,
                scale_640: Fixed16::ZERO,
            }
        );
        let mut view = ViewHarness::new();
        let camera = view.update(false, start.position, start.heading, false);
        let harness_world = cmr_core::gameplay_camera::car_world_q16(
            &cmr_core::fixed_point::build_sine_table(),
            start.position,
            start.heading,
        );
        let frame = resources
            .compose_with_car(&camera, Some(&car_model_world_q16(&harness_world)))
            .unwrap();
        assert!(matches!(
            frame.stats,
            StageRenderStats::Software(stats) if stats.prims_composed >= 3
        ));
    }

    #[test]
    #[ignore = "requires the reviewed installed-EU runtime data and a Vulkan adapter"]
    fn imported_au1_layered_gpu_matches_software_with_horizon_and_car() {
        let root = std::env::var_os("CMR_RUNTIME_DATA")
            .map(PathBuf::from)
            .expect("set CMR_RUNTIME_DATA to the reviewed installed-EU root");
        let (mut software, _, start, camera, _) = native_stage_setup(
            &root.join("Game/Au/1"),
            &root,
            "AU1",
            Some("Sub"),
            StageRendererChoice::Software,
        )
        .unwrap();
        let (mut gpu, _, gpu_start, gpu_camera, _) = native_stage_setup(
            &root.join("Game/Au/1"),
            &root,
            "AU1",
            Some("Sub"),
            StageRendererChoice::Gpu,
        )
        .unwrap();
        assert_eq!(gpu_start, start);
        assert_eq!(gpu_camera, camera);
        let harness_car_world = cmr_core::gameplay_camera::car_world_q16(
            &cmr_core::fixed_point::build_sine_table(),
            start.position,
            start.heading,
        );
        let car_world = car_model_world_q16(&harness_car_world);

        let software_frame = software
            .compose_with_car(&camera, Some(&car_world))
            .unwrap();
        let gpu_frame = gpu.compose_with_car(&camera, Some(&car_world)).unwrap();
        assert_eq!(gpu_frame.renderer, StageRendererKind::Gpu);
        assert_eq!(gpu_frame.frame.palette(), software_frame.frame.palette());
        let mismatches = gpu_frame
            .frame
            .pixels()
            .iter()
            .zip(software_frame.frame.pixels())
            .filter(|(gpu, software)| gpu != software)
            .count();
        assert!(
            mismatches <= 307,
            "imported AU1 exceeds the established 0.1% GPU/software raster boundary: {mismatches} pixels"
        );
    }

    #[test]
    #[ignore = "requires the reviewed installed-EU runtime data"]
    fn imported_au1_reports_car_vertical_clearance_metadata() {
        use cmr_core::c3d_camera::{apply_affine_q16, invert_affine_q16};

        let root = std::env::var_os("CMR_RUNTIME_DATA")
            .map(PathBuf::from)
            .expect("set CMR_RUNTIME_DATA to the reviewed installed-EU root");
        let (resources, nodes, start, _, _) = native_stage_setup(
            &root.join("Game/Au/1"),
            &root,
            "AU1",
            Some("Sub"),
            StageRendererChoice::Software,
        )
        .unwrap();
        let car_file = resources
            .car_scene
            .as_ref()
            .expect("the owned-media setup retains the selected car");
        let harness_world = cmr_core::gameplay_camera::car_world_q16(
            &cmr_core::fixed_point::build_sine_table(),
            start.position,
            start.heading,
        );
        let model_world = car_model_world_q16(&harness_world);
        let scene = StageScene::from_c3d(car_file, &model_world, VERTEXTYPE_LVERTEX).unwrap();
        let car_records: Vec<_> = car_file.group0_records().collect();
        let root_metadata: Vec<_> = car_records
            .iter()
            .enumerate()
            .filter(|(_, record)| record.parent_index().is_none())
            .map(|(index, record)| {
                let local = record.affine_matrix_q16();
                (
                    index,
                    record.node_class(),
                    [local[12], local[13], local[14]],
                )
            })
            .collect();
        let marker_metadata: Vec<_> = car_records
            .iter()
            .enumerate()
            .filter(|(_, record)| (1..=6).contains(&record.node_id()))
            .map(|(index, record)| {
                let local = record.affine_matrix_q16();
                (
                    index,
                    record.node_id(),
                    record.parent_index(),
                    [local[12], local[13], local[14]],
                )
            })
            .collect();
        let camera = cmr_core::gameplay_camera::chase_target_world(
            &harness_world,
            cmr_core::gameplay_camera::CHASE_OFFSETS[0],
        );
        let view = invert_affine_q16(&camera);

        let mut min_y = i32::MAX;
        let mut max_y = i32::MIN;
        let mut min_view_z = i32::MAX;
        let mut max_view_z = i32::MIN;
        let mut vertex_count = 0usize;
        for op in scene.draw_order() {
            let Some(lod) = scene.mesh(op.node).and_then(|mesh| mesh.active()) else {
                continue;
            };
            for prim in lod.prims() {
                for vertex in prim.unique_entries_32() {
                    let world = apply_affine_q16(scene.world(op.node), vertex.position_q16());
                    let camera_space = apply_affine_q16(&view, world);
                    min_y = min_y.min(world[1]);
                    max_y = max_y.max(world[1]);
                    min_view_z = min_view_z.min(camera_space[2]);
                    max_view_z = max_view_z.max(camera_space[2]);
                    vertex_count += 1;
                }
            }
        }
        assert!(vertex_count > 0);
        assert!(min_y <= max_y);
        assert!(min_view_z >= Fixed16::ONE.raw());

        // Diagnostic only: the native attach path applies the CSP translation
        // directly, while the later physics-derived vertical pose is not wired
        // into the planar harness. Do not bless a guessed clearance. Keep the
        // output path-free and payload-free for a bounded owned-media comparison.
        let road_reference_y = nodes[0].position[1];
        let stage_scene = StageScene::from_c3d(
            resources.bundle.scene(),
            &MATRIX_IDENTITY_Q16,
            VERTEXTYPE_LVERTEX,
        )
        .unwrap();
        let mut nearest_stage_vertex_xz_sq = i128::MAX;
        let mut nearest_stage_vertex = [0i32; 3];
        for op in stage_scene.draw_order() {
            let Some(lod) = stage_scene.mesh(op.node).and_then(|mesh| mesh.active()) else {
                continue;
            };
            for prim in lod.prims() {
                for vertex in prim.unique_entries_32() {
                    let world = apply_affine_q16(stage_scene.world(op.node), vertex.position_q16());
                    let dx = i128::from(world[0]) - i128::from(start.position.x.raw());
                    let dz = i128::from(world[2]) - i128::from(start.position.z.raw());
                    let distance = dx * dx + dz * dz;
                    if distance < nearest_stage_vertex_xz_sq {
                        nearest_stage_vertex_xz_sq = distance;
                        nearest_stage_vertex = world;
                    }
                }
            }
        }
        let stage_frame = compose_stage_scene_frame(
            &stage_scene,
            &camera,
            [ROOT_EXTENT.width, ROOT_EXTENT.height],
            &resources.palette,
            SoftwareC3dIndexedQuantization::NearestRgbL1LowestIndexNoDither,
            |ordinal| resources.textures.texture_for_ordinal(ordinal),
        )
        .unwrap();
        let car_frame = compose_stage_scene_frame_color_key(
            &scene,
            &camera,
            [ROOT_EXTENT.width, ROOT_EXTENT.height],
            &resources.palette,
            SoftwareC3dIndexedQuantization::NearestRgbL1LowestIndexNoDither,
            0,
            |ordinal| {
                resources
                    .car_textures
                    .as_ref()
                    .and_then(|textures| textures.texture_for_ordinal(ordinal))
            },
        )
        .unwrap();
        let mut car_depth_pixels = 0usize;
        let mut car_visible_pixels = 0usize;
        let mut car_terrain_occluded_pixels = 0usize;
        let mut car_bbox = [usize::MAX, usize::MAX, 0, 0];
        let mut visible_bbox = [usize::MAX, usize::MAX, 0, 0];
        let width = usize::from(ROOT_EXTENT.width);
        for (index, (&car_depth, &stage_depth)) in car_frame
            .depth()
            .values()
            .iter()
            .zip(stage_frame.depth().values())
            .enumerate()
        {
            if car_depth == 1.0 {
                continue;
            }
            car_depth_pixels += 1;
            let x = index % width;
            let y = index / width;
            car_bbox[0] = car_bbox[0].min(x);
            car_bbox[1] = car_bbox[1].min(y);
            car_bbox[2] = car_bbox[2].max(x);
            car_bbox[3] = car_bbox[3].max(y);
            if car_depth <= stage_depth {
                car_visible_pixels += 1;
                visible_bbox[0] = visible_bbox[0].min(x);
                visible_bbox[1] = visible_bbox[1].min(y);
                visible_bbox[2] = visible_bbox[2].max(x);
                visible_bbox[3] = visible_bbox[3].max(y);
            } else {
                car_terrain_occluded_pixels += 1;
            }
        }
        assert_eq!(
            car_depth_pixels,
            car_visible_pixels + car_terrain_occluded_pixels
        );
        assert!(car_visible_pixels > 0);
        assert!(car_terrain_occluded_pixels > 0);
        eprintln!(
            "owned-au1 car-clearance nodes={} roots={root_metadata:?} markers={marker_metadata:?} vertices={vertex_count} spawn={:?} min_y_q16={min_y} max_y_q16={max_y} cat_y_q16={road_reference_y} min_vs_cat_q16={} nearest_stage_vertex={nearest_stage_vertex:?} nearest_stage_vertex_xz_sq={nearest_stage_vertex_xz_sq} view_z_q16={min_view_z}..={max_view_z} car_prims={} car_fragments={} car_self_depth_rejected={} car_depth_pixels={car_depth_pixels} car_visible_pixels={car_visible_pixels} car_terrain_occluded_pixels={car_terrain_occluded_pixels} car_bbox={car_bbox:?} visible_bbox={visible_bbox:?}",
            car_records.len(),
            [
                start.position.x.raw(),
                start.position.y.raw(),
                start.position.z.raw()
            ],
            min_y.wrapping_sub(road_reference_y),
            car_frame.stats().prims_composed,
            car_frame.stats().covered,
            car_frame.stats().depth_rejected,
        );
    }

    /// Drives the windowed frame loop with the four-wheel producer on owned
    /// AU1 data and writes every `CMR_FRAMES_EVERY`th world frame as a PPM
    /// under `CMR_FRAMES_DIR` (default: the workspace `.work/frames`). `CMR_FRAMES_STEER`
    /// (`left`/`right`) holds that key from `CMR_FRAMES_STEER_AFTER`.
    #[test]
    #[ignore = "requires the reviewed installed-EU runtime data; writes frames under .work"]
    fn imported_au1_four_wheel_window_frames() {
        let root = std::env::var_os("CMR_RUNTIME_DATA")
            .map(PathBuf::from)
            .expect("set CMR_RUNTIME_DATA to the reviewed installed-EU root");
        let env_number = |name: &str, default: usize| {
            std::env::var(name)
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(default)
        };
        let frames = env_number("CMR_FRAMES_TICKS", 300);
        let every = env_number("CMR_FRAMES_EVERY", 50).max(1);
        let steer_after = env_number("CMR_FRAMES_STEER_AFTER", usize::MAX);
        let steer_vk = match std::env::var("CMR_FRAMES_STEER").as_deref() {
            Ok("left") => Some(0x25),
            Ok("right") => Some(0x27),
            _ => None,
        };
        let out_dir = std::env::var_os("CMR_FRAMES_DIR").map_or_else(
            || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.work/frames"),
            PathBuf::from,
        );
        std::fs::create_dir_all(&out_dir).unwrap();
        let (mut resources, nodes, mut start, _, actor_rows) = native_stage_setup(
            &root.join("Game/Au/1"),
            &root,
            "AU1",
            Some("Sub"),
            StageRendererChoice::Software,
        )
        .unwrap();
        let mut compiled_pose = SessionCompiledPose::open(&root, "AU1", "Sub").unwrap();
        compiled_pose.adjust_spawn(&mut start, actor_rows).unwrap();
        let trig = cmr_core::fixed_point::build_sine_table();
        let wheel_inputs = SessionWheelInputs::open(&root);
        let tyre_tables = SessionTyreTables::open(&root);
        let aux = SessionAuxControl::open(&root);
        let params = SimParams {
            inverse_mass: Fixed16::from_raw(0x8000),
            gravity: Vec3::ZERO,
        };
        let (trigger_volumes, trigger_cell_centers) = stage_trigger_inputs(&resources);
        // Harness: the frames run starts in the race session, skipping the
        // front end.
        let mut event = single_stage_event("AU1");
        event.commit_unconditional();
        event.start_race_session();
        let mut session = windowed_drive_session(
            &compiled_pose,
            &trig,
            &wheel_inputs,
            &tyre_tables,
            &aux,
            params,
            start,
            event,
        )
        .with_stage_triggers(&trigger_volumes, &trigger_cell_centers)
        .unwrap();
        assert!(session.is_four_wheel());
        eprintln!(
            "frames static_collision_scene={}",
            resources.static_collision_scene().is_some()
        );
        if let Some(rig) = resources.wheel_rig() {
            eprintln!(
                "frames wheel rig records={:?} radius={:#x} rest={:x?} rows0={:x?}",
                rig.records,
                rig.radius,
                rig.rest_translation,
                rig.rest_rows[0].map(|row| [row.x.raw(), row.y.raw(), row.z.raw()]),
            );
            let car = session.car();
            eprintln!(
                "frames wheel half_y={:#x} pose0={:x?}",
                car.motion.compiled_half_words.y.raw(),
                rig.locals(
                    &car,
                    [0; 4],
                    false,
                    &cmr_core::fixed_point::build_sine_table()
                )
            );
        } else {
            eprintln!("frames wheel rig missing");
        }
        let mut keyboard = KeyboardDriver::player_one();
        let mut analog = AnalogDriver::new();
        let response = DriveResponse {
            max_forward_force: Fixed16::from_raw(GAME_SESSION_DRIVE_FORCE_Q16),
            max_steer_rate: GAME_SESSION_MAX_STEER_RATE,
        };
        let mut progress = StageProgress::new(&nodes, 0);
        let mut camera = ViewHarness::new();
        let mut fade = None;
        let mut damage_owner = NoStaticDamageOwner;
        let mut keys = cmr_platform_sdl3::KeyboardVkState::new();
        let mut root_interaction =
            RootInteraction::from_saved_selection(0, Language::English, ROOT_EXTENT, 0).unwrap();
        // `CMR_FRAMES_AUDIO=1` runs the engine sound over a software mixer
        // rooted at the runtime data and writes the mix to `engine.wav`.
        let audio_mixer: Option<SharedSoftwareMixer> =
            std::env::var_os("CMR_FRAMES_AUDIO").map(|_| {
                let mut mixer = SoftwareMixer::with_source(FileWaveSource::new(&root));
                mixer.initialize().unwrap();
                std::sync::Arc::new(std::sync::Mutex::new(mixer))
            });
        let mut audio = RaceAudio::default();
        if audio_mixer.is_some() {
            audio.engine = EngineAudio::open(&root, compiled_pose.selector as u8);
            let srf = read_decoded_section(
                &root.join("Game/Au/1"),
                "AU1",
                ".srf",
                DecodeLimits::default(),
            )
            .ok()
            .flatten();
            let crowd = stage_crowd_points(&resources);
            audio.effects = EffectsAudio::open(&root, srf, crowd);
        }
        let mut pcm: Vec<i16> = Vec::new();
        let mut pause = PauseHarness::with_runtime_data(&root);
        let mut advance = |session: &mut GameplaySession,
                           progress: &mut StageProgress,
                           keys: &cmr_platform_sdl3::KeyboardVkState,
                           frontend: cmr_core::frontend::FrontendInput| {
            let outcome = advance_game_frame(
                session,
                &mut keyboard,
                &mut analog,
                &response,
                &mut root_interaction,
                progress,
                &StationaryClock,
                &mut resources,
                &mut camera,
                &mut fade,
                &mut damage_owner,
                &mut audio,
                &mut pause,
                WindowedInput {
                    keys,
                    gamepad: None,
                    frontend,
                    audio: audio_mixer.as_ref(),
                },
            )
            .unwrap();
            if let Some(mixer) = audio_mixer.as_ref() {
                // One 40 ms tick of 22050 Hz stereo output.
                let mut block = vec![0i16; 882 * 2];
                mixer.lock().unwrap().render(&mut block);
                pcm.extend_from_slice(&block);
            }
            outcome
        };
        let idle = cmr_core::frontend::FrontendInput::default();
        advance(&mut session, &mut progress, &keys, idle);
        advance(&mut session, &mut progress, &keys, idle);
        advance(
            &mut session,
            &mut progress,
            &keys,
            cmr_core::frontend::FrontendInput {
                next: true,
                ..Default::default()
            },
        );
        advance(
            &mut session,
            &mut progress,
            &keys,
            cmr_core::frontend::FrontendInput {
                confirm: true,
                ..Default::default()
            },
        );
        for _ in 0..8 {
            if session.state().id() == 5 {
                break;
            }
            advance(&mut session, &mut progress, &keys, idle);
        }
        assert_eq!(session.state().id(), 5);
        let origin = session.car().position;
        keys.apply_vk(0x26, true);
        // `CMR_FRAMES_PAUSE_AT=<frame>` taps Escape on that frame.
        let pause_at = std::env::var("CMR_FRAMES_PAUSE_AT")
            .ok()
            .and_then(|value| value.parse::<usize>().ok());
        for frame in 0..frames {
            if let Some(vk) = steer_vk {
                keys.apply_vk(vk, frame >= steer_after);
            }
            if let Some(at) = pause_at {
                keys.apply_vk(0x1b, frame == at);
            }
            let outcome = advance(&mut session, &mut progress, &keys, idle);
            for event in session.last_trigger_events() {
                eprintln!("frames frame={frame} trigger {event:?}");
            }
            if session.last_reset().placed {
                eprintln!("frames frame={frame} reset placed");
            }
            if frame % every == 0 || frame + 1 == frames {
                let car = session.car();
                let moved = car.position.sub(origin);
                eprintln!(
                    "frames frame={frame} state={} cursor={} speed={:#x} moved=({:#x},{:#x},{:#x}) row0=({:#x},{:#x},{:#x}) gear={} contacts={:?}",
                    session.state().id(),
                    progress.cursor(),
                    car.four_wheel.speed_ec,
                    moved.x.raw(),
                    moved.y.raw(),
                    moved.z.raw(),
                    car.motion.basis[0].x.raw(),
                    car.motion.basis[0].y.raw(),
                    car.motion.basis[0].z.raw(),
                    car.four_wheel.gearbox.gear_43a,
                    (
                        car.four_wheel.engine_2e0,
                        car.four_wheel.gearbox,
                        car.four_wheel.pedals.response_2d8
                    ),
                );
                if let Some(world) = outcome.world {
                    let rgba = world.expand_rgba8().unwrap();
                    let extent = rgba.extent();
                    let mut ppm =
                        format!("P6\n{} {}\n255\n", extent.width, extent.height).into_bytes();
                    let (pixels, _) = rgba.pixels().as_chunks::<4>();
                    for pixel in pixels {
                        ppm.extend_from_slice(&pixel[..3]);
                    }
                    std::fs::write(out_dir.join(format!("frame-{frame:05}.ppm")), ppm).unwrap();
                }
            }
        }
        drop(advance);
        if audio_mixer.is_some() {
            let peak = pcm.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
            eprintln!("frames engine audio samples={} peak={peak}", pcm.len());
            std::fs::write(out_dir.join("engine.wav"), stereo_wav(&pcm, 22_050)).unwrap();
        }
    }

    /// A 16-bit stereo PCM WAV of the given interleaved samples.
    fn stereo_wav(samples: &[i16], rate: u32) -> Vec<u8> {
        let data_len = (samples.len() * 2) as u32;
        let mut out = Vec::with_capacity(44 + data_len as usize);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * 4).to_le_bytes());
        out.extend_from_slice(&4u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        for sample in samples {
            out.extend_from_slice(&sample.to_le_bytes());
        }
        out
    }

    #[test]
    #[ignore = "requires the reviewed installed-EU runtime data"]
    fn imported_au1_live_input_moves_the_world_and_advances_checkpoints() {
        let root = std::env::var_os("CMR_RUNTIME_DATA")
            .map(PathBuf::from)
            .expect("set CMR_RUNTIME_DATA to the reviewed installed-EU root");
        let (mut resources, nodes, start, _, _) = native_stage_setup(
            &root.join("Game/Au/1"),
            &root,
            "AU1",
            Some("Sub"),
            StageRendererChoice::Software,
        )
        .unwrap();
        let mut session = GameplaySession::new_with_car(
            SimParams {
                inverse_mass: Fixed16::from_raw(0x8000),
                gravity: Vec3::ZERO,
            },
            InRaceScript::school_harness(),
            start,
        );
        let mut keyboard = KeyboardDriver::player_one();
        let mut analog = AnalogDriver::new();
        let response = DriveResponse {
            max_forward_force: Fixed16::from_raw(GAME_SESSION_DRIVE_FORCE_Q16),
            max_steer_rate: GAME_SESSION_MAX_STEER_RATE,
        };
        let mut progress = StageProgress::new(&nodes, 0);
        let mut camera = ViewHarness::new();
        let mut fade = None;
        let mut damage_owner = NoStaticDamageOwner;
        let mut keys = cmr_platform_sdl3::KeyboardVkState::new();
        let mut root_interaction =
            RootInteraction::from_saved_selection(0, Language::English, ROOT_EXTENT, 0).unwrap();
        macro_rules! advance {
            ($keys:expr, $frontend:expr) => {
                advance_game_frame(
                    &mut session,
                    &mut keyboard,
                    &mut analog,
                    &response,
                    &mut root_interaction,
                    &mut progress,
                    &StationaryClock,
                    &mut resources,
                    &mut camera,
                    &mut fade,
                    &mut damage_owner,
                    &mut RaceAudio::default(),
                    &mut PauseHarness::new(),
                    WindowedInput {
                        keys: $keys,
                        gamepad: None,
                        frontend: $frontend,
                        audio: None,
                    },
                )
                .unwrap()
            };
        }

        advance!(&keys, cmr_core::frontend::FrontendInput::default());
        advance!(&keys, cmr_core::frontend::FrontendInput::default());
        advance!(
            &keys,
            cmr_core::frontend::FrontendInput {
                next: true,
                ..Default::default()
            }
        );
        advance!(
            &keys,
            cmr_core::frontend::FrontendInput {
                confirm: true,
                ..Default::default()
            }
        );
        for _ in 0..8 {
            if session.state().id() == 5 {
                break;
            }
            advance!(&keys, cmr_core::frontend::FrontendInput::default());
        }
        assert_eq!(session.state().id(), 5);
        let initial = advance!(&keys, cmr_core::frontend::FrontendInput::default())
            .world
            .unwrap()
            .digest_summary();
        let initial_car = session.car();
        keys.apply_vk(0x26, true);
        for _ in 0..25 {
            advance!(&keys, cmr_core::frontend::FrontendInput::default());
        }
        keys.apply_vk(0x26, false);
        let driven = advance!(&keys, cmr_core::frontend::FrontendInput::default())
            .world
            .unwrap()
            .digest_summary();
        let driven_car = session.car();
        assert_ne!(driven_car.position, initial_car.position);
        assert_eq!(driven_car.heading, initial_car.heading);
        assert_ne!(
            driven.canonical_frame_sha256,
            initial.canonical_frame_sha256
        );
        assert!(progress.cursor() > 0);

        keys.apply_vk(0x25, true);
        for _ in 0..5 {
            advance!(&keys, cmr_core::frontend::FrontendInput::default());
        }
        keys.apply_vk(0x25, false);
        let steered_car = session.car();
        assert_eq!(
            steered_car.heading.wrapping_sub(driven_car.heading) as i16,
            -5 * GAME_SESSION_MAX_STEER_RATE
        );
        eprintln!(
            "native AU1: position={:?} velocity={:?} checkpoint={} splits={}",
            steered_car.position,
            steered_car.velocity,
            progress.cursor(),
            progress.splits_passed()
        );
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
    fn tyre_tables_opener_pins_the_reviewed_profile_and_rejects_a_missing_store() {
        let reviewed = ReviewedTyreTablesProfile::from_json(REVIEWED_TYRE_TABLES_PROFILE).unwrap();
        assert_eq!(reviewed.profile().id, "official-eu-tyre-surface-tables-v1");
        let temp = wheel_runtime();
        assert!(matches!(
            SessionTyreTables::open(&temp.0),
            Err(SessionTyreTablesError::Store(_))
        ));
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
                .four_wheel_params(
                    &trig,
                    &families,
                    &half_state,
                    Fixed16::ONE,
                    &HEADLESS_HARNESS_FOUR_WHEEL_CONSTANTS,
                    [0, 0, 0, 120]
                )
                .err(),
            Some(SurfaceCoefficientIndexError { index: 120 })
        );
        let wheel = inputs
            .four_wheel_params(
                &trig,
                &families,
                &half_state,
                Fixed16::ONE,
                &HEADLESS_HARNESS_FOUR_WHEEL_CONSTANTS,
                [0; 4],
            )
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
    #[ignore = "diagnostic probe: requires the reviewed installed-EU root in CMR_RUNTIME_DATA"]
    fn probe_compiled_four_wheel_car_on_au1() {
        use cmr_core::gearbox::GearboxState;
        use cmr_core::pedal_response::RawPedals;
        let root = std::env::var_os("CMR_RUNTIME_DATA")
            .map(PathBuf::from)
            .expect("set CMR_RUNTIME_DATA to the reviewed installed-EU root");
        let (_resources, nodes, mut start, _, actor_rows) = native_stage_setup(
            &root.join("Game/Au/1"),
            &root,
            "AU1",
            Some("Sub"),
            StageRendererChoice::Software,
        )
        .unwrap();
        let mut progress = StageProgress::new(&nodes, 0);
        let mut pose = SessionCompiledPose::open(&root, "AU1", "Sub").unwrap();
        pose.adjust_spawn(&mut start, actor_rows).unwrap();
        let inputs = SessionWheelInputs::open(&root).unwrap();
        let tyre_tables = SessionTyreTables::open(&root).unwrap();
        let trig = cmr_core::fixed_point::build_sine_table();
        let aux = SessionAuxControl::open(&root).unwrap();
        let wheel = pose
            .compiled_four_wheel_params(&inputs, &trig, tyre_tables.tables(), &aux)
            .unwrap();
        start.four_wheel.gearbox = GearboxState::at_setup(true);
        if let Some(gearbox) = &wheel.gearbox {
            eprintln!(
                "probe gearbox tables={:x?} ratios={:x?} redline={:#x}",
                gearbox.tables, gearbox.drive_ratios, wheel.engine_smoothing.redline_15c
            );
        }
        let mut state = cmr_core::race_state::RaceState::new(start);
        let throttle = Controls {
            raw_pedals: RawPedals {
                throttle_304: 0xffbe,
                ..RawPedals::default()
            },
            ..Controls::default()
        };
        eprintln!(
            "probe start contacts={:?} count674={} projection=({:#x},{:#x},{:#x})",
            start.motion.contact_distance_flags,
            start.motion.contact_count_674,
            start.motion.projection_axis.x.raw(),
            start.motion.projection_axis.y.raw(),
            start.motion.projection_axis.z.raw(),
        );
        for (index, node) in nodes.iter().take(4).enumerate() {
            eprintln!(
                "probe node[{index}] pos=({:#x},{:#x},{:#x}) dir=({:#x},{:#x},{:#x})",
                node.position[0],
                node.position[1],
                node.position[2],
                node.direction[0],
                node.direction[1],
                node.direction[2],
            );
        }
        eprintln!(
            "probe start pos=({:#x},{:#x},{:#x}) row0=({:#x},{:#x},{:#x})",
            start.position.x.raw(),
            start.position.y.raw(),
            start.position.z.raw(),
            start.motion.basis[0].x.raw(),
            start.motion.basis[0].y.raw(),
            start.motion.basis[0].z.raw(),
        );
        let origin = start.position;
        let every = std::env::var("CMR_PROBE_EVERY")
            .ok()
            .and_then(|value| value.parse::<i32>().ok())
            .unwrap_or(25);
        let ticks = std::env::var("CMR_PROBE_TICKS")
            .ok()
            .and_then(|value| value.parse::<i32>().ok())
            .unwrap_or(250);
        eprintln!(
            "probe inverse_mass_4c={:#x} offsets={:x?} config_158={:#x}",
            wheel.inverse_mass_4c.raw(),
            wheel
                .contact_offsets
                .map(|offset| [offset.x.raw(), offset.y.raw(), offset.z.raw()]),
            wheel.first[0].config_158,
        );
        for tick in 0..ticks {
            let steer_after = std::env::var("CMR_PROBE_STEER_AFTER")
                .ok()
                .and_then(|value| value.parse::<i32>().ok());
            let mut controls = throttle;
            if steer_after.is_some_and(|after| tick >= after) {
                let right = std::env::var("CMR_PROBE_STEER").as_deref() == Ok("right");
                controls.raw_steer = cmr_core::vehicle_dynamics::RawSteer {
                    left_2fc: if right { 0 } else { 0xfc00 },
                    right_300: if right { 0xfc00 } else { 0 },
                    analog_410: false,
                };
            }
            if let Err(error) = state.step_with_all_four_wheels(&controls, &wheel) {
                panic!("tick {tick}: {error:?}");
            }
            let mut config = |_lane: usize| Ok::<u8, std::convert::Infallible>(0);
            if let Err(error) = state.run_ground_pass(
                wheel.lane_time_step,
                &mut pose.geometry.eight_lane_providers(),
                &mut config,
            ) {
                panic!("tick {tick} ground: {error:?}");
            }
            let environment = cmr_core::gameplay_session::ResetEnvironment::single_player_harness();
            let dispatch = cmr_core::car_reset::ResetDispatchInput {
                nodes: progress.nodes(),
                cursor: progress.cursor() as i32,
                loop_offset_57b030: environment.loop_offset_57b030,
                race_state_57b0ac: environment.race_state_57b0ac,
                replay_d3ca88: environment.replay_d3ca88,
                config_166: environment.config_166,
                config_168: environment.config_168,
                comparison_xz: environment.comparison_xz,
                position: Vec3::ZERO,
                row0: Vec3::ZERO,
            };
            match state.run_reset(
                &dispatch,
                &mut pose.geometry.eight_lane_providers(),
                &mut config,
            ) {
                Ok(outcome) => {
                    if let Some(cursor) = outcome.cursor {
                        progress.set_cursor(cursor as usize);
                    }
                    if outcome.placed {
                        eprintln!(
                            "probe reset placed at tick {tick} cursor={:?}",
                            outcome.cursor
                        );
                    }
                }
                Err(error) => panic!("tick {tick} reset: {error:?}"),
            }
            let sample = cmr_core::race_state::TraceSample {
                clock: Fixed16::ZERO,
                centiseconds: 0,
                car: state.car(),
            };
            progress.update(sample.car.position, tick);
            let from = std::env::var("CMR_PROBE_FROM")
                .ok()
                .and_then(|value| value.parse::<i32>().ok())
                .unwrap_or(0);
            if tick >= from && (tick % every == 0 || tick == ticks - 1) {
                let car = sample.car;
                let moved = car.position.sub(origin);
                eprintln!(
                    "probe tick={tick} gear={} shift={} engine={:#x} speed={:#x} \
                     resp2d8={:#x} moved=({:#x},{:#x},{:#x}) y_axis=({:#x},{:#x},{:#x})",
                    car.four_wheel.gearbox.gear_43a,
                    car.four_wheel.gearbox.shift_40c,
                    car.four_wheel.engine_2e0,
                    car.four_wheel.speed_ec,
                    car.four_wheel.pedals.response_2d8,
                    moved.x.raw(),
                    moved.y.raw(),
                    moved.z.raw(),
                    car.motion.basis[1].x.raw(),
                    car.motion.basis[1].y.raw(),
                    car.motion.basis[1].z.raw(),
                );
                eprintln!(
                    "probe   upright={} force530={:#x} wheel_force={:x?} wheel_speed={:x?} smoothed={:x?}",
                    car.four_wheel.upright_404,
                    car.four_wheel.engine_force_530,
                    car.four_wheel.first.map(|wheel| wheel.force),
                    car.four_wheel.first.map(|wheel| wheel.wheel_speed),
                    car.four_wheel.smoothed_wheel_speeds,
                );
                eprintln!(
                    "probe   cursor={} row0=({:#x},{:#x},{:#x}) row2=({:#x},{:#x},{:#x}) heading={:#x}",
                    progress.cursor(),
                    car.motion.basis[0].x.raw(),
                    car.motion.basis[0].y.raw(),
                    car.motion.basis[0].z.raw(),
                    car.motion.basis[2].x.raw(),
                    car.motion.basis[2].y.raw(),
                    car.motion.basis[2].z.raw(),
                    car.heading,
                );
                if let Some(ground) = car.ground {
                    eprintln!(
                        "probe   pos_y={:#x} vel=({:#x},{:#x},{:#x}) clear743={} count742={} contacts={:?} lane_words={:x?}",
                        car.position.y.raw(),
                        car.velocity.x.raw(),
                        car.velocity.y.raw(),
                        car.velocity.z.raw(),
                        ground.thresholds.flag_743,
                        ground.thresholds.count_742,
                        car.motion.contact_distance_flags,
                        &ground.lanes.lane_words[..4],
                    );
                }
                eprintln!(
                    "probe   omega=({:#x},{:#x},{:#x}) ratio3b0={:x?} angle={:x?} component={:x?} source_s=({:#x},{:#x},{:#x}) ooc={} tumble={:?}",
                    car.motion.angular_velocity.x.raw(),
                    car.motion.angular_velocity.y.raw(),
                    car.motion.angular_velocity.z.raw(),
                    car.four_wheel.second.map(|lane| lane.ratio_at_3b0),
                    car.four_wheel.second.map(|lane| lane.angle),
                    car.four_wheel.second.map(|lane| lane.component),
                    car.motion.lane_source_s.x.raw(),
                    car.motion.lane_source_s.y.raw(),
                    car.motion.lane_source_s.z.raw(),
                    car.collision.response_state_680,
                    (
                        car.motion.projection_axis.x.raw(),
                        car.motion.projection_axis.y.raw(),
                        car.motion.projection_axis.z.raw(),
                        car.ground.map(|ground| (
                            ground.lanes.lane_words,
                            ground.thresholds.flags_73a,
                            ground.probes.map(|p| p[1])
                        ))
                    ),
                );
                eprintln!(
                    "probe   steering={:x?} gains35c={:x?} gains36c={:x?} surfaces={:?}",
                    car.four_wheel.steering,
                    car.four_wheel.tyre_gains.gain_35c,
                    car.four_wheel.tyre_gains.gain_36c,
                    car.ground.map(|ground| ground.lanes.type_words),
                );
                eprintln!(
                    "probe   second={:x?} lanes={:x?} qualified={:?}",
                    car.four_wheel.second.map(|lane| lane.accumulators),
                    car.four_wheel.postprocess.accumulators.map(|lane| [
                        lane.x.raw(),
                        lane.y.raw(),
                        lane.z.raw()
                    ]),
                    car.four_wheel.postprocess.qualified,
                );
            }
        }
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
    fn renderer_status_summaries_are_stable_and_path_free() {
        assert_eq!(
            StageRendererStatus {
                requested: StageRendererChoice::Auto,
                selected: StageRendererKind::Gpu,
                fallback: None,
            }
            .stable_summary(),
            "renderer requested=auto selected=gpu fallback=none"
        );
        assert_eq!(
            StageRendererStatus {
                requested: StageRendererChoice::Auto,
                selected: StageRendererKind::Software,
                fallback: Some(StageRendererFallback {
                    phase: StageRendererFallbackPhase::Frame,
                    reason: "gpu-compose",
                }),
            }
            .stable_summary(),
            "renderer requested=auto selected=software fallback=frame:gpu-compose"
        );
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

        // The hub ends the session in 0xc and hands back to the front end,
        // having stepped the in-race sim and crossed the recovered checkpoint
        // tracker's final gate.
        assert_eq!(first.final_state, 0xc);
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
                "--car-code",
                "Sub",
            ]),
            None,
            None,
        )
        .unwrap();

        assert_eq!(config.command(), AppCommand::HeadlessStageFrame);
        assert_eq!(config.stage(), Some("MONACO1"));
        assert_eq!(config.car_code(), Some("Sub"));
        assert_eq!(config.media_root(), Some(Path::new("/media")));

        assert_eq!(
            runtime_config(
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
            ),
            Err(ConfigError::MissingCarCode)
        );
        assert_eq!(
            runtime_config(
                arguments(&[
                    "--windowed-game-session",
                    "--runtime-data",
                    "/user",
                    "--source",
                    "/media",
                    "--stage",
                    "MONACO1",
                    "--car-code",
                    "../Sub",
                ]),
                None,
                None,
            ),
            Err(ConfigError::InvalidCarCode("../Sub".to_owned()))
        );
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
                "--car-code",
                "Sub",
            ]),
            None,
            None,
        )
        .unwrap();
        assert_eq!(config.command(), AppCommand::WindowedGameSession);
        assert_eq!(config.stage(), Some("MONACO1"));
        assert_eq!(config.car_code(), Some("Sub"));
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
