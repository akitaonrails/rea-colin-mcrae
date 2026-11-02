//! One launcher at a time (SDK STANDARDS, "Clean exit, single instance").
//!
//! The launcher holds a named mutex (`rea_launcher_win32::instance`) from
//! the moment it starts until the game it started has ended, so a second
//! start can neither open a second window nor start a second game from
//! the same runtime copy: it brings the first launcher's window forward,
//! or says the game is running. `--help` and `--check` only read, and run
//! alongside. On Linux, `cmr-play`'s session lock does the same job.

/// The instance name (`Local\rea-launcher-rea-colin-mcrae`).
pub const NAME: &str = "rea-colin-mcrae";

/// The settings window's title, which a second start looks for to bring
/// the window forward. Not the game's title: FindWindow would find the
/// game's window instead.
pub const WINDOW_TITLE: &str = "Colin McRae Rally launcher";

/// The game's own window title, the name the game is known by (to be
/// confirmed on the real game, `docs/RUNTIME_NOTES.md`). A second start
/// that found this window would raise the game and say nothing; 0.7's
/// settings window had this title.
pub const GAME_WINDOW_TITLE: &str = "Colin McRae Rally";

/// Shown when a second start finds no launcher window to bring forward:
/// the first launcher is waiting for the game.
pub const GAME_RUNNING: &str = "Colin McRae Rally is already running. \
     Quit the game first, then start the launcher again.";

/// What a second start does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecondStart {
    /// The running launcher's window came to the front; nothing to say.
    WindowRaised,
    /// No launcher window: the game is running. Say so ([`GAME_RUNNING`]).
    GameRunning,
}

impl SecondStart {
    /// From whether the running launcher's window was found and raised.
    pub fn new(window_raised: bool) -> SecondStart {
        if window_raised {
            SecondStart::WindowRaised
        } else {
            SecondStart::GameRunning
        }
    }

    /// The message for the player, if any.
    pub fn message(self) -> Option<&'static str> {
        match self {
            SecondStart::WindowRaised => None,
            SecondStart::GameRunning => Some(GAME_RUNNING),
        }
    }
}

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
    fn a_second_start_raises_the_window_or_says_the_game_runs() {
        assert_eq!(SecondStart::new(true).message(), None);
        assert_eq!(SecondStart::new(false).message(), Some(GAME_RUNNING));
    }

    /// Under Wine (`sdk/scripts/wine-test`): while one launcher holds the
    /// instance, a second start is refused, finds no window to raise
    /// (headless: no settings window), and so says the game is running;
    /// once the first ends, the next start gets it.
    #[cfg(windows)]
    #[test]
    fn a_second_launcher_is_refused_while_the_first_runs() {
        use rea_launcher_win32::instance::{self, Window};
        let name = format!("{NAME}-test-{}", std::process::id());
        let first = instance::single(&name).expect("the first launcher");
        let second = instance::single(&name).expect_err("a second launcher");
        let raised = second.bring_to_front(Window::Title(&format!("{NAME} no such window")));
        assert_eq!(SecondStart::new(raised), SecondStart::GameRunning);
        drop(first);
        drop(instance::single(&name).expect("free once the first ended"));
    }
}
