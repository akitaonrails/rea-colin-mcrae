//! Pure interactive controller for the recovered root menu.
//!
//! This composes the recovered [`FrontendState`] controller with a minimal
//! transition driver and the steady render bridge so a host loop can turn
//! per-frame [`FrontendInput`] into a [`RootRenderInput`] to present.
//!
//! The driver owns no clock: the caller supplies a monotonically increasing
//! `now`. A transition to the race sentinel `0xd437c8` or the rally-school
//! sentinel `0xd46b10` yields result 3 or 4 (`FUN_0044f6d0`); a transition to
//! the root or exit screen completes; every other confirmed entry begins its
//! transition and then waits, because the destination screens are not yet
//! reconstructed. Harness: a game-session host may let the championship, rally
//! and time-trial entries stand in for their unbuilt setup and name-entry
//! screens, whose Continue leads to the race sentinel, so confirming one starts
//! the race ([`RootInteraction::race_mode`] names the mode its setup screen's
//! enter callback writes). The live
//! selection's chrome is snapped to its recovered endpoint via
//! [`steady_root_render_input`]; the in-between tween is a separate,
//! not-yet-recovered concern. None of this models the original transition
//! timing, so it is not a compatibility claim.

use crate::{
    display::DisplayExtent,
    frontend::{
        CHAMPIONSHIP_SETUP_SCREEN_ID, EXIT_SCREEN_ID, FrontendEffect, FrontendInput, FrontendPhase,
        FrontendState, InvalidRootSelection, RACE_SENTINEL_SCREEN_ID,
        RALLY_SCHOOL_SENTINEL_SCREEN_ID, RALLY_SCREEN_ID, ROOT_SCREEN_ID, ScreenId,
        TIME_TRIAL_SCREEN_ID, TransitionResult,
    },
    localization::Language,
    root_menu_visual::{RootRenderInput, steady_root_render_input},
};

/// Host-visible outcome of one interactive root-menu tick.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RootTick {
    /// Steady render input while the interactive root screen is showing, or
    /// `None` once a confirmed selection has begun leaving it (or the frontend
    /// has stopped running).
    pub render_input: Option<RootRenderInput>,
    /// Recovered controller effects emitted by this tick, in order.
    pub effects: Vec<FrontendEffect>,
    /// Whether the recovered controller has requested process exit.
    pub exit_requested: bool,
}

/// Minimal interactive root-menu driver over the recovered [`FrontendState`].
pub struct RootInteraction {
    state: FrontendState,
    language: Language,
    viewport_extent: DisplayExtent,
}

impl RootInteraction {
    /// Starts the driver from the recovered saved-selection byte, in the same
    /// post-startup-art state the recovered frontend bootstraps into.
    pub fn from_saved_selection(
        saved_selection: u8,
        language: Language,
        viewport_extent: DisplayExtent,
        now: u32,
    ) -> Result<Self, InvalidRootSelection> {
        Ok(Self {
            state: FrontendState::after_startup_art(saved_selection, now)?,
            language,
            viewport_extent,
        })
    }

    /// Borrows the underlying recovered controller for inspection.
    #[must_use]
    pub const fn state(&self) -> &FrontendState {
        &self.state
    }

    /// Advances the controller by one tick and reports what to present.
    #[must_use]
    pub fn tick(&mut self, input: FrontendInput, now: u32, save_allowed: bool) -> RootTick {
        self.tick_with_rally_launch(input, now, save_allowed, false)
    }

    /// Advances one tick with the harness race launch: confirming the root
    /// championship, rally or time-trial entry starts the race. Other unbuilt
    /// destinations remain pending.
    #[must_use]
    pub fn tick_for_game_session(
        &mut self,
        input: FrontendInput,
        now: u32,
        save_allowed: bool,
    ) -> RootTick {
        self.tick_with_rally_launch(input, now, save_allowed, true)
    }

    fn tick_with_rally_launch(
        &mut self,
        input: FrontendInput,
        now: u32,
        save_allowed: bool,
        rally_launch: bool,
    ) -> RootTick {
        let transition = self.resolve_transition(rally_launch);
        let effects = self.state.step(input, now, transition, save_allowed);
        RootTick {
            render_input: self.current_render_input(),
            exit_requested: self.state.exit_requested(),
            effects,
        }
    }

