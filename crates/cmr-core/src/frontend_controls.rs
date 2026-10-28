//! The options subtree's control screens: Control setup `0xd45858` (the
//! method and controller type of player `DAT_00d90504`), Analogue setup
//! `0xd41278` (which controls are analogue, force feedback) and Button setup
//! `0xd445d0` (the eight bindings). They edit the player block
//! `P(p) = img + 0x44 + 0x9c p` of the cmRally.cfg record: `+0` the analogue
//! and force-feedback bits, word `+2` the device, dword `+4` its type.
//!
//! All three slide their panel `0xd904f4` in from `-0x3333 - A` to `0x1999`
//! (`FUN_004ccd60`, mode 2, 15 steps, the slide cue) and out (mode 3), with
//! `A` the layout global `0x50aa8c` (0x8000 until the first enter writes
//! 0xb333). After the slide the commit (`FUN_004ccb50`) waits
//! `((DAT_00551af0 != 0) + 1) * 25 / 4` further steps, then enables moving,
//! adjusting and confirming but not going back (`aec50(m, 1, 1, 1, 0)`).
//! A description of disassembly, not original source. Evidence:
//! `rea.official-eu.frontend-controls`.

use crate::config::LEGACY_CONFIG_SIZE;
use crate::descriptor_menu::{ItemKind, Menu, MenuId, MenuItem, MenuNav, MenuSet};
use crate::frontend::ScreenId;
use crate::frontend_menu::ValueWidget;
use crate::scalar_tween::ScalarTween;

type Record = [u8; LEGACY_CONFIG_SIZE];

/// A control screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ControlScreen {
    Control,
    Analogue,
    Buttons,
}

impl ControlScreen {
    pub const ALL: [Self; 3] = [Self::Control, Self::Analogue, Self::Buttons];

    pub const fn screen(self) -> ScreenId {
        ScreenId::from_legacy(match self {
            Self::Control => 0x00d4_5858,
            Self::Analogue => 0x00d4_1278,
            Self::Buttons => 0x00d4_45d0,
        })
    }

    /// The camera target (argument 4 of the create call).
    pub const fn camera_target(self) -> u32 {
        match self {
            Self::Control => 0x0050_7ad8,
            Self::Analogue | Self::Buttons => 0x0050_7ae8,
        }
    }

    pub fn of(screen: ScreenId) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.screen() == screen)
    }

    const fn index(self) -> usize {
        self as usize
    }
}

/// Item keys.
pub mod key {
    pub const METHOD: i32 = 0x12;
    pub const TYPE: i32 = 0x13;
    pub const FORCE_FEEDBACK: i32 = 3;
    pub const STEERING: i32 = 0x1b;
    pub const BRAKE: i32 = 0x1c;
    pub const ACCELERATION: i32 = 0x1d;
    pub const OK: i32 = 9;
}

/// The control sub-state `DAT_00d90518`.
pub mod state {
    pub const CONTROL: u8 = 1;
    pub const ANALOGUE: u8 = 2;
    pub const BUTTONS: u8 = 3;
    pub const CAPTURE: u8 = 4;
}

/// The host's input devices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputDevices {
    /// `FUN_0049d620`: bit `i` for each present device record (0 and 1 are
    /// the keyboards).
    pub present: u32,
    /// `FUN_0049d5c0`: one bit per keyboard, then one per joystick id that
    /// answers `joyGetPosEx`.
    pub polled: u32,
    /// `FUN_0049caf0`: a force-feedback device.
    pub force_feedback: bool,
}

impl InputDevices {
    /// Two keyboards and no joystick.
    pub const KEYBOARDS: Self = Self {
        present: 0b11,
        polled: 0b11,
        force_feedback: false,
    };
}

/// A host request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlsEffect {
    /// `FUN_004c9a20`: the slide cue.
    SlideCue,
    /// `FUN_0049d670(dev, type)`: the device's type changed.
    DeviceType { device: u16, kind: i32 },
}

/// The number of significant bits of `mask`.
fn bit_length(mask: u32) -> i32 {
    32 - mask.leading_zeros() as i32
}

fn player_block(player: u8) -> usize {
    0x44 + 0x9c * usize::from(player)
}

fn value(menu: &Menu, key: i32) -> i32 {
    crate::frontend_options::value(menu, key)
}

fn item_mut(menu: &mut Menu, key: i32) -> Option<&mut MenuItem> {
    let index = menu.find(key)?;
    menu.items.get_mut(index)
}

