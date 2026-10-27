//! Pure construction of the live car geometry consumed by static collision.
//!
//! The official-EU mechanics path snapshots the car's X/Z position before
//! integration, rebuilds its eight-point hull after integration, selects the
//! four active spatial cells, temporarily deforms the upper four hull points,
//! and lazily builds a horizontal projection cache. This module preserves that
//! ordering without binding it to the native car layout or global caches.
//!
//! `FUN_0048fd20`, which finds the primary cell from stage spatial tables, is
//! outside this contract. Callers provide that primary cell and its X/Z record
//! center; [`derive_active_cells`] exactly reproduces the recovered neighbor
//! expansion performed by `FUN_00490e80`.

use crate::{
    c3d_terrain_cull::{CellGrid, MAX_CELLS},
    compiled_car_initializer::normalize_compiled_lane_vector,
    fixed_point::{Fixed16, Vec3},
};

/// Collision reach used by `FUN_00490e80` for neighbor-cell admission.
pub const ACTIVE_CELL_REACH: Fixed16 = Fixed16::from_raw(0x0004_8000);

/// The car position captured before the primary pose integrator runs.
///
/// These values correspond to native car fields `+0x3e4/+0x3ec`. They belong
/// to the live car, not to a static-scene candidate or cell-list wrapper.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreviousCarXz {
    pub x: Fixed16,
    pub z: Fixed16,
}

impl PreviousCarXz {
    /// Capture the provenance-correct pre-integration X/Z snapshot.
    #[must_use]
    pub const fn capture(position: Vec3) -> Self {
        Self {
            x: position.x,
            z: position.z,
        }
    }

    #[must_use]
    pub const fn as_array(self) -> [Fixed16; 2] {
        [self.x, self.z]
    }
}

/// Build the post-integration eight-point car hull in `FUN_00409c10` order.
///
/// Each Q16 multiply and each wrapping add/subtract occurs in native order;
/// the expressions are intentionally not algebraically reassociated.
#[must_use]
pub fn build_live_car_hull(center: Vec3, basis: [Vec3; 3], half_extents: Vec3) -> [Vec3; 8] {
    let a = basis[0].scale(half_extents.x);
    let b = basis[1].scale(half_extents.y);
    let c = basis[2].scale(half_extents.z);
    [
        center.add(a).sub(b).add(c),
        center.add(a).sub(b).sub(c),
        center.sub(a).sub(b).add(c),
        center.sub(a).sub(b).sub(c),
        center.add(a).add(b).add(c),
        center.add(a).add(b).sub(c),
        center.sub(a).add(b).add(c),
        center.sub(a).add(b).sub(c),
    ]
}

/// Inputs to the collision-only upper-hull deformation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UpperHullDeformation {
    /// Native car vector at `+0x258`; it supplies both D and F.
    pub axis_258: Vec3,
    /// Native car vector at `+0x270`; it supplies E.
    pub axis_270: Vec3,
    /// Native scalar at `+0x638`.
    pub scale_638: Fixed16,
    /// Native scalar at `+0x63c`.
    pub scale_63c: Fixed16,
    /// Native scalar at `+0x640`.
    pub scale_640: Fixed16,
}

impl UpperHullDeformation {
    const fn scaled_vectors(self) -> (Vec3, Vec3, Vec3) {
        (
            self.axis_258.scale(self.scale_638),
            self.axis_270.scale(self.scale_640),
            self.axis_258.scale(self.scale_63c),
        )
    }
}

/// Apply `FUN_00417f60` to points 4..7, leaving points 0..3 untouched.
#[must_use]
pub fn apply_upper_hull_deformation(
    mut points: [Vec3; 8],
    deformation: UpperHullDeformation,
) -> [Vec3; 8] {
    let (d, e, f) = deformation.scaled_vectors();
    points[4] = points[4].add(d).add(e);
    points[5] = points[5].add(d).sub(e);
    points[6] = points[6].add(e).sub(f);
    points[7] = points[7].sub(e).sub(f);
    points
}

