//! Command line and the settings file (`launcher.cfg`).
//!
//! Every option is declared once, in [`OPTIONS`] (the SDK's
//! `rea_launcher::cfg::spec`): its key, flag, values, label and a one-line
//! description. `--help`, the commented template written on a first run,
//! the file and flag parsing and the settings window's rows all come from
//! that table, so the window and the command line can't drift apart.
//!
//! The file is plain `key = value` lines with `#` comments. The launcher
//! later changes only the lines it owns (`game`), keeping everything else
//! as the user left it. Unknown keys are reported, ignored and kept, so
//! older launchers can read newer files; values it cannot read are
//! reported and ignored.

use std::path::{Path, PathBuf};

use rea_launcher::cfg::spec::{self, Kind, OptionSpec, Value};
use rea_launcher::cfg::{self, ArgError, KeyStyle, Rejected, Resolution};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    pub game: Option<PathBuf>,
    pub runtime: Option<PathBuf>,
    /// Where the player's saves live (see [`OPTIONS`]).
    pub saves: Option<PathBuf>,
    pub language: Option<String>,
    /// A look's name ([`LOOKS`]): its picture values for every picture
    /// setting not given (see `form::with_look`).
    pub look: Option<&'static str>,
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
        match cfg::parse_bool(value) {
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

    fn from_key(v: &str) -> Option<Scaler> {
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

impl TextureFilter {
    pub const ALL: [TextureFilter; 3] = [
        TextureFilter::Original,
        TextureFilter::Smooth,
        TextureFilter::Sharp,
    ];

    pub fn key(self) -> &'static str {
        match self {
            TextureFilter::Original => "original",
            TextureFilter::Smooth => "smooth",
            TextureFilter::Sharp => "sharp",
        }
    }

    fn from_key(v: &str) -> Option<TextureFilter> {
        TextureFilter::ALL.into_iter().find(|f| f.key() == v)
    }
}

pub const FILE_NAME: &str = "launcher.cfg";

/// `resolution = original` (or `off`): the game's own sizes (used to
/// override a file's resolution from the command line).
pub const ORIGINAL: (u32, u32) = (0, 0);

/// The looks, as `--look` and `look =` name them (the window's cards).
pub const LOOKS: &[(&str, &str)] = &[
    ("original", "Original 1998"),
    ("enhanced", "Enhanced"),
    ("steam_deck", "Steam Deck"),
];
pub const LANGUAGE_CHOICES: &[(&str, &str)] = &[
    ("ENGLISH", "English"),
    ("FRENCH", "Français"),
    ("GERMAN", "Deutsch"),
    ("SPANISH", "Español"),
    ("ITALIAN", "Italiano"),
];
pub const FILTERS: &[(&str, &str)] = &[
    ("original", "Original"),
    ("smooth", "Smooth"),
    ("sharp", "Sharp"),
];
/// The scaler's chips. Not "Sharp"/"Smooth": the Textures row above them
/// uses those words for something else.
pub const SCALERS: &[(&str, &str)] = &[
    ("nearest", "Crisp pixels"),
    ("pixel", "Even pixels"),
    ("fsr", "FSR"),
    ("linear", "Soft"),
];

/// The window's chip for an unset option: the game keeps its own value.
pub const AS_THE_GAME: &str = "As the game";

/// Every option: one row each for the file, the flags, `--help`, the
/// template and the window.
pub const OPTIONS: &[OptionSpec] = &[
    OptionSpec::new(
        "game",
        Kind::Path,
        "Game folder",
        "The folder with your installed Rally.exe. It is only read, never changed. \
         Remembered once given.",
    ),
    OptionSpec::new(
        "saves",
        Kind::Path,
        "Saves",
        "Where your saves and settings (cmRally.cfg, SAVESLOT*.cms) are kept: copied into the \
         game before it starts and back when it ends. Default: Saved Games\\Colin McRae Rally.",
    ),
    OptionSpec::new(
        "runtime",
        Kind::Path,
        "Runtime folder",
        "Where the launcher keeps its private, writable copy of the game. \
         Default: %LOCALAPPDATA%\\CMRLauncher\\Rally.",
    )
    .launcher_only(),
    OptionSpec::new(
        "language",
        Kind::Choice(LANGUAGE_CHOICES),
        "Language",
        "The game's language, set when the launcher first sets the game up. After that the \
         game keeps the language it has.",
    )
    .unset_label(AS_THE_GAME)
    .with_example("ENGLISH"),
    OptionSpec::new(
        "look",
        Kind::Choice(LOOKS),
        "Look",
        "Every picture setting at once: the game as it shipped, the screen's own size with \
         every improvement, or the Steam Deck's screen. Picture settings given on their own \
         win over it.",
    ),
    OptionSpec::new(
        "resolution",
        Kind::Size {
            min: (320, 240),
            max: (7680, 4320),
            aliases: cfg::ORIGINAL_OR_OFF,
        },
        "Race resolution",
        "The size races are drawn at, in place of the game's 640×480 (keep 640×480 in the \
         game's own menu); the menus stay 640×480. Original keeps the game's sizes.",
    )
    .with_example("1920x1080"),
    OptionSpec::new(
        "widescreen",
        Kind::Toggle,
        "Widescreen",
        "On a 16:9 or wider race resolution, shows more to the sides instead of stretching \
         the 4:3 picture.",
    ),
    OptionSpec::new(
        "draw_distance",
        Kind::Text,
        "Draw distance",
        "How far you see: every stage's view and object distances times this, 1.0 to 3.0. \
         Longer costs speed and can flicker far away.",
    )
    .with_needs("1.0 to 3.0")
    .with_placeholder("<1.0-3.0>")
    .with_example("1.5"),
    OptionSpec::new(
        "texture_filter",
        Kind::Choice(FILTERS),
        "Textures",
        "Smooth: the road and scenery don't shimmer in the distance. Sharp: crisp, \
         unfiltered pixels. Original: the game's own.",
    ),
    OptionSpec::new(
        "fps_limit",
        Kind::Range { min: 10, max: 1000 },
        "Frame-rate cap",
        "The most frames a second the picture is redrawn (the driving keeps its own rate): \
         smoother pacing, less heat and noise. Off: no cap.",
    )
    .with_value_aliases(&[("off", "0")])
    .with_example("60"),
    OptionSpec::new(
        "vsync",
        Kind::Toggle,
        "Vsync",
        "Waits for the screen's refresh before showing each frame: no tearing. Unset keeps \
         the game's own choice.",
    )
    .unset_label(AS_THE_GAME),
    OptionSpec::new(
        "smooth_distance",
        Kind::Toggle,
        "Ease view distance",
        "Stages change their view distance from section to section at once, so scenery pops \
         in; this eases each change over about a second.",
    ),
    OptionSpec::new(
        "hd_art",
        Kind::Text,
        "HD art",
        "The race HUD, logos and menu pictures redrawn at high resolution: on uses the pack \
         that comes with the launcher, or give a folder made with cmr-hd.",
    )
    .with_needs("on, off or a folder")
    .with_placeholder("on|off|<folder>")
    .with_example("on"),
    OptionSpec::new(
        "scaler",
        Kind::Choice(SCALERS),
        "Scaler",
        "Linux only: how gamescope enlarges the picture to your screen. Crisp pixels: every \
         game pixel a sharp block. Even pixels: sharp, all the same size. FSR: smoothed, then \
         sharpened. Soft: plain smoothing (the default).",
    )
    .helper_owned(),
    OptionSpec::new(
        "gamepad_axes",
        Kind::Toggle,
        "Gamepad pedals",
        "The triggers or the right stick work as throttle and brake, and Start pauses a race. \
         Off: only the left stick drives, as in 1998. On unless set off.",
    ),
    OptionSpec::new(
        "rumble",
        Kind::Range { min: 0, max: 100 },
        "Rumble",
        "The pad shakes on impacts, at this strength in percent. Needs an Xbox-style \
         (XInput) pad. Off: none.",
    )
    .with_value_aliases(&[("off", "0")])
    .with_needs("0 to 100 or off")
    .with_placeholder("<0-100|off>")
    .with_example("60"),
    OptionSpec::new(
        "buttons",
        Kind::Text,
        "Pad buttons",
        "Modern (the default): A selects and is the handbrake, B goes back, Y changes the \
         view, LB and RB change gear, Start pauses. Original: the 1998 order. Or eight button \
         numbers for select, back, change view, gear down, handbrake, gear up, (unused), pause.",
    )
    .with_needs("original, modern or 8 button numbers")
    .with_placeholder("<layout>")
    .with_example("original"),
    OptionSpec::new(
        "close_quits",
        Kind::Toggle,
        "Closing quits",
        "Closing the game window (Alt+F4, the window's close button) quits the game. Off: \
         the 1998 behaviour, where closing does nothing. On unless set off.",
    ),
];

/// The row for `key`; every key the launcher uses has one.
pub fn option(key: &str) -> &'static OptionSpec {
    spec::find(OPTIONS, key).unwrap_or_else(|| panic!("no option row for {key}"))
}

