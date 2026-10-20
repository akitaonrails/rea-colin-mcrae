//! Backend-neutral planning for one selected championship car hierarchy.
//!
//! Static analysis shows that a car C3D creates one actor per group-0 record.
//! This planner preserves those actors and their source attachment order. It
//! owns draw payloads but deliberately stops before binding, rasterization,
//! palette application, or the unresolved pre-draw type-2 word mutation.

use std::fmt;

use crate::{
    c3d_batch::{C3dDrawCommand, C3dDrawOrdinal},
    c3d_topology::C3dObjectTopology,
    championship_assets::CHAMPIONSHIP_CARS,
    championship_car_transform::{
        ChampionshipCarFrame, ChampionshipCarInput, ChampionshipCarState,
    },
    championship_setup::CanonicalCar,
    championship_setup_visual::PixelPoint,
    display::DisplayExtent,
    frontend_c3d_resources::{
        CHAMPIONSHIP_C3D_ENTRY_RESOURCE_SNAPSHOTS, championship_mode8_policy,
    },
    root_menu_c3d_resources::{
        C3dLoadPolicy, C3dPreparedResourceProvenance, C3dVertexMutationBranch, FrontendC3dOwner,
    },
    root_menu_c3d_runtime::C3dResourceScaledObject,
    root_menu_c3d_transform::{
        C3dMatrixWords, C3dViewportWords, ROOT_MENU_C3D_PROJECTION, ROOT_MENU_C3D_VIEW,
        RootMenuC3dViewportError, root_menu_c3d_viewport,
    },
};

/// Recovered parent selected by one group-0 signed field at `+4`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChampionshipCarActorParent {
    PreviewRoot,
    Actor(usize),
}

/// One ordered mesh-level reference attached to an actor by the loader.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChampionshipCarActorLevel {
    value_index: usize,
    source_offset: usize,
    group1_ordinal: u16,
    flags: u32,
}

impl ChampionshipCarActorLevel {
    #[must_use]
    pub const fn value_index(self) -> usize {
        self.value_index
    }

    #[must_use]
    pub const fn source_offset(self) -> usize {
        self.source_offset
    }

    #[must_use]
    pub const fn group1_ordinal(self) -> u16 {
        self.group1_ordinal
    }

    /// Exact group-1 little-endian dword at `+0x14`; bit zero controls a
    /// temporary cull override.
    #[must_use]
    pub const fn flags(self) -> u32 {
        self.flags
    }
}

/// Immutable actor snapshot for one planned scene traversal.
#[derive(Debug, Eq, PartialEq)]
pub struct ChampionshipCarActorNode {
    source_index: usize,
    parent: ChampionshipCarActorParent,
    local_q16: [i32; 16],
    world_q16: Option<[i32; 16]>,
    world_f32_words: Option<[u32; 16]>,
    source_byte_77: u8,
    source_byte_78: u8,
    levels: Box<[ChampionshipCarActorLevel]>,
}

impl ChampionshipCarActorNode {
    #[must_use]
    pub const fn source_index(&self) -> usize {
        self.source_index
    }

    #[must_use]
    pub const fn parent(&self) -> ChampionshipCarActorParent {
        self.parent
    }

    #[must_use]
    pub const fn local_q16(&self) -> [i32; 16] {
        self.local_q16
    }

    #[must_use]
    pub const fn world_q16(&self) -> Option<[i32; 16]> {
        self.world_q16
    }

    /// Exact f32 words produced for a visible actor by signed-i32 -> binary64
    /// Q16 scale -> f32. Invisible parent traversal leaves this unvisited.
    #[must_use]
    pub const fn world_f32_words(&self) -> Option<[u32; 16]> {
        self.world_f32_words
    }

    #[must_use]
    pub const fn source_byte_77(&self) -> u8 {
        self.source_byte_77
    }

    #[must_use]
    pub const fn source_byte_78(&self) -> u8 {
        self.source_byte_78
    }

    #[must_use]
    pub fn levels(&self) -> &[ChampionshipCarActorLevel] {
        &self.levels
    }
}

/// One actor-scoped draw in depth-first source attachment order.
#[derive(Debug, Eq, PartialEq)]
pub struct ChampionshipCarActorDraw {
    actor_index: usize,
    actor_world_q16: [i32; 16],
    actor_world_f32_words: [u32; 16],
    command: C3dDrawCommand,
}

impl ChampionshipCarActorDraw {
    #[must_use]
    pub const fn actor_index(&self) -> usize {
        self.actor_index
    }

    #[must_use]
    pub const fn actor_world_q16(&self) -> [i32; 16] {
        self.actor_world_q16
    }

    #[must_use]
    pub const fn actor_world_f32_words(&self) -> [u32; 16] {
        self.actor_world_f32_words
    }

    #[must_use]
    pub const fn actor_world(&self) -> C3dMatrixWords {
        C3dMatrixWords::from_words(self.actor_world_f32_words)
    }

    #[must_use]
    pub const fn command(&self) -> &C3dDrawCommand {
        &self.command
    }
}

/// Owned description of one selected car's deferred scene submission.
///
/// This type is intentionally non-cloneable: it owns the one command snapshot
/// derived from a specific topology/runtime/frame join.
#[derive(Debug, Eq, PartialEq)]
pub struct ChampionshipCarFramePlan {
    provenance: C3dPreparedResourceProvenance,
    car: CanonicalCar,
    source_path: Box<str>,
    frame: ChampionshipCarFrame,
    extent: DisplayExtent,
    viewport: C3dViewportWords,
    unknown_render_flag_08: bool,
    actors: Box<[ChampionshipCarActorNode]>,
    draws: Box<[ChampionshipCarActorDraw]>,
}

impl ChampionshipCarFramePlan {
    #[must_use]
    pub const fn provenance(&self) -> C3dPreparedResourceProvenance {
        self.provenance
    }

