//! Ordered alternate-RGB16 execution of a prepared championship setup frame.
//!
//! Every supported ordinary-bitmap `Fast` command is validated before the
//! first target write. Text, recovered deferred commands, and bitmap stretching
//! remain typed ordered work; this module does not guess their execution.

use std::fmt;

use cmr_formats::bitmap::IndexedBitmap;

use cmr_core::{
    blit::{BlitDecision, BlitMethod},
    championship_setup_frame::{
        ChampionshipBitmapIdentity, ChampionshipSetupFramePrepareError,
        ChampionshipSetupFrameState, DeferredChampionshipCommand,
        PreparedChampionshipAlternateSetupFrame, PreparedChampionshipSetupCommand,
        PreparedChampionshipSetupExecutionError, PreparedChampionshipSetupFrame,
        prepare_championship_setup_frame_from_source_extents,
    },
    championship_setup_visual::{
        ChampionshipPreviewCache, ChampionshipSetupVisualInput, SetupTextCommand,
    },
};

use crate::{
    c3d_software_split_backend::{
        SoftwareSplitC3dBackendError, SoftwareSplitC3dOracleBackend, SoftwareSplitC3dTargetHandle,
    },
    software_championship_ordinary_rgb16::{
        ChampionshipOrdinaryRgb16Identity, SoftwareChampionshipOrdinaryRgb16Pool,
    },
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SoftwareChampionshipBitmapOutcome {
    NoOp,
    Blitted,
}

/// Work retained for a later executor, in its original transcript position.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SoftwareChampionshipSetupDeferredCommand {
    Text(SetupTextCommand),
    Bitmap {
        identity: ChampionshipBitmapIdentity,
        decision: BlitDecision,
    },
    Championship(DeferredChampionshipCommand),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SoftwareChampionshipSetupCommandOutcome {
    ExecutedBitmap {
        command_index: usize,
        identity: ChampionshipBitmapIdentity,
        outcome: SoftwareChampionshipBitmapOutcome,
    },
    Deferred {
        command_index: usize,
        command: SoftwareChampionshipSetupDeferredCommand,
    },
    /// A transcript boundary coordinated inline by an enclosing terminal
    /// owner. Unlike `Deferred`, consumers must not replay this command.
    CoordinatedChampionship {
        command_index: usize,
        command: DeferredChampionshipCommand,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SoftwareChampionshipSetupFrameOutcome {
    /// One entry per prepared command, in exact visual-transcript order.
    pub commands: Vec<SoftwareChampionshipSetupCommandOutcome>,
}

/// Prepares a complete alternate setup stream without indexed uploads, font
/// assets, localization bytes, glyph layout, or an indexed backend.
///
/// Ordinary bitmap extents come from the retained RGB16 pool. Selection is
/// optional because it is outside that pool; callers may supply its decoded
/// bitmap so aligned row width is derived here, or leave it as typed deferred
/// Stretch work. Its numeric entry-zero key belongs only to this unresolved
/// Selection plan and is not treated as an RGB16 packed key.
pub fn prepare_software_championship_alternate_setup_frame(
    input: ChampionshipSetupVisualInput<'_>,
    state: ChampionshipSetupFrameState,
    preview_cache: ChampionshipPreviewCache,
    sources: &SoftwareChampionshipOrdinaryRgb16Pool,
    selection: Option<&IndexedBitmap>,
) -> Result<PreparedChampionshipAlternateSetupFrame, ChampionshipSetupFramePrepareError> {
    prepare_championship_setup_frame_from_source_extents(input, state, preview_cache, |identity| {
        let extent = match identity {
            ChampionshipBitmapIdentity::Selection => {
                let selection = selection?;
                [selection.row_stride, selection.height]
            }
            ChampionshipBitmapIdentity::InfoBox => sources.info_box().aligned_surface_extent(),
            ChampionshipBitmapIdentity::ManufacturerLogo(logo) => {
                sources.logo(logo).aligned_surface_extent()
            }
            ChampionshipBitmapIdentity::ChequeredFrame(frame) => {
                sources.frame(frame).aligned_surface_extent()
            }
        };
        Some((extent[0], extent[1]))
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SoftwareChampionshipSetupFrameError {
    StalePreviewCache {
        expected: ChampionshipPreviewCache,
        actual: ChampionshipPreviewCache,
    },
    UnsupportedFastBitmapIdentity {
        command_index: usize,
        identity: ChampionshipBitmapIdentity,
    },
    BitmapPreflight {
        command_index: usize,
        identity: ChampionshipBitmapIdentity,
        source: SoftwareSplitC3dBackendError,
    },
    BitmapExecution {
        command_index: usize,
        identity: ChampionshipBitmapIdentity,
        source: SoftwareSplitC3dBackendError,
    },
}

impl fmt::Display for SoftwareChampionshipSetupFrameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StalePreviewCache { expected, actual } => write!(
                formatter,
                "championship alternate prepared-frame cache is stale: expected {expected:?}, got {actual:?}"
            ),
            Self::UnsupportedFastBitmapIdentity {
                command_index,
                identity,
            } => write!(
                formatter,
                "championship alternate bitmap {identity:?} at command {command_index} has no ordinary RGB16 source"
            ),
            Self::BitmapPreflight {
                command_index,
                identity,
                source,
            } => write!(
                formatter,
                "championship alternate bitmap {identity:?} at command {command_index} failed preflight: {source}"
            ),
            Self::BitmapExecution {
                command_index,
                identity,
                source,
            } => write!(
                formatter,
                "championship alternate bitmap {identity:?} at command {command_index} failed execution: {source}"
            ),
        }
    }
}

impl std::error::Error for SoftwareChampionshipSetupFrameError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::BitmapPreflight { source, .. } | Self::BitmapExecution { source, .. } => {
                Some(source)
            }
            Self::StalePreviewCache { .. } | Self::UnsupportedFastBitmapIdentity { .. } => None,
        }
    }
}

