//! The per-state render-handler routing - the **render (B) column** of the
//! top-level FSM, the counterpart to the update (A) side reconstructed in
//! [`crate::game_menu_flow`] / [`crate::game_race_flow`].
//!
//! The dispatch table at `0x4ee82c` pairs each state with an update handler A
//! (whose return is the next state) and a render handler B. The driver runs B
//! separately, against the **previous** state (`_DAT_005590a0 =
//! FUN_004440d0(prev); (*)()`). This module classifies each B handler into a
//! backend-neutral [`RenderRoute`] describing what that state composes, recovered
//! statically (`rea.official-eu.game-fsm-render`, asm).
//!
//! **Structural finding (observed):** the B handlers do **not** present or flip.
//! They are per-state scene-composition callbacks. The actual present pump
//! (`0x48b650`), buffer flip (`0x4446e0`) and HUD-group enable (`0x487170`) run
//! on the update (A) side and the shared present wrapper `FUN_00468200`, gated by
//! the present-request flags `0x57e66c`/`0xb63748`. So the present/flip is a
//! separate step a backend runs after routing composition - see [`PRESENT_NOTE`].
//!
//! The in-race states (`5..=9`) share a stereotyped composition chain (frame
//! begin -> render-interpolation alpha -> per-viewport camera/view transform
//! `0x439ee0` -> frame end -> object interpolation loop `0x423ec0` -> the shared
//! per-viewport finalizer/compositor `0x422ca0`, which draws the **state-selected**
//! HUD/overlay). States `5` and `6` share the **same** B handler `0x4231f0`, so
//! their render routes are identical at this level; the race-HUD-vs-replay-overlay
//! choice is made downstream inside the finalizer by game state (an inference,
//! flagged below), not by the B handler. States `7`/`8`/`9` have distinct B
//! handlers differing by the observed banner / dirty-consume / resource flags.
//!
//! Several per-state *roles* (which screen a state "is") remain unproven in the
//! recovery; this module captures only what the B handler is observed to compose.

use crate::game_state::{GameState, STATE_HANDLERS};

/// Where the shipped present/flip actually happens (not in any B handler): the
/// shared wrapper `FUN_00468200` runs HUD-enable `0x487170`, the present pump
/// `0x48b650` and the flip `0x4446e0`, gated by `0x57e66c`/`0xb63748`. A backend
/// routes [`RenderRoute`] composition, then runs this present step separately.
pub const PRESENT_NOTE: &str =
    "present/flip is a separate A-side step (FUN_00468200); B handlers only compose";

/// The observed composition variant for an in-race world render (states `5..=9`),
/// capturing the differences between the four distinct in-race B handlers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StageView {
    /// Draws the status/notification banner `0x46e460` (when the world is active):
    /// the handlers at `0x4231f0` (states 5/6) and `0x423380` (state 9).
    pub status_banner: bool,
    /// Consumes the overlay dirty flag `0x468d00` this pass: only `0x4231f0`
    /// (states 5/6).
    pub consume_overlay_dirty: bool,
    /// Runs the pre-start resource/path branch `0x4232f0`: only `0x423260`
    /// (state 8).
    pub resource_prestage: bool,
}

/// What a state's render handler B composes. The frame the viewer sees is this
/// composition followed by the separate present/flip step ([`PRESENT_NOTE`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderRoute {
    /// Nothing composed - a `ret` stub (`0x4234a0`). States `0` and `0xc`
    /// (terminal/sentinel): the frame is whatever the present step carries.
    None,
    /// Boot: reset the present-enable gate `0xb63748` to 0; no content
    /// (`0x421cc0`, state `1`).
    ResetPresentGate,
    /// Front-end display refresh, tearing down a lingering race on entry
    /// (`0x421cd0`, state `2`).
    FrontendDisplay,
    /// Front-end window/surface (re)configuration (`0x421d80` -> `0x498660`,
    /// state `3`).
    FrontendSurface,
    /// Load / enter-stage interstitial: the per-frame progress pump `0x44de20`
    /// between subsystem-init steps (`0x421da0`, state `4`).
    LoadInterstitial,
    /// The live in-race world composite (states `5..=9`), parameterized by the
    /// observed [`StageView`] flags.
    StageWorld(StageView),
    /// Overlay/alternate view: the alt transform `0x43af20` + a scrolling 2D
    /// overlay `0x468dd0` + composite, gated by the secondary mode flag
    /// `0xd3ca94` (`0x4233e0`, state `0xa`). Does not run the world camera
    /// transform or the object-interpolation loop.
    OverlayAlt,
    /// Fade-managed composite / results hub: queries the fade object
    /// (`DAT_0082b37c`) and either drives a device-level present op during a fade
    /// or composites normally, toggling the present-enable flags (`0x423420`,
    /// state `0xb`).
    FadeComposite,
}

/// The recovered render handler B address for a state (`STATE_HANDLERS[id].1`),
/// the single source of truth for the pointer (this module does not re-list it).
#[must_use]
pub fn render_handler(state: GameState) -> u32 {
    STATE_HANDLERS[state.id() as usize].1
}

