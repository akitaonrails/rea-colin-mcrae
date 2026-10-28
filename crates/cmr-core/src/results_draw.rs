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
//! (`FUN_0042b960`) and the post-stage menu (`FUN_0042d5a0`). The other
//! pages draw the frame only for now. Not drawn: the team and flag icons
//! (`FUN_0042e5c0`), the time trial's record rows (`FUN_0042b6b0`), and the
//! stage name when the host has none (`FUN_004ba470` reads the executable's
//! name table `0x506f80`, which is not extracted). The table rows are cut to
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
    PANEL_TRANSLUCENT, PauseFont, PauseTextRun, SELECTED_COLOR, UNSELECTED_COLOR, text_q16,
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
    /// `0x54f9d0`: the first row's centre.
    pub row_base: i32,
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

/// What the pages draw from.
pub struct ResultsDrawInput<'a> {
    pub director: &'a ResultsDirector,
    pub event: &'a EventState,
    /// The stage order (`0x54fdcc`) and the rally order (`0x54fe58`).
    pub stage_rows: &'a [ResultsRow<'a>],
    pub rally_rows: &'a [ResultsRow<'a>],
    /// `FUN_004ba470(country, stage)`, when the host has the name table.
    pub stage_name: Option<&'a [u8]>,
    /// `FUN_00430480`'s fraction of the tick, Q16.
    pub scroll_t_q16: i32,
    pub width: u16,
    pub height: u16,
    /// `FUN_004aa040() || FUN_00498af0() == 3`: the panel is drawn.
    pub panel_capable: bool,
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
        4 => post_stage_menu(&mut painter, input, text)?,
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
        let (plan, _, _) = text_q16(
            (painter.fonts)(font),
            &label,
            xs[index],
            MENU_Y_Q16,
            0x12,
            input.width,
            input.height,
        )?;
        painter.push(font, color, plan);
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
            stage_name: Some(b"STAGE"),
            scroll_t_q16: 0x10000,
            width: 640,
            height: 480,
            panel_capable: true,
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
        while director.page() != 4 {
            director.tick(&mut event, confirm);
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
    fn times_format_as_minutes_seconds_hundredths() {
        assert_eq!(time_text(12_345), b"02:03.45");
        assert_eq!(time_text(0), b"00:00.00");
    }
}
