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

    use cmr_launcher::config::Settings;
    use cmr_launcher::{edition, pe, prepare, registry, runtime};

    use crate::win32;

    const EXE: &str = "Rally.exe";

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
        let settings_path = home().join("launcher.cfg");
        let saved = fs::read_to_string(&settings_path)
            .map(|t| Settings::parse_file(&t))
            .unwrap_or_default();
        let settings = saved.merged(Settings::parse_args(std::env::args().skip(1))?);

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

        let _ = fs::write(
            &settings_path,
            Settings {
                game: Some(game.clone()),
                ..settings
            }
            .to_file(),
        );

        let code = win32::run_patched(
            &rt.join(EXE),
            &rt,
            edition.fixes,
            |cave| prepare(&image, edition.fixes, cave),
            &mut |line| log_line(line),
        )?;
        log_line(&format!("game exited with {code}"));
        Ok(code)
    }
}
