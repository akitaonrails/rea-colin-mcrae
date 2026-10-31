//! Command line and the settings file (`launcher.cfg`).
//!
//! The file is plain `key = value` lines with `#` comments, written once from
//! a commented template so users can edit it. The launcher later changes
//! only the lines it owns (`game`), keeping everything else as the user left
//! it. Unknown keys are ignored so older launchers can read newer files;
//! values it cannot read are reported and ignored.

use std::path::{Path, PathBuf};

use crate::registry::LANGUAGES;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    pub game: Option<PathBuf>,
    pub runtime: Option<PathBuf>,
    /// Where the player's saves live (see `TEMPLATE`).
    pub saves: Option<PathBuf>,
    pub language: Option<String>,
    /// Closing the window quits the game (fix F6). Default on.
    pub close_quits: Option<bool>,
    /// Pad triggers and right stick work as throttle/brake (fix F7). Default on.
    pub gamepad_axes: Option<bool>,
    /// Race resolution replacing the in-game 640x480 choice (fix F10). Off
    /// unless set.
    pub resolution: Option<(u32, u32)>,
    /// Hor+ widescreen projection (fix F11). Off unless set.
    pub widescreen: Option<bool>,
    /// Draw distance factor in Q16 (fix F12), 1.0 to 3.0. Off unless set.
    pub draw_distance: Option<u32>,
    /// Frame-rate cap in frames per second (fix F17); 0 or unset: none.
    pub fps_limit: Option<u32>,
    /// Wait for the screen's refresh when showing a frame (fix F17); unset
    /// keeps the game's own choice.
    pub vsync: Option<bool>,
    /// Texture filtering (fixes F18, F19); unset or original keeps the
    /// game's.
    pub texture_filter: Option<TextureFilter>,
    /// Smooth view distance changes (fix F20). Off unless set.
    pub smooth_distance: Option<bool>,
    /// Gamepad rumble strength in percent (fix F22); 0 or unset: none.
    pub rumble: Option<u32>,
    /// Linux only: how `cmr-play`'s gamescope enlarges the picture.
    pub scaler: Option<Scaler>,
    /// Pad button layout (fix F23): for the game's buttons 1 to 8, the
    /// physical button (1-based) that presses it, 0 for its own.
    pub buttons: Option<[u8; 8]>,
    /// HD art (fix F27). Off unless set.
    pub hd_art: Option<HdArt>,
}

/// Where the HD art comes from (`hd_art`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HdArt {
    Off,
    /// The pack that comes with the launcher (`hd` next to it).
    Bundled,
    /// A folder `cmr-hd upscale` wrote (with `x2` and `x4` sets).
    Folder(PathBuf),
}

impl HdArt {
    fn parse(value: &str) -> HdArt {
        match parse_bool(value) {
            Some(true) => HdArt::Bundled,
            Some(false) => HdArt::Off,
            None => HdArt::Folder(value.into()),
        }
    }
}

/// The game's own pad layout: every button presses its own.
pub const ORIGINAL_BUTTONS: [u8; 8] = [0; 8];

/// The modern layout for Xbox-style pads (WinMM numbers A 1, B 2, X 3, Y 4,
/// LB 5, RB 6, Back 7, Start 8). The game's buttons are: 1 select (menus),
/// 2 back (menus), 3 change view, 4 gear down, 5 handbrake, 6 gear up,
/// 7 (unused), 8 pause. A selects and is the handbrake, B goes back, Y
/// changes the view, the bumpers change gear, Start pauses.
pub const MODERN_BUTTONS: [u8; 8] = [1, 2, 4, 5, 1, 6, 7, 8];

/// The layout when `buttons` is unset: modern, as most pads today are
/// Xbox-style (owner decision 2026-10-30).
pub const DEFAULT_BUTTONS: [u8; 8] = MODERN_BUTTONS;

/// `original`, `modern` or eight button numbers separated by commas.
fn parse_buttons(v: &str) -> Option<[u8; 8]> {
    match v.trim().to_ascii_lowercase().as_str() {
        "original" => return Some(ORIGINAL_BUTTONS),
        "modern" => return Some(MODERN_BUTTONS),
        _ => {}
    }
    let numbers: Vec<u8> = v
        .split(',')
        .map(|n| n.trim().parse().ok().filter(|n| *n <= 32))
        .collect::<Option<_>>()?;
    numbers.try_into().ok()
}

