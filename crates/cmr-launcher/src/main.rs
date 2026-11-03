#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod ui;
#[cfg(windows)]
mod win32;

#[cfg(windows)]
fn main() {
    let code = match app::run() {
        Ok(code) => code,
        Err(err) => {
            app::log_line(&format!("error: {err}"));
            app::fatal(&err);
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
    use std::path::{Path, PathBuf};

    use cmr_launcher::config::{self, Settings};
    use cmr_launcher::form;
    use cmr_launcher::instance::{self, SecondStart};
    use cmr_launcher::registry::{self, When};
    use cmr_launcher::{Image, edition, fixes, hdpack, plan, prepare, runtime};
    use rea_launcher::cfg::ArgError;
    use rea_launcher::log::Log;
    use rea_launcher::{cfg, fsx, paths};
    use rea_launcher_win32::instance::{self as win32_instance, Window};
    use rea_launcher_win32::reg::Key;
    use rea_launcher_win32::report::{self, Audience, Run};
    use rea_launcher_win32::{display, gpu, wine, xinput};

    use crate::{ui, win32};

    const EXE: &str = "Rally.exe";
    const HOOKS_DLL: &str = "cmrhooks.dll";
    const TITLE: &str = instance::WINDOW_TITLE;

    fn home() -> PathBuf {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("CMRLauncher")
    }

    pub fn log_line(line: &str) {
        Log::new(home().join("launcher.log")).line(line);
    }

    /// Who sees this run's messages (the SDK's rule): under Wine (cmr-play,
    /// the AppImage, Proton) a person even with `--no-window`.
    fn audience() -> Audience {
        let args: Vec<String> = std::env::args().skip(1).collect();
        report::audience(Run::from_args(&args, cmr_launcher::COMMAND_FLAGS))
    }

    /// An error the launcher stops on: a dialog whenever a person may be
    /// watching, and printed (cmr-play's log).
    pub fn fatal(text: &str) {
        report::fatal(TITLE, text, audience().mode());
    }

    /// Information or a warning, routed like [`fatal`].
    fn note(text: &str, warning: bool) {
        report::note(TITLE, text, audience().mode(), warning);
    }

    /// Writes the game's registry values; returns where they went.
    fn write_registry(values: &[registry::Value]) -> Result<&'static str, String> {
        let (key, place) = Key::machine_or_virtual_store(registry::KEY)?;
        for v in values {
            match v.when {
                When::Always => key.set_string(v.name, &v.data)?,
                When::IfMissing => {
                    key.set_string_if_missing(v.name, &v.data)?;
                }
            }
        }
        Ok(place.describe())
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

    /// `--settings` exit codes for cmr-play: closed without playing, or no
    /// window could be shown (play with the saved settings).
    const WINDOW_CLOSED: u32 = 3;
    const WINDOW_UNAVAILABLE: u32 = 4;
    /// A flag the launcher doesn't know, or a value it can't use.
    const BAD_ARGUMENTS: u32 = 2;
    /// `--check` found an executable the launcher doesn't know.
    const CHECK_FAILED: u32 = 1;

    /// The settings file's text: `None` on a first run (no file yet).
    /// A file that cannot be read is never written over.
    fn read_settings(path: &Path) -> Result<Option<String>, String> {
        cfg::read_file(path).map_err(|e| format!("cannot read {}: {e}", path.display()))
    }

    pub fn run() -> Result<u32, String> {
        let mut args = std::env::args().skip(1).peekable();
        if args.peek().map(String::as_str) == Some("--controllers") {
            args.next();
            let seconds = args.next().and_then(|s| s.parse().ok()).unwrap_or(30);
            win32::controllers(seconds, &mut |line| log_line(line));
            note(
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
        let args: Vec<String> = std::env::args().skip(1).collect();
        let given = args.len();
        let (cli, switches) = match Settings::parse_args(args) {
            Ok(parsed) => parsed,
            Err(ArgError::Help(_)) => {
                note(&config::usage(), false);
                return Ok(0);
            }
            Err(e) => {
                log_line(&format!("error: {e}"));
                fatal(&format!("{e}\n\n{}", config::usage()));
                return Ok(BAD_ARGUMENTS);
            }
        };
        let switch = |name: &str| switches.iter().any(|s| s == name);
        // The settings window: shown when started without options (a
        // double click), or alone with --settings (Linux: cmr-play shows
        // it, then starts the game in gamescope). Any other option skips
        // it.
        let settings_only = switch("--settings");
        let linux = switch("--linux");
        let no_window = switch("--no-window");
        let check_only = switch("--check");
        let other_options = given > switches.len() || check_only;
        // One launcher at a time, held until the game this run starts has
        // ended: a second start raises the window or says the game runs
        // (`instance`). --check only reads and runs alongside.
        let _instance = if check_only {
            None
        } else {
            match win32_instance::single(instance::NAME) {
                Ok(guard) => Some(guard),
                Err(running) => {
                    let second = SecondStart::new(
                        running.bring_to_front(Window::Title(instance::WINDOW_TITLE)),
                    );
                    log_line(&format!("another launcher is running: {second:?}"));
                    // A play-time message: the player started a second
                    // copy and must see why nothing happened.
                    if let Some(message) = second.message() {
                        let interactive = audience().interactive;
                        report::note(TITLE, message, report::Mode::Play { interactive }, true);
                    }
                    // cmr-play --settings: nothing was started.
                    return Ok(if settings_only { WINDOW_CLOSED } else { 0 });
                }
            }
        };
        let template = config::template();
        if settings_only || (!other_options && !no_window) {
            // A window under Wine draws through OpenGL: GLX, not Wine 11's
            // default EGL (software on NVIDIA boxes without egl-x11).
            if wine::is_wine()
                && let Ok(true) = gpu::prefer_glx()
            {
                log_line("Wine: OpenGL through GLX (UseEGL=N)");
            }
            let read = read_settings(&settings_path);
            let text = match &read {
                Ok(text) => Ok(text.as_deref()),
                Err(why) => Err(why.as_str()),
            };
            match ui::show(&settings_path, text, &template, linux) {
                Ok(ui::Outcome::Play) => {
                    log_line("settings window: play");
                    if settings_only {
                        return Ok(0);
                    }
                }
                Ok(ui::Outcome::Closed) => {
                    log_line("settings window: closed");
                    return Ok(if settings_only { WINDOW_CLOSED } else { 0 });
                }
                Err(e) => {
                    // No window possible (no OpenGL): play with the saved
                    // settings.
                    log_line(&format!("settings window unavailable: {e}"));
                    if settings_only {
                        return Ok(WINDOW_UNAVAILABLE);
                    }
                }
            }
        }
        // Unreadable (not missing): played with the defaults, never
        // written over.
        let (existing, writable) = match read_settings(&settings_path) {
            Ok(text) => (text, true),
            Err(e) => {
                log_line(&format!("warning: {e}; left as it is"));
                (None, false)
            }
        };
        let file_text = existing.as_deref().unwrap_or(&template);
        let (saved, warnings) = Settings::parse_file(file_text);
        for w in &warnings {
            log_line(&format!("warning: {w}"));
        }
        let settings = form::with_look(saved.clone(), cli, display::screen_size());

        let game = find_game(&settings)?;
        let exe_bytes = fs::read(game.join(EXE))
            .map_err(|e| format!("cannot read {}: {e}", game.join(EXE).display()))?;
        let digest = edition::sha256(&exe_bytes);
        let edition = match edition::identify(&exe_bytes) {
            Ok(e) => e,
            Err(digest) => {
                let text = edition::refusal(&digest);
                if check_only {
                    log_line(&format!("check: {text}"));
                    note(&text, true);
                    return Ok(CHECK_FAILED);
                }
                return Err(text);
            }
        };
        let image = Image::parse(&exe_bytes)?;
        log_line(&format!(
            "game: {} ({}) [sha256 {}]",
            game.display(),
            edition.name,
            edition::to_hex(&digest)
        ));
        if check_only {
            let text = format!(
                "Ready to play.\n\nGame folder: {}\nVersion: {}",
                game.display(),
                edition.name
            );
            log_line(&format!("check: {}", text.replace('\n', " ")));
            note(&text, false);
            return Ok(0);
        }

        let rt = settings
            .runtime
            .clone()
            .unwrap_or_else(|| home().join("Rally"));
        paths::check_separate(&game, &rt)?;
        let report = runtime::sync(&game, &rt)
            .map_err(|e| format!("cannot prepare {}: {e}", rt.display()))?;
        log_line(&format!(
            "runtime: {} ({} files copied)",
            rt.display(),
            report.brought
        ));
        for w in &report.warnings {
            log_line(&format!("warning: {w}"));
        }
        // The player's saves live outside the runtime copy; the game reads
        // and writes its own `save` folder, kept in step before and after.
        let saves = settings.saves.clone().unwrap_or_else(default_saves);
        let rt_save = runtime::save_dir(&rt).map_err(|e| format!("cannot prepare saves: {e}"))?;
        let saves_state = rt.join(runtime::SAVES_STATE);
        let mirror = runtime::saves_mirror(&rt_save, &saves, &saves_state);
        let moved = mirror
            .before()
            .map_err(|e| format!("cannot prepare the saves: {e}"))?;
        log_line(&format!(
            "saves: {} ({} files in, {} out)",
            saves.display(),
            moved.to_runtime.len(),
            moved.to_saves.len()
        ));

        if let Ok(listing) = runtime::list(&game)
            && let Some((len, rel)) = runtime::longest_path(&rt, &listing)
            && len > runtime::MAX_GAME_PATH
        {
            log_line(&format!(
                "warning: {rel} is {len} characters below the runtime folder; the game opens at most {}",
                runtime::MAX_GAME_PATH
            ));
        }

        // What this launch does to the game, from the settings alone
        // (`plan`, tested and snapshotted on the host).
        let wine = wine::is_wine();
        let hooks_path = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join(HOOKS_DLL)));
        let env = plan::Env {
            wine,
            hooks: hooks_path.as_ref().is_some_and(|p| p.is_file()),
            axes: win32::centred_axes(),
        };
        let plan = plan::plan(&settings, &env);

        let root = rt.display().to_string();
        let place = write_registry(&plan.registry(&root))?;
        // The GPU, not software rendering, and the faster of two GPUs.
        let gpu = if wine {
            gpu::prefer_glx().map(|w| w.then_some("Wine: OpenGL through GLX (UseEGL=N)"))
        } else {
            gpu::prefer_fast_gpu(&rt.join(EXE))
                .map(|w| w.then_some("Windows: high-performance GPU for the game"))
        };
        match gpu {
            Ok(Some(what)) => log_line(what),
            Ok(None) => {}
            Err(e) => log_line(&format!("warning: {e}")),
        }
        log_line(&format!("registry written ({place}); wine: {wine}"));

        // Remember the game folder; the rest of the file stays as written.
        if writable && (existing.is_none() || saved.game.as_ref() != Some(&game)) {
            let text = config::set_key(file_text, "game", &game.display().to_string());
            if let Err(e) = fsx::write_atomic(&settings_path, text) {
                log_line(&format!(
                    "warning: cannot write {}: {e}",
                    settings_path.display()
                ));
            }
        }

        if plan.hooks_missing {
            log_line(&format!(
                "warning: {HOOKS_DLL} not found next to the launcher; the options that need it are off"
            ));
        }
        let skip = &plan.skip;
        if !skip.is_empty() {
            log_line(&format!(
                "fixes turned off by settings: {}",
                skip.join(", ")
            ));
        }
        // HD art (F27): the set for this resolution, read by the hook
        // library; the pack that comes with the launcher is installed into
        // the runtime folder once.
        let bundled = match (&plan.hd_art, hooks_path.as_ref().and_then(|p| p.parent())) {
            (Some(config::HdArt::Bundled), Some(dir)) => {
                let started = std::time::Instant::now();
                match hdpack::install(&dir.join("hd"), &rt.join("hd")) {
                    Ok(at) => {
                        log_line(&format!(
                            "HD art pack ready in {} ({} ms)",
                            at.display(),
                            started.elapsed().as_millis()
                        ));
                        at
                    }
                    Err(e) => {
                        log_line(&format!("warning: HD art pack: {e}"));
                        PathBuf::new()
                    }
                }
            }
            _ => PathBuf::new(),
        };
        if let Some(set) = plan.hd_set(&settings, &bundled) {
            if set.is_dir() {
                // SAFETY: set before the game starts; no other thread reads
                // the environment yet.
                unsafe { std::env::set_var("CMR_HD_ART", &set) };
                log_line(&format!("F27 HD art from {}", set.display()));
            } else {
                log_line(&format!(
                    "warning: hd_art: {} doesn't exist (run cmr-hd upscale, see docs/UPSCALING.md)",
                    set.display()
                ));
            }
        }
        let pads: Vec<u32> = xinput::pads().iter().map(|p| p.slot).collect();
        log_line(&if pads.is_empty() {
            "XInput pads: none (rumble and separate trigger pedals need an Xbox-style pad; on 8BitDo pads use X-input mode)".to_string()
        } else {
            format!("XInput pads: {pads:?}")
        });
        let code = win32::run_patched(
            &rt.join(EXE),
            &rt,
            edition.fixes.cave_size(skip),
            hooks_path
                .as_deref()
                .filter(|_| plan.hooks)
                .map(|path| win32::HookLibrary {
                    path,
                    table: fixes::F13_TABLE,
                    exports: &fixes::F13_EXPORTS,
                }),
            |cave| {
                let mut writes = prepare(&image, edition.fixes, skip, cave)?;
                for line in plan.fill(&mut writes, cave) {
                    log_line(&line);
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
            xinput::stop_rumble();
        }
        match mirror.after() {
            Ok(copied) => log_line(&format!("saves: {} files written back", copied.len())),
            Err(e) => log_line(&format!(
                "warning: saves not written back to {}: {e}",
                saves.display()
            )),
        }
        Ok(code)
    }
}
