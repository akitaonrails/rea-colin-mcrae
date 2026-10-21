//! Reconstruction of the native host clock and main event loop (B09): the
//! portable, backend-independent frame-pacing and loop-gating logic recovered
//! from the shipped main loop.
//!
//! Recovered shape (`FUN_0049db00` WinMain -> `FUN_004c1e00` frame driver ->
//! `FUN_00443e90` tick driver, with the `FUN_00443e30`/`FUN_00443e60` pacing
//! primitives): the sim is **lock-step 25 Hz** (40 ms `timeGetTime` buckets)
//! with an **uncapped but self-limiting catch-up** - the in-stage driver runs
//! `do { one tick } while (the tick crossed a 40 ms bucket)`, so on fast hardware
//! exactly one tick runs and the driver then busy-waits to the next 40 ms
//! boundary, while an overrunning tick pulls the sim forward until a tick fits
//! inside one bucket. Render is **decoupled** (reached through the per-state
//! frame proc, once per pass) and gated by a **one-frame render-skip**: a render
//! pass that overruns 40 ms is skipped on the next pass (`DAT_004ee900` toggling
//! 2<->1). The host loop itself runs the frame driver only while the window is
//! active (`DAT_0096efcc`), blocking on the message queue when inactive, and
//! exits on the quit flag (`DAT_0096efe8`); focus is driven by `WM_ACTIVATEAPP`
//! writing the active flag.
//!
//! This module reconstructs that logic over a [`HostClock`] abstraction so it is
//! unit-testable and later backed by SDL (B11/B12). The actual busy-wait spin
//! and the Win32 message pump are host glue left to the backend; what is
//! reconstructed here is the *decision* logic: the loop gating, the catch-up
//! tick count, and the render-skip toggle.
//!
//! See evidence `rea.official-eu.host-main-loop`.
//!
//! Observations: the 40 ms bucketing, the `do{tick}while(crossed)` catch-up with
//! no cap, the render-skip toggle, the active/quit/one-shot loop flags, and the
//! `WM_ACTIVATEAPP`->active-flag pause hinge. Inferences: the loop gating state
//! machine below. Unknowns (not reproduced): the regime-A menu timer period
//! (25/50 ms vs the 40 ms stage bucket - recorded as a constant, its interaction
//! not resolved) and the suspend/resume subsystem behind focus changes.

use crate::sim_clock::step_boundary_crossed;

/// A monotonic millisecond clock, the `timeGetTime` abstraction the host loop
/// paces against. Backends implement it; tests use a fake.
pub trait HostClock {
    /// The current time in milliseconds.
    fn millis(&self) -> u32;
}

/// The regime-A (menu / non-stage) periodic timer period in milliseconds,
/// reproducing `((double_rate)+1) * 0x19` (`FUN_004a7790` registration): 25 ms,
/// or 50 ms when the double-rate flag is set.
pub fn menu_timer_period_ms(double_rate: bool) -> u32 {
    (if double_rate { 2 } else { 1 }) * 0x19
}

/// What the host loop should do this iteration, reproducing the gating in
/// `FUN_0049db00`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostAction {
    /// The quit flag is set: leave the loop.
    Quit,
    /// Not active (or a one-shot wait is pending): block on the message queue.
    BlockingWait,
    /// Active and running: call the frame driver for one pass.
    RunFrame,
}

/// The host-loop control flags (`DAT_0096efe8` quit, `DAT_0096efcc` active,
/// `DAT_0096efe4` one-shot wait).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HostLoopState {
    quit: bool,
    active: bool,
    force_wait: bool,
}

impl HostLoopState {
    /// A loop that has not yet been activated (starts parked until the first
    /// `WM_ACTIVATEAPP`).
    pub fn new() -> Self {
        HostLoopState::default()
    }

    /// Request the loop to exit (the `WM_QUIT` / window-creation-failure path).
    pub fn request_quit(&mut self) {
        self.quit = true;
    }

    /// Apply a `WM_ACTIVATEAPP`: the active flag follows the activation state
    /// (`DAT_0096efcc = wParam`). Deactivation parks the loop on the next action.
    pub fn on_activate_app(&mut self, active: bool) {
        self.active = active;
    }

    /// Arm the one-shot "block for one message" (the `DAT_0096efe4` one-shot).
    pub fn force_single_wait(&mut self) {
        self.force_wait = true;
    }

