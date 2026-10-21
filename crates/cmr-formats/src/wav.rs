//! Minimal PCM `WAVE` (RIFF) decoder for the game's sound assets.
//!
//! The game's waves are PCM mono/stereo at 8 or 16 bits (see the recovered
//! loaders, e.g. `0x4958e0` building a PCM `WAVEFORMATEX`). This parser reads
//! the `fmt ` and `data` chunks, decodes the samples to interleaved `i16`, and
//! retains every other chunk uninterpreted. It is pure and bounds-checked, and
//! rejects malformed or truncated input rather than coercing it, matching the
//! sibling [`crate::riff_palette`] parser.

use std::fmt;

const RIFF_MAGIC: &[u8; 4] = b"RIFF";
const WAVE_FORM: &[u8; 4] = b"WAVE";
const FMT_CHUNK: &[u8; 4] = b"fmt ";
const DATA_CHUNK: &[u8; 4] = b"data";

/// PCM format tag (`WAVE_FORMAT_PCM`), the only one the game uses.
const WAVE_FORMAT_PCM: u16 = 1;

/// The parsed `WAVEFORMATEX` header fields (common block).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WavFormat {
    pub format_tag: u16,
    pub channels: u16,
    pub sample_rate: u32,
    pub avg_bytes_per_sec: u32,
    pub block_align: u16,
    pub bits_per_sample: u16,
}

/// A RIFF chunk retained without interpretation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WavChunk {
    pub id: [u8; 4],
    pub data: Vec<u8>,
}

/// A decoded PCM wave: the header, interleaved `i16` samples (8-bit unsigned
/// source is centred and scaled to 16-bit), and the retained ancillary chunks
/// and any bytes trailing the declared RIFF size.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WavPcm {
    pub format: WavFormat,
    /// Interleaved samples, `format.channels` per frame.
    pub samples: Vec<i16>,
    /// Non-`fmt `/`data` chunks, retained verbatim.
    pub ancillary_chunks: Vec<WavChunk>,
    /// Bytes after the declared RIFF size, retained verbatim.
    pub trailing: Vec<u8>,
}

impl WavPcm {
    /// Number of sample frames (samples divided by channel count).
    pub fn frame_count(&self) -> usize {
        self.samples.len() / self.format.channels.max(1) as usize
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum WavError {
    Truncated,
    NotRiff,
    NotWave,
    SizeMismatch { declared: u64, actual: u64 },
    TruncatedChunk { id: [u8; 4] },
    MissingFmt,
    MissingData,
    DuplicateFmt,
    DuplicateData,
    FmtTooShort { len: usize },
    UnsupportedFormatTag(u16),
    UnsupportedBitDepth(u16),
    ZeroChannels,
    ZeroSampleRate,
    BlockAlignMismatch { declared: u16, expected: u16 },
    DataNotFrameAligned { len: usize, block_align: u16 },
}

impl fmt::Display for WavError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => write!(formatter, "truncated RIFF WAVE header"),
            Self::NotRiff => write!(formatter, "file does not begin with RIFF"),
            Self::NotWave => write!(formatter, "RIFF form is not WAVE"),
            Self::SizeMismatch { declared, actual } => write!(
                formatter,
                "RIFF declares {declared} bytes but file has only {actual}"
            ),
            Self::TruncatedChunk { id } => write!(
                formatter,
                "truncated RIFF chunk {:?}",
                String::from_utf8_lossy(id)
            ),
            Self::MissingFmt => write!(formatter, "WAVE has no fmt chunk"),
            Self::MissingData => write!(formatter, "WAVE has no data chunk"),
            Self::DuplicateFmt => write!(formatter, "WAVE has multiple fmt chunks"),
            Self::DuplicateData => write!(formatter, "WAVE has multiple data chunks"),
            Self::FmtTooShort { len } => {
                write!(formatter, "fmt chunk is {len} bytes, need at least 16")
            }
            Self::UnsupportedFormatTag(tag) => {
                write!(formatter, "unsupported WAVE format tag {tag} (PCM only)")
            }
            Self::UnsupportedBitDepth(bits) => {
                write!(formatter, "unsupported bit depth {bits} (8 or 16 only)")
            }
            Self::ZeroChannels => write!(formatter, "fmt declares zero channels"),
            Self::ZeroSampleRate => write!(formatter, "fmt declares a zero sample rate"),
            Self::BlockAlignMismatch { declared, expected } => write!(
                formatter,
                "fmt block align {declared} does not match expected {expected}"
            ),
            Self::DataNotFrameAligned { len, block_align } => write!(
                formatter,
                "data length {len} is not a multiple of block align {block_align}"
            ),
        }
    }
}