/// The file written on first run: a commented line for every option.
pub fn template() -> String {
    spec::template(
        "Colin McRae Rally launcher settings.\n\
         \n\
         One setting per line as key = value; lines starting with # are comments. \
         A setting left unset keeps the game's own value (or the default named). \
         Command-line options (--resolution 1920x1080, ...) override this file for one run. \
         A launcher.cfg next to CMRLauncher.exe is used instead of this one when it exists \
         (portable installs).",
        OPTIONS,
    )
}

/// The launcher's own switches, which take no value.
pub const SWITCHES: [&str; 4] = ["--settings", "--linux", "--no-window", "--check"];

/// `--help`: the options from [`OPTIONS`], the switches and exit codes.
pub fn usage() -> String {
    format!(
        "CMRLauncher {}\n\n\
         CMRLauncher [options]\n\n\
         {}\n\n\
         These override launcher.cfg for one run (only --game is remembered);\n\
         every one is also a launcher.cfg key (--hd-art is hd_art) and a control\n\
         in the settings window. Without options the settings window opens first.\n\n\
         \x20 --no-window              play with the saved settings, no window\n\
         \x20 --settings [--linux]     only the settings window (exit 0: Play pressed,\n\
         \x20                          3: closed without Play, 4: no window possible);\n\
         \x20                          --linux adds the gamescope scaler, for cmr-play\n\
         \x20 --check                  checks the game folder and its Rally.exe without\n\
         \x20                          playing; the report goes to the terminal and\n\
         \x20                          launcher.log (exit 0: ready, 1: unknown Rally.exe)\n\
         \x20 --controllers [seconds]  lists the joysticks the game can see and logs every\n\
         \x20                          stick, trigger and button you move for that long\n\
         \x20                          (default 30) to launcher.log\n\
         \x20 --help                   this text\n\n\
         The log is %LOCALAPPDATA%\\CMRLauncher\\launcher.log.",
        env!("CARGO_PKG_VERSION"),
        spec::usage(OPTIONS, KeyStyle::Underscore).trim_end(),
    )
}