/// Move the selection past disabled items (`+0x2c`), unbounded as the
/// native loop is (it stops at the menu's end here).
fn skip_disabled(menu: &mut Menu) {
    while menu
        .items
        .get(menu.selected)
        .is_some_and(|item| !item.enabled)
    {
        menu.selected += 1;
    }
}

fn value_item(text: i32, key: i32, count: i32, value: i32, wrap: bool) -> MenuItem {
    let mut item = MenuItem::new(ItemKind::Value, text, 0);
    item.key = key;
    item.value = ValueWidget {
        value,
        value_count: count,
        wrap,
    };
    item
}

/// The control screens' state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControlScreens {
    ids: [MenuId; 3],
    /// The panel slide `0xd904f4` and its x `DAT_00d904fc`.
    tween: Option<ScalarTween>,
    pub panel_x: i32,
    /// `0x50aa8c`.
    amplitude: i32,
    /// `DAT_00d90548`: the steps since the slide ended.
    settle: i32,
    /// `DAT_00d90518`.
    pub state: u8,
    /// `DAT_00d9050c`: the type chosen is analogue.
    pub analogue: bool,
    /// `DAT_00d9050e`: capturing a binding.
    pub capturing: bool,
    /// `DAT_00d9050f`.
    keep_analogue: bool,
    /// `DAT_00d90521..d90524`: steering, accel, brake, force feedback.
    pub analogue_latch: [bool; 4],
    /// `0x50aaa0`: the device type chosen.
    pub device_type: i32,
    /// The layout `0x50aa94`/`0x50aa98`/`0x50aa9c`: label x, first row y,
    /// row gap.
    pub layout: (i32, i32, i32),
}

impl ControlScreens {
    /// Add the three menus to `set` below `settings`.
    pub fn build(set: &mut MenuSet, settings: MenuId) -> Self {
        let base = set.menus.len();
        let id = |screen: ControlScreen| MenuId(base + screen.index());
        let menu = |title: i32, parent: MenuId| {
            Menu::new(
                title,
                Some(parent),
                Some(crate::frontend_sounds::SETUP_SOUNDS),
            )
        };
        let mut control = menu(0xc1, settings);
        let mut pad = MenuItem::new(ItemKind::Action, 0xc5, 0);
        pad.activate = Some(0x004c_dbc0);
        let mut ok = MenuItem::new(ItemKind::Submenu(settings), 0xdf, 0);
        ok.activate = Some(0x004c_da80);
        control.items = vec![
            value_item(0xc2, key::METHOD, 0x12, 0, true),
            value_item(0xc3, key::TYPE, 3, 0, true),
            pad,
            ok,
        ];
        callbacks(
            &mut control,
            [
                0x004c_cbf0,
                0x004c_ee30,
                0x004c_dd80,
                0x004c_c6f0,
                0x004c_cb20,
            ],
        );
        let mut analogue = menu(0x38, settings);
        let toggle = |text, key, on: bool| value_item(text, key, 2, i32::from(on), true);
        analogue.items = vec![
            toggle(0x15, key::STEERING, true),
            toggle(0x16, key::ACCELERATION, false),
            toggle(0x17, key::BRAKE, false),
            toggle(0xc4, key::FORCE_FEEDBACK, false),
            MenuItem::new(ItemKind::Submenu(id(ControlScreen::Buttons)), 0xdf, 0),
        ];
        callbacks(
            &mut analogue,
            [
                0x004c_cdc0,
                0x004c_ee30,
                0x004c_e420,
                0x004c_c820,
                0x004c_cb20,
            ],
        );
        let mut buttons = menu(0x39, id(ControlScreen::Analogue));
        for (slot, text) in [0xb7, 0xb8, 0x3b, 0x3c, 0x3d, 0x3e, 0x3f, 0x40, 0xcc]
            .into_iter()
            .enumerate()
        {
            let mut item = MenuItem::new(ItemKind::Action, text, 0);
            item.key = slot as i32;
            item.activate = Some(0x004c_dbe0);
            buttons.items.push(item);
        }
        let mut ok = MenuItem::new(ItemKind::Submenu(settings), 0xdf, 0);
        ok.key = key::OK;
        ok.activate = Some(0x004c_dc30);
        buttons.items.push(ok);
        callbacks(
            &mut buttons,
            [
                0x004c_cf50,
                0x004c_ee30,
                0x004c_e650,
                0x004c_c9b0,
                0x004c_cb20,
            ],
        );
        set.add(control);
        set.add(analogue);
        set.add(buttons);
        Self {
            ids: std::array::from_fn(|i| MenuId(base + i)),
            tween: None,
            panel_x: 0,
            amplitude: 0x8000,
            settle: 0,
            state: 0,
            analogue: false,
            capturing: false,
            keep_analogue: false,
            analogue_latch: [false; 4],
            device_type: 0,
            layout: (0xf5c, 0xccc, 0x7ae),
        }
    }