    /// Completes only transitions whose destination screen is reconstructed
    /// (the root screen itself and the exit screen); all others stay pending.
    fn resolve_transition(&self, rally_launch: bool) -> TransitionResult {
        let target = match self.state.phase() {
            FrontendPhase::AdvanceToPending => self.state.pending_screen(),
            FrontendPhase::AdvanceToInteractive => Some(self.state.active_screen()),
            FrontendPhase::Interactive | FrontendPhase::BeginTransition => None,
        };
        match target {
            Some(screen) if screen == RACE_SENTINEL_SCREEN_ID => TransitionResult::StartRace,
            Some(screen) if screen == RALLY_SCHOOL_SENTINEL_SCREEN_ID => {
                TransitionResult::StartRallySchool
            }
            Some(screen) if rally_launch && race_mode_of(screen).is_some() => {
                TransitionResult::StartRace
            }
            Some(screen) if is_reconstructed_screen(screen) => TransitionResult::Complete,
            _ => TransitionResult::Pending,
        }
    }

    fn current_render_input(&self) -> Option<RootRenderInput> {
        if self.state.frontend_active()
            && self.state.phase() == FrontendPhase::Interactive
            && self.state.active_screen() == ROOT_SCREEN_ID
        {
            Some(steady_root_render_input(
                self.state.root_selection(),
                self.language,
                self.viewport_extent,
                false,
            ))
        } else {
            None
        }
    }
}

/// The event mode (`record[0x180]`) the setup screen's enter callback writes:
/// 0 championship (`0xd40900`), 1 single rally (`0xd48230`), 2 time trial
/// (`0xd4b240`).
fn race_mode_of(screen: ScreenId) -> Option<u8> {
    match screen {
        CHAMPIONSHIP_SETUP_SCREEN_ID => Some(0),
        RALLY_SCREEN_ID => Some(1),
        TIME_TRIAL_SCREEN_ID => Some(2),
        _ => None,
    }
}

impl RootInteraction {
    /// The mode of the setup screen the harness launch left through, while
    /// its transition is under way.
    #[must_use]
    pub fn race_mode(&self) -> Option<u8> {
        self.state.pending_screen().and_then(race_mode_of)
    }
}

