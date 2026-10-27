//! Stage/track bundle and cross-file reference formats (item E08).
//!
//! # Provenance
//!
//! A rally stage is **not a single container file and not a CMPR bundle**. It is
//! a set of sibling files that share one base path and differ only by extension
//! (the master loader `FUN_004228d0` walks a 14-entry extension table at
//! `0x4eb92c` and loads `<stagebase><ext>` for each through the shared,
//! CMPR-capable loader `FUN_00495ed0`). There is no stage-level magic, header, or
//! offset directory: the "directory" is the filesystem directory plus the
//! extension convention, and each *section file* may independently be CMPR
//! wrapped. This module therefore decodes the individual section payloads whose
//! on-disk layout was confidently recovered, and records the bundle structure as
//! metadata.
//!
//! Recovered, decodable payloads:
//! - The **`.cat` node spline** (`FUN_00466350` -> `DAT_0057ae6c`): a header-less
//!   flat array of `0x1C`-byte records; the element count is `filesize / 0x1C`.
//!   This is the timing/split line (not an AI racing line).
//! - The **`{e,i,n}.cjp` split-time reference**: a block of 13 dwords selected by
//!   difficulty (novice/intermediate/easy).
//! - The **trigger-volume record** (`0x30` bytes; walked by `FUN_00403a00`,
//!   linked by the fix-up `FUN_004904e0`): its containing section file was not
//!   confirmed, so only the record decode is reconstructed here.
//!
//! The semantic classification of a trigger's `raw_type` byte lives in
//! `cmr-core::collision` (`TriggerType`/`surface_switch_index`); this crate
//! stays a pure byte decoder and preserves the raw value. Unknown bytes are
//! preserved rather than dropped. Only format metadata is encoded - no game
//! payload bytes are vendored; fixtures are tiny synthetic records.
//!
//! See evidence `rea.official-eu.stage-bundle`.

use std::fmt;

use crate::c3d::{C3dError, C3dFile, C3dLimits, parse_c3d};

/// Stride of one `.cat` node record (7 dwords).
pub const CAT_NODE_STRIDE: usize = 0x1c;

/// Expected number of split nodes in a well-formed stage ("should be 7, not
/// including start/finish" - a developer diagnostic, enforced only as a check).
pub const EXPECTED_SPLIT_NODES: usize = 7;

/// Number of dwords the loader copies from a `.cjp` split-time file.
pub const CJP_DWORD_COUNT: usize = 13;

/// Minimum `.cjp` length (the 13 dwords the loader reads).
pub const CJP_RECORD_LEN: usize = CJP_DWORD_COUNT * 4;

/// Stride of one trigger-volume record.
pub const TRIGGER_VOLUME_STRIDE: usize = 0x30;

/// Length of the (unverified-split) bounding-volume geometry prefix of a trigger
/// record, preserved as raw bytes.
pub const TRIGGER_GEOMETRY_LEN: usize = 0x24;

/// The 14 stage section extensions, in the loader's order (`0x4eb92c` table).
pub const STAGE_EXTENSIONS: [&str; 14] = [
    ".c3d", ".dat", ".obj", ".moj", ".sht", ".ars", ".hzn", ".tsc", ".csp", ".cat", ".spl", ".anm",
    ".rsf", ".fuz",
];

/// Extension of the node-spline section.
pub const CAT_EXTENSION: &str = ".cat";
/// Extension of the 3D world-mesh section.
pub const C3D_EXTENSION: &str = ".c3d";
/// Extension of the horizon/background scene. The shipped loader sends this
/// payload through the same C3D parser as the world scene (`FUN_00480d80`),
/// with render mode 4 and lit vertices.
pub const HZN_EXTENSION: &str = ".hzn";
/// Extension of the four-word stage projection/horizon settings record.
pub const ARS_EXTENSION: &str = ".ars";
/// Bytes consumed by the shipped `.ars` loader (`FUN_00480960`).
pub const ARS_SETTINGS_LEN: usize = 16;
/// Extension of the primary object/scenery C3D graph (parser mode 2).
pub const OBJ_EXTENSION: &str = ".obj";
/// Extension of the secondary object/scenery C3D graph (parser mode 3).
pub const SHT_EXTENSION: &str = ".sht";
/// Extension of the authored car-start records.
pub const CSP_EXTENSION: &str = ".csp";
/// One normal or mirrored `.csp` block: two positions and two rotations.
pub const CSP_BLOCK_LEN: usize = 0x28;

/// Rally-School stage directory template (`%s` = track name, `%d` = task index).
pub const STAGE_DIR_TEMPLATE: &str = r"RALLSCHL\TRACKS\%s\TASK%d\";

/// The per-difficulty `.cjp` split-reference sub-extension (`FUN_00466350`
/// selects by `(cfg & 0x18) >> 3`): 0 -> novice, 1 -> intermediate, 2 -> easy.
pub fn cjp_extension(difficulty_index: u8) -> Option<&'static str> {
    match difficulty_index {
        0 => Some("n.cjp"),
        1 => Some("i.cjp"),
        2 => Some("e.cjp"),
        _ => None,
    }
}

