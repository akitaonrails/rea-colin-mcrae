//! The game keeps 8-bit images as palette indices (with index 0 as the
//! transparent key) and makes 8-bit textures of them, so an HD version of
//! an 8-bit image must be 8-bit too, in the original's own palette: each
//! upscaled pixel takes the nearest palette colour. The smooth edges the
//! upscaler draws survive as the palette's in-between shades.

/// The original bitmap's palette (`biBitCount` 8): 256 RGB entries, or
/// `None` for other depths.
pub fn of_bmp(bmp: &[u8]) -> Option<Vec<[u8; 3]>> {
    let u16_at = |o: usize| -> Option<usize> {
        Some(usize::from(u16::from_le_bytes([
            *bmp.get(o)?,
            *bmp.get(o + 1)?,
        ])))
    };
    let u32_at = |o: usize| -> Option<usize> {
        Some(u32::from_le_bytes(bmp.get(o..o + 4)?.try_into().ok()?) as usize)
    };
    let header = u32_at(14)?;
    let bits = u16_at(28)?;
    if bits != 8 {
        return None;
    }
    let used = match u32_at(46)? {
        0 => 256,
        n => n.min(256),
    };
    let start = 14 + header;
    let mut out = Vec::with_capacity(256);
    for i in 0..256 {
        let at = start + 4 * i;
        out.push(if i < used {
            let b = bmp.get(at..at + 3)?;
            [b[2], b[1], b[0]]
        } else {
            [0, 0, 0]
        });
    }
    Some(out)
}

/// The palette index for an RGBA pixel of a keyed image: transparent (alpha
/// below half, or pure black, the key `key::to_key` leaves) is index 0;
/// anything else the nearest colour of 1..=255. Unkeyed images use all.
pub fn index(palette: &[[u8; 3]], px: [u8; 4], keyed: bool) -> u8 {
    if keyed && (px[3] < 128 || px[..3] == [0, 0, 0]) {
        return 0;
    }
    let first = usize::from(keyed);
    let dist = |c: &[u8; 3]| {
        (0..3)
            .map(|i| {
                let d = i32::from(c[i]) - i32::from(px[i]);
                d * d
            })
            .sum::<i32>()
    };
    (first..palette.len())
        .min_by_key(|&i| dist(&palette[i]))
        .unwrap_or(0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bmp8(colours: &[[u8; 3]]) -> Vec<u8> {
        let mut b = vec![0u8; 54];
        b[0..2].copy_from_slice(b"BM");
        b[14..18].copy_from_slice(&40u32.to_le_bytes());
        b[28..30].copy_from_slice(&8u16.to_le_bytes());
        b[46..50].copy_from_slice(&(colours.len() as u32).to_le_bytes());
        for c in colours {
            b.extend_from_slice(&[c[2], c[1], c[0], 0]);
        }
        b
    }

    #[test]
    fn reads_the_palette() {
        let p = of_bmp(&bmp8(&[[0, 0, 0], [255, 0, 0], [10, 20, 30]])).unwrap();
        assert_eq!(p.len(), 256);
        assert_eq!(p[1], [255, 0, 0]);
        assert_eq!(p[2], [10, 20, 30]);
        assert_eq!(p[3], [0, 0, 0], "unused entries");
        let mut b24 = bmp8(&[]);
        b24[28] = 24;
        assert_eq!(of_bmp(&b24), None);
    }

    #[test]
    fn nearest_colour_and_the_key() {
        let p = vec![[0, 0, 0], [0, 0, 0], [200, 0, 0], [255, 255, 255]];
        assert_eq!(index(&p, [190, 10, 0, 255], true), 2);
        assert_eq!(index(&p, [250, 250, 250, 255], true), 3);
        // Transparent is the key; an opaque near-black is never index 0.
        assert_eq!(index(&p, [0, 0, 0, 0], true), 0);
        assert_eq!(index(&p, [0, 0, 0, 255], true), 0, "the restored key");
        assert_eq!(index(&p, [0, 0, 8, 255], true), 1);
        // Opaque images may use index 0 like any other.
        assert_eq!(index(&p, [0, 0, 0, 255], false), 0);
    }
}
