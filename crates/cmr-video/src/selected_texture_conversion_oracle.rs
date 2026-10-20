//! Source-owned materialization of selected logical texture storage.
//!
//! This is an implementation oracle, not a reconstruction of DirectDraw's
//! `Load` conversion. In particular, it makes no claim about legacy driver
//! dithering, intermediate storage, or byte layout. Canonical RGB is exposed
//! only as a sampling view over the already-materialized selected plane.

use std::fmt;

use crate::{
    c3d_software::SoftwareC3dSampledTexture,
    legacy_c3d_pixel_format::{
        KnownRgbLayout, LegacyNonAlphaLayout, PixelFormatError, RgbMaskLayout,
        SelectedNonAlphaTextureFormat,
    },
};

/// Explicit conversion rule implemented by this source-owned oracle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceOwnedTextureConversionOraclePolicy {
    /// Preserve selected palette planes exactly; for selected RGB masks, use
    /// nearest-integer pack/unpack with no claim of driver dithering.
    ExactPaletteOrNearestMaskNoDither,
}

/// Category retained beside storage produced by the source-owned oracle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceOwnedSelectedTextureCategory {
    Palette8,
    OpaqueRgb16(KnownRgbLayout),
}

/// Owned palette plane retained by the source-owned oracle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceOwnedPalette8TexturePlane {
    extent: [u16; 2],
    descriptor: crate::legacy_c3d_pixel_format::RawDdPixelFormat,
    policy: SourceOwnedTextureConversionOraclePolicy,
    indices: Box<[u8]>,
    palette: Box<[[u8; 3]]>,
}

impl SourceOwnedPalette8TexturePlane {
    #[must_use]
    pub const fn extent(&self) -> [u16; 2] {
        self.extent
    }

    #[must_use]
    pub const fn descriptor(&self) -> crate::legacy_c3d_pixel_format::RawDdPixelFormat {
        self.descriptor
    }

    #[must_use]
    pub fn indices(&self) -> &[u8] {
        &self.indices
    }

    #[must_use]
    pub fn palette(&self) -> &[[u8; 3]] {
        &self.palette
    }
}

/// Owned logical 16-bit words retained by the source-owned oracle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceOwnedOpaqueRgb16TexturePlane {
    extent: [u16; 2],
    descriptor: crate::legacy_c3d_pixel_format::RawDdPixelFormat,
    policy: SourceOwnedTextureConversionOraclePolicy,
    layout: KnownRgbLayout,
    mask_layout: RgbMaskLayout,
    words: Box<[u16]>,
}

impl SourceOwnedOpaqueRgb16TexturePlane {
    #[must_use]
    pub const fn extent(&self) -> [u16; 2] {
        self.extent
    }

    #[must_use]
    pub const fn descriptor(&self) -> crate::legacy_c3d_pixel_format::RawDdPixelFormat {
        self.descriptor
    }

    #[must_use]
    pub const fn layout(&self) -> KnownRgbLayout {
        self.layout
    }

    #[must_use]
    pub fn words(&self) -> &[u16] {
        &self.words
    }
}

/// Owned selected texture storage with retained descriptor/category provenance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceOwnedLogicalSelectedTexture {
    Palette8(SourceOwnedPalette8TexturePlane),
    OpaqueRgb16(SourceOwnedOpaqueRgb16TexturePlane),
}

impl SourceOwnedLogicalSelectedTexture {
    #[must_use]
    pub const fn extent(&self) -> [u16; 2] {
        match self {
            Self::Palette8(plane) => plane.extent(),
            Self::OpaqueRgb16(plane) => plane.extent(),
        }
    }

    #[must_use]
    pub const fn selected_format(&self) -> SelectedNonAlphaTextureFormat {
        match self {
            Self::Palette8(plane) => SelectedNonAlphaTextureFormat::Palette8(plane.descriptor()),
            Self::OpaqueRgb16(plane) => SelectedNonAlphaTextureFormat::OpaqueRgb16 {
                descriptor: plane.descriptor(),
                layout: plane.layout(),
            },
        }
    }

