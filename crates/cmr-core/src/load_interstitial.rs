//! The load/interstitial screen's recovered progress model (FSM render route
//! [`RenderRoute::LoadInterstitial`](crate::render_route::RenderRoute::LoadInterstitial),
//! state `4`, `rea.official-eu.game-fsm-load-surface`).
//!
//! State `4`'s render handler `FUN_00421da0` drives subsystem initialization and,
//! each frame, runs the progress pump `FUN_0044de20`: when loading is active
//! ([`load_interstitial_active`], `FUN_0044e100`) and the front-end context gate
//! `DAT_00d3ca88` is `0`, it draws a two-part loading bar via `FUN_0044dea0`
//! (a progress-driven `IMAGES\LOADING\BAR.BMP` fill, then a static 1px
//! `IMAGES\LOADING\LINE.BMP` rule on top), then presents. The fill width is scaled
//! by a 0..=255 progress byte (`DAT_0055c240`, capped at `DAT_0055c241`) and the
//! screen width, with the coordinate math special-cased for the 320- and 512-wide
//! internal resolutions.
//!
//! This module reconstructs the parts that are fully recovered and
//! backend/asset-independent: the active gate, the progress counter, the
//! loading-bar asset paths, and the exact destination-rectangle geometry
//! ([`loading_bar_fill_rect`]/[`loading_bar_line_rect`], from `FUN_0044dea0`). The
//! BMP sprite blit itself (`FUN_0049e3e0`, reading the user-owned `BAR.BMP`/
//! `LINE.BMP` from the source root and clipping/stretching into the DirectDraw
//! surface) stays asset-gated, left to a 2D-blit compositor. State `3`
//! ([`RenderRoute::FrontendSurface`](crate::render_route::RenderRoute::FrontendSurface),
//! `FUN_00421d80` -> `FUN_00498660`) draws no pixels - it is DirectDraw
//! window/surface (cooperative-level/clipper) setup a modern windowed backend
//! owns, so it has no reconstructed compositor.

/// The progress-driven loading-bar fill bitmap, relative to the source root
/// (`%sIMAGES\LOADING\BAR.BMP`, `0x4f1030`); its dest width scales with progress.
pub const LOADING_BAR_BMP: &str = r"IMAGES\LOADING\BAR.BMP";

/// The static 1px loading-bar rule bitmap, relative to the source root
/// (`%sIMAGES\LOADING\LINE.BMP`, `0x4f1014`); drawn on top of the fill at a fixed
/// rectangle from a 1x1 source.
pub const LOADING_LINE_BMP: &str = r"IMAGES\LOADING\LINE.BMP";

/// The two internal screen widths the loading-bar coordinate math special-cases
/// (`0x140` = 320 and `0x200` = 512), from `FUN_0044dea0`.
pub const LOADING_SCREEN_WIDTHS: [u16; 2] = [320, 512];

/// Whether the load/interstitial pump draws this frame (`FUN_0044e100`):
/// `(DAT_00d3ca88 != 0 || DAT_0055001c == 0) && DAT_0055c244 != 0` - i.e. the
/// front-end/attract context is active (`frontend_active`) or the alternate load
/// flag is clear (`config_zero`), and loading is armed (`loading_armed`).
#[must_use]
pub fn load_interstitial_active(
    frontend_active: bool,
    config_zero: bool,
    loading_armed: bool,
) -> bool {
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

/// A 2D blit destination rectangle in screen pixels, in the recovered
/// `[x, y, w, h]` convention `FUN_0049e3e0` consumes (right `= x + w`, bottom `=
/// y + h`). The blit clips this against the destination surface and stretches the
/// source to fit, so a width past the surface edge is clipped, not an error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BlitRect {
    /// Left edge (pixels).
    pub x: i32,
    /// Top edge (pixels).
    pub y: i32,
    /// Width (pixels); may exceed the surface and be clipped by the blit.
    pub w: i32,
    /// Height (pixels).
    pub h: i32,
}

/// The `FUN_0044dea0` mode constant `K`: `386` (`0x182`) at `screen_width >= 512`,
/// else `350` (`0x15e)`. It positions the bar's left edge the same way for both
/// the fill and the static line.
#[must_use]
pub fn loading_bar_mode_k(screen_width: i32) -> i32 {
    if screen_width >= 512 { 386 } else { 350 }
}

