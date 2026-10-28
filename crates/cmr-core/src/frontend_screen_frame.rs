//! The draw of the front end's setup screens (`FUN_004c1bc0` while a setup
//! menu is current): the backdrop window at the camera (`FUN_004bac90`), the
//! screen's title predraw (`FUN_004c3bb0`), then each visible item's draw
//! callback through `FUN_004c3c50`: the selection bar under the selected
//! row (`images\select\select.bmp`), the row's value text, and the two value
//! arrows beside a selected value row, oscillating by the static table
//! `0x508818`.
//!
//! The front end runs on the 8-bit display, so text and bitmaps take the
//! direct path: palette-index blits in issue order (glyph colours do not
//! apply). Value rows draw only their value (the car's full name, the gear,
//! the players' mode, the country), never the item label. Not drawn yet: the
//! preview panels (`FUN_004c5450`, `FUN_004c4540`, `FUN_004c9430`,
//! `FUN_004c9370`), the cars in 3D, the stage thumbnails and the flag
//! animations. The name entries (`0x4c6180`) and the stage select
//! (`0x4c8680`) are drawn by [`plan_name_entry_frame`] and
//! [`plan_stage_select_frame`]. A description of
//! disassembly, not original source. Evidence:
//! `rea.official-eu.frontend-setup-draw`.

use cmr_formats::font::{BitmapFont, measure_line};

use crate::descriptor_menu::{ItemKind, Menu};
use crate::display::DisplayExtent;
use crate::font_assets::FontStyle;
use crate::frontend_camera::Camera;
use crate::frontend_setup::{SetupKind, key};

/// One draw operation, in pixels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScreenCommand {
    /// `FUN_004bac90`: the backdrop window at the camera, to `(0, 0)`.
    Backdrop { x: i32, y: i32, w: i32, h: i32 },
    /// The selection bar: `select.bmp` (0, 0, 256, 256) stretched to the
    /// rectangle, colour key 0.
    Selection { x: i32, y: i32, w: i32, h: i32 },
    /// A whole-surface keyed blit (`FUN_0049f000`), or a stretched part of
    /// a bitmap (`FUN_0049e3e0`).
    Bitmap {
        bitmap: crate::frontend_screen_bitmaps::ScreenBitmap,
        source: Option<(i32, i32, i32, i32)>,
        x: i32,
        y: i32,
        size: Option<(i32, i32)>,
        keyed: bool,
    },
    /// Text with the legacy alignment flags.
    Text {
        font: FontStyle,
        x: i32,
        y: i32,
        flags: u8,
        text: Vec<u8>,
    },
}

/// The frame state the draw keeps: the arrow oscillation index
/// `DAT_00d4ce3c`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScreenFrameState {
    pub arrow_tick: u8,
}

/// The executable's strings and tables the rows read.
#[derive(Clone, Copy, Debug, Default)]
pub struct SetupDrawData<'a> {
    /// `PTR[0x506dd0 + car * 4]`: the cars' full names.
    pub car_names: &'a [Vec<u8>],
    /// `0x508850`: 10 x `{u32 picture, u16 name id, u16 description id}`.
    pub country_table: &'a [u8],
    /// `0x508834`/`0x508838`: the left and right arrow strings.
    pub arrows: Option<(&'a [u8], &'a [u8])>,
    /// `0x508818`: 16 signed arrow offsets.
    pub oscillation: &'a [u8],
    /// `PTR[0x506e08 + car * 4]`: the cars' short names.
    pub car_short_names: &'a [Vec<u8>],
    /// `0x508e18`: 14 x `{i32 bhp, i32 mm, i32 drive}`.
    pub car_stats: &'a [u8],
    /// `"%s: %d BHP^%s: %d MM^"` (`0x508f20`) and the locked line for the
    /// third difficulty (`0x508f10`).
    pub stats_format: &'a [u8],
    pub locked_line: &'a [u8],
}

/// What one setup frame draws from.
pub struct SetupDrawInput<'a> {
    pub kind: SetupKind,
    pub menu: &'a Menu,
    /// The screen's car list (`DAT_00508ec0`).
    pub cars: &'a [u8],
    /// The row channels' Q16 positions (`DAT_00d8ffe0`).
    pub channels: &'a [(i32, i32); 7],
    /// The descriptor's item text flags (`+0x48`).
    pub item_flags: u8,
    pub camera: Camera,
    pub extent: DisplayExtent,
    pub data: SetupDrawData<'a>,
    /// The info anchor (`DAT_00d90038/c`) and the descriptor's info flags
    /// (`+0x40`).
    pub info: (i32, i32),
    pub info_flags: u32,
    /// The cmRally.cfg record the previews read.
    pub record: &'a [u8],
}

fn q(value: i32, fraction: i32) -> i32 {
    ((i64::from(value) * i64::from(fraction)) >> 16) as i32
}

/// `FUN_004bac90`: the backdrop window at the camera.
fn backdrop(camera: Camera, w: i32, h: i32) -> ScreenCommand {
    ScreenCommand::Backdrop {
        x: q(w, camera.x) + 0x140,
        y: q(h, camera.y) + 0xf0,
        w,
        h,
    }
}

/// The item's row channel (`DAT_00d8ffe0` index) by key.
fn channel_of(item_key: i32) -> Option<usize> {
    match item_key {
        key::PLAYERS => Some(0),
        key::CAR_1 => Some(1),
        key::GEAR_1 => Some(2),
        key::CAR_2 => Some(3),
        key::GEAR_2 => Some(4),
        key::COUNTRY => Some(5),
        key::CONTINUE => Some(6),
        _ => None,
    }
}

