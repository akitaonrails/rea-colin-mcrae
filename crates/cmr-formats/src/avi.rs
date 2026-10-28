//! The 8-bit Microsoft Video 1 (`msvc`/`CRAM`) AVI movies the rally school
//! plays through MCI `avivideo` (`FUN_004af8a0`): a RIFF `AVI ` container
//! with one video stream, its palette in the stream format, and the frames
//! in `movi`.
//!
//! The container parse keeps only what playback needs (the frame period,
//! the size, the palette and each frame's byte range) and is pure and
//! bounds-checked. The codec is the public Video 1 format: 4x4 blocks from
//! the bottom row up, each skipped, filled with one colour, or drawn with
//! two or eight colours under a 16-bit mask; a frame updates the previous
//! one in place.

use std::fmt;

/// One parsed movie.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AviMovie {
    /// `avih.dwMicroSecPerFrame`.
    pub micros_per_frame: u32,
    pub width: u32,
    pub height: u32,
    /// The stream format's palette (`RGBQUAD`s as R, G, B).
    pub palette: Vec<[u8; 3]>,
    /// Each video frame's payload range in the source bytes.
    pub frames: Vec<std::ops::Range<usize>>,
}

#[derive(Debug, Eq, PartialEq)]
pub enum AviError {
    NotAvi,
    Truncated {
        offset: usize,
    },
    MissingHeader,
    MissingStreamFormat,
    UnsupportedCodec {
        handler: [u8; 4],
        bit_count: u16,
    },
    BadDimensions {
        width: i32,
        height: i32,
    },
    /// A frame's bitstream ran out at this offset within it.
    FrameTruncated {
        offset: usize,
    },
}

impl fmt::Display for AviError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for AviError {}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, AviError> {
    bytes
        .get(offset..offset + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or(AviError::Truncated { offset })
}

/// Parse a movie: `avih`, the first `vids` stream's `strh`/`strf`, and
/// the `##db`/`##dc` chunks of `movi` (in file order; `idx1` is not
/// needed). Bytes after the RIFF size are ignored.
pub fn parse_avi(bytes: &[u8]) -> Result<AviMovie, AviError> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"AVI " {
        return Err(AviError::NotAvi);
    }
    let end = (8 + u32_at(bytes, 4)? as usize).min(bytes.len());
    let mut movie = AviMovie {
        micros_per_frame: 0,
        width: 0,
        height: 0,
        palette: Vec::new(),
        frames: Vec::new(),
    };
    let mut header = false;
    let mut format = false;
    let mut handler = [0u8; 4];
    walk(bytes, 12, end, &mut |id, list, body| {
        match (id, list) {
            (b"avih", _) => {
                movie.micros_per_frame = u32_at(bytes, body.start)?;
                header = true;
            }
            (b"strh", _) if !format => {
                let b = bytes
                    .get(body.start..body.start + 8)
                    .ok_or(AviError::Truncated { offset: body.start })?;
                if &b[0..4] == b"vids" {
                    handler.copy_from_slice(&b[4..8]);
                }
            }
            (b"strf", _) if !format && handler != [0; 4] => {
                let width = u32_at(bytes, body.start + 4)? as i32;
                let height = u32_at(bytes, body.start + 8)? as i32;
                let bit_count = bytes
                    .get(body.start + 14..body.start + 16)
                    .map(|b| u16::from_le_bytes([b[0], b[1]]))
                    .ok_or(AviError::Truncated {
                        offset: body.start + 14,
                    })?;
                let compression =
                    bytes
                        .get(body.start + 16..body.start + 20)
                        .ok_or(AviError::Truncated {
                            offset: body.start + 16,
                        })?;
                let msvc = handler.eq_ignore_ascii_case(b"msvc")
                    || handler.eq_ignore_ascii_case(b"cram")
                    || compression.eq_ignore_ascii_case(b"CRAM")
                    || compression.eq_ignore_ascii_case(b"MSVC");
                if !msvc || bit_count != 8 {
                    return Err(AviError::UnsupportedCodec { handler, bit_count });
                }
                if width <= 0 || height <= 0 || width % 4 != 0 || height % 4 != 0 {
                    return Err(AviError::BadDimensions { width, height });
                }
                let header_size = u32_at(bytes, body.start)? as usize;
                let used = u32_at(bytes, body.start + 32)? as usize;
                let count = if used == 0 { 256 } else { used.min(256) };
                let at = body.start + header_size;
                let table = bytes
                    .get(at..at + count * 4)
                    .ok_or(AviError::Truncated { offset: at })?;
                movie.palette = table
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|q| [q[2], q[1], q[0]])
                    .collect();
                movie.width = width as u32;
                movie.height = height as u32;
                format = true;
            }
            (id, Some(b"movi")) if &id[2..4] == b"db" || &id[2..4] == b"dc" => {
                movie.frames.push(body);
            }
            _ => {}
        }
        Ok(())
    })?;
    if !header {
        return Err(AviError::MissingHeader);
    }
    if !format {
        return Err(AviError::MissingStreamFormat);
    }
    Ok(movie)
}

