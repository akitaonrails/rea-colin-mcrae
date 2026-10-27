//! Indexed software composition for the recovered state-4 loading overlay.
//!
//! The selected-stage loading artwork is the destination frame. The recovered
//! `FUN_0044dea0` order then stretches the complete `BAR.BMP` over the
//! progress rectangle and a 1x1 source from `LINE.BMP` over the static rule.
//! Both use source index zero as the recovered black color key. Geometry and
//! clipping go through the shared legacy indexed-blit planner. Stretching uses
//! the project's deterministic center-nearest policy and RGB values are mapped
//! to the active indexed palette by L1 distance with lowest-index ties. Those
//! two choices are explicit software-backend policy: the DX5 driver's stretched
//! raster and an RGB16-to-active-indexed presentation bridge are not statically
//! recoverable here. The ordinary RGB555/565 packing formula is recovered by
//! the separate RGB16 compatibility path.

use std::fmt;

use cmr_core::blit::{BlitDecision, BlitPlan, ColorKey};
use cmr_core::indexed_bitmap_raster::{
    IndexedBitmapPlanError, IndexedBitmapRect, plan_indexed_bitmap_blit_for_extent,
};
use cmr_core::load_interstitial::{
    BlitRect, STAGE_LOADING_PROGRESS_CAP, loading_bar_fill_rect, loading_bar_line_rect,
};
use cmr_formats::bitmap::{BitmapPaletteEntry, IndexedBitmap};
use cmr_formats::riff_palette::PaletteEntry;

use crate::{SoftwarePresentedFrame, SoftwareRgba8Error};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LoadInterstitialLayer {
    Bar,
    Line,
}

/// Recovered painter order from `FUN_0044dea0`.
pub const LOADING_OVERLAY_ORDER: [LoadInterstitialLayer; 2] =
    [LoadInterstitialLayer::Bar, LoadInterstitialLayer::Line];

#[derive(Debug, Eq, PartialEq)]
pub enum LoadInterstitialComposeError {
    Plan {
        layer: LoadInterstitialLayer,
        source: IndexedBitmapPlanError,
    },
    InvalidPaletteIndex {
        layer: LoadInterstitialLayer,
        index: u8,
        entries: usize,
    },
    ProgressExceedsStageCap {
        progress: u8,
        cap: u8,
    },
    ArithmeticOverflow,
    Frame(SoftwareRgba8Error),
}

impl fmt::Display for LoadInterstitialComposeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "loading-interstitial composition failed: {self:?}"
        )
    }
}

impl std::error::Error for LoadInterstitialComposeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Plan { source, .. } => Some(source),
            Self::Frame(source) => Some(source),
            Self::InvalidPaletteIndex { .. }
            | Self::ProgressExceedsStageCap { .. }
            | Self::ArithmeticOverflow => None,
        }
    }
}

/// Draw `BAR.BMP`, then `LINE.BMP`, over a selected-stage loading frame.
pub fn compose_load_interstitial(
    frame: SoftwarePresentedFrame,
    bar: &IndexedBitmap,
    line: &IndexedBitmap,
    progress: u8,
) -> Result<SoftwarePresentedFrame, LoadInterstitialComposeError> {
    if progress > STAGE_LOADING_PROGRESS_CAP {
        return Err(LoadInterstitialComposeError::ProgressExceedsStageCap {
            progress,
            cap: STAGE_LOADING_PROGRESS_CAP,
        });
    }
    let extent = frame.extent();
    let screen_width = i32::from(extent.width);
    let screen_height = i32::from(extent.height);
    let fill = loading_bar_fill_rect(progress, screen_width, screen_height);
    let rule = loading_bar_line_rect(screen_width, screen_height);
    compose_load_interstitial_with_rects(frame, bar, line, fill, rule)
}

