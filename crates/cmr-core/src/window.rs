pub const WM_PAINT: u32 = 0x000f;
pub const WM_CLOSE: u32 = 0x0010;
pub const WM_ERASEBKGND: u32 = 0x0014;
pub const WM_ACTIVATEAPP: u32 = 0x001c;
pub const WM_SETCURSOR: u32 = 0x0020;
pub const WM_SYSCOMMAND: u32 = 0x0112;
pub const WM_QUERYNEWPALETTE: u32 = 0x030f;
pub const WM_PALETTECHANGED: u32 = 0x0311;

pub const SC_MOUSEMENU: u32 = 0xf090;
pub const SC_KEYMENU: u32 = 0xf100;
pub const SC_SCREENSAVE: u32 = 0xf140;
pub const SC_MONITORPOWER: u32 = 0xf170;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowMode {
    Windowed,
    Fullscreen,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CursorSelection {
    Arrow,
    None,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowCreationPlan {
    pub class_style: u32,
    pub class_extra_bytes: i32,
    pub window_extra_bytes: i32,
    pub window_style: u32,
    pub extended_style: u32,
    pub cursor: CursorSelection,
    pub background_stock_object: i32,
    pub has_icon: bool,
    pub has_menu: bool,
    pub has_parent: bool,
    pub class_name_is_title: bool,
    pub x: i32,
    pub y: i32,
    pub show_command: i32,
    pub call_show_cursor_false_after_creation_attempt: bool,
    pub initial_screen_width: i32,
    pub initial_screen_height: i32,
}

/// Numeric Win32 creation contract observed at the original call boundary.
/// Symbolic style decomposition stays in the eventual compatibility backend.
pub fn window_creation_plan(
    mode: WindowMode,
    screen_width: i32,
    screen_height: i32,
) -> WindowCreationPlan {
    match mode {
        WindowMode::Windowed => WindowCreationPlan {
            class_style: 0x3003,
            class_extra_bytes: 0,
            window_extra_bytes: 0,
            window_style: 0x80ca_0000,
            extended_style: 8,
            cursor: CursorSelection::Arrow,
            background_stock_object: 5,
            has_icon: false,
            has_menu: false,
            has_parent: false,
            class_name_is_title: true,
            x: 0,
            y: 0,
            show_command: 0,
            call_show_cursor_false_after_creation_attempt: true,
            initial_screen_width: screen_width,
            initial_screen_height: screen_height,
        },
        WindowMode::Fullscreen => WindowCreationPlan {
            class_style: 3,
            class_extra_bytes: 0,
            window_extra_bytes: 0,
            window_style: 0x8008_0000,
            extended_style: 8,
            cursor: CursorSelection::None,
            background_stock_object: 5,
            has_icon: false,
            has_menu: false,
            has_parent: false,
            class_name_is_title: true,
            x: 0,
            y: 0,
            show_command: 3,
            call_show_cursor_false_after_creation_attempt: true,
            initial_screen_width: screen_width,
            initial_screen_height: screen_height,
        },
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowMessage {
    pub id: u32,
    pub wparam: u32,
    pub lparam: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MessageDisposition {
    Handled,
    DefaultWindowProcedure,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowEffect {
    ValidatePaint { clear_black: bool },
    SetCursorNone,
    RestoreLostSurfaces,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageOutcome {
    pub disposition: MessageDisposition,
    pub effects: Vec<WindowEffect>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowRuntimeState {
    pub active: bool,
    pub activation_dispatch_enabled: bool,
    pub force_wait_once: bool,
    pub exit_requested: bool,
    pub activation_latched: bool,
    pub rebuild_pending: bool,
    pub deactivation_guard: bool,
    pub clear_black_on_paint: bool,
    pub vertical_blank_fallback: bool,
}

impl WindowRuntimeState {
    pub fn handle_message(&mut self, mode: WindowMode, message: WindowMessage) -> MessageOutcome {
        let mut effects = Vec::new();
        let disposition = match message.id {
            WM_PAINT => {
                effects.push(WindowEffect::ValidatePaint {
                    clear_black: self.clear_black_on_paint,
                });
                MessageDisposition::Handled
            }
            WM_CLOSE | WM_ERASEBKGND | WM_QUERYNEWPALETTE | WM_PALETTECHANGED => {
                MessageDisposition::Handled
            }
            WM_ACTIVATEAPP => {
                self.active = message.wparam != 0;
                if self.active {
                    effects.push(WindowEffect::RestoreLostSurfaces);
                    if !self.activation_latched {
                        self.activation_latched = true;
                    }
                } else if self.deactivation_guard {
                    self.rebuild_pending = true;
                    self.activation_latched = false;
                }
                MessageDisposition::Handled
            }
            WM_SETCURSOR => {
                if mode == WindowMode::Fullscreen {
                    effects.push(WindowEffect::SetCursorNone);
                }
                MessageDisposition::Handled
            }
            WM_SYSCOMMAND
                if matches!(
                    message.wparam,
                    SC_MOUSEMENU | SC_KEYMENU | SC_SCREENSAVE | SC_MONITORPOWER
                ) =>
            {
                MessageDisposition::Handled
            }
            _ => MessageDisposition::DefaultWindowProcedure,
        };
        MessageOutcome {
            disposition,
            effects,
        }
    }

    pub fn plan_loop_iteration(&self, queue: QueueObservation) -> LoopIteration {
        if self.exit_requested {
            return LoopIteration {
                queued: QueuedMessageAction::None,
                continuation: LoopContinuation::StopForExitRequest,
            };
        }
        let queued = match queue {
            QueueObservation::Message {
                id: _,
                result: GetMessageResult::Quit { exit_code },
            } => {
                return LoopIteration {
                    queued: QueuedMessageAction::Quit { exit_code },
                    continuation: LoopContinuation::StopForQuitMessage,
                };
            }
            QueueObservation::Message {
                id: WM_ACTIVATEAPP,
                result: GetMessageResult::Message,
            } if !self.activation_dispatch_enabled => QueuedMessageAction::ConsumeWithoutDispatch,
            QueueObservation::Message {
                result: GetMessageResult::Message,
                ..
            } => QueuedMessageAction::Dispatch,
            QueueObservation::Empty => QueuedMessageAction::None,
        };
        LoopIteration {
            queued,
            continuation: LoopContinuation::EvaluateAfterQueuedPhase,
        }
    }

    /// Resolve only after the planned queued action has been dispatched or
    /// consumed: WM_ACTIVATEAPP may have changed `active` in that first phase.
    pub fn resolve_post_queue_action(&mut self) -> PostQueueAction {
        if !self.active || self.force_wait_once {
            self.force_wait_once = false;
            // This blocking branch dispatches after GetMessage without checking
            // its return, unlike the optional queued-message phase.
            PostQueueAction::WaitThenDispatchIgnoringGetMessageResult
        } else {
            PostQueueAction::Tick
        }
    }

    pub fn record_vertical_blank_wait(&mut self, succeeded: bool) {
        if !succeeded {
            self.vertical_blank_fallback = true;
        }
    }

    pub fn presentation_plan(&self, mode: WindowMode) -> PresentationPlan {
        match mode {
            WindowMode::Fullscreen => PresentationPlan::Fullscreen {
                sample_back_buffer_pixel: true,
                flip_numeric_flags: 1,
            },
            WindowMode::Windowed => PresentationPlan::Windowed {
                resolve_client_origin: true,
                synchronization: if self.vertical_blank_fallback {
                    WindowedSynchronization::PollVgaBlankEdge { port: 0x03da }
                } else {
                    WindowedSynchronization::WaitForVerticalBlank { numeric_flags: 1 }
                },
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GetMessageResult {
    Message,
    Quit { exit_code: i32 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueObservation {
    Empty,
    Message { id: u32, result: GetMessageResult },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueuedMessageAction {
    None,
    Quit { exit_code: i32 },
    ConsumeWithoutDispatch,
    Dispatch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LoopContinuation {
    StopForExitRequest,
    StopForQuitMessage,
    EvaluateAfterQueuedPhase,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PostQueueAction {
    WaitThenDispatchIgnoringGetMessageResult,
    Tick,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LoopIteration {
    pub queued: QueuedMessageAction,
    pub continuation: LoopContinuation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowedSynchronization {
    WaitForVerticalBlank { numeric_flags: u32 },
    PollVgaBlankEdge { port: u16 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PresentationPlan {
    Fullscreen {
        sample_back_buffer_pixel: bool,
        flip_numeric_flags: u32,
    },
    Windowed {
        resolve_client_origin: bool,
        synchronization: WindowedSynchronization,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> WindowRuntimeState {
        WindowRuntimeState {
            active: true,
            activation_dispatch_enabled: true,
            force_wait_once: false,
            exit_requested: false,
            activation_latched: true,
            rebuild_pending: false,
            deactivation_guard: true,
            clear_black_on_paint: false,
            vertical_blank_fallback: false,
        }
    }

    fn message(id: u32, wparam: u32) -> WindowMessage {
        WindowMessage {
            id,
            wparam,
            lparam: 0,
        }
    }

    #[test]
    fn exact_recovered_messages_are_swallowed_but_neighbors_delegate() {
        let mut runtime = state();
        for id in [
            WM_CLOSE,
            WM_ERASEBKGND,
            WM_QUERYNEWPALETTE,
            WM_PALETTECHANGED,
        ] {
            assert_eq!(
                runtime
                    .handle_message(WindowMode::Windowed, message(id, 0))
                    .disposition,
                MessageDisposition::Handled
            );
        }
        for command in [SC_MOUSEMENU, SC_KEYMENU, SC_SCREENSAVE, SC_MONITORPOWER] {
            assert_eq!(
                runtime
                    .handle_message(WindowMode::Windowed, message(WM_SYSCOMMAND, command))
                    .disposition,
                MessageDisposition::Handled
            );
        }
        for command in [
            SC_MOUSEMENU - 1,
            SC_KEYMENU + 1,
            SC_SCREENSAVE + 1,
            SC_MONITORPOWER + 1,
        ] {
            assert_eq!(
                runtime
                    .handle_message(WindowMode::Windowed, message(WM_SYSCOMMAND, command))
                    .disposition,
                MessageDisposition::DefaultWindowProcedure
            );
        }
        assert!(!runtime.exit_requested, "WM_CLOSE does not initiate exit");
    }

    #[test]
    fn window_creation_preserves_both_observed_numeric_style_branches() {
        assert_eq!(
            window_creation_plan(WindowMode::Windowed, 1920, 1080),
            WindowCreationPlan {
                class_style: 0x3003,
                class_extra_bytes: 0,
                window_extra_bytes: 0,
                window_style: 0x80ca_0000,
                extended_style: 8,
                cursor: CursorSelection::Arrow,
                background_stock_object: 5,
                has_icon: false,
                has_menu: false,
                has_parent: false,
                class_name_is_title: true,
                x: 0,
                y: 0,
                show_command: 0,
                call_show_cursor_false_after_creation_attempt: true,
                initial_screen_width: 1920,
                initial_screen_height: 1080,
            }
        );
        assert_eq!(
            window_creation_plan(WindowMode::Fullscreen, 1280, 720),
            WindowCreationPlan {
                class_style: 3,
                class_extra_bytes: 0,
                window_extra_bytes: 0,
                window_style: 0x8008_0000,
                extended_style: 8,
                cursor: CursorSelection::None,
                background_stock_object: 5,
                has_icon: false,
                has_menu: false,
                has_parent: false,
                class_name_is_title: true,
                x: 0,
                y: 0,
                show_command: 3,
                call_show_cursor_false_after_creation_attempt: true,
                initial_screen_width: 1280,
                initial_screen_height: 720,
            }
        );
    }

    #[test]
    fn activation_restores_or_marks_rebuild_and_controls_ticking() {
        let mut runtime = state();
        assert_eq!(
            runtime.plan_loop_iteration(QueueObservation::Message {
                id: WM_ACTIVATEAPP,
                result: GetMessageResult::Message,
            }),
            LoopIteration {
                queued: QueuedMessageAction::Dispatch,
                continuation: LoopContinuation::EvaluateAfterQueuedPhase,
            }
        );
        let deactivated = runtime.handle_message(WindowMode::Windowed, message(WM_ACTIVATEAPP, 0));
        assert_eq!(deactivated.effects, []);
        assert!(!runtime.active);
        assert!(runtime.rebuild_pending);
        assert!(!runtime.activation_latched);
        assert_eq!(
            runtime.resolve_post_queue_action(),
            PostQueueAction::WaitThenDispatchIgnoringGetMessageResult
        );

        assert_eq!(
            runtime.plan_loop_iteration(QueueObservation::Message {
                id: WM_ACTIVATEAPP,
                result: GetMessageResult::Message,
            }),
            LoopIteration {
                queued: QueuedMessageAction::Dispatch,
                continuation: LoopContinuation::EvaluateAfterQueuedPhase,
            }
        );
        let activated = runtime.handle_message(WindowMode::Windowed, message(WM_ACTIVATEAPP, 2));
        assert_eq!(activated.effects, [WindowEffect::RestoreLostSurfaces]);
        assert!(runtime.active);
        assert!(runtime.activation_latched);
        assert_eq!(runtime.resolve_post_queue_action(), PostQueueAction::Tick);
    }

    #[test]
    fn deactivation_without_guard_preserves_latch_and_rebuild_state() {
        let mut runtime = state();
        runtime.deactivation_guard = false;
        runtime.rebuild_pending = false;
        let outcome = runtime.handle_message(WindowMode::Windowed, message(WM_ACTIVATEAPP, 0));

        assert_eq!(outcome.effects, []);
        assert!(!runtime.active);
        assert!(runtime.activation_latched);
        assert!(!runtime.rebuild_pending);
    }

    #[test]
    fn activation_suppression_and_get_message_quit_precede_dispatch() {
        let mut runtime = state();
        runtime.activation_dispatch_enabled = false;
        assert_eq!(
            runtime.plan_loop_iteration(QueueObservation::Message {
                id: WM_ACTIVATEAPP,
                result: GetMessageResult::Message,
            }),
            LoopIteration {
                queued: QueuedMessageAction::ConsumeWithoutDispatch,
                continuation: LoopContinuation::EvaluateAfterQueuedPhase,
            }
        );
        assert_eq!(runtime.resolve_post_queue_action(), PostQueueAction::Tick);
        assert_eq!(
            runtime.plan_loop_iteration(QueueObservation::Message {
                id: WM_ACTIVATEAPP,
                result: GetMessageResult::Quit { exit_code: 7 },
            }),
            LoopIteration {
                queued: QueuedMessageAction::Quit { exit_code: 7 },
                continuation: LoopContinuation::StopForQuitMessage,
            }
        );
        assert_eq!(
            runtime.plan_loop_iteration(QueueObservation::Message {
                id: WM_PAINT,
                result: GetMessageResult::Message,
            }),
            LoopIteration {
                queued: QueuedMessageAction::Dispatch,
                continuation: LoopContinuation::EvaluateAfterQueuedPhase,
            }
        );
        assert_eq!(runtime.resolve_post_queue_action(), PostQueueAction::Tick);
        runtime.active = false;
        assert_eq!(
            runtime.plan_loop_iteration(QueueObservation::Message {
                id: WM_PAINT,
                result: GetMessageResult::Message,
            }),
            LoopIteration {
                queued: QueuedMessageAction::Dispatch,
                continuation: LoopContinuation::EvaluateAfterQueuedPhase,
            }
        );
        assert_eq!(
            runtime.resolve_post_queue_action(),
            PostQueueAction::WaitThenDispatchIgnoringGetMessageResult
        );
    }

    #[test]
    fn forced_wait_is_one_shot_and_explicit_exit_wins() {
        let mut runtime = state();
        runtime.force_wait_once = true;
        assert_eq!(
            runtime.plan_loop_iteration(QueueObservation::Empty),
            LoopIteration {
                queued: QueuedMessageAction::None,
                continuation: LoopContinuation::EvaluateAfterQueuedPhase,
            }
        );
        assert!(runtime.force_wait_once);
        assert_eq!(
            runtime.resolve_post_queue_action(),
            PostQueueAction::WaitThenDispatchIgnoringGetMessageResult
        );
        assert!(!runtime.force_wait_once);
        assert_eq!(
            runtime.plan_loop_iteration(QueueObservation::Empty),
            LoopIteration {
                queued: QueuedMessageAction::None,
                continuation: LoopContinuation::EvaluateAfterQueuedPhase,
            }
        );
        assert_eq!(runtime.resolve_post_queue_action(), PostQueueAction::Tick);
        runtime.exit_requested = true;
        assert_eq!(
            runtime.plan_loop_iteration(QueueObservation::Message {
                id: WM_PAINT,
                result: GetMessageResult::Message,
            }),
            LoopIteration {
                queued: QueuedMessageAction::None,
                continuation: LoopContinuation::StopForExitRequest,
            }
        );
    }

    #[test]
    fn paint_and_cursor_effects_preserve_mode_specific_behavior() {
        let mut runtime = state();
        assert_eq!(
            runtime
                .handle_message(WindowMode::Windowed, message(WM_PAINT, 0))
                .effects,
            [WindowEffect::ValidatePaint { clear_black: false }]
        );
        runtime.clear_black_on_paint = true;
        assert_eq!(
            runtime
                .handle_message(WindowMode::Windowed, message(WM_PAINT, 0))
                .effects,
            [WindowEffect::ValidatePaint { clear_black: true }]
        );
        assert_eq!(
            runtime
                .handle_message(WindowMode::Windowed, message(WM_SETCURSOR, 0))
                .effects,
            []
        );
        assert_eq!(
            runtime
                .handle_message(WindowMode::Fullscreen, message(WM_SETCURSOR, 0))
                .effects,
            [WindowEffect::SetCursorNone]
        );
    }

    #[test]
    fn presentation_distinguishes_flip_vblank_and_latched_fallback() {
        let mut runtime = state();
        assert_eq!(
            runtime.presentation_plan(WindowMode::Fullscreen),
            PresentationPlan::Fullscreen {
                sample_back_buffer_pixel: true,
                flip_numeric_flags: 1,
            }
        );
        assert_eq!(
            runtime.presentation_plan(WindowMode::Windowed),
            PresentationPlan::Windowed {
                resolve_client_origin: true,
                synchronization: WindowedSynchronization::WaitForVerticalBlank { numeric_flags: 1 },
            }
        );
        runtime.record_vertical_blank_wait(false);
        assert_eq!(
            runtime.presentation_plan(WindowMode::Windowed),
            PresentationPlan::Windowed {
                resolve_client_origin: true,
                synchronization: WindowedSynchronization::PollVgaBlankEdge { port: 0x03da },
            }
        );
        runtime.record_vertical_blank_wait(true);
        assert!(runtime.vertical_blank_fallback, "failure latch remains set");
        assert!(matches!(
            runtime.presentation_plan(WindowMode::Fullscreen),
            PresentationPlan::Fullscreen { .. }
        ));
    }
}
