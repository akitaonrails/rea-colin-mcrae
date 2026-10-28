//! The post-stage results pages' draw `FUN_0042ae80`, which state 8's
//! render handler runs over the replaying stage while the director is
//! active.
//!
//! Every frame: the display-width nudge `DAT_004ed558`, a translucent panel
//! behind the table pages (index table `0x42b140`; only with an
//! alpha-capable display or display mode 3), then `FUN_0042ad10(0)`, which
//! advances the highlight pulse and draws two opaque black letterbox bars.
//! While the page-change settle runs nothing else is drawn; otherwise the
//! page's draw (jump table `0x42b158`) runs. The layout is `FUN_0042e320`'s
//! Q16 fractions of the display; text goes through the pixel text routine
//! `FUN_004976a0`, or its Q16 wrapper `FUN_00497b40` for the menu.
//!
//! Drawn: the stage results (`FUN_0042b1d0`), the rally results
//! (`FUN_0042b960`), the championship standings (`FUN_0042bdb0`), the
//! post-stage menu (`FUN_0042d5a0`), GAME OVER (`FUN_0042e180`) and the
//! championship and rally complete pages (`FUN_0042df90`, `FUN_0042e0c0`).
//! The confirmations (`FUN_0042e210`: the dialog menu's three lines,
//! `FUN_004aacb0`, then a box sized to the widest) are drawn too, and the
//! save page's dialog (`FUN_0043bc90`), and the record notices and CAC
//! code pages (0xc..=0x11). The super-special result and the
//! bonus car draw the frame only for now. Not drawn: the team and flag icons
//! (`FUN_0042e5c0`) and the time trial's record rows (`FUN_0042b6b0`). The
//! host supplies the stage name (`FUN_004ba470`: the executable's name table
//! `0x506f80`, from the reviewed static tables). The table rows are cut to
//! `0x54f9e8` by `FUN_00497bb0`; that the queued glyph path treats those
//! eight bytes as a clip rectangle is an inference. A description of
//! disassembly, not original source. Evidence:
//! `rea.official-eu.results-draw`.

use cmr_formats::font::{
    BitmapFont, PointI32, TextAlignment, TextLayoutError, TextLayoutLimits, TextPlan, layout_text,
};

use crate::c_format::{FormatArg, c_format};
use crate::event_flow::EventState;
use crate::overlay_alt::OverlayQuad;
use crate::pause_menu_draw::{
    PANEL_OPAQUE, PANEL_TRANSLUCENT, PauseFont, PauseTextRun, SELECTED_COLOR,
    TITLE_COLOR as DIALOG_TITLE_COLOR, UNSELECTED_COLOR, text_q16,
};
use crate::results_director::{ResultsDirector, item};
use crate::standings::last_stage_of_rally;

/// The title colour record `0x4ed55c`.
pub const TITLE_COLOR: [u8; 4] = [0xf0, 0xba, 0x00, 0xff];
/// The body colour record `0x502498` (also `FUN_004976a0`'s default).
pub const BODY_COLOR: [u8; 4] = [0xff, 0xff, 0xff, 0xff];
/// The highlight record `0x4ed560` before the first pulse.
pub const HIGHLIGHT_INITIAL: [u8; 4] = [0xf7, 0xc5, 0x27, 0xff];
/// The pulse stops `0x4ed6cc` (stride 4, the fourth byte unused).
const PULSE_STOPS: [[u8; 3]; 3] = [[0xff, 0xd1, 0x00], [0xf3, 0x54, 0x00], [0xff, 0xd1, 0x00]];
/// The letterbox bars (a stack literal in `FUN_0042ad10`).
const BAR_COLOR: [u8; 4] = [0x00, 0x00, 0x00, 0xff];
/// The menu items' x positions `0x4ed69c[i + count * 3]` (Q16), by count.
const MENU_X_Q16: [&[i32]; 3] = [&[0x8000], &[0x4ccc, 0xb333], &[0x3333, 0x8000, 0xcccc]];
/// The menu row (`0xf332`, Q16 of H), inside the bottom bar.
const MENU_Y_Q16: i32 = 0xf332;

/// The pages with a panel (`0x42b140`): 1, 2, 3, 5, 6, 8 and 12..=21.
fn has_panel(page: u8) -> bool {
    matches!(page, 1..=3 | 5 | 6 | 8 | 12..=21)
}

/// `FUN_0042ad10`'s highlight pulse `DAT_0054f9fc` and the colour it writes
/// to `0x4ed560`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResultsPulse {
    phase: u8,
    color: [u8; 4],
}

impl Default for ResultsPulse {
    fn default() -> Self {
        Self {
            phase: 0,
            color: HIGHLIGHT_INITIAL,
        }
    }
}

impl ResultsPulse {
    /// The colour from the current phase (stop `phase >> 3` towards the next
    /// by `(phase & 7) / 8`, truncating), then the phase steps, wrapping at 16.
    pub fn advance(&mut self) -> [u8; 4] {
        let stop = usize::from(self.phase >> 3);
        let weight = i32::from(self.phase & 7);
        let (from, to) = (PULSE_STOPS[stop], PULSE_STOPS[stop + 1]);
        for (channel, out) in self.color[..3].iter_mut().enumerate() {
            let (from, to) = (i32::from(from[channel]), i32::from(to[channel]));
            *out = (from + (to - from) * weight / 8) as u8;
        }
        self.phase = (self.phase + 1) % 16;
        self.color
    }

    /// The current highlight colour.
    pub const fn color(&self) -> [u8; 4] {
        self.color
    }
}

/// `FUN_0042e320`: the results layout in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResultsLayout {
    pub width: i32,
    pub height: i32,
    /// `0x54f9b0`, `0x54f9b2`: the title.
    pub title_x: i32,
    pub title_y: i32,
    /// `0x54f9b4`: the header row.
    pub header_y: i32,
    /// `0x54f9f4`, `0x54f9c2`, `0x54f9c0`, `0x54f9cc`: the team, position,
    /// name and time columns.
    pub team_x: i32,
    pub position_x: i32,
    pub name_x: i32,
    pub time_x: i32,
    /// `0x54f9d4`: the corner text's right edge.
    pub corner_x: i32,
    /// `0x54f9d0` (`0x54f9d2` is the same): the first row's centre.
    pub row_base: i32,
    /// `0x54f9b6`, `0x54f9b8`, `0x54f9ce`: the championship position, name
    /// and total columns; `0x54f9d6` the first country column and
    /// `0x54f9f2` the country column pitch.
    pub championship_position_x: i32,
    pub championship_name_x: i32,
    pub championship_total_x: i32,
    pub country_x: i32,
    pub country_pitch: i32,
    /// `0x54f9e8`: the rows' clip.
    pub clip: OverlayQuad,
    /// `DAT_004ed558` (`FUN_0042ae80`).
    pub corner_nudge: i32,
}

fn fraction(value: i32, q16: i32) -> i32 {
    value.wrapping_mul(q16) >> 16
}

