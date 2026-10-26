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

/// Visibility and selectability flags of one `0x34`-byte widget record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WidgetState {
    pub visible: bool,
    pub selectable: bool,
}

/// Direction of the recovered vertical descriptor walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerticalDirection {
    Previous,
    Next,
}

/// Result of attempting vertical navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerticalNavigation {
    Selection {
        previous: usize,
        current: usize,
        moved: bool,
    },
    PreviousScreen,
    NextScreen,
}

/// Descriptor-owned selection state. Screen-link lookup remains a caller
/// callback, as in the native descriptor pointers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WidgetMenu {
    widgets: Vec<WidgetState>,
    selected: usize,
    wrap: bool,
}

impl WidgetMenu {
    #[must_use]
    pub fn new(widgets: Vec<WidgetState>, selected: usize, wrap: bool) -> Self {
        let mut menu = Self {
            widgets,
            selected: selected.min(usize::MAX - 1),
            wrap,
        };
        if menu.widgets.is_empty() {
            menu.selected = 0;
            return menu;
        }
        menu.selected = menu.selected.min(menu.widgets.len() - 1);
        for _ in 0..menu.widgets.len() {
            if menu.widgets[menu.selected].visible {
                break;
            }
            menu.selected = (menu.selected + 1) % menu.widgets.len();
        }
        menu
    }

    #[must_use]
    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn navigate(
        &mut self,
        direction: VerticalDirection,
        previous_screen: bool,
        next_screen: bool,
    ) -> VerticalNavigation {
        if self.widgets.is_empty() {
            return VerticalNavigation::Selection {
                previous: 0,
                current: 0,
                moved: false,
            };
        }
        let previous = self.selected;
        match direction {
            VerticalDirection::Previous if self.selected == 0 && previous_screen => {
                return VerticalNavigation::PreviousScreen;
            }
            VerticalDirection::Next if self.selected + 1 == self.widgets.len() && next_screen => {
                return VerticalNavigation::NextScreen;
            }
            _ => {}
        }

        let can_move = match direction {
            VerticalDirection::Previous => self.selected > 0 || self.wrap,
            VerticalDirection::Next => self.selected + 1 < self.widgets.len() || self.wrap,
        };
        if can_move {
            for _ in 0..self.widgets.len() {
                self.selected = match direction {
                    VerticalDirection::Previous => {
                        (self.selected + self.widgets.len() - 1) % self.widgets.len()
                    }
                    VerticalDirection::Next => (self.selected + 1) % self.widgets.len(),
                };
                let widget = self.widgets[self.selected];
                if widget.visible && widget.selectable {
                    return VerticalNavigation::Selection {
                        previous,
                        current: self.selected,
                        moved: true,
                    };
                }
            }
        }
        self.selected = previous;
        VerticalNavigation::Selection {
            previous,
            current: self.selected,
            moved: false,
        }
    }
}

/// Direction of a value-widget adjustment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueDirection {
    Decrement,
    Increment,
}

/// Mutable value fields of a type-3/type-6 widget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValueWidget {
    pub value: i32,
    pub value_count: i32,
    pub wrap: bool,
}

impl ValueWidget {
    /// Apply the native signed decrement/increment and wrap rules. The boolean
    /// is the success flag supplied to the descriptor's change callback.
    pub fn adjust(&mut self, direction: ValueDirection) -> bool {
        if self.value_count <= 0 {
            return false;
        }
        match direction {
            ValueDirection::Decrement => {
                if self.value < 1 && !self.wrap {
                    return false;
                }
                self.value = self.value.wrapping_sub(1);
                if self.value < 0 {
                    self.value = self.value_count.wrapping_add(self.value);
                }
            }
            ValueDirection::Increment => {
                if self.value >= self.value_count - 1 && !self.wrap {
                    return false;
                }
                self.value = self.value.wrapping_add(1);
                if self.value >= self.value_count {
                    self.value = 0;
                }
            }
        }
        true
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

    fn widget(visible: bool, selectable: bool) -> WidgetState {
        WidgetState {
            visible,
            selectable,
        }
    }

    #[test]
    fn widget_navigation_skips_unavailable_records_and_wraps() {
        let mut menu = WidgetMenu::new(
            vec![
                widget(false, true),
                widget(true, false),
                widget(true, true),
                widget(true, true),
            ],
            0,
            true,
        );
        assert_eq!(menu.selected(), 1);
        assert_eq!(
            menu.navigate(VerticalDirection::Next, false, false),
            VerticalNavigation::Selection {
                previous: 1,
                current: 2,
                moved: true,
            }
        );
        assert_eq!(
            menu.navigate(VerticalDirection::Next, false, false),
            VerticalNavigation::Selection {
                previous: 2,
                current: 3,
                moved: true,
            }
        );
        assert_eq!(
            menu.navigate(VerticalDirection::Next, false, false),
            VerticalNavigation::Selection {
                previous: 3,
                current: 2,
                moved: true,
            }
        );
        assert_eq!(
            menu.navigate(VerticalDirection::Previous, false, false),
            VerticalNavigation::Selection {
                previous: 2,
                current: 3,
                moved: true,
            }
        );
    }

    #[test]
    fn widget_navigation_distinguishes_boundaries_links_and_empty_menus() {
        let mut menu = WidgetMenu::new(vec![widget(true, true)], 0, false);
        assert_eq!(
            menu.navigate(VerticalDirection::Previous, false, false),
            VerticalNavigation::Selection {
                previous: 0,
                current: 0,
                moved: false,
            }
        );
        assert_eq!(
            menu.navigate(VerticalDirection::Previous, true, false),
            VerticalNavigation::PreviousScreen
        );
        assert_eq!(
            menu.navigate(VerticalDirection::Next, false, true),
            VerticalNavigation::NextScreen
        );

        let mut empty = WidgetMenu::new(Vec::new(), 99, true);
        assert_eq!(
            empty.navigate(VerticalDirection::Next, true, true),
            VerticalNavigation::Selection {
                previous: 0,
                current: 0,
                moved: false,
            }
        );
    }

    #[test]
    fn value_widget_wraps_or_blocks_at_native_edges() {
        let mut value = ValueWidget {
            value: 0,
            value_count: 3,
            wrap: false,
        };
        assert!(!value.adjust(ValueDirection::Decrement));
        assert_eq!(value.value, 0);
        assert!(value.adjust(ValueDirection::Increment));
        assert!(value.adjust(ValueDirection::Increment));
        assert_eq!(value.value, 2);
        assert!(!value.adjust(ValueDirection::Increment));
        assert!(value.adjust(ValueDirection::Decrement));
        assert_eq!(value.value, 1);

        value.wrap = true;
        assert!(value.adjust(ValueDirection::Decrement));
        assert_eq!(value.value, 0);
        assert!(value.adjust(ValueDirection::Decrement));
        assert_eq!(value.value, 2);
        assert!(value.adjust(ValueDirection::Increment));
        assert_eq!(value.value, 0);
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
