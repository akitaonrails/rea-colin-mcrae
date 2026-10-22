//! C3D terrain-cell visibility grid cull (reconstruction item E05, feeding the
//! E06/E07 scene dispatcher).
//!
//! # What this recovers
//!
//! The world is partitioned into a rectangular grid of terrain cells. Each
//! frame `FUN_00490040` rebuilds the **visible-cell list** (`DAT_00837788[]`,
//! count `DAT_008f59cc`) that the dispatcher [`crate::c3d_scene_graph`]
//! (`FUN_004a07d0`) iterates per object category, together with a **4096-bit
//! cell visibility mask** (`DAT_008f57b4`, 128 dwords). It:
//!
//! 1. finds the camera's cell (`FUN_00490af0`),
//! 2. scans a clamped box of cells of half-extent `radius / cell_size` around
//!    it,
//! 3. always keeps the 3x3 neighborhood of the camera's cell,
//! 4. for the rest, keeps a cell when any of its four corners lies inside the
//!    view-cone triangle (`FUN_00490800`, apex = camera, two far corners from
//!    the camera forward direction),
//! 5. records each visible cell's squared XZ distance to the camera and sets
//!    its visibility bit.
//!
//! The cull is the same 2D top-down XZ geometry as [`crate::c3d_camera`]: the
//! cell-corner test `FUN_00490800` is the identical three-edge predicate as
//! [`crate::c3d_camera::point_in_triangle_xz`] (deltas pre-scaled by `1/100` in
//! the shipped code to fit 32-bit; the i64 predicate here keeps the identical
//! sign), with a bounding-box pre-reject that only affects speed. The grid
//! dimensions, cell size, and world anchors are per-track runtime data, taken
//! as parameters (never vendored).
//!
//! See evidence `rea.official-eu.c3d-terrain-cull`. Everything here is pure and
//! backend-neutral.

use crate::c3d_camera::point_in_triangle_xz;

/// Number of `u32` words in the cell visibility mask (`DAT_008f57b4`); 128
/// words cover [`MAX_CELLS`] cells.
pub const CELL_MASK_DWORDS: usize = 128;
/// Maximum number of terrain cells the visibility mask can address
/// (`CELL_MASK_DWORDS * 32`).
pub const MAX_CELLS: usize = CELL_MASK_DWORDS * 32;
/// Byte size of one per-cell record in the grid arrays (`iVar13 * 0x8c`);
/// documented for the field layout, not used by this pure reconstruction.
pub const CELL_RECORD_STRIDE: usize = 0x8c;

/// The terrain-cell grid geometry and world anchors (per-track runtime data).
/// `width`/`height` are the cell counts per axis (`DAT_00869798`/`DAT_008f59b4`),
/// `cell_count` the total addressable cells (`DAT_008f57a0`), `cell_size` the
/// world units per cell (`DAT_008f57a8`), and the anchors place the grid in
/// world space (`anchor_x = DAT_00839788`, `anchor_z = DAT_008f57a4`,
/// `anchor_z2 = DAT_0083978c`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CellGrid {
    pub width: i32,
    pub height: i32,
    pub cell_count: i32,
    pub cell_size: i32,
    pub anchor_x: i32,
    pub anchor_z: i32,
    pub anchor_z2: i32,
}

impl CellGrid {
    /// Maps a world XZ point to its cell index (`FUN_00490af0`):
    /// `row = |z - anchor_z - anchor_z2| / cell_size`,
    /// `col = |x - anchor_x + anchor_z| / cell_size`,
    /// `cell = width * row + col`. An out-of-range result (or a zero cell size)
    /// clamps to `0`, matching the shipped fallback.
    pub fn cell_of_point(&self, x: i32, z: i32) -> i32 {
        if self.cell_size == 0 {
            return 0;
        }
        let row = self.axis_cell((z - self.anchor_z - self.anchor_z2).unsigned_abs());
        let col = self.axis_cell((x - self.anchor_x + self.anchor_z).unsigned_abs());
        let cell = self.width * row + col;
        if cell < 0 || self.cell_count <= cell {
            0
        } else {
            cell
        }
    }

