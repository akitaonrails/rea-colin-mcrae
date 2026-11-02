//! What a launch does to the game, worked out from the settings alone
//! (`sdk/docs/STANDARDS.md`, "Test the wiring"): which optional fixes are
//! off, the launch-time values written into the fixes (resolution, culling,
//! draw distance, frame-rate cap, vsync, rumble, pad layout, pad axes), the
//! registry values the game reads, and the HD art set. The Windows binary
//! applies it; the tests and golden snapshots (`tests/snapshots.rs`) check
//! it on the host.

use std::path::{Path, PathBuf};

use crate::config::{HdArt, Settings};
use crate::fixes::{self, FixSet};
use crate::{CheckedWrite, fill_data, registry};

/// What the launch runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Env {
    /// Running under Wine or Proton.
    pub wine: bool,
    /// `cmrhooks.dll` is next to the launcher.
    pub hooks: bool,
    /// For F7, per WinMM joystick id, the extra axes (Z, R) resting at
    /// centre now (`fixes::AXIS_Z`, `AXIS_R`).
    pub axes: [u32; 16],
}

/// Where a launch-time value goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// An offset into the fix memory (the cave).
    Cave(u32),
    /// An address in the game's code (an immediate a fix replaces).
    Image(u32),
}

/// One fix's launch-time values: every part must land for it to count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fill {
    pub fix: &'static str,
    /// The `launcher.log` line once it is filled; `None` logs nothing.
    pub log: Option<String>,
    pub parts: Vec<(Target, Vec<u8>)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// Optional fixes off, in the order they were turned off.
    pub skip: Vec<&'static str>,
    /// The hook library is loaded (some fix that needs it is on).
    pub hooks: bool,
    /// `cmrhooks.dll` is missing while settings need it.
    pub hooks_missing: bool,
    /// The launch-time values of the fixes that are on.
    pub fills: Vec<Fill>,
    /// `Language`, seeded when missing.
    pub language: String,
    /// Where the HD art comes from (F27 on).
    pub hd_art: Option<HdArt>,
}

