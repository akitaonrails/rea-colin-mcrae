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
    ChampionshipSetupVisualInput, DeferredInfoText, PixelRect, SetupTextIdentity, SetupTextRole,
    plan_championship_setup_frame,
};
use crate::display::{DisplayExtent, IndexedVideoBackend};
use crate::font_raster::{
    PersistentTextOutcome, PersistentTextPrepareError, PreparedPersistentText, UploadedFontPool,
    execute_prepared_persistent_text, prepare_persistent_text,
};
use crate::indexed_bitmap_raster::{
    IndexedBitmapBlitOutcome, IndexedBitmapPlanError, IndexedBitmapRect, plan_indexed_bitmap_blit,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChampionshipBitmapRole {
    Selection,
    InfoBox,
    ManufacturerLogo,
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
        role: ChampionshipBitmapRole,
        source: IndexedBitmapPlanError,
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
                role,
                source,
            } => write!(
                formatter,
                "championship frame bitmap {role:?} at command {command_index} failed preflight: {source}"
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
        role: ChampionshipBitmapRole,
        source: SourceAccessError,
    },
    BitmapBackend {
        command_index: usize,
        role: ChampionshipBitmapRole,
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
                role,
                source,
            } => write!(
                formatter,
                "championship frame bitmap {role:?} at command {command_index} cannot access its source: {source}"
            ),
            Self::BitmapBackend {
                command_index,
                role,
                source,
            } => write!(
                formatter,
                "championship frame bitmap {role:?} at command {command_index} failed: {source}"
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
        role: ChampionshipBitmapRole,
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

enum PreparedCommand {
    Text {
        command_index: usize,
        role: SetupTextRole,
        text: PreparedPersistentText,
    },
    Bitmap {
        command_index: usize,
        role: ChampionshipBitmapRole,
        bitmap: UploadedIndexedBitmap,
        decision: BlitDecision,
    },
    Deferred {
        command_index: usize,
        command: DeferredChampionshipCommand,
    },
}

struct PreparedFrame {
    commands: Vec<PreparedCommand>,
    next_cache: ChampionshipPreviewCache,
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
    execute_frame_with_bitmaps(
        input,
        state,
        resources.text,
        resources.fonts,
        &resources,
        preview_cache,
        sources,
        destination,
        text_limits,
    )
}

#[allow(clippy::too_many_arguments)]
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
    validate_prepared_sources(&prepared, sources)?;
    let mut outcomes = Vec::with_capacity(prepared.commands.len());
    for command in prepared.commands {
        match command {
            PreparedCommand::Text {
                command_index,
                role,
                text,
            } => {
                let rendered = execute_prepared_persistent_text(sources, text, destination)
                    .map_err(|source| ChampionshipSetupFrameExecutionError::TextAccess {
                        command_index,
                        role,
                        source,
                    })?;
                outcomes.push(ChampionshipSetupCommandOutcome::Text {
                    command_index,
                    role,
                    rendered,
                });
            }
            PreparedCommand::Bitmap {
                command_index,
                role,
                bitmap,
                decision,
            } => {
                let outcome = match decision {
                    BlitDecision::NoOp => IndexedBitmapBlitOutcome::NoOp,
                    BlitDecision::Blit(plan) => {
                        bitmap
                            .blit(sources, destination, plan)
                            .map_err(|source| match source {
                                SourceBlitError::Access(source) => {
                                    ChampionshipSetupFrameExecutionError::BitmapAccess {
                                        command_index,
                                        role,
                                        source,
                                    }
                                }
                                SourceBlitError::Backend(source) => {
                                    ChampionshipSetupFrameExecutionError::BitmapBackend {
                                        command_index,
                                        role,
                                        source,
                                    }
                                }
                            })?;
                        IndexedBitmapBlitOutcome::Blitted
                    }
                };
                outcomes.push(ChampionshipSetupCommandOutcome::Bitmap {
                    command_index,
                    role,
                    outcome,
                });
            }
            PreparedCommand::Deferred {
                command_index,
                command,
            } => outcomes.push(ChampionshipSetupCommandOutcome::Deferred {
                command_index,
                command,
            }),
        }
    }
    *preview_cache = prepared.next_cache;
    Ok(ChampionshipSetupFrameOutcome { commands: outcomes })
}

fn validate_prepared_sources<B: IndexedVideoBackend>(
    prepared: &PreparedFrame,
    sources: &UploadedSourceStore<'_, B>,
) -> Result<(), ChampionshipSetupFrameExecutionError<B::Error>> {
    for command in &prepared.commands {
        match command {
            PreparedCommand::Text {
                command_index,
                role,
                text,
            } => {
                if text.requires_source_access() {
                    text.validate_source(sources).map_err(|source| {
                        ChampionshipSetupFrameExecutionError::TextAccess {
                            command_index: *command_index,
                            role: *role,
                            source,
                        }
                    })?;
                }
            }
            PreparedCommand::Bitmap {
                command_index,
                role,
                bitmap,
                decision,
            } => {
                if matches!(decision, BlitDecision::Blit(_)) {
                    bitmap.validate_source(sources).map_err(|source| {
                        ChampionshipSetupFrameExecutionError::BitmapAccess {
                            command_index: *command_index,
                            role: *role,
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
    mut next_cache: ChampionshipPreviewCache,
    text_limits: TextLayoutLimits,
) -> Result<PreparedFrame, ChampionshipSetupFramePrepareError> {
    let extent = input.viewport;
    let visual = plan_championship_setup_frame(input, &mut next_cache)
        .map_err(ChampionshipSetupFramePrepareError::Visual)?;
    let mut commands = Vec::with_capacity(visual.len());
    for (command_index, command) in visual.into_iter().enumerate() {
        match command {
            ChampionshipSetupVisualCommand::Text(text) => {
                let bytes = resolve_text(text_table, text.identity);
                let prepared = prepare_persistent_text(
                    fonts,
                    bytes,
                    text.font,
                    PointI32 {
                        x: text.position.x,
                        y: text.position.y,
                    },
                    text.flags,
                    extent,
                    text_limits,
                )
                .map_err(|source| ChampionshipSetupFramePrepareError::Text {
                    command_index,
                    role: text.role,
                    source,
                })?;
                commands.push(PreparedCommand::Text {
                    command_index,
                    role: text.role,
                    text: prepared,
                });
            }
            ChampionshipSetupVisualCommand::Selection { .. } => {
                if let Some(bitmap) = resources.selection() {
                    commands.push(prepare_bitmap(
                        command_index,
                        ChampionshipBitmapRole::Selection,
                        bitmap,
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
            ChampionshipSetupVisualCommand::InfoBox { .. } => commands.push(prepare_bitmap(
                command_index,
                ChampionshipBitmapRole::InfoBox,
                resources.info_box(),
                command,
                extent,
            )?),
            ChampionshipSetupVisualCommand::ManufacturerLogo { logo, .. } => {
                commands.push(prepare_bitmap(
                    command_index,
                    ChampionshipBitmapRole::ManufacturerLogo,
                    resources.logo(logo),
                    command,
                    extent,
                )?);
            }
            ChampionshipSetupVisualCommand::DeferredNextPreview { .. } => {
                if let Some(frame) = state.chequered_frame {
                    commands.push(prepare_bitmap(
                        command_index,
                        ChampionshipBitmapRole::ChequeredFrame(frame),
                        resources.frame(frame),
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
                });
            }
            ChampionshipSetupVisualCommand::DeferredCarTransform { .. } => {
                commands.push(PreparedCommand::Deferred {
                    command_index,
                    command: DeferredChampionshipCommand::CarTransform(command),
                });
            }
            ChampionshipSetupVisualCommand::DeferredCarVisibilityLatch { .. } => {
                commands.push(PreparedCommand::Deferred {
                    command_index,
                    command: DeferredChampionshipCommand::CarVisibilityLatch(command),
                });
            }
            ChampionshipSetupVisualCommand::PaletteRequest { group, item } => {
                commands.push(PreparedCommand::Deferred {
                    command_index,
                    command: DeferredChampionshipCommand::PaletteRequest { group, item },
                });
            }
        }
    }
    Ok(PreparedFrame {
        commands,
        next_cache,
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

fn prepare_bitmap(
    command_index: usize,
    role: ChampionshipBitmapRole,
    bitmap: UploadedIndexedBitmap,
    command: ChampionshipSetupVisualCommand,
    extent: DisplayExtent,
) -> Result<PreparedCommand, ChampionshipSetupFramePrepareError> {
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
    let decision = plan_indexed_bitmap_blit(
        bitmap,
        indexed_rect(source),
        indexed_rect(destination),
        extent,
        color_key,
    )
    .map_err(|source| ChampionshipSetupFramePrepareError::Bitmap {
        command_index,
        role,
        source,
    })?;
    Ok(PreparedCommand::Bitmap {
        command_index,
        role,
        bitmap,
        decision,
    })
}

const fn indexed_rect(rectangle: PixelRect) -> IndexedBitmapRect {
    IndexedBitmapRect::new(rectangle.x, rectangle.y, rectangle.width, rectangle.height)
}

#[cfg(test)]
mod tests {
    use cmr_formats::font::{FontLimits, parse_legacy_font};
    use cmr_formats::riff_palette::PaletteEntry;

    use super::*;
    use crate::blit::BlitPlan;
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

    #[derive(Default)]
    struct RecordingBackend {
        next_source: u16,
        blits: Vec<u16>,
        fail_source: Option<u16>,
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
            Bitmap(ChampionshipBitmapRole),
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
                ChampionshipSetupCommandOutcome::Bitmap { role, .. } => Observed::Bitmap(*role),
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
                Observed::Bitmap(ChampionshipBitmapRole::Selection),
                Observed::Text(SetupTextRole::Row(
                    crate::championship_setup::SetupRowKind::Next
                )),
                Observed::Bitmap(ChampionshipBitmapRole::InfoBox),
                Observed::InfoText,
                Observed::PaletteRequest,
                Observed::Bitmap(ChampionshipBitmapRole::ChequeredFrame(
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
                    role: ChampionshipBitmapRole::ChequeredFrame(ChequeredFrame::Frame01),
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
                role: ChampionshipBitmapRole::ChequeredFrame(ChequeredFrame::Frame15),
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
                role: ChampionshipBitmapRole::ChequeredFrame(ChequeredFrame::Frame15),
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
                role: ChampionshipBitmapRole::Selection,
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
                role: ChampionshipBitmapRole::ChequeredFrame(ChequeredFrame::Frame15),
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
                role: ChampionshipBitmapRole::Selection,
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
                role: ChampionshipBitmapRole::ChequeredFrame(ChequeredFrame::Frame03),
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

        let roles = outcome
            .commands
            .iter()
            .filter_map(|outcome| match outcome {
                ChampionshipSetupCommandOutcome::Bitmap { role, .. } => Some(*role),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            roles,
            [
                ChampionshipBitmapRole::Selection,
                ChampionshipBitmapRole::InfoBox,
                ChampionshipBitmapRole::ManufacturerLogo,
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
}
