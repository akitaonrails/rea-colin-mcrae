//! The settings window's content, drawn with the SDK's widgets
//! (`rea_launcher_ui`) in CMR1's colours (`look`). It runs anywhere, so
//! the window is tested headless on the host (egui_kittest); the Windows
//! binary's `ui` module hosts it in a real window with the pad, the
//! folder picker, the rumble test and the portrait texture.
//!
//! Four tabs keep it short: Play (the game folder, one choice of look, the
//! saves folder and the language), Picture, Controls and Buttons. Every
//! control is a row of [`config::OPTIONS`]: its label, and its one-line
//! description under it, which is also its accessible description.

use std::path::{Path, PathBuf};

use rea_launcher_ui::egui::{self, Rect, Response, TextureHandle, pos2, vec2};
use rea_launcher_ui::spec::{mark_rendered, spec_row, toggle_text};
use rea_launcher_ui::theme::{body, decal};
use rea_launcher_ui::widgets::{self, CaptionStyle, Chip, Crop, LookCard};
use rea_launcher_ui::window::Layout;

use crate::config::{self, Scaler, Settings, TextureFilter};
use crate::edition;
use crate::form::{self, Form, Layout as Buttons, Look, Picture};
use crate::look::{self, ERROR, GOLD, INK, MUTED};

pub const TITLE: &str = "Colin McRae Rally";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tab {
    Play,
    Picture,
    Controls,
    Buttons,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Play, Tab::Picture, Tab::Controls, Tab::Buttons];

    /// The tab `by` places along (wrapping), for LB and RB.
    pub fn step(self, by: isize) -> Tab {
        let at = Tab::ALL.iter().position(|&t| t == self).unwrap_or(0) as isize;
        Tab::ALL[(at + by).rem_euclid(Tab::ALL.len() as isize) as usize]
    }
}

/// What the game folder holds, checked when its text changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    Empty,
    Found(&'static str),
    Problem(String),
}

pub fn check_game(folder: &str) -> Check {
    let folder = folder.trim();
    if folder.is_empty() {
        return Check::Empty;
    }
    let dir = Path::new(folder);
    if !dir.is_dir() {
        return Check::Problem("No such folder. Check the path, or use Browse… to pick it.".into());
    }
    let Some(exe) = std::fs::read_dir(dir).ok().and_then(|entries| {
        entries.flatten().map(|e| e.path()).find(|p| {
            p.file_name()
                .is_some_and(|n| n.eq_ignore_ascii_case("rally.exe"))
        })
    }) else {
        return Check::Problem(
            "There is no Rally.exe in this folder. Choose the folder the game is installed in."
                .into(),
        );
    };
    match std::fs::read(&exe) {
        Ok(bytes) => match edition::identify(&bytes) {
            Ok(e) => Check::Found(e.name),
            Err(digest) => Check::Problem(format!(
                "This Rally.exe is a version the launcher doesn't know (SHA-256 {}). \
                 Please report it: {}",
                edition::to_hex(&digest),
                edition::REPORT_URL
            )),
        },
        Err(e) => Check::Problem(format!(
            "Can't read Rally.exe ({e}). Check that the file isn't open elsewhere and that you may read it."
        )),
    }
}

/// Everything the window shows and edits.
pub struct State {
    pub form: Form,
    pub tab: Tab,
    pub check: Check,
    checked: String,
    pub screen: Option<(u32, u32)>,
    resolutions: Vec<(Option<(u32, u32)>, String)>,
    /// Xbox-style pads connected when the window opened.
    pub pads: usize,
    /// Shows the Linux-only row (the gamescope scaler).
    pub linux: bool,
    pub error: Option<String>,
    /// What the last rumble test found.
    pub rumble_test: Option<String>,
}

