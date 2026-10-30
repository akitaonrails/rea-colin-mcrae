//! Sheets of images for one upscaler run each.
//!
//! Real-ESRGAN loads its model and builds its GPU pipelines on every run
//! (about 5 s), so images can't go one per run. Its folder mode is not an
//! option either: `realesrgan-ncnn-vulkan` 20220424 on an RTX 5090 saved
//! many results under other images' names or as garbage, differently on
//! every run (one image per run was always right). So the images of a
//! model are packed into a few large sheets, each upscaled in one run, and
//! cut out again. Each image sits in a gutter of its own edge pixels, so
//! the upscaler sees what it would see at the image's border and nothing
//! of its neighbours.

use image::RgbaImage;

/// The gutter around each image, in source pixels: more than the
/// upscaler's reach (its own tiles overlap by 10).
pub const PAD: u32 = 16;

/// Where an image sits on its sheet (its own top-left, inside the gutter).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Place {
    pub index: usize,
    pub x: u32,
    pub y: u32,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Sheet {
    pub width: u32,
    pub height: u32,
    pub places: Vec<Place>,
}

/// Packs images of the given sizes into shelves (tallest first), starting a
/// sheet when one would grow past `max` on a side. An image wider or
/// taller than `max` gets a sheet as large as it needs.
pub fn pack(sizes: &[(u32, u32)], max: u32) -> Vec<Sheet> {
    let mut order: Vec<usize> = (0..sizes.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(sizes[i].1));
    let mut sheets = Vec::new();
    let mut sheet = Sheet::default();
    let (mut x, mut y, mut shelf) = (0, 0, 0);
    for i in order {
        let (w, h) = (sizes[i].0 + 2 * PAD, sizes[i].1 + 2 * PAD);
        if x > 0 && x + w > max {
            (x, y, shelf) = (0, y + shelf, 0);
        }
        if !sheet.places.is_empty() && y + h > max {
            sheets.push(std::mem::take(&mut sheet));
            (x, y, shelf) = (0, 0, 0);
        }
        sheet.places.push(Place {
            index: i,
            x: x + PAD,
            y: y + PAD,
        });
        sheet.width = sheet.width.max(x + w);
        sheet.height = sheet.height.max(y + h);
        x += w;
        shelf = shelf.max(h);
    }
    if !sheet.places.is_empty() {
        sheets.push(sheet);
    }
    sheets
}

/// Draws `img` at (`x`, `y`) with its edge pixels repeated into the gutter.
pub fn draw(sheet: &mut RgbaImage, img: &RgbaImage, x: u32, y: u32) {
    let (w, h) = img.dimensions();
    for sy in y - PAD..y + h + PAD {
        for sx in x - PAD..x + w + PAD {
            let ix = (sx + PAD).saturating_sub(x + PAD).min(w - 1);
            let iy = (sy + PAD).saturating_sub(y + PAD).min(h - 1);
            sheet.put_pixel(sx, sy, *img.get_pixel(ix, iy));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn overlaps(a: (u32, u32, u32, u32), b: (u32, u32, u32, u32)) -> bool {
        a.0 < b.0 + b.2 && b.0 < a.0 + a.2 && a.1 < b.1 + b.3 && b.1 < a.1 + a.3
    }

    #[test]
    fn packs_without_overlap_inside_the_sheet() {
        let sizes = [(32, 32), (640, 480), (16, 4), (2244, 960), (64, 16), (4, 4)];
        let sheets = pack(&sizes, 1024);
        let mut seen: Vec<usize> = sheets
            .iter()
            .flat_map(|s| s.places.iter().map(|p| p.index))
            .collect();
        seen.sort();
        assert_eq!(seen, [0, 1, 2, 3, 4, 5]);
        for s in &sheets {
            let boxes: Vec<_> = s
                .places
                .iter()
                .map(|p| {
                    let (w, h) = sizes[p.index];
                    (p.x - PAD, p.y - PAD, w + 2 * PAD, h + 2 * PAD)
                })
                .collect();
            for (i, a) in boxes.iter().enumerate() {
                assert!(a.0 + a.2 <= s.width && a.1 + a.3 <= s.height);
                for b in &boxes[i + 1..] {
                    assert!(!overlaps(*a, *b), "{a:?} {b:?}");
                }
            }
        }
        // The panorama is wider than the limit: a sheet of its own.
        assert!(sheets.iter().any(|s| s.width == 2244 + 2 * PAD));
    }

    #[test]
    fn splits_into_sheets_at_the_limit() {
        let sheets = pack(&[(200, 200); 9], 512);
        // Two 232-pixel slots per side fit in 512.
        assert_eq!(sheets.len(), 3);
        assert!(sheets.iter().all(|s| s.width <= 512 && s.height <= 512));
    }

    #[test]
    fn the_gutter_repeats_the_edges() {
        let img = RgbaImage::from_fn(2, 2, |x, y| image::Rgba([x as u8, y as u8, 7, 255]));
        let mut sheet = RgbaImage::new(2 + 2 * PAD, 2 + 2 * PAD);
        draw(&mut sheet, &img, PAD, PAD);
        assert_eq!(sheet.get_pixel(0, 0).0, [0, 0, 7, 255]);
        assert_eq!(sheet.get_pixel(PAD + 1, PAD).0, [1, 0, 7, 255]);
        assert_eq!(sheet.get_pixel(2 * PAD + 1, 2 * PAD + 1).0, [1, 1, 7, 255]);
        assert_eq!(sheet.get_pixel(PAD, 2 * PAD + 1).0, [0, 1, 7, 255]);
    }
}
