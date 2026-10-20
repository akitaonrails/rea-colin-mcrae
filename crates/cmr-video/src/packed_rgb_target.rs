//! Checked conversion from canonical RGB pixels into caller-selected masks.
//!
//! The runtime display reports framebuffer pixel-format metadata independently
//! from the alternate C3D texture descriptor. This module keeps that validated
//! framebuffer metadata beside logical packed words and provides a
//! deterministic source-owned conversion boundary.
//! A word is a numeric packed-pixel value; this module deliberately does not
//! claim a legacy byte order, row pitch, dithering rule, or driver-equivalent
//! conversion.

use std::fmt;

use crate::legacy_c3d_pixel_format::{
    LegacyNonAlphaLayout, PixelFormatError, RawDdPixelFormat, RgbMaskLayout,
};

/// Runtime framebuffer format kept distinct from enumerated texture formats.
///
/// The complete surface descriptor remains available as provenance. The
/// derived layout is accepted only for a non-palettized, non-alpha RGB surface.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FramebufferRgbFormat {
    descriptor: RawDdPixelFormat,
    layout: RgbMaskLayout,
}

impl FramebufferRgbFormat {
    pub fn from_runtime_surface_descriptor(
        descriptor: RawDdPixelFormat,
    ) -> Result<Self, FramebufferRgbFormatError> {
        if descriptor.size() != 32 {
            return Err(FramebufferRgbFormatError::InvalidDescriptorSize(
                descriptor.size(),
            ));
        }
        if !descriptor.has_rgb_flag() {
            return Err(FramebufferRgbFormatError::MissingRgbFlag);
        }
        if descriptor.has_four_cc_flag() {
            return Err(FramebufferRgbFormatError::FourCcFlagPresent);
        }
        if descriptor.has_palette_indexed_8_flag() {
            return Err(FramebufferRgbFormatError::PaletteIndexedFlagPresent);
        }
        let layout = match descriptor
            .non_alpha_layout()
            .map_err(FramebufferRgbFormatError::InvalidLayout)?
        {
            LegacyNonAlphaLayout::Rgb(layout) => layout,
            LegacyNonAlphaLayout::Maskless8Bit => {
                return Err(FramebufferRgbFormatError::MasklessRgbLayout);
            }
        };
        Ok(Self { descriptor, layout })
    }

    #[must_use]
    pub const fn descriptor(self) -> RawDdPixelFormat {
        self.descriptor
    }

    #[must_use]
    pub const fn layout(self) -> RgbMaskLayout {
        self.layout
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FramebufferRgbFormatError {
    InvalidDescriptorSize(u32),
    MissingRgbFlag,
    FourCcFlagPresent,
    PaletteIndexedFlagPresent,
    MasklessRgbLayout,
    InvalidLayout(PixelFormatError),
}

impl fmt::Display for FramebufferRgbFormatError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid framebuffer RGB format: {self:?}")
    }
}

impl std::error::Error for FramebufferRgbFormatError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidLayout(source) => Some(source),
            _ => None,
        }
    }
}

/// A canonical-RGB conversion failure detected before any target word changes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogicalMaskedRgbTargetError {
    InvalidTargetExtent([u16; 2]),
    InvalidSourceExtent([u16; 2]),
    SourceTargetExtentMismatch { source: [u16; 2], target: [u16; 2] },
    InvalidSourceLength { expected: usize, actual: usize },
}

impl fmt::Display for LogicalMaskedRgbTargetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "logical masked RGB target conversion failed: {self:?}"
        )
    }
}

impl std::error::Error for LogicalMaskedRgbTargetError {}

/// Row-major logical packed words plus their validated RGB mask metadata.
///
/// Keeping the full framebuffer format in the target prevents both descriptor
/// provenance and a 555/565/other driver-selected distinction from being
/// discarded after conversion. The words are intentionally not exposed as a
/// byte buffer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogicalMaskedRgbTarget {
    extent: [u16; 2],
    format: FramebufferRgbFormat,
    words: Box<[u32]>,
}