    /// Whether the loop is currently active.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Decide and consume this iteration's action, reproducing the loop body:
    /// quit wins; otherwise an inactive window or a pending one-shot blocks
    /// (consuming the one-shot); otherwise run a frame.
    pub fn next_action(&mut self) -> HostAction {
        if self.quit {
            return HostAction::Quit;
        }
        if !self.active || self.force_wait {
            self.force_wait = false;
            return HostAction::BlockingWait;
        }
        HostAction::RunFrame
    }
}

/// Run the self-limiting catch-up tick loop, reproducing the in-stage regime of
/// `FUN_00443e90`: `do { one tick } while (the tick crossed a 40 ms bucket)`.
/// Each `tick` runs one fixed sim step; the clock is sampled before and after to
/// decide whether to run another. Returns the number of ticks run (>= 1).
///
/// On fast hardware a tick fits inside one 40 ms bucket and exactly one tick
/// runs (lock-step). There is no iteration cap - termination is by a tick
/// fitting within a bucket, exactly as shipped.
pub fn run_catch_up_ticks<C: HostClock>(clock: &C, mut tick: impl FnMut()) -> u32 {
    let mut count = 0u32;
    loop {
        let before = clock.millis();
        tick();
        let after = clock.millis();
        count += 1;
        if !step_boundary_crossed(before, after) {
            return count;
        }
    }
}

/// The one-frame render-skip gate, reproducing `DAT_004ee900` (2 = render armed,
/// 1 = skip this pass): a render pass that overruns a 40 ms budget is skipped on
/// the following pass; a skipped pass re-arms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderGate {
    armed: bool,
}

impl Default for RenderGate {
    fn default() -> Self {
        RenderGate::new()
    }
}

impl RenderGate {
    /// A gate armed to render (the initial `DAT_004ee900 == 2`).
    pub const fn new() -> Self {
        RenderGate { armed: true }
    }

    /// Whether this pass should render.
    pub fn should_render(&self) -> bool {
        self.armed
    }

    /// Record the result of a pass. `overran` is whether the render crossed a
    /// 40 ms boundary (only meaningful when it rendered). A rendered-and-overran
    /// pass disarms the next one; a rendered-in-budget pass stays armed; a
    /// skipped pass re-arms.
    pub fn after_pass(&mut self, overran: bool) {
        self.armed = if self.armed { !overran } else { true };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// A fake clock whose time advances only when the tick closure says so.
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
    fn menu_timer_period_is_25_or_50() {
        assert_eq!(menu_timer_period_ms(false), 25);
        assert_eq!(menu_timer_period_ms(true), 50);
    }

    #[test]
    fn loop_gating_matches_the_recovered_flags() {
        let mut state = HostLoopState::new();
        // Not yet activated: parks on the message queue.
        assert_eq!(state.next_action(), HostAction::BlockingWait);
        // Activated: runs frames.
        state.on_activate_app(true);
        assert_eq!(state.next_action(), HostAction::RunFrame);
        // A one-shot wait blocks exactly once, then resumes.
        state.force_single_wait();
        assert_eq!(state.next_action(), HostAction::BlockingWait);
        assert_eq!(state.next_action(), HostAction::RunFrame);
        // Deactivation (focus loss) parks the loop.
        state.on_activate_app(false);
        assert_eq!(state.next_action(), HostAction::BlockingWait);
        // Quit wins over everything.
        state.on_activate_app(true);
        state.request_quit();
        assert_eq!(state.next_action(), HostAction::Quit);
    }

    #[test]
    fn catch_up_runs_one_tick_when_ticks_fit_in_a_bucket() {
        // Each tick takes 15 ms, starting at t=0: the first tick stays inside
        // bucket 0, so exactly one tick runs (lock-step).
        let clock = FakeClock::at(0);
        let ticks = run_catch_up_ticks(&clock, || clock.advance(15));
        assert_eq!(ticks, 1);
    }

    #[test]
    fn catch_up_runs_extra_ticks_while_a_tick_straddles_a_boundary() {
        // Start at t=30, each tick 15 ms: tick1 30->45 crosses (bucket 0->1),
        // tick2 45->60 stays in bucket 1, so two ticks run then it settles.
        let clock = FakeClock::at(30);
        let ticks = run_catch_up_ticks(&clock, || clock.advance(15));
        assert_eq!(ticks, 2);
    }

    #[test]
    fn render_gate_skips_one_frame_after_an_overrun() {
        let mut gate = RenderGate::new();
        // Armed by default.
        assert!(gate.should_render());
        // A render that stays in budget keeps it armed.
        gate.after_pass(false);
        assert!(gate.should_render());
        // A render that overruns disarms the next pass.
        gate.after_pass(true);
        assert!(!gate.should_render());
        // The skipped pass re-arms.
        gate.after_pass(false);
        assert!(gate.should_render());
    }
}
