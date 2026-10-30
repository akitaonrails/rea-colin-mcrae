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

#[cfg(test)]
mod tests {
    use super::*;

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
