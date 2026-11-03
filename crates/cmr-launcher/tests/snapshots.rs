//! Golden snapshots of everything the launcher produces (SDK STANDARDS,
//! "Golden snapshots"): `--help`, the `launcher.cfg` template, and for each
//! look and a few option combinations the launch plan — the fixes applied
//! to each known edition, the launch-time values written into them, the
//! registry values, the HD art set and the Linux helper's scaler. No game
//! bytes: only what the launcher generates.
//!
//! A diff is reviewed like code. To accept an intended change:
//! `CMR_UPDATE_SNAPSHOTS=1 cargo test --test snapshots` (or
//! `REA_UPDATE_SNAPSHOTS=1`), then read the diff.

use std::fmt::Write as _;
use std::path::Path;

use cmr_launcher::config::{self, Settings};
use cmr_launcher::edition::EDITIONS;
use cmr_launcher::form::{self, Form, Look};
use cmr_launcher::plan::{self, Env, Target};

/// Compares with `tests/snapshots/<name>` through the SDK helper, which
/// stores this crate's version as `<version>` and treats CRLF as LF.
fn check(name: &str, actual: &str) {
    rea_launcher::snapshots!()
        .update_var("CMR_UPDATE_SNAPSHOTS")
        .assert(name, actual);
}

const SCREEN: Option<(u32, u32)> = Some((2560, 1440));

const WINDOWS: Env = Env {
    wine: false,
    hooks: true,
    axes: [0; 16],
};

/// A pad whose Z axis (combined triggers) rests at centre, on Wine.
fn wine_with_pad() -> Env {
    let mut axes = [0; 16];
    axes[0] = cmr_launcher::fixes::AXIS_Z;
    Env {
        wine: true,
        hooks: true,
        axes,
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The plan for `settings` on `env`, as text.
fn describe(settings: &Settings, env: &Env) -> String {
    let p = plan::plan(settings, env);
    let mut out = String::new();
    writeln!(out, "env: wine {}, hooks {}", env.wine, env.hooks).unwrap();
    writeln!(out, "skipped: {}", p.skip.join(" ")).unwrap();
    writeln!(out, "hook library: {}", p.hooks).unwrap();
    for e in &EDITIONS {
        writeln!(
            out,
            "applied ({}): {}",
            e.value.name,
            p.applied(e.value.fixes).join(" ")
        )
        .unwrap();
    }
    for fill in &p.fills {
        for (at, bytes) in &fill.parts {
            let at = match at {
                Target::Cave(off) => format!("cave+0x{off:x}"),
                Target::Image(a) => format!("0x{a:08x}"),
            };
            writeln!(out, "fill {} {at} = {}", fill.fix, hex(bytes)).unwrap();
        }
    }
    for v in p.registry(r"C:\Users\José\AppData\Local\CMRLauncher\Rally") {
        writeln!(out, "registry {} = {} ({:?})", v.name, v.data, v.when).unwrap();
    }
    let set = p.hd_set(settings, Path::new("<runtime>/hd/<pack>"));
    writeln!(
        out,
        "CMR_HD_ART: {}",
        set.map_or("-".to_string(), |s| s
            .display()
            .to_string()
            .replace('\\', "/"))
    )
    .unwrap();
    writeln!(
        out,
        "cmr-play scaler: {}",
        settings.scaler.map_or("linear", |s| s.key())
    )
    .unwrap();
    out
}

/// The settings a look saves from the window, on the 2560x1440 screen.
fn look(look: Look) -> Settings {
    let mut f = Form::new(&Settings::default(), SCREEN);
    f.set_look(look, SCREEN);
    let saved = f.save(&config::template());
    let (s, warnings) = Settings::parse_file(&saved);
    assert!(warnings.is_empty(), "{warnings:?}");
    s
}

#[test]
fn help() {
    // The helper writes the release's version as `<version>`.
    check("help.txt", &config::usage());
}

#[test]
fn template() {
    check("launcher.cfg", &config::template());
}

#[test]
fn looks() {
    check(
        "look-original.txt",
        &describe(&look(Look::Original), &WINDOWS),
    );
    check(
        "look-enhanced.txt",
        &describe(&look(Look::Enhanced), &WINDOWS),
    );
    check(
        "look-steam-deck.txt",
        &describe(&look(Look::SteamDeck), &wine_with_pad()),
    );
}

/// `--look enhanced` from the command line gives the window's Enhanced.
#[test]
fn the_look_flag_matches_the_window() {
    let cli = Settings::parse_args(["--look".to_string(), "enhanced".to_string()])
        .unwrap()
        .0;
    let run = form::with_look(Settings::default(), cli, SCREEN);
    assert_eq!(
        describe(&run, &WINDOWS),
        describe(&look(Look::Enhanced), &WINDOWS)
    );
}

#[test]
fn combinations() {
    let parse = |text: &str| {
        let (s, warnings) = Settings::parse_file(text);
        assert!(warnings.is_empty(), "{warnings:?}");
        s
    };
    check(
        "combo-pad-custom.txt",
        &describe(
            &parse(
                "resolution = 1920x1080\nwidescreen = on\nfps_limit = 144\nvsync = off\n\
                 rumble = 100\nbuttons = 3,2,1,4,5,6,7,8\ntexture_filter = sharp\n\
                 language = GERMAN\nscaler = nearest\nhd_art = D:\\HD art\n",
            ),
            &wine_with_pad(),
        ),
    );
    check(
        "combo-original-handling.txt",
        &describe(
            &parse("gamepad_axes = off\nclose_quits = off\nbuttons = original\n"),
            &WINDOWS,
        ),
    );
    check(
        "combo-no-hook-library.txt",
        &describe(
            &parse("resolution = 3840x2160\nhd_art = on\nrumble = 60\ndraw_distance = 3\n"),
            &Env {
                hooks: false,
                ..WINDOWS
            },
        ),
    );
}