impl ResultsLayout {
    #[must_use]
    pub fn new(width: u16, height: u16) -> Self {
        let w = i32::from(width);
        let h = i32::from(height);
        let quarter = h << 14 >> 16;
        Self {
            width: w,
            height: h,
            title_x: fraction(w, 0xccc),
            title_y: fraction(h, 0x1c28),
            header_y: fraction(h, 0x35c2),
            team_x: fraction(w, 0x2666),
            position_x: fraction(w, 0x4f5c),
            name_x: fraction(w, 0x75c2),
            time_x: fraction(w, 0xd999),
            corner_x: fraction(w, 0xe666),
            championship_position_x: fraction(w, 0x147a),
            championship_name_x: fraction(w, 0x1eb8),
            championship_total_x: fraction(w, 0xe7ae),
            country_x: fraction(w, 0x5ef9),
            country_pitch: fraction(w, 0x10a3),
            row_base: h.wrapping_mul(37) / 480 / 2 + quarter,
            clip: OverlayQuad {
                x: 0,
                y: quarter as u16,
                w: width,
                h: fraction(h, 0xb0a3) as u16,
            },
            corner_nudge: match width {
                320 => 1,
                512 => -1,
                _ => -3,
            },
        }
    }

    /// The table's row pitch `H * 37 / 480`.
    pub const fn pitch(&self) -> i32 {
        self.height * 37 / 480
    }
}

/// One table row: the placed slot's name, its time and whether it is a
/// player (the order's flag byte bit `0x20`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResultsRow<'a> {
    pub name: &'a [u8],
    pub time: u32,
    pub player: bool,
}

/// One championship row: the placed slot's name, whether it is a player,
/// and its rally points by country (`cfg + 0x170 + 16 slot`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChampionshipRow<'a> {
    pub name: &'a [u8],
    pub player: bool,
    pub points: [u8; 9],
}

impl ChampionshipRow<'_> {
    /// The entry's total byte: the points summed through the current
    /// country, stored as `total << 1 | player` in a byte, drawn `>> 1`.
    fn total(&self, country: usize) -> u8 {
        let sum = self.points[..=country.min(8)]
            .iter()
            .fold(0u8, |sum, &points| sum.wrapping_add(points));
        sum.wrapping_shl(1) >> 1
    }
}

/// What the pages draw from.
pub struct ResultsDrawInput<'a> {
    pub director: &'a ResultsDirector,
    pub event: &'a EventState,
    /// The stage order (`0x54fdcc`) and the rally order (`0x54fe58`).
    pub stage_rows: &'a [ResultsRow<'a>],
    pub rally_rows: &'a [ResultsRow<'a>],
    /// The championship order (`0x54fee1`).
    pub championship_rows: &'a [ChampionshipRow<'a>],
    /// `FUN_004ba470(country, stage)`, when the host has the name table.
    pub stage_name: Option<&'a [u8]>,
    /// `FUN_00430480`'s fraction of the tick, Q16.
    pub scroll_t_q16: i32,
    pub width: u16,
    pub height: u16,
    /// `FUN_004aa040() || FUN_00498af0() == 3`: the panel is drawn.
    pub panel_capable: bool,
    /// `0x506e08`: the cars' short names the save slots' lines show.
    pub car_short_names: &'a [Vec<u8>],
    /// The save slots' line formats.
    pub save_label_formats: crate::save_dialog::SlotLabelFormats<'a>,
}

/// The rectangles and text one results frame draws, in order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResultsDrawPlan {
    pub quads: Vec<(OverlayQuad, [u8; 4])>,
    pub runs: Vec<PauseTextRun>,
}

struct Painter<'f, 'a> {
    fonts: &'f dyn Fn(PauseFont) -> &'a BitmapFont,
    layout: ResultsLayout,
    plan: ResultsDrawPlan,
    clip: Option<OverlayQuad>,
}

impl Painter<'_, '_> {
    /// `FUN_004976a0`: pixel text, wrapped to the display width.
    fn text(
        &mut self,
        x: i32,
        y: i32,
        text: &[u8],
        font: PauseFont,
        color: [u8; 4],
        flags: u8,
    ) -> Result<(), TextLayoutError> {
        let plan = layout_text(
            (self.fonts)(font),
            text,
            PointI32 { x, y },
            self.layout.width,
            TextAlignment::from_legacy_flags(flags),
            TextLayoutLimits::default(),
        )?;
        self.push(font, color, plan);
        Ok(())
    }

    /// `FUN_00497b40`: text at a Q16 position.
    fn text_q16(
        &mut self,
        x_q16: i32,
        y_q16: i32,
        text: &[u8],
        font: PauseFont,
        color: [u8; 4],
        flags: u8,
    ) -> Result<(), TextLayoutError> {
        let (width, height) = (self.layout.width as u16, self.layout.height as u16);
        let (plan, _, _) = text_q16((self.fonts)(font), text, x_q16, y_q16, flags, width, height)?;
        self.push(font, color, plan);
        Ok(())
    }

    fn push(&mut self, font: PauseFont, color: [u8; 4], plan: TextPlan) {
        self.plan.runs.push(PauseTextRun {
            font,
            color,
            plan,
            clip: self.clip,
        });
    }

    fn fill(&mut self, x: i32, y: i32, w: i32, h: i32, color: [u8; 4]) {
        let quad = OverlayQuad {
            x: x as u16,
            y: y as u16,
            w: w as u16,
            h: h as u16,
        };
        self.plan.quads.push((quad, color));
    }
}

/// `"%02d:%02d.%02d"` (`0x4ed6d8`) of a time in hundredths.
fn time_text(time: u32) -> Vec<u8> {
    c_format(
        b"%02d:%02d.%02d",
        &[
            FormatArg::Int(i64::from(time / 6000)),
            FormatArg::Int(i64::from(time / 100 % 60)),
            FormatArg::Int(i64::from(time % 100)),
        ],
    )
}

/// `FUN_0042ae80` for the director's current page. `text` is the string
/// lookup `FUN_0049b010`; `pulse` advances once per frame.
pub fn results_draw_plan<'a>(
    input: &ResultsDrawInput,
    pulse: &mut ResultsPulse,
    text: &dyn Fn(i32) -> Vec<u8>,
    fonts: &dyn Fn(PauseFont) -> &'a BitmapFont,
) -> Result<ResultsDrawPlan, TextLayoutError> {
    let layout = ResultsLayout::new(input.width, input.height);
    let mut painter = Painter {
        fonts,
        layout,
        plan: ResultsDrawPlan::default(),
        clip: None,
    };
    let (w, h) = (layout.width, layout.height);
    let page = input.director.page();
    let top = fraction(h, 0x1c28);
    let bottom = fraction(h, 0xf0a3);
    if has_panel(page) && input.panel_capable {
        painter.fill(0, top, w, fraction(h, 0xd47b) + 1, PANEL_TRANSLUCENT);
    }
    let highlight = pulse.advance();
    painter.fill(0, 0, w, top, BAR_COLOR);
    painter.fill(0, bottom, w, h - bottom, BAR_COLOR);
    if input.director.settle() != 0 {
        return Ok(painter.plan);
    }
    match page {
        1 => stage_results(&mut painter, input, highlight, text)?,
        2 => rally_results(&mut painter, input, highlight, text)?,
        3 => championship(&mut painter, input, highlight, text)?,
        4 => post_stage_menu(&mut painter, input, text)?,
        8 => game_over(&mut painter, input, highlight, text)?,
        5 => save_page(&mut painter, input, text)?,
        9 | 10 => confirmation(&mut painter, input, text)?,
        0x14 => championship_complete(&mut painter, input, highlight, text)?,
        0x15 => rally_complete(&mut painter, input, highlight, text)?,
        0xc..=0x11 => record_page(&mut painter, input, highlight, text)?,
        _ => {}
    }
    Ok(painter.plan)
}

