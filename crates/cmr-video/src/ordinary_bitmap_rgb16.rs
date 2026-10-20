//! Recovered ordinary `0x81` indexed-BMP conversion to logical 16-bit RGB.
//!
//! This module models the bitmap decoder's 16-bit branch only. It expands the
//! complete normalized stored DIB rows, including four-byte alignment bytes,
//! through the bitmap's own embedded palette. The resulting values are
//! logical packed words: no native byte order, surface pitch, driver stretch,
//! dithering, or presentation behavior is implied.

use std::fmt;

use cmr_formats::bitmap::IndexedBitmap;

/// Logical packing selected by the recovered display-format discriminator.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OrdinaryBitmapRgb16Layout {
    Rgb555,
    Rgb565,
}

/// Owned result of the ordinary `0x81` 8-bit-BMP conversion.
///
/// The nominal input type deliberately excludes font atlases, selected C3D
/// texture formats, and framebuffer formats. Those have independent recovered
/// ownership and conversion rules.
///
/// ```compile_fail
/// use cmr_formats::font::BitmapFont;
/// use cmr_video::ordinary_bitmap_rgb16::convert_ordinary_bitmap_0x81_to_logical_rgb16;
/// fn alternate_fonts_stay_separate(font: &BitmapFont) {
///     let _ = convert_ordinary_bitmap_0x81_to_logical_rgb16(font, 5);
/// }
/// ```
///
/// ```compile_fail
/// use cmr_video::{
///     legacy_c3d_pixel_format::SelectedNonAlphaTextureFormat,
///     ordinary_bitmap_rgb16::convert_ordinary_bitmap_0x81_to_logical_rgb16,
/// };
/// fn selected_textures_stay_separate(format: &SelectedNonAlphaTextureFormat) {
///     let _ = convert_ordinary_bitmap_0x81_to_logical_rgb16(format, 5);
/// }
/// ```
///
/// ```compile_fail
/// use cmr_video::{
///     ordinary_bitmap_rgb16::convert_ordinary_bitmap_0x81_to_logical_rgb16,
///     packed_rgb_target::FramebufferRgbFormat,
/// };
/// fn framebuffer_formats_stay_separate(format: &FramebufferRgbFormat) {
///     let _ = convert_ordinary_bitmap_0x81_to_logical_rgb16(format, 5);
/// }
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OrdinaryBitmapLogicalRgb16Source {
    dib_extent: [u32; 2],
    aligned_surface_extent: [u32; 2],
    source_was_top_down: bool,
    display_format_discriminator: u8,
    layout: OrdinaryBitmapRgb16Layout,
    embedded_palette_rgb: Box<[[u8; 3]]>,
    stored_indices: Box<[u8]>,
    logical_words: Box<[u16]>,
    packed_color_key: u16,
}

impl OrdinaryBitmapLogicalRgb16Source {
    /// Logical DIB dimensions before the recovered four-byte width alignment.
    #[must_use]
    pub const fn dib_extent(&self) -> [u32; 2] {
        self.dib_extent
    }

    /// Recovered surface dimensions in logical pixels.
    ///
    /// The width is the retained DIB row stride. This is an extent, not a
    /// claim about native 16-bit surface pitch or byte storage.
    #[must_use]
    pub const fn aligned_surface_extent(&self) -> [u32; 2] {
        self.aligned_surface_extent
    }

    /// Retained orientation provenance for the accepted bottom-up subset.
    #[must_use]
    pub const fn source_was_top_down(&self) -> bool {
        self.source_was_top_down
    }

    /// Raw display-format discriminator that selected the recovered formula.
    #[must_use]
    pub const fn display_format_discriminator(&self) -> u8 {
        self.display_format_discriminator
    }

    #[must_use]
    pub const fn layout(&self) -> OrdinaryBitmapRgb16Layout {
        self.layout
    }

    /// RGB channels copied from this bitmap's embedded palette.
    #[must_use]
    pub fn embedded_palette_rgb(&self) -> &[[u8; 3]] {
        &self.embedded_palette_rgb
    }

    /// Complete normalized stored rows, including DIB alignment bytes.
    #[must_use]
    pub fn stored_indices(&self) -> &[u8] {
        &self.stored_indices
    }

    /// Logical packed words for every retained stored-row byte.
    #[must_use]
    pub fn logical_words(&self) -> &[u16] {
        &self.logical_words
    }