fn le(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// The plan for `settings` on `env`.
pub fn plan(settings: &Settings, env: &Env) -> Plan {
    let mut skip = settings.skipped_fixes();
    // The joystick panel works on Windows; only Wine needs F24.
    if !env.wine {
        skip.push("F24");
    }
    // These fixes need the hook library next to the launcher.
    let mut hooks = fixes::HOOK_FIXES.iter().any(|f| !skip.contains(f));
    let hooks_missing = hooks && !env.hooks;
    if hooks_missing {
        let off: Vec<&'static str> = fixes::HOOK_FIXES
            .iter()
            .copied()
            .filter(|f| !skip.contains(f))
            .collect();
        skip.extend(off);
        hooks = false;
    }

    let mut fills = vec![Fill {
        fix: "F7",
        log: Some(format!(
            "F7 axes per joystick (1 = Z, 2 = R): {:?}",
            &env.axes[..4]
        )),
        parts: vec![(Target::Cave(fixes::F7_TABLE), le(&env.axes))],
    }];
    if let Some(factor) = settings.draw_distance {
        let q16 = |v: u32| ((u64::from(v) * u64::from(factor)) >> 16) as u32;
        let cap = q16(fixes::F12_CAP).to_le_bytes().to_vec();
        let mut parts = vec![(
            Target::Cave(fixes::F12_FACTOR),
            factor.to_le_bytes().to_vec(),
        )];
        parts.extend(
            fixes::F12_CAPS
                .iter()
                .map(|&a| (Target::Image(a), cap.clone())),
        );
        fills.push(Fill {
            fix: "F12",
            log: Some(format!(
                "F12 draw distance x{:.2}",
                f64::from(factor) / 65536.0
            )),
            parts,
        });
    }
    if !skip.contains(&"F11") {
        let (w, h) = settings.race_resolution().unwrap_or((640, 480));
        let t = fixes::cull_t(w, h);
        let mut parts = vec![(Target::Cave(fixes::F11_CULL_T), t.to_le_bytes().to_vec())];
        parts.extend(
            fixes::F11_CULL_IMMS
                .iter()
                .map(|&a| (Target::Image(a), t.to_le_bytes().to_vec())),
        );
        fills.push(Fill {
            fix: "F11",
            log: Some(format!(
                "F11 culling widened x{:.3}",
                f64::from(t) / 65536.0
            )),
            parts,
        });
    }
    if !skip.contains(&"F17") {
        let cap = settings.fps_cap().unwrap_or(0);
        fills.push(Fill {
            fix: "F17",
            log: Some(format!(
                "F17 frame-rate cap {}, vsync {}",
                if cap > 0 {
                    cap.to_string()
                } else {
                    "off".into()
                },
                match settings.vsync {
                    None => "as the game",
                    Some(true) => "on",
                    Some(false) => "off",
                }
            )),
            parts: vec![(
                Target::Cave(fixes::F17_SETTINGS),
                le(&[cap, settings.flip_flags(), settings.rumble_strength()]),
            )],
        });
    }
    let map = settings.button_map();
    fills.push(Fill {
        fix: "F23",
        log: (map != [0; 12]).then(|| format!("F23 button layout {map:?}")),
        parts: vec![(Target::Cave(fixes::F23_MAP), map.to_vec())],
    });
    if let Some((w, h)) = settings.race_resolution() {
        fills.push(Fill {
            fix: "F10",
            log: Some(format!("F10 race resolution {w}x{h}")),
            parts: vec![(Target::Cave(fixes::F10_SIZE), le(&[w, h]))],
        });
    }

    // A fix that is off has no values to fill.
    fills.retain(|f| !skip.contains(&f.fix));
    Plan {
        skip,
        hooks,
        hooks_missing,
        fills,
        language: settings
            .language
            .clone()
            .unwrap_or_else(|| "ENGLISH".into()),
        hd_art: settings
            .hd_art
            .clone()
            .filter(|_| hooks && settings.hd_on()),
    }
}

impl Plan {
    /// The registry values for a runtime folder at `root` (a Windows path).
    pub fn registry(&self, root: &str) -> Vec<registry::Value> {
        registry::values(root, &self.language)
    }

    /// The fixes of `set` this plan applies, by id.
    pub fn applied(&self, set: &FixSet) -> Vec<&'static str> {
        set.active(&self.skip).map(|f| f.id).collect()
    }

    /// The HD art set the hook library reads, below `bundled` (the
    /// installed pack) or the folder `hd_art` names.
    pub fn hd_set(&self, settings: &Settings, bundled: &Path) -> Option<PathBuf> {
        self.hd_art.as_ref()?;
        settings.hd_set(bundled)
    }

    /// Writes the fills into `writes` (the fix memory at `cave`); returns
    /// the log lines of the fills that landed completely.
    pub fn fill(&self, writes: &mut [CheckedWrite], cave: u32) -> Vec<String> {
        let mut log = Vec::new();
        for fill in &self.fills {
            let all = fill.parts.iter().all(|(at, bytes)| {
                let address = match *at {
                    Target::Cave(off) => cave.wrapping_add(off),
                    Target::Image(a) => a,
                };
                fill_data(writes, address, bytes)
            });
            if all && let Some(line) = &fill.log {
                log.push(line.clone());
            }
        }
        log
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;

    const WINDOWS: Env = Env {
        wine: false,
        hooks: true,
        axes: [0; 16],
    };

    fn settings(text: &str) -> Settings {
        Settings::parse_file(text).0
    }

    #[test]
    fn wine_alone_gets_the_joystick_panel_fix() {
        assert!(plan(&Settings::default(), &WINDOWS).skip.contains(&"F24"));
        let wine = Env {
            wine: true,
            ..WINDOWS
        };
        assert!(!plan(&Settings::default(), &wine).skip.contains(&"F24"));
    }

    #[test]
    fn without_the_hook_library_its_fixes_are_off() {
        let s = settings("resolution = 1920x1080\nhd_art = on\nrumble = 60\n");
        let with = plan(&s, &WINDOWS);
        assert!(with.hooks && !with.hooks_missing);
        assert_eq!(with.hd_art, Some(HdArt::Bundled));
        let without = plan(
            &s,
            &Env {
                hooks: false,
                ..WINDOWS
            },
        );
        assert!(!without.hooks && without.hooks_missing);
        for f in fixes::HOOK_FIXES {
            assert!(without.skip.contains(&f), "{f}");
        }
        assert_eq!(without.hd_art, None);
        // F26 is always on, so the library is always wanted.
        assert!(plan(&Settings::default(), &WINDOWS).hooks);
    }

    /// Each launch-time value reaches the bytes of its fix: the resolution
    /// (F10), culling (F11), draw distance (F12), cap/vsync/rumble (F17),
    /// buttons (F23), pad axes (F7).
    #[test]
    fn option_values_reach_their_fixes() {
        let mut env = WINDOWS;
        env.axes[0] = fixes::AXIS_Z;
        let s = settings(
            "resolution = 1920x1080\nwidescreen = on\ndraw_distance = 2\nfps_limit = 120\n\
             vsync = off\nrumble = 60\nbuttons = original\n",
        );
        let p = plan(&s, &env);
        let part = |fix: &str| {
            p.fills
                .iter()
                .find(|f| f.fix == fix)
                .unwrap_or_else(|| panic!("{fix}"))
                .parts
                .clone()
        };
        assert_eq!(
            part("F10"),
            [(Target::Cave(fixes::F10_SIZE), le(&[1920, 1080]))]
        );
        assert_eq!(part("F12")[0].1, 0x20000u32.to_le_bytes());
        assert_eq!(
            part("F12")[1].1,
            (fixes::F12_CAP * 2).to_le_bytes(),
            "the cap doubles"
        );
        assert_eq!(
            part("F11")[0].1,
            fixes::cull_t(1920, 1080).to_le_bytes(),
            "culling for 16:9"
        );
        assert_eq!(
            part("F17"),
            [(Target::Cave(fixes::F17_SETTINGS), le(&[120, 9, 60]))]
        );
        assert_eq!(
            part("F23")[0].1,
            [0u8; 12],
            "original: every button its own"
        );
        assert_eq!(part("F7")[0].1[..4], fixes::AXIS_Z.to_le_bytes());
        // Nothing set: no resolution, culling or cap values.
        let none = plan(&Settings::default(), &WINDOWS);
        for fix in ["F10", "F11", "F12", "F17"] {
            assert!(none.fills.iter().all(|f| f.fix != fix), "{fix}");
        }
    }

    #[test]
    fn the_language_is_seeded_into_the_registry() {
        let p = plan(&settings("language = italian\n"), &WINDOWS);
        let values = p.registry(r"C:\R\");
        let lang = values.iter().find(|v| v.name == "Language").unwrap();
        assert_eq!(
            (lang.data.as_str(), lang.when),
            ("ITALIAN", registry::When::IfMissing)
        );
        assert_eq!(plan(&Settings::default(), &WINDOWS).language, "ENGLISH");
    }

    #[test]
    fn hd_art_reads_the_set_for_the_resolution() {
        let s = settings("hd_art = on\nresolution = 3840x2160\n");
        let p = plan(&s, &WINDOWS);
        assert_eq!(
            p.hd_set(&s, Path::new("P")),
            Some(Path::new("P").join("x4"))
        );
        let off = settings("resolution = 3840x2160\n");
        assert_eq!(plan(&off, &WINDOWS).hd_set(&off, Path::new("P")), None);
    }

    /// Every game-affecting flag, end to end: `--flag value` → the same
    /// settings as its `launcher.cfg` key → a launch plan that differs from
    /// the default one (a fix on or off, a value written, the registry).
    #[test]
    fn every_flag_changes_the_launch() {
        use crate::config::OPTIONS;
        use rea_launcher::cfg::spec;
        let base = plan(&Settings::default(), &WINDOWS);
        let cases = [
            ("language", "french"),
            ("look", "enhanced"),
            ("resolution", "1920x1080"),
            ("widescreen", "on"),
            ("draw_distance", "1.5"),
            ("texture_filter", "smooth"),
            ("fps_limit", "60"),
            ("vsync", "on"),
            ("smooth_distance", "on"),
            ("hd_art", "on"),
            ("gamepad_axes", "off"),
            ("rumble", "30"),
            ("buttons", "original"),
            ("close_quits", "off"),
        ];
        for row in spec::active(OPTIONS) {
            let tested = cases.iter().any(|(k, _)| *k == row.key);
            // Folders, and the helper's scaler (linux/tests/scaler.sh).
            let elsewhere = matches!(row.key, "game" | "saves" | "runtime" | "scaler");
            assert!(tested || elsewhere, "{} has no end-to-end case", row.key);
        }
        for (key, value) in cases {
            let flag = config::option(key).flag_name(rea_launcher::cfg::KeyStyle::Underscore);
            let (cli, _) = Settings::parse_args([flag.clone(), value.to_string()]).unwrap();
            let file = Settings::parse_file(&format!("{key} = {value}\n")).0;
            assert_eq!(cli, file, "{flag} and {key} read alike");
            let run = crate::form::with_look(Settings::default(), cli, Some((2560, 1440)));
            assert_ne!(plan(&run, &WINDOWS), base, "{flag} {value} changes nothing");
        }
    }
}