/// The title predraw: championship `0x4c9790` (text 0 at
/// `(0x1c666, -0xf333)`, flags `0x13`), rally `FUN_004c9760` and time trial
/// `FUN_004c97c0` (texts 1 and `0x78` at `(0x19333, 0xf999)`, flags `0x33`).
fn title(kind: SetupKind) -> (i32, (i32, i32), u8) {
    match kind {
        SetupKind::Championship => (0, (0x1c666, -0xf333), 0x13),
        SetupKind::Rally => (1, (0x19333, 0xf999), 0x33),
        SetupKind::TimeTrial => (0x78, (0x19333, 0xf999), 0x33),
    }
}

/// The value text a setup item's draw callback shows.
fn row_text(
    input: &SetupDrawInput,
    item_key: i32,
    value: i32,
    text: &dyn Fn(i32) -> Vec<u8>,
) -> Vec<u8> {
    match item_key {
        // `0x4c9480`: jump table `0x4c96e4`.
        key::PLAYERS => text(match value {
            0 => 0x1b,
            1 => 0x8b,
            2 => 0x8a,
            3 => 0xb1,
            4 => 0xb5,
            _ => 0xb6,
        }),
        key::CAR_1 | key::CAR_2 => input
            .cars
            .get(value.max(0) as usize)
            .and_then(|&car| input.data.car_names.get(usize::from(car)))
            .cloned()
            .unwrap_or_default(),
        key::GEAR_1 | key::GEAR_2 => text(0x1d + value),
        // `FUN_004c45f0`: the name id at `0x508854 + 8c`.
        key::COUNTRY => {
            let at = 8 * value.max(0) as usize + 4;
            input
                .data
                .country_table
                .get(at..at + 2)
                .map_or_else(Vec::new, |id| {
                    text(i32::from(u16::from_le_bytes([id[0], id[1]])))
                })
        }
        _ => text(0x18),
    }
}

/// `FUN_004c1bc0`'s setup frame.
pub fn plan_setup_frame<'f>(
    input: &SetupDrawInput,
    state: &mut ScreenFrameState,
    text: &dyn Fn(i32) -> Vec<u8>,
    fonts: &dyn Fn(FontStyle) -> &'f BitmapFont,
) -> Vec<ScreenCommand> {
    let w = i32::from(input.extent.width);
    let h = i32::from(input.extent.height);
    let (ox, oy) = input.camera.offset();
    let mut commands = vec![backdrop(input.camera, w, h)];
    // `FUN_004c3bb0`: the title, camera-relative, in the menu's title font.
    let (title_id, (tx, ty), title_flags) = title(input.kind);
    commands.push(ScreenCommand::Text {
        font: FontStyle::Title,
        x: q(w, tx.wrapping_add(ox)),
        y: q(h, ty.wrapping_add(oy)),
        flags: title_flags,
        text: text(title_id),
    });
    for (index, item) in input.menu.items.iter().enumerate() {
        if !item.visible {
            continue;
        }
        let Some(channel) = channel_of(item.key) else {
            continue;
        };
        let (x, y) = input.channels[channel];
        let row = row_text(input, item.key, item.value.value, text);
        let selected = index == input.menu.selected;
        row_commands(
            input,
            state,
            (x.wrapping_add(ox), y.wrapping_add(oy)),
            &row,
            selected,
            item,
            fonts,
            &mut commands,
        );
        if selected {
            preview_commands(
                input,
                item.key,
                item.value.value,
                text,
                fonts,
                &mut commands,
            );
        }
    }
    commands
}

/// `FUN_004c41d0(x, y, flags, .., line1, line2)`: the info box
/// `R_INFO.BMP` (0, 0, 0x140, 0x84) at the camera-relative anchor and its two
/// lines in `BL_INFO` and `GR_INFO`, flags `0x11`.
#[allow(clippy::too_many_arguments)]
fn info_box<'f>(
    input: &SetupDrawInput,
    line1: &[u8],
    line2: &[u8],
    fonts: &dyn Fn(FontStyle) -> &'f BitmapFont,
    commands: &mut Vec<ScreenCommand>,
) {
    use crate::frontend_screen_bitmaps::ScreenBitmap;
    let w = i32::from(input.extent.width);
    let h = i32::from(input.extent.height);
    let (ox, oy) = input.camera.offset();
    let x = input.info.0.wrapping_add(ox);
    let y = input.info.1.wrapping_add(oy);
    let right = input.info_flags & 0xf == 1;
    commands.push(ScreenCommand::Bitmap {
        bitmap: ScreenBitmap::InfoRight,
        source: Some((0, 0, 0x140, 0x84)),
        x: q(w, x) + if right { 0x8e } else { 0x69 },
        y: q(h, y) - i32::from(right) * 2,
        size: None,
        keyed: false,
    });
    let base = x
        .wrapping_add(if right { 0x353 } else { 0x4c08 })
        .wrapping_add(0x8147);
    let (command, next) = q16_text(
        fonts,
        FontStyle::BlueInfo,
        line1,
        base,
        y.wrapping_add(0x7ad),
        0x11,
        input.extent,
    );
    commands.push(command);
    let (command, _) = q16_text(
        fonts,
        FontStyle::GreenInfo,
        line2,
        base,
        next.wrapping_add(0x51e),
        0x11,
        input.extent,
    );
    commands.push(command);
}

