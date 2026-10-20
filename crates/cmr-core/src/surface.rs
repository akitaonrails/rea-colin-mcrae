use std::fmt;

use cmr_formats::bitmap::{BitmapPaletteEntry, IndexedBitmap};
use cmr_formats::riff_palette::{LegacyPalette256, PaletteEntry};

/// Backend-neutral form of an 8-bit indexed legacy surface. Pixels are tightly
/// packed and ordered from the top row downward.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexedSurface {
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
    pub pixels: Vec<u8>,
    pub palette: [PaletteEntry; 256],
}

/// Selects the two recovered 8-to-8-bit copy paths.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaletteMapping {
    /// Direct byte copy, used when remapping is disabled by the surface flag or
    /// legacy global mode.
    Identity,
    /// Map nonzero source indices to the nearest nonzero active-palette entry.
    NearestNonzero,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SurfaceError {
    EmbeddedPaletteEntryCount(usize),
    PixelCount { expected: u64, actual: usize },
    UnsupportedUnalignedWidth(u32),
}

impl fmt::Display for SurfaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmbeddedPaletteEntryCount(count) => {
                write!(formatter, "indexed bitmap has {count} palette entries")
            }
            Self::PixelCount { expected, actual } => {
                write!(
                    formatter,
                    "indexed surface expects {expected} pixels, found {actual}"
                )
            }
            Self::UnsupportedUnalignedWidth(width) => write!(
                formatter,
                "indexed compatibility surface width {width} is not four-byte aligned"
            ),
        }
    }
}

impl std::error::Error for SurfaceError {}

/// Reconstructs the observed 8-bit surface-copy subset.
///
/// Active RGB comes from the separately loaded hardware palette. Flags are
/// initialized to zero because every observed startup PAL flag is zero while
/// the original destination's prior flag state has not yet been recovered.
/// Widths outside the observed four-byte-aligned subset are rejected: the
/// legacy routine exposes BMP padding as extra columns in that case.
pub fn indexed_surface(
    bitmap: IndexedBitmap,
    active_palette: LegacyPalette256,
    mapping: PaletteMapping,
) -> Result<IndexedSurface, SurfaceError> {
    if bitmap.palette.len() != 256 {
        return Err(SurfaceError::EmbeddedPaletteEntryCount(
            bitmap.palette.len(),
        ));
    }
    if bitmap.width & 3 != 0 {
        return Err(SurfaceError::UnsupportedUnalignedWidth(bitmap.width));
    }
    let expected_pixels = u64::from(bitmap.width) * u64::from(bitmap.height);
    if u64::try_from(bitmap.pixels.len()).ok() != Some(expected_pixels) {
        return Err(SurfaceError::PixelCount {
            expected: expected_pixels,
            actual: bitmap.pixels.len(),
        });
    }

    let pixels = match mapping {
        PaletteMapping::Identity => bitmap.pixels,
        PaletteMapping::NearestNonzero => {
            let map = nearest_nonzero_map(&bitmap.palette, &active_palette);
            bitmap
                .pixels
                .into_iter()
                .map(|index| map[index as usize])
                .collect()
        }
    };
    let palette = active_palette.entries.map(|active| PaletteEntry {
        red: active.red,
        green: active.green,
        blue: active.blue,
        flags: 0,
    });

    Ok(IndexedSurface {
        width: bitmap.width,
        height: bitmap.height,
        pitch: bitmap.width,
        pixels,
        palette,
    })
}

