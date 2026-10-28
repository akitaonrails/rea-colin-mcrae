//! Reconstruction of the in-race pause menu (FSM state `0xa`).
//!
//! - Entry: the Escape/focus request ([`crate::pause_request`]) is probed by
//!   `FUN_00468200` in the driving handlers; on a hit it mutes the race
//!   sounds (`FUN_00487170`, `FUN_0048b650`, `FUN_004446e0`), rebuilds every
//!   pause menu (`FUN_00468260`) and the handler clears the menu words
//!   (`FUN_00468da0`) and returns `0xa`. State 7 also stores `DAT_00551b28 = 7`.
//! - Each paused frame `FUN_00438970` zeroes the force-feedback magnitudes
//!   (`FUN_00433570`), reads the menu input (`FUN_00468ed0`), then picks the
//!   next state from Shift+Q, the quit/restart/alternate words and the
//!   consumed resume word.
//! - The menus run on [`crate::descriptor_menu`]. The root holds the sound
//!   and settings submenus, the restart and quit confirmations and the resume
//!   action (the default selection). Back/Escape in any menu resumes.
//!
//! Text ids are localized string ids (`FUN_0049b010`); labels such as
//! "resume" are inferred from behaviour. This is a description of
//! disassembly, not original source. Evidence: `rea.official-eu.pause-menu`.

use crate::descriptor_menu::{
    ItemKind, Menu, MenuEvent, MenuId, MenuItem, MenuSet, MenuSound, bits,
};
use crate::frontend_menu::ValueWidget;
use crate::pause_request::{PauseKeyGates, PauseRequest};

/// Host callback ids, named by their native addresses.
pub mod callback {
    /// `0x468cf0`: resume.
    pub const RESUME: u32 = 0x468cf0;
    /// `0x468cd0`: restart "yes".
    pub const RESTART_YES: u32 = 0x468cd0;
    /// `0x468c70`: quit "yes".
    pub const QUIT_YES: u32 = 0x468c70;
    /// `0x467f70` / `0x467fe0`: the sound submenu's enter and commit.
    pub const SOUND_ENTER: u32 = 0x467f70;
    pub const SOUND_COMMIT: u32 = 0x467fe0;
    /// `0x468070` / `0x4680d0`: the settings submenu's enter and commit.
    pub const SETTINGS_ENTER: u32 = 0x468070;
    pub const SETTINGS_COMMIT: u32 = 0x4680d0;
    /// `0x468910`: the title pre-draw.
    pub const TITLE: u32 = 0x468910;
    /// Draw callbacks.
    pub const DRAW_LABEL: u32 = 0x468a10;
    pub const DRAW_TOGGLE: u32 = 0x468ad0;
    pub const DRAW_THREE_WAY: u32 = 0x468be0;
    pub const DRAW_BAR: u32 = 0x468b20;
    pub const DRAW_VIEW_TOGGLE: u32 = 0x468a40;
    /// Sound tables `0x57e698` (default) and `0x57e6a8` (the sound submenu,
    /// whose value change previews the channel).
    pub const SOUNDS: u32 = 0x57e698;
    pub const SOUNDS_PREVIEW: u32 = 0x57e6a8;
}

/// Sound-submenu item keys.
const KEY_SPEECH: i32 = 3;
const KEY_EFFECTS: i32 = 5;
/// Settings-submenu item keys.
const KEY_FLAG: i32 = 1;
const KEY_THREE_WAY: i32 = 2;

/// The run-time conditions `FUN_00468260` builds the menus under.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PauseMenuConditions {
    /// `DAT_00d3ca88 == 0`.
    pub real_play: bool,
    /// `DAT_00d3ca94 != 0`.
    pub link_session: bool,
    /// `FUN_00401000()`: `cfg[0x166] & 0x1c` is 4 or 0xc in real play.
    pub two_player_mode: bool,
    /// `cfg[0x166] & 3`.
    pub cfg_166_mode: u8,
    /// `DAT_00551af0 != 0`.
    pub slow_motion: bool,
}

