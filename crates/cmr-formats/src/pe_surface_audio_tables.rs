//! Authenticated extraction and payload-free encoding of the static in-race
//! surface / skid / impact audio tables.
//!
//! Observation: the surface-noise and prang code reads these `.data` tables of
//! the official EU executable only through absolute-address operands; no store
//! instruction targets them, so they are read-only at runtime (the four
//! `initial_last_state` words are the exception: they are runtime state whose
//! static image value is the initial sentinel).
//!
//! - `u8[120][4]` per sound group, byte order `{ski, i, sko, o}`: variant
//!   counts. The runtime slot maps are 6 entries wide, so every count is
//!   bounded by [`SURFACE_VARIANT_SLOTS`].
//! - `i8[120][3]`: volume percent per camera class (read with `movsx`).
//! - `i32[120]`: surface id → sound group, every group in `0..120`.
//! - `i32[15]`: wet surface ids.
//! - four scalar `i32` factors and `i32[4]` initial last-group/last-slot state.
//! - 48 `char*` pointers to NUL-terminated ASCII prang/accident wave names.
//!
//! The meaning of the numeric values is not asserted here. No table payload is
//! compiled into this crate; tests use synthetic fixtures only. Wave names are
//! file names (metadata), kept as strings.

use std::fmt;

use crate::pe32::{Pe32Error, Pe32Image};

pub const SURFACE_AUDIO_GROUP_COUNT: usize = 120;
/// Width of the runtime per-group slot maps; bounds every variant count.
pub const SURFACE_VARIANT_SLOTS: u8 = 6;
pub const SURFACE_CAMERA_CLASS_COUNT: usize = 3;
pub const SURFACE_WET_SURFACE_COUNT: usize = 15;
pub const SURFACE_INITIAL_LAST_STATE_COUNT: usize = 4;
pub const PRANG_NAME_COUNT: usize = 48;
/// Bytes scanned for a name's NUL terminator; a name holds at most
/// `PRANG_NAME_MAX_BYTES - 1` characters.
pub const PRANG_NAME_MAX_BYTES: usize = 32;
pub const SURFACE_AUDIO_TABLES_FORMAT_VERSION: u32 = 1;

const VARIANT_RECORD_BYTES: usize = 4;
const MAGIC: &[u8; 8] = b"CMRSRFA\0";
const HEADER_LEN: usize = 16;
const COUNTS_LEN: usize = SURFACE_AUDIO_GROUP_COUNT * VARIANT_RECORD_BYTES;
const VOLUMES_LEN: usize = SURFACE_AUDIO_GROUP_COUNT * SURFACE_CAMERA_CLASS_COUNT;
const GROUPS_LEN: usize = SURFACE_AUDIO_GROUP_COUNT * 4;
const WET_LEN: usize = SURFACE_WET_SURFACE_COUNT * 4;
const SCALARS_LEN: usize = 4 * 4;
const LAST_STATE_LEN: usize = SURFACE_INITIAL_LAST_STATE_COUNT * 4;
const POINTERS_LEN: usize = PRANG_NAME_COUNT * 4;

const COUNTS_AT: usize = HEADER_LEN;
const VOLUMES_AT: usize = COUNTS_AT + COUNTS_LEN;
const GROUPS_AT: usize = VOLUMES_AT + VOLUMES_LEN;
const WET_AT: usize = GROUPS_AT + GROUPS_LEN;
const SCALARS_AT: usize = WET_AT + WET_LEN;
const LAST_STATE_AT: usize = SCALARS_AT + SCALARS_LEN;
const NAME_COUNT_AT: usize = LAST_STATE_AT + LAST_STATE_LEN;
const NAMES_AT: usize = NAME_COUNT_AT + 4;
/// Encoded name slot: one length byte, then the name padded with zero bytes.
const NAME_SLOT_BYTES: usize = PRANG_NAME_MAX_BYTES;
pub const SURFACE_AUDIO_TABLES_ENCODED_LEN: usize = NAMES_AT + PRANG_NAME_COUNT * NAME_SLOT_BYTES;

/// One group's variant counts, in image byte order `{ski, i, sko, o}`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SurfaceVariantCounts {
    /// Interior skid.
    pub ski: u8,
    /// Interior roll.
    pub i: u8,
    /// Exterior skid.
    pub sko: u8,
    /// Exterior roll.
    pub o: u8,
}

/// Which variant byte of a group record an error refers to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SurfaceVariant {
    Ski,
    I,
    Sko,
    O,
}

impl SurfaceVariant {
    pub const ALL: [Self; 4] = [Self::Ski, Self::I, Self::Sko, Self::O];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Ski => "ski",
            Self::I => "i",
            Self::Sko => "sko",
            Self::O => "o",
        }
    }
}

impl fmt::Display for SurfaceVariant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl SurfaceVariantCounts {
    fn from_bytes(b: [u8; 4]) -> Self {
        Self {
            ski: b[0],
            i: b[1],
            sko: b[2],
            o: b[3],
        }
    }
    fn to_bytes(self) -> [u8; 4] {
        [self.ski, self.i, self.sko, self.o]
    }
    pub fn get(self, variant: SurfaceVariant) -> u8 {
        match variant {
            SurfaceVariant::Ski => self.ski,
            SurfaceVariant::I => self.i,
            SurfaceVariant::Sko => self.sko,
            SurfaceVariant::O => self.o,
        }
    }
}

