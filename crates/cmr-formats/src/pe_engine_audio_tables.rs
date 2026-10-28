//! Authenticated extraction and payload-free encoding of the static
//! engine-sound rev-band tables and the upshift whine-dip sequence.
//!
//! Observation: four rev-band records live in `.data` of the official EU
//! executable. Each record is `i32 count; i32 lo; i32 hi;` followed by
//! `u16 pitch[N]` and then `u8 vol[N]`, with `N = count + 1`; the arrays are
//! contiguous, so `pitch` starts 12 bytes after the record base and `vol`
//! starts `2 * N` bytes after `pitch`. A separate `i32[24]` sequence (the
//! upshift whine dip) follows. The tables are read-only at runtime.
//!
//! Observation: one native branch reads the Top band's `pitch[-1]`, which is
//! the high 16-bit half of that band's `hi` dword (`0x4f84ca`). It is exposed
//! by [`EngineSoundTables::top_pitch_at_minus_one`], derived from `hi` rather
//! than stored twice.
//!
//! The meaning of the values is not asserted here. No table payload is
//! compiled into this crate; tests use synthetic fixtures only.

use std::fmt;

use crate::pe32::{Pe32Error, Pe32Image};

pub const ENGINE_SLOW_BAND_ENTRIES: usize = 21;
pub const ENGINE_MID_BAND_ENTRIES: usize = 127;
pub const ENGINE_TOP_BAND_ENTRIES: usize = 127;
pub const ENGINE_WHINE_BAND_ENTRIES: usize = 194;
pub const ENGINE_UPSHIFT_DIP_ENTRIES: usize = 24;
pub const ENGINE_AUDIO_TABLES_FORMAT_VERSION: u32 = 1;
pub const ENGINE_SOUND_BAND_COUNT: usize = 4;

/// Bytes of the `count`, `lo`, `hi` header that precedes `pitch`.
const BAND_HEADER_BYTES: usize = 12;
const MAGIC: &[u8; 8] = b"CMRENGA\0";
const HEADER_LEN: usize = 16;
/// Encoded band prefix: `entries`, `count`, `lo`, `hi`.
const ENCODED_BAND_PREFIX: usize = 16;
const fn encoded_band_len(entries: usize) -> usize {
    ENCODED_BAND_PREFIX + entries * 3
}
pub const ENGINE_AUDIO_TABLES_ENCODED_LEN: usize = HEADER_LEN
    + encoded_band_len(ENGINE_SLOW_BAND_ENTRIES)
    + encoded_band_len(ENGINE_MID_BAND_ENTRIES)
    + encoded_band_len(ENGINE_TOP_BAND_ENTRIES)
    + encoded_band_len(ENGINE_WHINE_BAND_ENTRIES)
    + ENGINE_UPSHIFT_DIP_ENTRIES * 4;

/// Which rev band a record or error refers to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EngineSoundBandId {
    Slow,
    Mid,
    Top,
    Whine,
}

impl EngineSoundBandId {
    pub const ALL: [Self; ENGINE_SOUND_BAND_COUNT] =
        [Self::Slow, Self::Mid, Self::Top, Self::Whine];

    /// The fixed array length `N` of this band in the reviewed layout.
    pub const fn entries(self) -> usize {
        match self {
            Self::Slow => ENGINE_SLOW_BAND_ENTRIES,
            Self::Mid => ENGINE_MID_BAND_ENTRIES,
            Self::Top => ENGINE_TOP_BAND_ENTRIES,
            Self::Whine => ENGINE_WHINE_BAND_ENTRIES,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Slow => "slow",
            Self::Mid => "mid",
            Self::Top => "top",
            Self::Whine => "whine",
        }
    }
}

