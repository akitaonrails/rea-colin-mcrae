//! The settings window (E13): one window for Windows and for Linux (shown
//! through Wine by `cmr-play`). What it edits and saves is `form::Form`;
//! this module only draws it.
//!
//! Look: the 555 Impreza livery from the game's cover, rally blue with one
//! gold accent for whatever is chosen, and a condensed italic title like a
//! door decal. Three tabs keep it short: Play (the game folder and one
//! choice of look), Picture (each picture setting) and Controls.

use std::path::{Path, PathBuf};

use cmr_launcher::config::Settings;
use cmr_launcher::edition;
use cmr_launcher::form::{self, Form, Look, Picture};
use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Key, Mesh,
    Rect, Sense, Shape, Stroke, StrokeKind, TextureHandle, Vec2, pos2, vec2,
};

use crate::win32;

const BLUE: Color32 = Color32::from_rgb(0x0f, 0x2e, 0x6e);
const PANEL: Color32 = Color32::from_rgb(0x16, 0x3a, 0x85);
const LINE: Color32 = Color32::from_rgb(0x2b, 0x4f, 0x9e);
const GOLD: Color32 = Color32::from_rgb(0xf5, 0xc4, 0x00);
const INK: Color32 = Color32::from_rgb(0xee, 0xf2, 0xfb);
const MUTED: Color32 = Color32::from_rgb(0xa7, 0xb6, 0xd9);
const ON_GOLD: Color32 = Color32::from_rgb(0x10, 0x24, 0x4f);
const ERROR: Color32 = Color32::from_rgb(0xff, 0x8a, 0x80);

const SIZE: Vec2 = vec2(880.0, 640.0);
const PHOTO_W: f32 = 300.0;

/// How the window was left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Play pressed: the settings are saved.
    Play,
    /// Closed without playing.
    Closed,
}

/// Shows the window over `settings_path` (its text, or the template for a
/// first run). `linux` adds the gamescope scaler choice. Errors when no
/// window can be made (no OpenGL); the caller then goes on without it.
pub fn show(settings_path: &Path, text: &str, linux: bool) -> Result<Outcome, String> {
    let mut outcome = Outcome::Closed;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Colin McRae Rally")
            .with_inner_size(SIZE)
            .with_min_inner_size(SIZE)
            .with_max_inner_size(SIZE)
            .with_resizable(false)
            .with_maximize_button(false),
        centered: true,
        ..Default::default()
    };
    let path = settings_path.to_path_buf();
    let text = text.to_string();
    eframe::run_native(
        "Colin McRae Rally",
        options,
        Box::new(|cc| {
            set_style(&cc.egui_ctx);
            Ok(Box::new(Window::new(
                &cc.egui_ctx,
                path,
                text,
                linux,
                &mut outcome,
            )))
        }),
    )
    .map_err(|e| e.to_string())?;
    Ok(outcome)
}

fn set_style(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let font = |bytes: &'static [u8]| std::sync::Arc::new(FontData::from_static(bytes));
    fonts.font_data.insert(
        "barlow".into(),
        font(include_bytes!("../assets/fonts/Barlow-Regular.ttf")),
    );
    fonts.font_data.insert(
        "barlow-semibold".into(),
        font(include_bytes!("../assets/fonts/Barlow-SemiBold.ttf")),
    );
    fonts.font_data.insert(
        "condensed".into(),
        font(include_bytes!(
            "../assets/fonts/BarlowCondensed-SemiBold.ttf"
        )),
    );
    fonts.font_data.insert(
        "decal".into(),
        font(include_bytes!(
            "../assets/fonts/BarlowCondensed-BlackItalic.ttf"
        )),
    );
    // Barlow everywhere (egui's own fonts are left out to keep the
    // launcher small); the named families are the other weights.
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts.families.insert(family, vec!["barlow".into()]);
    }
    for name in ["barlow-semibold", "condensed", "decal"] {
        fonts.families.insert(
            FontFamily::Name(name.into()),
            vec![name.into(), "barlow".into()],
        );
    }
    ctx.set_fonts(fonts);

    // One look whatever the system theme is.
    ctx.set_theme(egui::Theme::Dark);
    ctx.all_styles_mut(|style| {
        let v = &mut style.visuals;
        *v = egui::Visuals::dark();
        v.panel_fill = BLUE;
        v.extreme_bg_color = PANEL;
        v.text_edit_bg_color = Some(PANEL);
        v.override_text_color = Some(INK);
        v.selection.bg_fill = GOLD.gamma_multiply(0.5);
        v.selection.stroke = Stroke::new(1.0, GOLD);
        v.text_cursor.stroke = Stroke::new(2.0, GOLD);
        for w in [
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.bg_stroke = Stroke::new(1.0, LINE);
            w.corner_radius = CornerRadius::same(6);
        }
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, MUTED);
        v.widgets.active.bg_stroke = Stroke::new(1.0, GOLD);
        style.spacing.item_spacing = vec2(8.0, 8.0);
    });
}

