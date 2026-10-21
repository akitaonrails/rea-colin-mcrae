//! Persistent sources for the recovered no-driver-`Load` queued-font branch.
//!
//! This module implements only exact `0x90` atlas preparation when the
//! recovered device branch selects ARGB4444 directly. It does not model the
//! optional driver `Load`, queued primitive submission, tuple interpretation,
//! blending, or final framebuffer pixels.

use std::fmt;

use cmr_core::{
    blit::ColorKey,
    font_assets::{FONT_COUNT, FontSet, FontStyle},
};
use cmr_formats::{bitmap::IndexedBitmap, font::BitmapFont};

const LITERAL_ZERO_COLOR_KEY: ColorKey = ColorKey { low: 0, high: 0 };

/// One converted atlas admitted specifically for the no-driver-`Load`
/// alternate queued-font path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SoftwareQueuedFontArgb4444Source {
    style: FontStyle,
    dib_extent: [u32; 2],
    aligned_surface_extent: [u32; 2],
    embedded_palette_rgb: Box<[[u8; 3]]>,
    stored_indices: Box<[u8]>,
    logical_words: Box<[u16]>,
}

impl SoftwareQueuedFontArgb4444Source {
    #[must_use]
    pub const fn style(&self) -> FontStyle {
        self.style
    }

    #[must_use]
    pub const fn dib_extent(&self) -> [u32; 2] {
        self.dib_extent
    }

    /// Recovered logical surface extent. Width is the normalized aligned row
    /// stride, not a claim about native texture pitch or byte order.
    #[must_use]
    pub const fn aligned_surface_extent(&self) -> [u32; 2] {
        self.aligned_surface_extent
    }

    #[must_use]
    pub fn embedded_palette_rgb(&self) -> &[[u8; 3]] {
        &self.embedded_palette_rgb
    }

    /// Complete normalized aligned rows, including retained padding bytes.
    #[must_use]
    pub fn stored_indices(&self) -> &[u8] {
        &self.stored_indices
    }

    /// Logical ARGB4444 words corresponding one-for-one with stored indices.
    #[must_use]
    pub fn logical_words(&self) -> &[u16] {
        &self.logical_words
    }

    /// Literal numeric key installed by the recovered prepared-object path.
    /// It is not derived from embedded palette entry zero.
    #[must_use]
    pub const fn color_key(&self) -> ColorKey {
        LITERAL_ZERO_COLOR_KEY
    }
}

/// Persistent converted ownership for all 16 frontend font styles.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SoftwareQueuedFontArgb4444Pool {
    fonts: FontSet,
    sources: [SoftwareQueuedFontArgb4444Source; FONT_COUNT],
}

impl SoftwareQueuedFontArgb4444Pool {
    /// Parsed metrics and glyph lookup retained beside the converted source.
    #[must_use]
    pub fn font(&self, style: FontStyle) -> &BitmapFont {
        self.fonts.get(style)
    }

    #[must_use]
    pub fn source(&self, style: FontStyle) -> &SoftwareQueuedFontArgb4444Source {
        &self.sources[style as usize]
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = &SoftwareQueuedFontArgb4444Source> {
        self.sources.iter()
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        FONT_COUNT
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        false
    }
}

/// Invalid parsed-atlas state rejected before a converted source is returned.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SoftwareQueuedFontArgb4444SourceError {
    InvalidExtent([u32; 2]),
    UnsupportedTopDownSource,
    InvalidEmbeddedPaletteEntryCount(usize),
    LogicalPixelCount {
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
}

impl fmt::Display for SoftwareQueuedFontArgb4444SourceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "queued-font ARGB4444 preparation failed: {self:?}"
        )
    }
}

impl std::error::Error for SoftwareQueuedFontArgb4444SourceError {}

/// Contextual pool-construction failure with stable style and
/// original-media-relative path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SoftwareQueuedFontArgb4444PoolError {
    pub style: FontStyle,
    pub media_relative_path: &'static str,
    pub source: SoftwareQueuedFontArgb4444SourceError,
}

impl fmt::Display for SoftwareQueuedFontArgb4444PoolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "cannot prepare queued font {:?} at {} as no-driver-Load ARGB4444: {}",
            self.style, self.media_relative_path, self.source
        )
    }
}