/// The four column headers in the title font.
fn headers(painter: &mut Painter, text: &dyn Fn(i32) -> Vec<u8>) -> Result<(), TextLayoutError> {
    let l = painter.layout;
    for (id, x, flags) in [
        (0x7a, l.team_x, 0x32),
        (0x7b, l.position_x, 0x32),
        (0x22, l.name_x, 0x31),
        (0x23, l.time_x, 0x32),
    ] {
        painter.text(
            x,
            l.header_y,
            &text(id),
            PauseFont::Title,
            BODY_COLOR,
            flags,
        )?;
    }
    Ok(())
}

/// The clipped table: rows `index - 2 < i <= index + 8` at
/// `pitch * i + offset + base`, position, name and time.
fn table(
    painter: &mut Painter,
    input: &ResultsDrawInput,
    rows: &[ResultsRow],
    highlight: [u8; 4],
) -> Result<(), TextLayoutError> {
    let l = painter.layout;
    let scroll = input.director.scroll();
    let offset = i32::from(scroll.drawn(input.scroll_t_q16));
    painter.clip = Some(l.clip);
    for (i, row) in rows
        .iter()
        .enumerate()
        .take(usize::from(input.director.rows()))
    {
        let i = i as i32;
        if i <= scroll.index - 2 || i > scroll.index + 8 {
            continue;
        }
        let y = l.pitch() * i + offset + l.row_base;
        let color = if row.player { highlight } else { BODY_COLOR };
        let position = c_format(b"%d", &[FormatArg::Int(i64::from(i + 1))]);
        painter.text(l.position_x, y, &position, PauseFont::Title, color, 0x22)?;
        painter.text(l.name_x, y, row.name, PauseFont::Result, color, 0x21)?;
        painter.text(
            l.time_x,
            y,
            &time_text(row.time),
            PauseFont::Title,
            color,
            0x22,
        )?;
    }
    painter.clip = None;
    Ok(())
}

/// `FUN_0042b8e0`: a one-pixel rule under the headers, not in a time trial.
fn rule(painter: &mut Painter, input: &ResultsDrawInput) {
    if input.event.cfg.mode_166 & 3 == 2 {
        return;
    }
    let l = painter.layout;
    let x = fraction(l.width, 0xccc);
    painter.fill(x, l.header_y, l.width - 2 * x, 1, BODY_COLOR);
}

/// The corner text (right-aligned at `corner_x - 8`, nudged).
fn corner(painter: &mut Painter, text: &[u8]) -> Result<(), TextLayoutError> {
    let l = painter.layout;
    painter.text(
        l.corner_x - 8,
        l.title_y + l.corner_nudge,
        text,
        PauseFont::Result,
        BODY_COLOR,
        0x33,
    )
}

/// `FUN_0042b1d0`.
fn stage_results(
    painter: &mut Painter,
    input: &ResultsDrawInput,
    highlight: [u8; 4],
    text: &dyn Fn(i32) -> Vec<u8>,
) -> Result<(), TextLayoutError> {
    let l = painter.layout;
    let time_trial = input.event.cfg.mode_166 & 3 == 2;
    let title = text(if time_trial { 0xbe } else { 0xb9 });
    painter.text(
        l.title_x,
        l.title_y,
        &title,
        PauseFont::Title,
        TITLE_COLOR,
        0x31,
    )?;
    if !time_trial {
        headers(painter, text)?;
        table(painter, input, input.stage_rows, highlight)?;
        rule(painter, input);
    }
    let name = if input.event.super_special() {
        Some(text(0x44))
    } else {
        input.stage_name.map(<[u8]>::to_vec)
    };
    if let Some(name) = name {
        corner(painter, &name)?;
    }
    Ok(())
}

/// `FUN_0042b960`.
fn rally_results(
    painter: &mut Painter,
    input: &ResultsDrawInput,
    highlight: [u8; 4],
    text: &dyn Fn(i32) -> Vec<u8>,
) -> Result<(), TextLayoutError> {
    let l = painter.layout;
    headers(painter, text)?;
    table(painter, input, input.rally_rows, highlight)?;
    let cursor = input.event.cfg.cursor_168;
    if last_stage_of_rally(input.event) {
        let country = text(0x3c + i32::from(cursor & 0xf));
        let title = c_format(&text(0xe7), &[FormatArg::Bytes(&country)]);
        painter.text(
            l.title_x,
            l.title_y,
            &title,
            PauseFont::Title,
            TITLE_COLOR,
            0x31,
        )?;
    } else {
        painter.text(
            l.title_x,
            l.title_y,
            &text(0xba),
            PauseFont::Title,
            TITLE_COLOR,
            0x31,
        )?;
        let stage = i64::from(cursor >> 4) + 1;
        corner(painter, &c_format(&text(0xe5), &[FormatArg::Int(stage)]))?;
    }
    rule(painter, input);
    Ok(())
}

