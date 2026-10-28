//! The pause menu's draw plan (`FUN_00468dd0`).
//!
//! `FUN_004aee00` runs the current menu's title pre-draw (`0x468910`) and the
//! draw callback of every visible item; each line goes through the Q16 text
//! wrapper `FUN_00497b40`, which advances the y cursor `DAT_004f3e34` and
//! widens the text extent `DAT_0057e684`. The panel is then sized to the text
//! ([`crate::overlay_alt::pause_panel_quad`]). Text goes into draw lane 1 and
//! the panel is flushed before that lane, so the panel lies under the text.
//!
//! The label formats are exe literals: `"%s - %s"` (toggles), `"%s - "` plus a
//! value word, and the volume bar `"%s - "` plus five of `O`/`-`. The colour
//! records are title `ff ff 00 ff`, selected `ff ff ff ff`, unselected
//! `80 80 80 ff`; the panel is `00 00 00 80` on an alpha-capable display and
//! `14 14 14 ff` otherwise. A description of disassembly, not original source.
//! Evidence: `rea.official-eu.pause-menu`, `.work/ingame-text-notes.md`.

use cmr_formats::font::{
    BitmapFont, PointI32, TextAlignment, TextLayoutError, TextLayoutLimits, TextPlan, layout_text,
};

use crate::descriptor_menu::{MenuId, MenuItem};
use crate::overlay_alt::{OverlayQuad, pause_panel_quad};
use crate::pause_menu::{PauseMenus, callback};

/// The title colour record `0x5024b8`.
pub const TITLE_COLOR: [u8; 4] = [0xff, 0xff, 0x00, 0xff];
/// The selected-item colour record `0x4f3e38`.
pub const SELECTED_COLOR: [u8; 4] = [0xff, 0xff, 0xff, 0xff];
/// The unselected-item colour record `0x4f3e3c`.
pub const UNSELECTED_COLOR: [u8; 4] = [0x80, 0x80, 0x80, 0xff];
/// The translucent panel `0x5024a4`.
pub const PANEL_TRANSLUCENT: [u8; 4] = [0x00, 0x00, 0x00, 0x80];
/// The opaque panel `0x4eb604`.
pub const PANEL_OPAQUE: [u8; 4] = [0x14, 0x14, 0x14, 0xff];

/// The title's top (`0x5998`, about `0.35`).
const TITLE_Y_Q16: i32 = 0x5998;
/// The gap after the title (`+0x666`) and after each item (`+0x28f`).
const TITLE_GAP_Q16: i32 = 0x666;
const ITEM_GAP_Q16: i32 = 0x28f;
/// Horizontally centred, top-aligned.
const CENTRED: u8 = 0x12;

/// The three pause fonts (`FUN_00436a50`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PauseFont {
    /// `DAT_00550800`: `title.fon`.
    Title,
    /// `DAT_005507f8`: `result.fon`.
    Result,
    /// `DAT_005507fc`: `resultgr.fon`.
    ResultGrey,
}

impl PauseFont {
    /// The font file for a display width: `fonts\<WxH>\<name>.fon`, with
    /// `title` falling back to 640x480 above 512 wide.
    pub fn path(self, screen_width: u16) -> String {
        let resolution = match screen_width {
            320 => "320x240",
            512 => "512x384",
            640 => "640x480",
            _ if self == Self::Title => "640x480",
            _ => "800x600",
        };
        let name = match self {
            Self::Title => "title",
            Self::Result => "result",
            Self::ResultGrey => "resultgr",
        };
        format!("fonts/{resolution}/{name}.fon")
    }
}

/// One `FUN_00497b40` call: the laid-out glyphs, the font and the colour.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PauseTextRun {
    pub font: PauseFont,
    pub color: [u8; 4],
    pub plan: TextPlan,
}

/// Everything one paused frame draws over the world.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PauseDrawPlan {
    pub panel: OverlayQuad,
    pub panel_color: [u8; 4],
    pub runs: Vec<PauseTextRun>,
}

