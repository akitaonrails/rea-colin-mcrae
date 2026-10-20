//! Runtime-supplied pixel-format metadata used by the legacy C3D path.
//!
//! The recovered program copies complete, driver-enumerated `DDPixelFormat`
//! descriptors. This module therefore preserves all eight DWORDs and only
//! derives an RGB layout after validating caller-supplied masks. It does not
//! select a universal format or model a driver's texture conversion.

use std::fmt;

const DDPF_ALPHA_PIXELS: u32 = 0x0000_0001;
const DDPF_PALETTE_INDEXED_8: u32 = 0x0000_0020;
const DDPF_RGB: u32 = 0x0000_0040;

/// The eight DWORDs of a driver-enumerated `DDPixelFormat` descriptor.
///
/// Unknown flag bits and union fields are deliberately retained verbatim so
/// later reconstruction work can reinterpret them without losing provenance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawDdPixelFormat {
    dwords: [u32; 8],
}

impl RawDdPixelFormat {
    #[must_use]
    pub const fn from_dwords(dwords: [u32; 8]) -> Self {
        Self { dwords }
    }

    #[must_use]
    pub const fn dwords(self) -> [u32; 8] {
        self.dwords
    }

    #[must_use]
    pub const fn size(self) -> u32 {
        self.dwords[0]
    }

    #[must_use]
    pub const fn flags(self) -> u32 {
        self.dwords[1]
    }

    #[must_use]
    pub const fn four_cc(self) -> u32 {
        self.dwords[2]
    }

    #[must_use]
    pub const fn rgb_bit_count(self) -> u32 {
        self.dwords[3]
    }

    #[must_use]
    pub const fn rgb_masks(self) -> [u32; 3] {
        [self.dwords[4], self.dwords[5], self.dwords[6]]
    }

    #[must_use]
    pub const fn alpha_mask(self) -> u32 {
        self.dwords[7]
    }

    #[must_use]
    pub const fn has_alpha_pixels_flag(self) -> bool {
        self.flags() & DDPF_ALPHA_PIXELS != 0
    }

    #[must_use]
    pub const fn has_palette_indexed_8_flag(self) -> bool {
        self.flags() & DDPF_PALETTE_INDEXED_8 != 0
    }

    #[must_use]
    pub const fn has_rgb_flag(self) -> bool {
        self.flags() & DDPF_RGB != 0
    }

    /// Mirrors the recovered legacy palette-candidate flag predicate exactly.
    #[must_use]
    pub const fn is_legacy_palette_candidate(self) -> bool {
        self.has_palette_indexed_8_flag() && !self.has_alpha_pixels_flag()
    }

    /// Applies source-owned safety validation to a palette-8 descriptor.
    ///
    /// These shape checks are deliberately separate from
    /// [`Self::is_legacy_palette_candidate`], because static evidence does not
    /// establish that the legacy callback checked them.
    pub fn validate_palette_8_layout(self) -> Result<(), PixelFormatError> {
        if self.rgb_bit_count() != 8 {
            return Err(PixelFormatError::InvalidPaletteStorageBits(
                self.rgb_bit_count(),
            ));
        }
        if self.rgb_masks() != [0; 3] {
            return Err(PixelFormatError::PaletteRgbMasksPresent(self.rgb_masks()));
        }
        if self.alpha_mask() != 0 {
            return Err(PixelFormatError::AlphaMaskPresent(self.alpha_mask()));
        }
        Ok(())
    }

    /// Mirrors the recovered opaque RGB candidate conditions.
    ///
    /// The callback is bounded to the RGB flag, absence of the alpha-pixels
    /// flag, 16-bit storage, and exact RGB555 or RGB565 masks.
    #[must_use]
    pub const fn legacy_opaque_rgb_candidate(self) -> Option<KnownRgbLayout> {
        if !self.has_rgb_flag() || self.has_alpha_pixels_flag() || self.rgb_bit_count() != 16 {
            return None;
        }

        match self.rgb_masks() {
            [0x7c00, 0x03e0, 0x001f] => Some(KnownRgbLayout::Rgb555),
            [0xf800, 0x07e0, 0x001f] => Some(KnownRgbLayout::Rgb565),
            _ => None,
        }
    }

