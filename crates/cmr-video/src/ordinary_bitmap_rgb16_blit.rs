//! Logical blits for converted ordinary `0x81` bitmaps.
//!
//! This is a source-owned composition boundary over logical packed words
//! between matching RGB555 or RGB565 layouts, with the recovered packed
//! palette-entry-zero key when requested. It models both recovered
//! `FUN_0049e3e0` paths: a validated exact-size `Fast` copy, and a `Stretch`
//! resample for differing source/destination sizes (the loading bar's vertical
//! fill and its 1px horizontal rule, `rea.official-eu.game-fsm-load-surface`).
//!
//! The clip + Fast-vs-Stretch dispatch is the recovered
//! [`cmr_core::blit::plan_legacy_blit`] contract. The DirectDraw `Blt` stretch
//! samples in the driver (not in the static image), so the stretch resample here
//! is a documented backend-neutral **nearest-neighbor** policy - the same stance
//! as the sequential-`f32` software raster - not a byte-exact oracle of the DX5
//! driver. Native surface bytes and pitch, driver dithering, and presentation
//! remain outside this module.

use std::fmt;

use cmr_core::blit::{BlitMethod, BlitPlan, ColorKey, RectEdges};

use crate::{
    legacy_c3d_pixel_format::KnownRgbLayout,
    ordinary_bitmap_rgb16::{OrdinaryBitmapLogicalRgb16Source, OrdinaryBitmapRgb16Layout},
    packed_rgb_target::LogicalMaskedRgbTarget,
};

/// A complete validation failure detected before any destination word changes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OrdinaryBitmapRgb16BlitError {
    UnsupportedStretch,
    InvalidSourceRectangle(RectEdges),
    InvalidDestinationRectangle(RectEdges),
    SourceDestinationSizeMismatch {
        source: [u32; 2],
        destination: [u32; 2],
    },
    DestinationLayoutMismatch {
        source: OrdinaryBitmapRgb16Layout,
        destination: Option<KnownRgbLayout>,
    },
    UnsupportedColorKeyRange(ColorKey),
    UnsupportedColorKey {
        expected: u16,
        actual: u32,
    },
}

impl fmt::Display for OrdinaryBitmapRgb16BlitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "ordinary bitmap logical RGB16 blit failed: {self:?}"
        )
    }
}

impl std::error::Error for OrdinaryBitmapRgb16BlitError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ValidatedFastBlit {
    source_left: usize,
    source_top: usize,
    destination_left: usize,
    destination_top: usize,
    width: usize,
    height: usize,
    packed_color_key: Option<u16>,
}

/// Validates an exact-size ordinary-bitmap logical RGB16 blit without effects.
///
/// Source bounds use [`OrdinaryBitmapLogicalRgb16Source::aligned_surface_extent`]
/// because the recovered conversion retains complete aligned DIB rows. The
/// destination bounds use [`LogicalMaskedRgbTarget::extent`]. A color key, when
/// present, must be a single-value range equal to the source's recovered
/// packed embedded-palette entry zero.
pub fn preflight_ordinary_bitmap_logical_rgb16_fast_blit(
    source: &OrdinaryBitmapLogicalRgb16Source,
    target: &LogicalMaskedRgbTarget,
    plan: BlitPlan,
) -> Result<(), OrdinaryBitmapRgb16BlitError> {
    validate_fast_blit(source, target, plan).map(|_| ())
}

/// Composes a preflight-valid exact-size logical RGB16 copy.
///
/// Validation completes before the first write. Consequently every rejection
/// preserves the target exactly, including words outside the requested
/// rectangle and pixels matching a valid color key.
pub fn compose_ordinary_bitmap_logical_rgb16_fast_blit(
    source: &OrdinaryBitmapLogicalRgb16Source,
    target: &mut LogicalMaskedRgbTarget,
    plan: BlitPlan,
) -> Result<(), OrdinaryBitmapRgb16BlitError> {
    let validated = validate_fast_blit(source, target, plan)?;
    let source_stride = source.aligned_surface_extent()[0] as usize;
    let destination_stride = usize::from(target.extent()[0]);

    for row in 0..validated.height {
        let source_start = (validated.source_top + row) * source_stride + validated.source_left;
        let destination_start =
            (validated.destination_top + row) * destination_stride + validated.destination_left;
        for column in 0..validated.width {
            let word = source.logical_words()[source_start + column];
            if validated.packed_color_key != Some(word) {
                target.copy_matching_rgb16_word_at_source_owned_oracle(
                    destination_start + column,
                    word,
                );
            }
        }
    }
    Ok(())
}

