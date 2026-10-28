//! An 8-bit back surface as the 2D screens draw on it (the service screen,
//! the rally school): keyed blits at a bitmap's own size or stretched
//! (`FUN_0049f000`/`FUN_0049e3e0`, colour unused on 8 bits), colour fills of
//! the nearest palette index (`FUN_0049e350`/`FUN_004ae2f0`) and the direct
//! glyph path of `FUN_00497b40`. Bitmaps and fonts carry the load-time
//! remap of their palettes onto the live one.
//!
//! A description of disassembly, not original source. Evidence:
//! `rea.official-eu.service-screen-draw`.

use cmr_formats::bitmap::IndexedBitmap;
use cmr_formats::font::{BitmapFont, TextLayoutError};
use cmr_formats::riff_palette::PaletteEntry;

use crate::blit::{BlitDecision, BlitMethod, ColorKey, RectI16, plan_legacy_blit};

/// A loaded bitmap with the load-time remap of its palette onto the live one
/// (`FUN_00499320` at 8 bits: entries 1..255 to the nearest of the live
/// entries 1..255 by L1 RGB distance, first minimum; 0 stays 0).
#[derive(Clone, Debug)]
pub struct Sprite {
    pub bitmap: IndexedBitmap,
    map: [u8; 256],
}

fn remap_table(source: &[[u8; 3]], live: &[PaletteEntry; 256]) -> [u8; 256] {
    std::array::from_fn(|i| {
        if i == 0 {
            return 0;
        }
        let Some(&[r, g, b]) = source.get(i) else {
            return i as u8;
        };
        (1..256)
            .min_by_key(|&j| {
                let c = live[j];
                u16::from(r.abs_diff(c.red))
                    + u16::from(g.abs_diff(c.green))
                    + u16::from(b.abs_diff(c.blue))
            })
            .unwrap_or(0) as u8
    })
}

fn bitmap_rgb(bitmap: &IndexedBitmap) -> Vec<[u8; 3]> {
    bitmap
        .palette
        .iter()
        .map(|e| [e.red, e.green, e.blue])
        .collect()
}

impl Sprite {
    pub fn new(bitmap: IndexedBitmap, live: &[PaletteEntry; 256]) -> Self {
        let map = remap_table(&bitmap_rgb(&bitmap), live);
        Self { bitmap, map }
    }

    pub fn width(&self) -> i32 {
        self.bitmap.width as i32
    }

    pub fn height(&self) -> i32 {
        self.bitmap.height as i32
    }
}

/// A loaded font with the same load-time remap of its atlas (`FUN_004972d0`).
#[derive(Clone, Debug)]
pub struct CanvasFont {
    pub font: BitmapFont,
    map: [u8; 256],
}

impl CanvasFont {
    pub fn new(font: BitmapFont, live: &[PaletteEntry; 256]) -> Self {
        let map = remap_table(&bitmap_rgb(font.atlas()), live);
        Self { font, map }
    }
}

/// `FUN_004ae2f0`: the index nearest an RGB by L1 distance, strict `<`.
pub fn nearest_index(rgb: [u8; 3], palette: &[PaletteEntry; 256]) -> u8 {
    let mut best = (u16::MAX, 0u8);
    for (i, c) in palette.iter().enumerate() {
        let d = u16::from(rgb[0].abs_diff(c.red))
            + u16::from(rgb[1].abs_diff(c.green))
            + u16::from(rgb[2].abs_diff(c.blue));
        if d < best.0 {
            best = (d, i as u8);
        }
    }
    best.1
}

/// An 8-bit back surface (`ctx + 0x428`) the 2D screens draw on, with the
/// shared Q16 y cursor some screens thread between draws.
pub struct Canvas<'a> {
    pub pixels: &'a mut [u8],
    pub w: i32,
    pub h: i32,
    pub palette: &'a [PaletteEntry; 256],
    pub y: i32,
}

