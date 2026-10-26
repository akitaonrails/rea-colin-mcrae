//! Immutable retained C3D source textures for the software renderer.
//!
//! This retains immutable normalized sources and point-samples the statically
//! proven aligned, zero-padding root subset. The bounded software scene backend
//! consumes these bindings; alternate palette/material ownership and
//! unified/split upload topology remain later boundaries.

use std::{fmt, sync::Arc};

use cmr_core::frontend_c3d_resources::{FrontendC3dPrepared, bind_frontend_c3d_resources};
use cmr_core::root_menu_c3d_assets::RootMenuC3dSlot;
use cmr_core::root_menu_c3d_execute::{
    C3dBindingBuildError, C3dBindingCatalog, RootMenuC3dBindingBuildError,
    RootMenuC3dBindingCatalog, bind_root_menu_c3d_resources,
};
use cmr_core::root_menu_c3d_resources::{
    C3dPreparedResource, C3dSurfaceLayoutGates, FrontendC3dOwner,
};
use cmr_core::root_menu_c3d_runtime::{
    C3dSurfaceLayoutError, ResolvedC3dSurfaceLayout, RootMenuC3dMode2LoadStates,
    resolve_c3d_surface_layout,
};
use cmr_core::root_menu_c3d_topology::RootMenuC3dTopology;
use cmr_formats::bitmap::{
    BitmapError, BitmapLimits, BitmapPaletteEntry, IndexedBitmap, parse_indexed_bitmap,
};
use cmr_formats::c3d::{C3dEngineBitmap, C3dPalettedBitmap, C3dTruecolorBitmap};

#[derive(Debug, Eq, PartialEq)]
struct RetainedC3dSourceInner {
    owner: FrontendC3dOwner,
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
/// The bounded software scene backend consumes this directly. Hardware texture
/// ownership and unified/split upload topology remain later policy.
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
    NonRootOwner(FrontendC3dOwner),
    /// A paletted engine texture carried no inline palette (its palette comes
    /// from a separate device palette resource, not yet recovered).
    EnginePaletteMissing,
    /// The engine texture is direct-color or otherwise cannot be represented by
    /// the indexed sampler without inventing a palette.
    UnsupportedEngineTexture,
}

impl SoftwareC3dRetainedSource {
    #[must_use]
    pub fn from_prepared(resource: &C3dPreparedResource) -> Self {
        Self::from_resource(resource)
    }

    fn from_resource(resource: &C3dPreparedResource) -> Self {
        Self::from_bitmap(
            resource.origin().source().owner,
            resource.bitmap(),
            resource.source_dib_extent_low16(),
            resource.surface_color_key_index(),
        )
    }

    /// Root slot that established the resource, or `None` for a car-owned origin.
    #[must_use]
    pub fn first_slot(&self) -> Option<RootMenuC3dSlot> {
        match self.0.owner {
            FrontendC3dOwner::RootMenu(slot) => Some(slot),
            FrontendC3dOwner::ChampionshipCar(_) | FrontendC3dOwner::Stage => None,
        }
    }

    #[must_use]
    pub fn owner(&self) -> FrontendC3dOwner {
        self.0.owner
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
        owner: FrontendC3dOwner,
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
            owner,
            width: bitmap.width,
            height: bitmap.height,
            source_was_top_down: bitmap.source_was_top_down,
            source_dib_extent_low16,
            indices: bitmap.pixels.clone().into_boxed_slice(),
            palette,
            color_key_index,
        }))
    }

    /// Builds a retained source from an already-decoded indexed engine texture
    /// (one palette index per pixel, top-to-bottom), using the image extent as
    /// the raw DIB extent and installing no color key.
    fn from_engine_indexed(
        owner: FrontendC3dOwner,
        width: u32,
        height: u32,
        indices: Vec<u8>,
        palette: Vec<BitmapPaletteEntry>,
    ) -> Self {
        Self(Arc::new(RetainedC3dSourceInner {
            owner,
            width,
            height,
            source_was_top_down: false,
            source_dib_extent_low16: [width as u16, height as u16],
            indices: indices.into_boxed_slice(),
            palette: palette.into_boxed_slice(),
            color_key_index: None,
        }))
    }
}

