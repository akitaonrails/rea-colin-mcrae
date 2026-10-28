//! The state-`0xa` pause panel placement (`FUN_00468dd0`, B handler
//! `FUN_004233e0`, `rea.official-eu.game-fsm-overlay-alt`).
//!
//! After the pause menu draws its text, `FUN_00468dd0` sizes a background
//! panel to it from two words the text draw leaves behind: `DAT_0057e684`, the
//! widest line as a Q16 fraction of the screen width, and `DAT_004f3e34`, the
//! Q16 y cursor after the last line. It pads them (`+0x1999`, `+0x51e`),
//! writes them back, and builds a four-`u16` `[x, y, w, h]` rectangle centred
//! horizontally with its top at `0.3 H`. The title pre-draw resets both words
//! every frame.
//!
//! An earlier reading took the two words for free-running animation
//! accumulators; `rea.official-eu.pause-menu` corrects that, and adds the
//! `- 0x4ccc` (the `0.3` top) in the height that the earlier version missed.
//! The products are 32-bit, as the shipped `imul`s are.

/// The width padding (`DAT_0057e684 += 0x1999`, about `0.1`).
pub const PANEL_WIDTH_PAD_Q16: i32 = 0x1999;
/// The bottom padding (`DAT_004f3e34 += 0x51e`, about `0.02`).
pub const PANEL_BOTTOM_PAD_Q16: i32 = 0x51e;
/// The panel top, `19660 / 65536` of the height (about `0.3`).
pub const PANEL_TOP_Q16: i32 = 19660;
/// The top subtracted from the bottom cursor for the height (`0x4ccc`).
pub const PANEL_TOP_OFFSET_Q16: i32 = 0x4ccc;

/// A destination rectangle in screen pixels (the `[x, y, w, h]` u16 struct).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OverlayQuad {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

/// `FUN_00468dd0`: the panel rectangle and the padded words it writes back,
/// from the text width `DAT_0057e684` and the y cursor `DAT_004f3e34`.
#[must_use]
pub fn pause_panel_quad(
    text_width_q16: i32,
    cursor_y_q16: i32,
    screen_width: u16,
    screen_height: u16,
) -> (OverlayQuad, i32, i32) {
    let w = i32::from(screen_width);
    let h = i32::from(screen_height);
    let width = text_width_q16.wrapping_add(PANEL_WIDTH_PAD_Q16);
    let bottom = cursor_y_q16.wrapping_add(PANEL_BOTTOM_PAD_Q16);
    // cdq; sub eax, edx; sar eax, 1: halve rounding toward zero.
    let half = (width - (width >> 31)) >> 1;
    let quad = OverlayQuad {
        x: (0x8000_i32.wrapping_sub(half).wrapping_mul(w) >> 16) as u16,
        y: (h.wrapping_mul(PANEL_TOP_Q16) >> 16) as u16,
        w: (w.wrapping_mul(width) >> 16) as u16,
        h: (bottom.wrapping_sub(PANEL_TOP_OFFSET_Q16).wrapping_mul(h) >> 16) as u16,
    };
    (quad, width, bottom)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_panel_is_centred_and_sized_to_the_text() {
        // Text 0.5 wide, cursor at 0.7: the panel pads to 0.6 x (0.72 - 0.3).
        let (quad, width, bottom) = pause_panel_quad(0x8000, 0xb333, 640, 480);
        assert_eq!(width, 0x8000 + 0x1999);
        assert_eq!(bottom, 0xb333 + 0x51e);
        assert_eq!(quad.y, ((480 * 19660) >> 16) as u16);
        assert_eq!(quad.w, ((640 * width) >> 16) as u16);
        let half = width / 2;
        assert_eq!(quad.x, (((0x8000 - half) * 640) >> 16) as u16);
        assert_eq!(quad.h, (((bottom - 0x4ccc) * 480) >> 16) as u16);
        // Centred: left margin equals right margin within a pixel.
        let right = 640 - (u32::from(quad.x) + u32::from(quad.w));
        assert!(u32::from(quad.x).abs_diff(right) <= 1);
    }

    #[test]
    fn halving_rounds_toward_zero_for_a_negative_width() {
        // width = -0x1999 + 0x1999 - 1 = -1: half is 0, not -1.
        let (quad, width, _) = pause_panel_quad(-0x199a, 0x4ccc - 0x51e, 640, 480);
        assert_eq!(width, -1);
        assert_eq!(quad.x, ((0x8000 * 640) >> 16) as u16);
        assert_eq!(quad.h, 0);
    }
}
