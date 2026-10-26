//! Frontend screen-transition state machine from `FUN_004c20f0`.

/// Numeric identity of a frontend screen descriptor.
pub type ScreenId = u16;

/// The recovered frontend transition phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuPhase {
    /// State 1: poll input and run the active descriptor.
    Interactive,
    /// State 4: initialize the transition selected by the active descriptor.
    Transition,
    /// State 2: run the outgoing transition.
    FadeOut,
    /// State 3: run the incoming transition.
    FadeIn,
}

/// Observable work emitted by a transition step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    /// Initialize the selected outgoing transition.
    BeginTransition { from: ScreenId, to: ScreenId },
    /// The next descriptor has become active.
    Enter(ScreenId),
    /// Terminal fade result 3.
    Quit,
    /// Terminal fade result 4: commit settings/live state before quitting.
    SaveAndQuit,
}

/// Backend-neutral state for the active and selected descriptors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrontendMenu {
    current: ScreenId,
    next: Option<ScreenId>,
    phase: MenuPhase,
}

impl FrontendMenu {
    #[must_use]
    pub fn new(initial: ScreenId) -> Self {
        Self {
            current: initial,
            next: None,
            phase: MenuPhase::Interactive,
        }
    }

    #[must_use]
    pub fn current(&self) -> ScreenId {
        self.current
    }

    #[must_use]
    pub fn phase(&self) -> MenuPhase {
        self.phase
    }

    /// Supply the active descriptor's result. `Some` selects the next screen
    /// and enters state 4; `None` leaves the menu interactive.
    pub fn complete_interaction(&mut self, next: Option<ScreenId>) {
        if self.phase != MenuPhase::Interactive {
            return;
        }
        if let Some(next) = next {
            self.next = Some(next);
            self.phase = MenuPhase::Transition;
        }
    }

    /// Advance the non-interactive transition states. Fade result 0 enters the
    /// selected screen, 3 quits, and 4 saves then quits; other values continue
    /// the current fade. Returns at most one native callback boundary.
    pub fn advance(&mut self, fade_result: u8) -> Option<MenuAction> {
        match self.phase {
            MenuPhase::Interactive => None,
            MenuPhase::Transition => {
                let to = self.next?;
                self.phase = MenuPhase::FadeOut;
                Some(MenuAction::BeginTransition {
                    from: self.current,
                    to,
                })
            }
            MenuPhase::FadeOut => match fade_result {
                0 => {
                    let to = self.next?;
                    self.current = to;
                    self.next = None;
                    self.phase = MenuPhase::FadeIn;
                    Some(MenuAction::Enter(to))
                }
                3 => Some(MenuAction::Quit),
                4 => Some(MenuAction::SaveAndQuit),
                _ => None,
            },
            MenuPhase::FadeIn => {
                if fade_result == 0 {
                    self.phase = MenuPhase::Interactive;
                }
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_handoff_runs_all_four_native_phases() {
        let mut menu = FrontendMenu::new(10);
        menu.complete_interaction(None);
        assert_eq!(menu.phase(), MenuPhase::Interactive);
        menu.complete_interaction(Some(20));
        assert_eq!(menu.phase(), MenuPhase::Transition);
        assert_eq!(
            menu.advance(0),
            Some(MenuAction::BeginTransition { from: 10, to: 20 })
        );
        assert_eq!(menu.phase(), MenuPhase::FadeOut);
        assert_eq!(menu.advance(1), None);
        assert_eq!(menu.phase(), MenuPhase::FadeOut);
        assert_eq!(menu.advance(0), Some(MenuAction::Enter(20)));
        assert_eq!(menu.current(), 20);
        assert_eq!(menu.phase(), MenuPhase::FadeIn);
        assert_eq!(menu.advance(0), None);
        assert_eq!(menu.phase(), MenuPhase::Interactive);
        assert_eq!(menu.advance(0), None);
    }

    #[test]
    fn terminal_fade_results_are_distinct() {
        let mut quit = FrontendMenu::new(1);
        quit.complete_interaction(Some(2));
        quit.advance(0);
        assert_eq!(quit.advance(3), Some(MenuAction::Quit));

        let mut save = FrontendMenu::new(1);
        save.complete_interaction(Some(2));
        save.advance(0);
        assert_eq!(save.advance(4), Some(MenuAction::SaveAndQuit));
    }

    #[test]
    fn interaction_result_is_accepted_only_in_interactive_phase() {
        let mut menu = FrontendMenu::new(1);
        menu.complete_interaction(Some(2));
        menu.complete_interaction(Some(3));
        assert_eq!(
            menu.advance(0),
            Some(MenuAction::BeginTransition { from: 1, to: 2 })
        );
        assert_eq!(menu.advance(0), Some(MenuAction::Enter(2)));
        menu.complete_interaction(Some(3));
        menu.advance(0);
        assert_eq!(menu.phase(), MenuPhase::Interactive);
        menu.complete_interaction(Some(3));
        assert_eq!(menu.phase(), MenuPhase::Transition);
    }
}
