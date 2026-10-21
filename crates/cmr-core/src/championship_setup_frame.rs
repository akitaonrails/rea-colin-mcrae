//! Preflighted software execution of one recovered championship-setup frame.
//!
//! The visual planner remains the authority for command order and cache
//! transitions. This boundary prepares every currently executable text and
//! bitmap command before submitting the first backend effect. Work whose
//! execution contract is not recovered yet remains visible as typed deferred
//! output instead of being guessed here.

use std::fmt;

use cmr_formats::font::{PointI32, TextLayoutLimits};

use crate::blit::{BlitDecision, ColorKey};
use crate::championship_assets::ManufacturerLogo;
use crate::championship_bitmap_raster::UploadedChampionshipLogoPool;
use crate::championship_frame_assets::{ChequeredFrame, UploadedChampionshipFramePool};
use crate::championship_setup_visual::{
    ChampionshipPreviewCache, ChampionshipSetupVisualCommand, ChampionshipSetupVisualError,
    ChampionshipSetupVisualInput, DeferredInfoText, PixelRect, SetupTextCommand, SetupTextIdentity,
    SetupTextRole, plan_championship_setup_frame,
};
use crate::display::{DisplayExtent, IndexedVideoBackend};
use crate::font_raster::{
    PersistentTextOutcome, PersistentTextPrepareError, PreparedPersistentText, UploadedFontPool,
    execute_prepared_persistent_text, prepare_persistent_text,
};
use crate::indexed_bitmap_raster::{
    IndexedBitmapBlitOutcome, IndexedBitmapPlanError, IndexedBitmapRect,
    plan_indexed_bitmap_blit_for_extent,
};
use crate::localization::IndexedTextTable;
use crate::source_store::{
    SourceAccessError, SourceBlitError, UploadedIndexedBitmap, UploadedSourceStore,
};

/// Resources whose execution contracts are already available to this slice.
pub struct ChampionshipSetupFrameResources<'a> {
    pub text: &'a IndexedTextTable,
    pub fonts: &'a UploadedFontPool,
    /// The common frontend selection bitmap. Absence is kept as deferred work.
    pub selection: Option<UploadedIndexedBitmap>,
    pub logos: &'a UploadedChampionshipLogoPool,
    pub frames: &'a UploadedChampionshipFramePool,
}

trait ChampionshipBitmapResources {
    fn selection(&self) -> Option<UploadedIndexedBitmap>;
    fn info_box(&self) -> UploadedIndexedBitmap;
    fn logo(&self, logo: ManufacturerLogo) -> UploadedIndexedBitmap;
    fn frame(&self, frame: ChequeredFrame) -> UploadedIndexedBitmap;
}

impl ChampionshipBitmapResources for ChampionshipSetupFrameResources<'_> {
    fn selection(&self) -> Option<UploadedIndexedBitmap> {
        self.selection
    }

    fn info_box(&self) -> UploadedIndexedBitmap {
        self.frames.info_box()
    }

    fn logo(&self, logo: ManufacturerLogo) -> UploadedIndexedBitmap {
        self.logos.logo(logo)
    }

    fn frame(&self, frame: ChequeredFrame) -> UploadedIndexedBitmap {
        self.frames.frame(frame)
    }
}

/// Caller-selected animation state. The visual transcript recovers the frame
/// group and count, but not the enclosing timer's current frame.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ChampionshipSetupFrameState {
    pub chequered_frame: Option<ChequeredFrame>,
}

/// Stable source identity for a prepared championship bitmap command.
///
/// This is deliberately independent of uploaded indexed-surface handles. A
/// second executor can use the identity to resolve the same source from its
/// own catalog while sharing the recovered command order and blit decision.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ChampionshipBitmapIdentity {
    Selection,
    InfoBox,
    ManufacturerLogo(ManufacturerLogo),
    ChequeredFrame(ChequeredFrame),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeferredChampionshipCommand {
    Selection(ChampionshipSetupVisualCommand),
    InfoText(DeferredInfoText),
    CarTransform(ChampionshipSetupVisualCommand),
    CarVisibilityLatch(ChampionshipSetupVisualCommand),
    PaletteRequest { group: u8, item: u8 },
    NextPreview(ChampionshipSetupVisualCommand),
}

#[derive(Debug, Eq, PartialEq)]
pub enum ChampionshipSetupFramePrepareError {
    Visual(ChampionshipSetupVisualError),
    Text {
        command_index: usize,
        role: SetupTextRole,
        source: PersistentTextPrepareError,
    },
    Bitmap {
        command_index: usize,
        identity: ChampionshipBitmapIdentity,
        source: IndexedBitmapPlanError,
    },
    MissingBitmapExtent {
        command_index: usize,
        identity: ChampionshipBitmapIdentity,
    },
}

impl fmt::Display for ChampionshipSetupFramePrepareError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Visual(source) => source.fmt(formatter),
            Self::Text {
                command_index,
                role,
                source,
            } => write!(
                formatter,
                "championship frame text {role:?} at command {command_index} failed preflight: {source}"
            ),
            Self::Bitmap {
                command_index,
                identity,
                source,
            } => write!(
                formatter,
                "championship frame bitmap {identity:?} at command {command_index} failed preflight: {source}"
            ),
            Self::MissingBitmapExtent {
                command_index,
                identity,
            } => write!(
                formatter,
                "championship frame bitmap {identity:?} at command {command_index} has no source extent"
            ),
        }
    }
}

impl std::error::Error for ChampionshipSetupFramePrepareError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Visual(source) => Some(source),
            Self::Text { source, .. } => Some(source),
            Self::Bitmap { source, .. } => Some(source),
            Self::MissingBitmapExtent { .. } => None,
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum ChampionshipSetupFrameExecutionError<E> {
    Prepare(ChampionshipSetupFramePrepareError),
    TextAccess {
        command_index: usize,
        role: SetupTextRole,
        source: SourceAccessError,
    },
    BitmapAccess {
        command_index: usize,
        identity: ChampionshipBitmapIdentity,
        source: SourceAccessError,
    },
    BitmapBackend {
        command_index: usize,
        identity: ChampionshipBitmapIdentity,
        source: E,
    },
}

impl<E: fmt::Display> fmt::Display for ChampionshipSetupFrameExecutionError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Prepare(source) => source.fmt(formatter),
            Self::TextAccess {
                command_index,
                role,
                source,
            } => write!(
                formatter,
                "championship frame text {role:?} at command {command_index} cannot access its source: {source}"
            ),
            Self::BitmapAccess {
                command_index,
                identity,
                source,
            } => write!(
                formatter,
                "championship frame bitmap {identity:?} at command {command_index} cannot access its source: {source}"
            ),
            Self::BitmapBackend {
                command_index,
                identity,
                source,
            } => write!(
                formatter,
                "championship frame bitmap {identity:?} at command {command_index} failed: {source}"
            ),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for ChampionshipSetupFrameExecutionError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Prepare(source) => Some(source),
            Self::TextAccess { source, .. } | Self::BitmapAccess { source, .. } => Some(source),
            Self::BitmapBackend { source, .. } => Some(source),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum ChampionshipSetupCommandOutcome<E> {
    Text {
        command_index: usize,
        role: SetupTextRole,
        rendered: PersistentTextOutcome<E>,
    },
    Bitmap {
        command_index: usize,
        identity: ChampionshipBitmapIdentity,
        outcome: IndexedBitmapBlitOutcome,
    },
    Deferred {
        command_index: usize,
        command: DeferredChampionshipCommand,
    },
}

#[derive(Debug, Eq, PartialEq)]
pub struct ChampionshipSetupFrameOutcome<E> {
    /// One entry per visual command, in exact transcript order.
    pub commands: Vec<ChampionshipSetupCommandOutcome<E>>,
}

enum PreparedCommand<T, B> {
    Text {
        command_index: usize,
        command: SetupTextCommand,
        text: T,
    },
    Bitmap {
        command_index: usize,
        identity: ChampionshipBitmapIdentity,
        bitmap: B,
        decision: BlitDecision,
    },
    Deferred {
        command_index: usize,
        command: DeferredChampionshipCommand,
    },
}

/// Opaque, effect-free plan for one complete championship setup transcript.
///
/// Call [`Self::commands`] to inspect its exact order without gaining access
/// to uploaded indexed handles or mutable planner state.
pub struct PreparedChampionshipSetupFrame<T = PreparedPersistentText, B = UploadedIndexedBitmap> {
    commands: Vec<PreparedCommand<T, B>>,
    base_cache: ChampionshipPreviewCache,
    next_cache: ChampionshipPreviewCache,
}

/// Effect-free rejection of a prepared frame whose planning snapshot no
/// longer matches the caller-owned preview cache.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChampionshipPreviewCacheMismatch {
    pub expected: ChampionshipPreviewCache,
    pub actual: ChampionshipPreviewCache,
}

impl fmt::Display for ChampionshipPreviewCacheMismatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "championship prepared-frame preview cache is stale: expected {:?}, got {:?}",
            self.expected, self.actual
        )
    }
}

impl std::error::Error for ChampionshipPreviewCacheMismatch {}

/// Failure from a caller-owned execution pass over an opaque prepared frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PreparedChampionshipSetupExecutionError<E> {
    /// The plan was prepared from a different preview-cache snapshot. No
    /// callback was invoked.
    StalePreviewCache {
        expected: ChampionshipPreviewCache,
        actual: ChampionshipPreviewCache,
    },
    /// The ordered callback failed. The preview cache was not committed.
    Command { command_index: usize, source: E },
}

impl<E: fmt::Display> fmt::Display for PreparedChampionshipSetupExecutionError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StalePreviewCache { expected, actual } => write!(
                formatter,
                "championship prepared-frame preview cache is stale: expected {expected:?}, got {actual:?}"
            ),
            Self::Command {
                command_index,
                source,
            } => write!(
                formatter,
                "championship prepared-frame command {command_index} failed: {source}"
            ),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error
    for PreparedChampionshipSetupExecutionError<E>
{
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::StalePreviewCache { .. } => None,
            Self::Command { source, .. } => Some(source),
        }
    }
}

