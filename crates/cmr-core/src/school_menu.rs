//! The rally school's screens, FSM state 2 in a school session
//! (`FUN_00470e20` update, `FUN_0046f670` render): the level and task menus
//! with their locks, the zoom and briefing videos, the task results and the
//! level-complete screen. The school byte `cfg[0x6c0]` holds the screen
//! status (`& 7`), the level (`>> 3 & 3`) and the task (`>> 5`);
//! `cfg[0x6c1]` the highest unlocked level and task.
//!
//! The screen runs on the 8-bit display (`ini\schlpal.pal`), where blits and
//! the direct glyph path ignore colour, so the module's colour fade (its own
//! tween `DAT_005f13c4`) only paces the screens. Each status selects its
//! palette (`palettes\cabin.pal`, `results.pal`, `complet.pal`) over bitmaps
//! remapped to `schlpal` at load. The videos and the wipe block inside the
//! native calls; here they are effects the host plays between frames.
//!
//! A description of disassembly, not original source. Evidence:
//! `rea.official-eu.school-menu`.

use cmr_formats::font::{TextLayoutError, measure_line};
use cmr_formats::riff_palette::PaletteEntry;

use crate::descriptor_menu::{ItemKind, Menu, MenuEvent, MenuId, MenuItem, MenuSet, MenuSound};
use crate::indexed_canvas::{Canvas, CanvasFont, Sprite};
use crate::screen_fade::ScreenFade;

/// The menus' sound table (`0x5f1d88`).
pub const SOUNDS: u32 = 0x5f1d88;

/// The item activate callbacks: the select cue and wipe (`0x4701a0`) and
/// the quit (`0x470250`).
pub mod callback {
    pub const WIPE: u32 = 0x4701a0;
    pub const QUIT: u32 = 0x470250;
    /// The menus' commit callback `0x46ef40`: the fade evaluated each step.
    pub const COMMIT: u32 = 0x46ef40;
    /// The predraw `0x46ef90`.
    pub const PREDRAW: u32 = 0x46ef90;
}

/// The menu waves (`FUN_00471380`, `sounds\menu\*.wav`, 11025 Hz): slots by
/// load position, a missing file leaving its slot silent.
pub const WAVES: [&str; 5] = ["WHEEL", "UPDOWN", "TOGGLE", "SELECT", "BACK"];

/// The cue slots the callbacks play.
pub mod cue {
    pub const UPDOWN: u8 = 1;
    pub const TOGGLE: u8 = 2;
    pub const SELECT: u8 = 3;
}

/// The wipe's frames (`WIPE01..WIPE22`) and the fan and plant frames.
pub const WIPE_FRAMES: usize = 22;
pub const ANIMATION_FRAMES: usize = 31;

/// The briefing videos' rectangle and the fixed pixel positions.
pub const BRIEFING_RECT: [i32; 4] = [80, 32, 320, 208];
const WIPE_AT: (i32, i32) = (444, 66);
const FAN_AT: (i32, i32) = (367, 184);
const PLANT_AT: (i32, i32) = (183, 130);
const LOGO_AT: (i32, i32) = (454, 96);
const PASS_AT: (i32, i32) = (236, 320);

/// The level directories of the videos and speech (`0x4f4ba8`).
pub const LEVEL_DIRS: [&str; 3] = ["AVI\\EASY\\", "AVI\\MEDIUM\\", "AVI\\HARD\\"];
/// The speech languages by `cfg[0x18] & 7`.
pub const LANGUAGE_DIRS: [&str; 5] = [
    "ENGLISH\\",
    "FRENCH\\",
    "GERMAN\\",
    "SPANISH\\",
    "ITALIAN\\",
];

/// `FUN_0046ef30`: the tasks in a level.
pub const fn task_count(level: u8) -> u8 {
    level + 3
}

/// `FUN_0046f590(level, task)`: the task's name id, or 0.
pub const fn task_name(level: u8, task: u8) -> i32 {
    match (level, task) {
        (0, 0..=2) => 0x33 + task as i32,
        (1, 0..=3) | (2, 0..=4) => 0x36 + task as i32,
        _ => 0,
    }
}

/// The module fade's length: `((DAT_00551af0 != 0) + 1) * 25 / 4`.
pub const fn fade_count(flag_551af0: bool) -> u32 {
    if flag_551af0 { 12 } else { 6 }
}

/// A host effect of one update or render.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SchoolEffect {
    /// Stop channel 0, play the slot once, channel 0 to full volume.
    Cue(u8),
    /// `FUN_004701a0`'s blocking 22-frame wipe ([`SchoolMenu::wipe_frame`]).
    Wipe,
    /// A blocking video (`FUN_004af8a0`) at a pixel rectangle; the path is
    /// under the `AVIPath` root.
    Movie { path: String, rect: [i32; 4] },
    /// The instructor's speech, under the school root, on its own channel
    /// at volume `0x7d00`.
    Speech { path: String },
    /// `FUN_00471260`: load `COM_FONT`, `COM_BACK` and `TICK`.
    LoadCompletion,
    /// The lazy init's loads (`FUN_004702a0`).
    LoadAssets,
}