/// The selected row's preview: the players hint (`FUN_004c9430`) with its
/// picture, the car's stats (`FUN_004c54a0` for the car rows and the gear
/// rows' cars), the country's name and description (`FUN_004c4540`), and
/// Continue's hint (`FUN_004c9370`). The car logos and models, the flags and
/// the chequered flag are not drawn yet.
fn preview_commands<'f>(
    input: &SetupDrawInput,
    item_key: i32,
    value: i32,
    text: &dyn Fn(i32) -> Vec<u8>,
    fonts: &dyn Fn(FontStyle) -> &'f BitmapFont,
    commands: &mut Vec<ScreenCommand>,
) {
    use crate::frontend_screen_bitmaps::ScreenBitmap;
    let record: &[u8] = input.record;
    let full_record = <&[u8; crate::config::LEGACY_CONFIG_SIZE]>::try_from(record).ok();
    let difficulty = record.get(0x20).map_or(0, |b| (b >> 3) & 3);
    match item_key {
        key::PLAYERS => {
            info_box(input, &text(0x1f), &text(0x20), fonts, commands);
            let w = i32::from(input.extent.width);
            let h = i32::from(input.extent.height);
            let (ox, oy) = input.camera.offset();
            commands.push(ScreenCommand::Bitmap {
                bitmap: match value {
                    0 => ScreenBitmap::OnePlayer,
                    4 => ScreenBitmap::Host,
                    5 => ScreenBitmap::Join,
                    _ => ScreenBitmap::TwoPlayer,
                },
                source: None,
                x: q(w, input.info.0.wrapping_add(ox)) + 0x90,
                y: q(h, input.info.1.wrapping_add(oy)),
                size: None,
                keyed: false,
            });
        }
        key::CAR_1 | key::CAR_2 | key::GEAR_1 | key::GEAR_2 => {
            // The gear rows show their car's stats (`DAT_00d8ce8c/90`).
            let car_key = if matches!(item_key, key::CAR_1 | key::GEAR_1) {
                key::CAR_1
            } else {
                key::CAR_2
            };
            let car_index = input
                .menu
                .find(car_key)
                .map_or(0, |index| input.menu.items[index].value.value);
            let Some(&car) = input.cars.get(car_index.max(0) as usize) else {
                return;
            };
            let allowed =
                full_record.is_some_and(|record| crate::frontend_setup::car_allowed(record, car));
            let line2 = if allowed {
                let at = 12 * usize::from(car);
                let stat = |offset: usize| {
                    input
                        .data
                        .car_stats
                        .get(at + offset..at + offset + 4)
                        .map_or(0, |b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                };
                let mut line = crate::c_format::c_format(
                    input.data.stats_format,
                    &[
                        crate::c_format::FormatArg::Bytes(&text(0xa9)),
                        crate::c_format::FormatArg::Int(i64::from(stat(0))),
                        crate::c_format::FormatArg::Bytes(&text(0xaa)),
                        crate::c_format::FormatArg::Int(i64::from(stat(4))),
                    ],
                );
                match stat(8) {
                    0 => line.extend(text(0x73)),
                    1 => line.extend(text(0x74)),
                    2 => line.extend(text(0x75)),
                    _ => {}
                }
                line
            } else {
                match difficulty {
                    0 => text(0x76),
                    1 => text(0x77),
                    _ => input.data.locked_line.to_vec(),
                }
            };
            let short = input
                .data
                .car_short_names
                .get(usize::from(car))
                .cloned()
                .unwrap_or_default();
            info_box(input, &short, &line2, fonts, commands);
        }
        key::COUNTRY => {
            let at = 8 * value.max(0) as usize;
            let id = |offset: usize| {
                input
                    .data
                    .country_table
                    .get(at + offset..at + offset + 2)
                    .map_or(0, |b| i32::from(u16::from_le_bytes([b[0], b[1]])))
            };
            let time_trial = input.kind == SetupKind::TimeTrial;
            let ok = full_record.is_some_and(|record| {
                crate::frontend_setup::country_allowed(record, value as u8, time_trial)
            });
            let line2 = text(if ok { id(6) } else { 0x8c });
            info_box(input, &text(id(4)), &line2, fonts, commands);
        }
        key::CONTINUE => {
            let mode = record.get(0x180).map_or(0, |b| b & 3);
            let d = i32::from(difficulty);
            let line2 = match (mode, difficulty) {
                (0, 0..=2) => text(0xab + d),
                (1, 0..=2) => text(0xae + d),
                _ => text(0x21),
            };
            info_box(input, &text(0x18), &line2, fonts, commands);
        }
        _ => {}
    }
}

/// `FUN_004c3c50` for one row at the camera-relative Q16 anchor `(x, y)`.
#[allow(clippy::too_many_arguments)]
fn row_commands<'f>(
    input: &SetupDrawInput,
    state: &mut ScreenFrameState,
    (x, y): (i32, i32),
    row: &[u8],
    selected: bool,
    item: &crate::descriptor_menu::MenuItem,
    fonts: &dyn Fn(FontStyle) -> &'f BitmapFont,
    commands: &mut Vec<ScreenCommand>,
) {
    let w = i32::from(input.extent.width);
    let h = i32::from(input.extent.height);
    let flags = input.item_flags;
    let mut bar = None;
    if selected {
        let font = fonts(FontStyle::OptionRed);
        let text_width = measure_line(font, row).unwrap_or(0);
        let mut top = q(h, y) - 2;
        let mut left = q(w, x) - q(w, 0x7ae);
        if flags & 0xf == 3 {
            left -= text_width;
        }
        if flags & 0xf0 == 0x30 {
            top -= font.metrics().base_height as i32;
        }
        left -= q(w, 0x51e);
        let width = text_width + q(w, 0xf5c) + q(w, 0xa3d);
        let height = 0x21;
        bar = Some((left, top, width));
        // The bar: clip left/top, clamp right/bottom.
        let (mut bx, mut by, mut bw, mut bh) = (left, top, width, height);
        if bx < 0 {
            bw += bx;
            bx = 0;
        }
        if by < 0 {
            bh += by;
            by = 0;
        }
        if bx + bw >= w {
            bw = w - bx;
        }
        if by + bh >= h {
            bh = h - by;
        }
        commands.push(ScreenCommand::Selection {
            x: bx,
            y: by,
            w: bw,
            h: bh,
        });
    }
    // `FUN_004cf780`: the menu's selected, disabled or enabled font.
    let font = if selected {
        FontStyle::OptionRed
    } else if !item.enabled {
        FontStyle::OptionGrey
    } else {
        FontStyle::Options
    };
    commands.push(ScreenCommand::Text {
        font,
        x: q(w, x),
        y: q(h, y),
        flags,
        text: row.to_vec(),
    });
    // The value arrows.
    if let (Some((left, top, width)), ItemKind::Value) = (bar, item.kind) {
        let osc = input
            .data
            .oscillation
            .get(usize::from(state.arrow_tick))
            .map_or(0, |&byte| i32::from(byte as i8));
        let value = item.value;
        if let Some((left_arrow, right_arrow)) = input.data.arrows {
            if value.wrap || value.value < value.value_count - 1 {
                commands.push(ScreenCommand::Text {
                    font: FontStyle::OptionRed,
                    x: left + width - 0x19 + osc,
                    y: top,
                    flags: 0x11,
                    text: right_arrow.to_vec(),
                });
            }
            if value.wrap || value.value > 0 {
                commands.push(ScreenCommand::Text {
                    font: FontStyle::OptionRed,
                    x: left + 0x19 - osc,
                    y: top,
                    flags: 0x13,
                    text: left_arrow.to_vec(),
                });
            }
        }
        state.arrow_tick = (state.arrow_tick + 1) & 15;
    }
}

