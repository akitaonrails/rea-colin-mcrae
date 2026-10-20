//! Deterministic transform and clipping preflight for the software C3D path.
//!
//! The shipped renderer establishes row-vector world/view/projection transforms,
//! the `D3DVIEWPORT2` clip volume, and ordinary homogeneous clipping. It does not
//! establish one driver-exact floating-point evaluation order or equality rule.
//! This source-owned oracle therefore uses sequential `f32` arithmetic and closed
//! clip planes. Those two choices are reconstruction policy, not claims about a
//! particular 1990s Direct3D driver.

use std::fmt;

use cmr_core::{
    root_menu_c3d_frame_plan::RootMenuC3dDrawCommand,
    root_menu_c3d_runtime::C3dMode2ResourceScaledVertex,
    root_menu_c3d_transform::{C3dMatrixWords, C3dViewportWords},
};

const CLIP_PLANE_COUNT: usize = 6;

/// A lit C3D vertex after world/view/projection and clip-volume normalization.
///
/// Diffuse and specular channels are stored in `[alpha, red, green, blue]`
/// order, normalized to `0.0..=1.0`. The reserved D3DLVERTEX word is
/// deliberately ignored; the recovered legacy ABI identifies it as reserved,
/// not RHW.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SoftwareC3dClipVertex {
    clip_position: [f32; 4],
    diffuse_argb: [f32; 4],
    specular_argb: [f32; 4],
    uv: [f32; 2],
}

impl SoftwareC3dClipVertex {
    #[must_use]
    pub const fn clip_position(self) -> [f32; 4] {
        self.clip_position
    }

    #[must_use]
    pub const fn diffuse_argb(self) -> [f32; 4] {
        self.diffuse_argb
    }

    #[must_use]
    pub const fn specular_argb(self) -> [f32; 4] {
        self.specular_argb
    }

    #[must_use]
    pub const fn uv(self) -> [f32; 2] {
        self.uv
    }

    fn is_finite(self) -> bool {
        self.clip_position
            .into_iter()
            .chain(self.diffuse_argb)
            .chain(self.specular_argb)
            .chain(self.uv)
            .all(f32::is_finite)
    }

    fn interpolate(self, other: Self, amount: f32) -> Self {
        Self {
            clip_position: interpolate_array(self.clip_position, other.clip_position, amount),
            diffuse_argb: interpolate_array(self.diffuse_argb, other.diffuse_argb, amount),
            specular_argb: interpolate_array(self.specular_argb, other.specular_argb, amount),
            uv: interpolate_array(self.uv, other.uv, amount),
        }
    }
}

/// A clipped vertex projected into the target's pixel/depth coordinates.
///
/// `uv_over_w` and `reciprocal_w` are retained separately so the later
/// rasterizer can perform the recovered perspective-correct UV interpolation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SoftwareC3dScreenVertex {
    position: [f32; 3],
    reciprocal_w: f32,
    uv_over_w: [f32; 2],
    diffuse_argb: [f32; 4],
    specular_argb: [f32; 4],
}

impl SoftwareC3dScreenVertex {
    #[must_use]
    pub const fn position(self) -> [f32; 3] {
        self.position
    }

    #[must_use]
    pub const fn reciprocal_w(self) -> f32 {
        self.reciprocal_w
    }

    #[must_use]
    pub const fn uv_over_w(self) -> [f32; 2] {
        self.uv_over_w
    }

    #[must_use]
    pub const fn diffuse_argb(self) -> [f32; 4] {
        self.diffuse_argb
    }

    #[must_use]
    pub const fn specular_argb(self) -> [f32; 4] {
        self.specular_argb
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SoftwareC3dRasterError {
    IncompleteIndexTriplet {
        index_count: usize,
    },
    IndexOutOfRange {
        triangle: usize,
        corner: usize,
        index: u16,
        vertex_count: usize,
    },
    NonFiniteInput,
    InvalidViewport,
    NonFiniteResult,
    NonPositiveHomogeneousW,
}

impl fmt::Display for SoftwareC3dRasterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "software C3D raster preflight failed: {self:?}")
    }
}

impl std::error::Error for SoftwareC3dRasterError {}