impl std::error::Error for WavError {}

/// Parse a PCM `WAVE` file into decoded interleaved `i16` samples.
pub fn parse_wav_pcm(input: &[u8]) -> Result<WavPcm, WavError> {
    if input.len() < 12 {
        return Err(WavError::Truncated);
    }
    if &input[0..4] != RIFF_MAGIC {
        return Err(WavError::NotRiff);
    }
    if &input[8..12] != WAVE_FORM {
        return Err(WavError::NotWave);
    }
    let declared = u64::from(read_u32(input, 4)?) + 8;
    let actual = input.len() as u64;
    // Truncation is rejected; trailing bytes beyond the declared size are kept.
    if declared > actual {
        return Err(WavError::SizeMismatch { declared, actual });
    }
    let riff_end = declared as usize;
    let trailing = input[riff_end..].to_vec();

    let mut position = 12usize;
    let mut format = None;
    let mut data = None;
    let mut ancillary_chunks = Vec::new();
    while position < riff_end {
        if riff_end - position < 8 {
            return Err(WavError::TruncatedChunk { id: [0; 4] });
        }
        let id: [u8; 4] = input[position..position + 4]
            .try_into()
            .expect("fixed-size slice");
        let size = read_u32(input, position + 4)? as usize;
        let data_start = position + 8;
        let data_end = data_start
            .checked_add(size)
            .ok_or(WavError::TruncatedChunk { id })?;
        // RIFF chunks are word-aligned: an odd size is followed by a pad byte.
        let padded_end = data_end
            .checked_add(size & 1)
            .ok_or(WavError::TruncatedChunk { id })?;
        if padded_end > riff_end {
            return Err(WavError::TruncatedChunk { id });
        }
        let chunk = &input[data_start..data_end];

        if &id == FMT_CHUNK {
            if format.is_some() {
                return Err(WavError::DuplicateFmt);
            }
            format = Some(parse_fmt_chunk(chunk)?);
        } else if &id == DATA_CHUNK {
            if data.is_some() {
                return Err(WavError::DuplicateData);
            }
            data = Some(chunk.to_vec());
        } else {
            ancillary_chunks.push(WavChunk {
                id,
                data: chunk.to_vec(),
            });
        }
        position = padded_end;
    }

    let format = format.ok_or(WavError::MissingFmt)?;
    let data = data.ok_or(WavError::MissingData)?;
    let samples = decode_samples(&format, &data)?;
    Ok(WavPcm {
        format,
        samples,
        ancillary_chunks,
        trailing,
    })
}

fn parse_fmt_chunk(chunk: &[u8]) -> Result<WavFormat, WavError> {
    if chunk.len() < 16 {
        return Err(WavError::FmtTooShort { len: chunk.len() });
    }
    let format_tag = read_u16(chunk, 0);
    if format_tag != WAVE_FORMAT_PCM {
        return Err(WavError::UnsupportedFormatTag(format_tag));
    }
    let channels = read_u16(chunk, 2);
    if channels == 0 {
        return Err(WavError::ZeroChannels);
    }
    let sample_rate = u32::from_le_bytes(chunk[4..8].try_into().expect("fixed-size slice"));
    if sample_rate == 0 {
        return Err(WavError::ZeroSampleRate);
    }
    let avg_bytes_per_sec = u32::from_le_bytes(chunk[8..12].try_into().expect("fixed-size slice"));
    let block_align = read_u16(chunk, 12);
    let bits_per_sample = read_u16(chunk, 14);
    if bits_per_sample != 8 && bits_per_sample != 16 {
        return Err(WavError::UnsupportedBitDepth(bits_per_sample));
    }
    let expected_align = channels * (bits_per_sample / 8);
    if block_align != expected_align {
        return Err(WavError::BlockAlignMismatch {
            declared: block_align,
            expected: expected_align,
        });
    }
    Ok(WavFormat {
        format_tag,
        channels,
        sample_rate,
        avg_bytes_per_sec,
        block_align,
        bits_per_sample,
    })
}

