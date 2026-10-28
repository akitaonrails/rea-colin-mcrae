//! The time-trial stage select `0xd47450`: seven stage toggles (labels "1"
//! to "6" and the special stage, text `0x2b`) that Return flips
//! (`FUN_004aea40`), and an OK item (text `0xdf`) leading to the race.
//!
//! Enter (`0x4c7d60`): for country `c` (`record[0x181] & 0xf`) each toggle's
//! availability, and its value from bit `i` of the country's stage mask
//! `record[0x16c + 2c]`. The commit (`0x4c8320`) re-enables moving, confirm
//! and back once the slide-in ends, without left/right, and switches the
//! first stage on when at most one item is available (not for the secret
//! country 9). The predraw (`0x4c8680`) enables OK only with a stage on and
//! routes it to the race (the network screen for a network host). The post
//! (`0x4c84d0`) writes the mask back from the first `record[0x16d + 2c] & 0xf`
//! toggles and, going forward, commits the record (`FUN_004b93f0`). A
//! description of disassembly, not original source. Evidence:
//! `rea.official-eu.frontend-stage-select`.

use crate::config::LEGACY_CONFIG_SIZE;
use crate::descriptor_menu::{ItemKind, Menu, MenuId, MenuItem, MenuNav};
use crate::event_flow::SPECIAL_COUNTRIES;
use crate::frontend_menu::ValueWidget;

type Record = [u8; LEGACY_CONFIG_SIZE];

/// The OK item.
pub const OK_ITEM: usize = 7;

/// The stage select's menu and the screens its OK leads to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StageSelect {
    pub menu: Menu,
    race: MenuId,
    network: MenuId,
}

fn country(record: &Record) -> usize {
    usize::from(record[0x181] & 0xf)
}

impl StageSelect {
    /// The screen as `FUN_004be540` builds it: seven wrapping two-value
    /// toggles that Return increments, then OK. The toggles' labels are
    /// executable strings (`"1"`..`"6"`), shown as text id `-1`.
    pub fn build(parent: MenuId, race: MenuId, network: MenuId) -> Self {
        let mut menu = Menu::new(0x2f, Some(parent), Some(0x00d9_0050));
        for index in 0..7 {
            let mut item = MenuItem::new(ItemKind::Value, if index == 6 { 0x2b } else { -1 }, 0);
            item.value = ValueWidget {
                value: 0,
                value_count: 2,
                wrap: true,
            };
            item.confirm_increments = true;
            menu.items.push(item);
        }
        let mut ok = MenuItem::new(ItemKind::Submenu(race), 0xdf, 0);
        ok.key = 7;
        menu.items.push(ok);
        Self {
            menu,
            race,
            network,
        }
    }

    /// The enter callback's item rules, then the selection on the first
    /// item and no navigation until the slide-in ends.
    pub fn enter(&mut self, record: &Record) {
        let c = country(record);
        let mask = record[0x16c + 2 * c];
        for (i, item) in self.menu.items.iter_mut().take(7).enumerate() {
            let (enabled, visible) = match c {
                8 => {
                    let on = i < usize::from((record[0x2b] >> 1) & 7) && i < 4;
                    (on, on)
                }
                9 => match i {
                    0 => (record[0x2f] & 0x10 != 0, true),
                    1 => (record[0x2f] & 0x20 != 0, true),
                    2 => (record[0x2f] & 0x80 != 0, true),
                    3 => (record[0x2f] & 0x40 != 0, true),
                    _ => (false, false),
                },
                _ if i >= 6 && !SPECIAL_COUNTRIES.contains(&(c as u8)) => (false, false),
                _ => {
                    let reach = record[0x2a];
                    let on = c < usize::from(reach >> 4)
                        || (c < usize::from(reach & 0xf) && i < 3)
                        || i == 0;
                    (on, true)
                }
            };
            item.enabled = enabled;
            item.visible = visible;
            item.value.value = i32::from(mask & (1 << i) != 0);
        }
        self.menu.selected = 0;
        self.menu.nav = MenuNav::NONE;
    }

    /// The commit after the slide-in: move, confirm and back, no left/right;
    /// then the first stage is forced on when at most one item can be used.
    pub fn commit(&mut self, record: &Record, slide_finished: bool) {
        if slide_finished {
            self.menu.nav = MenuNav {
                moves: true,
                adjusts: false,
                confirms: true,
                backs: true,
            };
        }
        let usable = self
            .menu
            .items
            .iter()
            .filter(|item| item.enabled && item.visible)
            .count();
        if usable <= 1 && country(record) != 9 {
            self.menu.items[0].value.value = 1;
        }
    }

    /// The predraw's routing: OK is enabled with a stage on among the shown
    /// rows, and leads nowhere with none, to the network screen for a
    /// network host, else to the race.
    pub fn predraw(&mut self, record: &Record) {
        let rows = usize::from(record[0x16d + 2 * country(record)] & 0xf);
        let any = self
            .menu
            .items
            .iter()
            .take(7)
            .any(|item| item.value.value != 0);
        let on = self
            .menu
            .items
            .iter()
            .take(rows.min(7))
            .filter(|item| item.visible && item.value.value == 1)
            .count();
        let ok = &mut self.menu.items[OK_ITEM];
        ok.kind = if !any {
            ItemKind::Action
        } else if record[0x182] & 7 == 4 {
            ItemKind::Submenu(self.network)
        } else {
            ItemKind::Submenu(self.race)
        };
        ok.enabled = on > 0;
    }