/// Executes every currently supported ordinary-bitmap command in exact source
/// order and returns every unresolved command as a typed ordered outcome.
///
/// Every supported effectful `Fast` bitmap in the complete command stream is
/// preflighted before the first backend effect. `NoOp` and deferred `Stretch`
/// commands do not resolve an ordinary source. The core-owned pending preview
/// cache is committed only after the ordered ordinary-bitmap slice succeeds;
/// emitting typed unresolved work does not claim that text, stretching, or
/// other deferred visuals rendered. An unexpected compose-time failure cannot
/// roll back bitmap writes from earlier commands, but it suppresses the
/// outcome and cache commit.
/// The accepted prepared type contains no indexed handles or glyph payloads.
pub fn execute_software_championship_setup_frame(
    prepared: PreparedChampionshipAlternateSetupFrame,
    preview_cache: &mut ChampionshipPreviewCache,
    sources: &SoftwareChampionshipOrdinaryRgb16Pool,
    backend: &mut SoftwareSplitC3dOracleBackend,
    destination: SoftwareSplitC3dTargetHandle,
) -> Result<SoftwareChampionshipSetupFrameOutcome, SoftwareChampionshipSetupFrameError> {
    prepared
        .validate_preview_cache(preview_cache)
        .map_err(
            |stale| SoftwareChampionshipSetupFrameError::StalePreviewCache {
                expected: stale.expected,
                actual: stale.actual,
            },
        )?;

    preflight_software_championship_setup_frame(&prepared, sources, backend, destination)?;

    let commands = execute_preflighted_software_championship_setup_frame(
        prepared,
        preview_cache,
        sources,
        backend,
        destination,
        |command_index, command| {
            Ok(SoftwareChampionshipSetupCommandOutcome::Deferred {
                command_index,
                command: SoftwareChampionshipSetupDeferredCommand::Championship(command),
            })
        },
    )?;

    Ok(SoftwareChampionshipSetupFrameOutcome { commands })
}

pub(crate) fn preflight_software_championship_setup_frame<T, B>(
    prepared: &PreparedChampionshipSetupFrame<T, B>,
    sources: &SoftwareChampionshipOrdinaryRgb16Pool,
    backend: &SoftwareSplitC3dOracleBackend,
    destination: SoftwareSplitC3dTargetHandle,
) -> Result<(), SoftwareChampionshipSetupFrameError> {
    for command in prepared.commands() {
        let PreparedChampionshipSetupCommand::Bitmap {
            command_index,
            identity,
            decision: BlitDecision::Blit(plan),
        } = command
        else {
            continue;
        };
        if plan.method == BlitMethod::Stretch {
            continue;
        }
        let source = resolve_fast_source(sources, command_index, identity)?;
        backend
            .preflight_ordinary_bitmap_logical_rgb16_fast_blit(destination, source, plan)
            .map_err(
                |source| SoftwareChampionshipSetupFrameError::BitmapPreflight {
                    command_index,
                    identity,
                    source,
                },
            )?;
    }
    Ok(())
}