    #[must_use]
    pub const fn category(&self) -> SourceOwnedSelectedTextureCategory {
        match self {
            Self::Palette8(_) => SourceOwnedSelectedTextureCategory::Palette8,
            Self::OpaqueRgb16(plane) => {
                SourceOwnedSelectedTextureCategory::OpaqueRgb16(plane.layout())
            }
        }
    }

    /// The source-owned conversion policy that produced this storage.
    #[must_use]
    pub const fn policy(&self) -> SourceOwnedTextureConversionOraclePolicy {
        match self {
            Self::Palette8(plane) => plane.policy,
            Self::OpaqueRgb16(plane) => plane.policy,
        }
    }

    /// Decodes one already-materialized selected texel to canonical RGB.
    ///
    /// This accessor deliberately occurs after palette retention or RGB mask
    /// packing, so downstream filtering cannot accidentally sample the
    /// original indexed source.
    #[must_use]
    pub fn texel_rgb8_source_owned_oracle(&self, texel: usize) -> Option<[u8; 3]> {
        match self {
            Self::Palette8(plane) => plane
                .indices()
                .get(texel)
                .map(|&index| plane.palette()[usize::from(index)]),
            Self::OpaqueRgb16(plane) => plane
                .words()
                .get(texel)
                .map(|&word| plane.mask_layout.unpack_rgb8(u32::from(word))),
        }
    }

    /// Decodes the already-materialized selected plane to canonical RGB.
    #[must_use]
    pub fn canonical_rgb_texels_source_owned_oracle(&self) -> Vec<[u8; 3]> {
        let [width, height] = self.extent().map(usize::from);
        (0..width * height)
            .map(|texel| {
                self.texel_rgb8_source_owned_oracle(texel)
                    .expect("materialized plane length matches its extent")
            })
            .collect()
    }
}

/// Validation failure detected before the oracle returns an output texture.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceOwnedTextureConversionOracleError {
    InvalidDescriptorSize(u32),
    PaletteDescriptorCategoryMismatch,
    OpaqueRgbDescriptorCategoryMismatch,
    OpaqueRgbLayoutMismatch {
        selected: KnownRgbLayout,
        descriptor: KnownRgbLayout,
    },
    InvalidSelectedLayout(PixelFormatError),
    /// Defensive revalidation of the sampled-source invariant.
    InvalidSourceLength {
        expected: usize,
        actual: usize,
    },
    /// Defensive revalidation of the sampled-source invariant.
    SourcePaletteIndexOutOfBounds {
        texel: usize,
        index: u8,
        palette_len: usize,
    },
}

impl fmt::Display for SourceOwnedTextureConversionOracleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "source-owned selected-texture conversion failed: {self:?}"
        )
    }
}

impl std::error::Error for SourceOwnedTextureConversionOracleError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidSelectedLayout(source) => Some(source),
            _ => None,
        }
    }
}

