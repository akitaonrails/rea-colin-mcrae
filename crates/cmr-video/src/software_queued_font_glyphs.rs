//! Lane-neutral queued-font records for the alternate championship path.
//!
//! This module stops before native queue routing and rendering. It preserves
//! recovered text-command and glyph order, but stops before the producer's
//! conditional byte-3 rewrite. It does not claim the native 52-byte record
//! layout, select one of the three lanes, apply a lane capacity, flush
//! commands, or compose final pixels.

use std::fmt;

use cmr_core::{
    championship_setup_frame::{
        PreparedChampionshipAlternateSetupFrame, PreparedChampionshipSetupCommand,
    },
    championship_setup_visual::{SetupTextCommand, SetupTextIdentity, SetupTextRole},
    display::DisplayExtent,
    font_assets::FontStyle,
    font_raster::{
        FontRasterLayoutError, FontTextPrepareError, PreparedFontText, prepare_font_text,
    },
    localization::IndexedTextTable,
};
use cmr_formats::font::{PointI32, RectI16, RectI32, TextLayoutLimits, TextPlan};

use crate::software_queued_font_argb4444::{
    SoftwareQueuedFontArgb4444Pool, SoftwareQueuedFontArgb4444Source,
};

/// The two terminal numeric values recovered at the alternate glyph caller.
///
/// Bit `0x40` selects the fallback blitter when the eventual lane is flushed;
/// the clear-bit value follows the textured-quad consumer. This type does not
/// select the device branch which supplies either value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum SoftwareQueuedFontTerminal {
    TexturedQuad = 8,
    FallbackBlitter = 0x48,
}

impl SoftwareQueuedFontTerminal {
    #[must_use]
    pub const fn raw(self) -> u32 {
        self as u32
    }
}

/// Semantic queued-glyph producer input, deliberately independent of the
/// native record packing and unresolved lane assignment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LaneNeutralQueuedFontGlyph {
    pub command_index: usize,
    pub blit_index: usize,
    pub code: u8,
    pub glyph_index: u8,
    pub style: FontStyle,
    pub source: RectI16,
    pub destination: RectI32,
    /// Tuple after the recovered caller-side Q16 operation and before the
    /// producer's unresolved conditional byte-3 rewrite.
    pub caller_tuple: [u8; 4],
    pub terminal: SoftwareQueuedFontTerminal,
}

impl LaneNeutralQueuedFontGlyph {
    /// Resolves this record to the persistent source owned by `pool` without
    /// copying atlas data into the record.
    #[must_use]
    pub fn source_owner<'a>(
        &self,
        pool: &'a SoftwareQueuedFontArgb4444Pool,
    ) -> &'a SoftwareQueuedFontArgb4444Source {
        pool.source(self.style)
    }
}

/// One complete text command prepared without mutating a native queue.
#[derive(Debug)]
pub struct LaneNeutralQueuedFontText {
    command_index: usize,
    command: SetupTextCommand,
    terminal: SoftwareQueuedFontTerminal,
    prepared: PreparedFontText,
    records: Vec<LaneNeutralQueuedFontGlyph>,
}

impl LaneNeutralQueuedFontText {
    #[must_use]
    pub const fn command_index(&self) -> usize {
        self.command_index
    }

    #[must_use]
    pub const fn command(&self) -> SetupTextCommand {
        self.command
    }

    #[must_use]
    pub const fn terminal(&self) -> SoftwareQueuedFontTerminal {
        self.terminal
    }

    #[must_use]
    pub const fn plan(&self) -> &TextPlan {
        self.prepared.plan()
    }

    #[must_use]
    pub fn records(&self) -> &[LaneNeutralQueuedFontGlyph] {
        &self.records
    }

    /// Safe source-containment rejections in original glyph-plan order.
    #[must_use]
    pub fn layout_diagnostics(&self) -> &[FontRasterLayoutError] {
        self.prepared.layout_diagnostics()
    }
}

/// Complete effect-free queued text subset of one setup transcript.
#[derive(Debug)]
pub struct LaneNeutralChampionshipQueuedText {
    texts: Vec<LaneNeutralQueuedFontText>,
}

