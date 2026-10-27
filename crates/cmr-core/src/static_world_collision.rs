//! Static-scenery collision orchestration from the official-EU mechanics path.
//!
//! `FUN_0043afd0` runs `FUN_0043d5d0` before per-car mechanics. That wrapper
//! queries four cells from a 4,096-cell index, tests the current eight-point car
//! hull, accumulates linear/angular impulses, updates a local contact/lever arm,
//! and rotates a five-ID contact history. The static OBB builders, both narrow
//! paths and [`crate::static_collision_response`] arithmetic are recovered;
//! callers still supply the adapter kernel that binds them to car/damage state.
//! Previous X/Z is depenetration input after current overlap; this is
//! deliberately not a swept/TOI collision API.

use std::fmt;

use crate::c3d_terrain_cull::MAX_CELLS;
use crate::fixed_point::{Fixed16, Vec3};

pub const STATIC_CONTACT_CAPACITY: usize = 5;
/// Literal identity passed by the static-scenery wrapper to its response path.
pub const STATIC_SCENERY_CONTACT_ID: i32 = 0;

/// Emit the eight world-space OBB points of `FUN_0043ce70` in native order.
/// Every scale/add/subtract uses the existing wrapping Q16 operations.
#[must_use]
pub fn build_static_obb_points(center: Vec3, basis: [Vec3; 3], half_extents: Vec3) -> [Vec3; 8] {
    let a = basis[0].scale(half_extents.x);
    let b = basis[1].scale(half_extents.y);
    let c = basis[2].scale(half_extents.z);
    [
        center.add(a).sub(b).add(c),
        center.sub(a).sub(b).add(c),
        center.sub(a).sub(b).sub(c),
        center.add(a).sub(b).sub(c),
        center.add(a).add(b).add(c),
        center.add(a).add(b).sub(c),
        center.sub(a).add(b).add(c),
        center.sub(a).add(b).sub(c),
    ]
}

/// Horizontal projection and quantized vertical extents produced by
/// `FUN_0043d120` before `FUN_0043d4a0` emits the four XZ corners.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticObbProjection {
    pub max_y_shift9: i16,
    pub min_y_shift9: i16,
    pub axis_radius: Fixed16,
    pub perpendicular_radius: Fixed16,
    pub axis: Vec3,
    pub perpendicular: Vec3,
}

/// Derive the horizontal axes/radii and signed `>>9` Y extrema of
/// `FUN_0043d120` from eight world-space OBB points.
#[must_use]
pub fn project_static_obb(
    basis: [Vec3; 3],
    points: [Vec3; 8],
    center: Vec3,
) -> StaticObbProjection {
    use crate::compiled_car_initializer::normalize_compiled_lane_vector;

    let selected = if basis[0].y.raw().wrapping_abs() > 0xfd70 {
        basis[1]
    } else {
        basis[0]
    };
    let raw_axis = normalize_compiled_lane_vector([selected.x.raw(), 0, selected.z.raw()]);
    let axis = vec3_from_raw(raw_axis);
    let perpendicular = Vec3::new(axis.z, Fixed16::ZERO, Fixed16::ZERO.sub(axis.x));
    let mut projection = StaticObbProjection {
        max_y_shift9: 0,
        min_y_shift9: 0,
        axis_radius: Fixed16::ZERO,
        perpendicular_radius: Fixed16::ZERO,
        axis,
        perpendicular,
    };
    for point in points {
        let relative = point.sub(center);
        let along_axis = relative.dot(axis);
        let along_perpendicular = relative.dot(perpendicular);
        if along_axis.raw() > 0 && along_axis > projection.axis_radius {
            projection.axis_radius = along_axis;
        }
        if along_perpendicular.raw() > 0 && along_perpendicular > projection.perpendicular_radius {
            projection.perpendicular_radius = along_perpendicular;
        }
        let y = relative.y.raw();
        if y > i32::from(projection.max_y_shift9).wrapping_shl(9) {
            projection.max_y_shift9 = (y >> 9) as i16;
        }
        if y < i32::from(projection.min_y_shift9).wrapping_shl(9) {
            projection.min_y_shift9 = (y >> 9) as i16;
        }
    }
    projection
}

/// Emit the four signed `i16` XZ corner pairs of `FUN_0043d4a0`.
#[must_use]
pub fn static_obb_projected_corners(projection: StaticObbProjection) -> [[i16; 2]; 4] {
    let u = projection.axis.scale(projection.axis_radius);
    let v = projection
        .perpendicular
        .scale(projection.perpendicular_radius);
    let pair = |x: Fixed16, z: Fixed16| [(x.raw() >> 9) as i16, (z.raw() >> 9) as i16];
    [
        pair(u.x.sub(v.x), u.z.sub(v.z)),
        pair(u.x.add(v.x), u.z.add(v.z)),
        pair(
            Fixed16::ZERO.sub(u.x).sub(v.x),
            Fixed16::ZERO.sub(u.z).sub(v.z),
        ),
        pair(
            Fixed16::ZERO.sub(u.x).add(v.x),
            Fixed16::ZERO.sub(u.z).add(v.z),
        ),
    ]
}

const fn vec3_from_raw(raw: [i32; 3]) -> Vec3 {
    Vec3::new(
        Fixed16::from_raw(raw[0]),
        Fixed16::from_raw(raw[1]),
        Fixed16::from_raw(raw[2]),
    )
}

/// Recovered inputs to the OBJ collision-record OBB builder in `FUN_00481990`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticObbBuildInput {
    pub collision_translation: Vec3,
    /// Normalized rows from the root node's local matrix. Parent transforms can
    /// affect `collision_translation` but do not replace these local rows.
    pub normalized_root_local_basis: [Vec3; 3],
    pub root_scale: Vec3,
    /// First selected group-1 bound words copied to mesh `+0x90/+0x94/+0x98`.
    pub mesh_bound: Vec3,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticObbGeometry {
    pub center: Vec3,
    pub basis: [Vec3; 3],
    pub scaled_extents: Vec3,
    pub half_extents: Vec3,
    /// Separate `qmul(S, 0x8000)` vector used by the bounds scalar path. It is
    /// deliberately not replaced by truncation-toward-zero `half_extents`.
    pub bounds_half_scalars: Vec3,
    pub points: [Vec3; 8],
    pub projection: StaticObbProjection,
    pub projected_corners: [[i16; 2]; 4],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticObbBuildError {
    pub zero_scaled_axis: usize,
}

/// Build the recovered OBJ OBB geometry. Any zero post-Q16 scale rejects the
/// candidate before record publication, matching the native gate.
pub fn build_static_obb_geometry(
    input: StaticObbBuildInput,
) -> Result<StaticObbGeometry, StaticObbBuildError> {
    let scaled_extents = Vec3::new(
        input.root_scale.x.mul(input.mesh_bound.x),
        input.root_scale.y.mul(input.mesh_bound.y),
        input.root_scale.z.mul(input.mesh_bound.z),
    );
    let scaled_raw = [
        scaled_extents.x.raw(),
        scaled_extents.y.raw(),
        scaled_extents.z.raw(),
    ];
    if let Some(zero_scaled_axis) = scaled_raw.iter().position(|value| *value == 0) {
        return Err(StaticObbBuildError { zero_scaled_axis });
    }
    let half_raw = scaled_raw.map(|value| value.wrapping_sub(value >> 31) >> 1);
    let half_extents = vec3_from_raw(half_raw);
    let half_q16 = Fixed16::from_raw(0x8000);
    let bounds_half_scalars = scaled_extents.scale(half_q16);
    let center = input
        .collision_translation
        .add(input.normalized_root_local_basis[1].scale(half_extents.y));
    let points = build_static_obb_points(center, input.normalized_root_local_basis, half_extents);
    let projection = project_static_obb(input.normalized_root_local_basis, points, center);
    let projected_corners = static_obb_projected_corners(projection);
    Ok(StaticObbGeometry {
        center,
        basis: input.normalized_root_local_basis,
        scaled_extents,
        half_extents,
        bounds_half_scalars,
        points,
        projection,
        projected_corners,
    })
}

/// Projection/bounds fields consumed by the two static narrow-phase paths.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticNarrowObb {
    pub center: Vec3,
    pub projected_corners: [[i16; 2]; 4],
    pub radius: Fixed16,
    pub max_y_shift9: i16,
    pub min_y_shift9: i16,
    pub axis_radius: Fixed16,
    pub perpendicular_radius: Fixed16,
    pub axis: Vec3,
    pub perpendicular: Vec3,
}

