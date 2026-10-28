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

/// Maps virtual-screen pixels onto a 4:3 canvas centred on a `real` screen:
/// uniform scale `real_h / 480`, horizontal offset centring the canvas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Canvas {
    scale: f32,
    offset_x: f32,
}

impl Canvas {
    pub fn new(real_w: i32, real_h: i32) -> Canvas {
        let scale = real_h as f32 / VIRTUAL.1 as f32;
        let offset_x = (real_w as f32 - VIRTUAL.0 as f32 * scale) / 2.0;
        Canvas { scale, offset_x }
    }

    /// Edges are mapped and rounded separately, so neighbouring pieces stay
    /// joined without gaps.
    pub fn map(&self, r: Rect) -> Rect {
        let x0 = self.x(r.x as f32);
        let x1 = self.x(r.x as f32 + r.w as f32);
        let y0 = self.y(r.y as f32);
        let y1 = self.y(r.y as f32 + r.h as f32);
        Rect {
            x: x0,
            y: y0,
            w: x1 - x0,
            h: y1 - y0,
        }
    }

    fn x(&self, v: f32) -> i16 {
        round(self.offset_x + v * self.scale)
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
    fn wide_screens_centre_a_four_by_three_canvas() {
        let c = Canvas::new(1920, 1080);
        assert_eq!(c.map(r(0, 0, 640, 480)), r(240, 0, 1440, 1080));
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