/// The `buttons` value for a layout: its name when it is one.
pub fn buttons_value(b: &[u8; 8]) -> String {
    match *b {
        ORIGINAL_BUTTONS => "original".into(),
        MODERN_BUTTONS => "modern".into(),
        _ => b.map(|n| n.to_string()).join(","),
    }
}

/// `scaler` (Linux, gamescope's filter): sharp pixels, sharp pixels of
/// even size, FSR, or plain smoothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scaler {
    Nearest,
    Pixel,
    Fsr,
    Linear,
}

impl Scaler {
    pub const ALL: [Scaler; 4] = [Scaler::Nearest, Scaler::Pixel, Scaler::Fsr, Scaler::Linear];

    /// The value in launcher.cfg (and gamescope's `-F` name).
    pub fn key(self) -> &'static str {
        match self {
            Scaler::Nearest => "nearest",
            Scaler::Pixel => "pixel",
            Scaler::Fsr => "fsr",
            Scaler::Linear => "linear",
        }
    }

    fn parse(v: &str) -> Option<Scaler> {
        let v = v.trim().to_ascii_lowercase();
        Scaler::ALL.into_iter().find(|s| s.key() == v)
    }
}

/// `texture_filter`: the game's own filtering, smooth (mipmaps and
/// trilinear, F18) or sharp (nearest pixels everywhere, F19).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureFilter {
    Original,
    Smooth,
    Sharp,
}

fn parse_filter(v: &str) -> Option<TextureFilter> {
    match v.trim().to_ascii_lowercase().as_str() {
        "original" => Some(TextureFilter::Original),
        "smooth" => Some(TextureFilter::Smooth),
        "sharp" => Some(TextureFilter::Sharp),
        _ => None,
    }
}

pub const FILE_NAME: &str = "launcher.cfg";

/// The file written on first run.
pub const TEMPLATE: &str = r"# Colin McRae Rally launcher settings.
#
# One setting per line as key = value; lines starting with # are comments.
# Command-line options (--game, --runtime, --language) override this file
# for one run. A launcher.cfg next to CMRLauncher.exe is used instead of
# this one when it exists (for portable installs).

# The folder with your installed Rally.exe. Filled in by --game; the game
# folder is only read, never changed.
game =

# Where the launcher keeps its private, writable copy of the game, including
# your saves. Default: %LOCALAPPDATA%\CMRLauncher\Rally
#runtime =

# Where your saves and settings (cmRally.cfg, SAVESLOT*.cms) are kept. They
# are copied into the game before it starts and back when it exits.
# Default: %USERPROFILE%\Saved Games\Colin McRae Rally
#saves =

# Game language, used when the game is first set up:
# ENGLISH, FRENCH, GERMAN, SPANISH or ITALIAN.
#language = ENGLISH

# Closing the game window (Alt+F4, the window manager's close) quits the
# game. Set to false for the original 1998 behaviour, where closing the
# window does nothing.
close_quits = true

# Gamepads (CONTROL METHOD: JOYSTICK, CONTROLLER TYPE: ANALOGUE JOYSTICK):
# the left stick steers; R2 or the right stick up accelerates, L2 or the
# right stick down brakes (left stick up/down still works too, and moves
# through the menus). In Options > Settings > Controller 1 choose
# JOYSTICK 1, ANALOGUE JOYSTICK, and set steering, acceleration and braking
# to ANALOGUE. Only axes that rest at centre are used, so wheel pedals are
# not affected. Set to false for the original handling, where only the left
# stick drives. With it on, the pad's Start button also pauses a race.
gamepad_axes = true

# Race resolution, e.g. 1440x1080 (4:3) or 2880x2160. It replaces the
# 640x480 choice of the game's RESOLUTION option (Options > Settings >
# Graphics), so pick 640x480 there; the menus stay 640x480. The screen must
# offer that mode (Proton and gamescope do). Unset keeps the original sizes.
#resolution = 1440x1080

# For a 16:9 (or wider) race resolution such as 1920x1080 or 3840x2160:
# shows more to the sides instead of stretching the 4:3 picture. Has no
# effect at 4:3.
#widescreen = true

# Draw distance: multiplies every stage's view distance (usually 150 m)
# and its object distances, 1.0 to 3.0; the game's own Track Distance and
# Object Distance options still apply on top. Longer distances cost speed
# and can show flicker far away (the game's 16-bit depth buffer).
#draw_distance = 1.5

# Frame-rate cap in frames per second, e.g. 60, 120 or 144; off for none.
# The game's driving runs at its own fixed rate, so the cap only changes
# how often the picture is redrawn (smoother pacing, less heat and noise).
#fps_limit = 60

