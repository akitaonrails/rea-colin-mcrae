//! The per-frame game driver (`FUN_00443e90`): the composition that ties the
//! host clock pacing ([`crate::host_loop`]) to the top-level FSM dispatch
//! ([`crate::game_state`]).
//!
//! The tick driver sits between the frame driver `FUN_004c1e00` and the dispatch
//! tick `FUN_00443db0`. Its recovered per-frame shape
//! (`rea.official-eu.game-fsm` section 1, `rea.official-eu.host-main-loop`):
//!
//! 1. **First-run init**: if the init-done flag (`DAT_005590b0`) is clear, seed
//!    the live state by calling boot state `1`'s update handler directly and
//!    taking its return value as the initial state (observed `2`/`3`), then set
//!    the init flag. (The native driver also registers the 25 Hz multimedia
//!    timer here; that registration is host glue.)
//! 2. **Dispatch**: for an in-race state (`4 < state < 0xb`, i.e. `5..=10`) run
//!    the self-limiting catch-up loop `do { one dispatch } while (the tick
//!    crossed a 40 ms bucket)` ([`crate::host_loop::run_catch_up_ticks`]); for
//!    any other state run exactly one dispatch (the menu/timer regime).
//! 3. **Render**: the previous state's render handler B runs, gated by the
//!    one-frame render-skip toggle ([`crate::host_loop::RenderGate`],
//!    `DAT_004ee900`).
//!
//! This reconstructs the driver's **decision logic** - when to seed, how many
//! FSM dispatches to run, and whether to render - over a caller-supplied
//! `handler` oracle that stands in for the native handler table (each dispatch's
//! next state is `handler(current)`; see [`crate::game_state::STATE_HANDLERS`]).
//! The native handler bodies, the `timeGetTime` spin, the timer registration,
//! and the actual render/present are left to the backend and the per-state
//! reconstruction items. See `rea.official-eu.game-fsm`.

use crate::game_state::{GameState, GameStateMachine};
use crate::host_loop::{HostClock, RenderGate, run_catch_up_ticks};

/// What one [`FrameDriver::run_frame`] decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FramePlan {
    /// The number of FSM dispatches run this frame (`1` in the menu regime, `>=1`
    /// under the in-race catch-up loop).
    pub ticks: u32,
    /// The live state after the frame's dispatches.
    pub state: GameState,
    /// The state whose render handler B should run this frame (the previous
    /// state), or `None` when the render-skip gate is disarmed.
    pub render: Option<GameState>,
}

/// The reconstructed tick driver `FUN_00443e90` over the top-level FSM.
///
/// It owns the [`GameStateMachine`], the init-done flag, and the render-skip
/// [`RenderGate`]. A frame is driven with [`run_frame`](Self::run_frame); after
/// the caller runs the planned render it reports the result with
/// [`report_render`](Self::report_render).
#[derive(Debug, Clone)]
pub struct FrameDriver {
    machine: GameStateMachine,
    gate: RenderGate,
    initialized: bool,
}

impl Default for FrameDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameDriver {
    /// A fresh, uninitialized driver (init-done flag clear): the first
    /// [`run_frame`](Self::run_frame) seeds the live state from the boot handler,
    /// reproducing the native first-run init.
    pub fn new() -> Self {
        FrameDriver {
            // Placeholder until the first frame seeds the real state; the boot
            // state is never itself dispatched.
            machine: GameStateMachine::boot(GameState::boot()),
            gate: RenderGate::new(),
            initialized: false,
        }
    }

    /// A driver resumed around an already-seeded machine (init-done flag set),
    /// for composition and focused tests that start in a chosen state.
    pub fn with_machine(machine: GameStateMachine) -> Self {
        FrameDriver {
            machine,
            gate: RenderGate::new(),
            initialized: true,
        }
    }

    /// Whether the first-run init has seeded the live state.
    pub const fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// The live FSM state.
    pub const fn current(&self) -> GameState {
        self.machine.current()
    }

    /// Borrow the underlying state machine (step counters, active flags).
    pub const fn machine(&self) -> &GameStateMachine {
        &self.machine
    }

    /// Run one frame of the tick driver. `clock` paces the in-race catch-up loop;
    /// `handler` returns the next state the current state's update handler would
    /// produce (the dispatch's only input).
    ///
    /// On the first call it seeds the live state from the boot handler
    /// (`handler(GameState::boot())`) and sets the init flag. It then runs the
    /// catch-up loop for an in-race state or a single dispatch otherwise, and
    /// reports the render decision from the current gate.
    pub fn run_frame<C, H>(&mut self, clock: &C, mut handler: H) -> FramePlan
    where
        C: HostClock,
        H: FnMut(GameState) -> GameState,
    {
        if !self.initialized {
            let seed = handler(GameState::boot());
            self.machine = GameStateMachine::boot(seed);
            self.initialized = true;
        }

        let ticks = if self.machine.current().is_in_race() {
            run_catch_up_ticks(clock, || {
                let next = handler(self.machine.current());
                self.machine.dispatch(next);
            })
        } else {
            let next = handler(self.machine.current());
            self.machine.dispatch(next);
            1
        };

        let render = if self.gate.should_render() {
            Some(self.machine.previous())
        } else {
            None
        };

        FramePlan {
            ticks,
            state: self.machine.current(),
            render,
        }
    }

