//! Deterministic logical-RGB16 Selection stretching.
//!
//! The recovered application contract supplies already-clipped integer source
//! and destination rectangles, requests the numeric source key `0..0`, and
//! delegates sampling to DirectDraw. DirectDraw/driver sampling and rounding
//! remain unresolved. This module therefore names and isolates a source-owned
//! center-nearest policy; its pixels are deterministic reconstruction policy,
//! not a claim of DirectDraw pixel parity.

use std::fmt;

use cmr_core::blit::{BlitMethod, BlitPlan, ColorKey, RectEdges};

use crate::{
    legacy_c3d_pixel_format::KnownRgbLayout,
    ordinary_bitmap_rgb16::{OrdinaryBitmapLogicalRgb16Source, OrdinaryBitmapRgb16Layout},
    packed_rgb_target::LogicalMaskedRgbTarget,
};

/// The sole sampling policy admitted by this bounded software primitive.
///
/// For a zero-based destination offset `d`, source length `s`, and destination
/// length `n`, the sampled zero-based source offset is
/// `floor(((2*d + 1) * s) / (2*n))`. This maps destination pixel centers to
/// the nearest source pixel center using integer rational arithmetic.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SourceOwnedCenterNearestRgb16StretchPolicy;

impl SourceOwnedCenterNearestRgb16StretchPolicy {
    #[must_use]
    pub(crate) fn sample_offset(
        destination_offset: u32,
        source_len: u32,
        destination_len: u32,
    ) -> u32 {
        let numerator = (u64::from(destination_offset) * 2 + 1) * u64::from(source_len);
        let denominator = u64::from(destination_len) * 2;
        u32::try_from(numerator / denominator)
            .expect("a sampled offset inside a u32 source length fits u32")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OrdinaryBitmapRgb16StretchError {
    UnsupportedMethod(BlitMethod),
    InvalidSourceRectangle(RectEdges),
    InvalidDestinationRectangle(RectEdges),
    DestinationLayoutMismatch {
        source: OrdinaryBitmapRgb16Layout,
        destination: Option<KnownRgbLayout>,
    },
    RecoveredNumericZeroColorKeyRequired {
        actual: Option<ColorKey>,
    },
}

impl fmt::Display for OrdinaryBitmapRgb16StretchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "ordinary bitmap logical RGB16 center-nearest stretch failed: {self:?}"
        )
    }
}

impl std::error::Error for OrdinaryBitmapRgb16StretchError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ValidatedStretch {
    source_left: usize,
    source_top: usize,
    source_width: u32,
    source_height: u32,
    destination_left: usize,
    destination_top: usize,
    destination_width: u32,
    destination_height: u32,
}

/// Validates the recovered app-side Selection Stretch request without effects.
///
/// Rectangles must already contain the legacy caller's clipping result. This
/// primitive consumes them literally and performs no proportional recropping.
pub fn preflight_ordinary_bitmap_logical_rgb16_center_nearest_stretch(
    source: &OrdinaryBitmapLogicalRgb16Source,
    target: &LogicalMaskedRgbTarget,
    plan: BlitPlan,
) -> Result<(), OrdinaryBitmapRgb16StretchError> {
    validate_stretch(source, target, plan).map(|_| ())
}

/// Applies the source-owned center-nearest policy after complete validation.
///
/// The sampled logical word is compared to numeric zero. This deliberately
/// does not use [`OrdinaryBitmapLogicalRgb16Source::packed_color_key`]: the
/// recovered shared blitter overwrites Selection's key with literal `0..0`
/// immediately before submitting the DirectDraw Stretch request.
pub fn compose_ordinary_bitmap_logical_rgb16_center_nearest_stretch(
    source: &OrdinaryBitmapLogicalRgb16Source,
    target: &mut LogicalMaskedRgbTarget,
    plan: BlitPlan,
) -> Result<(), OrdinaryBitmapRgb16StretchError> {
    let validated = validate_stretch(source, target, plan)?;
    let source_stride = source.aligned_surface_extent()[0] as usize;
    let destination_stride = usize::from(target.extent()[0]);

    for destination_y in 0..validated.destination_height {
        let source_y = SourceOwnedCenterNearestRgb16StretchPolicy::sample_offset(
            destination_y,
            validated.source_height,
            validated.destination_height,
        );
        for destination_x in 0..validated.destination_width {
            let source_x = SourceOwnedCenterNearestRgb16StretchPolicy::sample_offset(
                destination_x,
                validated.source_width,
                validated.destination_width,
            );
            let source_offset = (validated.source_top + source_y as usize) * source_stride
                + validated.source_left
                + source_x as usize;
            let word = source.logical_words()[source_offset];
            if word != 0 {
                let destination_offset = (validated.destination_top + destination_y as usize)
                    * destination_stride
                    + validated.destination_left
                    + destination_x as usize;
                target.copy_matching_rgb16_word_at_source_owned_oracle(destination_offset, word);
            }
        }
    }
    Ok(())
}

