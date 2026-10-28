//! Authenticated extraction and payload-free encoding of the static per-stage
//! weather / lighting selector tables (evidence `rea.official-eu.stage-weather`).
//!
//! Observation: `FUN_0046b0f0`, `FUN_0046b160` and `FUN_0046b1b0` read six
//! little-endian `u32` tables of the official EU executable's `.data` section
//! through absolute-address operands. Each selector comes as a pair:
//!
//! - a rally-school table `u32[3][5]`, indexed `level * 5 + task`;
//! - a championship-stage table `u32[9][7]`, indexed `country * 7 + stage`.
//!
//! The three pairs are the weather type, the weather detail and the lighting
//! selectors. The meaning of the numeric values is not asserted here, and no
//! value is range-checked: every word is preserved as read. No table payload
//! is compiled into this crate; tests use synthetic fixtures only.

use std::fmt;

use crate::pe32::{Pe32Error, Pe32Image};

pub const SCHOOL_WEATHER_LEVELS: usize = 3;
pub const SCHOOL_WEATHER_TASKS: usize = 5;
pub const SCHOOL_WEATHER_ENTRIES: usize = SCHOOL_WEATHER_LEVELS * SCHOOL_WEATHER_TASKS;
pub const STAGE_WEATHER_COUNTRIES: usize = 9;
pub const STAGE_WEATHER_STAGES: usize = 7;
pub const STAGE_WEATHER_ENTRIES: usize = STAGE_WEATHER_COUNTRIES * STAGE_WEATHER_STAGES;
pub const STAGE_WEATHER_TABLES_FORMAT_VERSION: u32 = 1;

const MAGIC: &[u8; 8] = b"CMRSWTH\0";
const HEADER_LEN: usize = 16;
const SCHOOL_LEN: usize = SCHOOL_WEATHER_ENTRIES * 4;
const STAGE_LEN: usize = STAGE_WEATHER_ENTRIES * 4;
pub const STAGE_WEATHER_TABLES_ENCODED_LEN: usize = HEADER_LEN + 3 * (SCHOOL_LEN + STAGE_LEN);

/// Virtual addresses of every reviewed table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StageWeatherTablesLayout {
    pub image_base: u32,
    pub school_weather_type_va: u32,
    pub stage_weather_type_va: u32,
    pub school_weather_detail_va: u32,
    pub stage_weather_detail_va: u32,
    pub school_lighting_va: u32,
    pub stage_lighting_va: u32,
}

impl StageWeatherTablesLayout {
    /// `(name, va, byte length)` of every table span, in encoding order.
    fn spans(&self) -> [(&'static str, u32, usize); 6] {
        [
            (
                "school_weather_type",
                self.school_weather_type_va,
                SCHOOL_LEN,
            ),
            ("stage_weather_type", self.stage_weather_type_va, STAGE_LEN),
            (
                "school_weather_detail",
                self.school_weather_detail_va,
                SCHOOL_LEN,
            ),
            (
                "stage_weather_detail",
                self.stage_weather_detail_va,
                STAGE_LEN,
            ),
            ("school_lighting", self.school_lighting_va, SCHOOL_LEN),
            ("stage_lighting", self.stage_lighting_va, STAGE_LEN),
        ]
    }
}

pub const OFFICIAL_EU_STAGE_WEATHER_TABLES_LAYOUT: StageWeatherTablesLayout =
    StageWeatherTablesLayout {
        image_base: 0x0040_0000,
        school_weather_type_va: 0x004f_4538,
        stage_weather_type_va: 0x004f_4574,
        school_weather_detail_va: 0x004f_4670,
        stage_weather_detail_va: 0x004f_46ac,
        school_lighting_va: 0x004f_47a8,
        stage_lighting_va: 0x004f_47e4,
    };

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StageWeatherTables {
    pub school_type: [u32; SCHOOL_WEATHER_ENTRIES],
    pub stage_type: [u32; STAGE_WEATHER_ENTRIES],
    pub school_detail: [u32; SCHOOL_WEATHER_ENTRIES],
    pub stage_detail: [u32; STAGE_WEATHER_ENTRIES],
    pub school_lighting: [u32; SCHOOL_WEATHER_ENTRIES],
    pub stage_lighting: [u32; STAGE_WEATHER_ENTRIES],
}

/// One `(type, detail, lighting)` triple, as the three readers return it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StageWeatherSelectors {
    pub weather_type: u32,
    pub weather_detail: u32,
    pub lighting: u32,
}

