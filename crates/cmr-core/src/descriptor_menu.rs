//! The generic descriptor-menu engine the in-race pause menu runs on.
//!
//! A menu object (`0x4ae5f0` create) holds a title, up to twenty `0x34`-byte
//! items and a set of callbacks; `FUN_004aed50` activates a menu and
//! `FUN_004aee60(menu, bits)` applies one input step and returns the menu to
//! switch to. This module reconstructs the subset the pause menus reach:
//! vertical lists with wrap, the back/submenu/value/action item kinds, the
//! enter/commit/leave callbacks and the four sound-table slots. The linked
//! neighbour menus (`+0x20..+0x3c`), the type-6 value kind and the item
//! out-pointers are all zero in every pause menu and are not modelled.
//! A description of disassembly, not original source. Evidence:
//! `rea.official-eu.pause-menu`.
//!
//! Callbacks are host-owned: the engine reports them as [`MenuEvent`]s in the
//! order the native calls them, and the host performs them.

use crate::frontend_menu::{ValueDirection, ValueWidget};

/// Input bits of one step (`FUN_0043baa0`'s mask).
pub mod bits {
    pub const LEFT: u32 = 0x01;
    pub const RIGHT: u32 = 0x02;
    pub const UP: u32 = 0x04;
    pub const DOWN: u32 = 0x08;
    pub const CONFIRM: u32 = 0x10;
    pub const BACK: u32 = 0x20;
}

/// An index into a [`MenuSet`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MenuId(pub usize);

/// The item type at `+0x08`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemKind {
    /// `1`: go to the parent menu on confirm.
    Back,
    /// `2`: go to the linked submenu on confirm.
    Submenu(MenuId),
    /// `3`: a value or toggle adjusted by left/right.
    Value,
    /// `4`: fire the activate callback on confirm.
    Action,
}

/// One item record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuItem {
    /// `+0x04`: the label's string id.
    pub text_id: i32,
    pub kind: ItemKind,
    /// `+0x0c`: the key callbacks search by.
    pub key: i32,
    /// `+0x10`: the host's draw callback id.
    pub draw: u32,
    /// `+0x18`/`+0x1c`/`+0x2e`.
    pub value: ValueWidget,
    /// `+0x28`: the host's activate callback id.
    pub activate: Option<u32>,
    /// `+0x2c`: selectable and confirmable.
    pub enabled: bool,
    /// `+0x2d`: drawn.
    pub visible: bool,
    /// `+0x2f` (`FUN_004aea40`): confirming a value item also increments it.
    pub confirm_increments: bool,
}

impl MenuItem {
    /// `0x4ae6d0` plus the type builders: enabled and visible, no callbacks.
    pub fn new(kind: ItemKind, text_id: i32, draw: u32) -> Self {
        Self {
            text_id,
            kind,
            key: 0,
            draw,
            value: ValueWidget {
                value: 0,
                value_count: 0,
                wrap: false,
            },
            activate: None,
            enabled: true,
            visible: true,
            confirm_increments: false,
        }
    }
}

/// The four sound-table slots (`+0x498` vtable).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuSound {
    /// `[0]`: the selection moved (or tried to).
    Move,
    /// `[1]`: a value changed (or tried to).
    ValueChange,
    /// `[2]`: entered a submenu.
    Enter,
    /// `[3]`: went back to the parent.
    Back,
}

/// The navigation enables at `+0x0c..+0x0f` (`FUN_004aec50`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuNav {
    /// `+0x0c`: moving the selection.
    pub moves: bool,
    /// `+0x0d`: adjusting a value.
    pub adjusts: bool,
    /// `+0x0e`: confirm.
    pub confirms: bool,
    /// `+0x0f`: back.
    pub backs: bool,
}

impl MenuNav {
    pub const ALL: Self = Self {
        moves: true,
        adjusts: true,
        confirms: true,
        backs: true,
    };
    pub const NONE: Self = Self {
        moves: false,
        adjusts: false,
        confirms: false,
        backs: false,
    };
}