/// Validates a stretch (resampling) ordinary-bitmap logical RGB16 blit.
///
/// Like the fast preflight but with no source/destination size-match
/// requirement - the destination rectangle may be any positive size.
pub fn preflight_ordinary_bitmap_logical_rgb16_stretch_blit(
    source: &OrdinaryBitmapLogicalRgb16Source,
    target: &LogicalMaskedRgbTarget,
    plan: BlitPlan,
) -> Result<(), OrdinaryBitmapRgb16BlitError> {
    validate_stretch_blit(source, target, plan).map(|_| ())
}

/// Composes a preflight-valid stretch logical RGB16 blit by nearest-neighbor
/// resampling.
///
/// Each destination pixel samples the source at `src = src_origin + dst_offset *
/// src_size / dst_size` (integer, truncating) - the documented backend-neutral
/// stretch policy (the DX5 driver's `Blt` sampling is not in the static image).
/// Validation completes before the first write, so every rejection preserves the
/// target exactly, and a color-key-matching source pixel leaves its destination
/// word untouched.
pub fn compose_ordinary_bitmap_logical_rgb16_stretch_blit(
    source: &OrdinaryBitmapLogicalRgb16Source,
    target: &mut LogicalMaskedRgbTarget,
    plan: BlitPlan,
) -> Result<(), OrdinaryBitmapRgb16BlitError> {
    let validated = validate_stretch_blit(source, target, plan)?;
    let source_stride = source.aligned_surface_extent()[0] as usize;
    let destination_stride = usize::from(target.extent()[0]);

    for row in 0..validated.destination_height {
        let source_row =
            validated.source_top + row * validated.source_height / validated.destination_height;
        let destination_start =
            (validated.destination_top + row) * destination_stride + validated.destination_left;
        for column in 0..validated.destination_width {
            let source_column = validated.source_left
                + column * validated.source_width / validated.destination_width;
            let word = source.logical_words()[source_row * source_stride + source_column];
            if validated.packed_color_key != Some(word) {
                target.copy_matching_rgb16_word_at_source_owned_oracle(
                    destination_start + column,
                    word,
                );
            }
        }
    }
    Ok(())
}

fn validate_fast_blit(
    source: &OrdinaryBitmapLogicalRgb16Source,
    target: &LogicalMaskedRgbTarget,
    plan: BlitPlan,
) -> Result<ValidatedFastBlit, OrdinaryBitmapRgb16BlitError> {
    if plan.method == BlitMethod::Stretch {
        return Err(OrdinaryBitmapRgb16BlitError::UnsupportedStretch);
    }

    let (source_size, destination_size) = validate_blit_rectangles(source, target, plan)?;
    if source_size != destination_size {
        return Err(
            OrdinaryBitmapRgb16BlitError::SourceDestinationSizeMismatch {
                source: source_size,
                destination: destination_size,
            },
        );
    }

    let packed_color_key = validate_layout_and_color_key(source, target, plan.color_key)?;

    Ok(ValidatedFastBlit {
        source_left: plan.source.left as usize,
        source_top: plan.source.top as usize,
        destination_left: plan.destination.left as usize,
        destination_top: plan.destination.top as usize,
        width: source_size[0] as usize,
        height: source_size[1] as usize,
        packed_color_key,
    })
}

/// A preflight-valid stretch blit: an arbitrary-size source rectangle resampled
/// into an arbitrary-size destination rectangle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ValidatedStretchBlit {
    source_left: usize,
    source_top: usize,
    source_width: usize,
    source_height: usize,
    destination_left: usize,
    destination_top: usize,
    destination_width: usize,
    destination_height: usize,
    packed_color_key: Option<u16>,
}

