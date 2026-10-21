//! Host input translation into the recovered frontend input model.
//!
//! SDL keyboard and gamepad signals are mapped onto the recovered
//! [`cmr_core::frontend::FrontendInput`] contract. The recovered frontend is
//! edge-triggered: one call to `FrontendState::step` with `next` set advances
//! the selection exactly once. This mapper therefore emits one pulse per
//! press transition and never auto-repeats while an input is held; key/axis
//! auto-repeat is deliberately a host input policy, not a recovered behavior.
//!
//! The specific key and button bindings below are host policy. The shipped
//! game used DirectInput and its exact device bindings are not recovered, so
//! these choices are not a compatibility claim.

use std::collections::BTreeMap;

use cmr_core::frontend::FrontendInput;
use sdl3::gamepad::{Axis, Button};
use sdl3::keyboard::Keycode;

/// Minimum absolute analog-axis magnitude treated as a held direction.
///
/// SDL analog axes report `i16` values; half of the positive range gives a
/// generous dead zone so a centered stick never latches a direction.
pub const AXIS_DEAD_ZONE: i16 = 16_384;

/// One recovered frontend action a host device can drive.
///
/// The four actions correspond exactly to the fields of [`FrontendInput`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendAction {
    Previous,
    Next,
    Confirm,
    Cancel,
}

impl FrontendAction {
    const COUNT: usize = 4;

    const fn index(self) -> usize {
        self as usize
    }
}

/// Resolves a keyboard key to a frontend action, or `None` when unbound.
///
/// `Escape` is intentionally absent: the presenter uses it as a host
/// window-close shortcut, which is distinct from the recovered cancel action.
#[must_use]
pub fn action_for_key(keycode: Keycode) -> Option<FrontendAction> {
    match keycode {
        Keycode::Left | Keycode::Up => Some(FrontendAction::Previous),
        Keycode::Right | Keycode::Down => Some(FrontendAction::Next),
        Keycode::Return | Keycode::KpEnter | Keycode::Space => Some(FrontendAction::Confirm),
        Keycode::Backspace => Some(FrontendAction::Cancel),
        _ => None,
    }
}

/// Resolves a gamepad button to a frontend action, or `None` when unbound.
#[must_use]
pub fn action_for_button(button: Button) -> Option<FrontendAction> {
    match button {
        Button::DPadLeft | Button::DPadUp => Some(FrontendAction::Previous),
        Button::DPadRight | Button::DPadDown => Some(FrontendAction::Next),
        Button::South => Some(FrontendAction::Confirm),
        Button::East => Some(FrontendAction::Cancel),
        _ => None,
    }
}

/// Resolves a left-stick axis position to a direction, or `None` inside the
/// dead zone or for any non-navigation axis.
///
/// The vertical axis follows SDL's convention: negative is up (previous) and
/// positive is down (next).
#[must_use]
pub fn axis_action(axis: Axis, value: i16) -> Option<FrontendAction> {
    if !matches!(axis, Axis::LeftX | Axis::LeftY) {
        None
    } else if value <= -AXIS_DEAD_ZONE {
        Some(FrontendAction::Previous)
    } else if value >= AXIS_DEAD_ZONE {
        Some(FrontendAction::Next)
    } else {
        None
    }
}

/// A distinct physical input whose held state the mapper tracks.
///
/// Several sources may resolve to the same action (for example, the D-pad and
/// a keyboard arrow both request `Previous`); each is tracked independently so
/// releasing one never clears an action another source still holds.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct InputSource {
    kind: u8,
    code: i64,
}

impl InputSource {
    const KEY: u8 = 0;
    const BUTTON: u8 = 1;
    const AXIS: u8 = 2;

    const fn key(keycode: Keycode) -> Self {
        Self {
            kind: Self::KEY,
            code: keycode as u32 as i64,
        }
    }

    const fn button(button: Button) -> Self {
        Self {
            kind: Self::BUTTON,
            code: button as i64,
        }
    }

    const fn axis(axis: Axis) -> Self {
        Self {
            kind: Self::AXIS,
            code: axis as i64,
        }
    }
}