impl std::error::Error for SoftwareQueuedFontArgb4444PoolError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// Converts all parsed frontend atlases once in recovered startup style order.
///
/// Construction is all-or-nothing. The returned pool owns every converted
/// row, palette tuple, and word; it does not retain backend or upload handles.
pub fn prepare_software_queued_font_argb4444_pool(
    fonts: FontSet,
) -> Result<SoftwareQueuedFontArgb4444Pool, SoftwareQueuedFontArgb4444PoolError> {
    let mut sources = Vec::with_capacity(FONT_COUNT);
    for style in FontStyle::ALL {
        sources.push(prepare_source(style, fonts.get(style).atlas())?);
    }
    Ok(SoftwareQueuedFontArgb4444Pool {
        fonts,
        sources: sources
            .try_into()
            .expect("one converted source was prepared for every font style"),
    })
}

fn prepare_source(
    style: FontStyle,
    atlas: &IndexedBitmap,
) -> Result<SoftwareQueuedFontArgb4444Source, SoftwareQueuedFontArgb4444PoolError> {
    validate_atlas(atlas).map_err(|source| SoftwareQueuedFontArgb4444PoolError {
        style,
        media_relative_path: style.path(),
        source,
    })?;

    let embedded_palette_rgb = atlas
        .palette
        .iter()
        .map(|entry| [entry.red, entry.green, entry.blue])
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let logical_words = atlas
        .stored_pixels
        .iter()
        .map(|&index| pack_argb4444(embedded_palette_rgb[usize::from(index)]))
        .collect::<Vec<_>>()
        .into_boxed_slice();

    Ok(SoftwareQueuedFontArgb4444Source {
        style,
        dib_extent: [atlas.width, atlas.height],
        aligned_surface_extent: [atlas.row_stride, atlas.height],
        embedded_palette_rgb,
        stored_indices: atlas.stored_pixels.clone().into_boxed_slice(),
        logical_words,
    })
}

fn validate_atlas(atlas: &IndexedBitmap) -> Result<(), SoftwareQueuedFontArgb4444SourceError> {
    if atlas.width == 0 || atlas.height == 0 {
        return Err(SoftwareQueuedFontArgb4444SourceError::InvalidExtent([
            atlas.width,
            atlas.height,
        ]));
    }
    if atlas.source_was_top_down {
        return Err(SoftwareQueuedFontArgb4444SourceError::UnsupportedTopDownSource);
    }
    if atlas.palette.len() != 256 {
        return Err(
            SoftwareQueuedFontArgb4444SourceError::InvalidEmbeddedPaletteEntryCount(
                atlas.palette.len(),
            ),
        );
    }

    let expected_logical = u64::from(atlas.width) * u64::from(atlas.height);
    if u64::try_from(atlas.pixels.len()).ok() != Some(expected_logical) {
        return Err(SoftwareQueuedFontArgb4444SourceError::LogicalPixelCount {
            expected: expected_logical,
            actual: atlas.pixels.len(),
        });
    }

    let expected_stride = (u64::from(atlas.width) + 3) & !3;
    if u64::from(atlas.row_stride) != expected_stride {
        return Err(SoftwareQueuedFontArgb4444SourceError::StoredRowStride {
            expected: expected_stride,
            actual: atlas.row_stride,
        });
    }
    let expected_stored = expected_stride * u64::from(atlas.height);
    if u64::try_from(atlas.stored_pixels.len()).ok() != Some(expected_stored) {
        return Err(SoftwareQueuedFontArgb4444SourceError::StoredPixelCount {
            expected: expected_stored,
            actual: atlas.stored_pixels.len(),
        });
    }

    let width = usize::try_from(atlas.width)
        .expect("validated logical pixel count proves width fits usize");
    let stride = usize::try_from(atlas.row_stride)
        .expect("validated stored pixel count proves stride fits usize");
    for (logical_offset, &logical_index) in atlas.pixels.iter().enumerate() {
        let row = logical_offset / width;
        let column = logical_offset % width;
        let stored_offset = row * stride + column;
        let stored_index = atlas.stored_pixels[stored_offset];
        if logical_index != stored_index {
            return Err(
                SoftwareQueuedFontArgb4444SourceError::LogicalStoredPixelMismatch {
                    logical_offset,
                    stored_offset,
                    logical_index,
                    stored_index,
                },
            );
        }
    }
    Ok(())
}