# true waits for the screen's refresh before showing each frame (no
# tearing); false shows frames at once. Unset keeps the game's own choice.
#vsync = true

# Texture filtering: smooth adds mipmaps and trilinear filtering, so the
# road and scenery don't shimmer in the distance; sharp shows every texture
# with crisp, unfiltered pixels; original keeps the game's own.
#texture_filter = smooth

# Stages change their view distance from section to section at once, so
# scenery pops in or out; true eases each change over about a second.
#smooth_distance = true

# Gamepad rumble on impacts, as a strength from 1 to 100 (percent); off or
# unset for none. Needs an XInput pad (Xbox-style; most pads under Steam).
#rumble = 60

# Pad buttons: modern (the default; Xbox-style pads: A selects and is the
# handbrake, B goes back, Y changes the view, LB/RB change gear, Start
# pauses), original (the game's own 1998 order), or for the game's buttons select, back, change
# view, gear down, handbrake, gear up, (unused), pause the pad button number
# that presses each, e.g. 1,2,4,5,1,6,7,8. The d-pad moves through menus.
#buttons = original

# HD art: the race HUD and the menus' images redrawn at high resolution
# (docs/UPSCALING.md). on uses the pack that comes with the launcher, or
# give a folder made with cmr-hd. The 2x set is used up to 1080p, the 4x
# set above.
#hd_art = on

# Linux (cmr-play) only: how gamescope enlarges the picture. nearest (sharp
# pixels), pixel (sharp, all pixels the same size), fsr (smooth, then
# sharpened) or linear (smooth).
#scaler = nearest
";

pub const USAGE: &str = concat!(
    "CMRLauncher ",
    env!("CARGO_PKG_VERSION"),
    "\n\n",
    "\
CMRLauncher [options]

  --game <folder>          the folder with your installed Rally.exe (remembered)
  --saves <folder>         where your saves are kept
  --runtime <folder>       where the launcher keeps its private copy
  --language <name>        ENGLISH, FRENCH, GERMAN, SPANISH or ITALIAN
  --resolution <WxH>       race resolution, e.g. 1920x1080, or original
  --widescreen on|off      wider view on wide screens
  --draw-distance <n>      1.0 to 3.0
  --fps-limit <n>|off      frame-rate cap, e.g. 60
  --vsync on|off           wait for the screen's refresh
  --texture-filter <f>     original, smooth or sharp
  --smooth-distance on|off ease view distance changes
  --rumble <0-100>|off     gamepad rumble strength
  --buttons <layout>       pad buttons: original, modern or 8 numbers
  --hd-art <folder>        HD art made by cmr-hd (docs/UPSCALING.md)
  --gamepad-axes on|off    triggers/right stick as pedals, Start pauses
  --close-quits on|off     closing the window quits the game

These override launcher.cfg for one run (only --game is remembered). Without
options the settings window opens first.

  --no-window              play with the saved settings, no window
  --settings [--linux]     only the settings window (exit 0: play, 3: closed;
                           --linux adds the gamescope scaler, for cmr-play)

CMRLauncher --check [--game <folder>]
  checks the game folder and tells you whether its Rally.exe is supported

CMRLauncher --controllers [seconds]
  lists the joysticks the game can see and logs every stick, trigger and
  button you move for that long (default 30) to launcher.log

Other settings are in launcher.cfg (see the comments in it)."
);

fn parse_bool(v: &str) -> Option<bool> {
    match v.to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Some(true),
        "false" | "no" | "off" | "0" => Some(false),
        _ => None,
    }
}

/// `WxH`, within what a 1998 DirectDraw game and a modern screen can hold;
/// `original` (or `off`) is `ORIGINAL`, the game's own sizes.
fn parse_resolution(v: &str) -> Option<(u32, u32)> {
    let lower = v.trim().to_ascii_lowercase();
    if lower == "original" || lower == "off" {
        return Some(ORIGINAL);
    }
    let (w, h) = lower.split_once('x')?;
    let w: u32 = w.trim().parse().ok()?;
    let h: u32 = h.trim().parse().ok()?;
    ((320..=7680).contains(&w) && (240..=4320).contains(&h)).then_some((w, h))
}

/// `resolution` value meaning "the game's own sizes" (used to override a
/// file's resolution from the command line).
pub const ORIGINAL: (u32, u32) = (0, 0);