/// Build a stage section file path by appending an extension to the shared base
/// path, as every section loader does in place.
pub fn stage_section_path(base: &str, extension: &str) -> String {
    format!("{base}{extension}")
}

/// Errors from decoding a stage section payload, each carrying the offending
/// length/offset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StageError {
    /// A `.cat` payload whose length is not a whole number of `0x1C` records.
    CatNotNodeAligned { len: usize, stride: usize },
    /// A `.cjp` payload shorter than the 13 dwords the loader copies.
    CjpTooShort { len: usize, needed: usize },
    /// A trigger-volume blob whose length is not a whole number of `0x30`
    /// records.
    TriggerNotAligned { len: usize, stride: usize },
    /// A single trigger-volume slice of the wrong length.
    TriggerRecordLen { len: usize, expected: usize },
    /// An `.ars` payload shorter than the four dwords consumed by the loader.
    ArsTooShort { len: usize, needed: usize },
    /// A present `.csp` payload was neither one complete normal block nor a
    /// complete normal+mirrored pair.
    CspInvalidLength { len: usize },
}

impl fmt::Display for StageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StageError::CatNotNodeAligned { len, stride } => write!(
                f,
                "cat node table length {len} is not a multiple of the {stride}-byte node stride"
            ),
            StageError::CjpTooShort { len, needed } => {
                write!(
                    f,
                    "cjp split table length {len} is shorter than {needed} bytes"
                )
            }
            StageError::TriggerNotAligned { len, stride } => write!(
                f,
                "trigger-volume blob length {len} is not a multiple of the {stride}-byte stride"
            ),
            StageError::TriggerRecordLen { len, expected } => {
                write!(
                    f,
                    "trigger-volume record length {len} is not {expected} bytes"
                )
            }
            StageError::ArsTooShort { len, needed } => {
                write!(
                    f,
                    "ars settings length {len} is shorter than {needed} bytes"
                )
            }
            StageError::CspInvalidLength { len } => write!(
                f,
                "csp start-position length {len} is not {CSP_BLOCK_LEN} bytes or at least {} bytes",
                CSP_BLOCK_LEN * 2
            ),
        }
    }
}

impl std::error::Error for StageError {}

/// Supplies the CMPR-decoded bytes of a stage's sibling section files, keyed by
/// extension (`.c3d`, `.cat`, ...).
///
/// Keeping the byte source abstract keeps this a pure decoder: the base-path
/// resolution ([`stage_section_path`]), file IO, and the shared CMPR decode
/// (`FUN_00495ed0`, handled upstream in `cmr-formats::stored_asset`) all live in
/// the caller, which reads original media in place and hands the already-decoded
/// section payloads here.
pub trait StageSectionSource {
    /// The decoded bytes of the section with this extension, or `None` when the
    /// stage has no such sibling file.
    fn section(&self, extension: &str) -> Option<&[u8]>;
}

/// The decoded sections of one stage needed to render and navigate it: the
/// `.c3d` scene (always present), optional `.hzn` background scene, and the
/// `.cat` node spline (optional).
///
/// This is the recovered front half of the stage bundle (`FUN_004228d0` walks
/// the 14-extension sibling set): it ties the per-section decoders to one stage,
/// so a caller can go from a resolved stage base path to a parsed scene ready
/// for `cmr-core::c3d_mesh::assemble_scene_meshes`. Sections whose decode is not
/// yet reconstructed (`.obj`, `.sht`, ...) are left for later milestones.
#[derive(Debug)]
pub struct StageBundle {
    scene: C3dFile,
    objects: Option<C3dFile>,
    scenery: Option<C3dFile>,
    horizon: Option<C3dFile>,
    ars: Option<ArsSettings>,
    starts: Option<CspStarts>,
    nodes: Option<Vec<CatNode>>,
}

impl StageBundle {
    /// The parsed `.c3d` scene (group-0 node records + group-1 mesh records).
    #[must_use]
    pub fn scene(&self) -> &C3dFile {
        &self.scene
    }

    /// The parsed `.obj` object/scenery graph, if present.
    #[must_use]
    pub fn objects(&self) -> Option<&C3dFile> {
        self.objects.as_ref()
    }

    /// The parsed `.sht` secondary scenery graph, if present.
    #[must_use]
    pub fn scenery(&self) -> Option<&C3dFile> {
        self.scenery.as_ref()
    }

    /// The parsed `.hzn` background scene, if the stage carried one.
    #[must_use]
    pub fn horizon(&self) -> Option<&C3dFile> {
        self.horizon.as_ref()
    }

    /// The parsed `.ars` stage projection/horizon settings, if present.
    #[must_use]
    pub fn ars(&self) -> Option<&ArsSettings> {
        self.ars.as_ref()
    }

    /// Authored normal/mirrored car-start records from `.csp`, if present.
    #[must_use]
    pub fn starts(&self) -> Option<&CspStarts> {
        self.starts.as_ref()
    }

