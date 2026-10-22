//! The top-level game finite state machine (reconstruction item G01's
//! orchestration half).
//!
//! The shipped program runs one state machine for its whole lifetime: WinMain
//! `FUN_0049db00` -> frame driver `FUN_004c1e00` -> tick driver `FUN_00443e90`
//! -> the dispatch tick `FUN_00443db0`, over the current-state enum
//! `DAT_005590a8`. Each tick calls the current state's *update* handler, whose
//! **return value is the next state** - the enum is written in exactly one place
//! (`DAT_005590a8 = (*handlerA)()`), so every transition is a handler's return,
//! not a scattered assignment.
//!
//! The handler table is a single array of 13 sixteen-byte records based at
//! `0x4ee82c`, indexed by state id with stride `0x10` and **no bias**
//! (`rea.official-eu.game-fsm`, which refines the two-table reading in
//! `rea.official-eu.sim-fixed-step`):
//!
//! | record offset | field |
//! | --- | --- |
//! | `+0x0` (`0x4ee82c + s*0x10`) | active flag (0/1) |
//! | `+0x4` (`0x4ee830 + s*0x10`) | handler A - update tick, returns next state |
//! | `+0x8` (`0x4ee834 + s*0x10`) | handler B - render/present |
//! | `+0xc` (`0x4ee838 + s*0x10`) | id tag (`s + 1`; `0xffffffff` sentinel on the last row) |
//!
//! This module reconstructs the **backend-neutral dispatch contract** and the
//! recovered **transition graph**, not the native handler bodies (those are the
//! per-screen / per-race procedures, mostly separate reconstruction items). The
//! native handler addresses are retained as traceability evidence
//! ([`STATE_HANDLERS`]); the dispatch bookkeeping ([`GameStateMachine::dispatch`])
//! reproduces `FUN_00443db0`'s active-flag and step-counter arithmetic exactly.
//! The `timeGetTime` fixed-step spin, the master-enable gate (`DAT_0096efcc`),
//! and the render pump are host/runtime glue left to the platform host.
//!
//! State roles are reported only where the recovered calls prove them: states
//! `5..=9` run the in-race "Mechanics" update `FUN_0043afd0` (the fixed-step
//! band, [`crate::sim_clock::is_in_race_state`]), state `1` is the one-shot boot
//! handler the driver seeds from, and states `0`/`0xc` are terminal (their
//! handler is `xor eax,eax; ret`, returning `0`). The remaining roles are not
//! named here because the evidence does not pin them.

use crate::sim_clock::is_in_race_state;

/// The lowest valid state id.
pub const FIRST_STATE: u8 = 0;

/// The highest valid state id (`0xC`); its record is the `0xffffffff` sentinel
/// row, which reuses state `0`'s handlers.
pub const LAST_STATE: u8 = 0xC;

/// The one-shot bootstrap state. The tick driver's first-run init seeds the
/// machine by calling this state's update handler directly and taking its
/// return value as the initial live state (observed `2` or `3`); state `1` is
/// otherwise never dispatched.
pub const BOOT_STATE: u8 = 1;

/// The terminal "stopped" state: its handler returns `0`, so it self-loops.
pub const STOPPED_STATE: u8 = 0;

/// The quit funnel state: reached from the menu/results states, it transitions
/// to [`STOPPED_STATE`] (`0xc -> 0`).
pub const QUIT_FUNNEL_STATE: u8 = 0xC;

/// The recovered native handler addresses per state, `(update A, render B)`,
/// read from the 16-byte record table at `0x4ee830` (image base `0x400000`).
///
/// Evidence only - these are native procedures, not reconstructed here. State
/// `0xC` reuses the sentinel row and so shares state `0`'s handler pair, and
/// states `5` and `6` share the render handler `0x004231f0`.
pub const STATE_HANDLERS: [(u32, u32); 13] = [
    (0x0043_98a0, 0x0042_34a0), // 0   terminal (update = xor eax,eax; ret -> 0)
    (0x0043_6be0, 0x0042_1cc0), // 1   boot seed handler
    (0x0043_7160, 0x0042_1cd0), // 2   front-end / root-menu loop (owns quit path)
    (0x0043_71c0, 0x0042_1d80), // 3   pre-load / confirm gate
    (0x0043_7200, 0x0042_1da0), // 4   stage/race load + per-player init
    (0x0043_7ac0, 0x0042_31f0), // 5   in-race update (Mechanics)
    (0x0043_7690, 0x0042_31f0), // 6   in-race update (Mechanics; shares render B)
    (0x0043_7ef0, 0x0042_2c30), // 7   in-race update (Mechanics)
    (0x0043_8100, 0x0042_3260), // 8   in-race update (Mechanics)
    (0x0043_84a0, 0x0042_3380), // 9   in-race update (Mechanics)
    (0x0043_8970, 0x0042_33e0), // 0xa in-race overlay/hub (role unproven)
    (0x0043_8b90, 0x0042_3420), // 0xb post-race / results dispatch
    (0x0043_98a0, 0x0042_34a0), // 0xc quit funnel (sentinel row; shares state 0)
];