fn font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

fn named(name: &str, size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(name.into()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Play,
    Picture,
    Controls,
}

/// What the game folder holds, checked when its text changes.
enum Check {
    Empty,
    Found(&'static str),
    Problem(String),
}

fn check_game(folder: &str) -> Check {
    let folder = folder.trim();
    if folder.is_empty() {
        return Check::Empty;
    }
    let dir = Path::new(folder);
    if !dir.is_dir() {
        return Check::Problem("No such folder.".into());
    }
    let Some(exe) = std::fs::read_dir(dir).ok().and_then(|entries| {
        entries.flatten().map(|e| e.path()).find(|p| {
            p.file_name()
                .is_some_and(|n| n.eq_ignore_ascii_case("rally.exe"))
        })
    }) else {
        return Check::Problem("There is no Rally.exe in this folder.".into());
    };
    match std::fs::read(&exe) {
        Ok(bytes) => match edition::identify(&bytes) {
            Ok(e) => Check::Found(e.name),
            Err(_) => {
                Check::Problem("This Rally.exe is a version the launcher doesn't know.".into())
            }
        },
        Err(e) => Check::Problem(format!("Can't read Rally.exe: {e}")),
    }
}

struct Window<'a> {
    form: Form,
    tab: Tab,
    check: Check,
    checked: String,
    screen: Option<(u32, u32)>,
    resolutions: Vec<(Option<(u32, u32)>, String)>,
    pads: usize,
    linux: bool,
    portrait: Option<TextureHandle>,
    path: PathBuf,
    text: String,
    error: Option<String>,
    outcome: &'a mut Outcome,
}

impl<'a> Window<'a> {
    fn new(
        ctx: &egui::Context,
        path: PathBuf,
        text: String,
        linux: bool,
        outcome: &'a mut Outcome,
    ) -> Window<'a> {
        let screen = win32::screen_size();
        let (settings, _) = Settings::parse_file(&text);
        let form = Form::new(&settings, screen);
        let check = check_game(&form.game);
        Window {
            checked: form.game.clone(),
            check,
            form,
            tab: Tab::Play,
            screen,
            resolutions: form::resolutions(screen),
            pads: win32::xinput_pads().len(),
            linux,
            portrait: load_portrait(ctx),
            path,
            text,
            error: None,
            outcome,
        }
    }

    fn ready(&self) -> bool {
        matches!(self.check, Check::Found(_))
    }

    fn play(&mut self, ctx: &egui::Context) {
        if !self.ready() {
            self.tab = Tab::Play;
            return;
        }
        let saved = self.form.save(&self.text);
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match std::fs::write(&self.path, saved) {
            Ok(()) => {
                *self.outcome = Outcome::Play;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Err(e) => self.error = Some(format!("Can't save the settings: {e}")),
        }
    }
}

fn load_portrait(ctx: &egui::Context) -> Option<TextureHandle> {
    let mut decoder = jpeg_decoder::Decoder::new(&include_bytes!("../assets/portrait.jpg")[..]);
    let rgb = decoder.decode().ok()?;
    let info = decoder.info()?;
    let size = [usize::from(info.width), usize::from(info.height)];
    (rgb.len() == size[0] * size[1] * 3).then(|| {
        ctx.load_texture(
            "portrait",
            egui::ColorImage::from_rgb(size, &rgb),
            egui::TextureOptions::LINEAR,
        )
    })
}

impl eframe::App for Window<'_> {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let full = ui.max_rect();
        ui.painter().rect_filled(full, 0.0, BLUE);
        self.draw_photo(ui, full);
        draw_decal(ui, full);

        let content = Rect::from_min_max(
            pos2(full.left() + PHOTO_W + 12.0, full.top() + 24.0),
            pos2(full.right() - 32.0, full.bottom() - 24.0),
        );
        let footer_h = 52.0;
        let body = Rect::from_min_max(
            content.min,
            pos2(content.right(), content.bottom() - footer_h - 16.0),
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
            ui.label(
                egui::RichText::new("Colin McRae Rally")
                    .font(named("decal", 46.0))
                    .color(INK),
            );
            ui.add_space(-6.0);
            self.status(ui);
            ui.add_space(6.0);
            self.tabs(ui);
            ui.add_space(14.0);
            match self.tab {
                Tab::Play => self.play_tab(ui),
                Tab::Picture => self.picture_tab(ui),
                Tab::Controls => self.controls_tab(ui),
            }
        });
        let footer = Rect::from_min_max(
            pos2(content.left(), content.bottom() - footer_h),
            content.max,
        );
        self.footer(ui, footer, &ctx);

        if ctx.input(|i| i.key_pressed(Key::Enter)) && !ctx.egui_wants_keyboard_input() {
            self.play(&ctx);
        }
        if self.form.game != self.checked {
            self.checked = self.form.game.clone();
            self.check = check_game(&self.form.game);
        }
    }
}