    /// The parsed `.cat` node spline, if the stage carried a `.cat` sibling.
    #[must_use]
    pub fn nodes(&self) -> Option<&[CatNode]> {
        self.nodes.as_deref()
    }
}

/// Failure to load a stage bundle from its section source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StageLoadError {
    /// The stage has no `.c3d` section (the scene is mandatory).
    MissingScene,
    /// The `.c3d` scene payload failed to parse.
    Scene(C3dError),
    /// The optional `.hzn` background scene payload failed to parse.
    Horizon(C3dError),
    /// The optional `.obj` scene payload failed to parse.
    Objects(C3dError),
    /// The optional `.sht` scene payload failed to parse.
    Scenery(C3dError),
    /// The optional `.ars` settings payload failed to decode.
    Ars(StageError),
    /// The optional `.csp` start-position payload failed to decode.
    Starts(StageError),
    /// The `.cat` node-spline payload failed to decode.
    Nodes(StageError),
}

impl fmt::Display for StageLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StageLoadError::MissingScene => write!(f, "stage has no .c3d scene section"),
            StageLoadError::Scene(error) => write!(f, "stage .c3d scene: {error}"),
            StageLoadError::Horizon(error) => write!(f, "stage .hzn horizon: {error}"),
            StageLoadError::Objects(error) => write!(f, "stage .obj objects: {error}"),
            StageLoadError::Scenery(error) => write!(f, "stage .sht scenery: {error}"),
            StageLoadError::Ars(error) => write!(f, "stage .ars settings: {error}"),
            StageLoadError::Starts(error) => write!(f, "stage .csp starts: {error}"),
            StageLoadError::Nodes(error) => write!(f, "stage .cat nodes: {error}"),
        }
    }
}

impl std::error::Error for StageLoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StageLoadError::Scene(error) => Some(error),
            StageLoadError::Horizon(error) => Some(error),
            StageLoadError::Objects(error) => Some(error),
            StageLoadError::Scenery(error) => Some(error),
            StageLoadError::Ars(error) => Some(error),
            StageLoadError::Starts(error) => Some(error),
            StageLoadError::Nodes(error) => Some(error),
            StageLoadError::MissingScene => None,
        }
    }
}

/// Loads a stage bundle from its section source, parsing the `.c3d` scene (with
/// `c3d_limits`) and the `.cat` node spline when present.
///
/// The `.c3d` scene is mandatory ([`StageLoadError::MissingScene`] otherwise);
/// a missing `.cat` yields [`StageBundle::nodes`] `None`. Other sibling sections
/// are not decoded here yet.
pub fn load_stage(
    source: &impl StageSectionSource,
    c3d_limits: C3dLimits,
) -> Result<StageBundle, StageLoadError> {
    let scene_bytes = source
        .section(C3D_EXTENSION)
        .ok_or(StageLoadError::MissingScene)?;
    let scene = parse_c3d(scene_bytes, c3d_limits).map_err(StageLoadError::Scene)?;
    let objects = source
        .section(OBJ_EXTENSION)
        .map(|bytes| parse_c3d(bytes, c3d_limits))
        .transpose()
        .map_err(StageLoadError::Objects)?;
    let scenery = source
        .section(SHT_EXTENSION)
        .map(|bytes| parse_c3d(bytes, c3d_limits))
        .transpose()
        .map_err(StageLoadError::Scenery)?;
    let horizon = source
        .section(HZN_EXTENSION)
        .map(|bytes| parse_c3d(bytes, c3d_limits))
        .transpose()
        .map_err(StageLoadError::Horizon)?;
    let ars = source
        .section(ARS_EXTENSION)
        .map(parse_ars_settings)
        .transpose()
        .map_err(StageLoadError::Ars)?;
    let starts = source
        .section(CSP_EXTENSION)
        .map(parse_csp_starts)
        .transpose()
        .map_err(StageLoadError::Starts)?;
    let nodes = source
        .section(CAT_EXTENSION)
        .map(parse_cat_nodes)
        .transpose()
        .map_err(StageLoadError::Nodes)?;
    Ok(StageBundle {
        scene,
        objects,
        scenery,
        horizon,
        ars,
        starts,
        nodes,
    })
}

/// Four little-endian Q16.16 dwords consumed from a stage `.ars` payload.
///
/// Word zero is observed as the horizon Y offset. The remaining words feed the
/// projection/culling setup, but their exact semantic names are not yet proven,
/// so they remain indexed. Trailing bytes are preserved for later recovery.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArsSettings {
    words: [i32; 4],
    trailing: Vec<u8>,
}

impl ArsSettings {
    #[must_use]
    pub const fn words(&self) -> &[i32; 4] {
        &self.words
    }

    #[must_use]
    pub const fn horizon_y_offset_q16(&self) -> i32 {
        self.words[0]
    }

    #[must_use]
    pub fn trailing(&self) -> &[u8] {
        &self.trailing
    }
}

