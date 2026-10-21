//! Platform-neutral process configuration for the native application.
//!
//! This crate deliberately does not choose a renderer or inspect the current
//! directory. The native host supplies process arguments and the optional
//! environment fallback explicitly, which keeps data selection deterministic
//! and straightforward to exercise in contract tests.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::{Path, PathBuf};

use cmr_assets::{AssetStore, AssetStoreError, ReviewedProfile, ReviewedProfileError};
use cmr_core::deferred_palette::DeferredPaletteCommitOutcome;
use cmr_core::display::{CooperativeMode, DisplayExtent, IndexedMode};
use cmr_core::frontend::{FrontendPhase, RootSelection};
use cmr_core::localization::Language;
use cmr_core::root_menu_assets::VisualVariant;
use cmr_core::root_menu_interactive::RootInteraction;
use cmr_core::root_menu_visual::{
    ROOT_INITIAL_HORIZONTAL_TRANSFORM_Q16, ROOT_INITIAL_VERTICAL_TRANSFORM_Q16, RootRenderInput,
};
use cmr_core::startup::{StartupLanguage, StartupScreen};
use cmr_core::startup_display::{
    StartupDisplay, StartupDisplayError, StartupDisplayRequest, initialize_startup_display,
};
use cmr_core::surface::PaletteMapping;
use cmr_formats::bitmap::BitmapLimits;
use cmr_formats::stored_asset::DecodeLimits;
use cmr_platform_sdl3::{InteractiveRootError, run_interactive_root_frame};
use cmr_video::software_root_menu_session::{
    SoftwareRootMenuRenderError, SoftwareRootMenuSessionBuildError, SoftwareRootMenuSessionRequest,
    with_software_root_menu_session,
};
use cmr_video::{
    SoftwareIndexedBackend, SoftwarePresentedFrame, SoftwarePresentedFrameDigest,
    SoftwareVideoError,
};

/// Environment variable consumed by the `cmr-app` binary as a runtime-data
/// fallback.
pub const RUNTIME_DATA_ENV: &str = "CMR_RUNTIME_DATA";

const REVIEWED_PROFILE: &[u8] =
    include_bytes!("../../../profiles/installed-eu-derived-assets-v2.json");
const ROOT_EXTENT: DisplayExtent = DisplayExtent {
    width: 640,
    height: 480,
};

/// An executable operation selected on the command line.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppCommand {
    HeadlessRootFrame,
    WindowedRootFrame,
}

/// Parsed command-line options before environment fallback resolution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppOptions {
    command: AppCommand,
    runtime_data: Option<PathBuf>,
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
            return Err(ConfigError::UnknownArgument(argument));
        }

        Ok(Self {
            command: command.ok_or(ConfigError::MissingCommand)?,
            runtime_data,
        })
    }

    /// Resolves the explicit option against an injected environment fallback.
    ///
    /// An explicit option always wins, including when it is invalid. This
    /// prevents a malformed command from silently selecting unrelated data.
    pub fn resolve(
        self,
        runtime_data_environment: Option<OsString>,
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

        Ok(RuntimeConfig {
            command: self.command,
            runtime_data,
            source,
        })
    }
}

/// Complete process configuration required before a native backend can start.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeConfig {
    command: AppCommand,
    runtime_data: PathBuf,
    source: RuntimeDataSource,
}

impl RuntimeConfig {
    pub fn command(&self) -> AppCommand {
        self.command
    }

    pub fn runtime_data(&self) -> &Path {
        &self.runtime_data
    }

    pub fn runtime_data_source(&self) -> RuntimeDataSource {
        self.source
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
                "missing command; pass --headless-root-frame or --windowed-root-frame"
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
        }
    }
}

impl std::error::Error for ConfigError {}

/// Parses application arguments and resolves their injected environment
/// fallback in one call.
pub fn runtime_config(
    arguments: impl IntoIterator<Item = OsString>,
    runtime_data_environment: Option<OsString>,
) -> Result<RuntimeConfig, ConfigError> {
    AppOptions::parse(arguments)?.resolve(runtime_data_environment)
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
pub fn run_windowed_root_frame(runtime_data: &Path) -> Result<(), WindowedRootFrameError> {
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
        run_interactive_root_frame(interaction, true, |input| {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(values: &[&str]) -> impl Iterator<Item = OsString> {
        values.iter().map(OsString::from)
    }

    #[test]
    fn explicit_runtime_data_wins_over_environment_fallback() {
        let config = runtime_config(
            arguments(&["--headless-root-frame", "--runtime-data", "explicit-data"]),
            Some(OsString::from("environment-data")),
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
        )
        .unwrap();

        assert_eq!(config.command(), AppCommand::WindowedRootFrame);
        assert_eq!(config.runtime_data(), Path::new("native-data"));
    }

    #[test]
    fn missing_and_empty_runtime_data_are_distinct_errors() {
        assert_eq!(
            runtime_config(arguments(&["--headless-root-frame"]), None),
            Err(ConfigError::MissingRuntimeData)
        );
        assert_eq!(
            runtime_config(arguments(&["--headless-root-frame"]), Some(OsString::new())),
            Err(ConfigError::EmptyRuntimeData(
                RuntimeDataSource::Environment
            ))
        );
        assert_eq!(
            runtime_config(
                arguments(&["--headless-root-frame", "--runtime-data", ""]),
                Some(OsString::from("valid-fallback")),
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
        )
        .unwrap();

        assert_eq!(config.runtime_data().as_os_str(), path.as_os_str());
    }
}
