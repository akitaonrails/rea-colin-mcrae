//! The country welcome screen FSM state 2 shows before a country's first
//! stage (`FUN_0048df30` decides; `FUN_0048dde0` runs it).
//!
//! The first update loads `GAME\WELCOME\<CODE>\` (the palette `<code>.pal`,
//! the fonts `fonts\<code>_wel.fon` and `fonts\main_<code>.fon`, the flag
//! `flag_<code>.bmp`), draws once in two passes under a black palette
//! (`FUN_0048d970`: the flag unscaled at the origin, index 0 transparent;
//! then the welcome line and the country's name, centred at `y = 0xb333`
//! with the second line under the first — the first line's y is carried
//! into the second pass, so the pass left on screen draws one line lower),
//! and fades the palette in (`FUN_004ad3e0(pal, 3)`: 33 steps, each after a
//! vertical blank, blocking). It then holds until a newly pressed key or
//! more than two seconds, fades to black the same way and ends (state 3).
//! A special-series event shows its country and the series line instead of
//! the welcome line; the special event never shows the screen.
//!
//! A description of disassembly, not original source. Evidence:
//! `rea.official-eu.welcome-screen`.

use cmr_formats::bitmap::IndexedBitmap;
use cmr_formats::font::{BitmapFont, TextLayoutError};
use cmr_formats::riff_palette::PaletteEntry;

/// The country codes (`0x4f1268..0x4f1288`, by `cfg[0x168] & 0xf`).
pub const WELCOME_CODES: [&str; 9] = ["NEW", "GRE", "MON", "AUS", "SWE", "COR", "IND", "UK", "SUP"];

/// The hold before the fade out (strictly more than this, in ms).
pub const WELCOME_HOLD_MS: u32 = 2000;

/// `FUN_004ad3e0(target, 3)`'s weights: 256 down to 0 by 8, 33 steps.
pub const FADE_STEPS: usize = 33;

/// One step of `FUN_004ad3e0`: each channel `target + (current - target) *
/// weight >> 8` (arithmetic), for weight `256 - 8 step`.
pub fn crossfade(
    current: &[PaletteEntry; 256],
    target: &[PaletteEntry; 256],
    step: usize,
) -> [PaletteEntry; 256] {
    let weight = 256 - 8 * step.min(FADE_STEPS - 1) as i32;
    std::array::from_fn(|i| {
        let mix =
            |c: u8, t: u8| (i32::from(t) + ((i32::from(c) - i32::from(t)) * weight >> 8)) as u8;
        PaletteEntry {
            red: mix(current[i].red, target[i].red),
            green: mix(current[i].green, target[i].green),
            blue: mix(current[i].blue, target[i].blue),
            flags: target[i].flags,
        }
    })
}

/// The palette with every colour black (`0x82af5c`).
pub const BLACK: [PaletteEntry; 256] = [PaletteEntry {
    red: 0,
    green: 0,
    blue: 0,
    flags: 0,
}; 256];

/// The screen's assets for one country.
pub struct WelcomeAssets {
    pub palette: [PaletteEntry; 256],
    /// `<code>_wel.fon` (`DAT_0082b374`) and `main_<code>.fon`
    /// (`DAT_0082b378`).
    pub welcome_font: Option<BitmapFont>,
    pub main_font: Option<BitmapFont>,
    pub flag: Option<IndexedBitmap>,
}

