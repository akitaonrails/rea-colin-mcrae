//! The service screen's frame (`FUN_00476770`, state 3's render): the
//! backdrop, the header with the time dial, the part icon, then the current
//! menu's draw (`FUN_004aee00`), which also draws its neighbours greyed.
//!
//! The screen runs on the 8-bit display, where `FUN_0049f000` blits and the
//! direct glyph path ignore colour: bitmaps and text are keyed copies drawn
//! in call order, at full intensity whatever the fade. Only the colour
//! fills (`FUN_0049e350`) take a colour, matched to the nearest palette
//! index, so only they fade. The surface is not cleared per frame, so the
//! caller keeps the buffer between frames.
//!
//! A description of disassembly, not original source. Evidence:
//! `rea.official-eu.service-screen-draw`.

use cmr_formats::bitmap::IndexedBitmap;
use cmr_formats::font::{BitmapFont, TextLayoutError};
use cmr_formats::riff_palette::PaletteEntry;

use crate::blit::{BlitDecision, BlitMethod, ColorKey, RectI16, plan_legacy_blit};
use crate::service_screen::{ServiceScreen, key};

/// The part icons' bitmap names (`0x4f60c0`), in slot order.
pub const ICON_NAMES: [&str; 7] = [
    "brakes", "electric", "engine", "gear", "steer", "suspen", "wheel",
];

/// The stage pictures' and plaques' country codes (`0x4f6470..`).
pub const COUNTRY_CODES: [&str; 8] = ["nz", "gr", "mc", "aus", "sw", "co", "in", "uk"];

/// The secret stages' picture names (`47a3c0`) and plaque paths (`47a600`).
const SECRET_PICTURES: [&str; 4] = ["Danger", "MileHigh", "RetSil", "Trout"];
const SECRET_PLAQUES: [&str; 4] = [
    "Danger\\dang",
    "MileHigh\\mile",
    "Retsil\\Retsil",
    "Trout\\Trout",
];

/// The number of dial segments, stage pictures and plaque frames loaded.
pub const DIAL_SEGMENTS: usize = 30;
pub const STAGE_PICTURES: usize = 7;
pub const PLAQUE_FRAMES: usize = 15;

/// `0x47a3c0`: stage picture `i`'s path under `Game\SetRep`.
pub fn picture_path(i: usize, live: &[u8], school: bool) -> Option<String> {
    if school {
        return Some("school\\expert.bmp".to_owned());
    }
    if live[0x166] & 0x40 != 0 {
        return Some(format!(
            "secret\\{}.bmp",
            SECRET_PICTURES.get(i).unwrap_or(&"trout")
        ));
    }
    let cc = COUNTRY_CODES.get(usize::from(live[0x168] & 0xf))?;
    Some(format!("{cc}\\{cc}{}.bmp", i + 1))
}

/// `0x47a600`: plaque frame `i`'s path under `Game\SetRep`.
pub fn plaque_path(i: usize, live: &[u8], school: bool) -> Option<String> {
    if school {
        return Some(format!("uk\\plaque\\uk{:02}.bmp", i + 1));
    }
    if live[0x166] & 0x40 != 0 {
        let name = SECRET_PLAQUES.get(usize::from(live[0x168] >> 4))?;
        return Some(format!("secret\\plaque\\{name}{:02}.bmp", i + 1));
    }
    let cc = COUNTRY_CODES.get(usize::from(live[0x168] & 0xf))?;
    Some(format!("{cc}\\plaque\\{cc}{:02}.bmp", i + 1))
}

/// A loaded bitmap with the load-time remap of its palette onto the live one
/// (`FUN_00499320` at 8 bits: entries 1..255 to the nearest of the live
/// entries 1..255 by L1 RGB distance, first minimum; 0 stays 0).
#[derive(Clone, Debug)]
pub struct Sprite {
    pub bitmap: IndexedBitmap,
    map: [u8; 256],
}

fn remap_table(source: &[[u8; 3]], live: &[PaletteEntry; 256]) -> [u8; 256] {
    std::array::from_fn(|i| {
        if i == 0 {
            return 0;
        }
        let Some(&[r, g, b]) = source.get(i) else {
            return i as u8;
        };
        (1..256)
            .min_by_key(|&j| {
                let c = live[j];
                u16::from(r.abs_diff(c.red))
                    + u16::from(g.abs_diff(c.green))
                    + u16::from(b.abs_diff(c.blue))
            })
            .unwrap_or(0) as u8
    })
}

fn bitmap_rgb(bitmap: &IndexedBitmap) -> Vec<[u8; 3]> {
    bitmap
        .palette
        .iter()
        .map(|e| [e.red, e.green, e.blue])
        .collect()
}

impl Sprite {
    pub fn new(bitmap: IndexedBitmap, live: &[PaletteEntry; 256]) -> Self {
        let map = remap_table(&bitmap_rgb(&bitmap), live);
        Self { bitmap, map }
    }

    fn width(&self) -> i32 {
        self.bitmap.width as i32
    }

    fn height(&self) -> i32 {
        self.bitmap.height as i32
    }
}

/// A loaded font with the same load-time remap of its atlas (`FUN_004972d0`).
#[derive(Clone, Debug)]
pub struct ServiceFont {
    pub font: BitmapFont,
    map: [u8; 256],
}

impl ServiceFont {
    pub fn new(font: BitmapFont, live: &[PaletteEntry; 256]) -> Self {
        let map = remap_table(&bitmap_rgb(font.atlas()), live);
        Self { font, map }
    }
}

/// The font file names (`setrep\fonts\*.fon`), in [`ServiceFonts`] order.
pub const FONT_NAMES: [&str; 11] = [
    "diggreen", "diggrey", "digWG", "digred", "tripwhi", "tripwg", "tripgrey", "tripdgre",
    "tripgre", "tripyel", "tripyg",
];

/// The eleven fonts, in [`FONT_NAMES`] order.
pub type ServiceFonts = [Option<ServiceFont>; 11];

/// The screen's assets (`load_graphics` 0x479e70).
#[derive(Clone, Debug)]
pub struct ServiceAssets {
    /// `ini\setrpal.pal`, the live palette.
    pub palette: [PaletteEntry; 256],
    pub fonts: ServiceFonts,
    pub backdrop: Option<Sprite>,
    /// `dial\%03d.bmp`, 30 segments.
    pub dial: Vec<Option<Sprite>>,
    /// [`ICON_NAMES`] order.
    pub icons: Vec<Option<Sprite>>,
    pub left_arrow: Option<Sprite>,
    pub right_arrow: Option<Sprite>,
    pub damage_bar: Option<Sprite>,
    /// The stage pictures ([`picture_path`]) and plaque frames
    /// ([`plaque_path`]).
    pub pictures: Vec<Option<Sprite>>,
    pub plaques: Vec<Option<Sprite>>,
}

