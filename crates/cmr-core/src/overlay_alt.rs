//! The state-`0xa` ("OverlayAlt") animated screen-quad placement
//! ([`RenderRoute::OverlayAlt`](crate::render_route::RenderRoute::OverlayAlt),
//! B handler `FUN_004233e0` -> `FUN_00468dd0`, `rea.official-eu.game-fsm-overlay-alt`).
//!
//! State `0xa` composites the still-running in-race world (via the shared
//! compositor `FUN_00422ca0`, which treats `0xa` exactly as in-race state `5`)
//! plus two overlay layers: a runtime entity-table overlay (`FUN_0043af20`) and
//! this one animated screen-space quad (`FUN_00468dd0`). The entity table, the
//! destination surfaces, the display-format-selected colour/vertex templates
//! (`DAT_005024a4`/`DAT_004eb604`), and the visibility oracle are all
//! runtime/asset-gated. The one statically-pinned, asset-free piece is this
//! quad's destination-rectangle arithmetic, reconstructed here.
//!
//! `FUN_00468dd0` reads the runtime screen width/height (`W`/`H`, from the
//! surface descriptor `[PTR_DAT_00502518] + 0/+4`) and two per-frame animation
//! phase accumulators (`DAT_0057e684`, `DAT_004f3e34`) that step by fixed
//! increments each frame, then builds a four-`u16` `[x, y, w, h]` rectangle. All
//! multipliers are baked into the image; the accumulators' initial values and any
//! wrap/reset are runtime state, so they are modelled as caller-held state
//! starting at `0`.

/// The per-frame increment of the X-phase accumulator (`DAT_0057e684 += 0x1999`).
pub const OVERLAY_ALT_PHASE_X_STEP: i32 = 0x1999;

/// The per-frame increment of the height-phase accumulator (`DAT_004f3e34 += 0x51e`).
pub const OVERLAY_ALT_PHASE_H_STEP: i32 = 0x51e;

/// The Y-position fraction (`y = H * 19660 >> 16` ~= `0.300 * H`), a fixed
/// vertical placement independent of the animation phase.
pub const OVERLAY_ALT_Y_FRACTION_Q16: i32 = 19660;

/// The Q16.16 `0.5` the X phase is subtracted from: `x = (0x8000 - (phase_x >>
/// 1)) * W >> 16`, so at phase `0` the quad starts horizontally centered.
const X_CENTER_Q16: i32 = 0x8000;

/// A destination rectangle in screen pixels - the four-`u16` `[x, y, w, h]`
/// struct `FUN_00468dd0` builds for the draw (right `= x + w`, bottom `= y + h`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OverlayQuad {
    /// Left edge (pixels).
    pub x: u16,
    /// Top edge (pixels).
    pub y: u16,
    /// Width (pixels).
    pub w: u16,
    /// Height (pixels).
    pub h: u16,
}

/// The two animation phase accumulators `FUN_00468dd0` advances each frame
/// (`DAT_0057e684` and `DAT_004f3e34`). Modelled starting at `0`; the shipped
/// globals' initial values and any wrap/reset are runtime state.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OverlayAltPhase {
    x: i32,
    h: i32,
}

impl OverlayAltPhase {
    /// A fresh phase at `0`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Step both accumulators one frame (the `+= 0x1999` / `+= 0x51e` writes),
    /// wrapping like the shipped 32-bit integer adds.
    pub fn advance(&mut self) {
        self.x = self.x.wrapping_add(OVERLAY_ALT_PHASE_X_STEP);
        self.h = self.h.wrapping_add(OVERLAY_ALT_PHASE_H_STEP);
    }

    /// The X-phase accumulator (`DAT_0057e684`).
    #[must_use]
    pub fn x(self) -> i32 {
        self.x
    }

    /// The height-phase accumulator (`DAT_004f3e34`).
    #[must_use]
    pub fn h(self) -> i32 {
        self.h
    }
}

