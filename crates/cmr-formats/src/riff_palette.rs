use std::fmt;

const RIFF_MAGIC: &[u8; 4] = b"RIFF";
const PAL_FORM: &[u8; 4] = b"PAL ";
const DATA_CHUNK: &[u8; 4] = b"data";
const MAX_ENTRIES: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaletteEntry {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub flags: u8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaletteChunk {
    pub id: [u8; 4],
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RiffPalette {
    pub version: u16,
    pub entries: Vec<PaletteEntry>,
    /// Bytes in the `data` chunk after the declared entries.
    pub data_extra: Vec<u8>,
    /// Non-`data` RIFF chunks, retained without interpretation.
    pub ancillary_chunks: Vec<PaletteChunk>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LegacyPalette256 {
    pub entries: [PaletteEntry; 256],
}

#[derive(Debug, Eq, PartialEq)]
pub enum PaletteError {
    Truncated,
    NotRiff,
    NotPalette,
    SizeMismatch { declared: u64, actual: u64 },
    TruncatedChunk { id: [u8; 4] },
    MissingData,
    DuplicateData,
    DataTooShort,
    InvalidEntryCount(u16),
    UnsupportedVersion(u16),
    LegacyEntryCount(usize),
    LegacyDataOffset,
}

impl fmt::Display for PaletteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => write!(formatter, "truncated RIFF palette header"),
            Self::NotRiff => write!(formatter, "file does not begin with RIFF"),
            Self::NotPalette => write!(formatter, "RIFF form is not PAL"),
            Self::SizeMismatch { declared, actual } => {
                write!(
                    formatter,
                    "RIFF declares {declared} bytes but file has {actual}"
                )
            }
            Self::TruncatedChunk { id } => {
                write!(
                    formatter,
                    "truncated RIFF chunk {:?}",
                    String::from_utf8_lossy(id)
                )
            }
            Self::MissingData => write!(formatter, "RIFF palette has no data chunk"),
            Self::DuplicateData => write!(formatter, "RIFF palette has multiple data chunks"),
            Self::DataTooShort => write!(formatter, "palette data is shorter than its entry count"),
            Self::InvalidEntryCount(count) => {
                write!(formatter, "invalid palette entry count {count}")
            }
            Self::UnsupportedVersion(version) => {
                write!(formatter, "unsupported LOGPALETTE version {version:#06x}")
            }
            Self::LegacyEntryCount(count) => {
                write!(
                    formatter,
                    "legacy palette requires 256 entries, found {count}"
                )
            }
            Self::LegacyDataOffset => {
                write!(formatter, "legacy palette data chunk is not at offset 12")
            }
        }
    }
}

/// Parses the fixed-offset, 256-entry LOGPALETTE contract consumed by the
/// legacy startup palette loader. Chunks and bytes after the entries are
/// accepted because the recovered loader ignores them.
pub fn parse_legacy_palette_256(input: &[u8]) -> Result<LegacyPalette256, PaletteError> {
    let parsed = parse_riff_palette(input)?;
    if input.get(12..16) != Some(DATA_CHUNK.as_slice()) {
        return Err(PaletteError::LegacyDataOffset);
    }
    if parsed.version != 0x0300 {
        return Err(PaletteError::UnsupportedVersion(parsed.version));
    }
    let count = parsed.entries.len();
    let entries = parsed
        .entries
        .try_into()
        .map_err(|_| PaletteError::LegacyEntryCount(count))?;
    Ok(LegacyPalette256 { entries })
}

/// Matches the recovered palette-copy loop: update RGB while retaining each
/// destination entry's flag byte.
pub fn apply_rgb_preserving_flags(
    destination: &mut [PaletteEntry; 256],
    source: &LegacyPalette256,
) {
    for (destination, source) in destination.iter_mut().zip(source.entries) {
        destination.red = source.red;
        destination.green = source.green;
        destination.blue = source.blue;
    }
}

impl std::error::Error for PaletteError {}