impl SoftwareC3dSampledTexture {
    /// Builds a sampled texture using the policy that first allocated this
    /// frontend-session resource.
    pub fn from_prepared_frontend(
        resource: &C3dPreparedResource,
    ) -> Result<Self, SoftwareC3dSamplingError> {
        let source = SoftwareC3dRetainedSource::from_resource(resource);
        Self::from_retained(
            source,
            resource.origin().preparation_policy().surface_layout,
        )
    }

    /// Builds a sampled texture from a raw `BM` group-2 blob, mirroring the
    /// frontend prepare path (no color key, default surface gates) without the
    /// prepared-resource arena, so a caller decoding group-2 blobs one at a time
    /// gets the same result the arena-backed [`Self::from_prepared_frontend`]
    /// produces for a stage resource.
    pub fn from_bm_blob(
        bytes: &[u8],
        owner: FrontendC3dOwner,
        bitmap_limits: BitmapLimits,
    ) -> Result<Self, SoftwareC3dBitmapError> {
        let bitmap =
            parse_indexed_bitmap(bytes, bitmap_limits).map_err(SoftwareC3dBitmapError::Decode)?;
        let source_dib_extent_low16 = [
            u16::from_le_bytes([bytes[18], bytes[19]]),
            u16::from_le_bytes([bytes[22], bytes[23]]),
        ];
        let source =
            SoftwareC3dRetainedSource::from_bitmap(owner, &bitmap, source_dib_extent_low16, None);
        Self::from_retained(source, C3dSurfaceLayoutGates::default())
            .map_err(SoftwareC3dBitmapError::Sampling)
    }

    /// Builds a sampled texture from a parsed C3D engine inline texture blob
    /// (the real stage/car group-2 material format).
    ///
    /// Paletted and indexed truecolor layouts are decoded here. Palette-less
    /// sources need a separate device palette; direct 24-bit pixels are not
    /// quantized into an indexed texture.
    pub fn from_engine_bitmap(
        bitmap: &C3dEngineBitmap<'_>,
        owner: FrontendC3dOwner,
    ) -> Result<Self, SoftwareC3dSamplingError> {
        match bitmap {
            C3dEngineBitmap::Paletted(paletted) => Self::from_engine_paletted(paletted, owner),
            C3dEngineBitmap::Truecolor(truecolor) => Self::from_engine_truecolor(truecolor, owner),
        }
    }