    /// Interprets the non-alpha layout fields without assigning meaning to
    /// driver-specific flags or other retained descriptor fields.
    pub fn non_alpha_layout(self) -> Result<LegacyNonAlphaLayout, PixelFormatError> {
        if self.has_alpha_pixels_flag() {
            return Err(PixelFormatError::AlphaPixelsFlagPresent);
        }
        if self.alpha_mask() != 0 {
            return Err(PixelFormatError::AlphaMaskPresent(self.alpha_mask()));
        }

        let masks = self.rgb_masks();
        if self.rgb_bit_count() == 8 && masks == [0; 3] {
            return Ok(LegacyNonAlphaLayout::Maskless8Bit);
        }

        RgbMaskLayout::new(self.rgb_bit_count(), masks).map(LegacyNonAlphaLayout::Rgb)
    }
}

/// A bounded interpretation of the non-alpha descriptor fields.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LegacyNonAlphaLayout {
    /// An eight-bit descriptor with no RGB masks.
    ///
    /// This is the shape of an indexed candidate. The descriptor's retained
    /// flags must still be considered by code that needs to prove palette use.
    Maskless8Bit,
    Rgb(RgbMaskLayout),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RgbMaskLayout {
    storage_bits: u8,
    channels: [RgbChannelLayout; 3],
}

impl RgbMaskLayout {
    /// Validates disjoint, nonzero, contiguous RGB masks within the declared
    /// storage width.
    pub fn new(storage_bits: u32, masks: [u32; 3]) -> Result<Self, PixelFormatError> {
        let storage_bits = u8::try_from(storage_bits)
            .ok()
            .filter(|bits| (1..=32).contains(bits))
            .ok_or(PixelFormatError::InvalidStorageBits(storage_bits))?;

        let allowed_bits = low_bits_mask(storage_bits);
        for (channel, mask) in masks.into_iter().enumerate() {
            if mask == 0 {
                return Err(PixelFormatError::EmptyChannelMask { channel });
            }
            if mask & !allowed_bits != 0 {
                return Err(PixelFormatError::MaskOutsideStorage {
                    channel,
                    mask,
                    storage_bits,
                });
            }
            if !is_contiguous(mask) {
                return Err(PixelFormatError::NonContiguousMask { channel, mask });
            }
        }

        for first in 0..masks.len() {
            for second in first + 1..masks.len() {
                if masks[first] & masks[second] != 0 {
                    return Err(PixelFormatError::OverlappingMasks { first, second });
                }
            }
        }

        Ok(Self {
            storage_bits,
            channels: masks.map(RgbChannelLayout::from_valid_mask),
        })
    }

    #[must_use]
    pub const fn storage_bits(self) -> u8 {
        self.storage_bits
    }

    #[must_use]
    pub const fn channels(self) -> [RgbChannelLayout; 3] {
        self.channels
    }

    #[must_use]
    pub const fn known_layout(self) -> Option<KnownRgbLayout> {
        match (self.storage_bits, self.masks()) {
            (16, [0x7c00, 0x03e0, 0x001f]) => Some(KnownRgbLayout::Rgb555),
            (16, [0xf800, 0x07e0, 0x001f]) => Some(KnownRgbLayout::Rgb565),
            _ => None,
        }
    }

    #[must_use]
    pub const fn masks(self) -> [u32; 3] {
        [
            self.channels[0].mask,
            self.channels[1].mask,
            self.channels[2].mask,
        ]
    }

    /// Packs 8-bit RGB channels using nearest-integer scaling into the masks.
    ///
    /// This deterministic conversion is a project utility, not a claim about
    /// rounding performed by a particular legacy display driver.
    #[must_use]
    pub fn pack_rgb8(self, rgb: [u8; 3]) -> u32 {
        self.channels
            .into_iter()
            .zip(rgb)
            .fold(0, |packed, (channel, value)| {
                let maximum = channel.maximum();
                let quantized = (u64::from(value) * u64::from(maximum) + 127) / 255;
                packed | ((quantized as u32) << channel.shift)
            })
    }