impl fmt::Display for EngineSoundBandId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Virtual addresses of one band record and its two arrays.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EngineSoundBandLayout {
    pub base_va: u32,
    pub pitch_va: u32,
    pub vol_va: u32,
    pub entries: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EngineAudioTablesLayout {
    pub image_base: u32,
    pub slow: EngineSoundBandLayout,
    pub mid: EngineSoundBandLayout,
    pub top: EngineSoundBandLayout,
    pub whine: EngineSoundBandLayout,
    pub upshift_dip_va: u32,
}

impl EngineAudioTablesLayout {
    pub fn band(&self, id: EngineSoundBandId) -> &EngineSoundBandLayout {
        match id {
            EngineSoundBandId::Slow => &self.slow,
            EngineSoundBandId::Mid => &self.mid,
            EngineSoundBandId::Top => &self.top,
            EngineSoundBandId::Whine => &self.whine,
        }
    }
}

pub const OFFICIAL_EU_ENGINE_AUDIO_TABLES_LAYOUT: EngineAudioTablesLayout =
    EngineAudioTablesLayout {
        image_base: 0x0040_0000,
        slow: EngineSoundBandLayout {
            base_va: 0x004f_82e0,
            pitch_va: 0x004f_82ec,
            vol_va: 0x004f_8316,
            entries: ENGINE_SLOW_BAND_ENTRIES as u32,
        },
        mid: EngineSoundBandLayout {
            base_va: 0x004f_8330,
            pitch_va: 0x004f_833c,
            vol_va: 0x004f_843a,
            entries: ENGINE_MID_BAND_ENTRIES as u32,
        },
        top: EngineSoundBandLayout {
            base_va: 0x004f_84c0,
            pitch_va: 0x004f_84cc,
            vol_va: 0x004f_85ca,
            entries: ENGINE_TOP_BAND_ENTRIES as u32,
        },
        whine: EngineSoundBandLayout {
            base_va: 0x004f_8650,
            pitch_va: 0x004f_865c,
            vol_va: 0x004f_87e0,
            entries: ENGINE_WHINE_BAND_ENTRIES as u32,
        },
        upshift_dip_va: 0x004f_8b4c,
    };

/// One rev band: `count == pitch.len() - 1 == vol.len() - 1`, `lo < hi`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EngineSoundBand {
    pub count: i32,
    pub lo: i32,
    pub hi: i32,
    pub pitch: Vec<u16>,
    pub vol: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EngineSoundTables {
    pub slow: EngineSoundBand,
    pub mid: EngineSoundBand,
    pub top: EngineSoundBand,
    pub whine: EngineSoundBand,
    pub upshift_dip: [i32; ENGINE_UPSHIFT_DIP_ENTRIES],
}

impl EngineSoundTables {
    pub fn band(&self, id: EngineSoundBandId) -> &EngineSoundBand {
        match id {
            EngineSoundBandId::Slow => &self.slow,
            EngineSoundBandId::Mid => &self.mid,
            EngineSoundBandId::Top => &self.top,
            EngineSoundBandId::Whine => &self.whine,
        }
    }

    /// The `u16` the native reads as Top `pitch[-1]` (VA `0x4f84ca`): the
    /// high half of the little-endian Top `hi` dword.
    pub fn top_pitch_at_minus_one(&self) -> u16 {
        ((self.top.hi as u32) >> 16) as u16
    }

