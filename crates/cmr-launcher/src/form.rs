//! What the settings window shows and saves (E13), kept apart from the
//! drawing so it can be tested on any host.
//!
//! The window offers one choice first, the look: the original 1998 game,
//! an enhanced preset that fits the player's screen, or custom values. The
//! preset is never stored: the file keeps the individual settings (as the
//! command line and older launchers read them) and the look is worked out
//! from them when the window opens.

use std::path::PathBuf;

use crate::config::{self, Scaler, Settings, TextureFilter};

/// The picture settings the look presets cover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Picture {
    /// `None`: the game's own sizes.
    pub resolution: Option<(u32, u32)>,
    pub widescreen: bool,
    /// Q16, 1.0 (`0x10000`) is the game's own.
    pub draw_distance: u32,
    pub texture_filter: TextureFilter,
    pub fps_limit: Option<u32>,
    pub vsync: Option<bool>,
    pub smooth_distance: bool,
}

const ONE: u32 = 0x10000;

impl Picture {
    /// The game as it was.
    pub const ORIGINAL: Picture = Picture {
        resolution: None,
        widescreen: false,
        draw_distance: ONE,
        texture_filter: TextureFilter::Original,
        fps_limit: None,
        vsync: None,
        smooth_distance: false,
    };

    /// The recommended look for a screen: its own size (on its long side
    /// first, as gamescope and a rotated monitor give the game a landscape
    /// screen), widescreen when it is wider than 4:3, smooth textures, a
    /// longer view eased between sections, and vsync.
    pub fn enhanced(screen: Option<(u32, u32)>) -> Picture {
        let resolution = screen.and_then(landscape);
        Picture {
            resolution,
            widescreen: resolution.is_some_and(|(w, h)| w * 3 > h * 4),
            draw_distance: ONE * 3 / 2,
            texture_filter: TextureFilter::Smooth,
            fps_limit: None,
            vsync: Some(true),
            smooth_distance: true,
        }
    }
}

/// A screen size as a landscape race resolution the game can use, or
/// `None` when it is too small to be worth it.
fn landscape((a, b): (u32, u32)) -> Option<(u32, u32)> {
    let (w, h) = (a.max(b) & !1, a.min(b) & !1);
    (w > 640 && h > 480 && w <= 7680 && h <= 4320).then_some((w, h))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    Original,
    Enhanced,
    Custom,
}

/// Everything the window edits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Form {
    pub game: String,
    pub saves: String,
    pub picture: Picture,
    /// Remembered so a look picked by name stays picked even when its
    /// values match another (and "Custom" stays open while editing).
    pub look: Look,
    pub rumble: u32,
    pub gamepad_axes: bool,
    pub close_quits: bool,
    pub scaler: Scaler,
}

/// Choices offered for each picture setting, with their labels.
pub const DISTANCES: [(u32, &str); 6] = [
    (ONE, "1×"),
    (ONE * 5 / 4, "1.25×"),
    (ONE * 3 / 2, "1.5×"),
    (ONE * 2, "2×"),
    (ONE * 5 / 2, "2.5×"),
    (ONE * 3, "3×"),
];
pub const FRAME_RATES: [(Option<u32>, &str); 5] = [
    (None, "Original"),
    (Some(30), "30"),
    (Some(60), "60"),
    (Some(120), "120"),
    (Some(144), "144"),
];
pub const VSYNCS: [(Option<bool>, &str); 3] = [
    (None, "As the game"),
    (Some(true), "On"),
    (Some(false), "Off"),
];
pub const FILTERS: [(TextureFilter, &str); 3] = [
    (TextureFilter::Original, "Original"),
    (TextureFilter::Smooth, "Smooth"),
    (TextureFilter::Sharp, "Sharp"),
];
pub const RUMBLES: [(u32, &str); 4] = [(0, "Off"), (30, "Light"), (60, "Medium"), (100, "Strong")];
pub const SCALERS: [(Scaler, &str, &str); 4] = [
    (
        Scaler::Nearest,
        "Sharp",
        "Every game pixel becomes a crisp block. Best for the 1998 artwork.",
    ),
    (
        Scaler::Pixel,
        "Even pixels",
        "Crisp blocks, with only their edges blended so all are the same size.",
    ),
    (
        Scaler::Fsr,
        "FSR",
        "AMD FSR: smooth, then sharpened. Softer than Sharp.",
    ),
    (
        Scaler::Linear,
        "Smooth",
        "Plain smoothing. The softest, most blurred look.",
    ),
];