/// The executable's data the draw reads.
#[derive(Clone, Copy, Debug, Default)]
pub struct ServiceDrawTables<'a> {
    /// `0x4f6010`: the dial segments' pixel positions, `{x, y}` bytes.
    pub dial: &'a [u8],
    /// `0x4f604c`: the arrows' bob offsets (14 signed bytes).
    pub bob: &'a [u8],
    /// `0x4f60a0` and `0x4f60a8`: the icon slot by setup and repair item.
    pub setup_icons: &'a [u8],
    pub repair_icons: &'a [u8],
    /// `0x506dd0`: the car names.
    pub car_names: &'a [Vec<u8>],
    /// `0x4f48e0`: the stage lengths (tenths of a kilometre, 9 x 7).
    pub lengths: &'a [u8],
    /// `0x4f4448` (9 x 7) and `0x4f4508` (school, 3 x 5): the surface mix,
    /// three bytes per stage.
    pub surfaces: &'a [u8],
    pub school_surfaces: &'a [u8],
    /// The weather tables (`FUN_0046b0f0`, `46b160`, `46b1b0`).
    pub weather: Option<&'a crate::stage_weather::StageWeatherTables>,
}

/// The state the draw keeps across frames and screens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ServiceDrawState {
    /// `DAT_005fefbc`: the dial's count, one step per frame toward the
    /// budget; never reset, so it carries over between screens.
    pub shown_budget: i32,
    /// `DAT_005fef64`: the arrows' bob frame (0..14), and `DAT_005fefc4`
    /// the time it last advanced.
    pub bob: u8,
    pub bob_time: u32,
    /// `DAT_005fefc8`: SETUP.WAV's volume, ramped +100 a frame to 20000.
    pub music_volume: i32,
}

/// The session facts the draw reads besides the live block.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ServiceDrawContext {
    /// `DAT_00d3ca88`.
    pub school: bool,
    /// `DAT_00514478`.
    pub special: bool,
    pub width: u16,
    pub height: u16,
    /// `FUN_00476830`: `seconds * 1000 + milliseconds`.
    pub now_ms: u32,
}

fn qx(v: i32, w: i32) -> i32 {
    v.wrapping_mul(w) >> 16
}

/// `FUN_004ae2f0`: the index nearest an RGB by L1 distance, strict `<`.
pub fn nearest_index(rgb: [u8; 3], palette: &[PaletteEntry; 256]) -> u8 {
    let mut best = (u16::MAX, 0u8);
    for (i, c) in palette.iter().enumerate() {
        let d = u16::from(rgb[0].abs_diff(c.red))
            + u16::from(rgb[1].abs_diff(c.green))
            + u16::from(rgb[2].abs_diff(c.blue));
        if d < best.0 {
            best = (d, i as u8);
        }
    }
    best.1
}

fn scale(rgb: [u8; 3], fade: i32) -> [u8; 3] {
    rgb.map(|c| ((i32::from(c) * fade) >> 16) as u8)
}

/// The back surface and the draw's shared y cursor (`DAT_005fef74`).
struct Painter<'a> {
    pixels: &'a mut [u8],
    w: i32,
    h: i32,
    palette: &'a [PaletteEntry; 256],
    y: i32,
}

impl Painter<'_> {
    /// `FUN_0049e3e0`: the recovered clip, then a keyed (index 0) or opaque
    /// copy; a stretch samples nearest-neighbour (backend policy), and a
    /// clip that inverts the source draws nothing.
    fn blit(&mut self, sprite: &Sprite, src: RectI16, dst: RectI16, keyed: bool) {
        let key = keyed.then_some(ColorKey { low: 0, high: 0 });
        let BlitDecision::Blit(plan) =
            plan_legacy_blit(src, dst, self.w as i16, self.h as i16, key)
        else {
            return;
        };
        let (s, d) = (plan.source, plan.destination);
        let (sw, sh) = (s.right - s.left, s.bottom - s.top);
        let (dw, dh) = (d.right - d.left, d.bottom - d.top);
        if sw <= 0 || sh <= 0 || dw <= 0 || dh <= 0 {
            return;
        }
        let bw = sprite.width();
        for row in 0..dh {
            for column in 0..dw {
                let (sx, sy) = match plan.method {
                    BlitMethod::Fast => (s.left + column, s.top + row),
                    BlitMethod::Stretch => (s.left + column * sw / dw, s.top + row * sh / dh),
                };
                if sx < 0 || sy < 0 || sx >= bw || sy >= sprite.height() {
                    continue;
                }
                let index = sprite.bitmap.pixels[(sy * bw + sx) as usize];
                if keyed && index == 0 {
                    continue;
                }
                let at = ((d.top + row) * self.w + d.left + column) as usize;
                self.pixels[at] = sprite.map[usize::from(index)];
            }
        }
    }

    /// `FUN_0049f000`: a keyed copy at the bitmap's own size, pixel x, y.
    fn sprite(&mut self, sprite: Option<&Sprite>, x: i32, y: i32) {
        let Some(sprite) = sprite else {
            return;
        };
        let (w, h) = (sprite.width() as i16, sprite.height() as i16);
        let src = RectI16 {
            x: 0,
            y: 0,
            width: w,
            height: h,
        };
        let dst = RectI16 {
            x: x as i16,
            y: y as i16,
            width: w,
            height: h,
        };
        self.blit(sprite, src, dst, true);
    }

    /// `FUN_0049e350`: a colour fill of the nearest index. The rectangle is
    /// not clipped; one leaving the surface is taken to fail the Blt and
    /// draw nothing (inference: DirectDraw without a clipper).
    fn fill(&mut self, x: i32, y: i32, w: i32, h: i32, rgb: [u8; 3]) {
        if w <= 0 || h <= 0 || x < 0 || y < 0 || x + w > self.w || y + h > self.h {
            return;
        }
        let index = nearest_index(rgb, self.palette);
        for row in y..y + h {
            let start = (row * self.w + x) as usize;
            self.pixels[start..start + w as usize].fill(index);
        }
    }

    /// `FUN_00497b40` on the direct glyph path: the text at a Q16 position;
    /// returns the Q16 y after the last line and the widest line in pixels
    /// (`DAT_00a20068`). A missing font draws nothing and keeps y.
    fn text(
        &mut self,
        font: Option<&ServiceFont>,
        text: &[u8],
        x: i32,
        y: i32,
        flags: u8,
    ) -> Result<(i32, i32), TextLayoutError> {
        let Some(font) = font else {
            return Ok((y, 0));
        };
        let (plan, next, _) = crate::pause_menu_draw::text_q16(
            &font.font,
            text,
            x,
            y,
            flags,
            self.w as u16,
            self.h as u16,
        )?;
        let atlas = font.font.atlas();
        let aw = atlas.width as i32;
        for blit in &plan.blits {
            for row in 0..i32::from(blit.source.height) {
                for column in 0..i32::from(blit.source.width) {
                    let (dx, dy) = (blit.destination.x + column, blit.destination.y + row);
                    if dx < 0 || dy < 0 || dx >= self.w || dy >= self.h {
                        continue;
                    }
                    let sx = i32::from(blit.source.x) + column;
                    let sy = i32::from(blit.source.y) + row;
                    let Some(&index) = atlas.pixels.get((sy * aw + sx) as usize) else {
                        continue;
                    };
                    if index != 0 {
                        self.pixels[(dy * self.w + dx) as usize] = font.map[usize::from(index)];
                    }
                }
            }
        }
        let widest = plan
            .lines
            .iter()
            .map(|line| line.measured_width)
            .max()
            .unwrap_or(0);
        Ok((next, widest))
    }
}