impl LaneNeutralChampionshipQueuedText {
    #[must_use]
    pub fn texts(&self) -> &[LaneNeutralQueuedFontText] {
        &self.texts
    }

    /// Records in visual-command order and then glyph-plan order.
    pub fn records(&self) -> impl Iterator<Item = &LaneNeutralQueuedFontGlyph> {
        self.texts.iter().flat_map(|text| text.records.iter())
    }

    #[must_use]
    pub fn record_len(&self) -> usize {
        self.texts.iter().map(|text| text.records.len()).sum()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LaneNeutralChampionshipQueuedTextError {
    pub command_index: usize,
    pub role: SetupTextRole,
    pub source: FontTextPrepareError,
}

impl fmt::Display for LaneNeutralChampionshipQueuedTextError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "championship queued text {:?} at command {} failed preparation: {}",
            self.role, self.command_index, self.source
        )
    }
}

impl std::error::Error for LaneNeutralChampionshipQueuedTextError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// Prepares one command into semantic lane-neutral records.
pub fn prepare_lane_neutral_queued_font_text(
    pool: &SoftwareQueuedFontArgb4444Pool,
    command_index: usize,
    command: SetupTextCommand,
    text: &[u8],
    destination_extent: DisplayExtent,
    limits: TextLayoutLimits,
    terminal: SoftwareQueuedFontTerminal,
) -> Result<LaneNeutralQueuedFontText, FontTextPrepareError> {
    let prepared = prepare_font_text(
        pool.font(command.font),
        text,
        PointI32 {
            x: command.position.x,
            y: command.position.y,
        },
        command.flags,
        destination_extent,
        limits,
    )?;
    let tuple = command.effective_tuple();
    let records = prepared
        .glyphs()
        .iter()
        .map(|glyph| LaneNeutralQueuedFontGlyph {
            command_index,
            blit_index: glyph.blit_index,
            code: glyph.code,
            glyph_index: glyph.glyph_index,
            style: command.font,
            source: glyph.source,
            destination: glyph.destination,
            caller_tuple: tuple,
            terminal,
        })
        .collect();
    Ok(LaneNeutralQueuedFontText {
        command_index,
        command,
        terminal,
        prepared,
        records,
    })
}

