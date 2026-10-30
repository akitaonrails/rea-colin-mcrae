//! Keeps an upscaled image to the original's colours.
//!
//! Upscalers redraw as well as enlarge: on the game's art the artwork
//! model turns a one-pixel grey outline into a thin dark line, shifts flat
//! colours and invents texture. The player should see the original art,
//! only finer. Two locks, by the kind of art:
//!
//! - [`exact`], for flat palette art (the HUD): every pixel takes one of the
//!   colours the original has right there, the one the upscale is closest
//!   to. The upscale only decides where the edges between those colours
//!   run, so the colours and outlines stay exactly the artist's and the
//!   edges follow the upscaler's smooth contours.
//! - [`back_project`], for pictures and the interface: the upscale is
//!   corrected until, made as small as the original again, it gives the
//!   original back. Colours and shading are the original's; the detail the
//!   upscaler added stays.

use image::{Rgba, RgbaImage};

/// How much one source pixel of distance outweighs colour: an upscaled
/// pixel close to two candidate colours takes the nearer one in the
/// original, so thin lines the upscaler faded keep their width.
const NEARNESS: f32 = 2500.0;

/// How much a colour that is rare around the spot is held back: the art's
/// edges carry lone in-between pixels (the artist's anti-aliasing at 1x)
/// that, kept, would make a straight edge ragged. Colours that run on
/// (outlines, strokes) are common in the window and stay.
const RARITY: f32 = 8000.0;

/// How much the upscale is smoothed first (source pixels): the artwork
/// model draws slightly wavy edges along the original's staircases.
const SMOOTHING: f32 = 0.3;

/// The pixel as premultiplied colour and coverage, for comparisons.
fn premultiplied(p: Rgba<u8>) -> [f32; 4] {
    let a = f32::from(p.0[3]) / 255.0;
    [
        f32::from(p.0[0]) * a,
        f32::from(p.0[1]) * a,
        f32::from(p.0[2]) * a,
        f32::from(p.0[3]),
    ]
}

/// `up` (the original enlarged `scale` times) with each pixel replaced by
/// the best of the original's pixels around the same spot (a 4x4 window).
pub fn exact(original: &RgbaImage, up: &RgbaImage, scale: u32) -> RgbaImage {
    let (w, h) = original.dimensions();
    let s = scale as f32;
    let guide = image::imageops::blur(up, SMOOTHING * s);
    RgbaImage::from_fn(up.width(), up.height(), |x, y| {
        let want = premultiplied(*guide.get_pixel(x, y));
        // The spot in source pixels (centres at whole numbers).
        let (sx, sy) = ((x as f32 + 0.5) / s - 0.5, (y as f32 + 0.5) / s - 0.5);
        let (bx, by) = (sx.floor() as i64, sy.floor() as i64);
        let window: [(i64, i64, Rgba<u8>); 16] = core::array::from_fn(|k| {
            let (cx, cy) = (bx - 1 + k as i64 % 4, by - 1 + k as i64 / 4);
            let (ix, iy) = (cx.clamp(0, i64::from(w) - 1), cy.clamp(0, i64::from(h) - 1));
            (cx, cy, *original.get_pixel(ix as u32, iy as u32))
        });
        let mut best = (f32::MAX, Rgba([0, 0, 0, 0]));
        for &(cx, cy, candidate) in &window {
            let c = premultiplied(candidate);
            let colour: f32 = (0..4).map(|i| (c[i] - want[i]).powi(2)).sum();
            let near = (cx as f32 - sx).powi(2) + (cy as f32 - sy).powi(2);
            let common = window.iter().filter(|p| p.2 == candidate).count() as f32 / 16.0;
            let score = colour + NEARNESS * near + RARITY * (1.0 - common);
            if score < best.0 {
                best = (score, candidate);
            }
        }
        best.1
    })
}