/// Edge-triggered accumulator from host device signals to [`FrontendInput`].
///
/// Feed raw SDL events with [`apply_key`](Self::apply_key),
/// [`apply_button`](Self::apply_button), and [`apply_axis`](Self::apply_axis),
/// then drain one frame's worth of pulses with [`poll`](Self::poll).
#[derive(Clone, Debug, Default)]
pub struct FrontendInputMapper {
    active: BTreeMap<InputSource, FrontendAction>,
    pressed_edges: [bool; FrontendAction::COUNT],
}

impl FrontendInputMapper {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn is_held(&self, action: FrontendAction) -> bool {
        self.active.values().any(|&held| held == action)
    }

    fn press(&mut self, source: InputSource, action: FrontendAction) {
        if !self.is_held(action) {
            self.pressed_edges[action.index()] = true;
        }
        // Overwriting re-maps a source that changed direction (for example, an
        // axis swinging across center); the previous action is dropped so it is
        // no longer considered held unless another source still provides it.
        self.active.insert(source, action);
    }

    fn release(&mut self, source: InputSource) {
        self.active.remove(&source);
    }

    /// Applies a keyboard key-down (`pressed`) or key-up event.
    pub fn apply_key(&mut self, keycode: Keycode, pressed: bool) {
        self.apply(InputSource::key(keycode), action_for_key(keycode), pressed);
    }

    /// Applies a gamepad button-down (`pressed`) or button-up event.
    pub fn apply_button(&mut self, button: Button, pressed: bool) {
        self.apply(
            InputSource::button(button),
            action_for_button(button),
            pressed,
        );
    }

    /// Applies a gamepad analog-axis motion, latching or clearing a direction.
    pub fn apply_axis(&mut self, axis: Axis, value: i16) {
        let source = InputSource::axis(axis);
        match axis_action(axis, value) {
            Some(action) => self.press(source, action),
            None => self.release(source),
        }
    }

    fn apply(&mut self, source: InputSource, action: Option<FrontendAction>, pressed: bool) {
        match (action, pressed) {
            (Some(action), true) => self.press(source, action),
            (Some(_), false) => self.release(source),
            // An unbound key still clears any stale mapping for its source.
            (None, _) => self.release(source),
        }
    }