impl Window<'_> {
    fn draw_photo(&self, ui: &egui::Ui, full: Rect) {
        let rect = Rect::from_min_size(full.min, vec2(PHOTO_W, full.height()));
        let p = ui.painter();
        if let Some(tex) = &self.portrait {
            // Fill the height; keep the left of the picture (Colin's face).
            let [w, h] = tex.size();
            let shown_w = rect.height() * w as f32 / h as f32;
            let u = (rect.width() / shown_w).min(1.0);
            p.image(
                tex.id(),
                rect,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(u, 1.0)),
                Color32::WHITE,
            );
        }
        // Blue fades join the photo to the page.
        gradient(
            p,
            Rect::from_min_max(
                pos2(rect.left() + rect.width() * 0.55, rect.top()),
                rect.max,
            ),
            true,
        );
        gradient(
            p,
            Rect::from_min_max(pos2(rect.left(), rect.bottom() - 130.0), rect.max),
            false,
        );
        let x = rect.left() + 20.0;
        let y = rect.bottom() - 20.0;
        p.text(
            pos2(x, y),
            Align2::LEFT_BOTTOM,
            "Photo: Carolyn Williams, CC BY 2.0",
            font(11.0),
            MUTED,
        );
        p.text(
            pos2(x, y - 16.0),
            Align2::LEFT_BOTTOM,
            "1968–2007, World Rally Champion 1995",
            font(13.0),
            MUTED,
        );
        p.text(
            pos2(x, y - 34.0),
            Align2::LEFT_BOTTOM,
            "Colin McRae",
            named("barlow-semibold", 19.0),
            INK,
        );
    }

    fn status(&self, ui: &mut egui::Ui) {
        let (text, color) = match (&self.error, &self.check) {
            (Some(e), _) => (e.clone(), ERROR),
            (None, Check::Found(_)) => ("Ready. Press Play or Enter.".to_string(), MUTED),
            (None, _) => (
                "First, show the launcher where the game is.".to_string(),
                MUTED,
            ),
        };
        ui.label(egui::RichText::new(text).font(font(14.0)).color(color));
    }

    fn tabs(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 26.0;
            for (tab, name) in [
                (Tab::Play, "Play"),
                (Tab::Picture, "Picture"),
                (Tab::Controls, "Controls"),
            ] {
                let on = self.tab == tab;
                let galley = ui.painter().layout_no_wrap(
                    name.to_string(),
                    named("condensed", 22.0),
                    if on { INK } else { MUTED },
                );
                let (rect, resp) =
                    ui.allocate_exact_size(galley.size() + vec2(0.0, 8.0), Sense::click());
                let color = if on || resp.hovered() { INK } else { MUTED };
                ui.painter().galley(rect.min, galley, color);
                if on {
                    let bar = Rect::from_min_size(
                        pos2(rect.left(), rect.bottom() - 3.0),
                        vec2(rect.width(), 3.0),
                    );
                    ui.painter().rect_filled(bar, 0.0, GOLD);
                }
                if resp
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    self.tab = tab;
                }
            }
        });
        let y = ui.cursor().top() - 4.0;
        let r = ui.max_rect();
        ui.painter()
            .hline(r.left()..=r.right(), y, Stroke::new(1.0, LINE));
    }

    fn play_tab(&mut self, ui: &mut egui::Ui) {
        heading(ui, "Game folder");
        let game_hint = if self.linux {
            "The folder with your Rally.exe, e.g. Z:\\home\\you\\Games\\Colin McRae Rally"
        } else {
            "The folder with your installed Rally.exe"
        };
        if folder_field(
            ui,
            &mut self.form.game,
            game_hint,
            "Choose the folder with your Rally.exe",
        ) {
            self.error = None;
        }
        let (note, color) = match &self.check {
            Check::Empty => (
                "The game folder is only read, never changed.".to_string(),
                MUTED,
            ),
            Check::Found(name) => (format!("Found: {name}"), GOLD),
            Check::Problem(p) => (p.clone(), ERROR),
        };
        ui.label(egui::RichText::new(note).font(font(13.0)).color(color));
        ui.add_space(10.0);

        heading(ui, "Look");
        ui.add_space(6.0);
        let enhanced = Picture::enhanced(self.screen);
        let enhanced_note = match enhanced.resolution {
            Some((w, h)) => format!(
                "{w}×{h}{}, smooth textures, longer view.",
                if enhanced.widescreen {
                    " widescreen"
                } else {
                    ""
                }
            ),
            None => "Smooth textures and a longer view.".to_string(),
        };
        let cards = [
            (
                Look::Original,
                "Original 1998",
                "640×480, exactly as the game shipped.".to_string(),
                false,
            ),
            (Look::Enhanced, "Enhanced", enhanced_note, true),
            (
                Look::Custom,
                "Custom",
                "Pick each setting yourself in Picture.".to_string(),
                false,
            ),
        ];
        let gap = 12.0;
        let w = (ui.available_width() - 2.0 * gap) / 3.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for (look, title, note, recommended) in cards {
                if look_card(ui, w, self.form.look == look, title, &note, recommended) {
                    self.form.set_look(look, self.screen);
                    if look == Look::Custom {
                        self.tab = Tab::Picture;
                    }
                }
            }
        });
        ui.add_space(14.0);

        heading(ui, "Saves");
        folder_field(
            ui,
            &mut self.form.saves,
            "Saved Games\\Colin McRae Rally (the default)",
            "Choose where your saves are kept",
        );
        ui.label(
            egui::RichText::new("Leave empty for the default. Saves are copied in before the game starts and back when it ends.")
                .font(font(13.0))
                .color(MUTED),
        );
    }

    fn picture_tab(&mut self, ui: &mut egui::Ui) {
        let before = self.form.picture;
        let p = &mut self.form.picture;
        let resolutions = &self.resolutions;
        let row = |ui: &mut egui::Ui, name: &str, add: &mut dyn FnMut(&mut egui::Ui)| {
            ui.horizontal_top(|ui| {
                field(ui, name);
                ui.vertical(|ui| add(ui));
            });
        };
        row(ui, "Race resolution", &mut |ui| {
            chips(ui, resolutions, &mut p.resolution)
        });
        row(ui, "Draw distance", &mut |ui| {
            chips(ui, &form::DISTANCES, &mut p.draw_distance);
        });
        row(ui, "Textures", &mut |ui| {
            chips(ui, &form::FILTERS, &mut p.texture_filter);
        });
        row(ui, "Frame-rate cap", &mut |ui| {
            chips(ui, &form::FRAME_RATES, &mut p.fps_limit);
        });
        row(ui, "Vsync", &mut |ui| {
            chips(ui, &form::VSYNCS, &mut p.vsync);
        });
        row(ui, "", &mut |ui| {
            toggle(ui, &mut p.widescreen, "Widescreen: show more to the sides");
            toggle(
                ui,
                &mut p.smooth_distance,
                "Ease view distance changes (less pop-in)",
            );
        });
        if self.linux {
            let scaler = &mut self.form.scaler;
            row(ui, "Scaler", &mut |ui| {
                let options: Vec<_> = form::SCALERS.iter().map(|s| (s.0, s.1)).collect();
                chips(ui, &options, scaler);
                let note = form::SCALERS
                    .iter()
                    .find(|s| s.0 == *scaler)
                    .map_or("", |s| s.2);
                ui.label(egui::RichText::new(note).font(font(13.0)).color(MUTED));
            });
        }
        if self.form.picture != before {
            self.form.look = Form::look_of(&self.form.picture, self.screen);
        }
    }

    fn controls_tab(&mut self, ui: &mut egui::Ui) {
        toggle(
            ui,
            &mut self.form.gamepad_axes,
            "Gamepad: triggers or right stick as pedals, Start pauses",
        );
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            field(ui, "Rumble");
            chips(ui, &form::RUMBLES, &mut self.form.rumble);
        });
        let pads = match self.pads {
            0 => "Rumble and separate trigger pedals need an Xbox-style (XInput) pad; none is connected now. On 8BitDo pads use X-input mode.".to_string(),
            1 => "One Xbox-style pad connected.".to_string(),
            n => format!("{n} Xbox-style pads connected."),
        };
        ui.label(egui::RichText::new(pads).font(font(13.0)).color(MUTED));
        ui.add_space(12.0);
        toggle(
            ui,
            &mut self.form.close_quits,
            "Closing the game window quits the game",
        );
    }

    fn footer(&mut self, ui: &mut egui::Ui, rect: Rect, ctx: &egui::Context) {
        let button = Rect::from_min_size(
            pos2(rect.right() - 170.0, rect.top()),
            vec2(170.0, rect.height()),
        );
        let hint = if self.form.picture.resolution.is_some() {
            "In the game keep Resolution on 640×480; the race resolution replaces it."
        } else {
            "Settings are saved when you press Play."
        };
        let galley = ui.painter().layout(
            hint.to_string(),
            font(13.0),
            MUTED,
            button.left() - rect.left() - 20.0,
        );
        let y = rect.center().y - galley.size().y / 2.0;
        ui.painter().galley(pos2(rect.left(), y), galley, MUTED);

        let resp = ui.interact(button, ui.scope_id().with("play"), Sense::click());
        let ready = self.ready();
        let fill = match (ready, resp.hovered()) {
            (false, _) => LINE,
            (true, true) => Color32::from_rgb(0xff, 0xd3, 0x2a),
            (true, false) => GOLD,
        };
        // A slanted plate, like the stripes.
        let s = 10.0;
        let plate = vec![
            pos2(button.left() + s, button.top()),
            button.right_top(),
            pos2(button.right() - s, button.bottom()),
            button.left_bottom(),
        ];
        ui.painter()
            .add(Shape::convex_polygon(plate, fill, Stroke::NONE));
        ui.painter().text(
            button.center(),
            Align2::CENTER_CENTER,
            "Play",
            named("decal", 30.0),
            if ready { ON_GOLD } else { MUTED },
        );
        if resp
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            self.play(ctx);
        }
    }
}

