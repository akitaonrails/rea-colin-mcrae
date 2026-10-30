//! Which screen size the game sees (F13).
//!
//! In a race at a size the game has no layout for, its display state holds
//! a virtual 640x480 screen, the size all of its 2D was made for; the 2D
//! primitives scale what it draws onto the real screen. The game's update
//! code runs on a multimedia-timer thread while the main thread renders, so
//! the virtual size stays in place for the whole race. The display and 3D
//! set-up (while a stage loads or the menus return, with no update thread)
//! run in "real" sections that briefly hold the real size; per-frame 3D code
//! reads the real size from a copy instead (see `exports`), so nothing
//! waits and the update thread never sees the real size.

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
    /// The game's display state holds the virtual screen; `real` is the
    /// real one.
    active: bool,
    real: (i32, i32),
    /// Nested real sections.
    real_depth: u32,
    /// Nested race HUD sections (side pieces anchored to the screen edges).
    hud_depth: u32,
}

impl Sections {
    pub const fn new() -> Sections {
        Sections {
            active: false,
            real: (0, 0),
            real_depth: 0,
            hud_depth: 0,
        }
    }

    /// The game sees the virtual screen.
    pub fn active(&self) -> bool {
        self.active
    }

    /// The real screen size, while the game sees the virtual one.
    pub fn real_size(&self) -> Option<(i32, i32)> {
        self.active.then_some(self.real)
    }

    /// The mapping from the virtual screen, while 2D scaling is on.
    /// `anchored` asks for the race HUD's mapping when a HUD section is open.
    pub fn canvas(&self, anchored: bool) -> Option<Canvas> {
        (self.active && self.real_depth == 0)
            .then(|| Canvas::new(self.real.0, self.real.1, anchored && self.hud_depth > 0))
    }

    fn activate(&mut self, screen: &mut impl Screen) {
        if let Some(s) = screen.size().filter(|&s| needs_scaling(s)) {
            self.real = s;
            self.active = true;
            screen.set_size(VIRTUAL);
        }
    }

    /// Before each game update: entering a race at a size that needs
    /// scaling shows the virtual screen; leaving it shows the real one.
    pub fn update(&mut self, screen: &mut impl Screen) {
        if self.real_depth > 0 {
            return;
        }
        match (self.active, screen.in_race()) {
            (false, true) => self.activate(screen),
            (true, false) => {
                self.active = false;
                screen.set_size(self.real);
            }
            _ => {}
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
        self.active = false;
        if screen.in_race() {
            self.activate(screen);
        }
    }

    /// A race HUD layout or draw call begins or ends.
    pub fn hud_begin(&mut self) {
        self.hud_depth += 1;
    }

    pub fn hud_end(&mut self) {
        self.hud_depth = self.hud_depth.saturating_sub(1);
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
    fn a_race_keeps_the_virtual_screen_between_updates() {
        let (mut s, mut g) = (Sections::new(), race((3840, 2160)));
        s.update(&mut g);
        assert_eq!(g.size, (640, 480));
        assert!(s.canvas(false).is_some());
        s.update(&mut g);
        assert_eq!(g.size, (640, 480), "no per-update swapping");
    }

    #[test]
    fn real_sections_see_the_real_screen_without_scaling() {
        let (mut s, mut g) = (Sections::new(), race((1920, 1080)));
        s.update(&mut g);
        s.real_begin(&mut g);
        assert_eq!(g.size, (1920, 1080));
        assert!(
            s.canvas(false).is_none(),
            "3D projected sprites are not scaled"
        );
        s.real_begin(&mut g);
        s.real_end(&mut g);
        assert_eq!(g.size, (1920, 1080), "nested: still real");
        s.real_end(&mut g);
        assert_eq!(g.size, (640, 480));
        assert!(s.canvas(false).is_some());
    }

    #[test]
    fn the_front_end_and_native_sizes_are_left_alone() {
        let mut s = Sections::new();
        let mut fe = Fake {
            size: (1920, 1080),
            race: false,
        };
        s.update(&mut fe);
        assert_eq!(fe.size, (1920, 1080));
        let mut native = race((800, 600));
        s.update(&mut native);
        assert_eq!(native.size, (800, 600));
        assert!(!s.active());
    }

    #[test]
    fn a_race_display_set_up_scales_from_then_on() {
        // Loading: the race starts in the front end's 640x480.
        let (mut s, mut g) = (Sections::new(), race((640, 480)));
        s.update(&mut g);
        assert!(!s.active());
        s.real_begin(&mut g);
        g.size = (2560, 1440); // display set-up
        s.real_end(&mut g);
        assert_eq!(g.size, (640, 480));
        assert!(s.active());
        assert_eq!(s.canvas(false).unwrap(), Canvas::new(2560, 1440, false));
    }

    #[test]
    fn back_to_the_front_end() {
        // Through a display set-up: the front end's 640x480 is kept.
        let (mut s, mut g) = (Sections::new(), race((1920, 1080)));
        s.update(&mut g);
        g.race = false;
        s.real_begin(&mut g);
        g.size = (640, 480);
        s.real_end(&mut g);
        assert!(!s.active());
        assert_eq!(g.size, (640, 480));
        // Without one: the real size comes back at the next update.
        let (mut s, mut g) = (Sections::new(), race((1920, 1080)));
        s.update(&mut g);
        g.race = false;
        s.update(&mut g);
        assert_eq!(g.size, (1920, 1080));
    }

    #[test]
    fn only_the_hud_is_anchored() {
        let (mut s, mut g) = (Sections::new(), race((1920, 1080)));
        s.update(&mut g);
        let whole = s.canvas(true).unwrap();
        s.hud_begin();
        let hud = s.canvas(true).unwrap();
        assert_ne!(whole, hud);
        assert_eq!(s.canvas(false).unwrap(), whole, "other threads' 2D");
        s.hud_end();
        assert_eq!(s.canvas(true).unwrap(), whole);
    }
}
