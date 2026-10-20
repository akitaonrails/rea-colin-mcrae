use std::fmt;

use cmr_formats::bitmap::IndexedBitmap;
use cmr_formats::riff_palette::{LegacyPalette256, PaletteEntry};

/// Backend-neutral form of the indexed surface used by the recovered startup
/// path. Pixels are tightly packed and ordered from the top row downward.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexedSurface {
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
    pub pixels: Vec<u8>,
    pub palette: [PaletteEntry; 256],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SurfaceError {
    EmbeddedPaletteEntryCount(usize),
    PixelCount { expected: u64, actual: usize },
}

impl fmt::Display for SurfaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmbeddedPaletteEntryCount(count) => {
                write!(
                    formatter,
                    "indexed startup bitmap has {count} palette entries"
                )
            }
            Self::PixelCount { expected, actual } => {
                write!(
                    formatter,
                    "indexed surface expects {expected} pixels, found {actual}"
                )
            }
        }
    }
}

impl std::error::Error for SurfaceError {}

/// Builds the startup surface while keeping bitmap indices unchanged. Active
/// RGB comes from the separately loaded hardware palette. Flags are initialized
/// to zero: all observed startup PAL flags are zero, while the original copy
/// loop's pre-existing destination flags have not yet been recovered.
pub fn startup_indexed_surface(
    bitmap: IndexedBitmap,
    active_palette: LegacyPalette256,
) -> Result<IndexedSurface, SurfaceError> {
    if bitmap.palette.len() != 256 {
        return Err(SurfaceError::EmbeddedPaletteEntryCount(
            bitmap.palette.len(),
        ));
    }
    let expected_pixels = u64::from(bitmap.width) * u64::from(bitmap.height);
    if u64::try_from(bitmap.pixels.len()).ok() != Some(expected_pixels) {
        return Err(SurfaceError::PixelCount {
            expected: expected_pixels,
            actual: bitmap.pixels.len(),
        });
    }

    let palette = std::array::from_fn(|index| {
        let active = active_palette.entries[index];
        PaletteEntry {
            red: active.red,
            green: active.green,
            blue: active.blue,
            flags: 0,
        }
    });

    Ok(IndexedSurface {
        width: bitmap.width,
        height: bitmap.height,
        pitch: bitmap.width,
        pixels: bitmap.pixels,
        palette,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cmr_formats::bitmap::{BitmapMetadata, BitmapPaletteEntry};

    fn bitmap(palette_entries: usize) -> IndexedBitmap {
        IndexedBitmap {
            width: 2,
            height: 2,
            source_was_top_down: false,
            palette: vec![
                BitmapPaletteEntry {
                    red: 200,
                    green: 201,
                    blue: 202,
                    reserved: 17,
                };
                palette_entries
            ],
            pixels: vec![0, 1, 2, 3],
            metadata: BitmapMetadata {
                reserved_1: 0,
                reserved_2: 0,
                x_pixels_per_meter: 0,
                y_pixels_per_meter: 0,
                important_colors: 0,
            },
        }
    }

    #[test]
    fn startup_surface_keeps_indices_and_uses_external_palette() {
        let palette = LegacyPalette256 {
            entries: [PaletteEntry {
                red: 10,
                green: 11,
                blue: 12,
                flags: 99,
            }; 256],
        };

        let surface = startup_indexed_surface(bitmap(256), palette).unwrap();

        assert_eq!((surface.width, surface.height, surface.pitch), (2, 2, 2));
        assert_eq!(surface.pixels, [0, 1, 2, 3]);
        assert!(surface.palette.iter().all(|entry| {
            entry.red == 10 && entry.green == 11 && entry.blue == 12 && entry.flags == 0
        }));
    }

    #[test]
    fn startup_surface_requires_the_observed_256_entry_bitmap_palette() {
        assert_eq!(
            startup_indexed_surface(
                bitmap(16),
                LegacyPalette256 {
                    entries: [PaletteEntry {
                        red: 0,
                        green: 0,
                        blue: 0,
                        flags: 0,
                    }; 256],
                },
            ),
            Err(SurfaceError::EmbeddedPaletteEntryCount(16))
        );
    }

    #[test]
    fn startup_surface_rejects_an_inconsistent_pixel_buffer() {
        let mut invalid = bitmap(256);
        invalid.pixels.pop();
        assert_eq!(
            startup_indexed_surface(
                invalid,
                LegacyPalette256 {
                    entries: [PaletteEntry {
                        red: 0,
                        green: 0,
                        blue: 0,
                        flags: 0,
                    }; 256],
                },
            ),
            Err(SurfaceError::PixelCount {
                expected: 4,
                actual: 3,
            })
        );
    }
}