impl State {
    /// The window over a settings file's text; `None` on a first run
    /// (no file yet), which starts on the Steam Deck look (owner decision
    /// 2026-11-01, `AGENTS.md`).
    pub fn new(text: Option<&str>, linux: bool, screen: Option<(u32, u32)>, pads: usize) -> State {
        let form = match text {
            Some(text) => Form::new(&Settings::parse_file(text).0, screen),
            None => Form::first_run(screen),
        };
        State {
            check: check_game(&form.game),
            checked: form.game.clone(),
            form,
            tab: Tab::Play,
            screen,
            resolutions: form::resolutions(screen),
            pads,
            linux,
            error: None,
            rumble_test: None,
        }
    }

    /// Play is possible: a game folder with a known Rally.exe.
    pub fn ready(&self) -> bool {
        matches!(self.check, Check::Found(_))
    }

    /// Checks the game folder again after its text changed.
    pub fn refresh(&mut self) {
        if self.form.game != self.checked {
            self.checked = self.form.game.clone();
            self.check = check_game(&self.form.game);
        }
    }
}

/// What the window needs from the system: the folder picker (a title in,
/// a folder or nothing out) and the rumble test (a strength in, how many
/// pads took it out).
pub struct Platform<'a> {
    pub pick: &'a mut dyn FnMut(&str) -> Option<PathBuf>,
    pub rumble: &'a mut dyn FnMut(u32) -> usize,
}

/// Draws the whole window; true when Play was pressed (the caller saves).
pub fn draw(
    ui: &mut egui::Ui,
    state: &mut State,
    portrait: Option<&TextureHandle>,
    platform: &mut Platform<'_>,
) -> bool {
    let full = ui.max_rect();
    ui.painter().rect_filled(full, 0.0, look::BASE);
    let layout = Layout::new(full, look::PHOTO_W);
    draw_photo(ui, layout.photo, portrait);
    widgets::stripes(ui.painter(), full, &look::stripes());

    ui.scope_builder(egui::UiBuilder::new().max_rect(layout.body), |ui| {
        ui.label(egui::RichText::new(TITLE).font(decal(46.0)).color(INK));
        ui.add_space(-6.0);
        status(ui, state);
        ui.add_space(6.0);
        widgets::tabs_bar(
            ui,
            &[
                (Tab::Play, "Play"),
                (Tab::Picture, "Picture"),
                (Tab::Controls, "Controls"),
                (Tab::Buttons, "Buttons"),
            ],
            &mut state.tab,
        );
        ui.add_space(14.0);
        widgets::tab_body(ui, state.tab, |ui| match state.tab {
            Tab::Play => play_tab(ui, state, platform),
            Tab::Picture => picture_tab(ui, state),
            Tab::Controls => controls_tab(ui, state, platform),
            Tab::Buttons => buttons_tab(ui, state),
        });
    });
    let hint = if !state.ready() {
        "Play needs the game folder: choose it in the Play tab."
    } else if state.form.picture.resolution.is_some() {
        "In the game keep Resolution on 640×480; the race resolution replaces it."
    } else {
        "Settings are saved when you press Play."
    };
    let play = widgets::footer(ui, layout.footer, hint, state.ready());
    let play = state.ready() && play.clicked();
    state.refresh();
    play
}

/// What the portrait shows, for screen readers.
pub const PHOTO_ALT: &str = "Colin McRae, smiling, in a cap and racing suit";

/// The portrait's credit (THIRD_PARTY.md), two lines to fit the strip.
pub const PHOTO_CREDIT: [&str; 2] = [
    "Photo: Carolyn Williams, CC BY 2.0",
    "(via Wikimedia Commons)",
];

