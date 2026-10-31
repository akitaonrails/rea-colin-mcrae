//! HD images in the front end (F27): what a noted blit keeps (see
//! `fe_compose`, which composes the frame from them).

/// A rectangle: x, y, width, height.
pub type Rect = [i32; 4];

/// One blit of the frame: where it went, and whether it has a twin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Draw {
    pub dst: Rect,
    pub twin: bool,
}

/// A rectangle of an image `from` (w, h) in pixels of its twin `to`.
pub fn scale(r: Rect, from: (i32, i32), to: (i32, i32)) -> Rect {
    let x = |v: i32| (v as i64 * to.0 as i64 / from.0.max(1) as i64) as i32;
    let y = |v: i32| (v as i64 * to.1 as i64 / from.1.max(1) as i64) as i32;
    [
        x(r[0]),
        y(r[1]),
        x(r[0] + r[2]) - x(r[0]),
        y(r[1] + r[3]) - y(r[1]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twin_rectangles() {
        assert_eq!(
            scale([10, 4, 30, 8], (182, 128), (728, 512)),
            [40, 16, 120, 32]
        );
        assert_eq!(
            scale([640, 240, 640, 480], (2244, 960), (6732, 2880)),
            [1920, 720, 1920, 1440]
        );
    }
}