    /// Drains the press edges accumulated since the last poll into one
    /// [`FrontendInput`] suitable for a single `FrontendState::step` call.
    #[must_use]
    pub fn poll(&mut self) -> FrontendInput {
        let input = FrontendInput {
            previous: self.pressed_edges[FrontendAction::Previous.index()],
            next: self.pressed_edges[FrontendAction::Next.index()],
            confirm: self.pressed_edges[FrontendAction::Confirm.index()],
            cancel: self.pressed_edges[FrontendAction::Cancel.index()],
        };
        self.pressed_edges = [false; FrontendAction::COUNT];
        input
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cmr_core::frontend::{FrontendEffect, FrontendState, ROOT_SCREEN, TransitionResult};

    #[test]
    fn keyboard_and_dpad_bindings_resolve_to_the_recovered_actions() {
        assert_eq!(
            action_for_key(Keycode::Left),
            Some(FrontendAction::Previous)
        );
        assert_eq!(action_for_key(Keycode::Up), Some(FrontendAction::Previous));
        assert_eq!(action_for_key(Keycode::Right), Some(FrontendAction::Next));
        assert_eq!(action_for_key(Keycode::Down), Some(FrontendAction::Next));
        assert_eq!(
            action_for_key(Keycode::Return),
            Some(FrontendAction::Confirm)
        );
        assert_eq!(
            action_for_key(Keycode::Space),
            Some(FrontendAction::Confirm)
        );
        assert_eq!(
            action_for_key(Keycode::Backspace),
            Some(FrontendAction::Cancel)
        );
        // Escape stays a window-close shortcut, never the recovered cancel.
        assert_eq!(action_for_key(Keycode::Escape), None);

        assert_eq!(
            action_for_button(Button::DPadLeft),
            Some(FrontendAction::Previous)
        );
        assert_eq!(
            action_for_button(Button::DPadRight),
            Some(FrontendAction::Next)
        );
        assert_eq!(
            action_for_button(Button::South),
            Some(FrontendAction::Confirm)
        );
        assert_eq!(
            action_for_button(Button::East),
            Some(FrontendAction::Cancel)
        );
        assert_eq!(action_for_button(Button::Start), None);
    }

    #[test]
    fn axis_latches_direction_only_outside_the_dead_zone() {
        assert_eq!(axis_action(Axis::LeftX, 0), None);
        assert_eq!(axis_action(Axis::LeftX, AXIS_DEAD_ZONE - 1), None);
        assert_eq!(
            axis_action(Axis::LeftX, AXIS_DEAD_ZONE),
            Some(FrontendAction::Next)
        );
        assert_eq!(
            axis_action(Axis::LeftX, -AXIS_DEAD_ZONE),
            Some(FrontendAction::Previous)
        );
        assert_eq!(
            axis_action(Axis::LeftY, i16::MIN),
            Some(FrontendAction::Previous)
        );
        assert_eq!(axis_action(Axis::RightX, i16::MAX), None);
        assert_eq!(axis_action(Axis::TriggerLeft, i16::MAX), None);
    }

    #[test]
    fn a_held_key_pulses_once_until_released_and_pressed_again() {
        let mut mapper = FrontendInputMapper::new();

        mapper.apply_key(Keycode::Right, true);
        assert!(mapper.poll().next);
        // Still held, but no new press transition: no further pulse.
        assert!(!mapper.poll().next);

        mapper.apply_key(Keycode::Right, false);
        mapper.apply_key(Keycode::Right, true);
        assert!(mapper.poll().next);
    }

    #[test]
    fn releasing_one_source_keeps_an_action_another_source_still_holds() {
        let mut mapper = FrontendInputMapper::new();

        mapper.apply_key(Keycode::Up, true);
        mapper.apply_button(Button::DPadUp, true);
        assert!(mapper.poll().previous);

        // Releasing the key alone must not re-arm an edge while the D-pad holds.
        mapper.apply_key(Keycode::Up, false);
        mapper.apply_key(Keycode::Up, true);
        assert!(
            !mapper.poll().previous,
            "action was continuously held by the D-pad"
        );

        // Only after every source releases does a fresh press pulse again.
        mapper.apply_key(Keycode::Up, false);
        mapper.apply_button(Button::DPadUp, false);
        mapper.apply_key(Keycode::Up, true);
        assert!(mapper.poll().previous);
    }

    #[test]
    fn an_axis_swinging_across_center_re_arms_the_opposite_direction() {
        let mut mapper = FrontendInputMapper::new();

        mapper.apply_axis(Axis::LeftX, i16::MAX);
        assert!(mapper.poll().next);

        mapper.apply_axis(Axis::LeftX, 0);
        let centered = mapper.poll();
        assert!(!centered.next && !centered.previous);

        mapper.apply_axis(Axis::LeftX, i16::MIN);
        assert!(mapper.poll().previous);
    }

    #[test]
    fn poll_with_no_new_press_is_the_neutral_input() {
        let mut mapper = FrontendInputMapper::new();
        assert_eq!(mapper.poll(), FrontendInput::default());
    }

    #[test]
    fn mapped_input_drives_the_recovered_frontend_selection_contract() {
        // One previous pulse wraps 0 -> last; a confirm then opens that entry.
        let mut mapper = FrontendInputMapper::new();
        let mut state = FrontendState::after_startup_art(0, 0).unwrap();
        // Advance the bootstrap transition into the interactive root screen.
        let _ = state.step(
            FrontendInput::default(),
            1,
            TransitionResult::Complete,
            true,
        );
        let _ = state.step(
            FrontendInput::default(),
            2,
            TransitionResult::Complete,
            true,
        );

        let last_selection = (ROOT_SCREEN.entries.len() - 1) as u8;
        mapper.apply_button(Button::DPadLeft, true);
        let effects = state.step(mapper.poll(), 3, TransitionResult::Pending, true);
        assert_eq!(state.root_selection().index(), last_selection);
        assert_eq!(
            effects,
            vec![FrontendEffect::PersistRootSelection(last_selection)]
        );

        mapper.apply_button(Button::DPadLeft, false);
        mapper.apply_key(Keycode::Return, true);
        let _ = state.step(mapper.poll(), 4, TransitionResult::Pending, true);
        assert!(state.pending_screen().is_some());
    }
}