impl From<StaticObbGeometry> for StaticNarrowObb {
    fn from(geometry: StaticObbGeometry) -> Self {
        Self {
            center: geometry.center,
            projected_corners: geometry.projected_corners,
            radius: native_length(geometry.bounds_half_scalars),
            max_y_shift9: geometry.projection.max_y_shift9,
            min_y_shift9: geometry.projection.min_y_shift9,
            axis_radius: geometry.projection.axis_radius,
            perpendicular_radius: geometry.projection.perpendicular_radius,
            axis: geometry.projection.axis,
            perpendicular: geometry.projection.perpendicular,
        }
    }
}

/// Value snapshot of the car fields and global projection cache read by
/// `FUN_0043e640` and `FUN_0043e140`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticCarNarrowInput {
    pub position: Vec3,
    pub previous_center_xz: [Fixed16; 2],
    pub hull_points: [Vec3; 8],
    pub projected_corners: [Vec3; 4],
    pub projection_axis: Vec3,
    pub projection_perpendicular: Vec3,
    pub projection_axis_radius: Fixed16,
    pub projection_perpendicular_radius: Fixed16,
    /// Exact `DAT_00552f60` endpoint used by the first Y comparison.
    pub y_gate_552f60: Fixed16,
    /// Exact `DAT_00552f80` endpoint used by the second Y comparison.
    pub y_gate_552f80: Fixed16,
    pub velocity_d4: Vec3,
    /// Axis at `car+0x324..+0x32c` used by the alternate path.
    pub alternate_axis_324: Vec3,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ContainedCornerIndices {
    pub indices: [u8; 4],
    pub len: u8,
}

impl ContainedCornerIndices {
    fn push(&mut self, index: usize) {
        self.indices[usize::from(self.len)] = index as u8;
        self.len += 1;
    }

    fn iter(self) -> impl Iterator<Item = usize> {
        self.indices
            .into_iter()
            .take(usize::from(self.len))
            .map(usize::from)
    }
}

/// Pure result of the normal static narrow phase at `FUN_0043e640`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticNarrowResult {
    pub hit: bool,
    pub contact_normal: Option<Vec3>,
    pub pass1_inside_object_corners: ContainedCornerIndices,
    pub pass2_inside_car_corners: ContainedCornerIndices,
    pub pass1_correction: Vec3,
    pub pass2_correction: Vec3,
    pub final_position: Vec3,
    pub final_hull_points: [Vec3; 8],
    pub final_projected_corners: [Vec3; 4],
    /// World-space contact offset before `FUN_0048e100` rotates it into
    /// `car+0x3f0..+0x3f8`.
    pub contact_offset_world: Option<Vec3>,
}

/// Reproduce the current-pose corner containment and previous-XZ
/// depenetration of `FUN_0043e640`.
#[must_use]
pub fn resolve_static_obb_narrow_phase(
    car: StaticCarNarrowInput,
    object: StaticNarrowObb,
) -> StaticNarrowResult {
    let mut result = StaticNarrowResult {
        hit: false,
        contact_normal: None,
        pass1_inside_object_corners: ContainedCornerIndices::default(),
        pass2_inside_car_corners: ContainedCornerIndices::default(),
        pass1_correction: Vec3::ZERO,
        pass2_correction: Vec3::ZERO,
        final_position: car.position,
        final_hull_points: car.hull_points,
        final_projected_corners: car.projected_corners,
        contact_offset_world: None,
    };
    if !vertical_gate(car, object) {
        return result;
    }

    let direction = native_normalize(Vec3::new(
        object.center.x.sub(car.previous_center_xz[0]),
        Fixed16::ZERO,
        object.center.z.sub(car.previous_center_xz[1]),
    ));
    let car_direction = [
        car.projection_axis.dot(direction),
        car.projection_perpendicular.dot(direction),
    ];
    let mut pass1_max = Fixed16::ZERO;
    for index in 0..4 {
        let point = object_corner(object, index);
        let relative = point.sub(result.final_position);
        let projected = [
            car.projection_axis.dot(relative),
            car.projection_perpendicular.dot(relative),
        ];
        if inside_projected(
            projected,
            [
                car.projection_axis_radius,
                car.projection_perpendicular_radius,
            ],
        ) {
            result.pass1_inside_object_corners.push(index);
            update_pass_max(
                &mut pass1_max,
                projected,
                [
                    car.projection_axis_radius,
                    car.projection_perpendicular_radius,
                ],
                car_direction,
            );
        }
    }
    if result.pass1_inside_object_corners.len != 0 {
        result.hit = true;
        result.contact_normal = Some(direction);
        if pass1_max.raw() > 0 {
            let correction = direction.scale(pass1_max.neg());
            result.pass1_correction = correction;
            translate_car_cache(&mut result, correction);
        }
    }

    let negative_direction = direction.scale(Fixed16::from_raw(-0x10000));
    let object_direction = [
        object.axis.dot(negative_direction),
        object.perpendicular.dot(negative_direction),
    ];
    let mut pass2_max = Fixed16::ZERO;
    for index in 0..4 {
        let relative = result.final_projected_corners[index].sub(object.center);
        let projected = [
            object.axis.dot(relative),
            object.perpendicular.dot(relative),
        ];
        if inside_projected(projected, [object.axis_radius, object.perpendicular_radius]) {
            result.pass2_inside_car_corners.push(index);
            update_pass_max(
                &mut pass2_max,
                projected,
                [object.axis_radius, object.perpendicular_radius],
                object_direction,
            );
        }
    }
    if result.pass2_inside_car_corners.len != 0 {
        result.hit = true;
        result.contact_normal = Some(direction);
        if pass2_max.raw() > 0 {
            // Native uses the original direction times negative distance here,
            // not `negative_direction * distance`.
            let correction = direction.scale(pass2_max.neg());
            result.pass2_correction = correction;
            translate_car_cache(&mut result, correction);
        }
    }

    if result.hit {
        result.contact_offset_world = Some(if result.pass2_inside_car_corners.len != 0 {
            average_vectors(result.pass2_inside_car_corners, |index| {
                let relative = result.final_hull_points[index].sub(result.final_position);
                Vec3::new(relative.x, Fixed16::ZERO, relative.z)
            })
        } else {
            average_vectors(result.pass1_inside_object_corners, |index| {
                object_corner(object, index).sub(result.final_position)
            })
        });
    }
    result
}