fn validate_stretch(
    source: &OrdinaryBitmapLogicalRgb16Source,
    target: &LogicalMaskedRgbTarget,
    plan: BlitPlan,
) -> Result<ValidatedStretch, OrdinaryBitmapRgb16StretchError> {
    if plan.method != BlitMethod::Stretch {
        return Err(OrdinaryBitmapRgb16StretchError::UnsupportedMethod(
            plan.method,
        ));
    }

    let source_size = validate_rectangle(plan.source, source.aligned_surface_extent()).ok_or(
        OrdinaryBitmapRgb16StretchError::InvalidSourceRectangle(plan.source),
    )?;
    let destination_size = validate_rectangle(plan.destination, target.extent().map(u32::from))
        .ok_or(OrdinaryBitmapRgb16StretchError::InvalidDestinationRectangle(plan.destination))?;

    let expected_layout = match source.layout() {
        OrdinaryBitmapRgb16Layout::Rgb555 => KnownRgbLayout::Rgb555,
        OrdinaryBitmapRgb16Layout::Rgb565 => KnownRgbLayout::Rgb565,
    };
    let destination_layout = target.format().layout().known_layout();
    if destination_layout != Some(expected_layout) {
        return Err(OrdinaryBitmapRgb16StretchError::DestinationLayoutMismatch {
            source: source.layout(),
            destination: destination_layout,
        });
    }

    let recovered_key = ColorKey { low: 0, high: 0 };
    if plan.color_key != Some(recovered_key) {
        return Err(
            OrdinaryBitmapRgb16StretchError::RecoveredNumericZeroColorKeyRequired {
                actual: plan.color_key,
            },
        );
    }

    Ok(ValidatedStretch {
        source_left: plan.source.left as usize,
        source_top: plan.source.top as usize,
        source_width: source_size[0],
        source_height: source_size[1],
        destination_left: plan.destination.left as usize,
        destination_top: plan.destination.top as usize,
        destination_width: destination_size[0],
        destination_height: destination_size[1],
    })
}

fn validate_rectangle(rectangle: RectEdges, extent: [u32; 2]) -> Option<[u32; 2]> {
    let valid = rectangle.left >= 0
        && rectangle.top >= 0
        && rectangle.right > rectangle.left
        && rectangle.bottom > rectangle.top
        && u32::try_from(rectangle.right).is_ok_and(|right| right <= extent[0])
        && u32::try_from(rectangle.bottom).is_ok_and(|bottom| bottom <= extent[1]);
    valid.then(|| {
        [
            (rectangle.right - rectangle.left) as u32,
            (rectangle.bottom - rectangle.top) as u32,
        ]
    })
}

#[cfg(test)]
mod tests {
    use cmr_formats::bitmap::{BitmapMetadata, BitmapPaletteEntry, IndexedBitmap};

    use super::*;
    use crate::{
        legacy_c3d_pixel_format::RawDdPixelFormat,
        ordinary_bitmap_rgb16::convert_ordinary_bitmap_0x81_to_logical_rgb16,
        packed_rgb_target::FramebufferRgbFormat,
    };

    const DDPF_RGB: u32 = 0x40;

    fn source(discriminator: u8) -> OrdinaryBitmapLogicalRgb16Source {
        // Entry zero is deliberately nonblack. Entry one packs to numeric
        // zero, proving Selection keying follows the recovered request value
        // rather than the decoder's earlier packed palette-entry-zero key.
        let palette = vec![
            entry(248, 0, 248),
            entry(0, 0, 0),
            entry(248, 0, 0),
            entry(0, 252, 0),
        ];
        convert_ordinary_bitmap_0x81_to_logical_rgb16(
            &IndexedBitmap {
                width: 4,
                height: 2,
                source_was_top_down: false,
                palette,
                pixels: vec![0, 1, 2, 3, 3, 2, 1, 0],
                row_stride: 4,
                stored_pixels: vec![0, 1, 2, 3, 3, 2, 1, 0],
                metadata: BitmapMetadata {
                    reserved_1: 0,
                    reserved_2: 0,
                    x_pixels_per_meter: 0,
                    y_pixels_per_meter: 0,
                    important_colors: 0,
                },
            },
            discriminator,
        )
        .unwrap()
    }