    /// One axis's cell ordinal from an absolute world distance, reproducing the
    /// shipped `((raw << 16) / cell_size) >> 16` fixed-point reduction (which is
    /// integer division for the non-negative inputs the abs produces).
    fn axis_cell(&self, raw_abs: u32) -> i32 {
        let scaled = (i64::from(raw_abs) << 16) / i64::from(self.cell_size);
        (scaled as i32) >> 16
    }

    /// The `(row, col)` of a cell index (`cell / width`, `cell % width`).
    pub fn row_col(&self, cell: i32) -> (i32, i32) {
        (cell / self.width, cell % self.width)
    }

    /// The clamped box of cells scanned around `center_cell` for a half-extent
    /// of `radius_cells` (`FUN_00490040`): columns `[col-r-1, col+r+2)` clamped
    /// to `[0, width]`, rows `[row-r-1, row+r+2)` clamped to `[0, height]`.
    pub fn scan_box(&self, center_cell: i32, radius_cells: i32) -> ScanBox {
        let (row, col) = self.row_col(center_cell);
        ScanBox {
            col_lo: (col - radius_cells - 1).max(0),
            col_hi: (col + radius_cells + 2).min(self.width),
            row_lo: (row - radius_cells - 1).max(0),
            row_hi: (row + radius_cells + 2).min(self.height),
        }
    }
}

/// The half-open cell box `[col_lo, col_hi) x [row_lo, row_hi)` scanned for
/// visibility; iterate `cell = row * width + col`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScanBox {
    pub col_lo: i32,
    pub col_hi: i32,
    pub row_lo: i32,
    pub row_hi: i32,
}

/// The scan half-extent in cells, `(far_radius << 16) / cell_size >> 16`
/// (`FUN_00490040`): the far view radius expressed in whole cells. A zero cell
/// size yields the shipped sentinel `-1`.
pub fn radius_in_cells(far_radius: i32, cell_size: i32) -> i32 {
    if cell_size == 0 {
        return -1;
    }
    let scaled = (i64::from(far_radius) << 16) / i64::from(cell_size);
    (scaled as i32) >> 16
}

/// Whether `cell` is in the camera cell's always-visible 3x3 neighborhood
/// (`center` and its eight grid neighbors), which `FUN_00490040` keeps
/// regardless of the view-cone test.
pub fn is_in_3x3_neighborhood(cell: i32, center: i32, width: i32) -> bool {
    [
        center - width - 1,
        center - width,
        center - width + 1,
        center - 1,
        center,
        center + 1,
        center + width - 1,
        center + width,
        center + width + 1,
    ]
    .contains(&cell)
}

/// The per-cell visibility bitset (`DAT_008f57b4`): one bit per cell,
/// `mask[cell >> 5] & (1 << (cell & 31))`, covering up to [`MAX_CELLS`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CellVisibilityMask {
    bits: [u32; CELL_MASK_DWORDS],
}

impl Default for CellVisibilityMask {
    fn default() -> Self {
        Self::new()
    }
}

impl CellVisibilityMask {
    /// A mask with every cell hidden (all words zero), the per-frame start
    /// state after `FUN_00490040` clears `DAT_008f57b4`.
    pub fn new() -> Self {
        CellVisibilityMask {
            bits: [0; CELL_MASK_DWORDS],
        }
    }

    /// Clears every visibility bit.
    pub fn clear(&mut self) {
        self.bits = [0; CELL_MASK_DWORDS];
    }

    /// Marks `cell` visible (`mask[cell >> 5] |= 1 << (cell & 31)`). Cells at or
    /// beyond [`MAX_CELLS`] are ignored, matching the shipped valid-range gate.
    pub fn set(&mut self, cell: i32) {
        if let Some(word) = Self::word_index(cell) {
            self.bits[word] |= 1u32 << (cell as u32 & 31);
        }
    }