/// Pure side effects of the alternate static path `FUN_0043e140` after
/// reciprocal corner admission succeeds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AlternateStaticNarrowResult {
    pub velocity_d4: Vec3,
    pub radial_weights_304: [Fixed16; 8],
    pub point_flags_69c: [i32; 8],
}

/// Reproduce the alternate overlap, velocity damping, and radial point weights.
/// `None` means the Y/reciprocal-corner admission rejected the object and the
/// native function touched none of these outputs.
#[must_use]
pub fn apply_alternate_static_narrow_phase(
    car: StaticCarNarrowInput,
    object: StaticNarrowObb,
) -> Option<AlternateStaticNarrowResult> {
    if !vertical_gate(car, object) {
        return None;
    }
    let car_inside_object = car.projected_corners.iter().copied().any(|point| {
        point_inside_obb(
            point.sub(object.center),
            object.axis,
            object.perpendicular,
            object.axis_radius,
            object.perpendicular_radius,
        )
    });
    let reciprocal_overlap = car_inside_object
        || (0..4).any(|index| {
            point_inside_obb(
                object_corner(object, index).sub(car.position),
                car.projection_axis,
                car.projection_perpendicular,
                car.projection_axis_radius,
                car.projection_perpendicular_radius,
            )
        });
    if !reciprocal_overlap {
        return None;
    }

    let parallel_amount = car.velocity_d4.dot(car.alternate_axis_324);
    let residual = car
        .velocity_d4
        .sub(car.alternate_axis_324.scale(parallel_amount));
    let velocity_d4 = car
        .velocity_d4
        .add(residual.scale(Fixed16::from_raw(-0x51e)));
    let inv_radius = if object.radius.raw() == 0 {
        Fixed16::from_raw(-1)
    } else {
        native_qdiv(Fixed16::ONE, object.radius)
    };
    let full_scale = native_qdiv(Fixed16::ONE, Fixed16::from_raw(0xc000));
    let radial_weights_304 = car.hull_points.map(|point| {
        let distance = native_length(point.sub(object.center));
        if distance > object.radius {
            return Fixed16::ZERO;
        }
        let alpha = Fixed16::ONE
            .sub(distance.mul(inv_radius))
            .raw()
            .clamp(0, 0xc000);
        Fixed16::from_raw(alpha).mul(full_scale)
    });
    Some(AlternateStaticNarrowResult {
        velocity_d4,
        radial_weights_304,
        point_flags_69c: [1; 8],
    })
}

fn vertical_gate(car: StaticCarNarrowInput, object: StaticNarrowObb) -> bool {
    car.y_gate_552f60.sub(object.center.y).raw() >= i32::from(object.min_y_shift9).wrapping_shl(9)
        && car.y_gate_552f80.sub(object.center.y).raw()
            <= i32::from(object.max_y_shift9).wrapping_shl(9)
}

fn object_corner(object: StaticNarrowObb, index: usize) -> Vec3 {
    Vec3::new(
        object.center.x.add(Fixed16::from_raw(
            i32::from(object.projected_corners[index][0]).wrapping_shl(9),
        )),
        object.center.y,
        object.center.z.add(Fixed16::from_raw(
            i32::from(object.projected_corners[index][1]).wrapping_shl(9),
        )),
    )
}

fn point_inside_obb(
    relative: Vec3,
    axis: Vec3,
    perpendicular: Vec3,
    axis_radius: Fixed16,
    perpendicular_radius: Fixed16,
) -> bool {
    inside_projected(
        [axis.dot(relative), perpendicular.dot(relative)],
        [axis_radius, perpendicular_radius],
    )
}

fn inside_projected(projected: [Fixed16; 2], extents: [Fixed16; 2]) -> bool {
    native_abs(projected[0]).raw() <= extents[0].raw()
        && native_abs(projected[1]).raw() <= extents[1].raw()
}

fn native_abs(value: Fixed16) -> Fixed16 {
    if value.raw() < 0 { value.neg() } else { value }
}

fn exit_distance(projected: Fixed16, extent: Fixed16, direction: Fixed16) -> Option<Fixed16> {
    if direction.raw() > 0x41 {
        Some(native_qdiv(extent.sub(projected), direction))
    } else if direction.raw() < -0x41 {
        Some(native_qdiv(
            native_abs(extent.add(projected)),
            direction.neg(),
        ))
    } else {
        None
    }
}

fn update_pass_max(
    pass_max: &mut Fixed16,
    projected: [Fixed16; 2],
    extents: [Fixed16; 2],
    direction: [Fixed16; 2],
) {
    let t0 = exit_distance(projected[0], extents[0], direction[0]);
    let t1 = exit_distance(projected[1], extents[1], direction[1]);
    let mut best = Fixed16::from_raw(0x7d000000);
    if let Some(value) = t0
        && value < best
    {
        best = value;
    }
    if let Some(value) = t1
        && value < best
    {
        best = value;
    }
    if (t0.is_some() || t1.is_some()) && best > *pass_max {
        *pass_max = best;
    }
}

fn translate_car_cache(result: &mut StaticNarrowResult, correction: Vec3) {
    result.final_position = result.final_position.add(correction);
    for point in &mut result.final_hull_points {
        *point = point.add(correction);
    }
    for point in &mut result.final_projected_corners {
        *point = point.add(correction);
    }
}

fn average_vectors(indices: ContainedCornerIndices, mut vector: impl FnMut(usize) -> Vec3) -> Vec3 {
    let mut sum = Vec3::ZERO;
    for index in indices.iter() {
        sum = sum.add(vector(index));
    }
    let reciprocal = native_qdiv(
        Fixed16::ONE,
        Fixed16::from_raw(i32::from(indices.len).wrapping_shl(16)),
    );
    sum.scale(reciprocal)
}

fn native_qdiv(numerator: Fixed16, denominator: Fixed16) -> Fixed16 {
    let shifted = i64::from(numerator.raw()) << 16;
    Fixed16::from_raw((shifted / i64::from(denominator.raw())) as i32)
}

fn native_length(vector: Vec3) -> Fixed16 {
    let square_sum = vector.dot(vector).raw();
    Fixed16::from_raw(crate::legacy_math::sqrt_q16(square_sum as u32) as i32)
}

fn native_normalize(vector: Vec3) -> Vec3 {
    vec3_from_raw(
        crate::compiled_car_initializer::normalize_compiled_lane_vector([
            vector.x.raw(),
            vector.y.raw(),
            vector.z.raw(),
        ]),
    )
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct StaticCollisionNodeId(u16);

impl StaticCollisionNodeId {
    pub fn new(value: i32) -> Result<Self, StaticCollisionNodeIdError> {
        if value < 0 || value as usize >= MAX_CELLS {
            Err(StaticCollisionNodeIdError { value })
        } else {
            Ok(Self(value as u16))
        }
    }

    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticCollisionNodeIdError {
    pub value: i32,
}

impl fmt::Display for StaticCollisionNodeIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "collision cell {} is outside 0..{MAX_CELLS}", self.value)
    }
}