    fn unaligned_source() -> OrdinaryBitmapLogicalRgb16Source {
        let palette = vec![
            entry(0, 0, 0),
            entry(248, 0, 0),
            entry(0, 252, 0),
            entry(0, 0, 248),
            entry(248, 0, 248),
            entry(0, 252, 248),
            entry(248, 252, 0),
            entry(248, 252, 248),
        ];
        convert_ordinary_bitmap_0x81_to_logical_rgb16(
            &IndexedBitmap {
                width: 3,
                height: 2,
                source_was_top_down: false,
                palette,
                pixels: vec![1, 2, 3, 4, 5, 6],
                row_stride: 4,
                // Padding values differ from both adjacent logical rows so a
                // width-based row index cannot accidentally pass this test.
                stored_pixels: vec![1, 2, 3, 7, 4, 5, 6, 0],
                metadata: BitmapMetadata {
                    reserved_1: 0,
                    reserved_2: 0,
                    x_pixels_per_meter: 0,
                    y_pixels_per_meter: 0,
                    important_colors: 0,
                },
            },
            6,
        )
        .unwrap()
    }

    const fn entry(red: u8, green: u8, blue: u8) -> BitmapPaletteEntry {
        BitmapPaletteEntry {
            red,
            green,
            blue,
            reserved: 0,
        }
    }

    fn target(extent: [u16; 2], layout: OrdinaryBitmapRgb16Layout) -> LogicalMaskedRgbTarget {
        let masks = match layout {
            OrdinaryBitmapRgb16Layout::Rgb555 => [0x7c00, 0x03e0, 0x001f],
            OrdinaryBitmapRgb16Layout::Rgb565 => [0xf800, 0x07e0, 0x001f],
        };
        let format =
            FramebufferRgbFormat::from_runtime_surface_descriptor(RawDdPixelFormat::from_dwords([
                32, DDPF_RGB, 0, 16, masks[0], masks[1], masks[2], 0,
            ]))
            .unwrap();
        LogicalMaskedRgbTarget::new(extent, format).unwrap()
    }

    const fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RectEdges {
        RectEdges {
            left,
            top,
            right,
            bottom,
        }
    }

    const fn plan(source: RectEdges, destination: RectEdges) -> BlitPlan {
        BlitPlan {
            source,
            destination,
            method: BlitMethod::Stretch,
            color_key: Some(ColorKey { low: 0, high: 0 }),
        }
    }