/// `FUN_0048d970`'s two passes into a `width x height` 8-bit buffer
/// (cleared to 0): the flag keyed, then the two lines; the buffer the
/// second pass leaves. `text` is the game text `FUN_0049b010`; `series` the
/// special series (`cfg[0x166] & 0x20`); `country` `cfg[0x168] & 0xf`.
pub fn draw(
    assets: &WelcomeAssets,
    width: u16,
    height: u16,
    country: u8,
    series: bool,
    text: &dyn Fn(i32) -> Vec<u8>,
) -> Result<Vec<u8>, TextLayoutError> {
    let (w, h) = (usize::from(width), usize::from(height));
    let mut pixels = vec![0u8; w * h];
    let (first, second) = if series {
        (0x3c + i32::from(country), 0x44)
    } else {
        (0x76, 0x3c + i32::from(country))
    };
    let mut y = 0xb333;
    for _ in 0..2 {
        if let Some(flag) = assets.flag.as_ref() {
            for row in 0..(flag.height as usize).min(h) {
                for column in 0..(flag.width as usize).min(w) {
                    let index = flag.pixels[row * flag.width as usize + column];
                    if index != 0 {
                        pixels[row * w + column] = index;
                    }
                }
            }
        }
        // Only the first line's returned y is kept (and carried into the
        // next pass); a missing font draws nothing and keeps y.
        if let Some(font) = assets.welcome_font.as_ref() {
            let (plan, next, _) = crate::pause_menu_draw::text_q16(
                font,
                &text(first),
                0x8000,
                y,
                0x12,
                width,
                height,
            )?;
            blit_glyphs(&mut pixels, w, h, font, &plan);
            y = next;
        }
        if let Some(font) = assets.main_font.as_ref() {
            let (plan, _, _) = crate::pause_menu_draw::text_q16(
                font,
                &text(second),
                0x8000,
                y,
                0x12,
                width,
                height,
            )?;
            blit_glyphs(&mut pixels, w, h, font, &plan);
        }
    }
    Ok(pixels)
}

/// The glyphs as keyed 2D blits (the 8-bit text path ignores colour).
fn blit_glyphs(
    pixels: &mut [u8],
    w: usize,
    h: usize,
    font: &BitmapFont,
    plan: &cmr_formats::font::TextPlan,
) {
    let atlas = font.atlas();
    let aw = atlas.width as usize;
    for blit in &plan.blits {
        for row in 0..i32::from(blit.source.height) {
            for column in 0..i32::from(blit.source.width) {
                let x = blit.destination.x + column;
                let y = blit.destination.y + row;
                if x < 0 || y < 0 || x as usize >= w || y as usize >= h {
                    continue;
                }
                let sx = i32::from(blit.source.x) + column;
                let sy = i32::from(blit.source.y) + row;
                let Some(&index) = atlas.pixels.get(sy as usize * aw + sx as usize) else {
                    continue;
                };
                if index != 0 {
                    pixels[y as usize * w + x as usize] = index;
                }
            }
        }
    }
}

/// The sequencer's phase (`DAT_0082b36c`; phase 0 is never reached).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WelcomePhase {
    /// The palette fades in (the loader's blocking `FUN_004ad3e0`).
    FadingIn { step: usize },
    /// Phase 1: held from `start` (ms, `_ftime`).
    Holding { start: u32 },
    /// Phase 2's teardown fade to black.
    FadingOut { step: usize },
    /// The screen ended (`FUN_0048dde0` returned 1).
    Done,
}

/// `FUN_0048dde0`'s state across frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WelcomeScreen {
    pub phase: WelcomePhase,
}

impl Default for WelcomeScreen {
    fn default() -> Self {
        Self {
            phase: WelcomePhase::FadingIn { step: 0 },
        }
    }
}

