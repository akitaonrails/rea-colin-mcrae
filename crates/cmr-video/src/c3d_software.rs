//! Immutable retained C3D source textures for the software renderer.
//!
//! This is deliberately not yet a sampled/render-ready texture. Legacy
//! surface reduction, padding, palette attachment, and unified/split upload
//! topology remain explicit policy boundaries. Retaining normalized indices
//! and RGB palette data here lets later render bindings be rebuilt without
//! borrowing parser objects or rereading the user's installation.

use std::sync::Arc;

use cmr_core::root_menu_c3d_assets::RootMenuC3dSlot;
use cmr_core::root_menu_c3d_resources::C3dPreparedResource;
use cmr_formats::bitmap::{BitmapPaletteEntry, IndexedBitmap};

#[derive(Debug, Eq, PartialEq)]
struct RetainedC3dSourceInner {
    first_slot: RootMenuC3dSlot,
    width: u32,
    height: u32,
    source_dib_extent_low16: [u16; 2],
    indices: Box<[u8]>,
    palette: Box<[BitmapPaletteEntry]>,
    color_key_index: Option<u8>,
}

/// Owned, immutable source image retained by a software C3D binding catalog.
///
/// Clones share immutable storage so a backend can retain the currently bound
/// source after the catalog's `bind` callback returns.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SoftwareC3dRetainedSource(Arc<RetainedC3dSourceInner>);

impl SoftwareC3dRetainedSource {
    #[must_use]
    pub fn from_prepared(resource: &C3dPreparedResource) -> Self {
        Self::from_bitmap(
            resource.first_slot(),
            resource.bitmap(),
            resource.source_dib_extent_low16(),
            resource.surface_color_key_index(),
        )
    }

    /// First root slot that established the authoritative interned resource.
    #[must_use]
    pub fn first_slot(&self) -> RootMenuC3dSlot {
        self.0.first_slot
    }

    #[must_use]
    pub fn extent(&self) -> [u32; 2] {
        [self.0.width, self.0.height]
    }

    /// Raw low words consumed by the recovered legacy surface-layout policy.
    #[must_use]
    pub fn source_dib_extent_low16(&self) -> [u16; 2] {
        self.0.source_dib_extent_low16
    }

    #[must_use]
    pub fn indices(&self) -> &[u8] {
        &self.0.indices
    }

    #[must_use]
    pub fn palette(&self) -> &[BitmapPaletteEntry] {
        &self.0.palette
    }

    /// Root mode 2 does not install a surface color key. Index zero remains a
    /// normal sample and must not be treated as transparent by default.
    #[must_use]
    pub fn color_key_index(&self) -> Option<u8> {
        self.0.color_key_index
    }

    fn from_bitmap(
        first_slot: RootMenuC3dSlot,
        bitmap: &IndexedBitmap,
        source_dib_extent_low16: [u16; 2],
        color_key_index: Option<u8>,
    ) -> Self {
        let palette = bitmap
            .palette
            .iter()
            .map(|entry| BitmapPaletteEntry {
                red: entry.red,
                green: entry.green,
                blue: entry.blue,
                // The recovered legacy wrapper was zero-initialized and copied
                // only BGR channels from the BMP palette.
                reserved: 0,
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self(Arc::new(RetainedC3dSourceInner {
            first_slot,
            width: bitmap.width,
            height: bitmap.height,
            source_dib_extent_low16,
            indices: bitmap.pixels.clone().into_boxed_slice(),
            palette,
            color_key_index,
        }))
    }
}

#[cfg(test)]
mod tests {
    use cmr_formats::bitmap::BitmapMetadata;

    use super::*;

    #[test]
    fn retained_source_owns_normalized_indices_and_rgbx_palette() {
        let bitmap = IndexedBitmap {
            width: 2,
            height: 2,
            source_was_top_down: false,
            palette: vec![
                BitmapPaletteEntry {
                    red: 1,
                    green: 2,
                    blue: 3,
                    reserved: 0xaa,
                },
                BitmapPaletteEntry {
                    red: 4,
                    green: 5,
                    blue: 6,
                    reserved: 0xbb,
                },
            ],
            pixels: vec![0, 1, 1, 0],
            metadata: BitmapMetadata {
                reserved_1: 7,
                reserved_2: 8,
                x_pixels_per_meter: 9,
                y_pixels_per_meter: 10,
                important_colors: 2,
            },
        };

        let retained = SoftwareC3dRetainedSource::from_bitmap(
            RootMenuC3dSlot::Championship,
            &bitmap,
            [0xfffe, 2],
            None,
        );
        let shared = retained.clone();
        drop(bitmap);

        assert_eq!(retained.first_slot(), RootMenuC3dSlot::Championship);
        assert_eq!(retained.extent(), [2, 2]);
        assert_eq!(retained.source_dib_extent_low16(), [0xfffe, 2]);
        assert_eq!(retained.indices(), [0, 1, 1, 0]);
        assert_eq!(
            retained.palette(),
            [
                BitmapPaletteEntry {
                    red: 1,
                    green: 2,
                    blue: 3,
                    reserved: 0,
                },
                BitmapPaletteEntry {
                    red: 4,
                    green: 5,
                    blue: 6,
                    reserved: 0,
                },
            ]
        );
        assert_eq!(retained.color_key_index(), None);
        assert!(Arc::ptr_eq(&retained.0, &shared.0));
    }
}
