//! Who sees what the launcher says (`sdk/docs/STANDARDS.md`, "Messages
//! reach the player"). The binary hands [`Audience::mode`] to
//! `rea_launcher_win32::report`: a command's output (`--help`, `--check`, `--controllers`)
//! goes to the terminal, and a play-time error (game folder not found,
//! unknown build, already running) is a dialog whenever a person may be
//! watching, printed to the log as well.

/// The kind of run, from the launcher's arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Audience {
    /// A script-facing command (`--help`, `--check`, `--controllers`): its output and its
    /// errors are the command's result.
    pub command: bool,
    /// A person may be watching, so a dialog may be shown.
    pub interactive: bool,
}

/// The audience of a run started with `args` (the launcher's arguments,
/// program name excluded); `wine`: running under Wine.
///
/// `--no-window` usually means a script, so no dialog. Under Wine it
/// doesn't: `cmr-play` (the window run, then a `--no-window` game run),
/// the AppImage and Proton start the launcher for a player in front of the
/// screen, and its stdout goes to a log nobody reads. Without a dialog
/// there, a wrong game folder looked like a game that never started.
pub fn audience(args: &[String], wine: bool) -> Audience {
    let has = |flag: &str| args.iter().any(|arg| arg == flag);
    Audience {
        command: ["--check", "--controllers", "--help", "-h", "/?"]
            .into_iter()
            .any(has),
        interactive: wine || !has("--no-window"),
    }
}

#[cfg(any(windows, test))]
impl Audience {
    /// The `report::Mode` for this run's messages.
    pub fn mode(self) -> rea_launcher_win32::report::Mode {
        use rea_launcher_win32::report::Mode;
        let interactive = self.interactive;
        if self.command {
            Mode::Command { interactive }
        } else {
            Mode::Play { interactive }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rea_launcher_win32::report::{Route, Terminal, route};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    /// `cmr-play`, the AppImage and Proton: no console, stdout to a log.
    const UNDER_HELPER: Terminal = Terminal {
        attached: false,
        redirected: true,
        wine: true,
    };

    const BOTH: Route = Route {
        print: true,
        dialog: true,
    };

    #[test]
    fn a_play_error_under_the_linux_helper_is_a_dialog() {
        // cmr-play's game run (`--no-window`, one-run flags too), its
        // window run, and a Proton start without arguments.
        for run in [
            &["--no-window"][..],
            &["--no-window", "--scaler", "fsr"],
            &["--settings", "--linux"],
            &[],
        ] {
            let mode = audience(&args(run), true).mode();
            assert_eq!(route(mode, UNDER_HELPER), BOTH, "{run:?}");
        }
    }

    #[test]
    fn check_and_help_print_and_show_a_dialog_only_when_unseen() {
        for command in [&["--check"][..], &["--help"], &["-h"], &["/?"]] {
            assert!(audience(&args(command), false).command, "{command:?}");
        }
        // Under Wine stdout is a log: the player gets the dialog too.
        let check = audience(&args(&["--check", "--no-window"]), true);
        assert_eq!(route(check.mode(), UNDER_HELPER), BOTH);
        // From a terminal: printed, no dialog.
        let terminal = Terminal {
            attached: true,
            redirected: false,
            wine: false,
        };
        let check = audience(&args(&["--check"]), false);
        assert_eq!(
            route(check.mode(), terminal),
            Route {
                print: true,
                dialog: false
            }
        );
    }

    /// Under Wine (`sdk/scripts/wine-test` pipes stdout, as cmr-play
    /// does): the game run's error would reach the player as a dialog.
    #[cfg(windows)]
    #[test]
    fn under_wine_the_game_run_error_is_a_dialog() {
        use rea_launcher_win32::{report, wine};
        if !wine::is_wine() {
            return;
        }
        let mode = audience(&args(&["--no-window"]), true).mode();
        assert!(report::route_here(mode).dialog);
    }

    #[test]
    fn a_windows_script_with_no_window_never_gets_a_dialog() {
        let script = audience(&args(&["--no-window", "--game", "C:\\CMR"]), false);
        assert_eq!(
            script,
            Audience {
                command: false,
                interactive: false
            }
        );
        let redirected = Terminal {
            attached: false,
            redirected: true,
            wine: false,
        };
        assert!(!route(script.mode(), redirected).dialog);
        // `--check` from a Windows script: the terminal only.
        let check = audience(&args(&["--check", "--no-window"]), false);
        assert!(!route(check.mode(), redirected).dialog);
        // A double-clicked shortcut is a person.
        assert!(audience(&[], false).interactive);
    }
}
