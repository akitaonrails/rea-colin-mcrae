//! HD art (docs/UPSCALING.md): every bitmap the game loads goes through
//! `0x499ec0(bmp, flags)` with the whole BMP file in memory. For textures
//! (flags bit 1; the race HUD among them) an upscaled version made by
//! `cmr-hd` is used when there is one. Versions are found by the bitmap's
//! own bytes, not its path, so another game version never gets the wrong
//! picture: the file is `<HD folder>\<name>.bmp` with `name` below.

/// The FNV-1a hash of a bitmap file (its `bfSize` bytes), as 16 hex digits:
/// the name of its HD version.
pub fn name(bmp: &[u8]) -> [u8; 16] {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bmp {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let mut out = [0u8; 16];
    for (i, d) in out.iter_mut().enumerate() {
        let v = (h >> (60 - 4 * i)) & 0xf;
        *d = if v < 10 {
            b'0' + v as u8
        } else {
            b'a' + v as u8 - 10
        };
    }
    out
}

/// The length of a BMP file from its header (`bfSize`), when it looks like
/// one and is of a sane size.
pub fn bmp_len(header: &[u8; 6]) -> Option<usize> {
    let len = u32::from_le_bytes([header[2], header[3], header[4], header[5]]) as usize;
    (&header[..2] == b"BM" && (54..=64 << 20).contains(&len)).then_some(len)
}

/// The game's BMP reader (`0x499ec0`, the 24-bit loop at `0x499dbe`) steps
/// rows by the width rounded up to 4 *pixels* times 3 bytes, where the BMP
/// format rounds the row's *bytes* up to 4. Both agree for 8-bit images and
/// for widths that are a multiple of 4, which every image the game ships
/// has; an HD image of another width was read sheared and past its end
/// (a crash on native Windows, where a large heap block ends at a page).
///
/// For such a 24-bit, uncompressed, bottom-up image: the size of the same
/// image laid out the way the game reads it. None when it already is.
pub fn game_layout_len(bmp: &[u8]) -> Option<usize> {
    let (w, h, off) = rgb24_dims(bmp)?;
    if w % 4 == 0 {
        return None;
    }
    Some(off + game_stride(w) * h)
}

/// Copies `bmp` into `out` (`game_layout_len` bytes) with the game's row
/// stride, padding zeroed, and the header's sizes updated.
pub fn to_game_layout(bmp: &[u8], out: &mut [u8]) -> bool {
    let Some((w, h, off)) = rgb24_dims(bmp) else {
        return false;
    };
    let (from, to) = (bmp_stride(w), game_stride(w));
    if out.len() != off + to * h || bmp.len() < off + from * h {
        return false;
    }
    out[..off].copy_from_slice(&bmp[..off]);
    out[off..].fill(0);
    for y in 0..h {
        out[off + y * to..][..w * 3].copy_from_slice(&bmp[off + y * from..][..w * 3]);
    }
    let len = out.len() as u32;
    out[2..6].copy_from_slice(&len.to_le_bytes());
    // biSizeImage
    out[34..38].copy_from_slice(&((to * h) as u32).to_le_bytes());
    true
}

/// The game's texture loader (`0x499ef0`) takes its palette size from
/// `biClrUsed`, else 16 or 256 for 4- and 8-bit images, and for any other
/// depth leaves it unset (stack garbage). With a device that has palettized
/// textures (dgVoodoo2; not Wine) it then copies that many entries into a
/// 256-entry record: a 24-bit HD twin of an 8-bit texture wrote far past it
/// (the crash entering a race). Pixels are found by `bfOffBits`, so a
/// 24-bit image may say it has one palette entry: the copy becomes one
/// harmless dword and nothing else changes. Returns whether it changed.
pub fn define_palette_count(bmp: &mut [u8]) -> bool {
    if rgb24_dims(bmp).is_none() || bmp[46..50] != [0; 4] {
        return false;
    }
    bmp[46..50].copy_from_slice(&1u32.to_le_bytes());
    true
}

/// The largest texture side the HD art may have. dgVoodoo2's emulated card
/// stops at 4096 (Wine's OpenGL goes further): the x4 sky (6732x2880) came
/// out black on native Windows.
pub const MAX_SIDE: i32 = 4096;

/// Whether a BMP is wider or taller than `MAX_SIDE`.
pub fn too_large(bmp: &[u8]) -> bool {
    if bmp.len() < 26 || &bmp[..2] != b"BM" {
        return false;
    }
    let at = |o: usize| i32::from_le_bytes([bmp[o], bmp[o + 1], bmp[o + 2], bmp[o + 3]]);
    at(18).abs() > MAX_SIDE || at(22).abs() > MAX_SIDE
}

/// The size of the 8-bit version of a 24-bit BMP (`to_8bit`): header, a
/// 256-entry palette and rows padded to 4 bytes. None for other images.
pub fn to_8bit_len(bmp: &[u8]) -> Option<usize> {
    let (w, h, _) = rgb24_dims(bmp)?;
    Some(54 + 1024 + w.div_ceil(4) * 4 * h)
}

/// A 24-bit BMP made an 8-bit one with its own 256-colour palette (median
/// cut over the 15-bit colours), for a device with palettized textures:
/// the game makes 8-bit images palettized textures, which work there, and
/// has no working path for RGB ones (`cmr_hd_load`). The source rows may
/// have the BMP's stride or the game's (`to_game_layout`): the stride is
/// taken from the file's length. `hist` and `lut` are scratch space.
pub fn to_8bit(bmp: &[u8], out: &mut [u8], hist: &mut [u32; 32768], lut: &mut [u8; 32768]) -> bool {
    let Some((w, h, off)) = rgb24_dims(bmp) else {
        return false;
    };
    let Some(len) = to_8bit_len(bmp) else {
        return false;
    };
    let from = (bmp.len() - off.min(bmp.len())) / h;
    if out.len() != len || from < w * 3 {
        return false;
    }
    let bin = |p: &[u8; 3]| {
        (usize::from(p[2] >> 3) << 10) | (usize::from(p[1] >> 3) << 5) | usize::from(p[0] >> 3)
    };
    hist.fill(0);
    for y in 0..h {
        for p in bmp[off + y * from..][..w * 3].as_chunks::<3>().0 {
            hist[bin(p)] += 1;
        }
    }
    let mut palette = [[0u8; 3]; 256];
    let n = median_cut(hist, lut, &mut palette);
    // Header: the source's, as an 8-bit image with `n` colours.
    out.fill(0);
    out[..54].copy_from_slice(&bmp[..54]);
    out[2..6].copy_from_slice(&(len as u32).to_le_bytes());
    out[10..14].copy_from_slice(&(54u32 + 1024).to_le_bytes());
    out[14..18].copy_from_slice(&40u32.to_le_bytes());
    out[28..30].copy_from_slice(&8u16.to_le_bytes());
    out[30..34].copy_from_slice(&0u32.to_le_bytes());
    let to = w.div_ceil(4) * 4;
    out[34..38].copy_from_slice(&((to * h) as u32).to_le_bytes());
    out[46..50].copy_from_slice(&256u32.to_le_bytes());
    out[50..54].copy_from_slice(&0u32.to_le_bytes());
    for (i, c) in palette.iter().enumerate().take(n.max(1)) {
        // RGBQUAD: blue, green, red, 0
        out[54 + i * 4..][..3].copy_from_slice(&[c[2], c[1], c[0]]);
    }
    for y in 0..h {
        let src = &bmp[off + y * from..][..w * 3];
        let dst = &mut out[54 + 1024 + y * to..][..w];
        for (d, p) in dst.iter_mut().zip(src.as_chunks::<3>().0) {
            *d = lut[bin(p)];
        }
    }
    true
}

/// A box of 15-bit colours: per channel the inclusive 5-bit range.
#[derive(Clone, Copy)]
struct ColourBox {
    lo: [u8; 3],
    hi: [u8; 3],
}

impl ColourBox {
    fn bins(&self) -> impl Iterator<Item = (usize, [u8; 3])> + '_ {
        (self.lo[0]..=self.hi[0]).flat_map(move |r| {
            (self.lo[1]..=self.hi[1]).flat_map(move |g| {
                (self.lo[2]..=self.hi[2]).map(move |b| {
                    (
                        (usize::from(r) << 10) | (usize::from(g) << 5) | usize::from(b),
                        [r, g, b],
                    )
                })
            })
        })
    }

    /// The box shrunk to its used colours, with their count; None if empty.
    fn shrink(&self, hist: &[u32; 32768]) -> Option<(ColourBox, u64)> {
        let (mut lo, mut hi, mut count) = ([31u8; 3], [0u8; 3], 0u64);
        for (i, c) in self.bins() {
            if hist[i] != 0 {
                count += u64::from(hist[i]);
                for k in 0..3 {
                    lo[k] = lo[k].min(c[k]);
                    hi[k] = hi[k].max(c[k]);
                }
            }
        }
        (count > 0).then_some((ColourBox { lo, hi }, count))
    }
}