/// The inputs of one frame.
pub struct ServiceFrame<'a> {
    pub assets: &'a ServiceAssets,
    pub tables: ServiceDrawTables<'a>,
    pub context: ServiceDrawContext,
    /// The game text `FUN_0049b010`.
    pub text: &'a dyn Fn(i32) -> Vec<u8>,
    /// `FUN_004ba470(country, stage)`: the stage's name.
    pub stage_name: &'a dyn Fn(u8, u8) -> Vec<u8>,
}

/// The menus' draw callbacks (`MenuItem::draw`).
mod draw_id {
    pub const SETUP_ITEM: u32 = 0x478380;
    pub const REPAIR_ITEM: u32 = 0x4788e0;
    pub const BOTTOM_ITEM: u32 = 0x478cc0;
    pub const CATEGORY_PAGE: u32 = 0x478d50;
    pub const VIEW_STAGES: u32 = 0x4790a0;
}

/// Which font a draw asks for: its index in [`FONT_NAMES`].
#[derive(Clone, Copy)]
enum F {
    DigGreen,
    DigGrey,
    DigWg,
    DigRed,
    TripWhi,
    TripWg,
    TripGrey,
    TripDgre,
    TripGre,
    TripYel,
    TripYg,
}

struct Draw<'a, 'b> {
    frame: &'b ServiceFrame<'a>,
    screen: &'b mut ServiceScreen,
    live: &'b [u8],
    painter: Painter<'b>,
}

/// `FUN_00476770`: one frame into `pixels` (the back surface, `width *
/// height` indices, kept from the last frame). Returns nothing when the
/// screen has finished (the finish frame draws nothing).
pub fn draw_service_frame(
    frame: &ServiceFrame,
    state: &mut ServiceDrawState,
    screen: &mut ServiceScreen,
    live: &[u8],
    pixels: &mut [u8],
) -> Result<(), TextLayoutError> {
    if screen.finished() {
        state.music_volume = 0;
        return Ok(());
    }
    if state.music_volume < 20000 {
        state.music_volume += 100;
    }
    let mut draw = Draw {
        frame,
        live,
        painter: Painter {
            pixels,
            w: i32::from(frame.context.width),
            h: i32::from(frame.context.height),
            palette: &frame.assets.palette,
            y: 0,
        },
        screen,
    };
    draw.backdrop();
    draw.header(state)?;
    draw.part_icon();
    let current = draw.screen.current;
    draw.menu(current, state.bob)?;
    let t = frame.context.now_ms;
    if (t.wrapping_sub(state.bob_time) as i32).unsigned_abs() > 15 {
        state.bob_time = t;
        state.bob = (state.bob + 1) % 14;
    }
    Ok(())
}

impl Draw<'_, '_> {
    fn text(&mut self, f: F, s: &[u8], x: i32, y: i32, flags: u8) -> Result<i32, TextLayoutError> {
        self.text_measured(f, s, x, y, flags).map(|(next, _)| next)
    }

    fn text_measured(
        &mut self,
        f: F,
        s: &[u8],
        x: i32,
        y: i32,
        flags: u8,
    ) -> Result<(i32, i32), TextLayoutError> {
        let font = self.frame.assets.fonts[f as usize].as_ref();
        self.painter.text(font, s, x, y, flags)
    }
}

impl Draw<'_, '_> {
    fn t(&self, id: i32) -> Vec<u8> {
        (self.frame.text)(id)
    }

    fn w(&self) -> i32 {
        self.painter.w
    }

    fn h(&self) -> i32 {
        self.painter.h
    }

    fn player(&self) -> usize {
        usize::from(self.screen.player) * 0x9c
    }

    fn car_name(&self) -> Vec<u8> {
        let car = usize::from(self.live[self.player() + 0xb6] & 0xf);
        self.frame
            .tables
            .car_names
            .get(car)
            .cloned()
            .unwrap_or_default()
    }

    fn joined(a: &[u8], b: &[u8]) -> Vec<u8> {
        let mut s = a.to_vec();
        s.extend_from_slice(b": ");
        s.extend_from_slice(b);
        s
    }

    /// `FUN_00476ef0`: `BackDrop.bmp` keyed at the origin.
    fn backdrop(&mut self) {
        let sprite = self.frame.assets.backdrop.as_ref();
        self.painter.sprite(sprite, 0, 0);
    }

    /// `FUN_00476a70(budget)`: the dial, the player line, the LCD time and
    /// the title.
    fn header(&mut self, state: &mut ServiceDrawState) -> Result<(), TextLayoutError> {
        let budget = self.screen.budget;
        let mut c = state.shown_budget;
        if budget < c {
            c -= 1;
        } else if budget > c {
            c += 1;
        }
        state.shown_budget = c;
        // `FUN_00476ea0`: (v + 1) / 2 segments at absolute pixels.
        let segments = (c + 1) / 2;
        for i in 0..segments.max(0) as usize {
            let at = self.frame.tables.dial.get(2 * i..2 * i + 2);
            let sprite = self.frame.assets.dial.get(i).and_then(Option::as_ref);
            if let Some(&[x, y]) = at {
                self.painter.sprite(sprite, i32::from(x), i32::from(y));
            }
        }
        let p = self.player();
        let mut line = self.t(0x22);
        line.extend_from_slice(format!(" {}: ", self.screen.player + 1).as_bytes());
        let name = &self.live[p + 0x2c..];
        line.extend(name.iter().take_while(|&&b| b != 0));
        self.text(F::TripYel, &line, 0x4a3d, 0x1ced, 0x11)?;
        let lcd = format!("{c}:00").into_bytes();
        let t23 = self.t(0x23);
        self.text(F::TripYel, &t23, 0xbf3c, 0x1ced, 0x13)?;
        let (tens, ones) = if c >= 10 {
            (lcd[0], lcd[1])
        } else {
            (b'0', lcd[0])
        };
        self.text(F::DigRed, &[tens], 0xd3b6, 0x1ced, 0x11)?;
        self.text(F::DigRed, &[ones], 0xd830, 0x1ced, 0x11)?;
        self.text(F::DigRed, b":", 0xdc49, 0x1ced, 0x11)?;
        self.text(F::DigRed, b"00", 0xddf3, 0x1ced, 0x11)?;
        let title = self.title(&lcd);
        self.text(F::TripYel, &title, 0x4a3d, 0x2666, 0x11)?;
        Ok(())
    }

