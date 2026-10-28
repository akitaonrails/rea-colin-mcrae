//! Authenticated extraction and payload-free encoding of the static rival
//! driver records (evidence `rea.official-eu.rival-records`).
//!
//! Observation: the official EU executable's `.data` section holds sixteen
//! `0x14`-byte records starting at VA `0x00506e40`. Dword `+0x00` of each
//! record is an absolute VA pointer to a NUL-terminated driver-name string
//! inside the image, and byte `+0x08` is a car/tier kind byte read by the
//! caller of `FUN_00465d40` and by the state-1 slot pass at `0x436da8` (which
//! tests it against 0, 6, 9 and 10).
//!
//! Unknown: the remaining record fields. Every record's full raw `0x14` bytes
//! are preserved unchanged, and the name is kept as raw bytes (no character
//! decoding beyond the NUL terminator). No value is range-checked. No record
//! payload is compiled into this crate; tests use synthetic fixtures only.

use std::fmt;

use crate::pe32::{Pe32Error, Pe32Image};

pub const RIVAL_RECORD_COUNT: usize = 16;
pub const RIVAL_RECORD_STRIDE: usize = 0x14;
pub const RIVAL_RECORD_NAME_POINTER_OFFSET: usize = 0x00;
pub const RIVAL_RECORD_KIND_OFFSET: usize = 0x08;
/// Upper bound on a name string, including its NUL terminator.
pub const RIVAL_NAME_MAX_BYTES: usize = 64;
pub const RIVAL_RECORDS_FORMAT_VERSION: u32 = 1;

const MAGIC: &[u8; 8] = b"CMRRIVL\0";
const HEADER_LEN: usize = 16;
/// Largest valid encoding: header plus every record with a maximal name.
pub const RIVAL_RECORDS_MAX_ENCODED_LEN: usize =
    HEADER_LEN + RIVAL_RECORD_COUNT * (RIVAL_RECORD_STRIDE + 1 + RIVAL_NAME_MAX_BYTES - 1);

/// Reviewed placement of the record array.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RivalRecordsLayout {
    pub image_base: u32,
    pub records_va: u32,
    pub record_count: u32,
    pub stride: u32,
}

pub const OFFICIAL_EU_RIVAL_RECORDS_LAYOUT: RivalRecordsLayout = RivalRecordsLayout {
    image_base: 0x0040_0000,
    records_va: 0x0050_6e40,
    record_count: RIVAL_RECORD_COUNT as u32,
    stride: RIVAL_RECORD_STRIDE as u32,
};

/// One rival record: its raw bytes, the resolved name (without the NUL) and
/// the kind byte (`raw[8]`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RivalRecord {
    pub raw: [u8; RIVAL_RECORD_STRIDE],
    pub name: Vec<u8>,
    pub kind: u8,
}

