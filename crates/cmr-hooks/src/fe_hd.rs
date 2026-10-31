//! HD images in the front end (F27): the menus are drawn as one 640x480
//! picture, enlarged to the screen each frame (`present`). Blits from an
//! image with an HD twin are noted as the frame is drawn; after the
//! enlargement each twin is drawn again over its spot at the screen's size.
//! A twin only stays if nothing drawn later in the frame overlaps it (a
//! cursor, text) and the finished picture still shows the original there
//! (things the game draws without the blit, like the menu's globe), so the
//! picture's order is never broken: those spots keep the enlarged original.

/// A rectangle: x, y, width, height.
pub type Rect = [i32; 4];

fn overlaps(a: Rect, b: Rect) -> bool {
    a[0] < b[0] + b[2] && b[0] < a[0] + a[2] && a[1] < b[1] + b[3] && b[1] < a[1] + a[3]
}

/// One blit of the frame: where it went, and whether it has a twin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Draw {
    pub dst: Rect,
    pub twin: bool,
}

/// For each draw, whether its twin is shown: it has one, and no later draw
/// without a twin overlaps it (later twins are drawn after it, in order).
pub fn shown(draws: &[Draw]) -> impl Iterator<Item = bool> + '_ {
    draws.iter().enumerate().map(|(i, d)| {
        d.twin
            && !draws[i + 1..]
                .iter()
                .any(|later| !later.twin && overlaps(later.dst, d.dst))
    })
}

/// Whether the finished `picture` (640 pixels a row) still shows a blit's
/// image at `dst`: each opaque pixel of the image (`sample(x, y)` within
/// `src`; `key` the transparent value), at the spot it was drawn to, nearest
/// neighbour for stretched blits. A few differing pixels are allowed (2%),
/// for a stretch rounded differently by the driver.
pub fn intact(
    picture: &[u16],
    dst: Rect,
    src: Rect,
    sample: impl Fn(i32, i32) -> u16,
    key: Option<u16>,
) -> bool {
    let (mut seen, mut differ) = (0u32, 0u32);
    for y in 0..dst[3] {
        let sy = src[1] + y * src[3] / dst[3];
        for x in 0..dst[2] {
            let px = sample(src[0] + x * src[2] / dst[2], sy);
            if key == Some(px) {
                continue;
            }
            let at = ((dst[1] + y) * 640 + dst[0] + x) as usize;
            seen += 1;
            if picture.get(at) != Some(&px) {
                differ += 1;
            }
        }
    }
    differ * 50 <= seen
}

/// A blit clipped to a `w` x `h` destination, as the game's blit does, with
/// the source cut by the same fraction (stretched blits included). `None`
/// when nothing is left.
pub fn clip(dst: Rect, src: Rect, w: i32, h: i32) -> Option<(Rect, Rect)> {
    if dst[2] <= 0 || dst[3] <= 0 || src[2] <= 0 || src[3] <= 0 {
        return None;
    }
    let (x0, y0) = (dst[0].max(0), dst[1].max(0));
    let (x1, y1) = ((dst[0] + dst[2]).min(w), (dst[1] + dst[3]).min(h));
    if x0 >= x1 || y0 >= y1 {
        return None;
    }
    // Source edges at the same fractions of the destination.
    let along = |d0: i32, dlen: i32, s0: i32, slen: i32, v: i32| {
        s0 + ((v - d0) as i64 * slen as i64 / dlen as i64) as i32
    };
    let sx0 = along(dst[0], dst[2], src[0], src[2], x0);
    let sx1 = along(dst[0], dst[2], src[0], src[2], x1);
    let sy0 = along(dst[1], dst[3], src[1], src[3], y0);
    let sy1 = along(dst[1], dst[3], src[1], src[3], y1);
    (sx1 > sx0 && sy1 > sy0)
        .then_some(([x0, y0, x1 - x0, y1 - y0], [sx0, sy0, sx1 - sx0, sy1 - sy0]))
}

