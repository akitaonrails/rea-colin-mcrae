//! Command line and the settings file (`launcher.cfg`).
//!
//! The file is plain `key = value` lines with `#` comments, written once from
//! a commented template so users can edit it. The launcher later changes
//! only the lines it owns (`game`), keeping everything else as the user left
//! it. Unknown keys are ignored so older launchers can read newer files;
//! values it cannot read are reported and ignored.

use std::path::PathBuf;

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
";

pub const USAGE: &str = "\
CMRLauncher [options]

  --game <folder>          the folder with your installed Rally.exe (remembered)
  --saves <folder>         where your saves are kept
  --runtime <folder>       where the launcher keeps its private copy
  --language <name>        ENGLISH, FRENCH, GERMAN, SPANISH or ITALIAN
  --resolution <WxH>       race resolution, e.g. 1920x1080, or original
  --widescreen on|off      wider view on wide screens
  --draw-distance <n>      1.0 to 3.0
  --gamepad-axes on|off    triggers/right stick as pedals, Start pauses
  --close-quits on|off     closing the window quits the game

These override launcher.cfg for one run (only --game is remembered).

CMRLauncher --check [--game <folder>]
  checks the game folder and tells you whether its Rally.exe is supported

CMRLauncher --controllers [seconds]
  lists the joysticks the game can see and logs every stick, trigger and
  button you move for that long (default 30) to launcher.log

Other settings are in launcher.cfg (see the comments in it).";

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
        }
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
        }
        if self.race_resolution().is_none() {
            skip.push("F10");
            skip.push("F13");
        }
        if self.widescreen != Some(true) {
            skip.push("F11");
        }
        if self.draw_distance.is_none() {
            skip.push("F12");
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
                "--draw-distance" => {
                    let v = value("--draw-distance")?;
                    s.draw_distance = Some(parse_factor(&v).ok_or_else(|| {
                        format!("--draw-distance needs 1.0 to 3.0, not {v}\n\n{USAGE}")
                    })?);
                }
                flag @ ("--widescreen" | "--gamepad-axes" | "--close-quits") => {
                    let v = value(flag)?;
                    let b = parse_bool(&v)
                        .ok_or_else(|| format!("{flag} needs on or off, not {v}\n\n{USAGE}"))?;
                    match flag {
                        "--widescreen" => s.widescreen = Some(b),
                        "--gamepad-axes" => s.gamepad_axes = Some(b),
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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(s.skipped_fixes(), ["F6", "F7", "F14"]);
        assert!(args(&["--widescreen", "maybe"]).is_err());
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
        assert_eq!(s.skipped_fixes(), ["F10", "F13", "F11", "F12"]);
    }

    #[test]
    fn file_values_and_args_win() {
        let text = "game = C:\\old\nruntime=C:\\rt\nsaves = D:\\My Saves\nlanguage = french\nclose_quits = no\ngamepad_axes=off\nresolution = 1920X1440\ndraw_distance=1.5\n";
        let (saved, warnings) = Settings::parse_file(text);
        assert!(warnings.is_empty());
        assert_eq!(saved.language.as_deref(), Some("FRENCH"));
        assert_eq!(saved.resolution, Some((1920, 1440)));
        assert_eq!(saved.draw_distance, Some(0x18000));
        assert_eq!(saved.skipped_fixes(), ["F6", "F7", "F14", "F11"]);
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
