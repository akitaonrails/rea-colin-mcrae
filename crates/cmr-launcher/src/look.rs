//! The settings window's look: CMR1's colours for the SDK's colour roles,
//! its design size and its decal. The drawing itself is
//! `rea_launcher_ui`; this is game data, kept here so it is tested on the
//! host.
//!
//! The 555 Impreza livery from the game's cover: rally blue with one gold
//! accent for whatever is chosen, and a condensed italic title like a door
//! decal (CMR3's launcher wears the same design in crimson). Gold is both
//! the action colour (Play, the tab underline) and the selection colour.

use rea_launcher_ui::egui::{Color32, Vec2, vec2};
use rea_launcher_ui::theme::{CardMetrics, Metrics, Palette, tint};

pub const BASE: Color32 = Color32::from_rgb(0x0f, 0x2e, 0x6e);
pub const PANEL: Color32 = Color32::from_rgb(0x16, 0x3a, 0x85);
/// Control borders: 3:1 against `BASE` and `PANEL` (WCAG non-text
/// contrast); the least lightening of the old `#2b4f9e` that passes.
pub const LINE: Color32 = Color32::from_rgb(0x71, 0x89, 0xbe);
pub const GOLD: Color32 = Color32::from_rgb(0xf5, 0xc4, 0x00);
pub const GOLD_HOVER: Color32 = Color32::from_rgb(0xff, 0xd3, 0x2a);
pub const INK: Color32 = Color32::from_rgb(0xee, 0xf2, 0xfb);
pub const MUTED: Color32 = Color32::from_rgb(0xa7, 0xb6, 0xd9);
pub const ON_GOLD: Color32 = Color32::from_rgb(0x10, 0x24, 0x4f);
pub const ERROR: Color32 = Color32::from_rgb(0xff, 0x8a, 0x80);

pub const PALETTE: Palette = Palette {
    base: BASE,
    panel: PANEL,
    line: LINE,
    ink: INK,
    muted: MUTED,
    action: GOLD,
    action_hover: GOLD_HOVER,
    on_action: ON_GOLD,
    select: GOLD,
    on_select: ON_GOLD,
    error: ERROR,
    disabled_ink: Color32::from_rgb(0x8a, 0x9a, 0xc2),
    disabled_line: Color32::from_rgb(0x3a, 0x55, 0x92),
    focus: INK,
    selection: tint(GOLD, 128),
};

/// CMR1's widget sizes: the look cards' roomier padding and note (the
/// values its own widgets drew).
pub const METRICS: Metrics = Metrics {
    card: CardMetrics {
        pad: 14.0,
        note: 13.5,
        note_top: 32.0,
        ..Metrics::DEFAULT.card
    },
    ..Metrics::DEFAULT
};

/// The settings window's design size in points (taller than 0.7's 640:
/// every row now carries its one-line description).
pub const SIZE: Vec2 = vec2(880.0, 704.0);

/// The width of the portrait strip on the left.
pub const PHOTO_W: f32 = 300.0;

/// The two slanted gold livery stripes in the top right corner.
pub fn stripes() -> [(f32, Color32); 2] {
    [(30.0, GOLD), (16.0, GOLD.gamma_multiply(0.65))]
}

#[cfg(test)]
mod tests {
    use super::*;
    use rea_launcher_ui::window::creation_zoom;

    #[test]
    fn the_palette_reads_well() {
        assert_eq!(
            rea_launcher_ui::theme::contrast_problems(&PALETTE, &METRICS),
            Vec::<String>::new()
        );
        assert_eq!(
            rea_launcher_ui::theme::text_size_problems(&METRICS),
            Vec::<String>::new()
        );
    }

    #[test]
    fn under_wine_the_window_is_created_at_its_final_size() {
        // Wine's default 96 DPI: 1.5x at 4K, 1x at 1080p.
        assert_eq!(creation_zoom(SIZE, true, Some((3840, 2160)), 1.0), 1.5);
        assert_eq!(creation_zoom(SIZE, true, Some((1920, 1080)), 1.0), 1.0);
        // Wine at 144 DPI already scales the window: never zoomed on top.
        assert_eq!(creation_zoom(SIZE, true, Some((3840, 2160)), 1.5), 1.0);
        for dpi in [96.0, 120.0, 144.0, 192.0] {
            let native = dpi / 96.0;
            let zoom = creation_zoom(SIZE, true, Some((3840, 2160)), native);
            assert!(SIZE.y * zoom * native <= 2160.0, "{dpi} DPI");
        }
        // The Steam Deck's 800 lines: the window fits.
        assert!(creation_zoom(SIZE, true, Some((1280, 800)), 1.0) * SIZE.y <= 800.0);
    }
}