/// Race resolutions offered: the game's own, the screen's (when it is not
/// already listed), then common sizes.
pub fn resolutions(screen: Option<(u32, u32)>) -> Vec<(Option<(u32, u32)>, String)> {
    let common = [(1920, 1080), (2560, 1440), (3840, 2160)];
    let mut out = vec![(None, "Original".to_string())];
    if let Some(s) = screen.and_then(landscape)
        && !common.contains(&s)
    {
        out.push((Some(s), format!("Screen {}×{}", s.0, s.1)));
    }
    for (w, h) in common {
        let label = match h {
            1080 => "1080p",
            1440 => "1440p",
            _ => "4K",
        };
        out.push((Some((w, h)), label.to_string()));
    }
    out
}

impl Form {
    /// The window's starting state for saved settings on a screen.
    pub fn new(s: &Settings, screen: Option<(u32, u32)>) -> Form {
        let path = |p: &Option<PathBuf>| {
            p.as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default()
        };
        let picture = Picture {
            resolution: s.race_resolution(),
            widescreen: s.widescreen == Some(true),
            draw_distance: s.draw_distance.unwrap_or(ONE),
            texture_filter: s.texture_filter.unwrap_or(TextureFilter::Original),
            fps_limit: s.fps_cap(),
            vsync: s.vsync,
            smooth_distance: s.smooth_distance == Some(true),
        };
        Form {
            game: path(&s.game),
            saves: path(&s.saves),
            picture,
            look: Form::look_of(&picture, screen),
            rumble: s.rumble_strength(),
            gamepad_axes: s.gamepad_axes != Some(false),
            close_quits: s.close_quits != Some(false),
            scaler: s.scaler.unwrap_or(Scaler::Nearest),
        }
    }

    /// The look these picture values make on a screen.
    pub fn look_of(picture: &Picture, screen: Option<(u32, u32)>) -> Look {
        if *picture == Picture::ORIGINAL {
            Look::Original
        } else if *picture == Picture::enhanced(screen) {
            Look::Enhanced
        } else {
            Look::Custom
        }
    }

    /// Picks a look; Original and Enhanced set the picture, Custom keeps
    /// the current values to edit.
    pub fn set_look(&mut self, look: Look, screen: Option<(u32, u32)>) {
        self.look = look;
        match look {
            Look::Original => self.picture = Picture::ORIGINAL,
            Look::Enhanced => self.picture = Picture::enhanced(screen),
            Look::Custom => {}
        }
    }