    /// The header title; the school's level 3 formats nothing and draws the
    /// buffer's stale `"%d:00"` of the count.
    fn title(&self, stale: &[u8]) -> Vec<u8> {
        let live = self.live;
        let paren = |a: Vec<u8>, b: Vec<u8>, c: Vec<u8>| {
            let mut s = Self::joined(&a, &b);
            s.extend_from_slice(b" (");
            s.extend(c);
            s.push(b')');
            s
        };
        let country = live[0x168] & 0xf;
        let stage = live[0x168] >> 4;
        if self.frame.context.school {
            let b = live[0x6c0];
            let task = i32::from(b >> 5);
            return match (b >> 3) & 3 {
                0 => paren(self.t(0x2e), self.t(0x2f), self.t(0x33 + task)),
                1 => paren(self.t(0x2e), self.t(0x30), self.t(0x36 + task)),
                2 => paren(self.t(0x2e), self.t(0x31), self.t(0x36 + task)),
                _ => stale.to_vec(),
            };
        }
        if country == 8 {
            if live[0x166] & 0x40 != 0 {
                return paren(
                    self.t(0x24),
                    self.t(0x12f),
                    self.t(0x130 + i32::from(stage)),
                );
            }
            return paren(self.t(0x24), self.t(0x44), self.t(0x44));
        }
        if live[0x168] & 0xf0 == 0x60 {
            return paren(
                self.t(0x24),
                self.t(0x3c + i32::from(country)),
                self.t(0x44),
            );
        }
        let mut s = Self::joined(&self.t(0x24), &self.t(0x3c + i32::from(country)));
        s.extend_from_slice(b" (");
        s.extend(self.t(0x25));
        s.extend_from_slice(format!(" {} - ", stage + 1).as_bytes());
        s.extend((self.frame.stage_name)(country, stage));
        s.push(b')');
        s
    }

    /// `FUN_00476860`: the selected part's icon (the highlight lerp it also
    /// computes is overwritten before use).
    fn part_icon(&mut self) {
        let current = self.screen.current;
        let (slot, yq) =
            if current == self.screen.setup_root || self.screen.categories.contains(&current) {
                let sel = self.screen.set.menu(self.screen.setup_root).selected;
                (self.frame.tables.setup_icons.get(sel), 0x30a3)
            } else if current == self.screen.repair {
                let sel = self.screen.set.menu(self.screen.repair).selected;
                (self.frame.tables.repair_icons.get(sel), 0x8666)
            } else {
                return;
            };
        let Some(&slot) = slot else {
            return;
        };
        let x = (qx(0xbeb8, self.w())) as i16 as i32;
        let y = (((yq + 0x6a7) * self.h()) >> 16) as i16 as i32;
        let sprite = self
            .frame
            .assets
            .icons
            .get(usize::from(slot))
            .and_then(Option::as_ref);
        self.painter.sprite(sprite, x, y);
    }

    /// `FUN_004aee00(m)`: the predraw, then each item's draw.
    fn menu(
        &mut self,
        menu: crate::descriptor_menu::MenuId,
        bob: u8,
    ) -> Result<(), TextLayoutError> {
        let (predraw, count, selected) = {
            let m = self.screen.set.menu(menu);
            (m.on_predraw, m.items.len(), m.selected)
        };
        if let Some(predraw) = predraw {
            match predraw {
                0x478070 => self.setup_predraw(bob)?,
                0x478780 => self.repair_predraw(bob)?,
                0x478c90 if self.screen.current == menu => {
                    self.menu(self.screen.setup_root, bob)?;
                    self.menu(self.screen.repair, bob)?;
                }
                _ => {}
            }
        }
        for i in 0..count {
            let draw = self.screen.set.menu(menu).items[i].draw;
            let sel = i == selected;
            match draw {
                draw_id::SETUP_ITEM => self.setup_item(menu, i, sel)?,
                draw_id::REPAIR_ITEM => self.repair_item(menu, i, sel)?,
                draw_id::BOTTOM_ITEM => self.bottom_item(menu, i, sel)?,
                draw_id::CATEGORY_PAGE => self.category_page(menu, i, bob)?,
                draw_id::VIEW_STAGES => self.view_stages(menu, i, bob)?,
                _ => {}
            }
        }
        Ok(())
    }

    /// `FUN_004781e0(yq)`: the green panel box and the rule under the car
    /// line, fade-scaled every frame.
    fn panel(&mut self, yq: i32) {
        let (w, h) = (self.w(), self.h());
        let rgb = scale([0x18, 0x4b, 0x00], self.screen.fade_level);
        let bw = qx(0xa8f5, w) as i16 as i32;
        let bh = qx(0x4a3d, h) as i16 as i32;
        let x = qx(0x47ae, w);
        let top = (yq * h) >> 16;
        self.painter
            .fill(qx(0x4a3d, w), (((yq + 0x9ba) * h) >> 16) + 3, 0x113, 1, rgb);
        self.painter.fill(x, top, bw, 1, rgb);
        self.painter
            .fill(x, (top + bh - 1) as i16 as i32, bw, 1, rgb);
        self.painter.fill(x, top, 1, bh, rgb);
        self.painter
            .fill((x + bw - 1) as i16 as i32, top, 1, bh, rgb);
    }

    /// `FUN_00478070`: the setup heading (and, when current, the repair
    /// list and the bottom bar greyed, and the panel).
    fn setup_predraw(&mut self, bob: u8) -> Result<(), TextLayoutError> {
        let current = self.screen.current;
        let me = self.screen.setup_root;
        let (fa, fb) = if current == me {
            self.menu(self.screen.repair, bob)?;
            self.menu(self.screen.bottom, bob)?;
            self.panel(0x30a3);
            (F::TripWhi, F::TripDgre)
        } else {
            (F::TripGrey, F::TripGrey)
        };
        let line = Self::joined(&self.t(0x1b), &self.car_name());
        let y = self.text(fa, &line, 0x4a3d, 0x3333, 0x11)? + 0x20c;
        self.heads(fb, y, 0x1e)
    }

    /// The three column heads; the y cursor continues under the last.
    fn heads(&mut self, f: F, y: i32, last: i32) -> Result<(), TextLayoutError> {
        let (a, b, c) = (self.t(0x23), self.t(0x1d), self.t(last));
        self.text(f, &a, 0x4a3d, y, 0x11)?;
        self.text(f, &b, 0x68f5, y, 0x11)?;
        self.painter.y = self.text(f, &c, 0x8a3d, y, 0x11)? + 0x147;
        Ok(())
    }