fn decode_samples(format: &WavFormat, data: &[u8]) -> Result<Vec<i16>, WavError> {
    let block_align = format.block_align.max(1) as usize;
    if !data.len().is_multiple_of(block_align) {
        return Err(WavError::DataNotFrameAligned {
            len: data.len(),
            block_align: format.block_align,
        });
    }
    let samples = match format.bits_per_sample {
        8 => data
            // 8-bit WAV is unsigned; centre to signed and scale up to 16-bit.
            .iter()
            .map(|&byte| ((byte as i16) - 128) << 8)
            .collect(),
        16 => {
            // Data length is a multiple of block align (>= 2), so there is no
            // remainder; decode each little-endian 16-bit frame.
            let (frames, _remainder) = data.as_chunks::<2>();
            frames
                .iter()
                .map(|&pair| i16::from_le_bytes(pair))
                .collect()
        }
        other => return Err(WavError::UnsupportedBitDepth(other)),
    };
    Ok(samples)
}

fn read_u16(input: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(
        input[offset..offset + 2]
            .try_into()
            .expect("fixed-size slice"),
    )
}

fn read_u32(input: &[u8], offset: usize) -> Result<u32, WavError> {
    input
        .get(offset..offset + 4)
        .ok_or(WavError::Truncated)
        .map(|bytes| u32::from_le_bytes(bytes.try_into().expect("fixed-size slice")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(id: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut result = Vec::from(id.as_slice());
        result.extend_from_slice(&(data.len() as u32).to_le_bytes());
        result.extend_from_slice(data);
        if data.len() & 1 == 1 {
            result.push(0);
        }
        result
    }

    fn fmt_chunk(channels: u16, sample_rate: u32, bits: u16) -> Vec<u8> {
        let block_align = channels * (bits / 8);
        let avg = block_align as u32 * sample_rate;
        let mut data = Vec::new();
        data.extend_from_slice(&WAVE_FORMAT_PCM.to_le_bytes());
        data.extend_from_slice(&channels.to_le_bytes());
        data.extend_from_slice(&sample_rate.to_le_bytes());
        data.extend_from_slice(&avg.to_le_bytes());
        data.extend_from_slice(&block_align.to_le_bytes());
        data.extend_from_slice(&bits.to_le_bytes());
        data
    }

    fn wave(chunks: &[Vec<u8>], trailing: &[u8]) -> Vec<u8> {
        let mut body = Vec::from(WAVE_FORM.as_slice());
        for chunk in chunks {
            body.extend_from_slice(chunk);
        }
        let mut result = Vec::from(RIFF_MAGIC.as_slice());
        result.extend_from_slice(&(body.len() as u32).to_le_bytes());
        result.extend_from_slice(&body);
        result.extend_from_slice(trailing);
        result
    }

    #[test]
    fn parses_mono_16bit_with_ancillary_and_trailing() {
        let data: Vec<u8> = [100i16, -200, 300]
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();
        let input = wave(
            &[
                chunk(FMT_CHUNK, &fmt_chunk(1, 22_050, 16)),
                chunk(DATA_CHUNK, &data),
                chunk(b"LIST", &[1, 2, 3]),
            ],
            &[0xaa, 0xbb],
        );
        let parsed = parse_wav_pcm(&input).unwrap();
        assert_eq!(parsed.format.channels, 1);
        assert_eq!(parsed.format.sample_rate, 22_050);
        assert_eq!(parsed.format.bits_per_sample, 16);
        assert_eq!(parsed.samples, vec![100, -200, 300]);
        assert_eq!(parsed.frame_count(), 3);
        assert_eq!(parsed.ancillary_chunks[0].id, *b"LIST");
        assert_eq!(parsed.ancillary_chunks[0].data, [1, 2, 3]);
        assert_eq!(parsed.trailing, [0xaa, 0xbb]);
    }

    #[test]
    fn decodes_8bit_unsigned_centred_to_16bit() {
        // 0x80 is silence; 0xff is near +full, 0x00 near -full.
        let data = vec![0x80u8, 0xff, 0x00];
        let input = wave(
            &[
                chunk(FMT_CHUNK, &fmt_chunk(1, 11_025, 8)),
                chunk(DATA_CHUNK, &data),
            ],
            &[],
        );
        let parsed = parse_wav_pcm(&input).unwrap();
        assert_eq!(parsed.samples, vec![0, 127 << 8, -128 << 8]);
    }

    #[test]
    fn parses_stereo_frames() {
        let data: Vec<u8> = [10i16, 20, 30, 40]
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();
        let input = wave(
            &[
                chunk(FMT_CHUNK, &fmt_chunk(2, 22_050, 16)),
                chunk(DATA_CHUNK, &data),
            ],
            &[],
        );
        let parsed = parse_wav_pcm(&input).unwrap();
        assert_eq!(parsed.format.channels, 2);
        assert_eq!(parsed.frame_count(), 2);
        assert_eq!(parsed.samples, vec![10, 20, 30, 40]);
    }

    #[test]
    fn rejects_wrong_container() {
        assert_eq!(parse_wav_pcm(b"short"), Err(WavError::Truncated));
        let mut input = wave(
            &[
                chunk(FMT_CHUNK, &fmt_chunk(1, 22_050, 16)),
                chunk(DATA_CHUNK, &[0, 0]),
            ],
            &[],
        );
        input[0] = b'X';
        assert_eq!(parse_wav_pcm(&input), Err(WavError::NotRiff));
        input[0] = b'R';
        input[8..12].copy_from_slice(b"AVI ");
        assert_eq!(parse_wav_pcm(&input), Err(WavError::NotWave));
    }

    #[test]
    fn rejects_truncation_and_missing_chunks() {
        // Declared RIFF size larger than the file.
        let mut big = wave(
            &[
                chunk(FMT_CHUNK, &fmt_chunk(1, 22_050, 16)),
                chunk(DATA_CHUNK, &[0, 0]),
            ],
            &[],
        );
        big[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(
            parse_wav_pcm(&big),
            Err(WavError::SizeMismatch { .. })
        ));

        let no_fmt = wave(&[chunk(DATA_CHUNK, &[0, 0])], &[]);
        assert_eq!(parse_wav_pcm(&no_fmt), Err(WavError::MissingFmt));

        let no_data = wave(&[chunk(FMT_CHUNK, &fmt_chunk(1, 22_050, 16))], &[]);
        assert_eq!(parse_wav_pcm(&no_data), Err(WavError::MissingData));
    }

    #[test]
    fn rejects_bad_format_fields() {
        let bad_tag = {
            let mut f = fmt_chunk(1, 22_050, 16);
            f[0..2].copy_from_slice(&3u16.to_le_bytes());
            wave(&[chunk(FMT_CHUNK, &f), chunk(DATA_CHUNK, &[0, 0])], &[])
        };
        assert_eq!(
            parse_wav_pcm(&bad_tag),
            Err(WavError::UnsupportedFormatTag(3))
        );

        let bad_bits = wave(
            &[
                chunk(FMT_CHUNK, &fmt_chunk(1, 22_050, 24)),
                chunk(DATA_CHUNK, &[0, 0, 0]),
            ],
            &[],
        );
        assert_eq!(
            parse_wav_pcm(&bad_bits),
            Err(WavError::UnsupportedBitDepth(24))
        );

        let misaligned = wave(
            &[
                chunk(FMT_CHUNK, &fmt_chunk(1, 22_050, 16)),
                chunk(DATA_CHUNK, &[1, 2, 3]), // odd byte count for 16-bit mono
            ],
            &[],
        );
        assert!(matches!(
            parse_wav_pcm(&misaligned),
            Err(WavError::DataNotFrameAligned { .. })
        ));
    }

    #[test]
    fn rejects_duplicate_and_short_fmt() {
        let dup = wave(
            &[
                chunk(FMT_CHUNK, &fmt_chunk(1, 22_050, 16)),
                chunk(FMT_CHUNK, &fmt_chunk(1, 22_050, 16)),
                chunk(DATA_CHUNK, &[0, 0]),
            ],
            &[],
        );
        assert_eq!(parse_wav_pcm(&dup), Err(WavError::DuplicateFmt));

        let short = wave(
            &[chunk(FMT_CHUNK, &[1, 0, 1, 0]), chunk(DATA_CHUNK, &[0, 0])],
            &[],
        );
        assert!(matches!(
            parse_wav_pcm(&short),
            Err(WavError::FmtTooShort { .. })
        ));
    }
}
