//! Immutable retained C3D source textures for the software renderer.
//!
//! This retains immutable normalized sources and point-samples the statically
//! proven aligned, zero-padding root subset. Palette attachment, material
//! creation, unified/split upload topology, and rasterization remain explicit
//! later boundaries.

use std::{fmt, sync::Arc};

use cmr_core::root_menu_c3d_assets::RootMenuC3dSlot;
use cmr_core::root_menu_c3d_execute::{
    RootMenuC3dBindingBuildError, RootMenuC3dBindingCatalog, bind_root_menu_c3d_resources,
};
use cmr_core::root_menu_c3d_resources::C3dPreparedResource;
use cmr_core::root_menu_c3d_runtime::{
    C3dSurfaceLayoutError, ResolvedC3dSurfaceLayout, RootMenuC3dMode2LoadStates,
    resolve_c3d_surface_layout,
};
use cmr_core::root_menu_c3d_topology::RootMenuC3dTopology;
use cmr_formats::bitmap::{BitmapPaletteEntry, IndexedBitmap};

#[derive(Debug, Eq, PartialEq)]
struct RetainedC3dSourceInner {
    first_slot: RootMenuC3dSlot,
    width: u32,
    height: u32,
    source_was_top_down: bool,
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

/// Point-decimated, index-preserving root texture source.
///
/// This still is not a backend texture: palette attachment, material creation,
/// upload topology, and final display-palette conversion remain later policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SoftwareC3dSampledTexture {
    source: SoftwareC3dRetainedSource,
    layout: ResolvedC3dSurfaceLayout,
    indices: Arc<[u8]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SoftwareC3dSamplingError {
    Layout(C3dSurfaceLayoutError),
    SourceExtentMismatch {
        normalized: [u32; 2],
        raw_low16: [u16; 2],
    },
    UnalignedSourceWidth(u16),
    UnsupportedTopDownSource,
    InvalidNormalizedIndexLength {
        expected: usize,
        actual: usize,
    },
    UnsupportedPadding {
        horizontal: i32,
        vertical: i32,
    },
    EmptySampledExtent([u16; 2]),
}

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
    pub fn source_was_top_down(&self) -> bool {
        self.0.source_was_top_down
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
            source_was_top_down: bitmap.source_was_top_down,
            source_dib_extent_low16,
            indices: bitmap.pixels.clone().into_boxed_slice(),
            palette,
            color_key_index,
        }))
    }
}

impl SoftwareC3dSampledTexture {
    /// Builds only the statically proven square, aligned root texture subset.
    ///
    /// General rectangular padding is rejected because the legacy surface's
    /// unwritten padding bytes have no deterministic value. Non-four-aligned
    /// source rows are rejected because normalized bitmap storage no longer
    /// contains the original BMP row-padding bytes consumed by the legacy
    /// aligned-width loop.
    pub fn from_retained_root(
        source: SoftwareC3dRetainedSource,
        load_states: RootMenuC3dMode2LoadStates,
    ) -> Result<Self, SoftwareC3dSamplingError> {
        let raw = source.source_dib_extent_low16();
        let normalized = source.extent();
        if source.source_was_top_down() {
            return Err(SoftwareC3dSamplingError::UnsupportedTopDownSource);
        }
        if normalized != [u32::from(raw[0]), u32::from(raw[1])] {
            return Err(SoftwareC3dSamplingError::SourceExtentMismatch {
                normalized,
                raw_low16: raw,
            });
        }
        if raw[0] & 3 != 0 {
            return Err(SoftwareC3dSamplingError::UnalignedSourceWidth(raw[0]));
        }
        let expected_source_len = usize::from(raw[0]) * usize::from(raw[1]);
        if source.indices().len() != expected_source_len {
            return Err(SoftwareC3dSamplingError::InvalidNormalizedIndexLength {
                expected: expected_source_len,
                actual: source.indices().len(),
            });
        }

        let gates = load_states.get(source.first_slot()).surface_layout;
        let layout =
            resolve_c3d_surface_layout(raw, gates).map_err(SoftwareC3dSamplingError::Layout)?;
        if layout.horizontal_padding() != 0 || layout.vertical_padding() != 0 {
            return Err(SoftwareC3dSamplingError::UnsupportedPadding {
                horizontal: layout.horizontal_padding(),
                vertical: layout.vertical_padding(),
            });
        }

        let [target_width, target_height] = layout.extent_bits();
        if target_width == 0 || target_height == 0 {
            return Err(SoftwareC3dSamplingError::EmptySampledExtent([
                target_width,
                target_height,
            ]));
        }
        let divisor = usize::from(layout.reduction().divisor());
        let source_width = usize::from(raw[0]);
        let mut indices =
            Vec::with_capacity(usize::from(target_width) * usize::from(target_height));
        for y in 0..usize::from(target_height) {
            for x in 0..usize::from(target_width) {
                indices.push(source.indices()[(y * divisor) * source_width + x * divisor]);
            }
        }

        Ok(Self {
            source,
            layout,
            indices: indices.into(),
        })
    }