fn draw_photo(ui: &mut egui::Ui, rect: Rect, portrait: Option<&TextureHandle>) {
    // Fill the height; keep the left of the picture (Colin's face).
    widgets::hero_image(ui.painter(), rect, portrait, Crop::Start, look::BASE);
    // The caption sits on a solid band, never on the photo itself.
    let band = Rect::from_min_max(pos2(rect.left(), rect.bottom() - 104.0), rect.max);
    ui.painter().rect_filled(band, 0.0, look::BASE);
    widgets::photo_caption(
        ui,
        rect,
        PHOTO_ALT,
        &[
            ("Colin McRae", CaptionStyle::Title),
            ("1968–2007, World Rally Champion 1995", CaptionStyle::Text),
            (PHOTO_CREDIT[0], CaptionStyle::Credit),
            (PHOTO_CREDIT[1], CaptionStyle::Credit),
        ],
    );
}

fn status(ui: &mut egui::Ui, state: &State) {
    let (text, color) = match (&state.error, &state.check) {
        (Some(e), _) => (e.clone(), ERROR),
        (None, Check::Found(_)) => ("Ready. Press Play or Enter.".to_string(), MUTED),
        (None, _) => (
            "First, show the launcher where the game is.".to_string(),
            MUTED,
        ),
    };
    ui.label(egui::RichText::new(text).font(body(14.0)).color(color));
}

/// A row of chips the SDK's `spec_row` can't draw: a choice list built at
/// run time (the screen's size, a saved value outside the steps). Label,
/// chips, then the row's description.
fn option_chips<T: PartialEq + Clone>(
    ui: &mut egui::Ui,
    key: &'static str,
    chips: &[Chip<'_, T>],
    value: &mut T,
) -> Response {
    let row = config::option(key);
    mark_rendered(ui.ctx(), row.key);
    let mut response = None;
    ui.vertical(|ui| {
        ui.horizontal_top(|ui| {
            widgets::field(ui, row.label);
            let r = widgets::chips_with(ui, chips, value);
            widgets::name_control(&r, Some(row.label), Some(row.description));
            response = Some(r);
        });
        note_under(ui, row.description);
    });
    response.expect("drawn")
}

/// A description under a row, indented to the controls.
fn note_under(ui: &mut egui::Ui, text: &str) {
    ui.horizontal_top(|ui| {
        ui.add_space(rea_launcher_ui::theme::metrics(ui.ctx()).field_width + 8.0);
        ui.vertical(|ui| widgets::note(ui, text));
    });
    ui.add_space(6.0);
}

fn chips_of<T: Clone>(choices: &[(T, String)]) -> Vec<Chip<'_, T>> {
    choices
        .iter()
        .map(|(v, label)| Chip::new(v.clone(), label))
        .collect()
}

fn owned<T: Copy>(choices: &[(T, &str)]) -> Vec<(T, String)> {
    choices.iter().map(|(v, l)| (*v, l.to_string())).collect()
}

/// A tri-state toggle's value from its text (empty: unset).
fn toggle_of(text: &str) -> Option<bool> {
    match text {
        "on" => Some(true),
        "off" => Some(false),
        _ => None,
    }
}

/// A spec row edited through its `launcher.cfg` text; the new text when
/// it changed.
fn spec_choice(ui: &mut egui::Ui, key: &str, current: &str) -> Option<String> {
    let mut text = current.to_string();
    spec_row(ui, config::option(key), &mut text)
        .changed()
        .then_some(text)
}

/// A plain on/off row of a key whose `launcher.cfg` value is not a toggle
/// (`hd_art` also takes a folder): a switch, its description under it.
fn switch_row(ui: &mut egui::Ui, key: &'static str, on: &mut bool) -> Response {
    let row = config::option(key);
    mark_rendered(ui.ctx(), row.key);
    let response = widgets::toggle(ui, on, row.label, None);
    widgets::name_control(&response, Some(row.label), Some(row.description));
    widgets::note(ui, row.description);
    ui.add_space(6.0);
    response
}

