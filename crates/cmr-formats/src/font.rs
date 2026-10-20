use std::fmt;

use crate::bitmap::{BitmapMetadata, BitmapPaletteEntry, IndexedBitmap};

const FILE_HEADER_SIZE: usize = 14;
const INFO_HEADER_SIZE: usize = 40;
const FIXED_PIXEL_OFFSET: usize = 0x436;
const FONT_HEADER_SIZE: usize = 24;
const GLYPH_RECORD_SIZE: usize = 16;
const PALETTE_ENTRIES: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FontLimits {
    pub max_file_bytes: usize,
    pub max_width: u32,
    pub max_height: u32,
    pub max_pixels: u64,
    pub max_glyphs: usize,
}

impl Default for FontLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: 16 * 1024 * 1024,
            max_width: 4096,
            max_height: 4096,
            max_pixels: 16 * 1024 * 1024,
            max_glyphs: i8::MAX as usize,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FontMetrics {
    pub base_height: u32,
    pub special_advance: u32,
    pub tracking: u32,
    pub line_gap: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RectI16 {
    pub x: i16,
    pub y: i16,
    pub width: i16,
    pub height: i16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FontGlyph {
    pub code: u8,
    pub source: RectI16,
    /// The complete legacy record, including currently uninterpreted bytes.
    pub raw_record: [u8; GLYPH_RECORD_SIZE],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BitmapFont {
    atlas: IndexedBitmap,
    metrics: FontMetrics,
    /// The complete 24-byte legacy header. Bytes without known semantics remain
    /// available for later reconstruction work.
    raw_header: [u8; FONT_HEADER_SIZE],
    glyphs: Vec<FontGlyph>,
    lookup: [Option<u8>; 256],
}

impl BitmapFont {
    pub fn atlas(&self) -> &IndexedBitmap {
        &self.atlas
    }

    pub const fn metrics(&self) -> FontMetrics {
        self.metrics
    }

    pub const fn raw_header(&self) -> &[u8; FONT_HEADER_SIZE] {
        &self.raw_header
    }

    pub fn glyphs(&self) -> &[FontGlyph] {
        &self.glyphs
    }

    pub fn glyph(&self, code: u8) -> Option<&FontGlyph> {
        self.lookup[usize::from(code)].and_then(|index| self.glyphs.get(usize::from(index)))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FontError {
    FileLimitExceeded { actual: usize, limit: usize },
    Truncated { offset: usize, needed: usize },
    InvalidDimensions { width: i32, height: i32 },
    DimensionLimitExceeded,
    UnsupportedBitDepth(u16),
    UnsupportedCompression(u32),
    NegativeGlyphCount(i8),
    GlyphLimitExceeded { count: usize, limit: usize },
    ArithmeticOverflow,
}

impl fmt::Display for FontError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FileLimitExceeded { actual, limit } => {
                write!(formatter, "font has {actual} bytes; limit is {limit}")
            }
            Self::Truncated { offset, needed } => write!(
                formatter,
                "font is truncated at offset {offset:#x}; {needed} bytes are required"
            ),
            Self::InvalidDimensions { width, height } => {
                write!(formatter, "invalid font atlas dimensions {width}x{height}")
            }
            Self::DimensionLimitExceeded => {
                write!(formatter, "font atlas dimensions exceed configured limits")
            }
            Self::UnsupportedBitDepth(depth) => {
                write!(formatter, "unsupported font atlas depth {depth}")
            }
            Self::UnsupportedCompression(compression) => {
                write!(
                    formatter,
                    "unsupported font atlas compression {compression}"
                )
            }
            Self::NegativeGlyphCount(count) => {
                write!(formatter, "font has negative signed glyph count {count}")
            }
            Self::GlyphLimitExceeded { count, limit } => {
                write!(formatter, "font has {count} glyphs; limit is {limit}")
            }
            Self::ArithmeticOverflow => write!(formatter, "font offset arithmetic overflowed"),
        }
    }
}

impl std::error::Error for FontError {}

/// Parses the fixed-layout bitmap-font container used by the legacy frontend.
///
/// The observed compatibility checks require an 8-bit, uncompressed atlas and
/// locate the font trailer after `0x436 + aligned_stride * height`, without
/// consulting other BMP header fields. Positive dimensions, checked spans,
/// resource limits are reconstruction safety policy. Glyph rectangles are
/// preserved as signed metadata even when they are not valid atlas sources;
/// render backends validate a selected glyph before submitting it. Bytes after
/// the glyph table are ignored.
pub fn parse_legacy_font(input: &[u8], limits: FontLimits) -> Result<BitmapFont, FontError> {
    if input.len() > limits.max_file_bytes {
        return Err(FontError::FileLimitExceeded {
            actual: input.len(),
            limit: limits.max_file_bytes,
        });
    }
    require(input, 0, FILE_HEADER_SIZE + INFO_HEADER_SIZE)?;
    let width_signed = read_i32(input, 18)?;
    let height_signed = read_i32(input, 22)?;
    if width_signed <= 0 || height_signed <= 0 {
        return Err(FontError::InvalidDimensions {
            width: width_signed,
            height: height_signed,
        });
    }
    let width = width_signed as u32;
    let height = height_signed as u32;
    let pixel_count = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or(FontError::ArithmeticOverflow)?;
    if width > limits.max_width || height > limits.max_height || pixel_count > limits.max_pixels {
        return Err(FontError::DimensionLimitExceeded);
    }

    let depth = read_u16(input, 28)?;
    if depth != 8 {
        return Err(FontError::UnsupportedBitDepth(depth));
    }
    let compression = read_u32(input, 30)?;
    if compression != 0 {
        return Err(FontError::UnsupportedCompression(compression));
    }
    let important_colors = read_u32(input, 50)?;
    require(
        input,
        FILE_HEADER_SIZE + INFO_HEADER_SIZE,
        PALETTE_ENTRIES * 4,
    )?;

    let stride = usize::try_from(width)
        .map_err(|_| FontError::ArithmeticOverflow)?
        .checked_add(3)
        .ok_or(FontError::ArithmeticOverflow)?
        & !3;
    let pixel_bytes = stride
        .checked_mul(usize::try_from(height).map_err(|_| FontError::ArithmeticOverflow)?)
        .ok_or(FontError::ArithmeticOverflow)?;
    let trailer_offset = FIXED_PIXEL_OFFSET
        .checked_add(pixel_bytes)
        .ok_or(FontError::ArithmeticOverflow)?;
    require(input, FIXED_PIXEL_OFFSET, pixel_bytes)?;
    require(input, trailer_offset, FONT_HEADER_SIZE)?;

    let raw_header: [u8; FONT_HEADER_SIZE] = input
        [trailer_offset..trailer_offset + FONT_HEADER_SIZE]
        .try_into()
        .expect("fixed-size slice");
    let signed_count = raw_header[1] as i8;
    if signed_count < 0 {
        return Err(FontError::NegativeGlyphCount(signed_count));
    }
    let glyph_count = signed_count as usize;
    if glyph_count > limits.max_glyphs {
        return Err(FontError::GlyphLimitExceeded {
            count: glyph_count,
            limit: limits.max_glyphs,
        });
    }
    let records_offset = trailer_offset
        .checked_add(FONT_HEADER_SIZE)
        .ok_or(FontError::ArithmeticOverflow)?;
    let records_bytes = glyph_count
        .checked_mul(GLYPH_RECORD_SIZE)
        .ok_or(FontError::ArithmeticOverflow)?;
    require(input, records_offset, records_bytes)?;

    let palette = (0..PALETTE_ENTRIES)
        .map(|index| {
            let offset = FILE_HEADER_SIZE + INFO_HEADER_SIZE + index * 4;
            BitmapPaletteEntry {
                red: input[offset + 2],
                green: input[offset + 1],
                blue: input[offset],
                reserved: input[offset + 3],
            }
        })
        .collect();

    let width_usize = width as usize;
    let height_usize = height as usize;
    let normalized_size =
        usize::try_from(pixel_count).map_err(|_| FontError::ArithmeticOverflow)?;
    let mut pixels = Vec::with_capacity(normalized_size);
    let mut stored_pixels = Vec::with_capacity(pixel_bytes);
    for output_y in 0..height_usize {
        let source_y = height_usize - 1 - output_y;
        let row = FIXED_PIXEL_OFFSET + source_y * stride;
        pixels.extend_from_slice(&input[row..row + width_usize]);
        stored_pixels.extend_from_slice(&input[row..row + stride]);
    }

    let mut lookup = [None; 256];
    let mut glyphs = Vec::with_capacity(glyph_count);
    for index in 0..glyph_count {
        let offset = records_offset + index * GLYPH_RECORD_SIZE;
        let raw_record: [u8; GLYPH_RECORD_SIZE] = input[offset..offset + GLYPH_RECORD_SIZE]
            .try_into()
            .expect("checked record span");
        let code = raw_record[0];
        let source = RectI16 {
            x: i16::from_le_bytes([raw_record[2], raw_record[3]]),
            y: i16::from_le_bytes([raw_record[4], raw_record[5]]),
            width: i16::from_le_bytes([raw_record[6], raw_record[7]]),
            height: i16::from_le_bytes([raw_record[8], raw_record[9]]),
        };
        glyphs.push(FontGlyph {
            code,
            source,
            raw_record,
        });
        lookup[usize::from(code)] = Some(index as u8);
    }

    Ok(BitmapFont {
        atlas: IndexedBitmap {
            width,
            height,
            source_was_top_down: false,
            palette,
            pixels,
            row_stride: stride as u32,
            stored_pixels,
            metadata: BitmapMetadata {
                reserved_1: read_u16(input, 6)?,
                reserved_2: read_u16(input, 8)?,
                x_pixels_per_meter: read_i32(input, 38)?,
                y_pixels_per_meter: read_i32(input, 42)?,
                important_colors,
            },
        },
        metrics: FontMetrics {
            base_height: u32::from_le_bytes(raw_header[4..8].try_into().expect("fixed-size slice")),
            special_advance: u32::from_le_bytes(
                raw_header[8..12].try_into().expect("fixed-size slice"),
            ),
            tracking: u32::from_le_bytes(raw_header[12..16].try_into().expect("fixed-size slice")),
            line_gap: u32::from_le_bytes(raw_header[16..20].try_into().expect("fixed-size slice")),
        },
        raw_header,
        glyphs,
        lookup,
    })
}

fn require(input: &[u8], offset: usize, needed: usize) -> Result<(), FontError> {
    let end = offset
        .checked_add(needed)
        .ok_or(FontError::ArithmeticOverflow)?;
    if end > input.len() {
        return Err(FontError::Truncated { offset, needed });
    }
    Ok(())
}

fn read_u16(input: &[u8], offset: usize) -> Result<u16, FontError> {
    require(input, offset, 2)?;
    Ok(u16::from_le_bytes(
        input[offset..offset + 2]
            .try_into()
            .expect("checked fixed-size slice"),
    ))
}

fn read_u32(input: &[u8], offset: usize) -> Result<u32, FontError> {
    require(input, offset, 4)?;
    Ok(u32::from_le_bytes(
        input[offset..offset + 4]
            .try_into()
            .expect("checked fixed-size slice"),
    ))
}

fn read_i32(input: &[u8], offset: usize) -> Result<i32, FontError> {
    read_u32(input, offset).map(|value| value as i32)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HorizontalAlignment {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerticalAlignment {
    Top,
    Center,
    Bottom,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TextAlignment {
    pub horizontal: HorizontalAlignment,
    pub vertical: VerticalAlignment,
}

impl TextAlignment {
    pub fn from_legacy_flags(flags: u8) -> Self {
        let horizontal = match flags & 0x0f {
            2 => HorizontalAlignment::Center,
            3 => HorizontalAlignment::Right,
            _ => HorizontalAlignment::Left,
        };
        let vertical = match flags & 0xf0 {
            0x20 => VerticalAlignment::Center,
            0x30 => VerticalAlignment::Bottom,
            _ => VerticalAlignment::Top,
        };
        Self {
            horizontal,
            vertical,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PointI32 {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RectI32 {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TextLayoutLimits {
    pub max_input_bytes: usize,
    pub max_lines: usize,
    pub max_blits: usize,
}

impl Default for TextLayoutLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 64 * 1024,
            max_lines: 4096,
            max_blits: 64 * 1024,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TextLayoutError {
    InputLimitExceeded { actual: usize, limit: usize },
    LineLimitExceeded { limit: usize },
    BlitLimitExceeded { limit: usize },
    InvalidViewportWidth(i32),
    CoordinateOverflow,
}

impl fmt::Display for TextLayoutError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputLimitExceeded { actual, limit } => {
                write!(formatter, "text has {actual} bytes; limit is {limit}")
            }
            Self::LineLimitExceeded { limit } => {
                write!(formatter, "text exceeds the {limit}-line limit")
            }
            Self::BlitLimitExceeded { limit } => {
                write!(formatter, "text exceeds the {limit}-blit limit")
            }
            Self::InvalidViewportWidth(width) => {
                write!(formatter, "invalid text viewport width {width}")
            }
            Self::CoordinateOverflow => write!(formatter, "text layout coordinate overflowed"),
        }
    }
}

impl std::error::Error for TextLayoutError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GlyphBlit {
    pub code: u8,
    pub glyph_index: u8,
    pub source: RectI16,
    pub destination: RectI32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextLine {
    pub bytes: Vec<u8>,
    pub origin: PointI32,
    pub measured_width: i32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextPlan {
    pub lines: Vec<TextLine>,
    pub blits: Vec<GlyphBlit>,
    pub block_height: i32,
    pub next_y: i32,
}

pub fn measure_line(font: &BitmapFont, text: &[u8]) -> Result<i32, TextLayoutError> {
    let mut width = 0i64;
    for &byte in text {
        let advance = measured_advance(font, byte);
        width = width
            .checked_add(advance)
            .ok_or(TextLayoutError::CoordinateOverflow)?;
    }
    i32::try_from(width).map_err(|_| TextLayoutError::CoordinateOverflow)
}

pub fn layout_text(
    font: &BitmapFont,
    text: &[u8],
    anchor: PointI32,
    viewport_width: i32,
    alignment: TextAlignment,
    limits: TextLayoutLimits,
) -> Result<TextPlan, TextLayoutError> {
    if text.len() > limits.max_input_bytes {
        return Err(TextLayoutError::InputLimitExceeded {
            actual: text.len(),
            limit: limits.max_input_bytes,
        });
    }
    if viewport_width <= 0 {
        return Err(TextLayoutError::InvalidViewportWidth(viewport_width));
    }

    let mut wrapped = Vec::<Vec<u8>>::new();
    for paragraph in text.split(|byte| *byte == b'^') {
        wrap_paragraph(
            font,
            paragraph,
            viewport_width,
            &mut wrapped,
            limits.max_lines,
        )?;
    }
    if wrapped.is_empty() {
        push_line(&mut wrapped, Vec::new(), limits.max_lines)?;
    }

    let line_step = i64::from(font.metrics.base_height)
        .checked_add(i64::from(font.metrics.line_gap))
        .ok_or(TextLayoutError::CoordinateOverflow)?;
    let block_height = i64::from(font.metrics.base_height)
        .checked_add(
            line_step
                .checked_mul((wrapped.len() - 1) as i64)
                .ok_or(TextLayoutError::CoordinateOverflow)?,
        )
        .ok_or(TextLayoutError::CoordinateOverflow)?;
    let y_adjustment = match alignment.vertical {
        VerticalAlignment::Top => 0,
        VerticalAlignment::Center => block_height / 2,
        VerticalAlignment::Bottom => block_height,
    };
    let first_y = i64::from(anchor.y)
        .checked_sub(y_adjustment)
        .ok_or(TextLayoutError::CoordinateOverflow)?;

    let mut lines = Vec::with_capacity(wrapped.len());
    let mut blits = Vec::new();
    for (line_index, bytes) in wrapped.into_iter().enumerate() {
        let measured_width = measure_line(font, &bytes)?;
        let x_adjustment = match alignment.horizontal {
            HorizontalAlignment::Left => 0,
            HorizontalAlignment::Center => i64::from(measured_width) / 2,
            HorizontalAlignment::Right => i64::from(measured_width),
        };
        let x = i64::from(anchor.x)
            .checked_sub(x_adjustment)
            .ok_or(TextLayoutError::CoordinateOverflow)?;
        let y = first_y
            .checked_add(
                line_step
                    .checked_mul(line_index as i64)
                    .ok_or(TextLayoutError::CoordinateOverflow)?,
            )
            .ok_or(TextLayoutError::CoordinateOverflow)?;
        let origin = PointI32 {
            x: i32::try_from(x).map_err(|_| TextLayoutError::CoordinateOverflow)?,
            y: i32::try_from(y).map_err(|_| TextLayoutError::CoordinateOverflow)?,
        };
        emit_line(font, &bytes, origin, &mut blits, limits.max_blits)?;
        lines.push(TextLine {
            bytes,
            origin,
            measured_width,
        });
    }
    let next_y = first_y
        .checked_add(
            line_step
                .checked_mul(lines.len() as i64)
                .ok_or(TextLayoutError::CoordinateOverflow)?,
        )
        .ok_or(TextLayoutError::CoordinateOverflow)?;

    Ok(TextPlan {
        lines,
        blits,
        block_height: i32::try_from(block_height)
            .map_err(|_| TextLayoutError::CoordinateOverflow)?,
        next_y: i32::try_from(next_y).map_err(|_| TextLayoutError::CoordinateOverflow)?,
    })
}

fn wrap_paragraph(
    font: &BitmapFont,
    paragraph: &[u8],
    viewport_width: i32,
    output: &mut Vec<Vec<u8>>,
    max_lines: usize,
) -> Result<(), TextLayoutError> {
    if paragraph.is_empty() {
        return push_line(output, Vec::new(), max_lines);
    }

    let mut start = 0usize;
    while start < paragraph.len() {
        let mut boundary = start;
        let mut width = 0i64;
        while boundary < paragraph.len() {
            let candidate = width
                .checked_add(measured_advance(font, paragraph[boundary]))
                .ok_or(TextLayoutError::CoordinateOverflow)?;
            if candidate >= i64::from(viewport_width) {
                break;
            }
            width = candidate;
            boundary += 1;
        }
        if boundary == paragraph.len() {
            push_line(output, paragraph[start..].to_vec(), max_lines)?;
            break;
        }

        let split = paragraph[start..=boundary]
            .iter()
            .rposition(|byte| *byte == b' ')
            .map(|relative| start + relative);
        if let Some(space) = split.filter(|space| *space > start) {
            push_line(output, paragraph[start..space].to_vec(), max_lines)?;
            start = space + 1;
            while paragraph.get(start) == Some(&b' ') {
                start += 1;
            }
        } else if let Some(relative) = paragraph[boundary..].iter().position(|byte| *byte == b' ') {
            let space = boundary + relative;
            push_line(output, paragraph[start..space].to_vec(), max_lines)?;
            start = space + 1;
        } else {
            push_line(output, paragraph[start..].to_vec(), max_lines)?;
            break;
        }
    }
    Ok(())
}

fn measured_advance(font: &BitmapFont, byte: u8) -> i64 {
    if matches!(byte, b' ' | b'_' | b'^') {
        i64::from(font.metrics.special_advance)
    } else if let Some(glyph) = font.glyph(byte) {
        i64::from(glyph.source.width) + i64::from(font.metrics.tracking)
    } else {
        0
    }
}

fn push_line(
    output: &mut Vec<Vec<u8>>,
    line: Vec<u8>,
    max_lines: usize,
) -> Result<(), TextLayoutError> {
    if output.len() >= max_lines {
        return Err(TextLayoutError::LineLimitExceeded { limit: max_lines });
    }
    output.push(line);
    Ok(())
}

fn emit_line(
    font: &BitmapFont,
    bytes: &[u8],
    origin: PointI32,
    output: &mut Vec<GlyphBlit>,
    max_blits: usize,
) -> Result<(), TextLayoutError> {
    let mut x = i64::from(origin.x);
    for &code in bytes {
        if code == b' ' || code == b'^' {
            x = checked_advance(x, i64::from(font.metrics.special_advance))?;
            continue;
        }
        let Some(index) = font.lookup[usize::from(code)] else {
            if code == b'_' {
                x = checked_advance(x, i64::from(font.metrics.special_advance))?;
            }
            continue;
        };
        if output.len() >= max_blits {
            return Err(TextLayoutError::BlitLimitExceeded { limit: max_blits });
        }
        let glyph = &font.glyphs[usize::from(index)];
        output.push(GlyphBlit {
            code,
            glyph_index: index,
            source: glyph.source,
            destination: RectI32 {
                x: i32::try_from(x).map_err(|_| TextLayoutError::CoordinateOverflow)?,
                y: origin.y,
                width: i32::from(glyph.source.width),
                height: i32::from(glyph.source.height),
            },
        });
        x = checked_advance(
            x,
            i64::from(glyph.source.width) + i64::from(font.metrics.tracking),
        )?;
    }
    Ok(())
}

fn checked_advance(position: i64, advance: i64) -> Result<i64, TextLayoutError> {
    let result = position
        .checked_add(advance)
        .ok_or(TextLayoutError::CoordinateOverflow)?;
    i32::try_from(result).map_err(|_| TextLayoutError::CoordinateOverflow)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glyph(code: u8, x: i16, y: i16, width: i16, height: i16, fill: u8) -> [u8; 16] {
        let mut record = [fill; 16];
        record[0] = code;
        record[2..4].copy_from_slice(&x.to_le_bytes());
        record[4..6].copy_from_slice(&y.to_le_bytes());
        record[6..8].copy_from_slice(&width.to_le_bytes());
        record[8..10].copy_from_slice(&height.to_le_bytes());
        record
    }

    fn font_file(width: u32, height: u32, header: [u8; 24], glyphs: &[[u8; 16]]) -> Vec<u8> {
        let stride = (width as usize + 3) & !3;
        let pixel_bytes = stride * height as usize;
        let mut input = vec![0; FIXED_PIXEL_OFFSET + pixel_bytes];
        input[0..2].copy_from_slice(b"BM");
        input[10..14].copy_from_slice(&(FIXED_PIXEL_OFFSET as u32).to_le_bytes());
        input[14..18].copy_from_slice(&(INFO_HEADER_SIZE as u32).to_le_bytes());
        input[18..22].copy_from_slice(&(width as i32).to_le_bytes());
        input[22..26].copy_from_slice(&(height as i32).to_le_bytes());
        input[26..28].copy_from_slice(&1u16.to_le_bytes());
        input[28..30].copy_from_slice(&8u16.to_le_bytes());
        input[34..38].copy_from_slice(&(pixel_bytes as u32).to_le_bytes());
        for index in 0..PALETTE_ENTRIES {
            let offset = FILE_HEADER_SIZE + INFO_HEADER_SIZE + index * 4;
            let value = index as u8;
            input[offset..offset + 4].copy_from_slice(&[
                value,
                value.wrapping_add(1),
                value.wrapping_add(2),
                0,
            ]);
        }
        for row in 0..height as usize {
            let offset = FIXED_PIXEL_OFFSET + row * stride;
            for column in 0..width as usize {
                input[offset + column] = (row * width as usize + column) as u8;
            }
        }
        input.extend_from_slice(&header);
        for glyph in glyphs {
            input.extend_from_slice(glyph);
        }
        let len = input.len() as u32;
        input[2..6].copy_from_slice(&len.to_le_bytes());
        input
    }

    fn header(count: u8) -> [u8; 24] {
        let mut header = [0; 24];
        header[0] = 0xa1;
        header[1] = count;
        header[2..4].copy_from_slice(&[0xb2, 0xc3]);
        header[4..8].copy_from_slice(&10u32.to_le_bytes());
        header[8..12].copy_from_slice(&4u32.to_le_bytes());
        header[12..16].copy_from_slice(&1u32.to_le_bytes());
        header[16..20].copy_from_slice(&2u32.to_le_bytes());
        header[20..24].copy_from_slice(&[0xd4, 0xe5, 0xf6, 0x17]);
        header
    }

    fn parsed_font(records: &[[u8; 16]]) -> BitmapFont {
        parse_legacy_font(
            &font_file(32, 16, header(records.len() as u8), records),
            FontLimits::default(),
        )
        .unwrap()
    }

    #[test]
    fn parses_fixed_bitmap_trailer_metrics_and_preserves_unknown_bytes() {
        let record = glyph(0x80, 0, 0, 2, 2, 0xee);
        let input = font_file(2, 2, header(1), &[record]);
        let font = parse_legacy_font(&input, FontLimits::default()).unwrap();

        assert_eq!(font.atlas.width, 2);
        assert_eq!(font.atlas.height, 2);
        assert_eq!(font.atlas.pixels, [2, 3, 0, 1]);
        assert_eq!(font.atlas.palette[1].red, 3);
        assert_eq!(font.metrics.base_height, 10);
        assert_eq!(font.metrics.special_advance, 4);
        assert_eq!(font.metrics.tracking, 1);
        assert_eq!(font.metrics.line_gap, 2);
        assert_eq!(font.raw_header, header(1));
        assert_eq!(font.glyphs[0].raw_record, record);
        assert_eq!(
            font.glyph(0x80).unwrap().source,
            RectI16 {
                x: 0,
                y: 0,
                width: 2,
                height: 2,
            }
        );
    }

    #[test]
    fn later_duplicate_character_code_wins_lookup() {
        let first = glyph(b'A', 0, 0, 3, 4, 0x11);
        let second = glyph(b'A', 8, 9, 6, 7, 0x22);
        let font = parsed_font(&[first, second]);
        assert_eq!(font.glyph(b'A').unwrap().source.x, 8);
        assert_eq!(font.glyphs.len(), 2);
    }

    #[test]
    fn accepts_zero_and_ff_character_codes() {
        let font = parsed_font(&[glyph(0, 0, 0, 1, 1, 0), glyph(0xff, 1, 0, 1, 1, 0)]);
        assert_eq!(font.glyph(0).unwrap().code, 0);
        assert_eq!(font.glyph(0xff).unwrap().code, 0xff);
    }

    #[test]
    fn rejects_observed_format_fields_and_safety_limits() {
        let valid = font_file(1, 1, header(0), &[]);
        let cases = [
            (
                28,
                4u16.to_le_bytes().to_vec(),
                FontError::UnsupportedBitDepth(4),
            ),
            (
                30,
                1u32.to_le_bytes().to_vec(),
                FontError::UnsupportedCompression(1),
            ),
        ];
        for (offset, replacement, expected) in cases {
            let mut input = valid.clone();
            input[offset..offset + replacement.len()].copy_from_slice(&replacement);
            assert_eq!(
                parse_legacy_font(&input, FontLimits::default()),
                Err(expected)
            );
        }

        for (offset, value) in [(18, 0i32), (18, -1), (22, 0), (22, -1)] {
            let mut input = valid.clone();
            input[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            assert!(matches!(
                parse_legacy_font(&input, FontLimits::default()),
                Err(FontError::InvalidDimensions { .. })
            ));
        }

        assert_eq!(
            parse_legacy_font(
                &valid,
                FontLimits {
                    max_width: 0,
                    ..FontLimits::default()
                }
            ),
            Err(FontError::DimensionLimitExceeded)
        );
        for limits in [
            FontLimits {
                max_height: 0,
                ..FontLimits::default()
            },
            FontLimits {
                max_pixels: 0,
                ..FontLimits::default()
            },
        ] {
            assert_eq!(
                parse_legacy_font(&valid, limits),
                Err(FontError::DimensionLimitExceeded)
            );
        }
        assert!(matches!(
            parse_legacy_font(
                &valid,
                FontLimits {
                    max_file_bytes: 1,
                    ..FontLimits::default()
                }
            ),
            Err(FontError::FileLimitExceeded { .. })
        ));
    }

    #[test]
    fn accepts_mutations_to_ignored_bmp_fields_and_trailing_bytes() {
        let mut input = font_file(1, 1, header(0), &[]);
        input[0..2].copy_from_slice(b"NO");
        input[2..6].copy_from_slice(&0u32.to_le_bytes());
        input[10..14].copy_from_slice(&54u32.to_le_bytes());
        input[14..18].copy_from_slice(&12u32.to_le_bytes());
        input[26..28].copy_from_slice(&99u16.to_le_bytes());
        input[46..50].copy_from_slice(&u32::MAX.to_le_bytes());
        input[50..54].copy_from_slice(&u32::MAX.to_le_bytes());
        input.extend_from_slice(&[0xde, 0xad]);
        let font = parse_legacy_font(&input, FontLimits::default()).unwrap();
        assert_eq!(font.atlas.metadata.important_colors, u32::MAX);
    }

    #[test]
    fn preserves_signed_glyph_rectangles_for_deferred_render_validation() {
        let boundary = parsed_font(&[glyph(b'A', 0, 0, 32, 16, 0), glyph(b'B', 31, 15, 1, 1, 0)]);
        assert_eq!(boundary.glyphs.len(), 2);

        let records = [
            glyph(b'W', 0, 116, 27, 29, 0x11),
            glyph(b'X', -1, -2, 3, 4, 0x22),
            glyph(b'Y', 5, 6, 0, 0, 0x33),
        ];
        let font = parse_legacy_font(
            &font_file(128, 128, header(records.len() as u8), &records),
            FontLimits::default(),
        )
        .unwrap();
        let expected = [
            RectI16 {
                x: 0,
                y: 116,
                width: 27,
                height: 29,
            },
            RectI16 {
                x: -1,
                y: -2,
                width: 3,
                height: 4,
            },
            RectI16 {
                x: 5,
                y: 6,
                width: 0,
                height: 0,
            },
        ];
        for ((glyph, raw), source) in font.glyphs().iter().zip(records).zip(expected) {
            assert_eq!(glyph.source, source);
            assert_eq!(glyph.raw_record, raw);
            assert!(std::ptr::eq(font.glyph(glyph.code).unwrap(), glyph));
        }
    }

    #[test]
    fn rejects_negative_count_limit_and_truncated_regions() {
        let mut negative = font_file(1, 1, header(0), &[]);
        let trailer = FIXED_PIXEL_OFFSET + 4;
        negative[trailer + 1] = 0x80;
        assert_eq!(
            parse_legacy_font(&negative, FontLimits::default()),
            Err(FontError::NegativeGlyphCount(-128))
        );

        let one = font_file(1, 1, header(1), &[glyph(b'A', 0, 0, 1, 1, 0)]);
        assert_eq!(
            parse_legacy_font(
                &one,
                FontLimits {
                    max_glyphs: 0,
                    ..FontLimits::default()
                }
            ),
            Err(FontError::GlyphLimitExceeded { count: 1, limit: 0 })
        );

        for length in [
            1,
            53,
            FIXED_PIXEL_OFFSET - 1,
            trailer - 1,
            trailer + 23,
            one.len() - 1,
        ] {
            assert!(matches!(
                parse_legacy_font(&one[..length], FontLimits::default()),
                Err(FontError::Truncated { .. })
            ));
        }
    }

    #[test]
    fn measures_legacy_special_and_missing_byte_advances() {
        let font = parsed_font(&[
            glyph(b'A', 0, 0, 5, 7, 0),
            glyph(b'_', 5, 0, 9, 7, 0),
            glyph(0x80, 14, 0, 2, 7, 0),
        ]);
        assert_eq!(
            measure_line(&font, b"A _^\x80?").unwrap(),
            6 + 4 + 4 + 4 + 3
        );
    }

    #[test]
    fn lays_out_blits_and_underscore_fallback_without_losing_high_bytes() {
        let font = parsed_font(&[
            glyph(b'A', 1, 2, 5, 7, 0),
            glyph(b'_', 8, 2, 9, 7, 0),
            glyph(0x80, 20, 2, 2, 7, 0),
        ]);
        let plan = layout_text(
            &font,
            b"A _\x80?",
            PointI32 { x: 10, y: 20 },
            100,
            TextAlignment::from_legacy_flags(0),
            TextLayoutLimits::default(),
        )
        .unwrap();
        assert_eq!(plan.lines[0].bytes, b"A _\x80?");
        assert_eq!(
            plan.blits.iter().map(|blit| blit.code).collect::<Vec<_>>(),
            [b'A', b'_', 0x80]
        );
        assert_eq!(plan.blits[0].destination.x, 10);
        assert_eq!(plan.blits[1].destination.x, 20);
        assert_eq!(plan.blits[2].destination.x, 30);

        let without_underscore = parsed_font(&[glyph(b'A', 0, 0, 5, 7, 0)]);
        let fallback = layout_text(
            &without_underscore,
            b"_A",
            PointI32 { x: 0, y: 0 },
            100,
            TextAlignment::from_legacy_flags(0),
            TextLayoutLimits::default(),
        )
        .unwrap();
        assert_eq!(fallback.blits[0].destination.x, 4);
    }

    #[test]
    fn wraps_when_candidate_width_equals_viewport_and_caret_forces_line() {
        let font = parsed_font(&[glyph(b'A', 0, 0, 3, 7, 0)]);
        let plan = layout_text(
            &font,
            b"A A^A",
            PointI32 { x: 0, y: 0 },
            12,
            TextAlignment::from_legacy_flags(0),
            TextLayoutLimits::default(),
        )
        .unwrap();
        assert_eq!(
            plan.lines
                .iter()
                .map(|line| line.bytes.as_slice())
                .collect::<Vec<_>>(),
            [b"A".as_slice(), b"A".as_slice(), b"A".as_slice()]
        );
        assert_eq!(
            plan.lines
                .iter()
                .map(|line| line.origin.y)
                .collect::<Vec<_>>(),
            [0, 12, 24]
        );
        assert_eq!(plan.block_height, 34);
    }

    #[test]
    fn applies_legacy_horizontal_and_vertical_alignment_flags() {
        let font = parsed_font(&[glyph(b'A', 0, 0, 5, 7, 0)]);
        for (flags, expected_x, expected_y) in [(0x00, 100, 100), (0x22, 97, 95), (0x33, 94, 90)] {
            let plan = layout_text(
                &font,
                b"A",
                PointI32 { x: 100, y: 100 },
                100,
                TextAlignment::from_legacy_flags(flags),
                TextLayoutLimits::default(),
            )
            .unwrap();
            assert_eq!(
                plan.lines[0].origin,
                PointI32 {
                    x: expected_x,
                    y: expected_y
                }
            );
        }
    }

    #[test]
    fn enforces_input_line_blit_and_coordinate_limits() {
        let font = parsed_font(&[glyph(b'A', 0, 0, 5, 7, 0)]);
        let base = TextLayoutLimits::default();
        assert!(matches!(
            layout_text(
                &font,
                b"AA",
                PointI32 { x: 0, y: 0 },
                100,
                TextAlignment::from_legacy_flags(0),
                TextLayoutLimits {
                    max_input_bytes: 1,
                    ..base
                }
            ),
            Err(TextLayoutError::InputLimitExceeded { .. })
        ));
        assert_eq!(
            layout_text(
                &font,
                b"A^A",
                PointI32 { x: 0, y: 0 },
                100,
                TextAlignment::from_legacy_flags(0),
                TextLayoutLimits {
                    max_lines: 1,
                    ..base
                }
            ),
            Err(TextLayoutError::LineLimitExceeded { limit: 1 })
        );
        assert_eq!(
            layout_text(
                &font,
                b"AA",
                PointI32 { x: 0, y: 0 },
                100,
                TextAlignment::from_legacy_flags(0),
                TextLayoutLimits {
                    max_blits: 1,
                    ..base
                }
            ),
            Err(TextLayoutError::BlitLimitExceeded { limit: 1 })
        );
        assert_eq!(
            layout_text(
                &font,
                b"A",
                PointI32 { x: 0, y: 0 },
                0,
                TextAlignment::from_legacy_flags(0),
                base
            ),
            Err(TextLayoutError::InvalidViewportWidth(0))
        );

        let mut huge = font;
        huge.metrics.tracking = u32::MAX;
        assert_eq!(
            measure_line(&huge, b"A"),
            Err(TextLayoutError::CoordinateOverflow)
        );
    }
}
