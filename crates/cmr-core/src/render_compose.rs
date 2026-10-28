//! Per-frame composition plans for the two render-column (B) handlers whose
//! bodies [`crate::render_route`] classifies as [`RenderRoute::OverlayAlt`] (state
//! `0xa`, `FUN_004233e0`) and [`RenderRoute::FadeComposite`] (state `0xb`,
//! `FUN_00423420`). `render_route` recovered *which* route each state takes; this
//! module reconstructs *what each handler composes per frame*, as backend-neutral
//! decision logic (`rea.official-eu.game-fsm-overlay-fade`, asm-confirmed).
//!
//! Both handlers reach the shared per-viewport scene compositor `0x422ca0` (also
//! used by the `StageWorld` states `5..=9`); its full body and the exact meaning
//! of its three `u8` arguments are an open item, so the plans carry the observed
//! argument triples verbatim ([`SharedComposite`]) rather than inventing their
//! semantics. Present/flip stays an A-side step ([`crate::render_route::PRESENT_NOTE`]),
//! except that the fade handler, at fade completion, itself shuts the display
//! down (`0x4992e0`) and flips the present-enable pair - the one B-side path
//! that reaches the device layer (the hub's exit from the race display).
//!
//! # Observed handler bodies
//!
//! **State `0xa` (`FUN_004233e0`)**: if the secondary render sub-mode flag
//! `0xd3ca94` is non-zero, run the gated alt-transform group (`0x43af20` plus its
//! pre/post helpers); then *always* run the scrolling 2D overlay `0x468dd0` and
//! the shared composite `0x422ca0(1,1,0)`. No world-camera transform and no
//! object-interpolation loop run in this body.
//!
//! **State `0xb` (`FUN_00423420`)**: query the fade object (`0x48dfd0`, which
//! inverts the transition-counter test `0x4ad560(&DAT_0082b37c)`). If the fade is
//! **idle/finished** and the device is ready (`DAT_00502518->+0x428 != 0 &&
//! ->+0x10 != 0`), shut the display down (`0x4992e0`) and write
//! `0xb63748 <- 0` / `0xb6374c <- 1`. If the fade is **in progress**, composite
//! the live world through `0x422ca0(1,0,0)` so the fade blends fresh content -
//! unless the scene sub-mode predicate (`0x401030`) and the split flags
//! (`0x551ad8 == 1`, `0x514464 != 0`) all hold, which suppresses the composite.

/// The shared per-viewport compositor `0x422ca0` and the argument triple a handler
/// passes it. State `0xa` passes `[1, 1, 0]`; state `0xb`'s in-progress path passes
/// `[1, 0, 0]`. The three arguments' roles are an open item, so they are carried
/// verbatim rather than interpreted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SharedComposite {
    /// The observed argument triple (`0x422ca0(a, b, c)`).
    pub args: [u8; 3],
}

impl SharedComposite {
    /// The state `0xa` composite arguments (`0x422ca0(1, 1, 0)`).
    pub const OVERLAY_ALT: Self = Self { args: [1, 1, 0] };
    /// The state `0xb` in-progress composite arguments (`0x422ca0(1, 0, 0)`).
    pub const FADE_WORLD: Self = Self { args: [1, 0, 0] };
}

/// Per-frame increment of the primary overlay scroll accumulator (`[0x57e684]`),
/// advanced by the scrolling 2D overlay `0x468dd0`.
pub const OVERLAY_SCROLL_PRIMARY_STEP: u32 = 0x1999;

/// Per-frame increment of the secondary overlay scroll accumulator (`[0x4f3e34]`).
pub const OVERLAY_SCROLL_SECONDARY_STEP: u32 = 0x51e;

/// The two frame-scroll accumulators the scrolling overlay (`0x468dd0`) advances
/// each frame. The accumulators are read as Q16.16 fixed point downstream (the
/// handler derives UVs via `imul ...; sar 0x10`), so they wrap at the `u32`
/// boundary exactly as the shipped `add`+store does.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OverlayScroll {
    /// The primary accumulator (`[0x57e684]`).
    pub primary: u32,
    /// The secondary accumulator (`[0x4f3e34]`).
    pub secondary: u32,
}

impl OverlayScroll {
    /// Advance both accumulators one frame, wrapping on overflow (the shipped code
    /// stores the raw 32-bit sum back with no clamp).
    #[must_use]
    pub fn advance(self) -> Self {
        Self {
            primary: self.primary.wrapping_add(OVERLAY_SCROLL_PRIMARY_STEP),
            secondary: self.secondary.wrapping_add(OVERLAY_SCROLL_SECONDARY_STEP),
        }
    }
}