/// What the screen reads besides the live block.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SchoolContext {
    /// `DAT_00551af0`.
    pub flag_551af0: bool,
    /// `DAT_00bbabec`: instant fades.
    pub instant: bool,
}

/// `DAT_005f1df8`/`DAT_004f4bbc`/`DAT_005f1dfc`: the fan and plant frame,
/// its direction and the time it last stepped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Animation {
    pub frame: i32,
    pub direction: i32,
    pub last_ms: u32,
}

impl Default for Animation {
    fn default() -> Self {
        Self {
            frame: 0,
            direction: 1,
            last_ms: 0,
        }
    }
}

impl Animation {
    /// The frame step after the draw: every 55+ ms, ping-ponging 0..=30.
    fn step(&mut self, now: u32) {
        if (now.wrapping_sub(self.last_ms) as i32).unsigned_abs() > 55 {
            self.last_ms = now;
            self.frame += self.direction;
            if self.frame >= 31 {
                self.frame = 29;
                self.direction = -1;
            } else if self.frame <= -1 {
                self.frame = 1;
                self.direction = 1;
            }
        }
    }
}

/// The module's state (`0x5f1db8..0x5f1e14`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchoolMenu {
    pub set: MenuSet,
    pub root: MenuId,
    pub levels: [MenuId; 3],
    /// `0x5ef478`: the "task chosen" target, never shown.
    pub sentinel: MenuId,
    /// `DAT_005f1dec`.
    pub current: MenuId,
    /// `DAT_005f1db8`, `DAT_005f1dbc`, `DAT_005f1dc0`, `DAT_005f1dc8`.
    initialised: bool,
    skip: bool,
    done: bool,
    quit: bool,
    /// The text-colour tween `DAT_005f13c4`.
    pub fade: ScreenFade,
    /// `DAT_005f1e0c`/`08`/`04`: the typewriter's menu, revealed and total.
    typed: Option<MenuId>,
    revealed: usize,
    total: usize,
    pub animation: Animation,
    /// `DAT_005f1dc4` (the last results passed), `DAT_005f1dcc` (the level
    /// just completed), `DAT_005f1e10`/`DAT_005f1e14` (fade-out requested).
    passed: bool,
    completed_level: u8,
    results_out: bool,
    complete_out: bool,
}

impl Default for SchoolMenu {
    fn default() -> Self {
        Self {
            set: MenuSet::default(),
            root: MenuId(0),
            levels: [MenuId(0); 3],
            sentinel: MenuId(0),
            current: MenuId(0),
            initialised: false,
            skip: false,
            done: false,
            quit: false,
            fade: ScreenFade::default(),
            typed: None,
            revealed: 0,
            total: 0,
            animation: Animation::default(),
            passed: false,
            completed_level: 0,
            results_out: false,
            complete_out: false,
        }
    }
}

fn status(live: &[u8]) -> u8 {
    live[0x6c0] & 7
}

fn set_status(live: &mut [u8], s: u8) {
    live[0x6c0] = (live[0x6c0] & 0xf8) | s;
}

fn level_of(live: &[u8]) -> u8 {
    (live[0x6c0] >> 3) & 3
}

fn task_of(live: &[u8]) -> u8 {
    live[0x6c0] >> 5
}

impl SchoolMenu {
    /// `FUN_0046f650`: the quit latch with the fade finished.
    pub fn quit_requested(&self) -> bool {
        self.quit && self.fade.idle()
    }

    fn fade_in(&mut self, context: SchoolContext) {
        self.fade
            .start(fade_count(context.flag_551af0), 0, 0xff, context.instant);
    }

    fn fade_out(&mut self, context: SchoolContext) {
        self.fade
            .start(fade_count(context.flag_551af0), 0xff, 0, context.instant);
    }

    /// `FUN_0046ef50`: `true` once the fade is over; while it runs each call
    /// evaluates it (the colour it writes is unused on 8 bits).
    fn fade_done(&mut self) -> bool {
        if self.fade.idle() {
            return true;
        }
        self.fade.evaluate();
        false
    }

    /// `FUN_00470070`: the lazy init (`FUN_004702a0`: the display, text,
    /// sounds, fonts, bitmaps and palettes the host loads on
    /// [`SchoolEffect::LoadAssets`], the menus and locks; its fade in is
    /// replaced by a second one).
    fn init(&mut self, live: &[u8], context: SchoolContext, effects: &mut Vec<SchoolEffect>) {
        if self.skip || self.done || self.initialised {
            return;
        }
        effects.push(SchoolEffect::LoadAssets);
        self.build_menus(live);
        self.fade_in(context);
        self.initialised = true;
        self.quit = false;
    }