    /// `FUN_00478670(xq, yq)`: pixel (4, 0) of `DamBar.bmp` stretched to a
    /// 275 x 16 opaque bar (the fill branch is dead).
    fn selection_bar(&mut self, xq: i32, yq: i32) {
        let Some(sprite) = self.frame.assets.damage_bar.as_ref() else {
            return;
        };
        let dst = RectI16 {
            x: qx(xq, self.w()) as i16,
            y: (((self.h() * yq) >> 16) - 1) as i16,
            width: 0x113,
            height: 0x10,
        };
        let src = RectI16 {
            x: 4,
            y: 0,
            width: 1,
            height: 1,
        };
        self.painter.blit(sprite, src, dst, false);
    }

    /// `FUN_00478380`: a setup row: cost, label and value.
    fn setup_item(
        &mut self,
        menu: crate::descriptor_menu::MenuId,
        i: usize,
        sel: bool,
    ) -> Result<(), TextLayoutError> {
        let (item_key, text_id) = {
            let item = &self.screen.set.menu(menu).items[i];
            (item.key, item.text_id)
        };
        let k = item_key - key::CATEGORY;
        let rec = if (0..5).contains(&k) {
            let at = self.player() + 0x90 + 2 * k as usize;
            [self.live[at], self.live[at + 1]]
        } else {
            [0, 0]
        };
        let name = self.value_name(item_key, i32::from(rec[1] & 7));
        let current = self.screen.current;
        let active = sel && current == menu;
        let (fd, ft) = self.row_fonts(menu, active);
        let y = self.painter.y;
        let cost = format!("{}:00", rec[0] & 0x3f).into_bytes();
        self.text(fd, &cost, 0x5d6f, y + 0x111, 0x13)?;
        let label = self.t(text_id);
        self.text(ft, &label, 0x68f5, y, 0x11)?;
        let fv = if (rec[1] >> 3) & 7 != rec[1] & 7 {
            if active {
                F::TripYg
            } else if current == menu {
                F::TripYel
            } else {
                F::TripGrey
            }
        } else {
            ft
        };
        self.painter.y = self.text(fv, &name, 0x8a3d, y, 0x11)?;
        Ok(())
    }

    /// A setup or repair row's cost and label fonts; the selected row of
    /// the current list first draws the selection bar.
    fn row_fonts(&mut self, menu: crate::descriptor_menu::MenuId, active: bool) -> (F, F) {
        if active {
            self.selection_bar(0x4a3d, self.painter.y);
            (F::DigWg, F::TripWg)
        } else if self.screen.current == menu {
            (F::DigGreen, F::TripGre)
        } else {
            (F::DigGrey, F::TripGrey)
        }
    }

    /// `FUN_004785e0(key, v)`: the value's name.
    fn value_name(&self, item_key: i32, v: i32) -> Vec<u8> {
        const BASE: [i32; 5] = [0x91, 0x95, 0x98, 0x9b, 0x9e];
        match usize::try_from(item_key - key::CATEGORY)
            .ok()
            .and_then(|k| BASE.get(k))
        {
            Some(&base) => self.t(base + v),
            None => Vec::new(),
        }
    }

    /// `FUN_00478780`: the repair heading.
    fn repair_predraw(&mut self, bob: u8) -> Result<(), TextLayoutError> {
        let current = self.screen.current;
        let me = self.screen.repair;
        if current == me {
            self.menu(self.screen.setup_root, bob)?;
            self.menu(self.screen.bottom, bob)?;
        }
        let (fa, fb) = if current == me {
            self.panel(0x8666);
            (F::TripWhi, F::TripDgre)
        } else {
            (F::TripGrey, F::TripGrey)
        };
        let line = Self::joined(&self.t(0x1f), &self.car_name());
        let y = self.text(fa, &line, 0x4a3d, 0x88f5, 0x11)? + 0x20c;
        self.heads(fb, y, 0x20)
    }

    /// `FUN_004788e0`: a repair row: cost, condition bar and part name.
    fn repair_item(
        &mut self,
        menu: crate::descriptor_menu::MenuId,
        i: usize,
        sel: bool,
    ) -> Result<(), TextLayoutError> {
        let (item_key, text_id) = {
            let item = &self.screen.set.menu(menu).items[i];
            (item.key, item.text_id)
        };
        let k = item_key - key::PART;
        let a = if (0..5).contains(&k) {
            let at = self.player() + 0x9a + 3 * k as usize;
            [self.live[at], self.live[at + 1], self.live[at + 2]]
        } else {
            [0; 3]
        };
        let current = self.screen.current;
        let (fd, ft) = self.row_fonts(menu, sel && current == menu);
        let y = self.painter.y;
        let cost = format!("{}:00", a[0] & 0x3f).into_bytes();
        self.text(fd, &cost, 0x5d6f, y + 0x111, 0x13)?;
        self.condition_bar(0x8a3d, y, i32::from(a[2] & 0x7f), i32::from(a[1] & 0x7f));
        let label = self.t(text_id);
        self.painter.y = self.text(ft, &label, 0x68f5, y, 0x11)?;
        Ok(())
    }

    /// `FUN_00478ad0`: 35 segments of 2 x 10 pixels at a 3-pixel pitch. In
    /// the fades each use scales its class's colour again (the native
    /// scales the shared local in place).
    fn condition_bar(&mut self, xq: i32, yq: i32, start: i32, now: i32) {
        let on = self.screen.current == self.screen.repair;
        let mut colours: [[u8; 3]; 3] = if on {
            [[0x3f, 0xff, 0x08], [0xff, 0xff, 0x09], [0x0c, 0x25, 0x00]]
        } else {
            [[0x4b, 0x4b, 0x4b], [0x83, 0x83, 0x83], [0x13, 0x13, 0x13]]
        };
        let fading = self.screen.phase == 0 || self.screen.phase == 2;
        let fade = self.screen.fade_level;
        let mut x = qx(xq, self.w());
        let y = ((yq * self.h()) >> 16) + 2;
        for j in 0..35 {
            let t = 100 * j / 35;
            let below = |v: i32| t < v && !(j == 34 && v != 100);
            let class = if below(start) {
                0
            } else if below(now) {
                1
            } else {
                2
            };
            if fading {
                colours[class] = scale(colours[class], fade);
            }
            self.painter.fill(x, y, 2, 10, colours[class]);
            x += 3;
        }
    }

    /// `FUN_00478cc0`: a bottom-bar item (only the font shows on 8 bits).
    fn bottom_item(
        &mut self,
        menu: crate::descriptor_menu::MenuId,
        i: usize,
        sel: bool,
    ) -> Result<(), TextLayoutError> {
        let (item_key, text_id) = {
            let item = &self.screen.set.menu(menu).items[i];
            (item.key, item.text_id)
        };
        let font = if sel && self.screen.current == menu {
            F::TripGre
        } else {
            F::TripDgre
        };
        let (x, flags) = match item_key {
            key::VIEW_STAGES => (0x4a3d, 0x21),
            key::CONTINUE => (0x9ae1, 0x22),
            key::RESET => (0xeb85, 0x23),
            _ => (0, 0x21),
        };
        let label = self.t(text_id);
        self.text(font, &label, x, 0xd999, flags)?;
        Ok(())
    }