    /// Re-check every band invariant (used before encoding and after decode).
    pub fn validate(&self) -> Result<(), EngineAudioTablesError> {
        for id in EngineSoundBandId::ALL {
            validate_band(id, self.band(id))?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EngineAudioTablesError {
    Pe(String),
    ImageBase {
        expected: u32,
        actual: u32,
    },
    UnsupportedLayout,
    Misaligned {
        virtual_address: u32,
        alignment: u32,
    },
    /// The header `count` does not equal `N - 1` for the band's array length.
    BandCount {
        band: EngineSoundBandId,
        entries: usize,
        expected: i32,
        actual: i32,
    },
    /// The band window is empty or inverted.
    BandWindow {
        band: EngineSoundBandId,
        lo: i32,
        hi: i32,
    },
    /// An encoded band carries an array length other than the reviewed one.
    BandLength {
        band: EngineSoundBandId,
        offset: usize,
        expected: usize,
        actual: usize,
    },
    WrongLength {
        expected: usize,
        actual: usize,
    },
    Truncated {
        offset: usize,
        needed: usize,
        available: usize,
    },
    TrailingBytes {
        offset: usize,
        len: usize,
    },
    InvalidHeader {
        offset: usize,
    },
}

impl fmt::Display for EngineAudioTablesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pe(e) => write!(f, "invalid PE32 image: {e}"),
            Self::ImageBase { expected, actual } => write!(
                f,
                "image base mismatch: expected {expected:#x}, got {actual:#x}"
            ),
            Self::UnsupportedLayout => write!(f, "unsupported engine-audio-table layout"),
            Self::Misaligned {
                virtual_address,
                alignment,
            } => write!(
                f,
                "table address {virtual_address:#x} is not {alignment}-byte aligned"
            ),
            Self::BandCount {
                band,
                entries,
                expected,
                actual,
            } => write!(
                f,
                "{band} band count is {actual}, expected {expected} for {entries} entries"
            ),
            Self::BandWindow { band, lo, hi } => {
                write!(f, "{band} band window is not ascending: lo {lo}, hi {hi}")
            }
            Self::BandLength {
                band,
                offset,
                expected,
                actual,
            } => write!(
                f,
                "{band} band at offset {offset} has {actual} entries, expected {expected}"
            ),
            Self::WrongLength { expected, actual } => {
                write!(f, "wrong encoded length: expected {expected}, got {actual}")
            }
            Self::Truncated {
                offset,
                needed,
                available,
            } => write!(
                f,
                "truncated at offset {offset}: need {needed} bytes, {available} available"
            ),
            Self::TrailingBytes { offset, len } => {
                write!(f, "{len} trailing bytes at offset {offset}")
            }
            Self::InvalidHeader { offset } => {
                write!(f, "invalid engine-audio-table header at offset {offset}")
            }
        }
    }
}
impl std::error::Error for EngineAudioTablesError {}

fn pe_error(error: Pe32Error) -> EngineAudioTablesError {
    EngineAudioTablesError::Pe(format!("{error:?}"))
}

fn validate_band(
    id: EngineSoundBandId,
    band: &EngineSoundBand,
) -> Result<(), EngineAudioTablesError> {
    let entries = id.entries();
    if band.pitch.len() != entries || band.vol.len() != entries {
        return Err(EngineAudioTablesError::BandLength {
            band: id,
            offset: 0,
            expected: entries,
            actual: if band.pitch.len() != entries {
                band.pitch.len()
            } else {
                band.vol.len()
            },
        });
    }
    let expected = entries as i32 - 1;
    if band.count != expected {
        return Err(EngineAudioTablesError::BandCount {
            band: id,
            entries,
            expected,
            actual: band.count,
        });
    }
    if band.lo >= band.hi {
        return Err(EngineAudioTablesError::BandWindow {
            band: id,
            lo: band.lo,
            hi: band.hi,
        });
    }
    Ok(())
}

fn i32_at(bytes: &[u8], at: usize) -> i32 {
    i32::from_le_bytes(bytes[at..at + 4].try_into().expect("bounded span"))
}

/// Parse `count, lo, hi, pitch[N], vol[N]` from an exactly sized span.
fn parse_band_record(
    id: EngineSoundBandId,
    bytes: &[u8],
) -> Result<EngineSoundBand, EngineAudioTablesError> {
    let entries = id.entries();
    debug_assert_eq!(bytes.len(), BAND_HEADER_BYTES + entries * 3);
    let pitch_start = BAND_HEADER_BYTES;
    let vol_start = pitch_start + entries * 2;
    let band = EngineSoundBand {
        count: i32_at(bytes, 0),
        lo: i32_at(bytes, 4),
        hi: i32_at(bytes, 8),
        pitch: bytes[pitch_start..vol_start]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|w| u16::from_le_bytes(*w))
            .collect(),
        vol: bytes[vol_start..vol_start + entries].to_vec(),
    };
    validate_band(id, &band)?;
    Ok(band)
}

fn check_band_layout(
    id: EngineSoundBandId,
    band: &EngineSoundBandLayout,
) -> Result<(), EngineAudioTablesError> {
    let entries = id.entries() as u64;
    let pitch = u64::from(band.base_va) + BAND_HEADER_BYTES as u64;
    let vol = pitch + 2 * entries;
    if u64::from(band.entries) != entries
        || u64::from(band.pitch_va) != pitch
        || u64::from(band.vol_va) != vol
    {
        return Err(EngineAudioTablesError::UnsupportedLayout);
    }
    if !band.base_va.is_multiple_of(4) {
        return Err(EngineAudioTablesError::Misaligned {
            virtual_address: band.base_va,
            alignment: 4,
        });
    }
    Ok(())
}