    pub fn id(&self, screen: ControlScreen) -> MenuId {
        self.ids[screen.index()]
    }

    pub fn screen_of(&self, id: MenuId) -> Option<ControlScreen> {
        ControlScreen::ALL.into_iter().find(|&s| self.id(s) == id)
    }

    /// `FUN_004ccd60`: the slide in, the cue, the navigation off.
    fn slide_in(&mut self, menu: &mut Menu, no_animation: bool, effects: &mut Vec<ControlsEffect>) {
        let from = -0x3333 - self.amplitude;
        self.tween = Some(if no_animation {
            ScalarTween::ease_out(0x1999, 0x1999, 15, 0)
        } else {
            ScalarTween::ease_out(from, 0x1999, 15, 0)
        });
        effects.push(ControlsEffect::SlideCue);
        self.panel_x = -self.amplitude;
        menu.nav = MenuNav::NONE;
    }

    /// The posts' slide out (mode 3), the cue and the navigation off.
    fn slide_out(
        &mut self,
        menu: &mut Menu,
        no_animation: bool,
        effects: &mut Vec<ControlsEffect>,
    ) {
        let to = -0x3333 - self.amplitude;
        self.tween = Some(if no_animation {
            ScalarTween::ease_out(to, to, 15, 0)
        } else {
            ScalarTween::ease_in(0x1999, to, 15, 0)
        });
        effects.push(ControlsEffect::SlideCue);
        self.panel_x = 0x1999;
        menu.nav = MenuNav::NONE;
    }

    /// The enter callbacks (`0x4ccbf0`, `0x4ccdc0`, `0x4ccf50`).
    #[allow(clippy::too_many_arguments)]
    pub fn enter(
        &mut self,
        screen: ControlScreen,
        arg: u8,
        menu: &mut Menu,
        img: &mut Record,
        player: u8,
        devices: &InputDevices,
        no_animation: bool,
        effects: &mut Vec<ControlsEffect>,
    ) {
        self.slide_in(menu, no_animation, effects);
        self.amplitude = 0xb333;
        let p = player_block(player);
        match screen {
            ControlScreen::Control => {
                self.layout = (0xf5c, 0x1eb8, 0x7ae);
                self.state = state::CONTROL;
                if arg == 0 {
                    let length = bit_length(devices.present);
                    if let Some(item) = item_mut(menu, key::METHOD) {
                        item.value.value_count = length - 1;
                    }
                    let device = i32::from(u16::from_le_bytes([img[p + 2], img[p + 3]]));
                    if device < length {
                        let method = if device < 2 { 0 } else { device - 1 };
                        if let Some(item) = item_mut(menu, key::METHOD) {
                            item.value.value = method;
                        }
                    }
                    if value(menu, key::METHOD) != 0 {
                        let kind =
                            i32::from_le_bytes([img[p + 4], img[p + 5], img[p + 6], img[p + 7]]);
                        let index = match kind {
                            0 => Some(0),
                            2 => Some(1),
                            4 => Some(2),
                            _ => None,
                        };
                        if let (Some(index), Some(item)) = (index, item_mut(menu, key::TYPE)) {
                            item.value.value = index;
                        }
                    }
                    skip_disabled(menu);
                }
                self.capturing = false;
            }
            ControlScreen::Analogue => {
                self.layout = (0xf5c, 0x11eb, 0x7ae);
                self.state = state::ANALOGUE;
                if arg == 0 && !self.keep_analogue {
                    img[p] |= 4;
                    let bits = img[p];
                    let set = |menu: &mut Menu, key, v: u8| {
                        if let Some(item) = item_mut(menu, key) {
                            item.value.value = i32::from(v);
                        }
                    };
                    set(menu, key::STEERING, 1);
                    set(menu, key::ACCELERATION, (bits >> 1) & 1);
                    set(menu, key::BRAKE, bits & 1);
                    set(menu, key::FORCE_FEEDBACK, (bits >> 3) & 1);
                    skip_disabled(menu);
                    if let Some(item) = item_mut(menu, key::FORCE_FEEDBACK) {
                        item.enabled = devices.force_feedback;
                        if !devices.force_feedback {
                            item.value.value = 0;
                        }
                    }
                }
                self.keep_analogue = false;
                self.capturing = false;
            }
            ControlScreen::Buttons => {
                self.layout = (0x7333, 0xccc, 0x418);
                self.state = state::BUTTONS;
            }
        }
    }