/// `FUN_0042bdb0`: position, name, each country's points (6 columns for a
/// novice, 8 otherwise; `"-"` past the current country) and the total.
fn championship(
    painter: &mut Painter,
    input: &ResultsDrawInput,
    highlight: [u8; 4],
    text: &dyn Fn(i32) -> Vec<u8>,
) -> Result<(), TextLayoutError> {
    let l = painter.layout;
    let cfg = &input.event.cfg;
    let difficulty = cfg.difficulty_018 & 0x18 != 0;
    let country = usize::from(cfg.cursor_168 & 0xf);
    let (columns, first_column) = if difficulty {
        (8, l.country_x)
    } else {
        (6, l.country_x + l.country_pitch)
    };
    for (id, x, flags) in [
        (0x7b, l.championship_position_x, 0x32),
        (0x22, l.championship_name_x, 0x31),
        (0x7c, l.championship_total_x, 0x32),
    ] {
        painter.text(
            x,
            l.header_y,
            &text(id),
            PauseFont::Result,
            BODY_COLOR,
            flags,
        )?;
    }
    let scroll = input.director.scroll();
    let offset = i32::from(scroll.drawn(input.scroll_t_q16));
    painter.clip = Some(l.clip);
    for (i, row) in input
        .championship_rows
        .iter()
        .enumerate()
        .take(usize::from(input.director.rows()))
    {
        let i = i as i32;
        if i <= scroll.index - 2 || i > scroll.index + 8 {
            continue;
        }
        let y = l.pitch() * i + offset + l.row_base;
        let color = if row.player { highlight } else { BODY_COLOR };
        let position = c_format(b"%d", &[FormatArg::Int(i64::from(i + 1))]);
        let x = l.championship_position_x;
        painter.text(x, y, &position, PauseFont::Title, color, 0x22)?;
        let x = l.championship_name_x;
        painter.text(x, y, row.name, PauseFont::Result, color, 0x21)?;
        for column in 0..columns {
            let points = if column <= country {
                c_format(b"%02d", &[FormatArg::Int(i64::from(row.points[column]))])
            } else {
                b"-".to_vec()
            };
            let x = l.country_pitch * column as i32 + first_column;
            painter.text(x, y, &points, PauseFont::Result, color, 0x22)?;
        }
        let total = c_format(b"%d", &[FormatArg::Int(i64::from(row.total(country)))]);
        let x = l.championship_total_x;
        painter.text(x, y, &total, PauseFont::Title, color, 0x22)?;
    }
    painter.clip = None;
    let last = if difficulty { 7 } else { 5 };
    if country >= last {
        painter.text(
            l.title_x,
            l.title_y,
            &text(0xe9),
            PauseFont::Title,
            TITLE_COLOR,
            0x31,
        )?;
    } else {
        painter.text(
            l.title_x,
            l.title_y,
            &text(0xe8),
            PauseFont::Title,
            TITLE_COLOR,
            0x31,
        )?;
        let name = text(0x3c + country as i32);
        corner(painter, &c_format(&text(0xe6), &[FormatArg::Bytes(&name)]))?;
    }
    championship_rules(painter, input, columns);
    Ok(())
}

/// `FUN_0042c240`: the header rule, a vertical rule after the names and one
/// before each country column and after the last.
fn championship_rules(painter: &mut Painter, input: &ResultsDrawInput, columns: usize) {
    rule(painter, input);
    let l = painter.layout;
    let y = fraction(l.height, 0x2b85);
    let h = fraction(l.height, 0xc000);
    painter.fill(fraction(l.width, 0x1c28), y, 1, h, BODY_COLOR);
    let mut x = l.country_x - l.country_pitch / 2 - 1;
    if columns == 6 {
        x += l.country_pitch;
    }
    for _ in 0..=columns {
        painter.fill(x, y, 1, h, BODY_COLOR);
        x += l.country_pitch;
    }
}

/// `FUN_0042e180`: GAME OVER and why.
fn game_over(
    painter: &mut Painter,
    input: &ResultsDrawInput,
    highlight: [u8; 4],
    text: &dyn Fn(i32) -> Vec<u8>,
) -> Result<(), TextLayoutError> {
    painter.text_q16(
        0x8000,
        0x3333,
        &text(0x86),
        PauseFont::Title,
        highlight,
        0x12,
    )?;
    let reason = if input.director.context().disqualified {
        0x80
    } else {
        0x12a
    };
    painter.text_q16(
        0x8000,
        0x6666,
        &text(reason),
        PauseFont::Result,
        BODY_COLOR,
        0x22,
    )
}

/// `FUN_0042df90`: the championship finished, by position (and outside the
/// top three by difficulty; the expert set's line is an unset local, drawn
/// as nothing).
fn championship_complete(
    painter: &mut Painter,
    input: &ResultsDrawInput,
    highlight: [u8; 4],
    text: &dyn Fn(i32) -> Vec<u8>,
) -> Result<(), TextLayoutError> {
    painter.text_q16(
        0x8000,
        0x4000,
        &text(0x78),
        PauseFont::Title,
        highlight,
        0x12,
    )?;
    let position = input.director.context().championship_position;
    let (first, second) = match position {
        1 => (Some(0x10d), 0x10e),
        2 => (Some(0x10f), 0x111),
        3 => (Some(0x110), 0x111),
        _ if position > 3 => match (input.event.cfg.difficulty_018 >> 3) & 3 {
            0 => (Some(0x109), 0x10c),
            1 => (Some(0x10a), 0x10c),
            2 => (Some(0x10b), 0x10c),
            _ => (None, 0x10c),
        },
        _ => (Some(0x110), 0x111),
    };
    if let Some(first) = first {
        painter.text_q16(
            0x8000,
            0x6666,
            &text(first),
            PauseFont::Result,
            BODY_COLOR,
            0x12,
        )?;
    }
    painter.text_q16(
        0x8000,
        0x8000,
        &text(second),
        PauseFont::Result,
        BODY_COLOR,
        0x12,
    )
}

/// `"%s^%02d:%02d.%02d"` (`0x4ed718`): a name over a time (unsigned
/// division).
fn name_time(name: &[u8], time: u32) -> Vec<u8> {
    [name, b"^", &time_text(time)].concat()
}

/// The stage line: `"%s - %s"` (`0x4ed740`) on a super-special stage, else
/// `"%s - %s %d ( %s )"` (`0x4ed72c`) with the stage number and name (a
/// missing name prints as the CRT's `"(null)"`, as for a special country's
/// seventh stage).
fn stage_line(input: &ResultsDrawInput, place: &[u8], text: &dyn Fn(i32) -> Vec<u8>) -> Vec<u8> {
    if !input.event.globals.school_d3ca88 && input.event.super_special() {
        c_format(
            b"%s - %s",
            &[FormatArg::Bytes(place), FormatArg::Bytes(&text(0x44))],
        )
    } else {
        long_stage_line(input, place, text)
    }
}

/// `"%s - %s %d ( %s )"`: the place, the stage word, number and name.
fn long_stage_line(
    input: &ResultsDrawInput,
    place: &[u8],
    text: &dyn Fn(i32) -> Vec<u8>,
) -> Vec<u8> {
    c_format(
        b"%s - %s %d ( %s )",
        &[
            FormatArg::Bytes(place),
            FormatArg::Bytes(&text(0x25)),
            FormatArg::Int(i64::from(input.event.cfg.cursor_168 >> 4) + 1),
            FormatArg::Bytes(input.stage_name.unwrap_or(b"(null)")),
        ],
    )
}

/// A placed row's value (`order[position - 1]`), 0 past the order.
fn placed_time(rows: &[ResultsRow], position: usize) -> u32 {
    position
        .checked_sub(1)
        .and_then(|i| rows.get(i))
        .map_or(0, |row| row.time)
}