/// Why a parsed value can't be used: only an option row whose kind and
/// key disagree (a bug in [`OPTIONS`]).
fn mismatch() -> Rejected {
    Rejected::Unknown
}

fn text(v: Value<'_>) -> Result<&str, Rejected> {
    match v {
        Value::Text(t) => Ok(t),
        _ => Err(mismatch()),
    }
}

fn toggle(v: Value) -> Result<bool, Rejected> {
    match v {
        Value::Toggle(on) => Ok(on),
        _ => Err(mismatch()),
    }
}

fn number(v: Value) -> Result<u32, Rejected> {
    match v {
        Value::Number(n) => Ok(n),
        _ => Err(mismatch()),
    }
}

fn choice<T>(v: Value, from: fn(&str) -> Option<T>) -> Result<T, Rejected> {
    match v {
        Value::Choice(c) => from(c).ok_or_else(mismatch),
        _ => Err(mismatch()),
    }
}

/// A factor 1.0..=3.0 as Q16.
fn parse_factor(v: &str) -> Option<u32> {
    let f: f64 = v.trim().parse().ok()?;
    (1.0..=3.0)
        .contains(&f)
        .then(|| (f * 65536.0).round() as u32)
}

/// The `draw_distance` value for a Q16 factor.
pub fn factor_value(q16: u32) -> String {
    format!("{}", f64::from(q16) / 65536.0)
}