fn validate_stretch_blit(
    source: &OrdinaryBitmapLogicalRgb16Source,
    target: &LogicalMaskedRgbTarget,
    plan: BlitPlan,
) -> Result<ValidatedStretchBlit, OrdinaryBitmapRgb16BlitError> {
    // The stretch resample handles any positive source/destination sizes (an
    // equal-size plan degenerates to a 1:1 copy); the caller dispatches Fast vs
    // Stretch from the recovered plan, so the method flag is not re-checked here.
    let (source_size, destination_size) = validate_blit_rectangles(source, target, plan)?;
    let packed_color_key = validate_layout_and_color_key(source, target, plan.color_key)?;

    Ok(ValidatedStretchBlit {
        source_left: plan.source.left as usize,
        source_top: plan.source.top as usize,
        source_width: source_size[0] as usize,
        source_height: source_size[1] as usize,
        destination_left: plan.destination.left as usize,
        destination_top: plan.destination.top as usize,
        destination_width: destination_size[0] as usize,
        destination_height: destination_size[1] as usize,
        packed_color_key,
    })
}

/// Validate the source and destination rectangles against their surface extents,
/// returning `(source_size, destination_size)`.
fn validate_blit_rectangles(
    source: &OrdinaryBitmapLogicalRgb16Source,
    target: &LogicalMaskedRgbTarget,
    plan: BlitPlan,
) -> Result<([u32; 2], [u32; 2]), OrdinaryBitmapRgb16BlitError> {
    let source_size = validate_rectangle(
        plan.source,
        source.aligned_surface_extent(),
        OrdinaryBitmapRgb16BlitError::InvalidSourceRectangle,
    )?;
    let destination_size = validate_rectangle(
        plan.destination,
        target.extent().map(u32::from),
        OrdinaryBitmapRgb16BlitError::InvalidDestinationRectangle,
    )?;
    Ok((source_size, destination_size))
}

/// Validate the source/destination layout match and resolve the optional color
/// key against the source's recovered packed palette-entry-zero.
fn validate_layout_and_color_key(
    source: &OrdinaryBitmapLogicalRgb16Source,
    target: &LogicalMaskedRgbTarget,
    color_key: Option<ColorKey>,
) -> Result<Option<u16>, OrdinaryBitmapRgb16BlitError> {
    let expected_layout = match source.layout() {
        OrdinaryBitmapRgb16Layout::Rgb555 => KnownRgbLayout::Rgb555,
        OrdinaryBitmapRgb16Layout::Rgb565 => KnownRgbLayout::Rgb565,
    };
    let destination_layout = target.format().layout().known_layout();
    if destination_layout != Some(expected_layout) {
        return Err(OrdinaryBitmapRgb16BlitError::DestinationLayoutMismatch {
            source: source.layout(),
            destination: destination_layout,
        });
    }

    match color_key {
        None => Ok(None),
        Some(key) if key.low != key.high => {
            Err(OrdinaryBitmapRgb16BlitError::UnsupportedColorKeyRange(key))
        }
        Some(key) if key.low != u32::from(source.packed_color_key()) => {
            Err(OrdinaryBitmapRgb16BlitError::UnsupportedColorKey {
                expected: source.packed_color_key(),
                actual: key.low,
            })
        }
        Some(_) => Ok(Some(source.packed_color_key())),
    }
}