/// Prepares every non-deferred setup text command before publishing any
/// records. An error drops the local partial transcript, so callers cannot
/// observe a prefix from a rejected frame.
pub fn prepare_lane_neutral_championship_queued_text(
    setup: &PreparedChampionshipAlternateSetupFrame,
    text_table: &IndexedTextTable,
    pool: &SoftwareQueuedFontArgb4444Pool,
    destination_extent: DisplayExtent,
    limits: TextLayoutLimits,
    terminal: SoftwareQueuedFontTerminal,
) -> Result<LaneNeutralChampionshipQueuedText, LaneNeutralChampionshipQueuedTextError> {
    let mut texts = Vec::new();
    for prepared_command in setup.commands() {
        let PreparedChampionshipSetupCommand::Text {
            command_index,
            command,
        } = prepared_command
        else {
            continue;
        };
        let text = resolve_text(text_table, command.identity);
        let queued = prepare_lane_neutral_queued_font_text(
            pool,
            command_index,
            command,
            text,
            destination_extent,
            limits,
            terminal,
        )
        .map_err(|source| LaneNeutralChampionshipQueuedTextError {
            command_index,
            role: command.role,
            source,
        })?;
        texts.push(queued);
    }
    Ok(LaneNeutralChampionshipQueuedText { texts })
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

#[cfg(test)]
mod tests {
    use cmr_core::{
        championship_setup::{
            ActivePreview, CAR_ROW_TAG, CanonicalCar, ChampionshipSetupRenderTranscript,
            GEAR_ROW_TAG, GearChoice, NEXT_ROW_TAG, RenderRow, RowValue, SetupRowText,
        },
        championship_setup_frame::{
            ChampionshipSetupFrameState, prepare_championship_setup_frame_from_source_extents,
        },
        championship_setup_visual::{
            ChampionshipPreviewCache, ChampionshipSetupVisualInput, NextPreviewContext, PixelPoint,
            Q16Point, RowPresentation, RowTextTupleBank, SetupTextRole,
        },
        font_assets::FontSet,
        localization::{FRONTEND_TEXT_COUNT, IndexedTextTable},
    };
    use cmr_formats::font::{FontLimits, parse_legacy_font};

    use super::*;
    use crate::software_queued_font_argb4444::prepare_software_queued_font_argb4444_pool;

    #[test]
    fn records_retain_plan_order_effective_tuple_terminal_and_persistent_source() {
        let pool = pool(1);
        let command = command(FontStyle::Title, [1, 127, 255, 0x7e], 0x8000);
        let prepared = prepare_lane_neutral_queued_font_text(
            &pool,
            7,
            command,
            b"ABA",
            DisplayExtent {
                width: 640,
                height: 480,
            },
            TextLayoutLimits::default(),
            SoftwareQueuedFontTerminal::TexturedQuad,
        )
        .unwrap();

        assert_eq!(prepared.command_index(), 7);
        assert_eq!(prepared.command(), command);
        assert_eq!(prepared.terminal().raw(), 8);
        assert_eq!(prepared.records().len(), 3);
        assert_eq!(
            prepared
                .records()
                .iter()
                .map(|record| (record.code, record.blit_index, record.destination.x))
                .collect::<Vec<_>>(),
            [(b'A', 0, 10), (b'B', 1, 12), (b'A', 2, 15)]
        );
        for record in prepared.records() {
            assert_eq!(record.command_index, 7);
            assert_eq!(record.style, FontStyle::Title);
            assert_eq!(record.caller_tuple, [0, 63, 127, 0x7e]);
            assert_eq!(record.terminal, SoftwareQueuedFontTerminal::TexturedQuad);
            assert!(std::ptr::eq(
                record.source_owner(&pool),
                pool.source(FontStyle::Title)
            ));
        }
        assert!(prepared.layout_diagnostics().is_empty());
    }

    #[test]
    fn out_of_atlas_glyph_is_diagnostic_and_later_record_keeps_advance() {
        let pool = pool(1);
        let prepared = prepare_lane_neutral_queued_font_text(
            &pool,
            2,
            command(FontStyle::Options, [9, 8, 7, 6], 0x1_0000),
            b"WB",
            DisplayExtent {
                width: 640,
                height: 480,
            },
            TextLayoutLimits::default(),
            SoftwareQueuedFontTerminal::FallbackBlitter,
        )
        .unwrap();

        assert_eq!(prepared.plan().blits.len(), 2);
        assert_eq!(prepared.records().len(), 1);
        assert_eq!(prepared.records()[0].code, b'B');
        assert_eq!(prepared.records()[0].blit_index, 1);
        assert_eq!(prepared.records()[0].destination.x, 13);
        assert_eq!(prepared.records()[0].terminal.raw(), 0x48);
        assert!(matches!(
            prepared.layout_diagnostics(),
            [FontRasterLayoutError::GlyphSourceOutOfRange {
                blit_index: 0,
                code: b'W',
                ..
            }]
        ));
    }

    #[test]
    fn lane_neutral_production_does_not_invent_the_per_lane_capacity() {
        let pool = pool(0);
        let text = vec![b'A'; 2_048];
        let prepared = prepare_lane_neutral_queued_font_text(
            &pool,
            0,
            command(FontStyle::Title, [1, 2, 3, 4], 0x1_0000),
            &text,
            DisplayExtent {
                width: 4096,
                height: 16,
            },
            TextLayoutLimits::default(),
            SoftwareQueuedFontTerminal::TexturedQuad,
        )
        .unwrap();

        assert_eq!(prepared.records().len(), 2_048);
        assert_eq!(prepared.records()[2_047].blit_index, 2_047);
        assert_eq!(prepared.records()[2_047].destination.x, 2_057);
    }

    #[test]
    fn complete_setup_texts_follow_visual_command_order_atomically() {
        let transcript = transcript();
        let input = visual_input(&transcript);
        let setup = prepare_championship_setup_frame_from_source_extents(
            input,
            ChampionshipSetupFrameState::default(),
            ChampionshipPreviewCache::new(),
            |_| Some((512, 512)),
        )
        .unwrap();
        let pool = pool(0);
        let table_bytes = b"A\n".repeat(FRONTEND_TEXT_COUNT);
        let table = IndexedTextTable::parse(&table_bytes, FRONTEND_TEXT_COUNT).unwrap();

        let queued = prepare_lane_neutral_championship_queued_text(
            &setup,
            &table,
            &pool,
            input.viewport,
            TextLayoutLimits::default(),
            SoftwareQueuedFontTerminal::TexturedQuad,
        )
        .unwrap();

        assert_eq!(
            queued
                .texts()
                .iter()
                .map(LaneNeutralQueuedFontText::command_index)
                .collect::<Vec<_>>(),
            [0, 2, 9, 10]
        );
        let mut record_commands = queued
            .records()
            .map(|record| record.command_index)
            .collect::<Vec<_>>();
        record_commands.dedup();
        assert_eq!(record_commands, [0, 2, 9, 10]);
        assert!(queued.record_len() >= 4);
    }

    #[test]
    fn late_text_failure_returns_only_context_and_a_clean_retry_succeeds() {
        let transcript = transcript();
        let input = visual_input(&transcript);
        let setup = prepare_championship_setup_frame_from_source_extents(
            input,
            ChampionshipSetupFrameState::default(),
            ChampionshipPreviewCache::new(),
            |_| Some((512, 512)),
        )
        .unwrap();
        let pool = pool(0);
        let table_bytes = b"A\n".repeat(FRONTEND_TEXT_COUNT);
        let table = IndexedTextTable::parse(&table_bytes, FRONTEND_TEXT_COUNT).unwrap();

        let error = prepare_lane_neutral_championship_queued_text(
            &setup,
            &table,
            &pool,
            input.viewport,
            TextLayoutLimits {
                max_input_bytes: 1,
                ..TextLayoutLimits::default()
            },
            SoftwareQueuedFontTerminal::TexturedQuad,
        )
        .unwrap_err();
        assert_eq!(error.command_index, 2);
        assert_eq!(
            error.role,
            SetupTextRole::Row(cmr_core::championship_setup::SetupRowKind::ChooseCar)
        );

        let retry = prepare_lane_neutral_championship_queued_text(
            &setup,
            &table,
            &pool,
            input.viewport,
            TextLayoutLimits::default(),
            SoftwareQueuedFontTerminal::TexturedQuad,
        )
        .unwrap();
        assert_eq!(retry.texts().len(), 4);
    }

    fn command(style: FontStyle, source_tuple: [u8; 4], fade_q16: i32) -> SetupTextCommand {
        SetupTextCommand {
            role: SetupTextRole::Title,
            identity: SetupTextIdentity::Literal("test"),
            font: style,
            position: PixelPoint::new(10, 20),
            flags: 0x11,
            source_tuple,
            fade_q16,
        }
    }

    fn pool(tracking: u32) -> SoftwareQueuedFontArgb4444Pool {
        let fonts = FontSet::from_ordered_fonts(std::array::from_fn(|_| {
            parse_legacy_font(&font_file(tracking), FontLimits::default()).unwrap()
        }));
        prepare_software_queued_font_argb4444_pool(fonts).unwrap()
    }

    fn transcript() -> ChampionshipSetupRenderTranscript {
        let car = CanonicalCar::Subaru;
        ChampionshipSetupRenderTranscript {
            title_text_index: 0x13,
            rows: [
                RenderRow {
                    tag: CAR_ROW_TAG,
                    text: SetupRowText::ChooseCar,
                    selected: true,
                    value: RowValue::Car(car),
                },
                RenderRow {
                    tag: GEAR_ROW_TAG,
                    text: SetupRowText::ChooseGear,
                    selected: false,
                    value: RowValue::Gear(GearChoice::BitClear),
                },
                RenderRow {
                    tag: NEXT_ROW_TAG,
                    text: SetupRowText::Localized(0x18),
                    selected: false,
                    value: RowValue::None,
                },
            ],
            highlight_row: 0,
            preview: ActivePreview::Car {
                canonical_car: car,
                object_name: car.object_name(),
                logo_name: car.logo_name(),
            },
        }
    }

    fn visual_input<'a>(
        transcript: &'a ChampionshipSetupRenderTranscript,
    ) -> ChampionshipSetupVisualInput<'a> {
        ChampionshipSetupVisualInput {
            viewport: DisplayExtent {
                width: 640,
                height: 480,
            },
            frontend_offset_q16: Q16Point::new(0, 0),
            row_anchors_q16: [
                Q16Point::new(0x1_0000, 0x8000),
                Q16Point::new(0x1_2000, 0xa000),
                Q16Point::new(0x1_4000, 0xc000),
            ],
            preview_anchor_q16: Q16Point::new(0x8000, 0x4000),
            rows: [RowPresentation {
                visible: true,
                enabled: true,
            }; 3],
            selected_text_widths: [100, 80, 60],
            title_text_tuple: [1, 2, 3, 4],
            title_text_fade_q16: 0x1_0000,
            row_text_tuples: RowTextTupleBank {
                terminal: [5, 6, 7, 8],
                inactive: [9, 10, 11, 12],
                active: [13, 14, 15, 16],
            },
            row_text_fade_q16: 0x1_0000,
            transcript,
            next_context: NextPreviewContext {
                state_byte: 0,
                mode_byte: 0,
            },
        }
    }

    fn font_file(tracking: u32) -> Vec<u8> {
        const PIXEL_OFFSET: usize = 0x436;
        const FONT_HEADER_SIZE: usize = 24;
        const GLYPH_SIZE: usize = 16;
        let width = 8usize;
        let height = 8usize;
        let trailer = PIXEL_OFFSET + width * height;
        let mut input = vec![0; trailer + FONT_HEADER_SIZE + 3 * GLYPH_SIZE];
        input[0..2].copy_from_slice(b"BM");
        input[18..22].copy_from_slice(&(width as i32).to_le_bytes());
        input[22..26].copy_from_slice(&(height as i32).to_le_bytes());
        input[28..30].copy_from_slice(&8u16.to_le_bytes());
        for index in 0..256usize {
            let offset = 14 + 40 + index * 4;
            input[offset..offset + 4].copy_from_slice(&[index as u8, index as u8, index as u8, 0]);
        }
        input[PIXEL_OFFSET..trailer].fill(1);
        input[trailer + 1] = 3;
        input[trailer + 4..trailer + 8].copy_from_slice(&2u32.to_le_bytes());
        input[trailer + 8..trailer + 12].copy_from_slice(&1u32.to_le_bytes());
        input[trailer + 12..trailer + 16].copy_from_slice(&tracking.to_le_bytes());
        input[trailer + 16..trailer + 20].copy_from_slice(&1u32.to_le_bytes());
        let records = trailer + FONT_HEADER_SIZE;
        write_glyph(&mut input[records..records + GLYPH_SIZE], b'A', 0, 0, 1, 2);
        write_glyph(
            &mut input[records + GLYPH_SIZE..records + 2 * GLYPH_SIZE],
            b'W',
            7,
            0,
            2,
            2,
        );
        write_glyph(
            &mut input[records + 2 * GLYPH_SIZE..records + 3 * GLYPH_SIZE],
            b'B',
            2,
            0,
            2,
            2,
        );
        input
    }

    fn write_glyph(record: &mut [u8], code: u8, x: i16, y: i16, width: i16, height: i16) {
        record[0] = code;
        record[2..4].copy_from_slice(&x.to_le_bytes());
        record[4..6].copy_from_slice(&y.to_le_bytes());
        record[6..8].copy_from_slice(&width.to_le_bytes());
        record[8..10].copy_from_slice(&height.to_le_bytes());
    }
}