/// A factor 1.0..=3.0 as Q16.
fn parse_factor(v: &str) -> Option<u32> {
    let f: f64 = v.parse().ok()?;
    (1.0..=3.0)
        .contains(&f)
        .then(|| (f * 65536.0).round() as u32)
}

/// A percentage, 0..=100, or `off` (0).
fn parse_percent(v: &str) -> Option<u32> {
    let v = v.trim();
    if v.eq_ignore_ascii_case("off") {
        return Some(0);
    }
    v.parse().ok().filter(|p| *p <= 100)
}

/// A frame-rate cap, 10..=1000, or `off`/`0` for none (0).
fn parse_fps(v: &str) -> Option<u32> {
    let v = v.trim();
    if v.eq_ignore_ascii_case("off") || v == "0" {
        return Some(0);
    }
    let f: u32 = v.parse().ok()?;
    (10..=1000).contains(&f).then_some(f)
}

fn parse_language(v: &str) -> Option<String> {
    let v = v.to_ascii_uppercase();
    LANGUAGES.contains(&v.as_str()).then_some(v)
}

/// Splits `key = value`, ignoring comments and blank values.
fn entry(line: &str) -> Option<(&str, &str)> {
    let line = line.trim();
    if line.starts_with('#') {
        return None;
    }
    let (key, value) = line.split_once('=')?;
    let value = value.trim();
    (!value.is_empty()).then_some((key.trim(), value))
}

impl Settings {
    /// Reads a settings file; the second value lists lines it could not use.
    pub fn parse_file(text: &str) -> (Settings, Vec<String>) {
        let mut s = Settings::default();
        let mut warnings = Vec::new();
        for (n, line) in text.lines().enumerate() {
            let Some((key, value)) = entry(line) else {
                continue;
            };
            let mut bad = |what: &str| {
                warnings.push(format!("{FILE_NAME} line {}: {what} {value:?}", n + 1));
            };
            match key {
                "game" => s.game = Some(value.into()),
                "runtime" => s.runtime = Some(value.into()),
                "saves" => s.saves = Some(value.into()),
                "language" => match parse_language(value) {
                    Some(l) => s.language = Some(l),
                    None => bad("unknown language"),
                },
                "close_quits" => match parse_bool(value) {
                    Some(b) => s.close_quits = Some(b),
                    None => bad("close_quits needs true or false, not"),
                },
                "gamepad_axes" => match parse_bool(value) {
                    Some(b) => s.gamepad_axes = Some(b),
                    None => bad("gamepad_axes needs true or false, not"),
                },
                "widescreen" => match parse_bool(value) {
                    Some(b) => s.widescreen = Some(b),
                    None => bad("widescreen needs true or false, not"),
                },
                "draw_distance" => match parse_factor(value) {
                    Some(f) => s.draw_distance = Some(f),
                    None => bad("draw_distance needs a number from 1.0 to 3.0, not"),
                },
                "fps_limit" => match parse_fps(value) {
                    Some(f) => s.fps_limit = Some(f),
                    None => bad("fps_limit needs a number from 10 to 1000 or off, not"),
                },
                "rumble" => match parse_percent(value) {
                    Some(r) => s.rumble = Some(r),
                    None => bad("rumble needs a number from 0 to 100 or off, not"),
                },
                "smooth_distance" => match parse_bool(value) {
                    Some(b) => s.smooth_distance = Some(b),
                    None => bad("smooth_distance needs true or false, not"),
                },
                "texture_filter" => match parse_filter(value) {
                    Some(f) => s.texture_filter = Some(f),
                    None => bad("texture_filter needs original, smooth or sharp, not"),
                },
                "hd_art" => s.hd_art = Some(HdArt::parse(value)),
                "buttons" => match parse_buttons(value) {
                    Some(b) => s.buttons = Some(b),
                    None => bad("buttons needs original, modern or 8 button numbers, not"),
                },
                "scaler" => match Scaler::parse(value) {
                    Some(f) => s.scaler = Some(f),
                    None => bad("scaler needs nearest, pixel, fsr or linear, not"),
                },
                "vsync" => match parse_bool(value) {
                    Some(b) => s.vsync = Some(b),
                    None => bad("vsync needs true or false, not"),
                },
                "resolution" => match parse_resolution(value) {
                    Some(r) => s.resolution = Some(r),
                    None => bad("resolution needs WIDTHxHEIGHT (320x240 to 7680x4320), not"),
                },
                _ => {}
            }
        }
        (s, warnings)
    }