fn play_tab(ui: &mut egui::Ui, state: &mut State, platform: &mut Platform<'_>) {
    let game = config::option("game");
    mark_rendered(ui.ctx(), game.key);
    widgets::heading(ui, game.label);
    let game_hint = if state.linux {
        "The folder with your Rally.exe, e.g. Z:\\home\\you\\Games\\Colin McRae Rally"
    } else {
        "The folder with your installed Rally.exe"
    };
    if widgets::folder_field(ui, &mut state.form.game, game.label, game_hint, || {
        (platform.pick)("Choose the folder with your Rally.exe")
    }) {
        state.error = None;
        state.refresh();
    }
    let (note, color) = match &state.check {
        Check::Empty => (
            "The game folder is only read, never changed.".to_string(),
            MUTED,
        ),
        Check::Found(name) => (format!("Found: {name}"), GOLD),
        Check::Problem(p) => (p.clone(), ERROR),
    };
    ui.add(egui::Label::new(egui::RichText::new(note).font(body(13.0)).color(color)).wrap());
    ui.add_space(10.0);

    let looks = config::option("look");
    mark_rendered(ui.ctx(), looks.key);
    widgets::heading(ui, looks.label);
    ui.add_space(6.0);
    let enhanced_note = match Picture::enhanced(state.screen).resolution {
        Some((w, h)) => format!("Your screen's {w}×{h}, smoother."),
        None => "Smooth textures, a longer view.".to_string(),
    };
    let best = form::recommended(state.screen);
    let cards = [
        (
            Look::Original,
            "Original 1998",
            "640×480, as the game shipped.".to_string(),
        ),
        (Look::Enhanced, "Enhanced", enhanced_note),
        (
            Look::SteamDeck,
            "Steam Deck",
            "1280×800 at 60 fps, easy on the battery.".to_string(),
        ),
        (
            Look::Custom,
            "Custom",
            "Pick each setting in Picture.".to_string(),
        ),
    ];
    let gap = 10.0;
    let w = (ui.available_width() - 3.0 * gap) / 4.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for (look, title, note) in cards {
            let card = LookCard {
                title,
                note: &note,
                detail: None,
                selected: state.form.look == look,
                badge: (look == best).then_some("Recommended"),
                title_size: 0.0,
                disabled: None,
            };
            if widgets::look_card(ui, vec2(w, 104.0), &card).clicked() {
                state.form.set_look(look, state.screen);
                if look == Look::Custom {
                    state.tab = Tab::Picture;
                }
            }
        }
    });
    ui.add_space(14.0);

    let saves = config::option("saves");
    mark_rendered(ui.ctx(), saves.key);
    widgets::heading(ui, saves.label);
    widgets::folder_field(
        ui,
        &mut state.form.saves,
        saves.label,
        "Saved Games\\Colin McRae Rally (the default)",
        || (platform.pick)("Choose where your saves are kept"),
    );
    widgets::note(
        ui,
        "Leave empty for the default. Saves are copied in before the game starts and back \
         when it ends.",
    );
    ui.add_space(10.0);
    if let Some(language) = spec_choice(
        ui,
        "language",
        state.form.language.as_deref().unwrap_or_default(),
    ) {
        state.form.language = Some(language).filter(|l| !l.is_empty());
    }
}