    /// `FUN_004708c0`: the root (title `0x2e`: the three levels and quit
    /// `0x32`) and the level menus (their tasks and back `9`), none wrapping.
    fn build_menus(&mut self, live: &[u8]) {
        let mut set = MenuSet::default();
        let new_menu = |title: i32, parent: Option<MenuId>| {
            let mut menu = Menu::new(title, parent, Some(SOUNDS));
            menu.wrap = false;
            menu.on_commit = Some(callback::COMMIT);
            menu.on_predraw = Some(callback::PREDRAW);
            menu
        };
        let root = set.add(new_menu(0x2e, None));
        let sentinel = set.add(Menu::new(-1, None, None));
        let levels: [MenuId; 3] = std::array::from_fn(|level| {
            let id = set.add(new_menu(0x2f + level as i32, Some(root)));
            for task in 0..task_count(level as u8) {
                let mut item =
                    MenuItem::new(ItemKind::Submenu(sentinel), task_name(level as u8, task), 0);
                item.activate = Some(callback::WIPE);
                set.menu_mut(id).items.push(item);
            }
            let mut back = MenuItem::new(ItemKind::Back, 9, 0);
            back.activate = Some(callback::WIPE);
            set.menu_mut(id).items.push(back);
            id
        });
        for (level, &id) in levels.iter().enumerate() {
            let mut item = MenuItem::new(ItemKind::Submenu(id), 0x2f + level as i32, 0);
            item.activate = Some(callback::WIPE);
            set.menu_mut(root).items.push(item);
        }
        let mut quit = MenuItem::new(ItemKind::Action, 0x32, 0);
        quit.activate = Some(callback::QUIT);
        set.menu_mut(root).items.push(quit);
        self.set = set;
        self.root = root;
        self.levels = levels;
        self.sentinel = sentinel;
        self.current = root;
        let _ = self.set.activate(root, None, 0);
        self.apply_locks(live);
    }

    /// `FUN_00470d30`: lock the levels and tasks past `cfg[0x6c1]` and select
    /// the school byte's level and task.
    fn apply_locks(&mut self, live: &[u8]) {
        let unlocked_level = live[0x6c1] & 3;
        let unlocked_task = (live[0x6c1] >> 2) & 7;
        for level in 0..3u8 {
            self.set.menu_mut(self.root).items[usize::from(level)].enabled =
                level <= unlocked_level;
            let menu = self.set.menu_mut(self.levels[usize::from(level)]);
            menu.selected = usize::from(task_of(live));
            for t in 0..task_count(level) {
                if let Some(item) = menu.items.get_mut(usize::from(t)) {
                    item.enabled =
                        level < unlocked_level || (level == unlocked_level && t <= unlocked_task);
                }
            }
        }
        self.set.menu_mut(self.root).selected = usize::from(level_of(live));
    }

    /// `FUN_00470e20`: one update with the change-filtered key bits; `true`
    /// once the screen is done (state 2 then leaves, to 3 or, after a quit,
    /// 0xc).
    pub fn update(
        &mut self,
        live: &mut [u8],
        input: u32,
        context: SchoolContext,
    ) -> (bool, Vec<SchoolEffect>) {
        let mut effects = Vec::new();
        self.init(live, context, &mut effects);
        match status(live) {
            0 => {
                let (next, events) = self.set.step(self.current, input);
                self.run_events(live, &events, context, &mut effects);
                if let Some(next) = next {
                    let previous = self.current;
                    self.current = next;
                    if next == self.sentinel {
                        let level = self.set.menu(self.root).selected as u8;
                        let task = self
                            .set
                            .menu(self.levels[usize::from(level & 3).min(2)])
                            .selected as u8;
                        live[0x6c0] = (live[0x6c0] & 0xe7) | ((level & 3) << 3);
                        live[0x6c0] = (live[0x6c0] & 0x1f) | (task << 5);
                        live[0x6c0] = (live[0x6c0] & 0xf9) | 1;
                    } else {
                        let _ = self.set.activate(next, Some(previous), 0);
                    }
                }
            }
            3 | 7 => {
                if self.fade_done() {
                    self.teardown();
                }
            }
            4 => self.results_update(live, input, context, &mut effects),
            5 => self.complete_update(live, input, context),
            _ => {}
        }
        let done = std::mem::take(&mut self.done);
        (done, effects)
    }

    fn run_events(
        &mut self,
        live: &mut [u8],
        events: &[MenuEvent],
        context: SchoolContext,
        effects: &mut Vec<SchoolEffect>,
    ) {
        for event in events {
            match *event {
                MenuEvent::Sound {
                    menu, sound, ok, ..
                } => match sound {
                    MenuSound::Move if ok => effects.push(SchoolEffect::Cue(cue::UPDOWN)),
                    MenuSound::ValueChange if ok => effects.push(SchoolEffect::Cue(cue::TOGGLE)),
                    // `0x470170`: back on the root quits, elsewhere it wipes.
                    MenuSound::Back if menu == self.root => self.quit_school(live, context),
                    MenuSound::Back => Self::wipe(effects),
                    _ => {}
                },
                MenuEvent::Activate { callback, .. } => match callback {
                    callback::WIPE => Self::wipe(effects),
                    callback::QUIT => self.quit_school(live, context),
                    _ => {}
                },
                MenuEvent::Commit { .. } => {
                    self.fade_done();
                }
                _ => {}
            }
        }
    }

