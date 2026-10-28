//! Reconstruction of the in-race pause request.
//!
//! Two sources raise it, and the state-5/7/9 handlers probe it with
//! `FUN_00468200` right after the control map:
//!
//! - **Escape**, read by the control map `FUN_00449580` directly through
//!   `GetAsyncKeyState(VK_ESCAPE)` (`0x449714`), not through the action-bit
//!   bindings. The press is edge-armed by the single global `DAT_004ef5c0`:
//!   a release re-arms it, an armed press calls `FUN_004681d0(player)`
//!   (`DAT_0057e66c = 1`, `DAT_0057e690 = 1`, `DAT_0057e68c = player`) and
//!   disarms it. Every poll with the key down first clears the pause-menu
//!   words (`FUN_00468da0`). The mapper's prologue (`0x4495b3`) disarms the
//!   flag while any car's out-of-bounds countdown `car +0x75c` runs.
//! - **Focus loss**: the window procedure sets `DAT_00b63748 = 1` when the
//!   application is deactivated while `DAT_00514498` is set (`0x49dd5f`).
//!
//! `FUN_00468200` returns 1 (enter state `0xa`) when either word is set,
//! after stopping the race sounds and building the pause menu, and clears
//! `DAT_0057e66c`. This is a description of disassembly, not original source.
//! Evidence: `rea.official-eu.pause-request`.

/// The gates the control map checks before it reads Escape (`0x4496f5`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PauseKeyGates {
    /// `FUN_00443da0(player)`: the player has finished the stage.
    pub player_finished: bool,
    /// `player == 0 || FUN_00401030()`.
    pub player_active: bool,
    /// `DAT_0055ae9c != 0` blocks the read.
    pub blocked_55ae9c: bool,
    /// `DAT_00d3ca88 == 0`.
    pub real_play: bool,
    /// `DAT_00514478 != 0 && FUN_00469270() != 0`: in real play, Escape sets
    /// `DAT_0057e6c0` instead of pausing.
    pub alternate_6c0: bool,
}

/// What one Escape poll did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PauseKeyPoll {
    /// `FUN_00468da0` ran (the key was down).
    pub cleared_menu_words: bool,
    /// `FUN_004681d0(player)` ran.
    pub requested: bool,
    /// `DAT_0057e6c0 = 1` ran.
    pub set_6c0: bool,
}

/// The pause request words.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PauseRequest {
    /// `DAT_004ef5c0`: Escape is armed (zero in BSS).
    pub escape_armed_5c0: bool,
    /// `DAT_0057e66c`.
    pub requested_66c: bool,
    /// `DAT_0057e68c`: the requesting player.
    pub player_68c: u8,
    /// `DAT_00b63748`: the focus-loss request.
    pub focus_lost_748: bool,
}

impl PauseRequest {
    /// The mapper prologue: disarm while any car's `+0x75c` countdown runs.
    pub fn disarm_during_countdown(&mut self, any_countdown_75c: bool) {
        if any_countdown_75c {
            self.escape_armed_5c0 = false;
        }
    }

    /// The Escape block of `FUN_00449580` for one player.
    pub fn poll_escape(
        &mut self,
        escape_down: bool,
        gates: PauseKeyGates,
        player: u8,
    ) -> PauseKeyPoll {
        let mut poll = PauseKeyPoll::default();
        if gates.player_finished || !gates.player_active || gates.blocked_55ae9c {
            return poll;
        }
        if !escape_down {
            self.escape_armed_5c0 = true;
            return poll;
        }
        poll.cleared_menu_words = true;
        self.requested_66c = false;
        if gates.real_play && gates.alternate_6c0 {
            poll.set_6c0 = true;
        } else if self.escape_armed_5c0 {
            self.requested_66c = true;
            self.player_68c = player;
            self.escape_armed_5c0 = false;
            poll.requested = true;
        }
        poll
    }

    /// The window procedure's deactivation branch.
    pub fn focus_lost(&mut self, gate_514498: bool) {
        if gate_514498 {
            self.focus_lost_748 = true;
        }
    }

    /// `FUN_00468200`'s gate: enter the pause state and clear `DAT_0057e66c`.
    /// The focus word stays set until the pause handler clears it.
    pub fn probe(&mut self) -> bool {
        if !self.requested_66c && !self.focus_lost_748 {
            return false;
        }
        self.requested_66c = false;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPEN: PauseKeyGates = PauseKeyGates {
        player_finished: false,
        player_active: true,
        blocked_55ae9c: false,
        real_play: true,
        alternate_6c0: false,
    };

    #[test]
    fn escape_must_be_released_before_it_pauses() {
        let mut pause = PauseRequest::default();
        // BSS starts disarmed: holding Escape from the start never pauses.
        let held = pause.poll_escape(true, OPEN, 0);
        assert!(held.cleared_menu_words && !held.requested);
        pause.poll_escape(false, OPEN, 0);
        assert!(pause.escape_armed_5c0);
        let press = pause.poll_escape(true, OPEN, 0);
        assert!(press.requested);
        assert!(pause.requested_66c && !pause.escape_armed_5c0);
        // Still held: no second request.
        pause.requested_66c = false;
        assert!(!pause.poll_escape(true, OPEN, 0).requested);
    }

    #[test]
    fn probe_enters_the_pause_state_once_per_request() {
        let mut pause = PauseRequest::default();
        assert!(!pause.probe());
        pause.poll_escape(false, OPEN, 0);
        pause.poll_escape(true, OPEN, 1);
        assert_eq!(pause.player_68c, 1);
        assert!(pause.probe());
        assert!(!pause.probe());
        pause.focus_lost(false);
        assert!(!pause.probe());
        pause.focus_lost(true);
        assert!(pause.probe());
        assert!(
            pause.probe(),
            "the focus word is cleared by the pause handler"
        );
    }

    #[test]
    fn gates_block_the_read_entirely() {
        for gates in [
            PauseKeyGates {
                player_finished: true,
                ..OPEN
            },
            PauseKeyGates {
                player_active: false,
                ..OPEN
            },
            PauseKeyGates {
                blocked_55ae9c: true,
                ..OPEN
            },
        ] {
            let mut pause = PauseRequest::default();
            assert_eq!(pause.poll_escape(false, gates, 0), PauseKeyPoll::default());
            assert!(!pause.escape_armed_5c0);
        }
    }

    #[test]
    fn countdown_disarms_and_the_alternate_branch_does_not_pause() {
        let mut pause = PauseRequest::default();
        pause.poll_escape(false, OPEN, 0);
        pause.disarm_during_countdown(true);
        assert!(!pause.poll_escape(true, OPEN, 0).requested);
        pause.poll_escape(false, OPEN, 0);
        let alt = PauseKeyGates {
            alternate_6c0: true,
            ..OPEN
        };
        let poll = pause.poll_escape(true, alt, 0);
        assert!(poll.set_6c0 && !poll.requested && pause.escape_armed_5c0);
        // Outside real play the alternate gate is ignored.
        let poll = pause.poll_escape(
            true,
            PauseKeyGates {
                real_play: false,
                ..alt
            },
            0,
        );
        assert!(poll.requested);
    }
}