/// Classify a state's render handler B into its backend-neutral [`RenderRoute`].
///
/// A total function over the 13 states; states sharing a B handler share a route
/// (`0`/`0xc` -> [`RenderRoute::None`], `5`/`6` -> the same [`RenderRoute::StageWorld`]).
#[must_use]
pub fn render_route(state: GameState) -> RenderRoute {
    match state.id() {
        0x0 | 0xc => RenderRoute::None,
        0x1 => RenderRoute::ResetPresentGate,
        0x2 => RenderRoute::FrontendDisplay,
        0x3 => RenderRoute::FrontendSurface,
        0x4 => RenderRoute::LoadInterstitial,
        // States 5 and 6 share B handler 0x4231f0: banner + dirty-consume.
        0x5 | 0x6 => RenderRoute::StageWorld(StageView {
            status_banner: true,
            consume_overlay_dirty: true,
            resource_prestage: false,
        }),
        // State 7 (0x422c30): no banner, no dirty-consume, extra entry gates.
        0x7 => RenderRoute::StageWorld(StageView {
            status_banner: false,
            consume_overlay_dirty: false,
            resource_prestage: false,
        }),
        // State 8 (0x423260): resource/pre-start branch, no banner.
        0x8 => RenderRoute::StageWorld(StageView {
            status_banner: false,
            consume_overlay_dirty: false,
            resource_prestage: true,
        }),
        // State 9 (0x423380): status banner, no dirty-consume.
        0x9 => RenderRoute::StageWorld(StageView {
            status_banner: true,
            consume_overlay_dirty: false,
            resource_prestage: false,
        }),
        0xa => RenderRoute::OverlayAlt,
        0xb => RenderRoute::FadeComposite,
        // GameState is validated 0..=0xC, so no other id exists.
        other => unreachable!("GameState out of range: {other:#x}"),
    }
}

/// Whether a route composes the in-race world scene (the `StageWorld` family):
/// the states that run the camera/view transform + object-interpolation chain.
#[must_use]
pub fn composes_world(route: RenderRoute) -> bool {
    matches!(route, RenderRoute::StageWorld(_))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(id: u8) -> GameState {
        GameState::new(id).expect("valid state id")
    }

    #[test]
    fn render_handler_matches_the_recovered_table() {
        // The B pointer comes from STATE_HANDLERS, including the corrected
        // states 9 / 0xa (0x42_, not the earlier 0x43_ typo).
        assert_eq!(render_handler(state(0x9)), 0x0042_3380);
        assert_eq!(render_handler(state(0xa)), 0x0042_33e0);
        // Shared handlers: 0 and 0xc, and 5 and 6.
        assert_eq!(render_handler(state(0x0)), render_handler(state(0xc)));
        assert_eq!(render_handler(state(0x0)), 0x0042_34a0);
        assert_eq!(render_handler(state(0x5)), render_handler(state(0x6)));
        assert_eq!(render_handler(state(0x5)), 0x0042_31f0);
    }

    #[test]
    fn terminal_and_sentinel_states_compose_nothing() {
        assert_eq!(render_route(state(0x0)), RenderRoute::None);
        assert_eq!(render_route(state(0xc)), RenderRoute::None);
        assert!(!composes_world(render_route(state(0x0))));
    }

    #[test]
    fn pre_gameplay_states_map_to_their_display_routes() {
        assert_eq!(render_route(state(0x1)), RenderRoute::ResetPresentGate);
        assert_eq!(render_route(state(0x2)), RenderRoute::FrontendDisplay);
        assert_eq!(render_route(state(0x3)), RenderRoute::FrontendSurface);
        assert_eq!(render_route(state(0x4)), RenderRoute::LoadInterstitial);
    }

    #[test]
    fn states_sharing_a_b_handler_share_a_route() {
        // 5 and 6 share 0x4231f0, so their routes are identical.
        assert_eq!(render_route(state(0x5)), render_route(state(0x6)));
        let RenderRoute::StageWorld(view) = render_route(state(0x5)) else {
            panic!("state 5 should compose the world");
        };
        assert!(view.status_banner && view.consume_overlay_dirty && !view.resource_prestage);
    }

    #[test]
    fn in_race_handlers_differ_by_the_observed_flags() {
        // 7: distinct handler, no banner / no dirty-consume / no resource branch.
        assert_eq!(
            render_route(state(0x7)),
            RenderRoute::StageWorld(StageView {
                status_banner: false,
                consume_overlay_dirty: false,
                resource_prestage: false,
            })
        );
        // 8: the resource/pre-start branch.
        let RenderRoute::StageWorld(view8) = render_route(state(0x8)) else {
            panic!("state 8 world");
        };
        assert!(view8.resource_prestage && !view8.status_banner);
        // 9: the status banner, no dirty-consume.
        let RenderRoute::StageWorld(view9) = render_route(state(0x9)) else {
            panic!("state 9 world");
        };
        assert!(view9.status_banner && !view9.consume_overlay_dirty);
        // All of 5..=9 compose the world; 0xa / 0xb do not.
        for id in 0x5..=0x9 {
            assert!(composes_world(render_route(state(id))));
        }
    }

    #[test]
    fn overlay_and_fade_states_are_distinct_from_the_world() {
        assert_eq!(render_route(state(0xa)), RenderRoute::OverlayAlt);
        assert_eq!(render_route(state(0xb)), RenderRoute::FadeComposite);
        assert!(!composes_world(render_route(state(0xa))));
        assert!(!composes_world(render_route(state(0xb))));
    }

    #[test]
    fn render_route_is_total_over_all_states() {
        // Every valid state id has a route (no panic), and the only states that
        // compose nothing are the terminal pair.
        for id in 0x0..=0xc {
            let route = render_route(state(id));
            if id == 0x0 || id == 0xc {
                assert_eq!(route, RenderRoute::None);
            } else {
                assert_ne!(route, RenderRoute::None);
            }
        }
    }
}