    /// `FUN_004701a0`: the select cue, then the blocking wipe.
    fn wipe(effects: &mut Vec<SchoolEffect>) {
        effects.push(SchoolEffect::Cue(cue::SELECT));
        effects.push(SchoolEffect::Wipe);
    }

    /// `FUN_00470250`: fade out, the quit latch, status 7.
    fn quit_school(&mut self, live: &mut [u8], context: SchoolContext) {
        self.fade_out(context);
        self.quit = true;
        live[0x6c0] |= 7;
    }

    /// `FUN_0046f630` (without the display shutdown, the host's).
    fn teardown(&mut self) {
        self.initialised = false;
        self.skip = true;
        self.done = true;
    }

    /// `FUN_00470f50`: the results wait for confirm or back, fade out, then
    /// a pass advances the task (and the level, with the level-complete
    /// screen) and a fail retries it.
    fn results_update(
        &mut self,
        live: &mut [u8],
        input: u32,
        context: SchoolContext,
        effects: &mut Vec<SchoolEffect>,
    ) {
        if !self.fade_done() {
            return;
        }
        if !self.results_out {
            if input & 0x30 != 0 {
                self.fade_out(context);
                self.results_out = true;
            }
            return;
        }
        if self.passed {
            let s = live[0x6c0];
            live[0x6c0] = ((s & 0xe0).wrapping_add(0x20)) | (s & 0x1f);
            let level = level_of(live);
            if task_of(live) >= task_count(level) {
                self.completed_level = level;
                if level < 2 {
                    let next = level + 1;
                    live[0x6c0] = (live[0x6c0] & 0x07) | (next << 3);
                    if next > live[0x6c1] & 3 {
                        live[0x6c1] &= 0xe3;
                        live[0x6c1] = (live[0x6c1] & 0xfc) | next;
                    }
                }
                // `FUN_00471260` loads the screen's assets and fades in.
                effects.push(SchoolEffect::LoadCompletion);
                live[0x6c0] = (live[0x6c0] & 0xfd) | 5;
                self.results_out = false;
                self.fade_in(context);
                return;
            }
            let unlocked_task = (live[0x6c1] >> 2) & 7;
            if level == live[0x6c1] & 3 && task_of(live) > unlocked_task {
                live[0x6c1] = (live[0x6c1] & 0xe3) | (((unlocked_task + 1) & 7) << 2);
            }
        }
        set_status(live, 0);
        self.results_out = false;
        self.current = self.levels[usize::from(level_of(live)).min(2)];
        let _ = self.set.activate(self.current, None, 0);
        self.apply_locks(live);
    }

    /// `FUN_00471190`: the level-complete screen waits the same way; after
    /// the last level the school is finished (status 6).
    fn complete_update(&mut self, live: &mut [u8], input: u32, context: SchoolContext) {
        if !self.fade_done() {
            return;
        }
        if !self.complete_out {
            if input & 0x30 != 0 {
                self.fade_out(context);
                self.complete_out = true;
            }
            return;
        }
        self.complete_out = false;
        if self.completed_level == 2 {
            self.done = true;
            self.skip = true;
            self.quit = true;
            live[0x6c0] = (live[0x6c0] & 0xfe) | 6;
            return;
        }
        set_status(live, 0);
        self.apply_locks(live);
    }
}

/// The screen's assets (`FUN_004702a0`, under `Game\RallSchl`), bitmaps and
/// fonts remapped to `schlpal`.
#[derive(Clone, Debug, Default)]
pub struct SchoolAssets {
    /// `palettes\cabin.pal`, `results.pal`, `complet.pal`.
    pub cabin: Option<[PaletteEntry; 256]>,
    pub results_palette: Option<[PaletteEntry; 256]>,
    pub complete_palette: Option<[PaletteEntry; 256]>,
    /// `BLAKTIP`, `GREYTIP`, `REDTIP`, `CO_BOLD`, `FATWRIT`, and `COM_FONT`
    /// once a level completes.
    pub blaktip: Option<CanvasFont>,
    pub greytip: Option<CanvasFont>,
    pub redtip: Option<CanvasFont>,
    pub co_bold: Option<CanvasFont>,
    pub fatwrit: Option<CanvasFont>,
    pub com_font: Option<CanvasFont>,
    pub pass: Option<Sprite>,
    pub fail: Option<Sprite>,
    pub results: Option<Sprite>,
    pub back: Option<Sprite>,
    pub logo: Option<Sprite>,
    pub video: Option<Sprite>,
    pub fan: Vec<Option<Sprite>>,
    pub plant: Vec<Option<Sprite>>,
    pub wipe: Vec<Option<Sprite>>,
    pub complete_back: Option<Sprite>,
    pub tick: Option<Sprite>,
}

