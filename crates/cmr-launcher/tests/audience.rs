//! Who sees what the launcher says (`sdk/docs/STANDARDS.md`, "Messages
//! reach the player"), decided by the SDK's `report::Audience` from the
//! command lines this launcher is started with: a command's output
//! (`--help`, `--check`, `--controllers`) goes to the terminal, and a
//! play-time error (game folder not found, unknown build, already running)
//! is a dialog whenever a person may be watching.

use cmr_launcher::COMMAND_FLAGS;
use rea_launcher_win32::report::{Audience, Mode, Route, Run, Terminal, route};

fn audience(args: &[&str], wine: bool) -> Audience {
    Audience::of(Run::from_args(args, COMMAND_FLAGS), wine)
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
    // cmr-play's game run (`--no-window`, one-run flags too) and a Proton
    // start without arguments.
    for run in [
        &["--no-window"][..],
        &["--no-window", "--scaler", "fsr"],
        &[],
    ] {
        let mode = audience(run, true).mode();
        assert_eq!(route(mode, UNDER_HELPER), BOTH, "{run:?}");
    }
}

/// cmr-play's window run: the window itself or the helper reports (a
/// second start's "already running" is the helper's exit-5 line).
#[test]
fn the_helpers_window_run_shows_no_dialog_of_its_own() {
    let window = audience(&["--settings", "--linux"], true);
    assert!(!window.command);
    assert!(!window.interactive);
}

#[test]
fn check_help_and_controllers_are_commands() {
    for command in [
        &["--check"][..],
        &["--help"],
        &["-h"],
        &["/?"],
        &["--controllers", "10"],
    ] {
        assert!(audience(command, false).command, "{command:?}");
    }
    // Under Wine stdout is a log: the player gets the dialog too.
    let check = audience(&["--check", "--no-window"], true);
    assert_eq!(route(check.mode(), UNDER_HELPER), BOTH);
    // From a terminal: printed, no dialog.
    let terminal = Terminal {
        attached: true,
        redirected: false,
        wine: false,
    };
    assert_eq!(
        route(audience(&["--check"], false).mode(), terminal),
        Route {
            print: true,
            dialog: false
        }
    );
}

#[test]
fn a_windows_script_with_no_window_never_gets_a_dialog() {
    let script = audience(&["--no-window", "--game", "C:\\CMR"], false);
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
    let check = audience(&["--check", "--no-window"], false);
    assert!(!route(check.mode(), redirected).dialog);
    // A double-clicked shortcut is a person.
    assert!(audience(&[], false).interactive);
}

/// Under Wine (`sdk/scripts/wine-test` pipes stdout, as cmr-play does):
/// the game run's error would reach the player as a dialog.
#[cfg(windows)]
#[test]
fn under_wine_the_game_run_error_is_a_dialog() {
    use rea_launcher_win32::{report, wine};
    if !wine::is_wine() {
        return;
    }
    let mode = report::audience(Run::from_args(&["--no-window"], COMMAND_FLAGS)).mode();
    assert!(report::route_here(mode).dialog);
}

/// A command line that fails to parse answers like a command
/// (`Run::bad_command_line`, as `main` reports it): from PowerShell it is
/// printed and exits 2, with no modal box for a script to wait on
/// (rea-launcher-sdk#1: CMRLauncher showed one, 410x819, until OK).
#[test]
fn a_bad_command_line_is_printed_from_a_terminal() {
    use rea_launcher_win32::report::BAD_COMMAND_LINE;
    let terminal = Terminal {
        attached: true,
        redirected: false,
        wine: false,
    };
    let shortcut = Terminal {
        attached: false,
        redirected: false,
        wine: false,
    };
    for args in [&["--bogus-flag"][..], &["--look", "invalid"]] {
        assert!(
            cmr_launcher::config::Settings::parse_args(args.iter().map(|a| a.to_string())).is_err()
        );
        let mode = Audience::of(Run::bad_command_line(args), false).mode();
        assert!(matches!(mode, Mode::Command { .. }), "{args:?}");
        assert_eq!(
            route(mode, terminal),
            Route {
                print: true,
                dialog: false
            },
            "{args:?}"
        );
        // A shortcut with a typo: nobody sees a terminal, so the (short)
        // dialog.
        assert!(route(mode, shortcut).dialog, "{args:?}");
        // The play-run rule it replaced showed the box from a terminal too.
        assert!(route(audience(args, false).mode(), terminal).dialog);
    }
    assert_eq!(BAD_COMMAND_LINE, 2);
}
