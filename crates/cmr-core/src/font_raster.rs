//! Backend-neutral execution of precomputed legacy bitmap-font glyph plans.

use std::fmt;

use cmr_formats::font::{BitmapFont, GlyphBlit, RectI32, TextPlan};
use cmr_formats::riff_palette::PaletteEntry;

use crate::blit::{BlitDecision, ColorKey, RectI16, plan_legacy_blit};
use crate::display::{DisplayExtent, IndexedVideoBackend};
use crate::surface::IndexedSurface;

const INDEX_ZERO_COLOR_KEY: ColorKey = ColorKey { low: 0, high: 0 };

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FontAtlasConversionError {
    PaletteEntryCount(usize),
    PixelCount { expected: u64, actual: usize },
}

impl fmt::Display for FontAtlasConversionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PaletteEntryCount(count) => {
                write!(
                    formatter,
                    "font atlas has {count} palette entries; expected 256"
                )
            }
            Self::PixelCount { expected, actual } => write!(
                formatter,
                "font atlas expects {expected} pixels, found {actual}"
            ),
        }
    }
}

impl std::error::Error for FontAtlasConversionError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FontRasterLayoutError {
    InvalidDestinationExtent(DisplayExtent),
    GlyphSourceOutOfRange {
        blit_index: usize,
        code: u8,
        source: cmr_formats::font::RectI16,
        atlas_width: u32,
        atlas_height: u32,
    },
    GlyphDimensionsDiffer {
        blit_index: usize,
        code: u8,
        source: cmr_formats::font::RectI16,
        destination: RectI32,
    },
    GlyphDoesNotMatchFont {
        blit_index: usize,
        code: u8,
        glyph_index: u8,
        source: cmr_formats::font::RectI16,
    },
    GlyphDestinationOutOfRange {
        blit_index: usize,
        code: u8,
        destination: RectI32,
    },
}

impl fmt::Display for FontRasterLayoutError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDestinationExtent(extent) => write!(
                formatter,
                "font destination extent {}x{} is outside the positive i16 range",
                extent.width, extent.height
            ),
            Self::GlyphSourceOutOfRange {
                blit_index,
                code,
                source,
                atlas_width,
                atlas_height,
            } => write!(
                formatter,
                "font glyph blit {blit_index} for byte {code:#04x} has source {source:?} outside the positive {atlas_width}x{atlas_height} atlas"
            ),
            Self::GlyphDimensionsDiffer {
                blit_index,
                code,
                source,
                destination,
            } => write!(
                formatter,
                "font glyph blit {blit_index} for byte {code:#04x} would stretch source {source:?} to destination {destination:?}"
            ),
            Self::GlyphDoesNotMatchFont {
                blit_index,
                code,
                glyph_index,
                source,
            } => write!(
                formatter,
                "font glyph blit {blit_index} for byte {code:#04x} and glyph index {glyph_index} with source {source:?} does not match the supplied font"
            ),
            Self::GlyphDestinationOutOfRange {
                blit_index,
                code,
                destination,
            } => write!(
                formatter,
                "font glyph blit {blit_index} for byte {code:#04x} has destination {destination:?} outside the i16 rectangle range"
            ),
        }
    }
}

impl std::error::Error for FontRasterLayoutError {}

#[derive(Debug, Eq, PartialEq)]
pub enum FontRasterError<E> {
    Atlas(FontAtlasConversionError),
    Layout(FontRasterLayoutError),
    CreateSource(E),
}

impl<E: fmt::Display> fmt::Display for FontRasterError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Atlas(error) => error.fmt(formatter),
            Self::Layout(error) => error.fmt(formatter),
            Self::CreateSource(error) => {
                write!(formatter, "cannot create font atlas source: {error}")
            }
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for FontRasterError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Atlas(error) => Some(error),
            Self::Layout(error) => Some(error),
            Self::CreateSource(error) => Some(error),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct FontBlitDiagnostic<E> {
    pub blit_index: usize,
    pub code: u8,
    pub glyph_index: u8,
    pub source: E,
}

#[derive(Debug, Eq, PartialEq)]
pub struct FontRasterOutcome<E> {
    pub planned_glyphs: usize,
    pub no_ops: usize,
    pub successful_blits: usize,
    pub diagnostics: Vec<FontBlitDiagnostic<E>>,
}

/// Converts a parsed font atlas without remapping its indices or palette.
pub fn font_atlas_surface(font: &BitmapFont) -> Result<IndexedSurface, FontAtlasConversionError> {
    let atlas = font.atlas();
    if atlas.palette.len() != 256 {
        return Err(FontAtlasConversionError::PaletteEntryCount(
            atlas.palette.len(),
        ));
    }
    let expected = u64::from(atlas.width) * u64::from(atlas.height);
    if u64::try_from(atlas.pixels.len()).ok() != Some(expected) {
        return Err(FontAtlasConversionError::PixelCount {
            expected,
            actual: atlas.pixels.len(),
        });
    }
    let palette = std::array::from_fn(|index| {
        let entry = atlas.palette[index];
        PaletteEntry {
            red: entry.red,
            green: entry.green,
            blue: entry.blue,
            flags: entry.reserved,
        }
    });
    Ok(IndexedSurface {
        width: atlas.width,
        height: atlas.height,
        pitch: atlas.width,
        pixels: atlas.pixels.clone(),
        palette,
    })
}