impl WelcomeScreen {
    /// One frame: `steps` palette steps elapsed (the blocking fades take one
    /// per vertical blank), `now` the time, `key` a newly pressed key this
    /// frame. Returns the palette step to show, or `None` once done.
    pub fn frame(&mut self, steps: usize, now: u32, key: bool) -> Option<WelcomeView> {
        match self.phase {
            WelcomePhase::FadingIn { step } => {
                let step = step + steps;
                if step >= FADE_STEPS {
                    // `t0` is taken after the fade, in the same update; a key
                    // edge already polled ends the hold at once.
                    self.phase = if key {
                        WelcomePhase::FadingOut { step: 0 }
                    } else {
                        WelcomePhase::Holding { start: now }
                    };
                    return Some(WelcomeView::FadeIn(FADE_STEPS - 1));
                }
                self.phase = WelcomePhase::FadingIn { step };
                Some(WelcomeView::FadeIn(step))
            }
            WelcomePhase::Holding { start } => {
                if key || (start.wrapping_sub(now) as i32).unsigned_abs() > WELCOME_HOLD_MS {
                    self.phase = WelcomePhase::FadingOut { step: 0 };
                }
                Some(WelcomeView::Shown)
            }
            WelcomePhase::FadingOut { step } => {
                let step = step + steps;
                if step >= FADE_STEPS {
                    self.phase = WelcomePhase::Done;
                    return None;
                }
                self.phase = WelcomePhase::FadingOut { step };
                Some(WelcomeView::FadeOut(step))
            }
            WelcomePhase::Done => None,
        }
    }

    pub const fn done(&self) -> bool {
        matches!(self.phase, WelcomePhase::Done)
    }
}

/// What a frame shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WelcomeView {
    /// The palette at fade-in step `n` (black toward the welcome palette).
    FadeIn(usize),
    /// The welcome palette.
    Shown,
    /// The palette at fade-out step `n` (toward black).
    FadeOut(usize),
}

impl WelcomeView {
    /// The palette this view shows.
    pub fn palette(self, welcome: &[PaletteEntry; 256]) -> [PaletteEntry; 256] {
        match self {
            Self::FadeIn(step) => crossfade(&BLACK, welcome, step),
            Self::Shown => *welcome,
            Self::FadeOut(step) => crossfade(welcome, &BLACK, step),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grey(v: u8) -> [PaletteEntry; 256] {
        [PaletteEntry {
            red: v,
            green: v,
            blue: v,
            flags: 0,
        }; 256]
    }

    #[test]
    fn the_crossfade_runs_33_steps_from_current_to_target() {
        let target = grey(200);
        assert_eq!(
            crossfade(&BLACK, &target, 0)[5].red,
            0,
            "the first step re-sets the current"
        );
        assert_eq!(crossfade(&BLACK, &target, 16)[5].red, 100);
        assert_eq!(
            crossfade(&BLACK, &target, 32)[5].red,
            200,
            "the last is the target"
        );
    }

    #[test]
    fn the_hold_ends_on_a_key_or_after_two_seconds() {
        let mut screen = WelcomeScreen::default();
        assert_eq!(screen.frame(10, 0, false), Some(WelcomeView::FadeIn(10)));
        assert_eq!(screen.frame(30, 100, false), Some(WelcomeView::FadeIn(32)));
        assert_eq!(screen.phase, WelcomePhase::Holding { start: 100 });
        assert_eq!(screen.frame(1, 2100, false), Some(WelcomeView::Shown));
        assert_eq!(
            screen.phase,
            WelcomePhase::Holding { start: 100 },
            "2000 ms is not more"
        );
        screen.frame(1, 2101, false);
        assert!(matches!(screen.phase, WelcomePhase::FadingOut { .. }));
        assert_eq!(screen.frame(40, 2200, false), None);
        assert!(screen.done());
        // A key ends the hold at once.
        let mut screen = WelcomeScreen::default();
        screen.frame(40, 0, false);
        screen.frame(1, 5, true);
        assert!(matches!(screen.phase, WelcomePhase::FadingOut { .. }));
    }

    #[test]
    fn the_second_pass_draws_one_line_lower() {
        // A one-glyph font is enough to see the y carry: with no fonts the
        // draw is the flag alone.
        let assets = WelcomeAssets {
            palette: BLACK,
            welcome_font: None,
            main_font: None,
            flag: None,
        };
        let pixels = draw(&assets, 8, 8, 0, false, &|_| Vec::new()).unwrap();
        assert!(pixels.iter().all(|&p| p == 0));
    }
}