/// Pages 0xc..=0x11: the record notices (`FUN_0042cf60`, `FUN_0042d2c0`,
/// `FUN_0042d440`) and the CAC code pages (`FUN_0042caf0`, `FUN_0042cdc0`,
/// `FUN_0042ce80`), over the frames `FUN_0042d0e0` and `FUN_0042cc10`.
/// Names come from the record image's live block; the team icons are not
/// drawn. In a time trial the stage time is the player's own (`0x54fce4`,
/// inferred to hold the finish time).
fn record_page(
    painter: &mut Painter,
    input: &ResultsDrawInput,
    highlight: [u8; 4],
    text: &dyn Fn(i32) -> Vec<u8>,
) -> Result<(), TextLayoutError> {
    let director = input.director;
    let Some(run) = director.records() else {
        return Ok(());
    };
    let live = crate::config_commit::live(&run.image);
    let name = |p: usize| crate::results_director::player_name(live, p);
    let g = director.record_globals();
    let cfg = &input.event.cfg;
    let country = text(0x3c + i32::from(cfg.cursor_168 & 0xf));
    let context = director.context();
    let stage_time = |p: usize| {
        if cfg.mode_166 & 3 == 2 {
            run.players
                .get(p)
                .map_or(0, |finish| finish.stage_time as u32)
        } else {
            placed_time(input.stage_rows, context.stage_position)
        }
    };
    let championship_total = |position: usize| {
        position
            .checked_sub(1)
            .and_then(|i| input.championship_rows.get(i))
            .map_or(0, |row| row.total(usize::from(cfg.cursor_168 & 0xf)))
    };
    let total_line = |total: u8| {
        let points = c_format(&text(0xdf), &[FormatArg::Int(i64::from(total))]);
        [&name(0)[..], b"^", &points].concat()
    };
    let ordinal = |place: i32| text(0xc3 + place);
    match director.page() {
        0xf => {
            let line = stage_line(input, &country, text);
            let local = name_time(&name(0), stage_time(0));
            cac_frame(
                painter,
                live,
                highlight,
                &text(0x135),
                &line,
                &local,
                &g.stage_cac,
            )
        }
        0x10 => {
            let local = name_time(&name(0), placed_time(input.rally_rows, 1));
            cac_frame(
                painter,
                live,
                highlight,
                &text(0x136),
                &country,
                &local,
                &g.rally_cac,
            )
        }
        0x11 => {
            let local = total_line(championship_total(1));
            cac_frame(
                painter,
                live,
                highlight,
                &text(0x137),
                b"",
                &local,
                &g.championship_cac,
            )
        }
        0xc => {
            let q = usize::from(g.stage_player);
            // The secret series names its place `0x45` and always the stage.
            let line = if cfg.mode_166 & 0x40 != 0 {
                long_stage_line(input, &text(0x45), text)
            } else {
                stage_line(input, &country, text)
            };
            let line = [&text(0xd3)[..], b"^", &line].concat();
            let local = name_time(&name(q), stage_time(q));
            notice_frame(
                painter,
                input,
                highlight,
                &text(0xdd),
                &line,
                &local,
                &g.stage_code,
                text,
            )
        }
        0xd => {
            let q = usize::from(g.rally_shown[0]).min(1);
            let place = g.rally_places[q];
            let mut line = if place == 0 {
                text(0xd3)
            } else {
                c_format(&text(0xd4), &[FormatArg::Bytes(&ordinal(place))])
            };
            line.push(b'^');
            line.extend_from_slice(&country);
            let local = name_time(
                &name(q),
                placed_time(input.rally_rows, context.rally_position),
            );
            notice_frame(
                painter,
                input,
                highlight,
                &text(0xdc),
                &line,
                &local,
                &g.rally_code,
                text,
            )
        }
        _ => {
            let local = total_line(championship_total(context.championship_position));
            let d = i32::from((cfg.difficulty_018 >> 3) & 3);
            let line = if g.championship_place == 0 {
                text(0xd5 + d)
            } else {
                c_format(
                    &text(0xd8 + d),
                    &[FormatArg::Bytes(&ordinal(g.championship_place))],
                )
            };
            notice_frame(
                painter,
                input,
                highlight,
                &text(0xdb),
                &line,
                &local,
                &g.championship_code,
                text,
            )
        }
    }
}

/// `FUN_0042cc10(title, line1, line2, code)`: a CAC code page.
fn cac_frame(
    painter: &mut Painter,
    live: &[u8],
    highlight: [u8; 4],
    title: &[u8],
    line1: &[u8],
    line2: &[u8],
    code: &[u8],
) -> Result<(), TextLayoutError> {
    let l = painter.layout;
    painter.text(
        l.title_x,
        l.title_y,
        title,
        PauseFont::Title,
        TITLE_COLOR,
        0x31,
    )?;
    painter.text_q16(0x8000, 0x4000, line1, PauseFont::Result, BODY_COLOR, 0x12)?;
    painter.text_q16(0x8000, 0x6666, line2, PauseFont::Title, BODY_COLOR, 0x12)?;
    let n = u16::from_le_bytes([live[0x20], live[0x21]]);
    let check = crate::hiscore_codes::cac_check(i32::from(n));
    let line = c_format(
        b"CAC%04d%c: %s",
        &[
            FormatArg::Int(i64::from(n)),
            FormatArg::Int(i64::from(check)),
            FormatArg::Bytes(code),
        ],
    );
    painter.text_q16(0x8000, 0x9999, &line, PauseFont::Title, highlight, 0x12)
}

/// `FUN_0042d0e0(player, title, line1, line2, code)`: a record notice (the
/// player picks only the icons, not drawn); the code line is not drawn in
/// the secret series.
#[allow(clippy::too_many_arguments)]
fn notice_frame(
    painter: &mut Painter,
    input: &ResultsDrawInput,
    highlight: [u8; 4],
    title: &[u8],
    line1: &[u8],
    line2: &[u8],
    code: &[u8],
    text: &dyn Fn(i32) -> Vec<u8>,
) -> Result<(), TextLayoutError> {
    let l = painter.layout;
    painter.text(
        l.title_x,
        l.title_y,
        title,
        PauseFont::Title,
        TITLE_COLOR,
        0x31,
    )?;
    painter.text_q16(
        0x8000,
        0x4000,
        &text(0x77),
        PauseFont::Title,
        highlight,
        0x22,
    )?;
    painter.text_q16(0x8000, 0x5999, line1, PauseFont::Result, BODY_COLOR, 0x12)?;
    painter.text_q16(0x8000, 0x9999, line2, PauseFont::Title, highlight, 0x12)?;
    if input.event.cfg.mode_166 & 0x40 == 0 {
        let line = c_format(
            b"%s: %s",
            &[FormatArg::Bytes(&text(0xde)), FormatArg::Bytes(code)],
        );
        painter.text_q16(0x8000, 0xcccc, &line, PauseFont::Result, BODY_COLOR, 0x12)?;
    }
    Ok(())
}

/// `FUN_0042e0c0`: the rally finished, and a line by difficulty (none for
/// the fourth setting).
fn rally_complete(
    painter: &mut Painter,
    input: &ResultsDrawInput,
    highlight: [u8; 4],
    text: &dyn Fn(i32) -> Vec<u8>,
) -> Result<(), TextLayoutError> {
    let country = text(0x3c + i32::from(input.event.cfg.cursor_168 & 0xf));
    let title = c_format(&text(0x11f), &[FormatArg::Bytes(&country)]);
    painter.text_q16(0x8000, 0x6666, &title, PauseFont::Title, highlight, 0x12)?;
    let line = match (input.event.cfg.difficulty_018 >> 3) & 3 {
        0 => 0x120,
        1 => 0x121,
        2 => 0x122,
        _ => return Ok(()),
    };
    painter.text_q16(
        0x8000,
        0x9999,
        &text(line),
        PauseFont::Result,
        BODY_COLOR,
        0x12,
    )
}