/// A rectangle of an image `from` (w, h) in pixels of its twin `to`.
pub fn scale(r: Rect, from: (i32, i32), to: (i32, i32)) -> Rect {
    let x = |v: i32| (v as i64 * to.0 as i64 / from.0.max(1) as i64) as i32;
    let y = |v: i32| (v as i64 * to.1 as i64 / from.1.max(1) as i64) as i32;
    [
        x(r[0]),
        y(r[1]),
        x(r[0] + r[2]) - x(r[0]),
        y(r[1] + r[3]) - y(r[1]),
    ]
}

/// A picture rectangle on the screen, where `target` (x, y, w, h) holds
/// the enlarged picture: left, top, right, bottom (a Win32 `RECT`).
pub fn to_screen(r: Rect, target: (u32, u32, u32, u32)) -> [i32; 4] {
    let (tx, ty, tw, th) = (
        target.0 as i64,
        target.1 as i64,
        target.2 as i64,
        target.3 as i64,
    );
    let x = |v: i32| (tx + v as i64 * tw / 640) as i32;
    let y = |v: i32| (ty + v as i64 * th / 480) as i32;
    [x(r[0]), y(r[1]), x(r[0] + r[2]), y(r[1] + r[3])]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn later_plain_draws_hide_the_twins_they_overlap() {
        let d = |dst: Rect, twin| Draw { dst, twin };
        let draws = [
            d([0, 0, 640, 480], false),  // backdrop, before: fine
            d([10, 10, 100, 50], true),  // logo
            d([300, 10, 100, 50], true), // another logo
            d([20, 20, 10, 10], false),  // a cursor over the first logo
            d([310, 20, 50, 20], true),  // a twin over the second: in order
        ];
        let got: Vec<bool> = shown(&draws).collect();
        assert_eq!(got, [false, false, true, false, true]);
    }

    #[test]
    fn a_covered_image_is_not_intact() {
        let mut picture = vec![0u16; 640 * 480];
        // A 4x4 image of 7s with a transparent (0) corner, drawn at 10,10.
        let sample = |x: i32, y: i32| if x == 0 && y == 0 { 0 } else { 7 };
        for y in 10..14 {
            for x in 10..14 {
                picture[y * 640 + x] = 7;
            }
        }
        picture[10 * 640 + 10] = 99; // under the transparent corner: fine
        assert!(intact(
            &picture,
            [10, 10, 4, 4],
            [0, 0, 4, 4],
            sample,
            Some(0)
        ));
        picture[12 * 640 + 12] = 5; // something drew over it
        assert!(!intact(
            &picture,
            [10, 10, 4, 4],
            [0, 0, 4, 4],
            sample,
            Some(0)
        ));
    }

    #[test]
    fn clipping_cuts_the_source_by_the_same_fraction() {
        // A 2x stretched blit half off the left edge.
        let (d, s) = clip([-20, 10, 40, 20], [0, 0, 20, 10], 640, 480).unwrap();
        assert_eq!(d, [0, 10, 20, 20]);
        assert_eq!(s, [10, 0, 10, 10]);
        assert_eq!(clip([700, 0, 10, 10], [0, 0, 10, 10], 640, 480), None);
        assert_eq!(
            clip([0, 0, 10, 10], [0, 0, 10, 10], 640, 480).unwrap().1,
            [0, 0, 10, 10]
        );
    }

    #[test]
    fn twin_rectangles_and_the_screen() {
        assert_eq!(
            scale([10, 4, 30, 8], (182, 128), (728, 512)),
            [40, 16, 120, 32]
        );
        // 4K: the picture fills 2880x2160 at x=480.
        let t = crate::present::target(3840, 2160);
        assert_eq!(to_screen([0, 0, 640, 480], t), [480, 0, 3360, 2160]);
        assert_eq!(to_screen([320, 240, 10, 10], t), [1920, 1080, 1965, 1125]);
    }
}