/// Virtual addresses of every reviewed table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SurfaceAudioTablesLayout {
    pub image_base: u32,
    pub variant_counts_va: u32,
    pub class_volume_va: u32,
    pub surface_group_va: u32,
    pub wet_surfaces_va: u32,
    pub surface_volume_k_va: u32,
    pub wet_volume_k_va: u32,
    pub crowd_volume_k_va: u32,
    pub prang_full_volume_va: u32,
    pub initial_last_state_va: u32,
    pub prang_names_va: u32,
}

impl SurfaceAudioTablesLayout {
    /// `(name, va, byte length)` of every table span.
    fn spans(&self) -> [(&'static str, u32, usize); 10] {
        [
            ("variant_counts", self.variant_counts_va, COUNTS_LEN),
            ("class_volume", self.class_volume_va, VOLUMES_LEN),
            ("surface_group", self.surface_group_va, GROUPS_LEN),
            ("wet_surfaces", self.wet_surfaces_va, WET_LEN),
            ("surface_volume_k", self.surface_volume_k_va, 4),
            ("wet_volume_k", self.wet_volume_k_va, 4),
            ("crowd_volume_k", self.crowd_volume_k_va, 4),
            ("prang_full_volume", self.prang_full_volume_va, 4),
            (
                "initial_last_state",
                self.initial_last_state_va,
                LAST_STATE_LEN,
            ),
            ("prang_names", self.prang_names_va, POINTERS_LEN),
        ]
    }
}

pub const OFFICIAL_EU_SURFACE_AUDIO_TABLES_LAYOUT: SurfaceAudioTablesLayout =
    SurfaceAudioTablesLayout {
        image_base: 0x0040_0000,
        variant_counts_va: 0x004f_9408,
        class_volume_va: 0x004f_95e8,
        surface_group_va: 0x004f_9750,
        wet_surfaces_va: 0x004f_9930,
        surface_volume_k_va: 0x004f_996c,
        wet_volume_k_va: 0x0050_20e4,
        crowd_volume_k_va: 0x004e_b2b0,
        prang_full_volume_va: 0x004f_41a8,
        initial_last_state_va: 0x004f_9970,
        prang_names_va: 0x004f_40e0,
    };

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SurfaceAudioTables {
    pub counts: [SurfaceVariantCounts; SURFACE_AUDIO_GROUP_COUNT],
    pub class_volume_percent: [[i8; SURFACE_CAMERA_CLASS_COUNT]; SURFACE_AUDIO_GROUP_COUNT],
    pub surface_group: [i32; SURFACE_AUDIO_GROUP_COUNT],
    pub wet_surfaces: [i32; SURFACE_WET_SURFACE_COUNT],
    pub surface_volume_k: i32,
    pub wet_volume_k: i32,
    pub crowd_volume_k: i32,
    pub prang_full_volume: i32,
    pub initial_last_state: [i32; SURFACE_INITIAL_LAST_STATE_COUNT],
    /// Exactly [`PRANG_NAME_COUNT`] wave file names, in pointer-table order.
    pub prang_names: Vec<String>,
}

impl SurfaceAudioTables {
    /// Re-check every invariant (used before encoding and after decode).
    /// Offsets in errors are relative to the start of each table.
    pub fn validate(&self) -> Result<(), SurfaceAudioTablesError> {
        validate_values(&self.counts, &self.surface_group, 0, 0)?;
        if self.prang_names.len() != PRANG_NAME_COUNT {
            return Err(SurfaceAudioTablesError::NameCount {
                expected: PRANG_NAME_COUNT,
                actual: self.prang_names.len(),
            });
        }
        for (index, name) in self.prang_names.iter().enumerate() {
            check_name(index, name.as_bytes(), 0)?;
        }
        Ok(())
    }
}

/// Range-check the variant counts and surface groups. `counts_at` and
/// `groups_at` locate the tables for error offsets: virtual addresses when
/// extracting, encoded offsets when decoding, zero from `validate`.
fn validate_values(
    counts: &[SurfaceVariantCounts; SURFACE_AUDIO_GROUP_COUNT],
    surface_group: &[i32; SURFACE_AUDIO_GROUP_COUNT],
    counts_at: usize,
    groups_at: usize,
) -> Result<(), SurfaceAudioTablesError> {
    for (group, record) in counts.iter().enumerate() {
        for (byte, variant) in SurfaceVariant::ALL.into_iter().enumerate() {
            let count = record.get(variant);
            if count > SURFACE_VARIANT_SLOTS {
                return Err(SurfaceAudioTablesError::VariantCount {
                    group,
                    variant,
                    count,
                    offset: counts_at + group * VARIANT_RECORD_BYTES + byte,
                });
            }
        }
    }
    for (surface, &group) in surface_group.iter().enumerate() {
        if !(0..SURFACE_AUDIO_GROUP_COUNT as i32).contains(&group) {
            return Err(SurfaceAudioTablesError::SurfaceGroup {
                surface,
                group,
                offset: groups_at + surface * 4,
            });
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SurfaceAudioTablesError {
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
    /// A variant count exceeds the 6-wide runtime slot map.
    VariantCount {
        group: usize,
        variant: SurfaceVariant,
        count: u8,
        offset: usize,
    },
    /// A surface maps to a group outside `0..120`.
    SurfaceGroup {
        surface: usize,
        group: i32,
        offset: usize,
    },
    NameCount {
        expected: usize,
        actual: usize,
    },
    /// No NUL terminator within [`PRANG_NAME_MAX_BYTES`] bytes.
    NameUnterminated {
        index: usize,
        offset: usize,
    },
    NameEmpty {
        index: usize,
        offset: usize,
    },
    /// A name byte (or encoded length/padding byte) is not allowed.
    NameByte {
        index: usize,
        offset: usize,
        byte: u8,
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

impl fmt::Display for SurfaceAudioTablesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pe(e) => write!(f, "invalid PE32 image: {e}"),
            Self::ImageBase { expected, actual } => write!(
                f,
                "image base mismatch: expected {expected:#x}, got {actual:#x}"
            ),
            Self::UnsupportedLayout => write!(f, "unsupported surface-audio-table layout"),
            Self::Misaligned {
                virtual_address,
                alignment,
            } => write!(
                f,
                "table address {virtual_address:#x} is not {alignment}-byte aligned"
            ),
            Self::VariantCount {
                group,
                variant,
                count,
                offset,
            } => write!(
                f,
                "group {group} {variant} count {count} at offset {offset:#x} exceeds {SURFACE_VARIANT_SLOTS}"
            ),
            Self::SurfaceGroup {
                surface,
                group,
                offset,
            } => write!(
                f,
                "surface {surface} group {group} at offset {offset:#x} is outside 0..{SURFACE_AUDIO_GROUP_COUNT}"
            ),
            Self::NameCount { expected, actual } => {
                write!(f, "{actual} prang names, expected {expected}")
            }
            Self::NameUnterminated { index, offset } => write!(
                f,
                "prang name {index} at offset {offset:#x} has no NUL within {PRANG_NAME_MAX_BYTES} bytes"
            ),
            Self::NameEmpty { index, offset } => {
                write!(f, "prang name {index} at offset {offset:#x} is empty")
            }
            Self::NameByte {
                index,
                offset,
                byte,
            } => write!(
                f,
                "prang name {index} has invalid byte {byte:#04x} at offset {offset:#x}"
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
                write!(f, "invalid surface-audio-table header at offset {offset}")
            }
        }
    }
}
impl std::error::Error for SurfaceAudioTablesError {}

fn pe_error(error: Pe32Error) -> SurfaceAudioTablesError {
    SurfaceAudioTablesError::Pe(format!("{error:?}"))
}

const fn is_name_byte(b: u8) -> bool {
    matches!(b, 0x20..=0x7e)
}

/// `bytes` excludes the terminator; `offset` is where the name starts.
fn check_name(index: usize, bytes: &[u8], offset: usize) -> Result<(), SurfaceAudioTablesError> {
    if bytes.is_empty() {
        return Err(SurfaceAudioTablesError::NameEmpty { index, offset });
    }
    if bytes.len() >= PRANG_NAME_MAX_BYTES {
        return Err(SurfaceAudioTablesError::NameUnterminated { index, offset });
    }
    if let Some(at) = bytes.iter().position(|&b| !is_name_byte(b)) {
        return Err(SurfaceAudioTablesError::NameByte {
            index,
            offset: offset + at,
            byte: bytes[at],
        });
    }
    Ok(())
}

fn i32_at(bytes: &[u8], at: usize) -> i32 {
    i32::from_le_bytes(bytes[at..at + 4].try_into().expect("bounded span"))
}

fn i32_array<const N: usize>(bytes: &[u8]) -> [i32; N] {
    std::array::from_fn(|i| i32_at(bytes, i * 4))
}

fn parse_counts(bytes: &[u8]) -> [SurfaceVariantCounts; SURFACE_AUDIO_GROUP_COUNT] {
    std::array::from_fn(|g| {
        let at = g * VARIANT_RECORD_BYTES;
        SurfaceVariantCounts::from_bytes(bytes[at..at + 4].try_into().expect("bounded span"))
    })
}

fn parse_volumes(bytes: &[u8]) -> [[i8; SURFACE_CAMERA_CLASS_COUNT]; SURFACE_AUDIO_GROUP_COUNT] {
    std::array::from_fn(|g| {
        std::array::from_fn(|c| bytes[g * SURFACE_CAMERA_CLASS_COUNT + c] as i8)
    })
}

fn check_layout(layout: &SurfaceAudioTablesLayout) -> Result<(), SurfaceAudioTablesError> {
    let spans = layout.spans();
    for (_, va, _) in spans {
        if !va.is_multiple_of(4) {
            return Err(SurfaceAudioTablesError::Misaligned {
                virtual_address: va,
                alignment: 4,
            });
        }
    }
    for (i, &(_, a, a_len)) in spans.iter().enumerate() {
        for &(_, b, b_len) in &spans[i + 1..] {
            let (a, b) = (u64::from(a), u64::from(b));
            if a < b + b_len as u64 && b < a + a_len as u64 {
                return Err(SurfaceAudioTablesError::UnsupportedLayout);
            }
        }
    }
    Ok(())
}

/// Read the NUL-terminated name at `va`, scanning at most
/// [`PRANG_NAME_MAX_BYTES`] bytes and never past the section's raw data.
fn read_name(
    image: &Pe32Image<'_>,
    index: usize,
    va: u32,
) -> Result<String, SurfaceAudioTablesError> {
    let mut bytes = Vec::with_capacity(PRANG_NAME_MAX_BYTES);
    for i in 0..PRANG_NAME_MAX_BYTES as u32 {
        let at = va
            .checked_add(i)
            .ok_or(SurfaceAudioTablesError::NameUnterminated {
                index,
                offset: va as usize,
            })?;
        let byte = image.read_va_range(at, 1).map_err(pe_error)?[0];
        if byte == 0 {
            check_name(index, &bytes, va as usize)?;
            return Ok(String::from_utf8(bytes).expect("printable ASCII"));
        }
        if !is_name_byte(byte) {
            return Err(SurfaceAudioTablesError::NameByte {
                index,
                offset: at as usize,
                byte,
            });
        }
        bytes.push(byte);
    }
    Err(SurfaceAudioTablesError::NameUnterminated {
        index,
        offset: va as usize,
    })
}

pub fn extract_surface_audio_tables(
    input: &[u8],
    layout: &SurfaceAudioTablesLayout,
) -> Result<SurfaceAudioTables, SurfaceAudioTablesError> {
    check_layout(layout)?;
    let image = Pe32Image::parse(input).map_err(pe_error)?;
    if image.image_base() != layout.image_base {
        return Err(SurfaceAudioTablesError::ImageBase {
            expected: layout.image_base,
            actual: image.image_base(),
        });
    }
    let read = |va: u32, len: usize| image.read_va_range(va, len).map_err(pe_error);
    let scalar = |va: u32| read(va, 4).map(|b| i32_at(b, 0));
    let counts = read(layout.variant_counts_va, COUNTS_LEN)?;
    let volumes = read(layout.class_volume_va, VOLUMES_LEN)?;
    let groups = read(layout.surface_group_va, GROUPS_LEN)?;
    let wet = read(layout.wet_surfaces_va, WET_LEN)?;
    let last = read(layout.initial_last_state_va, LAST_STATE_LEN)?;
    let pointers = read(layout.prang_names_va, POINTERS_LEN)?;
    let counts = parse_counts(counts);
    let surface_group = i32_array(groups);
    validate_values(
        &counts,
        &surface_group,
        layout.variant_counts_va as usize,
        layout.surface_group_va as usize,
    )?;
    let prang_names = (0..PRANG_NAME_COUNT)
        .map(|index| read_name(&image, index, i32_at(pointers, index * 4) as u32))
        .collect::<Result<_, _>>()?;
    Ok(SurfaceAudioTables {
        counts,
        class_volume_percent: parse_volumes(volumes),
        surface_group,
        wet_surfaces: i32_array(wet),
        surface_volume_k: scalar(layout.surface_volume_k_va)?,
        wet_volume_k: scalar(layout.wet_volume_k_va)?,
        crowd_volume_k: scalar(layout.crowd_volume_k_va)?,
        prang_full_volume: scalar(layout.prang_full_volume_va)?,
        initial_last_state: i32_array(last),
        prang_names,
    })
}

/// Encode `cmr-surface-audio-tables-v1`. Callers hold validated tables; an
/// invalid table is rejected rather than written.
pub fn encode_surface_audio_tables(
    tables: &SurfaceAudioTables,
) -> Result<Vec<u8>, SurfaceAudioTablesError> {
    tables.validate()?;
    let mut out = Vec::with_capacity(SURFACE_AUDIO_TABLES_ENCODED_LEN);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&SURFACE_AUDIO_TABLES_FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&(SURFACE_AUDIO_GROUP_COUNT as u32).to_le_bytes());
    for record in &tables.counts {
        out.extend_from_slice(&record.to_bytes());
    }
    for row in &tables.class_volume_percent {
        out.extend(row.iter().map(|&v| v as u8));
    }
    let scalars = [
        tables.surface_volume_k,
        tables.wet_volume_k,
        tables.crowd_volume_k,
        tables.prang_full_volume,
    ];
    let words = tables
        .surface_group
        .iter()
        .chain(&tables.wet_surfaces)
        .chain(&scalars)
        .chain(&tables.initial_last_state);
    for word in words {
        out.extend_from_slice(&word.to_le_bytes());
    }
    out.extend_from_slice(&(PRANG_NAME_COUNT as u32).to_le_bytes());
    for name in &tables.prang_names {
        let mut slot = [0_u8; NAME_SLOT_BYTES];
        slot[0] = name.len() as u8;
        slot[1..=name.len()].copy_from_slice(name.as_bytes());
        out.extend_from_slice(&slot);
    }
    debug_assert_eq!(out.len(), SURFACE_AUDIO_TABLES_ENCODED_LEN);
    Ok(out)
}

struct Cursor<'a> {
    input: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, needed: usize) -> Result<&'a [u8], SurfaceAudioTablesError> {
        let available = self.input.len() - self.offset;
        if needed > available {
            return Err(SurfaceAudioTablesError::Truncated {
                offset: self.offset,
                needed,
                available,
            });
        }
        let bytes = &self.input[self.offset..self.offset + needed];
        self.offset += needed;
        Ok(bytes)
    }
    fn u32(&mut self) -> Result<u32, SurfaceAudioTablesError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn i32(&mut self) -> Result<i32, SurfaceAudioTablesError> {
        Ok(self.u32()? as i32)
    }
}

