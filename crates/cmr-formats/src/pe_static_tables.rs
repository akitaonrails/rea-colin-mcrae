//! Authenticated extraction and payload-free encoding of named static tables
//! read from the user's executable (evidence `rea.official-eu.static-tables`).
//!
//! A reviewed layout names a set of `.data`/`.rdata` spans by stable lookup
//! name. Each entry is one of:
//!
//! - a **block**: `len` raw bytes at a VA, preserved unchanged;
//! - a **string-pointer table**: `count` little-endian `u32` VA pointers, each
//!   to a NUL-terminated string inside the image;
//! - a **string**: one NUL-terminated string at a VA.
//!
//! Strings are kept as raw bytes (no character decoding beyond the NUL
//! terminator) and the raw pointer words of a pointer table are preserved.
//! Nothing here asserts what any value means. No payload is compiled into this
//! crate; tests use synthetic fixtures only.

use std::collections::BTreeSet;
use std::fmt;

use crate::pe32::{Pe32Error, Pe32Image};

pub const STATIC_TABLES_FORMAT_VERSION: u32 = 1;
/// Reviewed image base of the official EU executable.
pub const OFFICIAL_EU_STATIC_TABLES_IMAGE_BASE: u32 = 0x0040_0000;
/// Upper bound on the number of entries in one layout.
pub const STATIC_TABLES_MAX_ENTRIES: usize = 128;
/// Upper bound on an entry name's length in bytes.
pub const STATIC_TABLE_NAME_MAX_BYTES: usize = 64;
/// Upper bound on a block's byte length.
pub const STATIC_BLOCK_MAX_BYTES: usize = 0x1_0000;
/// Upper bound on a string-pointer table's pointer count.
pub const STATIC_STRING_TABLE_MAX_COUNT: usize = 256;
/// Upper bound on one string, including its NUL terminator.
pub const STATIC_STRING_MAX_BYTES: usize = 256;

const MAGIC: &[u8; 8] = b"CMRSTAT\0";
const HEADER_LEN: usize = 16;
const KIND_BLOCK: u8 = 1;
const KIND_STRING_POINTERS: u8 = 2;
const KIND_STRING: u8 = 3;
/// Encoded bytes of one maximal string (pointer, `u16` length, bytes).
const MAX_ENCODED_STRING: usize = 4 + 2 + STATIC_STRING_MAX_BYTES - 1;
/// Encoded bytes of one maximal entry of any kind.
const MAX_ENCODED_ENTRY: usize = {
    let head = 1 + STATIC_TABLE_NAME_MAX_BYTES + 1 + 4;
    let block = 4 + STATIC_BLOCK_MAX_BYTES;
    let table = 2 + STATIC_STRING_TABLE_MAX_COUNT * MAX_ENCODED_STRING;
    head + if block > table { block } else { table }
};
/// Largest valid encoding: header plus the maximal number of maximal entries.
pub const STATIC_TABLES_MAX_ENCODED_LEN: usize =
    HEADER_LEN + STATIC_TABLES_MAX_ENTRIES * MAX_ENCODED_ENTRY;

/// What one reviewed entry reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StaticTableKind {
    Block { len: u32 },
    StringPointers { count: u32 },
    String,
}

/// One reviewed entry: a stable lookup name, its VA and what it reads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StaticTableEntry {
    pub name: String,
    pub va: u32,
    pub kind: StaticTableKind,
}

/// A reviewed set of named spans in one image.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StaticTablesLayout {
    pub image_base: u32,
    pub entries: Vec<StaticTableEntry>,
}

/// A string-pointer table: the raw pointer words and the strings they name
/// (each without its NUL).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StaticStringTable {
    pub pointers: Vec<u32>,
    pub strings: Vec<Vec<u8>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StaticTableData {
    Block(Vec<u8>),
    StringPointers(StaticStringTable),
    String(Vec<u8>),
}