/// The recovered transition adjacency: for each state id, the set of next-state
/// ids its update handler is observed to return (`rea.official-eu.game-fsm`
/// section 3). A handler that returns its own id self-loops.
///
/// This is the observed graph; the shipped dispatch performs no validation (it
/// trusts the handler's return), so it is exposed for checking and is **not**
/// enforced by [`GameStateMachine::dispatch`].
const TRANSITIONS: [&[u8]; 13] = [
    &[0x0],                          // 0
    &[0x2, 0x3],                     // 1  boot -> 2 or 3
    &[0x2, 0x3, 0xC],                // 2
    &[0x3, 0x4],                     // 3
    &[0x5, 0x6, 0x7, 0x8],           // 4
    &[0x5, 0x8, 0x9, 0xA],           // 5
    &[0x6, 0x8, 0xB],                // 6
    &[0x5, 0x7, 0xA],                // 7
    &[0x8, 0xB],                     // 8
    &[0x9, 0xA, 0xB],                // 9
    &[0x5, 0x7, 0x8, 0x9, 0xA, 0xB], // 0xa
    &[0x2, 0x3, 0x5, 0xB, 0xC],      // 0xb
    &[0x0],                          // 0xc
];

/// A validated top-level FSM state id (`0..=0xC`).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct GameState(u8);

impl GameState {
    /// Wraps a raw state id, rejecting any id outside the recovered `0..=0xC`
    /// range (the shipped dispatch has no bounds check; this makes the invariant
    /// explicit for the port).
    pub const fn new(id: u8) -> Option<Self> {
        if id <= LAST_STATE {
            Some(Self(id))
        } else {
            None
        }
    }

    /// The raw state id.
    pub const fn id(self) -> u8 {
        self.0
    }

    /// The one-shot boot state ([`BOOT_STATE`]).
    pub const fn boot() -> Self {
        Self(BOOT_STATE)
    }

    /// The terminal stopped state ([`STOPPED_STATE`]).
    pub const fn stopped() -> Self {
        Self(STOPPED_STATE)
    }

    /// Whether this state runs under the fixed-step in-race loop (`5..=10`, the
    /// recovered `4 < state < 0xb` gate; see [`is_in_race_state`]).
    pub const fn is_in_race(self) -> bool {
        is_in_race_state(self.0 as u32)
    }

    /// Whether this is a terminal state (its handler returns `0` and self-loops
    /// at [`STOPPED_STATE`]).
    pub const fn is_terminal(self) -> bool {
        self.0 == STOPPED_STATE
    }

    /// Whether this is the quit funnel ([`QUIT_FUNNEL_STATE`], `0xc -> 0`).
    pub const fn is_quit_funnel(self) -> bool {
        self.0 == QUIT_FUNNEL_STATE
    }

    /// The cosmetic 1-based id tag stored at record `+0xc` (`id + 1`), with the
    /// last row carrying the `0xffffffff` sentinel.
    pub const fn id_tag(self) -> u32 {
        if self.0 == LAST_STATE {
            0xffff_ffff
        } else {
            self.0 as u32 + 1
        }
    }

    /// The recovered next-state ids this state's update handler is observed to
    /// return (see [`TRANSITIONS`]).
    pub fn known_next_states(self) -> &'static [u8] {
        TRANSITIONS[self.0 as usize]
    }

    /// Whether `next` is a recovered transition out of this state.
    pub fn is_known_transition(self, next: GameState) -> bool {
        TRANSITIONS[self.0 as usize].contains(&next.0)
    }
}