/// Splits the used colours of `hist` into at most 256 boxes, each split
/// across its longest side at its median; `palette` gets each box's mean
/// colour (8 bits a channel) and `lut` each colour's box. Returns the count.
fn median_cut(hist: &[u32; 32768], lut: &mut [u8; 32768], palette: &mut [[u8; 3]; 256]) -> usize {
    let mut boxes = [(
        ColourBox {
            lo: [0; 3],
            hi: [31; 3],
        },
        0u64,
    ); 256];
    let Some(first) = (ColourBox {
        lo: [0; 3],
        hi: [31; 3],
    })
    .shrink(hist) else {
        return 0;
    };
    boxes[0] = first;
    let mut n = 1;
    while n < 256 {
        // The most used box that can still be split.
        let Some(k) = (0..n)
            .filter(|&i| (0..3).any(|c| boxes[i].0.hi[c] > boxes[i].0.lo[c]))
            .max_by_key(|&i| boxes[i].1)
        else {
            break;
        };
        let (b, count) = boxes[k];
        let side = (0..3).max_by_key(|&c| b.hi[c] - b.lo[c]).unwrap_or(0);
        // The median plane along that side.
        let mut seen = 0u64;
        let mut cut = b.lo[side];
        for v in b.lo[side]..b.hi[side] {
            let mut slab = b;
            slab.lo[side] = v;
            slab.hi[side] = v;
            seen += slab.bins().map(|(i, _)| u64::from(hist[i])).sum::<u64>();
            cut = v;
            if seen * 2 >= count {
                break;
            }
        }
        let (mut low, mut high) = (b, b);
        low.hi[side] = cut;
        high.lo[side] = cut + 1;
        match (low.shrink(hist), high.shrink(hist)) {
            (Some(l), Some(h)) => {
                boxes[k] = l;
                boxes[n] = h;
                n += 1;
            }
            // Can't happen for a box with two used planes; stop rather
            // than loop.
            _ => break,
        }
    }
    for (i, (b, count)) in boxes[..n].iter().enumerate() {
        let mut sum = [0u64; 3];
        for (bin, c) in b.bins() {
            lut[bin] = i as u8;
            for k in 0..3 {
                sum[k] += u64::from(hist[bin]) * u64::from(c[k]);
            }
        }
        // 5-bit mean to 8 bits, rounded.
        palette[i] = core::array::from_fn(|k| {
            let v5 = (sum[k] * 2 + count) / (2 * count.max(&1));
            ((v5 << 3) | (v5 >> 2)) as u8
        });
    }
    n
}

