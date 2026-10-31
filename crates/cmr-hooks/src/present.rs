//! The front end's picture, enlarged to the screen.
//!
//! The menus and intro screens are drawn at 640x480 and can't be drawn any
//! larger. With a race resolution set, the front end runs in that screen
//! mode (F16) while the game keeps drawing its 640x480 picture into the top
//! left of its back buffer; each frame is copied to the screen here, enlarged
//! with sharp nearest-neighbour pixels into a centred 4:3 area.

/// Where the picture goes on a `w` x `h` screen: the largest centred 4:3
/// area (x, y, width, height).
pub fn target(w: u32, h: u32) -> (u32, u32, u32, u32) {
    let (vw, vh) = (640u64, 480u64);
    let (w64, h64) = (w as u64, h as u64);
    // Fit by height, or by width on screens narrower than 4:3.
    let (tw, th) = if w64 * vh >= h64 * vw {
        (h64 * vw / vh, h64)
    } else {
        (w64, w64 * vh / vw)
    };
    (
        ((w64 - tw) / 2) as u32,
        ((h64 - th) / 2) as u32,
        tw as u32,
        th as u32,
    )
}

/// The source pixel for destination pixel `i` of `n` when enlarging `src`
/// pixels (nearest neighbour, sampling pixel centres).
pub fn source(i: u32, n: u32, src: u32) -> u32 {
    (((2 * i as u64 + 1) * src as u64) / (2 * n as u64)).min(src as u64 - 1) as u32
}

/// A 16-bit source pixel as 0x00RRGGBB; `green6` for RGB565, else RGB555.
pub fn to_rgb32(p: u16, green6: bool) -> u32 {
    let p = p as u32;
    let (r, g, b) = if green6 {
        ((p >> 11) & 0x1f, (p >> 5) & 0x3f, p & 0x1f)
    } else {
        ((p >> 10) & 0x1f, (p >> 5) & 0x1f, p & 0x1f)
    };
    let r = (r << 3) | (r >> 2);
    let g = if green6 {
        (g << 2) | (g >> 4)
    } else {
        (g << 3) | (g >> 2)
    };
    let b = (b << 3) | (b >> 2);
    (r << 16) | (g << 8) | b
}

/// The picture's source rectangle (top-left 640x480 of the back buffer).
pub const SRC_W: u32 = 640;
pub const SRC_H: u32 = 480;

/// Copies the 640x480 16-bit picture (`src`, `src_pitch` bytes per row,
/// `green6` = RGB565) into a `w` x `h` screen of `dst_bits` (16 or 32) bits
/// per pixel (`dst`, `dst_pitch`), enlarged with nearest-neighbour pixels
/// and centred, the sides black. A 16-bit screen gets the pixels unchanged.
///
/// # Safety
/// `src` must hold 480 rows of `src_pitch` bytes of 16-bit pixels and `dst`
/// `h` rows of `dst_pitch` bytes of `dst_bits`-bit pixels.
#[allow(clippy::too_many_arguments)]
pub unsafe fn blit(
    src: *const u8,
    src_pitch: usize,
    green6: bool,
    dst: *mut u8,
    dst_pitch: usize,
    dst_bits: u32,
    w: u32,
    h: u32,
) {
    let (x0, y0, tw, th) = target(w, h);
    let px = dst_bits as usize / 8;
    let (left, picture) = (x0 as usize * px, tw as usize * px);
    for y in 0..h {
        let row = unsafe { dst.add(y as usize * dst_pitch) };
        if y < y0 || y >= y0 + th {
            unsafe { core::ptr::write_bytes(row, 0, w as usize * px) };
        } else {
            unsafe {
                core::ptr::write_bytes(row, 0, left);
                core::ptr::write_bytes(
                    row.add(left + picture),
                    0,
                    w as usize * px - left - picture,
                );
            }
        }
    }
    let pic = unsafe { dst.add(y0 as usize * dst_pitch + left) };
    unsafe { blit_picture(src, src_pitch, green6, pic, dst_pitch, dst_bits, tw, th) };
}

/// The picture part of `blit`: the 640x480 picture enlarged to `tw` x `th`
/// pixels at `dst` (the picture's top left), nothing outside it.
///
/// # Safety
/// As `blit`, with `dst` holding `th` rows of at least `tw` pixels.
#[allow(clippy::too_many_arguments)]
pub unsafe fn blit_picture(
    src: *const u8,
    src_pitch: usize,
    green6: bool,
    dst: *mut u8,
    dst_pitch: usize,
    dst_bits: u32,
    tw: u32,
    th: u32,
) {
    // The source column of every picture column, computed once per frame
    // (the same for all rows); screens are at most 7680 wide.
    let mut cols = [0u16; 7680];
    let tw = tw.min(cols.len() as u32);
    for (i, c) in cols.iter_mut().enumerate().take(tw as usize) {
        *c = source(i as u32, tw, SRC_W) as u16;
    }
    let picture = tw as usize * (dst_bits as usize / 8);
    let mut last: Option<(u32, *const u8)> = None;
    for y in 0..th {
        let pic = unsafe { dst.add(y as usize * dst_pitch) };
        let sy = source(y, th, SRC_H);
        // Rows enlarged from the same source row are copies of the first.
        if let Some((prev_sy, prev)) = last
            && prev_sy == sy
        {
            unsafe { core::ptr::copy_nonoverlapping(prev, pic, picture) };
            continue;
        }
        let srow = unsafe { src.add(sy as usize * src_pitch) } as *const u16;
        if dst_bits == 32 {
            let pic = pic as *mut u32;
            for (x, &c) in cols[..tw as usize].iter().enumerate() {
                unsafe { *pic.add(x) = to_rgb32(*srow.add(c as usize), green6) };
            }
        } else {
            let pic = pic as *mut u16;
            for (x, &c) in cols[..tw as usize].iter().enumerate() {
                unsafe { *pic.add(x) = *srow.add(c as usize) };
            }
        }
        last = Some((sy, pic as *const u8));
    }
}