    #[test]
    fn center_nearest_formula_is_integer_rational_and_bounded() {
        let sequence = |source_len, destination_len| {
            (0..destination_len)
                .map(|offset| {
                    SourceOwnedCenterNearestRgb16StretchPolicy::sample_offset(
                        offset,
                        source_len,
                        destination_len,
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(sequence(4, 2), [1, 3]);
        assert_eq!(sequence(5, 3), [0, 2, 4]);
        assert_eq!(sequence(3, 5), [0, 0, 1, 2, 2]);
        assert_eq!(sequence(2, 1), [1]);
        assert_eq!(sequence(1, 6), [0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn numeric_zero_is_transparent_even_when_palette_entry_zero_is_nonblack() {
        for discriminator in [5, 6] {
            let source = source(discriminator);
            assert_ne!(source.packed_color_key(), 0);
            let mut target = target([4, 2], source.layout());
            target
                .replace_from_canonical_rgb([4, 2], &[[255, 255, 255]; 8])
                .unwrap();

            compose_ordinary_bitmap_logical_rgb16_center_nearest_stretch(
                &source,
                &mut target,
                plan(rect(0, 0, 4, 2), rect(0, 0, 4, 2)),
            )
            .unwrap();

            let white = match source.layout() {
                OrdinaryBitmapRgb16Layout::Rgb555 => 0x7fff,
                OrdinaryBitmapRgb16Layout::Rgb565 => 0xffff,
            };
            assert_eq!(target.words()[0], u32::from(source.packed_color_key()));
            assert_eq!(target.words()[1], white);
            assert_eq!(target.words()[6], white);
            assert_eq!(target.words()[7], u32::from(source.packed_color_key()));
        }
    }

    #[test]
    fn consumes_already_clipped_rectangles_literally() {
        let source = source(6);
        let mut target = target([5, 4], source.layout());
        target
            .replace_from_canonical_rgb([5, 4], &[[255, 255, 255]; 20])
            .unwrap();

        compose_ordinary_bitmap_logical_rgb16_center_nearest_stretch(
            &source,
            &mut target,
            plan(rect(1, 0, 4, 2), rect(1, 1, 3, 3)),
        )
        .unwrap();

        // Source width 3 -> destination width 2 samples offsets 0 and 2.
        // No additional proportional clipping or rectangle adjustment occurs.
        assert_eq!(target.words()[6], 0xffff); // sampled numeric zero preserves white
        assert_eq!(target.words()[7], 0x07e0);
        assert_eq!(target.words()[11], 0xf800);
        assert_eq!(target.words()[12], u32::from(source.packed_color_key()));
        assert_eq!(target.words()[0], 0xffff);
    }

    #[test]
    fn asymmetric_vertical_stretch_indexes_rows_by_aligned_source_stride() {
        let source = unaligned_source();
        assert_eq!(source.dib_extent(), [3, 2]);
        assert_eq!(source.aligned_surface_extent(), [4, 2]);
        let mut target = target([2, 3], source.layout());

        compose_ordinary_bitmap_logical_rgb16_center_nearest_stretch(
            &source,
            &mut target,
            plan(rect(0, 0, 3, 2), rect(0, 0, 2, 3)),
        )
        .unwrap();

        // Horizontal 3->2 samples [0,2]; vertical 2->3 samples [0,1,1].
        assert_eq!(
            target.words(),
            [0xf800, 0x001f, 0xf81f, 0xffe0, 0xf81f, 0xffe0]
        );
    }

    #[test]
    fn rejects_every_contract_mismatch_before_writing() {
        let rgb565 = source(6);
        let mut target = target([4, 2], rgb565.layout());
        target
            .replace_from_canonical_rgb([4, 2], &[[17, 31, 47]; 8])
            .unwrap();
        let before = target.clone();
        let valid = plan(rect(0, 0, 4, 2), rect(0, 0, 4, 2));
        let cases = [
            (
                BlitPlan {
                    method: BlitMethod::Fast,
                    ..valid
                },
                OrdinaryBitmapRgb16StretchError::UnsupportedMethod(BlitMethod::Fast),
            ),
            (
                BlitPlan {
                    color_key: None,
                    ..valid
                },
                OrdinaryBitmapRgb16StretchError::RecoveredNumericZeroColorKeyRequired {
                    actual: None,
                },
            ),
            (
                BlitPlan {
                    color_key: Some(ColorKey { low: 1, high: 1 }),
                    ..valid
                },
                OrdinaryBitmapRgb16StretchError::RecoveredNumericZeroColorKeyRequired {
                    actual: Some(ColorKey { low: 1, high: 1 }),
                },
            ),
            (
                plan(rect(-1, 0, 3, 2), valid.destination),
                OrdinaryBitmapRgb16StretchError::InvalidSourceRectangle(rect(-1, 0, 3, 2)),
            ),
            (
                BlitPlan {
                    source: rect(-1, 0, 3, 2),
                    color_key: None,
                    ..valid
                },
                // Rectangle validation precedes request-key validation.
                OrdinaryBitmapRgb16StretchError::InvalidSourceRectangle(rect(-1, 0, 3, 2)),
            ),
            (
                plan(valid.source, rect(1, 0, 5, 2)),
                OrdinaryBitmapRgb16StretchError::InvalidDestinationRectangle(rect(1, 0, 5, 2)),
            ),
        ];
        for (request, expected) in cases {
            assert_eq!(
                compose_ordinary_bitmap_logical_rgb16_center_nearest_stretch(
                    &rgb565,
                    &mut target,
                    request,
                ),
                Err(expected)
            );
            assert_eq!(target, before);
        }

        let rgb555 = source(5);
        assert_eq!(
            compose_ordinary_bitmap_logical_rgb16_center_nearest_stretch(
                &rgb555,
                &mut target,
                BlitPlan {
                    color_key: None,
                    ..valid
                },
            ),
            // Layout validation precedes request-key validation.
            Err(OrdinaryBitmapRgb16StretchError::DestinationLayoutMismatch {
                source: OrdinaryBitmapRgb16Layout::Rgb555,
                destination: Some(KnownRgbLayout::Rgb565),
            })
        );
        assert_eq!(target, before);
    }
}