const fn pack_argb4444(rgb: [u8; 3]) -> u16 {
    let alpha = if rgb[0] == 0 && rgb[1] == 0 && rgb[2] == 0 {
        0
    } else {
        0xf000
    };
    alpha | ((rgb[0] as u16 >> 4) << 8) | ((rgb[1] as u16 >> 4) << 4) | (rgb[2] as u16 >> 4)
}

#[cfg(test)]
mod tests {
    use cmr_formats::{
        bitmap::{BitmapMetadata, BitmapPaletteEntry},
        font::{FontLimits, parse_legacy_font},
    };

    use super::*;

    #[test]
    fn pool_preserves_style_order_and_converts_aligned_rows_once() {
        let fonts = font_set();
        let pool = prepare_software_queued_font_argb4444_pool(fonts).unwrap();

        assert_eq!(pool.len(), FONT_COUNT);
        assert!(!pool.is_empty());
        assert_eq!(
            pool.iter()
                .map(SoftwareQueuedFontArgb4444Source::style)
                .collect::<Vec<_>>(),
            FontStyle::ALL
        );

        let title = pool.source(FontStyle::Title);
        assert_eq!(title.dib_extent(), [3, 2]);
        assert_eq!(title.aligned_surface_extent(), [4, 2]);
        assert_eq!(title.stored_indices(), [1, 2, 3, 4, 4, 3, 2, 1]);
        assert_eq!(
            title.logical_words(),
            [
                0xf123, 0xf001, 0xf000, 0x0000, 0x0000, 0xf000, 0xf001, 0xf123
            ]
        );
        assert_eq!(title.color_key(), ColorKey { low: 0, high: 0 });
        assert_eq!(title.embedded_palette_rgb().len(), 256);
        assert_eq!(title.embedded_palette_rgb()[1], [0x12, 0x23, 0x34]);
        assert_eq!(title.embedded_palette_rgb()[2], [0, 0, 0x1f]);
        assert_eq!(title.embedded_palette_rgb()[3], [0, 1, 0]);
        assert_eq!(title.embedded_palette_rgb()[4], [0, 0, 0]);
        assert_eq!(pool.font(FontStyle::Title).atlas().width, 3);
    }

    #[test]
    fn alpha_uses_exact_rgb_black_and_rgb_nibbles_truncate() {
        assert_eq!(pack_argb4444([0, 0, 0]), 0x0000);
        assert_eq!(pack_argb4444([0, 0, 1]), 0xf000);
        assert_eq!(pack_argb4444([1, 0, 0]), 0xf000);
        assert_eq!(pack_argb4444([0x1f, 0x2e, 0x3d]), 0xf123);
        assert_eq!(pack_argb4444([0xff, 0xff, 0xff]), 0xffff);
    }

    #[test]
    fn palette_reserved_bytes_do_not_change_words_or_retained_rgb() {
        let mut first = atlas();
        let mut second = first.clone();
        first.palette[1].reserved = 0;
        second.palette[1].reserved = 0xff;

        let first = prepare_source(FontStyle::Title, &first).unwrap();
        let second = prepare_source(FontStyle::Title, &second).unwrap();
        assert_eq!(first.embedded_palette_rgb(), second.embedded_palette_rgb());
        assert_eq!(first.logical_words(), second.logical_words());
    }