/// What one name-entry frame draws from (`0x4c6180`).
pub struct NameDrawInput<'a> {
    /// The parent setup's kind (its title predraw and descriptor).
    pub kind: SetupKind,
    /// The edit buffer.
    pub name: &'a [u8],
    /// `DAT_00d8cee4`.
    pub player: usize,
    pub record: &'a [u8],
    /// The two slides' values (`0xd8ceb8`, `0xd8ceb9`).
    pub slides: [i32; 2],
    /// The parent descriptor's info anchor (`+0x38`, `+0x3c`).
    pub info: (i32, i32),
    pub camera: Camera,
    pub extent: DisplayExtent,
    pub car_names: &'a [Vec<u8>],
    /// `"%s %d"` (`0x4f624c`).
    pub player_format: &'a [u8],
}

fn q16_text<'f>(
    fonts: &dyn Fn(FontStyle) -> &'f BitmapFont,
    font: FontStyle,
    text: &[u8],
    x: i32,
    y: i32,
    flags: u8,
    extent: DisplayExtent,
) -> (ScreenCommand, i32) {
    let w = i32::from(extent.width);
    let h = i32::from(extent.height);
    let px = q(w, x);
    let py = q(h, y);
    let next = crate::pause_menu_draw::text_q16(
        fonts(font),
        text,
        x,
        y,
        flags,
        extent.width,
        extent.height,
    )
    .map_or(y, |(_, next, _)| next);
    (
        ScreenCommand::Text {
            font,
            x: px,
            y: py,
            flags,
            text: text.to_vec(),
        },
        next,
    )
}