/// The progress-driven fill rectangle (`BAR.BMP`), exactly as `FUN_0044dea0`
/// computes it from the 0..=255 `progress` byte and the screen size:
/// `x = K*W/640`, `y = 106` when `W < 512` else `H*216/480`, `h = H*13/480`, and
/// the progress-scaled `w = progress*W*213/16640` (integer division, truncating
/// toward zero, so `w` is monotonic non-decreasing in `progress`). `W`/`H` are the
/// runtime screen-width/height globals (`[PTR_DAT_00502518 + 0/+4]`). The second
/// `FUN_0044dea0` argument (`0x10000`) is unused by the routine and plays no part
/// here. At high progress `w` can exceed the surface and is clipped by the blit;
/// the true "full" point is the runtime cap ([`LoadProgress::cap`]).
#[must_use]
pub fn loading_bar_fill_rect(progress: u8, screen_width: i32, screen_height: i32) -> BlitRect {
    let k = loading_bar_mode_k(screen_width);
    BlitRect {
        x: k * screen_width / 640,
        y: if screen_width < 512 {
            106
        } else {
            screen_height * 216 / 480
        },
        w: i32::from(progress) * screen_width * 213 / 16640,
        h: screen_height * 13 / 480,
    }
}

/// The static 1px rule rectangle (`LINE.BMP`), drawn on top of the fill:
/// `x = K*W/640`, `y = H*205/480`, `w = 141` when `W == 320` else `W*216/640`,
/// `h = 1`. Stretched from a 1x1 source, so it is a fixed horizontal line
/// independent of progress.
#[must_use]
pub fn loading_bar_line_rect(screen_width: i32, screen_height: i32) -> BlitRect {
    let k = loading_bar_mode_k(screen_width);
    BlitRect {
        x: k * screen_width / 640,
        y: screen_height * 205 / 480,
        w: if screen_width == 320 {
            141
        } else {
            screen_width * 216 / 640
        },
        h: 1,
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

    #[test]
    fn mode_constant_matches_recovered_threshold() {
        assert_eq!(loading_bar_mode_k(320), 350);
        assert_eq!(loading_bar_mode_k(511), 350);
        assert_eq!(loading_bar_mode_k(512), 386);
        assert_eq!(loading_bar_mode_k(640), 386);
    }

    #[test]
    fn fill_rect_matches_recovered_formula_at_320() {
        // 320-wide: K=350 -> x=175, y forced to 106, h = H*13/480.
        let empty = loading_bar_fill_rect(0, 320, 240);
        assert_eq!(
            empty,
            BlitRect {
                x: 175,
                y: 106,
                w: 0,
                h: 6
            }
        );
        // progress*213/52 (= progress*320*213/16640): at 52 -> 213px.
        assert_eq!(loading_bar_fill_rect(52, 320, 240).w, 213);
        // Full byte overshoots the 320 surface (clipped by the blit).
        assert_eq!(loading_bar_fill_rect(255, 320, 240).w, 1044);
    }

    #[test]
    fn fill_rect_matches_recovered_formula_at_512() {
        // 512-wide: K=386 -> x=308, y = H*216/480, h = H*13/480.
        let r = loading_bar_fill_rect(255, 512, 384);
        assert_eq!(
            r,
            BlitRect {
                x: 308,
                y: 172,
                w: 1671,
                h: 10
            }
        );
        // progress*426/65 (= progress*512*213/16640).
        assert_eq!(loading_bar_fill_rect(65, 512, 384).w, 426);
    }

    #[test]
    fn fill_width_is_monotonic_non_decreasing() {
        let mut last = -1;
        for p in 0..=255u8 {
            let w = loading_bar_fill_rect(p, 320, 240).w;
            assert!(w >= last, "fill width went backwards at progress {p}");
            last = w;
        }
    }

    #[test]
    fn line_rect_is_a_fixed_one_pixel_rule() {
        // 320-wide forces w=141; shares the fill's x; y = H*205/480; h=1.
        assert_eq!(
            loading_bar_line_rect(320, 240),
            BlitRect {
                x: 175,
                y: 102,
                w: 141,
                h: 1
            }
        );
        // 512-wide: w = W*216/640.
        assert_eq!(
            loading_bar_line_rect(512, 384),
            BlitRect {
                x: 308,
                y: 164,
                w: 172,
                h: 1
            }
        );
    }
}