/// Decode one encoded name slot that starts at `offset`.
fn decode_name(
    index: usize,
    slot: &[u8],
    offset: usize,
) -> Result<String, SurfaceAudioTablesError> {
    let len = slot[0] as usize;
    if len == 0 {
        return Err(SurfaceAudioTablesError::NameEmpty { index, offset });
    }
    if len >= NAME_SLOT_BYTES {
        return Err(SurfaceAudioTablesError::NameByte {
            index,
            offset,
            byte: slot[0],
        });
    }
    let name = &slot[1..=len];
    check_name(index, name, offset + 1)?;
    if let Some(at) = slot[len + 1..].iter().position(|&b| b != 0) {
        let at = len + 1 + at;
        return Err(SurfaceAudioTablesError::NameByte {
            index,
            offset: offset + at,
            byte: slot[at],
        });
    }
    Ok(String::from_utf8(name.to_vec()).expect("printable ASCII"))
}

pub fn decode_surface_audio_tables(
    input: &[u8],
) -> Result<SurfaceAudioTables, SurfaceAudioTablesError> {
    let mut cursor = Cursor { input, offset: 0 };
    if cursor.take(8)? != MAGIC {
        return Err(SurfaceAudioTablesError::InvalidHeader { offset: 0 });
    }
    if cursor.u32()? != SURFACE_AUDIO_TABLES_FORMAT_VERSION {
        return Err(SurfaceAudioTablesError::InvalidHeader { offset: 8 });
    }
    if cursor.u32()? != SURFACE_AUDIO_GROUP_COUNT as u32 {
        return Err(SurfaceAudioTablesError::InvalidHeader { offset: 12 });
    }
    let counts = parse_counts(cursor.take(COUNTS_LEN)?);
    let class_volume_percent = parse_volumes(cursor.take(VOLUMES_LEN)?);
    let surface_group = i32_array(cursor.take(GROUPS_LEN)?);
    let wet_surfaces = i32_array(cursor.take(WET_LEN)?);
    let surface_volume_k = cursor.i32()?;
    let wet_volume_k = cursor.i32()?;
    let crowd_volume_k = cursor.i32()?;
    let prang_full_volume = cursor.i32()?;
    let initial_last_state = i32_array(cursor.take(LAST_STATE_LEN)?);
    validate_values(&counts, &surface_group, COUNTS_AT, GROUPS_AT)?;
    if cursor.u32()? != PRANG_NAME_COUNT as u32 {
        return Err(SurfaceAudioTablesError::InvalidHeader {
            offset: NAME_COUNT_AT,
        });
    }
    let prang_names = (0..PRANG_NAME_COUNT)
        .map(|index| {
            let at = cursor.offset;
            decode_name(index, cursor.take(NAME_SLOT_BYTES)?, at)
        })
        .collect::<Result<_, _>>()?;
    if cursor.offset != input.len() {
        return Err(SurfaceAudioTablesError::TrailingBytes {
            offset: cursor.offset,
            len: input.len() - cursor.offset,
        });
    }
    Ok(SurfaceAudioTables {
        counts,
        class_volume_percent,
        surface_group,
        wet_surfaces,
        surface_volume_k,
        wet_volume_k,
        crowd_volume_k,
        prang_full_volume,
        initial_last_state,
        prang_names,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PE: usize = 0x80;
    const OPTIONAL: usize = PE + 24;
    const SECTIONS: usize = OPTIONAL + 0xe0;
    const RAW: usize = 0x400;
    /// One synthetic section covering every reviewed address.
    const SECTION_RVA: u32 = 0x000e_b000;
    const SECTION_SIZE: usize = 0x18000;
    /// Synthetic string area inside the section, away from every table.
    const STRINGS_VA: u32 = 0x004f_a000;

    fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    fn file_offset(va: u32) -> usize {
        RAW + (va - 0x0040_0000 - SECTION_RVA) as usize
    }

    fn synthetic_tables() -> SurfaceAudioTables {
        SurfaceAudioTables {
            counts: std::array::from_fn(|g| SurfaceVariantCounts {
                ski: (g % 7) as u8,
                i: ((g + 1) % 7) as u8,
                sko: ((g + 2) % 7) as u8,
                o: ((g + 3) % 7) as u8,
            }),
            class_volume_percent: std::array::from_fn(|g| {
                std::array::from_fn(|c| (g as i32 * 3 - 128 + c as i32) as i8)
            }),
            surface_group: std::array::from_fn(|s| ((s * 7) % 120) as i32),
            wet_surfaces: std::array::from_fn(|i| i as i32 * 5 - 3),
            surface_volume_k: -11,
            wet_volume_k: 22,
            crowd_volume_k: i32::MIN,
            prang_full_volume: i32::MAX,
            initial_last_state: [1, -2, 3, -4],
            prang_names: (0..PRANG_NAME_COUNT)
                .map(|i| format!("SYN {i}.WAV"))
                .collect(),
        }
    }

    fn synthetic_pe(tables: &SurfaceAudioTables) -> Vec<u8> {
        let l = OFFICIAL_EU_SURFACE_AUDIO_TABLES_LAYOUT;
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
        for (g, record) in tables.counts.iter().enumerate() {
            let at = file_offset(l.variant_counts_va) + g * 4;
            bytes[at..at + 4].copy_from_slice(&record.to_bytes());
        }
        for (g, row) in tables.class_volume_percent.iter().enumerate() {
            for (c, v) in row.iter().enumerate() {
                bytes[file_offset(l.class_volume_va) + g * 3 + c] = *v as u8;
            }
        }
        let mut words = |va: u32, values: &[i32]| {
            for (i, v) in values.iter().enumerate() {
                put_u32(&mut bytes, file_offset(va) + i * 4, *v as u32);
            }
        };
        words(l.surface_group_va, &tables.surface_group);
        words(l.wet_surfaces_va, &tables.wet_surfaces);
        words(l.surface_volume_k_va, &[tables.surface_volume_k]);
        words(l.wet_volume_k_va, &[tables.wet_volume_k]);
        words(l.crowd_volume_k_va, &[tables.crowd_volume_k]);
        words(l.prang_full_volume_va, &[tables.prang_full_volume]);
        words(l.initial_last_state_va, &tables.initial_last_state);
        let mut va = STRINGS_VA;
        for (i, name) in tables.prang_names.iter().enumerate() {
            put_u32(&mut bytes, file_offset(l.prang_names_va) + i * 4, va);
            let at = file_offset(va);
            bytes[at..at + name.len()].copy_from_slice(name.as_bytes());
            bytes[at + name.len()] = 0;
            va += name.len() as u32 + 1;
        }
        bytes
    }

    fn name_va(bytes: &[u8], index: usize) -> u32 {
        let at = file_offset(OFFICIAL_EU_SURFACE_AUDIO_TABLES_LAYOUT.prang_names_va) + index * 4;
        u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
    }

    #[test]
    fn reviewed_layout_is_aligned_disjoint_and_sized() {
        check_layout(&OFFICIAL_EU_SURFACE_AUDIO_TABLES_LAYOUT).unwrap();
        // The four per-group tables are contiguous in the reviewed image.
        let l = OFFICIAL_EU_SURFACE_AUDIO_TABLES_LAYOUT;
        assert_eq!(l.variant_counts_va + 0x1e0, l.class_volume_va);
        assert_eq!(l.class_volume_va + 0x168, l.surface_group_va);
        assert_eq!(l.surface_group_va + 0x1e0, l.wet_surfaces_va);
        assert_eq!(l.wet_surfaces_va + 0x3c, l.surface_volume_k_va);
        assert_eq!(l.prang_names_va + 0xc0, 0x4f41a0);
        assert_eq!(
            SURFACE_AUDIO_TABLES_ENCODED_LEN,
            16 + 480 + 360 + 480 + 60 + 16 + 16 + 4 + 48 * 32
        );
    }

    #[test]
    fn extracts_every_table_and_resolves_names() {
        let tables = synthetic_tables();
        let got = extract_surface_audio_tables(
            &synthetic_pe(&tables),
            &OFFICIAL_EU_SURFACE_AUDIO_TABLES_LAYOUT,
        )
        .unwrap();
        assert_eq!(got, tables);
        // Byte order {ski, i, sko, o} and signed volumes.
        assert_eq!(
            got.counts[1],
            SurfaceVariantCounts {
                ski: 1,
                i: 2,
                sko: 3,
                o: 4
            }
        );
        assert_eq!(got.class_volume_percent[0], [-128, -127, -126]);
    }

    #[test]
    fn accepts_boundary_values() {
        let mut tables = synthetic_tables();
        tables.counts[0] = SurfaceVariantCounts {
            ski: 6,
            i: 6,
            sko: 6,
            o: 6,
        };
        tables.counts[119] = SurfaceVariantCounts::default();
        tables.surface_group[0] = 0;
        tables.surface_group[119] = 119;
        tables.prang_names[0] = "A".into();
        tables.prang_names[47] = "~".repeat(PRANG_NAME_MAX_BYTES - 1);
        tables.prang_names[1] = " !".into();
        let l = OFFICIAL_EU_SURFACE_AUDIO_TABLES_LAYOUT;
        assert_eq!(
            extract_surface_audio_tables(&synthetic_pe(&tables), &l).unwrap(),
            tables
        );
        let encoded = encode_surface_audio_tables(&tables).unwrap();
        assert_eq!(decode_surface_audio_tables(&encoded).unwrap(), tables);
    }

    #[test]
    fn rejects_out_of_range_counts_and_groups_with_offsets() {
        let l = OFFICIAL_EU_SURFACE_AUDIO_TABLES_LAYOUT;
        let mut tables = synthetic_tables();
        tables.counts[5].sko = 7;
        let expected = SurfaceAudioTablesError::VariantCount {
            group: 5,
            variant: SurfaceVariant::Sko,
            count: 7,
            offset: 0x4f9408 + 5 * 4 + 2,
        };
        assert_eq!(
            extract_surface_audio_tables(&synthetic_pe(&tables), &l),
            Err(expected)
        );
        assert!(matches!(
            tables.validate(),
            Err(SurfaceAudioTablesError::VariantCount { offset: 22, .. })
        ));
        assert!(encode_surface_audio_tables(&tables).is_err());
        tables.counts[5].sko = 0xff;
        let err = extract_surface_audio_tables(&synthetic_pe(&tables), &l).unwrap_err();
        assert!(err.to_string().contains("group 5 sko count 255"), "{err}");

        for bad in [-1, 120, i32::MIN, i32::MAX] {
            let mut tables = synthetic_tables();
            tables.surface_group[9] = bad;
            assert_eq!(
                extract_surface_audio_tables(&synthetic_pe(&tables), &l),
                Err(SurfaceAudioTablesError::SurfaceGroup {
                    surface: 9,
                    group: bad,
                    offset: 0x4f9750 + 36
                })
            );
            assert!(matches!(
                encode_surface_audio_tables(&tables),
                Err(SurfaceAudioTablesError::SurfaceGroup { offset: 36, .. })
            ));
        }
    }

    #[test]
    fn rejects_unterminated_empty_and_non_printable_names() {
        let l = OFFICIAL_EU_SURFACE_AUDIO_TABLES_LAYOUT;
        let tables = synthetic_tables();
        let base = synthetic_pe(&tables);

        let mut bytes = base.clone();
        let va = name_va(&bytes, 3);
        bytes[file_offset(va) + 2] = 0x07;
        assert_eq!(
            extract_surface_audio_tables(&bytes, &l),
            Err(SurfaceAudioTablesError::NameByte {
                index: 3,
                offset: va as usize + 2,
                byte: 0x07
            })
        );
        let mut bytes = base.clone();
        let at = file_offset(name_va(&bytes, 0));
        bytes[at] = 0x80;
        assert!(matches!(
            extract_surface_audio_tables(&bytes, &l),
            Err(SurfaceAudioTablesError::NameByte {
                index: 0,
                byte: 0x80,
                ..
            })
        ));

        // Point name 7 at a 32-byte run with no terminator.
        let mut bytes = base.clone();
        let run = STRINGS_VA + 0x800;
        bytes[file_offset(run)..file_offset(run) + 40].fill(b'X');
        put_u32(&mut bytes, file_offset(l.prang_names_va) + 7 * 4, run);
        assert_eq!(
            extract_surface_audio_tables(&bytes, &l),
            Err(SurfaceAudioTablesError::NameUnterminated {
                index: 7,
                offset: run as usize
            })
        );
        // 31 characters then NUL is accepted.
        bytes[file_offset(run) + 31] = 0;
        let got = extract_surface_audio_tables(&bytes, &l).unwrap();
        assert_eq!(got.prang_names[7], "X".repeat(31));

        let mut bytes = base.clone();
        let at = file_offset(name_va(&bytes, 47));
        bytes[at] = 0;
        assert!(matches!(
            extract_surface_audio_tables(&bytes, &l),
            Err(SurfaceAudioTablesError::NameEmpty { index: 47, .. })
        ));

        // A pointer outside the image, and a name running off the section end.
        let mut bytes = base.clone();
        put_u32(&mut bytes, file_offset(l.prang_names_va), 0x10);
        assert!(matches!(
            extract_surface_audio_tables(&bytes, &l),
            Err(SurfaceAudioTablesError::Pe(_))
        ));
        let mut bytes = base;
        let tail = 0x0040_0000 + SECTION_RVA + SECTION_SIZE as u32 - 3;
        let at = file_offset(tail);
        bytes[at..at + 3].copy_from_slice(b"ABC");
        put_u32(&mut bytes, file_offset(l.prang_names_va), tail);
        assert!(matches!(
            extract_surface_audio_tables(&bytes, &l),
            Err(SurfaceAudioTablesError::Pe(text)) if text.contains("UnmappedRange")
        ));

        let mut tables = synthetic_tables();
        tables.prang_names.pop();
        assert_eq!(
            encode_surface_audio_tables(&tables),
            Err(SurfaceAudioTablesError::NameCount {
                expected: 48,
                actual: 47
            })
        );
        let mut tables = synthetic_tables();
        tables.prang_names[2] = "Ä".into();
        assert!(matches!(
            encode_surface_audio_tables(&tables),
            Err(SurfaceAudioTablesError::NameByte { index: 2, .. })
        ));
        tables.prang_names[2] = "Y".repeat(32);
        assert!(matches!(
            encode_surface_audio_tables(&tables),
            Err(SurfaceAudioTablesError::NameUnterminated { index: 2, .. })
        ));
    }

    #[test]
    fn rejects_truncation_wrong_image_and_bad_layouts() {
        let l = OFFICIAL_EU_SURFACE_AUDIO_TABLES_LAYOUT;
        let bytes = synthetic_pe(&synthetic_tables());
        let end = file_offset(l.wet_volume_k_va) + 4;
        let Err(SurfaceAudioTablesError::Pe(text)) =
            extract_surface_audio_tables(&bytes[..end - 1], &l)
        else {
            panic!("expected PE error");
        };
        assert!(
            text.contains("Truncated") && text.contains("offset"),
            "{text}"
        );
        let mut wrong_base = l;
        wrong_base.image_base += 0x1000;
        assert_eq!(
            extract_surface_audio_tables(&bytes, &wrong_base),
            Err(SurfaceAudioTablesError::ImageBase {
                expected: 0x0040_1000,
                actual: 0x0040_0000
            })
        );
        for shift in 0..10 {
            let mut m = l;
            let field = match shift {
                0 => &mut m.variant_counts_va,
                1 => &mut m.class_volume_va,
                2 => &mut m.surface_group_va,
                3 => &mut m.wet_surfaces_va,
                4 => &mut m.surface_volume_k_va,
                5 => &mut m.wet_volume_k_va,
                6 => &mut m.crowd_volume_k_va,
                7 => &mut m.prang_full_volume_va,
                8 => &mut m.initial_last_state_va,
                _ => &mut m.prang_names_va,
            };
            *field += 2;
            let va = *field;
            assert_eq!(
                extract_surface_audio_tables(&bytes, &m),
                Err(SurfaceAudioTablesError::Misaligned {
                    virtual_address: va,
                    alignment: 4
                })
            );
        }
        let mut overlap = l;
        overlap.wet_volume_k_va = l.surface_group_va + 0x1dc;
        assert_eq!(
            extract_surface_audio_tables(&bytes, &overlap),
            Err(SurfaceAudioTablesError::UnsupportedLayout)
        );
        let mut unmapped = l;
        unmapped.crowd_volume_k_va = 0x1000;
        assert!(matches!(
            extract_surface_audio_tables(&bytes, &unmapped),
            Err(SurfaceAudioTablesError::Pe(_))
        ));
        assert!(matches!(
            extract_surface_audio_tables(b"MZ", &l),
            Err(SurfaceAudioTablesError::Pe(_))
        ));
    }

    #[test]
    fn codec_round_trips_and_preserves_extremes() {
        let mut tables = synthetic_tables();
        tables.class_volume_percent[119] = [i8::MIN, -1, i8::MAX];
        tables.wet_surfaces[14] = i32::MIN;
        tables.initial_last_state = [i32::MAX, i32::MIN, 0, -1];
        let encoded = encode_surface_audio_tables(&tables).unwrap();
        assert_eq!(encoded.len(), SURFACE_AUDIO_TABLES_ENCODED_LEN);
        assert_eq!(decode_surface_audio_tables(&encoded).unwrap(), tables);
    }

    #[test]
    fn decode_reports_truncation_trailing_header_and_value_offsets() {
        let encoded = encode_surface_audio_tables(&synthetic_tables()).unwrap();
        assert_eq!(
            decode_surface_audio_tables(&encoded[..encoded.len() - 1]),
            Err(SurfaceAudioTablesError::Truncated {
                offset: SURFACE_AUDIO_TABLES_ENCODED_LEN - 32,
                needed: 32,
                available: 31
            })
        );
        assert_eq!(
            decode_surface_audio_tables(&encoded[..10]),
            Err(SurfaceAudioTablesError::Truncated {
                offset: 8,
                needed: 4,
                available: 2
            })
        );
        assert!(matches!(
            decode_surface_audio_tables(&[]),
            Err(SurfaceAudioTablesError::Truncated { offset: 0, .. })
        ));
        let mut trailing = encoded.clone();
        trailing.push(0);
        assert_eq!(
            decode_surface_audio_tables(&trailing),
            Err(SurfaceAudioTablesError::TrailingBytes {
                offset: SURFACE_AUDIO_TABLES_ENCODED_LEN,
                len: 1
            })
        );
        for (flip, offset) in [(0, 0), (8, 8), (12, 12), (NAME_COUNT_AT, NAME_COUNT_AT)] {
            let mut candidate = encoded.clone();
            candidate[flip] ^= 1;
            assert_eq!(
                decode_surface_audio_tables(&candidate),
                Err(SurfaceAudioTablesError::InvalidHeader { offset })
            );
        }
        let mut count = encoded.clone();
        count[COUNTS_AT + 4 * 10 + 3] = 7;
        assert!(matches!(
            decode_surface_audio_tables(&count),
            Err(SurfaceAudioTablesError::VariantCount {
                group: 10,
                variant: SurfaceVariant::O,
                offset,
                ..
            }) if offset == COUNTS_AT + 43
        ));
        let mut group = encoded.clone();
        group[GROUPS_AT + 4 * 2..GROUPS_AT + 4 * 3].copy_from_slice(&120_i32.to_le_bytes());
        assert_eq!(
            decode_surface_audio_tables(&group),
            Err(SurfaceAudioTablesError::SurfaceGroup {
                surface: 2,
                group: 120,
                offset: GROUPS_AT + 8
            })
        );
        let slot = NAMES_AT + 4 * NAME_SLOT_BYTES;
        let mut len = encoded.clone();
        len[slot] = 0;
        assert_eq!(
            decode_surface_audio_tables(&len),
            Err(SurfaceAudioTablesError::NameEmpty {
                index: 4,
                offset: slot
            })
        );
        len[slot] = 32;
        assert!(matches!(
            decode_surface_audio_tables(&len),
            Err(SurfaceAudioTablesError::NameByte {
                index: 4,
                byte: 32,
                ..
            })
        ));
        let mut padding = encoded.clone();
        padding[slot + NAME_SLOT_BYTES - 1] = b'Z';
        assert_eq!(
            decode_surface_audio_tables(&padding),
            Err(SurfaceAudioTablesError::NameByte {
                index: 4,
                offset: slot + NAME_SLOT_BYTES - 1,
                byte: b'Z'
            })
        );
        let mut byte = encoded;
        byte[slot + 1] = 0x7f;
        assert_eq!(
            decode_surface_audio_tables(&byte),
            Err(SurfaceAudioTablesError::NameByte {
                index: 4,
                offset: slot + 1,
                byte: 0x7f
            })
        );
    }
}
