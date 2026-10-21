//! Portable intro/cut-scene video backend (item H07, video half).
//!
//! Defines the [`VideoBackend`] compatibility interface - the open / position /
//! play / pause / close control surface recovered from the MCI `avivideo` player
//! in [`crate::intro_video`] - and a driver that runs the recovered
//! [`crate::intro_video::playback_step`] loop against any backend. The real MCI
//! device (or a portable decoder) is runtime glue, like the SDL audio device; a
//! fake backend exercises the driver's logic in tests.
//!
//! The original player (`FUN_004af8a0`) opens the device, positions the window
//! (`MCI_PUT`), plays with `MCI_NOTIFY`, and pumps messages, closing on
//! `MM_MCINOTIFY` (completed) or a skip key (aborted). This module keeps that
//! orchestration portable: the per-iteration *decision* is the already-tested
//! [`crate::intro_video::playback_step`], and this driver maps each
//! [`PlaybackStep`] onto backend calls.
//!
//! See evidence `rea.official-eu.intro-video`.

use crate::intro_video::{PlaybackInput, PlaybackStep, PumpMode, playback_step};

/// The window rectangle the movie is positioned into (`MCI_PUT`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MovieRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// How a movie playthrough ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovieOutcome {
    /// The clip reached its end (`MM_MCINOTIFY`).
    Completed,
    /// The user skipped it (ENTER/ESC).
    Skipped,
}

/// The compatibility interface a concrete video backend implements (MCI
/// `avivideo`, or a later portable decoder). Mirrors the recovered control
/// surface: open an element, position its window, play (notified), toggle pause,
/// and close.
pub trait VideoBackend {
    /// Backend-specific error type.
    type Error;

    /// Open a movie file element and attach its output window
    /// (`FUN_004afbd0`: `MCI_OPEN` + `MCI_WINDOW`).
    fn open(&mut self, path: &str) -> Result<(), Self::Error>;

    /// Position the movie window (`FUN_004afdd0`: `MCI_PUT`).
    fn set_bounds(&mut self, rect: MovieRect) -> Result<(), Self::Error>;

    /// Start playback with completion notification
    /// (`FUN_004afcc0`: `MCI_PLAY | MCI_NOTIFY`).
    fn play(&mut self) -> Result<(), Self::Error>;

    /// Toggle pause/resume (`FUN_004afd20`: `MCI_PAUSE` <-> `MCI_PLAY`).
    fn toggle_pause(&mut self) -> Result<(), Self::Error>;

    /// Stop and close the device (`FUN_004afa00`).
    fn close(&mut self) -> Result<(), Self::Error>;
}

/// Apply one decided playback step to the backend. A terminal step closes the
/// backend and yields its outcome; a pause step toggles the backend and
/// continues; a continue step does nothing.
pub fn apply_step<B: VideoBackend>(
    backend: &mut B,
    step: PlaybackStep,
) -> Result<Option<MovieOutcome>, B::Error> {
    match step {
        PlaybackStep::Finished => {
            backend.close()?;
            Ok(Some(MovieOutcome::Completed))
        }
        PlaybackStep::Aborted => {
            backend.close()?;
            Ok(Some(MovieOutcome::Skipped))
        }
        PlaybackStep::TogglePause => {
            backend.toggle_pause()?;
            Ok(None)
        }
        PlaybackStep::Continue => Ok(None),
    }
}