    fn bob(&self, bob: u8) -> i32 {
        i32::from(
            self.frame
                .tables
                .bob
                .get(usize::from(bob))
                .copied()
                .unwrap_or(0) as i8,
        )
    }

    /// `FUN_00478d50`: a category page: the repair list and bottom bar
    /// greyed, the panel, the title with the start value, the pending
    /// value's description and its name between the arrows.
    fn category_page(
        &mut self,
        menu: crate::descriptor_menu::MenuId,
        i: usize,
        bob: u8,
    ) -> Result<(), TextLayoutError> {
        let (item_key, value, count) = {
            let item = &self.screen.set.menu(menu).items[i];
            (item.key, item.value.value, item.value.value_count)
        };
        let k = item_key - key::CATEGORY;
        let rec1 = if (0..5).contains(&k) {
            self.live[self.player() + 0x91 + 2 * k as usize]
        } else {
            0
        };
        self.menu(self.screen.repair, bob)?;
        self.menu(self.screen.bottom, bob)?;
        self.panel(0x30a3);
        let line = Self::joined(&self.t(0x1b), &self.car_name());
        let y = self.text(F::TripDgre, &line, 0x4a3d, 0x3333, 0x11)? + 0x20c;
        let heading = Self::joined(
            &self.t(0x25 + item_key),
            &self.value_name(item_key, i32::from((rec1 >> 3) & 7)),
        );
        let y = self.text(F::TripGre, &heading, 0x4a3d, y, 0x11)? + 0x3d7;
        const DESC: [i32; 5] = [0xa3, 0xa7, 0xaa, 0xad, 0xb0];
        let desc = usize::try_from(k)
            .ok()
            .and_then(|k| DESC.get(k))
            .map_or_else(Vec::new, |&base| self.t(base + value));
        let y = self.text(F::TripWhi, &desc, 0x4a3d, y, 0x11)? + 0x51e;
        let name = self.value_name(item_key, value);
        let (_, wv) = self.text_measured(F::TripGre, &name, 0x7d70, y, 0x12)?;
        self.painter.y = y + 0x147;
        let ay = ((y + 0x147) * self.h()) >> 16;
        let cx = qx(0x7d70, self.w());
        let b = self.bob(bob);
        if value > 0 {
            let lw = self
                .frame
                .assets
                .left_arrow
                .as_ref()
                .map_or(0, |s| s.width() as i16 as i32);
            let sprite = self.frame.assets.left_arrow.as_ref();
            self.painter.sprite(sprite, cx - wv / 2 - lw - b - 8, ay);
        }
        if value < count - 1 {
            let sprite = self.frame.assets.right_arrow.as_ref();
            self.painter.sprite(sprite, wv / 2 + b + cx + 8, ay);
        }
        Ok(())
    }

    /// `FUN_004790a0`: the stage view: the picture, the plaque animation,
    /// the arrows and title, the two headings with their rules, and the
    /// typed-out stage information.
    fn view_stages(
        &mut self,
        menu: crate::descriptor_menu::MenuId,
        i: usize,
        bob: u8,
    ) -> Result<(), TextLayoutError> {
        let (value, count) = {
            let item = &self.screen.set.menu(menu).items[i];
            (item.value.value, item.value.value_count)
        };
        let live = self.live;
        let school = self.frame.context.school;
        let base = if school {
            i32::from(live[0x6c0] >> 5)
        } else {
            i32::from(live[0x168] >> 4)
        };
        let idx = base + value;
        let grp = if school {
            (live[0x6c0] >> 3) & 3
        } else {
            live[0x168] & 0xf
        };
        let (w, h) = (self.w(), self.h());
        let picture = usize::try_from(idx)
            .ok()
            .and_then(|i| self.frame.assets.pictures.get(i))
            .and_then(Option::as_ref);
        self.painter.sprite(picture, qx(0x91eb, w), qx(0x3bc6, h));
        let view = &mut self.screen.stage_view;
        view.frame = (view.frame + 1).min(14);
        let plaque = self
            .frame
            .assets
            .plaques
            .get(usize::from(view.frame))
            .and_then(Option::as_ref);
        if plaque.is_some() {
            let (x, y) = if live[0x166] & 0x40 != 0 {
                if idx == 0 {
                    (qx(0xc000, w), qx(0x2b85, h))
                } else {
                    (qx(0xbd70, w), qx(0x2e14, h))
                }
            } else {
                let c = live[0x168] & 0xf;
                let right = c == 1 || c == 5 || (idx == 2 && c == 0) || (idx == 1 && c == 2);
                (qx(if right { 0xbd70 } else { 0x9999 }, w), qx(0x28f5, h))
            };
            self.painter.sprite(plaque, x, y);
        }
        let b = self.bob(bob);
        let ay = qx(0xdc28, h);
        if value > 0 {
            let sprite = self.frame.assets.left_arrow.as_ref();
            let ah = sprite.map_or(0, |s| s.height() as i16 as i32);
            self.painter.sprite(sprite, qx(0x9999, w) - b, ay - ah / 2);
        }
        let title = if school {
            self.t(0x36 + i32::from(live[0x6c0] >> 5))
        } else if idx == 6 {
            self.t(0x44)
        } else {
            let mut s = self.t(0x25);
            s.extend_from_slice(format!(" {}", idx + 1).as_bytes());
            s
        };
        self.text(F::TripGre, &title, 0xc000, 0xdc28, 0x22)?;
        if value < count - 1 {
            let sprite = self.frame.assets.right_arrow.as_ref();
            let ah = sprite.map_or(0, |s| s.height() as i16 as i32);
            self.painter.sprite(sprite, qx(0xe147, w) + b, ay - ah / 2);
        }
        for (id, yq) in [(0xf9, 0x3333), (0x105, 0x8000)] {
            let head = self.t(id);
            let y = self.text(F::TripGrey, &head, 0x4a3d, yq, 0x11)?;
            self.painter.y = y;
            // White, not fade-scaled.
            self.painter
                .fill(qx(0x4a3d, w), (h * y) >> 16, qx(0x4ccc, w), 1, [0xff; 3]);
        }
        let cty = if school { 0 } else { grp };
        self.stage_info(cty, idx)
    }

