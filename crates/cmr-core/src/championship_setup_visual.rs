//! Backend-neutral frame planner for the recovered championship setup screen.
//!
//! This module emits ordered commands only. Bitmap lookup, font measurement,
//! localization, C3D execution, and pixel writes remain downstream concerns.

use std::fmt;

use crate::{
    championship_assets::ManufacturerLogo,
    championship_setup::{
        ActivePreview, CAR_ROW_TAG, CanonicalCar, ChampionshipSetupRenderTranscript, GEAR_ROW_TAG,
        GearChoice, NEXT_ROW_TAG, NEXT_ROW_TEXT_INDEX, RowValue, SetupRowKind, SetupRowText,
        TITLE_TEXT_INDEX,
    },
    display::DisplayExtent,
    font_assets::FontStyle,
};

pub const TITLE_FLAGS: u8 = 0x13;
pub const ROW_FLAGS: u8 = 0x11;
pub const TITLE_ANCHOR_Q16: Q16Point = Q16Point::new(0x1c666, -0xf333);
pub const SELECTION_SOURCE: PixelRect = PixelRect::new(0, 0, 256, 256);
pub const INFO_BOX_SOURCE: PixelRect = PixelRect::new(0, 0, 320, 132);
pub const PREVIEW_SOURCE: PixelRect = PixelRect::new(0, 0, 182, 128);
pub const CAR_ANGLE_DELTA: u16 = 0x16;
pub const INFO_PRIMARY_FONT: FontStyle = FontStyle::BlueInfo;
pub const INFO_SECONDARY_FONT: FontStyle = FontStyle::GreenInfo;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Q16Point {
    pub x: i32,
    pub y: i32,
}

impl Q16Point {
    #[must_use]
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    const fn wrapping_add(self, other: Self) -> Self {
        Self::new(self.x.wrapping_add(other.x), self.y.wrapping_add(other.y))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PixelPoint {
    pub x: i32,
    pub y: i32,
}

impl PixelPoint {
    #[must_use]
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PixelRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl PixelRect {
    #[must_use]
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RowPresentation {
    pub visible: bool,
    pub enabled: bool,
}

/// The three raw four-byte values selected by the recovered row-text branch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RowTextTupleBank {
    pub terminal: [u8; 4],
    pub inactive: [u8; 4],
    pub active: [u8; 4],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NextPreviewContext {
    /// Raw byte whose low two bits are tested by the shipped callback.
    pub state_byte: u8,
    /// Raw byte whose bits 3-4 choose one of three contextual strings.
    pub mode_byte: u8,
}

#[derive(Clone, Copy, Debug)]
pub struct ChampionshipSetupVisualInput<'a> {
    pub viewport: DisplayExtent,
    pub frontend_offset_q16: Q16Point,
    /// Recovered channels 1, 2, and 6, in car/gear/next order.
    pub row_anchors_q16: [Q16Point; 3],
    pub preview_anchor_q16: Q16Point,
    pub rows: [RowPresentation; 3],
    /// Font-measured selected-text width for each row.
    pub selected_text_widths: [u16; 3],
    pub title_text_tuple: [u8; 4],
    pub title_text_fade_q16: i32,
    pub row_text_tuples: RowTextTupleBank,
    pub row_text_fade_q16: i32,
    pub transcript: &'a ChampionshipSetupRenderTranscript,
    pub next_context: NextPreviewContext,
}

/// Paired native preview identities which invalidate one another on a change.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ChampionshipPreviewCache {
    car: Option<CanonicalCar>,
    next_group: Option<u8>,
}

impl ChampionshipPreviewCache {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            car: None,
            next_group: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChampionshipSetupVisualError {
    TitleTextIndex {
        actual: usize,
    },
    SelectedRowCount {
        actual: usize,
    },
    HighlightRowOutOfRange {
        actual: usize,
    },
    HighlightRowMismatch {
        highlight: usize,
        selected: usize,
    },
    RowTag {
        row: usize,
        expected: u8,
        actual: u8,
    },
    RowText {
        row: usize,
        expected: SetupRowText,
        actual: SetupRowText,
    },
    RowValue {
        row: usize,
        actual: RowValue,
    },
    Preview {
        expected: ActivePreview,
        actual: ActivePreview,
    },
}

impl fmt::Display for ChampionshipSetupVisualError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TitleTextIndex { actual } => write!(
                formatter,
                "championship setup title index {actual:#x} does not match {TITLE_TEXT_INDEX:#x}"
            ),
            Self::SelectedRowCount { actual } => write!(
                formatter,
                "championship setup transcript has {actual} selected rows; expected exactly one"
            ),
            Self::HighlightRowOutOfRange { actual } => write!(
                formatter,
                "championship setup highlight row {actual} is outside the three-row transcript"
            ),
            Self::HighlightRowMismatch {
                highlight,
                selected,
            } => write!(
                formatter,
                "championship setup highlight row {highlight} does not match selected row {selected}"
            ),
            Self::RowTag {
                row,
                expected,
                actual,
            } => write!(
                formatter,
                "championship setup row {row} tag {actual} does not match canonical tag {expected}"
            ),
            Self::RowText {
                row,
                expected,
                actual,
            } => write!(
                formatter,
                "championship setup row {row} text {actual:?} does not match canonical text {expected:?}"
            ),
            Self::RowValue { row, actual } => write!(
                formatter,
                "championship setup row {row} has invalid value {actual:?}"
            ),
            Self::Preview { expected, actual } => write!(
                formatter,
                "championship setup preview {actual:?} does not match selected-row preview {expected:?}"
            ),
        }
    }
}