    /// Packed embedded-palette entry zero used for both ends of the source
    /// color-key range by the recovered branch.
    #[must_use]
    pub const fn packed_color_key(&self) -> u16 {
        self.packed_color_key
    }
}

/// Validation failure detected before a converted source is returned.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OrdinaryBitmapRgb16Error {
    InvalidExtent([u32; 2]),
    UnsupportedTopDownSource,
    InvalidEmbeddedPaletteEntryCount(usize),
    PixelCount {
        expected: u64,
        actual: usize,
    },
    StoredRowStride {
        expected: u64,
        actual: u32,
    },
    StoredPixelCount {
        expected: u64,
        actual: usize,
    },
    LogicalStoredPixelMismatch {
        logical_offset: usize,
        stored_offset: usize,
        logical_index: u8,
        stored_index: u8,
    },
    StoredPaletteIndexOutOfBounds {
        stored_offset: usize,
        index: u8,
        palette_entries: usize,
    },
}

impl fmt::Display for OrdinaryBitmapRgb16Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "ordinary indexed-BMP logical RGB16 conversion failed: {self:?}"
        )
    }
}

impl std::error::Error for OrdinaryBitmapRgb16Error {}

/// Expands an ordinary `0x81` indexed bitmap into recovered logical 16-bit
/// words using its embedded palette.
///
/// Discriminator value `5` selects RGB555. Every other byte selects RGB565.
/// Both formulas truncate low component bits exactly as observed. The complete
/// aligned stored rows are converted, not only the DIB's logical-width pixels.
/// Top-down sources are rejected because the recovered branch is bounded to
/// positive-height, bottom-up inputs. Shape and duplicated logical/stored-row
/// consistency checks are source-safety policy at this public boundary.
pub fn convert_ordinary_bitmap_0x81_to_logical_rgb16(
    bitmap: &IndexedBitmap,
    display_format_discriminator: u8,
) -> Result<OrdinaryBitmapLogicalRgb16Source, OrdinaryBitmapRgb16Error> {
    if bitmap.width == 0 || bitmap.height == 0 {
        return Err(OrdinaryBitmapRgb16Error::InvalidExtent([
            bitmap.width,
            bitmap.height,
        ]));
    }
    if bitmap.source_was_top_down {
        return Err(OrdinaryBitmapRgb16Error::UnsupportedTopDownSource);
    }
    if bitmap.palette.is_empty() || bitmap.palette.len() > 256 {
        return Err(OrdinaryBitmapRgb16Error::InvalidEmbeddedPaletteEntryCount(
            bitmap.palette.len(),
        ));
    }

    let expected_pixels = u64::from(bitmap.width) * u64::from(bitmap.height);
    if u64::try_from(bitmap.pixels.len()).ok() != Some(expected_pixels) {
        return Err(OrdinaryBitmapRgb16Error::PixelCount {
            expected: expected_pixels,
            actual: bitmap.pixels.len(),
        });
    }

    let expected_stride = (u64::from(bitmap.width) + 3) & !3;
    if u64::from(bitmap.row_stride) != expected_stride {
        return Err(OrdinaryBitmapRgb16Error::StoredRowStride {
            expected: expected_stride,
            actual: bitmap.row_stride,
        });
    }
    let expected_stored_pixels = expected_stride * u64::from(bitmap.height);
    if u64::try_from(bitmap.stored_pixels.len()).ok() != Some(expected_stored_pixels) {
        return Err(OrdinaryBitmapRgb16Error::StoredPixelCount {
            expected: expected_stored_pixels,
            actual: bitmap.stored_pixels.len(),
        });
    }

    let width = usize::try_from(bitmap.width)
        .expect("validated logical pixel count proves width fits usize");
    let stride = usize::try_from(bitmap.row_stride)
        .expect("validated stored pixel count proves row stride fits usize");
    for (logical_offset, &logical_index) in bitmap.pixels.iter().enumerate() {
        let row = logical_offset / width;
        let column = logical_offset % width;
        let stored_offset = row * stride + column;
        let stored_index = bitmap.stored_pixels[stored_offset];
        if logical_index != stored_index {
            return Err(OrdinaryBitmapRgb16Error::LogicalStoredPixelMismatch {
                logical_offset,
                stored_offset,
                logical_index,
                stored_index,
            });
        }
    }

    if let Some((stored_offset, &index)) = bitmap
        .stored_pixels
        .iter()
        .enumerate()
        .find(|(_, index)| usize::from(**index) >= bitmap.palette.len())
    {
        return Err(OrdinaryBitmapRgb16Error::StoredPaletteIndexOutOfBounds {
            stored_offset,
            index,
            palette_entries: bitmap.palette.len(),
        });
    }

    let layout = if display_format_discriminator == 5 {
        OrdinaryBitmapRgb16Layout::Rgb555
    } else {
        OrdinaryBitmapRgb16Layout::Rgb565
    };
    let embedded_palette_rgb = bitmap
        .palette
        .iter()
        .map(|entry| [entry.red, entry.green, entry.blue])
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let packed_color_key = pack_rgb16(layout, embedded_palette_rgb[0]);
    let logical_words = bitmap
        .stored_pixels
        .iter()
        .map(|&index| pack_rgb16(layout, embedded_palette_rgb[usize::from(index)]))
        .collect::<Vec<_>>()
        .into_boxed_slice();

    Ok(OrdinaryBitmapLogicalRgb16Source {
        dib_extent: [bitmap.width, bitmap.height],
        aligned_surface_extent: [bitmap.row_stride, bitmap.height],
        source_was_top_down: bitmap.source_was_top_down,
        display_format_discriminator,
        layout,
        embedded_palette_rgb,
        stored_indices: bitmap.stored_pixels.clone().into_boxed_slice(),
        logical_words,
        packed_color_key,
    })
}