    /// Command-line values override the file's.
    pub fn merged(self, over: Settings) -> Settings {
        Settings {
            game: over.game.or(self.game),
            runtime: over.runtime.or(self.runtime),
            saves: over.saves.or(self.saves),
            language: over.language.or(self.language),
            close_quits: over.close_quits.or(self.close_quits),
            gamepad_axes: over.gamepad_axes.or(self.gamepad_axes),
            resolution: over.resolution.or(self.resolution),
            widescreen: over.widescreen.or(self.widescreen),
            draw_distance: over.draw_distance.or(self.draw_distance),
            fps_limit: over.fps_limit.or(self.fps_limit),
            vsync: over.vsync.or(self.vsync),
            texture_filter: over.texture_filter.or(self.texture_filter),
            smooth_distance: over.smooth_distance.or(self.smooth_distance),
            rumble: over.rumble.or(self.rumble),
            scaler: over.scaler.or(self.scaler),
            buttons: over.buttons.or(self.buttons),
            hd_art: over.hd_art.or(self.hd_art),
        }
    }

    /// The rumble strength in percent, 0 for none.
    pub fn rumble_strength(&self) -> u32 {
        self.rumble.unwrap_or(0).min(100)
    }

    /// The frame-rate cap to apply, if any.
    pub fn fps_cap(&self) -> Option<u32> {
        self.fps_limit.filter(|&f| f > 0)
    }

    /// `Flip` flags for the vsync choice: 0 keeps the game's own,
    /// `DDFLIP_WAIT` (1) waits for the refresh, adding `DDFLIP_NOVSYNC` (8)
    /// doesn't.
    pub fn flip_flags(&self) -> u32 {
        match self.vsync {
            None => 0,
            Some(true) => 1,
            Some(false) => 1 | 8,
        }
    }

    /// Whether HD art is on.
    pub fn hd_on(&self) -> bool {
        matches!(self.hd_art, Some(HdArt::Bundled | HdArt::Folder(_)))
    }

    /// The HD art set for the race resolution, from `bundled` (the
    /// installed pack) or the folder set: `x2` up to 2.5 times the game's
    /// 480 lines (1080p), `x4` above (1440p, 4K).
    pub fn hd_set(&self, bundled: &Path) -> Option<PathBuf> {
        let dir = match self.hd_art.as_ref()? {
            HdArt::Off => return None,
            HdArt::Bundled => bundled,
            HdArt::Folder(dir) => dir,
        };
        let height = self.race_resolution().map_or(480, |(_, h)| h);
        Some(dir.join(if height * 2 <= 480 * 5 { "x2" } else { "x4" }))
    }

    /// F23's map: the physical button for each of the game's 12 buttons
    /// (0 for its own).
    pub fn button_map(&self) -> [u8; 12] {
        let mut map = [0; 12];
        map[..8].copy_from_slice(&self.buttons.unwrap_or(DEFAULT_BUTTONS));
        map
    }

    /// The race resolution to apply, if any.
    pub fn race_resolution(&self) -> Option<(u32, u32)> {
        self.resolution.filter(|&r| r != ORIGINAL)
    }