/// What the state `0xa` handler composes this frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OverlayAltPlan {
    /// Whether the gated alt-transform group (`0x43af20` + its pre/post helpers)
    /// runs this pass: true exactly when the secondary render sub-mode flag
    /// `0xd3ca94` is non-zero.
    pub alt_transform: bool,
    /// The scrolling 2D overlay (`0x468dd0`) - always drawn in this handler.
    pub scrolling_overlay: bool,
    /// The shared viewport composite (`0x422ca0(1, 1, 0)`) - always run.
    pub composite: SharedComposite,
}

/// The state `0xa` (`OverlayAlt`) per-frame plan: the alt-transform group runs
/// only when the secondary sub-mode flag `0xd3ca94` is set; the scrolling overlay
/// and the shared composite always run.
#[must_use]
pub fn overlay_alt_plan(sub_mode_active: bool) -> OverlayAltPlan {
    OverlayAltPlan {
        alt_transform: sub_mode_active,
        scrolling_overlay: true,
        composite: SharedComposite::OVERLAY_ALT,
    }
}

/// The margin the fade transition test (`0x4ad560`) adds to the target counter: a
/// fade counts as still running until `current` reaches `target + 3`.
pub const FADE_COUNTER_MARGIN: i32 = 3;

/// The full-brightness fade level (Q16): the fade ramps between `0` (black) and
/// this value. The fade wrappers `FUN_0048df80`/`FUN_0048dfa0` program the tween
/// with `start`/`end` of `0` and `0x10000`.
pub const FADE_FULL_LEVEL: i32 = 0x1_0000;

// The fade channel itself (its triggers, level and the compositor's black
// quad `0xff - ((level * 255) >> 16)`) is `crate::screen_fade`.

/// The address the fade handler clears (`<- 0`) when it presents at fade
/// completion (present-enable flag A, `0xb63748`).
pub const PRESENT_GATE_CLEAR_ADDR: u32 = 0x00b6_3748;

/// The address the fade handler sets (`<- 1`) when it presents at fade completion
/// (present-enable flag B, `0xb6374c`).
pub const PRESENT_GATE_SET_ADDR: u32 = 0x00b6_374c;

/// The transition-counter record the fade test (`0x4ad560`, table `0x5046d8`,
/// stride `0x2c`) reads for a fade handle. This models the recovered counter
/// predicate only; locating the record by handle is a runtime table lookup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FadeTransition {
    /// The record's active flag (`[rec+0x14]`).
    pub active: bool,
    /// The current transition counter (`[rec+0x28]`).
    pub current: i32,
    /// The target the counter ramps toward (`[rec+0x04]`).
    pub target: i32,
}

impl FadeTransition {
    /// Whether a fade is in progress: the record is active and its current counter
    /// has not yet reached `target + FADE_COUNTER_MARGIN`. `0x4ad560` returns this;
    /// `0x48dfd0` inverts it into the "fade idle" test the handler branches on.
    #[must_use]
    pub fn in_progress(self) -> bool {
        self.active && self.current < self.target + FADE_COUNTER_MARGIN
    }
}

/// What the state `0xb` handler does this frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FadeCompositeStep {
    /// Fade in progress and the scene gates permit: composite the live world
    /// (`0x422ca0(1, 0, 0)`) so the fade blends fresh content.
    CompositeWorld(SharedComposite),
    /// Fade in progress but the composite is suppressed this pass (the scene
    /// sub-mode predicate `0x401030` and both split flags hold).
    HoldDuringFade,
    /// Fade idle/finished and the device is ready: shut the display down
    /// (`0x4992e0`) and flip the present-enable pair (`0xb63748 <- 0`,
    /// `0xb6374c <- 1`).
    PresentAndFlip,
    /// Fade idle but the device is not ready to present this pass (one of the
    /// `DAT_00502518` gates is clear).
    IdleNotReady,
}