/// The option bytes the menus read and write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PauseOptions {
    /// `cfg[5]`: effects volume `0..=5` in the low 7 bits.
    pub cfg_5: u8,
    /// `cfg[6]`: speech volume.
    pub cfg_6: u8,
    /// `cfg[8]`: bit 2 and bits 3..4 edited by the settings submenu.
    pub cfg_8: u8,
    /// `DAT_004eb5f0`.
    pub view_toggle_4eb5f0: u8,
    /// `DAT_004f653c`: the effects master percentage.
    pub effects_master_4f653c: i32,
    /// `DAT_004f6538`: the speech master percentage.
    pub speech_master_4f6538: i32,
}

/// The menus of one pause.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PauseMenus {
    pub set: MenuSet,
    /// `0x57d3f0`: the root's parent; reaching it resumes.
    pub sentinel: MenuId,
    pub root: MenuId,
    pub sound: MenuId,
    pub settings: MenuId,
    pub restart: Option<MenuId>,
    pub quit: MenuId,
    /// `menu + 0x34` of the sound submenu: where the absent speech item's
    /// value lands (index `-1`) when `FUN_00401000()` hides it.
    pub sound_phantom_34: i32,
    /// The same field of the settings submenu, for the absent three-way item.
    pub settings_phantom_34: i32,
    /// `DAT_00d3ca88 == 0` when the menus were built.
    pub real_play: bool,
}

fn value_item(text_id: i32, key: i32, draw: u32, count: i32, value: i32, wrap: bool) -> MenuItem {
    let mut item = MenuItem::new(ItemKind::Value, text_id, draw);
    item.key = key;
    item.value = ValueWidget {
        value,
        value_count: count,
        wrap,
    };
    item
}

fn action_item(text_id: i32, activate: u32) -> MenuItem {
    let mut item = MenuItem::new(ItemKind::Action, text_id, callback::DRAW_LABEL);
    item.activate = Some(activate);
    item
}

fn confirm_menu(set: &mut MenuSet, title: i32, parent: MenuId, yes: u32) -> MenuId {
    let id = set.add(Menu::new(title, Some(parent), Some(callback::SOUNDS)));
    let menu = set.menu_mut(id);
    menu.on_predraw = Some(callback::TITLE);
    menu.items = vec![
        action_item(0xe, yes),
        MenuItem::new(ItemKind::Back, 0xf, callback::DRAW_LABEL),
    ];
    id
}

/// `FUN_00468260`: build the pause menus.
pub fn build_pause_menus(conditions: PauseMenuConditions, options: PauseOptions) -> PauseMenus {
    let mut set = MenuSet::default();
    let sentinel = set.add(Menu::new(-1, None, None));
    let root = set.add(Menu::new(0, Some(sentinel), Some(callback::SOUNDS)));

    let sound = set.add(Menu::new(6, Some(root), Some(callback::SOUNDS_PREVIEW)));
    {
        let menu = set.menu_mut(sound);
        menu.on_enter = Some(callback::SOUND_ENTER);
        menu.on_commit = Some(callback::SOUND_COMMIT);
        menu.on_predraw = Some(callback::TITLE);
        if !conditions.two_player_mode {
            menu.items
                .push(value_item(7, KEY_SPEECH, callback::DRAW_BAR, 6, 5, false));
        }
        menu.items
            .push(value_item(8, KEY_EFFECTS, callback::DRAW_BAR, 6, 5, false));
        menu.items
            .push(MenuItem::new(ItemKind::Back, 9, callback::DRAW_LABEL));
    }

    let settings = set.add(Menu::new(0xa, Some(root), Some(callback::SOUNDS)));
    {
        let menu = set.menu_mut(settings);
        menu.on_enter = Some(callback::SETTINGS_ENTER);
        menu.on_commit = Some(callback::SETTINGS_COMMIT);
        menu.on_predraw = Some(callback::TITLE);
        menu.items
            .push(value_item(0xb, KEY_FLAG, callback::DRAW_TOGGLE, 2, 1, true));
        if !conditions.slow_motion {
            menu.items.push(value_item(
                0xea,
                KEY_THREE_WAY,
                callback::DRAW_THREE_WAY,
                3,
                2,
                false,
            ));
        }
        if conditions.real_play && conditions.two_player_mode {
            menu.items.push(value_item(
                0xc,
                4,
                callback::DRAW_VIEW_TOGGLE,
                2,
                i32::from(options.view_toggle_4eb5f0 != 0),
                true,
            ));
        }
        menu.items
            .push(MenuItem::new(ItemKind::Back, 9, callback::DRAW_LABEL));
    }

    let restart = (conditions.real_play
        && !conditions.link_session
        && conditions.cfg_166_mode != 3)
        .then(|| {
            let title = if conditions.cfg_166_mode == 2 {
                0xe3
            } else {
                0xe1
            };
            let id = confirm_menu(&mut set, title, root, callback::RESTART_YES);
            set.menu_mut(id).forced_selection = Some(1);
            id
        });
    let quit = confirm_menu(&mut set, 0xd, root, callback::QUIT_YES);
    set.menu_mut(quit).selected = 1;

    let menu = set.menu_mut(root);
    menu.on_predraw = Some(callback::TITLE);
    menu.items.push(MenuItem::new(
        ItemKind::Submenu(sound),
        1,
        callback::DRAW_LABEL,
    ));
    menu.items.push(MenuItem::new(
        ItemKind::Submenu(settings),
        2,
        callback::DRAW_LABEL,
    ));
    if let Some(restart) = restart {
        let text = if conditions.cfg_166_mode == 2 {
            0xe2
        } else {
            0xe0
        };
        menu.items.push(MenuItem::new(
            ItemKind::Submenu(restart),
            text,
            callback::DRAW_LABEL,
        ));
    }
    menu.items.push(MenuItem::new(
        ItemKind::Submenu(quit),
        3,
        callback::DRAW_LABEL,
    ));
    menu.items.push(action_item(5, callback::RESUME));
    menu.selected = menu.items.len() - 1;

    PauseMenus {
        set,
        sentinel,
        root,
        sound,
        settings,
        restart,
        quit,
        sound_phantom_34: 0,
        settings_phantom_34: 0,
        real_play: conditions.real_play,
    }
}