fn pack_rgb16(layout: OrdinaryBitmapRgb16Layout, [red, green, blue]: [u8; 3]) -> u16 {
    match layout {
        OrdinaryBitmapRgb16Layout::Rgb555 => {
            (u16::from(red >> 3) << 10) | (u16::from(green >> 3) << 5) | u16::from(blue >> 3)
        }
        OrdinaryBitmapRgb16Layout::Rgb565 => {
            (u16::from(red >> 3) << 11) | (u16::from(green >> 2) << 5) | u16::from(blue >> 3)
        }
    }
}

#[cfg(test)]
mod tests {
    use cmr_formats::bitmap::{BitmapMetadata, BitmapPaletteEntry};

    use super::*;

    fn bitmap() -> IndexedBitmap {
        let mut palette = vec![entry(0, 0, 0, 0); 4];
        palette[0] = entry(9, 13, 17, 0xaa);
        palette[1] = entry(255, 129, 7, 0xbb);
        palette[2] = entry(33, 65, 97, 0xcc);
        palette[3] = entry(248, 252, 255, 0xdd);
        IndexedBitmap {
            width: 3,
            height: 2,
            source_was_top_down: false,
            palette,
            pixels: vec![0, 1, 2, 3, 2, 1],
            row_stride: 4,
            stored_pixels: vec![0, 1, 2, 3, 3, 2, 1, 0],
            metadata: BitmapMetadata {
                reserved_1: 0,
                reserved_2: 0,
                x_pixels_per_meter: 0,
                y_pixels_per_meter: 0,
                important_colors: 0,
            },
        }
    }

    const fn entry(red: u8, green: u8, blue: u8, reserved: u8) -> BitmapPaletteEntry {
        BitmapPaletteEntry {
            red,
            green,
            blue,
            reserved,
        }
    }

    #[test]
    fn discriminator_five_expands_complete_aligned_rows_as_rgb555() {
        let converted = convert_ordinary_bitmap_0x81_to_logical_rgb16(&bitmap(), 5).unwrap();

        assert_eq!(converted.dib_extent(), [3, 2]);
        assert_eq!(converted.aligned_surface_extent(), [4, 2]);
        assert_eq!(converted.display_format_discriminator(), 5);
        assert_eq!(converted.layout(), OrdinaryBitmapRgb16Layout::Rgb555);
        assert_eq!(converted.stored_indices(), [0, 1, 2, 3, 3, 2, 1, 0]);
        assert_eq!(
            converted.logical_words(),
            [
                0x0422, 0x7e00, 0x110c, 0x7fff, 0x7fff, 0x110c, 0x7e00, 0x0422
            ]
        );
        assert_eq!(converted.packed_color_key(), 0x0422);
        assert_eq!(converted.embedded_palette_rgb()[0], [9, 13, 17]);
    }