/// Decodes the four dwords the shipped `.ars` loader consumes and preserves
/// any unconsumed tail. Missing data is handled by the bundle as `None`; a
/// present but truncated record fails closed with its observed length.
pub fn parse_ars_settings(bytes: &[u8]) -> Result<ArsSettings, StageError> {
    if bytes.len() < ARS_SETTINGS_LEN {
        return Err(StageError::ArsTooShort {
            len: bytes.len(),
            needed: ARS_SETTINGS_LEN,
        });
    }
    let mut words = [0i32; 4];
    for (index, word) in words.iter_mut().enumerate() {
        *word = read_i32(bytes, index * 4);
    }
    Ok(ArsSettings {
        words,
        trailing: bytes[ARS_SETTINGS_LEN..].to_vec(),
    })
}

/// One authored car start: Q16.16 position and three 12-bit BAM rotations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CspStart {
    pub position: [i32; 3],
    pub rotation_bam: [u16; 3],
    pub reserved: u16,
}

/// The two player starts for normal direction and, when present, reverse.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CspStarts {
    normal: [CspStart; 2],
    mirrored: Option<[CspStart; 2]>,
    trailing: Vec<u8>,
}

impl CspStarts {
    #[must_use]
    pub const fn normal(&self) -> &[CspStart; 2] {
        &self.normal
    }

    #[must_use]
    pub const fn mirrored(&self) -> Option<&[CspStart; 2]> {
        self.mirrored.as_ref()
    }

    #[must_use]
    pub fn trailing(&self) -> &[u8] {
        &self.trailing
    }
}

fn parse_csp_block(bytes: &[u8]) -> [CspStart; 2] {
    std::array::from_fn(|player| {
        let position_offset = player * 12;
        let rotation_offset = 24 + player * 8;
        CspStart {
            position: std::array::from_fn(|axis| read_i32(bytes, position_offset + axis * 4)),
            rotation_bam: std::array::from_fn(|axis| read_u16(bytes, rotation_offset + axis * 2)),
            reserved: read_u16(bytes, rotation_offset + 6),
        }
    })
}

/// Decodes the native `.csp` layout selected by `FUN_00486110`: one 0x28-byte
/// block is the normal direction; any longer form must contain the second
/// 0x28-byte mirrored block. Extra bytes beyond those blocks are preserved.
pub fn parse_csp_starts(bytes: &[u8]) -> Result<CspStarts, StageError> {
    if bytes.len() != CSP_BLOCK_LEN && bytes.len() < CSP_BLOCK_LEN * 2 {
        return Err(StageError::CspInvalidLength { len: bytes.len() });
    }
    let normal = parse_csp_block(&bytes[..CSP_BLOCK_LEN]);
    let mirrored = (bytes.len() >= CSP_BLOCK_LEN * 2)
        .then(|| parse_csp_block(&bytes[CSP_BLOCK_LEN..CSP_BLOCK_LEN * 2]));
    let consumed = if mirrored.is_some() {
        CSP_BLOCK_LEN * 2
    } else {
        CSP_BLOCK_LEN
    };
    Ok(CspStarts {
        normal,
        mirrored,
        trailing: bytes[consumed..].to_vec(),
    })
}

/// One `.cat` node: a position, a tangent/direction, a split flag, and a
/// sequence index. The byte at `+0x19` is an unrecovered field, preserved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CatNode {
    /// Position `(x, y, z)` at `+0x00/+0x04/+0x08`.
    pub position: [i32; 3],
    /// Tangent/direction `(x, y, z)` at `+0x0C/+0x10/+0x14` (negated on mirror).
    pub direction: [i32; 3],
    /// Split/checkpoint flag at `+0x18` (nonzero = split node).
    pub split_flag: u8,
    /// Preserved unknown byte at `+0x19`.
    pub reserved: u8,
    /// Node sequence index at `+0x1A` (recomputed on mirror).
    pub seq_index: u16,
}

impl CatNode {
    /// Whether this node is a split/checkpoint node.
    pub fn is_split(self) -> bool {
        self.split_flag != 0
    }
}