    /// The settings file with this form's values written in; every other
    /// line (comments, keys the window doesn't show) is kept.
    pub fn save(&self, text: &str) -> String {
        let p = &self.picture;
        let mut t = text.to_string();
        let mut put = |key: &str, value: Option<String>| {
            t = match &value {
                Some(v) => config::set_key(&t, key, v),
                None => config::clear_key(&t, key),
            };
        };
        let path = |s: &str| Some(s.trim().to_string()).filter(|s| !s.is_empty());
        put("game", path(&self.game));
        put("saves", path(&self.saves));
        put("resolution", p.resolution.map(|(w, h)| format!("{w}x{h}")));
        put("widescreen", p.widescreen.then(|| "true".into()));
        put(
            "draw_distance",
            (p.draw_distance != ONE).then(|| format!("{}", p.draw_distance as f64 / ONE as f64)),
        );
        put("fps_limit", p.fps_limit.map(|f| f.to_string()));
        put("vsync", p.vsync.map(|v| v.to_string()));
        put(
            "texture_filter",
            match p.texture_filter {
                TextureFilter::Original => None,
                TextureFilter::Smooth => Some("smooth".into()),
                TextureFilter::Sharp => Some("sharp".into()),
            },
        );
        put("smooth_distance", p.smooth_distance.then(|| "true".into()));
        put("rumble", (self.rumble > 0).then(|| self.rumble.to_string()));
        put("gamepad_axes", Some(self.gamepad_axes.to_string()));
        put("close_quits", Some(self.close_quits.to_string()));
        put("scaler", Some(self.scaler.key().into()));
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Option<(u32, u32)> = Some((2560, 1440));

    fn form(text: &str) -> Form {
        Form::new(&Settings::parse_file(text).0, SCREEN)
    }

    #[test]
    fn a_new_player_starts_with_the_original_game() {
        let f = form(config::TEMPLATE);
        assert_eq!(f.look, Look::Original);
        assert_eq!(f.picture, Picture::ORIGINAL);
        assert!(f.gamepad_axes && f.close_quits);
        assert_eq!(f.rumble, 0);
    }

    #[test]
    fn enhanced_fits_the_screen() {
        let p = Picture::enhanced(Some((2560, 1440)));
        assert_eq!(p.resolution, Some((2560, 1440)));
        assert!(p.widescreen);
        // A rotated monitor still gives a landscape race; 4:3 is not wide.
        assert_eq!(
            Picture::enhanced(Some((1440, 2560))).resolution,
            Some((2560, 1440))
        );
        assert!(!Picture::enhanced(Some((1600, 1200))).widescreen);
        // Odd sizes are made even; a tiny or unknown screen keeps 640x480.
        assert_eq!(
            Picture::enhanced(Some((1366, 769))).resolution,
            Some((1366, 768))
        );
        assert_eq!(Picture::enhanced(Some((640, 480))).resolution, None);
        assert_eq!(Picture::enhanced(None).resolution, None);
    }

    #[test]
    fn the_look_is_worked_out_from_the_saved_values() {
        let mut f = form(config::TEMPLATE);
        f.set_look(Look::Enhanced, SCREEN);
        let saved = f.save(config::TEMPLATE);
        assert_eq!(form(&saved).look, Look::Enhanced);
        // One value off the preset: custom.
        let tweaked = config::set_key(&saved, "draw_distance", "2");
        assert_eq!(form(&tweaked).look, Look::Custom);
        // Back to the original removes every picture key.
        let mut g = form(&saved);
        g.set_look(Look::Original, SCREEN);
        let back = g.save(&saved);
        assert_eq!(form(&back).look, Look::Original);
        let (s, warnings) = Settings::parse_file(&back);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(s.skipped_fixes(), Settings::default().skipped_fixes());
    }

    #[test]
    fn custom_keeps_the_values_to_edit() {
        let mut f = form(config::TEMPLATE);
        f.set_look(Look::Enhanced, SCREEN);
        f.set_look(Look::Custom, SCREEN);
        assert_eq!(f.picture, Picture::enhanced(SCREEN));
    }

    #[test]
    fn saving_keeps_comments_and_round_trips_every_field() {
        let mut f = form(config::TEMPLATE);
        f.game = r"D:\Games\CMR".into();
        f.saves = r"Z:\home\me\saves".into();
        f.picture = Picture {
            resolution: Some((1920, 1080)),
            widescreen: true,
            draw_distance: ONE * 5 / 4,
            texture_filter: TextureFilter::Sharp,
            fps_limit: Some(144),
            vsync: Some(false),
            smooth_distance: true,
        };
        f.rumble = 30;
        f.gamepad_axes = false;
        f.close_quits = false;
        f.scaler = Scaler::Fsr;
        let saved = f.save(config::TEMPLATE);
        assert!(saved.contains("# Colin McRae Rally launcher settings."));
        let mut back = form(&saved);
        assert_eq!(back.look, Look::Custom);
        back.look = f.look;
        assert_eq!(back, f);
        let (_, warnings) = Settings::parse_file(&saved);
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn the_screen_size_is_offered_once() {
        let r = resolutions(Some((2560, 1600)));
        assert_eq!(r[1], (Some((2560, 1600)), "Screen 2560×1600".to_string()));
        let listed = resolutions(Some((1920, 1080)));
        assert_eq!(
            listed
                .iter()
                .filter(|(r, _)| *r == Some((1920, 1080)))
                .count(),
            1
        );
        assert_eq!(resolutions(None).len(), 4);
    }
}