/// Fades `rect` from clear to blue: to the right when `horizontal`, else
/// downwards.
fn gradient(p: &egui::Painter, rect: Rect, horizontal: bool) {
    let mut mesh = Mesh::default();
    let clear = Color32::from_rgba_unmultiplied(BLUE.r(), BLUE.g(), BLUE.b(), 0);
    let (a, b, c, d) = if horizontal {
        (clear, BLUE, BLUE, clear)
    } else {
        (clear, clear, BLUE, BLUE)
    };
    mesh.colored_vertex(rect.left_top(), a);
    mesh.colored_vertex(rect.right_top(), b);
    mesh.colored_vertex(rect.right_bottom(), c);
    mesh.colored_vertex(rect.left_bottom(), d);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    p.add(Shape::mesh(mesh));
}

/// Two slanted gold livery stripes in the top right corner.
fn draw_decal(ui: &egui::Ui, full: Rect) {
    let p = ui.painter().with_clip_rect(full);
    for (i, (w, alpha)) in [(30.0, 1.0), (16.0, 0.65)].into_iter().enumerate() {
        let x = full.right() - 120.0 + i as f32 * 40.0;
        let slant = 60.0;
        let pts = vec![
            pos2(x, full.top()),
            pos2(x + w, full.top()),
            pos2(x + w - slant, full.top() + 112.0),
            pos2(x - slant, full.top() + 112.0),
        ];
        p.add(Shape::convex_polygon(
            pts,
            GOLD.gamma_multiply(alpha),
            Stroke::NONE,
        ));
    }
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .font(named("barlow-semibold", 14.0))
            .color(MUTED),
    );
}