/// Read-only public view of one prepared command.
///
/// Prepared glyph data and uploaded bitmap handles remain private. The exposed
/// data is the shared transcript contract needed by alternate executors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreparedChampionshipSetupCommand {
    Text {
        command_index: usize,
        command: SetupTextCommand,
    },
    Bitmap {
        command_index: usize,
        identity: ChampionshipBitmapIdentity,
        decision: BlitDecision,
    },
    Deferred {
        command_index: usize,
        command: DeferredChampionshipCommand,
    },
}

impl<T, B> PreparedChampionshipSetupFrame<T, B> {
    #[must_use]
    pub fn command_len(&self) -> usize {
        self.commands.len()
    }

    /// Checks the caller-owned cache snapshot before any alternate preflight
    /// or execution work is attempted.
    pub fn validate_preview_cache(
        &self,
        preview_cache: &ChampionshipPreviewCache,
    ) -> Result<(), ChampionshipPreviewCacheMismatch> {
        if *preview_cache == self.base_cache {
            Ok(())
        } else {
            Err(ChampionshipPreviewCacheMismatch {
                expected: self.base_cache,
                actual: *preview_cache,
            })
        }
    }

    /// Iterates over every prepared command in exact visual-transcript order.
    ///
    /// The returned values are copies of immutable command metadata; iterating
    /// cannot submit rendering effects or alter the pending preview cache.
    pub fn commands(&self) -> impl ExactSizeIterator<Item = PreparedChampionshipSetupCommand> + '_ {
        self.commands.iter().map(|command| match command {
            PreparedCommand::Text {
                command_index,
                command,
                ..
            } => PreparedChampionshipSetupCommand::Text {
                command_index: *command_index,
                command: *command,
            },
            PreparedCommand::Bitmap {
                command_index,
                identity,
                decision,
                ..
            } => PreparedChampionshipSetupCommand::Bitmap {
                command_index: *command_index,
                identity: *identity,
                decision: *decision,
            },
            PreparedCommand::Deferred {
                command_index,
                command,
            } => PreparedChampionshipSetupCommand::Deferred {
                command_index: *command_index,
                command: *command,
            },
        })
    }

    /// Executes a caller-owned command family after its complete preflight.
    ///
    /// The callback receives every command exactly once in visual-transcript
    /// order. The pending preview cache is committed only after every callback
    /// succeeds. A callback failure leaves the caller's cache unchanged,
    /// although effects already performed by earlier callbacks cannot be
    /// rolled back by this core-owned ordering boundary.
    pub fn execute_after_successful_preflight<E>(
        self,
        preview_cache: &mut ChampionshipPreviewCache,
        mut execute: impl FnMut(PreparedChampionshipSetupCommand) -> Result<(), E>,
    ) -> Result<(), PreparedChampionshipSetupExecutionError<E>> {
        self.validate_preview_cache(preview_cache)
            .map_err(
                |stale| PreparedChampionshipSetupExecutionError::StalePreviewCache {
                    expected: stale.expected,
                    actual: stale.actual,
                },
            )?;
        for command in self.commands() {
            let command_index = match command {
                PreparedChampionshipSetupCommand::Text { command_index, .. }
                | PreparedChampionshipSetupCommand::Bitmap { command_index, .. }
                | PreparedChampionshipSetupCommand::Deferred { command_index, .. } => command_index,
            };
            execute(command).map_err(|source| {
                PreparedChampionshipSetupExecutionError::Command {
                    command_index,
                    source,
                }
            })?;
        }
        *preview_cache = self.next_cache;
        Ok(())
    }

    /// Deferred work in exact visual-command order. This lets an enclosing
    /// terminal executor preflight its own command families before the first
    /// destination effect without exposing the prepared text/bitmap details.
    pub(crate) fn deferred_commands(
        &self,
    ) -> impl Iterator<Item = (usize, DeferredChampionshipCommand)> + '_ {
        self.commands().filter_map(|command| match command {
            PreparedChampionshipSetupCommand::Deferred {
                command_index,
                command,
            } => Some((command_index, command)),
            PreparedChampionshipSetupCommand::Text { .. }
            | PreparedChampionshipSetupCommand::Bitmap { .. } => None,
        })
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum PreparedChampionshipSetupFrameExecutionError<E, D> {
    Frame(ChampionshipSetupFrameExecutionError<E>),
    Deferred { command_index: usize, source: D },
}

impl<E: fmt::Display, D: fmt::Display> fmt::Display
    for PreparedChampionshipSetupFrameExecutionError<E, D>
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Frame(source) => source.fmt(formatter),
            Self::Deferred {
                command_index,
                source,
            } => write!(
                formatter,
                "championship frame deferred command {command_index} failed: {source}"
            ),
        }
    }
}

/// Preflights and executes one setup frame, committing the preview cache only
/// after every fatal backend operation succeeds. Preparation and source-access
/// checks therefore have no destination effects. Backend bitmap failures are
/// ordered and fail-fast, so they may leave effects from earlier commands.
/// Per-glyph backend failures remain nonfatal diagnostics inside their text
/// command outcome.
pub fn execute_championship_setup_frame<B: IndexedVideoBackend>(
    input: ChampionshipSetupVisualInput<'_>,
    state: ChampionshipSetupFrameState,
    resources: ChampionshipSetupFrameResources<'_>,
    preview_cache: &mut ChampionshipPreviewCache,
    sources: &mut UploadedSourceStore<'_, B>,
    destination: B::Surface,
    text_limits: TextLayoutLimits,
) -> Result<ChampionshipSetupFrameOutcome<B::Error>, ChampionshipSetupFrameExecutionError<B::Error>>
{
    let prepared =
        prepare_championship_setup_frame(input, state, resources, *preview_cache, text_limits)
            .map_err(ChampionshipSetupFrameExecutionError::Prepare)?;
    validate_championship_setup_frame_sources(&prepared, sources)?;
    execute_prepared_championship_setup_frame(
        prepared,
        preview_cache,
        sources,
        destination,
        |_, _| Ok::<_, std::convert::Infallible>(()),
    )
    .map_err(|error| match error {
        PreparedChampionshipSetupFrameExecutionError::Frame(source) => source,
        PreparedChampionshipSetupFrameExecutionError::Deferred { source, .. } => match source {},
    })
}

/// Prepares a complete setup-frame transcript without rendering effects.
///
/// Uploaded source ownership is validated separately by the indexed executor;
/// alternate executors can first inspect [`PreparedChampionshipSetupFrame::commands`]
/// and resolve bitmap identities against their own source catalog.
pub fn prepare_championship_setup_frame(
    input: ChampionshipSetupVisualInput<'_>,
    state: ChampionshipSetupFrameState,
    resources: ChampionshipSetupFrameResources<'_>,
    preview_cache: ChampionshipPreviewCache,
    text_limits: TextLayoutLimits,
) -> Result<PreparedChampionshipSetupFrame, ChampionshipSetupFramePrepareError> {
    prepare_frame_with_bitmaps(
        input,
        state,
        resources.text,
        resources.fonts,
        &resources,
        preview_cache,
        text_limits,
    )
}

/// Lightweight prepared setup stream with no glyph payloads or indexed
/// source handles.
pub type PreparedChampionshipAlternateSetupFrame = PreparedChampionshipSetupFrame<(), ()>;