/// `0x4c6180`: the name entry's panel, the player line and prompt, the LCD
/// name field, the parent setup's title and the summary.
pub fn plan_name_entry_frame<'f>(
    input: &NameDrawInput,
    text: &dyn Fn(i32) -> Vec<u8>,
    fonts: &dyn Fn(FontStyle) -> &'f BitmapFont,
) -> Vec<ScreenCommand> {
    use crate::frontend_screen_bitmaps::ScreenBitmap;
    let extent = input.extent;
    let w = i32::from(extent.width);
    let h = i32::from(extent.height);
    let base = if input.kind == SetupKind::Championship {
        0x849b
    } else {
        0x8000
    };
    let nx = input.slides[0].wrapping_add(base);
    let mut commands = vec![
        backdrop(input.camera, w, h),
        // `FUN_004c3a30(NX, 0x3333, 2)`.
        ScreenCommand::Bitmap {
            bitmap: ScreenBitmap::NameSmall,
            source: None,
            x: q(w, nx),
            y: q(h, 0x3333),
            size: None,
            keyed: true,
        },
    ];
    let players = input.record.get(0x182).map_or(0, |b| b & 7);
    let mut y = 0x451e;
    if !matches!(players, 0 | 4 | 5) {
        let line = crate::c_format::c_format(
            input.player_format,
            &[
                crate::c_format::FormatArg::Bytes(&text(0x5f)),
                crate::c_format::FormatArg::Int(input.player as i64 + 1),
            ],
        );
        let (command, next) = q16_text(
            fonts,
            FontStyle::BlueDynamic,
            &line,
            nx.wrapping_add(0x3851),
            y,
            0x12,
            extent,
        );
        commands.push(command);
        y = next;
    }
    let (command, _) = q16_text(
        fonts,
        FontStyle::BlueDynamic,
        &text(0x60),
        nx.wrapping_add(0x3851),
        y,
        0x12,
        extent,
    );
    commands.push(command);
    // `FUN_004c37a0(NX + 0x147a, 0x75c2, 0x4ccc, name, 1)`.
    let (fx, fy, fw) = (nx.wrapping_add(0x147a), 0x75c2, 0x4ccc);
    let (px, py, pw) = (q(w, fx), q(h, fy - 0x147), q(w, fw));
    commands.push(ScreenCommand::Bitmap {
        bitmap: ScreenBitmap::LcdLeft,
        source: None,
        x: px,
        y: py,
        size: None,
        keyed: true,
    });
    commands.push(ScreenCommand::Bitmap {
        bitmap: ScreenBitmap::LcdMiddle,
        source: Some((0, 0, 0x11, 0)),
        x: px + 0xe,
        y: py,
        size: Some((pw - 0x1c, 0x22)),
        keyed: true,
    });
    commands.push(ScreenCommand::Bitmap {
        bitmap: ScreenBitmap::LcdRight,
        source: None,
        x: px - 0xe + pw,
        y: py,
        size: None,
        keyed: true,
    });
    let (command, _) = q16_text(
        fonts,
        FontStyle::RedLcd,
        input.name,
        fx + fw / 2,
        fy + 0x28f,
        0x12,
        extent,
    );
    commands.push(command);
    // The parent setup's title, camera-relative.
    let (ox, oy) = input.camera.offset();
    let (title_id, (tx, ty), title_flags) = title(input.kind);
    commands.push(ScreenCommand::Text {
        font: FontStyle::Title,
        x: q(w, tx.wrapping_add(ox)),
        y: q(h, ty.wrapping_add(oy)),
        flags: title_flags,
        text: text(title_id),
    });
    // `FUN_004c4fc0`: the summary at the anchor.
    let mode = input.record.get(0x180).map_or(0, |b| b & 3);
    let anchor_x = input.info.0.wrapping_sub(input.slides[1]);
    let x = anchor_x.wrapping_add(ox).wrapping_add(0xb5c2);
    let y = input.info.1.wrapping_add(oy).wrapping_add(if mode == 0 {
        0x4a3d
    } else if matches!(players, 0 | 4 | 5) {
        -0x3333
    } else {
        -0x3916
    });
    let difficulty = input.record.get(0x20).map_or(0, |b| (b >> 3) & 3);
    let mut line_a = text(match mode {
        0 => 0,
        1 => 1,
        _ => 0x78,
    });
    line_a.push(b'^');
    line_a.extend(text(match difficulty {
        0 => 0x67,
        2 => 0x69,
        _ => 0x68,
    }));
    line_a.push(b'^');
    if mode != 0 {
        line_a.push(b'^');
        line_a.extend(text(match players {
            0 => 0x1b,
            1 => 0x8b,
            2 => 0x8a,
            3 => 0xb1,
            4 => 0xd0,
            _ => 0xd1,
        }));
        line_a.push(b'^');
        let country = input.record.get(0x181).map_or(0, |b| b & 0xf);
        line_a.extend(text(0x22 + i32::from(country)));
        line_a.push(b'^');
    }
    let (command, next) = q16_text(fonts, FontStyle::GreenInfo, &line_a, x, y, 0x13, extent);
    commands.push(command);
    let car_byte = input
        .record
        .get(0xbe + 0x9c * input.player)
        .copied()
        .unwrap_or(0);
    let mut line_b = Vec::new();
    if mode != 0 && !matches!(players, 0 | 4 | 5) {
        line_b = crate::c_format::c_format(
            input.player_format,
            &[
                crate::c_format::FormatArg::Bytes(&text(0x5f)),
                crate::c_format::FormatArg::Int(input.player as i64 + 1),
            ],
        );
        line_b.push(b'^');
    }
    line_b.extend(
        input
            .car_names
            .get(usize::from(car_byte & 0xf))
            .cloned()
            .unwrap_or_default(),
    );
    line_b.push(b'^');
    line_b.extend(text(if car_byte & 0x10 != 0 { 0x1e } else { 0x1d }));
    line_b.push(b'^');
    let (command, _) = q16_text(fonts, FontStyle::GreenInfo, &line_b, x, next, 0x13, extent);
    commands.push(command);
    commands
}

/// What one stage-select frame draws from (`0x4c8680`).
pub struct StageDrawInput<'a> {
    pub menu: &'a Menu,
    pub record: &'a [u8],
    /// The slide `0xd8ffd4` (`SX`).
    pub slide: i32,
    pub camera: Camera,
    pub extent: DisplayExtent,
    /// `PTR[0x509128 + 4i]`: the seven row labels.
    pub labels: &'a [Vec<u8>],
    /// `"%s      %d:%02d.%02d"` (`0x5092d0`).
    pub row_format: &'a [u8],
    /// `0x506dc8`: the bonus countries (country 8's record rows).
    pub bonus_countries: &'a [u8],
}