impl LogicalMaskedRgbTarget {
    /// Creates logical storage for a separately validated runtime framebuffer.
    ///
    /// Texture-selection types are deliberately not accepted here:
    ///
    /// ```compile_fail
    /// use cmr_video::{
    ///     legacy_c3d_pixel_format::SelectedNonAlphaTextureFormat,
    ///     packed_rgb_target::LogicalMaskedRgbTarget,
    /// };
    /// fn cannot_use_texture_format(texture: SelectedNonAlphaTextureFormat) {
    ///     let _ = LogicalMaskedRgbTarget::new([1, 1], texture);
    /// }
    /// ```
    pub fn new(
        extent: [u16; 2],
        format: FramebufferRgbFormat,
    ) -> Result<Self, LogicalMaskedRgbTargetError> {
        let pixel_count = checked_pixel_count(extent)
            .ok_or(LogicalMaskedRgbTargetError::InvalidTargetExtent(extent))?;
        Ok(Self {
            extent,
            format,
            words: vec![0; pixel_count].into_boxed_slice(),
        })
    }

    #[must_use]
    pub const fn extent(&self) -> [u16; 2] {
        self.extent
    }

    #[must_use]
    pub const fn format(&self) -> FramebufferRgbFormat {
        self.format
    }

    #[must_use]
    pub fn words(&self) -> &[u32] {
        &self.words
    }

    /// Packs one canonical RGB value into an already-validated logical pixel.
    ///
    /// This crate-private boundary lets a raster compositor update only the
    /// pixels that pass depth testing. In particular, it does not decode and
    /// repack untouched words, preserving their complete prior bit pattern.
    pub(crate) fn pack_rgb8_at_source_owned_oracle(&mut self, index: usize, rgb: [u8; 3]) {
        self.words[index] = self.format.layout().pack_rgb8(rgb);
    }

    /// Replaces every target word from canonical `[red, green, blue]` pixels.
    ///
    /// All extent and length checks happen before the first write, so every
    /// error leaves the previous target contents intact. Channel scaling uses
    /// [`RgbMaskLayout::pack_rgb8`], whose rounding rule is source-owned.
    pub fn replace_from_canonical_rgb(
        &mut self,
        source_extent: [u16; 2],
        source_pixels: &[[u8; 3]],
    ) -> Result<(), LogicalMaskedRgbTargetError> {
        let expected = checked_pixel_count(source_extent).ok_or(
            LogicalMaskedRgbTargetError::InvalidSourceExtent(source_extent),
        )?;
        if source_extent != self.extent {
            return Err(LogicalMaskedRgbTargetError::SourceTargetExtentMismatch {
                source: source_extent,
                target: self.extent,
            });
        }
        if source_pixels.len() != expected {
            return Err(LogicalMaskedRgbTargetError::InvalidSourceLength {
                expected,
                actual: source_pixels.len(),
            });
        }

        for (target, rgb) in self.words.iter_mut().zip(source_pixels.iter().copied()) {
            *target = self.format.layout().pack_rgb8(rgb);
        }
        Ok(())
    }
}