/// A menu object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Menu {
    /// `+0x04`.
    pub title_id: i32,
    /// `+0x08 != 1`: a horizontal list (left/right move, up/down adjust).
    pub horizontal: bool,
    /// `+0x0c..+0x0f`.
    pub nav: MenuNav,
    /// `+0x10`: the selection wraps.
    pub wrap: bool,
    /// `+0x14`.
    pub parent: Option<MenuId>,
    pub items: Vec<MenuItem>,
    /// `+0x44`.
    pub selected: usize,
    /// `+0x48`: the selection forced on activation.
    pub forced_selection: Option<usize>,
    /// `+0x460`/`+0x464`/`+0x468`/`+0x470` host callback ids.
    pub on_enter: Option<u32>,
    pub on_commit: Option<u32>,
    pub on_predraw: Option<u32>,
    /// `+0x46c`: called after the commit whenever the step leaves the menu,
    /// with whether it went back.
    pub on_post: Option<u32>,
    pub on_leave: Option<u32>,
    /// `+0x498`: the sound table id, or `None` for a silent menu.
    pub sounds: Option<u32>,
    /// `+0x494`: the last adjusted value.
    pub last_value: i32,
}

impl Menu {
    /// `0x4ae5f0` with `aec80(m, 1)`: a vertical, wrapping list.
    pub fn new(title_id: i32, parent: Option<MenuId>, sounds: Option<u32>) -> Self {
        Self {
            title_id,
            horizontal: false,
            nav: MenuNav::ALL,
            wrap: true,
            parent,
            items: Vec::new(),
            selected: 0,
            forced_selection: None,
            on_enter: None,
            on_commit: None,
            on_predraw: None,
            on_post: None,
            on_leave: None,
            sounds,
            last_value: 0,
        }
    }

    /// `FUN_004aea80`: the index of the item with `key`, or `None`.
    pub fn find(&self, key: i32) -> Option<usize> {
        self.items.iter().position(|item| item.key == key)
    }
}

/// What a step or an activation asks the host to do, in native order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuEvent {
    /// A sound-table slot fired (`ok` is the success byte the cue checks).
    Sound {
        menu: MenuId,
        table: u32,
        sound: MenuSound,
        item: usize,
        ok: bool,
    },
    /// An item's activate callback.
    Activate {
        menu: MenuId,
        item: usize,
        callback: u32,
    },
    /// The menu's commit callback (every step).
    Commit { menu: MenuId, callback: u32 },
    /// The post callback, with `DAT_00bbac04` (the step went back).
    Post {
        menu: MenuId,
        callback: u32,
        back: bool,
    },
    /// The previous menu's leave callback.
    Leave { menu: MenuId, callback: u32 },
    /// The menu's enter callback with its argument.
    Enter {
        menu: MenuId,
        callback: u32,
        arg: u8,
    },
}

/// The menus of one menu tree.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MenuSet {
    pub menus: Vec<Menu>,
    /// `DAT_00bbac08`: the menu the next step switches to; a callback can set
    /// it (`FUN_004af3a0`), and the step returns and clears it.
    pub pending: Option<MenuId>,
    /// `DAT_00bbac04`: whether the last step that left a menu went back.
    pub went_back: bool,
}

impl MenuSet {
    pub fn add(&mut self, menu: Menu) -> MenuId {
        self.menus.push(menu);
        MenuId(self.menus.len() - 1)
    }

    pub fn menu(&self, id: MenuId) -> &Menu {
        &self.menus[id.0]
    }

    pub fn menu_mut(&mut self, id: MenuId) -> &mut Menu {
        &mut self.menus[id.0]
    }

    /// `FUN_004aed50(m, arg)`: select the forced item, settle on the first
    /// enabled and visible item, leave the previous menu and enter this one.
    /// The pause menus never set the auto-parent byte `+0x12`.
    pub fn activate(&mut self, id: MenuId, previous: Option<MenuId>, arg: u8) -> Vec<MenuEvent> {
        let mut events = Vec::new();
        let menu = &mut self.menus[id.0];
        let count = menu.items.len();
        if let Some(forced) = menu.forced_selection.filter(|&forced| forced < count) {
            menu.selected = forced;
        }
        for _ in 0..count {
            let item = &menu.items[menu.selected];
            if item.enabled && item.visible {
                break;
            }
            menu.selected = (menu.selected + 1) % count;
        }
        if let Some(previous) = previous
            && let Some(callback) = self.menus[previous.0].on_leave
        {
            events.push(MenuEvent::Leave {
                menu: previous,
                callback,
            });
        }
        if let Some(callback) = self.menus[id.0].on_enter {
            events.push(MenuEvent::Enter {
                menu: id,
                callback,
                arg,
            });
        }
        events
    }