    fn from_engine_truecolor(
        bitmap: &C3dTruecolorBitmap<'_>,
        owner: FrontendC3dOwner,
    ) -> Result<Self, SoftwareC3dSamplingError> {
        if !bitmap.is_indexed() {
            return Err(SoftwareC3dSamplingError::UnsupportedEngineTexture);
        }
        let palette = (0..bitmap.palette_entry_count())
            .map(|index| {
                let [red, green, blue] = bitmap
                    .inline_palette_entry(index)
                    .ok_or(SoftwareC3dSamplingError::EnginePaletteMissing)?;
                Ok(BitmapPaletteEntry {
                    red,
                    green,
                    blue,
                    reserved: 0,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if palette.is_empty() {
            return Err(SoftwareC3dSamplingError::EnginePaletteMissing);
        }

        let height = usize::from(bitmap.height());
        let width = bitmap.padded_width();
        let row_bytes = if bitmap.bits_per_pixel() == 4 {
            width / 2
        } else {
            width
        };
        let required = row_bytes * height;
        let source = bitmap.pixel_data();
        if source.len() < required {
            return Err(SoftwareC3dSamplingError::InvalidNormalizedIndexLength {
                expected: required,
                actual: source.len(),
            });
        }
        let mut pixels = Vec::with_capacity(width * height);
        for row in source[..required].chunks_exact(row_bytes) {
            if bitmap.bits_per_pixel() == 4 {
                for &byte in row {
                    pixels.push(byte & 0x0f);
                    pixels.push(byte >> 4);
                }
            } else {
                pixels.extend_from_slice(row);
            }
        }

        let source = SoftwareC3dRetainedSource::from_engine_indexed(
            owner,
            width as u32,
            height as u32,
            pixels,
            palette,
        );
        Self::from_retained_with_alignment(source, C3dSurfaceLayoutGates::default(), false)
    }

    /// Decodes a paletted engine texture into an indexed sampled texture, scaling
    /// its inline `X1R5G5B5` palette to RGB and unpacking the 16-colour
    /// two-pixels-per-byte rows (low nibble first).
    fn from_engine_paletted(
        bitmap: &C3dPalettedBitmap<'_>,
        owner: FrontendC3dOwner,
    ) -> Result<Self, SoftwareC3dSamplingError> {
        let width = bitmap.pixel_width();
        let height = usize::from(bitmap.pixel_height());

        let palette = (0..bitmap.palette_entry_count())
            .map(|index| {
                let [red, green, blue] = bitmap
                    .inline_palette_entry(index)
                    .ok_or(SoftwareC3dSamplingError::EnginePaletteMissing)?;
                Ok(BitmapPaletteEntry {
                    red,
                    green,
                    blue,
                    reserved: 0,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let pixels = if bitmap.is_sixteen_colour() {
            let mut expanded = Vec::with_capacity(width * height);
            for &byte in bitmap.index_data() {
                expanded.push(byte & 0x0f);
                expanded.push(byte >> 4);
            }
            expanded
        } else {
            bitmap.index_data().to_vec()
        };

        let expected = width * height;
        if pixels.len() != expected {
            return Err(SoftwareC3dSamplingError::InvalidNormalizedIndexLength {
                expected,
                actual: pixels.len(),
            });
        }

        let source = SoftwareC3dRetainedSource::from_engine_indexed(
            owner,
            width as u32,
            height as u32,
            pixels,
            palette,
        );
        Self::from_retained_with_alignment(source, C3dSurfaceLayoutGates::default(), false)
    }

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
        let slot = source
            .first_slot()
            .ok_or(SoftwareC3dSamplingError::NonRootOwner(source.owner()))?;
        let gates = load_states.get(slot).surface_layout;
        Self::from_retained(source, gates)
    }

    fn from_retained(
        source: SoftwareC3dRetainedSource,
        gates: C3dSurfaceLayoutGates,
    ) -> Result<Self, SoftwareC3dSamplingError> {
        Self::from_retained_with_alignment(source, gates, true)
    }

    fn from_retained_with_alignment(
        source: SoftwareC3dRetainedSource,
        gates: C3dSurfaceLayoutGates,
        require_aligned_width: bool,
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
        if require_aligned_width && raw[0] & 3 != 0 {
            return Err(SoftwareC3dSamplingError::UnalignedSourceWidth(raw[0]));
        }
        let expected_source_len = usize::from(raw[0]) * usize::from(raw[1]);
        if source.indices().len() != expected_source_len {
            return Err(SoftwareC3dSamplingError::InvalidNormalizedIndexLength {
                expected: expected_source_len,
                actual: source.indices().len(),
            });
        }
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

/// Builds one sampled software texture for every resource in a prepared
/// frontend-session arena, including the championship-car prefix.
///
/// Surface layout comes from the allocating resource origin. This matters for
/// a root selector that aliases an earlier car resource: the shared resource is
/// still created once under its original mode-8 allocation policy.
pub fn prepare_software_frontend_c3d_bindings(
    prepared: &FrontendC3dPrepared,
) -> Result<
    C3dBindingCatalog<SoftwareC3dSampledTexture>,
    C3dBindingBuildError<SoftwareC3dSamplingError>,
> {
    bind_frontend_c3d_resources(prepared, |_, resource| {
        SoftwareC3dSampledTexture::from_prepared_frontend(resource)
    })
}

impl fmt::Display for SoftwareC3dSamplingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "unsupported C3D sampling input: {self:?}")
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

/// Why [`SoftwareC3dSampledTexture::from_bm_blob`] rejected a `BM` group-2 blob.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SoftwareC3dBitmapError {
    /// The `BM` payload failed to parse.
    Decode(BitmapError),
    /// The parsed bitmap was rejected by the software sampler.
    Sampling(SoftwareC3dSamplingError),
}

impl fmt::Display for SoftwareC3dBitmapError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Decode(source) => write!(formatter, "cannot decode BM group-2 blob: {source}"),
            Self::Sampling(source) => write!(formatter, "cannot sample BM group-2 blob: {source}"),
        }
    }
}

impl std::error::Error for SoftwareC3dBitmapError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Decode(source) => Some(source),
            Self::Sampling(source) => Some(source),
        }
    }
}

#[cfg(test)]
mod tests {
    use cmr_core::root_menu_c3d_runtime::{
        C3dMode2CornerSource, C3dMode2LoadState, C3dSurfaceLayoutGates, C3dSurfaceReduction,
    };
    use cmr_core::{
        championship_setup::CanonicalCar,
        root_menu_c3d_resources::{
            C3dEntryMaterialPolicy, C3dLoadPolicy, C3dPreparedArenaBuilder,
            C3dResourceAllocationPolicy, C3dSourceIdentity,
        },
    };
    use cmr_formats::{
        bitmap::{BitmapLimits, BitmapMetadata},
        c3d::{C3D_FORMAT_VERSION, C3D_MAGIC, C3dLimits, parse_c3d},
    };

