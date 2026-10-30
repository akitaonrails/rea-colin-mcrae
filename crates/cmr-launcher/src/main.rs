#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod win32;

#[cfg(windows)]
fn main() {
    let code = match app::run() {
        Ok(code) => code,
        Err(err) => {
            app::log_line(&format!("error: {err}"));
            win32::message("Colin McRae Rally launcher", &err, true);
            1
        }
    };
    std::process::exit(code as i32);
}

#[cfg(not(windows))]
fn main() {
    eprintln!("CMRLauncher is a Windows program; on Linux run it with Proton or Wine.");
    std::process::exit(2);
}

#[cfg(windows)]
mod app {
    use std::fs;
    use std::io::Write as _;
    use std::path::{Path, PathBuf};

    use cmr_launcher::config::{self, Settings};
    use cmr_launcher::{edition, fill_data, fixes, pe, prepare, registry, runtime};

    use crate::win32;

    const EXE: &str = "Rally.exe";
    const HOOKS_DLL: &str = "cmrhooks.dll";

    fn home() -> PathBuf {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("CMRLauncher")
    }

    pub fn log_line(line: &str) {
        let dir = home();
        let _ = fs::create_dir_all(&dir);
        if let Ok(mut f) = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("launcher.log"))
        {
            let _ = writeln!(f, "{line}");
        }
    }

    /// A `launcher.cfg` next to the exe wins (portable installs); otherwise
    /// the one in the launcher's home folder.
    fn settings_path() -> PathBuf {
        let portable = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join(config::FILE_NAME)));
        match portable {
            Some(p) if p.is_file() => p,
            _ => home().join(config::FILE_NAME),
        }
    }

    /// `%USERPROFILE%\Saved Games\Colin McRae Rally`, Windows' usual place
    /// for game saves (inside the prefix under Wine/Proton).
    fn default_saves() -> PathBuf {
        std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .unwrap_or_else(home)
            .join("Saved Games")
            .join("Colin McRae Rally")
    }

    fn find_game(settings: &Settings) -> Result<PathBuf, String> {
        if let Some(g) = &settings.game {
            return Ok(g.clone());
        }
        // A launcher placed in the game folder needs no argument.
        if let Some(dir) = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            && dir.join(EXE).is_file()
        {
            return Ok(dir);
        }
        Err(format!(
            "Tell the launcher where the game is: run it once with\n\n  CMRLauncher --game <folder with {EXE}>\n\nThe folder is remembered."
        ))
    }

    pub fn run() -> Result<u32, String> {
        let mut args = std::env::args().skip(1).peekable();
        if args.peek().map(String::as_str) == Some("--controllers") {
            args.next();
            let seconds = args.next().and_then(|s| s.parse().ok()).unwrap_or(30);
            win32::controllers(seconds, &mut |line| log_line(line));
            win32::message(
                "Colin McRae Rally launcher",
                &format!(
                    "Controller report written to {}",
                    home().join("launcher.log").display()
                ),
                false,
            );
            return Ok(0);
        }
        log_line(concat!("CMRLauncher ", env!("CARGO_PKG_VERSION")));
        let settings_path = settings_path();
        let existing = fs::read_to_string(&settings_path).ok();
        let file_text = existing.as_deref().unwrap_or(config::TEMPLATE);
        let (saved, warnings) = Settings::parse_file(file_text);
        for w in &warnings {
            log_line(&format!("warning: {w}"));
        }
        let mut cli: Vec<String> = std::env::args().skip(1).collect();
        let check_only = cli.iter().any(|a| a == "--check");
        cli.retain(|a| a != "--check");
        let settings = saved.clone().merged(Settings::parse_args(cli)?);

        let game = find_game(&settings)?;
        let exe_bytes = fs::read(game.join(EXE))
            .map_err(|e| format!("cannot read {}: {e}", game.join(EXE).display()))?;
        let edition = edition::identify(&exe_bytes).map_err(|digest| {
            format!(
                "This {EXE} is not a version the launcher knows yet.\n\nSHA-256: {}",
                edition::to_hex(&digest)
            )
        })?;
        let image = pe::Image::parse(&exe_bytes).map_err(|e| e.to_string())?;
        log_line(&format!("game: {} ({})", game.display(), edition.name));
        if check_only {
            win32::message(
                "Colin McRae Rally launcher",
                &format!(
                    "Ready to play.\n\nGame folder: {}\nVersion: {}",
                    game.display(),
                    edition.name
                ),
                false,
            );
            return Ok(0);
        }

        let rt = settings
            .runtime
            .clone()
            .unwrap_or_else(|| home().join("Rally"));
        runtime::check_separate(&game, &rt)?;
        let copied = runtime::sync(&game, &rt)
            .map_err(|e| format!("cannot prepare {}: {e}", rt.display()))?;
        log_line(&format!(
            "runtime: {} ({copied} files copied)",
            rt.display()
        ));
        // The player's saves live outside the runtime copy; the game reads
        // and writes its own `save` folder, kept in step before and after.
        let saves = settings.saves.clone().unwrap_or_else(default_saves);
        let rt_save = runtime::save_dir(&rt).map_err(|e| format!("cannot prepare saves: {e}"))?;
        if !saves.is_dir() {
            // First run with this folder: start it from what the game has.
            let n = runtime::mirror(&rt_save, &saves)
                .map_err(|e| format!("cannot create {}: {e}", saves.display()))?;
            log_line(&format!("saves: created {} ({n} files)", saves.display()));
        }
        let n = runtime::mirror(&saves, &rt_save)
            .map_err(|e| format!("cannot read saves from {}: {e}", saves.display()))?;
        log_line(&format!("saves: {} ({n} files updated)", saves.display()));

        if let Ok(listing) = runtime::list(&game)
            && let Some((len, rel)) = runtime::longest_path(&rt, &listing)
            && len > runtime::MAX_GAME_PATH
        {
            log_line(&format!(
                "warning: {rel} is {len} characters below the runtime folder; the game opens at most {}",
                runtime::MAX_GAME_PATH
            ));
        }

        let language = settings
            .language
            .clone()
            .unwrap_or_else(|| "ENGLISH".into());
        let root = rt.display().to_string();
        let place = win32::write_registry(&registry::values(&root, &language))?;
        log_line(&format!(
            "registry written ({place}); wine: {}",
            win32::is_wine()
        ));

        // Remember the game folder; the rest of the file stays as written.
        if existing.is_none() || saved.game.as_ref() != Some(&game) {
            let text = config::set_key(file_text, "game", &game.display().to_string());
            if let Err(e) = fs::write(&settings_path, text) {
                log_line(&format!(
                    "warning: cannot write {}: {e}",
                    settings_path.display()
                ));
            }
        }

        let mut skip = settings.skipped_fixes();
        // These fixes need the hook library next to the launcher.
        let hooks_path = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join(HOOKS_DLL)));
        let mut uses_hooks = fixes::HOOK_FIXES.iter().any(|f| !skip.contains(f));
        if uses_hooks && !hooks_path.as_ref().is_some_and(|p| p.is_file()) {
            log_line(&format!(
                "warning: {HOOKS_DLL} not found next to the launcher; the options that need it are off"
            ));
            let off: Vec<_> = fixes::HOOK_FIXES
                .iter()
                .filter(|f| !skip.contains(f))
                .collect();
            skip.extend(off);
            uses_hooks = false;
        }
        if !skip.is_empty() {
            log_line(&format!(
                "fixes turned off by settings: {}",
                skip.join(", ")
            ));
        }
        let code = win32::run_patched(
            &rt.join(EXE),
            &rt,
            edition.fixes.cave_size(&skip),
            hooks_path
                .as_deref()
                .filter(|_| uses_hooks)
                .map(|path| win32::HookLibrary {
                    path,
                    table: fixes::F13_TABLE,
                    exports: &fixes::F13_EXPORTS,
                }),
            |cave| {
                let mut writes = prepare(&image, edition.fixes, &skip, cave)?;
                let pads = win32::xinput_pads();
                log_line(&if pads.is_empty() {
                    "XInput pads: none (rumble and separate trigger pedals need an Xbox-style pad; on 8BitDo pads use X-input mode)".to_string()
                } else {
                    format!("XInput pads: {pads:?}")
                });
                let axes = win32::centred_axes();
                let bytes: Vec<u8> = axes.iter().flat_map(|a| a.to_le_bytes()).collect();
                if fill_data(&mut writes, cave.wrapping_add(fixes::F7_TABLE), &bytes) {
                    log_line(&format!(
                        "F7 axes per joystick (1 = Z, 2 = R): {:?}",
                        &axes[..4]
                    ));
                }
                if let Some(factor) = settings.draw_distance {
                    let q16 = |v: u32| ((u64::from(v) * u64::from(factor)) >> 16) as u32;
                    let cap = q16(fixes::F12_CAP).to_le_bytes();
                    let filled = fill_data(
                        &mut writes,
                        cave.wrapping_add(fixes::F12_FACTOR),
                        &factor.to_le_bytes(),
                    ) && fixes::F12_CAPS
                        .iter()
                        .all(|&a| fill_data(&mut writes, a, &cap));
                    if filled {
                        log_line(&format!(
                            "F12 draw distance x{:.2}",
                            f64::from(factor) / 65536.0
                        ));
                    }
                }
                if !skip.contains(&"F11") {
                    let (w, h) = settings.race_resolution().unwrap_or((640, 480));
                    let t = fixes::cull_t(w, h).to_le_bytes();
                    let filled = fill_data(&mut writes, cave.wrapping_add(fixes::F11_CULL_T), &t)
                        && fixes::F11_CULL_IMMS
                            .iter()
                            .all(|&a| fill_data(&mut writes, a, &t));
                    if filled {
                        log_line(&format!(
                            "F11 culling widened x{:.3}",
                            f64::from(fixes::cull_t(w, h)) / 65536.0
                        ));
                    }
                }
                if !skip.contains(&"F17") {
                    let cap = settings.fps_cap().unwrap_or(0);
                    let block: Vec<u8> = [cap, settings.flip_flags(), settings.rumble_strength()]
                        .iter()
                        .flat_map(|v| v.to_le_bytes())
                        .collect();
                    if fill_data(&mut writes, cave.wrapping_add(fixes::F17_SETTINGS), &block) {
                        log_line(&format!(
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
                        ));
                    }
                }
                if let Some((w, h)) = settings.race_resolution() {
                    let size: Vec<u8> = [w, h].iter().flat_map(|v| v.to_le_bytes()).collect();
                    if fill_data(&mut writes, cave.wrapping_add(fixes::F10_SIZE), &size) {
                        log_line(&format!("F10 race resolution {w}x{h}"));
                    }
                }
                Ok(writes)
            },
            &mut |line| log_line(line),
        )?;
        log_line(&format!("game exited with {code}"));
        // The hook library writes crash.log in the game's folder on a crash.
        let crash = rt.join("crash.log");
        if let Ok(report) = std::fs::read_to_string(&crash) {
            log_line(&format!("the game crashed: {}", report.trim()));
            let _ = std::fs::remove_file(&crash);
        }
        if settings.rumble_strength() > 0 {
            win32::stop_rumble();
        }
        match runtime::mirror(&rt_save, &saves) {
            Ok(n) => log_line(&format!("saves: {n} files written back")),
            Err(e) => log_line(&format!(
                "warning: saves not written back to {}: {e}",
                saves.display()
            )),
        }
        Ok(code)
    }
}