    /// Whether `cell` is currently marked visible.
    pub fn test(&self, cell: i32) -> bool {
        match Self::word_index(cell) {
            Some(word) => self.bits[word] & (1u32 << (cell as u32 & 31)) != 0,
            None => false,
        }
    }

    fn word_index(cell: i32) -> Option<usize> {
        if cell < 0 || cell as usize >= MAX_CELLS {
            None
        } else {
            Some(cell as usize >> 5)
        }
    }
}

/// Builds the view-cone triangle tested against each cell's corners
/// (`FUN_00490040` / `FUN_00490800`): the apex is the camera XZ position and
/// the two far corners are the camera forward direction rotated by +/- the
/// perpendicular, where `forward_offset = normalized_forward * far_radius`
/// (Q16.16). The vertex order matches the shipped `[apex, v1, v2]`:
/// `v1 = cam + (fx - fz, fx + fz)`, `v2 = cam + (fx + fz, fz - fx)`. The result
/// is clockwise-wound (see [`cell_corner_visible`]).
///
/// The normalization uses the shipped reciprocal-magnitude lookup table
/// (`DAT_0082b380`, shared with the object sort); the table is not vendored, so
/// callers supply the already-scaled forward offset and this builds the exact
/// corner geometry.
pub fn view_triangle(camera_xz: [i32; 2], forward_offset: [i32; 2]) -> [[i32; 2]; 3] {
    let [cx, cz] = camera_xz;
    let [fx, fz] = forward_offset;
    [
        [cx, cz],
        [cx + fx - fz, cz + fx + fz],
        [cx + fx + fz, cz + fz - fx],
    ]
}