    #[must_use]
    pub const fn car(&self) -> CanonicalCar {
        self.car
    }

    #[must_use]
    pub fn source_path(&self) -> &str {
        &self.source_path
    }

    #[must_use]
    pub const fn selected_parent_visible(&self) -> bool {
        self.frame.visible
    }

    #[must_use]
    pub const fn selected_parent_local_q16(&self) -> [i32; 16] {
        self.frame.local_transform_q16
    }

    #[must_use]
    pub const fn logo_extent(&self) -> (i32, i32) {
        self.frame.logo_extent
    }

    #[must_use]
    pub const fn view(&self) -> C3dMatrixWords {
        ROOT_MENU_C3D_VIEW
    }

    #[must_use]
    pub const fn projection(&self) -> C3dMatrixWords {
        ROOT_MENU_C3D_PROJECTION
    }

    #[must_use]
    pub const fn viewport(&self) -> C3dViewportWords {
        self.viewport
    }

    #[must_use]
    pub const fn extent(&self) -> DisplayExtent {
        self.extent
    }

    /// The unresolved caller-flag bit retained by the mode-8 load contract.
    #[must_use]
    pub const fn unknown_render_flag_08(&self) -> bool {
        self.unknown_render_flag_08
    }

    #[must_use]
    pub fn actors(&self) -> &[ChampionshipCarActorNode] {
        &self.actors
    }

    #[must_use]
    pub fn draws(&self) -> &[ChampionshipCarActorDraw] {
        &self.draws
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ChampionshipCarFramePlanError {
    Viewport(RootMenuC3dViewportError),
    ProvenanceMismatch,
    SourceMismatch {
        car: CanonicalCar,
    },
    PolicyMismatch {
        car: CanonicalCar,
    },
    Group0Count {
        topology: usize,
        runtime: usize,
    },
    Group0Metadata {
        record_index: usize,
    },
    MissingPreviewRoot,
    AdditionalPreviewRoot {
        record_index: usize,
    },
    InvalidNegativeParent {
        record_index: usize,
        parent: i16,
    },
    NonPriorParent {
        record_index: usize,
        parent: usize,
    },
    ReferenceCount {
        record_index: usize,
        topology: usize,
        runtime: usize,
    },
    ReferenceMetadata {
        record_index: usize,
        value_index: usize,
    },
    MissingGroup1 {
        record_index: usize,
        value_index: usize,
        ordinal: u16,
    },
    BatchCount {
        record_index: usize,
        value_index: usize,
        topology: usize,
        runtime: usize,
    },
    BatchMetadata {
        record_index: usize,
        value_index: usize,
        batch_index: usize,
    },
    BatchPayload {
        record_index: usize,
        value_index: usize,
        batch_index: usize,
    },
}

impl fmt::Display for ChampionshipCarFramePlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "cannot plan championship-car C3D frame: {self:?}"
        )
    }
}

impl std::error::Error for ChampionshipCarFramePlanError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Viewport(source) => Some(source),
            _ => None,
        }
    }
}

struct PreflightActor {
    parent: ChampionshipCarActorParent,
    local_q16: [i32; 16],
    source_byte_77: u8,
    source_byte_78: u8,
    levels: Box<[ChampionshipCarActorLevel]>,
    commands: Vec<C3dDrawCommand>,
}

/// Plans one selected car only after validating the complete immutable join.
///
/// Every error occurs before `state` advances.  In particular, malformed
/// hierarchy parents and topology/runtime disagreements cannot consume a spin
/// step or replace the previous logo origin.
#[allow(clippy::too_many_arguments)]
pub fn plan_championship_car_frame(
    topology: &C3dObjectTopology<'_>,
    runtime: &C3dResourceScaledObject,
    state: &mut ChampionshipCarState,
    car: CanonicalCar,
    input: ChampionshipCarInput,
    base_basis: [i32; 9],
    logo_origin: PixelPoint,
    extent: DisplayExtent,
) -> Result<ChampionshipCarFramePlan, ChampionshipCarFramePlanError> {
    let viewport =
        root_menu_c3d_viewport(extent).map_err(ChampionshipCarFramePlanError::Viewport)?;
    preflight_identity(topology, runtime, car)?;
    let (mut actors, root_children) = preflight_actors(topology, runtime)?;

    let frame = state
        .advance_normal_frame(input, base_basis, logo_origin, extent)
        .map_err(ChampionshipCarFramePlanError::Viewport)?;

    let mut nodes: Vec<ChampionshipCarActorNode> = Vec::with_capacity(actors.len());
    for (source_index, actor) in actors.iter().enumerate() {
        let world_q16 = if frame.visible {
            let parent_world = match actor.parent {
                ChampionshipCarActorParent::PreviewRoot => frame.local_transform_q16,
                ChampionshipCarActorParent::Actor(parent) => nodes[parent]
                    .world_q16
                    .expect("visible prior parent has a composed world"),
            };
            Some(compose_q16(actor.local_q16, parent_world))
        } else {
            None
        };
        nodes.push(ChampionshipCarActorNode {
            source_index,
            parent: actor.parent,
            local_q16: actor.local_q16,
            world_q16,
            world_f32_words: world_q16.map(q16_to_f32_words),
            source_byte_77: actor.source_byte_77,
            source_byte_78: actor.source_byte_78,
            levels: actor.levels.clone(),
        });
    }

    let mut children = vec![Vec::new(); actors.len()];
    for (index, actor) in actors.iter().enumerate() {
        if let ChampionshipCarActorParent::Actor(parent) = actor.parent {
            children[parent].push(index);
        }
    }
    let mut traversal = Vec::with_capacity(actors.len());
    for root in root_children {
        append_depth_first(root, &children, &mut traversal);
    }

    let mut draws = Vec::new();
    if frame.visible {
        for actor_index in traversal {
            let world_q16 = nodes[actor_index]
                .world_q16
                .expect("visible actor has a composed world");
            let world_f32_words = nodes[actor_index]
                .world_f32_words
                .expect("visible actor has converted world words");
            for command in std::mem::take(&mut actors[actor_index].commands) {
                draws.push(ChampionshipCarActorDraw {
                    actor_index,
                    actor_world_q16: world_q16,
                    actor_world_f32_words: world_f32_words,
                    command,
                });
            }
        }
    }

    Ok(ChampionshipCarFramePlan {
        provenance: topology.provenance(),
        car,
        source_path: topology.source().path.clone(),
        frame,
        extent,
        viewport,
        unknown_render_flag_08: topology.policy().unknown_render_flag_08(),
        actors: nodes.into_boxed_slice(),
        draws: draws.into_boxed_slice(),
    })
}