    /// `FUN_0046b770(cty, stg, 0x4a3d, 0x428f, 0x8f5c, tripgre)`: the
    /// stage's tagline and facts, typed out two characters a frame once the
    /// plaque has played.
    fn stage_info(&mut self, cty: u8, stg: i32) -> Result<(), TextLayoutError> {
        let live = self.live;
        let school = self.frame.context.school;
        let c = live[0x168] & 0xf;
        let secret = live[0x166] & 0x40 != 0;
        let stg_u8 = stg as u8;
        let mut name = self.t(0xb5);
        name.extend_from_slice(b": ");
        name.extend((self.frame.stage_name)(cty, stg_u8));
        let mut length = self.t(0x8e);
        length.extend_from_slice(b": ");
        if !school {
            let at = usize::from(cty) * 7 + stg as usize;
            if let Some(&v) = self.frame.tables.lengths.get(at) {
                length.extend_from_slice(format!("{}.{}KM", v / 10, v % 10).as_bytes());
            }
        }
        let mut weather = self.t(0x8f);
        weather.extend_from_slice(b": ");
        let (kind, detail, lighting) = self.stage_weather(cty, stg);
        if detail == 3 {
            weather.extend(self.t(0x75));
        } else if (1..=5).contains(&kind) {
            weather.extend(self.t(0x6f + kind as i32));
        }
        let mut time = self.t(0x90);
        time.extend_from_slice(b": ");
        const LIGHT: [i32; 8] = [0xb6, 0x6b, 0xb7, 0x6c, 0x6d, 0xb8, 0x6e, 0x6f];
        if (1..=8).contains(&lighting) {
            time.extend(self.t(LIGHT[lighting as usize - 1]));
        }
        let mut surface = self.t(0x12d);
        surface.extend_from_slice(b": ");
        surface.extend(self.surface_mix(cty, stg));
        let tag_id = if school {
            0x140
        } else if c == 8 && secret {
            0x139 + stg
        } else {
            0xfc + i32::from(cty)
        };
        let mut tagline = vec![b'"'];
        tagline.extend(self.t(tag_id));
        tagline.push(b'"');
        // Reveal order: tagline, name, length, weather, time, surface.
        let mut lines = [tagline, name, length, weather, time, surface];
        let view = &mut self.screen.stage_view;
        if view.frame == 14
            && let Some(i) = (0..6).find(|&i| view.reveal[i] < lines[i].len())
        {
            view.reveal[i] += 2;
        }
        for (line, &n) in lines.iter_mut().zip(view.reveal.iter()) {
            line.truncate(n);
        }
        let [tagline, name, length, weather, time, surface] = lines;
        let named = school || !self.secret_route() || (c == 8 && secret);
        self.text(F::TripGre, &tagline, 0x4a3d, 0x428f, 0x11)?;
        let mut y = 0x8f5c;
        if named {
            y = self.text(F::TripGre, &name, 0x4a3d, y, 0x11)?;
        }
        y = self.text(F::TripGre, &length, 0x4a3d, y, 0x11)?;
        y = self.text(F::TripGre, &weather, 0x4a3d, y, 0x11)?;
        y = self.text(F::TripGre, &time, 0x4a3d, y, 0x11)?;
        self.text(F::TripGre, &surface, 0x4a3d, y, 0x11)?;
        Ok(())
    }

    /// `FUN_00485bb0` (reads the current stage, not the browsed one).
    fn secret_route(&self) -> bool {
        let live = self.live;
        if live[0x166] & 0x40 != 0 {
            live[0x168] & 0xf0 != 0x20
        } else {
            live[0x166] & 0x20 != 0 || live[0x168] & 0xf0 == 0x60
        }
    }

    /// `FUN_0046b0f0`, `46b160` and `46b1b0` for a browsed stage: the
    /// weather type, detail and lighting.
    fn stage_weather(&self, cty: u8, stg: i32) -> (u32, u32, u32) {
        let Some(tables) = self.frame.tables.weather else {
            return (0, 0, 0);
        };
        let school = self.frame.context.school;
        let read = |table: &[u32], width: usize| {
            usize::try_from(stg)
                .ok()
                .and_then(|s| table.get(usize::from(cty) * width + s))
                .copied()
                .unwrap_or(0)
        };
        if school {
            return (
                read(&tables.school_type, 5),
                read(&tables.school_detail, 5),
                read(&tables.school_lighting, 5),
            );
        }
        if self.frame.context.special {
            return (2, 1, 4);
        }
        let options = self.live[0x25];
        let kind = if options & 0x80 != 0 {
            1
        } else if options & 0x40 != 0 {
            5
        } else {
            read(&tables.stage_type, 7)
        };
        let lighting = if options & 0x80 != 0 {
            8
        } else {
            read(&tables.stage_lighting, 7)
        };
        (kind, read(&tables.stage_detail, 7), lighting)
    }

