//! Backend-neutral presentation scaling: where a completed logical frame lands
//! inside the physical window surface (reconstruction item B11).
//!
//! The recovered DirectDraw frame-completion contract ([`crate::presentation`])
//! flips a back buffer that is already the game's logical resolution. A modern
//! backend instead owns a resizable window whose drawable size rarely equals
//! that logical resolution, so it must decide how the logical frame is scaled
//! and placed. The shipped-equivalent choice realized by the SDL backend is an
//! aspect-preserving integer scale, centered, with letterbox bars
//! (`SDL_LOGICAL_PRESENTATION_INTEGER_SCALE`).
//!
//! This module defines that geometry as a project-owned contract so every
//! backend agrees pixel-for-pixel. The SDL backend selects the matching SDL
//! logical-presentation mode; a later GPU backend computes [`fit_viewport`]
//! directly and sets its viewport from the result. Both are checked against the
//! same worked examples, so the platform expectation is shared rather than
//! duplicated. Keeping the geometry here - not inside a backend - is what lets
//! the compatibility and modern paths be verified against one definition.

/// The logical (game) frame resolution to present.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LogicalSize {
    /// Logical width in pixels.
    pub width: u32,
    /// Logical height in pixels.
    pub height: u32,
}

/// The physical drawable size of the window surface.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhysicalSize {
    /// Physical width in pixels.
    pub width: u32,
    /// Physical height in pixels.
    pub height: u32,
}

/// A destination rectangle inside the physical surface. `x`/`y` are signed
/// because a frame larger than the surface is centered and overflows
/// symmetrically (the edges are clipped by the window).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Viewport {
    /// Left edge within the physical surface (may be negative).
    pub x: i32,
    /// Top edge within the physical surface (may be negative).
    pub y: i32,
    /// Destination width in pixels.
    pub width: u32,
    /// Destination height in pixels.
    pub height: u32,
}

/// How a logical frame is fitted into the physical surface. Each variant maps
/// one-to-one onto an SDL logical-presentation mode in the SDL backend.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PresentationScaling {
    /// Fill the whole surface, ignoring aspect ratio.
    Stretch,
    /// Largest aspect-preserving fit (fractional scale), centered with bars.
    Letterbox,
    /// Largest aspect-preserving *integer* scale (>= 1), centered with bars.
    IntegerScale,
}

/// The scaling the SDL backend is configured with, and therefore the geometry a
/// differential comparison expects by default.
pub const DEFAULT_SCALING: PresentationScaling = PresentationScaling::IntegerScale;

/// Compute the destination [`Viewport`] for a logical frame inside a physical
/// surface under a scaling policy.
///
/// An empty logical frame (zero on either axis) yields a zero viewport at the
/// origin. Under [`PresentationScaling::IntegerScale`] the scale is clamped to
/// at least `1`, so a surface smaller than the logical frame still presents at
/// 1x and overflows (matching SDL's integer-scale behavior).
pub fn fit_viewport(
    logical: LogicalSize,
    physical: PhysicalSize,
    scaling: PresentationScaling,
) -> Viewport {
    if logical.width == 0 || logical.height == 0 {
        return Viewport {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        };
    }

    let (width, height) = match scaling {
        PresentationScaling::Stretch => (physical.width, physical.height),
        PresentationScaling::Letterbox => {
            let scale = fit_scale(logical, physical);
            (
                (f64::from(logical.width) * scale).round() as u32,
                (f64::from(logical.height) * scale).round() as u32,
            )
        }
        PresentationScaling::IntegerScale => {
            let scale = fit_scale(logical, physical).floor().max(1.0) as u32;
            (logical.width * scale, logical.height * scale)
        }
    };

    Viewport {
        x: center_offset(physical.width, width),
        y: center_offset(physical.height, height),
        width,
        height,
    }
}

/// The largest aspect-preserving (fractional) scale that fits `logical` into
/// `physical`.
fn fit_scale(logical: LogicalSize, physical: PhysicalSize) -> f64 {
    let horizontal = f64::from(physical.width) / f64::from(logical.width);
    let vertical = f64::from(physical.height) / f64::from(logical.height);
    horizontal.min(vertical)
}