fn picture_tab(ui: &mut egui::Ui, state: &mut State) {
    let before = state.form.picture;
    let p = &mut state.form.picture;
    let resolutions = form::with_value(&state.resolutions, &p.resolution, |r| match r {
        Some((w, h)) => format!("{w}×{h}"),
        None => "Original".into(),
    });
    option_chips(ui, "resolution", &chips_of(&resolutions), &mut p.resolution);
    if let Some(on) = spec_choice(ui, "widescreen", toggle_text(p.widescreen)) {
        p.widescreen = on == "on";
    }
    let distances = form::with_value(&owned(&form::DISTANCES), &p.draw_distance, |d| {
        format!("{}×", config::factor_value(*d))
    });
    option_chips(
        ui,
        "draw_distance",
        &chips_of(&distances),
        &mut p.draw_distance,
    );
    if let Some(filter) = spec_choice(ui, "texture_filter", p.texture_filter.key()) {
        p.texture_filter = TextureFilter::ALL
            .into_iter()
            .find(|f| f.key() == filter)
            .unwrap_or(TextureFilter::Original);
    }
    let rates = form::with_value(&owned(&form::FRAME_RATES), &p.fps_limit, |f| match f {
        Some(f) => f.to_string(),
        None => "Original".into(),
    });
    option_chips(ui, "fps_limit", &chips_of(&rates), &mut p.fps_limit);
    let vsync = p.vsync.map(toggle_text).unwrap_or_default();
    if let Some(text) = spec_choice(ui, "vsync", vsync) {
        p.vsync = toggle_of(&text);
    }
    if let Some(on) = spec_choice(ui, "smooth_distance", toggle_text(p.smooth_distance)) {
        p.smooth_distance = on == "on";
    }
    switch_row(ui, "hd_art", &mut p.hd_art);
    if state.linux
        && let Some(scaler) = spec_choice(ui, "scaler", state.form.scaler.key())
    {
        state.form.scaler = Scaler::ALL
            .into_iter()
            .find(|s| s.key() == scaler)
            .unwrap_or(Scaler::Linear);
    }
    if state.form.picture != before {
        state.form.look = Form::look_of(&state.form.picture, state.screen);
    }
}

fn controls_tab(ui: &mut egui::Ui, state: &mut State, platform: &mut Platform<'_>) {
    if let Some(on) = spec_choice(ui, "gamepad_axes", toggle_text(state.form.gamepad_axes)) {
        state.form.gamepad_axes = on == "on";
    }
    let rumble = config::option("rumble");
    mark_rendered(ui.ctx(), rumble.key);
    let rumbles = form::with_value(&owned(&form::RUMBLES), &state.form.rumble, |r| {
        format!("{r} %")
    });
    ui.vertical(|ui| {
        ui.horizontal_top(|ui| {
            widgets::field(ui, rumble.label);
            let r = widgets::chips_with(ui, &chips_of(&rumbles), &mut state.form.rumble);
            widgets::name_control(&r, Some(rumble.label), Some(rumble.description));
            ui.add_space(8.0);
            if state.form.rumble > 0 && widgets::outline_button(ui, "Test").clicked() {
                let took = (platform.rumble)(state.form.rumble);
                state.rumble_test = Some(match took {
                    0 => "No pad took the rumble. Under Wine the pad's device must be writable \
                          (most desktops allow it); check that it is in X-input mode."
                        .to_string(),
                    _ => "Sent: your pad should have shaken for half a second. In a race it \
                          shakes on impacts."
                        .to_string(),
                });
            }
        });
        note_under(ui, rumble.description);
    });
    if let Some(result) = &state.rumble_test {
        ui.add(egui::Label::new(egui::RichText::new(result).font(body(13.0)).color(GOLD)).wrap());
    }
    let pads = match state.pads {
        0 => "Rumble and separate trigger pedals need an Xbox-style (XInput) pad; none is \
              connected now. On 8BitDo pads use X-input mode."
            .to_string(),
        1 => "One Xbox-style pad connected.".to_string(),
        n => format!("{n} Xbox-style pads connected."),
    };
    widgets::note(ui, &pads);
    ui.add_space(12.0);
    if let Some(on) = spec_choice(ui, "close_quits", toggle_text(state.form.close_quits)) {
        state.form.close_quits = on == "on";
    }
}