/// Uploads one atlas source and executes every visible glyph plan in order.
///
/// Individual backend blit failures are diagnostics and do not stop later
/// glyphs. A successfully created source is always destroyed after the plan.
/// This per-plan source lifetime is a deliberate compatibility bridge and
/// differs from the recovered executable's persistent startup font surfaces;
/// runtime integration should replace it with a persistent uploaded font pool.
pub fn execute_text_plan<B: IndexedVideoBackend>(
    backend: &mut B,
    font: &BitmapFont,
    plan: &TextPlan,
    destination: B::Surface,
    destination_extent: DisplayExtent,
) -> Result<FontRasterOutcome<B::Error>, FontRasterError<B::Error>> {
    let width = positive_i16(destination_extent.width)
        .ok_or(FontRasterLayoutError::InvalidDestinationExtent(
            destination_extent,
        ))
        .map_err(FontRasterError::Layout)?;
    let height = positive_i16(destination_extent.height)
        .ok_or(FontRasterLayoutError::InvalidDestinationExtent(
            destination_extent,
        ))
        .map_err(FontRasterError::Layout)?;
    for (blit_index, glyph) in plan.blits.iter().enumerate() {
        validate_glyph_source(font, blit_index, glyph).map_err(FontRasterError::Layout)?;
        destination_rectangle(blit_index, glyph).map_err(FontRasterError::Layout)?;
    }
    let atlas = font_atlas_surface(font).map_err(FontRasterError::Atlas)?;
    let source = backend
        .create_source(&atlas)
        .map_err(FontRasterError::CreateSource)?;

    let mut outcome = FontRasterOutcome {
        planned_glyphs: plan.blits.len(),
        no_ops: 0,
        successful_blits: 0,
        diagnostics: Vec::new(),
    };
    for (blit_index, glyph) in plan.blits.iter().enumerate() {
        let destination_rectangle = destination_rectangle(blit_index, glyph)
            .expect("all glyph rectangles were validated before source creation");
        match plan_legacy_blit(
            RectI16 {
                x: glyph.source.x,
                y: glyph.source.y,
                width: glyph.source.width,
                height: glyph.source.height,
            },
            destination_rectangle,
            width,
            height,
            Some(INDEX_ZERO_COLOR_KEY),
        ) {
            BlitDecision::NoOp => outcome.no_ops += 1,
            BlitDecision::Blit(blit_plan) => match backend.blit(source, destination, blit_plan) {
                Ok(()) => outcome.successful_blits += 1,
                Err(error) => outcome.diagnostics.push(FontBlitDiagnostic {
                    blit_index,
                    code: glyph.code,
                    glyph_index: glyph.glyph_index,
                    source: error,
                }),
            },
        }
    }
    backend.destroy_source(source);
    Ok(outcome)
}

fn validate_glyph_source(
    font: &BitmapFont,
    blit_index: usize,
    glyph: &GlyphBlit,
) -> Result<(), FontRasterLayoutError> {
    let source = glyph.source;
    let atlas = font.atlas();
    let source_right = i64::from(source.x) + i64::from(source.width);
    let source_bottom = i64::from(source.y) + i64::from(source.height);
    if source.x < 0
        || source.y < 0
        || source.width <= 0
        || source.height <= 0
        || source_right > i64::from(atlas.width)
        || source_bottom > i64::from(atlas.height)
    {
        return Err(FontRasterLayoutError::GlyphSourceOutOfRange {
            blit_index,
            code: glyph.code,
            source,
            atlas_width: atlas.width,
            atlas_height: atlas.height,
        });
    }
    if i32::from(source.width) != glyph.destination.width
        || i32::from(source.height) != glyph.destination.height
    {
        return Err(FontRasterLayoutError::GlyphDimensionsDiffer {
            blit_index,
            code: glyph.code,
            source,
            destination: glyph.destination,
        });
    }
    let indexed = font.glyphs().get(usize::from(glyph.glyph_index));
    let selected = font.glyph(glyph.code);
    if indexed.is_none()
        || selected.is_none()
        || !std::ptr::eq(
            indexed.expect("checked above"),
            selected.expect("checked above"),
        )
        || indexed.expect("checked above").source != source
    {
        return Err(FontRasterLayoutError::GlyphDoesNotMatchFont {
            blit_index,
            code: glyph.code,
            glyph_index: glyph.glyph_index,
            source,
        });
    }
    Ok(())
}

fn positive_i16(value: u16) -> Option<i16> {
    let converted = i16::try_from(value).ok()?;
    (converted > 0).then_some(converted)
}

fn destination_rectangle(
    blit_index: usize,
    glyph: &GlyphBlit,
) -> Result<RectI16, FontRasterLayoutError> {
    let destination = glyph.destination;
    let rectangle = RectI16 {
        x: i16::try_from(destination.x).map_err(|_| layout_error(blit_index, glyph))?,
        y: i16::try_from(destination.y).map_err(|_| layout_error(blit_index, glyph))?,
        width: i16::try_from(destination.width).map_err(|_| layout_error(blit_index, glyph))?,
        height: i16::try_from(destination.height).map_err(|_| layout_error(blit_index, glyph))?,
    };
    if rectangle.width <= 0 || rectangle.height <= 0 {
        return Err(layout_error(blit_index, glyph));
    }
    Ok(rectangle)
}

fn layout_error(blit_index: usize, glyph: &GlyphBlit) -> FontRasterLayoutError {
    FontRasterLayoutError::GlyphDestinationOutOfRange {
        blit_index,
        code: glyph.code,
        destination: glyph.destination,
    }
}