/// `FUN_0043bc90`: the save dialog — the message (`FUN_0044f1a0`: the text
/// at `(0x8000, 0x6666)` in `result` and `0x502498`, and `0x4d` 0xccc below
/// when it waits for a confirm), the prompt's lines (`FUN_004aadf0`, no box),
/// or the slot list (`FUN_0044f070`: the heading `0x47` in the title font and
/// `0x5024b8` at `(0x8000, 0x4ccc)`, then each slot's line 0x1333 lower in
/// `result`, `0x4f3e38` for the cursor's and `0x4f3e3c` for the others —
/// the occupied slots' summaries, `0x4b` for a free one, and `0x4c` for
/// cancel after a further 0x1333 — each passed through `sprintf` as its own
/// format). Nothing on the frame the dialog's state changed.
fn save_page(
    painter: &mut Painter,
    input: &ResultsDrawInput,
    text: &dyn Fn(i32) -> Vec<u8>,
) -> Result<(), TextLayoutError> {
    use crate::save_dialog::{CANCEL_ROW, SaveView, slot_label};
    let (width, height) = (input.width, input.height);
    let line = |painter: &mut Painter,
                y: i32,
                words: &[u8],
                font: PauseFont,
                color: [u8; 4],
                flags: u8|
     -> Result<i32, TextLayoutError> {
        let (plan, next, _) = text_q16(
            (painter.fonts)(font),
            words,
            0x8000,
            y,
            flags,
            width,
            height,
        )?;
        painter.push(font, color, plan);
        Ok(next)
    };
    match input.director.save_dialog().view() {
        Some(SaveView::Message { text: id, waits }) => {
            let y = line(
                painter,
                0x6666,
                &text(id),
                PauseFont::Result,
                BODY_COLOR,
                0x12,
            )?;
            if waits {
                line(
                    painter,
                    y + 0xccc,
                    &text(0x4d),
                    PauseFont::Result,
                    BODY_COLOR,
                    0x12,
                )?;
            }
        }
        Some(SaveView::Prompt { prompt, selected }) => {
            line(
                painter,
                prompt.y,
                &text(prompt.question),
                PauseFont::Title,
                DIALOG_TITLE_COLOR,
                prompt.flags,
            )?;
            for (index, (y, id)) in [(0x8000, 0xe), (0x8ccc, 0xf)].into_iter().enumerate() {
                let color = if index == usize::from(selected) {
                    SELECTED_COLOR
                } else {
                    UNSELECTED_COLOR
                };
                line(painter, y, &text(id), PauseFont::Result, color, 0x12)?;
            }
        }
        Some(SaveView::Slots { cursor, slots }) => {
            let mut y = line(
                painter,
                0x4ccc,
                &text(0x47),
                PauseFont::Title,
                DIALOG_TITLE_COLOR,
                0x12,
            )? + 0x1333;
            for row in 0..=CANCEL_ROW {
                let color = if row == cursor {
                    SELECTED_COLOR
                } else {
                    UNSELECTED_COLOR
                };
                let label = if row == CANCEL_ROW {
                    y += 0x1333;
                    text(0x4c)
                } else {
                    let entry = slots.get(row as usize).and_then(Option::as_ref);
                    let label = entry.map_or_else(
                        || text(0x4b),
                        |entry| {
                            slot_label(
                                entry,
                                width >= 0x200,
                                input.save_label_formats,
                                text,
                                input.car_short_names,
                            )
                        },
                    );
                    c_format(&label, &[])
                };
                y = line(painter, y, &label, PauseFont::Result, color, 0x12)?;
            }
        }
        None => {}
    }
    Ok(())
}

/// `FUN_0042e210`: the dialog's lines (`FUN_004aadf0` draws the menu
/// `0xbb3448` through `FUN_004aacb0` and returns the widest line), then the
/// box. The records `0x4ed828` (quit) and `0x4ed888` (restart) place the
/// question at `(0x8000, 0x6666)` and "yes"/"no" (`0xe`/`0xf`) at `0x8000`
/// and `0x8ccc`, all centred (`0x12`): the question in the title font and
/// `0x5024b8`, the selected answer in `result` and `0x4f3e38`, the other in
/// `result` (`resultgr` only in display mode 6) and `0x4f3e3c`.
fn confirmation(
    painter: &mut Painter,
    input: &ResultsDrawInput,
    text: &dyn Fn(i32) -> Vec<u8>,
) -> Result<(), TextLayoutError> {
    let question = if input.director.page() == 9 {
        0xd
    } else if input.event.cfg.mode_166 & 3 == 2 {
        0xe3
    } else {
        0xe1
    };
    let selected = input.director.dialog_selection().unwrap_or(1);
    let (width, height) = (input.width, input.height);
    let mut widest = 0;
    let mut line = |painter: &mut Painter,
                    y: i32,
                    id: i32,
                    font: PauseFont,
                    color: [u8; 4]|
     -> Result<(), TextLayoutError> {
        let (plan, _, line_width) = text_q16(
            (painter.fonts)(font),
            &text(id),
            0x8000,
            y,
            0x12,
            width,
            height,
        )?;
        widest = widest.max(line_width);
        painter.push(font, color, plan);
        Ok(())
    };
    line(
        painter,
        0x6666,
        question,
        PauseFont::Title,
        DIALOG_TITLE_COLOR,
    )?;
    for (index, (y, id)) in [(0x8000, 0xe), (0x8ccc, 0xf)].into_iter().enumerate() {
        let color = if index == usize::from(selected) {
            SELECTED_COLOR
        } else {
            UNSELECTED_COLOR
        };
        line(painter, y, id, PauseFont::Result, color)?;
    }
    let box_width = widest + 0x1999;
    let l = painter.layout;
    let x = (0x8000 - box_width / 2).wrapping_mul(l.width) >> 16;
    let color = if input.panel_capable {
        PANEL_TRANSLUCENT
    } else {
        PANEL_OPAQUE
    };
    painter.fill(
        x,
        fraction(l.height, 0x5eb8),
        box_width.wrapping_mul(l.width) >> 16,
        fraction(l.height, 0x428f),
        color,
    );
    Ok(())
}

