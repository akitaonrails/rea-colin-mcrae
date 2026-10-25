//! Decoded stage `.hpc` height/geometry payload.
//!
//! `FUN_00480a10` loads the `.hpc` sibling through the shared CMPR-capable
//! resource loader and passes the decoded payload to `FUN_00447640`. The latter
//! constructs runtime views over five spatial-cell levels, vertices, triangle
//! records, a candidate-handle pool, and the root grid dimensions. This module
//! parses that decoded payload only; filesystem lookup and gzip/CMPR removal
//! remain the caller's responsibility.
//!
//! Unknown/reserved words are retained. Fixtures are synthetic and contain no
//! bytes copied from the game.

use std::fmt;

pub const HPC_HEADER_LEN: usize = 0x14;
pub const HPC_LEVEL_COUNT: usize = 5;
pub const HPC_CELL_LEN: usize = 8;
pub const HPC_VERTEX_LEN: usize = 12;
pub const HPC_RECORD_LEN: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HpcSpatialCell {
    pub marker: i16,
    pub reserved: u16,
    pub link_or_candidate_offset: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HpcRecord {
    pub vertex_indices: [u16; 3],
    pub type_word: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HpcFile {
    origin_xz: [i32; 2],
    header_reserved: u16,
    levels: [Vec<HpcSpatialCell>; HPC_LEVEL_COUNT],
    vertex_count_reserved: u16,
    vertices: Vec<[i32; 3]>,
    record_count_reserved: u16,
    records: Vec<HpcRecord>,
    candidates: Vec<i16>,
    width: i16,
    height: i16,
    trailing_bytes: Vec<u8>,
}

impl HpcFile {
    #[must_use]
    pub const fn origin_xz(&self) -> [i32; 2] {
        self.origin_xz
    }

    #[must_use]
    pub const fn header_reserved(&self) -> u16 {
        self.header_reserved
    }

    #[must_use]
    pub fn levels(&self) -> &[Vec<HpcSpatialCell>; HPC_LEVEL_COUNT] {
        &self.levels
    }

    #[must_use]
    pub const fn vertex_count_reserved(&self) -> u16 {
        self.vertex_count_reserved
    }

    #[must_use]
    pub fn vertices(&self) -> &[[i32; 3]] {
        &self.vertices
    }

    #[must_use]
    pub const fn record_count_reserved(&self) -> u16 {
        self.record_count_reserved
    }

    #[must_use]
    pub fn records(&self) -> &[HpcRecord] {
        &self.records
    }

    #[must_use]
    pub fn candidates(&self) -> &[i16] {
        &self.candidates
    }

    #[must_use]
    pub const fn width(&self) -> i16 {
        self.width
    }

    #[must_use]
    pub const fn height(&self) -> i16 {
        self.height
    }

    #[must_use]
    pub fn trailing_bytes(&self) -> &[u8] {
        &self.trailing_bytes
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HpcError {
    Truncated {
        section: &'static str,
        offset: usize,
        needed: usize,
        available: usize,
    },
    NegativeLevelCount {
        level: usize,
        offset: usize,
        count: i16,
    },
    CountArithmeticOverflow {
        section: &'static str,
        offset: usize,
        count: usize,
        stride: usize,
    },
    NonPositiveDimensions {
        offset: usize,
        width: i16,
        height: i16,
    },
    RootGridOutOfBounds {
        offset: usize,
        required: usize,
        available: usize,
    },
    ChildRangeOutOfBounds {
        level: usize,
        cell_index: usize,
        offset: usize,
        child_start: i32,
        next_level_cells: usize,
    },
    DeepAliasReachable {
        level: usize,
        cell_index: usize,
        offset: usize,
    },
    CandidateRangeOutOfBounds {
        level: usize,
        cell_index: usize,
        offset: usize,
        candidate_start: i32,
        count: i16,
        available: usize,
    },
    CandidateHandleOutOfBounds {
        ordinal: usize,
        offset: usize,
        handle: i16,
        records: usize,
    },
    RecordVertexOutOfBounds {
        record: usize,
        slot: usize,
        offset: usize,
        vertex_index: u16,
        vertices: usize,
    },
}

impl fmt::Display for HpcError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated {
                section,
                offset,
                needed,
                available,
            } => write!(
                formatter,
                "hpc {section} at offset {offset:#x} needs {needed} bytes; {available} remain"
            ),
            Self::NegativeLevelCount {
                level,
                offset,
                count,
            } => write!(
                formatter,
                "hpc level {level} count at offset {offset:#x} is negative ({count})"
            ),
            Self::CountArithmeticOverflow {
                section,
                offset,
                count,
                stride,
            } => write!(
                formatter,
                "hpc {section} count {count} at offset {offset:#x} overflows stride {stride}"
            ),
            Self::NonPositiveDimensions {
                offset,
                width,
                height,
            } => write!(
                formatter,
                "hpc root dimensions at offset {offset:#x} are non-positive ({width}x{height})"
            ),
            Self::RootGridOutOfBounds {
                offset,
                required,
                available,
            } => write!(
                formatter,
                "hpc root grid at offset {offset:#x} needs {required} cells; level 0 has {available}"
            ),
            Self::ChildRangeOutOfBounds {
                level,
                cell_index,
                offset,
                child_start,
                next_level_cells,
            } => write!(
                formatter,
                "hpc level {level} cell {cell_index} at offset {offset:#x} selects 16 children from {child_start}; next level has {next_level_cells}"
            ),
            Self::DeepAliasReachable {
                level,
                cell_index,
                offset,
            } => write!(
                formatter,
                "hpc level {level} cell {cell_index} at offset {offset:#x} reaches the native vertex-pointer alias"
            ),
            Self::CandidateRangeOutOfBounds {
                level,
                cell_index,
                offset,
                candidate_start,
                count,
                available,
            } => write!(
                formatter,
                "hpc level {level} cell {cell_index} at offset {offset:#x} selects {count} candidates from {candidate_start}; pool has {available}"
            ),
            Self::CandidateHandleOutOfBounds {
                ordinal,
                offset,
                handle,
                records,
            } => write!(
                formatter,
                "hpc candidate {ordinal} at offset {offset:#x} has handle {handle}; record table has {records} entries"
            ),
            Self::RecordVertexOutOfBounds {
                record,
                slot,
                offset,
                vertex_index,
                vertices,
            } => write!(
                formatter,
                "hpc record {record} vertex slot {slot} at offset {offset:#x} selects vertex {vertex_index}; table has {vertices} entries"
            ),
        }
    }
}

impl std::error::Error for HpcError {}

fn range<'a>(
    input: &'a [u8],
    offset: usize,
    len: usize,
    section: &'static str,
) -> Result<&'a [u8], HpcError> {
    let end = offset
        .checked_add(len)
        .ok_or(HpcError::CountArithmeticOverflow {
            section,
            offset,
            count: len,
            stride: 1,
        })?;
    input.get(offset..end).ok_or(HpcError::Truncated {
        section,
        offset,
        needed: len,
        available: input.len().saturating_sub(offset),
    })
}