impl std::error::Error for StaticCollisionNodeIdError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StaticCollisionRecord<G> {
    /// Sequential collision-record ordinal (`record+0x58`). This is not the
    /// static contact-history identity.
    pub collision_ordinal: u16,
    pub raw_material_flags: u8,
    /// Recovered collision geometry, never renderer-derived bounds.
    pub geometry: G,
}

impl<G> StaticCollisionRecord<G> {
    #[must_use]
    pub const fn raw_material_id(&self) -> u8 {
        self.raw_material_flags & 0x7f
    }

    #[must_use]
    pub const fn alternate_collision_path(&self) -> bool {
        self.raw_material_flags & 0x80 != 0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StaticCollisionScene<G> {
    records: Vec<StaticCollisionRecord<G>>,
    nodes: Box<[Vec<usize>; MAX_CELLS]>,
}

impl<G> StaticCollisionScene<G> {
    /// Memberships arrive in builder traversal order. Each is prepended to its
    /// cell, reproducing the native linked-list reversal. Invalid references
    /// fail before a scene is published.
    pub fn new(
        records: Vec<StaticCollisionRecord<G>>,
        memberships: impl IntoIterator<Item = (StaticCollisionNodeId, usize)>,
    ) -> Result<Self, StaticCollisionSceneError> {
        let mut nodes = Box::new(std::array::from_fn(|_| Vec::new()));
        for (node, record_index) in memberships {
            if record_index >= records.len() {
                return Err(StaticCollisionSceneError::RecordIndex {
                    node,
                    record_index,
                    record_count: records.len(),
                });
            }
            nodes[node.index()].insert(0, record_index);
        }
        Ok(Self { records, nodes })
    }

    #[must_use]
    pub fn records(&self) -> &[StaticCollisionRecord<G>] {
        &self.records
    }

    #[must_use]
    pub fn node_records(&self, node: StaticCollisionNodeId) -> &[usize] {
        &self.nodes[node.index()]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StaticCollisionSceneError {
    RecordIndex {
        node: StaticCollisionNodeId,
        record_index: usize,
        record_count: usize,
    },
}

impl fmt::Display for StaticCollisionSceneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::RecordIndex {
                node,
                record_index,
                record_count,
            } => write!(
                f,
                "collision cell {} references record {record_index}, arena length is {record_count}",
                node.index()
            ),
        }
    }
}

impl std::error::Error for StaticCollisionSceneError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticCarCollisionFrame {
    pub center: Vec3,
    /// Response/depenetration input only, not swept admission.
    pub previous_center_xz: [Fixed16; 2],
    pub world_hull_points: [Vec3; 8],
    pub active_nodes: [Option<StaticCollisionNodeId>; 4],
    pub linear_impulse_3cc: Vec3,
    pub angular_impulse_3d8: Vec3,
    /// Local contact/lever arm at `car+0x3f0..+0x3f8`, distinct from normals.
    pub local_contact_arm_3f0: Vec3,
    pub angular_limit_2c: Vec3,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticContactHistory {
    pub previous_ids: [i16; STATIC_CONTACT_CAPACITY],
    pub previous_len: u8,
    pub current_ids: [i16; STATIC_CONTACT_CAPACITY],
    pub current_len: u8,
    /// Reconstruction diagnostic only; the shipped path silently stops storing
    /// IDs at five and has no corresponding field.
    pub current_overflow: bool,
}

impl Default for StaticContactHistory {
    fn default() -> Self {
        Self {
            previous_ids: [0; STATIC_CONTACT_CAPACITY],
            previous_len: 0,
            current_ids: [0; STATIC_CONTACT_CAPACITY],
            current_len: 0,
            current_overflow: false,
        }
    }
}

impl StaticContactHistory {
    /// Rotate the completed current-contact list into the native previous list
    /// and clear storage for one new collision tick.
    #[must_use]
    pub fn begin_tick(self) -> Self {
        Self {
            previous_ids: self.current_ids,
            previous_len: self.current_len.min(STATIC_CONTACT_CAPACITY as u8),
            current_ids: [0; STATIC_CONTACT_CAPACITY],
            current_len: 0,
            current_overflow: false,
        }
    }

    #[must_use]
    pub fn contained_last_tick(&self, id: i32) -> bool {
        self.previous_ids[..usize::from(self.previous_len.min(STATIC_CONTACT_CAPACITY as u8))]
            .iter()
            .any(|stored| i32::from(*stored) == id)
    }