    #[must_use]
    pub const fn layout(&self) -> ResolvedC3dSurfaceLayout {
        self.layout
    }

    #[must_use]
    pub fn extent(&self) -> [u16; 2] {
        self.layout.extent_bits()
    }

    #[must_use]
    pub fn indices(&self) -> &[u8] {
        &self.indices
    }

    #[must_use]
    pub fn palette(&self) -> &[BitmapPaletteEntry] {
        self.source.palette()
    }

    #[must_use]
    pub fn color_key_index(&self) -> Option<u8> {
        self.source.color_key_index()
    }

    #[must_use]
    pub const fn source(&self) -> &SoftwareC3dRetainedSource {
        &self.source
    }
}

/// Builds one sampled software texture for every interned root C3D resource.
///
/// The returned catalog keeps the topology arena identity, so a frame plan
/// cannot accidentally bind textures prepared from another asset set.
pub fn prepare_software_root_c3d_bindings(
    topology: &RootMenuC3dTopology<'_>,
    load_states: RootMenuC3dMode2LoadStates,
) -> Result<
    RootMenuC3dBindingCatalog<SoftwareC3dSampledTexture>,
    RootMenuC3dBindingBuildError<SoftwareC3dSamplingError>,
> {
    bind_root_menu_c3d_resources(topology, |_, resource| {
        SoftwareC3dSampledTexture::from_retained_root(
            SoftwareC3dRetainedSource::from_prepared(resource),
            load_states,
        )
    })
}

impl fmt::Display for SoftwareC3dSamplingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "unsupported root C3D sampling input: {self:?}")
    }
}