type Visit<'a> =
    dyn FnMut(&[u8; 4], Option<&[u8; 4]>, std::ops::Range<usize>) -> Result<(), AviError> + 'a;

/// Visit the chunks between `start` and `end`, descending into `LIST`s;
/// each chunk reports the innermost list type it sits in.
fn walk(bytes: &[u8], start: usize, end: usize, visit: &mut Visit) -> Result<(), AviError> {
    fn inner(
        bytes: &[u8],
        mut offset: usize,
        end: usize,
        list: Option<[u8; 4]>,
        visit: &mut Visit,
    ) -> Result<(), AviError> {
        while offset + 8 <= end {
            let id: [u8; 4] = bytes[offset..offset + 4].try_into().expect("four bytes");
            let size = u32_at(bytes, offset + 4)? as usize;
            let body = offset + 8;
            let body_end = body
                .checked_add(size)
                .filter(|&e| e <= end)
                .ok_or(AviError::Truncated { offset })?;
            if &id == b"LIST" {
                let kind: [u8; 4] = bytes
                    .get(body..body + 4)
                    .ok_or(AviError::Truncated { offset: body })?
                    .try_into()
                    .expect("four bytes");
                inner(bytes, body + 4, body_end, Some(kind), visit)?;
            } else {
                visit(&id, list.as_ref(), body..body_end)?;
            }
            offset = body_end + (size & 1);
        }
        Ok(())
    }
    inner(bytes, start, end, None, visit)
}

