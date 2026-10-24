use std::fmt;

const FILE_HEADER_SIZE: usize = 14;
const INFO_HEADER_SIZE: usize = 40;
const PALETTE_ENTRY_SIZE: usize = 4;
const BI_RGB: u32 = 0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BitmapLimits {
    pub max_width: u32,
    pub max_height: u32,
    pub max_pixels: u64,
}

impl Default for BitmapLimits {
    fn default() -> Self {
        Self {
            max_width: 8192,
            max_height: 8192,
            max_pixels: 64 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BitmapPaletteEntry {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub reserved: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BitmapMetadata {
    pub reserved_1: u16,
    pub reserved_2: u16,
    pub x_pixels_per_meter: i32,
    pub y_pixels_per_meter: i32,
    pub important_colors: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexedBitmap {
    pub width: u32,
    pub height: u32,
    pub source_was_top_down: bool,
    pub palette: Vec<BitmapPaletteEntry>,
    /// One palette index per pixel, normalized to top-to-bottom row order.
    pub pixels: Vec<u8>,
    /// Bytes in each stored DIB row, including its four-byte alignment padding.
    pub row_stride: u32,
    /// Complete stored DIB rows, normalized to top-to-bottom row order. Unlike
    /// `pixels`, this preserves padding bytes exactly.
    pub stored_pixels: Vec<u8>,
    pub metadata: BitmapMetadata,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BitmapError {
    Truncated,
    NotBitmap,
    FileSizeMismatch {
        declared: usize,
        actual: usize,
    },
    UnsupportedDibHeader(u32),
    InvalidDimensions {
        width: i32,
        height: i32,
    },
    LimitsExceeded,
    InvalidPlanes(u16),
    UnsupportedBitDepth(u16),
    UnsupportedCompression(u32),
    InvalidPaletteCount(u32),
    ImportantColorsExceedPalette {
        important: u32,
        palette: u32,
    },
    PaletteOverlapsPixels,
    ImageSizeMismatch {
        declared: usize,
        expected: usize,
    },
    PixelDataSizeMismatch {
        expected_end: usize,
        actual_end: usize,
    },
    PaletteIndexOutOfRange {
        index: u8,
        palette_entries: usize,
    },
}

impl fmt::Display for BitmapError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => write!(formatter, "truncated bitmap header"),
            Self::NotBitmap => write!(formatter, "file does not begin with BM"),
            Self::FileSizeMismatch { declared, actual } => {
                write!(
                    formatter,
                    "bitmap declares {declared} bytes but file has {actual}"
                )
            }
            Self::UnsupportedDibHeader(size) => {
                write!(formatter, "unsupported DIB header size {size}")
            }
            Self::InvalidDimensions { width, height } => {
                write!(formatter, "invalid bitmap dimensions {width}x{height}")
            }
            Self::LimitsExceeded => write!(formatter, "bitmap dimensions exceed configured limits"),
            Self::InvalidPlanes(planes) => write!(formatter, "bitmap declares {planes} planes"),
            Self::UnsupportedBitDepth(depth) => {
                write!(formatter, "unsupported bitmap depth {depth}")
            }
            Self::UnsupportedCompression(compression) => {
                write!(formatter, "unsupported bitmap compression {compression}")
            }
            Self::InvalidPaletteCount(count) => {
                write!(formatter, "invalid bitmap palette count {count}")
            }
            Self::ImportantColorsExceedPalette { important, palette } => write!(
                formatter,
                "bitmap marks {important} important colors but has {palette} palette entries"
            ),
            Self::PaletteOverlapsPixels => write!(formatter, "bitmap palette overlaps pixel data"),
            Self::ImageSizeMismatch { declared, expected } => {
                write!(
                    formatter,
                    "bitmap declares {declared} image bytes; expected {expected}"
                )
            }
            Self::PixelDataSizeMismatch {
                expected_end,
                actual_end,
            } => write!(
                formatter,
                "bitmap pixels end at {expected_end}; file ends at {actual_end}"
            ),
            Self::PaletteIndexOutOfRange {
                index,
                palette_entries,
            } => write!(
                formatter,
                "pixel index {index} exceeds the {palette_entries}-entry palette"
            ),
        }
    }
}

impl std::error::Error for BitmapError {}

/// Parses the Windows V3, 8-bit or 4-bit, uncompressed indexed bitmap subset
/// observed in the startup and stage assets.
///
/// A short all-zero trailing pad is accepted when it rounds the file up to a
/// four-byte boundary: the shipped stage `.c3d` textures carry exactly that pad
/// (their own declared file size includes it). Any other trailing byte pattern
/// is still rejected as malformed.
pub fn parse_indexed_bitmap(
    input: &[u8],
    limits: BitmapLimits,
) -> Result<IndexedBitmap, BitmapError> {
    if input.len() < FILE_HEADER_SIZE + INFO_HEADER_SIZE {
        return Err(BitmapError::Truncated);
    }
    if &input[0..2] != b"BM" {
        return Err(BitmapError::NotBitmap);
    }
    let declared_file_size = read_u32(input, 2)? as usize;
    if declared_file_size != input.len() {
        return Err(BitmapError::FileSizeMismatch {
            declared: declared_file_size,
            actual: input.len(),
        });
    }
    let pixel_offset = read_u32(input, 10)? as usize;
    let dib_size = read_u32(input, 14)?;
    if dib_size != INFO_HEADER_SIZE as u32 {
        return Err(BitmapError::UnsupportedDibHeader(dib_size));
    }

    let signed_width = read_i32(input, 18)?;
    let signed_height = read_i32(input, 22)?;
    if signed_width <= 0 || signed_height == 0 || signed_height == i32::MIN {
        return Err(BitmapError::InvalidDimensions {
            width: signed_width,
            height: signed_height,
        });
    }
    let width = signed_width as u32;
    let height = signed_height.unsigned_abs();
    let pixels = u64::from(width) * u64::from(height);
    if width > limits.max_width || height > limits.max_height || pixels > limits.max_pixels {
        return Err(BitmapError::LimitsExceeded);
    }

    let planes = read_u16(input, 26)?;
    if planes != 1 {
        return Err(BitmapError::InvalidPlanes(planes));
    }
    let depth = read_u16(input, 28)?;
    if depth != 8 && depth != 4 {
        return Err(BitmapError::UnsupportedBitDepth(depth));
    }
    let compression = read_u32(input, 30)?;
    if compression != BI_RGB {
        return Err(BitmapError::UnsupportedCompression(compression));
    }

    let declared_image_size = read_u32(input, 34)? as usize;
    let colors_used = read_u32(input, 46)?;
    // A zero `biClrUsed` means the maximum palette for the bit depth: 256
    // entries for 8bpp and 16 for 4bpp.
    let palette_count = if colors_used == 0 {
        1u32 << depth
    } else {
        colors_used
    };
    if palette_count == 0 || palette_count > 256 {
        return Err(BitmapError::InvalidPaletteCount(palette_count));
    }
    let important_colors = read_u32(input, 50)?;
    if important_colors > palette_count {
        return Err(BitmapError::ImportantColorsExceedPalette {
            important: important_colors,
            palette: palette_count,
        });
    }

    let palette_start = FILE_HEADER_SIZE + INFO_HEADER_SIZE;
    let palette_end = palette_start
        .checked_add(palette_count as usize * PALETTE_ENTRY_SIZE)
        .ok_or(BitmapError::LimitsExceeded)?;
    if palette_end > pixel_offset || pixel_offset > input.len() {
        return Err(BitmapError::PaletteOverlapsPixels);
    }
    let palette_bytes = &input[palette_start..palette_end];
    let palette = (0..palette_count as usize)
        .map(|index| {
            let start = index * PALETTE_ENTRY_SIZE;
            let entry = &palette_bytes[start..start + PALETTE_ENTRY_SIZE];
            BitmapPaletteEntry {
                red: entry[2],
                green: entry[1],
                blue: entry[0],
                reserved: entry[3],
            }
        })
        .collect::<Vec<_>>();

    let row_bits = u64::from(width) * u64::from(depth);
    let stride = row_bits
        .checked_add(31)
        .map(|bits| (bits / 32) * 4)
        .ok_or(BitmapError::LimitsExceeded)?;
    let stored_size = stride
        .checked_mul(u64::from(height))
        .and_then(|size| usize::try_from(size).ok())
        .ok_or(BitmapError::LimitsExceeded)?;
    if declared_image_size != 0 && declared_image_size != stored_size {
        return Err(BitmapError::ImageSizeMismatch {
            declared: declared_image_size,
            expected: stored_size,
        });
    }
    let pixel_end = pixel_offset
        .checked_add(stored_size)
        .ok_or(BitmapError::LimitsExceeded)?;
    // Shipped stage textures (for example the padded blobs in `Game/Au/*`)
    // carry a zero pad after the pixel rows that rounds the declared file size
    // up to a four-byte boundary - the same alignment the DIB row stride
    // already requires. Accept only that pad: nonzero trailing bytes, a pad
    // longer than three bytes, or a misaligned end remain malformed.
    let trailing = &input[pixel_end.min(input.len())..];
    if pixel_end > input.len()
        || (!trailing.is_empty()
            && (trailing.len() > 3
                || !input.len().is_multiple_of(4)
                || trailing.iter().any(|&byte| byte != 0)))
    {
        return Err(BitmapError::PixelDataSizeMismatch {
            expected_end: pixel_end,
            actual_end: input.len(),
        });
    }

    let stride = stride as usize;
    let width = width as usize;
    let height_usize = height as usize;
    let source_was_top_down = signed_height < 0;
    let mut normalized = Vec::with_capacity(width * height_usize);
    let mut normalized_stored = Vec::with_capacity(stride * height_usize);
    for output_y in 0..height_usize {
        let source_y = if source_was_top_down {
            output_y
        } else {
            height_usize - 1 - output_y
        };
        let row_start = pixel_offset + source_y * stride;
        let stored_row = &input[row_start..row_start + stride];
        // 8bpp stores one index per byte; 4bpp packs two indices per byte with
        // the higher nibble first (the legacy DIB convention).
        let row: Vec<u8> = if depth == 8 {
            stored_row[..width].to_vec()
        } else {
            let mut row = Vec::with_capacity(width);
            for byte in &stored_row[..width.div_ceil(2)] {
                row.push(byte >> 4);
                if row.len() < width {
                    row.push(byte & 0x0f);
                }
            }
            row
        };
        if let Some(index) = row
            .iter()
            .copied()
            .find(|index| *index as usize >= palette.len())
        {
            return Err(BitmapError::PaletteIndexOutOfRange {
                index,
                palette_entries: palette.len(),
            });
        }
        normalized.extend_from_slice(&row);
        normalized_stored.extend_from_slice(stored_row);
    }

    Ok(IndexedBitmap {
        width: width as u32,
        height,
        source_was_top_down,
        palette,
        pixels: normalized,
        row_stride: stride as u32,
        stored_pixels: normalized_stored,
        metadata: BitmapMetadata {
            reserved_1: read_u16(input, 6)?,
            reserved_2: read_u16(input, 8)?,
            x_pixels_per_meter: read_i32(input, 38)?,
            y_pixels_per_meter: read_i32(input, 42)?,
            important_colors,
        },
    })
}

fn read_u16(input: &[u8], offset: usize) -> Result<u16, BitmapError> {
    input
        .get(offset..offset + 2)
        .ok_or(BitmapError::Truncated)
        .map(|bytes| u16::from_le_bytes(bytes.try_into().expect("fixed-size slice")))
}

fn read_u32(input: &[u8], offset: usize) -> Result<u32, BitmapError> {
    input
        .get(offset..offset + 4)
        .ok_or(BitmapError::Truncated)
        .map(|bytes| u32::from_le_bytes(bytes.try_into().expect("fixed-size slice")))
}

fn read_i32(input: &[u8], offset: usize) -> Result<i32, BitmapError> {
    read_u32(input, offset).map(|value| value as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bitmap(width: i32, height: i32, colors: &[[u8; 4]], stored_rows: &[u8]) -> Vec<u8> {
        let pixel_offset = FILE_HEADER_SIZE + INFO_HEADER_SIZE + colors.len() * 4;
        let file_size = pixel_offset + stored_rows.len();
        let mut result = vec![0; pixel_offset];
        result[0..2].copy_from_slice(b"BM");
        result[2..6].copy_from_slice(&(file_size as u32).to_le_bytes());
        result[10..14].copy_from_slice(&(pixel_offset as u32).to_le_bytes());
        result[14..18].copy_from_slice(&(INFO_HEADER_SIZE as u32).to_le_bytes());
        result[18..22].copy_from_slice(&width.to_le_bytes());
        result[22..26].copy_from_slice(&height.to_le_bytes());
        result[26..28].copy_from_slice(&1u16.to_le_bytes());
        result[28..30].copy_from_slice(&8u16.to_le_bytes());
        result[34..38].copy_from_slice(&(stored_rows.len() as u32).to_le_bytes());
        result[46..50].copy_from_slice(&(colors.len() as u32).to_le_bytes());
        for (index, color) in colors.iter().enumerate() {
            let start = FILE_HEADER_SIZE + INFO_HEADER_SIZE + index * 4;
            result[start..start + 4].copy_from_slice(color);
        }
        result.extend_from_slice(stored_rows);
        result
    }

    const COLORS: [[u8; 4]; 4] = [[3, 2, 1, 0], [6, 5, 4, 0], [9, 8, 7, 0], [12, 11, 10, 13]];

    #[test]
    fn parses_bottom_up_rows_and_preserves_normalized_stored_padding() {
        let mut input = bitmap(3, 2, &COLORS, &[2, 3, 1, 99, 0, 1, 2, 88]);
        input[6..8].copy_from_slice(&7u16.to_le_bytes());
        input[8..10].copy_from_slice(&8u16.to_le_bytes());
        input[38..42].copy_from_slice(&2_834i32.to_le_bytes());
        input[42..46].copy_from_slice(&2_835i32.to_le_bytes());
        input[50..54].copy_from_slice(&2u32.to_le_bytes());
        let parsed = parse_indexed_bitmap(&input, BitmapLimits::default()).unwrap();
        assert_eq!(parsed.width, 3);
        assert_eq!(parsed.height, 2);
        assert!(!parsed.source_was_top_down);
        assert_eq!(parsed.pixels, [0, 1, 2, 2, 3, 1]);
        assert_eq!(parsed.row_stride, 4);
        assert_eq!(parsed.stored_pixels, [0, 1, 2, 88, 2, 3, 1, 99]);
        assert_eq!(
            parsed.palette[0],
            BitmapPaletteEntry {
                red: 1,
                green: 2,
                blue: 3,
                reserved: 0,
            }
        );
        assert_eq!(parsed.palette[3].reserved, 13);
        assert_eq!(parsed.palette.len(), 4);
        assert_eq!(parsed.metadata.reserved_1, 7);
        assert_eq!(parsed.metadata.reserved_2, 8);
        assert_eq!(parsed.metadata.x_pixels_per_meter, 2_834);
        assert_eq!(parsed.metadata.y_pixels_per_meter, 2_835);
        assert_eq!(parsed.metadata.important_colors, 2);
    }

    #[test]
    fn preserves_top_down_row_order() {
        let input = bitmap(3, -2, &COLORS, &[0, 1, 2, 88, 2, 3, 1, 99]);
        let parsed = parse_indexed_bitmap(&input, BitmapLimits::default()).unwrap();
        assert!(parsed.source_was_top_down);
        assert_eq!(parsed.pixels, [0, 1, 2, 2, 3, 1]);
        assert_eq!(parsed.stored_pixels, [0, 1, 2, 88, 2, 3, 1, 99]);
    }

    /// Builds a 4bpp V3 bitmap fixture with one nibble pair per byte.
    fn bitmap_4bpp(width: i32, height: i32, colors: &[[u8; 4]], stored_rows: &[u8]) -> Vec<u8> {
        let mut result = bitmap(width, height, colors, stored_rows);
        result[28..30].copy_from_slice(&4u16.to_le_bytes());
        // `bitmap` already wrote the stored rows and the file size for the
        // 8bpp layout; the depths share the same stride here only because the
        // fixture supplies its own packed rows and the declared image size is
        // overwritten below.
        let stored = stored_rows.len() as u32;
        result[34..38].copy_from_slice(&stored.to_le_bytes());
        result
    }

    #[test]
    fn parses_4bpp_packed_rows_with_high_nibble_first() {
        // 4x2, 4 bytes per row: row 0 = indices 0,1,2,3 then padding 0xee,
        // row 1 = 3,2,1,0 then padding 0xdd. Rows are bottom-up.
        let colors = &COLORS;
        let input = bitmap_4bpp(
            4,
            2,
            colors,
            &[0x01, 0x23, 0xee, 0xee, 0x32, 0x10, 0xdd, 0xdd],
        );
        let parsed = parse_indexed_bitmap(&input, BitmapLimits::default()).unwrap();
        assert_eq!(parsed.width, 4);
        assert_eq!(parsed.height, 2);
        assert_eq!(parsed.row_stride, 4);
        assert_eq!(parsed.palette.len(), colors.len());
        // Bottom row first, then the top row; padding is preserved verbatim.
        assert_eq!(parsed.pixels, [3, 2, 1, 0, 0, 1, 2, 3]);
        assert_eq!(
            parsed.stored_pixels,
            [0x32, 0x10, 0xdd, 0xdd, 0x01, 0x23, 0xee, 0xee]
        );
    }

    #[test]
    fn defaults_a_zero_color_count_to_sixteen_entries_for_4bpp() {
        let colors: Vec<[u8; 4]> = (0..16).map(|index| [index as u8, 0, 0, 0]).collect();
        let mut input = bitmap_4bpp(2, 1, &colors, &[0x01, 0x00, 0x00, 0x00]);
        input[46..50].fill(0);
        let parsed = parse_indexed_bitmap(&input, BitmapLimits::default()).unwrap();
        assert_eq!(parsed.palette.len(), 16);
        assert_eq!(parsed.pixels, [0, 1]);
    }

    #[test]
    fn rejects_a_4bpp_index_outside_the_palette() {
        // Palette declares one usable entry, so index 1 is out of range.
        let input = bitmap_4bpp(2, 1, &COLORS, &[0x01, 0x00, 0x00, 0x00]);
        let mut input = input;
        input[46..50].copy_from_slice(&1u32.to_le_bytes());
        assert_eq!(
            parse_indexed_bitmap(&input, BitmapLimits::default()),
            Err(BitmapError::PaletteIndexOutOfRange {
                index: 1,
                palette_entries: 1
            })
        );
    }

    #[test]
    fn accepts_zero_image_size_for_uncompressed_bitmap() {
        let mut input = bitmap(1, 1, &COLORS, &[0, 0, 0, 0]);
        input[34..38].fill(0);
        parse_indexed_bitmap(&input, BitmapLimits::default()).unwrap();
    }

    #[test]
    fn rejects_invalid_container_and_core_format_fields() {
        let valid = bitmap(1, 1, &COLORS, &[0, 0, 0, 0]);
        let cases: &[(usize, &[u8], BitmapError)] = &[
            (0, b"XM", BitmapError::NotBitmap),
            (
                14,
                &12u32.to_le_bytes(),
                BitmapError::UnsupportedDibHeader(12),
            ),
            (26, &2u16.to_le_bytes(), BitmapError::InvalidPlanes(2)),
            (28, &1u16.to_le_bytes(), BitmapError::UnsupportedBitDepth(1)),
            (
                30,
                &1u32.to_le_bytes(),
                BitmapError::UnsupportedCompression(1),
            ),
        ];
        for (offset, replacement, expected) in cases {
            let mut input = valid.clone();
            input[*offset..*offset + replacement.len()].copy_from_slice(replacement);
            assert_eq!(
                parse_indexed_bitmap(&input, BitmapLimits::default()),
                Err(expected.clone())
            );
        }
    }

    #[test]
    fn rejects_dimensions_limits_sizes_palette_and_indices() {
        let valid = bitmap(1, 1, &COLORS, &[0, 0, 0, 0]);

        let mut zero_width = valid.clone();
        zero_width[18..22].fill(0);
        assert!(matches!(
            parse_indexed_bitmap(&zero_width, BitmapLimits::default()),
            Err(BitmapError::InvalidDimensions { .. })
        ));

        let mut negative_width = valid.clone();
        negative_width[18..22].copy_from_slice(&(-1i32).to_le_bytes());
        assert!(matches!(
            parse_indexed_bitmap(&negative_width, BitmapLimits::default()),
            Err(BitmapError::InvalidDimensions { .. })
        ));

        for height in [0i32, i32::MIN] {
            let mut invalid_height = valid.clone();
            invalid_height[22..26].copy_from_slice(&height.to_le_bytes());
            assert!(matches!(
                parse_indexed_bitmap(&invalid_height, BitmapLimits::default()),
                Err(BitmapError::InvalidDimensions { .. })
            ));
        }

        assert_eq!(
            parse_indexed_bitmap(
                &valid,
                BitmapLimits {
                    max_width: 0,
                    ..BitmapLimits::default()
                }
            ),
            Err(BitmapError::LimitsExceeded)
        );

        assert_eq!(
            parse_indexed_bitmap(
                &valid,
                BitmapLimits {
                    max_height: 0,
                    ..BitmapLimits::default()
                }
            ),
            Err(BitmapError::LimitsExceeded)
        );

        assert_eq!(
            parse_indexed_bitmap(
                &valid,
                BitmapLimits {
                    max_pixels: 0,
                    ..BitmapLimits::default()
                }
            ),
            Err(BitmapError::LimitsExceeded)
        );

        let mut wrong_image_size = valid.clone();
        wrong_image_size[34..38].copy_from_slice(&5u32.to_le_bytes());
        assert!(matches!(
            parse_indexed_bitmap(&wrong_image_size, BitmapLimits::default()),
            Err(BitmapError::ImageSizeMismatch { .. })
        ));

        let mut important = valid.clone();
        important[50..54].copy_from_slice(&5u32.to_le_bytes());
        assert!(matches!(
            parse_indexed_bitmap(&important, BitmapLimits::default()),
            Err(BitmapError::ImportantColorsExceedPalette { .. })
        ));

        let mut too_many_colors = valid.clone();
        too_many_colors[46..50].copy_from_slice(&257u32.to_le_bytes());
        assert_eq!(
            parse_indexed_bitmap(&too_many_colors, BitmapLimits::default()),
            Err(BitmapError::InvalidPaletteCount(257))
        );

        let out_of_range = bitmap(1, 1, &COLORS, &[4, 0, 0, 0]);
        assert!(matches!(
            parse_indexed_bitmap(&out_of_range, BitmapLimits::default()),
            Err(BitmapError::PaletteIndexOutOfRange { .. })
        ));
    }

    #[test]
    fn rejects_truncation_file_size_offsets_and_pixel_span_mismatches() {
        assert_eq!(
            parse_indexed_bitmap(b"BM", BitmapLimits::default()),
            Err(BitmapError::Truncated)
        );

        let valid = bitmap(1, 1, &COLORS, &[0, 0, 0, 0]);
        let mut wrong_file_size = valid.clone();
        wrong_file_size[2..6].copy_from_slice(&0u32.to_le_bytes());
        assert!(matches!(
            parse_indexed_bitmap(&wrong_file_size, BitmapLimits::default()),
            Err(BitmapError::FileSizeMismatch { .. })
        ));

        let mut overlap = valid.clone();
        overlap[10..14].copy_from_slice(&54u32.to_le_bytes());
        assert_eq!(
            parse_indexed_bitmap(&overlap, BitmapLimits::default()),
            Err(BitmapError::PaletteOverlapsPixels)
        );

        let mut past_end = valid.clone();
        past_end[10..14].copy_from_slice(&(valid.len() as u32 + 1).to_le_bytes());
        assert_eq!(
            parse_indexed_bitmap(&past_end, BitmapLimits::default()),
            Err(BitmapError::PaletteOverlapsPixels)
        );

        let mut truncated_pixels = valid.clone();
        truncated_pixels.pop();
        let truncated_len = truncated_pixels.len() as u32;
        truncated_pixels[2..6].copy_from_slice(&truncated_len.to_le_bytes());
        assert!(matches!(
            parse_indexed_bitmap(&truncated_pixels, BitmapLimits::default()),
            Err(BitmapError::PixelDataSizeMismatch { .. })
        ));

        let mut trailing_pixels = valid;
        trailing_pixels.push(0);
        let trailing_len = trailing_pixels.len() as u32;
        trailing_pixels[2..6].copy_from_slice(&trailing_len.to_le_bytes());
        assert!(matches!(
            parse_indexed_bitmap(&trailing_pixels, BitmapLimits::default()),
            Err(BitmapError::PixelDataSizeMismatch { .. })
        ));
    }

    #[test]
    fn accepts_the_shipped_stage_texture_zero_pad_and_rejects_other_trailing() {
        // The shipped stage shape: a 64x64, 256-color blob whose own declared
        // file size includes a two-byte zero pad rounding 5174 up to 5176.
        let colors = [[0u8; 4]; 256];
        let rows = vec![0u8; 64 * 64];
        let exact = bitmap(64, 64, &colors, &rows);
        assert_eq!(exact.len() % 4, 2);

        let mut padded = exact.clone();
        padded.extend_from_slice(&[0, 0]);
        let padded_len = padded.len() as u32;
        padded[2..6].copy_from_slice(&padded_len.to_le_bytes());
        assert_eq!(padded.len() % 4, 0);
        let parsed = parse_indexed_bitmap(&padded, BitmapLimits::default()).unwrap();
        assert_eq!(
            parsed.pixels,
            parse_indexed_bitmap(&exact, BitmapLimits::default())
                .unwrap()
                .pixels
        );

        // A nonzero pad byte is malformed even when the end is aligned.
        let mut nonzero = exact.clone();
        nonzero.extend_from_slice(&[0, 1]);
        let nonzero_len = nonzero.len() as u32;
        nonzero[2..6].copy_from_slice(&nonzero_len.to_le_bytes());
        assert!(matches!(
            parse_indexed_bitmap(&nonzero, BitmapLimits::default()),
            Err(BitmapError::PixelDataSizeMismatch { .. })
        ));

        // A zero pad that leaves the end unaligned is malformed.
        let mut unaligned = exact.clone();
        unaligned.extend_from_slice(&[0, 0, 0]);
        let unaligned_len = unaligned.len() as u32;
        unaligned[2..6].copy_from_slice(&unaligned_len.to_le_bytes());
        assert!(matches!(
            parse_indexed_bitmap(&unaligned, BitmapLimits::default()),
            Err(BitmapError::PixelDataSizeMismatch { .. })
        ));

        // More than three pad bytes is malformed even when aligned and zero.
        let mut long_pad = exact.clone();
        long_pad.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
        let long_pad_len = long_pad.len() as u32;
        long_pad[2..6].copy_from_slice(&long_pad_len.to_le_bytes());
        assert_eq!(long_pad.len() % 4, 0);
        assert!(matches!(
            parse_indexed_bitmap(&long_pad, BitmapLimits::default()),
            Err(BitmapError::PixelDataSizeMismatch { .. })
        ));
    }
}