fn buttons_tab(ui: &mut egui::Ui, state: &mut State) {
    let buttons = config::option("buttons");
    mark_rendered(ui.ctx(), buttons.key);
    widgets::heading(ui, buttons.label);
    ui.add_space(6.0);
    let cards = [
        (
            Buttons::Original,
            "Original 1998",
            "As shipped in 1998.",
            false,
        ),
        (Buttons::Modern, "Modern", "Like today's racers.", true),
        (Buttons::Custom, "Custom", "Your own choice.", false),
    ];
    let gap = 12.0;
    let w = (ui.available_width() - 2.0 * gap) / 3.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for (layout, title, note, recommended) in cards {
            let card = LookCard {
                title,
                note,
                detail: None,
                selected: state.form.layout == layout,
                badge: recommended.then_some("Recommended"),
                title_size: 0.0,
                disabled: None,
            };
            if widgets::look_card(ui, vec2(w, 70.0), &card).clicked() {
                state.form.set_layout(layout);
            }
        }
    });
    widgets::note(
        ui,
        "Names follow Xbox-style pads (A is button 1). The d-pad moves through the menus.",
    );
    ui.spacing_mut().item_spacing.y = 4.0;
    let options: Vec<_> = form::PAD_BUTTONS[..8]
        .iter()
        .map(|(b, l)| Chip::new(*b, l))
        .collect();
    for (k, name) in form::ACTIONS {
        ui.horizontal_top(|ui| {
            widgets::field(ui, name);
            let mut button = state.form.pad_button(k);
            let r = widgets::chips_with(ui, &options, &mut button);
            widgets::name_control(&r, Some(name), None);
            state.form.set_pad_button(k, button);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rea_launcher::cfg::spec;
    use rea_launcher_ui::egui::accesskit::Role;
    use rea_launcher_ui::testing::{self, Harness, Queryable};

    const SCREEN: Option<(u32, u32)> = Some((2560, 1440));

    fn temp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("cmr-window-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    struct App {
        state: State,
        played: u32,
        rumbled: Vec<u32>,
    }

    fn open(text: Option<&str>, linux: bool) -> Harness<'static, App> {
        let app = App {
            state: State::new(text, linux, SCREEN, 0),
            played: 0,
            rumbled: Vec::new(),
        };
        testing::harness_sized(
            &look::PALETTE,
            &look::METRICS,
            look::SIZE,
            app,
            |ui, app| {
                let mut rumbled = Vec::new();
                let mut platform = Platform {
                    pick: &mut |_| None,
                    rumble: &mut |n| {
                        rumbled.push(n);
                        1
                    },
                };
                if draw(ui, &mut app.state, None, &mut platform) {
                    app.played += 1;
                }
                app.rumbled.extend(rumbled);
            },
        )
    }

    /// Scrolls a control into view, then clicks it (a long tab scrolls).
    fn press(ui: &mut Harness<'static, App>, role: Role, label: &str) {
        ui.get_by_role_and_label(role, label).scroll_to_me();
        ui.run();
        ui.get_by_role_and_label(role, label).click();
        ui.run();
    }

    fn tab(ui: &mut Harness<'static, App>, name: &str) {
        ui.get_by_role_and_label(Role::Tab, name).click();
        ui.run();
    }

    fn saved(ui: &Harness<'static, App>) -> Settings {
        let (s, warnings) = Settings::parse_file(&ui.state().state.form.save(""));
        assert!(warnings.is_empty(), "{warnings:?}");
        s
    }

    #[test]
    fn every_option_has_a_control_in_the_window() {
        let mut ui = open(Some(""), true);
        for name in ["Picture", "Controls", "Buttons", "Play"] {
            tab(&mut ui, name);
        }
        spec::assert_parity(
            config::OPTIONS,
            &rea_launcher_ui::spec::rendered_keys(&ui.ctx),
        );
    }

    #[test]
    fn each_tab_renders_its_labelled_controls() {
        let mut ui = open(Some(""), true);
        let game = ui.get_by_role_and_label(Role::TextInput, "Game folder");
        assert_eq!(
            testing::description(&game).as_deref(),
            Some("The folder with your Rally.exe, e.g. Z:\\home\\you\\Games\\Colin McRae Rally")
        );
        ui.get_by_role_and_label(Role::TextInput, "Saves");
        ui.get_by_role_and_label(Role::RadioButton, "Enhanced");
        ui.get_by_role_and_label(Role::RadioGroup, "Language");
        tab(&mut ui, "Picture");
        for key in [
            "resolution",
            "draw_distance",
            "texture_filter",
            "fps_limit",
            "vsync",
            "scaler",
        ] {
            let row = config::option(key);
            let node = ui.get_by_role_and_label(Role::RadioGroup, row.label);
            assert_eq!(
                testing::description(&node).as_deref(),
                Some(row.description),
                "{key}"
            );
        }
        for key in ["widescreen", "smooth_distance", "hd_art"] {
            let row = config::option(key);
            let node = ui.get_by_role_and_label(Role::Switch, row.label);
            assert_eq!(
                testing::description(&node).as_deref(),
                Some(row.description),
                "{key}"
            );
        }
        tab(&mut ui, "Controls");
        ui.get_by_role_and_label(Role::Switch, "Gamepad pedals");
        ui.get_by_role_and_label(Role::RadioGroup, "Rumble");
        ui.get_by_role_and_label(Role::Switch, "Closing quits");
        tab(&mut ui, "Buttons");
        ui.get_by_role_and_label(Role::RadioButton, "Modern");
        for (_, action) in form::ACTIONS {
            ui.get_by_role_and_label(Role::RadioGroup, action);
        }
    }

    #[test]
    fn the_portrait_and_its_full_credit_reach_screen_readers() {
        let ui = open(Some(""), false);
        ui.get_by_role_and_label(Role::Image, PHOTO_ALT);
        ui.get_by_role_and_label(
            Role::Label,
            &format!(
                "Colin McRae. 1968–2007, World Rally Champion 1995. {}. {}",
                PHOTO_CREDIT[0], PHOTO_CREDIT[1]
            ),
        );
    }

    #[test]
    fn the_scaler_is_only_for_cmr_play() {
        let mut ui = open(Some(""), false);
        tab(&mut ui, "Picture");
        assert!(ui.query_by_label(config::option("scaler").label).is_none());
    }

    #[test]
    fn clicking_changes_the_saved_values() {
        let mut ui = open(Some(""), true);
        tab(&mut ui, "Picture");
        let click = |ui: &mut Harness<'static, App>, label: &str| {
            press(ui, Role::RadioButton, label);
        };
        click(&mut ui, "1440p");
        click(&mut ui, "2×");
        // Textures' Sharp comes first; the scaler has one too.
        let sharp: Vec<_> = ui
            .get_all_by_role_and_label(Role::RadioButton, "Sharp")
            .collect();
        sharp[0].click();
        ui.run();
        click(&mut ui, "120");
        click(&mut ui, "FSR");
        for switch in ["Widescreen", "Ease view distance", "HD art"] {
            press(&mut ui, Role::Switch, switch);
        }
        let s = saved(&ui);
        assert_eq!(s.resolution, Some((2560, 1440)));
        assert_eq!(s.draw_distance, Some(0x20000));
        assert_eq!(s.texture_filter, Some(TextureFilter::Sharp));
        assert_eq!(s.fps_limit, Some(120));
        assert_eq!(s.scaler, Some(Scaler::Fsr));
        assert_eq!(
            (s.widescreen, s.smooth_distance, s.hd_on()),
            (Some(true), Some(true), true)
        );
        assert_eq!(ui.state().state.form.look, Look::Custom);

        tab(&mut ui, "Controls");
        click(&mut ui, "Strong");
        press(&mut ui, Role::Button, "Test");
        assert_eq!(ui.state().rumbled, [100]);
        assert!(ui.state().state.rumble_test.is_some());
        press(&mut ui, Role::Switch, "Gamepad pedals");
        press(&mut ui, Role::Switch, "Closing quits");
        let s = saved(&ui);
        assert_eq!(s.rumble, Some(100));
        assert_eq!((s.gamepad_axes, s.close_quits), (Some(false), Some(false)));

        tab(&mut ui, "Buttons");
        click(&mut ui, "Original 1998");
        assert_eq!(saved(&ui).buttons, Some(config::ORIGINAL_BUTTONS));

        tab(&mut ui, "Play");
        click(&mut ui, "Deutsch");
        assert_eq!(saved(&ui).language.as_deref(), Some("GERMAN"));
    }

    #[test]
    fn vsync_as_the_game_unsets_and_opening_picture_changes_nothing() {
        let mut ui = open(Some("vsync = on\n"), false);
        tab(&mut ui, "Picture");
        assert_eq!(saved(&ui).vsync, Some(true));
        press(&mut ui, Role::RadioButton, config::AS_THE_GAME);
        assert_eq!(saved(&ui).vsync, None);
        // The Original look leaves vsync to the game; opening the tabs
        // must not turn that into Custom.
        let mut ui = open(Some("resolution = original\n"), false);
        assert_eq!(ui.state().state.form.look, Look::Original);
        tab(&mut ui, "Picture");
        tab(&mut ui, "Controls");
        assert_eq!(ui.state().state.form.look, Look::Original);
    }

    #[test]
    fn a_saved_value_outside_the_chips_is_shown_and_kept() {
        let mut ui = open(
            Some("fps_limit = 75\nresolution = 1600x900\ndraw_distance = 1.75\nrumble = 45\n"),
            false,
        );
        tab(&mut ui, "Picture");
        for label in ["75", "1600×900", "1.75×"] {
            assert!(
                testing::checked(&ui.get_by_role_and_label(Role::RadioButton, label)),
                "{label}"
            );
        }
        tab(&mut ui, "Controls");
        assert!(testing::checked(
            &ui.get_by_role_and_label(Role::RadioButton, "45 %")
        ));
        let s = saved(&ui);
        assert_eq!(
            (s.fps_limit, s.resolution, s.draw_distance, s.rumble),
            (Some(75), Some((1600, 900)), Some(0x1c000), Some(45))
        );
    }

    #[test]
    fn looks_set_the_picture_and_play_needs_a_known_game() {
        let mut ui = open(None, false);
        assert_eq!(ui.state().state.form.look, Look::SteamDeck, "first run");
        ui.get_by_role_and_label(Role::RadioButton, "Original 1998")
            .click();
        ui.run();
        assert_eq!(saved(&ui).resolution, None);
        // Play is disabled until the folder holds a known game, and says
        // why; an unknown Rally.exe gives its SHA-256 and the report link.
        let play = ui.get_by_role_and_label(Role::Button, "Play");
        assert!(testing::disabled(&play));
        play.click();
        ui.run();
        assert_eq!(ui.state().played, 0);
        let game = temp("play-game");
        std::fs::write(game.join("Rally.exe"), b"not a known build").unwrap();
        ui.state_mut().state.form.game = game.display().to_string();
        ui.run();
        let Check::Problem(problem) = &ui.state().state.check else {
            panic!("an unknown build is not ready");
        };
        assert!(problem.contains(&edition::to_hex(&edition::sha256(b"not a known build"))));
        assert!(problem.contains(edition::REPORT_URL));
        ui.get_by_role_and_label(Role::Button, "Play").click();
        ui.run();
        assert_eq!(ui.state().played, 0);
        std::fs::remove_dir_all(game).unwrap();
    }

    #[test]
    fn a_game_folder_with_a_hostile_name_is_checked() {
        let game = temp("José/Jogos d'Ação/Colin McRae Rally");
        std::fs::write(game.join("RALLY.EXE"), b"x").unwrap();
        let Check::Problem(p) = check_game(&game.display().to_string()) else {
            panic!("found");
        };
        assert!(p.contains("doesn't know"), "{p}");
        assert!(matches!(
            check_game(&game.join("missing").display().to_string()),
            Check::Problem(p) if p.starts_with("No such folder")
        ));
        std::fs::remove_dir_all(temp("José")).unwrap();
    }
}