fn preflight_identity(
    topology: &C3dObjectTopology<'_>,
    runtime: &C3dResourceScaledObject,
    car: CanonicalCar,
) -> Result<(), ChampionshipCarFramePlanError> {
    if topology.provenance() != runtime.provenance() {
        return Err(ChampionshipCarFramePlanError::ProvenanceMismatch);
    }
    let descriptor = CHAMPIONSHIP_CARS[car.index() as usize];
    if topology.source() != runtime.source()
        || topology.source().owner != FrontendC3dOwner::ChampionshipCar(car)
        || topology.source().path.as_ref() != descriptor.object_path
    {
        return Err(ChampionshipCarFramePlanError::SourceMismatch { car });
    }
    if topology.policy() != runtime.policy() || topology.policy() != expected_policy(car) {
        return Err(ChampionshipCarFramePlanError::PolicyMismatch { car });
    }
    Ok(())
}

fn expected_policy(car: CanonicalCar) -> C3dLoadPolicy {
    championship_mode8_policy(CHAMPIONSHIP_C3D_ENTRY_RESOURCE_SNAPSHOTS[car.index() as usize])
}

fn preflight_actors(
    topology: &C3dObjectTopology<'_>,
    runtime: &C3dResourceScaledObject,
) -> Result<(Vec<PreflightActor>, Vec<usize>), ChampionshipCarFramePlanError> {
    let topology_records = topology.group0_records().collect::<Vec<_>>();
    let runtime_records = runtime.group0_records().collect::<Vec<_>>();
    if topology_records.len() != runtime_records.len() {
        return Err(ChampionshipCarFramePlanError::Group0Count {
            topology: topology_records.len(),
            runtime: runtime_records.len(),
        });
    }
    if topology_records.is_empty() {
        return Err(ChampionshipCarFramePlanError::MissingPreviewRoot);
    }

    let mut actors = Vec::with_capacity(topology_records.len());
    let mut root_children = Vec::new();
    for (record_index, (topology_record, runtime_record)) in topology_records
        .into_iter()
        .zip(runtime_records)
        .enumerate()
    {
        if topology_record.record_index() != record_index
            || runtime_record.record_index() != record_index
            || topology_record.record_offset() != runtime_record.record_offset()
        {
            return Err(ChampionshipCarFramePlanError::Group0Metadata { record_index });
        }
        let parent = match topology_record.record().field_04_i16() {
            -1 => {
                if record_index != 0 {
                    return Err(ChampionshipCarFramePlanError::AdditionalPreviewRoot {
                        record_index,
                    });
                }
                root_children.push(record_index);
                ChampionshipCarActorParent::PreviewRoot
            }
            ..=-2 => {
                return Err(ChampionshipCarFramePlanError::InvalidNegativeParent {
                    record_index,
                    parent: topology_record.record().field_04_i16(),
                });
            }
            parent => {
                let parent = usize::try_from(parent).expect("nonnegative i16 fits usize");
                if parent >= record_index {
                    return Err(ChampionshipCarFramePlanError::NonPriorParent {
                        record_index,
                        parent,
                    });
                }
                ChampionshipCarActorParent::Actor(parent)
            }
        };
        let topology_references = topology_record.references().collect::<Vec<_>>();
        let runtime_references = runtime_record.references().collect::<Vec<_>>();
        if topology_references.len() != runtime_references.len() {
            return Err(ChampionshipCarFramePlanError::ReferenceCount {
                record_index,
                topology: topology_references.len(),
                runtime: runtime_references.len(),
            });
        }
        let mut levels = Vec::with_capacity(topology_references.len());
        let mut commands = Vec::new();
        for (value_index, (topology_reference, runtime_reference)) in topology_references
            .into_iter()
            .zip(runtime_references)
            .enumerate()
        {
            if topology_reference.value_index() != value_index
                || runtime_reference.value_index() != value_index
                || topology_reference.source_offset() != runtime_reference.source_offset()
                || topology_reference.group1_ordinal() != runtime_reference.group1_ordinal()
            {
                return Err(ChampionshipCarFramePlanError::ReferenceMetadata {
                    record_index,
                    value_index,
                });
            }
            let ordinal = topology_reference.group1_ordinal();
            let group = topology.group(usize::from(ordinal)).ok_or(
                ChampionshipCarFramePlanError::MissingGroup1 {
                    record_index,
                    value_index,
                    ordinal,
                },
            )?;
            levels.push(ChampionshipCarActorLevel {
                value_index,
                source_offset: topology_reference.source_offset(),
                group1_ordinal: ordinal,
                flags: u32::from_le_bytes(
                    group.record().bytes()[0x14..0x18]
                        .try_into()
                        .expect("validated group-1 fixed prefix"),
                ),
            });

            // The loader initializes the active mesh level to zero.  Preserve
            // every level above, but submit only level zero for this frame.
            if value_index == 0 {
                append_level_commands(
                    record_index,
                    value_index,
                    group,
                    runtime_reference,
                    topology.policy().vertex_mutation_branch(),
                    &mut commands,
                )?;
            }
        }
        let bytes = topology_record.record().bytes();
        actors.push(PreflightActor {
            parent,
            local_q16: topology_record.record().affine_matrix_q16(),
            source_byte_77: bytes[77],
            source_byte_78: bytes[78],
            levels: levels.into_boxed_slice(),
            commands,
        });
    }
    Ok((actors, root_children))
}