/// Immutable transform state consumed by one software batch preflight.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SoftwareC3dTransformState {
    world: C3dMatrixWords,
    view: C3dMatrixWords,
    projection: C3dMatrixWords,
    viewport: C3dViewportWords,
}

impl SoftwareC3dTransformState {
    #[must_use]
    pub const fn new(
        world: C3dMatrixWords,
        view: C3dMatrixWords,
        projection: C3dMatrixWords,
        viewport: C3dViewportWords,
    ) -> Self {
        Self {
            world,
            view,
            projection,
            viewport,
        }
    }
}

/// One clockwise, non-degenerate screen triangle ready for later coverage.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SoftwareC3dPreparedTriangle {
    vertices: [SoftwareC3dScreenVertex; 3],
    flat_diffuse: u32,
}

impl SoftwareC3dPreparedTriangle {
    #[must_use]
    pub const fn vertices(&self) -> &[SoftwareC3dScreenVertex; 3] {
        &self.vertices
    }

    /// Diffuse value from the original triangle's first submitted index.
    #[must_use]
    pub const fn flat_diffuse(&self) -> u32 {
        self.flat_diffuse
    }
}

/// Deterministic result of validating, transforming, clipping, and culling a
/// source batch. Omission counters make every successful discard observable.
#[derive(Clone, Debug, PartialEq)]
pub struct SoftwareC3dPreparedBatch {
    triangles: Box<[SoftwareC3dPreparedTriangle]>,
    source_triangle_count: usize,
    fully_clipped_source_triangle_count: usize,
    counter_clockwise_triangle_count: usize,
    degenerate_triangle_count: usize,
}

impl SoftwareC3dPreparedBatch {
    #[must_use]
    pub fn triangles(&self) -> &[SoftwareC3dPreparedTriangle] {
        &self.triangles
    }

    #[must_use]
    pub const fn source_triangle_count(&self) -> usize {
        self.source_triangle_count
    }

    #[must_use]
    pub const fn fully_clipped_source_triangle_count(&self) -> usize {
        self.fully_clipped_source_triangle_count
    }

    #[must_use]
    pub const fn counter_clockwise_triangle_count(&self) -> usize {
        self.counter_clockwise_triangle_count
    }

    #[must_use]
    pub const fn degenerate_triangle_count(&self) -> usize {
        self.degenerate_triangle_count
    }
}

/// Validates and preflights one recovered source-ordered triangle-list batch.
///
/// Culling happens after clipping and the single viewport Y inversion. The
/// recovered `D3DCULL_CCW` state keeps positive signed screen areas.
pub fn preflight_root_c3d_batch(
    command: &RootMenuC3dDrawCommand,
    transforms: SoftwareC3dTransformState,
) -> Result<SoftwareC3dPreparedBatch, SoftwareC3dRasterError> {
    let vertices = command
        .vertices()
        .iter()
        .copied()
        .map(RasterInputVertex::from)
        .collect::<Vec<_>>();
    preflight_indexed_triangle_list(&vertices, command.indices(), transforms)
}

/// Applies the recovered row-vector transform chain and `D3DVIEWPORT2` clip
/// volume to one untransformed lit vertex.
pub fn transform_root_c3d_vertex(
    vertex: C3dMode2ResourceScaledVertex,
    world: C3dMatrixWords,
    view: C3dMatrixWords,
    projection: C3dMatrixWords,
    viewport: C3dViewportWords,
) -> Result<SoftwareC3dClipVertex, SoftwareC3dRasterError> {
    let input = RasterInputVertex::from(vertex).into_clip_vertex();
    transform_vertex(input, world, view, projection, viewport)
}

/// Clips one source-ordered triangle against all six homogeneous clip planes.
///
/// The returned polygon retains source winding and has zero vertices when the
/// triangle is fully outside. Boundary points are inside by project policy.
pub fn clip_root_c3d_triangle(
    triangle: [SoftwareC3dClipVertex; 3],
) -> Result<Vec<SoftwareC3dClipVertex>, SoftwareC3dRasterError> {
    if !triangle.into_iter().all(SoftwareC3dClipVertex::is_finite) {
        return Err(SoftwareC3dRasterError::NonFiniteInput);
    }

    let mut polygon = triangle.to_vec();
    for plane in 0..CLIP_PLANE_COUNT {
        polygon = clip_polygon_against_plane(&polygon, plane)?;
        if polygon.is_empty() {
            break;
        }
    }
    Ok(polygon)
}