/// One render's inputs.
pub struct SchoolFrame<'a> {
    pub assets: &'a SchoolAssets,
    /// The game text (`FUN_0049b010`, the school's language table).
    pub text: &'a dyn Fn(i32) -> Vec<u8>,
    pub width: u16,
    pub height: u16,
    /// `_ftime` milliseconds.
    pub now_ms: u32,
    /// `cfg[0x18] & 7`: the speech language.
    pub language: u8,
    /// `DAT_005f9e90`/`DAT_005f1e20`: the level and task the school stage
    /// loader last loaded.
    pub loaded: (u8, u8),
}

/// What a render shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchoolView {
    /// The back buffer to present, or `None` when nothing was drawn.
    pub pixels: Option<Vec<u8>>,
    /// The palette the status selected.
    pub palette: Option<[PaletteEntry; 256]>,
    pub effects: Vec<SchoolEffect>,
}

impl SchoolMenu {
    /// `FUN_0046f670`: one render into a cleared back buffer.
    pub fn render(
        &mut self,
        live: &mut [u8],
        frame: &SchoolFrame,
        context: SchoolContext,
    ) -> Result<SchoolView, TextLayoutError> {
        let mut view = SchoolView {
            pixels: None,
            palette: None,
            effects: Vec::new(),
        };
        if std::mem::take(&mut self.skip) {
            return Ok(view);
        }
        self.init(live, context, &mut view.effects);
        let (w, h) = (i32::from(frame.width), i32::from(frame.height));
        let mut pixels = vec![0u8; (w * h) as usize];
        let assets = frame.assets;
        // The canvas palette only matters to fills, which these screens do
        // not use.
        let palette = assets.cabin.unwrap_or(
            [PaletteEntry {
                red: 0,
                green: 0,
                blue: 0,
                flags: 0,
            }; 256],
        );
        let mut canvas = Canvas {
            pixels: &mut pixels,
            w,
            h,
            palette: &palette,
            y: 0,
        };
        let s = live[0x6c0];
        let level = usize::from((s >> 3) & 3).min(2);
        let task = s >> 5;
        match s & 7 {
            0 | 7 => {
                view.palette = assets.cabin;
                self.draw_menu(&mut canvas, frame)?;
            }
            1 => {
                view.palette = assets.cabin;
                canvas.sprite(assets.back.as_ref(), 0, 0);
                view.effects.push(SchoolEffect::Movie {
                    path: "game\\rallschl\\avi\\zoom.avi".to_owned(),
                    rect: [0, 0, w, h],
                });
                canvas.pixels.fill(0);
                canvas.sprite(assets.video.as_ref(), 0, 0);
                live[0x6c0] = (live[0x6c0] & 0xfa) | 2;
            }
            2 => {
                view.palette = assets.cabin;
                canvas.sprite(assets.video.as_ref(), 0, 0);
                let language = LANGUAGE_DIRS
                    .get(usize::from(frame.language))
                    .copied()
                    .unwrap_or("");
                view.effects.push(SchoolEffect::Speech {
                    path: format!("{}{language}{task}.wav", LEVEL_DIRS[level]),
                });
                view.effects.push(SchoolEffect::Movie {
                    path: format!("game\\rallschl\\{}{task}.avi", LEVEL_DIRS[level]),
                    rect: BRIEFING_RECT,
                });
                self.fade_out(context);
                live[0xb6] &= !0x10;
                live[0x6c0] = (live[0x6c0] & 0xfb) | 3;
            }
            3 => canvas.sprite(assets.video.as_ref(), 0, 0),
            4 => {
                view.palette = assets.results_palette;
                self.passed = draw_results(&mut canvas, frame, live)?;
            }
            5 => {
                view.palette = assets.complete_palette;
                draw_complete(&mut canvas, frame)?;
            }
            _ => {}
        }
        if std::mem::take(&mut self.skip) {
            self.done = false;
            return Ok(view);
        }
        view.pixels = Some(pixels);
        Ok(view)
    }

    /// `FUN_0046efd0`: the fan and the plant, then the frame step.
    fn draw_animation(&mut self, canvas: &mut Canvas, frame: &SchoolFrame) {
        let f = usize::try_from(self.animation.frame).unwrap_or(0);
        let assets = frame.assets;
        canvas.sprite(
            assets.fan.get(f).and_then(Option::as_ref),
            FAN_AT.0,
            FAN_AT.1,
        );
        canvas.sprite(
            assets.plant.get(f).and_then(Option::as_ref),
            PLANT_AT.0,
            PLANT_AT.1,
        );
        self.animation.step(frame.now_ms);
    }