/// The display facts the draw reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PauseDisplay {
    pub width: u16,
    pub height: u16,
    /// `FUN_00498af0() == 6`: unselected items use `resultgr`.
    pub mode_6: bool,
    /// `FUN_004aa040()`: the alpha-capable panel colour.
    pub alpha_capable: bool,
}

/// `FUN_00497b40`: lay out `text` at a Q16 screen position. Returns the plan,
/// the Q16 y after the last line and the Q16 widest line (`DAT_00a2006c`).
pub fn text_q16(
    font: &BitmapFont,
    text: &[u8],
    x_q16: i32,
    y_q16: i32,
    flags: u8,
    width: u16,
    height: u16,
) -> Result<(TextPlan, i32, i32), TextLayoutError> {
    let w = i32::from(width);
    let h = i32::from(height);
    let px = if x_q16 == -1 {
        -1
    } else {
        x_q16.wrapping_mul(w) >> 16
    };
    let py = h.wrapping_mul(y_q16) >> 16;
    let plan = layout_text(
        font,
        text,
        PointI32 { x: px, y: py },
        w,
        TextAlignment::from_legacy_flags(flags),
        TextLayoutLimits::default(),
    )?;
    let widest = plan
        .lines
        .iter()
        .map(|line| line.measured_width)
        .max()
        .unwrap_or(0);
    let width_q16 = (widest << 16) / w;
    let next_q16 = (plan.next_y << 16) / h;
    Ok((plan, next_q16, width_q16))
}

/// The label an item's draw callback formats.
fn item_label(item: &MenuItem, text: &dyn Fn(i32) -> Vec<u8>) -> Vec<u8> {
    let label = text(item.text_id);
    let value = item.value.value;
    let with = |suffix: Vec<u8>| {
        let mut line = label.clone();
        line.extend_from_slice(b" - ");
        line.extend(suffix);
        line
    };
    match item.draw {
        callback::DRAW_TOGGLE => with(text(if value == 1 { 0xeb } else { 0xec })),
        callback::DRAW_VIEW_TOGGLE => with(text(if value == 1 { 0xf0 } else { 0xf1 })),
        callback::DRAW_THREE_WAY => with(match value {
            0 => text(0xec),
            1 => text(0xed),
            2 => text(0xee),
            _ => Vec::new(),
        }),
        callback::DRAW_BAR => with(if value == 0 {
            text(0xec)
        } else {
            (0..5)
                .map(|i| if i < value { b'O' } else { b'-' })
                .collect()
        }),
        _ => label,
    }
}