impl std::error::Error for ChampionshipSetupVisualError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SetupTextIdentity {
    Localized(usize),
    Literal(&'static str),
    /// Formatting remains deferred until car specification descriptors land.
    DeferredCarSpecification(CanonicalCar),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SetupTextRole {
    Title,
    Row(SetupRowKind),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SetupTextCommand {
    pub role: SetupTextRole,
    pub identity: SetupTextIdentity,
    pub font: FontStyle,
    pub position: PixelPoint,
    pub flags: u8,
    /// Raw four-byte value retained by the recovered text command.
    pub source_tuple: [u8; 4],
    /// Raw signed Q16 multiplier applied to the first three tuple bytes.
    pub fade_q16: i32,
}

impl SetupTextCommand {
    /// Applies the recovered signed Q16 arithmetic without saturation.
    #[must_use]
    pub fn effective_tuple(self) -> [u8; 4] {
        let mut effective = self.source_tuple;
        for component in &mut effective[..3] {
            *component = i32::from(*component)
                .wrapping_mul(self.fade_q16)
                .wrapping_shr(16) as u8;
        }
        effective
    }
}

/// Parameters retained for the exact recovered info-text helper.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeferredInfoText {
    pub anchor_q16: Q16Point,
    pub frontend_offset_q16: Q16Point,
    pub primary: SetupTextIdentity,
    pub primary_font: FontStyle,
    pub secondary: SetupTextIdentity,
    pub secondary_font: FontStyle,
    pub flags: u8,
    pub horizontal_adjust_q16: i32,
    pub horizontal_variant_adjust_q16: i32,
    pub text_block_adjust_q16: i32,
    pub baseline_adjust_q16: i32,
    pub secondary_baseline_adjust_q16: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeferredCarTransformStep {
    ApplyPitchState,
    ApplyPreviewTransform,
    SubmitPlacement,
}

pub const CAR_TRANSFORM_STEPS: [DeferredCarTransformStep; 3] = [
    DeferredCarTransformStep::ApplyPitchState,
    DeferredCarTransformStep::ApplyPreviewTransform,
    DeferredCarTransformStep::SubmitPlacement,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChampionshipSetupVisualCommand {
    Text(SetupTextCommand),
    Selection {
        row: SetupRowKind,
        source: PixelRect,
        destination: PixelRect,
        color_key: u8,
    },
    InfoBox {
        source: PixelRect,
        destination: PixelRect,
    },
    DeferredInfoText(DeferredInfoText),
    DeferredCarTransform {
        car: CanonicalCar,
        angle_delta: u16,
        steps: [DeferredCarTransformStep; 3],
    },
    DeferredCarVisibilityLatch {
        car: CanonicalCar,
        tested_rectangle: PixelRect,
    },
    ManufacturerLogo {
        car: CanonicalCar,
        logo: ManufacturerLogo,
        source: PixelRect,
        destination: PixelRect,
    },
    PaletteRequest {
        group: u8,
        item: u8,
    },
    DeferredNextPreview {
        group: u8,
        item: u8,
        frame_count: u8,
        source: PixelRect,
        destination: PixelRect,
    },
}

pub fn plan_championship_setup_frame(
    input: ChampionshipSetupVisualInput<'_>,
    preview_cache: &mut ChampionshipPreviewCache,
) -> Result<Vec<ChampionshipSetupVisualCommand>, ChampionshipSetupVisualError> {
    validate_transcript(input.transcript)?;
    let mut commands = Vec::with_capacity(16);
    commands.push(ChampionshipSetupVisualCommand::Text(SetupTextCommand {
        role: SetupTextRole::Title,
        identity: SetupTextIdentity::Localized(TITLE_TEXT_INDEX),
        font: FontStyle::Title,
        position: scaled_point(
            input.viewport,
            TITLE_ANCHOR_Q16.wrapping_add(input.frontend_offset_q16),
        ),
        flags: TITLE_FLAGS,
        source_tuple: input.title_text_tuple,
        fade_q16: input.title_text_fade_q16,
    }));

    for index in 0..input.transcript.rows.len() {
        if !input.rows[index].visible {
            continue;
        }
        let row = input.transcript.rows[index];
        let kind = row_kind(index);
        let position = scaled_point(
            input.viewport,
            input.row_anchors_q16[index].wrapping_add(input.frontend_offset_q16),
        );
        if row.selected {
            commands.push(ChampionshipSetupVisualCommand::Selection {
                row: kind,
                source: SELECTION_SOURCE,
                destination: selection_destination(
                    input.viewport,
                    position,
                    input.selected_text_widths[index],
                ),
                color_key: 0,
            });
        }
        commands.push(ChampionshipSetupVisualCommand::Text(SetupTextCommand {
            role: SetupTextRole::Row(kind),
            identity: row_text_identity(row.value),
            font: row_font(row.selected, input.rows[index].enabled),
            position,
            flags: ROW_FLAGS,
            source_tuple: row_text_tuple(kind, input.rows[index], input.row_text_tuples),
            fade_q16: input.row_text_fade_q16,
        }));

        if row.selected {
            append_preview(&mut commands, &input, preview_cache);
        }
    }
    Ok(commands)
}

fn row_text_tuple(
    kind: SetupRowKind,
    presentation: RowPresentation,
    tuples: RowTextTupleBank,
) -> [u8; 4] {
    if kind == SetupRowKind::Next {
        tuples.terminal
    } else if !presentation.enabled {
        tuples.inactive
    } else {
        tuples.active
    }
}

fn append_preview(
    commands: &mut Vec<ChampionshipSetupVisualCommand>,
    input: &ChampionshipSetupVisualInput<'_>,
    preview_cache: &mut ChampionshipPreviewCache,
) {
    match input.transcript.preview {
        ActivePreview::Car { canonical_car, .. } | ActivePreview::Gear { canonical_car, .. } => {
            append_car_preview(commands, input, preview_cache, canonical_car)
        }
        ActivePreview::Next => append_next_preview(commands, input, preview_cache),
    }
}

fn append_car_preview(
    commands: &mut Vec<ChampionshipSetupVisualCommand>,
    input: &ChampionshipSetupVisualInput<'_>,
    preview_cache: &mut ChampionshipPreviewCache,
    car: CanonicalCar,
) {
    let base = scaled_point(
        input.viewport,
        input
            .preview_anchor_q16
            .wrapping_add(input.frontend_offset_q16),
    );
    let logo_destination = logo_destination(input);
    commands.push(ChampionshipSetupVisualCommand::InfoBox {
        source: INFO_BOX_SOURCE,
        destination: PixelRect::new(base.x.wrapping_add(142), base.y.wrapping_sub(2), 320, 132),
    });
    commands.push(ChampionshipSetupVisualCommand::DeferredInfoText(
        DeferredInfoText {
            anchor_q16: input.preview_anchor_q16,
            frontend_offset_q16: input.frontend_offset_q16,
            // The preview helper uses the separate short-name table here;
            // the full model-name table is used only by the car row.
            primary: SetupTextIdentity::Literal(car.object_name()),
            primary_font: INFO_PRIMARY_FONT,
            secondary: SetupTextIdentity::DeferredCarSpecification(car),
            secondary_font: INFO_SECONDARY_FONT,
            flags: ROW_FLAGS,
            horizontal_adjust_q16: 0x35c2,
            horizontal_variant_adjust_q16: 0x0353,
            text_block_adjust_q16: 0x8147,
            baseline_adjust_q16: 0x07ad,
            secondary_baseline_adjust_q16: 0x051e,
        },
    ));
    commands.push(ChampionshipSetupVisualCommand::DeferredCarTransform {
        car,
        angle_delta: CAR_ANGLE_DELTA,
        steps: CAR_TRANSFORM_STEPS,
    });
    commands.push(ChampionshipSetupVisualCommand::DeferredCarVisibilityLatch {
        car,
        tested_rectangle: logo_destination,
    });
    commands.push(ChampionshipSetupVisualCommand::ManufacturerLogo {
        car,
        logo: car.championship_descriptor().logo,
        source: PREVIEW_SOURCE,
        destination: logo_destination,
    });
    if preview_cache.car != Some(car) {
        commands.push(ChampionshipSetupVisualCommand::PaletteRequest {
            group: 4,
            item: car.index(),
        });
        preview_cache.car = Some(car);
        preview_cache.next_group = None;
    }
}

fn append_next_preview(
    commands: &mut Vec<ChampionshipSetupVisualCommand>,
    input: &ChampionshipSetupVisualInput<'_>,
    preview_cache: &mut ChampionshipPreviewCache,
) {
    let base = scaled_point(
        input.viewport,
        input
            .preview_anchor_q16
            .wrapping_add(input.frontend_offset_q16),
    );
    commands.push(ChampionshipSetupVisualCommand::InfoBox {
        source: INFO_BOX_SOURCE,
        destination: PixelRect::new(base.x.wrapping_add(142), base.y.wrapping_sub(2), 320, 132),
    });
    commands.push(ChampionshipSetupVisualCommand::DeferredInfoText(
        DeferredInfoText {
            anchor_q16: input.preview_anchor_q16,
            frontend_offset_q16: input.frontend_offset_q16,
            primary: SetupTextIdentity::Localized(0x18),
            primary_font: INFO_PRIMARY_FONT,
            secondary: SetupTextIdentity::Localized(next_context_text_index(input.next_context)),
            secondary_font: INFO_SECONDARY_FONT,
            flags: ROW_FLAGS,
            horizontal_adjust_q16: 0x35c2,
            horizontal_variant_adjust_q16: 0x0353,
            text_block_adjust_q16: 0x8147,
            baseline_adjust_q16: 0x07ad,
            secondary_baseline_adjust_q16: 0x051e,
        },
    ));
    if preview_cache.next_group != Some(9) {
        commands.push(ChampionshipSetupVisualCommand::PaletteRequest { group: 1, item: 9 });
        preview_cache.next_group = Some(9);
        preview_cache.car = None;
    }
    commands.push(ChampionshipSetupVisualCommand::DeferredNextPreview {
        group: 9,
        item: 0,
        frame_count: 15,
        source: PREVIEW_SOURCE,
        destination: PixelRect::new(base.x.wrapping_add(144), base.y, 182, 128),
    });
}

fn validate_transcript(
    transcript: &ChampionshipSetupRenderTranscript,
) -> Result<(), ChampionshipSetupVisualError> {
    if transcript.title_text_index != TITLE_TEXT_INDEX {
        return Err(ChampionshipSetupVisualError::TitleTextIndex {
            actual: transcript.title_text_index,
        });
    }

    let selected_count = transcript.rows.iter().filter(|row| row.selected).count();
    if selected_count != 1 {
        return Err(ChampionshipSetupVisualError::SelectedRowCount {
            actual: selected_count,
        });
    }
    if transcript.highlight_row >= transcript.rows.len() {
        return Err(ChampionshipSetupVisualError::HighlightRowOutOfRange {
            actual: transcript.highlight_row,
        });
    }
    let selected = transcript
        .rows
        .iter()
        .position(|row| row.selected)
        .expect("exactly one selected row was checked");
    if transcript.highlight_row != selected {
        return Err(ChampionshipSetupVisualError::HighlightRowMismatch {
            highlight: transcript.highlight_row,
            selected,
        });
    }

    let expected_tags = [CAR_ROW_TAG, GEAR_ROW_TAG, NEXT_ROW_TAG];
    let expected_texts = [
        SetupRowText::ChooseCar,
        SetupRowText::ChooseGear,
        SetupRowText::Localized(NEXT_ROW_TEXT_INDEX),
    ];
    for index in 0..transcript.rows.len() {
        let row = transcript.rows[index];
        if row.tag != expected_tags[index] {
            return Err(ChampionshipSetupVisualError::RowTag {
                row: index,
                expected: expected_tags[index],
                actual: row.tag,
            });
        }
        if row.text != expected_texts[index] {
            return Err(ChampionshipSetupVisualError::RowText {
                row: index,
                expected: expected_texts[index],
                actual: row.text,
            });
        }
    }

    let RowValue::Car(car) = transcript.rows[0].value else {
        return Err(ChampionshipSetupVisualError::RowValue {
            row: 0,
            actual: transcript.rows[0].value,
        });
    };
    let RowValue::Gear(gear) = transcript.rows[1].value else {
        return Err(ChampionshipSetupVisualError::RowValue {
            row: 1,
            actual: transcript.rows[1].value,
        });
    };
    if transcript.rows[2].value != RowValue::None {
        return Err(ChampionshipSetupVisualError::RowValue {
            row: 2,
            actual: transcript.rows[2].value,
        });
    }

    let expected_preview = match selected {
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
    };
    if transcript.preview != expected_preview {
        return Err(ChampionshipSetupVisualError::Preview {
            expected: expected_preview,
            actual: transcript.preview,
        });
    }
    Ok(())
}

#[must_use]
pub const fn next_context_text_index(context: NextPreviewContext) -> usize {
    let state = context.state_byte & 0x03;
    let mode = (context.mode_byte >> 3) & 0x03;
    match (state, mode) {
        (0, 0..=2) => 0xab + mode as usize,
        (1, 0..=2) => 0xae + mode as usize,
        _ => 0x21,
    }
}

const fn row_kind(index: usize) -> SetupRowKind {
    match index {
        0 => SetupRowKind::ChooseCar,
        1 => SetupRowKind::ChooseGear,
        _ => SetupRowKind::Next,
    }
}

const fn row_text_identity(value: RowValue) -> SetupTextIdentity {
    match value {
        RowValue::Car(car) => SetupTextIdentity::Literal(car.full_name()),
        RowValue::Gear(GearChoice::BitClear) => SetupTextIdentity::Localized(0x1d),
        RowValue::Gear(GearChoice::BitSet) => SetupTextIdentity::Localized(0x1e),
        RowValue::None => SetupTextIdentity::Localized(0x18),
    }
}

const fn row_font(selected: bool, enabled: bool) -> FontStyle {
    if selected {
        FontStyle::OptionRed
    } else if enabled {
        FontStyle::Options
    } else {
        FontStyle::OptionGrey
    }
}

fn logo_destination(input: &ChampionshipSetupVisualInput<'_>) -> PixelRect {
    let q16 = input
        .preview_anchor_q16
        .wrapping_add(input.frontend_offset_q16);
    PixelRect::new(
        scale_q16(input.viewport.width, q16.x.wrapping_add(0x3999)),
        scale_q16(input.viewport.height, q16.y),
        182,
        128,
    )
}

fn selection_destination(
    viewport: DisplayExtent,
    text_position: PixelPoint,
    text_width: u16,
) -> PixelRect {
    PixelRect::new(
        // `0x4c3cda..0x4c3d08` then `0x4c3d46`: both margins come off.
        text_position
            .x
            .wrapping_sub(scale_q16(viewport.width, 0x07ae))
            .wrapping_sub(scale_q16(viewport.width, 0x051e)),
        text_position.y.wrapping_sub(2),
        i32::from(text_width)
            .wrapping_add(scale_q16(viewport.width, 0x0f5c))
            .wrapping_add(scale_q16(viewport.width, 0x0a3d)),
        33,
    )
}

fn scaled_point(viewport: DisplayExtent, point: Q16Point) -> PixelPoint {
    PixelPoint::new(
        scale_q16(viewport.width, point.x),
        scale_q16(viewport.height, point.y),
    )
}

fn scale_q16(extent: u16, value: i32) -> i32 {
    i32::from(extent).wrapping_mul(value) >> 16
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::championship_setup::{
        CAR_ROW_TAG, GEAR_ROW_TAG, NEXT_ROW_TAG, RenderRow, SetupRowText,
    };

    fn transcript(
        selected: usize,
        car: CanonicalCar,
        gear: GearChoice,
    ) -> ChampionshipSetupRenderTranscript {
        let rows = [
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
        ];
        let preview = match selected {
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
        };
        ChampionshipSetupRenderTranscript {
            title_text_index: 0,
            rows,
            highlight_row: selected,
            preview,
        }
    }

    fn input<'a>(
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

    fn plan(input: ChampionshipSetupVisualInput<'_>) -> Vec<ChampionshipSetupVisualCommand> {
        plan_championship_setup_frame(input, &mut ChampionshipPreviewCache::new()).unwrap()
    }

    fn text(command: &ChampionshipSetupVisualCommand) -> &SetupTextCommand {
        let ChampionshipSetupVisualCommand::Text(text) = command else {
            panic!("expected text command, got {command:?}")
        };
        text
    }

    #[test]
    fn initial_next_selection_has_exact_order_fonts_and_context_preview() {
        let transcript = transcript(2, CanonicalCar::Subaru, GearChoice::BitClear);
        let commands = plan(input(&transcript));
        assert_eq!(commands.len(), 9);
        assert_eq!(text(&commands[0]).font, FontStyle::Title);
        assert_eq!(
            text(&commands[0]).identity,
            SetupTextIdentity::Localized(0)
        );
        assert_eq!(text(&commands[1]).font, FontStyle::Options);
        assert_eq!(text(&commands[2]).font, FontStyle::Options);
        assert!(matches!(
            commands[3],
            ChampionshipSetupVisualCommand::Selection {
                row: SetupRowKind::Next,
                ..
            }
        ));
        assert_eq!(text(&commands[4]).font, FontStyle::OptionRed);
        assert!(matches!(
            commands[5],
            ChampionshipSetupVisualCommand::InfoBox { .. }
        ));
        assert!(matches!(
            commands[6],
            ChampionshipSetupVisualCommand::DeferredInfoText(DeferredInfoText {
                primary: SetupTextIdentity::Localized(0x18),
                secondary: SetupTextIdentity::Localized(0xab),
                primary_font: FontStyle::BlueInfo,
                secondary_font: FontStyle::GreenInfo,
                ..
            })
        ));
        assert!(matches!(
            commands[8],
            ChampionshipSetupVisualCommand::DeferredNextPreview {
                group: 9,
                item: 0,
                frame_count: 15,
                ..
            }
        ));
    }

    #[test]
    fn text_tuples_follow_terminal_then_inactive_then_active_mapping() {
        let transcript = transcript(2, CanonicalCar::Subaru, GearChoice::BitClear);
        let mut input = input(&transcript);
        input.rows[0].enabled = true;
        input.rows[1].enabled = false;
        input.rows[2].enabled = false;
        let commands = plan(input);
        let texts = commands
            .iter()
            .filter_map(|command| match command {
                ChampionshipSetupVisualCommand::Text(text) => Some(text),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(texts[0].source_tuple, [1, 2, 3, 4]);
        assert_eq!(texts[1].source_tuple, [13, 14, 15, 16]);
        assert_eq!(texts[2].source_tuple, [9, 10, 11, 12]);
        assert_eq!(texts[3].source_tuple, [5, 6, 7, 8]);
    }

    #[test]
    fn effective_tuple_uses_signed_q16_low_bytes_and_preserves_byte_three() {
        let mut command = SetupTextCommand {
            role: SetupTextRole::Title,
            identity: SetupTextIdentity::Localized(TITLE_TEXT_INDEX),
            font: FontStyle::Title,
            position: PixelPoint::new(0, 0),
            flags: TITLE_FLAGS,
            source_tuple: [0x20, 0x80, 0xff, 0x7b],
            fade_q16: 0x1_0000,
        };
        assert_eq!(command.effective_tuple(), [0x20, 0x80, 0xff, 0x7b]);

        command.fade_q16 = 0x8000;
        assert_eq!(command.effective_tuple(), [0x10, 0x40, 0x7f, 0x7b]);

        command.source_tuple = [1, 2, 255, 0xa5];
        command.fade_q16 = -0x1_0000;
        assert_eq!(command.effective_tuple(), [0xff, 0xfe, 1, 0xa5]);
    }

    #[test]
    fn selection_info_and_logo_geometry_match_640_by_480() {
        let transcript = transcript(0, CanonicalCar::Subaru, GearChoice::BitClear);
        let commands = plan(input(&transcript));
        assert_eq!(
            commands[1],
            ChampionshipSetupVisualCommand::Selection {
                row: SetupRowKind::ChooseCar,
                source: SELECTION_SOURCE,
                destination: PixelRect::new(609, 238, 163, 33),
                color_key: 0,
            }
        );
        assert!(commands.contains(&ChampionshipSetupVisualCommand::InfoBox {
            source: INFO_BOX_SOURCE,
            destination: PixelRect::new(462, 118, 320, 132),
        }));
        assert!(
            commands.contains(&ChampionshipSetupVisualCommand::ManufacturerLogo {
                car: CanonicalCar::Subaru,
                logo: ManufacturerLogo::Subaru,
                source: PREVIEW_SOURCE,
                destination: PixelRect::new(463, 120, 182, 128),
            })
        );
    }

    #[test]
    fn geometry_scales_each_q16_term_at_another_extent_and_offset() {
        let transcript = transcript(0, CanonicalCar::Celica, GearChoice::BitSet);
        let mut input = input(&transcript);
        input.viewport = DisplayExtent {
            width: 800,
            height: 600,
        };
        input.frontend_offset_q16 = Q16Point::new(0x2000, -0x1000);
        input.preview_anchor_q16 = Q16Point::new(0x6000, 0x5000);
        input.selected_text_widths[0] = 77;
        let commands = plan(input);
        assert_eq!(
            commands[1],
            ChampionshipSetupVisualCommand::Selection {
                row: SetupRowKind::ChooseCar,
                source: SELECTION_SOURCE,
                destination: PixelRect::new(862, 260, 155, 33),
                color_key: 0,
            }
        );
        assert!(commands.contains(&ChampionshipSetupVisualCommand::InfoBox {
            source: INFO_BOX_SOURCE,
            destination: PixelRect::new(542, 148, 320, 132),
        }));
        assert!(
            commands.contains(&ChampionshipSetupVisualCommand::ManufacturerLogo {
                car: CanonicalCar::Celica,
                logo: ManufacturerLogo::Toyo,
                source: PREVIEW_SOURCE,
                destination: PixelRect::new(579, 150, 182, 128),
            })
        );
    }

    #[test]
    fn hidden_rows_emit_nothing_and_disabled_rows_use_grey_unless_selected() {
        let transcript = transcript(1, CanonicalCar::Subaru, GearChoice::BitSet);
        let mut input = input(&transcript);
        input.rows = [
            RowPresentation {
                visible: false,
                enabled: true,
            },
            RowPresentation {
                visible: true,
                enabled: false,
            },
            RowPresentation {
                visible: true,
                enabled: false,
            },
        ];
        let commands = plan(input);
        let texts = commands
            .iter()
            .filter_map(|command| {
                if let ChampionshipSetupVisualCommand::Text(text) = command {
                    Some(text)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(texts.len(), 3);
        assert_eq!(texts[1].role, SetupTextRole::Row(SetupRowKind::ChooseGear));
        assert_eq!(texts[1].font, FontStyle::OptionRed);
        assert_eq!(texts[2].role, SetupTextRole::Row(SetupRowKind::Next));
        assert_eq!(texts[2].font, FontStyle::OptionGrey);
    }

    #[test]
    fn all_full_car_names_are_exact_literals() {
        let expected = [
            "SUBARU IMPREZA WRC",
            "SEAT IBIZA KIT CAR EVO2",
            "SKODA FELICIA KIT CAR",
            "AUDI QUATTRO",
            "FORD RS200",
            "FORD ESCORT MK II",
            "MITSUBISHI LANCER E4",
            "VW GOLF GTI KIT CAR",
            "LANCIA DELTA INTEGRALE",
            "TOYOTA COROLLA WRC",
            "FORD ESCORT WRC",
            "RENAULT MAXI MEGANE",
            "MILLENNIUM 4 RALLYE",
            "TOYOTA CELICA GT4 WRC",
        ];
        assert_eq!(CanonicalCar::ALL.map(CanonicalCar::full_name), expected);
        for car in CanonicalCar::ALL {
            let transcript = transcript(2, car, GearChoice::BitClear);
            let commands = plan(input(&transcript));
            assert_eq!(
                text(&commands[1]).identity,
                SetupTextIdentity::Literal(car.full_name())
            );
        }
    }

    #[test]
    fn gear_row_uses_both_exact_localization_indices() {
        for (gear, expected) in [(GearChoice::BitClear, 0x1d), (GearChoice::BitSet, 0x1e)] {
            let transcript = transcript(2, CanonicalCar::Subaru, gear);
            let commands = plan(input(&transcript));
            assert_eq!(
                text(&commands[2]).identity,
                SetupTextIdentity::Localized(expected)
            );
        }
    }

    #[test]
    fn car_and_gear_rows_emit_the_same_car_preview_identity() {
        for selected in [0, 1] {
            let transcript = transcript(selected, CanonicalCar::Celica, GearChoice::BitSet);
            let commands = plan(input(&transcript));
            assert!(
                commands.contains(&ChampionshipSetupVisualCommand::ManufacturerLogo {
                    car: CanonicalCar::Celica,
                    logo: ManufacturerLogo::Toyo,
                    source: PREVIEW_SOURCE,
                    destination: PixelRect::new(463, 120, 182, 128),
                })
            );
            assert!(
                commands.contains(&ChampionshipSetupVisualCommand::DeferredCarTransform {
                    car: CanonicalCar::Celica,
                    angle_delta: 0x16,
                    steps: CAR_TRANSFORM_STEPS,
                })
            );
        }
    }

    #[test]
    fn paired_preview_cache_preserves_exact_requests_and_cross_invalidations() {
        let car = transcript(0, CanonicalCar::Celica, GearChoice::BitClear);
        let gear = transcript(1, CanonicalCar::Celica, GearChoice::BitSet);
        let next = transcript(2, CanonicalCar::Celica, GearChoice::BitSet);
        let mut cache = ChampionshipPreviewCache::new();

        let first_car = plan_championship_setup_frame(input(&car), &mut cache).unwrap();
        let request_index = first_car
            .iter()
            .position(|command| {
                *command
                    == ChampionshipSetupVisualCommand::PaletteRequest {
                        group: 4,
                        item: CanonicalCar::Celica.index(),
                    }
            })
            .unwrap();
        assert!(matches!(
            first_car[request_index - 1],
            ChampionshipSetupVisualCommand::ManufacturerLogo {
                car: CanonicalCar::Celica,
                logo: ManufacturerLogo::Toyo,
                ..
            }
        ));

        let same_car_through_gear =
            plan_championship_setup_frame(input(&gear), &mut cache).unwrap();
        assert!(!has_palette_request(&same_car_through_gear, 4));

        let first_next = plan_championship_setup_frame(input(&next), &mut cache).unwrap();
        let next_request = first_next
            .iter()
            .position(|command| {
                *command == ChampionshipSetupVisualCommand::PaletteRequest { group: 1, item: 9 }
            })
            .unwrap();
        assert!(matches!(
            first_next[next_request - 1],
            ChampionshipSetupVisualCommand::DeferredInfoText(_)
        ));
        assert!(matches!(
            first_next[next_request + 1],
            ChampionshipSetupVisualCommand::DeferredNextPreview { group: 9, .. }
        ));
        assert!(!has_palette_request(
            &plan_championship_setup_frame(input(&next), &mut cache).unwrap(),
            1
        ));
        assert!(has_palette_request(
            &plan_championship_setup_frame(input(&car), &mut cache).unwrap(),
            4
        ));
    }

    #[test]
    fn next_context_indices_cover_both_states_three_modes_and_fallbacks() {
        for state in 0..=1 {
            for mode in 0..=2 {
                assert_eq!(
                    next_context_text_index(NextPreviewContext {
                        state_byte: state,
                        mode_byte: mode << 3,
                    }),
                    if state == 0 {
                        0xab + mode as usize
                    } else {
                        0xae + mode as usize
                    }
                );
            }
        }
        assert_eq!(
            next_context_text_index(NextPreviewContext {
                state_byte: 2,
                mode_byte: 0
            }),
            0x21
        );
        assert_eq!(
            next_context_text_index(NextPreviewContext {
                state_byte: 0,
                mode_byte: 3 << 3
            }),
            0x21
        );
    }

    #[test]
    fn hidden_selected_callbacks_do_not_mutate_either_preview_cache() {
        let car = transcript(0, CanonicalCar::Subaru, GearChoice::BitClear);
        let gear = transcript(1, CanonicalCar::Subaru, GearChoice::BitClear);
        let next = transcript(2, CanonicalCar::Subaru, GearChoice::BitClear);
        let mut cache = ChampionshipPreviewCache::new();

        let mut hidden_car = input(&car);
        hidden_car.rows[0].visible = false;
        assert!(!has_palette_request(
            &plan_championship_setup_frame(hidden_car, &mut cache).unwrap(),
            4
        ));
        assert!(has_palette_request(
            &plan_championship_setup_frame(input(&car), &mut cache).unwrap(),
            4
        ));

        let mut hidden_next = input(&next);
        hidden_next.rows[2].visible = false;
        assert!(!has_palette_request(
            &plan_championship_setup_frame(hidden_next, &mut cache).unwrap(),
            1
        ));
        assert!(!has_palette_request(
            &plan_championship_setup_frame(input(&gear), &mut cache).unwrap(),
            4
        ));

        assert!(has_palette_request(
            &plan_championship_setup_frame(input(&next), &mut cache).unwrap(),
            1
        ));
        let mut hidden_car_after_next = input(&car);
        hidden_car_after_next.rows[0].visible = false;
        assert!(!has_palette_request(
            &plan_championship_setup_frame(hidden_car_after_next, &mut cache).unwrap(),
            4
        ));
        assert!(has_palette_request(
            &plan_championship_setup_frame(input(&car), &mut cache).unwrap(),
            4
        ));
    }

    #[test]
    fn forged_transcripts_return_typed_errors_without_mutating_preview_cache() {
        let valid = transcript(0, CanonicalCar::Celica, GearChoice::BitSet);
        let mut cache = ChampionshipPreviewCache::new();
        let _ = plan_championship_setup_frame(input(&valid), &mut cache).unwrap();
        let established = cache;

        let mut forged = valid.clone();
        forged.title_text_index = 0x14;
        assert_rejected_without_cache_effect(
            &forged,
            &mut cache,
            established,
            ChampionshipSetupVisualError::TitleTextIndex { actual: 0x14 },
        );

        forged = valid.clone();
        forged.rows[0].selected = false;
        assert_rejected_without_cache_effect(
            &forged,
            &mut cache,
            established,
            ChampionshipSetupVisualError::SelectedRowCount { actual: 0 },
        );

        forged = valid.clone();
        forged.rows[1].selected = true;
        assert_rejected_without_cache_effect(
            &forged,
            &mut cache,
            established,
            ChampionshipSetupVisualError::SelectedRowCount { actual: 2 },
        );

        forged = valid.clone();
        forged.highlight_row = 3;
        assert_rejected_without_cache_effect(
            &forged,
            &mut cache,
            established,
            ChampionshipSetupVisualError::HighlightRowOutOfRange { actual: 3 },
        );

        forged = valid.clone();
        forged.highlight_row = 1;
        assert_rejected_without_cache_effect(
            &forged,
            &mut cache,
            established,
            ChampionshipSetupVisualError::HighlightRowMismatch {
                highlight: 1,
                selected: 0,
            },
        );

        forged = valid.clone();
        forged.rows[0].tag = GEAR_ROW_TAG;
        assert_rejected_without_cache_effect(
            &forged,
            &mut cache,
            established,
            ChampionshipSetupVisualError::RowTag {
                row: 0,
                expected: CAR_ROW_TAG,
                actual: GEAR_ROW_TAG,
            },
        );

        forged = valid.clone();
        forged.rows[1].text = SetupRowText::Localized(0x18);
        assert_rejected_without_cache_effect(
            &forged,
            &mut cache,
            established,
            ChampionshipSetupVisualError::RowText {
                row: 1,
                expected: SetupRowText::ChooseGear,
                actual: SetupRowText::Localized(0x18),
            },
        );

        forged = valid.clone();
        forged.rows[2].value = RowValue::Car(CanonicalCar::Subaru);
        assert_rejected_without_cache_effect(
            &forged,
            &mut cache,
            established,
            ChampionshipSetupVisualError::RowValue {
                row: 2,
                actual: RowValue::Car(CanonicalCar::Subaru),
            },
        );

        forged = valid.clone();
        forged.preview = ActivePreview::Car {
            canonical_car: CanonicalCar::Subaru,
            object_name: "SUBARU",
            logo_name: "SUBARU",
        };
        assert_rejected_without_cache_effect(
            &forged,
            &mut cache,
            established,
            ChampionshipSetupVisualError::Preview {
                expected: valid.preview,
                actual: forged.preview,
            },
        );

        let gear_selected = transcript(1, CanonicalCar::Celica, GearChoice::BitSet);
        forged = gear_selected.clone();
        forged.preview = ActivePreview::Gear {
            gear: GearChoice::BitClear,
            canonical_car: CanonicalCar::Celica,
            object_name: "CELICA",
            logo_name: "TOYO",
        };
        assert_rejected_without_cache_effect(
            &forged,
            &mut cache,
            established,
            ChampionshipSetupVisualError::Preview {
                expected: gear_selected.preview,
                actual: forged.preview,
            },
        );
    }

    fn has_palette_request(commands: &[ChampionshipSetupVisualCommand], group: u8) -> bool {
        commands.iter().any(|command| {
            matches!(
                command,
                ChampionshipSetupVisualCommand::PaletteRequest {
                    group: actual,
                    ..
                } if *actual == group
            )
        })
    }

    fn assert_rejected_without_cache_effect(
        transcript: &ChampionshipSetupRenderTranscript,
        cache: &mut ChampionshipPreviewCache,
        expected_cache: ChampionshipPreviewCache,
        expected_error: ChampionshipSetupVisualError,
    ) {
        assert_eq!(
            plan_championship_setup_frame(input(transcript), cache),
            Err(expected_error)
        );
        assert_eq!(*cache, expected_cache);
    }
}