/// Converts one sampled indexed source according to a validated selected
/// texture category.
///
/// Palette selection preserves indices and their RGB palette association.
/// Opaque RGB selection packs every expanded palette color into logical 16-bit
/// words with the selected mask layout. That deterministic quantization is
/// source-owned and intentionally does not model driver `Load` conversion or
/// dithering.
pub fn convert_selected_texture_source_owned_oracle(
    source: &SoftwareC3dSampledTexture,
    selected_format: SelectedNonAlphaTextureFormat,
    policy: SourceOwnedTextureConversionOraclePolicy,
) -> Result<SourceOwnedLogicalSelectedTexture, SourceOwnedTextureConversionOracleError> {
    let descriptor = selected_format.descriptor();
    if descriptor.size() != 32 {
        return Err(
            SourceOwnedTextureConversionOracleError::InvalidDescriptorSize(descriptor.size()),
        );
    }

    let quantize_layout = validate_selected_format(selected_format)?;
    match policy {
        SourceOwnedTextureConversionOraclePolicy::ExactPaletteOrNearestMaskNoDither => {}
    }
    let extent = source.extent();
    let expected = usize::from(extent[0]) * usize::from(extent[1]);
    if source.indices().len() != expected {
        return Err(
            SourceOwnedTextureConversionOracleError::InvalidSourceLength {
                expected,
                actual: source.indices().len(),
            },
        );
    }

    for (texel, &index) in source.indices().iter().enumerate() {
        if usize::from(index) >= source.palette().len() {
            return Err(
                SourceOwnedTextureConversionOracleError::SourcePaletteIndexOutOfBounds {
                    texel,
                    index,
                    palette_len: source.palette().len(),
                },
            );
        }
    }

    match (selected_format, quantize_layout) {
        (SelectedNonAlphaTextureFormat::Palette8(descriptor), None) => Ok(
            SourceOwnedLogicalSelectedTexture::Palette8(SourceOwnedPalette8TexturePlane {
                extent,
                descriptor,
                policy,
                indices: source.indices().into(),
                palette: source
                    .palette()
                    .iter()
                    .map(|entry| [entry.red, entry.green, entry.blue])
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            }),
        ),
        (SelectedNonAlphaTextureFormat::OpaqueRgb16 { descriptor, layout }, Some(mask_layout)) => {
            Ok(SourceOwnedLogicalSelectedTexture::OpaqueRgb16(
                SourceOwnedOpaqueRgb16TexturePlane {
                    extent,
                    descriptor,
                    policy,
                    layout,
                    mask_layout,
                    words: source
                        .indices()
                        .iter()
                        .map(|&index| {
                            let entry = source.palette()[usize::from(index)];
                            mask_layout.pack_rgb8([entry.red, entry.green, entry.blue]) as u16
                        })
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                },
            ))
        }
        _ => unreachable!("selected-format validation returned a mismatched plane"),
    }
}

fn validate_selected_format(
    selected: SelectedNonAlphaTextureFormat,
) -> Result<
    Option<crate::legacy_c3d_pixel_format::RgbMaskLayout>,
    SourceOwnedTextureConversionOracleError,
> {
    match selected {
        SelectedNonAlphaTextureFormat::Palette8(descriptor) => {
            if !descriptor.is_legacy_palette_candidate()
                || descriptor.has_rgb_flag()
                || descriptor.has_four_cc_flag()
            {
                return Err(
                    SourceOwnedTextureConversionOracleError::PaletteDescriptorCategoryMismatch,
                );
            }
            descriptor
                .validate_palette_8_layout()
                .map_err(SourceOwnedTextureConversionOracleError::InvalidSelectedLayout)?;
            Ok(None)
        }
        SelectedNonAlphaTextureFormat::OpaqueRgb16 {
            descriptor,
            layout: selected_layout,
        } => {
            if descriptor.has_palette_indexed_8_flag() || descriptor.has_four_cc_flag() {
                return Err(
                    SourceOwnedTextureConversionOracleError::OpaqueRgbDescriptorCategoryMismatch,
                );
            }
            let descriptor_layout = descriptor.legacy_opaque_rgb_candidate().ok_or(
                SourceOwnedTextureConversionOracleError::OpaqueRgbDescriptorCategoryMismatch,
            )?;
            if descriptor_layout != selected_layout {
                return Err(
                    SourceOwnedTextureConversionOracleError::OpaqueRgbLayoutMismatch {
                        selected: selected_layout,
                        descriptor: descriptor_layout,
                    },
                );
            }
            let mask_layout = match descriptor
                .non_alpha_layout()
                .map_err(SourceOwnedTextureConversionOracleError::InvalidSelectedLayout)?
            {
                LegacyNonAlphaLayout::Rgb(layout) => layout,
                LegacyNonAlphaLayout::Maskless8Bit => {
                    return Err(
                        SourceOwnedTextureConversionOracleError::OpaqueRgbDescriptorCategoryMismatch,
                    );
                }
            };
            Ok(Some(mask_layout))
        }
    }
}

