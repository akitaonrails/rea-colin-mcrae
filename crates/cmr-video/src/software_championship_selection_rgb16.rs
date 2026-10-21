//! Nominal owned RGB16 source for championship Selection Stretch.

use std::fmt;

use cmr_core::{
    championship_setup_frame::ChampionshipBitmapIdentity, root_menu_assets::ROOT_MENU_SELECT_PATH,
};
use cmr_formats::bitmap::IndexedBitmap;

use crate::ordinary_bitmap_rgb16::{
    OrdinaryBitmapLogicalRgb16Source, OrdinaryBitmapRgb16Error,
    convert_ordinary_bitmap_0x81_to_logical_rgb16,
};

/// Conversion failure retaining the stable logical identity and portable
/// original-media-relative path of the required Selection source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SoftwareChampionshipSelectionRgb16Error {
    pub identity: ChampionshipBitmapIdentity,
    pub media_relative_path: &'static str,
    pub source: OrdinaryBitmapRgb16Error,
}

impl fmt::Display for SoftwareChampionshipSelectionRgb16Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "cannot prepare championship RGB16 source {:?} at {}: {}",
            self.identity, self.media_relative_path, self.source
        )
    }
}

impl std::error::Error for SoftwareChampionshipSelectionRgb16Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// Converted source admitted specifically for championship Selection Stretch.
///
/// The private field prevents an ordinary info box, logo, or Chequered source
/// from satisfying the Selection slot accidentally.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SoftwareChampionshipSelectionRgb16Source(OrdinaryBitmapLogicalRgb16Source);

impl SoftwareChampionshipSelectionRgb16Source {
    #[must_use]
    pub const fn aligned_surface_extent(&self) -> [u32; 2] {
        self.0.aligned_surface_extent()
    }

    #[must_use]
    pub const fn logical_source(&self) -> &OrdinaryBitmapLogicalRgb16Source {
        &self.0
    }
}

pub fn prepare_software_championship_selection_rgb16_source(
    bitmap: &IndexedBitmap,
    display_format_discriminator: u8,
) -> Result<SoftwareChampionshipSelectionRgb16Source, SoftwareChampionshipSelectionRgb16Error> {
    convert_ordinary_bitmap_0x81_to_logical_rgb16(bitmap, display_format_discriminator)
        .map(SoftwareChampionshipSelectionRgb16Source)
        .map_err(|source| SoftwareChampionshipSelectionRgb16Error {
            identity: ChampionshipBitmapIdentity::Selection,
            media_relative_path: ROOT_MENU_SELECT_PATH,
            source,
        })
}

#[cfg(test)]
mod tests {
    use cmr_formats::bitmap::{BitmapMetadata, BitmapPaletteEntry};

    use super::*;

    fn bitmap() -> IndexedBitmap {
        IndexedBitmap {
            width: 1,
            height: 1,
            source_was_top_down: false,
            palette: vec![BitmapPaletteEntry {
                red: 8,
                green: 16,
                blue: 24,
                reserved: 0,
            }],
            pixels: vec![0],
            row_stride: 4,
            stored_pixels: vec![0; 4],
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
    fn nominal_selection_source_retains_aligned_extent() {
        let source = prepare_software_championship_selection_rgb16_source(&bitmap(), 5).unwrap();
        assert_eq!(source.aligned_surface_extent(), [4, 1]);
    }

    #[test]
    fn conversion_error_retains_selection_identity_and_relative_path() {
        let mut invalid = bitmap();
        invalid.source_was_top_down = true;

        assert_eq!(
            prepare_software_championship_selection_rgb16_source(&invalid, 5),
            Err(SoftwareChampionshipSelectionRgb16Error {
                identity: ChampionshipBitmapIdentity::Selection,
                media_relative_path: ROOT_MENU_SELECT_PATH,
                source: OrdinaryBitmapRgb16Error::UnsupportedTopDownSource,
            })
        );
        assert!(!ROOT_MENU_SELECT_PATH.starts_with('/'));
    }
}