/// `FUN_0042d5a0`: each item centred on its slot in the bottom bar, the
/// selected one in `result` white, the others in `resultgr` grey.
fn post_stage_menu(
    painter: &mut Painter,
    input: &ResultsDrawInput,
    text: &dyn Fn(i32) -> Vec<u8>,
) -> Result<(), TextLayoutError> {
    let (cursor, count) = input.director.menu();
    let Some(xs) = count.checked_sub(1).and_then(|index| MENU_X_Q16.get(index)) else {
        return Ok(());
    };
    let time_trial = input.event.cfg.mode_166 & 3 == 2;
    for (index, &menu_item) in input.director.menu_items().iter().enumerate() {
        let label = match menu_item {
            item::RESTART => text(if time_trial { 0xe2 } else { 0xe0 }),
            item::CONTINUE => text(5),
            item::QUIT => text(0x32),
            _ => continue,
        };
        let (font, color) = if index == cursor {
            (PauseFont::Result, SELECTED_COLOR)
        } else {
            (PauseFont::ResultGrey, UNSELECTED_COLOR)
        };
        painter.text_q16(xs[index], MENU_Y_Q16, &label, font, color, 0x12)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::results_director::{DirectorContext, DirectorInput};

    #[test]
    fn the_layout_at_640x480() {
        let l = ResultsLayout::new(640, 480);
        assert_eq!((l.title_x, l.title_y, l.header_y), (31, 52, 100));
        assert_eq!(
            (l.team_x, l.position_x, l.name_x, l.time_x),
            (95, 198, 294, 543)
        );
        assert_eq!((l.corner_x, l.row_base, l.pitch()), (575, 138, 37));
        assert_eq!((l.clip.y, l.clip.h), (120, 331));
        assert_eq!(l.corner_nudge, -3);
        assert_eq!(ResultsLayout::new(320, 240).corner_nudge, 1);
    }

    #[test]
    fn the_highlight_pulses_between_the_stops() {
        let mut pulse = ResultsPulse::default();
        assert_eq!(pulse.color(), HIGHLIGHT_INITIAL);
        assert_eq!(pulse.advance(), [0xff, 0xd1, 0x00, 0xff]);
        // Phase 4: halfway to the second stop, truncating toward zero.
        for _ in 0..3 {
            pulse.advance();
        }
        assert_eq!(pulse.advance(), [0xf9, 0x93, 0x00, 0xff]);
        for _ in 0..11 {
            pulse.advance();
        }
        assert_eq!(pulse.advance(), [0xff, 0xd1, 0x00, 0xff], "wraps after 16");
    }

    use crate::pause_menu_draw::tests::font;

    fn input<'a>(
        director: &'a ResultsDirector,
        event: &'a EventState,
        rows: &'a [ResultsRow<'a>],
    ) -> ResultsDrawInput<'a> {
        ResultsDrawInput {
            director,
            event,
            stage_rows: rows,
            rally_rows: rows,
            championship_rows: &[],
            stage_name: Some(b"STAGE"),
            scroll_t_q16: 0x10000,
            width: 640,
            height: 480,
            panel_capable: true,
            car_short_names: &[],
            save_label_formats: Default::default(),
        }
    }

    #[test]
    fn the_stage_results_page() {
        let font = font();
        let mut event = EventState::default();
        event.cfg.difficulty_018 = 0x08;
        event.cfg.stage_count_165 = 6;
        let director = ResultsDirector::start(
            &mut event,
            DirectorContext {
                stage_position: 1,
                screen_height: 480,
                ..Default::default()
            },
        );
        let rows: Vec<ResultsRow> = (0..16)
            .map(|i| ResultsRow {
                name: b"AB",
                time: 6000 + i,
                player: i == 0,
            })
            .collect();
        let mut pulse = ResultsPulse::default();
        let text = |id: i32| format!("T{id:X}").into_bytes();
        let plan = results_draw_plan(&input(&director, &event, &rows), &mut pulse, &text, &|_| {
            &font
        })
        .unwrap();
        // Panel, two bars, the rule.
        assert_eq!(plan.quads.len(), 4);
        assert_eq!(
            plan.quads[1].0,
            OverlayQuad {
                x: 0,
                y: 0,
                w: 640,
                h: 52
            }
        );
        assert_eq!(
            plan.quads[3].0,
            OverlayQuad {
                x: 31,
                y: 100,
                w: 578,
                h: 1
            }
        );
        // Title, four headers, nine rows of three, the corner.
        assert_eq!(plan.runs.len(), 1 + 4 + 9 * 3 + 1);
        let first_row = &plan.runs[5];
        assert_eq!(
            first_row.color,
            [0xff, 0xd1, 0x00, 0xff],
            "the player row pulses"
        );
        assert_eq!(first_row.clip, Some(ResultsLayout::new(640, 480).clip));
        assert_eq!(plan.runs[6].color, [0xff, 0xd1, 0x00, 0xff]);
        assert_eq!(plan.runs[8].color, BODY_COLOR);
        assert!(plan.runs[0].clip.is_none());
    }

    #[test]
    fn the_settle_draws_only_the_frame_and_the_menu_draws_its_items() {
        let font = font();
        let mut event = EventState::default();
        event.cfg.difficulty_018 = 0x08;
        event.cfg.stage_count_165 = 6;
        let mut director = ResultsDirector::start(&mut event, DirectorContext::default());
        let confirm = DirectorInput {
            confirm: true,
            ..Default::default()
        };
        director.tick(&mut event, confirm);
        let text = |id: i32| format!("T{id:X}").into_bytes();
        let mut pulse = ResultsPulse::default();
        let plan = results_draw_plan(&input(&director, &event, &[]), &mut pulse, &text, &|_| {
            &font
        })
        .unwrap();
        assert!(director.settle() > 0);
        assert!(plan.runs.is_empty());
        assert_eq!(plan.quads.len(), 3);
        let mut tick = 0;
        while director.page() != 4 {
            tick += 1;
            let keys = if tick % 2 == 0 { 0x10 } else { 0 };
            director.tick(&mut event, DirectorInput { keys, ..confirm });
        }
        while director.settle() > 0 {
            director.tick(&mut event, DirectorInput::default());
        }
        let plan = results_draw_plan(&input(&director, &event, &[]), &mut pulse, &text, &|_| {
            &font
        })
        .unwrap();
        assert_eq!(plan.quads.len(), 2, "the menu page has no panel");
        assert_eq!(plan.runs.len(), 3);
        assert_eq!(plan.runs[1].color, SELECTED_COLOR, "continue is selected");
        assert_eq!(plan.runs[0].font, PauseFont::ResultGrey);
    }

    #[test]
    fn the_championship_page_and_the_end_pages() {
        let font = font();
        let mut event = EventState::default();
        event.cfg.stage_count_165 = 6;
        event.cfg.cursor_168 = 0x22; // novice, country 2
        let mut director = ResultsDirector::start(
            &mut event,
            DirectorContext {
                championship_position: 2,
                screen_height: 480,
                ..Default::default()
            },
        );
        let confirm = DirectorInput {
            confirm: true,
            ..Default::default()
        };
        let advance = |director: &mut ResultsDirector, event: &mut EventState| {
            director.tick(event, confirm);
            while director.settle() > 0 {
                director.tick(event, DirectorInput::default());
            }
        };
        advance(&mut director, &mut event);
        advance(&mut director, &mut event);
        assert_eq!(director.page(), 3);
        let row = ChampionshipRow {
            name: b"AB",
            player: true,
            points: [10, 6, 4, 3, 0, 0, 0, 0, 0],
        };
        assert_eq!(row.total(2), 20);
        let rows = [row; 8];
        let text = |id: i32| format!("T{id:X}").into_bytes();
        let mut pulse = ResultsPulse::default();
        let mut draw = |director: &ResultsDirector, event: &EventState| {
            let mut input = input(director, event, &[]);
            input.championship_rows = &rows;
            results_draw_plan(&input, &mut pulse, &text, &|_| &font).unwrap()
        };
        let plan = draw(&director, &event);
        // Headers, eight rows of position, name, six columns and total, title,
        // corner.
        assert_eq!(plan.runs.len(), 3 + 8 * 9 + 2);
        // Panel, bars, header rule, the vertical rule and seven dividers.
        assert_eq!(plan.quads.len(), 3 + 1 + 1 + 7);
        // Novice, country 2 of 5: the save is offered, then rally complete.
        advance(&mut director, &mut event);
        assert_eq!(director.page(), 5);
        // Answer "no" to the save prompt.
        for keys in [0, 0x08, 0x10, 0] {
            director.tick(
                &mut event,
                DirectorInput {
                    keys,
                    ..Default::default()
                },
            );
        }
        while director.settle() > 0 {
            director.tick(&mut event, DirectorInput::default());
        }
        assert_eq!(director.page(), 0x15);
        assert_eq!(draw(&director, &event).runs.len(), 2);
        let mut over = EventState::default();
        over.cfg.difficulty_018 = 0x18;
        let director = ResultsDirector::start(
            &mut over,
            DirectorContext {
                disqualified: true,
                ..Default::default()
            },
        );
        assert_eq!(director.page(), 8);
        let plan = draw(&director, &over);
        // GAME OVER and the disqualification line.
        assert_eq!(plan.runs.len(), 2);
        assert_eq!(plan.runs[1].color, BODY_COLOR);
    }

    #[test]
    fn the_confirmation_draws_its_lines_and_a_box_sized_to_them() {
        let font = font();
        let mut event = EventState::default();
        let mut director = ResultsDirector::start(&mut event, DirectorContext::default());
        let confirm = DirectorInput {
            confirm: true,
            ..Default::default()
        };
        // Confirm through the pages; the save page's dialog sees the key
        // change every other tick (it saves to slot 0 and ends).
        let mut tick = 0;
        while director.page() != 4 {
            tick += 1;
            let keys = if tick % 2 == 0 { 0x10 } else { 0 };
            director.tick(&mut event, DirectorInput { keys, ..confirm });
        }
        while director.settle() > 0 {
            director.tick(&mut event, DirectorInput::default());
        }
        director.tick(
            &mut event,
            DirectorInput {
                right: true,
                ..Default::default()
            },
        );
        director.tick(&mut event, confirm);
        while director.settle() > 0 {
            director.tick(&mut event, DirectorInput::default());
        }
        assert_eq!(director.page(), 9);
        let text = |id: i32| format!("T{id:X}").into_bytes();
        let mut pulse = ResultsPulse::default();
        let plan = results_draw_plan(&input(&director, &event, &[]), &mut pulse, &text, &|_| {
            &font
        })
        .unwrap();
        assert_eq!(plan.runs.len(), 3);
        assert_eq!(plan.runs[0].color, DIALOG_TITLE_COLOR);
        assert_eq!(plan.runs[1].color, UNSELECTED_COLOR, "yes");
        assert_eq!(plan.runs[2].color, SELECTED_COLOR, "no is the default");
        // Bars, then the box: centred, from 0.37H, 0.26H tall.
        let (quad, color) = plan.quads[2];
        assert_eq!(color, PANEL_TRANSLUCENT);
        assert_eq!((quad.y, quad.h), (177, 124));
        let span = 2 * u32::from(quad.x) + u32::from(quad.w);
        assert!((639..=641).contains(&span), "centred: {quad:?}");
        assert!(quad.w >= 63, "at least the 0x1999 margin");
    }

    #[test]
    fn times_format_as_minutes_seconds_hundredths() {
        assert_eq!(time_text(12_345), b"02:03.45");
        assert_eq!(time_text(0), b"00:00.00");
    }

    /// Each run's lines joined by `^`.
    fn lines(plan: &ResultsDrawPlan) -> Vec<Vec<u8>> {
        plan.runs
            .iter()
            .map(|run| {
                let lines: Vec<&[u8]> = run.plan.lines.iter().map(|l| &l.bytes[..]).collect();
                lines.join(&b'^')
            })
            .collect()
    }

    #[test]
    fn the_record_pages_show_the_code_and_the_notice() {
        use crate::results_director::{PlayerFinish, RecordRun};
        let font = font();
        let mut event = EventState::default();
        event.cfg.cursor_168 = 0x21;
        event.cfg.stage_count_165 = 6;
        let mut image = Box::new([0u8; crate::config::LEGACY_CONFIG_SIZE]);
        for record in 0..(7 * 9) {
            let at = 0xffc + record * 30 + 0x1a;
            image[at..at + 2].copy_from_slice(&0xffffu16.to_le_bytes());
        }
        let live = crate::config_commit::live_mut(&mut image);
        live[0x2c..0x31].copy_from_slice(b"COLIN");
        live[0x20..0x22].copy_from_slice(&1234u16.to_le_bytes());
        let run = RecordRun {
            image,
            schedule: vec![0; crate::hiscore_codes::SCHEDULE_LEN],
            players: vec![PlayerFinish {
                slot: 0,
                stage_time: 6123,
                splits: [0; 7],
            }],
            salt: 0,
        };
        let context = DirectorContext {
            stage_position: 1,
            rally_position: 1,
            screen_height: 480,
            ..Default::default()
        };
        let mut director = ResultsDirector::start(&mut event, context).with_records(run);
        let confirm = DirectorInput {
            confirm: true,
            ..Default::default()
        };
        director.tick(&mut event, confirm);
        while director.settle() > 0 {
            director.tick(&mut event, DirectorInput::default());
        }
        assert_eq!(director.page(), 0xf);
        let rows = [ResultsRow {
            name: b"COLIN",
            time: 6123,
            player: true,
        }];
        let text = |id: i32| format!("T{id:X}").into_bytes();
        let mut pulse = ResultsPulse::default();
        let plan = results_draw_plan(&input(&director, &event, &rows), &mut pulse, &text, &|_| {
            &font
        })
        .unwrap();
        let code = director.record_globals().stage_cac.clone();
        let cac = [&b"CAC12340: "[..], &code].concat();
        assert_eq!(
            lines(&plan),
            [
                b"T135".to_vec(),
                b"T3D - T25 3 ( STAGE )".to_vec(),
                b"COLIN^01:01.23".to_vec(),
                cac,
            ]
        );
        director.tick(&mut event, confirm);
        while director.settle() > 0 {
            director.tick(&mut event, DirectorInput::default());
        }
        assert_eq!(director.page(), 0xc);
        let plan = results_draw_plan(&input(&director, &event, &rows), &mut pulse, &text, &|_| {
            &font
        })
        .unwrap();
        let code = director.record_globals().stage_code.clone();
        assert_eq!(
            lines(&plan),
            [
                b"TDD".to_vec(),
                b"T77".to_vec(),
                b"TD3^T3D - T25 3 ( STAGE )".to_vec(),
                b"COLIN^01:01.23".to_vec(),
                [&b"TDE: "[..], &code].concat(),
            ]
        );
    }
}