/// Row-major `level * 5 + task`, or `None` outside `[0..3][0..5]`.
pub const fn school_weather_index(level: usize, task: usize) -> Option<usize> {
    if level < SCHOOL_WEATHER_LEVELS && task < SCHOOL_WEATHER_TASKS {
        Some(level * SCHOOL_WEATHER_TASKS + task)
    } else {
        None
    }
}

/// Row-major `country * 7 + stage`, or `None` outside `[0..9][0..7]`.
pub const fn stage_weather_index(country: usize, stage: usize) -> Option<usize> {
    if country < STAGE_WEATHER_COUNTRIES && stage < STAGE_WEATHER_STAGES {
        Some(country * STAGE_WEATHER_STAGES + stage)
    } else {
        None
    }
}

impl StageWeatherTables {
    /// Rally-school selectors; `None` outside `[0..3][0..5]`.
    pub fn school(&self, level: usize, task: usize) -> Option<StageWeatherSelectors> {
        let i = school_weather_index(level, task)?;
        Some(StageWeatherSelectors {
            weather_type: self.school_type[i],
            weather_detail: self.school_detail[i],
            lighting: self.school_lighting[i],
        })
    }
    /// Championship-stage selectors; `None` outside `[0..9][0..7]`.
    pub fn stage(&self, country: usize, stage: usize) -> Option<StageWeatherSelectors> {
        let i = stage_weather_index(country, stage)?;
        Some(StageWeatherSelectors {
            weather_type: self.stage_type[i],
            weather_detail: self.stage_detail[i],
            lighting: self.stage_lighting[i],
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StageWeatherTablesError {
    Pe(String),
    ImageBase {
        expected: u32,
        actual: u32,
    },
    /// Two reviewed table spans overlap.
    UnsupportedLayout,
    Misaligned {
        virtual_address: u32,
        alignment: u32,
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

impl fmt::Display for StageWeatherTablesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pe(e) => write!(f, "invalid PE32 image: {e}"),
            Self::ImageBase { expected, actual } => write!(
                f,
                "image base mismatch: expected {expected:#x}, got {actual:#x}"
            ),
            Self::UnsupportedLayout => write!(f, "unsupported stage-weather-table layout"),
            Self::Misaligned {
                virtual_address,
                alignment,
            } => write!(
                f,
                "table address {virtual_address:#x} is not {alignment}-byte aligned"
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
                write!(f, "invalid stage-weather-table header at offset {offset}")
            }
        }
    }
}
impl std::error::Error for StageWeatherTablesError {}

fn pe_error(error: Pe32Error) -> StageWeatherTablesError {
    StageWeatherTablesError::Pe(format!("{error:?}"))
}

fn u32_array<const N: usize>(bytes: &[u8]) -> [u32; N] {
    std::array::from_fn(|i| {
        u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().expect("bounded span"))
    })
}

fn check_layout(layout: &StageWeatherTablesLayout) -> Result<(), StageWeatherTablesError> {
    let spans = layout.spans();
    for (_, va, _) in spans {
        if !va.is_multiple_of(4) {
            return Err(StageWeatherTablesError::Misaligned {
                virtual_address: va,
                alignment: 4,
            });
        }
    }
    for (i, &(_, a, a_len)) in spans.iter().enumerate() {
        for &(_, b, b_len) in &spans[i + 1..] {
            let (a, b) = (u64::from(a), u64::from(b));
            if a < b + b_len as u64 && b < a + a_len as u64 {
                return Err(StageWeatherTablesError::UnsupportedLayout);
            }
        }
    }
    Ok(())
}

pub fn extract_stage_weather_tables(
    input: &[u8],
    layout: &StageWeatherTablesLayout,
) -> Result<StageWeatherTables, StageWeatherTablesError> {
    check_layout(layout)?;
    let image = Pe32Image::parse(input).map_err(pe_error)?;
    if image.image_base() != layout.image_base {
        return Err(StageWeatherTablesError::ImageBase {
            expected: layout.image_base,
            actual: image.image_base(),
        });
    }
    let read = |va: u32, len: usize| image.read_va_range(va, len).map_err(pe_error);
    Ok(StageWeatherTables {
        school_type: u32_array(read(layout.school_weather_type_va, SCHOOL_LEN)?),
        stage_type: u32_array(read(layout.stage_weather_type_va, STAGE_LEN)?),
        school_detail: u32_array(read(layout.school_weather_detail_va, SCHOOL_LEN)?),
        stage_detail: u32_array(read(layout.stage_weather_detail_va, STAGE_LEN)?),
        school_lighting: u32_array(read(layout.school_lighting_va, SCHOOL_LEN)?),
        stage_lighting: u32_array(read(layout.stage_lighting_va, STAGE_LEN)?),
    })
}

/// Encode `cmr-stage-weather-tables-v1`: a 16-byte header (magic, version,
/// school entries, stage entries) then the six tables as `u32` LE words in
/// layout order (type, detail, lighting; school before stage).
pub fn encode_stage_weather_tables(tables: &StageWeatherTables) -> Vec<u8> {
    let mut out = Vec::with_capacity(STAGE_WEATHER_TABLES_ENCODED_LEN);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&STAGE_WEATHER_TABLES_FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&(SCHOOL_WEATHER_ENTRIES as u16).to_le_bytes());
    out.extend_from_slice(&(STAGE_WEATHER_ENTRIES as u16).to_le_bytes());
    let words = tables
        .school_type
        .iter()
        .chain(&tables.stage_type)
        .chain(&tables.school_detail)
        .chain(&tables.stage_detail)
        .chain(&tables.school_lighting)
        .chain(&tables.stage_lighting);
    for word in words {
        out.extend_from_slice(&word.to_le_bytes());
    }
    debug_assert_eq!(out.len(), STAGE_WEATHER_TABLES_ENCODED_LEN);
    out
}

struct Cursor<'a> {
    input: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, needed: usize) -> Result<&'a [u8], StageWeatherTablesError> {
        let available = self.input.len() - self.offset;
        if needed > available {
            return Err(StageWeatherTablesError::Truncated {
                offset: self.offset,
                needed,
                available,
            });
        }
        let bytes = &self.input[self.offset..self.offset + needed];
        self.offset += needed;
        Ok(bytes)
    }
    fn u16(&mut self) -> Result<u16, StageWeatherTablesError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, StageWeatherTablesError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
}

