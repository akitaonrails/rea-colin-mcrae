//! End-to-end native host lifecycle: focus, pause, restore, and shutdown
//! (reconstruction item B12).
//!
//! B09 recovered the host loop ([`crate::host_loop`]): the active flag
//! (`DAT_0096efcc`) gates whether the loop runs frames or parks on the message
//! queue, the quit flag (`DAT_0096efe8`) leaves the loop, and `WM_ACTIVATEAPP`
//! writes the active flag. The window procedure ([`crate::window`]) turns the
//! same `WM_ACTIVATEAPP` into surface effects: a gain restores lost surfaces
//! and relatches activation, a loss (under the deactivation guard) marks a
//! surface rebuild as owed. B11 defined how a frame is presented once the loop
//! runs.
//!
//! This module composes those recovered pieces into the single lifecycle a
//! modern backend drives. A platform event (SDL window focus, a close request)
//! becomes a [`PlatformLifecycleEvent`]; [`HostLifecycle::apply`] feeds the
//! recovered `WM_ACTIVATEAPP`/quit transitions to both state machines at once
//! and reports the resulting surface effects, pause state, and whether a
//! surface rebuild is owed. The loop then drives frames or parks through
//! [`HostLifecycle::next_action`]. The mapping is backend-neutral and fully
//! testable without a window system; the SDL backend supplies the events.
//!
//! A platform close request sets the loop quit flag directly. In the recovered
//! window procedure `WM_CLOSE` is swallowed and does not itself initiate exit;
//! the shutdown path is the loop quit flag, so that is the integration point a
//! modern backend's close button drives.

use crate::host_loop::{HostAction, HostLoopState};
use crate::window::{WM_ACTIVATEAPP, WindowEffect, WindowMessage, WindowMode, WindowRuntimeState};

/// A platform lifecycle event, neutral across backends.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlatformLifecycleEvent {
    /// The window gained input focus (SDL `FOCUS_GAINED`): resume and restore
    /// surfaces.
    FocusGained,
    /// The window lost input focus (SDL `FOCUS_LOST`, or was minimized): pause
    /// and mark a surface rebuild owed.
    FocusLost,
    /// The user asked to close the window (SDL `Quit` / window close): shut
    /// down.
    CloseRequested,
}

/// What a backend must do after a lifecycle event: apply the recovered window
/// effects, observe whether the loop is now paused or shutting down, and
/// perform a surface rebuild if one is owed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LifecycleTransition {
    /// Surface effects the recovered window procedure produced (e.g.
    /// [`WindowEffect::RestoreLostSurfaces`] on a focus gain).
    pub effects: Vec<WindowEffect>,
    /// Whether the loop is now parked (inactive and not shutting down).
    pub paused: bool,
    /// Whether a shutdown has been requested.
    pub shutting_down: bool,
    /// Whether a surface rebuild is owed (latched by a focus loss, cleared by
    /// [`HostLifecycle::take_surface_rebuild`]).
    pub surface_rebuild_owed: bool,
}

/// The composed native host lifecycle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostLifecycle {
    mode: WindowMode,
    loop_state: HostLoopState,
    window: WindowRuntimeState,
    shutting_down: bool,
}

impl HostLifecycle {
    /// A lifecycle parked until the first
    /// [`PlatformLifecycleEvent::FocusGained`], matching the recovered loop,
    /// which starts inactive until the first `WM_ACTIVATEAPP`.
    pub fn new(mode: WindowMode) -> Self {
        HostLifecycle {
            mode,
            loop_state: HostLoopState::new(),
            window: WindowRuntimeState {
                active: false,
                activation_dispatch_enabled: true,
                force_wait_once: false,
                exit_requested: false,
                activation_latched: false,
                rebuild_pending: false,
                deactivation_guard: true,
                clear_black_on_paint: false,
                vertical_blank_fallback: false,
            },
            shutting_down: false,
        }
    }

    /// Apply a platform lifecycle event to both recovered state machines and
    /// report the resulting transition.
    pub fn apply(&mut self, event: PlatformLifecycleEvent) -> LifecycleTransition {
        let effects = match event {
            PlatformLifecycleEvent::FocusGained => {
                let outcome = self.window.handle_message(self.mode, activate(true));
                self.loop_state.on_activate_app(true);
                outcome.effects
            }
            PlatformLifecycleEvent::FocusLost => {
                let outcome = self.window.handle_message(self.mode, activate(false));
                self.loop_state.on_activate_app(false);
                outcome.effects
            }
            PlatformLifecycleEvent::CloseRequested => {
                self.loop_state.request_quit();
                self.shutting_down = true;
                Vec::new()
            }
        };

        LifecycleTransition {
            effects,
            paused: !self.shutting_down && !self.window.active,
            shutting_down: self.shutting_down,
            surface_rebuild_owed: self.window.rebuild_pending,
        }
    }