    /// `FUN_004aee60(m, bits)`: one input step. Returns the menu to switch to
    /// (the parent, a submenu or one a callback set), and the callbacks in
    /// native order.
    pub fn step(&mut self, id: MenuId, input: u32) -> (Option<MenuId>, Vec<MenuEvent>) {
        let mut events = Vec::new();
        let mut back = false;
        if self.pending.is_none() {
            back = self.input_step(id, input, &mut events);
        }
        let (pending, post) = self.leave(id, back);
        events.extend(post);
        (pending, events)
    }

    /// The step's tail (`0x4af2c1`) for a host whose commit callback set the
    /// pending menu (`FUN_004af3a0`): the post callback and the switch. The
    /// native runs callbacks inside the step, so this completes the same step.
    pub fn resolve_pending(&mut self, id: MenuId) -> (Option<MenuId>, Vec<MenuEvent>) {
        self.leave(id, false)
    }

    fn leave(&mut self, id: MenuId, back: bool) -> (Option<MenuId>, Vec<MenuEvent>) {
        let mut events = Vec::new();
        let menu = &self.menus[id.0];
        if let (Some(callback), Some(_)) = (menu.on_post, self.pending) {
            let back = back
                || menu
                    .items
                    .get(menu.selected)
                    .is_some_and(|item| item.kind == ItemKind::Back);
            self.went_back = back;
            events.push(MenuEvent::Post {
                menu: id,
                callback,
                back,
            });
        }
        (self.pending.take(), events)
    }