/// `0x4c8680`: the panel, the time-trial title, the heading, each shown
/// stage's label and best time with its LED, and the OK text and button.
pub fn plan_stage_select_frame<'f>(
    input: &StageDrawInput,
    text: &dyn Fn(i32) -> Vec<u8>,
    fonts: &dyn Fn(FontStyle) -> &'f BitmapFont,
) -> Vec<ScreenCommand> {
    use crate::frontend_screen_bitmaps::ScreenBitmap;
    let extent = input.extent;
    let w = i32::from(extent.width);
    let h = i32::from(extent.height);
    let sx = input.slide;
    let (ox, oy) = input.camera.offset();
    let mut commands = vec![
        backdrop(input.camera, w, h),
        // `FUN_004c3a30(SX + 0x8000, 0xf5c, 0)`.
        ScreenCommand::Bitmap {
            bitmap: ScreenBitmap::Name,
            source: None,
            x: q(w, sx.wrapping_add(0x8000)),
            y: q(h, 0xf5c),
            size: None,
            keyed: true,
        },
    ];
    let (title_id, (tx, ty), title_flags) = title(SetupKind::TimeTrial);
    commands.push(ScreenCommand::Text {
        font: FontStyle::Title,
        x: q(w, tx.wrapping_add(ox)),
        y: q(h, ty.wrapping_add(oy)),
        flags: title_flags,
        text: text(title_id),
    });
    let (command, _) = q16_text(
        fonts,
        FontStyle::BlueDynamic,
        &text(0x2f),
        sx.wrapping_add(0xb851),
        0x1999,
        0x12,
        extent,
    );
    commands.push(command);
    let country = usize::from(input.record.get(0x181).map_or(0, |b| b & 0xf));
    let rows = usize::from(input.record.get(0x16d + 2 * country).map_or(0, |b| b & 0xf));
    let font_of = |index: usize, enabled: bool| {
        if index == input.menu.selected {
            FontStyle::RedDynamic
        } else if !enabled {
            FontStyle::GreyDynamic
        } else {
            FontStyle::BlueDynamic
        }
    };
    for (i, item) in input.menu.items.iter().enumerate().take(rows.min(7)) {
        if !item.visible {
            continue;
        }
        let y = 0x3333 + i as i32 * 0x147a;
        let at = if country == 8 {
            let bonus = usize::from(input.bonus_countries.get(i).copied().unwrap_or(0));
            0x10ca + bonus * 210
        } else {
            0x1016 + (country * 7 + i) * 30
        };
        let time = input
            .record
            .get(at..at + 2)
            .map_or(0, |b| i64::from(u16::from_le_bytes([b[0], b[1]])));
        let label = input.labels.get(i).cloned().unwrap_or_default();
        let row = crate::c_format::c_format(
            input.row_format,
            &[
                crate::c_format::FormatArg::Bytes(&label),
                crate::c_format::FormatArg::Int(time / 6000),
                crate::c_format::FormatArg::Int(time / 100 - 60 * (time / 6000)),
                crate::c_format::FormatArg::Int(time % 100),
            ],
        );
        let (command, _) = q16_text(
            fonts,
            font_of(i, item.enabled),
            &row,
            sx.wrapping_add(0xa666),
            y,
            0x11,
            extent,
        );
        commands.push(command);
        // `FUN_004c3af0(SX + 0x9999, y, 2, value)`.
        commands.push(ScreenCommand::Bitmap {
            bitmap: if item.value.value == 1 {
                ScreenBitmap::LightOn
            } else {
                ScreenBitmap::LightOff
            },
            source: None,
            x: q(w, sx.wrapping_add(0x9999)),
            y: q(h, y) + 1,
            size: None,
            keyed: true,
        });
    }
    let ok_enabled = input
        .menu
        .items
        .get(crate::frontend_stage_select::OK_ITEM)
        .is_some_and(|item| item.enabled);
    let (command, _) = q16_text(
        fonts,
        font_of(crate::frontend_stage_select::OK_ITEM, ok_enabled),
        &text(0x18),
        sx.wrapping_add(0xab85),
        0xc51e,
        0x11,
        extent,
    );
    commands.push(command);
    // `FUN_004c3950(SX + 0xab85, 0xc7ae, sel == 7)`.
    commands.push(ScreenCommand::Bitmap {
        bitmap: if input.menu.selected == crate::frontend_stage_select::OK_ITEM {
            ScreenBitmap::DoneIn
        } else {
            ScreenBitmap::DoneOut
        },
        source: None,
        x: q(w, sx.wrapping_add(0xab85) - 0xe14),
        y: q(h, 0xc7ae - 0x83),
        size: None,
        keyed: true,
    });
    commands
}

/// Why a screen frame could not be executed.
#[derive(Debug)]
pub enum ScreenFrameError<E, F> {
    Bitmap(crate::root_menu_bitmap_raster::RootBitmapCommandError<E>),
    Selection(String),
    Text(String),
    Palette(crate::deferred_palette::DeferredPaletteCommitError<E>),
    Present(F),
}

impl<E: std::fmt::Debug, F: std::fmt::Debug> std::fmt::Display for ScreenFrameError<E, F> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "front-end screen frame failed: {self:?}")
    }
}