/// Apply the exact `FUN_00418120` inverse to a deformed live hull.
#[must_use]
pub fn undo_upper_hull_deformation(
    mut points: [Vec3; 8],
    deformation: UpperHullDeformation,
) -> [Vec3; 8] {
    let (d, e, f) = deformation.scaled_vectors();
    points[4] = points[4].sub(d).sub(e);
    points[5] = points[5].add(e).sub(d);
    points[6] = points[6].add(f).sub(e);
    points[7] = points[7].add(e).add(f);
    points
}

/// Lazy horizontal car projection produced by `FUN_0043f250/0043f600`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveCarProjectionCache {
    pub axis: Vec3,
    pub perpendicular: Vec3,
    pub axis_radius: Fixed16,
    pub perpendicular_radius: Fixed16,
    pub min_y: Fixed16,
    pub max_y: Fixed16,
    /// World-space corners in native order: `u-v`, `u+v`, `-u-v`, `-u+v`.
    /// Every Y component is [`Self::max_y`].
    pub projected_corners: [Vec3; 4],
}

/// Build the horizontal projection cache from the collision-visible hull.
#[must_use]
pub fn build_live_car_projection(
    points: [Vec3; 8],
    center: Vec3,
    axis_258: Vec3,
    axis_264: Vec3,
) -> LiveCarProjectionCache {
    let selected = if axis_258.y.raw().wrapping_abs() > 0x0000_fd70 {
        axis_264
    } else {
        axis_258
    };
    let normalized = normalize_compiled_lane_vector([selected.x.raw(), 0, selected.z.raw()]);
    let axis = raw_vec(normalized);
    let perpendicular = Vec3::new(axis.z, Fixed16::ZERO, axis.x.neg());

    let mut axis_radius = Fixed16::ZERO;
    let mut perpendicular_radius = Fixed16::ZERO;
    let mut relative_min_y = Fixed16::ZERO;
    let mut relative_max_y = Fixed16::ZERO;
    for point in points {
        let relative = point.sub(center);
        let along_axis = axis.dot(relative);
        let along_perpendicular = perpendicular.dot(relative);
        if along_axis > axis_radius {
            axis_radius = along_axis;
        }
        if along_perpendicular > perpendicular_radius {
            perpendicular_radius = along_perpendicular;
        }
        if relative.y > relative_max_y {
            relative_max_y = relative.y;
        }
        if relative.y < relative_min_y {
            relative_min_y = relative.y;
        }
    }

    let min_y = center.y.add(relative_min_y);
    let max_y = center.y.add(relative_max_y);
    let u = axis.scale(axis_radius);
    let v = perpendicular.scale(perpendicular_radius);
    let corner = |offset: Vec3| {
        let world = center.add(offset);
        Vec3::new(world.x, max_y, world.z)
    };
    let projected_corners = [
        corner(u.sub(v)),
        corner(u.add(v)),
        corner(Vec3::ZERO.sub(u).sub(v)),
        corner(Vec3::ZERO.sub(u).add(v)),
    ];

    LiveCarProjectionCache {
        axis,
        perpendicular,
        axis_radius,
        perpendicular_radius,
        min_y,
        max_y,
        projected_corners,
    }
}

const fn raw_vec(raw: [i32; 3]) -> Vec3 {
    Vec3::new(
        Fixed16::from_raw(raw[0]),
        Fixed16::from_raw(raw[1]),
        Fixed16::from_raw(raw[2]),
    )
}

/// Inputs already resolved from the primary stage-cell record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActiveCellInput {
    pub primary: i16,
    pub position: Vec3,
    pub primary_center_x: Fixed16,
    pub primary_center_z: Fixed16,
    /// Half the authored cell spacing (`DAT_008f57a4`).
    pub half_cell: Fixed16,
    /// Signed native row stride (`DAT_00869798`).
    pub row_stride: i32,
}

/// Primary-cell metadata paired with the frame's post-integration position.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActiveCellRecord {
    pub primary: i16,
    pub primary_center_x: Fixed16,
    pub primary_center_z: Fixed16,
    pub half_cell: Fixed16,
    pub row_stride: i32,
}

