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

impl Rect {
    pub const ZERO: Rect = Rect {
        x: 0,
        y: 0,
        w: 0,
        h: 0,
    };
}

/// Maps virtual-screen pixels onto a `real` screen with a uniform scale
/// `real_h / 480`, centred. For the race HUD (`anchored`) the extra width of
/// a screen wider than 4:3 goes to the sides: pieces centred in the left 30%
/// of the virtual screen stay at the left edge, pieces in the right 30% at
/// the right edge, and the rest (the middle, where centred text and the
/// countdown sit) stays centred. Screens laid out as a whole (loading,
/// results, pause) are not anchored, so their text lines stay together.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Canvas {
    real_w: i16,
    real_h: i16,
    scale: f32,
    anchored: bool,
    /// Horizontal offsets for left-, centre- and right-anchored pieces.
    left: f32,
    centre: f32,
    right: f32,
}

impl Canvas {
    pub fn new(real_w: i32, real_h: i32, anchored: bool) -> Canvas {
        let scale = real_h as f32 / VIRTUAL.1 as f32;
        let spare = real_w as f32 - VIRTUAL.0 as f32 * scale;
        Canvas {
            real_w: real_w as i16,
            real_h: real_h as i16,
            scale,
            anchored,
            left: 0.0,
            centre: spare / 2.0,
            right: spare,
        }
    }

    /// Edges are mapped and rounded separately, so neighbouring pieces stay
    /// joined without gaps. A rectangle spanning the whole virtual width (or
    /// height) — a fade, a dimming panel, a backdrop — spans the whole real
    /// screen in that direction instead.
    pub fn map(&self, r: Rect) -> Rect {
        let full_w = r.x <= 0 && r.x as i32 + r.w as i32 >= VIRTUAL.0;
        let full_h = r.y <= 0 && r.y as i32 + r.h as i32 >= VIRTUAL.1;
        let mut out = self.map_parts(r);
        if full_w {
            out.x = 0;
            out.w = self.real_w;
        }
        if full_h {
            out.y = 0;
            out.h = self.real_h;
        }
        out
    }

    fn map_parts(&self, r: Rect) -> Rect {
        let side = VIRTUAL.0 as f32 * 0.3;
        let centre_x = r.x as f32 + r.w as f32 / 2.0;
        let offset = if !self.anchored {
            self.centre
        } else if centre_x < side {
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
        let c = Canvas::new(1440, 1080, true);
        assert_eq!(c.map(r(0, 0, 640, 480)), r(0, 0, 1440, 1080));
        assert_eq!(c.map(r(10, 10, 600, 400)), r(23, 23, 1350, 900));
        // The rev dial's 640x480 position lands in the same place, scaled.
        assert_eq!(c.map(r(487, 316, 147, 157)), r(1096, 711, 331, 353));
    }

    #[test]
    fn wide_screens_push_side_pieces_to_the_edges() {
        let c = Canvas::new(1920, 1080, true);
        // The timer (bottom left) stays at the left edge.
        assert_eq!(c.map(r(30, 430, 100, 20)), r(68, 968, 225, 45));
        // The rev dial (bottom right) goes to the right edge.
        assert_eq!(c.map(r(487, 316, 147, 157)), r(1576, 711, 331, 353));
        // A centred message stays centred.
        let m = c.map(r(270, 200, 100, 20));
        assert_eq!(m.x + m.w / 2, 1920 / 2);
    }

    #[test]
    fn full_screen_rectangles_cover_the_real_screen() {
        let c = Canvas::new(1920, 1080, true);
        assert_eq!(c.map(r(0, 0, 640, 480)), r(0, 0, 1920, 1080));
        // A full-width bar keeps its height, spans the real width.
        assert_eq!(c.map(r(0, 100, 640, 40)), r(0, 225, 1920, 90));
    }

    #[test]
    fn whole_screens_stay_together() {
        let c = Canvas::new(1920, 1080, false);
        // Two halves of a text line on the right stay side by side.
        let a = c.map(r(400, 150, 50, 10));
        let b = c.map(r(450, 150, 50, 10));
        assert_eq!((a.x, a.x + a.w), (1140, 1253));
        assert_eq!(a.x + a.w, b.x);
        // A backdrop still covers the screen.
        assert_eq!(c.map(r(0, 0, 640, 480)), r(0, 0, 1920, 1080));
    }

    #[test]
    fn neighbours_stay_joined() {
        let c = Canvas::new(1366, 768, true);
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
