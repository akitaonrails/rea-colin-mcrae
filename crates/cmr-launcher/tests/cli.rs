//! The real CMRLauncher.exe on its command line, under
//! `sdk/scripts/wine-test` (and on Windows). Every run passes `--linux`:
//! under Wine a run without it is a player's, its messages are dialogs, and
//! a dialog would wait forever in a headless test. A run that does show
//! one is killed after [`LIMIT`] and fails.

#[cfg(windows)]
mod program {
    use std::path::PathBuf;
    use std::process::{Command, Output, Stdio};
    use std::time::{Duration, Instant};

    use rea_launcher_win32::report::BAD_COMMAND_LINE;

    /// Far longer than any of these runs takes.
    const LIMIT: Duration = Duration::from_secs(60);

    /// A scratch home with a hostile name (spaces, apostrophe, accents).
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("cmr-cli-{name}-{}", std::process::id()))
            .join("Jogos d'Ação");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Runs the launcher with `args` and `--linux`; returns its output and
    /// its launcher.log.
    fn run(name: &str, args: &[&str]) -> (Output, String) {
        let home = scratch(name);
        let mut child = Command::new(env!("CARGO_BIN_EXE_CMRLauncher"))
            .args(args)
            .arg("--linux")
            .env("LOCALAPPDATA", &home)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let started = Instant::now();
        while child.try_wait().unwrap().is_none() {
            if started.elapsed() > LIMIT {
                let _ = child.kill();
                let _ = child.wait();
                panic!("{args:?} did not exit (a dialog waiting for OK?)");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let output = child.wait_with_output().unwrap();
        let log = std::fs::read_to_string(home.join("CMRLauncher").join("launcher.log"))
            .unwrap_or_default();
        (output, log)
    }

    fn stdout(output: &Output) -> String {
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    /// rea-launcher-sdk#1: a typo or a bad value showed a modal box and the
    /// process waited for OK. Now: printed with the usage, exit 2.
    #[test]
    fn a_bad_command_line_is_printed_and_exits_2() {
        for (name, args, says) in [
            (
                "bogus",
                &["--bogus-flag"][..],
                "unknown argument --bogus-flag",
            ),
            (
                "look",
                &["--look", "invalid"],
                "--look needs original, enhanced or steam_deck, not invalid",
            ),
        ] {
            let (output, log) = run(name, args);
            assert_eq!(output.status.code(), Some(BAD_COMMAND_LINE), "{args:?}");
            let out = stdout(&output);
            assert!(out.contains(says), "{args:?}: {out}");
            assert!(out.contains("--no-window"), "the usage: {out}");
            assert!(log.contains(says), "{log}");
        }
    }
}
