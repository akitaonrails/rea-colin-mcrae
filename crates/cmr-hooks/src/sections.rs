//! Which screen size the game sees, section by section (F13).
//!
//! During a race frame the game sees a virtual 640x480 screen, the size all
//! of its 2D layouts were made for, and the 2D primitives scale what it
//! draws onto the real screen. Only the code that needs the real screen
//! (display and 3D set-up, the 3D draw) runs in a "real" section, where the
//! real size is back and 2D scaling pauses.

use crate::canvas::{Canvas, VIRTUAL, is_native};

/// The game's screen size and state, as the sections see them.
pub trait Screen {
    /// The size the game's display state holds now.
    fn size(&self) -> Option<(i32, i32)>;
    fn set_size(&mut self, size: (i32, i32));
    /// The game is in a race state (loading to replay), not the front end.
    fn in_race(&self) -> bool;
}

/// A size the game has no layout for (larger than its own resolutions).
fn needs_scaling(s: (i32, i32)) -> bool {
    s.0 > 0 && s.1 > 0 && !is_native(s.0, s.1)
}

#[derive(Debug, Default)]
pub struct Sections {
    /// Nested frame/HUD sections; bit `i` of `anchored` is set when level
    /// `i` is a race HUD section (side pieces anchored to the screen edges).
    depth: u32,
    anchored: u32,
    /// Nested real sections.
    real_depth: u32,
    /// The game's real screen size, valid while `active`.
    real: (i32, i32),
    /// The game currently sees the virtual screen.
    active: bool,
}

impl Sections {
    pub const fn new() -> Sections {
        Sections {
            depth: 0,
            anchored: 0,
            real_depth: 0,
            real: (0, 0),
            active: false,
        }
    }

    /// The game sees the virtual screen (a race frame at a size it has no
    /// layout for).
    pub fn active(&self) -> bool {
        self.active
    }

    /// The mapping from the virtual screen, while 2D scaling is on.
    pub fn canvas(&self) -> Option<Canvas> {
        (self.active && self.real_depth == 0)
            .then(|| Canvas::new(self.real.0, self.real.1, self.anchored != 0))
    }

    /// In a race, shows the game the virtual screen if its real one needs
    /// scaling.
    fn activate(&mut self, screen: &mut impl Screen) {
        if self.active || self.real_depth > 0 || !screen.in_race() {
            return;
        }
        if let Some(s) = screen.size().filter(|&s| needs_scaling(s)) {
            self.real = s;
            self.active = true;
            screen.set_size(VIRTUAL);
        }
    }

    /// A frame (`anchored` false) or race HUD section begins.
    pub fn enter(&mut self, anchored: bool, screen: &mut impl Screen) {
        if self.depth < 32 && anchored {
            self.anchored |= 1 << self.depth;
        }
        self.depth += 1;
        self.activate(screen);
    }

    /// A frame or HUD section ends; after the outermost one the game sees
    /// its real screen again.
    pub fn leave(&mut self, screen: &mut impl Screen) {
        if self.depth == 0 {
            return;
        }
        self.depth -= 1;
        if self.depth < 32 {
            self.anchored &= !(1 << self.depth);
        }
        if self.depth == 0 && self.active {
            self.active = false;
            screen.set_size(self.real);
        }
    }

    /// Code that needs the real screen begins.
    pub fn real_begin(&mut self, screen: &mut impl Screen) {
        self.real_depth += 1;
        if self.real_depth == 1 && self.active {
            screen.set_size(self.real);
        }
    }

    /// Code that needs the real screen ends. If the game changed its screen
    /// size inside (a new display mode), that is the real size from now on;
    /// a size of its own needs no scaling.
    pub fn real_end(&mut self, screen: &mut impl Screen) {
        if self.real_depth == 0 {
            return;
        }
        self.real_depth -= 1;
        if self.real_depth > 0 {
            return;
        }
        if self.active {
            match screen.size() {
                Some(s) if needs_scaling(s) => {
                    self.real = s;
                    screen.set_size(VIRTUAL);
                }
                _ => self.active = false,
            }
        } else if self.depth > 0 {
            // A race display created inside a frame: scale from now on.
            self.activate(screen);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        size: (i32, i32),
        race: bool,
    }

    impl Screen for Fake {
        fn size(&self) -> Option<(i32, i32)> {
            Some(self.size)
        }
        fn set_size(&mut self, size: (i32, i32)) {
            self.size = size;
        }
        fn in_race(&self) -> bool {
            self.race
        }
    }

    fn race(size: (i32, i32)) -> Fake {
        Fake { size, race: true }
    }

    #[test]
    fn a_race_frame_sees_640x480_and_gets_its_screen_back() {
        let (mut s, mut g) = (Sections::new(), race((3840, 2160)));
        s.enter(false, &mut g);
        assert_eq!(g.size, (640, 480));
        assert!(s.canvas().is_some());
        s.leave(&mut g);
        assert_eq!(g.size, (3840, 2160));
        assert!(s.canvas().is_none());
    }

    #[test]
    fn real_sections_see_the_real_screen_without_scaling() {
        let (mut s, mut g) = (Sections::new(), race((1920, 1080)));
        s.enter(false, &mut g);
        s.real_begin(&mut g);
        assert_eq!(g.size, (1920, 1080));
        assert!(s.canvas().is_none(), "3D projected sprites are not scaled");
        s.real_begin(&mut g);
        s.real_end(&mut g);
        assert_eq!(g.size, (1920, 1080), "nested: still real");
        s.real_end(&mut g);
        assert_eq!(g.size, (640, 480));
        assert!(s.canvas().is_some());
    }

    #[test]
    fn the_front_end_and_native_sizes_are_left_alone() {
        let mut s = Sections::new();
        let mut fe = Fake {
            size: (1920, 1080),
            race: false,
        };
        s.enter(false, &mut fe);
        assert_eq!(fe.size, (1920, 1080));
        s.leave(&mut fe);
        let mut native = race((800, 600));
        s.enter(false, &mut native);
        assert_eq!(native.size, (800, 600));
        assert!(!s.active());
        s.leave(&mut native);
    }

    #[test]
    fn a_race_display_created_mid_frame_scales_from_then_on() {
        // Loading: the frame starts in the front end's 640x480.
        let (mut s, mut g) = (Sections::new(), race((640, 480)));
        s.enter(false, &mut g);
        assert!(!s.active());
        s.real_begin(&mut g);
        g.size = (2560, 1440); // display set-up
        s.real_end(&mut g);
        assert_eq!(g.size, (640, 480));
        assert!(s.active());
        s.leave(&mut g);
        assert_eq!(g.size, (2560, 1440));
    }

    #[test]
    fn back_to_the_front_end_keeps_its_640x480() {
        let (mut s, mut g) = (Sections::new(), race((1920, 1080)));
        s.enter(false, &mut g);
        s.real_begin(&mut g);
        g.size = (640, 480); // the front end's display
        s.real_end(&mut g);
        assert!(!s.active());
        s.leave(&mut g);
        assert_eq!(g.size, (640, 480), "not overwritten by the old race size");
    }

    #[test]
    fn hud_sections_anchor_only_while_open() {
        let (mut s, mut g) = (Sections::new(), race((1920, 1080)));
        s.enter(false, &mut g);
        let whole = s.canvas().unwrap();
        s.enter(true, &mut g);
        let hud = s.canvas().unwrap();
        assert_ne!(whole, hud);
        s.leave(&mut g);
        assert_eq!(s.canvas().unwrap(), whole);
        assert_eq!(g.size, (640, 480), "still inside the frame");
        s.leave(&mut g);
    }
}