    /// Optional fixes these settings turn off (see `fixes::Fix::optional`).
    pub fn skipped_fixes(&self) -> Vec<&'static str> {
        let mut skip = Vec::new();
        if self.close_quits == Some(false) {
            skip.push("F6");
        }
        if self.gamepad_axes == Some(false) {
            skip.push("F7");
            skip.push("F14");
            skip.push("F21");
            skip.push("F23");
        }
        if self.race_resolution().is_none() {
            skip.push("F10");
            skip.push("F13");
            skip.push("F16");
        }
        if self.widescreen != Some(true) {
            skip.push("F11");
        }
        if self.draw_distance.is_none() {
            skip.push("F12");
            skip.push("F25");
        }
        if self.race_resolution().is_none()
            && self.fps_cap().is_none()
            && self.vsync.is_none()
            && self.rumble_strength() == 0
        {
            skip.push("F17");
        }
        if self.texture_filter != Some(TextureFilter::Smooth) {
            skip.push("F18");
        }
        if self.texture_filter != Some(TextureFilter::Sharp) {
            skip.push("F19");
        }
        if self.smooth_distance != Some(true) {
            skip.push("F20");
        }
        if self.rumble_strength() == 0 {
            skip.push("F22");
        }
        if !self.hd_on() {
            skip.push("F27");
        }
        skip
    }

    pub fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Result<Settings, String> {
        let mut s = Settings::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let mut value = |name: &str| {
                args.next()
                    .ok_or_else(|| format!("{name} needs a value\n\n{USAGE}"))
            };
            match arg.as_str() {
                "--game" => s.game = Some(value("--game")?.into()),
                "--runtime" => s.runtime = Some(value("--runtime")?.into()),
                "--saves" => s.saves = Some(value("--saves")?.into()),
                "--language" => {
                    let v = value("--language")?;
                    s.language = Some(
                        parse_language(&v)
                            .ok_or_else(|| format!("unknown language {v}\n\n{USAGE}"))?,
                    );
                }
                "--resolution" => {
                    let v = value("--resolution")?;
                    s.resolution = Some(parse_resolution(&v).ok_or_else(|| {
                        format!("--resolution needs WIDTHxHEIGHT or original, not {v}\n\n{USAGE}")
                    })?);
                }
                "--hd-art" => s.hd_art = Some(HdArt::parse(&value("--hd-art")?)),
                "--buttons" => {
                    let v = value("--buttons")?;
                    s.buttons = Some(parse_buttons(&v).ok_or_else(|| {
                        format!("--buttons needs original, modern or 8 button numbers, not {v}\n\n{USAGE}")
                    })?);
                }
                "--rumble" => {
                    let v = value("--rumble")?;
                    s.rumble = Some(parse_percent(&v).ok_or_else(|| {
                        format!("--rumble needs 0 to 100 or off, not {v}\n\n{USAGE}")
                    })?);
                }
                "--texture-filter" => {
                    let v = value("--texture-filter")?;
                    s.texture_filter = Some(parse_filter(&v).ok_or_else(|| {
                        format!(
                            "--texture-filter needs original, smooth or sharp, not {v}\n\n{USAGE}"
                        )
                    })?);
                }
                "--fps-limit" => {
                    let v = value("--fps-limit")?;
                    s.fps_limit = Some(parse_fps(&v).ok_or_else(|| {
                        format!("--fps-limit needs 10 to 1000 or off, not {v}\n\n{USAGE}")
                    })?);
                }
                "--draw-distance" => {
                    let v = value("--draw-distance")?;
                    s.draw_distance = Some(parse_factor(&v).ok_or_else(|| {
                        format!("--draw-distance needs 1.0 to 3.0, not {v}\n\n{USAGE}")
                    })?);
                }
                flag @ ("--widescreen" | "--gamepad-axes" | "--close-quits" | "--vsync"
                | "--smooth-distance") => {
                    let v = value(flag)?;
                    let b = parse_bool(&v)
                        .ok_or_else(|| format!("{flag} needs on or off, not {v}\n\n{USAGE}"))?;
                    match flag {
                        "--widescreen" => s.widescreen = Some(b),
                        "--gamepad-axes" => s.gamepad_axes = Some(b),
                        "--vsync" => s.vsync = Some(b),
                        "--smooth-distance" => s.smooth_distance = Some(b),
                        _ => s.close_quits = Some(b),
                    }
                }
                "--help" | "-h" | "/?" => return Err(USAGE.into()),
                other => return Err(format!("unknown argument {other}\n\n{USAGE}")),
            }
        }
        Ok(s)
    }
}