impl Settings {
    /// Sets one value by its `launcher.cfg` key, already checked against
    /// its row in [`OPTIONS`]. The command line's `--some-key` is
    /// `some_key`, so the file and the flags read alike.
    fn apply(&mut self, key: &str, value: Value) -> Result<(), Rejected> {
        let needs = || Rejected::needs(option(key).needs());
        match key {
            "game" => self.game = Some(text(value)?.into()),
            "runtime" => self.runtime = Some(text(value)?.into()),
            "saves" => self.saves = Some(text(value)?.into()),
            "language" => self.language = Some(choice(value, |c| Some(c.to_string()))?),
            "look" => {
                self.look = Some(choice(value, |c| {
                    LOOKS.iter().find(|(v, _)| *v == c).map(|(v, _)| *v)
                })?)
            }
            "close_quits" => self.close_quits = Some(toggle(value)?),
            "gamepad_axes" => self.gamepad_axes = Some(toggle(value)?),
            "widescreen" => self.widescreen = Some(toggle(value)?),
            "vsync" => self.vsync = Some(toggle(value)?),
            "smooth_distance" => self.smooth_distance = Some(toggle(value)?),
            "resolution" => {
                let Value::Size(r) = value else {
                    return Err(mismatch());
                };
                self.resolution = Some(match r {
                    Resolution::Size(w, h) => (w, h),
                    Resolution::Original | Resolution::Fullscreen => ORIGINAL,
                });
            }
            "draw_distance" => {
                self.draw_distance = Some(parse_factor(text(value)?).ok_or_else(needs)?)
            }
            "fps_limit" => self.fps_limit = Some(number(value)?),
            "rumble" => self.rumble = Some(number(value)?),
            "texture_filter" => self.texture_filter = Some(choice(value, TextureFilter::from_key)?),
            "scaler" => self.scaler = Some(choice(value, Scaler::from_key)?),
            "buttons" => self.buttons = Some(parse_buttons(text(value)?).ok_or_else(needs)?),
            "hd_art" => self.hd_art = Some(HdArt::parse(text(value)?)),
            _ => return Err(Rejected::Unknown),
        }
        Ok(())
    }

    /// Reads a settings file; the second value lists lines it could not use.
    pub fn parse_file(text: &str) -> (Settings, Vec<String>) {
        let mut s = Settings::default();
        let warnings = spec::parse_file(text, FILE_NAME, OPTIONS, |k, v| s.apply(k, v));
        (s, warnings)
    }

    /// The command line: `--key value` settings and the launcher's own
    /// [`SWITCHES`] (returned in order), in one pass.
    pub fn parse_args<I: IntoIterator<Item = String>>(
        args: I,
    ) -> Result<(Settings, Vec<String>), ArgError> {
        let mut s = Settings::default();
        let switches = spec::parse_args(args, KeyStyle::Underscore, OPTIONS, &SWITCHES, |k, v| {
            s.apply(k, v)
        })?;
        Ok((s, switches))
    }