/// The recovered quad placement (`FUN_00468dd0`): the `[x, y, w, h]` destination
/// rectangle from the current phase accumulators and the runtime screen size.
///
/// Each field is a Q16.16 product arithmetic-shifted right by 16 then stored into
/// a `u16`, matching the shipped `(u16)(.. >> 16)` writes (the `as u16` keeps the
/// low 16 bits, wrapping, as the C cast does): `x = (0x8000 - (phase_x >> 1)) *
/// W >> 16`, `y = H * 19660 >> 16`, `w = W * phase_x >> 16`, `h = phase_h * H >>
/// 16`.
#[must_use]
pub fn overlay_alt_quad(
    phase: OverlayAltPhase,
    screen_width: u16,
    screen_height: u16,
) -> OverlayQuad {
    let w = i64::from(screen_width);
    let h = i64::from(screen_height);
    let phase_x = i64::from(phase.x);
    let phase_h = i64::from(phase.h);
    OverlayQuad {
        x: (((i64::from(X_CENTER_Q16) - (phase_x >> 1)) * w) >> 16) as u16,
        y: ((h * i64::from(OVERLAY_ALT_Y_FRACTION_Q16)) >> 16) as u16,
        w: ((w * phase_x) >> 16) as u16,
        h: ((phase_h * h) >> 16) as u16,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_zero_starts_centered_with_a_fixed_vertical_and_zero_size() {
        // At phase 0: x = 0.5*W (0x8000 Q16.16), y = 0.300*H, w = h = 0.
        let quad = overlay_alt_quad(OverlayAltPhase::new(), 640, 480);
        assert_eq!(
            quad,
            OverlayQuad {
                x: 320,
                y: 143,
                w: 0,
                h: 0
            }
        );
        // Independent of width for the centering and of the phase for y.
        assert_eq!(overlay_alt_quad(OverlayAltPhase::new(), 512, 384).x, 256);
        assert_eq!(overlay_alt_quad(OverlayAltPhase::new(), 512, 384).y, 115);
    }

    #[test]
    fn accumulators_step_by_the_recovered_increments() {
        let mut phase = OverlayAltPhase::new();
        phase.advance();
        assert_eq!((phase.x(), phase.h()), (0x1999, 0x51e));
        phase.advance();
        assert_eq!((phase.x(), phase.h()), (0x3332, 0xa3c));
    }

    #[test]
    fn one_frame_in_grows_and_slides_the_quad() {
        // After one frame the quad has a nonzero width/height and the left edge
        // has slid in from center (phase_x reduces the 0.5 centering term).
        let mut phase = OverlayAltPhase::new();
        phase.advance();
        let quad = overlay_alt_quad(phase, 640, 480);
        // x = (0x8000 - 0xccc)*640 >> 16 = 288; w = 640*0x1999 >> 16 = 63;
        // h = 0x51e*480 >> 16 = 9; y unchanged at 143.
        assert_eq!(
            quad,
            OverlayQuad {
                x: 288,
                y: 143,
                w: 63,
                h: 9
            }
        );
        assert!(quad.x < 320 && quad.w > 0 && quad.h > 0);
    }

    #[test]
    fn placement_truncates_to_u16_like_the_shipped_stores() {
        // A large phase drives the width product past 16 bits; the store keeps the
        // low 16 bits (wrapping), as the shipped (u16) cast does.
        let phase = OverlayAltPhase { x: 0x20_0000, h: 0 };
        let quad = overlay_alt_quad(phase, 640, 480);
        // w = (640 * 0x20_0000) >> 16 = 0x50_0000 >> ... = 640*32 = 20480 fits,
        // but x = (0x8000 - 0x10_0000)*640 >> 16 is negative -> wraps in u16.
        assert_eq!(quad.w, ((640i64 * 0x20_0000) >> 16) as u16);
        assert_eq!(quad.x, (((0x8000i64 - 0x10_0000) * 640) >> 16) as u16);
    }
}