fn append_level_commands(
    record_index: usize,
    value_index: usize,
    topology_group: &crate::c3d_topology::C3dGroup1Topology<'_>,
    runtime_reference: &crate::root_menu_c3d_runtime::C3dResourceScaledReference,
    branch: C3dVertexMutationBranch,
    commands: &mut Vec<C3dDrawCommand>,
) -> Result<(), ChampionshipCarFramePlanError> {
    let topology_batches = topology_group.batches();
    let runtime_batches = runtime_reference.batches().collect::<Vec<_>>();
    if topology_batches.len() != runtime_batches.len() {
        return Err(ChampionshipCarFramePlanError::BatchCount {
            record_index,
            value_index,
            topology: topology_batches.len(),
            runtime: runtime_batches.len(),
        });
    }
    for (batch_index, (topology_batch, runtime_batch)) in
        topology_batches.iter().zip(runtime_batches).enumerate()
    {
        if topology_batch.selector() != runtime_batch.selector()
            || topology_batch.unique_entry32_ordinals() != runtime_batch.source_entry32_ordinals()
            || topology_batch.indices() != runtime_batch.indices()
        {
            return Err(ChampionshipCarFramePlanError::BatchMetadata {
                record_index,
                value_index,
                batch_index,
            });
        }
        let vertex_count = runtime_batch.vertices().len();
        if vertex_count != runtime_batch.source_entry32_ordinals().len()
            || vertex_count != runtime_batch.side_vectors().len()
            || runtime_batch.indices().len() != topology_batch.entries_24().len().saturating_mul(3)
            || runtime_batch
                .indices()
                .iter()
                .any(|index| usize::from(*index) >= vertex_count)
        {
            return Err(ChampionshipCarFramePlanError::BatchPayload {
                record_index,
                value_index,
                batch_index,
            });
        }
        commands.push(C3dDrawCommand::new(
            C3dDrawOrdinal::new(
                record_index,
                value_index,
                runtime_reference.group1_ordinal(),
                batch_index,
            ),
            runtime_batch.selector(),
            branch,
            runtime_batch.source_entry32_ordinals(),
            runtime_batch.vertices(),
            runtime_batch.side_vectors(),
            runtime_batch.indices(),
        ));
    }
    Ok(())
}

fn append_depth_first(actor: usize, children: &[Vec<usize>], output: &mut Vec<usize>) {
    let mut pending = vec![actor];
    while let Some(current) = pending.pop() {
        output.push(current);
        for child in children[current].iter().rev() {
            pending.push(*child);
        }
    }
}

/// Exact recovered row-vector composition: `child_local * parent_world`.
fn compose_q16(local: [i32; 16], parent: [i32; 16]) -> [i32; 16] {
    let mut result = [0; 16];
    for row in 0..4 {
        for column in 0..4 {
            let mut value = 0_i32;
            for inner in 0..4 {
                let product = (i64::from(local[row * 4 + inner])
                    * i64::from(parent[inner * 4 + column]))
                    >> 16;
                value = value.wrapping_add(product as i32);
            }
            result[row * 4 + column] = value;
        }
    }
    result
}

fn q16_to_f32_words(matrix: [i32; 16]) -> [u32; 16] {
    matrix.map(|value| ((f64::from(value) * (1.0_f64 / 65_536.0)) as f32).to_bits())
}

#[cfg(test)]
pub(crate) mod tests {
    use cmr_formats::{
        bitmap::BitmapLimits,
        c3d::{
            C3D_ENTRY24_ENTRY32_ORDINAL_OFFSETS, C3D_FORMAT_VERSION, C3D_GROUP0_VALUES_OFFSET,
            C3D_GROUP1_ENTRY24_LEN, C3D_GROUP1_ENTRY32_LEN, C3D_GROUP1_FIXED_LEN, C3D_HEADER_LEN,
            C3D_MAGIC, C3dLimits, parse_c3d,
        },
    };

    use super::*;
    use crate::{
        c3d_topology::build_c3d_object_topology,
        root_menu_c3d_resources::{
            C3dPreparedArenaBuilder, C3dPreparedSelector, C3dSourceIdentity,
        },
        root_menu_c3d_runtime::materialize_c3d_resource_scaled,
    };

    const Q16_ONE: i32 = 0x1_0000;
    const BASIS: [i32; 9] = [Q16_ONE, 0, 0, 0, Q16_ONE, 0, 0, 0, Q16_ONE];
    const EXTENT: DisplayExtent = DisplayExtent {
        width: 640,
        height: 480,
    };
    const ORIGIN: PixelPoint = PixelPoint { x: 100, y: 100 };
    const INPUT: ChampionshipCarInput = ChampionshipCarInput {
        f9: false,
        f10: false,
    };

    struct Group0 {
        parent: i16,
        matrix: [i32; 16],
        references: Vec<u16>,
    }

    fn translated(x: i32, y: i32, z: i32) -> [i32; 16] {
        let mut matrix = [0; 16];
        matrix[0] = Q16_ONE;
        matrix[5] = Q16_ONE;
        matrix[10] = Q16_ONE;
        matrix[15] = Q16_ONE;
        matrix[12] = x;
        matrix[13] = y;
        matrix[14] = z;
        matrix
    }