    /// The loop action to take this iteration (quit wins; an inactive window
    /// parks on the queue; otherwise run a frame).
    pub fn next_action(&mut self) -> HostAction {
        self.loop_state.next_action()
    }

    /// Whether the window is currently active (focused and not shutting down).
    pub fn is_active(&self) -> bool {
        self.window.active && !self.shutting_down
    }

    /// Whether a shutdown has been requested.
    pub fn is_shutting_down(&self) -> bool {
        self.shutting_down
    }

    /// Consume the owed-surface-rebuild latch, returning whether one was owed.
    pub fn take_surface_rebuild(&mut self) -> bool {
        let owed = self.window.rebuild_pending;
        self.window.rebuild_pending = false;
        owed
    }

    /// The window mode the lifecycle was created with.
    pub fn mode(&self) -> WindowMode {
        self.mode
    }

    /// The recovered window runtime state, for inspection.
    pub fn window(&self) -> &WindowRuntimeState {
        &self.window
    }
}

fn activate(active: bool) -> WindowMessage {
    WindowMessage {
        id: WM_ACTIVATEAPP,
        wparam: u32::from(active),
        lparam: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_lifecycle_is_parked_until_first_focus() {
        let mut lifecycle = HostLifecycle::new(WindowMode::Windowed);
        assert!(!lifecycle.is_active());
        assert!(!lifecycle.is_shutting_down());
        assert_eq!(lifecycle.next_action(), HostAction::BlockingWait);
    }

    #[test]
    fn focus_gain_resumes_and_restores_surfaces() {
        let mut lifecycle = HostLifecycle::new(WindowMode::Windowed);
        let transition = lifecycle.apply(PlatformLifecycleEvent::FocusGained);
        assert_eq!(transition.effects, [WindowEffect::RestoreLostSurfaces]);
        assert!(!transition.paused);
        assert!(!transition.surface_rebuild_owed);
        assert!(lifecycle.is_active());
        assert_eq!(lifecycle.next_action(), HostAction::RunFrame);
    }

    #[test]
    fn focus_loss_pauses_and_owes_a_rebuild() {
        let mut lifecycle = HostLifecycle::new(WindowMode::Windowed);
        lifecycle.apply(PlatformLifecycleEvent::FocusGained);
        let transition = lifecycle.apply(PlatformLifecycleEvent::FocusLost);
        assert_eq!(transition.effects, []);
        assert!(transition.paused);
        assert!(transition.surface_rebuild_owed);
        assert!(!lifecycle.is_active());
        assert_eq!(lifecycle.next_action(), HostAction::BlockingWait);
    }

    #[test]
    fn restore_after_focus_loss_emits_restore_and_clears_the_owed_rebuild() {
        let mut lifecycle = HostLifecycle::new(WindowMode::Windowed);
        lifecycle.apply(PlatformLifecycleEvent::FocusGained);
        lifecycle.apply(PlatformLifecycleEvent::FocusLost);
        let transition = lifecycle.apply(PlatformLifecycleEvent::FocusGained);
        assert_eq!(transition.effects, [WindowEffect::RestoreLostSurfaces]);
        assert!(!transition.paused);
        assert!(transition.surface_rebuild_owed);
        // The backend performs the rebuild once, then the latch is clear.
        assert!(lifecycle.take_surface_rebuild());
        assert!(!lifecycle.take_surface_rebuild());
        assert_eq!(lifecycle.next_action(), HostAction::RunFrame);
    }

    #[test]
    fn close_request_shuts_down_regardless_of_focus() {
        let mut lifecycle = HostLifecycle::new(WindowMode::Windowed);
        lifecycle.apply(PlatformLifecycleEvent::FocusGained);
        let transition = lifecycle.apply(PlatformLifecycleEvent::CloseRequested);
        assert!(transition.shutting_down);
        assert!(!transition.paused);
        assert!(lifecycle.is_shutting_down());
        assert!(!lifecycle.is_active());
        assert_eq!(lifecycle.next_action(), HostAction::Quit);
    }

    #[test]
    fn quit_wins_over_an_inactive_window() {
        let mut lifecycle = HostLifecycle::new(WindowMode::Windowed);
        lifecycle.apply(PlatformLifecycleEvent::FocusLost);
        lifecycle.apply(PlatformLifecycleEvent::CloseRequested);
        // Even a later focus gain does not revive a shutting-down loop.
        lifecycle.apply(PlatformLifecycleEvent::FocusGained);
        assert!(lifecycle.is_shutting_down());
        assert_eq!(lifecycle.next_action(), HostAction::Quit);
    }

    #[test]
    fn fullscreen_mode_is_carried_through() {
        let lifecycle = HostLifecycle::new(WindowMode::Fullscreen);
        assert_eq!(lifecycle.mode(), WindowMode::Fullscreen);
        assert!(!lifecycle.window().active);
    }
}