    /// Expands masked RGB channels to 8 bits using nearest-integer scaling.
    #[must_use]
    pub fn unpack_rgb8(self, packed: u32) -> [u8; 3] {
        self.channels.map(|channel| {
            let maximum = channel.maximum();
            let quantized = (packed & channel.mask) >> channel.shift;
            ((u64::from(quantized) * 255 + u64::from(maximum / 2)) / u64::from(maximum)) as u8
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RgbChannelLayout {
    mask: u32,
    shift: u8,
    width: u8,
}

impl RgbChannelLayout {
    const fn from_valid_mask(mask: u32) -> Self {
        Self {
            mask,
            shift: mask.trailing_zeros() as u8,
            width: mask.count_ones() as u8,
        }
    }

    #[must_use]
    pub const fn mask(self) -> u32 {
        self.mask
    }

    #[must_use]
    pub const fn shift(self) -> u8 {
        self.shift
    }

    #[must_use]
    pub const fn width(self) -> u8 {
        self.width
    }

    const fn maximum(self) -> u32 {
        low_bits_mask(self.width)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KnownRgbLayout {
    Rgb555,
    Rgb565,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PixelFormatError {
    AlphaPixelsFlagPresent,
    AlphaMaskPresent(u32),
    InvalidPaletteStorageBits(u32),
    PaletteRgbMasksPresent([u32; 3]),
    InvalidStorageBits(u32),
    EmptyChannelMask {
        channel: usize,
    },
    MaskOutsideStorage {
        channel: usize,
        mask: u32,
        storage_bits: u8,
    },
    NonContiguousMask {
        channel: usize,
        mask: u32,
    },
    OverlappingMasks {
        first: usize,
        second: usize,
    },
}

impl fmt::Display for PixelFormatError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlphaPixelsFlagPresent => {
                write!(
                    formatter,
                    "non-alpha pixel format has the alpha-pixels flag"
                )
            }
            Self::AlphaMaskPresent(mask) => {
                write!(
                    formatter,
                    "non-alpha pixel format has alpha mask {mask:#010x}"
                )
            }
            Self::InvalidPaletteStorageBits(bits) => {
                write!(formatter, "palette-8 storage width is {bits}, expected 8")
            }
            Self::PaletteRgbMasksPresent(masks) => {
                write!(formatter, "palette-8 descriptor has RGB masks {masks:#x?}")
            }
            Self::InvalidStorageBits(bits) => {
                write!(formatter, "RGB storage width {bits} is outside 1..=32")
            }
            Self::EmptyChannelMask { channel } => {
                write!(formatter, "RGB channel {channel} has an empty mask")
            }
            Self::MaskOutsideStorage {
                channel,
                mask,
                storage_bits,
            } => write!(
                formatter,
                "RGB channel {channel} mask {mask:#010x} exceeds {storage_bits}-bit storage"
            ),
            Self::NonContiguousMask { channel, mask } => write!(
                formatter,
                "RGB channel {channel} mask {mask:#010x} is not contiguous"
            ),
            Self::OverlappingMasks { first, second } => {
                write!(formatter, "RGB channel masks {first} and {second} overlap")
            }
        }
    }
}

impl std::error::Error for PixelFormatError {}

const fn low_bits_mask(width: u8) -> u32 {
    if width == 32 {
        u32::MAX
    } else {
        (1_u32 << width) - 1
    }
}

const fn is_contiguous(mask: u32) -> bool {
    let shifted = mask >> mask.trailing_zeros();
    shifted & shifted.wrapping_add(1) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(
        flags: u32,
        bit_count: u32,
        masks: [u32; 3],
        alpha_mask: u32,
    ) -> RawDdPixelFormat {
        RawDdPixelFormat::from_dwords([
            32,
            flags,
            0x4142_4344,
            bit_count,
            masks[0],
            masks[1],
            masks[2],
            alpha_mask,
        ])
    }

    #[test]
    fn retains_the_exact_driver_descriptor() {
        let raw = [
            32,
            0x8123_4567,
            0x4142_4344,
            24,
            0x00ff_0000,
            0x0000_ff00,
            0x0000_00ff,
            0xdead_beef,
        ];
        let descriptor = RawDdPixelFormat::from_dwords(raw);

        assert_eq!(descriptor.dwords(), raw);
        assert_eq!(descriptor.size(), 32);
        assert_eq!(descriptor.flags(), 0x8123_4567);
        assert_eq!(descriptor.four_cc(), 0x4142_4344);
        assert_eq!(descriptor.rgb_bit_count(), 24);
        assert_eq!(descriptor.rgb_masks(), raw[4..7]);
        assert_eq!(descriptor.alpha_mask(), 0xdead_beef);
    }

    #[test]
    fn separates_exact_palette_candidate_flags_from_safe_layout_validation() {
        let palette_descriptor = descriptor(DDPF_PALETTE_INDEXED_8, 8, [0; 3], 0);

        assert_eq!(
            palette_descriptor.non_alpha_layout(),
            Ok(LegacyNonAlphaLayout::Maskless8Bit)
        );
        assert!(palette_descriptor.is_legacy_palette_candidate());
        assert_eq!(palette_descriptor.validate_palette_8_layout(), Ok(()));
        assert!(palette_descriptor.has_palette_indexed_8_flag());
        assert!(!palette_descriptor.has_rgb_flag());

        let mixed_with_unrelated_bits = descriptor(
            DDPF_PALETTE_INDEXED_8 | DDPF_RGB | 0x8000_0000,
            8,
            [0; 3],
            0,
        );
        assert!(mixed_with_unrelated_bits.is_legacy_palette_candidate());

        let malformed = descriptor(DDPF_PALETTE_INDEXED_8, 16, [0x7c00, 0x03e0, 0x001f], 0);
        assert!(malformed.is_legacy_palette_candidate());
        assert_eq!(
            malformed.validate_palette_8_layout(),
            Err(PixelFormatError::InvalidPaletteStorageBits(16))
        );
        assert_eq!(
            descriptor(DDPF_PALETTE_INDEXED_8, 8, [4, 2, 1], 0).validate_palette_8_layout(),
            Err(PixelFormatError::PaletteRgbMasksPresent([4, 2, 1]))
        );
        assert_eq!(
            descriptor(DDPF_PALETTE_INDEXED_8, 8, [0; 3], 0x80).validate_palette_8_layout(),
            Err(PixelFormatError::AlphaMaskPresent(0x80))
        );

        let alpha_flagged = descriptor(DDPF_PALETTE_INDEXED_8 | DDPF_ALPHA_PIXELS, 8, [0; 3], 0);
        assert!(!alpha_flagged.is_legacy_palette_candidate());
    }

    #[test]
    fn derives_and_recognizes_rgb_555() {
        let layout = RgbMaskLayout::new(16, [0x7c00, 0x03e0, 0x001f]).unwrap();

        assert_eq!(layout.known_layout(), Some(KnownRgbLayout::Rgb555));
        assert_eq!(layout.channels()[0].shift(), 10);
        assert_eq!(layout.channels()[0].width(), 5);
        assert_eq!(layout.channels()[1].shift(), 5);
        assert_eq!(layout.channels()[1].width(), 5);
        assert_eq!(layout.channels()[2].shift(), 0);
        assert_eq!(layout.channels()[2].width(), 5);
    }

    #[test]
    fn recognizes_only_exact_opaque_legacy_rgb_candidates() {
        let rgb_555 = descriptor(DDPF_RGB, 16, [0x7c00, 0x03e0, 0x001f], 0);
        let rgb_565 = descriptor(DDPF_RGB, 16, [0xf800, 0x07e0, 0x001f], 0);

        assert_eq!(
            rgb_555.legacy_opaque_rgb_candidate(),
            Some(KnownRgbLayout::Rgb555)
        );
        assert_eq!(
            rgb_565.legacy_opaque_rgb_candidate(),
            Some(KnownRgbLayout::Rgb565)
        );
        assert_eq!(
            descriptor(DDPF_RGB | 0x8000_0000, 16, [0xf800, 0x07e0, 0x001f], 0)
                .legacy_opaque_rgb_candidate(),
            Some(KnownRgbLayout::Rgb565)
        );
        assert_eq!(
            descriptor(DDPF_RGB, 24, [0xff0000, 0x00ff00, 0x0000ff], 0)
                .legacy_opaque_rgb_candidate(),
            None
        );
        assert_eq!(
            descriptor(
                DDPF_RGB | DDPF_ALPHA_PIXELS,
                16,
                [0x7c00, 0x03e0, 0x001f],
                0,
            )
            .legacy_opaque_rgb_candidate(),
            None
        );
        assert_eq!(
            descriptor(0, 16, [0x7c00, 0x03e0, 0x001f], 0).legacy_opaque_rgb_candidate(),
            None
        );
        // The recovered predicate keys alpha admission on the flag. The
        // generic safe layout parser below remains free to reject this mask.
        assert_eq!(
            descriptor(DDPF_RGB, 16, [0x7c00, 0x03e0, 0x001f], 0x8000)
                .legacy_opaque_rgb_candidate(),
            Some(KnownRgbLayout::Rgb555)
        );
    }

    #[test]
    fn derives_and_recognizes_rgb_565() {
        let layout = RgbMaskLayout::new(16, [0xf800, 0x07e0, 0x001f]).unwrap();

        assert_eq!(layout.known_layout(), Some(KnownRgbLayout::Rgb565));
        assert_eq!(layout.channels()[0].shift(), 11);
        assert_eq!(layout.channels()[0].width(), 5);
        assert_eq!(layout.channels()[1].shift(), 5);
        assert_eq!(layout.channels()[1].width(), 6);
        assert_eq!(layout.channels()[2].shift(), 0);
        assert_eq!(layout.channels()[2].width(), 5);
    }

    #[test]
    fn rejects_empty_overlapping_noncontiguous_and_out_of_range_masks() {
        assert_eq!(
            RgbMaskLayout::new(16, [0, 0x03e0, 0x001f]),
            Err(PixelFormatError::EmptyChannelMask { channel: 0 })
        );
        assert_eq!(
            RgbMaskLayout::new(16, [0x7c00, 0x07e0, 0x001f]),
            Err(PixelFormatError::OverlappingMasks {
                first: 0,
                second: 1
            })
        );
        assert_eq!(
            RgbMaskLayout::new(16, [0x7400, 0x03e0, 0x001f]),
            Err(PixelFormatError::NonContiguousMask {
                channel: 0,
                mask: 0x7400
            })
        );
        assert_eq!(
            RgbMaskLayout::new(15, [0xf800, 0x07e0, 0x001f]),
            Err(PixelFormatError::MaskOutsideStorage {
                channel: 0,
                mask: 0xf800,
                storage_bits: 15
            })
        );
    }

    #[test]
    fn rejects_invalid_storage_width_and_alpha_layout() {
        assert_eq!(
            RgbMaskLayout::new(0, [4, 2, 1]),
            Err(PixelFormatError::InvalidStorageBits(0))
        );
        assert_eq!(
            RgbMaskLayout::new(33, [4, 2, 1]),
            Err(PixelFormatError::InvalidStorageBits(33))
        );
        assert_eq!(
            descriptor(DDPF_RGB, 16, [0x7c00, 0x03e0, 0x001f], 0x8000).non_alpha_layout(),
            Err(PixelFormatError::AlphaMaskPresent(0x8000))
        );
        assert_eq!(
            descriptor(
                DDPF_RGB | DDPF_ALPHA_PIXELS,
                16,
                [0x7c00, 0x03e0, 0x001f],
                0,
            )
            .non_alpha_layout(),
            Err(PixelFormatError::AlphaPixelsFlagPresent)
        );
    }

    #[test]
    fn rgb_565_pack_unpack_covers_extremes_and_quantization_boundaries() {
        let layout = RgbMaskLayout::new(16, [0xf800, 0x07e0, 0x001f]).unwrap();

        assert_eq!(layout.pack_rgb8([0, 0, 0]), 0);
        assert_eq!(layout.pack_rgb8([255, 255, 255]), 0xffff);
        assert_eq!(layout.unpack_rgb8(0), [0, 0, 0]);
        assert_eq!(layout.unpack_rgb8(0xffff), [255, 255, 255]);

        // Nearest scaling crosses the first 5-bit bucket at input 5 and the
        // first 6-bit bucket at input 3.
        assert_eq!(layout.pack_rgb8([4, 2, 4]), 0);
        assert_eq!(layout.pack_rgb8([5, 3, 5]), 0x0821);
        assert_eq!(layout.unpack_rgb8(0x0821), [8, 4, 8]);
    }

    #[test]
    fn rgb_roundtrip_error_stays_within_channel_quantization() {
        for layout in [
            RgbMaskLayout::new(16, [0x7c00, 0x03e0, 0x001f]).unwrap(),
            RgbMaskLayout::new(16, [0xf800, 0x07e0, 0x001f]).unwrap(),
        ] {
            for value in 0..=u8::MAX {
                let input = [value, value, value];
                let output = layout.unpack_rgb8(layout.pack_rgb8(input));
                for (channel, component) in layout.channels().into_iter().zip(output) {
                    let maximum_error = 255_u16.div_ceil((1_u16 << channel.width()) - 1) / 2;
                    assert!(u16::from(value.abs_diff(component)) <= maximum_error + 1);
                }
            }
        }
    }
}