/// Sets `key` in a settings file, keeping every other line (and comments)
/// as it was: replaces the first active or empty `key =` line, else appends.
pub fn set_key(text: &str, key: &str, value: &str) -> String {
    let is_key = |line: &str| {
        let line = line.trim();
        !line.starts_with('#') && line.split_once('=').is_some_and(|(k, _)| k.trim() == key)
    };
    let mut out = String::with_capacity(text.len() + key.len() + value.len() + 4);
    let mut done = false;
    for line in text.lines() {
        if !done && is_key(line) {
            out.push_str(&format!("{key} = {value}"));
            done = true;
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    if !done {
        out.push_str(&format!("{key} = {value}\n"));
    }
    out
}

/// Turns `key` off in a settings file: its active line becomes a comment
/// (`#key = `), so the default applies; other lines are kept.
pub fn clear_key(text: &str, key: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    for line in text.lines() {
        let t = line.trim();
        if !t.starts_with('#') && t.split_once('=').is_some_and(|(k, _)| k.trim() == key) {
            out.push_str(&format!("#{key} ="));
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hd_art_set_follows_the_resolution() {
        let pack = Path::new("P:\\pack");
        let at = |art: &str, res: &str| {
            Settings::parse_file(&format!("hd_art = {art}\nresolution = {res}"))
                .0
                .hd_set(pack)
        };
        assert_eq!(
            at("C:\\hd", "1920x1080"),
            Some(PathBuf::from("C:\\hd").join("x2"))
        );
        assert_eq!(
            at("C:\\hd", "2560x1440"),
            Some(PathBuf::from("C:\\hd").join("x4"))
        );
        assert_eq!(
            at("C:\\hd", "original"),
            Some(PathBuf::from("C:\\hd").join("x2"))
        );
        assert_eq!(at("on", "3840x2160"), Some(pack.join("x4")));
        assert_eq!(at("off", "3840x2160"), None);
        assert_eq!(Settings::default().hd_set(pack), None);
        assert!(Settings::default().skipped_fixes().contains(&"F27"));
        let s = Settings::parse_args(["--hd-art".to_string(), "on".to_string()]).unwrap();
        assert_eq!(s.hd_art, Some(HdArt::Bundled));
        assert!(!s.skipped_fixes().contains(&"F27"));
        // The command line's off wins over the file's on.
        let file = Settings::parse_file("hd_art = on").0;
        let off = Settings::parse_args(["--hd-art".to_string(), "off".to_string()]).unwrap();
        assert!(!file.merged(off).hd_on());
    }

    #[test]
    fn button_layouts() {
        let parse = |v: &str| Settings::parse_file(&format!("buttons = {v}"));
        assert_eq!(parse("modern").0.buttons, Some(MODERN_BUTTONS));
        assert_eq!(parse("Original").0.buttons, Some(ORIGINAL_BUTTONS));
        assert_eq!(
            parse("3, 2,1,0,0,0,0,8").0.buttons,
            Some([3, 2, 1, 0, 0, 0, 0, 8])
        );
        assert_eq!(parse("1,2,3").1.len(), 1, "eight numbers");
        assert_eq!(parse("1,2,3,4,5,6,7,99").1.len(), 1, "up to 32");
        assert_eq!(buttons_value(&MODERN_BUTTONS), "modern");
        assert_eq!(buttons_value(&[3, 2, 1, 0, 0, 0, 0, 8]), "3,2,1,0,0,0,0,8");
        let s = parse("modern").0;
        assert_eq!(s.button_map()[..8], MODERN_BUTTONS);
        assert_eq!(s.button_map()[8..], [0; 4]);
        assert_eq!(
            Settings::default().button_map()[..8],
            MODERN_BUTTONS,
            "the default"
        );
        assert_eq!(parse("original").0.button_map(), [0; 12]);
    }

    fn args(a: &[&str]) -> Result<Settings, String> {
        Settings::parse_args(a.iter().map(|s| s.to_string()))
    }

    #[test]
    fn parses_arguments() {
        let s = args(&["--game", r"D:\Games\CMR", "--language", "german"]).unwrap();
        assert_eq!(s.game, Some(PathBuf::from(r"D:\Games\CMR")));
        assert_eq!(s.language.as_deref(), Some("GERMAN"));
        assert!(args(&["--language", "klingon"]).is_err());
        assert!(args(&["--game"]).is_err());
        assert!(args(&["--bogus"]).is_err());
    }

    #[test]
    fn every_setting_has_a_flag() {
        let s = args(&[
            "--resolution",
            "2560x1440",
            "--widescreen",
            "on",
            "--draw-distance",
            "2",
            "--gamepad-axes",
            "off",
            "--close-quits",
            "no",
        ])
        .unwrap();
        assert_eq!(s.race_resolution(), Some((2560, 1440)));
        assert_eq!(s.widescreen, Some(true));
        assert_eq!(s.draw_distance, Some(0x20000));
        assert_eq!(
            s.skipped_fixes(),
            [
                "F6", "F7", "F14", "F21", "F23", "F18", "F19", "F20", "F22", "F27"
            ]
        );
        assert!(args(&["--widescreen", "maybe"]).is_err());
        let s = args(&["--fps-limit", "120", "--vsync", "off"]).unwrap();
        assert_eq!((s.fps_cap(), s.flip_flags()), (Some(120), 9));
        assert_eq!(args(&["--fps-limit", "off"]).unwrap().fps_cap(), None);
        assert!(args(&["--fps-limit", "5"]).is_err());
        let smooth = args(&["--texture-filter", "smooth"])
            .unwrap()
            .skipped_fixes();
        assert!(!smooth.contains(&"F18") && smooth.contains(&"F19"));
        let sharp = args(&["--texture-filter", "SHARP"])
            .unwrap()
            .skipped_fixes();
        assert!(sharp.contains(&"F18") && !sharp.contains(&"F19"));
        assert!(args(&["--texture-filter", "blurry"]).is_err());
        let r = args(&["--rumble", "60"]).unwrap();
        assert_eq!(r.rumble_strength(), 60);
        assert!(!r.skipped_fixes().contains(&"F22") && !r.skipped_fixes().contains(&"F17"));
        assert!(args(&["--rumble", "150"]).is_err());
        assert!(
            args(&["--gamepad-axes", "off"])
                .unwrap()
                .skipped_fixes()
                .contains(&"F21")
        );
        let eased = args(&["--smooth-distance", "on"]).unwrap();
        assert!(!eased.skipped_fixes().contains(&"F20"));
        // A cap alone needs the present hook (F17), not the race resolution.
        let cap = args(&["--fps-limit", "60"]).unwrap();
        assert!(cap.skipped_fixes().contains(&"F16") && !cap.skipped_fixes().contains(&"F17"));
        assert!(args(&["--draw-distance", "9"]).is_err());

        // `original` on the command line overrides a file's resolution.
        let (file, _) = Settings::parse_file("resolution = 1920x1080\n");
        let m = file.merged(args(&["--resolution", "original"]).unwrap());
        assert_eq!(m.race_resolution(), None);
        assert!(m.skipped_fixes().contains(&"F10"));
    }

    #[test]
    fn template_parses_to_the_defaults() {
        let (s, warnings) = Settings::parse_file(TEMPLATE);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            s,
            Settings {
                close_quits: Some(true),
                gamepad_axes: Some(true),
                ..Settings::default()
            }
        );
        assert_eq!(
            s.skipped_fixes(),
            [
                "F10", "F13", "F16", "F11", "F12", "F25", "F17", "F18", "F19", "F20", "F22", "F27"
            ]
        );
    }

    #[test]
    fn file_values_and_args_win() {
        let text = "game = C:\\old\nruntime=C:\\rt\nsaves = D:\\My Saves\nlanguage = french\nclose_quits = no\ngamepad_axes=off\nresolution = 1920X1440\ndraw_distance=1.5\n";
        let (saved, warnings) = Settings::parse_file(text);
        assert!(warnings.is_empty());
        assert_eq!(saved.language.as_deref(), Some("FRENCH"));
        assert_eq!(saved.resolution, Some((1920, 1440)));
        assert_eq!(saved.draw_distance, Some(0x18000));
        assert_eq!(
            saved.skipped_fixes(),
            [
                "F6", "F7", "F14", "F21", "F23", "F11", "F18", "F19", "F20", "F22", "F27"
            ]
        );
        let m = saved.merged(args(&["--game", r"C:\new"]).unwrap());
        assert_eq!(m.game, Some(PathBuf::from(r"C:\new")));
        assert_eq!(m.runtime, Some(PathBuf::from(r"C:\rt")));
        assert_eq!(m.saves, Some(PathBuf::from(r"D:\My Saves")));
    }

    #[test]
    fn file_ignores_junk_and_reports_bad_values() {
        let (s, warnings) = Settings::parse_file(
            "# game = C:\\commented\nfuture=1\ngame = C:\\G \n\nruntime=\nclose_quits = maybe\nlanguage=xx\nresolution=8000x100\n",
        );
        assert_eq!(s.game, Some(PathBuf::from(r"C:\G")));
        assert_eq!(s.runtime, None);
        assert_eq!(s.close_quits, None);
        assert_eq!(warnings.len(), 3, "{warnings:?}");
        assert!(warnings[0].contains("line 6"));
    }

    #[test]
    fn set_key_keeps_the_rest_of_the_file() {
        let filled = set_key(TEMPLATE, "game", r"D:\CMR");
        assert_eq!(filled.lines().count(), TEMPLATE.lines().count());
        assert!(filled.contains("\ngame = D:\\CMR\n"));
        assert!(filled.contains("#runtime =\n"));
        let (s, _) = Settings::parse_file(&filled);
        assert_eq!(s.game, Some(PathBuf::from(r"D:\CMR")));

        // Replaced again in place; a commented key is not touched.
        let again = set_key(&filled, "game", r"E:\CMR");
        assert_eq!(again.matches("game =").count(), 1);
        assert!(set_key("#game = x\n", "game", "y").ends_with("#game = x\ngame = y\n"));
    }
}