    /// `0x4cee30` (all three): the method count from the polled devices,
    /// the panel and the settle (`FUN_004ccb50`), then the control setup's
    /// type gating.
    pub fn commit(&mut self, menu: &mut Menu, devices: &InputDevices, slow_motion: bool) {
        let mut count = bit_length(devices.polled);
        if count > 2 {
            count -= 1;
        }
        if let Some(item) = item_mut(menu, key::METHOD) {
            item.value.value_count = count - 1;
            if count - 2 < item.value.value {
                item.value.value = count - 2;
            }
        }
        if let Some(tween) = self.tween.as_mut()
            && !tween.is_finished()
        {
            self.panel_x = tween.evaluate_and_advance();
            self.settle = 0;
        } else if self.settle >= (i32::from(slow_motion) + 1) * 25 / 4 {
            if !self.capturing {
                menu.nav = MenuNav {
                    moves: true,
                    adjusts: true,
                    confirms: true,
                    backs: false,
                };
            }
        } else {
            self.settle += 1;
        }
        if self.state == state::CONTROL {
            if value(menu, key::METHOD) == 0 {
                if let Some(item) = item_mut(menu, key::TYPE) {
                    item.enabled = false;
                    item.value.value = -1;
                }
            } else if let Some(item) = item_mut(menu, key::TYPE) {
                item.enabled = true;
                if item.value.value < 0 {
                    item.value.value = 0;
                }
            }
        }
    }

    /// `0x4cda80`, the control setup's OK: the device and its type into the
    /// player block, then on to the analogue setup (types 2 and 4) or the
    /// button setup, whose parent (`0xd445e4`) becomes the screen it is
    /// reached from. Returns the menu to open and the button setup's parent.
    pub fn control_ok(
        &mut self,
        menu: &Menu,
        img: &mut Record,
        player: u8,
        effects: &mut Vec<ControlsEffect>,
    ) -> (MenuId, MenuId) {
        let p = player_block(player);
        let kind = value(menu, key::TYPE);
        let method = value(menu, key::METHOD);
        let (device, device_type) = if method == 0 {
            (u16::from(player), 1)
        } else {
            match kind {
                0 => ((method + 1) as u16, 0),
                1 => ((method + 1) as u16, 2),
                2 => ((method + 1) as u16, 4),
                _ => (0, 1),
            }
        };
        img[p + 2..p + 4].copy_from_slice(&device.to_le_bytes());
        self.device_type = device_type;
        effects.push(ControlsEffect::DeviceType {
            device,
            kind: device_type,
        });
        if matches!(device_type, 2 | 4) {
            self.analogue = true;
            self.state = state::ANALOGUE;
            let analogue = self.id(ControlScreen::Analogue);
            (analogue, analogue)
        } else {
            self.analogue = false;
            self.state = state::BUTTONS;
            (
                self.id(ControlScreen::Buttons),
                self.id(ControlScreen::Control),
            )
        }
    }

    /// The posts: `0x4cc6f0` (a digital or keyboard type clears the analogue
    /// and force-feedback bits), `0x4cc820` (the analogue bits latched and
    /// written) and `0x4cc9b0`.
    #[allow(clippy::too_many_arguments)]
    pub fn post(
        &mut self,
        screen: ControlScreen,
        back: bool,
        menu: &mut Menu,
        img: &mut Record,
        player: u8,
        no_animation: bool,
        effects: &mut Vec<ControlsEffect>,
    ) {
        self.slide_out(menu, no_animation, effects);
        let p = player_block(player);
        match screen {
            ControlScreen::Control if !back => {
                if matches!(self.device_type, 0 | 1) {
                    img[p] &= !0x0f;
                }
                skip_disabled(menu);
            }
            ControlScreen::Analogue => {
                let latch = [
                    value(menu, key::STEERING) != 0,
                    value(menu, key::ACCELERATION) != 0,
                    value(menu, key::BRAKE) != 0,
                    value(menu, key::FORCE_FEEDBACK) != 0,
                ];
                self.analogue_latch = latch;
                img[p] = (img[p] & !4) | u8::from(latch[0]) << 2;
                img[p] = (img[p] & !2) | u8::from(latch[1]) << 1;
                img[p] = (img[p] & !1) | u8::from(latch[2]);
                img[p] = (img[p] & !8) | u8::from(latch[3]) << 3;
            }
            _ => {}
        }
        menu.nav = MenuNav::NONE;
    }
}