impl StaticTableData {
    fn kind(&self) -> StaticTableKind {
        match self {
            Self::Block(bytes) => StaticTableKind::Block {
                len: bytes.len() as u32,
            },
            Self::StringPointers(table) => StaticTableKind::StringPointers {
                count: table.strings.len() as u32,
            },
            Self::String(_) => StaticTableKind::String,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StaticTable {
    pub name: String,
    pub va: u32,
    pub data: StaticTableData,
}

/// Every extracted entry, in layout order, with unique names.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StaticTables {
    entries: Vec<StaticTable>,
}

impl StaticTables {
    /// Build from entries checked for valid, unique names and bounded sizes.
    pub fn new(entries: Vec<StaticTable>) -> Result<Self, StaticTablesError> {
        if entries.is_empty() || entries.len() > STATIC_TABLES_MAX_ENTRIES {
            return Err(StaticTablesError::EntryCount(entries.len()));
        }
        let mut seen = BTreeSet::new();
        for entry in &entries {
            check_name(&entry.name)?;
            if !seen.insert(entry.name.as_str()) {
                return Err(StaticTablesError::DuplicateName(entry.name.clone()));
            }
            check_kind(&entry.name, entry.data.kind())?;
            let strings: &[Vec<u8>] = match &entry.data {
                StaticTableData::Block(_) => &[],
                StaticTableData::StringPointers(table) => {
                    if table.pointers.len() != table.strings.len() {
                        return Err(StaticTablesError::InvalidSize(entry.name.clone()));
                    }
                    &table.strings
                }
                StaticTableData::String(s) => std::slice::from_ref(s),
            };
            if strings
                .iter()
                .any(|s| s.len() >= STATIC_STRING_MAX_BYTES || s.contains(&0))
            {
                return Err(StaticTablesError::InvalidSize(entry.name.clone()));
            }
        }
        Ok(Self { entries })
    }
    pub fn entries(&self) -> &[StaticTable] {
        &self.entries
    }
    pub fn get(&self, name: &str) -> Option<&StaticTable> {
        self.entries.iter().find(|e| e.name == name)
    }
    /// The raw bytes of block `name`.
    pub fn block(&self, name: &str) -> Option<&[u8]> {
        match &self.get(name)?.data {
            StaticTableData::Block(bytes) => Some(bytes),
            _ => None,
        }
    }
    /// The strings (without NULs) of string-pointer table `name`.
    pub fn strings(&self, name: &str) -> Option<&[Vec<u8>]> {
        match &self.get(name)?.data {
            StaticTableData::StringPointers(table) => Some(&table.strings),
            _ => None,
        }
    }
    /// The raw pointer words of string-pointer table `name`.
    pub fn string_pointers(&self, name: &str) -> Option<&[u32]> {
        match &self.get(name)?.data {
            StaticTableData::StringPointers(table) => Some(&table.pointers),
            _ => None,
        }
    }
    /// The bytes (without NUL) of string `name`.
    pub fn string(&self, name: &str) -> Option<&[u8]> {
        match &self.get(name)?.data {
            StaticTableData::String(bytes) => Some(bytes),
            _ => None,
        }
    }
    /// Whether these tables have exactly `layout`'s names, VAs and shapes, in
    /// order.
    pub fn matches_layout(&self, layout: &StaticTablesLayout) -> bool {
        self.entries.len() == layout.entries.len()
            && self
                .entries
                .iter()
                .zip(&layout.entries)
                .all(|(t, e)| t.name == e.name && t.va == e.va && t.data.kind() == e.kind)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StaticTablesError {
    Pe(String),
    ImageBase {
        expected: u32,
        actual: u32,
    },
    /// Zero entries, or more than [`STATIC_TABLES_MAX_ENTRIES`].
    EntryCount(usize),
    /// A name is empty, too long, or not `[a-z0-9_]`.
    InvalidName(String),
    DuplicateName(String),
    /// A block length or pointer count is zero or above its bound.
    InvalidSize(String),
    Misaligned {
        name: String,
        virtual_address: u32,
        alignment: u32,
    },
    /// An entry's start VA does not resolve inside the image.
    OutsideImage {
        name: String,
        virtual_address: u32,
    },
    /// An entry starts inside a section but its span runs past the section.
    PastSection {
        name: String,
        virtual_address: u32,
        len: usize,
    },
    /// A string pointer does not resolve inside the image. `index` is the
    /// table slot, or `None` for a string-at-VA entry.
    BadStringPointer {
        name: String,
        index: Option<usize>,
        virtual_address: u32,
    },
    /// No NUL within [`STATIC_STRING_MAX_BYTES`] mapped bytes.
    UnterminatedString {
        name: String,
        index: Option<usize>,
        virtual_address: u32,
    },
    /// A stored payload is not the length its import state recorded.
    WrongLength {
        expected: usize,
        actual: usize,
    },
    /// An encoded entry kind byte is unknown.
    InvalidKind {
        offset: usize,
    },
    /// An encoded name, size or string is invalid.
    InvalidField {
        offset: usize,
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

fn index_text(index: Option<usize>) -> String {
    index.map_or_else(String::new, |i| format!("[{i}]"))
}

impl fmt::Display for StaticTablesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pe(e) => write!(f, "invalid PE32 image: {e}"),
            Self::ImageBase { expected, actual } => write!(
                f,
                "image base mismatch: expected {expected:#x}, got {actual:#x}"
            ),
            Self::EntryCount(n) => write!(
                f,
                "static-table layout has {n} entries; need 1..={STATIC_TABLES_MAX_ENTRIES}"
            ),
            Self::InvalidName(n) => write!(f, "invalid static-table name {n:?}"),
            Self::DuplicateName(n) => write!(f, "duplicate static-table name {n:?}"),
            Self::InvalidSize(n) => write!(f, "static table {n:?}: size out of bounds"),
            Self::Misaligned {
                name,
                virtual_address,
                alignment,
            } => write!(
                f,
                "static table {name:?}: address {virtual_address:#x} is not {alignment}-byte aligned"
            ),
            Self::OutsideImage {
                name,
                virtual_address,
            } => write!(
                f,
                "static table {name:?}: address {virtual_address:#x} is outside the image"
            ),
            Self::PastSection {
                name,
                virtual_address,
                len,
            } => write!(
                f,
                "static table {name:?}: {len} bytes at {virtual_address:#x} run past its section"
            ),
            Self::BadStringPointer {
                name,
                index,
                virtual_address,
            } => write!(
                f,
                "static table {name:?}{}: string pointer {virtual_address:#x} is outside the image",
                index_text(*index)
            ),
            Self::UnterminatedString {
                name,
                index,
                virtual_address,
            } => write!(
                f,
                "static table {name:?}{}: string at {virtual_address:#x} has no NUL within {STATIC_STRING_MAX_BYTES} bytes",
                index_text(*index)
            ),
            Self::WrongLength { expected, actual } => {
                write!(f, "wrong encoded length: expected {expected}, got {actual}")
            }
            Self::InvalidKind { offset } => write!(f, "invalid entry kind at offset {offset}"),
            Self::InvalidField { offset } => write!(f, "invalid encoded field at offset {offset}"),
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
                write!(f, "invalid static-tables header at offset {offset}")
            }
        }
    }
}
impl std::error::Error for StaticTablesError {}

fn pe_error(error: Pe32Error) -> StaticTablesError {
    StaticTablesError::Pe(format!("{error:?}"))
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= STATIC_TABLE_NAME_MAX_BYTES
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn check_name(name: &str) -> Result<(), StaticTablesError> {
    if valid_name(name) {
        Ok(())
    } else {
        Err(StaticTablesError::InvalidName(name.into()))
    }
}

fn check_kind(name: &str, kind: StaticTableKind) -> Result<(), StaticTablesError> {
    let ok = match kind {
        StaticTableKind::Block { len } => (1..=STATIC_BLOCK_MAX_BYTES as u32).contains(&len),
        StaticTableKind::StringPointers { count } => {
            (1..=STATIC_STRING_TABLE_MAX_COUNT as u32).contains(&count)
        }
        StaticTableKind::String => true,
    };
    if ok {
        Ok(())
    } else {
        Err(StaticTablesError::InvalidSize(name.into()))
    }
}

/// Check a layout's structure: entry count, names (valid and unique), sizes
/// and pointer-table alignment. Image contents are not consulted.
pub fn check_static_tables_layout(layout: &StaticTablesLayout) -> Result<(), StaticTablesError> {
    if layout.entries.is_empty() || layout.entries.len() > STATIC_TABLES_MAX_ENTRIES {
        return Err(StaticTablesError::EntryCount(layout.entries.len()));
    }
    let mut seen = BTreeSet::new();
    for entry in &layout.entries {
        check_name(&entry.name)?;
        if !seen.insert(entry.name.as_str()) {
            return Err(StaticTablesError::DuplicateName(entry.name.clone()));
        }
        check_kind(&entry.name, entry.kind)?;
        if matches!(entry.kind, StaticTableKind::StringPointers { .. })
            && !entry.va.is_multiple_of(4)
        {
            return Err(StaticTablesError::Misaligned {
                name: entry.name.clone(),
                virtual_address: entry.va,
                alignment: 4,
            });
        }
    }
    Ok(())
}

/// Read `len` bytes at `va`, distinguishing a start outside the image from a
/// span that runs past its section.
fn read_span<'a>(
    image: &Pe32Image<'a>,
    name: &str,
    va: u32,
    len: usize,
) -> Result<&'a [u8], StaticTablesError> {
    if image.read_va_range(va, 1).is_err() {
        return Err(StaticTablesError::OutsideImage {
            name: name.into(),
            virtual_address: va,
        });
    }
    image.read_va_range(va, len).map_err(|error| match error {
        Pe32Error::UnmappedRange { .. } | Pe32Error::ArithmeticOverflow { .. } => {
            StaticTablesError::PastSection {
                name: name.into(),
                virtual_address: va,
                len,
            }
        }
        other => pe_error(other),
    })
}

/// Resolve the NUL-terminated string at `va`, byte by byte, so a string that
/// runs off its section is reported rather than read past.
fn resolve_string(
    image: &Pe32Image<'_>,
    name: &str,
    index: Option<usize>,
    va: u32,
) -> Result<Vec<u8>, StaticTablesError> {
    let mut out = Vec::new();
    for i in 0..STATIC_STRING_MAX_BYTES {
        let at = va.checked_add(i as u32);
        let byte = at.and_then(|at| image.read_va_range(at, 1).ok());
        match byte {
            Some([0]) => return Ok(out),
            Some(&[b]) => out.push(b),
            _ if i == 0 => {
                return Err(StaticTablesError::BadStringPointer {
                    name: name.into(),
                    index,
                    virtual_address: va,
                });
            }
            _ => break,
        }
    }
    Err(StaticTablesError::UnterminatedString {
        name: name.into(),
        index,
        virtual_address: va,
    })
}

pub fn extract_static_tables(
    input: &[u8],
    layout: &StaticTablesLayout,
) -> Result<StaticTables, StaticTablesError> {
    check_static_tables_layout(layout)?;
    let image = Pe32Image::parse(input).map_err(pe_error)?;
    if image.image_base() != layout.image_base {
        return Err(StaticTablesError::ImageBase {
            expected: layout.image_base,
            actual: image.image_base(),
        });
    }
    let mut entries = Vec::with_capacity(layout.entries.len());
    for entry in &layout.entries {
        let name = entry.name.as_str();
        let data = match entry.kind {
            StaticTableKind::Block { len } => {
                StaticTableData::Block(read_span(&image, name, entry.va, len as usize)?.to_vec())
            }
            StaticTableKind::StringPointers { count } => {
                let words = read_span(&image, name, entry.va, count as usize * 4)?;
                let mut table = StaticStringTable {
                    pointers: Vec::with_capacity(count as usize),
                    strings: Vec::with_capacity(count as usize),
                };
                for (index, word) in words.as_chunks::<4>().0.iter().enumerate() {
                    let pointer = u32::from_le_bytes(*word);
                    table
                        .strings
                        .push(resolve_string(&image, name, Some(index), pointer)?);
                    table.pointers.push(pointer);
                }
                StaticTableData::StringPointers(table)
            }
            StaticTableKind::String => {
                StaticTableData::String(resolve_string(&image, name, None, entry.va)?)
            }
        };
        entries.push(StaticTable {
            name: entry.name.clone(),
            va: entry.va,
            data,
        });
    }
    StaticTables::new(entries)
}

/// Encode `cmr-static-tables-v1`: a 16-byte header (magic, version `u32`,
/// entry count `u16`, reserved zero `u16`) then, per entry, a `u8` name
/// length, the name, a `u8` kind (1 block, 2 string pointers, 3 string), the
/// `u32` VA and the body:
///
/// - block: `u32` length then the bytes;
/// - string pointers: `u16` count then, per slot, the raw `u32` pointer, a
///   `u16` string length and the string bytes (no NUL);
/// - string: `u16` length then the bytes (no NUL).
pub fn encode_static_tables(tables: &StaticTables) -> Vec<u8> {
    fn string(out: &mut Vec<u8>, s: &[u8]) {
        out.extend_from_slice(&(s.len() as u16).to_le_bytes());
        out.extend_from_slice(s);
    }
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&STATIC_TABLES_FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&(tables.entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    for entry in &tables.entries {
        out.push(entry.name.len() as u8);
        out.extend_from_slice(entry.name.as_bytes());
        match &entry.data {
            StaticTableData::Block(bytes) => {
                out.push(KIND_BLOCK);
                out.extend_from_slice(&entry.va.to_le_bytes());
                out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                out.extend_from_slice(bytes);
            }
            StaticTableData::StringPointers(table) => {
                out.push(KIND_STRING_POINTERS);
                out.extend_from_slice(&entry.va.to_le_bytes());
                out.extend_from_slice(&(table.strings.len() as u16).to_le_bytes());
                for (pointer, s) in table.pointers.iter().zip(&table.strings) {
                    out.extend_from_slice(&pointer.to_le_bytes());
                    string(&mut out, s);
                }
            }
            StaticTableData::String(s) => {
                out.push(KIND_STRING);
                out.extend_from_slice(&entry.va.to_le_bytes());
                string(&mut out, s);
            }
        }
    }
    out
}

struct Cursor<'a> {
    input: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, needed: usize) -> Result<&'a [u8], StaticTablesError> {
        let available = self.input.len() - self.offset;
        if needed > available {
            return Err(StaticTablesError::Truncated {
                offset: self.offset,
                needed,
                available,
            });
        }
        let bytes = &self.input[self.offset..self.offset + needed];
        self.offset += needed;
        Ok(bytes)
    }
    fn u8(&mut self) -> Result<u8, StaticTablesError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, StaticTablesError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, StaticTablesError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn string(&mut self) -> Result<Vec<u8>, StaticTablesError> {
        let len_offset = self.offset;
        let len = self.u16()? as usize;
        if len >= STATIC_STRING_MAX_BYTES {
            return Err(StaticTablesError::InvalidField { offset: len_offset });
        }
        let start = self.offset;
        let bytes = self.take(len)?;
        if let Some(nul) = bytes.iter().position(|&b| b == 0) {
            return Err(StaticTablesError::InvalidField {
                offset: start + nul,
            });
        }
        Ok(bytes.to_vec())
    }
}