#[cfg(test)]
mod tests {
    use cmr_core::{
        championship_setup::CanonicalCar,
        root_menu_c3d_resources::{
            C3dEntryMaterialPolicy, C3dLoadPolicy, C3dPreparedArenaBuilder,
            C3dResourceAllocationPolicy, C3dSourceIdentity, C3dSurfaceLayoutGates,
            FrontendC3dOwner,
        },
    };
    use cmr_formats::{
        bitmap::BitmapLimits,
        c3d::{C3D_FORMAT_VERSION, C3D_MAGIC, C3dLimits, parse_c3d},
    };

    use super::*;
    use crate::legacy_c3d_pixel_format::RawDdPixelFormat;

    const DDPF_PALETTE_INDEXED_8: u32 = 0x20;
    const DDPF_RGB: u32 = 0x40;

    fn palette_descriptor(flags: u32, storage_bits: u32, masks: [u32; 3]) -> RawDdPixelFormat {
        RawDdPixelFormat::from_dwords([32, flags, 0, storage_bits, masks[0], masks[1], masks[2], 0])
    }

    fn rgb_descriptor(layout: KnownRgbLayout, extra_flags: u32) -> RawDdPixelFormat {
        let masks = match layout {
            KnownRgbLayout::Rgb555 => [0x7c00, 0x03e0, 0x001f],
            KnownRgbLayout::Rgb565 => [0xf800, 0x07e0, 0x001f],
        };
        palette_descriptor(DDPF_RGB | extra_flags, 16, masks)
    }

    const POLICY: SourceOwnedTextureConversionOraclePolicy =
        SourceOwnedTextureConversionOraclePolicy::ExactPaletteOrNearestMaskNoDither;

    fn sampled_texture(
        palette: &[[u8; 3]],
        row_major_indices: [u8; 16],
    ) -> SoftwareC3dSampledTexture {
        const WIDTH: usize = 4;
        const HEIGHT: usize = 4;
        const PIXEL_OFFSET: usize = 14 + 40 + 256 * 4;
        let mut bitmap = vec![0; PIXEL_OFFSET + WIDTH * HEIGHT];
        let bitmap_len = bitmap.len() as u32;
        bitmap[0..2].copy_from_slice(b"BM");
        bitmap[2..6].copy_from_slice(&bitmap_len.to_le_bytes());
        bitmap[10..14].copy_from_slice(&(PIXEL_OFFSET as u32).to_le_bytes());
        bitmap[14..18].copy_from_slice(&40u32.to_le_bytes());
        bitmap[18..22].copy_from_slice(&(WIDTH as i32).to_le_bytes());
        bitmap[22..26].copy_from_slice(&(HEIGHT as i32).to_le_bytes());
        bitmap[26..28].copy_from_slice(&1u16.to_le_bytes());
        bitmap[28..30].copy_from_slice(&8u16.to_le_bytes());
        bitmap[34..38].copy_from_slice(&((WIDTH * HEIGHT) as u32).to_le_bytes());
        bitmap[46..50].copy_from_slice(&(palette.len() as u32).to_le_bytes());
        for (index, &[red, green, blue]) in palette.iter().enumerate() {
            let offset = 14 + 40 + index * 4;
            bitmap[offset..offset + 4].copy_from_slice(&[blue, green, red, 0]);
        }
        // Positive-height BMP rows are stored bottom-up.
        for stored_y in 0..HEIGHT {
            let source_y = HEIGHT - 1 - stored_y;
            let source = &row_major_indices[source_y * WIDTH..(source_y + 1) * WIDTH];
            let target = PIXEL_OFFSET + stored_y * WIDTH;
            bitmap[target..target + WIDTH].copy_from_slice(source);
        }

        let mut bytes = vec![0; 32 + 16 + bitmap.len()];
        bytes[0..4].copy_from_slice(&C3D_MAGIC);
        bytes[4..8].copy_from_slice(&C3D_FORMAT_VERSION.to_le_bytes());
        bytes[16..18].copy_from_slice(&1u16.to_le_bytes());
        for index in 0..3 {
            bytes[20 + index * 4..24 + index * 4].copy_from_slice(&32u32.to_le_bytes());
        }
        bytes[32..44].copy_from_slice(b"ORACLE-TEX!!");
        bytes[44..48].copy_from_slice(&(bitmap.len() as u32).to_le_bytes());
        bytes[48..].copy_from_slice(&bitmap);

        let file = parse_c3d(&bytes, C3dLimits::default()).unwrap();
        let mut builder = C3dPreparedArenaBuilder::new().unwrap();
        let object = builder
            .prepare_object(
                C3dSourceIdentity {
                    owner: FrontendC3dOwner::ChampionshipCar(CanonicalCar::Subaru),
                    path: "synthetic-source-owned-oracle.c3d".into(),
                },
                &file,
                C3dLoadPolicy {
                    mode: 8,
                    caller_option: 1,
                    caller_flags: 0x18,
                    surface_constructor_flags: 0x15,
                    entry_resource_count: 0,
                    allocation: C3dResourceAllocationPolicy::AppendEveryResource,
                    surface_layout: C3dSurfaceLayoutGates::default(),
                    material: C3dEntryMaterialPolicy::FixedUniformWord(0x9696_9696),
                },
                BitmapLimits::default(),
            )
            .unwrap();
        let id = object.ordinal_resources()[0];
        let arena = builder.finish();
        SoftwareC3dSampledTexture::from_prepared_frontend(arena.resource(id).unwrap()).unwrap()
    }