/// Whether a cell is kept by the view-cone test: true when any of its four
/// corners lies inside the view triangle (`FUN_00490040` tests all four corners
/// through `FUN_00490800`). The corner test is
/// [`crate::c3d_camera::point_in_triangle_xz`].
///
/// [`view_triangle`] is consistently clockwise-wound, and `FUN_00490800` uses
/// the opposite edge cross-product sign to `point_in_triangle_xz` (its "inside"
/// is the clockwise interior). The two conventions agree once the triangle is
/// reversed to counter-clockwise, so the vertices are passed as
/// `[apex, v2, v1]`.
pub fn cell_corner_visible(corners: &[[i32; 2]; 4], triangle: &[[i32; 2]; 3]) -> bool {
    let tri = [
        [i64::from(triangle[0][0]), i64::from(triangle[0][1])],
        [i64::from(triangle[2][0]), i64::from(triangle[2][1])],
        [i64::from(triangle[1][0]), i64::from(triangle[1][1])],
    ];
    corners
        .iter()
        .any(|&[x, z]| point_in_triangle_xz([i64::from(x), i64::from(z)], tri))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid() -> CellGrid {
        // 8x8 grid of 10-unit cells anchored at the world origin.
        CellGrid {
            width: 8,
            height: 8,
            cell_count: 64,
            cell_size: 10,
            anchor_x: 0,
            anchor_z: 0,
            anchor_z2: 0,
        }
    }

    #[test]
    fn mask_dimensions_cover_4096_cells() {
        assert_eq!(CELL_MASK_DWORDS, 128);
        assert_eq!(MAX_CELLS, 4096);
        assert_eq!(CELL_RECORD_STRIDE, 0x8c);
    }

    #[test]
    fn cell_of_point_indexes_row_major_and_clamps_out_of_range() {
        let g = grid();
        // (x=5, z=5) -> col 0, row 0 -> cell 0.
        assert_eq!(g.cell_of_point(5, 5), 0);
        // (x=25, z=35) -> col 2, row 3 -> cell 3*8 + 2 = 26.
        assert_eq!(g.cell_of_point(25, 35), 26);
        // Negative coordinates fold through the absolute value.
        assert_eq!(g.cell_of_point(-25, -5), 2);
        // Out of range (row 100) clamps to 0.
        assert_eq!(g.cell_of_point(5, 2000), 0);
        // Zero cell size clamps to 0.
        let mut degenerate = g;
        degenerate.cell_size = 0;
        assert_eq!(degenerate.cell_of_point(25, 35), 0);
    }

    #[test]
    fn row_col_inverts_the_cell_index() {
        let g = grid();
        assert_eq!(g.row_col(26), (3, 2));
        assert_eq!(g.row_col(0), (0, 0));
        assert_eq!(g.row_col(63), (7, 7));
    }

    #[test]
    fn radius_in_cells_is_far_radius_over_cell_size() {
        assert_eq!(radius_in_cells(100, 10), 10);
        assert_eq!(radius_in_cells(35, 10), 3);
        // Zero cell size yields the shipped -1 sentinel.
        assert_eq!(radius_in_cells(100, 0), -1);
    }

    #[test]
    fn scan_box_is_centered_and_clamped_to_the_grid() {
        let g = grid();
        // Center cell 27 = (row 3, col 3); radius 1 -> cols [1,6), rows [1,6).
        let center = g.scan_box(27, 1);
        assert_eq!(
            center,
            ScanBox {
                col_lo: 1,
                col_hi: 6,
                row_lo: 1,
                row_hi: 6,
            }
        );
        // Corner cell 0 = (0,0); the low bounds clamp to 0.
        let corner = g.scan_box(0, 1);
        assert_eq!(corner.col_lo, 0);
        assert_eq!(corner.row_lo, 0);
        assert_eq!(corner.col_hi, 3);
        assert_eq!(corner.row_hi, 3);
        // Far corner clamps the high bounds to width/height.
        let far = g.scan_box(63, 2);
        assert_eq!(far.col_hi, 8);
        assert_eq!(far.row_hi, 8);
    }

    #[test]
    fn neighborhood_is_the_3x3_block_around_the_center() {
        let width = 8;
        let center = 27; // (row 3, col 3)
        for cell in [18, 19, 20, 26, 27, 28, 34, 35, 36] {
            assert!(is_in_3x3_neighborhood(cell, center, width), "cell {cell}");
        }
        // Two cells away, or off-block, are not neighbors.
        assert!(!is_in_3x3_neighborhood(29, center, width));
        assert!(!is_in_3x3_neighborhood(center - 2 * width, center, width));
    }

    #[test]
    fn visibility_mask_sets_tests_and_clears_bits() {
        let mut mask = CellVisibilityMask::new();
        assert!(!mask.test(40));
        mask.set(40);
        mask.set(4095);
        assert!(mask.test(40));
        assert!(mask.test(4095));
        assert!(!mask.test(41));
        // Out-of-range cells are ignored, not panicking.
        mask.set(-1);
        mask.set(MAX_CELLS as i32);
        assert!(!mask.test(MAX_CELLS as i32));
        mask.clear();
        assert!(!mask.test(40));
        assert!(!mask.test(4095));
    }

    #[test]
    fn view_triangle_apex_is_the_camera_and_corners_follow_the_forward_offset() {
        let tri = view_triangle([100, 200], [10, 4]);
        assert_eq!(tri[0], [100, 200]); // apex = camera
        // v1 = cam + (fx - fz, fx + fz); v2 = cam + (fx + fz, fz - fx).
        assert_eq!(tri[1], [100 + 10 - 4, 200 + 10 + 4]);
        assert_eq!(tri[2], [100 + 10 + 4, 200 + 4 - 10]);
    }

    #[test]
    fn cell_is_visible_when_any_corner_is_inside_the_view_triangle() {
        // A large forward-facing triangle around the origin.
        let triangle = view_triangle([0, 0], [100, 0]);
        // A cell straddling the triangle: one corner near the centroid is inside.
        let inside = [[40, -10], [60, -10], [60, 10], [40, 10]];
        assert!(cell_corner_visible(&inside, &triangle));
        // A cell far behind the camera: no corner inside.
        let behind = [[-100, -10], [-80, -10], [-80, 10], [-100, 10]];
        assert!(!cell_corner_visible(&behind, &triangle));
    }
}