fn validate_rectangle(
    rectangle: RectEdges,
    extent: [u32; 2],
    invalid: impl FnOnce(RectEdges) -> OrdinaryBitmapRgb16BlitError,
) -> Result<[u32; 2], OrdinaryBitmapRgb16BlitError> {
    let valid = rectangle.left >= 0
        && rectangle.top >= 0
        && rectangle.right > rectangle.left
        && rectangle.bottom > rectangle.top
        && u32::try_from(rectangle.right).is_ok_and(|right| right <= extent[0])
        && u32::try_from(rectangle.bottom).is_ok_and(|bottom| bottom <= extent[1]);
    if !valid {
        return Err(invalid(rectangle));
    }

    Ok([
        u32::try_from(rectangle.right - rectangle.left)
            .expect("validated positive rectangle width fits u32"),
        u32::try_from(rectangle.bottom - rectangle.top)
            .expect("validated positive rectangle height fits u32"),
    ])
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

    fn converted_source(discriminator: u8) -> OrdinaryBitmapLogicalRgb16Source {
        let mut palette = vec![entry(0, 0, 0); 4];
        palette[0] = entry(8, 4, 248);
        palette[1] = entry(248, 0, 0);
        palette[2] = entry(0, 252, 0);
        palette[3] = entry(0, 0, 248);
        convert_ordinary_bitmap_0x81_to_logical_rgb16(
            &IndexedBitmap {
                width: 3,
                height: 2,
                source_was_top_down: false,
                palette,
                pixels: vec![0, 1, 2, 3, 2, 1],
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

    const fn entry(red: u8, green: u8, blue: u8) -> BitmapPaletteEntry {
        BitmapPaletteEntry {
            red,
            green,
            blue,
            reserved: 0,
        }
    }

    fn format(layout: OrdinaryBitmapRgb16Layout) -> FramebufferRgbFormat {
        let masks = match layout {
            OrdinaryBitmapRgb16Layout::Rgb555 => [0x7c00, 0x03e0, 0x001f],
            OrdinaryBitmapRgb16Layout::Rgb565 => [0xf800, 0x07e0, 0x001f],
        };
        FramebufferRgbFormat::from_runtime_surface_descriptor(RawDdPixelFormat::from_dwords([
            32, DDPF_RGB, 0, 16, masks[0], masks[1], masks[2], 0,
        ]))
        .unwrap()
    }

    const fn edges(left: i32, top: i32, right: i32, bottom: i32) -> RectEdges {
        RectEdges {
            left,
            top,
            right,
            bottom,
        }
    }

    fn plan(source: RectEdges, destination: RectEdges, color_key: Option<ColorKey>) -> BlitPlan {
        BlitPlan {
            source,
            destination,
            method: BlitMethod::Fast,
            color_key,
        }
    }

    #[test]
    fn exact_fast_copy_uses_aligned_source_rows_and_preserves_other_words() {
        let source = converted_source(6);
        let mut target = LogicalMaskedRgbTarget::new([5, 3], format(source.layout())).unwrap();
        target
            .replace_from_canonical_rgb([5, 3], &[[255, 255, 255]; 15])
            .unwrap();

        let plan = plan(edges(1, 0, 4, 2), edges(0, 1, 3, 3), None);
        preflight_ordinary_bitmap_logical_rgb16_fast_blit(&source, &target, plan).unwrap();
        compose_ordinary_bitmap_logical_rgb16_fast_blit(&source, &mut target, plan).unwrap();

        assert_eq!(
            target.words(),
            [
                0xffff, 0xffff, 0xffff, 0xffff, 0xffff, // untouched row
                0xf800, 0x07e0, 0x001f, 0xffff, 0xffff, // includes stored padding
                0x07e0, 0xf800, 0x083f, 0xffff, 0xffff,
            ]
        );
    }

    #[test]
    fn recovered_nonzero_packed_key_preserves_only_matching_destination_words() {
        let source = converted_source(5);
        let mut target = LogicalMaskedRgbTarget::new([4, 2], format(source.layout())).unwrap();
        target
            .replace_from_canonical_rgb([4, 2], &[[255, 255, 255]; 8])
            .unwrap();
        let key = u32::from(source.packed_color_key());
        let plan = plan(
            edges(0, 0, 4, 2),
            edges(0, 0, 4, 2),
            Some(ColorKey {
                low: key,
                high: key,
            }),
        );

        compose_ordinary_bitmap_logical_rgb16_fast_blit(&source, &mut target, plan).unwrap();

        assert_eq!(
            target.words(),
            [
                0x7fff, 0x7c00, 0x03e0, 0x001f, 0x001f, 0x03e0, 0x7c00, 0x7fff
            ]
        );
    }

    #[test]
    fn rejects_stretch_range_wrong_key_layout_and_invalid_rectangles_atomically() {
        let source = converted_source(6);
        let mut target = LogicalMaskedRgbTarget::new([4, 2], format(source.layout())).unwrap();
        target
            .replace_from_canonical_rgb([4, 2], &[[255, 255, 255]; 8])
            .unwrap();
        let before = target.words().to_vec();
        let base = plan(edges(0, 0, 4, 2), edges(0, 0, 4, 2), None);

        let cases = [
            (
                BlitPlan {
                    method: BlitMethod::Stretch,
                    ..base
                },
                OrdinaryBitmapRgb16BlitError::UnsupportedStretch,
            ),
            (
                plan(
                    base.source,
                    base.destination,
                    Some(ColorKey { low: 1, high: 2 }),
                ),
                OrdinaryBitmapRgb16BlitError::UnsupportedColorKeyRange(ColorKey {
                    low: 1,
                    high: 2,
                }),
            ),
            (
                plan(
                    base.source,
                    base.destination,
                    Some(ColorKey { low: 1, high: 1 }),
                ),
                OrdinaryBitmapRgb16BlitError::UnsupportedColorKey {
                    expected: source.packed_color_key(),
                    actual: 1,
                },
            ),
            (
                plan(edges(-1, 0, 3, 2), base.destination, None),
                OrdinaryBitmapRgb16BlitError::InvalidSourceRectangle(edges(-1, 0, 3, 2)),
            ),
            (
                plan(edges(0, 0, 0, 2), base.destination, None),
                OrdinaryBitmapRgb16BlitError::InvalidSourceRectangle(edges(0, 0, 0, 2)),
            ),
            (
                plan(edges(0, 0, 4, 3), base.destination, None),
                OrdinaryBitmapRgb16BlitError::InvalidSourceRectangle(edges(0, 0, 4, 3)),
            ),
            (
                plan(base.source, edges(1, 0, 5, 2), None),
                OrdinaryBitmapRgb16BlitError::InvalidDestinationRectangle(edges(1, 0, 5, 2)),
            ),
            (
                plan(edges(0, 0, 3, 2), base.destination, None),
                OrdinaryBitmapRgb16BlitError::SourceDestinationSizeMismatch {
                    source: [3, 2],
                    destination: [4, 2],
                },
            ),
        ];

        for (plan, expected) in cases {
            assert_eq!(
                compose_ordinary_bitmap_logical_rgb16_fast_blit(&source, &mut target, plan),
                Err(expected)
            );
            assert_eq!(target.words(), before);
        }

        let mismatched = converted_source(5);
        assert_eq!(
            compose_ordinary_bitmap_logical_rgb16_fast_blit(&mismatched, &mut target, base),
            Err(OrdinaryBitmapRgb16BlitError::DestinationLayoutMismatch {
                source: OrdinaryBitmapRgb16Layout::Rgb555,
                destination: Some(KnownRgbLayout::Rgb565),
            })
        );
        assert_eq!(target.words(), before);

        compose_ordinary_bitmap_logical_rgb16_fast_blit(&source, &mut target, base).unwrap();
        assert_eq!(
            target.words(),
            source
                .logical_words()
                .iter()
                .copied()
                .map(u32::from)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn rejects_other_valid_rgb_mask_layouts_without_mutation() {
        let source = converted_source(6);
        let generic_format =
            FramebufferRgbFormat::from_runtime_surface_descriptor(RawDdPixelFormat::from_dwords([
                32,
                DDPF_RGB,
                0,
                24,
                0x00ff_0000,
                0x0000_ff00,
                0x0000_00ff,
                0,
            ]))
            .unwrap();
        let mut target = LogicalMaskedRgbTarget::new([4, 2], generic_format).unwrap();
        target
            .replace_from_canonical_rgb([4, 2], &[[17, 31, 47]; 8])
            .unwrap();
        let before = target.clone();
        let plan = plan(edges(0, 0, 4, 2), edges(0, 0, 4, 2), None);

        assert_eq!(
            compose_ordinary_bitmap_logical_rgb16_fast_blit(&source, &mut target, plan),
            Err(OrdinaryBitmapRgb16BlitError::DestinationLayoutMismatch {
                source: OrdinaryBitmapRgb16Layout::Rgb565,
                destination: None,
            })
        );
        assert_eq!(target, before);
    }

    #[test]
    fn preflight_has_no_effect_on_success_or_failure() {
        let source = converted_source(6);
        let target = LogicalMaskedRgbTarget::new([4, 2], format(source.layout())).unwrap();
        let before = target.clone();
        let valid = plan(edges(0, 0, 4, 2), edges(0, 0, 4, 2), None);

        preflight_ordinary_bitmap_logical_rgb16_fast_blit(&source, &target, valid).unwrap();
        assert_eq!(target, before);
        assert_eq!(
            preflight_ordinary_bitmap_logical_rgb16_fast_blit(
                &source,
                &target,
                BlitPlan {
                    method: BlitMethod::Stretch,
                    ..valid
                }
            ),
            Err(OrdinaryBitmapRgb16BlitError::UnsupportedStretch)
        );
        assert_eq!(target, before);
    }

    fn stretch_plan(
        source: RectEdges,
        destination: RectEdges,
        color_key: Option<ColorKey>,
    ) -> BlitPlan {
        BlitPlan {
            source,
            destination,
            method: BlitMethod::Stretch,
            color_key,
        }
    }

    #[test]
    fn horizontal_stretch_nearest_samples_each_destination_column() {
        // Source row 0, columns 1..3 = [red, green] (RGB565); stretch 2 -> 4 wide.
        let source = converted_source(6);
        let mut target = LogicalMaskedRgbTarget::new([4, 1], format(source.layout())).unwrap();
        target
            .replace_from_canonical_rgb([4, 1], &[[255, 255, 255]; 4])
            .unwrap();

        let plan = stretch_plan(edges(1, 0, 3, 1), edges(0, 0, 4, 1), None);
        preflight_ordinary_bitmap_logical_rgb16_stretch_blit(&source, &target, plan).unwrap();
        compose_ordinary_bitmap_logical_rgb16_stretch_blit(&source, &mut target, plan).unwrap();

        // sx = 1 + col*2/4 = {1,1,2,2} -> [red, red, green, green].
        assert_eq!(target.words(), [0xf800, 0xf800, 0x07e0, 0x07e0]);
    }

    #[test]
    fn one_pixel_source_fills_a_line_the_loading_bar_rule_case() {
        // A 1x1 source (one red pixel) stretched to 3x1 - the static LINE.BMP rule.
        let source = converted_source(6);
        let mut target = LogicalMaskedRgbTarget::new([3, 1], format(source.layout())).unwrap();
        target
            .replace_from_canonical_rgb([3, 1], &[[255, 255, 255]; 3])
            .unwrap();

        let plan = stretch_plan(edges(1, 0, 2, 1), edges(0, 0, 3, 1), None);
        compose_ordinary_bitmap_logical_rgb16_stretch_blit(&source, &mut target, plan).unwrap();

        assert_eq!(target.words(), [0xf800, 0xf800, 0xf800]);
    }

    #[test]
    fn stretch_honors_the_color_key_leaving_keyed_destinations_untouched() {
        // Source columns 0..2 = [key, red]; stretch 2 -> 4 with the color key, so
        // the two columns sampling the key keep the white background.
        let source = converted_source(6);
        let mut target = LogicalMaskedRgbTarget::new([4, 1], format(source.layout())).unwrap();
        target
            .replace_from_canonical_rgb([4, 1], &[[255, 255, 255]; 4])
            .unwrap();
        let key = u32::from(source.packed_color_key());

        let plan = stretch_plan(
            edges(0, 0, 2, 1),
            edges(0, 0, 4, 1),
            Some(ColorKey {
                low: key,
                high: key,
            }),
        );
        compose_ordinary_bitmap_logical_rgb16_stretch_blit(&source, &mut target, plan).unwrap();

        // sx = {0,0,1,1}: cols 0,1 sample the key (skipped), cols 2,3 sample red.
        assert_eq!(target.words(), [0xffff, 0xffff, 0xf800, 0xf800]);
    }

    #[test]
    fn stretch_preflight_accepts_differing_sizes_and_rejections_are_atomic() {
        let source = converted_source(6);
        let mut target = LogicalMaskedRgbTarget::new([4, 2], format(source.layout())).unwrap();
        target
            .replace_from_canonical_rgb([4, 2], &[[255, 255, 255]; 8])
            .unwrap();
        let before = target.words().to_vec();

        // A differing-size plan the fast path rejects is valid for the stretch path.
        let grow = stretch_plan(edges(0, 0, 2, 1), edges(0, 0, 4, 2), None);
        preflight_ordinary_bitmap_logical_rgb16_stretch_blit(&source, &target, grow).unwrap();

        // An out-of-bounds source rectangle is still rejected, atomically.
        let bad = stretch_plan(edges(0, 0, 5, 1), edges(0, 0, 4, 2), None);
        assert_eq!(
            compose_ordinary_bitmap_logical_rgb16_stretch_blit(&source, &mut target, bad),
            Err(OrdinaryBitmapRgb16BlitError::InvalidSourceRectangle(edges(
                0, 0, 5, 1
            )))
        );
        assert_eq!(target.words(), before);
    }
}