fn read_i32(bytes: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

/// Parse the header-less `.cat` node spline. The count is the file length
/// divided by the `0x1C` stride; a length that is not a whole number of records
/// is rejected with its length and the stride.
pub fn parse_cat_nodes(input: &[u8]) -> Result<Vec<CatNode>, StageError> {
    if !input.len().is_multiple_of(CAT_NODE_STRIDE) {
        return Err(StageError::CatNotNodeAligned {
            len: input.len(),
            stride: CAT_NODE_STRIDE,
        });
    }
    let nodes = input
        .as_chunks::<CAT_NODE_STRIDE>()
        .0
        .iter()
        .map(|r| CatNode {
            position: [read_i32(r, 0x00), read_i32(r, 0x04), read_i32(r, 0x08)],
            direction: [read_i32(r, 0x0c), read_i32(r, 0x10), read_i32(r, 0x14)],
            split_flag: r[0x18],
            reserved: r[0x19],
            seq_index: read_u16(r, 0x1a),
        })
        .collect();
    Ok(nodes)
}

/// Apply the recovered mirror/reverse transform (`FUN_00466350` when the
/// mirror-stage config bit is set): reverse the node order, negate each
/// direction vector, and recompute each sequence index as `count - 1 - index`.
pub fn mirror_nodes(nodes: &[CatNode]) -> Vec<CatNode> {
    let last = nodes.len().saturating_sub(1);
    nodes
        .iter()
        .rev()
        .map(|n| CatNode {
            position: n.position,
            direction: [-n.direction[0], -n.direction[1], -n.direction[2]],
            split_flag: n.split_flag,
            reserved: n.reserved,
            seq_index: (last.saturating_sub(n.seq_index as usize)) as u16,
        })
        .collect()
}

/// Count the split/checkpoint nodes (compare with [`EXPECTED_SPLIT_NODES`]).
pub fn count_split_nodes(nodes: &[CatNode]) -> usize {
    nodes.iter().filter(|n| n.is_split()).count()
}

/// A `.cjp` per-difficulty split-time reference: the 13 dwords the loader
/// copies, plus any trailing byte count beyond them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CjpSplitTable {
    /// The 13 recovered dwords.
    pub values: [i32; CJP_DWORD_COUNT],
    /// Bytes present beyond the 13 dwords (preserved as a count).
    pub trailing: usize,
}

impl CjpSplitTable {
    /// The first reference value (`+0x00`, the "6000" reference).
    pub fn reference_a(self) -> i32 {
        self.values[0]
    }
    /// The second reference value (`+0x04`, the "12000" reference).
    pub fn reference_b(self) -> i32 {
        self.values[1]
    }
    /// The target split-time entries (the 11 dwords after the two references;
    /// the recovered code wires nine of them to the active split globals).
    pub fn split_times(&self) -> &[i32] {
        &self.values[2..]
    }
}

/// Parse a `.cjp` split-time reference. The loader copies 13 dwords, so a
/// shorter payload is rejected; trailing bytes are preserved as a count.
pub fn parse_cjp(input: &[u8]) -> Result<CjpSplitTable, StageError> {
    if input.len() < CJP_RECORD_LEN {
        return Err(StageError::CjpTooShort {
            len: input.len(),
            needed: CJP_RECORD_LEN,
        });
    }
    let mut values = [0i32; CJP_DWORD_COUNT];
    for (i, value) in values.iter_mut().enumerate() {
        *value = read_i32(input, i * 4);
    }
    Ok(CjpSplitTable {
        values,
        trailing: input.len() - CJP_RECORD_LEN,
    })
}

/// One trigger volume (`0x30` bytes). The bounding-volume geometry split was not
/// verified, so it is preserved as raw bytes; the type byte is kept raw for
/// classification by `cmr-core::collision`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TriggerVolume {
    /// `+0x00..+0x24` bounding-volume geometry, raw (split unverified).
    pub geometry: [u8; TRIGGER_GEOMETRY_LEN],
    /// `+0x24` payload (pacenote id / sound id / resource / surface arg).
    pub payload: i32,
    /// `+0x28` node index on disk (rewritten to a list pointer during fix-up).
    pub node_index: i32,
    /// `+0x2C` type byte (classify with `cmr-core::collision::TriggerType`).
    pub raw_type: u8,
    /// `+0x2D` flag byte.
    pub flag: u8,
    /// `+0x2E` per-car passed-bits de-dupe mask (initial on-disk value).
    pub passed_bits: u16,
}

/// Decode one `0x30`-byte trigger-volume record.
pub fn parse_trigger_volume(record: &[u8]) -> Result<TriggerVolume, StageError> {
    if record.len() != TRIGGER_VOLUME_STRIDE {
        return Err(StageError::TriggerRecordLen {
            len: record.len(),
            expected: TRIGGER_VOLUME_STRIDE,
        });
    }
    let mut geometry = [0u8; TRIGGER_GEOMETRY_LEN];
    geometry.copy_from_slice(&record[0x00..TRIGGER_GEOMETRY_LEN]);
    Ok(TriggerVolume {
        geometry,
        payload: read_i32(record, 0x24),
        node_index: read_i32(record, 0x28),
        raw_type: record[0x2c],
        flag: record[0x2d],
        passed_bits: read_u16(record, 0x2e),
    })
}