fn field(ui: &mut egui::Ui, text: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(118.0, 30.0), Sense::hover());
    ui.painter().text(
        rect.left_center(),
        Align2::LEFT_CENTER,
        text,
        font(15.0),
        INK,
    );
}

/// A text field with a Browse button; true when it changed.
fn folder_field(ui: &mut egui::Ui, value: &mut String, hint: &str, title: &str) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        let w = ui.available_width() - 100.0;
        let edit = egui::TextEdit::singleline(value)
            .hint_text(egui::RichText::new(hint).color(MUTED))
            .font(font(15.0))
            .margin(vec2(10.0, 8.0))
            .desired_width(w);
        changed |= ui.add(edit).changed();
        if outline_button(ui, "Browse…").clicked()
            && let Some(dir) = win32::pick_folder(title)
        {
            *value = dir.display().to_string();
            changed = true;
        }
    });
    changed
}

fn outline_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(92.0, 34.0), Sense::click());
    let hover = resp.hovered();
    ui.painter().rect(
        rect,
        CornerRadius::same(6),
        if hover { LINE } else { Color32::TRANSPARENT },
        Stroke::new(1.5, GOLD),
        StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        named("barlow-semibold", 15.0),
        GOLD,
    );
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A row of choices; the chosen one is gold.
fn chips<T: PartialEq + Copy, S: AsRef<str>>(
    ui: &mut egui::Ui,
    options: &[(T, S)],
    current: &mut T,
) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for (value, label) in options {
            let on = *current == *value;
            let galley = ui.painter().layout_no_wrap(
                label.as_ref().to_string(),
                if on {
                    named("barlow-semibold", 14.0)
                } else {
                    font(14.0)
                },
                if on { ON_GOLD } else { INK },
            );
            let size = vec2(galley.size().x + 24.0, 30.0);
            let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
            let fill = if on {
                GOLD
            } else if resp.hovered() {
                LINE
            } else {
                Color32::TRANSPARENT
            };
            ui.painter().rect(
                rect,
                CornerRadius::same(6),
                fill,
                Stroke::new(1.0, if on { GOLD } else { LINE }),
                StrokeKind::Inside,
            );
            let at = rect.center() - galley.size() / 2.0;
            ui.painter().galley(at, galley, INK);
            if resp
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                *current = *value;
            }
        }
    });
}