fn checked_pixel_count(extent: [u16; 2]) -> Option<usize> {
    if extent[0] == 0 || extent[1] == 0 {
        return None;
    }
    Some(usize::from(extent[0]) * usize::from(extent[1]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::legacy_c3d_pixel_format::{
        KnownRgbLayout, RetainedNonAlphaTextureFormats, SelectedNonAlphaTextureFormat,
    };

    const DDPF_ALPHA_PIXELS: u32 = 0x01;
    const DDPF_FOUR_CC: u32 = 0x04;
    const DDPF_PALETTE_INDEXED_8: u32 = 0x20;
    const DDPF_RGB: u32 = 0x40;

    fn descriptor(flags: u32, masks: [u32; 3], alpha: u32) -> RawDdPixelFormat {
        RawDdPixelFormat::from_dwords([32, flags, 0, 16, masks[0], masks[1], masks[2], alpha])
    }

    fn framebuffer_555() -> FramebufferRgbFormat {
        FramebufferRgbFormat::from_runtime_surface_descriptor(descriptor(
            DDPF_RGB,
            [0x7c00, 0x03e0, 0x001f],
            0,
        ))
        .unwrap()
    }

    fn framebuffer_565() -> FramebufferRgbFormat {
        FramebufferRgbFormat::from_runtime_surface_descriptor(descriptor(
            DDPF_RGB,
            [0xf800, 0x07e0, 0x001f],
            0,
        ))
        .unwrap()
    }

    #[test]
    fn packs_canonical_rgb_into_555_and_preserves_raw_framebuffer_provenance() {
        let raw = RawDdPixelFormat::from_dwords([
            32,
            DDPF_RGB | 0x8000_0000,
            0,
            16,
            0x7c00,
            0x03e0,
            0x001f,
            0,
        ]);
        let format = FramebufferRgbFormat::from_runtime_surface_descriptor(raw).unwrap();
        let mut target = LogicalMaskedRgbTarget::new([2, 1], format).unwrap();
        target
            .replace_from_canonical_rgb([2, 1], &[[255, 255, 255], [255, 0, 0]])
            .unwrap();

        assert_eq!(target.extent(), [2, 1]);
        assert_eq!(target.format().descriptor(), raw);
        assert_eq!(target.format().layout().masks(), [0x7c00, 0x03e0, 0x001f]);
        assert_eq!(target.words(), &[0x7fff, 0x7c00]);
    }

    #[test]
    fn packs_canonical_rgb_into_565_with_distinct_green_precision() {
        let mut target_555 = LogicalMaskedRgbTarget::new([1, 1], framebuffer_555()).unwrap();
        let mut target_565 = LogicalMaskedRgbTarget::new([1, 1], framebuffer_565()).unwrap();
        let barely_visible_green = [[0, 4, 0]];

        target_555
            .replace_from_canonical_rgb([1, 1], &barely_visible_green)
            .unwrap();
        target_565
            .replace_from_canonical_rgb([1, 1], &barely_visible_green)
            .unwrap();

        assert_eq!(target_555.words(), &[0]);
        assert_eq!(target_565.words(), &[0x20]);
        assert_eq!(
            target_565.format().layout().masks(),
            [0xf800, 0x07e0, 0x001f]
        );
    }

    #[test]
    fn generic_runtime_masks_pack_24_and_32_bit_logical_words_without_byte_order_claims() {
        for bit_count in [24, 32] {
            let raw = RawDdPixelFormat::from_dwords([
                32,
                DDPF_RGB,
                0,
                bit_count,
                0x00ff_0000,
                0x0000_ff00,
                0x0000_00ff,
                0,
            ]);
            let format = FramebufferRgbFormat::from_runtime_surface_descriptor(raw).unwrap();
            let mut target = LogicalMaskedRgbTarget::new([1, 1], format).unwrap();

            target
                .replace_from_canonical_rgb([1, 1], &[[0x12, 0x34, 0x56]])
                .unwrap();

            assert_eq!(target.words(), &[0x0012_3456]);
            assert_eq!(target.format().descriptor(), raw);
        }
    }

    #[test]
    fn framebuffer_format_rejects_non_rgb_palette_alpha_and_malformed_descriptors() {
        let wrong_size =
            RawDdPixelFormat::from_dwords([0, DDPF_RGB, 0, 16, 0xf800, 0x07e0, 0x001f, 0]);
        assert_eq!(
            FramebufferRgbFormat::from_runtime_surface_descriptor(wrong_size),
            Err(FramebufferRgbFormatError::InvalidDescriptorSize(0))
        );
        assert_eq!(
            FramebufferRgbFormat::from_runtime_surface_descriptor(descriptor(
                0,
                [0xf800, 0x07e0, 0x001f],
                0,
            )),
            Err(FramebufferRgbFormatError::MissingRgbFlag)
        );
        assert_eq!(
            FramebufferRgbFormat::from_runtime_surface_descriptor(descriptor(
                DDPF_RGB | DDPF_FOUR_CC,
                [0xf800, 0x07e0, 0x001f],
                0,
            )),
            Err(FramebufferRgbFormatError::FourCcFlagPresent)
        );
        assert_eq!(
            FramebufferRgbFormat::from_runtime_surface_descriptor(descriptor(
                DDPF_RGB | DDPF_PALETTE_INDEXED_8,
                [0xf800, 0x07e0, 0x001f],
                0,
            )),
            Err(FramebufferRgbFormatError::PaletteIndexedFlagPresent)
        );
        assert_eq!(
            FramebufferRgbFormat::from_runtime_surface_descriptor(descriptor(
                DDPF_RGB | DDPF_ALPHA_PIXELS,
                [0xf800, 0x07e0, 0x001f],
                0,
            )),
            Err(FramebufferRgbFormatError::InvalidLayout(
                PixelFormatError::AlphaPixelsFlagPresent
            ))
        );
        assert_eq!(
            FramebufferRgbFormat::from_runtime_surface_descriptor(descriptor(
                DDPF_RGB,
                [0xf800, 0x07e0, 0x001f],
                0x8000,
            )),
            Err(FramebufferRgbFormatError::InvalidLayout(
                PixelFormatError::AlphaMaskPresent(0x8000)
            ))
        );
        assert_eq!(
            FramebufferRgbFormat::from_runtime_surface_descriptor(descriptor(
                DDPF_RGB,
                [0xf800, 0x07e0, 0x003f],
                0,
            )),
            Err(FramebufferRgbFormatError::InvalidLayout(
                PixelFormatError::OverlappingMasks {
                    first: 1,
                    second: 2,
                }
            ))
        );
    }

    #[test]
    fn framebuffer_packing_does_not_reuse_a_different_texture_candidate_layout() {
        let texture_555 = descriptor(DDPF_RGB, [0x7c00, 0x03e0, 0x001f], 0);
        let selected_texture = RetainedNonAlphaTextureFormats::new(None, Some(texture_555))
            .select_primary(false)
            .unwrap();
        assert_eq!(
            selected_texture,
            SelectedNonAlphaTextureFormat::OpaqueRgb16 {
                descriptor: texture_555,
                layout: KnownRgbLayout::Rgb555,
            }
        );

        let mut framebuffer = LogicalMaskedRgbTarget::new([1, 1], framebuffer_565()).unwrap();
        framebuffer
            .replace_from_canonical_rgb([1, 1], &[[0, 4, 0]])
            .unwrap();

        assert_eq!(framebuffer.words(), &[0x20]);
        assert_eq!(
            framebuffer.format().layout().masks(),
            [0xf800, 0x07e0, 0x001f]
        );
    }

    #[test]
    fn extent_and_length_errors_leave_existing_words_unchanged() {
        let mut target = LogicalMaskedRgbTarget::new([2, 1], framebuffer_565()).unwrap();
        target
            .replace_from_canonical_rgb([2, 1], &[[255, 0, 0], [0, 0, 255]])
            .unwrap();
        let before = target.words().to_vec();

        assert_eq!(
            target.replace_from_canonical_rgb([1, 1], &[[0, 255, 0]]),
            Err(LogicalMaskedRgbTargetError::SourceTargetExtentMismatch {
                source: [1, 1],
                target: [2, 1],
            })
        );
        assert_eq!(target.words(), before);

        assert_eq!(
            target.replace_from_canonical_rgb([2, 1], &[[0, 255, 0]]),
            Err(LogicalMaskedRgbTargetError::InvalidSourceLength {
                expected: 2,
                actual: 1,
            })
        );
        assert_eq!(target.words(), before);

        assert_eq!(
            target.replace_from_canonical_rgb([0, 1], &[]),
            Err(LogicalMaskedRgbTargetError::InvalidSourceExtent([0, 1]))
        );
        assert_eq!(target.words(), before);
    }

    #[test]
    fn rejects_zero_sized_target() {
        assert_eq!(
            LogicalMaskedRgbTarget::new([1, 0], framebuffer_555()),
            Err(LogicalMaskedRgbTargetError::InvalidTargetExtent([1, 0]))
        );
    }
}