    /// `FUN_0046b4e0`: the surface mix, `"%d%% %s^"` per surface, or the one
    /// surface's name for an `0xa_` record.
    fn surface_mix(&self, cty: u8, stg: i32) -> Vec<u8> {
        let school = self.frame.context.school;
        let (table, width) = if school {
            (self.frame.tables.school_surfaces, 5)
        } else {
            (self.frame.tables.surfaces, 7)
        };
        let at = (usize::from(cty) * width + stg.max(0) as usize) * 3;
        let Some(r) = table.get(at..at + 3) else {
            return Vec::new();
        };
        let name = |b: u8| {
            let k = b & 0xf;
            self.t(0x5c
                + if (1..=14).contains(&k) {
                    i32::from(k)
                } else {
                    0
                })
        };
        if r[0] & 0xf0 == 0xa0 {
            return name(r[0]);
        }
        let mut out = Vec::new();
        for &b in r.iter().take_while(|&&b| b & 0xf != 0) {
            out.extend_from_slice(format!("{}% ", i32::from(b >> 4) * 10).as_bytes());
            out.extend(name(b));
            out.push(b'^');
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grey() -> [PaletteEntry; 256] {
        std::array::from_fn(|i| PaletteEntry {
            red: i as u8,
            green: i as u8,
            blue: i as u8,
            flags: 0,
        })
    }

    #[test]
    fn the_fill_colour_is_the_first_nearest_index() {
        let palette = grey();
        assert_eq!(nearest_index([10, 10, 10], &palette), 10);
        let mut dup = palette;
        dup[3] = dup[7];
        assert_eq!(nearest_index([7, 7, 7], &dup), 3, "the first minimum wins");
    }

    #[test]
    fn the_paths_follow_the_country_school_and_secret() {
        let mut live = vec![0u8; 0x6c4];
        live[0x168] = 0x13; // Australia, stage 1
        assert_eq!(picture_path(0, &live, false).unwrap(), "aus\\aus1.bmp");
        assert_eq!(
            plaque_path(0, &live, false).unwrap(),
            "aus\\plaque\\aus01.bmp"
        );
        assert_eq!(picture_path(3, &live, true).unwrap(), "school\\expert.bmp");
        assert_eq!(
            plaque_path(14, &live, true).unwrap(),
            "uk\\plaque\\uk15.bmp"
        );
        live[0x166] = 0x40;
        assert_eq!(picture_path(5, &live, false).unwrap(), "secret\\trout.bmp");
        assert_eq!(
            plaque_path(1, &live, false).unwrap(),
            "secret\\plaque\\MileHigh\\mile02.bmp"
        );
        live[0x166] = 0;
        live[0x168] = 0x08;
        assert_eq!(picture_path(0, &live, false), None, "country 8 has no code");
    }

    #[test]
    fn the_remap_keeps_zero_and_matches_the_live_entries_from_one() {
        let live = grey();
        let source = vec![[5, 5, 5], [0, 0, 0], [200, 200, 200]];
        let map = remap_table(&source, &live);
        assert_eq!(map[0], 0);
        assert_eq!(map[1], 1, "black maps to the nearest of 1..255");
        assert_eq!(map[2], 200);
        assert_eq!(map[3], 3, "entries past the source palette stay");
    }

    fn empty_assets(palette: [PaletteEntry; 256]) -> ServiceAssets {
        ServiceAssets {
            palette,
            fonts: Default::default(),
            backdrop: None,
            dial: Vec::new(),
            icons: Vec::new(),
            left_arrow: None,
            right_arrow: None,
            damage_bar: None,
            pictures: Vec::new(),
            plaques: Vec::new(),
        }
    }

    /// A palette of black, the panel green and the condition-bar greys.
    fn screen_palette() -> [PaletteEntry; 256] {
        let mut palette = [PaletteEntry {
            red: 0,
            green: 0,
            blue: 0,
            flags: 0,
        }; 256];
        for (i, [red, green, blue]) in [[0x18, 0x4b, 0], [0x4b, 0x4b, 0x4b], [0x13, 0x13, 0x13]]
            .into_iter()
            .enumerate()
        {
            palette[i + 1] = PaletteEntry {
                red,
                green,
                blue,
                flags: 0,
            };
        }
        palette
    }

    fn opened(live: &mut [u8]) -> ServiceScreen {
        let mask = [0x15u8; 9];
        ServiceScreen::open(
            live,
            crate::service_screen::ServiceContext::default(),
            crate::service_screen::ServiceTables {
                mask: &mask,
                country_defaults: &[1; 45],
                school_defaults: &[1; 25],
            },
        )
    }

    fn draw_once(
        screen: &mut ServiceScreen,
        state: &mut ServiceDrawState,
        live: &[u8],
        palette: [PaletteEntry; 256],
        now_ms: u32,
    ) -> Vec<u8> {
        let assets = empty_assets(palette);
        let frame = ServiceFrame {
            assets: &assets,
            tables: ServiceDrawTables::default(),
            context: ServiceDrawContext {
                width: 640,
                height: 480,
                now_ms,
                ..ServiceDrawContext::default()
            },
            text: &|_| Vec::new(),
            stage_name: &|_, _| Vec::new(),
        };
        let mut pixels = vec![0u8; 640 * 480];
        draw_service_frame(&frame, state, screen, live, &mut pixels).unwrap();
        pixels
    }

    fn live() -> Vec<u8> {
        let mut live = vec![0u8; 0x6c4];
        live[0x168] = 0x20;
        live[0x167] = 1;
        for i in 0..5 {
            live[0x9a + 3 * i] = 5;
            live[0x9b + 3 * i] = 50;
        }
        live
    }

    #[test]
    fn the_current_setup_list_draws_its_panel_box_and_rule() {
        let mut live = live();
        let mut screen = opened(&mut live);
        let mut state = ServiceDrawState::default();
        let palette = screen_palette();
        // The bottom bar draws no panel.
        screen.fade_level = 0x1_0000;
        let pixels = draw_once(&mut screen, &mut state, &live, palette, 0);
        assert!(pixels.iter().all(|&p| p != 1));
        screen.current = screen.setup_root;
        let pixels = draw_once(&mut screen, &mut state, &live, palette, 0);
        let at = |x: usize, y: usize| pixels[y * 640 + x];
        // Box x 179..600, y 91..229; the rule (185, 112, 275, 1).
        assert_eq!(
            (at(179, 91), at(600, 91), at(179, 229), at(600, 229)),
            (1, 1, 1, 1)
        );
        assert_eq!((at(180, 92), at(601, 91), at(179, 230)), (0, 0, 0));
        assert_eq!((at(185, 112), at(459, 112), at(460, 112)), (1, 1, 0));
        // Faded out, the fill matches black.
        screen.fade_level = 0;
        let pixels = draw_once(&mut screen, &mut state, &live, palette, 0);
        assert_eq!(pixels[91 * 640 + 179], 0);
    }

    #[test]
    fn the_dial_count_steps_once_a_frame_and_the_bob_after_15_ms() {
        let mut live = live();
        let mut screen = opened(&mut live);
        let mut state = ServiceDrawState {
            shown_budget: 57,
            ..ServiceDrawState::default()
        };
        let palette = screen_palette();
        draw_once(&mut screen, &mut state, &live, palette, 15);
        assert_eq!(
            (state.shown_budget, state.bob, state.music_volume),
            (58, 0, 100)
        );
        draw_once(&mut screen, &mut state, &live, palette, 16);
        assert_eq!((state.shown_budget, state.bob), (59, 1));
        screen.budget = 40;
        draw_once(&mut screen, &mut state, &live, palette, 16);
        assert_eq!(state.shown_budget, 58, "back down toward the budget");
    }

    #[test]
    fn the_condition_bar_lights_by_class_and_compounds_the_fade() {
        let mut live = live();
        let mut screen = opened(&mut live);
        let mut state = ServiceDrawState::default();
        let palette = screen_palette();
        // Not current: the 0x4b greys below the start (50), 0x13 above.
        screen.phase = 1;
        screen.fade_level = 0x1_0000;
        let pixels = draw_once(&mut screen, &mut state, &live, palette, 0);
        // The first repair row's bar starts at x 345; segments every 3 px.
        let row: Vec<usize> = (0..480).filter(|&y| pixels[y * 640 + 345] != 0).collect();
        assert!(!row.is_empty(), "a bar is drawn");
        let y = row[0];
        let segment = |j: usize| pixels[y * 640 + 345 + 3 * j];
        assert_eq!(
            (segment(0), segment(17), segment(18), segment(34)),
            (2, 2, 3, 3)
        );
        assert_eq!(pixels[y * 640 + 347], 0, "a one-pixel gap");
        // In a fade each use scales the class again: later segments darken.
        screen.phase = 0;
        screen.fade_level = 0xf000;
        let pixels = draw_once(&mut screen, &mut state, &live, palette, 0);
        let segment = |j: usize| pixels[y * 640 + 345 + 3 * j];
        assert_eq!(segment(0), 2);
        assert_eq!(
            segment(17),
            3,
            "0x4b scaled 18 times by 15/16 (about 0x17) is nearer 0x13"
        );
    }

    #[test]
    fn a_fill_leaving_the_surface_draws_nothing() {
        let palette = grey();
        let mut pixels = vec![0u8; 16];
        let mut painter = Painter {
            pixels: &mut pixels,
            w: 4,
            h: 4,
            palette: &palette,
            y: 0,
        };
        painter.fill(3, 0, 2, 1, [9, 9, 9]);
        painter.fill(1, 1, 2, 2, [9, 9, 9]);
        assert_eq!(pixels, [0, 0, 0, 0, 0, 9, 9, 0, 0, 9, 9, 0, 0, 0, 0, 0]);
    }
}