/// Execute `commands` in order onto `destination`, then commit the pending
/// palette and present (`FUN_004c4960`, `FUN_00498660`). A backdrop window
/// outside the backdrop draws nothing, as `FUN_004bac90` checks; text the
/// layout cannot place is skipped.
#[allow(clippy::too_many_arguments)]
pub fn execute_screen_frame<B>(
    commands: &[ScreenCommand],
    fonts: &crate::font_raster::UploadedFontPool,
    bitmaps: &crate::root_menu_bitmap_raster::UploadedRootMenuBitmapPool,
    screen_bitmaps: &crate::frontend_screen_bitmaps::ScreenBitmapPool,
    palette: crate::root_menu_frame::RootFramePaletteResources<'_>,
    sources: &mut crate::source_store::UploadedSourceStore<'_, B>,
    destination: B::Surface,
    extent: DisplayExtent,
    limits: cmr_formats::font::TextLayoutLimits,
) -> Result<(), ScreenFrameError<<B as crate::display::IndexedVideoBackend>::Error, B::FinalError>>
where
    B: crate::root_menu_frame::RootFrameHooks
        + crate::deferred_palette::DeferredPaletteAdapter<
            Error = <B as crate::display::IndexedVideoBackend>::Error,
        >,
    <B as crate::display::IndexedVideoBackend>::Error: std::fmt::Debug,
{
    use crate::root_menu_bitmap_assets::RootMenuBitmapSlot;
    use crate::root_menu_visual::{Point, Rect, RenderCommand};
    for command in commands {
        match command {
            ScreenCommand::Backdrop { x, y, w, h } => {
                let Some((bw, bh)) = bitmaps.extent(RootMenuBitmapSlot::Backdrop) else {
                    continue;
                };
                let fits = *x >= 0
                    && *y >= 0
                    && i64::from(*x) + i64::from(*w) <= i64::from(bw)
                    && i64::from(*y) + i64::from(*h) <= i64::from(bh);
                if !fits {
                    continue;
                }
                let render = RenderCommand::Backdrop {
                    slot: RootMenuBitmapSlot::Backdrop,
                    source: Rect {
                        x: *x,
                        y: *y,
                        width: *w as u16,
                        height: *h as u16,
                    },
                    destination: Point::new(0, 0),
                };
                crate::root_menu_bitmap_raster::execute_root_bitmap_command(
                    sources,
                    bitmaps,
                    render,
                    destination,
                    extent,
                )
                .map_err(ScreenFrameError::Bitmap)?;
            }
            ScreenCommand::Selection { x, y, w, h } => {
                let (Some(bitmap), Some(source_extent)) = (
                    bitmaps.bitmap(RootMenuBitmapSlot::Selection),
                    bitmaps.extent(RootMenuBitmapSlot::Selection),
                ) else {
                    continue;
                };
                let decision = crate::indexed_bitmap_raster::plan_indexed_bitmap_blit_for_extent(
                    source_extent,
                    crate::indexed_bitmap_raster::IndexedBitmapRect {
                        x: 0,
                        y: 0,
                        width: 0x100,
                        height: 0x100,
                    },
                    crate::indexed_bitmap_raster::IndexedBitmapRect {
                        x: *x,
                        y: *y,
                        width: *w,
                        height: *h,
                    },
                    extent,
                    Some(crate::blit::ColorKey { low: 0, high: 0 }),
                );
                match decision {
                    Ok(crate::blit::BlitDecision::Blit(plan)) => bitmap
                        .blit(sources, destination, plan)
                        .map_err(|error| ScreenFrameError::Selection(format!("{error:?}")))?,
                    Ok(crate::blit::BlitDecision::NoOp) | Err(_) => {}
                }
            }
            ScreenCommand::Bitmap {
                bitmap,
                source,
                x,
                y,
                size,
                keyed,
            } => {
                let Some(uploaded) = screen_bitmaps.get(*bitmap) else {
                    continue;
                };
                let (width, height) = uploaded.extent();
                let (sx, sy, sw, sh) = source.unwrap_or((0, 0, width as i32, height as i32));
                let sh = if sh == 0 { height as i32 } else { sh };
                let (dw, dh) = size.unwrap_or((sw, sh));
                let decision = crate::indexed_bitmap_raster::plan_indexed_bitmap_blit_for_extent(
                    (width, height),
                    crate::indexed_bitmap_raster::IndexedBitmapRect {
                        x: sx,
                        y: sy,
                        width: sw,
                        height: sh,
                    },
                    crate::indexed_bitmap_raster::IndexedBitmapRect {
                        x: *x,
                        y: *y,
                        width: dw,
                        height: dh,
                    },
                    extent,
                    keyed.then_some(crate::blit::ColorKey { low: 0, high: 0 }),
                );
                if let Ok(crate::blit::BlitDecision::Blit(plan)) = decision {
                    uploaded
                        .blit(sources, destination, plan)
                        .map_err(|error| ScreenFrameError::Selection(format!("{error:?}")))?;
                }
            }
            ScreenCommand::Text {
                font,
                x,
                y,
                flags,
                text,
            } => {
                let Ok(prepared) = crate::font_raster::prepare_persistent_text(
                    fonts,
                    text,
                    *font,
                    cmr_formats::font::PointI32 { x: *x, y: *y },
                    *flags,
                    extent,
                    limits,
                ) else {
                    continue;
                };
                crate::font_raster::execute_prepared_persistent_text(
                    sources,
                    prepared,
                    destination,
                )
                .map_err(|error| ScreenFrameError::Text(format!("{error:?}")))?;
            }
        }
    }
    let backend = sources.backend_mut();
    crate::deferred_palette::commit_deferred_palette(
        palette.state,
        palette.bits_per_pixel,
        palette.arena,
        palette.renderer_cache,
        palette.global_cache,
        backend,
    )
    .map_err(ScreenFrameError::Palette)?;
    backend
        .present_root_frame(destination, extent)
        .map_err(ScreenFrameError::Present)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::descriptor_menu::MenuId;
    use crate::frontend_setup::SetupScreen;

    fn font() -> BitmapFont {
        crate::pause_menu_draw::tests::font()
    }

    #[test]
    fn a_championship_frame_draws_title_rows_bar_and_arrows() {
        let font = font();
        let mut screen = SetupScreen::build(SetupKind::Championship, MenuId(0), MenuId(1));
        screen.cars = vec![0, 6];
        screen.menu.selected = 0; // the car row
        screen.menu.items[0].value.value = 1;
        screen.menu.items[0].value.value_count = 2;
        let channels = [(0x8000, 0x4000); 7];
        let names = vec![b"CAR ZERO".to_vec(); 16];
        let mut country = vec![0u8; 80];
        country[4] = 0x22;
        let input = SetupDrawInput {
            kind: SetupKind::Championship,
            menu: &screen.menu,
            cars: &screen.cars,
            channels: &channels,
            item_flags: 0x11,
            camera: Camera::at(0x8000, 0),
            extent: DisplayExtent {
                width: 640,
                height: 480,
            },
            data: SetupDrawData {
                car_names: &names,
                country_table: &country,
                arrows: Some((b"<", b">")),
                oscillation: &[3; 16],
                car_short_names: &names,
                car_stats: &[0; 168],
                stats_format: b"%s: %d BHP^%s: %d MM^",
                locked_line: b"LOCKED",
            },
            info: (0x8000, 0x4000),
            info_flags: 0,
            record: &[],
        };
        let mut state = ScreenFrameState::default();
        let text = |id: i32| format!("T{id:X}").into_bytes();
        let commands = plan_setup_frame(&input, &mut state, &text, &|_| &font);
        assert_eq!(
            commands[0],
            ScreenCommand::Backdrop {
                x: 320 + 320,
                y: 240,
                w: 640,
                h: 480
            }
        );
        // Title, then the car row's bar, text and one arrow (the last car:
        // no right arrow without wrap), then the gear and Continue rows.
        assert!(matches!(
            commands[1],
            ScreenCommand::Text {
                font: FontStyle::Title,
                flags: 0x13,
                ..
            }
        ));
        assert!(matches!(commands[2], ScreenCommand::Selection { .. }));
        assert!(matches!(
            &commands[3],
            ScreenCommand::Text { font: FontStyle::OptionRed, text, .. } if text == b"CAR ZERO"
        ));
        assert!(
            matches!(&commands[4], ScreenCommand::Text { flags: 0x13, text, .. } if text == b"<")
        );
        // The car row's preview: the info box and its two lines; the empty
        // record (difficulty 0) allows no car 0, so the locked line `0x76`.
        assert!(matches!(
            commands[5],
            ScreenCommand::Bitmap {
                bitmap: crate::frontend_screen_bitmaps::ScreenBitmap::InfoRight,
                ..
            }
        ));
        assert!(matches!(&commands[7], ScreenCommand::Text { text, .. } if text == b"T76"));
        assert_eq!(commands.len(), 10);
        assert_eq!(state.arrow_tick, 1);
        // The bar starts 31 pixels left of the text at 640 wide.
        let ScreenCommand::Selection { x, h, .. } = commands[2] else {
            unreachable!()
        };
        assert_eq!((x, h), (320 - 19 - 12, 0x21));
    }

    #[test]
    fn the_name_entry_draws_its_panel_field_and_summary() {
        use crate::frontend_screen_bitmaps::ScreenBitmap;
        let font = font();
        let mut record = vec![0u8; 0x200];
        record[0x180] = 1; // a rally
        record[0x182] = 2; // two players
        record[0xbe] = 0x16; // car 6, manual
        let names = vec![b"CAR".to_vec(); 16];
        let input = NameDrawInput {
            kind: SetupKind::Rally,
            name: b"AB",
            player: 0,
            record: &record,
            slides: [0, 0],
            info: (0x1000, 0x2000),
            camera: Camera::at(0x8000, 0),
            extent: DisplayExtent {
                width: 640,
                height: 480,
            },
            car_names: &names,
            player_format: b"%s %d",
        };
        let text = |id: i32| format!("T{id:X}").into_bytes();
        let commands = plan_name_entry_frame(&input, &text, &|_| &font);
        let bitmaps: Vec<ScreenBitmap> = commands
            .iter()
            .filter_map(|command| match command {
                ScreenCommand::Bitmap { bitmap, .. } => Some(*bitmap),
                _ => None,
            })
            .collect();
        assert_eq!(
            bitmaps,
            [
                ScreenBitmap::NameSmall,
                ScreenBitmap::LcdLeft,
                ScreenBitmap::LcdMiddle,
                ScreenBitmap::LcdRight
            ]
        );
        let texts: Vec<&[u8]> = commands
            .iter()
            .filter_map(|command| match command {
                ScreenCommand::Text { text, .. } => Some(text.as_slice()),
                _ => None,
            })
            .collect();
        // Player line (two players), prompt, the name, the title, summary A, B.
        assert_eq!(texts[0], b"T5F 1");
        assert_eq!(texts[1], b"T60");
        assert_eq!(texts[2], b"AB");
        assert_eq!(texts[3], b"T1");
        assert_eq!(texts[4], b"T1^T67^^T8A^T22^");
        assert_eq!(texts[5], b"T5F 1^CAR^T1E^");
        // The panel sits at NX = 0x8000 for a rally.
        assert!(matches!(
            commands[1],
            ScreenCommand::Bitmap { x: 320, y: 95, .. }
        ));
    }

    #[test]
    fn the_stage_select_draws_rows_leds_and_the_button() {
        use crate::descriptor_menu::MenuId;
        use crate::frontend_screen_bitmaps::ScreenBitmap;
        use crate::frontend_stage_select::StageSelect;
        let font = font();
        let mut record = vec![0u8; 0x1100];
        record[0x181] = 0;
        record[0x16d] = 6;
        record[0x1016..0x1018].copy_from_slice(&6123u16.to_le_bytes());
        let mut select = StageSelect::build(MenuId(0), MenuId(1), MenuId(2));
        select.menu.items[0].value.value = 1;
        select.menu.items[6].visible = false;
        select.menu.selected = crate::frontend_stage_select::OK_ITEM;
        let labels: Vec<Vec<u8>> = (1..=7).map(|i| format!("{i}").into_bytes()).collect();
        let input = StageDrawInput {
            menu: &select.menu,
            record: &record,
            slide: 0,
            camera: Camera::at(0x8000, 0),
            extent: DisplayExtent {
                width: 640,
                height: 480,
            },
            labels: &labels,
            row_format: b"%s %d:%02d.%02d",
            bonus_countries: &[],
        };
        let text = |id: i32| format!("T{id:X}").into_bytes();
        let commands = plan_stage_select_frame(&input, &text, &|_| &font);
        let leds: Vec<ScreenBitmap> = commands
            .iter()
            .filter_map(|command| match command {
                ScreenCommand::Bitmap { bitmap, .. }
                    if matches!(bitmap, ScreenBitmap::LightOn | ScreenBitmap::LightOff) =>
                {
                    Some(*bitmap)
                }
                _ => None,
            })
            .collect();
        assert_eq!(leds.len(), 6, "six rows shown");
        assert_eq!(leds[0], ScreenBitmap::LightOn);
        assert!(commands.iter().any(|command| matches!(
            command,
            ScreenCommand::Text { text, .. } if text == b"1 1:01.23"
        )));
        assert!(matches!(
            commands.last(),
            Some(ScreenCommand::Bitmap {
                bitmap: ScreenBitmap::DoneIn,
                ..
            })
        ));
    }
}