/// The keys `FUN_0043baa0` and the state-`0xa` handler read with
/// `GetAsyncKeyState`, plus the pad's edge mask.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PauseKeys {
    pub escape: bool,
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub enter: bool,
    pub shift: bool,
    pub q: bool,
    /// The player's device record `+8` edge mask, `0` for a keyboard device.
    pub pad_mask: u32,
}

impl PauseKeys {
    fn mask(self) -> u32 {
        let mut mask = self.pad_mask;
        for (down, bit) in [
            (self.escape, bits::BACK),
            (self.up, bits::UP),
            (self.down, bits::DOWN),
            (self.left, bits::LEFT),
            (self.right, bits::RIGHT),
            (self.enter, bits::CONFIRM),
        ] {
            if down {
                mask |= bit;
            }
        }
        mask
    }
}

/// A side effect the pause code asks the host to perform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PauseEffect {
    /// `FUN_00487170(0, car)`: mute the car's engine channels that play.
    MuteEngine { car: u8 },
    /// `FUN_0048b650`: stop the five surface loops (one player only).
    StopSurfaceLoops,
    /// `FUN_004446e0`: the speech channel volume to 0.
    MuteSpeech,
    /// `FUN_004446f0`: restore the speech channel volume.
    RestoreSpeech,
    /// `FUN_00433570`: zero every force-feedback magnitude.
    ZeroForceFeedback,
    /// A menu sound-table cue.
    MenuSound {
        table: u32,
        sound: MenuSound,
        ok: bool,
        preview_speech: bool,
        value: i32,
    },
    /// `FUN_0048dfa0(frames)`: start the fade-out.
    FadeOut { frames: i32 },
    /// `FUN_00420db0` + `FUN_00443b20` (the unreachable normal-play branch of
    /// the alternate word).
    CommitTelemetryAndStopAudio,
}

/// The state-`0xa` handler's decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PauseDecision {
    Stay,
    /// Shift+Q: straight to the hub, no fade.
    QuitHotkey,
    Quit,
    Restart,
    /// `DAT_0057e67c`: front-end `9`, otherwise `8`.
    Alternate,
    /// `DAT_0057e670` consumed: `5`, or `7` when paused from state 7.
    Resume {
        to_entry_state: bool,
    },
}

