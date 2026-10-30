//! The colour key around upscaling: the game treats pure black as
//! transparent, which an upscaler would smear into dark halos. Black
//! becomes transparent before, and after upscaling the alpha is made hard
//! again (half way), transparent pixels pure black and opaque ones never
//! pure black, so the game's key still matches exactly.

/// RGBA pixels (4 bytes each): pure black becomes fully transparent.
pub fn to_alpha(rgba: &mut [u8]) {
    for px in rgba.as_chunks_mut::<4>().0 {
        px[3] = if px[..3] == [0, 0, 0] { 0 } else { 255 };
    }
}

/// RGBA pixels after upscaling: back to keyed, fully opaque.
pub fn to_key(rgba: &mut [u8]) {
    for px in rgba.as_chunks_mut::<4>().0 {
        if px[3] < 128 {
            px[..3].copy_from_slice(&[0, 0, 0]);
        } else if px[..3] == [0, 0, 0] {
            // Nearest colour that isn't the key.
            px[2] = 8;
        }
        px[3] = 255;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn black_round_trips_as_the_key() {
        let mut px = vec![0, 0, 0, 255, 10, 20, 30, 255];
        to_alpha(&mut px);
        assert_eq!(px, [0, 0, 0, 0, 10, 20, 30, 255]);
        // The upscaler blends: a half-covered edge and a dark opaque pixel.
        let mut out = vec![40, 40, 40, 100, 0, 0, 0, 200, 9, 9, 9, 255];
        to_key(&mut out);
        assert_eq!(out, [0, 0, 0, 255, 0, 0, 8, 255, 9, 9, 9, 255]);
    }
}