/// `up` (the original enlarged `scale` times) corrected so that, averaged
/// back over each `scale` x `scale` block, it gives `original`. The error is
/// spread smoothly (bilinear) and the correction repeated, so no block
/// edges show.
pub fn back_project(original: &RgbaImage, up: &RgbaImage, scale: u32) -> RgbaImage {
    let (w, h) = (original.width() as usize, original.height() as usize);
    let (uw, s) = (up.width() as usize, scale as usize);
    let mut out: Vec<f32> = up.as_raw().iter().map(|&v| f32::from(v)).collect();
    let mut error = vec![0f32; w * h * 4];
    for _ in 0..8 {
        for (i, e) in error.iter_mut().enumerate() {
            let (c, x, y) = (i % 4, i / 4 % w, i / 4 / w);
            let mut sum = 0.0;
            for by in y * s..(y + 1) * s {
                for bx in x * s..(x + 1) * s {
                    sum += out[(by * uw + bx) * 4 + c];
                }
            }
            *e = f32::from(original.as_raw()[i]) - sum / (s * s) as f32;
        }
        let at = |x: isize, y: isize, c: usize| {
            let (x, y) = (
                x.clamp(0, w as isize - 1) as usize,
                y.clamp(0, h as isize - 1) as usize,
            );
            error[(y * w + x) * 4 + c]
        };
        for (i, v) in out.iter_mut().enumerate() {
            let (c, x, y) = (i % 4, i / 4 % uw, i / 4 / uw);
            // The spot in source pixels (centres at whole numbers).
            let sx = (x as f32 + 0.5) / scale as f32 - 0.5;
            let sy = (y as f32 + 0.5) / scale as f32 - 0.5;
            let (x0, y0) = (sx.floor(), sy.floor());
            let (fx, fy) = (sx - x0, sy - y0);
            let (x0, y0) = (x0 as isize, y0 as isize);
            let top = at(x0, y0, c) * (1.0 - fx) + at(x0 + 1, y0, c) * fx;
            let bottom = at(x0, y0 + 1, c) * (1.0 - fx) + at(x0 + 1, y0 + 1, c) * fx;
            *v = (*v + top * (1.0 - fy) + bottom * fy).clamp(0.0, 255.0);
        }
    }
    RgbaImage::from_raw(
        up.width(),
        up.height(),
        out.iter().map(|v| v.round() as u8).collect(),
    )
    .expect("same size")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn colours(img: &RgbaImage) -> std::collections::BTreeSet<[u8; 4]> {
        img.pixels().map(|p| p.0).collect()
    }

    #[test]
    fn exact_uses_only_the_originals_colours_and_keeps_thin_lines() {
        // A white bar with a one-pixel grey outline on transparent.
        let original = RgbaImage::from_fn(8, 5, |_, y| match y {
            1 | 3 => Rgba([120, 120, 120, 255]),
            2 => Rgba([255, 255, 255, 255]),
            _ => Rgba([0, 0, 0, 0]),
        });
        // An upscale that drew the outline as a faint dark line.
        let up = RgbaImage::from_fn(32, 20, |_, y| match y {
            5..=6 | 13..=14 => Rgba([40, 40, 40, 255]),
            7..=12 => Rgba([250, 250, 250, 255]),
            _ => Rgba([0, 0, 0, 0]),
        });
        let out = exact(&original, &up, 4);
        assert!(colours(&out).is_subset(&colours(&original)));
        assert_eq!(out.get_pixel(10, 5).0, [120, 120, 120, 255]);
        assert_eq!(out.get_pixel(10, 10).0, [255, 255, 255, 255]);
        assert_eq!(out.get_pixel(10, 1).0[3], 0);
    }

    #[test]
    fn back_projection_restores_the_originals_colours() {
        let original = RgbaImage::from_fn(8, 8, |x, _| Rgba([x as u8 * 30, 100, 50, 255]));
        // An upscale that drifted: everything a bit too bright and red.
        let up = RgbaImage::from_fn(32, 32, |x, _| Rgba([(x / 4) as u8 * 30 + 25, 115, 50, 255]));
        // Each 4x4 block averaged, as the lock measures.
        let small = |img: &RgbaImage| {
            RgbaImage::from_fn(8, 8, |x, y| {
                Rgba(core::array::from_fn(|c| {
                    let sum: u32 = (0..16)
                        .map(|k| u32::from(img.get_pixel(x * 4 + k % 4, y * 4 + k / 4).0[c]))
                        .sum();
                    ((sum + 8) / 16) as u8
                }))
            })
        };
        let drift = |img: &RgbaImage| {
            small(img)
                .as_raw()
                .iter()
                .zip(original.as_raw())
                .map(|(a, b)| (i32::from(*a) - i32::from(*b)).abs())
                .max()
                .unwrap()
        };
        assert!(drift(&up) >= 15);
        assert!(drift(&back_project(&original, &up, 4)) <= 1);
    }
}