impl Canvas<'_> {
    /// `FUN_0049e3e0`: the recovered clip, then a keyed (index 0) or opaque
    /// copy; a stretch samples nearest-neighbour (backend policy), and a
    /// clip that inverts the source draws nothing.
    pub fn blit(&mut self, sprite: &Sprite, src: RectI16, dst: RectI16, keyed: bool) {
        let key = keyed.then_some(ColorKey { low: 0, high: 0 });
        let BlitDecision::Blit(plan) =
            plan_legacy_blit(src, dst, self.w as i16, self.h as i16, key)
        else {
            return;
        };
        let (s, d) = (plan.source, plan.destination);
        let (sw, sh) = (s.right - s.left, s.bottom - s.top);
        let (dw, dh) = (d.right - d.left, d.bottom - d.top);
        if sw <= 0 || sh <= 0 || dw <= 0 || dh <= 0 {
            return;
        }
        let bw = sprite.width();
        for row in 0..dh {
            for column in 0..dw {
                let (sx, sy) = match plan.method {
                    BlitMethod::Fast => (s.left + column, s.top + row),
                    BlitMethod::Stretch => (s.left + column * sw / dw, s.top + row * sh / dh),
                };
                if sx < 0 || sy < 0 || sx >= bw || sy >= sprite.height() {
                    continue;
                }
                let index = sprite.bitmap.pixels[(sy * bw + sx) as usize];
                if keyed && index == 0 {
                    continue;
                }
                let at = ((d.top + row) * self.w + d.left + column) as usize;
                self.pixels[at] = sprite.map[usize::from(index)];
            }
        }
    }

    /// `FUN_0049f000`: a keyed copy at the bitmap's own size, pixel x, y.
    pub fn sprite(&mut self, sprite: Option<&Sprite>, x: i32, y: i32) {
        let Some(sprite) = sprite else {
            return;
        };
        let (w, h) = (sprite.width() as i16, sprite.height() as i16);
        let src = RectI16 {
            x: 0,
            y: 0,
            width: w,
            height: h,
        };
        let dst = RectI16 {
            x: x as i16,
            y: y as i16,
            width: w,
            height: h,
        };
        self.blit(sprite, src, dst, true);
    }

    /// `FUN_0049e350`: a colour fill of the nearest index. The rectangle is
    /// not clipped; one leaving the surface is taken to fail the Blt and
    /// draw nothing (inference: DirectDraw without a clipper).
    pub fn fill(&mut self, x: i32, y: i32, w: i32, h: i32, rgb: [u8; 3]) {
        if w <= 0 || h <= 0 || x < 0 || y < 0 || x + w > self.w || y + h > self.h {
            return;
        }
        let index = nearest_index(rgb, self.palette);
        for row in y..y + h {
            let start = (row * self.w + x) as usize;
            self.pixels[start..start + w as usize].fill(index);
        }
    }

    /// `FUN_00497b40` on the direct glyph path: the text at a Q16 position;
    /// returns the Q16 y after the last line and the widest line in pixels
    /// (`DAT_00a20068`). A missing font draws nothing and keeps y.
    pub fn text(
        &mut self,
        font: Option<&CanvasFont>,
        text: &[u8],
        x: i32,
        y: i32,
        flags: u8,
    ) -> Result<(i32, i32), TextLayoutError> {
        let Some(font) = font else {
            return Ok((y, 0));
        };
        let (plan, next, _) = crate::pause_menu_draw::text_q16(
            &font.font,
            text,
            x,
            y,
            flags,
            self.w as u16,
            self.h as u16,
        )?;
        let atlas = font.font.atlas();
        let aw = atlas.width as i32;
        for blit in &plan.blits {
            for row in 0..i32::from(blit.source.height) {
                for column in 0..i32::from(blit.source.width) {
                    let (dx, dy) = (blit.destination.x + column, blit.destination.y + row);
                    if dx < 0 || dy < 0 || dx >= self.w || dy >= self.h {
                        continue;
                    }
                    let sx = i32::from(blit.source.x) + column;
                    let sy = i32::from(blit.source.y) + row;
                    let Some(&index) = atlas.pixels.get((sy * aw + sx) as usize) else {
                        continue;
                    };
                    if index != 0 {
                        self.pixels[(dy * self.w + dx) as usize] = font.map[usize::from(index)];
                    }
                }
            }
        }
        let widest = plan
            .lines
            .iter()
            .map(|line| line.measured_width)
            .max()
            .unwrap_or(0);
        Ok((next, widest))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grey() -> [PaletteEntry; 256] {
        std::array::from_fn(|i| PaletteEntry {
            red: i as u8,
            green: i as u8,
            blue: i as u8,
            flags: 0,
        })
    }

    #[test]
    fn the_fill_colour_is_the_first_nearest_index() {
        let palette = grey();
        assert_eq!(nearest_index([10, 10, 10], &palette), 10);
        let mut dup = palette;
        dup[3] = dup[7];
        assert_eq!(nearest_index([7, 7, 7], &dup), 3, "the first minimum wins");
    }

    #[test]
    fn the_remap_keeps_zero_and_matches_the_live_entries_from_one() {
        let live = grey();
        let source = vec![[5, 5, 5], [0, 0, 0], [200, 200, 200]];
        let map = remap_table(&source, &live);
        assert_eq!(map[0], 0);
        assert_eq!(map[1], 1, "black maps to the nearest of 1..255");
        assert_eq!(map[2], 200);
        assert_eq!(map[3], 3, "entries past the source palette stay");
    }

    #[test]
    fn a_fill_leaving_the_surface_draws_nothing() {
        let palette = grey();
        let mut pixels = vec![0u8; 16];
        let mut canvas = Canvas {
            pixels: &mut pixels,
            w: 4,
            h: 4,
            palette: &palette,
            y: 0,
        };
        canvas.fill(3, 0, 2, 1, [9, 9, 9]);
        canvas.fill(1, 1, 2, 2, [9, 9, 9]);
        assert_eq!(pixels, [0, 0, 0, 0, 0, 9, 9, 0, 0, 9, 9, 0, 0, 0, 0, 0]);
    }
}