/// Decode one 8-bit Video 1 frame over the previous picture `pixels`
/// (`width * height` indices, top row first).
pub fn decode_msvideo1_8bit(
    pixels: &mut [u8],
    width: usize,
    height: usize,
    data: &[u8],
) -> Result<(), AviError> {
    let blocks_wide = width / 4;
    let blocks_high = height / 4;
    let mut total = blocks_wide * blocks_high;
    let mut at = 0usize;
    let mut take = |n: usize| -> Result<&[u8], AviError> {
        let slice = data
            .get(at..at + n)
            .ok_or(AviError::FrameTruncated { offset: at })?;
        at += n;
        Ok(slice)
    };
    let mut skip = 0usize;
    for block_y in (1..=blocks_high).rev() {
        for block_x in 0..blocks_wide {
            if skip > 0 {
                skip -= 1;
                total -= 1;
                continue;
            }
            let ab = take(2)?;
            let (a, b) = (ab[0], ab[1]);
            if a == 0 && b == 0 && total == 0 {
                return Ok(());
            }
            // The block's rows from its bottom one up.
            let mut put = |pixel_y: usize, pixel_x: usize, colour: u8| {
                let row = block_y * 4 - 1 - pixel_y;
                pixels[row * width + block_x * 4 + pixel_x] = colour;
            };
            if b & 0xfc == 0x84 {
                skip = ((usize::from(b) - 0x84) << 8) + usize::from(a) - 1;
            } else if b < 0x80 {
                let mut flags = (u16::from(b) << 8) | u16::from(a);
                let c = take(2)?;
                let colours = [c[0], c[1]];
                for pixel_y in 0..4 {
                    for pixel_x in 0..4 {
                        put(pixel_y, pixel_x, colours[usize::from((flags & 1) ^ 1)]);
                        flags >>= 1;
                    }
                }
            } else if b >= 0x90 {
                let mut flags = (u16::from(b) << 8) | u16::from(a);
                let c = take(8)?;
                for pixel_y in 0..4 {
                    for pixel_x in 0..4 {
                        let k = ((pixel_y & 2) << 1) + (pixel_x & 2) + usize::from((flags & 1) ^ 1);
                        put(pixel_y, pixel_x, c[k]);
                        flags >>= 1;
                    }
                }
            } else {
                for pixel_y in 0..4 {
                    for pixel_x in 0..4 {
                        put(pixel_y, pixel_x, a);
                    }
                }
            }
            total -= 1;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = id.to_vec();
        out.extend((body.len() as u32).to_le_bytes());
        out.extend(body);
        if body.len() % 2 == 1 {
            out.push(0);
        }
        out
    }

    fn list(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut inner = kind.to_vec();
        inner.extend(body);
        chunk(b"LIST", &inner)
    }

    fn movie(frames: &[Vec<u8>]) -> Vec<u8> {
        let mut avih = vec![0u8; 56];
        avih[0..4].copy_from_slice(&40_000u32.to_le_bytes());
        let mut strh = b"vidsmsvc".to_vec();
        strh.resize(56, 0);
        let mut strf = Vec::new();
        strf.extend(40u32.to_le_bytes());
        strf.extend(8i32.to_le_bytes());
        strf.extend(4i32.to_le_bytes());
        strf.extend(1u16.to_le_bytes());
        strf.extend(8u16.to_le_bytes());
        strf.extend(b"CRAM");
        strf.resize(32, 0);
        strf.extend(2u32.to_le_bytes());
        strf.extend(0u32.to_le_bytes());
        strf.extend([10, 20, 30, 0, 1, 2, 3, 0]);
        let strl = [chunk(b"strh", &strh), chunk(b"strf", &strf)].concat();
        let hdrl = [chunk(b"avih", &avih), list(b"strl", &strl)].concat();
        let movi: Vec<u8> = frames.iter().flat_map(|f| chunk(b"00db", f)).collect();
        let body = [b"AVI ".to_vec(), list(b"hdrl", &hdrl), list(b"movi", &movi)].concat();
        chunk(b"RIFF", &body)
    }

    #[test]
    fn the_container_yields_the_period_size_palette_and_frames() {
        let bytes = movie(&[vec![1, 2, 3], vec![4]]);
        let parsed = parse_avi(&bytes).unwrap();
        assert_eq!(parsed.micros_per_frame, 40_000);
        assert_eq!((parsed.width, parsed.height), (8, 4));
        assert_eq!(parsed.palette, [[30, 20, 10], [3, 2, 1]]);
        assert_eq!(parsed.frames.len(), 2);
        assert_eq!(&bytes[parsed.frames[0].clone()], &[1, 2, 3]);
        assert_eq!(&bytes[parsed.frames[1].clone()], &[4]);
    }

    #[test]
    fn a_truncated_or_foreign_container_is_rejected() {
        assert_eq!(parse_avi(b"RIFF\0\0\0\0WAVE"), Err(AviError::NotAvi));
        let mut bytes = movie(&[vec![1, 2, 3]]);
        let cut = bytes.len() - 3;
        bytes[4..8].copy_from_slice(&((cut - 8) as u32).to_le_bytes());
        bytes.truncate(cut);
        assert!(matches!(parse_avi(&bytes), Err(AviError::Truncated { .. })));
    }

    #[test]
    fn blocks_fill_skip_and_draw_two_and_eight_colours() {
        // An 8x4 frame: two blocks.
        let mut pixels = vec![9u8; 32];
        // Block 0: one colour 5. Block 1: two colours, mask with bit 0 set
        // (bottom-left pixel takes colour[0]).
        let frame = [5, 0x80, 0x01, 0x00, 7, 8];
        decode_msvideo1_8bit(&mut pixels, 8, 4, &frame).unwrap();
        assert!((0..4).all(|y| (0..4).all(|x| pixels[y * 8 + x] == 5)));
        assert_eq!(pixels[3 * 8 + 4], 7, "the bottom-left pixel of block 1");
        assert_eq!(pixels[3 * 8 + 5], 8);
        assert_eq!(pixels[4], 8, "its top row");
        // Skip both blocks: the picture stays.
        let before = pixels.clone();
        decode_msvideo1_8bit(&mut pixels, 8, 4, &[2, 0x84]).unwrap();
        assert_eq!(pixels, before);
        // Eight colours: quadrant pairs by the mask bit.
        let mut pixels = vec![0u8; 16];
        let frame = [0xff, 0xff, 1, 2, 3, 4, 5, 6, 7, 8];
        decode_msvideo1_8bit(&mut pixels, 4, 4, &frame).unwrap();
        assert_eq!(pixels[3 * 4], 1, "bottom-left quadrant, mask set");
        assert_eq!(pixels[3 * 4 + 2], 3, "bottom-right quadrant");
        assert_eq!(pixels[0], 5, "top-left quadrant");
        assert_eq!(pixels[2], 7, "top-right quadrant");
    }

    #[test]
    fn a_short_frame_reports_where_it_ran_out() {
        let mut pixels = vec![0u8; 16];
        assert_eq!(
            decode_msvideo1_8bit(&mut pixels, 4, 4, &[0x00, 0x10, 7]),
            Err(AviError::FrameTruncated { offset: 2 })
        );
    }
}
