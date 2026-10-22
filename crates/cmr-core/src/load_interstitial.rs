//! The load/interstitial screen's recovered progress model (FSM render route
//! [`RenderRoute::LoadInterstitial`](crate::render_route::RenderRoute::LoadInterstitial),
//! state `4`, `rea.official-eu.game-fsm-load-surface`).
//!
//! State `4`'s render handler `FUN_00421da0` drives subsystem initialization and,
//! each frame, runs the progress pump `FUN_0044de20`: when loading is active
//! ([`load_interstitial_active`], `FUN_0044e100`) and the front-end context gate
//! `DAT_00d3ca88` is `0`, it draws a two-part loading bar via `FUN_0044dea0`
//! (`IMAGES\LOADING\BAR.BMP` static frame + a progress-driven `IMAGES\LOADING\LINE.BMP`
//! fill), then presents. The fill position is scaled by a 0..=255 progress byte
//! (`DAT_0055c240`, capped at `DAT_0055c241`) and the screen width, with the
//! coordinate math special-cased for the 320- and 512-wide internal resolutions.
//!
//! This module reconstructs the parts that are fully recovered and
//! backend/asset-independent: the active gate, the progress counter, and the
//! loading-bar asset paths. The exact destination-rect arithmetic in
//! `FUN_0044dea0` (the `progress * width * 0xd5 / ...` fill formula and the
//! per-resolution offsets) and the BMP sprite blit (`FUN_0049e3e0`, reading the
//! user-owned `BAR.BMP`/`LINE.BMP` from the source root) are the remaining
//! asset-gated reconstruction, left to a 2D-blit compositor. State `3`
//! ([`RenderRoute::FrontendSurface`](crate::render_route::RenderRoute::FrontendSurface),
//! `FUN_00421d80` -> `FUN_00498660`) draws no pixels - it is DirectDraw
//! window/surface (cooperative-level/clipper) setup a modern windowed backend
//! owns, so it has no reconstructed compositor.

/// The static loading-bar frame bitmap, relative to the source root
/// (`%sIMAGES\LOADING\BAR.BMP`, `0x4f1030`).
pub const LOADING_BAR_BMP: &str = r"IMAGES\LOADING\BAR.BMP";

/// The progress-driven loading-bar fill bitmap, relative to the source root
/// (`%sIMAGES\LOADING\LINE.BMP`, `0x4f1014`).
pub const LOADING_LINE_BMP: &str = r"IMAGES\LOADING\LINE.BMP";

/// The two internal screen widths the loading-bar coordinate math special-cases
/// (`0x140` = 320 and `0x200` = 512), from `FUN_0044dea0`.
pub const LOADING_SCREEN_WIDTHS: [u16; 2] = [320, 512];

/// Whether the load/interstitial pump draws this frame (`FUN_0044e100`):
/// `(DAT_00d3ca88 != 0 || DAT_0055001c == 0) && DAT_0055c244 != 0` - i.e. the
/// front-end/attract context is active (`frontend_active`) or the alternate load
/// flag is clear (`config_zero`), and loading is armed (`loading_armed`).
#[must_use]
pub fn load_interstitial_active(frontend_active: bool, config_zero: bool, loading_armed: bool) -> bool {
    (frontend_active || config_zero) && loading_armed
}

/// The recovered loading-bar progress counter (`DAT_0055c240`, capped at
/// `DAT_0055c241`): a value the loader advances as subsystem init proceeds, read
/// as a 0..=255 byte (`DAT_0055c240 & 0xff`) that scales the `LINE.BMP` fill.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LoadProgress {
    current: u8,
    cap: u8,
}

impl LoadProgress {
    /// A fresh counter at `0`, completing at `cap`.
    #[must_use]
    pub fn new(cap: u8) -> Self {
        Self { current: 0, cap }
    }

    /// Advance one step toward the cap, saturating (the pump increments
    /// `DAT_0055c240` and never past `DAT_0055c241`).
    pub fn advance(&mut self) {
        if self.current < self.cap {
            self.current += 1;
        }
    }

    /// Set the counter to a loader-supplied value, clamped to the cap.
    pub fn set(&mut self, value: u8) {
        self.current = value.min(self.cap);
    }

    /// The 0..=255 progress byte the drawer reads (`DAT_0055c240 & 0xff`).
    #[must_use]
    pub fn byte(self) -> u8 {
        self.current
    }

    /// The cap the counter completes at (`DAT_0055c241`).
    #[must_use]
    pub fn cap(self) -> u8 {
        self.cap
    }

    /// Whether the counter has reached its cap.
    #[must_use]
    pub fn is_complete(self) -> bool {
        self.current >= self.cap
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_gate_matches_the_recovered_condition() {
        // Armed + front-end active -> draw.
        assert!(load_interstitial_active(true, false, true));
        // Armed + config flag clear -> draw.
        assert!(load_interstitial_active(false, true, true));
        // Armed but neither context condition -> no draw.
        assert!(!load_interstitial_active(false, false, true));
        // Not armed -> never draws, regardless of context.
        assert!(!load_interstitial_active(true, true, false));
    }

    #[test]
    fn progress_counter_advances_clamps_and_completes() {
        let mut p = LoadProgress::new(3);
        assert_eq!(p.byte(), 0);
        assert!(!p.is_complete());
        p.advance();
        p.advance();
        assert_eq!(p.byte(), 2);
        p.advance();
        assert_eq!(p.byte(), 3);
        assert!(p.is_complete());
        // Saturates at the cap.
        p.advance();
        assert_eq!(p.byte(), 3);
        // `set` clamps to the cap.
        let mut q = LoadProgress::new(100);
        q.set(50);
        assert_eq!(q.byte(), 50);
        q.set(200);
        assert_eq!(q.byte(), 100);
        assert!(q.is_complete());
    }

    #[test]
    fn loading_assets_and_widths_match_recovery() {
        assert_eq!(LOADING_BAR_BMP, r"IMAGES\LOADING\BAR.BMP");
        assert_eq!(LOADING_LINE_BMP, r"IMAGES\LOADING\LINE.BMP");
        assert_eq!(LOADING_SCREEN_WIDTHS, [320, 512]);
    }
}