    fn fixture_bytes(group0: &[Group0], selectors: &[i16]) -> Vec<u8> {
        let group0_len = group0
            .iter()
            .map(|record| C3D_GROUP0_VALUES_OFFSET + record.references.len() * 2)
            .sum::<usize>();
        let group1_len = selectors.len()
            * (C3D_GROUP1_FIXED_LEN + C3D_GROUP1_ENTRY32_LEN + C3D_GROUP1_ENTRY24_LEN);
        let group1_start = C3D_HEADER_LEN + group0_len;
        let blob_start = group1_start + group1_len;
        let bitmap = indexed_bmp();
        let has_resource = selectors.iter().any(|selector| *selector >= 0);
        let mut bytes = vec![0; blob_start + usize::from(has_resource) * (16 + bitmap.len())];
        bytes[..4].copy_from_slice(&C3D_MAGIC);
        bytes[4..8].copy_from_slice(&C3D_FORMAT_VERSION.to_le_bytes());
        bytes[12..14].copy_from_slice(&(group0.len() as u16).to_le_bytes());
        bytes[14..16].copy_from_slice(&(selectors.len() as u16).to_le_bytes());
        bytes[16..18].copy_from_slice(&u16::from(has_resource).to_le_bytes());
        for (index, offset) in [C3D_HEADER_LEN, group1_start, blob_start]
            .into_iter()
            .enumerate()
        {
            bytes[20 + index * 4..24 + index * 4].copy_from_slice(&(offset as u32).to_le_bytes());
        }
        let mut offset = C3D_HEADER_LEN;
        for record in group0 {
            let len = C3D_GROUP0_VALUES_OFFSET + record.references.len() * 2;
            bytes[offset..offset + 4].copy_from_slice(&(len as u32).to_le_bytes());
            bytes[offset + 4..offset + 6].copy_from_slice(&record.parent.to_le_bytes());
            for (index, word) in record.matrix.iter().enumerate() {
                bytes[offset + 8 + index * 4..offset + 12 + index * 4]
                    .copy_from_slice(&word.to_le_bytes());
            }
            bytes[offset + 76] = record.references.len() as u8;
            bytes[offset + 77] = record.parent as u8;
            bytes[offset + 78] = record.references.len() as u8;
            for (index, reference) in record.references.iter().enumerate() {
                bytes[offset + 80 + index * 2..offset + 82 + index * 2]
                    .copy_from_slice(&reference.to_le_bytes());
            }
            offset += len;
        }
        for selector in selectors {
            let len = C3D_GROUP1_FIXED_LEN + C3D_GROUP1_ENTRY32_LEN + C3D_GROUP1_ENTRY24_LEN;
            bytes[offset + 12..offset + 16].copy_from_slice(&(len as u32).to_le_bytes());
            bytes[offset + 16..offset + 18].copy_from_slice(&1_u16.to_le_bytes());
            bytes[offset + 18..offset + 20].copy_from_slice(&1_u16.to_le_bytes());
            let vertex = offset + C3D_GROUP1_FIXED_LEN;
            bytes[vertex + 28..vertex + 32].copy_from_slice(&Q16_ONE.to_le_bytes());
            let entry = vertex + C3D_GROUP1_ENTRY32_LEN;
            for ordinal_offset in C3D_ENTRY24_ENTRY32_ORDINAL_OFFSETS {
                bytes[entry + ordinal_offset..entry + ordinal_offset + 2]
                    .copy_from_slice(&0_u16.to_le_bytes());
            }
            bytes[entry + 8..entry + 10].copy_from_slice(&selector.to_le_bytes());
            offset += len;
        }
        if has_resource {
            bytes[offset..offset + 12].copy_from_slice(b"PLAN-TEXTURE");
            bytes[offset + 12..offset + 16].copy_from_slice(&(bitmap.len() as u32).to_le_bytes());
            bytes[offset + 16..offset + 16 + bitmap.len()].copy_from_slice(&bitmap);
        }
        bytes
    }

    fn indexed_bmp() -> Vec<u8> {
        let palette_len = 256 * 4;
        let pixel_offset = 14 + 40 + palette_len;
        let file_len = pixel_offset + 4;
        let mut bytes = vec![0; file_len];
        bytes[0..2].copy_from_slice(b"BM");
        bytes[2..6].copy_from_slice(&(file_len as u32).to_le_bytes());
        bytes[10..14].copy_from_slice(&(pixel_offset as u32).to_le_bytes());
        bytes[14..18].copy_from_slice(&40_u32.to_le_bytes());
        bytes[18..22].copy_from_slice(&1_i32.to_le_bytes());
        bytes[22..26].copy_from_slice(&1_i32.to_le_bytes());
        bytes[26..28].copy_from_slice(&1_u16.to_le_bytes());
        bytes[28..30].copy_from_slice(&8_u16.to_le_bytes());
        bytes[34..38].copy_from_slice(&4_u32.to_le_bytes());
        bytes[46..50].copy_from_slice(&256_u32.to_le_bytes());
        bytes[pixel_offset] = 1;
        bytes
    }

    fn inputs(
        group0: &[Group0],
        selectors: &[i16],
    ) -> (C3dObjectTopology<'static>, C3dResourceScaledObject) {
        inputs_with_policy(group0, selectors, expected_policy(CanonicalCar::Subaru))
    }

    fn inputs_with_policy(
        group0: &[Group0],
        selectors: &[i16],
        policy: C3dLoadPolicy,
    ) -> (C3dObjectTopology<'static>, C3dResourceScaledObject) {
        let bytes = fixture_bytes(group0, selectors);
        let file = Box::leak(Box::new(parse_c3d(&bytes, C3dLimits::default()).unwrap()));
        let mut builder = C3dPreparedArenaBuilder::new().unwrap();
        let prepared = builder
            .prepare_object(
                C3dSourceIdentity {
                    owner: FrontendC3dOwner::ChampionshipCar(CanonicalCar::Subaru),
                    path: CHAMPIONSHIP_CARS[0].object_path.into(),
                },
                file,
                policy,
                BitmapLimits::default(),
            )
            .unwrap();
        let arena = builder.finish();
        let topology = build_c3d_object_topology(file, &prepared, arena.provenance()).unwrap();
        let runtime =
            materialize_c3d_resource_scaled(&topology, policy, arena.provenance(), |id| {
                arena.resource(id)
            })
            .unwrap();
        (topology, runtime)
    }