/// Checked failures while binding authored cell centers to aggregate grid
/// metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActiveCellRecordBuildError {
    TooManyCellCenters {
        count: usize,
        maximum: usize,
    },
    CellCountMismatch {
        grid_count: i32,
        center_count: usize,
    },
    NoPrimaryCell,
}

impl ActiveCellRecord {
    const fn at_position(self, position: Vec3) -> ActiveCellInput {
        ActiveCellInput {
            primary: self.primary,
            position,
            primary_center_x: self.primary_center_x,
            primary_center_z: self.primary_center_z,
            half_cell: self.half_cell,
            row_stride: self.row_stride,
        }
    }
}

/// Select the authored cell nearest to `position` as `FUN_0048fd20` does.
///
/// Distances use native wrapping subtraction and branchless signed absolute
/// value. The sum order is `abs(dz) + abs(dy) + abs(dx)`, followed by signed
/// Q16 division by `0x30000`. A strict comparison preserves the first index on
/// ties. An empty slice returns the native sentinel `-1`.
#[must_use]
pub fn select_primary_cell(cell_centers: &[Vec3], position: Vec3) -> i16 {
    let mut selected = -1_i16;
    let mut best = 0x7fff_0000_i32;
    for (index, center) in cell_centers.iter().copied().enumerate() {
        let dx = wrapping_abs(position.x.raw().wrapping_sub(center.x.raw()));
        let dy = wrapping_abs(position.y.raw().wrapping_sub(center.y.raw()));
        let dz = wrapping_abs(position.z.raw().wrapping_sub(center.z.raw()));
        let sum = dz.wrapping_add(dy).wrapping_add(dx);
        let mean = ((i64::from(sum) << 16) / 0x0003_0000_i64) as i32;
        if mean < best {
            best = mean;
            selected = index as i16;
        }
    }
    selected
}

const fn wrapping_abs(value: i32) -> i32 {
    let sign = value >> 31;
    (value ^ sign).wrapping_sub(sign)
}

/// Bind exact primary selection to the metadata consumed by neighbor
/// expansion. `cell_centers` must retain main-C3D source order with the
/// container record excluded.
pub fn build_active_cell_record(
    grid: &CellGrid,
    cell_centers: &[Vec3],
    position: Vec3,
) -> Result<ActiveCellRecord, ActiveCellRecordBuildError> {
    if cell_centers.len() > MAX_CELLS {
        return Err(ActiveCellRecordBuildError::TooManyCellCenters {
            count: cell_centers.len(),
            maximum: MAX_CELLS,
        });
    }
    if usize::try_from(grid.cell_count).ok() != Some(cell_centers.len()) {
        return Err(ActiveCellRecordBuildError::CellCountMismatch {
            grid_count: grid.cell_count,
            center_count: cell_centers.len(),
        });
    }
    let primary = select_primary_cell(cell_centers, position);
    let center = cell_centers
        .get(primary as usize)
        .ok_or(ActiveCellRecordBuildError::NoPrimaryCell)?;
    Ok(ActiveCellRecord {
        primary,
        primary_center_x: center.x,
        primary_center_z: center.z,
        half_cell: Fixed16::from_raw(grid.half_cell),
        row_stride: grid.width,
    })
}

/// Publish primary, horizontal, vertical and diagonal cells in native order.
///
/// Unused slots are `-1`. All coordinate arithmetic wraps as signed i32 and
/// all cell arithmetic wraps before its low 16 bits are published.
#[must_use]
pub fn derive_active_cells(input: ActiveCellInput) -> [i16; 4] {
    let mut output = [input.primary, -1, -1, -1];
    let mut count = 1;
    let primary = i32::from(input.primary);
    let reach = ACTIVE_CELL_REACH;

    let left = input.position.x.sub(reach) < input.primary_center_x.sub(input.half_cell);
    let right = !left && input.position.x.add(reach) > input.primary_center_x.add(input.half_cell);
    let horizontal_delta = if left {
        Some(-1_i32)
    } else if right {
        Some(1_i32)
    } else {
        None
    };
    if let Some(delta) = horizontal_delta {
        output[count] = primary.wrapping_add(delta) as i16;
        count += 1;
    }

    let upper = input.position.z.add(reach) > input.primary_center_z.add(input.half_cell);
    let lower = !upper && input.position.z.sub(reach) < input.primary_center_z.sub(input.half_cell);
    let vertical_delta = if upper {
        Some(input.row_stride.wrapping_neg())
    } else if lower {
        Some(input.row_stride)
    } else {
        None
    };
    if let Some(delta) = vertical_delta {
        output[count] = primary.wrapping_add(delta) as i16;
        count += 1;
    }

    if let (Some(horizontal), Some(vertical)) = (horizontal_delta, vertical_delta) {
        output[count] = primary.wrapping_add(vertical).wrapping_add(horizontal) as i16;
    }
    output
}

