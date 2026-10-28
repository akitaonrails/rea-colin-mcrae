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
//! `FUN_004c9370`), the name entries and the stage select. A description of
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
}

fn q(value: i32, fraction: i32) -> i32 {
    ((i64::from(value) * i64::from(fraction)) >> 16) as i32
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
    let mut commands = vec![ScreenCommand::Backdrop {
        x: q(w, input.camera.x) + 0x140,
        y: q(h, input.camera.y) + 0xf0,
        w,
        h,
    }];
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
    }
    commands
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
            },
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
        assert_eq!(commands.len(), 7);
        assert_eq!(state.arrow_tick, 1);
        // The bar starts 31 pixels left of the text at 640 wide.
        let ScreenCommand::Selection { x, h, .. } = commands[2] else {
            unreachable!()
        };
        assert_eq!((x, h), (320 - 19 - 12, 0x21));
    }
}