/// Performs homogeneous divide and the recovered viewport mapping.
pub fn project_root_c3d_polygon(
    polygon: &[SoftwareC3dClipVertex],
    viewport: C3dViewportWords,
) -> Result<Vec<SoftwareC3dScreenVertex>, SoftwareC3dRasterError> {
    let viewport = DecodedViewport::decode(viewport)?;
    polygon
        .iter()
        .copied()
        .map(|vertex| project_vertex(vertex, viewport))
        .collect()
}

fn transform_vertex(
    mut vertex: SoftwareC3dClipVertex,
    world: C3dMatrixWords,
    view: C3dMatrixWords,
    projection: C3dMatrixWords,
    viewport: C3dViewportWords,
) -> Result<SoftwareC3dClipVertex, SoftwareC3dRasterError> {
    if !vertex.is_finite()
        || !world
            .words()
            .map(f32::from_bits)
            .into_iter()
            .all(f32::is_finite)
        || !view
            .words()
            .map(f32::from_bits)
            .into_iter()
            .all(f32::is_finite)
        || !projection
            .words()
            .map(f32::from_bits)
            .into_iter()
            .all(f32::is_finite)
    {
        return Err(SoftwareC3dRasterError::NonFiniteInput);
    }
    let viewport = DecodedViewport::decode(viewport)?;
    vertex.clip_position = multiply_row_vector(vertex.clip_position, decode_matrix(world));
    vertex.clip_position = multiply_row_vector(vertex.clip_position, decode_matrix(view));
    vertex.clip_position = multiply_row_vector(vertex.clip_position, decode_matrix(projection));

    let [x, y, z, w] = vertex.clip_position;
    let clip_center_x = viewport.clip_x + viewport.clip_width * 0.5;
    let clip_center_y = viewport.clip_y - viewport.clip_height * 0.5;
    vertex.clip_position = [
        (x - clip_center_x * w) * (2.0 / viewport.clip_width),
        (y - clip_center_y * w) * (2.0 / viewport.clip_height),
        (z - viewport.min_z * w) * (1.0 / (viewport.max_z - viewport.min_z)),
        w,
    ];
    if !vertex.is_finite() {
        return Err(SoftwareC3dRasterError::NonFiniteResult);
    }
    Ok(vertex)
}

fn clip_polygon_against_plane(
    polygon: &[SoftwareC3dClipVertex],
    plane: usize,
) -> Result<Vec<SoftwareC3dClipVertex>, SoftwareC3dRasterError> {
    let mut output = Vec::with_capacity(polygon.len().saturating_add(1));
    let Some(mut previous) = polygon.last().copied() else {
        return Ok(output);
    };
    let mut previous_distance = clip_distance(previous.clip_position, plane);
    let mut previous_inside = previous_distance >= 0.0;

    for current in polygon.iter().copied() {
        let current_distance = clip_distance(current.clip_position, plane);
        let current_inside = current_distance >= 0.0;
        if current_inside != previous_inside {
            // Do not synthesize an endpoint already lying on this closed plane:
            // interpolation at exactly zero/one can perturb unrelated payload
            // bits and defeat whole-vertex duplicate suppression.
            if previous_distance != 0.0 && current_distance != 0.0 {
                let amount = previous_distance / (previous_distance - current_distance);
                let intersection = previous.interpolate(current, amount);
                if !intersection.is_finite() {
                    return Err(SoftwareC3dRasterError::NonFiniteResult);
                }
                push_distinct(&mut output, intersection);
            }
        }
        if current_inside {
            push_distinct(&mut output, current);
        }
        previous = current;
        previous_distance = current_distance;
        previous_inside = current_inside;
    }
    if output.len() > 1 && output.first() == output.last() {
        output.pop();
    }
    Ok(output)
}

fn push_distinct(output: &mut Vec<SoftwareC3dClipVertex>, vertex: SoftwareC3dClipVertex) {
    if output.last() != Some(&vertex) {
        output.push(vertex);
    }
}