    #[test]
    fn every_nonfive_discriminator_uses_rgb565_and_embedded_palette_entry_zero_as_key() {
        let zero = convert_ordinary_bitmap_0x81_to_logical_rgb16(&bitmap(), 0).unwrap();
        let other = convert_ordinary_bitmap_0x81_to_logical_rgb16(&bitmap(), 0xff).unwrap();

        assert_eq!(zero.layout(), OrdinaryBitmapRgb16Layout::Rgb565);
        assert_eq!(other.layout(), OrdinaryBitmapRgb16Layout::Rgb565);
        assert_eq!(zero.logical_words(), other.logical_words());
        assert_eq!(zero.logical_words()[1], 0xfc00);
        assert_eq!(zero.packed_color_key(), 0x0862);
        assert_ne!(zero.packed_color_key(), 0);
    }

    #[test]
    fn palette_reserved_bytes_do_not_change_copied_rgb_or_words() {
        let first = bitmap();
        let mut second = first.clone();
        for (index, entry) in second.palette.iter_mut().enumerate() {
            entry.reserved = index as u8;
        }

        let first = convert_ordinary_bitmap_0x81_to_logical_rgb16(&first, 5).unwrap();
        let second = convert_ordinary_bitmap_0x81_to_logical_rgb16(&second, 5).unwrap();

        assert_eq!(first.embedded_palette_rgb(), second.embedded_palette_rgb());
        assert_eq!(first.logical_words(), second.logical_words());
    }

    #[test]
    fn rejects_invalid_shapes_and_stored_padding_index_before_output() {
        let mut empty_extent = bitmap();
        empty_extent.width = 0;
        assert_eq!(
            convert_ordinary_bitmap_0x81_to_logical_rgb16(&empty_extent, 5),
            Err(OrdinaryBitmapRgb16Error::InvalidExtent([0, 2]))
        );

        let mut top_down = bitmap();
        top_down.source_was_top_down = true;
        assert_eq!(
            convert_ordinary_bitmap_0x81_to_logical_rgb16(&top_down, 5),
            Err(OrdinaryBitmapRgb16Error::UnsupportedTopDownSource)
        );

        let mut no_palette = bitmap();
        no_palette.palette.clear();
        assert_eq!(
            convert_ordinary_bitmap_0x81_to_logical_rgb16(&no_palette, 5),
            Err(OrdinaryBitmapRgb16Error::InvalidEmbeddedPaletteEntryCount(
                0
            ))
        );

        let mut bad_pixels = bitmap();
        bad_pixels.pixels.pop();
        assert_eq!(
            convert_ordinary_bitmap_0x81_to_logical_rgb16(&bad_pixels, 5),
            Err(OrdinaryBitmapRgb16Error::PixelCount {
                expected: 6,
                actual: 5,
            })
        );

        let mut bad_stride = bitmap();
        bad_stride.row_stride = 3;
        assert_eq!(
            convert_ordinary_bitmap_0x81_to_logical_rgb16(&bad_stride, 5),
            Err(OrdinaryBitmapRgb16Error::StoredRowStride {
                expected: 4,
                actual: 3,
            })
        );

        let mut short_stored = bitmap();
        short_stored.stored_pixels.pop();
        assert_eq!(
            convert_ordinary_bitmap_0x81_to_logical_rgb16(&short_stored, 5),
            Err(OrdinaryBitmapRgb16Error::StoredPixelCount {
                expected: 8,
                actual: 7,
            })
        );

        let mut contradictory_rows = bitmap();
        contradictory_rows.pixels[4] = 0;
        assert_eq!(
            convert_ordinary_bitmap_0x81_to_logical_rgb16(&contradictory_rows, 5),
            Err(OrdinaryBitmapRgb16Error::LogicalStoredPixelMismatch {
                logical_offset: 4,
                stored_offset: 5,
                logical_index: 0,
                stored_index: 2,
            })
        );

        let mut bad_padding = bitmap();
        bad_padding.stored_pixels[3] = 4;
        assert_eq!(
            convert_ordinary_bitmap_0x81_to_logical_rgb16(&bad_padding, 5),
            Err(OrdinaryBitmapRgb16Error::StoredPaletteIndexOutOfBounds {
                stored_offset: 3,
                index: 4,
                palette_entries: 4,
            })
        );
    }
}