    fn stationary_plan(
        topology: &C3dObjectTopology<'_>,
        runtime: &C3dResourceScaledObject,
    ) -> ChampionshipCarFramePlan {
        let mut state = ChampionshipCarState::new();
        let first = plan_championship_car_frame(
            topology,
            runtime,
            &mut state,
            CanonicalCar::Subaru,
            INPUT,
            BASIS,
            ORIGIN,
            EXTENT,
        )
        .unwrap();
        assert!(!first.selected_parent_visible());
        assert!(first.draws().is_empty());
        assert!(
            first
                .actors()
                .iter()
                .all(|actor| actor.world_q16().is_none() && actor.world_f32_words().is_none())
        );
        let second = plan_championship_car_frame(
            topology,
            runtime,
            &mut state,
            CanonicalCar::Subaru,
            INPUT,
            BASIS,
            ORIGIN,
            EXTENT,
        )
        .unwrap();
        assert!(
            second
                .actors()
                .iter()
                .all(|actor| { actor.world_q16().is_some() && actor.world_f32_words().is_some() })
        );
        second
    }

    pub(crate) fn scene_test_fixture() -> (
        ChampionshipCarFramePlan,
        crate::root_menu_c3d_resources::C3dPreparedArena,
    ) {
        let (mut plans, arena) = scene_test_fixtures(1);
        (plans.pop().unwrap(), arena)
    }

    pub(crate) struct TerminalSceneTestFixture {
        pub topology: C3dObjectTopology<'static>,
        pub runtime: C3dResourceScaledObject,
        pub arena: crate::root_menu_c3d_resources::C3dPreparedArena,
    }

    pub(crate) fn terminal_scene_test_fixture() -> TerminalSceneTestFixture {
        terminal_scene_test_fixture_with_selectors(&[0, -1])
    }

    pub(crate) fn terminal_scene_test_fixture_with_selectors(
        selectors: &[i16],
    ) -> TerminalSceneTestFixture {
        let actors = selectors
            .iter()
            .enumerate()
            .map(|(index, _)| Group0 {
                parent: if index == 0 { -1 } else { 0 },
                matrix: translated(0, 0, 0),
                references: vec![u16::try_from(index).unwrap()],
            })
            .collect::<Vec<_>>();
        let bytes = fixture_bytes(&actors, selectors);
        let file = Box::leak(Box::new(parse_c3d(&bytes, C3dLimits::default()).unwrap()));
        let policy = expected_policy(CanonicalCar::Subaru);
        let mut builder = C3dPreparedArenaBuilder::new().unwrap();
        let prepared = builder
            .prepare_object(
                C3dSourceIdentity {
                    owner: FrontendC3dOwner::ChampionshipCar(CanonicalCar::Subaru),
                    path: CHAMPIONSHIP_CARS[0].object_path.into(),
                },
                file,
                policy,
                BitmapLimits::default(),
            )
            .unwrap();
        let arena = builder.finish();
        let topology = build_c3d_object_topology(file, &prepared, arena.provenance()).unwrap();
        let runtime =
            materialize_c3d_resource_scaled(&topology, policy, arena.provenance(), |id| {
                arena.resource(id)
            })
            .unwrap();
        TerminalSceneTestFixture {
            topology,
            runtime,
            arena,
        }
    }

    pub(crate) fn scene_test_fixtures(
        count: usize,
    ) -> (
        Vec<ChampionshipCarFramePlan>,
        crate::root_menu_c3d_resources::C3dPreparedArena,
    ) {
        scene_test_fixtures_with_selectors(count, &[0, -1])
    }

    pub(crate) fn scene_test_fixtures_with_selectors(
        count: usize,
        selectors: &[i16],
    ) -> (
        Vec<ChampionshipCarFramePlan>,
        crate::root_menu_c3d_resources::C3dPreparedArena,
    ) {
        let actors = (0..selectors.len())
            .map(|index| Group0 {
                parent: if index == 0 { -1 } else { 0 },
                matrix: translated(0, 0, 0),
                references: vec![u16::try_from(index).unwrap()],
            })
            .collect::<Vec<_>>();
        let bytes = fixture_bytes(&actors, selectors);
        let file = Box::leak(Box::new(parse_c3d(&bytes, C3dLimits::default()).unwrap()));
        let policy = expected_policy(CanonicalCar::Subaru);
        let mut builder = C3dPreparedArenaBuilder::new().unwrap();
        let prepared = builder
            .prepare_object(
                C3dSourceIdentity {
                    owner: FrontendC3dOwner::ChampionshipCar(CanonicalCar::Subaru),
                    path: CHAMPIONSHIP_CARS[0].object_path.into(),
                },
                file,
                policy,
                BitmapLimits::default(),
            )
            .unwrap();
        let arena = builder.finish();
        let topology = build_c3d_object_topology(file, &prepared, arena.provenance()).unwrap();
        let runtime =
            materialize_c3d_resource_scaled(&topology, policy, arena.provenance(), |id| {
                arena.resource(id)
            })
            .unwrap();
        let mut state = ChampionshipCarState::new();
        let hidden = plan_championship_car_frame(
            &topology,
            &runtime,
            &mut state,
            CanonicalCar::Subaru,
            INPUT,
            BASIS,
            ORIGIN,
            EXTENT,
        )
        .unwrap();
        assert!(hidden.draws().is_empty());
        let plans = (0..count)
            .map(|_| {
                plan_championship_car_frame(
                    &topology,
                    &runtime,
                    &mut state,
                    CanonicalCar::Subaru,
                    INPUT,
                    BASIS,
                    ORIGIN,
                    EXTENT,
                )
                .unwrap()
            })
            .collect();
        (plans, arena)
    }