/// The pause words and the active menus.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PauseController {
    pub request: PauseRequest,
    pub menus: Option<PauseMenus>,
    /// `DAT_0057e688`.
    pub current: Option<MenuId>,
    /// `DAT_0057e670` / `674` / `678` / `67c` / `680`.
    pub resume_670: bool,
    pub resume_674: bool,
    pub quit_678: bool,
    pub alternate_67c: bool,
    pub restart_680: bool,
    /// `DAT_0055ae9c`: the menu-input latch.
    pub latch_55ae9c: bool,
    /// `DAT_00550c20`: the previous input mask of the change filter.
    pub previous_mask_550c20: u32,
    /// `DAT_00551b28`.
    pub prepause_551b28: u8,
    /// `DAT_00550018`: the restart request the hub reads.
    pub restart_request_550018: bool,
}

/// Inputs of one driving-state pause poll.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DrivingPauseInput {
    pub state_id: u8,
    pub escape_down: bool,
    pub player: u8,
    pub any_countdown_75c: bool,
    pub gates: PauseKeyGates,
    pub conditions: PauseMenuConditions,
    pub one_player: bool,
}

impl PauseController {
    /// `FUN_00468da0`, minus `DAT_0057e66c`, which [`PauseRequest`] owns.
    fn clear_menu_words(&mut self) {
        self.resume_670 = false;
        self.resume_674 = false;
        self.quit_678 = false;
        self.alternate_67c = false;
        self.restart_680 = false;
    }

    /// `FUN_00468cb0`: close the menu.
    fn close(&mut self) {
        self.request.focus_lost_748 = false;
        self.resume_670 = true;
        self.resume_674 = true;
        self.latch_55ae9c = false;
    }

    /// The control map's Escape block and the handler's `FUN_00468200` probe
    /// for states 5, 7 and 9. Returns whether the FSM enters `0xa`; on entry
    /// the menus are built and the race sounds muted (`effects`).
    pub fn driving_tick(
        &mut self,
        input: DrivingPauseInput,
        options: PauseOptions,
        effects: &mut Vec<PauseEffect>,
    ) -> bool {
        self.request
            .disarm_during_countdown(input.any_countdown_75c);
        let gates = PauseKeyGates {
            blocked_55ae9c: self.latch_55ae9c,
            ..input.gates
        };
        let poll = self
            .request
            .poll_escape(input.escape_down, gates, input.player);
        if poll.cleared_menu_words {
            self.clear_menu_words();
        }
        if !self.request.probe() {
            return false;
        }
        effects.push(PauseEffect::MuteEngine { car: 0 });
        if input.conditions.real_play && input.conditions.two_player_mode {
            effects.push(PauseEffect::MuteEngine { car: 1 });
        }
        if input.one_player {
            effects.push(PauseEffect::StopSurfaceLoops);
        }
        effects.push(PauseEffect::MuteSpeech);
        let mut menus = build_pause_menus(input.conditions, options);
        menus.set.activate(menus.root, None, 0);
        self.current = Some(menus.root);
        self.menus = Some(menus);
        self.clear_menu_words();
        if input.state_id == 7 {
            self.prepause_551b28 = 7;
        }
        true
    }

    /// `FUN_0043baa0`: the change filter.
    fn filtered_mask(&mut self, mask: u32) -> u32 {
        if mask == self.previous_mask_550c20 {
            0
        } else {
            self.previous_mask_550c20 = mask;
            mask
        }
    }

    /// `FUN_00468ed0`: one paused frame's menu input.
    fn menu_input(
        &mut self,
        keys: PauseKeys,
        options: &mut PauseOptions,
        effects: &mut Vec<PauseEffect>,
    ) {
        let input = self.filtered_mask(keys.mask());
        if input & bits::BACK != 0 {
            if self.latch_55ae9c {
                self.close();
            }
        } else if self.latch_55ae9c
            && let (Some(current), Some(mut menus)) = (self.current, self.menus.take())
        {
            let (next, events) = menus.set.step(current, input);
            self.apply_events(&mut menus, &events, options, effects);
            if next == Some(menus.sentinel) {
                self.close();
            } else if let Some(next) = next {
                self.current = Some(next);
                let events = menus.set.activate(next, Some(current), 0);
                self.apply_events(&mut menus, &events, options, effects);
            }
            self.menus = Some(menus);
        }
        if input == 0 {
            self.latch_55ae9c = true;
        }
    }