    #[test]
    fn palette_oracle_retains_exact_plane_and_complete_provenance() {
        let source = sampled_texture(
            &[[1, 2, 3], [40, 50, 60]],
            [0, 1, 0, 1, 1, 0, 1, 0, 0, 0, 1, 1, 1, 1, 0, 0],
        );
        let descriptor = palette_descriptor(DDPF_PALETTE_INDEXED_8 | 0x8000_0000, 8, [0; 3]);
        let selected = SelectedNonAlphaTextureFormat::Palette8(descriptor);

        let converted =
            convert_selected_texture_source_owned_oracle(&source, selected, POLICY).unwrap();
        drop(source);

        assert_eq!(converted.extent(), [4, 4]);
        assert_eq!(converted.policy(), POLICY);
        assert_eq!(converted.selected_format(), selected);
        assert_eq!(
            converted.selected_format().descriptor().dwords(),
            descriptor.dwords()
        );
        assert_eq!(
            converted.category(),
            SourceOwnedSelectedTextureCategory::Palette8
        );
        assert_eq!(
            converted.canonical_rgb_texels_source_owned_oracle()[..4],
            [[1, 2, 3], [40, 50, 60], [1, 2, 3], [40, 50, 60]]
        );
        let SourceOwnedLogicalSelectedTexture::Palette8(plane) = converted else {
            panic!("palette selection must retain a palette plane")
        };
        assert_eq!(plane.indices()[..4], [0, 1, 0, 1]);
        assert_eq!(plane.palette()[..2], [[1, 2, 3], [40, 50, 60]]);
    }