fn nearest_nonzero_map(embedded: &[BitmapPaletteEntry], active: &LegacyPalette256) -> [u8; 256] {
    let mut map = [0u8; 256];
    for source_index in 1..256 {
        let source = embedded[source_index];
        let mut best_distance = 768u16;
        let mut best_index = 1u8;
        for destination_index in 1..256 {
            let destination = active.entries[destination_index];
            let distance = u16::from(source.red.abs_diff(destination.red))
                + u16::from(source.green.abs_diff(destination.green))
                + u16::from(source.blue.abs_diff(destination.blue));
            if distance < best_distance {
                best_distance = distance;
                best_index = destination_index as u8;
                if distance == 0 {
                    break;
                }
            }
        }
        map[source_index] = best_index;
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use cmr_formats::bitmap::BitmapMetadata;

    fn bitmap(palette_entries: usize) -> IndexedBitmap {
        IndexedBitmap {
            width: 4,
            height: 1,
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

    fn palette() -> LegacyPalette256 {
        LegacyPalette256 {
            entries: [PaletteEntry {
                red: 10,
                green: 11,
                blue: 12,
                flags: 99,
            }; 256],
        }
    }

    #[test]
    fn identity_surface_keeps_indices_and_uses_external_rgb_with_zero_flags() {
        let surface = indexed_surface(bitmap(256), palette(), PaletteMapping::Identity).unwrap();

        assert_eq!((surface.width, surface.height, surface.pitch), (4, 1, 4));
        assert_eq!(surface.pixels, [0, 1, 2, 3]);
        assert!(surface.palette.iter().all(|entry| {
            entry.red == 10 && entry.green == 11 && entry.blue == 12 && entry.flags == 0
        }));
    }

    #[test]
    fn nearest_mapping_covers_zero_exclusion_ties_and_last_index() {
        let mut bitmap = bitmap(256);
        bitmap.palette[0] = color(0, 0, 0);
        bitmap.palette[1] = color(1, 2, 3);
        bitmap.palette[2] = color(10, 10, 10);
        bitmap.palette[255] = color(250, 250, 250);
        bitmap.pixels = vec![0, 1, 2, 255];

        let mut palette = palette();
        for entry in &mut palette.entries {
            *entry = entry_color(100, 100, 100);
        }
        // A nonzero source entry may exactly match destination zero, but zero
        // is excluded from the recovered search range.
        palette.entries[0] = entry_color(1, 2, 3);
        palette.entries[1] = entry_color(2, 2, 3);
        // Source entry two is equidistant from entries two and three; strict
        // improvement retains the lower destination index.
        palette.entries[2] = entry_color(0, 10, 10);
        palette.entries[3] = entry_color(20, 10, 10);
        // Both endpoints of the recovered 1..=255 loops are exercised.
        palette.entries[255] = entry_color(250, 250, 250);

        let surface = indexed_surface(bitmap, palette, PaletteMapping::NearestNonzero).unwrap();

        assert_eq!(surface.pixels, [0, 1, 2, 255]);
    }

    #[test]
    fn surface_rejects_palette_pixel_and_unaligned_width_outside_recovered_subset() {
        assert_eq!(
            indexed_surface(bitmap(16), palette(), PaletteMapping::Identity),
            Err(SurfaceError::EmbeddedPaletteEntryCount(16))
        );

        let mut invalid_pixels = bitmap(256);
        invalid_pixels.pixels.pop();
        assert_eq!(
            indexed_surface(invalid_pixels, palette(), PaletteMapping::Identity),
            Err(SurfaceError::PixelCount {
                expected: 4,
                actual: 3,
            })
        );

        let mut unaligned = bitmap(256);
        unaligned.width = 3;
        unaligned.pixels.pop();
        assert_eq!(
            indexed_surface(unaligned, palette(), PaletteMapping::Identity),
            Err(SurfaceError::UnsupportedUnalignedWidth(3))
        );
    }

    fn color(red: u8, green: u8, blue: u8) -> BitmapPaletteEntry {
        BitmapPaletteEntry {
            red,
            green,
            blue,
            reserved: 0,
        }
    }

    fn entry_color(red: u8, green: u8, blue: u8) -> PaletteEntry {
        PaletteEntry {
            red,
            green,
            blue,
            flags: 0,
        }
    }
}