fn clip_distance([x, y, z, w]: [f32; 4], plane: usize) -> f32 {
    match plane {
        0 => x + w,
        1 => w - x,
        2 => y + w,
        3 => w - y,
        4 => z,
        5 => w - z,
        _ => unreachable!("exactly six clip planes"),
    }
}

fn project_vertex(
    vertex: SoftwareC3dClipVertex,
    viewport: DecodedViewport,
) -> Result<SoftwareC3dScreenVertex, SoftwareC3dRasterError> {
    if !vertex.is_finite() {
        return Err(SoftwareC3dRasterError::NonFiniteInput);
    }
    let [x, y, z, w] = vertex.clip_position;
    if w <= 0.0 {
        return Err(SoftwareC3dRasterError::NonPositiveHomogeneousW);
    }
    let reciprocal_w = 1.0 / w;
    let normalized_x = x * reciprocal_w;
    let normalized_y = y * reciprocal_w;
    let normalized_z = z * reciprocal_w;
    let result = SoftwareC3dScreenVertex {
        position: [
            viewport.x + (normalized_x + 1.0) * viewport.width * 0.5,
            viewport.y + (1.0 - normalized_y) * viewport.height * 0.5,
            normalized_z,
        ],
        reciprocal_w,
        uv_over_w: [vertex.uv[0] * reciprocal_w, vertex.uv[1] * reciprocal_w],
        diffuse_argb: vertex.diffuse_argb,
        specular_argb: vertex.specular_argb,
    };
    if result
        .position
        .into_iter()
        .chain([result.reciprocal_w])
        .chain(result.uv_over_w)
        .all(f32::is_finite)
    {
        Ok(result)
    } else {
        Err(SoftwareC3dRasterError::NonFiniteResult)
    }
}