    fn apply_events(
        &mut self,
        menus: &mut PauseMenus,
        events: &[MenuEvent],
        options: &mut PauseOptions,
        effects: &mut Vec<PauseEffect>,
    ) {
        for event in events {
            match *event {
                MenuEvent::Sound {
                    menu,
                    table,
                    sound,
                    item,
                    ok,
                } => {
                    let entry = menus.set.menu(menu);
                    let value = entry.items.get(item).map_or(5, |item| item.value.value);
                    let preview_speech = menu == menus.sound
                        && entry.selected == 0
                        && entry.find(KEY_SPEECH).is_some();
                    effects.push(PauseEffect::MenuSound {
                        table,
                        sound,
                        ok,
                        preview_speech,
                        value,
                    });
                }
                MenuEvent::Activate { callback, .. } => match callback {
                    callback::RESUME => self.close(),
                    callback::RESTART_YES => {
                        self.close();
                        self.restart_680 = true;
                    }
                    callback::QUIT_YES => {
                        self.close();
                        // The front-end/replay branch also raises
                        // `DAT_005fbe78`/`DAT_005fbe14` and runs the replay
                        // teardown `FUN_00471810`, which the host owns.
                        if menus.real_play {
                            self.quit_678 = true;
                        } else {
                            self.alternate_67c = true;
                        }
                    }
                    _ => {}
                },
                MenuEvent::Enter {
                    menu,
                    callback,
                    arg,
                } => {
                    if arg == 0 {
                        enter_callback(menus, menu, callback, options);
                    }
                }
                MenuEvent::Commit { menu, callback } => {
                    commit_callback(menus, menu, callback, options);
                }
                MenuEvent::Leave { .. } => {}
            }
        }
    }

    /// `FUN_00438970`: one paused frame. `real_play` is `DAT_00d3ca88 == 0`;
    /// `slow_motion` selects the 25-frame fade.
    pub fn paused_tick(
        &mut self,
        keys: PauseKeys,
        real_play: bool,
        slow_motion: bool,
        options: &mut PauseOptions,
        effects: &mut Vec<PauseEffect>,
    ) -> PauseDecision {
        effects.push(PauseEffect::ZeroForceFeedback);
        self.menu_input(keys, options, effects);
        let fade = ((((i32::from(slow_motion) + 1) * 25) << 16) / 2) >> 16;
        if keys.shift && keys.q {
            return PauseDecision::QuitHotkey;
        }
        if self.quit_678 {
            effects.push(PauseEffect::FadeOut { frames: fade });
            return PauseDecision::Quit;
        }
        if real_play && self.restart_680 {
            self.restart_request_550018 = true;
            effects.push(PauseEffect::FadeOut { frames: fade });
            return PauseDecision::Restart;
        }
        if self.alternate_67c {
            if real_play {
                effects.push(PauseEffect::CommitTelemetryAndStopAudio);
            }
            return PauseDecision::Alternate;
        }
        if self.resume_670 {
            self.resume_670 = false;
            effects.push(PauseEffect::RestoreSpeech);
            let to_entry_state = !real_play && self.prepause_551b28 == 7;
            if to_entry_state {
                self.prepause_551b28 = 0;
            }
            return PauseDecision::Resume { to_entry_state };
        }
        PauseDecision::Stay
    }
}

fn value_slot(menus: &mut PauseMenus, menu: MenuId, key: i32) -> &mut i32 {
    let is_sound = menu == menus.sound;
    match menus.set.menu(menu).find(key) {
        Some(index) => &mut menus.set.menu_mut(menu).items[index].value.value,
        None if is_sound => &mut menus.sound_phantom_34,
        None => &mut menus.settings_phantom_34,
    }
}

/// `0x467f70` / `0x468070`: load the item values from the option bytes.
fn enter_callback(menus: &mut PauseMenus, menu: MenuId, callback: u32, options: &PauseOptions) {
    match callback {
        callback::SOUND_ENTER => {
            *value_slot(menus, menu, KEY_EFFECTS) = i32::from(options.cfg_5 & 0x7f);
            *value_slot(menus, menu, KEY_SPEECH) = i32::from(options.cfg_6 & 0x7f);
        }
        callback::SETTINGS_ENTER => {
            *value_slot(menus, menu, KEY_FLAG) = i32::from((options.cfg_8 >> 2) & 1);
            *value_slot(menus, menu, KEY_THREE_WAY) = i32::from((options.cfg_8 >> 3) & 3);
        }
        _ => {}
    }
}