    /// Command-line values override the file's.
    pub fn merged(self, over: Settings) -> Settings {
        Settings {
            game: over.game.or(self.game),
            runtime: over.runtime.or(self.runtime),
            saves: over.saves.or(self.saves),
            language: over.language.or(self.language),
            look: over.look.or(self.look),
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
}

/// Sets `key` in a settings file, keeping every other line (and comments)
/// as it was; turns `key` off by commenting its lines out.
pub use rea_launcher::cfg::{clear_key, set_key};

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Result<Settings, String> {
        Settings::parse_args(a.iter().map(|s| s.to_string()))
            .map(|(s, _)| s)
            .map_err(|e| e.to_string())
    }

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
        let s = args(&["--hd-art", "on"]).unwrap();
        assert_eq!(s.hd_art, Some(HdArt::Bundled));
        assert!(!s.skipped_fixes().contains(&"F27"));
        // The command line's off wins over the file's on.
        let file = Settings::parse_file("hd_art = on").0;
        let off = args(&["--hd-art", "off"]).unwrap();
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

    #[test]
    fn the_languages_are_the_games() {
        let keys: Vec<_> = LANGUAGE_CHOICES.iter().map(|(k, _)| *k).collect();
        assert_eq!(keys, crate::registry::LANGUAGES);
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
    fn switches_come_back_in_order_and_values_stay_values() {
        let (s, switches) = Settings::parse_args(
            [
                "--settings",
                "--linux",
                "--vsync",
                "on",
                "--no-window",
                "--check",
            ]
            .map(String::from),
        )
        .unwrap();
        assert_eq!(
            switches,
            ["--settings", "--linux", "--no-window", "--check"]
        );
        assert_eq!(s.vsync, Some(true));
        let (s, switches) = Settings::parse_args(["--game", "--check"].map(String::from)).unwrap();
        assert!(switches.is_empty());
        assert_eq!(s.game, Some(PathBuf::from("--check")));
    }

    /// Scripts pass `--key=value` and the underscore spelling of a
    /// hyphenated flag; both read.
    #[test]
    fn equals_and_underscore_flag_spellings_read() {
        let s = args(&["--scaler=fsr", "--draw_distance", "2", "--fps-limit=off"]).unwrap();
        assert_eq!(s.scaler, Some(Scaler::Fsr));
        assert_eq!(s.draw_distance, Some(0x20000));
        assert_eq!(s.fps_limit, Some(0));
    }

    #[test]
    fn bad_flags_say_what_they_need() {
        for (given, message) in [
            (
                &["--vsync", "maybe"][..],
                "--vsync needs on or off, not maybe",
            ),
            (&["--fps-limit"], "--fps-limit needs a value"),
            (
                &["--fps-limit", "5"],
                "--fps-limit needs 10 to 1000 or off, not 5",
            ),
            (
                &["--draw-distance", "9"],
                "--draw-distance needs 1.0 to 3.0, not 9",
            ),
            (
                &["--rumble", "150"],
                "--rumble needs 0 to 100 or off, not 150",
            ),
            (
                &["--texture-filter", "blurry"],
                "--texture-filter needs original, smooth or sharp, not blurry",
            ),
            (
                &["--buttons", "1,2"],
                "--buttons needs original, modern or 8 button numbers, not 1,2",
            ),
            (
                &["--resolution", "8000x100"],
                "--resolution needs WIDTHxHEIGHT, original or off, not 8000x100",
            ),
            (
                &["--scaler", "blurry"],
                "--scaler needs nearest, pixel, fsr or linear, not blurry",
            ),
            (
                &["--look", "modern"],
                "--look needs original, enhanced or steam_deck, not modern",
            ),
            (&["--bogus"], "unknown argument --bogus"),
        ] {
            assert_eq!(args(given).unwrap_err(), message, "{given:?}");
        }
        for help in ["--help", "-h", "/?"] {
            assert!(matches!(
                Settings::parse_args([help.to_string()]),
                Err(ArgError::Help(_))
            ));
        }
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
        let s = args(&["--fps-limit", "120", "--vsync", "off"]).unwrap();
        assert_eq!((s.fps_cap(), s.flip_flags()), (Some(120), 9));
        assert_eq!(args(&["--fps-limit", "off"]).unwrap().fps_cap(), None);
        assert_eq!(args(&["--fps-limit", "0"]).unwrap().fps_cap(), None);
        let smooth = args(&["--texture-filter", "smooth"])
            .unwrap()
            .skipped_fixes();
        assert!(!smooth.contains(&"F18") && smooth.contains(&"F19"));
        let sharp = args(&["--texture-filter", "SHARP"])
            .unwrap()
            .skipped_fixes();
        assert!(sharp.contains(&"F18") && !sharp.contains(&"F19"));
        let r = args(&["--rumble", "60"]).unwrap();
        assert_eq!(r.rumble_strength(), 60);
        assert!(!r.skipped_fixes().contains(&"F22") && !r.skipped_fixes().contains(&"F17"));
        assert_eq!(args(&["--rumble", "off"]).unwrap().rumble_strength(), 0);
        let eased = args(&["--smooth-distance", "on"]).unwrap();
        assert!(!eased.skipped_fixes().contains(&"F20"));
        // A cap alone needs the present hook (F17), not the race resolution.
        let cap = args(&["--fps-limit", "60"]).unwrap();
        assert!(cap.skipped_fixes().contains(&"F16") && !cap.skipped_fixes().contains(&"F17"));
        assert_eq!(
            args(&["--scaler", "pixel"]).unwrap().scaler,
            Some(Scaler::Pixel)
        );
        assert_eq!(
            args(&["--look", "steam_deck"]).unwrap().look,
            Some("steam_deck")
        );

        // `original` on the command line overrides a file's resolution.
        let (file, _) = Settings::parse_file("resolution = 1920x1080\n");
        let m = file.merged(args(&["--resolution", "original"]).unwrap());
        assert_eq!(m.race_resolution(), None);
        assert!(m.skipped_fixes().contains(&"F10"));
    }

    #[test]
    fn template_parses_to_the_defaults() {
        let (s, warnings) = Settings::parse_file(&template());
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(s, Settings::default());
        // close_quits and gamepad_axes are on unless set off.
        assert_eq!(
            s.skipped_fixes(),
            [
                "F10", "F13", "F16", "F11", "F12", "F25", "F17", "F18", "F19", "F20", "F22", "F27"
            ]
        );
        for row in spec::active(OPTIONS) {
            assert!(
                template().contains(&format!("\n#{} =", row.key)),
                "{}",
                row.key
            );
        }
    }

    /// The template launcher 0.7 wrote (with its active `close_quits =
    /// true` and `gamepad_axes = true` lines) still reads, warning-free,
    /// to the same settings.
    #[test]
    fn a_launcher_cfg_from_0_7_still_reads() {
        let old = include_str!("../tests/fixtures/launcher-0.7.cfg");
        let (s, warnings) = Settings::parse_file(old);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(s.close_quits, Some(true));
        assert_eq!(s.gamepad_axes, Some(true));
        assert_eq!(
            s.skipped_fixes(),
            Settings::parse_file(&template()).0.skipped_fixes()
        );
        // The game folder is filled in place.
        let filled = set_key(old, "game", r"D:\CMR");
        assert_eq!(filled.lines().count(), old.lines().count());
        assert!(filled.contains("\ngame = D:\\CMR\n"));
    }

    #[test]
    fn file_values_and_args_win() {
        let text = "game = C:\\old\nruntime=C:\\rt\nsaves = D:\\My Saves\nlanguage = french\nclose_quits = no\ngamepad_axes=off\nresolution = 1920X1440\ndraw_distance=1.5\n";
        let (saved, warnings) = Settings::parse_file(text);
        assert!(warnings.is_empty(), "{warnings:?}");
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
    fn file_reports_bad_values_and_unknown_keys() {
        let (s, warnings) = Settings::parse_file(
            "# game = C:\\commented\nfuture=1\ngame = C:\\G \n\nruntime=\nclose_quits = maybe\nlanguage=xx\nresolution=8000x100\n",
        );
        assert_eq!(s.game, Some(PathBuf::from(r"C:\G")));
        assert_eq!(s.runtime, None);
        assert_eq!(s.close_quits, None);
        // The unknown key is reported (and kept in the file), then the
        // three bad values.
        assert_eq!(warnings.len(), 4, "{warnings:?}");
        assert!(warnings[0].contains("future"), "{warnings:?}");
        assert!(warnings[1].contains("line 6"), "{warnings:?}");
    }

    /// A launcher.cfg saved by Notepad in a Windows code page (Latin-1),
    /// with CRLF and accented folder names, reads as text.
    #[test]
    fn a_code_page_file_with_crlf_reads() {
        let bytes = b"game = C:\\Jogos d'A\xe7\xe3o\\CMR\r\nvsync = on\r\n".to_vec();
        let text = cfg::decode(bytes);
        let (s, warnings) = Settings::parse_file(&text);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(s.game, Some(PathBuf::from("C:\\Jogos d'Ação\\CMR")));
        assert_eq!(s.vsync, Some(true));
    }
}