/// The state `0xb` (`FadeComposite`) per-frame decision.
///
/// `device_ready` is the path-A gate `DAT_00502518->+0x428 != 0 && ->+0x10 != 0`.
/// The path-B composite runs unless all three suppress gates hold:
/// `scene_submode_predicate` (`0x401030`: `([0xd3ca80]+0x166)&0x1c == 8 &&
/// [0xd3ca88] == 0`), `split_mode_one` (`[0x551ad8] == 1`), and
/// `world_present_ready` (`[0x514464] != 0`).
#[must_use]
pub fn fade_composite_plan(
    fade: FadeTransition,
    device_ready: bool,
    scene_submode_predicate: bool,
    split_mode_one: bool,
    world_present_ready: bool,
) -> FadeCompositeStep {
    if fade.in_progress() {
        // Path B: the shipped `if (r == 0 || flag != 1 || ptr == 0) composite;`,
        // so the composite is suppressed only when none of those escape hatches
        // fire - i.e. all three gates hold.
        let suppress = scene_submode_predicate && split_mode_one && world_present_ready;
        if suppress {
            FadeCompositeStep::HoldDuringFade
        } else {
            FadeCompositeStep::CompositeWorld(SharedComposite::FADE_WORLD)
        }
    } else if device_ready {
        FadeCompositeStep::PresentAndFlip
    } else {
        FadeCompositeStep::IdleNotReady
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_scroll_advances_by_the_recovered_steps_and_wraps() {
        let s = OverlayScroll::default();
        assert_eq!(s.primary, 0);
        assert_eq!(s.secondary, 0);
        let s = s.advance();
        assert_eq!(s.primary, OVERLAY_SCROLL_PRIMARY_STEP);
        assert_eq!(s.secondary, OVERLAY_SCROLL_SECONDARY_STEP);
        assert_eq!(OVERLAY_SCROLL_PRIMARY_STEP, 0x1999);
        assert_eq!(OVERLAY_SCROLL_SECONDARY_STEP, 0x51e);
        // Three frames accumulate linearly.
        let s = s.advance().advance();
        assert_eq!(s.primary, 3 * OVERLAY_SCROLL_PRIMARY_STEP);
        assert_eq!(s.secondary, 3 * OVERLAY_SCROLL_SECONDARY_STEP);
        // The primary accumulator wraps at the u32 boundary with no clamp.
        let near_wrap = OverlayScroll {
            primary: u32::MAX - 1,
            secondary: 0,
        };
        assert_eq!(
            near_wrap.advance().primary,
            (u32::MAX - 1).wrapping_add(OVERLAY_SCROLL_PRIMARY_STEP)
        );
    }

    #[test]
    fn overlay_alt_plan_gates_only_the_alt_transform() {
        // Sub-mode active: alt-transform group runs; overlay + composite always do.
        let active = overlay_alt_plan(true);
        assert!(active.alt_transform);
        assert!(active.scrolling_overlay);
        assert_eq!(active.composite, SharedComposite { args: [1, 1, 0] });
        // Sub-mode inactive: only the alt-transform group is skipped.
        let inactive = overlay_alt_plan(false);
        assert!(!inactive.alt_transform);
        assert!(inactive.scrolling_overlay);
        assert_eq!(inactive.composite, SharedComposite::OVERLAY_ALT);
    }

    #[test]
    fn fade_in_progress_tracks_the_active_flag_and_counter_margin() {
        // Inactive record: never in progress regardless of counters.
        assert!(
            !FadeTransition {
                active: false,
                current: 0,
                target: 100,
            }
            .in_progress()
        );
        // Active and below target+3: in progress.
        assert!(
            FadeTransition {
                active: true,
                current: 50,
                target: 100,
            }
            .in_progress()
        );
        // At exactly target+2 (< target+3): still in progress.
        assert!(
            FadeTransition {
                active: true,
                current: 102,
                target: 100,
            }
            .in_progress()
        );
        // At target+3: reached, no longer in progress.
        assert!(
            !FadeTransition {
                active: true,
                current: 103,
                target: 100,
            }
            .in_progress()
        );
        assert_eq!(FADE_COUNTER_MARGIN, 3);
    }

    #[test]
    fn fade_idle_presents_only_when_the_device_is_ready() {
        let idle = FadeTransition {
            active: false,
            current: 0,
            target: 0,
        };
        // Path A, device ready -> present + flip the present-enable pair.
        assert_eq!(
            fade_composite_plan(idle, true, false, false, false),
            FadeCompositeStep::PresentAndFlip
        );
        // Path A, device not ready -> idle, no present.
        assert_eq!(
            fade_composite_plan(idle, false, true, true, true),
            FadeCompositeStep::IdleNotReady
        );
        assert_eq!(PRESENT_GATE_CLEAR_ADDR, 0x00b6_3748);
        assert_eq!(PRESENT_GATE_SET_ADDR, 0x00b6_374c);
    }

    #[test]
    fn fade_in_progress_composites_unless_all_three_gates_hold() {
        let fading = FadeTransition {
            active: true,
            current: 10,
            target: 100,
        };
        // All three suppress gates hold -> hold (skip the composite).
        assert_eq!(
            fade_composite_plan(fading, true, true, true, true),
            FadeCompositeStep::HoldDuringFade
        );
        // Any single escape hatch -> composite the world with (1,0,0).
        let world = FadeCompositeStep::CompositeWorld(SharedComposite { args: [1, 0, 0] });
        assert_eq!(fade_composite_plan(fading, true, false, true, true), world);
        assert_eq!(fade_composite_plan(fading, true, true, false, true), world);
        assert_eq!(fade_composite_plan(fading, true, true, true, false), world);
        // The device-ready flag is ignored while a fade is in progress (path B).
        assert_eq!(
            fade_composite_plan(fading, false, false, false, false),
            world
        );
    }
}