    /// `FUN_0046ef90`: the backdrop, the animation and the typewriter menu
    /// (`FUN_0046f080`: one more character each frame, the text's left edge
    /// held while it reveals).
    fn draw_menu(
        &mut self,
        canvas: &mut Canvas,
        frame: &SchoolFrame,
    ) -> Result<(), TextLayoutError> {
        let assets = frame.assets;
        canvas.sprite(assets.back.as_ref(), 0, 0);
        self.draw_animation(canvas, frame);
        let menu = self.set.menu(self.current).clone();
        let labels: Vec<Vec<u8>> = menu
            .items
            .iter()
            .map(|item| (frame.text)(item.text_id))
            .collect();
        let title = (frame.text)(menu.title_id);
        let is_root = self.current == self.root;
        if self.typed != Some(self.current) {
            self.typed = Some(self.current);
            self.revealed = 0;
            self.total =
                if is_root { 0 } else { title.len() } + labels.iter().map(Vec::len).sum::<usize>();
        }
        if self.revealed <= self.total {
            self.revealed += 1;
        }
        let reveal = self.revealed;
        let w = canvas.w;
        // `0xd1eb - ((w(full)/2 - w(shown)/2) << 16) / W`.
        let left = |font: &CanvasFont, full: &[u8], shown: &[u8]| -> Result<i32, TextLayoutError> {
            let a = measure_line(&font.font, full)?;
            let b = measure_line(&font.font, shown)?;
            Ok(0xd1eb - ((a / 2 - b / 2) << 16) / w)
        };
        let mut used = 0;
        let mut y;
        if is_root {
            canvas.sprite(assets.logo.as_ref(), LOGO_AT.0, LOGO_AT.1);
            y = 0x6666;
        } else {
            let n = reveal.min(title.len());
            if let Some(font) = assets.blaktip.as_ref() {
                let x = left(font, &title, &title[..n])?;
                canvas.text(Some(font), &title[..n], x, 0x3333, 0x12)?;
            }
            used = n;
            if used >= reveal {
                return Ok(());
            }
            y = 0x4ccc;
        }
        let count = menu.items.len();
        for (i, (item, label)) in menu.items.iter().zip(&labels).enumerate() {
            let mut font = if i == menu.selected {
                assets.redtip.as_ref()
            } else {
                assets.blaktip.as_ref()
            };
            if i + 1 == count {
                y = 0x8ccc;
            }
            if !item.enabled {
                font = assets.greytip.as_ref();
            }
            let n = (reveal - used).min(label.len());
            let x = match font {
                Some(font) => left(font, label, &label[..n])?,
                None => 0xd1eb,
            };
            y = canvas.text(font, &label[..n], x, y, 0x12)?.0 + 0xc4;
            used += n;
            if used >= reveal {
                return Ok(());
            }
        }
        Ok(())
    }

    /// One frame of `FUN_004701a0`'s wipe: the shown frame copied back, the
    /// animation over it and wipe `k` at (444, 66).
    pub fn wipe_frame(&mut self, shown: &[u8], k: usize, frame: &SchoolFrame) -> Vec<u8> {
        let (w, h) = (i32::from(frame.width), i32::from(frame.height));
        let mut pixels = shown.to_vec();
        pixels.resize((w * h) as usize, 0);
        let palette = [PaletteEntry {
            red: 0,
            green: 0,
            blue: 0,
            flags: 0,
        }; 256];
        let mut canvas = Canvas {
            pixels: &mut pixels,
            w,
            h,
            palette: &palette,
            y: 0,
        };
        self.draw_animation(&mut canvas, frame);
        canvas.sprite(
            frame.assets.wipe.get(k).and_then(Option::as_ref),
            WIPE_AT.0,
            WIPE_AT.1,
        );
        pixels
    }
}