pub fn parse_riff_palette(input: &[u8]) -> Result<RiffPalette, PaletteError> {
    if input.len() < 12 {
        return Err(PaletteError::Truncated);
    }
    if &input[0..4] != RIFF_MAGIC {
        return Err(PaletteError::NotRiff);
    }
    if &input[8..12] != PAL_FORM {
        return Err(PaletteError::NotPalette);
    }
    let declared = u64::from(read_u32(input, 4)?) + 8;
    let actual = input.len() as u64;
    if declared != actual {
        return Err(PaletteError::SizeMismatch { declared, actual });
    }

    let mut position = 12usize;
    let mut palette = None;
    let mut ancillary_chunks = Vec::new();
    while position < input.len() {
        if input.len() - position < 8 {
            return Err(PaletteError::TruncatedChunk { id: [0; 4] });
        }
        let id: [u8; 4] = input[position..position + 4]
            .try_into()
            .expect("fixed-size slice");
        let size = read_u32(input, position + 4)? as usize;
        let data_start = position + 8;
        let data_end = data_start
            .checked_add(size)
            .ok_or(PaletteError::TruncatedChunk { id })?;
        let padded_end = data_end
            .checked_add(size & 1)
            .ok_or(PaletteError::TruncatedChunk { id })?;
        if padded_end > input.len() {
            return Err(PaletteError::TruncatedChunk { id });
        }
        let data = &input[data_start..data_end];

        if &id == DATA_CHUNK {
            if palette.is_some() {
                return Err(PaletteError::DuplicateData);
            }
            palette = Some(parse_data_chunk(data)?);
        } else {
            ancillary_chunks.push(PaletteChunk {
                id,
                data: data.to_vec(),
            });
        }
        position = padded_end;
    }

    let (version, entries, data_extra) = palette.ok_or(PaletteError::MissingData)?;
    Ok(RiffPalette {
        version,
        entries,
        data_extra,
        ancillary_chunks,
    })
}

fn parse_data_chunk(data: &[u8]) -> Result<(u16, Vec<PaletteEntry>, Vec<u8>), PaletteError> {
    if data.len() < 4 {
        return Err(PaletteError::DataTooShort);
    }
    let version = u16::from_le_bytes(data[0..2].try_into().expect("fixed-size slice"));
    let count = u16::from_le_bytes(data[2..4].try_into().expect("fixed-size slice"));
    if count == 0 || count as usize > MAX_ENTRIES {
        return Err(PaletteError::InvalidEntryCount(count));
    }
    let entries_end = 4usize
        .checked_add(count as usize * 4)
        .ok_or(PaletteError::DataTooShort)?;
    if entries_end > data.len() {
        return Err(PaletteError::DataTooShort);
    }
    let entry_bytes = &data[4..entries_end];
    let entries = (0..count as usize)
        .map(|index| {
            let start = index * 4;
            PaletteEntry {
                red: entry_bytes[start],
                green: entry_bytes[start + 1],
                blue: entry_bytes[start + 2],
                flags: entry_bytes[start + 3],
            }
        })
        .collect();
    Ok((version, entries, data[entries_end..].to_vec()))
}