fn bmp_stride(w: usize) -> usize {
    (w * 3).div_ceil(4) * 4
}

fn game_stride(w: usize) -> usize {
    w.div_ceil(4) * 4 * 3
}

/// Width, height and pixel offset of a 24-bit uncompressed bottom-up BMP.
fn rgb24_dims(bmp: &[u8]) -> Option<(usize, usize, usize)> {
    if bmp.len() < 54 || &bmp[..2] != b"BM" {
        return None;
    }
    let u32_at = |o: usize| u32::from_le_bytes([bmp[o], bmp[o + 1], bmp[o + 2], bmp[o + 3]]);
    let i32_at = |o: usize| u32_at(o) as i32;
    let (off, w, h) = (u32_at(10) as usize, i32_at(18), i32_at(22));
    let bpp = u16::from_le_bytes([bmp[28], bmp[29]]);
    let compression = u32_at(30);
    let sane = (1..=16384).contains(&w) && (1..=16384).contains(&h) && (54..=4096).contains(&off);
    (sane && bpp == 24 && compression == 0).then_some((w as usize, h as usize, off))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 24-bit BMP of `w` x `h` with the standard row padding; pixel
    /// bytes count up from 1, padding is 0xee.
    fn bmp24(w: usize, h: usize) -> std::vec::Vec<u8> {
        let stride = (w * 3).div_ceil(4) * 4;
        let mut b = std::vec![0u8; 54 + stride * h];
        b[..2].copy_from_slice(b"BM");
        let len = b.len() as u32;
        b[2..6].copy_from_slice(&len.to_le_bytes());
        b[10..14].copy_from_slice(&54u32.to_le_bytes());
        b[14..18].copy_from_slice(&40u32.to_le_bytes());
        b[18..22].copy_from_slice(&(w as u32).to_le_bytes());
        b[22..26].copy_from_slice(&(h as u32).to_le_bytes());
        b[26..28].copy_from_slice(&1u16.to_le_bytes());
        b[28..30].copy_from_slice(&24u16.to_le_bytes());
        let mut v = 1u8;
        for y in 0..h {
            for x in 0..stride {
                b[54 + y * stride + x] = if x < w * 3 {
                    v = v.wrapping_add(1);
                    v
                } else {
                    0xee
                };
            }
        }
        b
    }

    #[test]
    fn widths_the_game_reads_right_are_left_alone() {
        assert_eq!(game_layout_len(&bmp24(4, 3)), None);
        assert_eq!(game_layout_len(&bmp24(1280, 2)), None);
        let mut eight = bmp24(3, 2);
        eight[28] = 8;
        assert_eq!(game_layout_len(&eight), None, "8-bit rows agree already");
    }

    #[test]
    fn odd_widths_get_the_games_stride() {
        // 3366 wide (an HD sky): BMP rows of 10100 bytes, the game's 10104.
        let src = bmp24(3366, 3);
        let len = game_layout_len(&src).unwrap();
        assert_eq!(len, 54 + 10104 * 3);
        let mut out = std::vec![0xffu8; len];
        assert!(to_game_layout(&src, &mut out));
        for y in 0..3 {
            assert_eq!(
                out[54 + y * 10104..][..3366 * 3],
                src[54 + y * 10100..][..3366 * 3]
            );
            assert_eq!(out[54 + y * 10104 + 3366 * 3..][..6], [0; 6]);
        }
        assert_eq!(bmp_len(&out[..6].try_into().unwrap()), Some(len));
    }

    #[test]
    fn a_two_pixel_image_too() {
        let src = bmp24(2, 2); // BMP stride 8, the game's 12
        let mut out = std::vec![0u8; game_layout_len(&src).unwrap()];
        assert!(to_game_layout(&src, &mut out));
        assert_eq!(out.len(), 54 + 24);
        assert_eq!(out[54..60], src[54..60]);
        assert_eq!(out[66..72], src[62..68]);
    }

    #[test]
    fn rgb_images_get_a_one_entry_palette_count() {
        let mut b = bmp24(4, 2);
        let pixels = b[54..].to_vec();
        assert!(define_palette_count(&mut b));
        assert_eq!(b[46..50], 1u32.to_le_bytes());
        assert_eq!(b[54..], pixels[..], "pixels untouched");
        assert!(!define_palette_count(&mut b), "already set");
        let mut eight = bmp24(4, 2);
        eight[28] = 8;
        assert!(!define_palette_count(&mut eight), "8-bit keeps its palette");
        assert_eq!(eight[46..50], [0; 4]);
    }

    #[test]
    fn sides_over_the_texture_limit_are_too_large() {
        assert!(!too_large(&bmp24(4, 2)));
        let mut b = bmp24(4, 2);
        b[18..22].copy_from_slice(&6732u32.to_le_bytes());
        assert!(too_large(&b), "the x4 sky");
        b[18..22].copy_from_slice(&4096u32.to_le_bytes());
        assert!(!too_large(&b), "4096 still fits");
        b[22..26].copy_from_slice(&4097u32.to_le_bytes());
        assert!(too_large(&b));
        assert!(!too_large(b"BM"), "short input");
    }

    fn eight_bit(src: &[u8]) -> std::vec::Vec<u8> {
        let mut out = std::vec![0u8; to_8bit_len(src).unwrap()];
        let mut hist = std::boxed::Box::new([0u32; 32768]);
        let mut lut = std::boxed::Box::new([0u8; 32768]);
        assert!(to_8bit(src, &mut out, &mut hist, &mut lut));
        out
    }

    fn colour_at(out: &[u8], w: usize, x: usize, y: usize) -> [u8; 3] {
        let i = out[54 + 1024 + y * w.div_ceil(4) * 4 + x] as usize;
        let q = &out[54 + i * 4..][..3];
        [q[2], q[1], q[0]] // r, g, b
    }

    #[test]
    fn few_colours_come_through_exactly() {
        // 3x2, BGR: red, green, blue / white, black, red.
        let mut src = bmp24(3, 2);
        let px: [[u8; 3]; 6] = [
            [0, 0, 255],
            [0, 255, 0],
            [255, 0, 0],
            [255, 255, 255],
            [0, 0, 0],
            [0, 0, 255],
        ];
        for (i, p) in px.iter().enumerate() {
            let (x, y) = (i % 3, i / 3);
            src[54 + y * 12 + x * 3..][..3].copy_from_slice(p);
        }
        let out = eight_bit(&src);
        assert_eq!(out.len(), 54 + 1024 + 4 * 2);
        assert_eq!(u16::from_le_bytes([out[28], out[29]]), 8);
        assert_eq!(
            u32::from_le_bytes(out[10..14].try_into().unwrap()),
            54 + 1024
        );
        assert_eq!(bmp_len(&out[..6].try_into().unwrap()), Some(out.len()));
        assert_eq!(colour_at(&out, 3, 0, 0), [255, 0, 0]);
        assert_eq!(colour_at(&out, 3, 1, 0), [0, 255, 0]);
        assert_eq!(colour_at(&out, 3, 2, 0), [0, 0, 255]);
        assert_eq!(colour_at(&out, 3, 0, 1), [255, 255, 255]);
        assert_eq!(colour_at(&out, 3, 1, 1), [0, 0, 0]);
        assert_eq!(colour_at(&out, 3, 2, 1), [255, 0, 0]);
    }

    #[test]
    fn a_gradient_with_thousands_of_colours_stays_close() {
        // 64x64 of 4096 colours: each pixel lands within a few steps.
        let (w, h) = (64, 64);
        let mut src = bmp24(w, h);
        for y in 0..h {
            for x in 0..w {
                let p = [(x * 4) as u8, (y * 4) as u8, ((x + y) * 2) as u8];
                src[54 + y * w * 3 + x * 3..][..3].copy_from_slice(&p);
            }
        }
        let out = eight_bit(&src);
        for y in 0..h {
            for x in 0..w {
                let want = [((x + y) * 2) as i32, (y * 4) as i32, (x * 4) as i32];
                let got = colour_at(&out, w, x, y);
                for k in 0..3 {
                    assert!(
                        (got[k] as i32 - want[k]).abs() <= 40,
                        "{x},{y}: {got:?} vs {want:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn game_layout_rows_are_read_too() {
        let src = bmp24(3366, 2);
        let mut laid = std::vec![0u8; game_layout_len(&src).unwrap()];
        assert!(to_game_layout(&src, &mut laid));
        assert_eq!(eight_bit(&src), eight_bit(&laid));
    }

    #[test]
    fn a_wrong_output_size_is_refused() {
        let src = bmp24(3, 1);
        let mut out = std::vec![0u8; 10];
        assert!(!to_game_layout(&src, &mut out));
    }

    #[test]
    fn names_are_stable_hex() {
        // FNV-1a 64 of the empty input and of "a" (published test values).
        assert_eq!(&name(b""), b"cbf29ce484222325");
        assert_eq!(&name(b"a"), b"af63dc4c8601ec8c");
    }

    #[test]
    fn bitmap_lengths() {
        assert_eq!(bmp_len(b"BM\x36\x08\x00\x00"), Some(0x836));
        assert_eq!(bmp_len(b"XX\x36\x08\x00\x00"), None, "not a BMP");
        assert_eq!(bmp_len(b"BM\x10\x00\x00\x00"), None, "too short");
        assert_eq!(bmp_len(b"BM\xff\xff\xff\x7f"), None, "too long");
    }
}
