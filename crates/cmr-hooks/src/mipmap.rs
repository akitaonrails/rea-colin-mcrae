//! Mipmaps for the game's textures (E9, F18).
//!
//! The game uploads every race texture once, without smaller levels, so far
//! away the road and scenery shimmer. F18 gives video-memory textures a
//! mipmap chain; after each upload the levels are made here by averaging
//! 2x2 blocks. Textures are 16-bit (RGB565, and ARGB4444 for a few with
//! alpha); a colour-keyed (transparent) pixel stays transparent when most
//! of its block is, and doesn't darken the edges of the rest.

/// How many levels a `w` x `h` texture gets, down to 1 pixel on its longer
/// side; 1 for sizes that aren't powers of two.
pub fn levels(w: u32, h: u32) -> u32 {
    if w == 0 || h == 0 || !w.is_power_of_two() || !h.is_power_of_two() {
        return 1;
    }
    32 - w.max(h).leading_zeros()
}

/// A 16-bit pixel layout from its masks (`a` 0 for none).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Format {
    masks: [u16; 4],
}

impl Format {
    pub fn new(r: u32, g: u32, b: u32, a: u32) -> Option<Format> {
        // A channel mask is one run of bits (or 0 for no alpha).
        let ok =
            |m: u32| m <= 0xffff && (m == 0 || ((m >> m.trailing_zeros()) + 1).is_power_of_two());
        (r != 0 && g != 0 && b != 0 && [r, g, b, a].iter().all(|&m| ok(m))).then_some(Format {
            masks: [r as u16, g as u16, b as u16, a as u16],
        })
    }

    /// The 2x2 average of four pixels, channel by channel. With `key`,
    /// pixels of that value are transparent: the result is the key when
    /// more than two of the four are, else the average of the others.
    pub fn average(&self, px: [u16; 4], key: Option<u16>) -> u16 {
        let kept: [bool; 4] = core::array::from_fn(|i| key != Some(px[i]));
        let n = kept.iter().filter(|&&k| k).count() as u32;
        if let Some(k) = key
            && n < 2
        {
            return k;
        }
        let mut out = 0u16;
        for &m in &self.masks {
            if m == 0 {
                continue;
            }
            let shift = m.trailing_zeros();
            let sum: u32 = (0..4)
                .filter(|&i| kept[i])
                .map(|i| ((px[i] & m) >> shift) as u32)
                .sum();
            // Rounded average.
            let v = (sum + n / 2) / n;
            out |= ((v << shift) as u16) & m;
        }
        // An averaged pixel must not become the key by accident.
        match key {
            Some(k) if out == k => out ^ (self.masks[2] & self.masks[2].wrapping_neg()),
            _ => out,
        }
    }
}

/// Makes the next level (`w/2` x `h/2`, at least 1x1) from a `w` x `h`
/// level. Pitches are in pixels.
///
/// # Safety
/// `src` must hold `h` rows of `src_pitch` pixels and `dst` the next
/// level's rows of `dst_pitch` pixels.
#[allow(clippy::too_many_arguments)]
pub unsafe fn downsample(
    fmt: &Format,
    key: Option<u16>,
    src: *const u16,
    src_pitch: usize,
    w: u32,
    h: u32,
    dst: *mut u16,
    dst_pitch: usize,
) {
    let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
    for y in 0..nh as usize {
        let (y0, y1) = ((2 * y).min(h as usize - 1), (2 * y + 1).min(h as usize - 1));
        for x in 0..nw as usize {
            let (x0, x1) = ((2 * x).min(w as usize - 1), (2 * x + 1).min(w as usize - 1));
            let at = |yy: usize, xx: usize| unsafe { *src.add(yy * src_pitch + xx) };
            let px = [at(y0, x0), at(y0, x1), at(y1, x0), at(y1, x1)];
            unsafe { *dst.add(y * dst_pitch + x) = fmt.average(px, key) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb565() -> Format {
        Format::new(0xf800, 0x7e0, 0x1f, 0).unwrap()
    }

    #[test]
    fn formats_need_contiguous_masks() {
        assert!(Format::new(0xf800, 0x7e0, 0x1f, 0).is_some());
        assert!(Format::new(0xf00f, 0x7e0, 0x1f, 0).is_none());
        assert!(Format::new(0, 0x7e0, 0x1f, 0).is_none());
    }

    #[test]
    fn level_counts() {
        assert_eq!(levels(64, 64), 7);
        assert_eq!(levels(256, 64), 9);
        assert_eq!(levels(4, 1), 3);
        assert_eq!(levels(1, 1), 1);
        assert_eq!(levels(48, 64), 1, "not a power of two");
    }

    #[test]
    fn averages_each_channel() {
        let f = rgb565();
        assert_eq!(f.average([0xffff; 4], None), 0xffff);
        assert_eq!(
            f.average([0xf800, 0xf800, 0, 0], None),
            0x8000,
            "red 31+31 / 4 -> 16"
        );
        let argb4444 = Format::new(0x0f00, 0x00f0, 0x000f, 0xf000).unwrap();
        assert_eq!(
            argb4444.average([0xf000, 0xf000, 0, 0], None),
            0x8000,
            "alpha too"
        );
    }

    #[test]
    fn colour_key_stays_transparent_and_doesnt_darken() {
        let f = rgb565();
        let key = Some(0x0000);
        // Mostly keyed: transparent.
        assert_eq!(f.average([0, 0, 0, 0xffff], key), 0);
        // Half keyed: the opaque half's colour, not darkened.
        assert_eq!(f.average([0, 0, 0xffff, 0xffff], key), 0xffff);
        // An average landing on the key is nudged off it.
        assert_ne!(f.average([0x0001, 0, 0x0001, 0x0000], Some(0x0001)), 0x0001);
    }

    #[test]
    fn downsamples_a_level() {
        let f = rgb565();
        // 4x2: left half white, right half black.
        let src = [0xffffu16, 0xffff, 0, 0, 0xffff, 0xffff, 0, 0];
        let mut dst = [0x1234u16; 2];
        unsafe { downsample(&f, None, src.as_ptr(), 4, 4, 2, dst.as_mut_ptr(), 2) };
        assert_eq!(dst, [0xffff, 0]);
        // 1-pixel-high levels keep going along the long side.
        let mut one = [0u16; 1];
        unsafe { downsample(&f, None, dst.as_ptr(), 2, 2, 1, one.as_mut_ptr(), 1) };
        assert_eq!(one, [f.average([0xffff, 0, 0xffff, 0], None)]);
    }
}