fn counted_len(
    section: &'static str,
    offset: usize,
    count: usize,
    stride: usize,
) -> Result<usize, HpcError> {
    count
        .checked_mul(stride)
        .ok_or(HpcError::CountArithmeticOverflow {
            section,
            offset,
            count,
            stride,
        })
}

fn read_u16(input: &[u8], offset: usize, section: &'static str) -> Result<u16, HpcError> {
    Ok(u16::from_le_bytes(
        range(input, offset, 2, section)?
            .try_into()
            .expect("length checked"),
    ))
}

fn read_i16(input: &[u8], offset: usize, section: &'static str) -> Result<i16, HpcError> {
    Ok(i16::from_le_bytes(
        range(input, offset, 2, section)?
            .try_into()
            .expect("length checked"),
    ))
}

fn read_u32(input: &[u8], offset: usize, section: &'static str) -> Result<u32, HpcError> {
    Ok(u32::from_le_bytes(
        range(input, offset, 4, section)?
            .try_into()
            .expect("length checked"),
    ))
}

fn read_i32(input: &[u8], offset: usize, section: &'static str) -> Result<i32, HpcError> {
    Ok(i32::from_le_bytes(
        range(input, offset, 4, section)?
            .try_into()
            .expect("length checked"),
    ))
}

/// Parses one already-decoded `.hpc` payload and validates every reachable
/// native lookup before returning owned tables.
pub fn parse_hpc(input: &[u8]) -> Result<HpcFile, HpcError> {
    range(input, 0, HPC_HEADER_LEN, "header")?;
    let origin_xz = [
        read_i32(input, 0, "origin x")?,
        read_i32(input, 4, "origin z")?,
    ];
    let mut level_counts = [0_usize; HPC_LEVEL_COUNT];
    for (level, count) in level_counts.iter_mut().enumerate() {
        let offset = 8 + level * 2;
        let signed = read_i16(input, offset, "level count")?;
        *count = usize::try_from(signed).map_err(|_| HpcError::NegativeLevelCount {
            level,
            offset,
            count: signed,
        })?;
    }
    let header_reserved = read_u16(input, 0x12, "reserved header word")?;

    let mut offset = HPC_HEADER_LEN;
    let mut level_offsets = [0_usize; HPC_LEVEL_COUNT];
    let mut parsed_levels = Vec::with_capacity(HPC_LEVEL_COUNT);
    for (level, count) in level_counts.into_iter().enumerate() {
        level_offsets[level] = offset;
        let bytes = counted_len("spatial cells", offset, count, HPC_CELL_LEN)?;
        range(input, offset, bytes, "spatial cells")?;
        let mut cells = Vec::with_capacity(count);
        for index in 0..count {
            let cell_offset = offset + index * HPC_CELL_LEN;
            cells.push(HpcSpatialCell {
                marker: read_i16(input, cell_offset, "spatial cell marker")?,
                reserved: read_u16(input, cell_offset + 2, "spatial cell reserved word")?,
                link_or_candidate_offset: read_i32(input, cell_offset + 4, "spatial cell link")?,
            });
        }
        parsed_levels.push(cells);
        offset += bytes;
    }
    let levels: [Vec<HpcSpatialCell>; HPC_LEVEL_COUNT] = parsed_levels
        .try_into()
        .expect("five level vectors were parsed");

    let vertex_count = usize::from(read_u16(input, offset, "vertex count")?);
    let vertex_count_reserved = read_u16(input, offset + 2, "vertex count reserved word")?;
    offset += 4;
    let vertex_bytes = counted_len("vertices", offset, vertex_count, HPC_VERTEX_LEN)?;
    range(input, offset, vertex_bytes, "vertices")?;
    let mut vertices = Vec::with_capacity(vertex_count);
    for index in 0..vertex_count {
        let base = offset + index * HPC_VERTEX_LEN;
        vertices.push([
            read_i32(input, base, "vertex x")?,
            read_i32(input, base + 4, "vertex y")?,
            read_i32(input, base + 8, "vertex z")?,
        ]);
    }
    offset += vertex_bytes;

    let record_count = usize::from(read_u16(input, offset, "record count")?);
    let record_count_reserved = read_u16(input, offset + 2, "record count reserved word")?;
    offset += 4;
    let record_offset = offset;
    let record_bytes = counted_len("records", offset, record_count, HPC_RECORD_LEN)?;
    range(input, offset, record_bytes, "records")?;
    let mut records = Vec::with_capacity(record_count);
    for index in 0..record_count {
        let base = offset + index * HPC_RECORD_LEN;
        records.push(HpcRecord {
            vertex_indices: [
                read_u16(input, base, "record vertex 0")?,
                read_u16(input, base + 2, "record vertex 1")?,
                read_u16(input, base + 4, "record vertex 2")?,
            ],
            type_word: read_u16(input, base + 6, "record type")?,
        });
    }
    offset += record_bytes;

    let candidate_count =
        usize::try_from(read_u32(input, offset, "candidate count")?).map_err(|_| {
            HpcError::CountArithmeticOverflow {
                section: "candidates",
                offset,
                count: usize::MAX,
                stride: 2,
            }
        })?;
    offset += 4;
    let candidate_offset = offset;
    let candidate_bytes = counted_len("candidates", offset, candidate_count, 2)?;
    range(input, offset, candidate_bytes, "candidates")?;
    let mut candidates = Vec::with_capacity(candidate_count);
    for ordinal in 0..candidate_count {
        candidates.push(read_i16(input, offset + ordinal * 2, "candidate handle")?);
    }
    offset += candidate_bytes;

    let dimensions_offset = offset;
    let width = read_i16(input, offset, "root width")?;
    let height = read_i16(input, offset + 2, "root height")?;
    offset += 4;
    let trailing_bytes = input[offset..].to_vec();

    if width <= 0 || height <= 0 {
        return Err(HpcError::NonPositiveDimensions {
            offset: dimensions_offset,
            width,
            height,
        });
    }
    let root_cells = usize::from(width as u16)
        .checked_mul(usize::from(height as u16))
        .ok_or(HpcError::CountArithmeticOverflow {
            section: "root grid",
            offset: dimensions_offset,
            count: usize::from(width as u16),
            stride: usize::from(height as u16),
        })?;
    if root_cells > levels[0].len() {
        return Err(HpcError::RootGridOutOfBounds {
            offset: dimensions_offset,
            required: root_cells,
            available: levels[0].len(),
        });
    }

    for (record_index, record) in records.iter().enumerate() {
        for (slot, &vertex_index) in record.vertex_indices.iter().enumerate() {
            if usize::from(vertex_index) >= vertices.len() {
                return Err(HpcError::RecordVertexOutOfBounds {
                    record: record_index,
                    slot,
                    offset: record_offset + record_index * HPC_RECORD_LEN + slot * 2,
                    vertex_index,
                    vertices: vertices.len(),
                });
            }
        }
    }

    let mut visited: [Vec<bool>; HPC_LEVEL_COUNT] =
        std::array::from_fn(|level| vec![false; levels[level].len()]);
    let mut pending: Vec<(usize, usize)> = (0..root_cells).map(|index| (0, index)).collect();
    while let Some((level, cell_index)) = pending.pop() {
        if visited[level][cell_index] {
            continue;
        }
        visited[level][cell_index] = true;
        let cell = levels[level][cell_index];
        let cell_offset = level_offsets[level] + cell_index * HPC_CELL_LEN;
        if cell.marker == -1 {
            if level + 1 == HPC_LEVEL_COUNT {
                return Err(HpcError::DeepAliasReachable {
                    level,
                    cell_index,
                    offset: cell_offset,
                });
            }
            let Ok(child_start) = usize::try_from(cell.link_or_candidate_offset) else {
                return Err(HpcError::ChildRangeOutOfBounds {
                    level,
                    cell_index,
                    offset: cell_offset + 4,
                    child_start: cell.link_or_candidate_offset,
                    next_level_cells: levels[level + 1].len(),
                });
            };
            let Some(child_end) = child_start.checked_add(16) else {
                return Err(HpcError::ChildRangeOutOfBounds {
                    level,
                    cell_index,
                    offset: cell_offset + 4,
                    child_start: cell.link_or_candidate_offset,
                    next_level_cells: levels[level + 1].len(),
                });
            };
            if child_end > levels[level + 1].len() {
                return Err(HpcError::ChildRangeOutOfBounds {
                    level,
                    cell_index,
                    offset: cell_offset + 4,
                    child_start: cell.link_or_candidate_offset,
                    next_level_cells: levels[level + 1].len(),
                });
            }
            pending.extend((child_start..child_end).map(|index| (level + 1, index)));
        } else if cell.marker > 0 {
            let Ok(candidate_start) = usize::try_from(cell.link_or_candidate_offset) else {
                return Err(HpcError::CandidateRangeOutOfBounds {
                    level,
                    cell_index,
                    offset: cell_offset + 4,
                    candidate_start: cell.link_or_candidate_offset,
                    count: cell.marker,
                    available: candidates.len(),
                });
            };
            let Some(candidate_end) = candidate_start.checked_add(cell.marker as usize) else {
                return Err(HpcError::CandidateRangeOutOfBounds {
                    level,
                    cell_index,
                    offset: cell_offset + 4,
                    candidate_start: cell.link_or_candidate_offset,
                    count: cell.marker,
                    available: candidates.len(),
                });
            };
            if candidate_end > candidates.len() {
                return Err(HpcError::CandidateRangeOutOfBounds {
                    level,
                    cell_index,
                    offset: cell_offset + 4,
                    candidate_start: cell.link_or_candidate_offset,
                    count: cell.marker,
                    available: candidates.len(),
                });
            }
            for (ordinal, &handle) in candidates
                .iter()
                .enumerate()
                .take(candidate_end)
                .skip(candidate_start)
            {
                if handle < 0 || handle as usize >= records.len() {
                    return Err(HpcError::CandidateHandleOutOfBounds {
                        ordinal,
                        offset: candidate_offset + ordinal * 2,
                        handle,
                        records: records.len(),
                    });
                }
            }
        }
    }

    Ok(HpcFile {
        origin_xz,
        header_reserved,
        levels,
        vertex_count_reserved,
        vertices,
        record_count_reserved,
        records,
        candidates,
        width,
        height,
        trailing_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::stored_asset::{DecodeLimits, decode_stored_asset};

    fn push_i16(bytes: &mut Vec<u8>, value: i16) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn push_u16(bytes: &mut Vec<u8>, value: u16) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn push_i32(bytes: &mut Vec<u8>, value: i32) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn push_u32(bytes: &mut Vec<u8>, value: u32) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn fixture() -> Vec<u8> {
        let mut bytes = Vec::new();
        push_i32(&mut bytes, -100);
        push_i32(&mut bytes, 200);
        for count in [1_i16, 0, 0, 0, 0] {
            push_i16(&mut bytes, count);
        }
        push_u16(&mut bytes, 0x55aa);
        push_i16(&mut bytes, 1);
        push_u16(&mut bytes, 0xa55a);
        push_i32(&mut bytes, 0);
        push_u16(&mut bytes, 3);
        push_u16(&mut bytes, 0x1111);
        for vertex in [[1, 2, 3], [-4, -5, -6], [7, 8, 9]] {
            for component in vertex {
                push_i32(&mut bytes, component);
            }
        }
        push_u16(&mut bytes, 1);
        push_u16(&mut bytes, 0x2222);
        for word in [0_u16, 1, 2, 0xabcd] {
            push_u16(&mut bytes, word);
        }
        push_u32(&mut bytes, 1);
        push_i16(&mut bytes, 0);
        push_i16(&mut bytes, 1);
        push_i16(&mut bytes, 1);
        bytes
    }

    #[test]
    fn parses_complete_payload_and_preserves_unknown_words() {
        let parsed = parse_hpc(&fixture()).unwrap();
        assert_eq!(parsed.origin_xz, [-100, 200]);
        assert_eq!(parsed.header_reserved, 0x55aa);
        assert_eq!(
            parsed.levels.each_ref().map(|level| level.len()),
            [1, 0, 0, 0, 0]
        );
        assert_eq!(parsed.levels[0][0].reserved, 0xa55a);
        assert_eq!(parsed.vertex_count_reserved, 0x1111);
        assert_eq!(parsed.vertices, [[1, 2, 3], [-4, -5, -6], [7, 8, 9]]);
        assert_eq!(parsed.record_count_reserved, 0x2222);
        assert_eq!(parsed.records[0].vertex_indices, [0, 1, 2]);
        assert_eq!(parsed.records[0].type_word, 0xabcd);
        assert_eq!(parsed.candidates, [0]);
        assert_eq!((parsed.width, parsed.height), (1, 1));
        assert!(parsed.trailing_bytes.is_empty());
    }

    #[test]
    fn reports_every_truncated_prefix_without_panicking() {
        let bytes = fixture();
        assert_eq!(
            parse_hpc(&[]),
            Err(HpcError::Truncated {
                section: "header",
                offset: 0,
                needed: HPC_HEADER_LEN,
                available: 0,
            })
        );
        for len in 0..bytes.len() {
            assert!(
                matches!(parse_hpc(&bytes[..len]), Err(HpcError::Truncated { .. })),
                "prefix {len}"
            );
        }
    }

    #[test]
    fn rejects_negative_level_counts_and_preserves_an_opaque_tail() {
        let mut negative = fixture();
        negative[8..10].copy_from_slice(&(-1_i16).to_le_bytes());
        assert_eq!(
            parse_hpc(&negative),
            Err(HpcError::NegativeLevelCount {
                level: 0,
                offset: 8,
                count: -1,
            })
        );

        let mut trailing = fixture();
        trailing.extend_from_slice(&[0xde, 0xad]);
        assert_eq!(parse_hpc(&trailing).unwrap().trailing_bytes(), [0xde, 0xad]);
    }

    #[test]
    fn rejects_bad_dimensions_but_allows_unreachable_extra_root_cells() {
        let mut zero = fixture();
        let end = zero.len();
        zero[end - 4..end - 2].copy_from_slice(&0_i16.to_le_bytes());
        assert_eq!(
            parse_hpc(&zero),
            Err(HpcError::NonPositiveDimensions {
                offset: end - 4,
                width: 0,
                height: 1,
            })
        );

        let mut oversized = fixture();
        let end = oversized.len();
        oversized[end - 2..].copy_from_slice(&2_i16.to_le_bytes());
        assert_eq!(
            parse_hpc(&oversized),
            Err(HpcError::RootGridOutOfBounds {
                offset: end - 4,
                required: 2,
                available: 1,
            })
        );
    }

    #[test]
    fn validates_reachable_child_ranges_and_rejects_the_vertex_alias_path() {
        let mut child = fixture();
        child[HPC_HEADER_LEN..HPC_HEADER_LEN + 2].copy_from_slice(&(-1_i16).to_le_bytes());
        assert_eq!(
            parse_hpc(&child),
            Err(HpcError::ChildRangeOutOfBounds {
                level: 0,
                cell_index: 0,
                offset: HPC_HEADER_LEN + 4,
                child_start: 0,
                next_level_cells: 0,
            })
        );

        let mut bytes = Vec::new();
        push_i32(&mut bytes, 0);
        push_i32(&mut bytes, 0);
        for count in [1_i16, 16, 16, 16, 16] {
            push_i16(&mut bytes, count);
        }
        push_u16(&mut bytes, 0);
        for level in 0..HPC_LEVEL_COUNT {
            let count = if level == 0 { 1 } else { 16 };
            for index in 0..count {
                push_i16(&mut bytes, if index == 0 { -1 } else { 0 });
                push_u16(&mut bytes, 0);
                push_i32(&mut bytes, 0);
            }
        }
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 0);
        push_i16(&mut bytes, 1);
        push_i16(&mut bytes, 1);
        assert_eq!(
            parse_hpc(&bytes),
            Err(HpcError::DeepAliasReachable {
                level: 4,
                cell_index: 0,
                offset: HPC_HEADER_LEN + HPC_CELL_LEN + 3 * 16 * HPC_CELL_LEN,
            })
        );
    }

    #[test]
    fn nonpositive_leaf_ignores_its_offset_but_positive_ranges_are_checked() {
        let mut ignored = fixture();
        ignored[HPC_HEADER_LEN..HPC_HEADER_LEN + 2].copy_from_slice(&0_i16.to_le_bytes());
        ignored[HPC_HEADER_LEN + 4..HPC_HEADER_LEN + 8].copy_from_slice(&i32::MAX.to_le_bytes());
        parse_hpc(&ignored).unwrap();

        let mut invalid = fixture();
        invalid[HPC_HEADER_LEN + 4..HPC_HEADER_LEN + 8].copy_from_slice(&1_i32.to_le_bytes());
        assert_eq!(
            parse_hpc(&invalid),
            Err(HpcError::CandidateRangeOutOfBounds {
                level: 0,
                cell_index: 0,
                offset: HPC_HEADER_LEN + 4,
                candidate_start: 1,
                count: 1,
                available: 1,
            })
        );
    }

    #[test]
    fn validates_candidate_handles_and_record_vertices_at_exact_offsets() {
        let bytes = fixture();
        let candidate_offset = bytes.len() - 6;
        let mut bad_handle = bytes.clone();
        bad_handle[candidate_offset..candidate_offset + 2].copy_from_slice(&(-1_i16).to_le_bytes());
        assert_eq!(
            parse_hpc(&bad_handle),
            Err(HpcError::CandidateHandleOutOfBounds {
                ordinal: 0,
                offset: candidate_offset,
                handle: -1,
                records: 1,
            })
        );

        let record_offset = HPC_HEADER_LEN + HPC_CELL_LEN + 4 + 3 * HPC_VERTEX_LEN + 4;
        let mut bad_vertex = bytes;
        bad_vertex[record_offset + 2..record_offset + 4].copy_from_slice(&3_u16.to_le_bytes());
        assert_eq!(
            parse_hpc(&bad_vertex),
            Err(HpcError::RecordVertexOutOfBounds {
                record: 0,
                slot: 1,
                offset: record_offset + 2,
                vertex_index: 3,
                vertices: 3,
            })
        );
    }

    #[test]
    fn shared_child_ranges_are_deduplicated_and_full_u16_record_counts_are_preserved() {
        let mut shared = Vec::new();
        push_i32(&mut shared, 0);
        push_i32(&mut shared, 0);
        for count in [2_i16, 16, 0, 0, 0] {
            push_i16(&mut shared, count);
        }
        push_u16(&mut shared, 0);
        for _ in 0..2 {
            push_i16(&mut shared, -1);
            push_u16(&mut shared, 0);
            push_i32(&mut shared, 0);
        }
        for _ in 0..16 {
            push_i16(&mut shared, 0);
            push_u16(&mut shared, 0);
            push_i32(&mut shared, i32::MAX);
        }
        push_u32(&mut shared, 0);
        push_u32(&mut shared, 0);
        push_u32(&mut shared, 0);
        push_i16(&mut shared, 2);
        push_i16(&mut shared, 1);
        let parsed = parse_hpc(&shared).unwrap();
        assert_eq!(parsed.levels()[1].len(), 16);

        let mut many_records = Vec::new();
        push_i32(&mut many_records, 0);
        push_i32(&mut many_records, 0);
        for count in [1_i16, 0, 0, 0, 0] {
            push_i16(&mut many_records, count);
        }
        push_u16(&mut many_records, 0);
        push_i16(&mut many_records, 0);
        push_u16(&mut many_records, 0);
        push_i32(&mut many_records, i32::MAX);
        push_u16(&mut many_records, 1);
        push_u16(&mut many_records, 0);
        for component in [1_i32, 2, 3] {
            push_i32(&mut many_records, component);
        }
        let record_count = i16::MAX as usize + 2;
        push_u16(&mut many_records, record_count as u16);
        push_u16(&mut many_records, 0);
        for _ in 0..record_count {
            for word in [0_u16; 4] {
                push_u16(&mut many_records, word);
            }
        }
        push_u32(&mut many_records, 0);
        push_i16(&mut many_records, 1);
        push_i16(&mut many_records, 1);
        assert_eq!(
            parse_hpc(&many_records).unwrap().records().len(),
            record_count
        );
    }

    #[test]
    #[ignore = "requires user-owned media selected only by CMR_ORIGINAL_DIR"]
    fn installed_hpc_sidecars_decode_and_validate_without_copying_payloads() {
        fn visit(root: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(root).unwrap() {
                let entry = entry.unwrap();
                let file_type = entry.file_type().unwrap();
                if file_type.is_dir() {
                    visit(&entry.path(), files);
                } else if file_type.is_file()
                    && entry
                        .path()
                        .extension()
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("hpc"))
                {
                    files.push(entry.path());
                }
            }
        }

        let source = std::env::var_os("CMR_ORIGINAL_DIR")
            .expect("set CMR_ORIGINAL_DIR to a user-owned game source root");
        let mut files = Vec::new();
        visit(std::path::Path::new(&source), &mut files);
        files.sort();
        assert!(!files.is_empty(), "source root contains no .hpc sidecars");
        for path in files {
            let stored = std::fs::read(&path).unwrap();
            let decoded = decode_stored_asset(&stored, DecodeLimits::default()).unwrap();
            parse_hpc(decoded.bytes()).unwrap_or_else(|error| {
                panic!("{}: {error}", path.strip_prefix(&source).unwrap().display())
            });
        }
    }
}