pub fn decode_stage_weather_tables(
    input: &[u8],
) -> Result<StageWeatherTables, StageWeatherTablesError> {
    let mut cursor = Cursor { input, offset: 0 };
    if cursor.take(8)? != MAGIC {
        return Err(StageWeatherTablesError::InvalidHeader { offset: 0 });
    }
    if cursor.u32()? != STAGE_WEATHER_TABLES_FORMAT_VERSION {
        return Err(StageWeatherTablesError::InvalidHeader { offset: 8 });
    }
    if cursor.u16()? != SCHOOL_WEATHER_ENTRIES as u16 {
        return Err(StageWeatherTablesError::InvalidHeader { offset: 12 });
    }
    if cursor.u16()? != STAGE_WEATHER_ENTRIES as u16 {
        return Err(StageWeatherTablesError::InvalidHeader { offset: 14 });
    }
    let tables = StageWeatherTables {
        school_type: u32_array(cursor.take(SCHOOL_LEN)?),
        stage_type: u32_array(cursor.take(STAGE_LEN)?),
        school_detail: u32_array(cursor.take(SCHOOL_LEN)?),
        stage_detail: u32_array(cursor.take(STAGE_LEN)?),
        school_lighting: u32_array(cursor.take(SCHOOL_LEN)?),
        stage_lighting: u32_array(cursor.take(STAGE_LEN)?),
    };
    if cursor.offset != input.len() {
        return Err(StageWeatherTablesError::TrailingBytes {
            offset: cursor.offset,
            len: input.len() - cursor.offset,
        });
    }
    Ok(tables)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PE: usize = 0x80;
    const OPTIONAL: usize = PE + 24;
    const SECTIONS: usize = OPTIONAL + 0xe0;
    const RAW: usize = 0x400;
    /// One synthetic section covering every reviewed address.
    const SECTION_RVA: u32 = 0x000f_4000;
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

    /// Distinct synthetic words per table; not game data.
    fn synthetic_tables() -> StageWeatherTables {
        StageWeatherTables {
            school_type: std::array::from_fn(|i| 0x1000 + i as u32),
            stage_type: std::array::from_fn(|i| 0x2000 + i as u32),
            school_detail: std::array::from_fn(|i| 0x3000 + i as u32),
            stage_detail: std::array::from_fn(|i| 0x4000 + i as u32),
            school_lighting: std::array::from_fn(|i| 0x5000 + i as u32),
            stage_lighting: std::array::from_fn(|i| 0x6000 + i as u32),
        }
    }

    fn synthetic_pe(tables: &StageWeatherTables) -> Vec<u8> {
        let l = OFFICIAL_EU_STAGE_WEATHER_TABLES_LAYOUT;
        let mut bytes = vec![0xa5_u8; RAW + SECTION_SIZE];
        bytes[..RAW].fill(0);
        bytes[..2].copy_from_slice(b"MZ");
        put_u32(&mut bytes, 0x3c, PE as u32);
        bytes[PE..PE + 4].copy_from_slice(b"PE\0\0");
        put_u16(&mut bytes, PE + 4, 0x14c);
        put_u16(&mut bytes, PE + 6, 1);
        put_u16(&mut bytes, PE + 20, 0xe0);
        put_u16(&mut bytes, OPTIONAL, 0x10b);
        put_u32(&mut bytes, OPTIONAL + 28, l.image_base);
        put_u32(&mut bytes, SECTIONS + 8, SECTION_SIZE as u32);
        put_u32(&mut bytes, SECTIONS + 12, SECTION_RVA);
        put_u32(&mut bytes, SECTIONS + 16, SECTION_SIZE as u32);
        put_u32(&mut bytes, SECTIONS + 20, RAW as u32);
        let mut words = |va: u32, values: &[u32]| {
            for (i, v) in values.iter().enumerate() {
                put_u32(&mut bytes, file_offset(va) + i * 4, *v);
            }
        };
        words(l.school_weather_type_va, &tables.school_type);
        words(l.stage_weather_type_va, &tables.stage_type);
        words(l.school_weather_detail_va, &tables.school_detail);
        words(l.stage_weather_detail_va, &tables.stage_detail);
        words(l.school_lighting_va, &tables.school_lighting);
        words(l.stage_lighting_va, &tables.stage_lighting);
        bytes
    }

    #[test]
    fn reviewed_layout_is_aligned_disjoint_contiguous_and_sized() {
        let l = OFFICIAL_EU_STAGE_WEATHER_TABLES_LAYOUT;
        check_layout(&l).unwrap();
        // The six tables are back to back in the reviewed image.
        assert_eq!(l.school_weather_type_va + 15 * 4, l.stage_weather_type_va);
        assert_eq!(l.stage_weather_type_va + 63 * 4, l.school_weather_detail_va);
        assert_eq!(
            l.school_weather_detail_va + 15 * 4,
            l.stage_weather_detail_va
        );
        assert_eq!(l.stage_weather_detail_va + 63 * 4, l.school_lighting_va);
        assert_eq!(l.school_lighting_va + 15 * 4, l.stage_lighting_va);
        assert_eq!(STAGE_WEATHER_TABLES_ENCODED_LEN, 16 + 3 * (60 + 252));
    }

    #[test]
    fn extracts_every_table_in_place() {
        let tables = synthetic_tables();
        let got = extract_stage_weather_tables(
            &synthetic_pe(&tables),
            &OFFICIAL_EU_STAGE_WEATHER_TABLES_LAYOUT,
        )
        .unwrap();
        assert_eq!(got, tables);
    }

    #[test]
    fn indexes_school_and_stage_row_major_with_bounds() {
        let t = synthetic_tables();
        assert_eq!(school_weather_index(0, 0), Some(0));
        assert_eq!(school_weather_index(1, 0), Some(5));
        assert_eq!(school_weather_index(2, 4), Some(14));
        assert_eq!(school_weather_index(3, 0), None);
        assert_eq!(school_weather_index(0, 5), None);
        assert_eq!(stage_weather_index(1, 0), Some(7));
        assert_eq!(stage_weather_index(8, 6), Some(62));
        assert_eq!(stage_weather_index(9, 0), None);
        assert_eq!(stage_weather_index(0, 7), None);
        assert_eq!(
            t.school(2, 3),
            Some(StageWeatherSelectors {
                weather_type: 0x1000 + 13,
                weather_detail: 0x3000 + 13,
                lighting: 0x5000 + 13,
            })
        );
        assert_eq!(
            t.stage(4, 2),
            Some(StageWeatherSelectors {
                weather_type: 0x2000 + 30,
                weather_detail: 0x4000 + 30,
                lighting: 0x6000 + 30,
            })
        );
        assert_eq!(t.school(3, 0), None);
        assert_eq!(t.stage(0, 7), None);
        assert_eq!(t.stage(usize::MAX, 0), None);
    }

    #[test]
    fn rejects_truncation_wrong_section_wrong_image_and_bad_layouts() {
        let l = OFFICIAL_EU_STAGE_WEATHER_TABLES_LAYOUT;
        let bytes = synthetic_pe(&synthetic_tables());
        // The last table runs past a file truncated one byte early.
        let end = file_offset(l.stage_lighting_va) + STAGE_LEN;
        let Err(StageWeatherTablesError::Pe(text)) =
            extract_stage_weather_tables(&bytes[..end - 1], &l)
        else {
            panic!("expected PE error");
        };
        assert!(
            text.contains("Truncated") && text.contains("offset"),
            "{text}"
        );
        // A table that starts in the section but runs off its end.
        let mut tail = l;
        tail.stage_lighting_va = 0x0040_0000 + SECTION_RVA + SECTION_SIZE as u32 - 8;
        assert!(matches!(
            extract_stage_weather_tables(&bytes, &tail),
            Err(StageWeatherTablesError::Pe(text)) if text.contains("UnmappedRange")
        ));
        // A table outside every section.
        let mut unmapped = l;
        unmapped.school_lighting_va = 0x0040_1000;
        assert!(matches!(
            extract_stage_weather_tables(&bytes, &unmapped),
            Err(StageWeatherTablesError::Pe(_))
        ));
        let mut wrong_base = l;
        wrong_base.image_base += 0x1000;
        assert_eq!(
            extract_stage_weather_tables(&bytes, &wrong_base),
            Err(StageWeatherTablesError::ImageBase {
                expected: 0x0040_1000,
                actual: 0x0040_0000
            })
        );
        for shift in 0..6 {
            let mut m = l;
            let field = match shift {
                0 => &mut m.school_weather_type_va,
                1 => &mut m.stage_weather_type_va,
                2 => &mut m.school_weather_detail_va,
                3 => &mut m.stage_weather_detail_va,
                4 => &mut m.school_lighting_va,
                _ => &mut m.stage_lighting_va,
            };
            *field += 2;
            let va = *field;
            assert_eq!(
                extract_stage_weather_tables(&bytes, &m),
                Err(StageWeatherTablesError::Misaligned {
                    virtual_address: va,
                    alignment: 4
                })
            );
        }
        let mut overlap = l;
        overlap.school_weather_detail_va = l.stage_weather_type_va + STAGE_LEN as u32 - 4;
        assert_eq!(
            extract_stage_weather_tables(&bytes, &overlap),
            Err(StageWeatherTablesError::UnsupportedLayout)
        );
        // Adjacent (touching) spans are not an overlap.
        check_layout(&l).unwrap();
        assert!(matches!(
            extract_stage_weather_tables(b"MZ", &l),
            Err(StageWeatherTablesError::Pe(_))
        ));
    }

    #[test]
    fn codec_round_trips_and_preserves_extremes() {
        let mut tables = synthetic_tables();
        tables.school_type[0] = u32::MAX;
        tables.stage_lighting[62] = 0;
        tables.stage_detail[31] = 0x8000_0000;
        let encoded = encode_stage_weather_tables(&tables);
        assert_eq!(encoded.len(), STAGE_WEATHER_TABLES_ENCODED_LEN);
        assert_eq!(&encoded[..8], MAGIC);
        // Encoding order: school type immediately after the header.
        assert_eq!(&encoded[16..20], &u32::MAX.to_le_bytes());
        assert_eq!(decode_stage_weather_tables(&encoded).unwrap(), tables);
    }

    #[test]
    fn decode_reports_truncation_trailing_and_header_offsets() {
        let encoded = encode_stage_weather_tables(&synthetic_tables());
        assert_eq!(
            decode_stage_weather_tables(&encoded[..encoded.len() - 1]),
            Err(StageWeatherTablesError::Truncated {
                offset: STAGE_WEATHER_TABLES_ENCODED_LEN - STAGE_LEN,
                needed: STAGE_LEN,
                available: STAGE_LEN - 1
            })
        );
        assert_eq!(
            decode_stage_weather_tables(&encoded[..10]),
            Err(StageWeatherTablesError::Truncated {
                offset: 8,
                needed: 4,
                available: 2
            })
        );
        assert!(matches!(
            decode_stage_weather_tables(&[]),
            Err(StageWeatherTablesError::Truncated { offset: 0, .. })
        ));
        let mut trailing = encoded.clone();
        trailing.push(0);
        assert_eq!(
            decode_stage_weather_tables(&trailing),
            Err(StageWeatherTablesError::TrailingBytes {
                offset: STAGE_WEATHER_TABLES_ENCODED_LEN,
                len: 1
            })
        );
        for (flip, offset) in [(0, 0), (7, 0), (8, 8), (12, 12), (14, 14)] {
            let mut candidate = encoded.clone();
            candidate[flip] ^= 1;
            assert_eq!(
                decode_stage_weather_tables(&candidate),
                Err(StageWeatherTablesError::InvalidHeader { offset })
            );
        }
    }
}