impl RivalRecord {
    /// A record whose `kind` is derived from `raw`.
    pub fn new(raw: [u8; RIVAL_RECORD_STRIDE], name: Vec<u8>) -> Self {
        Self {
            kind: raw[RIVAL_RECORD_KIND_OFFSET],
            raw,
            name,
        }
    }
    /// The raw dword at `+0x00`: the name's VA in the source image.
    pub fn name_va(&self) -> u32 {
        u32::from_le_bytes(
            self.raw[RIVAL_RECORD_NAME_POINTER_OFFSET..RIVAL_RECORD_NAME_POINTER_OFFSET + 4]
                .try_into()
                .expect("bounded field"),
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RivalRecords {
    pub records: [RivalRecord; RIVAL_RECORD_COUNT],
}

impl RivalRecords {
    /// Build from a vector checked to hold exactly sixteen records.
    pub fn from_vec(records: Vec<RivalRecord>) -> Result<Self, RivalRecordsError> {
        let actual = records.len();
        let records = records
            .try_into()
            .map_err(|_| RivalRecordsError::RecordCount {
                expected: RIVAL_RECORD_COUNT,
                actual,
            })?;
        Ok(Self { records })
    }
    pub fn get(&self, index: usize) -> Option<&RivalRecord> {
        self.records.get(index)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RivalRecordsError {
    Pe(String),
    ImageBase {
        expected: u32,
        actual: u32,
    },
    /// The record count or stride is not the reviewed one.
    UnsupportedLayout,
    Misaligned {
        virtual_address: u32,
        alignment: u32,
    },
    /// A record's name pointer does not resolve inside the image.
    BadNamePointer {
        record: usize,
        virtual_address: u32,
    },
    /// No NUL within [`RIVAL_NAME_MAX_BYTES`] mapped bytes of the name start.
    UnterminatedName {
        record: usize,
        virtual_address: u32,
    },
    RecordCount {
        expected: usize,
        actual: usize,
    },
    /// A stored payload is not the length its import state recorded.
    WrongLength {
        expected: usize,
        actual: usize,
    },
    /// An encoded name is too long or holds a NUL byte.
    InvalidName {
        offset: usize,
    },
    /// An encoded record's `kind` disagrees with its raw byte `+0x08`.
    KindMismatch {
        record: usize,
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

impl fmt::Display for RivalRecordsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pe(e) => write!(f, "invalid PE32 image: {e}"),
            Self::ImageBase { expected, actual } => write!(
                f,
                "image base mismatch: expected {expected:#x}, got {actual:#x}"
            ),
            Self::UnsupportedLayout => write!(f, "unsupported rival-record layout"),
            Self::Misaligned {
                virtual_address,
                alignment,
            } => write!(
                f,
                "record address {virtual_address:#x} is not {alignment}-byte aligned"
            ),
            Self::BadNamePointer {
                record,
                virtual_address,
            } => write!(
                f,
                "rival record {record}: name pointer {virtual_address:#x} is outside the image"
            ),
            Self::UnterminatedName {
                record,
                virtual_address,
            } => write!(
                f,
                "rival record {record}: name at {virtual_address:#x} has no NUL within {RIVAL_NAME_MAX_BYTES} bytes"
            ),
            Self::RecordCount { expected, actual } => {
                write!(f, "expected {expected} rival records, got {actual}")
            }
            Self::WrongLength { expected, actual } => {
                write!(f, "wrong encoded length: expected {expected}, got {actual}")
            }
            Self::InvalidName { offset } => write!(f, "invalid encoded name at offset {offset}"),
            Self::KindMismatch { record } => {
                write!(f, "rival record {record}: kind does not match raw byte +8")
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
                write!(f, "invalid rival-record header at offset {offset}")
            }
        }
    }
}
impl std::error::Error for RivalRecordsError {}

fn pe_error(error: Pe32Error) -> RivalRecordsError {
    RivalRecordsError::Pe(format!("{error:?}"))
}

fn check_layout(layout: &RivalRecordsLayout) -> Result<(), RivalRecordsError> {
    if layout.record_count != RIVAL_RECORD_COUNT as u32
        || layout.stride != RIVAL_RECORD_STRIDE as u32
    {
        return Err(RivalRecordsError::UnsupportedLayout);
    }
    if !layout.records_va.is_multiple_of(4) {
        return Err(RivalRecordsError::Misaligned {
            virtual_address: layout.records_va,
            alignment: 4,
        });
    }
    Ok(())
}

/// Resolve the NUL-terminated name at `va`, byte by byte, so a string that
/// runs off its section is reported rather than read past.
fn resolve_name(
    image: &Pe32Image<'_>,
    record: usize,
    va: u32,
) -> Result<Vec<u8>, RivalRecordsError> {
    let mut name = Vec::new();
    for i in 0..RIVAL_NAME_MAX_BYTES {
        let at = va.checked_add(i as u32);
        let byte = at.and_then(|at| image.read_va_range(at, 1).ok());
        match byte {
            Some([0]) => return Ok(name),
            Some(&[b]) => name.push(b),
            _ if i == 0 => {
                return Err(RivalRecordsError::BadNamePointer {
                    record,
                    virtual_address: va,
                });
            }
            _ => break,
        }
    }
    Err(RivalRecordsError::UnterminatedName {
        record,
        virtual_address: va,
    })
}

pub fn extract_rival_records(
    input: &[u8],
    layout: &RivalRecordsLayout,
) -> Result<RivalRecords, RivalRecordsError> {
    check_layout(layout)?;
    let image = Pe32Image::parse(input).map_err(pe_error)?;
    if image.image_base() != layout.image_base {
        return Err(RivalRecordsError::ImageBase {
            expected: layout.image_base,
            actual: image.image_base(),
        });
    }
    let array = image
        .read_va_range(layout.records_va, RIVAL_RECORD_COUNT * RIVAL_RECORD_STRIDE)
        .map_err(pe_error)?;
    let mut records = Vec::with_capacity(RIVAL_RECORD_COUNT);
    for (index, &raw) in array
        .as_chunks::<RIVAL_RECORD_STRIDE>()
        .0
        .iter()
        .enumerate()
    {
        let pointer = u32::from_le_bytes(raw[..4].try_into().expect("bounded field"));
        let name = resolve_name(&image, index, pointer)?;
        records.push(RivalRecord::new(raw, name));
    }
    RivalRecords::from_vec(records)
}

/// Encode `cmr-rival-records-v1`: a 16-byte header (magic, version, record
/// count `u16`, stride `u16`) then, per record, its raw `0x14` bytes, a `u8`
/// name length and the name bytes (no NUL).
pub fn encode_rival_records(records: &RivalRecords) -> Vec<u8> {
    let mut out = Vec::with_capacity(RIVAL_RECORDS_MAX_ENCODED_LEN);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&RIVAL_RECORDS_FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&(RIVAL_RECORD_COUNT as u16).to_le_bytes());
    out.extend_from_slice(&(RIVAL_RECORD_STRIDE as u16).to_le_bytes());
    for record in &records.records {
        debug_assert!(record.name.len() < RIVAL_NAME_MAX_BYTES);
        out.extend_from_slice(&record.raw);
        out.push(record.name.len() as u8);
        out.extend_from_slice(&record.name);
    }
    out
}

struct Cursor<'a> {
    input: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, needed: usize) -> Result<&'a [u8], RivalRecordsError> {
        let available = self.input.len() - self.offset;
        if needed > available {
            return Err(RivalRecordsError::Truncated {
                offset: self.offset,
                needed,
                available,
            });
        }
        let bytes = &self.input[self.offset..self.offset + needed];
        self.offset += needed;
        Ok(bytes)
    }
    fn u8(&mut self) -> Result<u8, RivalRecordsError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, RivalRecordsError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, RivalRecordsError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
}