/// As `blit`, with each picture pixel from `sample(fx, fy)`, the colour
/// at that point of the 640x480 picture (fractions: the screen pixel's
/// centre), for pictures composed at the screen's own size
/// (`fe_compose`).
///
/// # Safety
/// `dst` must hold `h` rows of `dst_pitch` bytes of `dst_bits`-bit pixels.
#[allow(clippy::too_many_arguments)]
pub unsafe fn blit_sampled(
    sample: impl Fn(f32, f32) -> u16,
    green6: bool,
    dst: *mut u8,
    dst_pitch: usize,
    dst_bits: u32,
    w: u32,
    h: u32,
) {
    let (x0, y0, tw, th) = target(w, h);
    let px = dst_bits as usize / 8;
    for y in 0..h {
        let row = unsafe { dst.add(y as usize * dst_pitch) };
        unsafe { core::ptr::write_bytes(row, 0, w as usize * px) };
        if y < y0 || y >= y0 + th {
            continue;
        }
        let fy = ((y - y0) as f32 + 0.5) * SRC_H as f32 / th as f32;
        for x in 0..tw {
            let fx = (x as f32 + 0.5) * SRC_W as f32 / tw as f32;
            let v = sample(fx, fy);
            let at = (x0 + x) as usize;
            unsafe {
                if dst_bits == 32 {
                    *(row as *mut u32).add(at) = to_rgb32(v, green6);
                } else {
                    *(row as *mut u16).add(at) = v;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_are_centred_four_by_three() {
        assert_eq!(target(1920, 1080), (240, 0, 1440, 1080));
        assert_eq!(target(3840, 2160), (480, 0, 2880, 2160));
        assert_eq!(target(1440, 1080), (0, 0, 1440, 1080));
        assert_eq!(target(1280, 1024), (0, 32, 1280, 960));
    }

    #[test]
    fn nearest_neighbour_repeats_pixels_evenly() {
        // 640 -> 1920 (3x): every source pixel three times.
        let xs: [u32; 6] = core::array::from_fn(|i| source(i as u32, 1920, 640));
        assert_eq!(xs, [0, 0, 0, 1, 1, 1]);
        assert_eq!(source(1919, 1920, 640), 639);
        assert_eq!(source(0, 640, 640), 0);
        assert_eq!(source(639, 640, 640), 639);
    }

    #[test]
    fn converts_565_and_555_to_rgb32() {
        assert_eq!(to_rgb32(0xffff, true), 0xffffff);
        assert_eq!(to_rgb32(0xf800, true), 0xff0000);
        assert_eq!(to_rgb32(0x07e0, true), 0x00ff00);
        assert_eq!(to_rgb32(0x001f, true), 0x0000ff);
        assert_eq!(to_rgb32(0x7c00, false), 0xff0000);
        assert_eq!(to_rgb32(0x03e0, false), 0x00ff00);
        assert_eq!(to_rgb32(0, true), 0);
    }

    #[test]
    fn blit_to_32_bit_converts() {
        let src = vec![0xf800u16; 640 * 480];
        let mut dst = vec![1u32; 1440 * 1080];
        unsafe {
            blit(
                src.as_ptr().cast(),
                640 * 2,
                true,
                dst.as_mut_ptr().cast(),
                1440 * 4,
                32,
                1440,
                1080,
            );
        }
        assert!(dst.iter().all(|&p| p == 0xff0000));
    }

    #[test]
    fn repeated_rows_match_the_per_pixel_result() {
        // Every screen pixel equals the nearest source pixel, at a
        // non-integer scale (4K: 4.5x) and on a narrow screen (bars on top).
        let src: Vec<u16> = (0..640 * 480).map(|i| (i * 7 % 65521) as u16).collect();
        for (w, h) in [(3840u32, 2160u32), (1280, 1024)] {
            let mut dst = vec![0xaaaau16; (w * h) as usize];
            unsafe {
                blit(
                    src.as_ptr().cast(),
                    640 * 2,
                    true,
                    dst.as_mut_ptr().cast(),
                    w as usize * 2,
                    16,
                    w,
                    h,
                );
            }
            let (x0, y0, tw, th) = target(w, h);
            for y in 0..h {
                for x in 0..w {
                    let want = if x >= x0 && x < x0 + tw && y >= y0 && y < y0 + th {
                        src[(source(y - y0, th, SRC_H) * 640 + source(x - x0, tw, SRC_W)) as usize]
                    } else {
                        0
                    };
                    assert_eq!(dst[(y * w + x) as usize], want, "{w}x{h} at {x},{y}");
                }
            }
        }
    }

    #[test]
    fn blit_fills_the_picture_and_black_sides() {
        let src: Vec<u16> = (0..640 * 480).map(|i| ((i % 640) as u16) | 1).collect();
        let (w, h) = (1920u32, 1080u32);
        let mut dst = vec![0xffffu16; 1920 * 1080];
        unsafe {
            blit(
                src.as_ptr().cast(),
                640 * 2,
                true,
                dst.as_mut_ptr().cast(),
                1920 * 2,
                16,
                w,
                h,
            );
        }
        assert_eq!(dst[0], 0, "left bar black");
        assert_eq!(dst[1919], 0, "right bar black");
        assert_eq!(dst[240], 1, "first picture pixel");
        assert_eq!(dst[240 + 1439], 639 | 1, "last picture pixel");
    }
}