    use super::*;

    #[test]
    fn engine_paletted_blob_samples_into_an_indexed_texture() {
        use cmr_formats::c3d::{C3D_ENGINE_PALETTED_TAG, C3dEngineBitmap, parse_c3d_engine_bitmap};

        // 16-colour (mode & 3 == 0) with an inline palette (mode & 8).
        let palette_entries = 16usize;
        let descriptor = 0x14 + palette_entries * 2; // palette ends here
        let data_start = descriptor + 0x0c;
        // stored_width 1 -> pixel width 4 (4 * stored_width); stored_height 2.
        let (stored_width, stored_height) = (1u16, 2u16);
        let row_bytes = 2 * stored_width as usize;
        let mut blob = vec![0u8; data_start + row_bytes * stored_height as usize];
        blob[0] = C3D_ENGINE_PALETTED_TAG;
        blob[0x04] = 0x08; // 16-colour + inline palette
        // Palette entry 1 = red, entry 2 = green (X1R5G5B5), 2 bytes per entry.
        blob[0x16..0x18].copy_from_slice(&0x001fu16.to_le_bytes());
        blob[0x18..0x1a].copy_from_slice(&0x03e0u16.to_le_bytes());
        blob[descriptor + 0x08..descriptor + 0x0a].copy_from_slice(&stored_width.to_le_bytes());
        blob[descriptor + 0x0a..descriptor + 0x0c].copy_from_slice(&stored_height.to_le_bytes());
        // Four bytes (2 per row), each two pixels low nibble first:
        // row0 = [1,2,3,4], row1 = [5,6,7,8].
        blob[data_start] = 0x21;
        blob[data_start + 1] = 0x43;
        blob[data_start + 2] = 0x65;
        blob[data_start + 3] = 0x87;

        let C3dEngineBitmap::Paletted(paletted) = parse_c3d_engine_bitmap(&blob).unwrap() else {
            panic!("tag 0x10 must parse as paletted");
        };
        assert_eq!(paletted.pixel_width(), 4);
        assert_eq!(paletted.pixel_height(), 2);

        let texture = SoftwareC3dSampledTexture::from_engine_bitmap(
            &C3dEngineBitmap::Paletted(paletted),
            FrontendC3dOwner::Stage,
        )
        .unwrap();
        // Default gates apply no reduction, so the sampled extent is the source.
        assert_eq!(texture.extent(), [4, 2]);
        // 16-colour rows unpack low nibble first.
        assert_eq!(texture.indices(), [1, 2, 3, 4, 5, 6, 7, 8]);
        // The inline X1R5G5B5 palette scaled each 5-bit channel to 8 bits.
        assert_eq!(texture.palette().len(), 16);
        assert_eq!(
            texture.palette()[1],
            BitmapPaletteEntry {
                red: 248,
                green: 0,
                blue: 0,
                reserved: 0,
            }
        );
        assert_eq!(
            texture.palette()[2],
            BitmapPaletteEntry {
                red: 0,
                green: 248,
                blue: 0,
                reserved: 0,
            }
        );
    }