/// `FUN_00468dd0`'s menu draw and panel for the current menu. `text` is the
/// string lookup `FUN_0049b010`; `fonts` resolves the three pause fonts.
pub fn pause_draw_plan<'a>(
    menus: &PauseMenus,
    current: MenuId,
    text: &dyn Fn(i32) -> Vec<u8>,
    fonts: &dyn Fn(PauseFont) -> &'a BitmapFont,
    display: PauseDisplay,
) -> Result<PauseDrawPlan, TextLayoutError> {
    let menu = menus.set.menu(current);
    let (w, h) = (display.width, display.height);
    let mut runs = Vec::new();
    let mut cursor = 0;
    let mut widest = 0;
    if menu.on_predraw == Some(callback::TITLE) {
        let title = if menu.title_id < 0 {
            Vec::new()
        } else {
            text(menu.title_id)
        };
        let (plan, next, width) = text_q16(
            fonts(PauseFont::Title),
            &title,
            0x8000,
            TITLE_Y_Q16,
            CENTRED,
            w,
            h,
        )?;
        cursor = next + TITLE_GAP_Q16;
        widest = width;
        runs.push(PauseTextRun {
            font: PauseFont::Title,
            color: TITLE_COLOR,
            plan,
        });
    }
    for (index, item) in menu.items.iter().enumerate() {
        if !item.visible {
            continue;
        }
        let selected = index == menu.selected;
        let font = match (selected, display.mode_6) {
            (true, _) | (false, false) => PauseFont::Result,
            (false, true) => PauseFont::ResultGrey,
        };
        let color = if selected {
            SELECTED_COLOR
        } else {
            UNSELECTED_COLOR
        };
        let label = item_label(item, text);
        let (plan, next, width) = text_q16(fonts(font), &label, 0x8000, cursor, CENTRED, w, h)?;
        cursor = next + ITEM_GAP_Q16;
        widest = widest.max(width);
        runs.push(PauseTextRun { font, color, plan });
    }
    let (panel, _, _) = pause_panel_quad(widest, cursor, w, h);
    Ok(PauseDrawPlan {
        panel,
        panel_color: if display.alpha_capable {
            PANEL_TRANSLUCENT
        } else {
            PANEL_OPAQUE
        },
        runs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pause_menu::{PauseMenuConditions, PauseOptions, build_pause_menus};
    use cmr_formats::font::{FontLimits, parse_legacy_font};

    /// A synthetic FON: an 8x8 8-bit atlas with uppercase glyphs 2x4 each.
    fn font() -> BitmapFont {
        let width = 8u32;
        let height = 8u32;
        let mut file = Vec::new();
        let palette_len = 256 * 4;
        let pixel_offset = 14 + 40 + palette_len;
        let image = (width * height) as usize;
        file.extend_from_slice(b"BM");
        file.extend_from_slice(&((pixel_offset + image) as u32).to_le_bytes());
        file.extend_from_slice(&0u32.to_le_bytes());
        file.extend_from_slice(&(pixel_offset as u32).to_le_bytes());
        file.extend_from_slice(&40u32.to_le_bytes());
        file.extend_from_slice(&(width as i32).to_le_bytes());
        file.extend_from_slice(&(height as i32).to_le_bytes());
        file.extend_from_slice(&1u16.to_le_bytes());
        file.extend_from_slice(&8u16.to_le_bytes());
        file.extend_from_slice(&[0; 24]);
        file.extend(std::iter::repeat_n(0u8, palette_len));
        file.extend(std::iter::repeat_n(1u8, image));
        let glyphs: Vec<u8> = (b'A'..=b'Z')
            .chain(b'-'..=b'-')
            .chain(b'O'..=b'O')
            .collect();
        let mut header = [0u8; 24];
        header[1] = glyphs.len() as u8;
        header[4..8].copy_from_slice(&4i32.to_le_bytes()); // height
        header[8..12].copy_from_slice(&2i32.to_le_bytes()); // space
        header[12..16].copy_from_slice(&1i32.to_le_bytes()); // tracking
        header[16..20].copy_from_slice(&1i32.to_le_bytes()); // gap
        file.extend_from_slice(&header);
        for code in glyphs {
            let mut record = [0u8; 16];
            record[0] = code;
            record[6..8].copy_from_slice(&2i16.to_le_bytes());
            record[8..10].copy_from_slice(&4i16.to_le_bytes());
            file.extend_from_slice(&record);
        }
        parse_legacy_font(&file, FontLimits::default()).expect("synthetic font parses")
    }

    fn strings(id: i32) -> Vec<u8> {
        format!("T{id:X}").into_bytes()
    }

    const DISPLAY: PauseDisplay = PauseDisplay {
        width: 640,
        height: 480,
        mode_6: false,
        alpha_capable: true,
    };

    #[test]
    fn font_paths_follow_the_display_width() {
        assert_eq!(PauseFont::Result.path(640), "fonts/640x480/result.fon");
        assert_eq!(
            PauseFont::ResultGrey.path(800),
            "fonts/800x600/resultgr.fon"
        );
        assert_eq!(PauseFont::Title.path(800), "fonts/640x480/title.fon");
        assert_eq!(PauseFont::Title.path(512), "fonts/512x384/title.fon");
    }

    #[test]
    fn the_wrapper_scales_q16_and_reports_the_extent() {
        let font = font();
        let (plan, next, width) =
            text_q16(&font, b"AB", 0x8000, 0x8000, CENTRED, 640, 480).unwrap();
        assert_eq!(plan.lines[0].origin.y, 240);
        // Width 2+1+2+1 = 6 px, centred at 320.
        assert_eq!(plan.lines[0].measured_width, 6);
        assert_eq!(width, (6 << 16) / 640);
        assert_eq!(next, ((240 + 4 + 1) << 16) / 480);
    }

    #[test]
    fn the_plan_stacks_title_and_items_and_sizes_the_panel() {
        let font = font();
        let menus = build_pause_menus(
            PauseMenuConditions {
                real_play: true,
                ..PauseMenuConditions::default()
            },
            PauseOptions {
                cfg_5: 5,
                cfg_6: 5,
                cfg_8: 0,
                view_toggle_4eb5f0: 0,
                effects_master_4f653c: 100,
                speech_master_4f6538: 100,
            },
        );
        let plan = pause_draw_plan(&menus, menus.root, &strings, &|_| &font, DISPLAY).unwrap();
        assert_eq!(plan.runs.len(), 6, "title plus five items");
        assert_eq!(plan.runs[0].color, TITLE_COLOR);
        assert_eq!(plan.runs[5].color, SELECTED_COLOR, "resume is selected");
        assert_eq!(plan.runs[1].color, UNSELECTED_COLOR);
        let ys: Vec<i32> = plan
            .runs
            .iter()
            .map(|run| run.plan.lines[0].origin.y)
            .collect();
        assert!(ys.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(plan.panel_color, PANEL_TRANSLUCENT);
        assert_eq!(plan.panel.y, ((480 * 19660) >> 16) as u16);
        assert!(u32::from(plan.panel.y) + u32::from(plan.panel.h) > ys[5] as u32);
    }

    #[test]
    fn value_items_format_their_suffixes() {
        let menus = build_pause_menus(
            PauseMenuConditions::default(),
            PauseOptions {
                cfg_5: 3,
                cfg_6: 0,
                cfg_8: 0,
                view_toggle_4eb5f0: 0,
                effects_master_4f653c: 60,
                speech_master_4f6538: 0,
            },
        );
        let sound = menus.set.menu(menus.sound);
        let mut bar = sound.items[1].clone();
        bar.value.value = 3;
        assert_eq!(item_label(&bar, &strings), b"T8 - OOO--".to_vec());
        bar.value.value = 0;
        assert_eq!(item_label(&bar, &strings), b"T8 - TEC".to_vec());
        let settings = menus.set.menu(menus.settings);
        assert_eq!(
            item_label(&settings.items[0], &strings),
            b"TB - TEB".to_vec()
        );
        assert_eq!(
            item_label(&settings.items[1], &strings),
            b"TEA - TEE".to_vec()
        );
        assert_eq!(item_label(&settings.items[2], &strings), b"T9".to_vec());
    }

    #[test]
    fn mode_six_greys_unselected_items_with_resultgr() {
        let font = font();
        let menus = build_pause_menus(
            PauseMenuConditions::default(),
            PauseOptions {
                cfg_5: 5,
                cfg_6: 5,
                cfg_8: 0,
                view_toggle_4eb5f0: 0,
                effects_master_4f653c: 100,
                speech_master_4f6538: 100,
            },
        );
        let plan = pause_draw_plan(
            &menus,
            menus.root,
            &strings,
            &|_| &font,
            PauseDisplay {
                mode_6: true,
                alpha_capable: false,
                ..DISPLAY
            },
        )
        .unwrap();
        assert_eq!(plan.runs[1].font, PauseFont::ResultGrey);
        assert_eq!(plan.runs.last().unwrap().font, PauseFont::Result);
        assert_eq!(plan.panel_color, PANEL_OPAQUE);
    }
}