/// `FUN_0046fce0`: the marks out of 10 by category (`cfg[0x6bc..0x6bf]`
/// nibbles), the total and the pass or fail stamp; returns the pass
/// (`DAT_005f1dc4`: the total at least 75% of the maximum).
fn draw_results(
    canvas: &mut Canvas,
    frame: &SchoolFrame,
    live: &[u8],
) -> Result<bool, TextLayoutError> {
    let assets = frame.assets;
    let m = &live[0x6bc..0x6c0];
    let (loaded_level, loaded_task) = frame.loaded;
    let rows = if loaded_level == 2 && loaded_task == 2 {
        6
    } else {
        7
    };
    canvas.sprite(assets.results.as_ref(), 0, 0);
    let s = live[0x6c0];
    let name = (frame.text)(task_name((s >> 3) & 3, s >> 5));
    canvas.text(assets.co_bold.as_ref(), &name, 0x4ccc, 0x3d70, 0x31)?;
    canvas.text(
        assets.co_bold.as_ref(),
        &(frame.text)(0x116),
        0xd70a,
        0x3d70,
        0x32,
    )?;
    let (mut total, mut max) = (0i32, 0i32);
    for r in 0..rows {
        let y = 0xed9 * (r + 5);
        let (label, v) = match r {
            0 => ((frame.text)(0x117), m[0] & 15),
            1 => ((frame.text)(0x118), m[0] >> 4),
            2 => ((frame.text)(0x119), m[1] & 15),
            3 => ((frame.text)(0x11a), m[1] >> 4),
            4 => ((frame.text)(0x11b), m[2] & 15),
            5 => ((frame.text)(0x11c), m[2] >> 4),
            _ if loaded_level >= 2 && loaded_task >= 2 => {
                if loaded_level == 2 && loaded_task > 2 {
                    ((frame.text)(0x11e), m[3] >> 4)
                } else {
                    (b"------INVALID CATEGORY-----".to_vec(), 100)
                }
            }
            _ => ((frame.text)(0x11d), m[3] >> 4),
        };
        canvas.text(assets.fatwrit.as_ref(), &label, 0x4ccc, y, 0x21)?;
        let score = format!("{v}/10").into_bytes();
        canvas.text(assets.fatwrit.as_ref(), &score, 0xd70a, y, 0x22)?;
        total += i32::from(v);
        max += 10;
    }
    canvas.text(
        assets.co_bold.as_ref(),
        &(frame.text)(0x7c),
        0xc000,
        0xced9,
        0x23,
    )?;
    let sum = format!("{total}/{max}").into_bytes();
    canvas.text(assets.fatwrit.as_ref(), &sum, 0xd70a, 0xced9, 0x22)?;
    let passed = (total << 16) >= ((max * 3) << 14);
    let stamp = if passed { &assets.pass } else { &assets.fail };
    canvas.sprite(stamp.as_ref(), PASS_AT.0, PASS_AT.1);
    Ok(passed)
}