fn compose_load_interstitial_with_rects(
    frame: SoftwarePresentedFrame,
    bar: &IndexedBitmap,
    line: &IndexedBitmap,
    fill: BlitRect,
    rule: BlitRect,
) -> Result<SoftwarePresentedFrame, LoadInterstitialComposeError> {
    let extent = frame.extent();
    let mut pixels = frame.pixels().to_vec();
    let palette = Box::new(*frame.palette());
    let key = Some(ColorKey { low: 0, high: 0 });

    compose_layer(
        LoadInterstitialLayer::Bar,
        bar,
        IndexedBitmapRect::new(0, 0, i32_from_u32(bar.width)?, i32_from_u32(bar.height)?),
        IndexedBitmapRect::new(fill.x, fill.y, fill.w, fill.h),
        extent,
        key,
        &mut pixels,
        palette.as_ref(),
    )?;
    compose_layer(
        LoadInterstitialLayer::Line,
        line,
        IndexedBitmapRect::new(0, 0, 1, 1),
        IndexedBitmapRect::new(rule.x, rule.y, rule.w, rule.h),
        extent,
        key,
        &mut pixels,
        palette.as_ref(),
    )?;

    SoftwarePresentedFrame::from_indexed(extent, pixels.into_boxed_slice(), palette)
        .map_err(LoadInterstitialComposeError::Frame)
}

#[allow(clippy::too_many_arguments)]
fn compose_layer(
    layer: LoadInterstitialLayer,
    bitmap: &IndexedBitmap,
    source: IndexedBitmapRect,
    destination: IndexedBitmapRect,
    extent: cmr_core::display::DisplayExtent,
    color_key: Option<ColorKey>,
    target: &mut [u8],
    active_palette: &[PaletteEntry; 256],
) -> Result<(), LoadInterstitialComposeError> {
    if destination.width <= 0 || destination.height <= 0 {
        return Ok(());
    }
    let decision = plan_indexed_bitmap_blit_for_extent(
        (bitmap.width, bitmap.height),
        source,
        destination,
        extent,
        color_key,
    )
    .map_err(|source| LoadInterstitialComposeError::Plan { layer, source })?;
    let BlitDecision::Blit(plan) = decision else {
        return Ok(());
    };
    compose_plan(layer, bitmap, plan, target, extent, active_palette)
}

fn compose_plan(
    layer: LoadInterstitialLayer,
    bitmap: &IndexedBitmap,
    plan: BlitPlan,
    target: &mut [u8],
    extent: cmr_core::display::DisplayExtent,
    active_palette: &[PaletteEntry; 256],
) -> Result<(), LoadInterstitialComposeError> {
    let source_width = positive_span(plan.source.left, plan.source.right)?;
    let source_height = positive_span(plan.source.top, plan.source.bottom)?;
    let destination_width = positive_span(plan.destination.left, plan.destination.right)?;
    let destination_height = positive_span(plan.destination.top, plan.destination.bottom)?;
    let bitmap_stride = usize::try_from(bitmap.width)
        .map_err(|_| LoadInterstitialComposeError::ArithmeticOverflow)?;
    let target_stride = usize::from(extent.width);
    let mut mapped = [None; 256];

    for destination_y in 0..destination_height {
        let source_y = center_nearest_offset(destination_y, source_height, destination_height);
        let source_row = usize::try_from(plan.source.top)
            .ok()
            .and_then(|top| top.checked_add(source_y))
            .and_then(|y| y.checked_mul(bitmap_stride))
            .ok_or(LoadInterstitialComposeError::ArithmeticOverflow)?;
        let target_row = usize::try_from(plan.destination.top)
            .ok()
            .and_then(|top| top.checked_add(destination_y))
            .and_then(|y| y.checked_mul(target_stride))
            .ok_or(LoadInterstitialComposeError::ArithmeticOverflow)?;
        for destination_x in 0..destination_width {
            let source_x = center_nearest_offset(destination_x, source_width, destination_width);
            let source_offset = usize::try_from(plan.source.left)
                .ok()
                .and_then(|left| left.checked_add(source_x))
                .and_then(|x| source_row.checked_add(x))
                .ok_or(LoadInterstitialComposeError::ArithmeticOverflow)?;
            let source_index = *bitmap
                .pixels
                .get(source_offset)
                .ok_or(LoadInterstitialComposeError::ArithmeticOverflow)?;
            if plan.color_key.is_some() && source_index == 0 {
                continue;
            }
            let mapped_index = match mapped[usize::from(source_index)] {
                Some(index) => index,
                None => {
                    let source_color = bitmap.palette.get(usize::from(source_index)).ok_or(
                        LoadInterstitialComposeError::InvalidPaletteIndex {
                            layer,
                            index: source_index,
                            entries: bitmap.palette.len(),
                        },
                    )?;
                    let index = nearest_palette_index(source_color, active_palette);
                    mapped[usize::from(source_index)] = Some(index);
                    index
                }
            };
            let target_offset = usize::try_from(plan.destination.left)
                .ok()
                .and_then(|left| left.checked_add(destination_x))
                .and_then(|x| target_row.checked_add(x))
                .ok_or(LoadInterstitialComposeError::ArithmeticOverflow)?;
            *target
                .get_mut(target_offset)
                .ok_or(LoadInterstitialComposeError::ArithmeticOverflow)? = mapped_index;
        }
    }
    Ok(())
}