    #[test]
    fn opaque_555_and_565_oracles_apply_distinct_source_owned_quantization() {
        let source = sampled_texture(&[[0, 129, 0]], [0; 16]);
        let selected_555 = SelectedNonAlphaTextureFormat::OpaqueRgb16 {
            descriptor: rgb_descriptor(KnownRgbLayout::Rgb555, 0),
            layout: KnownRgbLayout::Rgb555,
        };
        let selected_565 = SelectedNonAlphaTextureFormat::OpaqueRgb16 {
            descriptor: rgb_descriptor(KnownRgbLayout::Rgb565, 0x4000_0000),
            layout: KnownRgbLayout::Rgb565,
        };

        let rgb_555 =
            convert_selected_texture_source_owned_oracle(&source, selected_555, POLICY).unwrap();
        let rgb_565 =
            convert_selected_texture_source_owned_oracle(&source, selected_565, POLICY).unwrap();

        assert_eq!(
            rgb_555.canonical_rgb_texels_source_owned_oracle()[0],
            [0, 132, 0]
        );
        assert_eq!(
            rgb_565.canonical_rgb_texels_source_owned_oracle()[0],
            [0, 130, 0]
        );
        assert_eq!(
            rgb_555.category(),
            SourceOwnedSelectedTextureCategory::OpaqueRgb16(KnownRgbLayout::Rgb555)
        );
        assert_eq!(
            rgb_565.category(),
            SourceOwnedSelectedTextureCategory::OpaqueRgb16(KnownRgbLayout::Rgb565)
        );
        assert_eq!(rgb_565.selected_format(), selected_565);
        let SourceOwnedLogicalSelectedTexture::OpaqueRgb16(plane_555) = rgb_555 else {
            panic!("RGB selection must materialize packed words")
        };
        let SourceOwnedLogicalSelectedTexture::OpaqueRgb16(plane_565) = rgb_565 else {
            panic!("RGB selection must materialize packed words")
        };
        assert_ne!(plane_555.words()[0], plane_565.words()[0]);
    }

    #[test]
    fn invalid_selected_palette_shape_returns_no_partial_output() {
        let source = sampled_texture(&[[9, 8, 7]], [0; 16]);
        let selected = SelectedNonAlphaTextureFormat::Palette8(palette_descriptor(
            DDPF_PALETTE_INDEXED_8,
            16,
            [0; 3],
        ));

        let result = convert_selected_texture_source_owned_oracle(&source, selected, POLICY);

        assert_eq!(
            result,
            Err(
                SourceOwnedTextureConversionOracleError::InvalidSelectedLayout(
                    PixelFormatError::InvalidPaletteStorageBits(16),
                )
            )
        );
        assert_eq!(source.indices(), [0; 16]);
        assert_eq!(
            [
                source.palette()[0].red,
                source.palette()[0].green,
                source.palette()[0].blue
            ],
            [9, 8, 7]
        );
    }

    #[test]
    fn invalid_descriptor_size_is_rejected_before_conversion() {
        let valid_source = sampled_texture(&[[9, 8, 7]], [0; 16]);
        let bad_size = SelectedNonAlphaTextureFormat::Palette8(RawDdPixelFormat::from_dwords([
            31,
            DDPF_PALETTE_INDEXED_8,
            0,
            8,
            0,
            0,
            0,
            0,
        ]));
        assert_eq!(
            convert_selected_texture_source_owned_oracle(&valid_source, bad_size, POLICY),
            Err(SourceOwnedTextureConversionOracleError::InvalidDescriptorSize(31))
        );
    }

    #[test]
    fn mismatched_opaque_descriptor_category_and_layout_are_rejected() {
        let source = sampled_texture(&[[9, 8, 7]], [0; 16]);
        let category_mismatch = SelectedNonAlphaTextureFormat::OpaqueRgb16 {
            descriptor: palette_descriptor(
                DDPF_RGB | DDPF_PALETTE_INDEXED_8,
                16,
                [0x7c00, 0x03e0, 0x001f],
            ),
            layout: KnownRgbLayout::Rgb555,
        };
        let layout_mismatch = SelectedNonAlphaTextureFormat::OpaqueRgb16 {
            descriptor: rgb_descriptor(KnownRgbLayout::Rgb565, 0),
            layout: KnownRgbLayout::Rgb555,
        };

        assert_eq!(
            convert_selected_texture_source_owned_oracle(&source, category_mismatch, POLICY),
            Err(SourceOwnedTextureConversionOracleError::OpaqueRgbDescriptorCategoryMismatch)
        );
        assert_eq!(
            convert_selected_texture_source_owned_oracle(&source, layout_mismatch, POLICY),
            Err(
                SourceOwnedTextureConversionOracleError::OpaqueRgbLayoutMismatch {
                    selected: KnownRgbLayout::Rgb555,
                    descriptor: KnownRgbLayout::Rgb565,
                }
            )
        );
    }
}