    #[test]
    fn malformed_atlas_errors_keep_style_and_relative_path() {
        let cases = [
            (
                mutate(atlas(), |atlas| atlas.width = 0),
                SoftwareQueuedFontArgb4444SourceError::InvalidExtent([0, 2]),
            ),
            (
                mutate(atlas(), |atlas| atlas.source_was_top_down = true),
                SoftwareQueuedFontArgb4444SourceError::UnsupportedTopDownSource,
            ),
            (
                mutate(atlas(), |atlas| {
                    atlas.palette.pop();
                }),
                SoftwareQueuedFontArgb4444SourceError::InvalidEmbeddedPaletteEntryCount(255),
            ),
            (
                mutate(atlas(), |atlas| {
                    atlas.pixels.pop();
                }),
                SoftwareQueuedFontArgb4444SourceError::LogicalPixelCount {
                    expected: 6,
                    actual: 5,
                },
            ),
            (
                mutate(atlas(), |atlas| atlas.row_stride = 3),
                SoftwareQueuedFontArgb4444SourceError::StoredRowStride {
                    expected: 4,
                    actual: 3,
                },
            ),
            (
                mutate(atlas(), |atlas| {
                    atlas.stored_pixels.pop();
                }),
                SoftwareQueuedFontArgb4444SourceError::StoredPixelCount {
                    expected: 8,
                    actual: 7,
                },
            ),
            (
                mutate(atlas(), |atlas| atlas.stored_pixels[1] = 9),
                SoftwareQueuedFontArgb4444SourceError::LogicalStoredPixelMismatch {
                    logical_offset: 1,
                    stored_offset: 1,
                    logical_index: 2,
                    stored_index: 9,
                },
            ),
        ];

        for (atlas, source) in cases {
            assert_eq!(
                prepare_source(FontStyle::OptionGrey, &atlas),
                Err(SoftwareQueuedFontArgb4444PoolError {
                    style: FontStyle::OptionGrey,
                    media_relative_path: FontStyle::OptionGrey.path(),
                    source,
                })
            );
            assert!(!FontStyle::OptionGrey.path().starts_with('/'));
        }
    }

    fn mutate(mut atlas: IndexedBitmap, change: impl FnOnce(&mut IndexedBitmap)) -> IndexedBitmap {
        change(&mut atlas);
        atlas
    }

    fn font_set() -> FontSet {
        FontSet::from_ordered_fonts(std::array::from_fn(|_| {
            parse_legacy_font(&synthetic_font(), FontLimits::default()).unwrap()
        }))
    }

    fn synthetic_font() -> Vec<u8> {
        const PIXEL_OFFSET: usize = 0x436;
        const FONT_HEADER_SIZE: usize = 24;
        let stride = 4usize;
        let height = 2usize;
        let trailer = PIXEL_OFFSET + stride * height;
        let mut input = vec![0; trailer + FONT_HEADER_SIZE];
        input[0..2].copy_from_slice(b"BM");
        input[18..22].copy_from_slice(&3i32.to_le_bytes());
        input[22..26].copy_from_slice(&2i32.to_le_bytes());
        input[28..30].copy_from_slice(&8u16.to_le_bytes());
        set_palette(&mut input, 1, [0x12, 0x23, 0x34], 0x55);
        set_palette(&mut input, 2, [0, 0, 0x1f], 0x66);
        set_palette(&mut input, 3, [0, 1, 0], 0x77);
        set_palette(&mut input, 4, [0, 0, 0], 0x88);
        // Stored FON rows are bottom-up; parsing normalizes both aligned rows.
        input[PIXEL_OFFSET..PIXEL_OFFSET + 4].copy_from_slice(&[4, 3, 2, 1]);
        input[PIXEL_OFFSET + 4..PIXEL_OFFSET + 8].copy_from_slice(&[1, 2, 3, 4]);
        input
    }

    fn set_palette(input: &mut [u8], index: usize, rgb: [u8; 3], reserved: u8) {
        let offset = 14 + 40 + index * 4;
        input[offset..offset + 4].copy_from_slice(&[rgb[2], rgb[1], rgb[0], reserved]);
    }

    fn atlas() -> IndexedBitmap {
        let palette = (0..256)
            .map(|index| BitmapPaletteEntry {
                red: index as u8,
                green: index as u8,
                blue: index as u8,
                reserved: 0,
            })
            .collect();
        IndexedBitmap {
            width: 3,
            height: 2,
            source_was_top_down: false,
            palette,
            pixels: vec![1, 2, 3, 4, 3, 2],
            row_stride: 4,
            stored_pixels: vec![1, 2, 3, 0, 4, 3, 2, 0],
            metadata: BitmapMetadata {
                reserved_1: 0,
                reserved_2: 0,
                x_pixels_per_meter: 0,
                y_pixels_per_meter: 0,
                important_colors: 0,
            },
        }
    }
}