pub fn extract_engine_audio_tables(
    input: &[u8],
    layout: &EngineAudioTablesLayout,
) -> Result<EngineSoundTables, EngineAudioTablesError> {
    for id in EngineSoundBandId::ALL {
        check_band_layout(id, layout.band(id))?;
    }
    if !layout.upshift_dip_va.is_multiple_of(4) {
        return Err(EngineAudioTablesError::Misaligned {
            virtual_address: layout.upshift_dip_va,
            alignment: 4,
        });
    }
    let image = Pe32Image::parse(input).map_err(pe_error)?;
    if image.image_base() != layout.image_base {
        return Err(EngineAudioTablesError::ImageBase {
            expected: layout.image_base,
            actual: image.image_base(),
        });
    }
    let band = |id: EngineSoundBandId| {
        let span = image
            .read_va_range(
                layout.band(id).base_va,
                BAND_HEADER_BYTES + id.entries() * 3,
            )
            .map_err(pe_error)?;
        parse_band_record(id, span)
    };
    let slow = band(EngineSoundBandId::Slow)?;
    let mid = band(EngineSoundBandId::Mid)?;
    let top = band(EngineSoundBandId::Top)?;
    let whine = band(EngineSoundBandId::Whine)?;
    let dip = image
        .read_va_range(layout.upshift_dip_va, ENGINE_UPSHIFT_DIP_ENTRIES * 4)
        .map_err(pe_error)?;
    Ok(EngineSoundTables {
        slow,
        mid,
        top,
        whine,
        upshift_dip: std::array::from_fn(|i| i32_at(dip, i * 4)),
    })
}

/// Encode `cmr-engine-audio-tables-v1`. Callers hold validated tables; an
/// invalid table is rejected rather than written.
pub fn encode_engine_audio_tables(
    tables: &EngineSoundTables,
) -> Result<Vec<u8>, EngineAudioTablesError> {
    tables.validate()?;
    let mut out = Vec::with_capacity(ENGINE_AUDIO_TABLES_ENCODED_LEN);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&ENGINE_AUDIO_TABLES_FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&(ENGINE_SOUND_BAND_COUNT as u32).to_le_bytes());
    for id in EngineSoundBandId::ALL {
        let band = tables.band(id);
        out.extend_from_slice(&(band.pitch.len() as u32).to_le_bytes());
        out.extend_from_slice(&band.count.to_le_bytes());
        out.extend_from_slice(&band.lo.to_le_bytes());
        out.extend_from_slice(&band.hi.to_le_bytes());
        for word in &band.pitch {
            out.extend_from_slice(&word.to_le_bytes());
        }
        out.extend_from_slice(&band.vol);
    }
    for value in &tables.upshift_dip {
        out.extend_from_slice(&value.to_le_bytes());
    }
    debug_assert_eq!(out.len(), ENGINE_AUDIO_TABLES_ENCODED_LEN);
    Ok(out)
}

struct Cursor<'a> {
    input: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, needed: usize) -> Result<&'a [u8], EngineAudioTablesError> {
        let available = self.input.len() - self.offset;
        if needed > available {
            return Err(EngineAudioTablesError::Truncated {
                offset: self.offset,
                needed,
                available,
            });
        }
        let bytes = &self.input[self.offset..self.offset + needed];
        self.offset += needed;
        Ok(bytes)
    }
    fn u32(&mut self) -> Result<u32, EngineAudioTablesError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
}