    fn truecolor_indexed_blob(bits: u16, width: u16, height: u16, pixels: &[u8]) -> Vec<u8> {
        let pixel_offset = 0x80usize;
        let mut blob = vec![0u8; pixel_offset + pixels.len()];
        blob[0] = 5;
        blob[0x0a..0x0e].copy_from_slice(&(pixel_offset as u32).to_le_bytes());
        blob[0x12..0x14].copy_from_slice(&width.to_le_bytes());
        blob[0x16..0x18].copy_from_slice(&height.to_le_bytes());
        blob[0x1c..0x1e].copy_from_slice(&bits.to_le_bytes());
        blob[0x2e..0x32].copy_from_slice(&2u32.to_le_bytes());
        blob[0x3a..0x3e].copy_from_slice(&[3, 2, 1, 0]);
        blob[pixel_offset..].copy_from_slice(pixels);
        blob
    }

    #[test]
    fn truecolor_four_bit_rows_unpack_with_four_pixel_alignment() {
        use cmr_formats::c3d::parse_c3d_engine_bitmap;
        let blob = truecolor_indexed_blob(4, 3, 2, &[0x21, 0x43, 0x65, 0x87]);
        let bitmap = parse_c3d_engine_bitmap(&blob).unwrap();
        let texture =
            SoftwareC3dSampledTexture::from_engine_bitmap(&bitmap, FrontendC3dOwner::Stage)
                .unwrap();
        assert_eq!(texture.extent(), [4, 2]);
        assert_eq!(texture.indices(), [1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(texture.palette()[1].red, 1);
        assert_eq!(texture.palette()[1].green, 2);
        assert_eq!(texture.palette()[1].blue, 3);
    }

    #[test]
    fn truecolor_eight_bit_rows_skip_four_pixel_padding() {
        use cmr_formats::c3d::parse_c3d_engine_bitmap;
        let blob = truecolor_indexed_blob(8, 3, 2, &[1, 1, 1, 0, 1, 1, 1, 0]);
        let bitmap = parse_c3d_engine_bitmap(&blob).unwrap();
        let texture =
            SoftwareC3dSampledTexture::from_engine_bitmap(&bitmap, FrontendC3dOwner::Stage)
                .unwrap();
        assert_eq!(texture.extent(), [4, 2]);
        assert_eq!(texture.indices(), [1, 1, 1, 0, 1, 1, 1, 0]);
    }

    #[test]
    fn engine_truecolor_and_palette_less_paletted_are_rejected_closed() {
        use cmr_formats::c3d::{C3D_ENGINE_PALETTED_TAG, C3dEngineBitmap, parse_c3d_engine_bitmap};

        // A truecolor blob (any non-0x10 tag) is not yet decodable here.
        let mut truecolor = vec![0u8; 0x40];
        truecolor[0] = 0x05;
        truecolor[0x1c] = 0x18; // 24 bpp
        let bitmap = parse_c3d_engine_bitmap(&truecolor).unwrap();
        assert_eq!(
            SoftwareC3dSampledTexture::from_engine_bitmap(&bitmap, FrontendC3dOwner::Stage),
            Err(SoftwareC3dSamplingError::UnsupportedEngineTexture)
        );

        // A paletted blob without an inline palette (mode & 8 clear) has no
        // self-contained palette to sample.
        let descriptor = 0x08;
        let data_start = descriptor + 0x0c;
        // 256-colour, stored width 2 -> 4 bytes per row, one row.
        let mut paletted = vec![0u8; data_start + 4];
        paletted[0] = C3D_ENGINE_PALETTED_TAG;
        paletted[0x04] = 0x01; // 256-colour, no inline palette
        paletted[descriptor + 0x08..descriptor + 0x0a].copy_from_slice(&2u16.to_le_bytes());
        paletted[descriptor + 0x0a..descriptor + 0x0c].copy_from_slice(&1u16.to_le_bytes());
        let C3dEngineBitmap::Paletted(no_palette) = parse_c3d_engine_bitmap(&paletted).unwrap()
        else {
            panic!("tag 0x10 parses as paletted");
        };
        assert_eq!(
            SoftwareC3dSampledTexture::from_engine_bitmap(
                &C3dEngineBitmap::Paletted(no_palette),
                FrontendC3dOwner::Stage,
            ),
            Err(SoftwareC3dSamplingError::EnginePaletteMissing)
        );
    }

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
            row_stride: 4,
            stored_pixels: vec![0, 1, 0, 0, 1, 0, 0, 0],
            metadata: BitmapMetadata {
                reserved_1: 7,
                reserved_2: 8,
                x_pixels_per_meter: 9,
                y_pixels_per_meter: 10,
                important_colors: 2,
            },
        };

        let retained = SoftwareC3dRetainedSource::from_bitmap(
            FrontendC3dOwner::RootMenu(RootMenuC3dSlot::Championship),
            &bitmap,
            [0xfffe, 2],
            None,
        );
        let shared = retained.clone();
        drop(bitmap);

        assert_eq!(retained.first_slot(), Some(RootMenuC3dSlot::Championship));
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
            row_stride: 8,
            stored_pixels: (0..64).collect(),
            metadata: BitmapMetadata {
                reserved_1: 0,
                reserved_2: 0,
                x_pixels_per_meter: 0,
                y_pixels_per_meter: 0,
                important_colors: 0,
            },
        };
        let retained = SoftwareC3dRetainedSource::from_bitmap(
            FrontendC3dOwner::RootMenu(RootMenuC3dSlot::Championship),
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
    fn frontend_sampling_ignores_different_alias_consumer_gates_and_uses_origin_layout() {
        let bitmap = indexed_bmp_8x8();
        let mut bytes = vec![0; 32 + 16 + bitmap.len()];
        bytes[0..4].copy_from_slice(&C3D_MAGIC);
        bytes[4..8].copy_from_slice(&C3D_FORMAT_VERSION.to_le_bytes());
        bytes[16..18].copy_from_slice(&1u16.to_le_bytes());
        for index in 0..3 {
            bytes[20 + index * 4..24 + index * 4].copy_from_slice(&32u32.to_le_bytes());
        }
        bytes[32..44].copy_from_slice(b"CAR-TEXTURE!");
        bytes[44..48].copy_from_slice(&(bitmap.len() as u32).to_le_bytes());
        bytes[48..].copy_from_slice(&bitmap);
        let file = parse_c3d(&bytes, C3dLimits::default()).unwrap();
        let mut builder = C3dPreparedArenaBuilder::new().unwrap();
        let object = builder
            .prepare_object(
                C3dSourceIdentity {
                    owner: FrontendC3dOwner::ChampionshipCar(CanonicalCar::Subaru),
                    path: "synthetic-car.c3d".into(),
                },
                &file,
                C3dLoadPolicy {
                    mode: 8,
                    caller_option: 1,
                    caller_flags: 0x18,
                    surface_constructor_flags: 0x15,
                    entry_resource_count: 0,
                    allocation: C3dResourceAllocationPolicy::AppendEveryResource,
                    surface_layout: C3dSurfaceLayoutGates {
                        force_quarter: true,
                        ..C3dSurfaceLayoutGates::default()
                    },
                    material: C3dEntryMaterialPolicy::FixedUniformWord(0x9696_9696),
                },
                BitmapLimits::default(),
            )
            .unwrap();
        let id = object.ordinal_resources()[0];
        let alias = builder
            .prepare_object(
                C3dSourceIdentity {
                    owner: FrontendC3dOwner::RootMenu(RootMenuC3dSlot::Championship),
                    path: "synthetic-root.c3d".into(),
                },
                &file,
                C3dLoadPolicy {
                    mode: 2,
                    caller_option: 0,
                    caller_flags: 0x11,
                    surface_constructor_flags: 0x35,
                    entry_resource_count: 1,
                    allocation: C3dResourceAllocationPolicy::InternExact,
                    surface_layout: C3dSurfaceLayoutGates::default(),
                    material: C3dEntryMaterialPolicy::Mode2Ordinary,
                },
                BitmapLimits::default(),
            )
            .unwrap();
        assert_eq!(alias.ordinal_resources(), [id]);
        assert_ne!(
            alias.policy().surface_layout,
            object.policy().surface_layout
        );
        let arena = builder.finish();
        let resource = arena.resource(id).unwrap();
        let generic_retained = SoftwareC3dRetainedSource::from_prepared(resource);
        assert_eq!(generic_retained.first_slot(), None);
        assert_eq!(generic_retained.owner(), resource.origin().source().owner);
        let sampled = SoftwareC3dSampledTexture::from_prepared_frontend(resource).unwrap();

        assert_eq!(
            sampled.source().owner(),
            FrontendC3dOwner::ChampionshipCar(CanonicalCar::Subaru)
        );
        assert_eq!(sampled.extent(), [2, 2]);
        assert_eq!(sampled.layout().reduction(), C3dSurfaceReduction::Quarter);
        assert_eq!(sampled.indices(), [56, 60, 24, 28]);
        assert_eq!(
            resolve_c3d_surface_layout([8, 8], alias.policy().surface_layout)
                .unwrap()
                .extent_bits(),
            [8, 8]
        );
    }

    #[test]
    fn half_sampling_uses_every_second_source_coordinate() {
        let bitmap = IndexedBitmap {
            width: 64,
            height: 64,
            source_was_top_down: false,
            palette: Vec::new(),
            pixels: (0..4096).map(|value| value as u8).collect(),
            row_stride: 64,
            stored_pixels: (0..4096).map(|value| value as u8).collect(),
            metadata: BitmapMetadata {
                reserved_1: 0,
                reserved_2: 0,
                x_pixels_per_meter: 0,
                y_pixels_per_meter: 0,
                important_colors: 0,
            },
        };
        let retained = SoftwareC3dRetainedSource::from_bitmap(
            FrontendC3dOwner::RootMenu(RootMenuC3dSlot::Championship),
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
            row_stride: 4,
            stored_pixels: vec![0; 32],
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
            FrontendC3dOwner::RootMenu(RootMenuC3dSlot::Championship),
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
            FrontendC3dOwner::RootMenu(RootMenuC3dSlot::Championship),
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
            FrontendC3dOwner::RootMenu(RootMenuC3dSlot::Championship),
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
            FrontendC3dOwner::RootMenu(RootMenuC3dSlot::Championship),
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
            FrontendC3dOwner::RootMenu(RootMenuC3dSlot::Championship),
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

    fn indexed_bmp_8x8() -> Vec<u8> {
        const PIXEL_OFFSET: usize = 14 + 40 + 256 * 4;
        const PIXEL_LEN: usize = 8 * 8;
        let mut bytes = vec![0; PIXEL_OFFSET + PIXEL_LEN];
        let file_len = bytes.len() as u32;
        bytes[0..2].copy_from_slice(b"BM");
        bytes[2..6].copy_from_slice(&file_len.to_le_bytes());
        bytes[10..14].copy_from_slice(&(PIXEL_OFFSET as u32).to_le_bytes());
        bytes[14..18].copy_from_slice(&40u32.to_le_bytes());
        bytes[18..22].copy_from_slice(&8i32.to_le_bytes());
        bytes[22..26].copy_from_slice(&8i32.to_le_bytes());
        bytes[26..28].copy_from_slice(&1u16.to_le_bytes());
        bytes[28..30].copy_from_slice(&8u16.to_le_bytes());
        bytes[34..38].copy_from_slice(&(PIXEL_LEN as u32).to_le_bytes());
        bytes[46..50].copy_from_slice(&256u32.to_le_bytes());
        bytes[PIXEL_OFFSET..].copy_from_slice(&(0..64).collect::<Vec<_>>());
        bytes
    }
}
