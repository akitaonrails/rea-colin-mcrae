//! The HUD canvas: a virtual 640x480 screen mapped onto the real one.

/// The virtual screen the HUD is laid out and drawn on.
pub const VIRTUAL: (i32, i32) = (640, 480);

/// The game's own resolutions keep their native HUD layout.
pub fn is_native(w: i32, h: i32) -> bool {
    matches!((w, h), (320, 240) | (512, 384) | (640, 480) | (800, 600))
}

/// The game's 2D rectangle: x, y, width, height in pixels.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i16,
    pub y: i16,
    pub w: i16,
    pub h: i16,
}

/// Maps virtual-screen pixels onto a `real` screen with a uniform scale
/// `real_h / 480`. On a screen wider than 4:3 the extra width goes to the
/// sides: pieces centred in the left 30% of the virtual screen stay at the
/// left edge, pieces in the right 30% at the right edge, and the rest (the
/// middle, where centred text and the countdown sit) stays centred.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Canvas {
    scale: f32,
    /// Horizontal offsets for left-, centre- and right-anchored pieces.
    left: f32,
    centre: f32,
    right: f32,
}

impl Canvas {
    pub fn new(real_w: i32, real_h: i32) -> Canvas {
        let scale = real_h as f32 / VIRTUAL.1 as f32;
        let spare = real_w as f32 - VIRTUAL.0 as f32 * scale;
        Canvas {
            scale,
            left: 0.0,
            centre: spare / 2.0,
            right: spare,
        }
    }

    /// Edges are mapped and rounded separately, so neighbouring pieces stay
    /// joined without gaps.
    pub fn map(&self, r: Rect) -> Rect {
        let side = VIRTUAL.0 as f32 * 0.3;
        let centre_x = r.x as f32 + r.w as f32 / 2.0;
        let offset = if centre_x < side {
            self.left
        } else if centre_x > VIRTUAL.0 as f32 - side {
            self.right
        } else {
            self.centre
        };
        let x0 = self.x(offset, r.x as f32);
        let x1 = self.x(offset, r.x as f32 + r.w as f32);
        let y0 = self.y(r.y as f32);
        let y1 = self.y(r.y as f32 + r.h as f32);
        Rect {
            x: x0,
            y: y0,
            w: x1 - x0,
            h: y1 - y0,
        }
    }

    fn x(&self, offset: f32, v: f32) -> i16 {
        round(offset + v * self.scale)
    }

    fn y(&self, v: f32) -> i16 {
        round(v * self.scale)
    }
}

/// Round half away from zero (no `std` in the DLL).
fn round(v: f32) -> i16 {
    (if v < 0.0 { v - 0.5 } else { v + 0.5 }) as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: i16, y: i16, w: i16, h: i16) -> Rect {
        Rect { x, y, w, h }
    }

    #[test]
    fn four_by_three_scales_uniformly() {
        let c = Canvas::new(1440, 1080);
        assert_eq!(c.map(r(0, 0, 640, 480)), r(0, 0, 1440, 1080));
        // The rev dial's 640x480 position lands in the same place, scaled.
        assert_eq!(c.map(r(487, 316, 147, 157)), r(1096, 711, 331, 353));
    }

    #[test]
    fn wide_screens_push_side_pieces_to_the_edges() {
        let c = Canvas::new(1920, 1080);
        // The timer (bottom left) stays at the left edge.
        assert_eq!(c.map(r(30, 430, 100, 20)), r(68, 968, 225, 45));
        // The rev dial (bottom right) goes to the right edge.
        assert_eq!(c.map(r(487, 316, 147, 157)), r(1576, 711, 331, 353));
        // A centred message stays centred.
        let m = c.map(r(270, 200, 100, 20));
        assert_eq!(m.x + m.w / 2, 1920 / 2);
        let c = Canvas::new(3840, 2160);
        assert_eq!(c.map(r(0, 0, 640, 480)), r(480, 0, 2880, 2160));
    }

    #[test]
    fn neighbours_stay_joined() {
        let c = Canvas::new(1366, 768);
        let a = c.map(r(0, 0, 33, 10));
        let b = c.map(r(33, 0, 33, 10));
        assert_eq!(a.x + a.w, b.x);
    }

    #[test]
    fn native_sizes_are_left_alone() {
        assert!(is_native(640, 480) && is_native(800, 600));
        assert!(!is_native(1440, 1080) && !is_native(1920, 1080));
    }
}