fn read_u32(input: &[u8], offset: usize) -> Result<u32, PaletteError> {
    input
        .get(offset..offset + 4)
        .ok_or(PaletteError::Truncated)
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

    fn riff(chunks: &[Vec<u8>]) -> Vec<u8> {
        let mut body = Vec::from(PAL_FORM.as_slice());
        for chunk in chunks {
            body.extend_from_slice(chunk);
        }
        let mut result = Vec::from(RIFF_MAGIC.as_slice());
        result.extend_from_slice(&(body.len() as u32).to_le_bytes());
        result.extend_from_slice(&body);
        result
    }

    fn palette_data(count: u16, entries: &[[u8; 4]], extra: &[u8]) -> Vec<u8> {
        let mut data = Vec::from(0x0300u16.to_le_bytes());
        data.extend_from_slice(&count.to_le_bytes());
        for entry in entries {
            data.extend_from_slice(entry);
        }
        data.extend_from_slice(extra);
        data
    }

    #[test]
    fn parses_entries_extra_data_and_odd_ancillary_chunk() {
        let data = palette_data(2, &[[1, 2, 3, 0], [4, 5, 6, 7]], &[9, 10]);
        let input = riff(&[chunk(DATA_CHUNK, &data), chunk(b"note", &[8, 9, 10])]);
        let parsed = parse_riff_palette(&input).unwrap();
        assert_eq!(parsed.version, 0x0300);
        assert_eq!(
            parsed.entries,
            [
                PaletteEntry {
                    red: 1,
                    green: 2,
                    blue: 3,
                    flags: 0,
                },
                PaletteEntry {
                    red: 4,
                    green: 5,
                    blue: 6,
                    flags: 7,
                }
            ]
        );
        assert_eq!(parsed.data_extra, [9, 10]);
        assert_eq!(parsed.ancillary_chunks[0].id, *b"note");
        assert_eq!(parsed.ancillary_chunks[0].data, [8, 9, 10]);
    }

    #[test]
    fn rejects_wrong_container_and_declared_size() {
        assert_eq!(parse_riff_palette(b"short"), Err(PaletteError::Truncated));
        let mut input = riff(&[chunk(DATA_CHUNK, &palette_data(1, &[[0; 4]], &[]))]);
        input[0] = b'X';
        assert_eq!(parse_riff_palette(&input), Err(PaletteError::NotRiff));
        input[0] = b'R';
        input[4..8].copy_from_slice(&0u32.to_le_bytes());
        assert!(matches!(
            parse_riff_palette(&input),
            Err(PaletteError::SizeMismatch { .. })
        ));

        let mut wrong_form = riff(&[chunk(DATA_CHUNK, &palette_data(1, &[[0; 4]], &[]))]);
        wrong_form[8..12].copy_from_slice(b"WAVE");
        assert_eq!(
            parse_riff_palette(&wrong_form),
            Err(PaletteError::NotPalette)
        );

        let mut huge = riff(&[]);
        huge[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(
            parse_riff_palette(&huge),
            Err(PaletteError::SizeMismatch {
                declared: u64::from(u32::MAX) + 8,
                actual: 12,
            })
        );
    }

    #[test]
    fn rejects_missing_duplicate_truncated_and_oversized_data() {
        assert_eq!(
            parse_riff_palette(&riff(&[])),
            Err(PaletteError::MissingData)
        );
        let data = chunk(DATA_CHUNK, &palette_data(1, &[[0; 4]], &[]));
        assert_eq!(
            parse_riff_palette(&riff(&[data.clone(), data])),
            Err(PaletteError::DuplicateData)
        );
        let truncated = riff(&[chunk(DATA_CHUNK, &palette_data(2, &[[0; 4]], &[]))]);
        assert_eq!(
            parse_riff_palette(&truncated),
            Err(PaletteError::DataTooShort)
        );
        let oversized = riff(&[chunk(DATA_CHUNK, &palette_data(257, &[], &[]))]);
        assert_eq!(
            parse_riff_palette(&oversized),
            Err(PaletteError::InvalidEntryCount(257))
        );

        let zero = riff(&[chunk(DATA_CHUNK, &palette_data(0, &[], &[]))]);
        assert_eq!(
            parse_riff_palette(&zero),
            Err(PaletteError::InvalidEntryCount(0))
        );

        let incomplete_header = riff(&[vec![b'x']]);
        assert!(matches!(
            parse_riff_palette(&incomplete_header),
            Err(PaletteError::TruncatedChunk { .. })
        ));

        let mut incomplete_data = Vec::from(DATA_CHUNK.as_slice());
        incomplete_data.extend_from_slice(&4u32.to_le_bytes());
        incomplete_data.extend_from_slice(&[1, 2]);
        assert!(matches!(
            parse_riff_palette(&riff(&[incomplete_data])),
            Err(PaletteError::TruncatedChunk { .. })
        ));

        let mut missing_padding = Vec::from(b"note".as_slice());
        missing_padding.extend_from_slice(&1u32.to_le_bytes());
        missing_padding.push(1);
        assert!(matches!(
            parse_riff_palette(&riff(&[missing_padding])),
            Err(PaletteError::TruncatedChunk { .. })
        ));
    }

    #[test]
    fn legacy_contract_requires_version_0300_and_256_entries() {
        let entries = vec![[1, 2, 3, 4]; 256];
        let valid = riff(&[chunk(DATA_CHUNK, &palette_data(256, &entries, &[9; 4]))]);
        let parsed = parse_legacy_palette_256(&valid).unwrap();
        assert_eq!(parsed.entries.len(), 256);

        let short = riff(&[chunk(DATA_CHUNK, &palette_data(255, &entries[..255], &[]))]);
        assert_eq!(
            parse_legacy_palette_256(&short),
            Err(PaletteError::LegacyEntryCount(255))
        );

        let mut wrong_version_data = palette_data(256, &entries, &[]);
        wrong_version_data[0..2].copy_from_slice(&0x0200u16.to_le_bytes());
        let wrong_version = riff(&[chunk(DATA_CHUNK, &wrong_version_data)]);
        assert_eq!(
            parse_legacy_palette_256(&wrong_version),
            Err(PaletteError::UnsupportedVersion(0x0200))
        );

        let leading_chunk = riff(&[
            chunk(b"note", &[1, 2]),
            chunk(DATA_CHUNK, &palette_data(256, &entries, &[])),
        ]);
        assert_eq!(
            parse_legacy_palette_256(&leading_chunk),
            Err(PaletteError::LegacyDataOffset)
        );
    }

    #[test]
    fn legacy_rgb_application_preserves_destination_flags() {
        let mut destination = [PaletteEntry {
            red: 0,
            green: 0,
            blue: 0,
            flags: 17,
        }; 256];
        let source = LegacyPalette256 {
            entries: [PaletteEntry {
                red: 1,
                green: 2,
                blue: 3,
                flags: 99,
            }; 256],
        };

        apply_rgb_preserving_flags(&mut destination, &source);

        assert!(destination.iter().all(|entry| {
            entry.red == 1 && entry.green == 2 && entry.blue == 3 && entry.flags == 17
        }));
    }
}