impl std::error::Error for SoftwareC3dSamplingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Layout(source) => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use cmr_core::root_menu_c3d_runtime::{
        C3dMode2CornerSource, C3dMode2LoadState, C3dSurfaceLayoutGates, C3dSurfaceReduction,
    };
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
        assert!(!retained.source_was_top_down());
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

    #[test]
    fn first_slot_gates_select_quarter_top_left_point_decimation() {
        let bitmap = IndexedBitmap {
            width: 8,
            height: 8,
            source_was_top_down: false,
            palette: vec![BitmapPaletteEntry {
                red: 1,
                green: 2,
                blue: 3,
                reserved: 4,
            }],
            pixels: (0..64).collect(),
            metadata: BitmapMetadata {
                reserved_1: 0,
                reserved_2: 0,
                x_pixels_per_meter: 0,
                y_pixels_per_meter: 0,
                important_colors: 0,
            },
        };
        let retained = SoftwareC3dRetainedSource::from_bitmap(
            RootMenuC3dSlot::Championship,
            &bitmap,
            [8, 8],
            None,
        );
        let default_state = C3dMode2LoadState {
            prepared_resource_count: 0,
            corner_source: C3dMode2CornerSource::OrdinaryDefaults,
            surface_layout: C3dSurfaceLayoutGates::default(),
        };
        let mut slots = [default_state; 6];
        slots[RootMenuC3dSlot::Championship.index()]
            .surface_layout
            .force_quarter = true;
        let states = RootMenuC3dMode2LoadStates::new(slots);

        let sampled = SoftwareC3dSampledTexture::from_retained_root(retained, states).unwrap();
        let shared = sampled.clone();

        assert_eq!(sampled.extent(), [2, 2]);
        assert_eq!(sampled.layout().reduction(), C3dSurfaceReduction::Quarter);
        assert_eq!(sampled.indices(), [0, 4, 32, 36]);
        assert_eq!(sampled.palette()[0].reserved, 0);
        assert_eq!(sampled.color_key_index(), None);
        assert_eq!(sampled.source().indices().len(), 64);
        assert!(Arc::ptr_eq(&sampled.indices, &shared.indices));
    }

    #[test]
    fn half_sampling_uses_every_second_source_coordinate() {
        let bitmap = IndexedBitmap {
            width: 64,
            height: 64,
            source_was_top_down: false,
            palette: Vec::new(),
            pixels: (0..4096).map(|value| value as u8).collect(),
            metadata: BitmapMetadata {
                reserved_1: 0,
                reserved_2: 0,
                x_pixels_per_meter: 0,
                y_pixels_per_meter: 0,
                important_colors: 0,
            },
        };
        let retained = SoftwareC3dRetainedSource::from_bitmap(
            RootMenuC3dSlot::Championship,
            &bitmap,
            [64, 64],
            None,
        );
        let states = RootMenuC3dMode2LoadStates::splat(C3dMode2LoadState {
            prepared_resource_count: 0,
            corner_source: C3dMode2CornerSource::OrdinaryDefaults,
            surface_layout: C3dSurfaceLayoutGates {
                half_if_large: true,
                ..C3dSurfaceLayoutGates::default()
            },
        });

        let sampled = SoftwareC3dSampledTexture::from_retained_root(retained, states).unwrap();

        assert_eq!(sampled.extent(), [32, 32]);
        assert_eq!(sampled.layout().reduction(), C3dSurfaceReduction::Half);
        assert_eq!(
            &sampled.indices()[..34],
            &[
                0, 2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 22, 24, 26, 28, 30, 32, 34, 36, 38, 40, 42,
                44, 46, 48, 50, 52, 54, 56, 58, 60, 62, 128, 130
            ]
        );
        assert_eq!(sampled.indices()[1023], 190);
    }

    #[test]
    fn unsupported_source_shapes_fail_before_sampling() {
        let bitmap = IndexedBitmap {
            width: 4,
            height: 8,
            source_was_top_down: false,
            palette: Vec::new(),
            pixels: vec![0; 32],
            metadata: BitmapMetadata {
                reserved_1: 0,
                reserved_2: 0,
                x_pixels_per_meter: 0,
                y_pixels_per_meter: 0,
                important_colors: 0,
            },
        };
        let default_state = C3dMode2LoadState {
            prepared_resource_count: 0,
            corner_source: C3dMode2CornerSource::OrdinaryDefaults,
            surface_layout: C3dSurfaceLayoutGates::default(),
        };
        let states = RootMenuC3dMode2LoadStates::splat(default_state);

        let padded = SoftwareC3dRetainedSource::from_bitmap(
            RootMenuC3dSlot::Championship,
            &bitmap,
            [4, 8],
            None,
        );
        assert_eq!(
            SoftwareC3dSampledTexture::from_retained_root(padded, states),
            Err(SoftwareC3dSamplingError::UnsupportedPadding {
                horizontal: 5,
                vertical: 0,
            })
        );

        let mismatched = SoftwareC3dRetainedSource::from_bitmap(
            RootMenuC3dSlot::Championship,
            &bitmap,
            [4, 0xffff],
            None,
        );
        assert_eq!(
            SoftwareC3dSampledTexture::from_retained_root(mismatched, states),
            Err(SoftwareC3dSamplingError::SourceExtentMismatch {
                normalized: [4, 8],
                raw_low16: [4, 0xffff],
            })
        );

        let mut unaligned_bitmap = bitmap.clone();
        unaligned_bitmap.width = 6;
        unaligned_bitmap.pixels = vec![0; 48];
        let unaligned = SoftwareC3dRetainedSource::from_bitmap(
            RootMenuC3dSlot::Championship,
            &unaligned_bitmap,
            [6, 8],
            None,
        );
        assert_eq!(
            SoftwareC3dSampledTexture::from_retained_root(unaligned, states),
            Err(SoftwareC3dSamplingError::UnalignedSourceWidth(6))
        );

        let mut top_down_bitmap = bitmap.clone();
        top_down_bitmap.source_was_top_down = true;
        let top_down = SoftwareC3dRetainedSource::from_bitmap(
            RootMenuC3dSlot::Championship,
            &top_down_bitmap,
            [4, 8],
            None,
        );
        assert_eq!(
            SoftwareC3dSampledTexture::from_retained_root(top_down, states),
            Err(SoftwareC3dSamplingError::UnsupportedTopDownSource)
        );

        let mut short_bitmap = bitmap;
        short_bitmap.pixels.pop();
        let short = SoftwareC3dRetainedSource::from_bitmap(
            RootMenuC3dSlot::Championship,
            &short_bitmap,
            [4, 8],
            None,
        );
        assert_eq!(
            SoftwareC3dSampledTexture::from_retained_root(short, states),
            Err(SoftwareC3dSamplingError::InvalidNormalizedIndexLength {
                expected: 32,
                actual: 31,
            })
        );
    }
}