    pub fn push(&mut self, id: i32) -> bool {
        let index = usize::from(self.current_len);
        if index < STATIC_CONTACT_CAPACITY {
            self.current_ids[index] = id as i16;
            self.current_len += 1;
            true
        } else {
            self.current_overflow = true;
            false
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticCollisionContribution {
    pub contact_normal: Vec3,
    pub linear_impulse_3cc: Vec3,
    pub angular_impulse_3d8: Vec3,
    pub local_contact_arm_3f0: Vec3,
    /// The one native signed response-history owner after this contact.
    pub history: StaticContactHistory,
}

/// Sequential state visible to each native response invocation. The response
/// reads and rewrites these values per contact, including component clamps.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticCollisionRunningState {
    pub linear_impulse_3cc: Vec3,
    pub angular_impulse_3d8: Vec3,
    pub local_contact_arm_3f0: Vec3,
    pub history: StaticContactHistory,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StaticResolvedContact {
    /// `FUN_0043e140` overlap. It does not call the normal response and does not
    /// append the literal-zero response-history identity.
    Alternate {
        collision_ordinal: u16,
        raw_material_id: u8,
    },
    /// `FUN_0043e640` overlap followed by `FUN_0043d760`.
    Normal {
        collision_ordinal: u16,
        contact_history_id: i32,
        raw_material_id: u8,
        contact_normal: Vec3,
        repeated_from_previous_tick: bool,
        stored_in_current_history: bool,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StaticCollisionEffects {
    pub linear_impulse_3cc: Vec3,
    pub angular_impulse_3d8: Vec3,
    pub local_contact_arm_3f0: Vec3,
    pub history: StaticContactHistory,
    pub contacts: Vec<StaticResolvedContact>,
}

pub trait StaticCollisionKernel<G> {
    type Error;

    fn resolve(
        &mut self,
        frame: &StaticCarCollisionFrame,
        record: &StaticCollisionRecord<G>,
        running: StaticCollisionRunningState,
    ) -> Result<StaticCollisionResolution, Self::Error>;
}

/// Explicit path metadata returned by a static collision kernel. In
/// particular, an alternate overlap is not conflated with a normal response
/// carrying a zero vector (zero is a legitimate normal result).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StaticCollisionResolution {
    Miss,
    Alternate,
    Normal(StaticCollisionContribution),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StaticCollisionExecutionError<E> {
    pub node: StaticCollisionNodeId,
    pub record_index: usize,
    pub source: E,
}

/// Resolve ordered static-object candidates. Kernel errors return no aggregate,
/// an explicit safe reconstruction policy.
pub fn resolve_static_world_collisions<G, K: StaticCollisionKernel<G>>(
    scene: &StaticCollisionScene<G>,
    frame: &StaticCarCollisionFrame,
    prior_history: StaticContactHistory,
    kernel: &mut K,
) -> Result<StaticCollisionEffects, StaticCollisionExecutionError<K::Error>> {
    let mut effects = StaticCollisionEffects {
        linear_impulse_3cc: frame.linear_impulse_3cc,
        angular_impulse_3d8: frame.angular_impulse_3d8,
        local_contact_arm_3f0: frame.local_contact_arm_3f0,
        history: prior_history.begin_tick(),
        contacts: Vec::new(),
    };
    for node in frame.active_nodes.into_iter().flatten() {
        for &record_index in scene.node_records(node) {
            let record = &scene.records[record_index];
            let resolution = kernel
                .resolve(
                    frame,
                    record,
                    StaticCollisionRunningState {
                        linear_impulse_3cc: effects.linear_impulse_3cc,
                        angular_impulse_3d8: effects.angular_impulse_3d8,
                        local_contact_arm_3f0: effects.local_contact_arm_3f0,
                        history: effects.history,
                    },
                )
                .map_err(|source| StaticCollisionExecutionError {
                    node,
                    record_index,
                    source,
                })?;
            match resolution {
                StaticCollisionResolution::Miss => {}
                StaticCollisionResolution::Alternate => {
                    effects.contacts.push(StaticResolvedContact::Alternate {
                        collision_ordinal: record.collision_ordinal,
                        raw_material_id: record.raw_material_id(),
                    });
                }
                StaticCollisionResolution::Normal(contribution) => {
                    let repeated = effects
                        .history
                        .contained_last_tick(STATIC_SCENERY_CONTACT_ID);
                    let stored = contribution.history.current_len > effects.history.current_len;
                    effects.linear_impulse_3cc = contribution.linear_impulse_3cc;
                    effects.angular_impulse_3d8 = contribution.angular_impulse_3d8;
                    effects.local_contact_arm_3f0 = contribution.local_contact_arm_3f0;
                    effects.history = contribution.history;
                    effects.contacts.push(StaticResolvedContact::Normal {
                        collision_ordinal: record.collision_ordinal,
                        contact_history_id: STATIC_SCENERY_CONTACT_ID,
                        raw_material_id: record.raw_material_id(),
                        contact_normal: contribution.contact_normal,
                        repeated_from_previous_tick: repeated,
                        stored_in_current_history: stored,
                    });
                }
            }
        }
    }
    Ok(effects)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(value: i32) -> StaticCollisionNodeId {
        StaticCollisionNodeId::new(value).unwrap()
    }
    fn vec3(x: i32, y: i32, z: i32) -> Vec3 {
        Vec3::new(
            Fixed16::from_raw(x),
            Fixed16::from_raw(y),
            Fixed16::from_raw(z),
        )
    }
    fn record(id: u16, material: u8, geometry: u8) -> StaticCollisionRecord<u8> {
        StaticCollisionRecord {
            collision_ordinal: id,
            raw_material_flags: material,
            geometry,
        }
    }
    fn frame(nodes: [Option<StaticCollisionNodeId>; 4]) -> StaticCarCollisionFrame {
        StaticCarCollisionFrame {
            center: Vec3::ZERO,
            previous_center_xz: [Fixed16::ZERO; 2],
            world_hull_points: [Vec3::ZERO; 8],
            active_nodes: nodes,
            linear_impulse_3cc: Vec3::ZERO,
            angular_impulse_3d8: Vec3::ZERO,
            local_contact_arm_3f0: vec3(7, 8, 9),
            angular_limit_2c: vec3(10, 11, 12),
        }
    }

    fn projected_box(x: i32, z: i32) -> [Vec3; 4] {
        [
            vec3(-x, 0, -z),
            vec3(x, 0, -z),
            vec3(x, 0, z),
            vec3(-x, 0, z),
        ]
    }

    fn narrow_car(x: i32, z: i32) -> StaticCarNarrowInput {
        let projected_corners = projected_box(x, z);
        let mut hull_points = [Vec3::ZERO; 8];
        hull_points[..4].copy_from_slice(&projected_corners);
        hull_points[4..].copy_from_slice(&projected_corners);
        StaticCarNarrowInput {
            position: Vec3::ZERO,
            previous_center_xz: [Fixed16::from_raw(-(10 << 16)), Fixed16::ZERO],
            hull_points,
            projected_corners,
            projection_axis: vec3(0x10000, 0, 0),
            projection_perpendicular: vec3(0, 0, 0x10000),
            projection_axis_radius: Fixed16::from_raw(x),
            projection_perpendicular_radius: Fixed16::from_raw(z),
            y_gate_552f60: Fixed16::ZERO,
            y_gate_552f80: Fixed16::ZERO,
            velocity_d4: Vec3::ZERO,
            alternate_axis_324: vec3(0x10000, 0, 0),
        }
    }

    fn narrow_object(x: i16, z: i16) -> StaticNarrowObb {
        StaticNarrowObb {
            center: Vec3::ZERO,
            projected_corners: [[-x, -z], [x, -z], [x, z], [-x, z]],
            radius: Fixed16::from_raw(0x10000),
            max_y_shift9: 128,
            min_y_shift9: -128,
            axis_radius: Fixed16::from_raw(i32::from(x) << 9),
            perpendicular_radius: Fixed16::from_raw(i32::from(z) << 9),
            axis: vec3(0x10000, 0, 0),
            perpendicular: vec3(0, 0, 0x10000),
        }
    }

    #[derive(Default)]
    struct Script {
        calls: Vec<(u16, [i32; 2], StaticCollisionRunningState)>,
        fail: Option<u16>,
    }
    impl StaticCollisionKernel<u8> for Script {
        type Error = u16;
        fn resolve(
            &mut self,
            frame: &StaticCarCollisionFrame,
            record: &StaticCollisionRecord<u8>,
            running: StaticCollisionRunningState,
        ) -> Result<StaticCollisionResolution, Self::Error> {
            self.calls.push((
                record.collision_ordinal,
                frame.previous_center_xz.map(Fixed16::raw),
                running,
            ));
            if self.fail == Some(record.collision_ordinal) {
                return Err(record.collision_ordinal);
            }
            if record.geometry == 0 {
                return Ok(StaticCollisionResolution::Miss);
            }
            if record.alternate_collision_path() {
                return Ok(StaticCollisionResolution::Alternate);
            }
            let mut history = running.history;
            history.push(STATIC_SCENERY_CONTACT_ID);
            Ok(StaticCollisionResolution::Normal(
                StaticCollisionContribution {
                    contact_normal: vec3(i32::from(record.geometry), 0, 0),
                    linear_impulse_3cc: running.linear_impulse_3cc.add(vec3(
                        i32::MAX,
                        i32::from(record.geometry),
                        0,
                    )),
                    angular_impulse_3d8: running.angular_impulse_3d8.add(vec3(
                        0,
                        0,
                        i32::from(record.geometry),
                    )),
                    local_contact_arm_3f0: running.local_contact_arm_3f0.add(vec3(1, 0, 0)),
                    history,
                },
            ))
        }
    }

    #[test]
    fn node_domain_and_membership_are_checked_and_ordered() {
        assert_eq!(node(0).index(), 0);
        assert_eq!(node(4095).index(), 4095);
        assert!(StaticCollisionNodeId::new(-1).is_err());
        assert!(StaticCollisionNodeId::new(4096).is_err());
        assert!(StaticCollisionScene::new(vec![record(1, 0, 1)], [(node(4), 1)]).is_err());
        let scene = StaticCollisionScene::new(
            vec![record(10, 0, 1), record(11, 0, 1)],
            [(node(7), 1), (node(7), 0)],
        )
        .unwrap();
        assert_eq!(scene.node_records(node(7)), &[0, 1]);
    }

    #[test]
    fn obb_points_follow_native_sign_order_and_wrapping_q16() {
        let one = Fixed16::ONE;
        let basis = [
            Vec3::new(one, Fixed16::ZERO, Fixed16::ZERO),
            Vec3::new(Fixed16::ZERO, one, Fixed16::ZERO),
            Vec3::new(Fixed16::ZERO, Fixed16::ZERO, one),
        ];
        let points = build_static_obb_points(
            vec3(10 << 16, 20 << 16, 30 << 16),
            basis,
            vec3(1 << 16, 2 << 16, 3 << 16),
        );
        assert_eq!(
            points,
            [
                vec3(11 << 16, 18 << 16, 33 << 16),
                vec3(9 << 16, 18 << 16, 33 << 16),
                vec3(9 << 16, 18 << 16, 27 << 16),
                vec3(11 << 16, 18 << 16, 27 << 16),
                vec3(11 << 16, 22 << 16, 33 << 16),
                vec3(11 << 16, 22 << 16, 27 << 16),
                vec3(9 << 16, 22 << 16, 33 << 16),
                vec3(9 << 16, 22 << 16, 27 << 16),
            ]
        );

        let wrapping = build_static_obb_points(vec3(i32::MAX, 0, 0), basis, vec3(1 << 16, 0, 0));
        assert_eq!(wrapping[0].x.raw(), i32::MIN + 0xffff);
        assert_eq!(wrapping[1].x.raw(), i32::MAX - (1 << 16));
    }

    #[test]
    fn obb_projection_selects_horizontal_axis_extents_and_native_corners() {
        let one = Fixed16::ONE;
        let basis = [
            Vec3::new(one, Fixed16::ZERO, Fixed16::ZERO),
            Vec3::new(Fixed16::ZERO, one, Fixed16::ZERO),
            Vec3::new(Fixed16::ZERO, Fixed16::ZERO, one),
        ];
        let center = vec3(10 << 16, 20 << 16, 30 << 16);
        let points = build_static_obb_points(center, basis, vec3(1 << 16, 2 << 16, 3 << 16));
        let projection = project_static_obb(basis, points, center);
        assert_eq!(projection.axis, vec3(65522, 0, 0));
        assert_eq!(projection.perpendicular, vec3(0, 0, -65522));
        assert_eq!(projection.axis_radius, Fixed16::from_raw(65522));
        assert_eq!(projection.perpendicular_radius, Fixed16::from_raw(196566));
        assert_eq!(projection.max_y_shift9, 256);
        assert_eq!(projection.min_y_shift9, -256);
        assert_eq!(
            static_obb_projected_corners(projection),
            [[127, 383], [127, -384], [-128, 383], [-128, -384]]
        );
    }

    #[test]
    fn near_vertical_first_basis_selects_and_flattens_second_basis() {
        let basis = [vec3(0, 0x10000, 0), vec3(0, 0x8000, 0x10000), Vec3::ZERO];
        let projection = project_static_obb(basis, [Vec3::ZERO; 8], Vec3::ZERO);
        assert_eq!(projection.axis, vec3(0, 0, 65522));
        assert_eq!(projection.perpendicular, vec3(65522, 0, 0));
    }

    #[test]
    fn obj_obb_builder_uses_scaled_bounds_truncating_halves_and_y_basis_center() {
        let one = Fixed16::ONE;
        let basis = [
            Vec3::new(one, Fixed16::ZERO, Fixed16::ZERO),
            Vec3::new(Fixed16::ZERO, one, Fixed16::ZERO),
            Vec3::new(Fixed16::ZERO, Fixed16::ZERO, one),
        ];
        let geometry = build_static_obb_geometry(StaticObbBuildInput {
            collision_translation: vec3(10 << 16, 20 << 16, 30 << 16),
            normalized_root_local_basis: basis,
            root_scale: vec3(2 << 16, 4 << 16, 6 << 16),
            mesh_bound: vec3(1 << 16, 1 << 16, 1 << 16),
        })
        .unwrap();
        assert_eq!(geometry.scaled_extents, vec3(2 << 16, 4 << 16, 6 << 16));
        assert_eq!(geometry.half_extents, vec3(1 << 16, 2 << 16, 3 << 16));
        assert_eq!(geometry.bounds_half_scalars, geometry.half_extents);
        assert_eq!(geometry.center, vec3(10 << 16, 22 << 16, 30 << 16));
        assert_eq!(geometry.basis, basis);
    }

    #[test]
    fn obj_obb_builder_keeps_negative_odd_half_policies_distinct_and_rejects_zero_scale() {
        let basis = [
            vec3(0x10000, 0, 0),
            vec3(0, 0x10000, 0),
            vec3(0, 0, 0x10000),
        ];
        let geometry = build_static_obb_geometry(StaticObbBuildInput {
            collision_translation: Vec3::ZERO,
            normalized_root_local_basis: basis,
            root_scale: vec3(-1, 0x10000, 0x10000),
            mesh_bound: vec3(0x10000, 0x10000, 0x10000),
        })
        .unwrap();
        assert_eq!(geometry.scaled_extents.x.raw(), -1);
        assert_eq!(geometry.half_extents.x.raw(), 0);
        assert_eq!(geometry.bounds_half_scalars.x.raw(), -1);

        assert_eq!(
            build_static_obb_geometry(StaticObbBuildInput {
                root_scale: vec3(1, 0x10000, 0x10000),
                mesh_bound: vec3(1, 0x10000, 0x10000),
                collision_translation: Vec3::ZERO,
                normalized_root_local_basis: basis,
            }),
            Err(StaticObbBuildError {
                zero_scaled_axis: 0
            })
        );
    }

    #[test]
    fn normal_narrow_phase_pins_y_gate_thresholds_and_previous_xz_push() {
        let car = narrow_car(1 << 16, 1 << 16);
        let point_object = narrow_object(0, 0);
        let hit = resolve_static_obb_narrow_phase(car, point_object);
        assert!(hit.hit);
        assert_eq!(hit.contact_normal, Some(vec3(65520, 0, 0)));
        assert_eq!(hit.pass1_inside_object_corners.len, 4);
        assert_eq!(hit.pass2_inside_car_corners.len, 0);
        assert_eq!(hit.pass1_correction, vec3(-(1 << 16), 0, 0));
        assert_eq!(hit.final_position, vec3(-(1 << 16), 0, 0));
        assert_eq!(hit.contact_offset_world, Some(vec3(1 << 16, 0, 0)));

        let mut rejected_car = car;
        rejected_car.y_gate_552f60 = Fixed16::from_raw((-128_i32 << 9) - 1);
        assert!(!resolve_static_obb_narrow_phase(rejected_car, point_object).hit);
        rejected_car.y_gate_552f60 = Fixed16::from_raw(-128_i32 << 9);
        rejected_car.y_gate_552f80 = Fixed16::from_raw(128_i32 << 9);
        assert!(resolve_static_obb_narrow_phase(rejected_car, point_object).hit);

        assert_eq!(
            exit_distance(Fixed16::ZERO, Fixed16::ONE, Fixed16::from_raw(65)),
            None
        );
        assert_eq!(
            exit_distance(Fixed16::ZERO, Fixed16::ONE, Fixed16::from_raw(-65)),
            None
        );
        assert!(exit_distance(Fixed16::ZERO, Fixed16::ONE, Fixed16::from_raw(66)).is_some());
        assert!(exit_distance(Fixed16::ZERO, Fixed16::ONE, Fixed16::from_raw(-66)).is_some());
    }

    #[test]
    fn exit_candidate_pins_sentinel_ceiling_and_equal_axis_tie() {
        let mut pass_max = Fixed16::ZERO;
        update_pass_max(
            &mut pass_max,
            [Fixed16::ZERO; 2],
            [Fixed16::from_raw(0x7d000000), Fixed16::ZERO],
            [Fixed16::ONE, Fixed16::ZERO],
        );
        // A valid ratio equal to the sentinel does not replace it, but the
        // still-valid candidate promotes that sentinel to the pass maximum.
        assert_eq!(pass_max.raw(), 0x7d000000);

        let tied = exit_distance(Fixed16::ZERO, Fixed16::ONE, Fixed16::ONE).unwrap();
        assert_eq!(
            exit_distance(Fixed16::ZERO, Fixed16::ONE, Fixed16::ONE),
            Some(tied)
        );
        pass_max = Fixed16::ZERO;
        update_pass_max(
            &mut pass_max,
            [Fixed16::ZERO; 2],
            [Fixed16::ONE; 2],
            [Fixed16::ONE; 2],
        );
        // Axis one replaces only on strict `<`, so an equal ratio leaves the
        // axis-zero candidate unchanged (the distance is one in either case).
        assert_eq!(pass_max, Fixed16::ONE);
    }

    #[test]
    fn pass1_translation_changes_the_points_seen_by_pass2_admission() {
        let car = narrow_car(2 << 16, 1 << 16);
        let object = narrow_object(128, 128);
        assert!(car.projected_corners.iter().all(|point| {
            !point_inside_obb(
                point.sub(object.center),
                object.axis,
                object.perpendicular,
                object.axis_radius,
                object.perpendicular_radius,
            )
        }));

        let result = resolve_static_obb_narrow_phase(car, object);
        assert_eq!(result.pass1_inside_object_corners.len, 4);
        assert_eq!(result.pass1_correction, vec3(-(3 << 16), 0, 0));
        assert_eq!(result.pass2_inside_car_corners.indices[..2], [1, 2]);
        assert_eq!(result.pass2_inside_car_corners.len, 2);
    }

    #[test]
    fn native_correction_order_diverges_at_wrapping_negation_boundary() {
        let direction = vec3(i32::MIN, 0, 0);
        let distance = Fixed16::from_raw(0x8000);
        let native = direction.scale(distance.neg());
        let via_negated_direction = direction.scale(Fixed16::from_raw(-0x10000)).scale(distance);
        assert_eq!(native.x.raw(), 0x40000000);
        assert_eq!(via_negated_direction.x.raw(), -0x40000000);
        assert_ne!(native, via_negated_direction);
    }

    #[test]
    fn projections_abs_and_translation_keep_wrapping_i32_behavior() {
        let overflowing_dot = vec3(i32::MAX, i32::MAX, 0).dot(vec3(0x10000, 0x10000, 0));
        assert_eq!(overflowing_dot.raw(), -2);
        assert_eq!(native_abs(Fixed16::from_raw(i32::MIN)).raw(), i32::MIN);
        assert!(inside_projected(
            [Fixed16::from_raw(i32::MIN), Fixed16::ZERO],
            [Fixed16::ZERO; 2]
        ));

        let mut translated = StaticNarrowResult {
            hit: true,
            contact_normal: None,
            pass1_inside_object_corners: ContainedCornerIndices::default(),
            pass2_inside_car_corners: ContainedCornerIndices::default(),
            pass1_correction: Vec3::ZERO,
            pass2_correction: Vec3::ZERO,
            final_position: vec3(i32::MAX, 0, 0),
            final_hull_points: [vec3(i32::MAX, 0, 0); 8],
            final_projected_corners: [vec3(i32::MAX, 0, 0); 4],
            contact_offset_world: None,
        };
        translate_car_cache(&mut translated, vec3(1, 0, 0));
        assert_eq!(translated.final_position.x.raw(), i32::MIN);
        assert!(
            translated
                .final_hull_points
                .iter()
                .all(|point| point.x.raw() == i32::MIN)
        );
        assert!(
            translated
                .final_projected_corners
                .iter()
                .all(|point| point.x.raw() == i32::MIN)
        );
    }

    #[test]
    fn reciprocal_corner_test_deliberately_misses_edge_only_cross_overlap() {
        let car = narrow_car(3 << 16, 1 << 16);
        let object = narrow_object(128, 384);
        let result = resolve_static_obb_narrow_phase(car, object);
        assert!(!result.hit);
        assert_eq!(result.final_position, car.position);
        assert_eq!(result.contact_normal, None);
    }

    #[test]
    fn second_pass_uses_car_corners_and_forces_contact_offset_y_to_zero() {
        let mut car = narrow_car(1 << 16, 1 << 16);
        for point in &mut car.hull_points[..4] {
            point.x = point.x.add(Fixed16::from_raw(2 << 16));
            point.y = Fixed16::from_raw(5 << 16);
            point.z = point.z.add(Fixed16::from_raw(3 << 16));
        }
        let object = narrow_object(384, 384);
        let result = resolve_static_obb_narrow_phase(car, object);
        assert!(result.hit);
        assert_eq!(result.pass1_inside_object_corners.len, 0);
        assert_eq!(result.pass2_inside_car_corners.len, 4);
        assert_eq!(result.pass2_correction, vec3(-(4 << 16), 0, 0));
        // Admission indices come from the projected-corner cache, but native
        // contact averaging indexes the full hull at car+0xfc/car+0x104.
        assert_eq!(result.contact_offset_world, Some(vec3(2 << 16, 0, 3 << 16)));
    }

    #[test]
    fn zero_previous_delta_still_reports_containment_without_translation() {
        let mut car = narrow_car(1 << 16, 1 << 16);
        car.previous_center_xz = [Fixed16::ZERO; 2];
        let result = resolve_static_obb_narrow_phase(car, narrow_object(0, 0));
        assert!(result.hit);
        assert_eq!(result.contact_normal, Some(Vec3::ZERO));
        assert_eq!(result.pass1_correction, Vec3::ZERO);
        assert_eq!(result.pass2_correction, Vec3::ZERO);
    }

    #[test]
    fn alternate_path_damps_only_residual_and_pins_radial_weights() {
        let mut car = narrow_car(1 << 16, 1 << 16);
        car.velocity_d4 = vec3(1 << 16, 1 << 16, 0);
        car.hull_points[0] = Vec3::ZERO;
        car.hull_points[1] = vec3(1 << 16, 0, 0);
        let result = apply_alternate_static_narrow_phase(car, narrow_object(0, 0)).unwrap();
        assert_eq!(result.velocity_d4, vec3(1 << 16, (1 << 16) - 0x51e, 0));
        assert_eq!(result.radial_weights_304[0].raw(), 0xffff);
        assert_eq!(result.radial_weights_304[1], Fixed16::ZERO);
        assert_eq!(result.point_flags_69c, [1; 8]);

        let mut zero_radius = narrow_object(0, 0);
        zero_radius.radius = Fixed16::ZERO;
        let result = apply_alternate_static_narrow_phase(car, zero_radius).unwrap();
        assert_eq!(result.radial_weights_304[0].raw(), 0xffff);
        assert_eq!(result.radial_weights_304[1], Fixed16::ZERO);
    }

    #[test]
    fn alternate_path_rejection_has_no_effect_result() {
        let car = narrow_car(1 << 16, 1 << 16);
        let mut object = narrow_object(0, 0);
        object.center = vec3(10 << 16, 0, 10 << 16);
        assert_eq!(apply_alternate_static_narrow_phase(car, object), None);
    }

    #[test]
    fn alternate_path_can_admit_only_through_object_corner_in_car() {
        let car = narrow_car(1 << 16, 1 << 16);
        let object = narrow_object(0, 0);
        assert!(car.projected_corners.iter().all(|point| {
            !point_inside_obb(
                point.sub(object.center),
                object.axis,
                object.perpendicular,
                object.axis_radius,
                object.perpendicular_radius,
            )
        }));
        assert!(point_inside_obb(
            object_corner(object, 0).sub(car.position),
            car.projection_axis,
            car.projection_perpendicular,
            car.projection_axis_radius,
            car.projection_perpendicular_radius,
        ));
        assert!(apply_alternate_static_narrow_phase(car, object).is_some());
    }

    #[test]
    fn alternate_radius_equality_plus_one_and_clamp_are_exact() {
        let mut car = narrow_car(1 << 16, 1 << 16);
        car.hull_points[0] = vec3(1 << 16, 0, 0);
        car.hull_points[1] = Vec3::ZERO;
        let mut object = narrow_object(0, 0);
        let distance = native_length(car.hull_points[0].sub(object.center));

        object.radius = distance;
        let equal = apply_alternate_static_narrow_phase(car, object).unwrap();
        assert_eq!(equal.radial_weights_304[0].raw(), 1);
        // Zero distance produces alpha=1.0, which is clamped to 0xc000 before
        // the exact 0x15555 expansion yields 0xffff.
        assert_eq!(equal.radial_weights_304[1].raw(), 0xffff);

        object.radius = Fixed16::from_raw(distance.raw() - 1);
        let plus_one = apply_alternate_static_narrow_phase(car, object).unwrap();
        assert_eq!(plus_one.radial_weights_304[0], Fixed16::ZERO);
    }

    #[test]
    fn ordered_current_contacts_accumulate_and_rotate_history() {
        let scene = StaticCollisionScene::new(
            vec![record(10, 0x85, 1), record(11, 0x7f, 0), record(12, 3, 2)],
            [(node(2), 0), (node(2), 1), (node(3), 2)],
        )
        .unwrap();
        let mut input = frame([Some(node(2)), Some(node(3)), None, None]);
        input.previous_center_xz = [Fixed16::from_raw(123), Fixed16::from_raw(-456)];
        input.linear_impulse_3cc = vec3(5, 6, 7);
        input.angular_impulse_3d8 = vec3(1, 2, 3);
        let history = StaticContactHistory {
            current_ids: [0, 99, 0, 0, 0],
            current_len: 2,
            ..Default::default()
        };
        let mut kernel = Script::default();
        let effects =
            resolve_static_world_collisions(&scene, &input, history, &mut kernel).unwrap();
        assert_eq!(
            kernel.calls.iter().map(|v| v.0).collect::<Vec<_>>(),
            [11, 10, 12]
        );
        assert!(kernel.calls.iter().all(|v| v.1 == [123, -456]));
        assert_eq!(effects.linear_impulse_3cc, vec3(i32::MIN + 4, 8, 7));
        assert_eq!(effects.angular_impulse_3d8, vec3(1, 2, 5));
        assert_eq!(kernel.calls[2].2.linear_impulse_3cc, vec3(5, 6, 7));
        assert_eq!(kernel.calls[2].2.angular_impulse_3d8, vec3(1, 2, 3));
        assert_eq!(effects.local_contact_arm_3f0, vec3(8, 8, 9));
        assert_eq!(effects.history.previous_ids[..2], [0, 99]);
        assert_eq!(effects.history.current_ids[..1], [0]);
        assert_eq!(
            effects.contacts[0],
            StaticResolvedContact::Alternate {
                collision_ordinal: 10,
                raw_material_id: 5,
            }
        );
        assert_eq!(
            effects.contacts[1],
            StaticResolvedContact::Normal {
                collision_ordinal: 12,
                contact_history_id: 0,
                raw_material_id: 3,
                contact_normal: vec3(2, 0, 0),
                repeated_from_previous_tick: true,
                stored_in_current_history: true,
            }
        );
    }

    #[test]
    fn path_crossing_without_current_overlap_is_not_swept_collision() {
        let scene = StaticCollisionScene::new(vec![record(10, 0, 0)], [(node(1), 0)]).unwrap();
        let mut input = frame([Some(node(1)), None, None, None]);
        input.previous_center_xz = [Fixed16::from_int(-100), Fixed16::from_int(100)];
        input.center = vec3(100 << 16, 0, -(100 << 16));
        let effects = resolve_static_world_collisions(
            &scene,
            &input,
            Default::default(),
            &mut Script::default(),
        )
        .unwrap();
        assert!(effects.contacts.is_empty());
        assert_eq!(effects.linear_impulse_3cc, Vec3::ZERO);
    }

    #[test]
    fn history_saturates_but_all_contacts_still_contribute() {
        let records = (0..7).map(|id| record(id, id as u8, 1)).collect();
        let scene = StaticCollisionScene::new(records, (0..7).map(|i| (node(8), i))).unwrap();
        let effects = resolve_static_world_collisions(
            &scene,
            &frame([Some(node(8)), None, None, None]),
            Default::default(),
            &mut Script::default(),
        )
        .unwrap();
        assert_eq!(effects.contacts.len(), 7);
        assert_eq!(effects.history.current_ids, [0; 5]);
        assert_eq!(effects.history.current_len, 5);
        assert!(effects.history.current_overflow);
        assert_eq!(effects.linear_impulse_3cc.x.raw(), i32::MAX - 6);
    }

    #[test]
    fn kernel_failure_reports_ordered_record_without_an_aggregate() {
        let scene = StaticCollisionScene::new(
            vec![record(20, 0, 1), record(21, 0, 1)],
            [(node(9), 0), (node(9), 1)],
        )
        .unwrap();
        let mut kernel = Script {
            fail: Some(21),
            ..Default::default()
        };
        let error = resolve_static_world_collisions(
            &scene,
            &frame([Some(node(9)), None, None, None]),
            Default::default(),
            &mut kernel,
        )
        .unwrap_err();
        assert_eq!(
            error,
            StaticCollisionExecutionError {
                node: node(9),
                record_index: 1,
                source: 21
            }
        );
    }
}