/// `FUN_0046f370`: the completed level's tasks, each ticked.
fn draw_complete(canvas: &mut Canvas, frame: &SchoolFrame) -> Result<(), TextLayoutError> {
    let assets = frame.assets;
    let level = frame.loaded.0;
    canvas.sprite(assets.complete_back.as_ref(), 0, 0);
    let title = (frame.text)(match level {
        0 => 0x2f,
        1 => 0x30,
        _ => 0x31,
    });
    let font = assets.com_font.as_ref();
    canvas.text(font, &title, 0x4ccc, 0x522d, 0x31)?;
    for i in 0..task_count(level) {
        let yy = 0x5916 + i32::from(i) * 0x1020;
        canvas.text(
            font,
            &(frame.text)(task_name(level, i)),
            0x4ccc,
            yy + 0x873,
            0x31,
        )?;
        let (w, h) = (canvas.w, canvas.h);
        canvas.sprite(assets.tick.as_ref(), (w * 0xa624) >> 16, (h * yy) >> 16);
    }
    canvas.text(font, &(frame.text)(0x78), 0x8000, 0xc560, 0x22)?;
    let line = (frame.text)(match level {
        0 => 0xf6,
        1 => 0xf7,
        _ => 0xf8,
    });
    canvas.text(font, &line, 0x8000, 0xa872, 0x12)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::descriptor_menu::bits;

    fn live() -> Vec<u8> {
        vec![0u8; 0x6c4]
    }

    fn ctx() -> SchoolContext {
        SchoolContext::default()
    }

    /// Updates until the fade is over (the module's fade advances on each
    /// update that waits on it, and on each menu step).
    fn settle(menu: &mut SchoolMenu, live: &mut [u8]) -> Vec<SchoolEffect> {
        let mut all = Vec::new();
        for _ in 0..20 {
            let (_, effects) = menu.update(live, 0, ctx());
            all.extend(effects);
        }
        all
    }

    #[test]
    fn the_locks_follow_the_unlocked_level_and_task() {
        let mut live = live();
        live[0x6c1] = 1 | (1 << 2); // level 1, task 1
        live[0x6c0] = (1 << 3) | (1 << 5); // at level 1, task 1
        let mut menu = SchoolMenu::default();
        let (_, effects) = menu.update(&mut live, 0, ctx());
        assert_eq!(effects[0], SchoolEffect::LoadAssets);
        let root = menu.set.menu(menu.root);
        let enabled: Vec<bool> = root.items.iter().map(|i| i.enabled).collect();
        assert_eq!(enabled, [true, true, false, true]);
        assert_eq!(root.selected, 1);
        let level1 = menu.set.menu(menu.levels[1]);
        let enabled: Vec<bool> = level1.items.iter().map(|i| i.enabled).collect();
        assert_eq!(
            enabled,
            [true, true, false, false, true],
            "the back item stays"
        );
        assert_eq!(level1.selected, 1);
        assert!(
            menu.set
                .menu(menu.levels[0])
                .items
                .iter()
                .all(|i| i.enabled)
        );
        assert_eq!(menu.set.menu(menu.levels[2]).items.len(), 6);
    }

    #[test]
    fn confirming_a_task_wipes_and_writes_the_school_byte() {
        let mut live = live();
        live[0x6c1] = 2 << 2; // tasks 0..=2 of level 0
        let mut menu = SchoolMenu::default();
        settle(&mut menu, &mut live);
        let (_, effects) = menu.update(&mut live, bits::CONFIRM, ctx());
        assert_eq!(
            effects,
            [SchoolEffect::Cue(cue::SELECT), SchoolEffect::Wipe]
        );
        assert_eq!(menu.current, menu.levels[0]);
        menu.update(&mut live, 0, ctx());
        menu.update(&mut live, bits::DOWN, ctx());
        menu.update(&mut live, 0, ctx());
        let (done, _) = menu.update(&mut live, bits::CONFIRM, ctx());
        assert!(!done);
        assert_eq!(menu.current, menu.sentinel);
        assert_eq!(live[0x6c0], 1 | (1 << 5), "status 1, level 0, task 1");
    }

    #[test]
    fn back_on_the_root_quits_after_the_fade() {
        let mut live = live();
        let mut menu = SchoolMenu::default();
        settle(&mut menu, &mut live);
        menu.update(&mut live, bits::BACK, ctx());
        assert_eq!(live[0x6c0] & 7, 7);
        assert!(!menu.quit_requested(), "the fade out is running");
        let mut done = false;
        for _ in 0..20 {
            done |= menu.update(&mut live, 0, ctx()).0;
        }
        assert!(done && menu.quit_requested());
    }

    #[test]
    fn the_back_item_wipes_twice() {
        let mut live = live();
        let mut menu = SchoolMenu::default();
        settle(&mut menu, &mut live);
        menu.update(&mut live, bits::CONFIRM, ctx());
        for _ in 0..3 {
            menu.update(&mut live, bits::DOWN, ctx());
            menu.update(&mut live, 0, ctx());
        }
        let (_, effects) = menu.update(&mut live, bits::CONFIRM, ctx());
        let wipes = effects.iter().filter(|e| **e == SchoolEffect::Wipe).count();
        assert_eq!(wipes, 2, "the activate and the back sound");
        assert_eq!(menu.current, menu.root);
    }

    fn results(live: &mut [u8], marks: [u8; 4]) -> SchoolMenu {
        live[0x6bc..0x6c0].copy_from_slice(&marks);
        let mut menu = SchoolMenu::default();
        settle(&mut menu, live);
        live[0x6c0] = (live[0x6c0] & 0xf8) | 4;
        let assets = SchoolAssets::default();
        let frame = SchoolFrame {
            assets: &assets,
            text: &|_| Vec::new(),
            width: 64,
            height: 48,
            now_ms: 0,
            language: 0,
            loaded: (0, 0),
        };
        menu.render(live, &frame, ctx()).unwrap();
        settle(&mut menu, live);
        menu.update(live, bits::CONFIRM, ctx());
        settle(&mut menu, live);
        menu
    }

    #[test]
    fn a_pass_moves_on_and_unlocks_the_next_task() {
        let mut live = live();
        // 7 rows of 8/10: 56/70 = 80%.
        let menu = results(&mut live, [0x88, 0x88, 0x88, 0x80]);
        assert_eq!(live[0x6c0], 1 << 5, "status 0 at task 1");
        assert_eq!((live[0x6c1] >> 2) & 7, 1);
        assert_eq!(menu.current, menu.levels[0]);
    }

    #[test]
    fn a_fail_keeps_the_task() {
        let mut live = live();
        // 7 rows of 7/10 is 49/70, short of 75%.
        results(&mut live, [0x77, 0x77, 0x77, 0x70]);
        assert_eq!(live[0x6c0], 0);
        assert_eq!(live[0x6c1], 0);
    }

    #[test]
    fn the_last_task_of_a_level_completes_it() {
        let mut live = live();
        live[0x6c0] = 2 << 5;
        live[0x6c1] = 2 << 2;
        let mut menu = results(&mut live, [0xaa, 0xaa, 0xaa, 0xa0]);
        assert_eq!(live[0x6c0], (1 << 3) | 5, "level 1, task 0, status 5");
        assert_eq!(live[0x6c1], 1, "level 1 unlocked from its first task");
        menu.update(&mut live, bits::CONFIRM, ctx());
        settle(&mut menu, &mut live);
        assert_eq!(live[0x6c0] & 7, 0);
    }

    #[test]
    fn the_animation_ping_pongs() {
        let mut a = Animation::default();
        let mut t = 0;
        let mut frames = Vec::new();
        for _ in 0..64 {
            t += 56;
            a.step(t);
            frames.push(a.frame);
        }
        assert_eq!(frames[29], 30);
        assert_eq!(frames[30], 29);
        assert_eq!(*frames.iter().min().unwrap(), 0);
        assert_eq!(*frames.iter().max().unwrap(), 30);
    }
}