/// Pure collision-frame product assembled after pose integration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveStaticCollisionCarFrame {
    pub previous_center_xz: PreviousCarXz,
    pub current_center: Vec3,
    /// The temporary, collision-visible hull. If deformation was requested,
    /// callers may restore it with [`undo_upper_hull_deformation`].
    pub collision_hull: [Vec3; 8],
    pub projection: LiveCarProjectionCache,
    pub active_cells: [i16; 4],
}

/// Inputs whose timeline has already crossed the pose-integration boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveStaticCollisionCarFrameInput {
    pub previous_center_xz: PreviousCarXz,
    pub current_center: Vec3,
    pub current_basis: [Vec3; 3],
    pub half_extents: Vec3,
    pub projection_axis_258: Vec3,
    pub projection_axis_264: Vec3,
    pub deformation: Option<UpperHullDeformation>,
    pub primary_cell: ActiveCellRecord,
}

/// Build the post-integration collision frame without performing integration
/// or primary-cell lookup.
#[must_use]
pub fn build_live_static_collision_car_frame(
    input: LiveStaticCollisionCarFrameInput,
) -> LiveStaticCollisionCarFrame {
    let base_hull = build_live_car_hull(
        input.current_center,
        input.current_basis,
        input.half_extents,
    );
    let collision_hull = input.deformation.map_or(base_hull, |value| {
        apply_upper_hull_deformation(base_hull, value)
    });
    let projection = build_live_car_projection(
        collision_hull,
        input.current_center,
        input.projection_axis_258,
        input.projection_axis_264,
    );
    LiveStaticCollisionCarFrame {
        previous_center_xz: input.previous_center_xz,
        current_center: input.current_center,
        collision_hull,
        projection,
        active_cells: derive_active_cells(input.primary_cell.at_position(input.current_center)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn f(raw: i32) -> Fixed16 {
        Fixed16::from_raw(raw)
    }

    const fn v(x: i32, y: i32, z: i32) -> Vec3 {
        Vec3::new(f(x), f(y), f(z))
    }

    const UNIT_BASIS: [Vec3; 3] = [v(0x1_0000, 0, 0), v(0, 0x1_0000, 0), v(0, 0, 0x1_0000)];

    const fn grid(width: i32, count: i32) -> CellGrid {
        CellGrid {
            width,
            height: if width == 0 { 0 } else { count / width },
            cell_count: count,
            cell_size: 0x20_0000,
            min_x: 0,
            half_cell: 0x10_0000,
            max_z: 0,
        }
    }

    #[test]
    fn hull_uses_native_point_order() {
        let points = build_live_car_hull(v(100, 200, 300), UNIT_BASIS, v(10, 20, 30));
        assert_eq!(
            points,
            [
                v(110, 180, 330),
                v(110, 180, 270),
                v(90, 180, 330),
                v(90, 180, 270),
                v(110, 220, 330),
                v(110, 220, 270),
                v(90, 220, 330),
                v(90, 220, 270),
            ]
        );
    }

    #[test]
    fn hull_preserves_wrapping_operation_order() {
        let points = build_live_car_hull(
            v(i32::MAX, 0, 0),
            [v(0x1_0000, 0, 0), v(0x1_0000, 0, 0), v(0x1_0000, 0, 0)],
            v(1, 2, 4),
        );
        assert_eq!(points[0].x.raw(), i32::MIN.wrapping_add(2));
        assert_eq!(points[3].x.raw(), i32::MAX.wrapping_sub(7));
    }

    #[test]
    fn hull_q16_multiply_uses_arithmetic_shift_for_negative_fraction() {
        let points = build_live_car_hull(
            Vec3::ZERO,
            [v(-1, 0, 0), Vec3::ZERO, Vec3::ZERO],
            v(1, 0, 0),
        );
        assert_eq!(points[0].x.raw(), -1);
        assert_eq!(points[2].x.raw(), 1);
    }

    #[test]
    fn deformation_only_changes_upper_points_and_inverse_is_exact() {
        let original =
            std::array::from_fn(|index| v(index as i32, -(index as i32), 2 * index as i32));
        let deformation = UpperHullDeformation {
            axis_258: v(i32::MAX, 0x20000, -0x30000),
            axis_270: v(-0x40000, i32::MIN, 0x60000),
            scale_638: f(0x18000),
            scale_63c: f(-0x14000),
            scale_640: f(0x28000),
        };
        let deformed = apply_upper_hull_deformation(original, deformation);
        assert_eq!(&deformed[..4], &original[..4]);
        assert_ne!(&deformed[4..], &original[4..]);
        assert_eq!(undo_upper_hull_deformation(deformed, deformation), original);
        assert_ne!(deformed, original, "a collision snapshot remains deformed");
    }

    #[test]
    fn projection_axis_threshold_is_strict_and_wrapping_abs_is_native() {
        let points = build_live_car_hull(Vec3::ZERO, UNIT_BASIS, v(0x10000, 0x10000, 0x10000));
        let axis_264 = v(0, 0, 0x10000);
        let equal = build_live_car_projection(points, Vec3::ZERO, v(0x10000, 0xfd70, 0), axis_264);
        assert!(equal.axis.x.raw() > 0);
        assert_eq!(equal.axis.z, Fixed16::ZERO);
        let greater =
            build_live_car_projection(points, Vec3::ZERO, v(0x10000, 0xfd71, 0), axis_264);
        assert_eq!(greater.axis.x, Fixed16::ZERO);
        assert!(greater.axis.z.raw() > 0);
        let int_min =
            build_live_car_projection(points, Vec3::ZERO, v(0x10000, i32::MIN, 0), axis_264);
        assert!(int_min.axis.x.raw() > 0);
        assert_eq!(int_min.axis.z, Fixed16::ZERO);
    }

    #[test]
    fn projection_extrema_include_center_and_corner_order() {
        let center = v(0x10_0000, 0x20_0000, 0x30_0000);
        let points = [
            v(0x11_0000, 0x21_0000, 0x31_0000),
            v(0x12_0000, 0x22_0000, 0x32_0000),
            v(0x0f_0000, 0x23_0000, 0x2f_0000),
            v(0x10_0000, 0x24_0000, 0x30_0000),
            v(0x10_0000, 0x21_0000, 0x30_0000),
            v(0x10_0000, 0x22_0000, 0x30_0000),
            v(0x10_0000, 0x23_0000, 0x30_0000),
            v(0x10_0000, 0x24_0000, 0x30_0000),
        ];
        let cache = build_live_car_projection(points, center, v(0x10000, 0, 0), v(0, 0, 0x10000));
        assert_eq!(cache.min_y, center.y);
        assert_eq!(cache.max_y, f(0x24_0000));
        assert_eq!(cache.axis_radius, f(0x1_ffe4));
        assert_eq!(cache.perpendicular_radius, f(0xfff2));
        let u = cache.axis.scale(cache.axis_radius);
        let perpendicular = cache.perpendicular.scale(cache.perpendicular_radius);
        let projected = |offset: Vec3| {
            let world = center.add(offset);
            Vec3::new(world.x, cache.max_y, world.z)
        };
        assert_eq!(
            cache.projected_corners,
            [
                projected(u.sub(perpendicular)),
                projected(u.add(perpendicular)),
                projected(Vec3::ZERO.sub(u).sub(perpendicular)),
                projected(Vec3::ZERO.sub(u).add(perpendicular)),
            ]
        );
    }

    #[test]
    fn projection_radii_ignore_non_positive_values() {
        let center = v(0x10000, 0x20000, 0x30000);
        let point = center.add(v(-0x10000, -0x20000, 0x10000));
        let cache =
            build_live_car_projection([point; 8], center, v(0x10000, 0, 0), v(0, 0, 0x10000));
        assert_eq!(cache.axis_radius, Fixed16::ZERO);
        assert_eq!(cache.perpendicular_radius, Fixed16::ZERO);
        assert_eq!(cache.max_y, center.y);
        assert_eq!(cache.min_y, point.y);
        assert_eq!(cache.projected_corners, [center; 4]);
    }

    fn cells(position: Vec3) -> ActiveCellInput {
        ActiveCellInput {
            primary: 100,
            position,
            primary_center_x: f(0),
            primary_center_z: f(0),
            half_cell: f(0x10_0000),
            row_stride: 10,
        }
    }

    #[test]
    fn active_cells_use_strict_thresholds_and_native_order() {
        assert_eq!(derive_active_cells(cells(Vec3::ZERO)), [100, -1, -1, -1]);
        assert_eq!(
            derive_active_cells(cells(v(-0xb_8001, 0, 0))),
            [100, 99, -1, -1]
        );
        assert_eq!(
            derive_active_cells(cells(v(0xb_8001, 0, 0))),
            [100, 101, -1, -1]
        );
        assert_eq!(
            derive_active_cells(cells(v(0, 0, 0xb_8001))),
            [100, 90, -1, -1]
        );
        assert_eq!(
            derive_active_cells(cells(v(0, 0, -0xb_8001))),
            [100, 110, -1, -1]
        );
        assert_eq!(
            derive_active_cells(cells(v(-0xb_8001, 0, 0xb_8001))),
            [100, 99, 90, 89]
        );
        assert_eq!(
            derive_active_cells(cells(v(0xb_8001, 0, 0xb_8001))),
            [100, 101, 90, 91]
        );
        assert_eq!(
            derive_active_cells(cells(v(-0xb_8001, 0, -0xb_8001))),
            [100, 99, 110, 109]
        );
        assert_eq!(
            derive_active_cells(cells(v(0xb_8001, 0, -0xb_8001))),
            [100, 101, 110, 111]
        );
        assert_eq!(
            derive_active_cells(cells(v(-0xb_8000, 0, 0xb_8000))),
            [100, -1, -1, -1]
        );
        assert_eq!(
            derive_active_cells(cells(v(0xb_8000, 0, -0xb_8000))),
            [100, -1, -1, -1]
        );
    }

    #[test]
    fn active_cell_coordinates_and_ids_wrap_before_comparison_and_publication() {
        let input = ActiveCellInput {
            primary: i16::MAX,
            position: v(i32::MAX, 0, i32::MAX),
            primary_center_x: f(i32::MIN),
            primary_center_z: f(i32::MIN),
            half_cell: f(1),
            row_stride: i32::MIN,
        };
        assert_eq!(derive_active_cells(input), [i16::MAX, 32766, 32767, 32766]);

        let mut id_wrap = cells(v(0xb_8001, 0, 0));
        id_wrap.primary = i16::MAX;
        assert_eq!(derive_active_cells(id_wrap), [i16::MAX, i16::MIN, -1, -1]);
    }

    #[test]
    fn primary_selection_uses_authored_y_and_first_wins_ties() {
        let y_sensitive = [v(0, 0x100_0000, 0), v(0x10_0000, 0, 0)];
        assert_eq!(select_primary_cell(&y_sensitive, Vec3::ZERO), 1);

        let tied = [v(-0x10_0000, 0, 0), v(0x10_0000, 0, 0)];
        assert_eq!(select_primary_cell(&tied, Vec3::ZERO), 0);
    }

    #[test]
    fn primary_selection_preserves_int_min_wrapping_abs() {
        let position = v(i32::MIN, 0, 0);
        let centers = [Vec3::ZERO, position];
        assert_eq!(select_primary_cell(&centers, position), 0);
    }

    #[test]
    fn primary_selection_empty_and_bridge_alignment_errors_are_explicit() {
        assert_eq!(select_primary_cell(&[], Vec3::ZERO), -1);
        assert_eq!(
            build_active_cell_record(&grid(2, 2), &[Vec3::ZERO], Vec3::ZERO),
            Err(ActiveCellRecordBuildError::CellCountMismatch {
                grid_count: 2,
                center_count: 1,
            })
        );
        assert_eq!(
            build_active_cell_record(&grid(0, 0), &[], Vec3::ZERO),
            Err(ActiveCellRecordBuildError::NoPrimaryCell)
        );
    }

    #[test]
    fn primary_bridge_retains_selected_authored_xz_and_grid_metadata() {
        let centers = [
            v(0, 0, 0),
            v(0x20_0000, 0, 0),
            v(0, 0x30_0000, -0x20_0000),
            v(0x20_0000, 0, -0x20_0000),
        ];
        let position = v(1, 0x30_0000, -0x20_0000);
        assert_eq!(
            build_active_cell_record(&grid(2, 4), &centers, position),
            Ok(ActiveCellRecord {
                primary: 2,
                primary_center_x: f(0),
                primary_center_z: f(-0x20_0000),
                half_cell: f(0x10_0000),
                row_stride: 2,
            })
        );
    }

    #[test]
    fn primary_bridge_accepts_4096_cells_and_rejects_one_more() {
        let mut maximum = vec![v(0x10_0000, 0, 0); MAX_CELLS];
        maximum[MAX_CELLS - 1] = Vec3::ZERO;
        assert_eq!(
            build_active_cell_record(&grid(64, MAX_CELLS as i32), &maximum, Vec3::ZERO)
                .unwrap()
                .primary,
            4095
        );

        let too_many = vec![Vec3::ZERO; MAX_CELLS + 1];
        assert_eq!(
            build_active_cell_record(&grid(64, (MAX_CELLS + 1) as i32), &too_many, Vec3::ZERO,),
            Err(ActiveCellRecordBuildError::TooManyCellCenters {
                count: MAX_CELLS + 1,
                maximum: MAX_CELLS,
            })
        );
    }

    #[test]
    fn frame_keeps_preintegration_reference_distinct_from_current_geometry() {
        let old_position = v(0x1_0000, 0x2_0000, 0x3_0000);
        let current_center = v(0x9_0000, 0xa_0000, 0xb_0000);
        let previous = PreviousCarXz::capture(old_position);
        let deformation = UpperHullDeformation {
            axis_258: UNIT_BASIS[0],
            axis_270: UNIT_BASIS[2],
            scale_638: f(0x8000),
            scale_63c: f(0x4000),
            scale_640: f(0x2000),
        };
        let frame = build_live_static_collision_car_frame(LiveStaticCollisionCarFrameInput {
            previous_center_xz: previous,
            current_center,
            current_basis: UNIT_BASIS,
            half_extents: v(0x10000, 0x10000, 0x10000),
            projection_axis_258: UNIT_BASIS[0],
            projection_axis_264: UNIT_BASIS[2],
            deformation: Some(deformation),
            primary_cell: ActiveCellRecord {
                primary: 7,
                primary_center_x: current_center.x,
                primary_center_z: current_center.z,
                half_cell: f(0x10_0000),
                row_stride: 4,
            },
        });
        let base = build_live_car_hull(current_center, UNIT_BASIS, v(0x10000, 0x10000, 0x10000));
        assert_eq!(
            frame.previous_center_xz.as_array(),
            [old_position.x, old_position.z]
        );
        assert_eq!(frame.current_center, current_center);
        assert_eq!(
            frame.collision_hull,
            apply_upper_hull_deformation(base, deformation)
        );
        assert_eq!(
            undo_upper_hull_deformation(frame.collision_hull, deformation),
            base
        );
        assert_eq!(frame.active_cells, [7, -1, -1, -1]);
    }
}