fn callbacks(menu: &mut Menu, [enter, commit, predraw, post, leave]: [u32; 5]) {
    menu.on_enter = Some(enter);
    menu.on_commit = Some(commit);
    menu.on_predraw = Some(predraw);
    menu.on_post = Some(post);
    menu.on_leave = Some(leave);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build() -> (MenuSet, ControlScreens, Box<Record>) {
        let mut set = MenuSet::default();
        set.add(Menu::new(-1, None, None));
        let controls = ControlScreens::build(&mut set, MenuId(0));
        (set, controls, Box::new([0; LEGACY_CONFIG_SIZE]))
    }

    #[test]
    fn the_control_setup_reads_the_device_and_gates_the_type() {
        let (mut set, mut c, mut img) = build();
        let p = player_block(0);
        img[p + 2] = 3; // device 3
        img[p + 4] = 4; // analogue type 4
        let devices = InputDevices {
            present: 0b1111,
            polled: 0b1111,
            force_feedback: false,
        };
        let m = set.menu_mut(c.id(ControlScreen::Control));
        let mut effects = Vec::new();
        c.enter(
            ControlScreen::Control,
            0,
            m,
            &mut img,
            0,
            &devices,
            false,
            &mut effects,
        );
        assert_eq!(effects, vec![ControlsEffect::SlideCue]);
        assert_eq!(value(m, key::METHOD), 2);
        assert_eq!(value(m, key::TYPE), 2);
        assert_eq!(m.nav, MenuNav::NONE);
        // The slide, then six settle steps, then no back.
        for _ in 0..15 + 6 {
            c.commit(m, &devices, false);
            assert!(!m.nav.moves);
        }
        c.commit(m, &devices, false);
        assert!(m.nav.moves && !m.nav.backs);
        // Keyboard (method 0): the type is disabled at -1.
        m.items[0].value.value = 0;
        c.commit(m, &devices, false);
        assert!(!m.items[1].enabled);
        assert_eq!(value(m, key::TYPE), -1);
        let (target, parent) = c.control_ok(m, &mut img, 0, &mut effects);
        assert_eq!(target, c.id(ControlScreen::Buttons));
        assert_eq!(parent, c.id(ControlScreen::Control));
        assert_eq!((img[p + 2], c.device_type), (0, 1));
    }

    #[test]
    fn an_analogue_type_opens_the_analogue_setup_which_writes_the_bits() {
        let (mut set, mut c, mut img) = build();
        let p = player_block(1);
        let mut effects = Vec::new();
        let control = c.id(ControlScreen::Control);
        set.menu_mut(control).items[0].value.value = 1;
        set.menu_mut(control).items[1].value.value = 1;
        let m = set.menu(control).clone();
        assert_eq!(
            c.control_ok(&m, &mut img, 1, &mut effects).0,
            c.id(ControlScreen::Analogue)
        );
        assert_eq!(img[p + 2], 2);
        img[p] = 0b1011;
        let a = set.menu_mut(c.id(ControlScreen::Analogue));
        c.enter(
            ControlScreen::Analogue,
            0,
            a,
            &mut img,
            1,
            &InputDevices::KEYBOARDS,
            true,
            &mut effects,
        );
        assert_eq!(img[p] & 4, 4, "steering forced on");
        assert_eq!(value(a, key::STEERING), 1);
        assert_eq!(value(a, key::ACCELERATION), 1);
        assert_eq!(value(a, key::BRAKE), 1);
        assert_eq!(value(a, key::FORCE_FEEDBACK), 0, "no force feedback device");
        a.items[2].value.value = 0;
        c.post(
            ControlScreen::Analogue,
            false,
            a,
            &mut img,
            1,
            true,
            &mut effects,
        );
        assert_eq!(img[p] & 0xf, 0b0110);
        // A digital type clears the bits on the control setup's post.
        c.device_type = 0;
        let m = set.menu_mut(control);
        c.post(
            ControlScreen::Control,
            false,
            m,
            &mut img,
            1,
            true,
            &mut effects,
        );
        assert_eq!(img[p] & 0xf, 0);
    }
}
