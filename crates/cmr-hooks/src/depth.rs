//! The game's one read of the depth buffer (F26): is anything drawn in front
//! at a screen point? It used to lock the whole buffer, which under Wine
//! with a GPU copies every depth value back each frame (24 fps at 4K
//! instead of 60). The hook locks the one pixel it reads.

/// Whether a depth pixel is the far plane (nothing drawn there), for a
/// buffer of `bits` per pixel: the game's own test is a 16-bit value of
/// `0xffff`; deeper buffers are compared on their top 16 bits, as F25's
/// 24-bit one is. `None` for depths the game never handled.
pub fn is_far(bits: u32, value: u32) -> Option<bool> {
    let top16 = match bits {
        16 => value & 0xffff,
        24 => (value & 0xff_ffff) >> 8,
        32 => value >> 16,
        _ => return None,
    };
    Some(top16 == 0xffff)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn far_is_the_top_value_at_every_depth() {
        assert_eq!(is_far(16, 0xffff), Some(true));
        assert_eq!(is_far(16, 0xfffe), Some(false));
        assert_eq!(is_far(16, 0x1234_ffff), Some(true), "only the low 16 bits");
        assert_eq!(is_far(24, 0x00ff_ffff), Some(true));
        assert_eq!(
            is_far(24, 0xabff_ff00),
            Some(true),
            "the top byte is not depth"
        );
        assert_eq!(is_far(24, 0x00ff_feff), Some(false));
        assert_eq!(is_far(32, 0xffff_0000), Some(true));
        assert_eq!(is_far(8, 0xff), None);
    }
}