fn i32_from_u32(value: u32) -> Result<i32, LoadInterstitialComposeError> {
    i32::try_from(value).map_err(|_| LoadInterstitialComposeError::ArithmeticOverflow)
}

fn positive_span(start: i32, end: i32) -> Result<usize, LoadInterstitialComposeError> {
    end.checked_sub(start)
        .filter(|span| *span > 0)
        .and_then(|span| usize::try_from(span).ok())
        .ok_or(LoadInterstitialComposeError::ArithmeticOverflow)
}

fn center_nearest_offset(destination: usize, source_len: usize, destination_len: usize) -> usize {
    ((destination as u64 * 2 + 1) * source_len as u64 / (destination_len as u64 * 2)) as usize
}

fn nearest_palette_index(source: &BitmapPaletteEntry, active: &[PaletteEntry; 256]) -> u8 {
    active
        .iter()
        .enumerate()
        .min_by_key(|(_, candidate)| {
            u16::from(source.red.abs_diff(candidate.red))
                + u16::from(source.green.abs_diff(candidate.green))
                + u16::from(source.blue.abs_diff(candidate.blue))
        })
        .map(|(index, _)| index as u8)
        .expect("the active palette always has 256 entries")
}

#[cfg(test)]
mod tests {
    use cmr_core::display::DisplayExtent;
    use cmr_formats::bitmap::{BitmapMetadata, BitmapPaletteEntry};

    use super::*;

    const EXTENT: DisplayExtent = DisplayExtent {
        width: 640,
        height: 480,
    };

    fn active_palette() -> Box<[PaletteEntry; 256]> {
        Box::new(std::array::from_fn(|index| PaletteEntry {
            red: index as u8,
            green: index as u8,
            blue: index as u8,
            flags: 0,
        }))
    }

    fn frame(index: u8) -> SoftwarePresentedFrame {
        SoftwarePresentedFrame::from_indexed(
            EXTENT,
            vec![index; 640 * 480].into_boxed_slice(),
            active_palette(),
        )
        .unwrap()
    }

