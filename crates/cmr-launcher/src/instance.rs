//! One launcher at a time (SDK STANDARDS, "Clean exit, single instance").
//!
//! The launcher holds a named mutex (`rea_launcher_win32::instance`) from
//! the moment it starts until the game it started has ended, so a second
//! start can neither open a second window nor start a second game from
//! the same runtime copy: it brings the first launcher's window forward
//! (exit 0), or says the game is running (exit [`ALREADY_RUNNING`]); a run
//! without the window never just brings the window forward
//! (`SecondStart::decide`). Before Play, a game still running from the
//! runtime copy (its launcher killed) refuses the start too
//! (`process::find_running`). `--check` takes the instance like any
//! action (`rea_launcher_win32::instance::is_action`): with the window
//! open it is refused with exit 5, never run alongside; `--help` and
//! `--controllers` only read, and run alongside. On Linux, `cmr-play`'s
//! session lock does the same job.

/// The instance name (`Local\rea-launcher-rea-colin-mcrae`).
pub const NAME: &str = "rea-colin-mcrae";

/// The settings window's title, which a second start looks for to bring
/// the window forward. Not the game's title: FindWindow would find the
/// game's window instead.
pub const WINDOW_TITLE: &str = "Colin McRae Rally launcher";

/// The game's own window title, seen in the real game on 2026-11-02 (the
/// only mapped window of `Rally.exe`, 640x480; `docs/RUNTIME_NOTES.md`).
/// A second start that found this window would raise the game and say
/// nothing; 0.7's settings window had this title.
pub const GAME_WINDOW_TITLE: &str = "Colin McRae Rally";

/// The game's name in the second start's message
/// (`rea_launcher_win32::instance::SecondStart::message`).
pub const GAME: &str = "Colin McRae Rally";

/// The exit code of a second start whose own code is `code`
/// (`SecondStart::exit_code`: 0 when the open window came forward, else
/// [`ALREADY_RUNNING`]). cmr-play's window run (`--settings`) reads 0 as
/// Play and would start a game run the instance then refuses, so it gets
/// [`ALREADY_RUNNING`] either way: the helper says another launcher is
/// running and starts nothing.
pub fn second_start_exit(code: i32, settings_only: bool) -> i32 {
    if settings_only { ALREADY_RUNNING } else { code }
}

pub use rea_launcher::instance::ALREADY_RUNNING;

/// This launcher's action flags on top of the family's (`--check`,
/// `--setup`, `--download-*`: `rea_launcher_win32::instance::ACTION_FLAGS`):
/// none. A second start with one is refused while the window is open.
pub const ACTIONS: &[&str] = &[];

#[cfg(test)]
mod tests {
    use super::*;

    /// CMR3 measured it on 2026-11-02 in its real game: with the settings
    /// window titled like the game, a second start found the game's
    /// window, raised it and never said the game was running.
    #[test]
    fn the_launcher_window_is_never_mistaken_for_the_game() {
        assert_ne!(WINDOW_TITLE, GAME_WINDOW_TITLE);
    }

    #[test]
    fn a_second_window_run_under_cmr_play_never_reads_as_play() {
        assert_eq!(ALREADY_RUNNING, 5);
        // The window came forward (0) or the game runs (5).
        assert_eq!(second_start_exit(0, false), 0);
        assert_eq!(second_start_exit(ALREADY_RUNNING, false), ALREADY_RUNNING);
        assert_eq!(second_start_exit(0, true), ALREADY_RUNNING);
        assert_eq!(second_start_exit(ALREADY_RUNNING, true), ALREADY_RUNNING);
    }

    /// rea-launcher-sdk#1: `--check` with the settings window open ran
    /// the check alongside. It is an action now: the window stays where it
    /// is, the run says to use it and exits 5. A plain start still brings
    /// the window forward.
    #[test]
    fn check_with_the_window_open_is_refused() {
        use rea_launcher_win32::instance::{SecondStart, is_action};
        let action = |args: &[&str]| is_action(args, ACTIONS);
        for args in [
            &["--check"][..],
            &["--check", "--game", "C:\\CMR"],
            &["--check", "--linux"],
        ] {
            assert!(action(args), "{args:?}");
            let start = SecondStart::decide(action(args), || true, || panic!("brought forward"));
            assert_eq!(start, SecondStart::WindowOpen, "{args:?}");
            assert_eq!(second_start_exit(start.exit_code(), false), ALREADY_RUNNING);
            let text = start.message(GAME).unwrap();
            assert!(
                text.starts_with("The Colin McRae Rally launcher window is open"),
                "{text}"
            );
        }
        assert!(!action(&[]));
        assert!(!action(&["--no-window"]));
        let plain = SecondStart::decide(action(&[]), || panic!("only looked"), || true);
        assert_eq!(plain, SecondStart::Focused);
        assert_eq!(second_start_exit(plain.exit_code(), false), 0);
    }

    /// Under Wine (`sdk/scripts/wine-test`): while one launcher holds the
    /// instance, a second start is refused, finds no window of its own to
    /// bring forward (headless: no settings window), says the game is
    /// running and exits 5; once the first ends, the next start gets it.
    #[cfg(windows)]
    #[test]
    fn a_second_launcher_is_refused_while_the_first_runs() {
        use rea_launcher_win32::instance::{self, SecondStart, Window};
        let name = format!("{NAME}-test-{}", std::process::id());
        let first = instance::single(&name).expect("the first launcher");
        let second = instance::single(&name).expect_err("a second launcher");
        for action in [false, true] {
            let start = second.second_start(action, Window::Own(WINDOW_TITLE));
            assert_eq!(start, SecondStart::Busy, "action {action}");
            assert_eq!(second_start_exit(start.exit_code(), false), ALREADY_RUNNING);
            let text = start.message(GAME).expect("a message");
            assert!(
                text.starts_with("Colin McRae Rally is already running"),
                "{text}"
            );
        }
        drop(first);
        drop(instance::single(&name).expect("free once the first ended"));
    }
}