pub fn decode_rival_records(input: &[u8]) -> Result<RivalRecords, RivalRecordsError> {
    let mut cursor = Cursor { input, offset: 0 };
    if cursor.take(8)? != MAGIC {
        return Err(RivalRecordsError::InvalidHeader { offset: 0 });
    }
    if cursor.u32()? != RIVAL_RECORDS_FORMAT_VERSION {
        return Err(RivalRecordsError::InvalidHeader { offset: 8 });
    }
    if cursor.u16()? != RIVAL_RECORD_COUNT as u16 {
        return Err(RivalRecordsError::InvalidHeader { offset: 12 });
    }
    if cursor.u16()? != RIVAL_RECORD_STRIDE as u16 {
        return Err(RivalRecordsError::InvalidHeader { offset: 14 });
    }
    let mut records = Vec::with_capacity(RIVAL_RECORD_COUNT);
    for _ in 0..RIVAL_RECORD_COUNT {
        let raw: [u8; RIVAL_RECORD_STRIDE] = cursor.take(RIVAL_RECORD_STRIDE)?.try_into().unwrap();
        let len_offset = cursor.offset;
        let len = cursor.u8()? as usize;
        if len >= RIVAL_NAME_MAX_BYTES {
            return Err(RivalRecordsError::InvalidName { offset: len_offset });
        }
        let name_offset = cursor.offset;
        let name = cursor.take(len)?;
        if let Some(nul) = name.iter().position(|&b| b == 0) {
            return Err(RivalRecordsError::InvalidName {
                offset: name_offset + nul,
            });
        }
        records.push(RivalRecord::new(raw, name.to_vec()));
    }
    if cursor.offset != input.len() {
        return Err(RivalRecordsError::TrailingBytes {
            offset: cursor.offset,
            len: input.len() - cursor.offset,
        });
    }
    RivalRecords::from_vec(records)
}