/// The centered offset of a `content` extent within an `available` extent.
fn center_offset(available: u32, content: u32) -> i32 {
    (i64::from(available) - i64::from(content)) as i32 / 2
}

#[cfg(test)]
mod tests {
    use super::*;

    const VGA: LogicalSize = LogicalSize {
        width: 320,
        height: 240,
    };

    #[test]
    fn integer_scale_exact_multiple_fills_surface() {
        let viewport = fit_viewport(
            VGA,
            PhysicalSize {
                width: 1280,
                height: 960,
            },
            PresentationScaling::IntegerScale,
        );
        assert_eq!(
            viewport,
            Viewport {
                x: 0,
                y: 0,
                width: 1280,
                height: 960
            }
        );
    }

    #[test]
    fn integer_scale_picks_largest_fitting_multiple_and_centers() {
        // min(1000/320=3.125, 1000/240=4.16) -> floor 3; 960x720 centered.
        let viewport = fit_viewport(
            VGA,
            PhysicalSize {
                width: 1000,
                height: 1000,
            },
            PresentationScaling::IntegerScale,
        );
        assert_eq!(
            viewport,
            Viewport {
                x: 20,
                y: 140,
                width: 960,
                height: 720
            }
        );
    }

    #[test]
    fn integer_scale_clamps_to_one_and_overflows_centered() {
        // Surface smaller than logical: 1x, centered with negative origin.
        let viewport = fit_viewport(
            VGA,
            PhysicalSize {
                width: 100,
                height: 100,
            },
            PresentationScaling::IntegerScale,
        );
        assert_eq!(
            viewport,
            Viewport {
                x: (100 - 320) / 2,
                y: (100 - 240) / 2,
                width: 320,
                height: 240
            }
        );
        assert_eq!(viewport.x, -110);
        assert_eq!(viewport.y, -70);
    }

    #[test]
    fn letterbox_uses_fractional_scale_and_preserves_aspect() {
        // min(1000/320=3.125, 1000/240=4.16) -> 3.125; 1000x750 centered.
        let viewport = fit_viewport(
            VGA,
            PhysicalSize {
                width: 1000,
                height: 1000,
            },
            PresentationScaling::Letterbox,
        );
        assert_eq!(
            viewport,
            Viewport {
                x: 0,
                y: 125,
                width: 1000,
                height: 750
            }
        );
        // 4:3 aspect preserved exactly.
        assert_eq!(viewport.width * 3, viewport.height * 4);
    }

    #[test]
    fn stretch_fills_the_surface_ignoring_aspect() {
        let viewport = fit_viewport(
            VGA,
            PhysicalSize {
                width: 1000,
                height: 1000,
            },
            PresentationScaling::Stretch,
        );
        assert_eq!(
            viewport,
            Viewport {
                x: 0,
                y: 0,
                width: 1000,
                height: 1000
            }
        );
    }

    #[test]
    fn letterbox_bars_can_be_horizontal_or_vertical() {
        // Wide surface -> pillarboxed (bars left/right).
        let wide = fit_viewport(
            VGA,
            PhysicalSize {
                width: 1920,
                height: 480,
            },
            PresentationScaling::Letterbox,
        );
        assert_eq!(wide.height, 480);
        assert!(wide.x > 0 && wide.y == 0);
        // Tall surface -> letterboxed (bars top/bottom).
        let tall = fit_viewport(
            VGA,
            PhysicalSize {
                width: 640,
                height: 1200,
            },
            PresentationScaling::Letterbox,
        );
        assert_eq!(tall.width, 640);
        assert!(tall.y > 0 && tall.x == 0);
    }

    #[test]
    fn empty_logical_frame_yields_zero_viewport() {
        let viewport = fit_viewport(
            LogicalSize {
                width: 0,
                height: 240,
            },
            PhysicalSize {
                width: 800,
                height: 600,
            },
            PresentationScaling::IntegerScale,
        );
        assert_eq!(
            viewport,
            Viewport {
                x: 0,
                y: 0,
                width: 0,
                height: 0
            }
        );
    }

    #[test]
    fn default_scaling_is_integer_scale() {
        assert_eq!(DEFAULT_SCALING, PresentationScaling::IntegerScale);
    }
}