fn preflight_indexed_triangle_list(
    vertices: &[RasterInputVertex],
    indices: &[u16],
    transforms: SoftwareC3dTransformState,
) -> Result<SoftwareC3dPreparedBatch, SoftwareC3dRasterError> {
    let (source_triangles, remainder) = indices.as_chunks::<3>();
    if !remainder.is_empty() {
        return Err(SoftwareC3dRasterError::IncompleteIndexTriplet {
            index_count: indices.len(),
        });
    }

    let transformed = vertices
        .iter()
        .copied()
        .map(|vertex| {
            transform_vertex(
                vertex.into_clip_vertex(),
                transforms.world,
                transforms.view,
                transforms.projection,
                transforms.viewport,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut prepared = Vec::new();
    let mut fully_clipped = 0;
    let mut counter_clockwise = 0;
    let mut degenerate = 0;

    for (triangle_index, source_indices) in source_triangles.iter().enumerate() {
        let resolve = |corner: usize| {
            let source_index = source_indices[corner];
            transformed.get(usize::from(source_index)).copied().ok_or(
                SoftwareC3dRasterError::IndexOutOfRange {
                    triangle: triangle_index,
                    corner,
                    index: source_index,
                    vertex_count: transformed.len(),
                },
            )
        };
        let triangle = [resolve(0)?, resolve(1)?, resolve(2)?];
        let flat_diffuse = vertices[usize::from(source_indices[0])].diffuse;
        let clipped = clip_root_c3d_triangle(triangle)?;
        if clipped.is_empty() {
            fully_clipped += 1;
            continue;
        }
        if clipped.len() < 3 {
            degenerate += 1;
            continue;
        }
        let projected = project_root_c3d_polygon(&clipped, transforms.viewport)?;
        for fan_index in 1..projected.len() - 1 {
            let fan = [projected[0], projected[fan_index], projected[fan_index + 1]];
            let area = signed_screen_area(fan);
            if area == 0.0 {
                degenerate += 1;
            } else if area < 0.0 {
                counter_clockwise += 1;
            } else {
                prepared.push(SoftwareC3dPreparedTriangle {
                    vertices: fan,
                    flat_diffuse,
                });
            }
        }
    }

    Ok(SoftwareC3dPreparedBatch {
        triangles: prepared.into_boxed_slice(),
        source_triangle_count: indices.len() / 3,
        fully_clipped_source_triangle_count: fully_clipped,
        counter_clockwise_triangle_count: counter_clockwise,
        degenerate_triangle_count: degenerate,
    })
}

fn signed_screen_area(vertices: [SoftwareC3dScreenVertex; 3]) -> f32 {
    let [a, b, c] = vertices.map(SoftwareC3dScreenVertex::position);
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

fn decode_matrix(matrix: C3dMatrixWords) -> [f32; 16] {
    matrix.words().map(f32::from_bits)
}

fn multiply_row_vector(vector: [f32; 4], matrix: [f32; 16]) -> [f32; 4] {
    std::array::from_fn(|column| {
        let first = vector[0] * matrix[column];
        let second = vector[1] * matrix[4 + column];
        let third = vector[2] * matrix[8 + column];
        let fourth = vector[3] * matrix[12 + column];
        ((first + second) + third) + fourth
    })
}

fn interpolate_array<const N: usize>(left: [f32; N], right: [f32; N], amount: f32) -> [f32; N] {
    std::array::from_fn(|index| left[index] + (right[index] - left[index]) * amount)
}

fn unpack_argb(value: u32) -> [f32; 4] {
    [
        ((value >> 24) & 0xff) as f32 / 255.0,
        ((value >> 16) & 0xff) as f32 / 255.0,
        ((value >> 8) & 0xff) as f32 / 255.0,
        (value & 0xff) as f32 / 255.0,
    ]
}

#[derive(Clone, Copy)]
struct RasterInputVertex {
    position: [f32; 3],
    diffuse: u32,
    specular: u32,
    uv: [f32; 2],
}

impl RasterInputVertex {
    fn into_clip_vertex(self) -> SoftwareC3dClipVertex {
        SoftwareC3dClipVertex {
            clip_position: [self.position[0], self.position[1], self.position[2], 1.0],
            diffuse_argb: unpack_argb(self.diffuse),
            specular_argb: unpack_argb(self.specular),
            uv: self.uv,
        }
    }
}

impl From<C3dMode2ResourceScaledVertex> for RasterInputVertex {
    fn from(vertex: C3dMode2ResourceScaledVertex) -> Self {
        Self {
            position: vertex.first_triplet(),
            diffuse: vertex.diffuse(),
            specular: vertex.specular(),
            uv: vertex.adjusted_pair(),
        }
    }
}

#[derive(Clone, Copy)]
struct DecodedViewport {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    clip_x: f32,
    clip_y: f32,
    clip_width: f32,
    clip_height: f32,
    min_z: f32,
    max_z: f32,
}

impl DecodedViewport {
    fn decode(viewport: C3dViewportWords) -> Result<Self, SoftwareC3dRasterError> {
        let words = viewport.words();
        let decoded = Self {
            x: words[1] as f32,
            y: words[2] as f32,
            width: words[3] as f32,
            height: words[4] as f32,
            clip_x: f32::from_bits(words[5]),
            clip_y: f32::from_bits(words[6]),
            clip_width: f32::from_bits(words[7]),
            clip_height: f32::from_bits(words[8]),
            min_z: f32::from_bits(words[9]),
            max_z: f32::from_bits(words[10]),
        };
        let finite = [
            decoded.x,
            decoded.y,
            decoded.width,
            decoded.height,
            decoded.clip_x,
            decoded.clip_y,
            decoded.clip_width,
            decoded.clip_height,
            decoded.min_z,
            decoded.max_z,
        ]
        .into_iter()
        .all(f32::is_finite);
        if !finite
            || decoded.width <= 0.0
            || decoded.height <= 0.0
            || decoded.clip_width <= 0.0
            || decoded.clip_height <= 0.0
            || decoded.max_z <= decoded.min_z
        {
            return Err(SoftwareC3dRasterError::InvalidViewport);
        }
        Ok(decoded)
    }
}

#[cfg(test)]
mod tests {
    use cmr_core::{
        display::DisplayExtent,
        root_menu_c3d_transform::{
            ROOT_MENU_C3D_PROJECTION, ROOT_MENU_C3D_VIEW, root_menu_c3d_viewport,
            root_menu_c3d_world,
        },
    };

    use super::*;

    fn vertex(position: [f32; 4], uv: [f32; 2]) -> SoftwareC3dClipVertex {
        SoftwareC3dClipVertex {
            clip_position: position,
            diffuse_argb: [0.5, 1.0, 0.0, 0.0],
            specular_argb: [1.0, 0.0, 0.0, 0.0],
            uv,
        }
    }

    fn raw(position: [f32; 3], diffuse: u32, uv: [f32; 2]) -> RasterInputVertex {
        RasterInputVertex {
            position,
            diffuse,
            specular: 0xff00_0000,
            uv,
        }
    }

    fn identity() -> C3dMatrixWords {
        ROOT_MENU_C3D_VIEW
    }

    fn viewport() -> C3dViewportWords {
        root_menu_c3d_viewport(DisplayExtent {
            width: 640,
            height: 480,
        })
        .unwrap()
    }

    fn identity_state() -> SoftwareC3dTransformState {
        SoftwareC3dTransformState::new(identity(), identity(), identity(), viewport())
    }

    fn assert_near(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() <= 0.000_2,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn root_origin_uses_row_vectors_and_viewport_clip_aspect() {
        let transformed = transform_vertex(
            vertex([0.0, 0.0, 0.0, 1.0], [0.25, 0.75]),
            root_menu_c3d_world(0),
            ROOT_MENU_C3D_VIEW,
            ROOT_MENU_C3D_PROJECTION,
            viewport(),
        )
        .unwrap();
        let screen = project_root_c3d_polygon(&[transformed], viewport()).unwrap()[0];

        assert_near(screen.position()[0], 329.599_6);
        assert_near(screen.position()[1], 237.907_5);
        assert_near(screen.position()[2], 0.950_951);
        assert_near(screen.reciprocal_w(), 0.05);
        assert_eq!(screen.uv_over_w(), [0.0125, 0.0375]);
    }

    #[test]
    fn viewport_clip_y_extent_maps_to_top_and_bottom() {
        let aspect = 640.0 / 480.0;
        let top = transform_vertex(
            vertex([0.0, aspect, 0.5, 1.0], [0.0, 0.0]),
            identity(),
            identity(),
            identity(),
            viewport(),
        )
        .unwrap();
        let bottom = transform_vertex(
            vertex([0.0, -aspect, 0.5, 1.0], [0.0, 0.0]),
            identity(),
            identity(),
            identity(),
            viewport(),
        )
        .unwrap();
        let projected = project_root_c3d_polygon(&[top, bottom], viewport()).unwrap();

        assert_near(top.clip_position()[1], 1.0);
        assert_near(bottom.clip_position()[1], -1.0);
        assert_near(projected[0].position()[1], 0.0);
        assert_near(projected[1].position()[1], 480.0);
    }

    #[test]
    fn clips_each_homogeneous_plane_and_interpolates_attributes() {
        let inside_a = vertex([0.0, 0.0, 0.5, 1.0], [0.0, 0.0]);
        let inside_b = vertex([0.5, 0.0, 0.5, 1.0], [1.0, 0.0]);
        let outside_positions = [
            [-2.0, 0.0, 0.5, 1.0],
            [2.0, 0.0, 0.5, 1.0],
            [0.0, -2.0, 0.5, 1.0],
            [0.0, 2.0, 0.5, 1.0],
            [0.0, 0.0, -1.0, 1.0],
            [0.0, 0.0, 2.0, 1.0],
        ];

        for (plane, outside_position) in outside_positions.into_iter().enumerate() {
            let outside = vertex(outside_position, [0.5, 1.0]);
            let polygon = clip_root_c3d_triangle([inside_a, inside_b, outside]).unwrap();
            assert_eq!(polygon.len(), 4, "plane {plane}");
            assert!(polygon.iter().all(|candidate| {
                (0..CLIP_PLANE_COUNT).all(|candidate_plane| {
                    clip_distance(candidate.clip_position(), candidate_plane) >= 0.0
                })
            }));
            assert!(polygon.iter().any(|candidate| {
                clip_distance(candidate.clip_position(), plane).abs() <= f32::EPSILON
                    && candidate.uv()[1] > 0.0
                    && candidate.uv()[1] < 1.0
            }));
        }
    }

    #[test]
    fn fully_outside_triangle_is_rejected_and_boundary_is_closed() {
        let outside = clip_root_c3d_triangle([
            vertex([-3.0, 0.0, 0.5, 1.0], [0.0, 0.0]),
            vertex([-2.0, 0.5, 0.5, 1.0], [0.0, 0.0]),
            vertex([-2.0, -0.5, 0.5, 1.0], [0.0, 0.0]),
        ])
        .unwrap();
        assert!(outside.is_empty());

        let boundary = [
            vertex([-1.0, 0.0, 0.0, 1.0], [0.0, 0.0]),
            vertex([1.0, 0.0, 1.0, 1.0], [1.0, 0.0]),
            vertex([0.0, 1.0, 0.5, 1.0], [0.5, 1.0]),
        ];
        assert_eq!(clip_root_c3d_triangle(boundary).unwrap(), boundary);
    }

    #[test]
    fn projection_retains_perspective_interpolation_terms() {
        let projected = project_root_c3d_polygon(
            &[
                vertex([0.0, 0.0, 0.5, 1.0], [0.0, 1.0]),
                vertex([0.0, 0.0, 1.0, 2.0], [1.0, 0.5]),
            ],
            viewport(),
        )
        .unwrap();

        assert_eq!(projected[0].reciprocal_w(), 1.0);
        assert_eq!(projected[0].uv_over_w(), [0.0, 1.0]);
        assert_eq!(projected[1].reciprocal_w(), 0.5);
        assert_eq!(projected[1].uv_over_w(), [0.5, 0.25]);
    }

    #[test]
    fn clips_in_homogeneous_space_before_divide() {
        let inside = vertex([0.0, 0.0, 0.5, 1.0], [0.0, 0.0]);
        let outside = vertex([-3.0, 1.0, 1.0, 2.0], [1.0, 1.0]);
        let polygon = clip_root_c3d_triangle([outside, inside, inside]).unwrap();
        let generated = polygon
            .iter()
            .copied()
            .find(|candidate| {
                let [x, _, _, w] = candidate.clip_position();
                x == -w && candidate.uv() == [0.5, 0.5]
            })
            .unwrap();
        let projected = project_root_c3d_polygon(&[generated], viewport()).unwrap()[0];

        assert_eq!(generated.clip_position(), [-1.5, 0.5, 0.75, 1.5]);
        assert_near(projected.reciprocal_w(), 2.0 / 3.0);
        assert_near(projected.uv_over_w()[0], 1.0 / 3.0);
        assert_near(projected.uv_over_w()[1], 1.0 / 3.0);
    }

    #[test]
    fn indexed_preflight_fans_clipped_polygon_and_preserves_flat_source_color() {
        let aspect = 640.0 / 480.0;
        let vertices = [
            raw([-1.5, -0.5 * aspect, 0.5], 0x9611_2233, [0.0, 0.0]),
            raw([0.0, 0.5 * aspect, 0.5], 0x9644_5566, [0.5, 1.0]),
            raw([0.5, -0.5 * aspect, 0.5], 0x9677_8899, [1.0, 0.0]),
        ];

        let batch =
            preflight_indexed_triangle_list(&vertices, &[0, 1, 2], identity_state()).unwrap();

        assert_eq!(batch.source_triangle_count(), 1);
        assert_eq!(batch.triangles().len(), 2);
        assert_eq!(batch.fully_clipped_source_triangle_count(), 0);
        assert_eq!(batch.counter_clockwise_triangle_count(), 0);
        assert_eq!(batch.degenerate_triangle_count(), 0);
        assert!(
            batch
                .triangles()
                .iter()
                .all(|triangle| triangle.flat_diffuse() == 0x9611_2233)
        );
    }

    #[test]
    fn indexed_preflight_culls_screen_counter_clockwise_and_discards_degenerate() {
        let aspect = 640.0 / 480.0;
        let vertices = [
            raw([-0.5, -0.5 * aspect, 0.5], 1, [0.0, 0.0]),
            raw([0.0, 0.5 * aspect, 0.5], 2, [0.5, 1.0]),
            raw([0.5, -0.5 * aspect, 0.5], 3, [1.0, 0.0]),
        ];
        let indices = [0, 1, 2, 0, 2, 1, 0, 0, 0];

        let batch = preflight_indexed_triangle_list(&vertices, &indices, identity_state()).unwrap();

        assert_eq!(batch.source_triangle_count(), 3);
        assert_eq!(batch.triangles().len(), 1);
        assert_eq!(batch.counter_clockwise_triangle_count(), 1);
        assert_eq!(batch.degenerate_triangle_count(), 1);
    }

    #[test]
    fn boundary_crossing_does_not_create_a_duplicate_fan_vertex() {
        let aspect = 640.0 / 480.0;
        let mut outside = vertex([-2.0, 1.0, 0.5, 1.0], [1.0, 1.0]);
        outside.diffuse_argb = unpack_argb(0xff01_0000);
        let mut boundary = vertex([-1.0, 0.0, 0.5, 1.0], [f32::MIN_POSITIVE, 0.123]);
        boundary.diffuse_argb = unpack_argb(0xff03_0000);
        let inside = vertex([0.0, -0.5, 0.5, 1.0], [0.5, 0.0]);
        let clipped = clip_root_c3d_triangle([outside, boundary, inside]).unwrap();
        assert_eq!(clipped.len(), 3);
        assert_eq!(clipped[1], boundary);

        let vertices = [
            raw([-1.0, 0.0, 0.5], 1, [0.0, 0.0]),
            raw([-2.0, aspect, 0.5], 2, [0.0, 1.0]),
            raw([0.0, -0.5 * aspect, 0.5], 3, [1.0, 0.0]),
        ];

        let batch =
            preflight_indexed_triangle_list(&vertices, &[0, 1, 2], identity_state()).unwrap();

        assert_eq!(batch.triangles().len(), 1);
        assert_eq!(batch.degenerate_triangle_count(), 0);
        assert_eq!(batch.counter_clockwise_triangle_count(), 0);
    }

    #[test]
    fn indexed_preflight_reports_malformed_indices_without_panicking() {
        let empty = preflight_indexed_triangle_list(&[], &[], identity_state()).unwrap();
        assert!(empty.triangles().is_empty());
        assert_eq!(empty.source_triangle_count(), 0);
        assert_eq!(empty.fully_clipped_source_triangle_count(), 0);
        assert_eq!(empty.counter_clockwise_triangle_count(), 0);
        assert_eq!(empty.degenerate_triangle_count(), 0);

        let one = [raw([0.0, 0.0, 0.5], 0, [0.0, 0.0])];
        assert_eq!(
            preflight_indexed_triangle_list(&one, &[0, 0], identity_state()),
            Err(SoftwareC3dRasterError::IncompleteIndexTriplet { index_count: 2 })
        );
        assert_eq!(
            preflight_indexed_triangle_list(&one, &[0, 1, 0], identity_state()),
            Err(SoftwareC3dRasterError::IndexOutOfRange {
                triangle: 0,
                corner: 1,
                index: 1,
                vertex_count: 1,
            })
        );
        assert_eq!(
            preflight_indexed_triangle_list(&[], &[0, 0, 0], identity_state()),
            Err(SoftwareC3dRasterError::IndexOutOfRange {
                triangle: 0,
                corner: 0,
                index: 0,
                vertex_count: 0,
            })
        );
    }

    #[test]
    fn non_finite_inputs_and_nonpositive_w_are_rejected() {
        let error = transform_vertex(
            vertex([f32::NAN, 0.0, 0.0, 1.0], [0.0, 0.0]),
            identity(),
            identity(),
            identity(),
            viewport(),
        )
        .unwrap_err();
        assert_eq!(error, SoftwareC3dRasterError::NonFiniteInput);

        for w in [0.0, -1.0] {
            let error =
                project_root_c3d_polygon(&[vertex([0.0, 0.0, 0.0, w], [0.0, 0.0])], viewport())
                    .unwrap_err();
            assert_eq!(error, SoftwareC3dRasterError::NonPositiveHomogeneousW);
        }
    }
}