/// The outcome of one dispatch tick.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DispatchOutcome {
    /// The new current (live) state after the handler returned.
    pub state: GameState,
    /// Whether the state changed this tick (the handler returned a new id).
    pub changed: bool,
}

/// The reconstructed top-level dispatch machine, reproducing `FUN_00443db0`'s
/// per-tick bookkeeping over the current/previous state, the per-state active
/// flags, and the two step counters.
///
/// The master-enable gate (`DAT_0096efcc`) and the fixed-step spin are the
/// caller's concern; a tick is driven by calling [`dispatch`](Self::dispatch)
/// with the next state the current state's update handler returned.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GameStateMachine {
    current: GameState,
    previous: GameState,
    active: [bool; 13],
    render_counter: u32,
    pretick_counter: u32,
}

impl GameStateMachine {
    /// Seeds the machine the way the tick driver's first-run init does: the boot
    /// handler (state `1`'s update) runs once outside dispatch and its return
    /// value becomes the initial live state (`seed`, observed `2` or `3`).
    ///
    /// The active flags and step counters start cleared, and the previous state
    /// starts at [`STOPPED_STATE`] (`0`, the BSS zero-initialised value of
    /// `DAT_005590ac` before the first dispatch).
    pub fn boot(seed: GameState) -> Self {
        Self {
            current: seed,
            previous: GameState::stopped(),
            active: [false; 13],
            render_counter: 0,
            pretick_counter: 0,
        }
    }

    /// The current (live) state.
    pub const fn current(&self) -> GameState {
        self.current
    }

    /// The previous state - the one whose render handler B the driver runs this
    /// frame (its active flag is set).
    pub const fn previous(&self) -> GameState {
        self.previous
    }

    /// Whether a state's active flag (record `+0x0`) is set.
    pub fn is_active(&self, state: GameState) -> bool {
        self.active[state.0 as usize]
    }

    /// The render-pass step counter (`DAT_005590c0`), reset on every state
    /// change.
    pub const fn render_counter(&self) -> u32 {
        self.render_counter
    }

    /// The pre-gameplay tick counter (`DAT_005590c4`), reset on every state
    /// change and incremented each tick the new state is below the in-race band
    /// (`state < 5`).
    pub const fn pretick_counter(&self) -> u32 {
        self.pretick_counter
    }