pub fn decode_engine_audio_tables(
    input: &[u8],
) -> Result<EngineSoundTables, EngineAudioTablesError> {
    let mut cursor = Cursor { input, offset: 0 };
    if cursor.take(8)? != MAGIC {
        return Err(EngineAudioTablesError::InvalidHeader { offset: 0 });
    }
    if cursor.u32()? != ENGINE_AUDIO_TABLES_FORMAT_VERSION {
        return Err(EngineAudioTablesError::InvalidHeader { offset: 8 });
    }
    if cursor.u32()? != ENGINE_SOUND_BAND_COUNT as u32 {
        return Err(EngineAudioTablesError::InvalidHeader { offset: 12 });
    }
    let mut band = |id: EngineSoundBandId| {
        let at = cursor.offset;
        let entries = cursor.u32()? as usize;
        if entries != id.entries() {
            return Err(EngineAudioTablesError::BandLength {
                band: id,
                offset: at,
                expected: id.entries(),
                actual: entries,
            });
        }
        parse_band_record(id, cursor.take(BAND_HEADER_BYTES + entries * 3)?)
    };
    let slow = band(EngineSoundBandId::Slow)?;
    let mid = band(EngineSoundBandId::Mid)?;
    let top = band(EngineSoundBandId::Top)?;
    let whine = band(EngineSoundBandId::Whine)?;
    let dip = cursor.take(ENGINE_UPSHIFT_DIP_ENTRIES * 4)?;
    if cursor.offset != input.len() {
        return Err(EngineAudioTablesError::TrailingBytes {
            offset: cursor.offset,
            len: input.len() - cursor.offset,
        });
    }
    Ok(EngineSoundTables {
        slow,
        mid,
        top,
        whine,
        upshift_dip: std::array::from_fn(|i| i32_at(dip, i * 4)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PE: usize = 0x80;
    const OPTIONAL: usize = PE + 24;
    const SECTIONS: usize = OPTIONAL + 0xe0;
    const RAW: usize = 0x400;
    const SECTION_RVA: u32 = 0x000f_8000;
    const SECTION_SIZE: usize = 0x1000;

    fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    fn file_offset(va: u32) -> usize {
        RAW + (va - 0x0040_0000 - SECTION_RVA) as usize
    }

    fn synthetic_band(id: EngineSoundBandId, seed: u16) -> EngineSoundBand {
        let n = id.entries();
        EngineSoundBand {
            count: n as i32 - 1,
            lo: -(seed as i32),
            // A high half that is distinct from any pitch entry.
            hi: (i32::from(seed) << 16) | 0x0123,
            pitch: (0..n).map(|i| seed.wrapping_mul(31) ^ i as u16).collect(),
            vol: (0..n).map(|i| (i as u8).wrapping_add(seed as u8)).collect(),
        }
    }

    fn synthetic_tables() -> EngineSoundTables {
        EngineSoundTables {
            slow: synthetic_band(EngineSoundBandId::Slow, 3),
            mid: synthetic_band(EngineSoundBandId::Mid, 5),
            top: synthetic_band(EngineSoundBandId::Top, 0x7abc),
            whine: synthetic_band(EngineSoundBandId::Whine, 11),
            upshift_dip: std::array::from_fn(|i| i as i32 * -13 + 7),
        }
    }

    fn write_band(bytes: &mut [u8], layout: &EngineSoundBandLayout, band: &EngineSoundBand) {
        let at = file_offset(layout.base_va);
        put_u32(bytes, at, band.count as u32);
        put_u32(bytes, at + 4, band.lo as u32);
        put_u32(bytes, at + 8, band.hi as u32);
        for (i, word) in band.pitch.iter().enumerate() {
            put_u16(bytes, file_offset(layout.pitch_va) + i * 2, *word);
        }
        let vol = file_offset(layout.vol_va);
        bytes[vol..vol + band.vol.len()].copy_from_slice(&band.vol);
    }

    fn synthetic_pe(tables: &EngineSoundTables) -> Vec<u8> {
        let layout = OFFICIAL_EU_ENGINE_AUDIO_TABLES_LAYOUT;
        let mut bytes = vec![0xa5_u8; RAW + SECTION_SIZE];
        bytes[..RAW].fill(0);
        bytes[..2].copy_from_slice(b"MZ");
        put_u32(&mut bytes, 0x3c, PE as u32);
        bytes[PE..PE + 4].copy_from_slice(b"PE\0\0");
        put_u16(&mut bytes, PE + 4, 0x14c);
        put_u16(&mut bytes, PE + 6, 1);
        put_u16(&mut bytes, PE + 20, 0xe0);
        put_u16(&mut bytes, OPTIONAL, 0x10b);
        put_u32(&mut bytes, OPTIONAL + 28, layout.image_base);
        put_u32(&mut bytes, SECTIONS + 8, SECTION_SIZE as u32);
        put_u32(&mut bytes, SECTIONS + 12, SECTION_RVA);
        put_u32(&mut bytes, SECTIONS + 16, SECTION_SIZE as u32);
        put_u32(&mut bytes, SECTIONS + 20, RAW as u32);
        for id in EngineSoundBandId::ALL {
            write_band(&mut bytes, layout.band(id), tables.band(id));
        }
        for (i, value) in tables.upshift_dip.iter().enumerate() {
            put_u32(
                &mut bytes,
                file_offset(layout.upshift_dip_va) + i * 4,
                *value as u32,
            );
        }
        bytes
    }

    #[test]
    fn reviewed_addresses_are_internally_consistent() {
        let l = OFFICIAL_EU_ENGINE_AUDIO_TABLES_LAYOUT;
        for id in EngineSoundBandId::ALL {
            check_band_layout(id, l.band(id)).unwrap();
        }
        // The Top band's `pitch[-1]` is the high half of its `hi` dword.
        assert_eq!(l.top.pitch_va - 2, 0x4f84ca);
        assert_eq!(l.top.base_va + 8 + 2, 0x4f84ca);
        assert_eq!(
            ENGINE_AUDIO_TABLES_ENCODED_LEN,
            16 + 4 * 16 + 3 * (21 + 127 + 127 + 194) + 96
        );
    }

    #[test]
    fn extracts_every_band_and_the_dip_sequence() {
        let tables = synthetic_tables();
        let bytes = synthetic_pe(&tables);
        let got =
            extract_engine_audio_tables(&bytes, &OFFICIAL_EU_ENGINE_AUDIO_TABLES_LAYOUT).unwrap();
        assert_eq!(got, tables);
        let raw = file_offset(0x4f84ca);
        assert_eq!(
            got.top_pitch_at_minus_one(),
            u16::from_le_bytes([bytes[raw], bytes[raw + 1]])
        );
        assert_eq!(got.top_pitch_at_minus_one(), 0x7abc);
    }

    #[test]
    fn rejects_count_mismatch_and_non_ascending_window() {
        let l = OFFICIAL_EU_ENGINE_AUDIO_TABLES_LAYOUT;
        let mut tables = synthetic_tables();
        tables.mid.count += 1;
        assert_eq!(
            extract_engine_audio_tables(&synthetic_pe(&tables), &l),
            Err(EngineAudioTablesError::BandCount {
                band: EngineSoundBandId::Mid,
                entries: 127,
                expected: 126,
                actual: 127
            })
        );
        let mut tables = synthetic_tables();
        tables.whine.count = 194;
        let err = extract_engine_audio_tables(&synthetic_pe(&tables), &l).unwrap_err();
        assert!(err.to_string().contains("whine band count is 194"), "{err}");

        for (lo, hi) in [(5, 5), (6, 5), (i32::MAX, i32::MIN)] {
            let mut tables = synthetic_tables();
            tables.slow.lo = lo;
            tables.slow.hi = hi;
            assert_eq!(
                extract_engine_audio_tables(&synthetic_pe(&tables), &l),
                Err(EngineAudioTablesError::BandWindow {
                    band: EngineSoundBandId::Slow,
                    lo,
                    hi
                })
            );
            assert!(matches!(
                encode_engine_audio_tables(&tables),
                Err(EngineAudioTablesError::BandWindow { .. })
            ));
        }
        let mut tables = synthetic_tables();
        tables.top.lo = tables.top.hi - 1;
        extract_engine_audio_tables(&synthetic_pe(&tables), &l).unwrap();
    }

    #[test]
    fn rejects_truncation_wrong_image_and_bad_layouts() {
        let l = OFFICIAL_EU_ENGINE_AUDIO_TABLES_LAYOUT;
        let bytes = synthetic_pe(&synthetic_tables());
        let end = file_offset(l.upshift_dip_va) + ENGINE_UPSHIFT_DIP_ENTRIES * 4;
        for len in [end - 1, file_offset(l.whine.vol_va) + 10] {
            let Err(EngineAudioTablesError::Pe(text)) =
                extract_engine_audio_tables(&bytes[..len], &l)
            else {
                panic!("expected PE error");
            };
            assert!(
                text.contains("Truncated") && text.contains("offset"),
                "{text}"
            );
        }
        extract_engine_audio_tables(&bytes[..end], &l).unwrap();
        let mut wrong_base = l;
        wrong_base.image_base += 0x1000;
        assert_eq!(
            extract_engine_audio_tables(&bytes, &wrong_base),
            Err(EngineAudioTablesError::ImageBase {
                expected: 0x0040_1000,
                actual: 0x0040_0000
            })
        );
        let mut gap = l;
        gap.top.vol_va += 1;
        assert_eq!(
            extract_engine_audio_tables(&bytes, &gap),
            Err(EngineAudioTablesError::UnsupportedLayout)
        );
        let mut entries = l;
        entries.slow.entries += 1;
        assert_eq!(
            extract_engine_audio_tables(&bytes, &entries),
            Err(EngineAudioTablesError::UnsupportedLayout)
        );
        let mut shifted = l;
        shifted.mid.base_va += 2;
        shifted.mid.pitch_va += 2;
        shifted.mid.vol_va += 2;
        assert_eq!(
            extract_engine_audio_tables(&bytes, &shifted),
            Err(EngineAudioTablesError::Misaligned {
                virtual_address: 0x4f8332,
                alignment: 4
            })
        );
        let mut dip = l;
        dip.upshift_dip_va += 2;
        assert!(matches!(
            extract_engine_audio_tables(&bytes, &dip),
            Err(EngineAudioTablesError::Misaligned { alignment: 4, .. })
        ));
        let mut unmapped = l;
        unmapped.upshift_dip_va = 0x1000;
        assert!(matches!(
            extract_engine_audio_tables(&bytes, &unmapped),
            Err(EngineAudioTablesError::Pe(_))
        ));
    }

    #[test]
    fn codec_round_trips_and_preserves_extremes() {
        let mut tables = synthetic_tables();
        tables.whine.lo = i32::MIN;
        tables.whine.hi = i32::MAX;
        tables.slow.pitch[0] = u16::MAX;
        tables.mid.vol[126] = 0xff;
        tables.upshift_dip[23] = i32::MIN;
        let encoded = encode_engine_audio_tables(&tables).unwrap();
        assert_eq!(encoded.len(), ENGINE_AUDIO_TABLES_ENCODED_LEN);
        assert_eq!(decode_engine_audio_tables(&encoded).unwrap(), tables);
    }

    #[test]
    fn decode_reports_truncation_trailing_and_header_offsets() {
        let encoded = encode_engine_audio_tables(&synthetic_tables()).unwrap();
        assert_eq!(
            decode_engine_audio_tables(&encoded[..encoded.len() - 1]),
            Err(EngineAudioTablesError::Truncated {
                offset: ENGINE_AUDIO_TABLES_ENCODED_LEN - 96,
                needed: 96,
                available: 95
            })
        );
        assert_eq!(
            decode_engine_audio_tables(&encoded[..10]),
            Err(EngineAudioTablesError::Truncated {
                offset: 8,
                needed: 4,
                available: 2
            })
        );
        assert!(matches!(
            decode_engine_audio_tables(&[]),
            Err(EngineAudioTablesError::Truncated { offset: 0, .. })
        ));
        let mut trailing = encoded.clone();
        trailing.push(0);
        assert_eq!(
            decode_engine_audio_tables(&trailing),
            Err(EngineAudioTablesError::TrailingBytes {
                offset: ENGINE_AUDIO_TABLES_ENCODED_LEN,
                len: 1
            })
        );
        for (flip, offset) in [(0, 0), (8, 8), (12, 12)] {
            let mut candidate = encoded.clone();
            candidate[flip] ^= 1;
            assert_eq!(
                decode_engine_audio_tables(&candidate),
                Err(EngineAudioTablesError::InvalidHeader { offset })
            );
        }
        let mid_at = HEADER_LEN + encoded_band_len(ENGINE_SLOW_BAND_ENTRIES);
        let mut length = encoded.clone();
        length[mid_at] ^= 1;
        assert_eq!(
            decode_engine_audio_tables(&length),
            Err(EngineAudioTablesError::BandLength {
                band: EngineSoundBandId::Mid,
                offset: mid_at,
                expected: 127,
                actual: 126
            })
        );
        let mut count = encoded.clone();
        count[mid_at + 4] ^= 1;
        assert!(matches!(
            decode_engine_audio_tables(&count),
            Err(EngineAudioTablesError::BandCount {
                band: EngineSoundBandId::Mid,
                ..
            })
        ));
        let mut window = encoded;
        // Slow lo := hi.
        let slow_hi = HEADER_LEN + 12;
        window.copy_within(slow_hi..slow_hi + 4, slow_hi - 4);
        assert!(matches!(
            decode_engine_audio_tables(&window),
            Err(EngineAudioTablesError::BandWindow {
                band: EngineSoundBandId::Slow,
                ..
            })
        ));
    }

    #[test]
    fn encode_rejects_wrong_array_lengths() {
        let mut tables = synthetic_tables();
        tables.top.vol.pop();
        assert!(matches!(
            encode_engine_audio_tables(&tables),
            Err(EngineAudioTablesError::BandLength {
                band: EngineSoundBandId::Top,
                expected: 127,
                actual: 126,
                ..
            })
        ));
    }
}