pub(crate) fn execute_preflighted_software_championship_setup_frame<T, B>(
    prepared: PreparedChampionshipSetupFrame<T, B>,
    preview_cache: &mut ChampionshipPreviewCache,
    sources: &SoftwareChampionshipOrdinaryRgb16Pool,
    backend: &mut SoftwareSplitC3dOracleBackend,
    destination: SoftwareSplitC3dTargetHandle,
    mut execute_championship: impl FnMut(
        usize,
        DeferredChampionshipCommand,
    ) -> Result<
        SoftwareChampionshipSetupCommandOutcome,
        SoftwareChampionshipSetupFrameError,
    >,
) -> Result<Vec<SoftwareChampionshipSetupCommandOutcome>, SoftwareChampionshipSetupFrameError> {
    let mut commands = Vec::with_capacity(prepared.command_len());
    prepared
        .execute_after_successful_preflight(preview_cache, |command| {
            match command {
                PreparedChampionshipSetupCommand::Text {
                    command_index,
                    command,
                } => commands.push(SoftwareChampionshipSetupCommandOutcome::Deferred {
                    command_index,
                    command: SoftwareChampionshipSetupDeferredCommand::Text(command),
                }),
                PreparedChampionshipSetupCommand::Deferred {
                    command_index,
                    command,
                } => commands.push(execute_championship(command_index, command)?),
                PreparedChampionshipSetupCommand::Bitmap {
                    command_index,
                    identity,
                    decision: BlitDecision::NoOp,
                } => commands.push(SoftwareChampionshipSetupCommandOutcome::ExecutedBitmap {
                    command_index,
                    identity,
                    outcome: SoftwareChampionshipBitmapOutcome::NoOp,
                }),
                PreparedChampionshipSetupCommand::Bitmap {
                    command_index,
                    identity,
                    decision: BlitDecision::Blit(plan),
                } if plan.method == BlitMethod::Stretch => {
                    commands.push(SoftwareChampionshipSetupCommandOutcome::Deferred {
                        command_index,
                        command: SoftwareChampionshipSetupDeferredCommand::Bitmap {
                            identity,
                            decision: BlitDecision::Blit(plan),
                        },
                    });
                }
                PreparedChampionshipSetupCommand::Bitmap {
                    command_index,
                    identity,
                    decision: BlitDecision::Blit(plan),
                } => {
                    let source = resolve_fast_source(sources, command_index, identity)?;
                    backend
                        .compose_ordinary_bitmap_logical_rgb16_fast_blit(destination, source, plan)
                        .map_err(
                            |source| SoftwareChampionshipSetupFrameError::BitmapExecution {
                                command_index,
                                identity,
                                source,
                            },
                        )?;
                    commands.push(SoftwareChampionshipSetupCommandOutcome::ExecutedBitmap {
                        command_index,
                        identity,
                        outcome: SoftwareChampionshipBitmapOutcome::Blitted,
                    });
                }
            }
            Ok(())
        })
        .map_err(|error| match error {
            PreparedChampionshipSetupExecutionError::StalePreviewCache { expected, actual } => {
                SoftwareChampionshipSetupFrameError::StalePreviewCache { expected, actual }
            }
            PreparedChampionshipSetupExecutionError::Command {
                command_index,
                source,
            } => match source {
                SoftwareChampionshipSetupFrameError::UnsupportedFastBitmapIdentity {
                    identity,
                    ..
                } => SoftwareChampionshipSetupFrameError::UnsupportedFastBitmapIdentity {
                    command_index,
                    identity,
                },
                SoftwareChampionshipSetupFrameError::BitmapPreflight {
                    identity, source, ..
                } => SoftwareChampionshipSetupFrameError::BitmapPreflight {
                    command_index,
                    identity,
                    source,
                },
                SoftwareChampionshipSetupFrameError::BitmapExecution {
                    identity, source, ..
                } => SoftwareChampionshipSetupFrameError::BitmapExecution {
                    command_index,
                    identity,
                    source,
                },
                SoftwareChampionshipSetupFrameError::StalePreviewCache { expected, actual } => {
                    SoftwareChampionshipSetupFrameError::StalePreviewCache { expected, actual }
                }
            },
        })?;

    Ok(commands)
}

pub(crate) fn resolve_fast_source(
    sources: &SoftwareChampionshipOrdinaryRgb16Pool,
    command_index: usize,
    identity: ChampionshipBitmapIdentity,
) -> Result<
    &crate::ordinary_bitmap_rgb16::OrdinaryBitmapLogicalRgb16Source,
    SoftwareChampionshipSetupFrameError,
> {
    let identity = match identity {
        ChampionshipBitmapIdentity::Selection => {
            return Err(
                SoftwareChampionshipSetupFrameError::UnsupportedFastBitmapIdentity {
                    command_index,
                    identity,
                },
            );
        }
        ChampionshipBitmapIdentity::InfoBox => ChampionshipOrdinaryRgb16Identity::InfoBox,
        ChampionshipBitmapIdentity::ManufacturerLogo(logo) => {
            ChampionshipOrdinaryRgb16Identity::ManufacturerLogo(logo)
        }
        ChampionshipBitmapIdentity::ChequeredFrame(frame) => {
            ChampionshipOrdinaryRgb16Identity::ChequeredFrame(frame)
        }
    };
    Ok(sources.get(identity))
}