/// `0x467fe0` / `0x4680d0`: write the item values back, every input step.
fn commit_callback(
    menus: &mut PauseMenus,
    menu: MenuId,
    callback: u32,
    options: &mut PauseOptions,
) {
    match callback {
        callback::SOUND_COMMIT => {
            let effects = *value_slot(menus, menu, KEY_EFFECTS) as u8;
            let speech = *value_slot(menus, menu, KEY_SPEECH) as u8;
            options.cfg_5 = (options.cfg_5 & 0x80) | (effects & 0x7f);
            options.cfg_6 = (options.cfg_6 & 0x80) | (speech & 0x7f);
            options.effects_master_4f653c = i32::from(options.cfg_5 & 0x7f) * 20;
            options.speech_master_4f6538 = i32::from(options.cfg_6 & 0x7f) * 20;
        }
        callback::SETTINGS_COMMIT => {
            let flag = *value_slot(menus, menu, KEY_FLAG) == 1;
            let three_way = *value_slot(menus, menu, KEY_THREE_WAY) as u8 & 3;
            options.cfg_8 = (options.cfg_8 & !0x04) | (u8::from(flag) << 2);
            options.cfg_8 = (options.cfg_8 & !0x18) | (three_way << 3);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONDITIONS: PauseMenuConditions = PauseMenuConditions {
        real_play: true,
        link_session: false,
        two_player_mode: false,
        cfg_166_mode: 0,
        slow_motion: false,
    };

    const OPTIONS: PauseOptions = PauseOptions {
        cfg_5: 5,
        cfg_6: 5,
        cfg_8: 0,
        view_toggle_4eb5f0: 0,
        effects_master_4f653c: 100,
        speech_master_4f6538: 100,
    };

    const OPEN: PauseKeyGates = PauseKeyGates {
        player_finished: false,
        player_active: true,
        blocked_55ae9c: false,
        real_play: true,
        alternate_6c0: false,
    };

    fn driving(escape_down: bool) -> DrivingPauseInput {
        DrivingPauseInput {
            state_id: 5,
            escape_down,
            player: 0,
            any_countdown_75c: false,
            gates: OPEN,
            conditions: CONDITIONS,
            one_player: true,
        }
    }

    fn paused(
        c: &mut PauseController,
        keys: PauseKeys,
        options: &mut PauseOptions,
    ) -> PauseDecision {
        let mut effects = Vec::new();
        c.paused_tick(keys, true, false, options, &mut effects)
    }

    fn enter_pause(c: &mut PauseController) {
        let mut effects = Vec::new();
        assert!(!c.driving_tick(driving(false), OPTIONS, &mut effects));
        assert!(c.driving_tick(driving(true), OPTIONS, &mut effects));
        assert_eq!(
            effects,
            vec![
                PauseEffect::MuteEngine { car: 0 },
                PauseEffect::StopSurfaceLoops,
                PauseEffect::MuteSpeech
            ]
        );
    }

    #[test]
    fn the_root_lists_the_recovered_items_with_resume_selected() {
        let menus = build_pause_menus(CONDITIONS, OPTIONS);
        let root = menus.set.menu(menus.root);
        let texts: Vec<i32> = root.items.iter().map(|item| item.text_id).collect();
        assert_eq!(texts, vec![1, 2, 0xe0, 3, 5]);
        assert_eq!(root.selected, 4);
        assert_eq!(root.parent, Some(menus.sentinel));
        // No restart outside real play, mode 3, or a link session.
        for conditions in [
            PauseMenuConditions {
                real_play: false,
                ..CONDITIONS
            },
            PauseMenuConditions {
                cfg_166_mode: 3,
                ..CONDITIONS
            },
            PauseMenuConditions {
                link_session: true,
                ..CONDITIONS
            },
        ] {
            let menus = build_pause_menus(conditions, OPTIONS);
            assert!(menus.restart.is_none());
            assert_eq!(menus.set.menu(menus.root).items.len(), 4);
        }
        let mode_two = build_pause_menus(
            PauseMenuConditions {
                cfg_166_mode: 2,
                ..CONDITIONS
            },
            OPTIONS,
        );
        assert_eq!(mode_two.set.menu(mode_two.root).items[2].text_id, 0xe2);
        assert_eq!(mode_two.set.menu(mode_two.restart.unwrap()).title_id, 0xe3);
    }

    #[test]
    fn the_submenus_follow_their_conditions() {
        let menus = build_pause_menus(CONDITIONS, OPTIONS);
        assert_eq!(menus.set.menu(menus.sound).items.len(), 3);
        assert_eq!(menus.set.menu(menus.settings).items.len(), 3);
        let two = build_pause_menus(
            PauseMenuConditions {
                two_player_mode: true,
                slow_motion: true,
                ..CONDITIONS
            },
            OPTIONS,
        );
        let sound: Vec<i32> = two
            .set
            .menu(two.sound)
            .items
            .iter()
            .map(|i| i.text_id)
            .collect();
        assert_eq!(sound, vec![8, 9]);
        let settings: Vec<i32> = two
            .set
            .menu(two.settings)
            .items
            .iter()
            .map(|i| i.text_id)
            .collect();
        assert_eq!(settings, vec![0xb, 0xc, 9]);
        // Confirmations default to "no".
        assert_eq!(menus.set.menu(menus.quit).selected, 1);
        assert_eq!(
            menus.set.menu(menus.restart.unwrap()).forced_selection,
            Some(1)
        );
    }

    #[test]
    fn the_opening_escape_is_swallowed_and_a_second_resumes() {
        let mut c = PauseController::default();
        enter_pause(&mut c);
        let mut options = OPTIONS;
        let held = PauseKeys {
            escape: true,
            ..PauseKeys::default()
        };
        // The Escape that opened the pause is still down: ignored.
        assert_eq!(paused(&mut c, held, &mut options), PauseDecision::Stay);
        // Unchanged mask -> 0 -> the latch arms.
        assert_eq!(paused(&mut c, held, &mut options), PauseDecision::Stay);
        assert!(c.latch_55ae9c);
        paused(&mut c, PauseKeys::default(), &mut options);
        let decision = paused(&mut c, held, &mut options);
        assert_eq!(
            decision,
            PauseDecision::Resume {
                to_entry_state: false
            }
        );
        assert!(!c.latch_55ae9c && !c.resume_670);
    }

    #[test]
    fn confirming_the_default_item_resumes_and_restores_speech() {
        let mut c = PauseController::default();
        enter_pause(&mut c);
        let mut options = OPTIONS;
        paused(&mut c, PauseKeys::default(), &mut options);
        let mut effects = Vec::new();
        let enter = PauseKeys {
            enter: true,
            ..PauseKeys::default()
        };
        let decision = c.paused_tick(enter, true, false, &mut options, &mut effects);
        assert_eq!(
            decision,
            PauseDecision::Resume {
                to_entry_state: false
            }
        );
        assert!(effects.contains(&PauseEffect::RestoreSpeech));
    }

    fn press(
        c: &mut PauseController,
        keys: PauseKeys,
        options: &mut PauseOptions,
    ) -> PauseDecision {
        let decision = paused(c, keys, options);
        if decision != PauseDecision::Stay {
            return decision;
        }
        paused(c, PauseKeys::default(), options)
    }

    #[test]
    fn quit_needs_yes_in_its_confirmation() {
        let mut c = PauseController::default();
        enter_pause(&mut c);
        let mut options = OPTIONS;
        paused(&mut c, PauseKeys::default(), &mut options);
        let up = PauseKeys {
            up: true,
            ..PauseKeys::default()
        };
        let enter = PauseKeys {
            enter: true,
            ..PauseKeys::default()
        };
        press(&mut c, up, &mut options);
        assert_eq!(press(&mut c, enter, &mut options), PauseDecision::Stay);
        let menus = c.menus.as_ref().unwrap();
        assert_eq!(c.current, Some(menus.quit));
        // "No" is selected: confirming it goes back to the root.
        press(&mut c, enter, &mut options);
        assert_eq!(c.current, Some(c.menus.as_ref().unwrap().root));
        press(&mut c, enter, &mut options);
        press(&mut c, up, &mut options);
        let mut effects = Vec::new();
        let decision = c.paused_tick(enter, true, false, &mut options, &mut effects);
        assert_eq!(decision, PauseDecision::Quit);
        assert!(effects.contains(&PauseEffect::FadeOut { frames: 12 }));
    }

    #[test]
    fn restart_sets_the_hub_request() {
        let mut c = PauseController::default();
        enter_pause(&mut c);
        let mut options = OPTIONS;
        paused(&mut c, PauseKeys::default(), &mut options);
        let up = PauseKeys {
            up: true,
            ..PauseKeys::default()
        };
        let enter = PauseKeys {
            enter: true,
            ..PauseKeys::default()
        };
        press(&mut c, up, &mut options);
        press(&mut c, up, &mut options);
        press(&mut c, enter, &mut options);
        assert_eq!(c.current, c.menus.as_ref().unwrap().restart);
        assert_eq!(
            c.menus
                .as_ref()
                .unwrap()
                .set
                .menu(c.current.unwrap())
                .selected,
            1
        );
        press(&mut c, up, &mut options);
        assert_eq!(press(&mut c, enter, &mut options), PauseDecision::Restart);
        assert!(c.restart_request_550018);
    }

    #[test]
    fn the_sound_submenu_writes_the_masters_on_every_step() {
        let mut c = PauseController::default();
        enter_pause(&mut c);
        let mut options = PauseOptions {
            cfg_5: 0x83,
            ..OPTIONS
        };
        paused(&mut c, PauseKeys::default(), &mut options);
        let down = PauseKeys {
            down: true,
            ..PauseKeys::default()
        };
        let enter = PauseKeys {
            enter: true,
            ..PauseKeys::default()
        };
        let left = PauseKeys {
            left: true,
            ..PauseKeys::default()
        };
        press(&mut c, down, &mut options);
        press(&mut c, enter, &mut options);
        assert_eq!(c.current, Some(c.menus.as_ref().unwrap().sound));
        // Entering loaded cfg[5] & 0x7f = 3 into the effects item.
        assert_eq!(options.effects_master_4f653c, 60);
        press(&mut c, down, &mut options);
        let mut effects = Vec::new();
        c.paused_tick(left, true, false, &mut options, &mut effects);
        assert_eq!(options.cfg_5, 0x82);
        assert_eq!(options.effects_master_4f653c, 40);
        assert!(effects.iter().any(|effect| matches!(
            effect,
            PauseEffect::MenuSound {
                sound: MenuSound::ValueChange,
                ok: true,
                preview_speech: false,
                value: 2,
                ..
            }
        )));
    }

    #[test]
    fn shift_q_quits_without_closing_and_blocks_escape_afterwards() {
        let mut c = PauseController::default();
        enter_pause(&mut c);
        let mut options = OPTIONS;
        paused(&mut c, PauseKeys::default(), &mut options);
        let decision = paused(
            &mut c,
            PauseKeys {
                shift: true,
                q: true,
                ..PauseKeys::default()
            },
            &mut options,
        );
        assert_eq!(decision, PauseDecision::QuitHotkey);
        assert!(c.latch_55ae9c);
        let mut effects = Vec::new();
        c.driving_tick(driving(false), OPTIONS, &mut effects);
        assert!(!c.driving_tick(driving(true), OPTIONS, &mut effects));
    }

    #[test]
    fn pausing_from_state_7_returns_there_in_front_end_mode() {
        let mut c = PauseController::default();
        let mut effects = Vec::new();
        let input = DrivingPauseInput {
            state_id: 7,
            ..driving(false)
        };
        c.driving_tick(input, OPTIONS, &mut effects);
        c.driving_tick(
            DrivingPauseInput {
                escape_down: true,
                ..input
            },
            OPTIONS,
            &mut effects,
        );
        assert_eq!(c.prepause_551b28, 7);
        let mut options = OPTIONS;
        c.paused_tick(
            PauseKeys::default(),
            false,
            false,
            &mut options,
            &mut effects,
        );
        let decision = c.paused_tick(
            PauseKeys {
                escape: true,
                ..PauseKeys::default()
            },
            false,
            false,
            &mut options,
            &mut effects,
        );
        assert_eq!(
            decision,
            PauseDecision::Resume {
                to_entry_state: true
            }
        );
        assert_eq!(c.prepause_551b28, 0);
    }
}