/// Check that every record's `kind` is its raw byte `+0x08`.
pub fn check_rival_record_kinds(records: &RivalRecords) -> Result<(), RivalRecordsError> {
    for (record, r) in records.records.iter().enumerate() {
        if r.kind != r.raw[RIVAL_RECORD_KIND_OFFSET] {
            return Err(RivalRecordsError::KindMismatch { record });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PE: usize = 0x80;
    const OPTIONAL: usize = PE + 24;
    const SECTIONS: usize = OPTIONAL + 0xe0;
    const RAW: usize = 0x400;
    /// One synthetic section covering the record array and its names.
    const SECTION_RVA: u32 = 0x0010_6000;
    const SECTION_SIZE: usize = 0x1000;
    const SECTION_END_VA: u32 = 0x0040_0000 + SECTION_RVA + SECTION_SIZE as u32;
    /// Synthetic name area inside the section.
    const NAMES_VA: u32 = 0x0050_6100;

    fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    fn file_offset(va: u32) -> usize {
        RAW + (va - 0x0040_0000 - SECTION_RVA) as usize
    }
    fn name_va(i: usize) -> u32 {
        NAMES_VA + (i as u32) * 0x20
    }
    /// Synthetic placeholder name; not game data.
    fn synthetic_name(i: usize) -> Vec<u8> {
        format!("Rival{i:02}").into_bytes()
    }

    /// Synthetic records whose pointers target [`synthetic_name`] strings.
    fn synthetic_records() -> RivalRecords {
        let records = (0..RIVAL_RECORD_COUNT)
            .map(|i| {
                let mut raw = [0u8; RIVAL_RECORD_STRIDE];
                raw[..4].copy_from_slice(&name_va(i).to_le_bytes());
                for (j, b) in raw.iter_mut().enumerate().skip(4) {
                    *b = (0x40 + i * 3 + j) as u8;
                }
                raw[RIVAL_RECORD_KIND_OFFSET] = (i % 11) as u8;
                RivalRecord::new(raw, synthetic_name(i))
            })
            .collect();
        RivalRecords::from_vec(records).unwrap()
    }

    fn synthetic_pe(records: &RivalRecords) -> Vec<u8> {
        let l = OFFICIAL_EU_RIVAL_RECORDS_LAYOUT;
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
        for (i, r) in records.records.iter().enumerate() {
            let at = file_offset(l.records_va) + i * RIVAL_RECORD_STRIDE;
            bytes[at..at + RIVAL_RECORD_STRIDE].copy_from_slice(&r.raw);
            let name = file_offset(r.name_va());
            bytes[name..name + r.name.len()].copy_from_slice(&r.name);
            bytes[name + r.name.len()] = 0;
        }
        bytes
    }

    fn set_pointer(bytes: &mut [u8], record: usize, va: u32) {
        let at =
            file_offset(OFFICIAL_EU_RIVAL_RECORDS_LAYOUT.records_va) + record * RIVAL_RECORD_STRIDE;
        put_u32(bytes, at, va);
    }

    #[test]
    fn reviewed_layout_spans_sixteen_records() {
        let l = OFFICIAL_EU_RIVAL_RECORDS_LAYOUT;
        check_layout(&l).unwrap();
        assert_eq!(l.records_va + l.record_count * l.stride, 0x0050_6f80);
        assert_eq!(RIVAL_RECORDS_MAX_ENCODED_LEN, 16 + 16 * (0x14 + 1 + 63));
    }

    #[test]
    fn extracts_raw_records_names_and_kinds() {
        let records = synthetic_records();
        let got = extract_rival_records(&synthetic_pe(&records), &OFFICIAL_EU_RIVAL_RECORDS_LAYOUT)
            .unwrap();
        assert_eq!(got, records);
        for (i, r) in got.records.iter().enumerate() {
            assert_eq!(r.kind, r.raw[8]);
            assert_eq!(r.kind, (i % 11) as u8);
            assert_eq!(r.name_va(), name_va(i));
        }
        assert_eq!(got.get(15).unwrap().name, synthetic_name(15));
        assert!(got.get(16).is_none());
    }

    #[test]
    fn accepts_empty_and_maximal_names_preserving_raw_bytes() {
        let mut records = synthetic_records();
        records.records[0].name.clear();
        records.records[1].name = vec![0xe9; RIVAL_NAME_MAX_BYTES - 1];
        // The maximal name needs more room than the default 0x20 slot.
        let wide = NAMES_VA + 0x400;
        records.records[1].raw[..4].copy_from_slice(&wide.to_le_bytes());
        let got = extract_rival_records(&synthetic_pe(&records), &OFFICIAL_EU_RIVAL_RECORDS_LAYOUT)
            .unwrap();
        assert_eq!(got, records);
    }

    #[test]
    fn rejects_bad_name_pointers() {
        let records = synthetic_records();
        let l = OFFICIAL_EU_RIVAL_RECORDS_LAYOUT;
        for bad in [0, 0x0040_1000, SECTION_END_VA, u32::MAX] {
            let mut bytes = synthetic_pe(&records);
            set_pointer(&mut bytes, 7, bad);
            assert_eq!(
                extract_rival_records(&bytes, &l),
                Err(RivalRecordsError::BadNamePointer {
                    record: 7,
                    virtual_address: bad
                })
            );
        }
    }

    #[test]
    fn rejects_missing_terminators() {
        let records = synthetic_records();
        let l = OFFICIAL_EU_RIVAL_RECORDS_LAYOUT;
        // 64 non-NUL bytes: no terminator within the bound.
        let mut bytes = synthetic_pe(&records);
        let at = NAMES_VA + 0x600;
        bytes[file_offset(at)..file_offset(at) + RIVAL_NAME_MAX_BYTES].fill(b'x');
        bytes[file_offset(at) + RIVAL_NAME_MAX_BYTES] = 0;
        set_pointer(&mut bytes, 3, at);
        assert_eq!(
            extract_rival_records(&bytes, &l),
            Err(RivalRecordsError::UnterminatedName {
                record: 3,
                virtual_address: at
            })
        );
        // A string that runs off the end of its section.
        let mut bytes = synthetic_pe(&records);
        let tail = SECTION_END_VA - 4;
        let off = file_offset(tail);
        bytes[off..off + 4].fill(b'y');
        set_pointer(&mut bytes, 15, tail);
        assert_eq!(
            extract_rival_records(&bytes, &l),
            Err(RivalRecordsError::UnterminatedName {
                record: 15,
                virtual_address: tail
            })
        );
        // The last name byte inside the bound may be the NUL.
        let mut bytes = synthetic_pe(&records);
        bytes[file_offset(at)..file_offset(at) + RIVAL_NAME_MAX_BYTES - 1].fill(b'x');
        bytes[file_offset(at) + RIVAL_NAME_MAX_BYTES - 1] = 0;
        set_pointer(&mut bytes, 3, at);
        assert_eq!(
            extract_rival_records(&bytes, &l).unwrap().records[3]
                .name
                .len(),
            RIVAL_NAME_MAX_BYTES - 1
        );
    }

    #[test]
    fn rejects_truncation_wrong_image_and_bad_layouts() {
        let l = OFFICIAL_EU_RIVAL_RECORDS_LAYOUT;
        let bytes = synthetic_pe(&synthetic_records());
        let end = file_offset(l.records_va) + RIVAL_RECORD_COUNT * RIVAL_RECORD_STRIDE;
        assert!(matches!(
            extract_rival_records(&bytes[..end - 1], &l),
            Err(RivalRecordsError::Pe(text)) if text.contains("Truncated")
        ));
        let mut tail = l;
        tail.records_va = SECTION_END_VA - 0x10;
        assert!(matches!(
            extract_rival_records(&bytes, &tail),
            Err(RivalRecordsError::Pe(text)) if text.contains("UnmappedRange")
        ));
        let mut wrong_base = l;
        wrong_base.image_base += 0x1000;
        assert_eq!(
            extract_rival_records(&bytes, &wrong_base),
            Err(RivalRecordsError::ImageBase {
                expected: 0x0040_1000,
                actual: 0x0040_0000
            })
        );
        let mut misaligned = l;
        misaligned.records_va += 2;
        assert_eq!(
            extract_rival_records(&bytes, &misaligned),
            Err(RivalRecordsError::Misaligned {
                virtual_address: l.records_va + 2,
                alignment: 4
            })
        );
        for (count, stride) in [(15, 0x14), (17, 0x14), (16, 0x10), (16, 0x18)] {
            let mut m = l;
            m.record_count = count;
            m.stride = stride;
            assert_eq!(
                extract_rival_records(&bytes, &m),
                Err(RivalRecordsError::UnsupportedLayout)
            );
        }
        assert!(matches!(
            extract_rival_records(b"MZ", &l),
            Err(RivalRecordsError::Pe(_))
        ));
    }

    #[test]
    fn from_vec_requires_exactly_sixteen() {
        let mut v = synthetic_records().records.to_vec();
        v.pop();
        assert_eq!(
            RivalRecords::from_vec(v.clone()),
            Err(RivalRecordsError::RecordCount {
                expected: 16,
                actual: 15
            })
        );
        v.push(v[0].clone());
        v.push(v[0].clone());
        assert!(matches!(
            RivalRecords::from_vec(v),
            Err(RivalRecordsError::RecordCount { actual: 17, .. })
        ));
    }

    #[test]
    fn codec_round_trips_and_derives_kind() {
        let records = synthetic_records();
        check_rival_record_kinds(&records).unwrap();
        let encoded = encode_rival_records(&records);
        assert_eq!(&encoded[..8], MAGIC);
        assert_eq!(&encoded[16..36], &records.records[0].raw);
        assert_eq!(encoded[36] as usize, records.records[0].name.len());
        assert_eq!(decode_rival_records(&encoded).unwrap(), records);
        let mut bad = records.clone();
        bad.records[4].kind ^= 1;
        assert_eq!(
            check_rival_record_kinds(&bad),
            Err(RivalRecordsError::KindMismatch { record: 4 })
        );
    }

    #[test]
    fn decode_reports_truncation_trailing_names_and_header_offsets() {
        let encoded = encode_rival_records(&synthetic_records());
        let last_name = synthetic_name(15).len();
        assert_eq!(
            decode_rival_records(&encoded[..encoded.len() - 1]),
            Err(RivalRecordsError::Truncated {
                offset: encoded.len() - last_name,
                needed: last_name,
                available: last_name - 1
            })
        );
        assert_eq!(
            decode_rival_records(&encoded[..10]),
            Err(RivalRecordsError::Truncated {
                offset: 8,
                needed: 4,
                available: 2
            })
        );
        assert!(matches!(
            decode_rival_records(&[]),
            Err(RivalRecordsError::Truncated { offset: 0, .. })
        ));
        let mut trailing = encoded.clone();
        trailing.push(0);
        assert_eq!(
            decode_rival_records(&trailing),
            Err(RivalRecordsError::TrailingBytes {
                offset: encoded.len(),
                len: 1
            })
        );
        for (flip, offset) in [(0, 0), (7, 0), (8, 8), (12, 12), (14, 14)] {
            let mut candidate = encoded.clone();
            candidate[flip] ^= 1;
            assert_eq!(
                decode_rival_records(&candidate),
                Err(RivalRecordsError::InvalidHeader { offset })
            );
        }
        // First record's name length byte at 16 + 0x14.
        let mut long = encoded.clone();
        long[36] = RIVAL_NAME_MAX_BYTES as u8;
        assert_eq!(
            decode_rival_records(&long),
            Err(RivalRecordsError::InvalidName { offset: 36 })
        );
        let mut nul = encoded.clone();
        nul[38] = 0;
        assert_eq!(
            decode_rival_records(&nul),
            Err(RivalRecordsError::InvalidName { offset: 38 })
        );
    }
}