    /// The post callback: the stage mask from the first `count` toggles;
    /// back returns `DAT_00d8cee8` (the last name entry's player), forward
    /// commits.
    pub fn post(&self, record: &mut Record, back: bool) -> PostOutcome {
        let c = country(record);
        let count = usize::from(record[0x16d + 2 * c] & 0xf);
        let mut mask = 0u8;
        for (i, item) in self.menu.items.iter().take(count.min(8)).enumerate() {
            if item.value.value > 0 {
                mask |= 1 << i;
            }
        }
        record[0x16c + 2 * c] = mask;
        if back {
            let players = record[0x182] & 7;
            PostOutcome::Back {
                next_player: usize::from(!matches!(players, 0 | 4 | 5)),
            }
        } else {
            PostOutcome::Commit
        }
    }
}

/// What the post decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PostOutcome {
    /// Back to the name entry, naming this player.
    Back { next_player: usize },
    /// `FUN_004b93f0`.
    Commit,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::descriptor_menu::{MenuSet, bits};

    fn screen() -> (MenuSet, MenuId, StageSelect) {
        let mut set = MenuSet::default();
        let parent = set.add(Menu::new(0, None, None));
        let race = set.add(Menu::new(-1, None, None));
        let network = set.add(Menu::new(-1, None, None));
        let select = StageSelect::build(parent, race, network);
        let id = set.add(select.menu.clone());
        (set, id, select)
    }

    fn record(country: u8, reach: u8, mask: u8, stages: u8) -> Box<Record> {
        let mut record = Box::new([0u8; LEGACY_CONFIG_SIZE]);
        record[0x181] = country;
        record[0x2a] = reach;
        record[0x16c + 2 * usize::from(country)] = mask;
        record[0x16d + 2 * usize::from(country)] = stages;
        record
    }

    #[test]
    fn a_locked_country_offers_its_first_stage_only() {
        let (_, _, mut select) = screen();
        let record = record(2, 0x11, 0, 6);
        select.enter(&record);
        let enabled: Vec<bool> = select.menu.items[..7].iter().map(|i| i.enabled).collect();
        assert_eq!(enabled, [true, false, false, false, false, false, false]);
        assert!(
            !select.menu.items[6].visible,
            "no special stage in country 2"
        );
        // The frame's predraw runs before the next step's commit.
        select.predraw(&record);
        select.commit(&record, true);
        assert_eq!(select.menu.items[0].value.value, 1, "forced on");
        assert!(!select.menu.nav.adjusts);
    }

    #[test]
    fn reached_countries_open_their_stages() {
        let (_, _, mut select) = screen();
        // Country 1 is below the rally reach 2: every stage, and its special.
        let record = record(1, 0x21, 0b100, 7);
        select.enter(&record);
        assert!(select.menu.items[..7].iter().all(|item| item.enabled));
        assert_eq!(select.menu.items[2].value.value, 1);
        // Below only the championship reach: the first three.
        let record = record_with(3, 0x14);
        select.enter(&record);
        let enabled: Vec<bool> = select.menu.items[..7].iter().map(|i| i.enabled).collect();
        assert_eq!(enabled, [true, true, true, false, false, false, false]);
    }

    fn record_with(country: u8, reach: u8) -> Box<Record> {
        record(country, reach, 0, 7)
    }

    #[test]
    fn return_flips_a_stage_and_ok_writes_the_mask() {
        let (mut set, id, mut select) = screen();
        let mut record = record(0, 0x10, 0b1, 6);
        select.enter(&record);
        select.commit(&record, true);
        set.menus[id.0] = select.menu.clone();
        // Down to stage 2 (index 1 is enabled: country 0 is below the reach).
        set.step(id, bits::DOWN);
        set.step(id, bits::CONFIRM);
        assert_eq!(set.menu(id).items[1].value.value, 1);
        // Return again turns it off (the toggles wrap).
        set.step(id, bits::CONFIRM);
        set.step(id, bits::CONFIRM);
        select.menu = set.menu(id).clone();
        select.predraw(&record);
        assert!(select.menu.items[OK_ITEM].enabled);
        assert_eq!(select.post(&mut record, false), PostOutcome::Commit);
        assert_eq!(record[0x16c], 0b11);
        // Nothing on: OK leads nowhere and is disabled.
        for item in &mut select.menu.items[..7] {
            item.value.value = 0;
        }
        select.predraw(&record);
        assert_eq!(select.menu.items[OK_ITEM].kind, ItemKind::Action);
        assert!(!select.menu.items[OK_ITEM].enabled);
        record[0x182] = 2;
        assert_eq!(
            select.post(&mut record, true),
            PostOutcome::Back { next_player: 1 }
        );
    }

    #[test]
    fn the_super_special_and_secret_countries() {
        let (_, _, mut select) = screen();
        let mut record = record(8, 0, 0, 4);
        record[0x2b] = 0b0110; // three super-special stages
        select.enter(&record);
        let enabled: Vec<bool> = select.menu.items[..7].iter().map(|i| i.enabled).collect();
        assert_eq!(enabled, [true, true, true, false, false, false, false]);
        assert!(!select.menu.items[3].visible);
        let mut secret = record_with(9, 0);
        secret[0x2f] = 0x80;
        select.enter(&secret);
        let enabled: Vec<bool> = select.menu.items[..4].iter().map(|i| i.enabled).collect();
        assert_eq!(enabled, [false, false, true, false]);
        select.commit(&secret, true);
        assert_eq!(
            select.menu.items[0].value.value, 0,
            "not forced for country 9"
        );
    }
}