    #[test]
    fn three_level_hierarchy_composes_child_local_before_parent_and_draws_depth_first() {
        let mut scaled = translated(5 * Q16_ONE, 6 * Q16_ONE, 7 * Q16_ONE);
        scaled[0] = 2 * Q16_ONE;
        scaled[5] = 3 * Q16_ONE;
        scaled[10] = 4 * Q16_ONE;
        let (topology, runtime) = inputs(
            &[
                Group0 {
                    parent: -1,
                    matrix: translated(Q16_ONE, 0, 0),
                    references: vec![],
                },
                Group0 {
                    parent: 0,
                    matrix: scaled,
                    references: vec![0],
                },
                Group0 {
                    parent: 0,
                    matrix: translated(100 * Q16_ONE, 0, 0),
                    references: vec![1],
                },
                Group0 {
                    parent: 1,
                    matrix: translated(8 * Q16_ONE, 9 * Q16_ONE, 10 * Q16_ONE),
                    references: vec![2],
                },
            ],
            &[-1, -1, -1],
        );
        let plan = stationary_plan(&topology, &runtime);
        assert!(plan.selected_parent_visible());
        assert_eq!(
            plan.draws()
                .iter()
                .map(|draw| draw.actor_index())
                .collect::<Vec<_>>(),
            [1, 3, 2]
        );
        let preview = plan.selected_parent_local_q16();
        let root = compose_q16(translated(Q16_ONE, 0, 0), preview);
        assert_eq!(plan.actors()[0].world_q16(), Some(root));
        assert_eq!(
            plan.actors()[1].world_q16(),
            Some(compose_q16(scaled, root))
        );
        assert_eq!(
            plan.actors()[2].world_q16(),
            Some(compose_q16(translated(100 * Q16_ONE, 0, 0), root))
        );
        let nested = compose_q16(
            translated(8 * Q16_ONE, 9 * Q16_ONE, 10 * Q16_ONE),
            plan.actors()[1].world_q16().unwrap(),
        );
        assert_eq!(plan.actors()[3].world_q16(), Some(nested));
        assert_eq!(
            plan.actors()[3].world_f32_words(),
            Some(q16_to_f32_words(nested))
        );
        assert_eq!(
            plan.draws()[1].actor_world().words(),
            plan.draws()[1].actor_world_f32_words()
        );
    }

    #[test]
    fn q16_composition_matches_independent_hierarchy_and_f32_goldens() {
        let preview = translated(10 * Q16_ONE, 20 * Q16_ONE, 30 * Q16_ONE);
        let root_local = translated(Q16_ONE, 2 * Q16_ONE, 3 * Q16_ONE);
        let mut scaled_local = translated(5 * Q16_ONE, 6 * Q16_ONE, 7 * Q16_ONE);
        scaled_local[0] = 2 * Q16_ONE;
        scaled_local[5] = 3 * Q16_ONE;
        scaled_local[10] = 4 * Q16_ONE;
        let nested_local = translated(8 * Q16_ONE, 9 * Q16_ONE, 10 * Q16_ONE);

        let root = compose_q16(root_local, preview);
        assert_eq!(root, translated(11 * Q16_ONE, 22 * Q16_ONE, 33 * Q16_ONE));
        let scaled = compose_q16(scaled_local, root);
        assert_eq!(
            scaled,
            [
                2 * Q16_ONE,
                0,
                0,
                0,
                0,
                3 * Q16_ONE,
                0,
                0,
                0,
                0,
                4 * Q16_ONE,
                0,
                16 * Q16_ONE,
                28 * Q16_ONE,
                40 * Q16_ONE,
                Q16_ONE,
            ]
        );
        let nested = compose_q16(nested_local, scaled);
        assert_eq!(
            nested,
            [
                2 * Q16_ONE,
                0,
                0,
                0,
                0,
                3 * Q16_ONE,
                0,
                0,
                0,
                0,
                4 * Q16_ONE,
                0,
                32 * Q16_ONE,
                55 * Q16_ONE,
                80 * Q16_ONE,
                Q16_ONE,
            ]
        );
        assert_eq!(
            q16_to_f32_words(nested),
            [
                0x4000_0000,
                0,
                0,
                0,
                0,
                0x4040_0000,
                0,
                0,
                0,
                0,
                0x4080_0000,
                0,
                0x4200_0000,
                0x425c_0000,
                0x42a0_0000,
                0x3f80_0000,
            ]
        );
    }

    #[test]
    fn only_active_level_zero_draws_while_all_levels_remain_in_the_actor() {
        let (topology, runtime) = inputs(
            &[Group0 {
                parent: -1,
                matrix: translated(0, 0, 0),
                references: vec![1, 0],
            }],
            &[-1, -1],
        );
        let plan = stationary_plan(&topology, &runtime);

        assert_eq!(plan.actors()[0].levels().len(), 2);
        assert_eq!(plan.actors()[0].levels()[0].value_index(), 0);
        assert_eq!(plan.actors()[0].levels()[0].group1_ordinal(), 1);
        assert_eq!(plan.actors()[0].levels()[1].value_index(), 1);
        assert_eq!(plan.actors()[0].levels()[1].group1_ordinal(), 0);
        assert_eq!(plan.draws().len(), 1);
        assert_eq!(plan.draws()[0].command().value_index(), 0);
        assert_eq!(plan.draws()[0].command().group1_ordinal(), 1);
    }

    #[test]
    fn q16_composition_wraps_each_shifted_product_and_accumulated_term() {
        let mut local = [0; 16];
        let mut parent = [0; 16];
        local[0..4].fill(i32::MAX);
        parent[0] = 0x1_0001;
        parent[4] = 0x1_0001;
        parent[8] = 0x1_0001;
        parent[12] = 0x1_0001;

        // Each product narrows to 0x8000_7ffe and four wrapping additions
        // produce 0x0001_fff8.
        assert_eq!(compose_q16(local, parent)[0], 0x0001_fff8);

        local = [0; 16];
        parent = [0; 16];
        local[0] = -1;
        parent[0] = 0x8000;
        assert_eq!(compose_q16(local, parent)[0], -1);
    }