/// Prepares the exact setup transcript from source extents alone.
///
/// The visual planner and bitmap geometry kernel are shared with indexed
/// preparation. Text commands retain their complete public command payload but
/// perform no glyph lookup or layout. Returning `None` for Selection preserves
/// it as typed deferred work; every other requested identity must resolve.
pub fn prepare_championship_setup_frame_from_source_extents(
    input: ChampionshipSetupVisualInput<'_>,
    state: ChampionshipSetupFrameState,
    preview_cache: ChampionshipPreviewCache,
    mut source_extent: impl FnMut(ChampionshipBitmapIdentity) -> Option<(u32, u32)>,
) -> Result<PreparedChampionshipAlternateSetupFrame, ChampionshipSetupFramePrepareError> {
    prepare_frame_kernel(
        input,
        state,
        preview_cache,
        |_, _| Ok(()),
        |identity| Ok(source_extent(identity).map(|extent| ((), extent))),
    )
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
fn execute_frame_with_bitmaps<B: IndexedVideoBackend, R: ChampionshipBitmapResources>(
    input: ChampionshipSetupVisualInput<'_>,
    state: ChampionshipSetupFrameState,
    text: &IndexedTextTable,
    fonts: &UploadedFontPool,
    bitmaps: &R,
    preview_cache: &mut ChampionshipPreviewCache,
    sources: &mut UploadedSourceStore<'_, B>,
    destination: B::Surface,
    text_limits: TextLayoutLimits,
) -> Result<ChampionshipSetupFrameOutcome<B::Error>, ChampionshipSetupFrameExecutionError<B::Error>>
{
    let prepared = prepare_frame_with_bitmaps(
        input,
        state,
        text,
        fonts,
        bitmaps,
        *preview_cache,
        text_limits,
    )
    .map_err(ChampionshipSetupFrameExecutionError::Prepare)?;
    validate_championship_setup_frame_sources(&prepared, sources)?;
    execute_prepared_championship_setup_frame(
        prepared,
        preview_cache,
        sources,
        destination,
        |_, _| Ok::<_, std::convert::Infallible>(()),
    )
    .map_err(|error| match error {
        PreparedChampionshipSetupFrameExecutionError::Frame(source) => source,
        PreparedChampionshipSetupFrameExecutionError::Deferred { source, .. } => match source {},
    })
}

pub(crate) fn execute_prepared_championship_setup_frame<B, F, D>(
    prepared: PreparedChampionshipSetupFrame,
    preview_cache: &mut ChampionshipPreviewCache,
    sources: &mut UploadedSourceStore<'_, B>,
    destination: B::Surface,
    mut execute_deferred: F,
) -> Result<
    ChampionshipSetupFrameOutcome<B::Error>,
    PreparedChampionshipSetupFrameExecutionError<B::Error, D>,
>
where
    B: IndexedVideoBackend,
    F: FnMut(usize, DeferredChampionshipCommand) -> Result<(), D>,
{
    let mut outcomes = Vec::with_capacity(prepared.commands.len());
    for command in prepared.commands {
        match command {
            PreparedCommand::Text {
                command_index,
                command,
                text,
            } => {
                let role = command.role;
                let rendered = execute_prepared_persistent_text(sources, text, destination)
                    .map_err(|source| {
                        PreparedChampionshipSetupFrameExecutionError::Frame(
                            ChampionshipSetupFrameExecutionError::TextAccess {
                                command_index,
                                role,
                                source,
                            },
                        )
                    })?;
                outcomes.push(ChampionshipSetupCommandOutcome::Text {
                    command_index,
                    role,
                    rendered,
                });
            }
            PreparedCommand::Bitmap {
                command_index,
                identity,
                bitmap,
                decision,
            } => {
                let outcome = match decision {
                    BlitDecision::NoOp => IndexedBitmapBlitOutcome::NoOp,
                    BlitDecision::Blit(plan) => {
                        bitmap.blit(sources, destination, plan).map_err(|source| {
                            PreparedChampionshipSetupFrameExecutionError::Frame(match source {
                                SourceBlitError::Access(source) => {
                                    ChampionshipSetupFrameExecutionError::BitmapAccess {
                                        command_index,
                                        identity,
                                        source,
                                    }
                                }
                                SourceBlitError::Backend(source) => {
                                    ChampionshipSetupFrameExecutionError::BitmapBackend {
                                        command_index,
                                        identity,
                                        source,
                                    }
                                }
                            })
                        })?;
                        IndexedBitmapBlitOutcome::Blitted
                    }
                };
                outcomes.push(ChampionshipSetupCommandOutcome::Bitmap {
                    command_index,
                    identity,
                    outcome,
                });
            }
            PreparedCommand::Deferred {
                command_index,
                command,
            } => {
                execute_deferred(command_index, command).map_err(|source| {
                    PreparedChampionshipSetupFrameExecutionError::Deferred {
                        command_index,
                        source,
                    }
                })?;
                outcomes.push(ChampionshipSetupCommandOutcome::Deferred {
                    command_index,
                    command,
                });
            }
        }
    }
    *preview_cache = prepared.next_cache;
    Ok(ChampionshipSetupFrameOutcome { commands: outcomes })
}

pub(crate) fn validate_championship_setup_frame_sources<B: IndexedVideoBackend>(
    prepared: &PreparedChampionshipSetupFrame,
    sources: &UploadedSourceStore<'_, B>,
) -> Result<(), ChampionshipSetupFrameExecutionError<B::Error>> {
    for command in &prepared.commands {
        match command {
            PreparedCommand::Text {
                command_index,
                command,
                text,
            } => {
                if text.requires_source_access() {
                    text.validate_source(sources).map_err(|source| {
                        ChampionshipSetupFrameExecutionError::TextAccess {
                            command_index: *command_index,
                            role: command.role,
                            source,
                        }
                    })?;
                }
            }
            PreparedCommand::Bitmap {
                command_index,
                identity,
                bitmap,
                decision,
            } => {
                if matches!(decision, BlitDecision::Blit(_)) {
                    bitmap.validate_source(sources).map_err(|source| {
                        ChampionshipSetupFrameExecutionError::BitmapAccess {
                            command_index: *command_index,
                            identity: *identity,
                            source,
                        }
                    })?;
                }
            }
            PreparedCommand::Deferred { .. } => {}
        }
    }
    Ok(())
}

fn prepare_frame_with_bitmaps<R: ChampionshipBitmapResources>(
    input: ChampionshipSetupVisualInput<'_>,
    state: ChampionshipSetupFrameState,
    text_table: &IndexedTextTable,
    fonts: &UploadedFontPool,
    resources: &R,
    next_cache: ChampionshipPreviewCache,
    text_limits: TextLayoutLimits,
) -> Result<PreparedChampionshipSetupFrame, ChampionshipSetupFramePrepareError> {
    let extent = input.viewport;
    prepare_frame_kernel(
        input,
        state,
        next_cache,
        |command_index, command| {
            let bytes = resolve_text(text_table, command.identity);
            prepare_persistent_text(
                fonts,
                bytes,
                command.font,
                PointI32 {
                    x: command.position.x,
                    y: command.position.y,
                },
                command.flags,
                extent,
                text_limits,
            )
            .map_err(|source| ChampionshipSetupFramePrepareError::Text {
                command_index,
                role: command.role,
                source,
            })
        },
        |identity| {
            let bitmap = match identity {
                ChampionshipBitmapIdentity::Selection => match resources.selection() {
                    Some(bitmap) => bitmap,
                    None => return Ok(None),
                },
                ChampionshipBitmapIdentity::InfoBox => resources.info_box(),
                ChampionshipBitmapIdentity::ManufacturerLogo(logo) => resources.logo(logo),
                ChampionshipBitmapIdentity::ChequeredFrame(frame) => resources.frame(frame),
            };
            Ok(Some((bitmap, bitmap.extent())))
        },
    )
}

fn prepare_frame_kernel<T, B>(
    input: ChampionshipSetupVisualInput<'_>,
    state: ChampionshipSetupFrameState,
    mut next_cache: ChampionshipPreviewCache,
    mut prepare_text: impl FnMut(
        usize,
        SetupTextCommand,
    ) -> Result<T, ChampionshipSetupFramePrepareError>,
    mut resolve_bitmap: impl FnMut(
        ChampionshipBitmapIdentity,
    ) -> Result<
        Option<(B, (u32, u32))>,
        ChampionshipSetupFramePrepareError,
    >,
) -> Result<PreparedChampionshipSetupFrame<T, B>, ChampionshipSetupFramePrepareError> {
    let base_cache = next_cache;
    let extent = input.viewport;
    let visual = plan_championship_setup_frame(input, &mut next_cache)
        .map_err(ChampionshipSetupFramePrepareError::Visual)?;
    let mut commands = Vec::with_capacity(visual.len());
    for (command_index, command) in visual.into_iter().enumerate() {
        match command {
            ChampionshipSetupVisualCommand::Text(command) => {
                let text = prepare_text(command_index, command)?;
                commands.push(PreparedCommand::Text {
                    command_index,
                    command,
                    text,
                });
            }
            ChampionshipSetupVisualCommand::Selection { .. } => {
                if let Some((bitmap, source_extent)) =
                    resolve_bitmap(ChampionshipBitmapIdentity::Selection)?
                {
                    commands.push(prepare_bitmap_payload(
                        command_index,
                        ChampionshipBitmapIdentity::Selection,
                        bitmap,
                        source_extent,
                        command,
                        extent,
                    )?);
                } else {
                    commands.push(PreparedCommand::Deferred {
                        command_index,
                        command: DeferredChampionshipCommand::Selection(command),
                    });
                }
            }
            ChampionshipSetupVisualCommand::InfoBox { .. } => {
                let identity = ChampionshipBitmapIdentity::InfoBox;
                let (bitmap, source_extent) =
                    required_bitmap(command_index, identity, resolve_bitmap(identity)?)?;
                commands.push(prepare_bitmap_payload(
                    command_index,
                    identity,
                    bitmap,
                    source_extent,
                    command,
                    extent,
                )?);
            }
            ChampionshipSetupVisualCommand::ManufacturerLogo { logo, .. } => {
                let identity = ChampionshipBitmapIdentity::ManufacturerLogo(logo);
                let (bitmap, source_extent) =
                    required_bitmap(command_index, identity, resolve_bitmap(identity)?)?;
                commands.push(prepare_bitmap_payload(
                    command_index,
                    identity,
                    bitmap,
                    source_extent,
                    command,
                    extent,
                )?);
            }
            ChampionshipSetupVisualCommand::DeferredNextPreview { .. } => {
                if let Some(frame) = state.chequered_frame {
                    let identity = ChampionshipBitmapIdentity::ChequeredFrame(frame);
                    let (bitmap, source_extent) =
                        required_bitmap(command_index, identity, resolve_bitmap(identity)?)?;
                    commands.push(prepare_bitmap_payload(
                        command_index,
                        identity,
                        bitmap,
                        source_extent,
                        command,
                        extent,
                    )?);
                } else {
                    commands.push(PreparedCommand::Deferred {
                        command_index,
                        command: DeferredChampionshipCommand::NextPreview(command),
                    });
                }
            }
            ChampionshipSetupVisualCommand::DeferredInfoText(info) => {
                commands.push(PreparedCommand::Deferred {
                    command_index,
                    command: DeferredChampionshipCommand::InfoText(info),
                })
            }
            ChampionshipSetupVisualCommand::DeferredCarTransform { .. } => {
                commands.push(PreparedCommand::Deferred {
                    command_index,
                    command: DeferredChampionshipCommand::CarTransform(command),
                })
            }
            ChampionshipSetupVisualCommand::DeferredCarVisibilityLatch { .. } => {
                commands.push(PreparedCommand::Deferred {
                    command_index,
                    command: DeferredChampionshipCommand::CarVisibilityLatch(command),
                })
            }
            ChampionshipSetupVisualCommand::PaletteRequest { group, item } => {
                commands.push(PreparedCommand::Deferred {
                    command_index,
                    command: DeferredChampionshipCommand::PaletteRequest { group, item },
                })
            }
        }
    }
    Ok(PreparedChampionshipSetupFrame {
        commands,
        base_cache,
        next_cache,
    })
}

fn required_bitmap<B>(
    command_index: usize,
    identity: ChampionshipBitmapIdentity,
    resolved: Option<(B, (u32, u32))>,
) -> Result<(B, (u32, u32)), ChampionshipSetupFramePrepareError> {
    resolved.ok_or(ChampionshipSetupFramePrepareError::MissingBitmapExtent {
        command_index,
        identity,
    })
}

fn resolve_text(table: &IndexedTextTable, identity: SetupTextIdentity) -> &[u8] {
    match identity {
        SetupTextIdentity::Localized(index) => table.lookup(index),
        SetupTextIdentity::Literal(text) => text.as_bytes(),
        SetupTextIdentity::DeferredCarSpecification(_) => {
            unreachable!("car specifications occur only in deferred info-text commands")
        }
    }
}

fn prepare_bitmap_payload<T, B>(
    command_index: usize,
    identity: ChampionshipBitmapIdentity,
    bitmap: B,
    source_extent: (u32, u32),
    command: ChampionshipSetupVisualCommand,
    extent: DisplayExtent,
) -> Result<PreparedCommand<T, B>, ChampionshipSetupFramePrepareError> {
    let (source, destination, color_key) = match command {
        ChampionshipSetupVisualCommand::Selection {
            source,
            destination,
            color_key,
            ..
        } => (
            source,
            destination,
            Some(ColorKey {
                low: u32::from(color_key),
                high: u32::from(color_key),
            }),
        ),
        ChampionshipSetupVisualCommand::InfoBox {
            source,
            destination,
        }
        | ChampionshipSetupVisualCommand::ManufacturerLogo {
            source,
            destination,
            ..
        }
        | ChampionshipSetupVisualCommand::DeferredNextPreview {
            source,
            destination,
            ..
        } => (source, destination, None),
        _ => unreachable!("only bitmap visual commands are prepared as bitmaps"),
    };
    let decision = plan_indexed_bitmap_blit_for_extent(
        source_extent,
        indexed_rect(source),
        indexed_rect(destination),
        extent,
        color_key,
    )
    .map_err(|source| ChampionshipSetupFramePrepareError::Bitmap {
        command_index,
        identity,
        source,
    })?;
    Ok(PreparedCommand::Bitmap {
        command_index,
        identity,
        bitmap,
        decision,
    })
}

const fn indexed_rect(rectangle: PixelRect) -> IndexedBitmapRect {
    IndexedBitmapRect::new(rectangle.x, rectangle.y, rectangle.width, rectangle.height)
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use cmr_formats::font::{FontLimits, parse_legacy_font};
    use cmr_formats::riff_palette::PaletteEntry;

    use super::*;
    use crate::blit::{BlitMethod, BlitPlan};
    use crate::championship_setup::{
        ActivePreview, CAR_ROW_TAG, CanonicalCar, ChampionshipSetupRenderTranscript, GEAR_ROW_TAG,
        GearChoice, NEXT_ROW_TAG, RenderRow, RowValue, SetupRowText,
    };
    use crate::championship_setup_visual::{NextPreviewContext, Q16Point, RowPresentation};
    use crate::display::{CooperativeMode, DisplaySurfaces, IndexedMode, SurfaceTopology};
    use crate::font_assets::FontSet;
    use crate::surface::IndexedSurface;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum BackendError {
        Blit(u16),
    }

    impl fmt::Display for BackendError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "{self:?}")
        }
    }

    impl std::error::Error for BackendError {}

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum ExecutionEvent {
        Blit(u16),
        Deferred(usize),
    }

    #[derive(Default)]
    struct RecordingBackend {
        next_source: u16,
        blits: Vec<u16>,
        fail_source: Option<u16>,
        events: Rc<RefCell<Vec<ExecutionEvent>>>,
    }

    impl IndexedVideoBackend for RecordingBackend {
        type Error = BackendError;
        type Surface = u16;
        type Palette = u8;

        fn create_device(&mut self) -> Result<(), Self::Error> {
            unreachable!()
        }

        fn set_cooperative_mode(&mut self, _: CooperativeMode) -> Result<(), Self::Error> {
            unreachable!()
        }

        fn set_display_mode(&mut self, _: IndexedMode) -> Result<(), Self::Error> {
            unreachable!()
        }

        fn create_surfaces(
            &mut self,
            _: SurfaceTopology,
            _: IndexedMode,
        ) -> Result<DisplaySurfaces<Self::Surface>, Self::Error> {
            unreachable!()
        }

        fn create_palette(
            &mut self,
            _: &[PaletteEntry; 256],
        ) -> Result<Self::Palette, Self::Error> {
            unreachable!()
        }

        fn attach_palette(
            &mut self,
            _: Self::Surface,
            _: Self::Palette,
        ) -> Result<(), Self::Error> {
            unreachable!()
        }

        fn upload_palette(
            &mut self,
            _: Self::Palette,
            _: &[PaletteEntry; 256],
        ) -> Result<(), Self::Error> {
            unreachable!()
        }

        fn create_source(&mut self, _: &IndexedSurface) -> Result<Self::Surface, Self::Error> {
            let source = self.next_source;
            self.next_source += 1;
            Ok(source)
        }

        fn blit(
            &mut self,
            source: Self::Surface,
            _: Self::Surface,
            _: BlitPlan,
        ) -> Result<(), Self::Error> {
            self.blits.push(source);
            self.events.borrow_mut().push(ExecutionEvent::Blit(source));
            if self.fail_source == Some(source) {
                Err(BackendError::Blit(source))
            } else {
                Ok(())
            }
        }

        fn destroy_source(&mut self, _: Self::Surface) {}
    }

    #[derive(Clone, Copy)]
    struct TestBitmaps {
        selection: Option<UploadedIndexedBitmap>,
        info: UploadedIndexedBitmap,
        logo: UploadedIndexedBitmap,
        frame: UploadedIndexedBitmap,
    }

    impl ChampionshipBitmapResources for TestBitmaps {
        fn selection(&self) -> Option<UploadedIndexedBitmap> {
            self.selection
        }

        fn info_box(&self) -> UploadedIndexedBitmap {
            self.info
        }

        fn logo(&self, _: ManufacturerLogo) -> UploadedIndexedBitmap {
            self.logo
        }

        fn frame(&self, _: ChequeredFrame) -> UploadedIndexedBitmap {
            self.frame
        }
    }

    #[test]
    fn next_frame_executes_available_work_in_exact_visual_order() {
        #[derive(Debug, Eq, PartialEq)]
        enum Observed {
            Text(SetupTextRole),
            Bitmap(ChampionshipBitmapIdentity),
            InfoText,
            PaletteRequest,
        }

        let transcript = transcript(2);
        let mut fixture = Fixture::new(true);
        let mut cache = ChampionshipPreviewCache::new();
        let outcome = fixture
            .execute(
                input(&transcript),
                Some(ChequeredFrame::Frame04),
                &mut cache,
            )
            .unwrap();

        let order = outcome
            .commands
            .iter()
            .map(|outcome| match outcome {
                ChampionshipSetupCommandOutcome::Text { role, .. } => Observed::Text(*role),
                ChampionshipSetupCommandOutcome::Bitmap { identity, .. } => {
                    Observed::Bitmap(*identity)
                }
                ChampionshipSetupCommandOutcome::Deferred {
                    command: DeferredChampionshipCommand::InfoText(_),
                    ..
                } => Observed::InfoText,
                ChampionshipSetupCommandOutcome::Deferred {
                    command: DeferredChampionshipCommand::PaletteRequest { .. },
                    ..
                } => Observed::PaletteRequest,
                ChampionshipSetupCommandOutcome::Deferred { command, .. } => {
                    panic!("unexpected deferred command {command:?}")
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(
            order,
            [
                Observed::Text(SetupTextRole::Title),
                Observed::Text(SetupTextRole::Row(
                    crate::championship_setup::SetupRowKind::ChooseCar
                )),
                Observed::Text(SetupTextRole::Row(
                    crate::championship_setup::SetupRowKind::ChooseGear
                )),
                Observed::Bitmap(ChampionshipBitmapIdentity::Selection),
                Observed::Text(SetupTextRole::Row(
                    crate::championship_setup::SetupRowKind::Next
                )),
                Observed::Bitmap(ChampionshipBitmapIdentity::InfoBox),
                Observed::InfoText,
                Observed::PaletteRequest,
                Observed::Bitmap(ChampionshipBitmapIdentity::ChequeredFrame(
                    ChequeredFrame::Frame04
                )),
            ]
        );
        assert!(matches!(
            outcome.commands[3],
            ChampionshipSetupCommandOutcome::Bitmap {
                command_index: 3,
                ..
            }
        ));
        assert!(matches!(
            outcome.commands[8],
            ChampionshipSetupCommandOutcome::Bitmap {
                command_index: 8,
                ..
            }
        ));
    }

    #[test]
    fn prepared_deferred_callback_runs_inline_between_bitmap_commands() {
        let transcript = transcript(2);
        let mut fixture = Fixture::new(true);
        let selection = fixture.bitmaps.selection.unwrap().source_key().slot() as u16;
        let info = fixture.bitmaps.info.source_key().slot() as u16;
        let frame = fixture.bitmaps.frame.source_key().slot() as u16;
        let mut cache = ChampionshipPreviewCache::new();
        let prepared = prepare_frame_with_bitmaps(
            input(&transcript),
            ChampionshipSetupFrameState {
                chequered_frame: Some(ChequeredFrame::Frame04),
            },
            &fixture.text,
            &fixture.fonts,
            &fixture.bitmaps,
            cache,
            TextLayoutLimits::default(),
        )
        .unwrap();

        assert_eq!(
            prepared
                .deferred_commands()
                .map(|(index, _)| index)
                .collect::<Vec<_>>(),
            [6, 7]
        );
        validate_championship_setup_frame_sources(&prepared, &fixture.store).unwrap();

        let events = Rc::clone(&fixture.store.backend().events);
        let mut callbacks = Vec::new();
        execute_prepared_championship_setup_frame(
            prepared,
            &mut cache,
            &mut fixture.store,
            999,
            |command_index, command| {
                events
                    .borrow_mut()
                    .push(ExecutionEvent::Deferred(command_index));
                callbacks.push((command_index, command));
                Ok::<_, std::convert::Infallible>(())
            },
        )
        .unwrap();

        assert!(fixture.store.backend().blits.contains(&selection));
        assert!(fixture.store.backend().blits.contains(&info));
        assert!(fixture.store.backend().blits.contains(&frame));
        assert!(matches!(
            callbacks.as_slice(),
            [
                (6, DeferredChampionshipCommand::InfoText(_)),
                (
                    7,
                    DeferredChampionshipCommand::PaletteRequest { group: 1, item: 9 }
                )
            ]
        ));
        let events = fixture.store.backend().events.borrow();
        let event_position = |needle| events.iter().position(|event| *event == needle).unwrap();
        assert!(
            event_position(ExecutionEvent::Blit(selection))
                < event_position(ExecutionEvent::Deferred(6))
        );
        assert!(
            event_position(ExecutionEvent::Blit(info))
                < event_position(ExecutionEvent::Deferred(6))
        );
        assert!(
            event_position(ExecutionEvent::Deferred(6))
                < event_position(ExecutionEvent::Deferred(7))
        );
        assert!(
            event_position(ExecutionEvent::Deferred(7))
                < event_position(ExecutionEvent::Blit(frame))
        );
    }

    #[test]
    fn deferred_callback_failure_stops_later_bitmap_and_cache_commit() {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        struct DeferredFailure;

        let transcript = transcript(2);
        let mut fixture = Fixture::new(true);
        let frame = fixture.bitmaps.frame.source_key().slot() as u16;
        let mut cache = ChampionshipPreviewCache::new();
        let prepared = prepare_frame_with_bitmaps(
            input(&transcript),
            ChampionshipSetupFrameState {
                chequered_frame: Some(ChequeredFrame::Frame04),
            },
            &fixture.text,
            &fixture.fonts,
            &fixture.bitmaps,
            cache,
            TextLayoutLimits::default(),
        )
        .unwrap();
        validate_championship_setup_frame_sources(&prepared, &fixture.store).unwrap();

        let error = execute_prepared_championship_setup_frame(
            prepared,
            &mut cache,
            &mut fixture.store,
            999,
            |command_index, _| {
                if command_index == 6 {
                    Err(DeferredFailure)
                } else {
                    Ok(())
                }
            },
        )
        .unwrap_err();

        assert_eq!(
            error,
            PreparedChampionshipSetupFrameExecutionError::Deferred {
                command_index: 6,
                source: DeferredFailure,
            }
        );
        assert!(!fixture.store.backend().blits.contains(&frame));
        assert_eq!(cache, ChampionshipPreviewCache::new());
    }

    #[test]
    fn compatibility_wrapper_matches_prepared_execution_with_noop_callback() {
        let transcript = transcript(2);
        let mut wrapper_fixture = Fixture::new(true);
        let mut prepared_fixture = Fixture::new(true);
        let mut wrapper_cache = ChampionshipPreviewCache::new();
        let mut prepared_cache = ChampionshipPreviewCache::new();

        let wrapper_outcome = wrapper_fixture
            .execute(
                input(&transcript),
                Some(ChequeredFrame::Frame04),
                &mut wrapper_cache,
            )
            .unwrap();
        let prepared = prepare_frame_with_bitmaps(
            input(&transcript),
            ChampionshipSetupFrameState {
                chequered_frame: Some(ChequeredFrame::Frame04),
            },
            &prepared_fixture.text,
            &prepared_fixture.fonts,
            &prepared_fixture.bitmaps,
            prepared_cache,
            TextLayoutLimits::default(),
        )
        .unwrap();
        validate_championship_setup_frame_sources(&prepared, &prepared_fixture.store).unwrap();
        let prepared_outcome = execute_prepared_championship_setup_frame(
            prepared,
            &mut prepared_cache,
            &mut prepared_fixture.store,
            999,
            |_, _| Ok::<_, std::convert::Infallible>(()),
        )
        .unwrap();

        assert_eq!(prepared_outcome, wrapper_outcome);
        assert_eq!(prepared_cache, wrapper_cache);
        assert_eq!(
            prepared_fixture.store.backend().blits,
            wrapper_fixture.store.backend().blits
        );
    }

    #[test]
    fn missing_selection_and_animation_state_remain_typed_deferred_work() {
        let transcript = transcript(2);
        let mut fixture = Fixture::new(false);
        let mut cache = ChampionshipPreviewCache::new();
        let outcome = fixture
            .execute(input(&transcript), None, &mut cache)
            .unwrap();

        assert!(matches!(
            outcome.commands[3],
            ChampionshipSetupCommandOutcome::Deferred {
                command: DeferredChampionshipCommand::Selection(_),
                ..
            }
        ));
        assert!(matches!(
            outcome.commands[8],
            ChampionshipSetupCommandOutcome::Deferred {
                command: DeferredChampionshipCommand::NextPreview(_),
                ..
            }
        ));
    }

    #[test]
    fn whole_frame_preflight_rejects_late_bitmap_geometry_without_effect_or_cache_commit() {
        let transcript = transcript(2);
        let mut fixture = Fixture::new(true);
        // A valid 182x128 frame is required only by the final command. Rebind
        // it to a smaller uploaded source so the failure occurs late in preflight.
        fixture.bitmaps.frame = fixture.store.upload_bitmap(&surface(100, 100)).unwrap();
        let blits_before = fixture.store.backend().blits.len();
        let mut cache = ChampionshipPreviewCache::new();
        let error = fixture
            .execute(
                input(&transcript),
                Some(ChequeredFrame::Frame01),
                &mut cache,
            )
            .unwrap_err();

        assert!(matches!(
            error,
            ChampionshipSetupFrameExecutionError::Prepare(
                ChampionshipSetupFramePrepareError::Bitmap {
                    command_index: 8,
                    identity: ChampionshipBitmapIdentity::ChequeredFrame(ChequeredFrame::Frame01),
                    source: IndexedBitmapPlanError::SourceOutOfBounds { .. },
                }
            )
        ));
        assert_eq!(fixture.store.backend().blits.len(), blits_before);
        assert_eq!(cache, ChampionshipPreviewCache::new());
    }

    #[test]
    fn late_foreign_source_is_rejected_before_any_effect_or_cache_commit() {
        let transcript = transcript(2);
        let mut fixture = Fixture::new(true);
        let mut foreign_backend = RecordingBackend::default();
        let mut foreign_store = UploadedSourceStore::new(&mut foreign_backend);
        fixture.bitmaps.frame = foreign_store.upload_bitmap(&surface(184, 128)).unwrap();
        let mut cache = ChampionshipPreviewCache::new();

        let error = fixture
            .execute(
                input(&transcript),
                Some(ChequeredFrame::Frame15),
                &mut cache,
            )
            .unwrap_err();

        assert!(matches!(
            error,
            ChampionshipSetupFrameExecutionError::BitmapAccess {
                command_index: 8,
                identity: ChampionshipBitmapIdentity::ChequeredFrame(ChequeredFrame::Frame15),
                source: SourceAccessError::ForeignKey(_),
            }
        ));
        assert!(fixture.store.backend().blits.is_empty());
        assert_eq!(cache, ChampionshipPreviewCache::new());
    }

    #[test]
    fn late_released_source_is_rejected_before_any_effect_or_cache_commit() {
        let transcript = transcript(2);
        let mut fixture = Fixture::new(true);
        let released = fixture.bitmaps.frame.source_key();
        fixture.store.release(released).unwrap();
        let mut cache = ChampionshipPreviewCache::new();

        let error = fixture
            .execute(
                input(&transcript),
                Some(ChequeredFrame::Frame15),
                &mut cache,
            )
            .unwrap_err();

        assert!(matches!(
            error,
            ChampionshipSetupFrameExecutionError::BitmapAccess {
                command_index: 8,
                identity: ChampionshipBitmapIdentity::ChequeredFrame(ChequeredFrame::Frame15),
                source: SourceAccessError::ReleasedKey(key),
            } if key == released
        ));
        assert!(fixture.store.backend().blits.is_empty());
        assert_eq!(cache, ChampionshipPreviewCache::new());
    }

    #[test]
    fn released_no_op_bitmap_succeeds_without_observing_its_source() {
        let transcript = transcript(2);
        let mut fixture = Fixture::new(true);
        let released = fixture.bitmaps.selection.unwrap().source_key();
        fixture.store.release(released).unwrap();
        let mut visual_input = input(&transcript);
        visual_input.row_anchors_q16[2].x = -0x4000;
        let mut cache = ChampionshipPreviewCache::new();

        let outcome = fixture
            .execute(visual_input, Some(ChequeredFrame::Frame01), &mut cache)
            .unwrap();

        assert!(matches!(
            outcome.commands[3],
            ChampionshipSetupCommandOutcome::Bitmap {
                identity: ChampionshipBitmapIdentity::Selection,
                outcome: IndexedBitmapBlitOutcome::NoOp,
                ..
            }
        ));
    }

    #[test]
    fn foreign_no_op_bitmap_succeeds_without_observing_its_source() {
        let transcript = transcript(2);
        let mut fixture = Fixture::new(true);
        let mut foreign_backend = RecordingBackend::default();
        let mut foreign_store = UploadedSourceStore::new(&mut foreign_backend);
        fixture.bitmaps.frame = foreign_store.upload_bitmap(&surface(184, 128)).unwrap();
        let mut visual_input = input(&transcript);
        visual_input.preview_anchor_q16.x = -0x999a;
        let mut cache = ChampionshipPreviewCache::new();

        let outcome = fixture
            .execute(visual_input, Some(ChequeredFrame::Frame15), &mut cache)
            .unwrap();

        assert!(matches!(
            outcome.commands[8],
            ChampionshipSetupCommandOutcome::Bitmap {
                identity: ChampionshipBitmapIdentity::ChequeredFrame(ChequeredFrame::Frame15),
                outcome: IndexedBitmapBlitOutcome::NoOp,
                ..
            }
        ));
        assert!(foreign_store.backend().blits.is_empty());
    }

    #[test]
    fn foreign_zero_submission_fonts_succeed_without_observing_their_sources() {
        let transcript = transcript(2);
        let mut fixture = Fixture::new(true);
        let mut foreign_backend = RecordingBackend::default();
        let mut foreign_store = UploadedSourceStore::new(&mut foreign_backend);
        let foreign_fonts = UploadedFontPool::upload(&mut foreign_store, font_set()).unwrap();
        let mut visual_input = input(&transcript);
        visual_input.frontend_offset_q16.x = -0x7_0000;
        let mut cache = ChampionshipPreviewCache::new();

        let outcome = execute_frame_with_bitmaps(
            visual_input,
            ChampionshipSetupFrameState {
                chequered_frame: Some(ChequeredFrame::Frame01),
            },
            &fixture.text,
            &foreign_fonts,
            &fixture.bitmaps,
            &mut cache,
            &mut fixture.store,
            999,
            TextLayoutLimits::default(),
        )
        .unwrap();

        assert!(outcome.commands.iter().all(|command| {
            !matches!(
                command,
                ChampionshipSetupCommandOutcome::Text { rendered, .. }
                    if rendered.raster.successful_blits != 0
            )
        }));
        assert!(foreign_store.backend().blits.is_empty());
    }

    #[test]
    fn fatal_bitmap_failure_stops_at_boundary_and_does_not_commit_cache() {
        let transcript = transcript(2);
        let mut fixture = Fixture::new(true);
        let selection_source = fixture.bitmaps.selection.unwrap().source_key().slot() as u16;
        // Handles are allocated sequentially from zero, matching source slots.
        fixture.store.backend_mut().fail_source = Some(selection_source);
        let mut cache = ChampionshipPreviewCache::new();
        let error = fixture
            .execute(
                input(&transcript),
                Some(ChequeredFrame::Frame01),
                &mut cache,
            )
            .unwrap_err();

        assert!(matches!(
            error,
            ChampionshipSetupFrameExecutionError::BitmapBackend {
                command_index: 3,
                identity: ChampionshipBitmapIdentity::Selection,
                source: BackendError::Blit(_),
            }
        ));
        assert_eq!(cache, ChampionshipPreviewCache::new());
        assert!(
            fixture
                .store
                .backend()
                .blits
                .iter()
                .any(|source| usize::from(*source) < crate::font_assets::FONT_COUNT)
        );
        assert!(
            !fixture
                .store
                .backend()
                .blits
                .contains(&fixture.bitmaps.info.source_key().slot().try_into().unwrap())
        );
    }

    #[test]
    fn successful_cache_commit_suppresses_repeated_palette_request_only() {
        let transcript = transcript(2);
        let mut fixture = Fixture::new(true);
        let mut cache = ChampionshipPreviewCache::new();
        let first = fixture
            .execute(
                input(&transcript),
                Some(ChequeredFrame::Frame02),
                &mut cache,
            )
            .unwrap();
        let second = fixture
            .execute(
                input(&transcript),
                Some(ChequeredFrame::Frame03),
                &mut cache,
            )
            .unwrap();

        assert_eq!(first.commands.len(), 9);
        assert_eq!(second.commands.len(), 8);
        assert!(first.commands.iter().any(|outcome| matches!(
            outcome,
            ChampionshipSetupCommandOutcome::Deferred {
                command: DeferredChampionshipCommand::PaletteRequest { group: 1, item: 9 },
                ..
            }
        )));
        assert!(!second.commands.iter().any(|outcome| matches!(
            outcome,
            ChampionshipSetupCommandOutcome::Deferred {
                command: DeferredChampionshipCommand::PaletteRequest { .. },
                ..
            }
        )));
        assert!(matches!(
            second.commands.last(),
            Some(ChampionshipSetupCommandOutcome::Bitmap {
                identity: ChampionshipBitmapIdentity::ChequeredFrame(ChequeredFrame::Frame03),
                ..
            })
        ));
    }

    #[test]
    fn car_preview_executes_info_and_logo_but_keeps_c3d_and_info_text_deferred() {
        let transcript = transcript(0);
        let mut fixture = Fixture::new(true);
        let mut cache = ChampionshipPreviewCache::new();
        let outcome = fixture
            .execute(input(&transcript), None, &mut cache)
            .unwrap();

        let identities = outcome
            .commands
            .iter()
            .filter_map(|outcome| match outcome {
                ChampionshipSetupCommandOutcome::Bitmap { identity, .. } => Some(*identity),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            identities,
            [
                ChampionshipBitmapIdentity::Selection,
                ChampionshipBitmapIdentity::InfoBox,
                ChampionshipBitmapIdentity::ManufacturerLogo(ManufacturerLogo::Subaru),
            ]
        );
        assert!(outcome.commands.iter().any(|outcome| matches!(
            outcome,
            ChampionshipSetupCommandOutcome::Deferred {
                command: DeferredChampionshipCommand::CarTransform(_),
                ..
            }
        )));
        assert!(outcome.commands.iter().any(|outcome| matches!(
            outcome,
            ChampionshipSetupCommandOutcome::Deferred {
                command: DeferredChampionshipCommand::CarVisibilityLatch(_),
                ..
            }
        )));
    }

    struct Fixture<'a> {
        text: IndexedTextTable,
        fonts: UploadedFontPool,
        bitmaps: TestBitmaps,
        store: UploadedSourceStore<'a, RecordingBackend>,
    }

    impl Fixture<'_> {
        fn new(with_selection: bool) -> Self {
            let backend = Box::leak(Box::new(RecordingBackend::default()));
            let mut store = UploadedSourceStore::new(backend);
            let fonts = UploadedFontPool::upload(&mut store, font_set()).unwrap();
            let selection =
                with_selection.then(|| store.upload_bitmap(&surface(256, 256)).unwrap());
            let info = store.upload_bitmap(&surface(320, 132)).unwrap();
            let logo = store.upload_bitmap(&surface(184, 128)).unwrap();
            let frame = store.upload_bitmap(&surface(184, 128)).unwrap();
            let records = b"A\n".repeat(0xfe);
            Self {
                text: IndexedTextTable::parse(&records, 0xfe).unwrap(),
                fonts,
                bitmaps: TestBitmaps {
                    selection,
                    info,
                    logo,
                    frame,
                },
                store,
            }
        }

        fn execute(
            &mut self,
            input: ChampionshipSetupVisualInput<'_>,
            chequered_frame: Option<ChequeredFrame>,
            cache: &mut ChampionshipPreviewCache,
        ) -> Result<
            ChampionshipSetupFrameOutcome<BackendError>,
            ChampionshipSetupFrameExecutionError<BackendError>,
        > {
            execute_frame_with_bitmaps(
                input,
                ChampionshipSetupFrameState { chequered_frame },
                &self.text,
                &self.fonts,
                &self.bitmaps,
                cache,
                &mut self.store,
                999,
                TextLayoutLimits::default(),
            )
        }
    }

    fn transcript(selected: usize) -> ChampionshipSetupRenderTranscript {
        let car = CanonicalCar::Subaru;
        let gear = GearChoice::BitClear;
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
                    value: RowValue::Gear(gear),
                },
                RenderRow {
                    tag: NEXT_ROW_TAG,
                    text: SetupRowText::Localized(0x18),
                    selected: selected == 2,
                    value: RowValue::None,
                },
            ],
            highlight_row: selected,
            preview: match selected {
                0 => ActivePreview::Car {
                    canonical_car: car,
                    object_name: car.object_name(),
                    logo_name: car.logo_name(),
                },
                1 => ActivePreview::Gear {
                    gear,
                    canonical_car: car,
                    object_name: car.object_name(),
                    logo_name: car.logo_name(),
                },
                _ => ActivePreview::Next,
            },
        }
    }

    fn input(transcript: &ChampionshipSetupRenderTranscript) -> ChampionshipSetupVisualInput<'_> {
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
            transcript,
            next_context: NextPreviewContext {
                state_byte: 0,
                mode_byte: 0,
            },
        }
    }

    fn font_set() -> FontSet {
        FontSet::from_ordered_fonts(std::array::from_fn(|index| {
            parse_legacy_font(&synthetic_font(index as u8), FontLimits::default()).unwrap()
        }))
    }

    fn synthetic_font(identity: u8) -> Vec<u8> {
        const PIXEL_OFFSET: usize = 0x436;
        const FONT_HEADER_SIZE: usize = 24;
        const GLYPH_SIZE: usize = 16;
        let trailer = PIXEL_OFFSET + 4;
        let mut input = vec![0; trailer + FONT_HEADER_SIZE + GLYPH_SIZE];
        input[0..2].copy_from_slice(b"BM");
        input[18..22].copy_from_slice(&1i32.to_le_bytes());
        input[22..26].copy_from_slice(&1i32.to_le_bytes());
        input[28..30].copy_from_slice(&8u16.to_le_bytes());
        input[PIXEL_OFFSET] = identity + 1;
        input[trailer] = identity;
        input[trailer + 1] = 1;
        input[trailer + 4..trailer + 8].copy_from_slice(&1u32.to_le_bytes());
        input[trailer + 12..trailer + 16].copy_from_slice(&1u32.to_le_bytes());
        let glyph = trailer + FONT_HEADER_SIZE;
        input[glyph] = b'A';
        input[glyph + 6..glyph + 8].copy_from_slice(&1i16.to_le_bytes());
        input[glyph + 8..glyph + 10].copy_from_slice(&1i16.to_le_bytes());
        input
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

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum PreparedDecision {
        NoOp,
        Blit(BlitMethod),
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum DeferredKind {
        Selection,
        InfoText,
        CarTransform(CanonicalCar),
        CarVisibilityLatch(CanonicalCar),
        PaletteRequest { group: u8, item: u8 },
        NextPreview,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum PreparedKind {
        Text(SetupTextRole),
        Bitmap(ChampionshipBitmapIdentity, PreparedDecision),
        Deferred(DeferredKind),
    }

    fn prepared_kinds(prepared: &PreparedChampionshipSetupFrame) -> Vec<PreparedKind> {
        assert_eq!(prepared.commands().len(), prepared.command_len());
        prepared
            .commands()
            .enumerate()
            .map(|(expected_index, command)| match command {
                PreparedChampionshipSetupCommand::Text {
                    command_index,
                    command,
                } => {
                    assert_eq!(command_index, expected_index);
                    PreparedKind::Text(command.role)
                }
                PreparedChampionshipSetupCommand::Bitmap {
                    command_index,
                    identity,
                    decision,
                } => {
                    assert_eq!(command_index, expected_index);
                    PreparedKind::Bitmap(
                        identity,
                        match decision {
                            BlitDecision::NoOp => PreparedDecision::NoOp,
                            BlitDecision::Blit(plan) => PreparedDecision::Blit(plan.method),
                        },
                    )
                }
                PreparedChampionshipSetupCommand::Deferred {
                    command_index,
                    command,
                } => {
                    assert_eq!(command_index, expected_index);
                    PreparedKind::Deferred(match command {
                        DeferredChampionshipCommand::Selection(_) => DeferredKind::Selection,
                        DeferredChampionshipCommand::InfoText(_) => DeferredKind::InfoText,
                        DeferredChampionshipCommand::CarTransform(
                            ChampionshipSetupVisualCommand::DeferredCarTransform { car, .. },
                        ) => DeferredKind::CarTransform(car),
                        DeferredChampionshipCommand::CarVisibilityLatch(
                            ChampionshipSetupVisualCommand::DeferredCarVisibilityLatch {
                                car, ..
                            },
                        ) => DeferredKind::CarVisibilityLatch(car),
                        DeferredChampionshipCommand::PaletteRequest { group, item } => {
                            DeferredKind::PaletteRequest { group, item }
                        }
                        DeferredChampionshipCommand::NextPreview(_) => DeferredKind::NextPreview,
                        DeferredChampionshipCommand::CarTransform(command)
                        | DeferredChampionshipCommand::CarVisibilityLatch(command) => {
                            panic!("unexpected deferred command shape {command:?}")
                        }
                    })
                }
            })
            .collect()
    }

    fn prepare_fixture_frame(
        fixture: &Fixture<'_>,
        transcript: &ChampionshipSetupRenderTranscript,
        chequered_frame: Option<ChequeredFrame>,
        cache: ChampionshipPreviewCache,
    ) -> PreparedChampionshipSetupFrame {
        prepare_frame_with_bitmaps(
            input(transcript),
            ChampionshipSetupFrameState { chequered_frame },
            &fixture.text,
            &fixture.fonts,
            &fixture.bitmaps,
            cache,
            TextLayoutLimits::default(),
        )
        .unwrap()
    }

    fn prepare_fixture_frame_from_extents(
        fixture: &Fixture<'_>,
        transcript: &ChampionshipSetupRenderTranscript,
        chequered_frame: Option<ChequeredFrame>,
        cache: ChampionshipPreviewCache,
    ) -> PreparedChampionshipAlternateSetupFrame {
        prepare_championship_setup_frame_from_source_extents(
            input(transcript),
            ChampionshipSetupFrameState { chequered_frame },
            cache,
            |identity| match identity {
                ChampionshipBitmapIdentity::Selection => {
                    fixture.bitmaps.selection.map(UploadedIndexedBitmap::extent)
                }
                ChampionshipBitmapIdentity::InfoBox => Some(fixture.bitmaps.info.extent()),
                ChampionshipBitmapIdentity::ManufacturerLogo(_) => {
                    Some(fixture.bitmaps.logo.extent())
                }
                ChampionshipBitmapIdentity::ChequeredFrame(_) => {
                    Some(fixture.bitmaps.frame.extent())
                }
            },
        )
        .unwrap()
    }

    fn assert_rich_and_extent_preparation_match(
        fixture: &Fixture<'_>,
        transcript: &ChampionshipSetupRenderTranscript,
        chequered_frame: Option<ChequeredFrame>,
        cache: ChampionshipPreviewCache,
    ) {
        let rich = prepare_fixture_frame(fixture, transcript, chequered_frame, cache);
        let extent_only =
            prepare_fixture_frame_from_extents(fixture, transcript, chequered_frame, cache);
        assert_eq!(
            rich.commands().collect::<Vec<_>>(),
            extent_only.commands().collect::<Vec<_>>()
        );
    }

    fn commit_fixture_frame(
        fixture: &mut Fixture<'_>,
        prepared: PreparedChampionshipSetupFrame,
        cache: &mut ChampionshipPreviewCache,
    ) {
        validate_championship_setup_frame_sources(&prepared, &fixture.store).unwrap();
        execute_prepared_championship_setup_frame(
            prepared,
            cache,
            &mut fixture.store,
            999,
            |_, _| Ok::<_, std::convert::Infallible>(()),
        )
        .unwrap();
    }

    #[test]
    fn public_prepared_transcript_has_exact_car_gear_and_next_cold_warm_order() {
        use PreparedDecision::Blit;

        let mut fixture = Fixture::new(true);
        let subaru_logo = CanonicalCar::Subaru.championship_descriptor().logo;
        let car_cold_expected = [
            PreparedKind::Text(SetupTextRole::Title),
            PreparedKind::Bitmap(
                ChampionshipBitmapIdentity::Selection,
                Blit(BlitMethod::Stretch),
            ),
            PreparedKind::Text(SetupTextRole::Row(
                crate::championship_setup::SetupRowKind::ChooseCar,
            )),
            PreparedKind::Bitmap(ChampionshipBitmapIdentity::InfoBox, Blit(BlitMethod::Fast)),
            PreparedKind::Deferred(DeferredKind::InfoText),
            PreparedKind::Deferred(DeferredKind::CarTransform(CanonicalCar::Subaru)),
            PreparedKind::Deferred(DeferredKind::CarVisibilityLatch(CanonicalCar::Subaru)),
            PreparedKind::Bitmap(
                ChampionshipBitmapIdentity::ManufacturerLogo(subaru_logo),
                Blit(BlitMethod::Fast),
            ),
            PreparedKind::Deferred(DeferredKind::PaletteRequest { group: 4, item: 0 }),
            PreparedKind::Text(SetupTextRole::Row(
                crate::championship_setup::SetupRowKind::ChooseGear,
            )),
            PreparedKind::Text(SetupTextRole::Row(
                crate::championship_setup::SetupRowKind::Next,
            )),
        ];
        let car = transcript(0);
        let mut car_cache = ChampionshipPreviewCache::new();
        let car_cold = prepare_fixture_frame(&fixture, &car, None, car_cache);
        assert_eq!(prepared_kinds(&car_cold), car_cold_expected);
        commit_fixture_frame(&mut fixture, car_cold, &mut car_cache);
        let car_warm = prepare_fixture_frame(&fixture, &car, None, car_cache);
        let mut car_warm_expected = car_cold_expected.to_vec();
        car_warm_expected.remove(8);
        assert_eq!(prepared_kinds(&car_warm), car_warm_expected);

        let gear_cold_expected = [
            PreparedKind::Text(SetupTextRole::Title),
            PreparedKind::Text(SetupTextRole::Row(
                crate::championship_setup::SetupRowKind::ChooseCar,
            )),
            PreparedKind::Bitmap(
                ChampionshipBitmapIdentity::Selection,
                Blit(BlitMethod::Stretch),
            ),
            PreparedKind::Text(SetupTextRole::Row(
                crate::championship_setup::SetupRowKind::ChooseGear,
            )),
            PreparedKind::Bitmap(ChampionshipBitmapIdentity::InfoBox, Blit(BlitMethod::Fast)),
            PreparedKind::Deferred(DeferredKind::InfoText),
            PreparedKind::Deferred(DeferredKind::CarTransform(CanonicalCar::Subaru)),
            PreparedKind::Deferred(DeferredKind::CarVisibilityLatch(CanonicalCar::Subaru)),
            PreparedKind::Bitmap(
                ChampionshipBitmapIdentity::ManufacturerLogo(subaru_logo),
                Blit(BlitMethod::Fast),
            ),
            PreparedKind::Deferred(DeferredKind::PaletteRequest { group: 4, item: 0 }),
            PreparedKind::Text(SetupTextRole::Row(
                crate::championship_setup::SetupRowKind::Next,
            )),
        ];
        let gear = transcript(1);
        let mut gear_cache = ChampionshipPreviewCache::new();
        let gear_cold = prepare_fixture_frame(&fixture, &gear, None, gear_cache);
        assert_eq!(prepared_kinds(&gear_cold), gear_cold_expected);
        commit_fixture_frame(&mut fixture, gear_cold, &mut gear_cache);
        let gear_warm = prepare_fixture_frame(&fixture, &gear, None, gear_cache);
        let mut gear_warm_expected = gear_cold_expected.to_vec();
        gear_warm_expected.remove(9);
        assert_eq!(prepared_kinds(&gear_warm), gear_warm_expected);

        let next_cold_expected = [
            PreparedKind::Text(SetupTextRole::Title),
            PreparedKind::Text(SetupTextRole::Row(
                crate::championship_setup::SetupRowKind::ChooseCar,
            )),
            PreparedKind::Text(SetupTextRole::Row(
                crate::championship_setup::SetupRowKind::ChooseGear,
            )),
            PreparedKind::Bitmap(
                ChampionshipBitmapIdentity::Selection,
                Blit(BlitMethod::Stretch),
            ),
            PreparedKind::Text(SetupTextRole::Row(
                crate::championship_setup::SetupRowKind::Next,
            )),
            PreparedKind::Bitmap(ChampionshipBitmapIdentity::InfoBox, Blit(BlitMethod::Fast)),
            PreparedKind::Deferred(DeferredKind::InfoText),
            PreparedKind::Deferred(DeferredKind::PaletteRequest { group: 1, item: 9 }),
            PreparedKind::Bitmap(
                ChampionshipBitmapIdentity::ChequeredFrame(ChequeredFrame::Frame04),
                Blit(BlitMethod::Fast),
            ),
        ];
        let next = transcript(2);
        let mut next_cache = ChampionshipPreviewCache::new();
        let next_cold =
            prepare_fixture_frame(&fixture, &next, Some(ChequeredFrame::Frame04), next_cache);
        assert_eq!(prepared_kinds(&next_cold), next_cold_expected);
        commit_fixture_frame(&mut fixture, next_cold, &mut next_cache);
        let next_warm =
            prepare_fixture_frame(&fixture, &next, Some(ChequeredFrame::Frame04), next_cache);
        let mut next_warm_expected = next_cold_expected.to_vec();
        next_warm_expected.remove(7);
        assert_eq!(prepared_kinds(&next_warm), next_warm_expected);
    }

    #[test]
    fn extent_only_preparation_exactly_matches_rich_indexed_commands() {
        let mut fixture = Fixture::new(true);
        for (selected, chequered_frame) in
            [(0, None), (1, None), (2, Some(ChequeredFrame::Frame04))]
        {
            let transcript = transcript(selected);
            let mut cache = ChampionshipPreviewCache::new();

            assert_rich_and_extent_preparation_match(&fixture, &transcript, chequered_frame, cache);

            let prepared = prepare_fixture_frame(&fixture, &transcript, chequered_frame, cache);
            commit_fixture_frame(&mut fixture, prepared, &mut cache);

            assert_rich_and_extent_preparation_match(&fixture, &transcript, chequered_frame, cache);
        }
    }

    #[test]
    fn extent_only_preparation_defers_missing_selection_but_requires_other_extents() {
        let transcript = transcript(0);
        let prepared = prepare_championship_setup_frame_from_source_extents(
            input(&transcript),
            ChampionshipSetupFrameState::default(),
            ChampionshipPreviewCache::new(),
            |identity| match identity {
                ChampionshipBitmapIdentity::Selection => None,
                ChampionshipBitmapIdentity::InfoBox => Some((320, 132)),
                ChampionshipBitmapIdentity::ManufacturerLogo(_) => Some((184, 128)),
                ChampionshipBitmapIdentity::ChequeredFrame(_) => Some((184, 128)),
            },
        )
        .unwrap();
        assert!(matches!(
            prepared.commands().nth(1),
            Some(PreparedChampionshipSetupCommand::Deferred {
                command_index: 1,
                command: DeferredChampionshipCommand::Selection(_),
            })
        ));

        let error = match prepare_championship_setup_frame_from_source_extents(
            input(&transcript),
            ChampionshipSetupFrameState::default(),
            ChampionshipPreviewCache::new(),
            |identity| match identity {
                ChampionshipBitmapIdentity::Selection | ChampionshipBitmapIdentity::InfoBox => None,
                ChampionshipBitmapIdentity::ManufacturerLogo(_) => Some((184, 128)),
                ChampionshipBitmapIdentity::ChequeredFrame(_) => Some((184, 128)),
            },
        ) {
            Ok(_) => panic!("missing required InfoBox extent unexpectedly prepared"),
            Err(error) => error,
        };
        assert_eq!(
            error,
            ChampionshipSetupFramePrepareError::MissingBitmapExtent {
                command_index: 3,
                identity: ChampionshipBitmapIdentity::InfoBox,
            }
        );
    }

    #[test]
    fn prepared_logo_identity_preserves_every_canonical_alias() {
        let fixture = Fixture::new(true);
        for car in CanonicalCar::ALL {
            let mut car_transcript = transcript(0);
            car_transcript.rows[0].value = RowValue::Car(car);
            car_transcript.preview = ActivePreview::Car {
                canonical_car: car,
                object_name: car.object_name(),
                logo_name: car.logo_name(),
            };
            let prepared = prepare_fixture_frame(
                &fixture,
                &car_transcript,
                None,
                ChampionshipPreviewCache::new(),
            );
            let logos = prepared
                .commands()
                .filter_map(|command| match command {
                    PreparedChampionshipSetupCommand::Bitmap {
                        identity: ChampionshipBitmapIdentity::ManufacturerLogo(logo),
                        ..
                    } => Some(logo),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(logos, [car.championship_descriptor().logo], "{car:?}");
        }

        assert_eq!(
            CanonicalCar::Rs200.championship_descriptor().logo,
            CanonicalCar::Mk2.championship_descriptor().logo
        );
        assert_eq!(
            CanonicalCar::Mk2.championship_descriptor().logo,
            CanonicalCar::Escort.championship_descriptor().logo
        );
        assert_eq!(
            CanonicalCar::Corolla.championship_descriptor().logo,
            CanonicalCar::Celica.championship_descriptor().logo
        );
    }

    #[test]
    fn public_text_commands_are_exact_and_inspection_is_effect_free() {
        let transcript = transcript(0);
        let fixture = Fixture::new(true);
        let caller_cache = ChampionshipPreviewCache::new();
        let cache_before = caller_cache;
        let prepared = prepare_fixture_frame(&fixture, &transcript, None, caller_cache);

        let first_pass = prepared.commands().collect::<Vec<_>>();
        let second_pass = prepared.commands().collect::<Vec<_>>();
        assert_eq!(first_pass, second_pass);
        assert_eq!(caller_cache, cache_before);

        let mut visual_cache = cache_before;
        let expected_text = plan_championship_setup_frame(input(&transcript), &mut visual_cache)
            .unwrap()
            .into_iter()
            .enumerate()
            .filter_map(|(command_index, command)| match command {
                ChampionshipSetupVisualCommand::Text(command) => Some((command_index, command)),
                _ => None,
            })
            .collect::<Vec<_>>();
        let public_text = first_pass
            .into_iter()
            .filter_map(|command| match command {
                PreparedChampionshipSetupCommand::Text {
                    command_index,
                    command,
                } => Some((command_index, command)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(public_text, expected_text);
    }

    #[test]
    fn public_prepared_view_preserves_noop_identities_without_indexed_effects() {
        let transcript = transcript(0);
        let mut fixture = Fixture::new(true);
        let mut offscreen = input(&transcript);
        offscreen.frontend_offset_q16 = Q16Point::new(100 << 16, 100 << 16);
        let prepared = prepare_frame_with_bitmaps(
            offscreen,
            ChampionshipSetupFrameState::default(),
            &fixture.text,
            &fixture.fonts,
            &fixture.bitmaps,
            ChampionshipPreviewCache::new(),
            TextLayoutLimits::default(),
        )
        .unwrap();
        let bitmap_identities = prepared
            .commands()
            .filter_map(|command| match command {
                PreparedChampionshipSetupCommand::Bitmap {
                    identity,
                    decision: BlitDecision::NoOp,
                    ..
                } => Some(identity),
                PreparedChampionshipSetupCommand::Bitmap { decision, .. } => {
                    panic!("offscreen bitmap unexpectedly retained {decision:?}")
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            bitmap_identities,
            [
                ChampionshipBitmapIdentity::Selection,
                ChampionshipBitmapIdentity::InfoBox,
                ChampionshipBitmapIdentity::ManufacturerLogo(
                    CanonicalCar::Subaru.championship_descriptor().logo,
                ),
            ]
        );

        validate_championship_setup_frame_sources(&prepared, &fixture.store).unwrap();
        let blits_before = fixture.store.backend().blits.len();
        let mut cache = ChampionshipPreviewCache::new();
        let outcome = execute_prepared_championship_setup_frame(
            prepared,
            &mut cache,
            &mut fixture.store,
            999,
            |_, _| Ok::<_, std::convert::Infallible>(()),
        )
        .unwrap();
        assert_eq!(fixture.store.backend().blits.len(), blits_before);
        assert!(outcome.commands.iter().all(|command| match command {
            ChampionshipSetupCommandOutcome::Bitmap { outcome, .. } => {
                *outcome == IndexedBitmapBlitOutcome::NoOp
            }
            _ => true,
        }));
    }

    #[test]
    fn public_prepared_transcript_matches_indexed_execution_and_inline_deferred_visibility() {
        let transcript = transcript(2);
        let mut fixture = Fixture::new(true);
        let prepared = prepare_fixture_frame(
            &fixture,
            &transcript,
            Some(ChequeredFrame::Frame04),
            ChampionshipPreviewCache::new(),
        );
        let public_commands = prepared.commands().collect::<Vec<_>>();
        validate_championship_setup_frame_sources(&prepared, &fixture.store).unwrap();
        let mut callbacks = Vec::new();
        let mut cache = ChampionshipPreviewCache::new();
        let outcome = execute_prepared_championship_setup_frame(
            prepared,
            &mut cache,
            &mut fixture.store,
            999,
            |command_index, command| {
                callbacks.push((command_index, command));
                Ok::<_, std::convert::Infallible>(())
            },
        )
        .unwrap();

        assert_eq!(public_commands.len(), outcome.commands.len());
        for (public, indexed) in public_commands.iter().zip(&outcome.commands) {
            match (*public, indexed) {
                (
                    PreparedChampionshipSetupCommand::Text {
                        command_index,
                        command,
                    },
                    ChampionshipSetupCommandOutcome::Text {
                        command_index: actual_index,
                        role: actual_role,
                        ..
                    },
                ) => {
                    assert_eq!(command_index, *actual_index);
                    assert_eq!(command.role, *actual_role);
                }
                (
                    PreparedChampionshipSetupCommand::Bitmap {
                        command_index,
                        identity,
                        decision,
                    },
                    ChampionshipSetupCommandOutcome::Bitmap {
                        command_index: actual_index,
                        identity: actual_identity,
                        outcome,
                    },
                ) => {
                    assert_eq!(command_index, *actual_index);
                    assert_eq!(identity, *actual_identity);
                    assert_eq!(
                        *outcome,
                        match decision {
                            BlitDecision::NoOp => IndexedBitmapBlitOutcome::NoOp,
                            BlitDecision::Blit(_) => IndexedBitmapBlitOutcome::Blitted,
                        }
                    );
                }
                (
                    PreparedChampionshipSetupCommand::Deferred {
                        command_index,
                        command,
                    },
                    ChampionshipSetupCommandOutcome::Deferred {
                        command_index: actual_index,
                        command: actual_command,
                    },
                ) => {
                    assert_eq!(command_index, *actual_index);
                    assert_eq!(command, *actual_command);
                }
                pair => panic!("public/indexed command mismatch: {pair:?}"),
            }
        }

        let publicly_deferred = public_commands
            .iter()
            .filter_map(|command| match command {
                PreparedChampionshipSetupCommand::Deferred {
                    command_index,
                    command,
                } => Some((*command_index, *command)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(callbacks, publicly_deferred);
    }

    #[test]
    fn public_prepared_runner_rejects_stale_cache_before_first_callback() {
        let fixture = Fixture::new(true);
        let transcript = transcript(0);
        let prepared =
            prepare_fixture_frame(&fixture, &transcript, None, ChampionshipPreviewCache::new());
        let mut actual_cache = ChampionshipPreviewCache::new();
        prepare_fixture_frame(&fixture, &transcript, None, ChampionshipPreviewCache::new())
            .execute_after_successful_preflight(&mut actual_cache, |_| {
                Ok::<_, std::convert::Infallible>(())
            })
            .unwrap();
        let preserved = actual_cache;
        let mut callback_count = 0;

        let error = prepared
            .execute_after_successful_preflight(&mut actual_cache, |_| {
                callback_count += 1;
                Ok::<_, std::convert::Infallible>(())
            })
            .unwrap_err();

        assert_eq!(callback_count, 0);
        assert_eq!(actual_cache, preserved);
        assert_eq!(
            error,
            PreparedChampionshipSetupExecutionError::StalePreviewCache {
                expected: ChampionshipPreviewCache::new(),
                actual: preserved,
            }
        );
    }

    #[test]
    fn public_prepared_runner_reports_callback_index_without_committing_cache() {
        let fixture = Fixture::new(true);
        let transcript = transcript(0);
        let prepared =
            prepare_fixture_frame(&fixture, &transcript, None, ChampionshipPreviewCache::new());
        let mut cache = ChampionshipPreviewCache::new();
        let mut visited = Vec::new();

        let error = prepared
            .execute_after_successful_preflight(&mut cache, |command| {
                let command_index = match command {
                    PreparedChampionshipSetupCommand::Text { command_index, .. }
                    | PreparedChampionshipSetupCommand::Bitmap { command_index, .. }
                    | PreparedChampionshipSetupCommand::Deferred { command_index, .. } => {
                        command_index
                    }
                };
                visited.push(command_index);
                if command_index == 3 {
                    Err("injected callback failure")
                } else {
                    Ok(())
                }
            })
            .unwrap_err();

        assert_eq!(visited, [0, 1, 2, 3]);
        assert_eq!(cache, ChampionshipPreviewCache::new());
        assert_eq!(
            error,
            PreparedChampionshipSetupExecutionError::Command {
                command_index: 3,
                source: "injected callback failure",
            }
        );
    }
}