    /// Reproduces one `FUN_00443db0` dispatch step. `handler_result` is the next
    /// state the current state's update handler A returned - the only writer of
    /// the state enum.
    ///
    /// The step clears the previous state's active flag, promotes the current
    /// state to previous and sets its active flag, adopts `handler_result` as
    /// the new current, resets both step counters on a state change, and
    /// increments the pre-gameplay counter when the new state is below the
    /// in-race band. It performs no bounds or transition check - exactly as the
    /// shipped dispatch trusts the handler's return (the `0..=0xC` invariant is
    /// held by [`GameState`]).
    pub fn dispatch(&mut self, handler_result: GameState) -> DispatchOutcome {
        self.active[self.previous.0 as usize] = false;
        self.previous = self.current;
        self.active[self.current.0 as usize] = true;

        self.current = handler_result;
        let changed = self.current != self.previous;
        if changed {
            self.render_counter = 0;
            self.pretick_counter = 0;
        }
        if (self.current.0 as u32) < crate::sim_clock::FIRST_IN_RACE_STATE {
            self.pretick_counter += 1;
        }
        DispatchOutcome {
            state: self.current,
            changed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(id: u8) -> GameState {
        GameState::new(id).expect("valid state id")
    }

    #[test]
    fn state_ids_are_bounded_to_the_recovered_range() {
        for id in 0..=0xC {
            assert_eq!(GameState::new(id).map(GameState::id), Some(id));
        }
        assert_eq!(GameState::new(0xD), None);
        assert_eq!(GameState::new(0xFF), None);
    }

    #[test]
    fn id_tag_is_one_based_with_the_sentinel_on_the_last_row() {
        for id in 0..=0xB {
            assert_eq!(state(id).id_tag(), id as u32 + 1);
        }
        assert_eq!(state(0xC).id_tag(), 0xffff_ffff);
    }

    #[test]
    fn handler_table_shares_the_recovered_rows() {
        assert_eq!(STATE_HANDLERS.len(), 13);
        // State 0xc reuses the sentinel row and so shares state 0's handlers.
        assert_eq!(STATE_HANDLERS[0], STATE_HANDLERS[0xC]);
        assert_eq!(STATE_HANDLERS[0], (0x0043_98a0, 0x0042_34a0));
        // States 5 and 6 share the render handler B.
        assert_eq!(STATE_HANDLERS[5].1, STATE_HANDLERS[6].1);
        assert_eq!(STATE_HANDLERS[5].1, 0x0042_31f0);
        // Every update handler A is distinct except the shared terminal pair.
        assert_eq!(STATE_HANDLERS[5].0, 0x0043_7ac0);
    }

    #[test]
    fn in_race_band_matches_sim_clock() {
        for id in 0..5 {
            assert!(!state(id).is_in_race(), "state {id} is untimed");
        }
        for id in 5..=0xA {
            assert!(state(id).is_in_race(), "state {id} is in-race");
        }
        assert!(!state(0xB).is_in_race());
        assert!(!state(0xC).is_in_race());
    }

    #[test]
    fn terminal_and_quit_states_are_classified() {
        assert!(state(0).is_terminal());
        assert!(!state(0).is_quit_funnel());
        assert!(state(0xC).is_quit_funnel());
        assert!(!state(0xC).is_terminal());
        assert!(!state(5).is_terminal());
    }

    #[test]
    fn transitions_cover_the_recovered_graph() {
        // Boot seeds the live state as 2 or 3.
        assert_eq!(state(BOOT_STATE).known_next_states(), &[0x2, 0x3]);
        // Terminal funnel: 0xc -> 0 -> 0.
        assert_eq!(state(0xC).known_next_states(), &[0x0]);
        assert_eq!(state(0).known_next_states(), &[0x0]);
        // State 4 arms the race into one of the in-race entry states.
        assert!(state(4).is_known_transition(state(8)));
        assert!(!state(4).is_known_transition(state(9)));
        // Results/menu dispatch can quit or restart.
        assert!(state(0xB).is_known_transition(state(0xC)));
        assert!(state(0xB).is_known_transition(state(5)));
        assert!(!state(0xB).is_known_transition(state(4)));
        // Every recovered edge targets a valid state id.
        for from in 0..=0xC {
            for &to in state(from).known_next_states() {
                assert!(GameState::new(to).is_some(), "edge {from}->{to} invalid");
            }
        }
    }

    #[test]
    fn dispatch_tracks_previous_and_active_flags() {
        let mut machine = GameStateMachine::boot(state(3));
        assert_eq!(machine.current(), state(3));
        assert_eq!(machine.previous(), GameState::stopped());

        // State 3 stays in 3 (no change): previous becomes 3, 3 active.
        let outcome = machine.dispatch(state(3));
        assert_eq!(
            outcome,
            DispatchOutcome {
                state: state(3),
                changed: false
            }
        );
        assert_eq!(machine.previous(), state(3));
        assert!(machine.is_active(state(3)));
        assert!(!machine.is_active(GameState::stopped()));

        // 3 -> 4 (change): counters reset; 4 < 5 so the pre-tick counter ticks.
        let outcome = machine.dispatch(state(4));
        assert_eq!(
            outcome,
            DispatchOutcome {
                state: state(4),
                changed: true
            }
        );
        assert_eq!(machine.previous(), state(3));
        assert_eq!(machine.pretick_counter(), 1);
        assert_eq!(machine.render_counter(), 0);
        assert!(machine.is_active(state(3)));
    }

    #[test]
    fn dispatch_counter_rules_match_the_recovered_arithmetic() {
        let mut machine = GameStateMachine::boot(state(2));

        // Self-loop in a pre-gameplay state (< 5): no reset, pre-tick increments.
        machine.dispatch(state(2));
        machine.dispatch(state(2));
        assert_eq!(machine.pretick_counter(), 2);

        // Entering the in-race band (>= 5) resets counters and does NOT tick the
        // pre-gameplay counter.
        let outcome = machine.dispatch(state(5));
        assert!(outcome.changed);
        assert_eq!(machine.pretick_counter(), 0);
        assert_eq!(machine.render_counter(), 0);

        // An in-race self-loop keeps the pre-tick counter at zero (state >= 5).
        machine.dispatch(state(5));
        assert_eq!(machine.pretick_counter(), 0);
    }
}