/// Decode a flat array of trigger-volume records (stride `0x30`).
pub fn parse_trigger_volumes(input: &[u8]) -> Result<Vec<TriggerVolume>, StageError> {
    if !input.len().is_multiple_of(TRIGGER_VOLUME_STRIDE) {
        return Err(StageError::TriggerNotAligned {
            len: input.len(),
            stride: TRIGGER_VOLUME_STRIDE,
        });
    }
    input
        .as_chunks::<TRIGGER_VOLUME_STRIDE>()
        .0
        .iter()
        .map(|r| parse_trigger_volume(r))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build one synthetic `.cat` node record.
    fn cat_record(pos: [i32; 3], dir: [i32; 3], split: u8, reserved: u8, seq: u16) -> Vec<u8> {
        let mut r = Vec::with_capacity(CAT_NODE_STRIDE);
        for v in pos.into_iter().chain(dir) {
            r.extend_from_slice(&v.to_le_bytes());
        }
        r.push(split);
        r.push(reserved);
        r.extend_from_slice(&seq.to_le_bytes());
        assert_eq!(r.len(), CAT_NODE_STRIDE);
        r
    }

    #[test]
    fn parses_cat_nodes_and_counts_from_file_size() {
        let mut buf = cat_record([1, 2, 3], [10, 20, 30], 0, 0x7, 0);
        buf.extend(cat_record([4, 5, 6], [-7, -8, -9], 1, 0, 1));
        let nodes = parse_cat_nodes(&buf).unwrap();
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].position, [1, 2, 3]);
        assert_eq!(nodes[0].direction, [10, 20, 30]);
        assert_eq!(nodes[0].reserved, 0x7);
        assert!(!nodes[0].is_split());
        assert_eq!(nodes[1].direction, [-7, -8, -9]);
        assert!(nodes[1].is_split());
        assert_eq!(nodes[1].seq_index, 1);
    }

    #[test]
    fn empty_cat_is_zero_nodes_misaligned_is_rejected() {
        assert_eq!(parse_cat_nodes(&[]).unwrap().len(), 0);
        let err = parse_cat_nodes(&[0u8; CAT_NODE_STRIDE + 1]).unwrap_err();
        assert_eq!(
            err,
            StageError::CatNotNodeAligned {
                len: CAT_NODE_STRIDE + 1,
                stride: CAT_NODE_STRIDE,
            }
        );
    }

    #[test]
    fn mirror_reverses_negates_and_renumbers() {
        let mut buf = cat_record([0, 0, 0], [1, 2, 3], 0, 0, 0);
        buf.extend(cat_record([9, 9, 9], [4, 5, 6], 1, 0, 1));
        buf.extend(cat_record([7, 7, 7], [7, 8, 9], 0, 0, 2));
        let nodes = parse_cat_nodes(&buf).unwrap();
        let mirrored = mirror_nodes(&nodes);
        assert_eq!(mirrored.len(), 3);
        // First mirrored node is the original last, direction negated, seq 2->0.
        assert_eq!(mirrored[0].position, [7, 7, 7]);
        assert_eq!(mirrored[0].direction, [-7, -8, -9]);
        assert_eq!(mirrored[0].seq_index, 0);
        // Last mirrored node is the original first, seq 0->2.
        assert_eq!(mirrored[2].position, [0, 0, 0]);
        assert_eq!(mirrored[2].direction, [-1, -2, -3]);
        assert_eq!(mirrored[2].seq_index, 2);
        // The split node is preserved through the mirror.
        assert_eq!(count_split_nodes(&mirrored), 1);
    }

    #[test]
    fn parses_cjp_references_and_splits() {
        let mut buf = Vec::new();
        for i in 0..CJP_DWORD_COUNT as i32 {
            buf.extend_from_slice(&(i * 100).to_le_bytes());
        }
        buf.extend_from_slice(&[0xaa, 0xbb]); // trailing
        let table = parse_cjp(&buf).unwrap();
        assert_eq!(table.reference_a(), 0);
        assert_eq!(table.reference_b(), 100);
        assert_eq!(table.split_times().len(), 11);
        assert_eq!(table.split_times()[0], 200);
        assert_eq!(table.trailing, 2);
    }

    #[test]
    fn cjp_too_short_is_rejected() {
        let err = parse_cjp(&[0u8; CJP_RECORD_LEN - 1]).unwrap_err();
        assert_eq!(
            err,
            StageError::CjpTooShort {
                len: CJP_RECORD_LEN - 1,
                needed: CJP_RECORD_LEN,
            }
        );
    }

    #[test]
    fn parses_trigger_volume_record() {
        let mut r = vec![0u8; TRIGGER_VOLUME_STRIDE];
        r[0x00] = 0x11; // geometry sentinel
        r[0x23] = 0x22;
        r[0x24..0x28].copy_from_slice(&42i32.to_le_bytes());
        r[0x28..0x2c].copy_from_slice(&5i32.to_le_bytes());
        r[0x2c] = 0x15; // out-of-bounds type
        r[0x2d] = 1;
        r[0x2e..0x30].copy_from_slice(&0x0102u16.to_le_bytes());
        let v = parse_trigger_volume(&r).unwrap();
        assert_eq!(v.geometry[0], 0x11);
        assert_eq!(v.geometry[0x23], 0x22);
        assert_eq!(v.payload, 42);
        assert_eq!(v.node_index, 5);
        assert_eq!(v.raw_type, 0x15);
        assert_eq!(v.flag, 1);
        assert_eq!(v.passed_bits, 0x0102);
    }

    #[test]
    fn trigger_volume_length_errors() {
        assert_eq!(
            parse_trigger_volume(&[0u8; 10]).unwrap_err(),
            StageError::TriggerRecordLen {
                len: 10,
                expected: TRIGGER_VOLUME_STRIDE,
            }
        );
        assert_eq!(
            parse_trigger_volumes(&[0u8; TRIGGER_VOLUME_STRIDE + 3]).unwrap_err(),
            StageError::TriggerNotAligned {
                len: TRIGGER_VOLUME_STRIDE + 3,
                stride: TRIGGER_VOLUME_STRIDE,
            }
        );
        let two = vec![0u8; TRIGGER_VOLUME_STRIDE * 2];
        assert_eq!(parse_trigger_volumes(&two).unwrap().len(), 2);
    }

    #[test]
    fn bundle_metadata_matches_recovery() {
        assert_eq!(STAGE_EXTENSIONS.len(), 14);
        assert_eq!(STAGE_EXTENSIONS[0], ".c3d");
        assert_eq!(STAGE_EXTENSIONS[9], ".cat");
        assert_eq!(STAGE_EXTENSIONS[13], ".fuz");
        assert_eq!(
            stage_section_path(r"TRACKS\MONACO\TASK1\stage", CAT_EXTENSION),
            r"TRACKS\MONACO\TASK1\stage.cat"
        );
        assert_eq!(cjp_extension(0), Some("n.cjp"));
        assert_eq!(cjp_extension(1), Some("i.cjp"));
        assert_eq!(cjp_extension(2), Some("e.cjp"));
        assert_eq!(cjp_extension(3), None);
    }

    /// A stage section source backed by an in-memory extension -> bytes map.
    struct MapSource(std::collections::HashMap<&'static str, Vec<u8>>);

    impl StageSectionSource for MapSource {
        fn section(&self, extension: &str) -> Option<&[u8]> {
            self.0.get(extension).map(Vec::as_slice)
        }
    }

    /// A minimal valid header-only CM3D payload (no records).
    fn empty_scene_bytes() -> Vec<u8> {
        let mut buf = vec![0u8; 32];
        buf[0..4].copy_from_slice(b"CM3D");
        buf[4..8].copy_from_slice(&100u32.to_le_bytes()); // version
        buf[20..24].copy_from_slice(&32u32.to_le_bytes()); // group0 at end
        buf[24..28].copy_from_slice(&32u32.to_le_bytes()); // group1 at end
        buf[28..32].copy_from_slice(&32u32.to_le_bytes()); // blob at end
        buf
    }

    #[test]
    fn load_stage_parses_the_scene_and_optional_cat_nodes() {
        let mut sections = std::collections::HashMap::new();
        sections.insert(C3D_EXTENSION, empty_scene_bytes());
        sections.insert(CAT_EXTENSION, vec![0u8; CAT_NODE_STRIDE * 2]); // two nodes
        let source = MapSource(sections);

        let bundle = load_stage(&source, C3dLimits::default()).unwrap();
        assert_eq!(bundle.scene().group0_records().count(), 0);
        assert!(bundle.objects().is_none());
        assert!(bundle.scenery().is_none());
        assert!(bundle.horizon().is_none());
        assert!(bundle.ars().is_none());
        assert!(bundle.starts().is_none());
        assert_eq!(bundle.nodes().unwrap().len(), 2);
    }

    #[test]
    fn load_stage_parses_an_optional_horizon_as_c3d() {
        let mut sections = std::collections::HashMap::new();
        sections.insert(C3D_EXTENSION, empty_scene_bytes());
        sections.insert(HZN_EXTENSION, empty_scene_bytes());
        let source = MapSource(sections);

        let bundle = load_stage(&source, C3dLimits::default()).unwrap();
        assert_eq!(bundle.horizon().unwrap().group0_records().count(), 0);
    }

    #[test]
    fn load_stage_parses_optional_object_and_scenery_c3d_graphs() {
        let mut sections = std::collections::HashMap::new();
        sections.insert(C3D_EXTENSION, empty_scene_bytes());
        sections.insert(OBJ_EXTENSION, empty_scene_bytes());
        sections.insert(SHT_EXTENSION, empty_scene_bytes());
        let source = MapSource(sections);

        let bundle = load_stage(&source, C3dLimits::default()).unwrap();
        assert_eq!(bundle.objects().unwrap().group0_records().count(), 0);
        assert_eq!(bundle.scenery().unwrap().group0_records().count(), 0);
    }

    #[test]
    fn load_stage_identifies_the_malformed_optional_c3d_graph() {
        for (extension, is_expected) in [
            (
                OBJ_EXTENSION,
                StageLoadError::Objects as fn(C3dError) -> StageLoadError,
            ),
            (
                SHT_EXTENSION,
                StageLoadError::Scenery as fn(C3dError) -> StageLoadError,
            ),
        ] {
            let mut sections = std::collections::HashMap::new();
            sections.insert(C3D_EXTENSION, empty_scene_bytes());
            sections.insert(extension, b"short".to_vec());
            let error = load_stage(&MapSource(sections), C3dLimits::default()).unwrap_err();
            assert_eq!(
                error,
                is_expected(C3dError::TruncatedHeader {
                    actual: 5,
                    required: 32,
                })
            );
        }
    }

    #[test]
    fn load_stage_propagates_a_malformed_horizon_section() {
        let mut sections = std::collections::HashMap::new();
        sections.insert(C3D_EXTENSION, empty_scene_bytes());
        sections.insert(HZN_EXTENSION, b"not a c3d".to_vec());
        let source = MapSource(sections);

        assert!(matches!(
            load_stage(&source, C3dLimits::default()).unwrap_err(),
            StageLoadError::Horizon(C3dError::TruncatedHeader { .. })
        ));
    }

    #[test]
    fn ars_settings_decode_four_words_and_preserve_the_tail() {
        let mut bytes = Vec::new();
        for word in [-0x20000i32, 0x960000, 0xc80000, 0x640000] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend_from_slice(&[0xaa, 0xbb]);
        let settings = parse_ars_settings(&bytes).unwrap();
        assert_eq!(settings.words(), &[-0x20000, 0x960000, 0xc80000, 0x640000]);
        assert_eq!(settings.horizon_y_offset_q16(), -0x20000);
        assert_eq!(settings.trailing(), &[0xaa, 0xbb]);
    }

    #[test]
    fn load_stage_rejects_a_present_but_truncated_ars_section() {
        let mut sections = std::collections::HashMap::new();
        sections.insert(C3D_EXTENSION, empty_scene_bytes());
        sections.insert(ARS_EXTENSION, vec![0u8; ARS_SETTINGS_LEN - 1]);
        let source = MapSource(sections);
        assert_eq!(
            load_stage(&source, C3dLimits::default()).unwrap_err(),
            StageLoadError::Ars(StageError::ArsTooShort {
                len: ARS_SETTINGS_LEN - 1,
                needed: ARS_SETTINGS_LEN,
            })
        );
    }

    fn csp_block(base: i32) -> Vec<u8> {
        let mut bytes = Vec::new();
        for player in 0..2 {
            for axis in 0..3 {
                bytes.extend_from_slice(&(base + player * 10 + axis).to_le_bytes());
            }
        }
        for player in 0..2u16 {
            for axis in 0..3u16 {
                bytes.extend_from_slice(&(0x100 + player * 10 + axis).to_le_bytes());
            }
            bytes.extend_from_slice(&(0x9000 + player).to_le_bytes());
        }
        bytes
    }

    #[test]
    fn csp_decodes_two_normal_and_optional_mirrored_starts() {
        let normal = csp_block(100);
        let starts = parse_csp_starts(&normal).unwrap();
        assert_eq!(starts.normal()[0].position, [100, 101, 102]);
        assert_eq!(starts.normal()[1].position, [110, 111, 112]);
        assert_eq!(starts.normal()[0].rotation_bam, [0x100, 0x101, 0x102]);
        assert_eq!(starts.normal()[1].reserved, 0x9001);
        assert!(starts.mirrored().is_none());

        let mut both = normal;
        both.extend_from_slice(&csp_block(200));
        both.extend_from_slice(&[0xaa, 0xbb]);
        let starts = parse_csp_starts(&both).unwrap();
        assert_eq!(starts.mirrored().unwrap()[0].position, [200, 201, 202]);
        assert_eq!(starts.trailing(), &[0xaa, 0xbb]);
    }

    #[test]
    fn csp_rejects_truncated_second_block() {
        assert_eq!(
            parse_csp_starts(&[0; CSP_BLOCK_LEN + 1]).unwrap_err(),
            StageError::CspInvalidLength {
                len: CSP_BLOCK_LEN + 1
            }
        );
    }

    #[test]
    fn load_stage_without_a_cat_sibling_has_no_nodes() {
        let mut sections = std::collections::HashMap::new();
        sections.insert(C3D_EXTENSION, empty_scene_bytes());
        let source = MapSource(sections);

        let bundle = load_stage(&source, C3dLimits::default()).unwrap();
        assert!(bundle.nodes().is_none());
    }

    #[test]
    fn load_stage_requires_the_c3d_scene() {
        let source = MapSource(std::collections::HashMap::new());
        assert_eq!(
            load_stage(&source, C3dLimits::default()).unwrap_err(),
            StageLoadError::MissingScene
        );
    }

    #[test]
    fn load_stage_propagates_a_malformed_cat_section() {
        let mut sections = std::collections::HashMap::new();
        sections.insert(C3D_EXTENSION, empty_scene_bytes());
        sections.insert(CAT_EXTENSION, vec![0u8; CAT_NODE_STRIDE + 3]); // misaligned
        let source = MapSource(sections);

        assert_eq!(
            load_stage(&source, C3dLimits::default()).unwrap_err(),
            StageLoadError::Nodes(StageError::CatNotNodeAligned {
                len: CAT_NODE_STRIDE + 3,
                stride: CAT_NODE_STRIDE,
            })
        );
    }
}