    #[test]
    fn no_mesh_parent_contributes_transform_and_missing_selector_is_retained() {
        let (topology, runtime) = inputs(
            &[
                Group0 {
                    parent: -1,
                    matrix: translated(Q16_ONE, 0, 0),
                    references: vec![],
                },
                Group0 {
                    parent: 0,
                    matrix: translated(0, Q16_ONE, 0),
                    references: vec![0],
                },
            ],
            &[-1],
        );
        let plan = stationary_plan(&topology, &runtime);
        assert_eq!(plan.draws().len(), 1);
        assert_eq!(plan.draws()[0].actor_index(), 1);
        assert_eq!(
            plan.draws()[0].command().selector(),
            C3dPreparedSelector::Missing
        );
        assert_eq!(plan.draws()[0].command().vertex_type(), 2);
        assert_eq!(plan.draws()[0].command().primitive_code(), 4);
        assert_eq!(plan.draws()[0].command().draw_flags(), 8);
        assert_eq!(&plan.actors()[0].levels(), &[]);
        assert_eq!(plan.actors()[1].source_byte_77(), 0);
        assert_eq!(plan.actors()[1].source_byte_78(), 1);
    }

    #[test]
    fn invalid_parent_and_invalid_viewport_leave_mutable_state_unchanged() {
        for bad_parent in [-2, 1] {
            let (topology, runtime) = inputs(
                &[Group0 {
                    parent: bad_parent,
                    matrix: translated(0, 0, 0),
                    references: vec![0],
                }],
                &[-1],
            );
            let mut state = ChampionshipCarState::new();
            let before = state;
            assert!(
                plan_championship_car_frame(
                    &topology,
                    &runtime,
                    &mut state,
                    CanonicalCar::Subaru,
                    INPUT,
                    BASIS,
                    ORIGIN,
                    EXTENT,
                )
                .is_err()
            );
            assert_eq!(state, before);
        }

        let (topology, runtime) = inputs(
            &[Group0 {
                parent: -1,
                matrix: translated(0, 0, 0),
                references: vec![0],
            }],
            &[-1],
        );
        let mut state = ChampionshipCarState::new();
        let before = state;
        assert!(matches!(
            plan_championship_car_frame(
                &topology,
                &runtime,
                &mut state,
                CanonicalCar::Subaru,
                INPUT,
                BASIS,
                ORIGIN,
                DisplayExtent {
                    width: 0,
                    height: 1
                },
            ),
            Err(ChampionshipCarFramePlanError::Viewport(_))
        ));
        assert_eq!(state, before);
    }

    #[test]
    fn first_moving_frame_is_invisible_and_stationary_second_frame_emits() {
        let (topology, runtime) = inputs(
            &[Group0 {
                parent: -1,
                matrix: translated(0, 0, 0),
                references: vec![0],
            }],
            &[-1],
        );
        let mut state = ChampionshipCarState::new();
        let first = plan_championship_car_frame(
            &topology,
            &runtime,
            &mut state,
            CanonicalCar::Subaru,
            INPUT,
            BASIS,
            ORIGIN,
            EXTENT,
        )
        .unwrap();
        assert!(!first.selected_parent_visible());
        assert!(first.draws().is_empty());
        let second = plan_championship_car_frame(
            &topology,
            &runtime,
            &mut state,
            CanonicalCar::Subaru,
            INPUT,
            BASIS,
            ORIGIN,
            EXTENT,
        )
        .unwrap();
        assert!(second.selected_parent_visible());
        assert_eq!(second.draws().len(), 1);
        assert!(second.unknown_render_flag_08());
        assert_eq!(second.view(), ROOT_MENU_C3D_VIEW);
        assert_eq!(second.projection(), ROOT_MENU_C3D_PROJECTION);
    }

    #[test]
    fn present_selector_is_retained_and_identity_policy_joins_fail_atomically() {
        let shape = [Group0 {
            parent: -1,
            matrix: translated(0, 0, 0),
            references: vec![0],
        }];
        let (topology, runtime) = inputs(&shape, &[0]);
        let plan = stationary_plan(&topology, &runtime);
        let C3dPreparedSelector::Resource(resource) = plan.draws()[0].command().selector() else {
            panic!("present selector must retain its arena resource")
        };
        assert_eq!(resource.index(), 0);

        let (foreign_topology, _) = inputs(&shape, &[0]);
        let mut state = ChampionshipCarState::new();
        let before = state;
        assert_eq!(
            plan_championship_car_frame(
                &foreign_topology,
                &runtime,
                &mut state,
                CanonicalCar::Subaru,
                INPUT,
                BASIS,
                ORIGIN,
                EXTENT,
            ),
            Err(ChampionshipCarFramePlanError::ProvenanceMismatch)
        );
        assert_eq!(state, before);

        assert_eq!(
            plan_championship_car_frame(
                &topology,
                &runtime,
                &mut state,
                CanonicalCar::Ibiza,
                INPUT,
                BASIS,
                ORIGIN,
                EXTENT,
            ),
            Err(ChampionshipCarFramePlanError::SourceMismatch {
                car: CanonicalCar::Ibiza,
            })
        );
        assert_eq!(state, before);

        let mut wrong_policy = expected_policy(CanonicalCar::Subaru);
        wrong_policy.entry_resource_count = 1;
        let (wrong_topology, wrong_runtime) = inputs_with_policy(&shape, &[0], wrong_policy);
        assert_eq!(
            plan_championship_car_frame(
                &wrong_topology,
                &wrong_runtime,
                &mut state,
                CanonicalCar::Subaru,
                INPUT,
                BASIS,
                ORIGIN,
                EXTENT,
            ),
            Err(ChampionshipCarFramePlanError::PolicyMismatch {
                car: CanonicalCar::Subaru,
            })
        );
        assert_eq!(state, before);
    }
}