fn is_reconstructed_screen(screen: ScreenId) -> bool {
    screen == ROOT_SCREEN_ID || screen == EXIT_SCREEN_ID
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::root_menu_visual::steady_root_render_input;

    const EXTENT: DisplayExtent = DisplayExtent {
        width: 640,
        height: 480,
    };

    fn driver(saved: u8) -> RootInteraction {
        RootInteraction::from_saved_selection(saved, Language::English, EXTENT, 0).unwrap()
    }

    fn settle_to_interactive(driver: &mut RootInteraction) -> RootTick {
        // The recovered bootstrap takes two completed transitions to reach the
        // interactive root screen.
        let _ = driver.tick(FrontendInput::default(), 1, true);
        driver.tick(FrontendInput::default(), 2, true)
    }

    #[test]
    fn bootstrap_settles_on_the_interactive_root_screen() {
        let mut driver = driver(0);

        let first = driver.tick(FrontendInput::default(), 1, true);
        assert!(first.render_input.is_none(), "still activating the screen");

        let second = driver.tick(FrontendInput::default(), 2, true);
        assert_eq!(
            second.render_input,
            Some(steady_root_render_input(
                driver.state().root_selection(),
                Language::English,
                EXTENT,
                false
            ))
        );
        assert_eq!(driver.state().phase(), FrontendPhase::Interactive);
        assert!(!second.exit_requested);
    }

    #[test]
    fn a_next_pulse_advances_the_live_selection_render_input() {
        let mut driver = driver(0);
        let settled = settle_to_interactive(&mut driver);
        assert_eq!(
            settled.render_input.map(|input| input.selection.index()),
            Some(0)
        );

        let advanced = driver.tick(
            FrontendInput {
                next: true,
                ..FrontendInput::default()
            },
            3,
            true,
        );
        assert_eq!(
            advanced.render_input.map(|input| input.selection.index()),
            Some(1)
        );
        assert_eq!(
            advanced.effects,
            vec![FrontendEffect::PersistRootSelection(1)]
        );
    }

    #[test]
    fn confirming_exit_drives_through_to_the_recovered_exit_effects() {
        for (save_allowed, expect_save) in [(true, true), (false, false)] {
            let mut driver = driver(6);
            let _ = settle_to_interactive(&mut driver);
            // Saved selection 6 maps to 0 on load, so navigate to the exit entry.
            let last = (crate::frontend::ROOT_SCREEN.entries.len() - 1) as u8;
            let mut now = 3;
            while driver.state().root_selection().index() != last {
                let _ = driver.tick(
                    FrontendInput {
                        previous: true,
                        ..FrontendInput::default()
                    },
                    now,
                    save_allowed,
                );
                now += 1;
            }

            let confirm = driver.tick(
                FrontendInput {
                    confirm: true,
                    ..FrontendInput::default()
                },
                now,
                save_allowed,
            );
            assert!(!confirm.exit_requested, "confirm begins the transition");

            // Drive the remaining transition ticks until exit is requested.
            let mut exit_tick = None;
            for _ in 0..4 {
                now += 1;
                let tick = driver.tick(FrontendInput::default(), now, save_allowed);
                if tick.exit_requested {
                    exit_tick = Some(tick);
                    break;
                }
            }
            let exit = exit_tick.expect("exit must be reached");
            assert!(exit.render_input.is_none());
            assert!(exit.effects.contains(&FrontendEffect::RequestExit));
            assert_eq!(
                exit.effects.contains(&FrontendEffect::SaveConfig),
                expect_save
            );
        }
    }

    #[test]
    fn confirming_an_unreconstructed_entry_waits_without_exiting() {
        let mut driver = driver(0);
        let _ = settle_to_interactive(&mut driver);

        // Selection 0 targets the championship setup screen, which is not yet
        // reconstructed.
        let _ = driver.tick(
            FrontendInput {
                confirm: true,
                ..FrontendInput::default()
            },
            3,
            true,
        );

        for now in 4..10 {
            let tick = driver.tick(FrontendInput::default(), now, true);
            assert!(!tick.exit_requested);
            assert!(
                tick.render_input.is_none(),
                "it has left the interactive root screen and waits for an unbuilt screen"
            );
        }
        assert_ne!(driver.state().phase(), FrontendPhase::Interactive);
    }

    #[test]
    fn game_session_policy_launches_the_three_race_entries() {
        let mut rally = driver(0);
        let _ = settle_to_interactive(&mut rally);
        let _ = rally.tick_for_game_session(
            FrontendInput {
                next: true,
                ..FrontendInput::default()
            },
            3,
            true,
        );
        let _ = rally.tick_for_game_session(
            FrontendInput {
                confirm: true,
                ..FrontendInput::default()
            },
            4,
            true,
        );
        let begin = rally.tick_for_game_session(FrontendInput::default(), 5, true);
        assert!(begin.effects.iter().any(|effect| matches!(
            effect,
            FrontendEffect::BeginTransition {
                to: RALLY_SCREEN_ID,
                ..
            }
        )));
        assert_eq!(rally.race_mode(), Some(1));
        let leave = rally.tick_for_game_session(FrontendInput::default(), 6, true);
        assert_eq!(leave.effects, vec![FrontendEffect::StartRace]);

        let mut championship = driver(0);
        let _ = settle_to_interactive(&mut championship);
        let _ = championship.tick_for_game_session(
            FrontendInput {
                confirm: true,
                ..FrontendInput::default()
            },
            3,
            true,
        );
        let _ = championship.tick_for_game_session(FrontendInput::default(), 4, true);
        assert_eq!(championship.race_mode(), Some(0));
        let leave = championship.tick_for_game_session(FrontendInput::default(), 5, true);
        assert_eq!(leave.effects, vec![FrontendEffect::StartRace]);

        // The options entry (4) has no launch.
        let mut options = driver(4);
        let _ = settle_to_interactive(&mut options);
        let _ = options.tick_for_game_session(
            FrontendInput {
                confirm: true,
                ..FrontendInput::default()
            },
            3,
            true,
        );
        for now in 4..8 {
            let tick = options.tick_for_game_session(FrontendInput::default(), now, true);
            assert!(!tick.effects.contains(&FrontendEffect::StartRace));
        }
        assert_eq!(options.race_mode(), None);
    }
}