    /// The input part of the step; returns the gated back input.
    fn input_step(&mut self, id: MenuId, input: u32, events: &mut Vec<MenuEvent>) -> bool {
        let menu = &mut self.menus[id.0];
        let count = menu.items.len();
        let nav = menu.nav;
        let confirm = nav.confirms && input & bits::CONFIRM != 0;
        let back = nav.backs && input & bits::BACK != 0;
        let (previous_bit, next_bit, decrement_bit, increment_bit) = if menu.horizontal {
            (bits::LEFT, bits::RIGHT, bits::UP, bits::DOWN)
        } else {
            (bits::UP, bits::DOWN, bits::LEFT, bits::RIGHT)
        };
        let previous = nav.moves && input & previous_bit != 0;
        let next = nav.moves && !previous && input & next_bit != 0;
        let moving = nav.moves && input & (previous_bit | next_bit) != 0;
        let adjusting = nav.adjusts && !moving;
        let left = adjusting && input & decrement_bit != 0;
        let right = adjusting && !left && input & increment_bit != 0;
        let mut pending = None;

        if count > 0 {
            for _ in 0..count {
                if menu.items[menu.selected].enabled {
                    break;
                }
                menu.selected = (menu.selected + 1) % count;
            }
        }

        if count > 0 && (previous || next) {
            let at_edge = if previous {
                menu.selected == 0
            } else {
                menu.selected + 1 >= count
            };
            let moved = if at_edge && !menu.wrap {
                false
            } else {
                for _ in 0..count {
                    menu.selected = if previous {
                        (menu.selected + count - 1) % count
                    } else {
                        (menu.selected + 1) % count
                    };
                    let item = &menu.items[menu.selected];
                    if item.enabled && item.visible {
                        break;
                    }
                }
                true
            };
            if let Some(table) = menu.sounds {
                events.push(MenuEvent::Sound {
                    menu: id,
                    table,
                    sound: MenuSound::Move,
                    item: menu.selected,
                    ok: moved,
                });
            }
        }

        if count == 0 {
            if back {
                pending = menu.parent;
            }
            if let Some(callback) = menu.on_commit {
                events.push(MenuEvent::Commit { menu: id, callback });
            }
            self.pending = self.pending.or(pending);
            return back;
        }

        let selected = menu.selected;
        let item = &mut menu.items[selected];
        let confirm_effective = confirm && item.enabled;
        if item.kind == ItemKind::Value {
            // `0x4af1d0`: a confirm on an item with `+0x2f` increments as
            // Right does, except that an item with an activate callback only
            // cues it.
            let confirm_increment = !left && !right && confirm_effective && item.confirm_increments;
            if left || right || confirm_increment {
                let direction = if left {
                    ValueDirection::Decrement
                } else {
                    ValueDirection::Increment
                };
                let ok = if confirm_increment && item.activate.is_some() {
                    true
                } else {
                    item.value.adjust(direction)
                };
                if let Some(table) = menu.sounds {
                    events.push(MenuEvent::Sound {
                        menu: id,
                        table,
                        sound: MenuSound::ValueChange,
                        item: selected,
                        ok,
                    });
                }
            }
            menu.last_value = item.value.value;
        }

        let item = &menu.items[selected];
        let sound = |sound| {
            menu.sounds.map(|table| MenuEvent::Sound {
                menu: id,
                table,
                sound,
                item: selected,
                ok: true,
            })
        };
        if confirm_effective {
            if let Some(callback) = item.activate {
                events.push(MenuEvent::Activate {
                    menu: id,
                    item: selected,
                    callback,
                });
            }
            if back || item.kind == ItemKind::Back {
                pending = menu.parent;
                events.extend(sound(MenuSound::Back));
            } else if let ItemKind::Submenu(target) = item.kind {
                pending = Some(target);
                events.extend(sound(MenuSound::Enter));
            }
        } else if back {
            pending = menu.parent;
            events.extend(sound(MenuSound::Back));
        }
        if let Some(callback) = menu.on_commit {
            events.push(MenuEvent::Commit { menu: id, callback });
        }
        self.pending = self.pending.or(pending);
        back
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value_item(text: i32, key: i32, count: i32, value: i32) -> MenuItem {
        let mut item = MenuItem::new(ItemKind::Value, text, 0);
        item.key = key;
        item.value = ValueWidget {
            value,
            value_count: count,
            wrap: false,
        };
        item
    }

    fn tree() -> (MenuSet, MenuId, MenuId) {
        let mut set = MenuSet::default();
        let root = set.add(Menu::new(0, None, Some(1)));
        let sub = set.add(Menu::new(6, Some(root), Some(2)));
        set.menu_mut(sub).items = vec![
            value_item(7, 3, 6, 5),
            value_item(8, 5, 6, 0),
            MenuItem::new(ItemKind::Back, 9, 0),
        ];
        set.menu_mut(sub).on_commit = Some(42);
        let mut resume = MenuItem::new(ItemKind::Action, 5, 0);
        resume.activate = Some(7);
        set.menu_mut(root).items = vec![
            MenuItem::new(ItemKind::Submenu(sub), 1, 0),
            MenuItem::new(ItemKind::Back, 3, 0),
            resume,
        ];
        (set, root, sub)
    }

    #[test]
    fn the_nav_enables_gate_each_input() {
        let (mut set, _, sub) = tree();
        set.menu_mut(sub).nav = MenuNav::NONE;
        let (pending, _) = set.step(sub, bits::DOWN | bits::RIGHT | bits::BACK);
        assert_eq!(pending, None);
        assert_eq!(set.menu(sub).selected, 0);
        assert_eq!(set.menu(sub).items[0].value.value, 5);
        set.menu_mut(sub).nav.adjusts = true;
        set.step(sub, bits::LEFT);
        assert_eq!(set.menu(sub).items[0].value.value, 4);
    }

    #[test]
    fn a_horizontal_list_moves_with_left_and_right() {
        let (mut set, root, _) = tree();
        set.menu_mut(root).horizontal = true;
        set.step(root, bits::DOWN);
        assert_eq!(set.menu(root).selected, 0);
        set.step(root, bits::RIGHT);
        assert_eq!(set.menu(root).selected, 1);
    }

    #[test]
    fn leaving_runs_the_post_callback_with_the_back_flag() {
        let (mut set, root, sub) = tree();
        set.menu_mut(sub).on_post = Some(9);
        let (pending, events) = set.step(sub, bits::BACK);
        assert_eq!(pending, Some(root));
        assert_eq!(
            events.last(),
            Some(&MenuEvent::Post {
                menu: sub,
                callback: 9,
                back: true
            })
        );
        assert!(set.went_back);
        // A pending menu a callback set skips the input and leaves at once.
        set.pending = Some(root);
        let (pending, events) = set.step(sub, bits::DOWN);
        assert_eq!(pending, Some(root));
        assert_eq!(set.menu(sub).selected, 0);
        assert!(matches!(events[..], [MenuEvent::Post { back: false, .. }]));
        assert_eq!(set.pending, None);
        // Without a pending menu nothing leaves.
        assert_eq!(set.resolve_pending(sub), (None, Vec::new()));
    }

    #[test]
    fn moving_wraps_and_skips_hidden_items() {
        let (mut set, root, _) = tree();
        set.menu_mut(root).items[1].visible = false;
        let (next, events) = set.step(root, bits::DOWN);
        assert_eq!(next, None);
        assert_eq!(set.menu(root).selected, 2);
        assert!(matches!(
            events[0],
            MenuEvent::Sound {
                sound: MenuSound::Move,
                ok: true,
                ..
            }
        ));
        set.step(root, bits::DOWN);
        assert_eq!(set.menu(root).selected, 0, "wraps past the end");
        set.step(root, bits::UP);
        assert_eq!(set.menu(root).selected, 2);
    }

    #[test]
    fn a_non_wrapping_menu_reports_a_failed_move() {
        let (mut set, root, _) = tree();
        set.menu_mut(root).wrap = false;
        let (_, events) = set.step(root, bits::UP);
        assert_eq!(set.menu(root).selected, 0);
        assert!(matches!(events[0], MenuEvent::Sound { ok: false, .. }));
    }

    #[test]
    fn confirm_enters_submenus_fires_actions_and_back_returns() {
        let (mut set, root, sub) = tree();
        let (next, events) = set.step(root, bits::CONFIRM);
        assert_eq!(next, Some(sub));
        assert!(matches!(
            events[0],
            MenuEvent::Sound {
                sound: MenuSound::Enter,
                ..
            }
        ));
        set.menu_mut(root).selected = 2;
        let (next, events) = set.step(root, bits::CONFIRM);
        assert_eq!(next, None);
        assert_eq!(
            events,
            vec![MenuEvent::Activate {
                menu: root,
                item: 2,
                callback: 7
            }]
        );
        set.menu_mut(sub).selected = 2;
        let (next, _) = set.step(sub, bits::CONFIRM);
        assert_eq!(next, Some(root));
        set.menu_mut(sub).selected = 0;
        let (next, _) = set.step(sub, bits::BACK);
        assert_eq!(next, Some(root));
    }

    #[test]
    fn left_and_right_adjust_values_and_commit_runs_every_step() {
        let (mut set, _, sub) = tree();
        let (_, events) = set.step(sub, bits::RIGHT);
        assert_eq!(set.menu(sub).items[0].value.value, 5, "already at the top");
        assert!(matches!(events[0], MenuEvent::Sound { ok: false, .. }));
        assert_eq!(
            events[1],
            MenuEvent::Commit {
                menu: sub,
                callback: 42
            }
        );
        set.step(sub, bits::LEFT);
        assert_eq!(set.menu(sub).items[0].value.value, 4);
        assert_eq!(set.menu(sub).last_value, 4);
        let (_, events) = set.step(sub, 0);
        assert_eq!(
            events,
            vec![MenuEvent::Commit {
                menu: sub,
                callback: 42
            }]
        );
        // A vertical move suppresses left/right in the same step.
        set.step(sub, bits::DOWN | bits::LEFT);
        assert_eq!(set.menu(sub).selected, 1);
        assert_eq!(set.menu(sub).items[1].value.value, 0);
    }

    #[test]
    fn activation_forces_the_selection_and_settles_on_a_live_item() {
        let (mut set, root, sub) = tree();
        set.menu_mut(sub).forced_selection = Some(2);
        set.menu_mut(sub).on_enter = Some(9);
        set.menu_mut(root).on_leave = Some(4);
        let events = set.activate(sub, Some(root), 0);
        assert_eq!(set.menu(sub).selected, 2);
        assert_eq!(
            events,
            vec![
                MenuEvent::Leave {
                    menu: root,
                    callback: 4
                },
                MenuEvent::Enter {
                    menu: sub,
                    callback: 9,
                    arg: 0
                }
            ]
        );
        set.menu_mut(root).items[0].enabled = false;
        set.menu_mut(root).selected = 0;
        set.activate(root, None, 0);
        assert_eq!(set.menu(root).selected, 1);
    }
}