pub fn decode_static_tables(input: &[u8]) -> Result<StaticTables, StaticTablesError> {
    let mut cursor = Cursor { input, offset: 0 };
    if cursor.take(8)? != MAGIC {
        return Err(StaticTablesError::InvalidHeader { offset: 0 });
    }
    if cursor.u32()? != STATIC_TABLES_FORMAT_VERSION {
        return Err(StaticTablesError::InvalidHeader { offset: 8 });
    }
    let count = cursor.u16()? as usize;
    if count == 0 || count > STATIC_TABLES_MAX_ENTRIES {
        return Err(StaticTablesError::InvalidHeader { offset: 12 });
    }
    if cursor.u16()? != 0 {
        return Err(StaticTablesError::InvalidHeader { offset: 14 });
    }
    let mut entries = Vec::with_capacity(count);
    let mut seen = BTreeSet::new();
    for _ in 0..count {
        let name_offset = cursor.offset;
        let len = cursor.u8()? as usize;
        let name = cursor.take(len)?;
        let name = std::str::from_utf8(name)
            .ok()
            .filter(|n| valid_name(n))
            .ok_or(StaticTablesError::InvalidField {
                offset: name_offset,
            })?;
        if !seen.insert(name) {
            return Err(StaticTablesError::DuplicateName(name.into()));
        }
        let kind_offset = cursor.offset;
        let kind = cursor.u8()?;
        let va = cursor.u32()?;
        let size_offset = cursor.offset;
        let data = match kind {
            KIND_BLOCK => {
                let len = cursor.u32()? as usize;
                if len == 0 || len > STATIC_BLOCK_MAX_BYTES {
                    return Err(StaticTablesError::InvalidField {
                        offset: size_offset,
                    });
                }
                StaticTableData::Block(cursor.take(len)?.to_vec())
            }
            KIND_STRING_POINTERS => {
                let count = cursor.u16()? as usize;
                if count == 0 || count > STATIC_STRING_TABLE_MAX_COUNT {
                    return Err(StaticTablesError::InvalidField {
                        offset: size_offset,
                    });
                }
                let mut table = StaticStringTable {
                    pointers: Vec::with_capacity(count),
                    strings: Vec::with_capacity(count),
                };
                for _ in 0..count {
                    table.pointers.push(cursor.u32()?);
                    table.strings.push(cursor.string()?);
                }
                StaticTableData::StringPointers(table)
            }
            KIND_STRING => StaticTableData::String(cursor.string()?),
            _ => {
                return Err(StaticTablesError::InvalidKind {
                    offset: kind_offset,
                });
            }
        };
        entries.push(StaticTable {
            name: name.into(),
            va,
            data,
        });
    }
    if cursor.offset != input.len() {
        return Err(StaticTablesError::TrailingBytes {
            offset: cursor.offset,
            len: input.len() - cursor.offset,
        });
    }
    StaticTables::new(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PE: usize = 0x80;
    const OPTIONAL: usize = PE + 24;
    const SECTIONS: usize = OPTIONAL + 0xe0;
    const RAW: usize = 0x400;
    const BASE: u32 = OFFICIAL_EU_STATIC_TABLES_IMAGE_BASE;
    /// One synthetic section; no real layout is reproduced.
    const SECTION_RVA: u32 = 0x0001_0000;
    const SECTION_SIZE: usize = 0x1000;
    const SECTION_VA: u32 = BASE + SECTION_RVA;
    const SECTION_END_VA: u32 = SECTION_VA + SECTION_SIZE as u32;
    const BLOCK_VA: u32 = SECTION_VA + 0x10;
    const TABLE_VA: u32 = SECTION_VA + 0x100;
    const STRINGS_VA: u32 = SECTION_VA + 0x200;
    const LONE_VA: u32 = SECTION_VA + 0x800;

    fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    fn file_offset(va: u32) -> usize {
        RAW + (va - SECTION_VA) as usize
    }
    fn string_va(i: usize) -> u32 {
        STRINGS_VA + i as u32 * 0x10
    }

    fn layout() -> StaticTablesLayout {
        StaticTablesLayout {
            image_base: BASE,
            entries: vec![
                StaticTableEntry {
                    name: "synthetic_block".into(),
                    va: BLOCK_VA,
                    kind: StaticTableKind::Block { len: 9 },
                },
                StaticTableEntry {
                    name: "synthetic_names".into(),
                    va: TABLE_VA,
                    kind: StaticTableKind::StringPointers { count: 3 },
                },
                StaticTableEntry {
                    name: "synthetic_name".into(),
                    va: LONE_VA,
                    kind: StaticTableKind::String,
                },
            ],
        }
    }

    /// Synthetic placeholder content; not game data.
    fn synthetic_pe() -> Vec<u8> {
        let mut bytes = vec![0xa5_u8; RAW + SECTION_SIZE];
        bytes[..RAW].fill(0);
        bytes[..2].copy_from_slice(b"MZ");
        put_u32(&mut bytes, 0x3c, PE as u32);
        bytes[PE..PE + 4].copy_from_slice(b"PE\0\0");
        put_u16(&mut bytes, PE + 4, 0x14c);
        put_u16(&mut bytes, PE + 6, 1);
        put_u16(&mut bytes, PE + 20, 0xe0);
        put_u16(&mut bytes, OPTIONAL, 0x10b);
        put_u32(&mut bytes, OPTIONAL + 28, BASE);
        put_u32(&mut bytes, SECTIONS + 8, SECTION_SIZE as u32);
        put_u32(&mut bytes, SECTIONS + 12, SECTION_RVA);
        put_u32(&mut bytes, SECTIONS + 16, SECTION_SIZE as u32);
        put_u32(&mut bytes, SECTIONS + 20, RAW as u32);
        let block = file_offset(BLOCK_VA);
        for (i, b) in bytes[block..block + 9].iter_mut().enumerate() {
            *b = i as u8;
        }
        for i in 0..3 {
            put_u32(&mut bytes, file_offset(TABLE_VA) + i * 4, string_va(i));
            let s = format!("Name{i}");
            let at = file_offset(string_va(i));
            bytes[at..at + s.len()].copy_from_slice(s.as_bytes());
            bytes[at + s.len()] = 0;
        }
        let at = file_offset(LONE_VA);
        bytes[at..at + 5].copy_from_slice(b"Lone\0");
        bytes
    }

    fn expected() -> StaticTables {
        StaticTables::new(vec![
            StaticTable {
                name: "synthetic_block".into(),
                va: BLOCK_VA,
                data: StaticTableData::Block((0..9).collect()),
            },
            StaticTable {
                name: "synthetic_names".into(),
                va: TABLE_VA,
                data: StaticTableData::StringPointers(StaticStringTable {
                    pointers: (0..3).map(string_va).collect(),
                    strings: (0..3).map(|i| format!("Name{i}").into_bytes()).collect(),
                }),
            },
            StaticTable {
                name: "synthetic_name".into(),
                va: LONE_VA,
                data: StaticTableData::String(b"Lone".to_vec()),
            },
        ])
        .unwrap()
    }

    #[test]
    fn extracts_blocks_pointer_tables_and_strings_by_name() {
        let got = extract_static_tables(&synthetic_pe(), &layout()).unwrap();
        assert_eq!(got, expected());
        assert!(got.matches_layout(&layout()));
        assert_eq!(
            got.block("synthetic_block"),
            Some(&(0..9).collect::<Vec<u8>>()[..])
        );
        assert_eq!(got.strings("synthetic_names").unwrap()[2], b"Name2");
        assert_eq!(
            got.string_pointers("synthetic_names"),
            Some(&[string_va(0), string_va(1), string_va(2)][..])
        );
        assert_eq!(got.string("synthetic_name"), Some(&b"Lone"[..]));
        // Lookups are typed: a name of the wrong kind or unknown name is None.
        assert_eq!(got.block("synthetic_names"), None);
        assert_eq!(got.strings("synthetic_name"), None);
        assert_eq!(got.string("synthetic_block"), None);
        assert_eq!(got.block("missing"), None);
        let mut other = layout();
        other.entries[0].kind = StaticTableKind::Block { len: 8 };
        assert!(!got.matches_layout(&other));
    }

    #[test]
    fn block_may_end_exactly_at_section_end() {
        let mut l = layout();
        l.entries[0].va = SECTION_END_VA - 9;
        let got = extract_static_tables(&synthetic_pe(), &l).unwrap();
        assert_eq!(got.block("synthetic_block").unwrap().len(), 9);
    }

    #[test]
    fn rejects_entries_outside_the_image_or_past_a_section() {
        let bytes = synthetic_pe();
        for bad in [0, BASE - 1, SECTION_END_VA, u32::MAX] {
            let mut l = layout();
            l.entries[0].va = bad;
            assert_eq!(
                extract_static_tables(&bytes, &l),
                Err(StaticTablesError::OutsideImage {
                    name: "synthetic_block".into(),
                    virtual_address: bad
                })
            );
        }
        let mut l = layout();
        l.entries[0].va = SECTION_END_VA - 8;
        assert_eq!(
            extract_static_tables(&bytes, &l),
            Err(StaticTablesError::PastSection {
                name: "synthetic_block".into(),
                virtual_address: SECTION_END_VA - 8,
                len: 9
            })
        );
        let mut l = layout();
        l.entries[1].va = SECTION_END_VA - 8;
        assert_eq!(
            extract_static_tables(&bytes, &l),
            Err(StaticTablesError::PastSection {
                name: "synthetic_names".into(),
                virtual_address: SECTION_END_VA - 8,
                len: 12
            })
        );
        let mut l = layout();
        l.entries[2].va = SECTION_END_VA;
        assert_eq!(
            extract_static_tables(&bytes, &l),
            Err(StaticTablesError::BadStringPointer {
                name: "synthetic_name".into(),
                index: None,
                virtual_address: SECTION_END_VA
            })
        );
    }

    #[test]
    fn rejects_bad_string_pointers() {
        for bad in [0, BASE + 0x1000, SECTION_END_VA, u32::MAX] {
            let mut bytes = synthetic_pe();
            put_u32(&mut bytes, file_offset(TABLE_VA) + 4, bad);
            assert_eq!(
                extract_static_tables(&bytes, &layout()),
                Err(StaticTablesError::BadStringPointer {
                    name: "synthetic_names".into(),
                    index: Some(1),
                    virtual_address: bad
                })
            );
        }
    }

    #[test]
    fn rejects_strings_without_a_terminator() {
        // STATIC_STRING_MAX_BYTES non-NUL bytes: no terminator within bound.
        let at = SECTION_VA + 0x900;
        let mut bytes = synthetic_pe();
        bytes[file_offset(at)..file_offset(at) + STATIC_STRING_MAX_BYTES].fill(b'x');
        bytes[file_offset(at) + STATIC_STRING_MAX_BYTES] = 0;
        put_u32(&mut bytes, file_offset(TABLE_VA) + 8, at);
        assert_eq!(
            extract_static_tables(&bytes, &layout()),
            Err(StaticTablesError::UnterminatedString {
                name: "synthetic_names".into(),
                index: Some(2),
                virtual_address: at
            })
        );
        // One byte shorter: the NUL is the last byte inside the bound.
        bytes[file_offset(at) + STATIC_STRING_MAX_BYTES - 1] = 0;
        let got = extract_static_tables(&bytes, &layout()).unwrap();
        assert_eq!(
            got.strings("synthetic_names").unwrap()[2].len(),
            STATIC_STRING_MAX_BYTES - 1
        );
        // A lone string that runs off the end of its section.
        let mut bytes = synthetic_pe();
        let tail = SECTION_END_VA - 4;
        bytes[file_offset(tail)..file_offset(tail) + 4].fill(b'y');
        let mut l = layout();
        l.entries[2].va = tail;
        assert_eq!(
            extract_static_tables(&bytes, &l),
            Err(StaticTablesError::UnterminatedString {
                name: "synthetic_name".into(),
                index: None,
                virtual_address: tail
            })
        );
    }

    #[test]
    fn rejects_malformed_layouts() {
        let bytes = synthetic_pe();
        let mut dup = layout();
        dup.entries[2].name = "synthetic_block".into();
        assert_eq!(
            extract_static_tables(&bytes, &dup),
            Err(StaticTablesError::DuplicateName("synthetic_block".into()))
        );
        for name in [
            "",
            "Upper",
            "has-dash",
            &"x".repeat(STATIC_TABLE_NAME_MAX_BYTES + 1),
        ] {
            let mut l = layout();
            l.entries[0].name = name.into();
            assert_eq!(
                extract_static_tables(&bytes, &l),
                Err(StaticTablesError::InvalidName(name.into()))
            );
        }
        let mut longest = layout();
        longest.entries[0].name = "x".repeat(STATIC_TABLE_NAME_MAX_BYTES);
        extract_static_tables(&bytes, &longest).unwrap();
        for kind in [
            StaticTableKind::Block { len: 0 },
            StaticTableKind::Block {
                len: STATIC_BLOCK_MAX_BYTES as u32 + 1,
            },
            StaticTableKind::StringPointers { count: 0 },
            StaticTableKind::StringPointers {
                count: STATIC_STRING_TABLE_MAX_COUNT as u32 + 1,
            },
        ] {
            let mut l = layout();
            l.entries[1].kind = kind;
            assert_eq!(
                extract_static_tables(&bytes, &l),
                Err(StaticTablesError::InvalidSize("synthetic_names".into()))
            );
        }
        let mut misaligned = layout();
        misaligned.entries[1].va += 2;
        assert_eq!(
            extract_static_tables(&bytes, &misaligned),
            Err(StaticTablesError::Misaligned {
                name: "synthetic_names".into(),
                virtual_address: TABLE_VA + 2,
                alignment: 4
            })
        );
        let mut empty = layout();
        empty.entries.clear();
        assert_eq!(
            extract_static_tables(&bytes, &empty),
            Err(StaticTablesError::EntryCount(0))
        );
        let mut many = layout();
        many.entries = (0..=STATIC_TABLES_MAX_ENTRIES)
            .map(|i| StaticTableEntry {
                name: format!("e{i}"),
                va: BLOCK_VA,
                kind: StaticTableKind::Block { len: 1 },
            })
            .collect();
        assert_eq!(
            extract_static_tables(&bytes, &many),
            Err(StaticTablesError::EntryCount(STATIC_TABLES_MAX_ENTRIES + 1))
        );
        let mut wrong_base = layout();
        wrong_base.image_base += 0x1000;
        assert_eq!(
            extract_static_tables(&bytes, &wrong_base),
            Err(StaticTablesError::ImageBase {
                expected: BASE + 0x1000,
                actual: BASE
            })
        );
        assert!(matches!(
            extract_static_tables(b"MZ", &layout()),
            Err(StaticTablesError::Pe(_))
        ));
        assert!(matches!(
            extract_static_tables(&bytes[..file_offset(BLOCK_VA) + 4], &layout()),
            Err(StaticTablesError::Pe(text)) if text.contains("Truncated")
        ));
    }

    #[test]
    fn codec_round_trips() {
        let tables = expected();
        let encoded = encode_static_tables(&tables);
        assert_eq!(&encoded[..8], MAGIC);
        assert!(encoded.len() <= STATIC_TABLES_MAX_ENCODED_LEN);
        assert_eq!(decode_static_tables(&encoded).unwrap(), tables);
    }

    #[test]
    fn decode_rejects_truncated_payloads_at_every_length() {
        let encoded = encode_static_tables(&expected());
        for len in 0..encoded.len() {
            assert!(
                matches!(
                    decode_static_tables(&encoded[..len]),
                    Err(StaticTablesError::Truncated { .. })
                ),
                "length {len}"
            );
        }
        assert_eq!(
            decode_static_tables(&encoded[..10]),
            Err(StaticTablesError::Truncated {
                offset: 8,
                needed: 4,
                available: 2
            })
        );
    }

    #[test]
    fn decode_reports_header_kind_field_and_trailing_offsets() {
        let encoded = encode_static_tables(&expected());
        let mut trailing = encoded.clone();
        trailing.push(0);
        assert_eq!(
            decode_static_tables(&trailing),
            Err(StaticTablesError::TrailingBytes {
                offset: encoded.len(),
                len: 1
            })
        );
        for (flip, offset) in [(0, 0), (7, 0), (8, 8), (14, 14)] {
            let mut c = encoded.clone();
            c[flip] ^= 1;
            assert_eq!(
                decode_static_tables(&c),
                Err(StaticTablesError::InvalidHeader { offset })
            );
        }
        for count in [0u16, STATIC_TABLES_MAX_ENTRIES as u16 + 1] {
            let mut c = encoded.clone();
            c[12..14].copy_from_slice(&count.to_le_bytes());
            assert_eq!(
                decode_static_tables(&c),
                Err(StaticTablesError::InvalidHeader { offset: 12 })
            );
        }
        // First entry: name length at 16, name "synthetic_block" (15 bytes),
        // kind at 32, VA 33..37, block length at 37.
        let mut c = encoded.clone();
        c[17] = b'-';
        assert_eq!(
            decode_static_tables(&c),
            Err(StaticTablesError::InvalidField { offset: 16 })
        );
        let mut c = encoded.clone();
        c[32] = 9;
        assert_eq!(
            decode_static_tables(&c),
            Err(StaticTablesError::InvalidKind { offset: 32 })
        );
        for len in [0u32, STATIC_BLOCK_MAX_BYTES as u32 + 1] {
            let mut c = encoded.clone();
            c[37..41].copy_from_slice(&len.to_le_bytes());
            assert_eq!(
                decode_static_tables(&c),
                Err(StaticTablesError::InvalidField { offset: 37 })
            );
        }
        // Second entry starts at 41 + 9 = 50: name length 15 → kind at 66,
        // count at 71, first pointer 73..77, string length at 77, bytes at 79.
        let mut c = encoded.clone();
        c[71..73].copy_from_slice(&0u16.to_le_bytes());
        assert_eq!(
            decode_static_tables(&c),
            Err(StaticTablesError::InvalidField { offset: 71 })
        );
        let mut c = encoded.clone();
        c[77..79].copy_from_slice(&(STATIC_STRING_MAX_BYTES as u16).to_le_bytes());
        assert_eq!(
            decode_static_tables(&c),
            Err(StaticTablesError::InvalidField { offset: 77 })
        );
        let mut c = encoded.clone();
        c[80] = 0;
        assert_eq!(
            decode_static_tables(&c),
            Err(StaticTablesError::InvalidField { offset: 80 })
        );
        // Duplicate names in an otherwise valid payload.
        let mut dup = expected().entries;
        dup[2].name = "synthetic_names".into();
        let raw = StaticTables { entries: dup };
        assert_eq!(
            decode_static_tables(&encode_static_tables(&raw)),
            Err(StaticTablesError::DuplicateName("synthetic_names".into()))
        );
    }

    #[test]
    fn new_rejects_inconsistent_tables() {
        let mut entries = expected().entries;
        entries[1].name = "synthetic_block".into();
        assert_eq!(
            StaticTables::new(entries),
            Err(StaticTablesError::DuplicateName("synthetic_block".into()))
        );
        let mut entries = expected().entries;
        if let StaticTableData::StringPointers(t) = &mut entries[1].data {
            t.pointers.pop();
        }
        assert_eq!(
            StaticTables::new(entries),
            Err(StaticTablesError::InvalidSize("synthetic_names".into()))
        );
        let mut entries = expected().entries;
        entries[2].data = StaticTableData::String(b"a\0b".to_vec());
        assert_eq!(
            StaticTables::new(entries),
            Err(StaticTablesError::InvalidSize("synthetic_name".into()))
        );
        assert_eq!(
            StaticTables::new(Vec::new()),
            Err(StaticTablesError::EntryCount(0))
        );
    }
}