/// A switch: knob left and outlined is off, knob right on gold is on.
fn toggle(ui: &mut egui::Ui, on: &mut bool, text: &str) {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), font(15.0), INK);
    let size = vec2(46.0 + 12.0 + galley.size().x, 30.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
    }
    let t = ui.ctx().animate_bool(resp.id, *on);
    let track = Rect::from_min_size(pos2(rect.left(), rect.center().y - 13.0), vec2(46.0, 26.0));
    let p = ui.painter();
    p.rect(
        track,
        CornerRadius::same(13),
        if *on { GOLD } else { Color32::TRANSPARENT },
        Stroke::new(1.5, if *on { GOLD } else { MUTED }),
        StrokeKind::Inside,
    );
    let x = egui::lerp(track.left() + 13.0..=track.right() - 13.0, t);
    p.circle_filled(
        pos2(x, track.center().y),
        9.0,
        if *on { ON_GOLD } else { MUTED },
    );
    p.galley(
        pos2(
            track.right() + 12.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        INK,
    );
    resp.on_hover_cursor(egui::CursorIcon::PointingHand);
}

/// One of the three looks; true when clicked.
fn look_card(
    ui: &mut egui::Ui,
    w: f32,
    on: bool,
    title: &str,
    note: &str,
    recommended: bool,
) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 104.0), Sense::click());
    let p = ui.painter();
    let fill = if on {
        PANEL
    } else if resp.hovered() {
        PANEL.gamma_multiply(0.6)
    } else {
        Color32::TRANSPARENT
    };
    p.rect(
        rect,
        CornerRadius::same(8),
        fill,
        Stroke::new(if on { 2.0 } else { 1.0 }, if on { GOLD } else { LINE }),
        StrokeKind::Inside,
    );
    let pad = 14.0;
    p.text(
        rect.left_top() + vec2(pad, pad - 2.0),
        Align2::LEFT_TOP,
        title,
        named("condensed", 22.0),
        if on { GOLD } else { INK },
    );
    if recommended {
        // A small gold tab on the card's top edge.
        let galley = p.layout_no_wrap(
            "Recommended".into(),
            named("barlow-semibold", 12.0),
            ON_GOLD,
        );
        let tab = Rect::from_min_size(
            pos2(
                rect.right() - pad - galley.size().x - 12.0,
                rect.top() - 9.0,
            ),
            galley.size() + vec2(12.0, 2.0),
        );
        p.rect_filled(tab, CornerRadius::same(4), GOLD);
        p.galley(tab.min + vec2(6.0, 1.0), galley, ON_GOLD);
    }
    let galley = p.layout(note.to_string(), font(13.5), MUTED, w - 2.0 * pad);
    p.galley(rect.left_top() + vec2(pad, pad + 32.0), galley, MUTED);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}