    fn bitmap(width: u32, height: u32, index: u8) -> IndexedBitmap {
        let palette = (0..=index.max(1))
            .map(|value| BitmapPaletteEntry {
                red: value,
                green: value,
                blue: value,
                reserved: 0,
            })
            .collect();
        IndexedBitmap {
            width,
            height,
            source_was_top_down: true,
            palette,
            pixels: vec![index; width as usize * height as usize],
            row_stride: width,
            stored_pixels: vec![index; width as usize * height as usize],
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
    fn recovered_order_is_bar_then_line() {
        assert_eq!(
            LOADING_OVERLAY_ORDER,
            [LoadInterstitialLayer::Bar, LoadInterstitialLayer::Line]
        );
    }

    #[test]
    fn progress_zero_skips_fill_but_keeps_the_static_line() {
        let output =
            compose_load_interstitial(frame(1), &bitmap(267, 13, 10), &bitmap(8, 8, 20), 0)
                .unwrap();
        assert_eq!(output.pixels()[216 * 640 + 386], 1);
        assert_eq!(output.pixels()[205 * 640 + 386], 20);
        assert_eq!(output.pixels()[205 * 640 + 601], 20);
        assert_eq!(output.pixels()[205 * 640 + 602], 1);
    }

    #[test]
    fn completed_progress_draws_the_recovered_fill_rectangle() {
        let output =
            compose_load_interstitial(frame(1), &bitmap(267, 13, 10), &bitmap(8, 8, 20), 0x1a)
                .unwrap();
        assert_eq!(output.pixels()[216 * 640 + 385], 1);
        assert_eq!(output.pixels()[216 * 640 + 386], 10);
        assert_eq!(output.pixels()[228 * 640 + 598], 10);
        assert_eq!(output.pixels()[228 * 640 + 599], 1);
    }

    #[test]
    fn progress_above_the_recovered_stage_cap_is_rejected_directly() {
        for progress in [STAGE_LOADING_PROGRESS_CAP + 1, u8::MAX] {
            assert!(matches!(
                compose_load_interstitial(
                    frame(1),
                    &bitmap(267, 13, 10),
                    &bitmap(8, 8, 20),
                    progress,
                ),
                Err(LoadInterstitialComposeError::ProgressExceedsStageCap {
                    progress: actual,
                    cap: STAGE_LOADING_PROGRESS_CAP,
                }) if actual == progress
            ));
        }
    }

    #[test]
    fn zero_is_transparent_and_rgb_is_remapped_into_the_active_palette() {
        let transparent = compose_load_interstitial(
            frame(7),
            &bitmap(267, 13, 0),
            &bitmap(8, 8, 0),
            STAGE_LOADING_PROGRESS_CAP,
        )
        .unwrap();
        assert!(transparent.pixels().iter().all(|pixel| *pixel == 7));

        let mut bar = bitmap(267, 13, 10);
        bar.palette[10] = BitmapPaletteEntry {
            red: 200,
            green: 200,
            blue: 200,
            reserved: 0,
        };
        let remapped =
            compose_load_interstitial(frame(7), &bar, &bitmap(8, 8, 0), STAGE_LOADING_PROGRESS_CAP)
                .unwrap();
        assert_eq!(remapped.pixels()[216 * 640 + 386], 200);
    }

    #[test]
    fn line_is_painted_after_bar_and_its_one_pixel_source_stretches() {
        let output = compose_load_interstitial_with_rects(
            frame(1),
            &bitmap(267, 13, 10),
            &bitmap(8, 8, 20),
            BlitRect {
                x: 50,
                y: 60,
                w: 8,
                h: 1,
            },
            BlitRect {
                x: 50,
                y: 60,
                w: 8,
                h: 1,
            },
        )
        .unwrap();
        assert_eq!(&output.pixels()[60 * 640 + 50..60 * 640 + 58], &[20; 8]);
    }

    #[test]
    fn destination_clipping_uses_the_recovered_literal_source_shift() {
        let mut bar = bitmap(4, 1, 10);
        bar.pixels = vec![10, 11, 12, 13];
        bar.stored_pixels.clone_from(&bar.pixels);
        bar.palette = (0..=13)
            .map(|value| BitmapPaletteEntry {
                red: value,
                green: value,
                blue: value,
                reserved: 0,
            })
            .collect();
        let output = compose_load_interstitial_with_rects(
            frame(1),
            &bar,
            &bitmap(1, 1, 0),
            BlitRect {
                x: -2,
                y: 60,
                w: 4,
                h: 1,
            },
            BlitRect {
                x: 0,
                y: 70,
                w: 1,
                h: 1,
            },
        )
        .unwrap();
        assert_eq!(&output.pixels()[60 * 640..60 * 640 + 3], &[12, 13, 1]);
    }

    #[test]
    fn malformed_palette_and_line_extent_keep_layer_identity() {
        let mut bad_bar = bitmap(267, 13, 3);
        bad_bar.palette.truncate(2);
        assert!(matches!(
            compose_load_interstitial(frame(1), &bad_bar, &bitmap(8, 8, 20), 0x1a),
            Err(LoadInterstitialComposeError::InvalidPaletteIndex {
                layer: LoadInterstitialLayer::Bar,
                index: 3,
                entries: 2,
            })
        ));

        let empty_line = bitmap(0, 0, 0);
        assert!(matches!(
            compose_load_interstitial(frame(1), &bitmap(267, 13, 10), &empty_line, 0x1a),
            Err(LoadInterstitialComposeError::Plan {
                layer: LoadInterstitialLayer::Line,
                ..
            })
        ));
    }
}