    /// Report the planned render's outcome to the render-skip gate: `overran` is
    /// whether the render crossed a 40 ms budget (only meaningful when a render
    /// ran). A rendered-and-overran frame skips the next render; a skipped frame
    /// re-arms. A no-op when the last frame did not render.
    pub fn report_render(&mut self, overran: bool) {
        self.gate.after_pass(overran);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_state::GameState;
    use std::cell::Cell;

    fn state(id: u8) -> GameState {
        GameState::new(id).expect("valid state id")
    }

    /// A fake clock advanced only by the tick closure, matching the `host_loop`
    /// test harness.
    struct FakeClock {
        now: Cell<u32>,
    }
    impl FakeClock {
        fn at(start: u32) -> Self {
            FakeClock {
                now: Cell::new(start),
            }
        }
        fn advance(&self, ms: u32) {
            self.now.set(self.now.get() + ms);
        }
    }
    impl HostClock for FakeClock {
        fn millis(&self) -> u32 {
            self.now.get()
        }
    }

    #[test]
    fn first_frame_seeds_the_live_state_from_the_boot_handler() {
        let clock = FakeClock::at(0);
        let mut driver = FrameDriver::new();
        assert!(!driver.is_initialized());

        // The boot handler is asked for the seed with the boot state, returns 3.
        let seen = Cell::new(None);
        let plan = driver.run_frame(&clock, |cur| {
            if cur == GameState::boot() {
                seen.set(Some(cur));
                state(3) // seed
            } else {
                cur // state 3 self-loops this frame
            }
        });
        assert_eq!(seen.get(), Some(GameState::boot()));
        assert!(driver.is_initialized());
        assert_eq!(plan.state, state(3));
        assert_eq!(plan.ticks, 1);
    }

    #[test]
    fn a_menu_state_runs_exactly_one_dispatch_per_frame() {
        // A non-in-race state ignores the catch-up loop regardless of the clock.
        let clock = FakeClock::at(30);
        let mut driver = FrameDriver::with_machine(GameStateMachine::boot(state(2)));
        let plan = driver.run_frame(&clock, |cur| {
            clock.advance(100); // would straddle several buckets if it mattered
            cur
        });
        assert_eq!(plan.ticks, 1);
        assert_eq!(plan.state, state(2));
    }

    #[test]
    fn an_in_race_state_runs_the_catch_up_loop() {
        // Start in state 5 at t=30; each dispatch costs 15 ms: dispatch1 30->45
        // crosses bucket 0->1, dispatch2 45->60 settles in bucket 1 => 2 ticks.
        let clock = FakeClock::at(30);
        let mut driver = FrameDriver::with_machine(GameStateMachine::boot(state(5)));
        let plan = driver.run_frame(&clock, |cur| {
            clock.advance(15);
            cur // self-loop in 5
        });
        assert_eq!(plan.ticks, 2);
        assert_eq!(plan.state, state(5));
    }

    #[test]
    fn render_gate_skips_the_frame_after_an_overrun() {
        let clock = FakeClock::at(0);
        let mut driver = FrameDriver::with_machine(GameStateMachine::boot(state(2)));

        let plan = driver.run_frame(&clock, |cur| cur);
        assert_eq!(plan.render, Some(state(2)));
        driver.report_render(true); // the render overran

        let plan = driver.run_frame(&clock, |cur| cur);
        assert_eq!(plan.render, None); // this frame's render is skipped
        driver.report_render(false); // skipped frame re-arms

        let plan = driver.run_frame(&clock, |cur| cur);
        assert_eq!(plan.render, Some(state(2)));
    }

    #[test]
    fn drives_boot_through_the_recovered_graph_into_the_in_race_regime() {
        // A handler oracle that follows the first recovered edge out of each
        // state drives boot(1)->3 (seed), then 3->4->5; entering 5 flips the
        // driver into the catch-up regime.
        let clock = FakeClock::at(0);
        let mut driver = FrameDriver::new();

        // Seed: boot handler returns 3 (a recovered boot target), then 3 holds.
        let plan = driver.run_frame(&clock, |cur| {
            if cur == GameState::boot() {
                state(3)
            } else {
                cur
            }
        });
        assert_eq!(plan.state, state(3));

        // 3 -> 4 (a recovered edge); still the single-dispatch menu regime.
        let plan = driver.run_frame(&clock, |cur| {
            assert!(cur.is_known_transition(state(4)));
            state(4)
        });
        assert_eq!(plan.state, state(4));
        assert_eq!(plan.ticks, 1);

        // 4 -> 5 arms the race; state 4 is not yet in-race, so this frame is a
        // single dispatch, but the live state is now in the in-race band.
        let plan = driver.run_frame(&clock, |cur| {
            assert!(cur.is_known_transition(state(5)));
            state(5)
        });
        assert_eq!(plan.state, state(5));
        assert_eq!(plan.ticks, 1);
        assert!(driver.current().is_in_race());

        // The next frame, now in state 5, uses the catch-up loop.
        let plan = driver.run_frame(&clock, |cur| {
            clock.advance(15);
            cur
        });
        assert!(plan.ticks >= 1);
        assert!(driver.current().is_in_race());
    }
}