/// Open, position, play, then run the recovered playback loop over a sequence of
/// per-iteration inputs (the real poll loop supplies these from
/// `GetAsyncKeyState`/`PeekMessage`). Returns the first terminal outcome. If the
/// inputs are exhausted without a terminal step the backend is closed and
/// [`MovieOutcome::Completed`] is returned - a modelling convenience; the real
/// loop runs until a terminal step rather than exhausting.
pub fn run_movie<B, I>(
    backend: &mut B,
    path: &str,
    rect: MovieRect,
    mode: PumpMode,
    inputs: I,
) -> Result<MovieOutcome, B::Error>
where
    B: VideoBackend,
    I: IntoIterator<Item = PlaybackInput>,
{
    backend.open(path)?;
    backend.set_bounds(rect)?;
    backend.play()?;
    for input in inputs {
        let step = playback_step(mode, input);
        if let Some(outcome) = apply_step(backend, step)? {
            return Ok(outcome);
        }
    }
    backend.close()?;
    Ok(MovieOutcome::Completed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FakeBackend {
        log: Vec<String>,
        open_fails: bool,
    }

    impl VideoBackend for FakeBackend {
        type Error = ();

        fn open(&mut self, path: &str) -> Result<(), ()> {
            if self.open_fails {
                return Err(());
            }
            self.log.push(format!("open:{path}"));
            Ok(())
        }

        fn set_bounds(&mut self, rect: MovieRect) -> Result<(), ()> {
            self.log
                .push(format!("bounds:{},{}", rect.width, rect.height));
            Ok(())
        }

        fn play(&mut self) -> Result<(), ()> {
            self.log.push("play".into());
            Ok(())
        }

        fn toggle_pause(&mut self) -> Result<(), ()> {
            self.log.push("pause".into());
            Ok(())
        }

        fn close(&mut self) -> Result<(), ()> {
            self.log.push("close".into());
            Ok(())
        }
    }

    fn rect() -> MovieRect {
        MovieRect {
            x: 0,
            y: 0,
            width: 640,
            height: 480,
        }
    }

    fn notify() -> PlaybackInput {
        PlaybackInput {
            mci_notify: true,
            ..Default::default()
        }
    }

    #[test]
    fn plays_to_completion_on_notify() {
        let mut b = FakeBackend::default();
        let inputs = vec![PlaybackInput::default(), PlaybackInput::default(), notify()];
        let outcome = run_movie(&mut b, "zoom.avi", rect(), PumpMode::Interactive, inputs).unwrap();
        assert_eq!(outcome, MovieOutcome::Completed);
        assert_eq!(
            b.log,
            vec!["open:zoom.avi", "bounds:640,480", "play", "close"]
        );
    }

    #[test]
    fn interactive_skip_aborts_and_closes() {
        let mut b = FakeBackend::default();
        let pause = PlaybackInput {
            space: true,
            ..Default::default()
        };
        let skip = PlaybackInput {
            escape: true,
            ..Default::default()
        };
        let outcome = run_movie(
            &mut b,
            "intro.avi",
            rect(),
            PumpMode::Interactive,
            vec![pause, skip],
        )
        .unwrap();
        assert_eq!(outcome, MovieOutcome::Skipped);
        // The pause toggle is applied, then the skip closes the device.
        assert_eq!(
            b.log,
            vec!["open:intro.avi", "bounds:640,480", "play", "pause", "close"]
        );
    }

    #[test]
    fn passive_pump_ignores_keys_until_notify() {
        let mut b = FakeBackend::default();
        let keys = PlaybackInput {
            enter: true,
            escape: true,
            space: true,
            mci_notify: false,
        };
        // In passive mode the keys do nothing; only the notify ends it.
        let outcome = run_movie(
            &mut b,
            "logo.avi",
            rect(),
            PumpMode::Passive,
            vec![keys, keys, notify()],
        )
        .unwrap();
        assert_eq!(outcome, MovieOutcome::Completed);
        assert!(!b.log.iter().any(|e| e == "pause"));
    }

    #[test]
    fn open_failure_propagates() {
        let mut b = FakeBackend {
            open_fails: true,
            ..Default::default()
        };
        let result = run_movie(
            &mut b,
            "x.avi",
            rect(),
            PumpMode::Interactive,
            vec![notify()],
        );
        assert!(result.is_err());
        assert!(b.log.is_empty());
    }

    #[test]
    fn apply_step_is_terminal_on_finished_and_aborted() {
        let mut b = FakeBackend::default();
        assert_eq!(
            apply_step(&mut b, PlaybackStep::Finished).unwrap(),
            Some(MovieOutcome::Completed)
        );
        assert_eq!(
            apply_step(&mut b, PlaybackStep::Aborted).unwrap(),
            Some(MovieOutcome::Skipped)
        );
        assert_eq!(apply_step(&mut b, PlaybackStep::TogglePause).unwrap(), None);
        assert_eq!(apply_step(&mut b, PlaybackStep::Continue).unwrap(), None);
        assert_eq!(b.log, vec!["close", "close", "pause"]);
    }
}
