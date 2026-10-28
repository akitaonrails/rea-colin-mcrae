//! The race-setup screens' logic: the championship (`0xd40900`), single rally
//! (`0xd48230`) and time-trial (`0xd4b240`) setups built by `FUN_004be540`.
//!
//! Each is a vertical descriptor menu ([`crate::descriptor_menu`]) whose last
//! item, Continue, leads to the screen's name entry. The shared enter body
//! `FUN_004c8c20` loads the values from the cmRally.cfg record, and the
//! post-input callback `0x4c90c0` is the only writer of the selections back
//! into it. Which cars and countries may be chosen is the save progress:
//! [`car_allowed`] (`FUN_004c5620`) and [`country_allowed`] (`FUN_004c4490`).
//! The Continue item is enabled only when every enabled car and country item
//! passes ([`continue_enabled`], the `DAT_0050941c` AND the item draws build).
//!
//! The slide-in that holds the input until it finishes (`FUN_004c9040`) is the
//! host's: [`SetupScreen::slide_finished`] re-enables the navigation.
//! A description of disassembly, not original source. Evidence:
//! `rea.official-eu.frontend-setup`.

use crate::config::LEGACY_CONFIG_SIZE;
use crate::descriptor_menu::{ItemKind, Menu, MenuId, MenuItem, MenuNav};
use crate::frontend_menu::ValueWidget;

type Record = [u8; LEGACY_CONFIG_SIZE];

/// The first eight cars of the list, in order (`0x50940c`).
pub const BASE_CARS: [u8; 8] = [0, 6, 0xa, 9, 0xb, 1, 7, 2];
/// The cars bits 0..5 of the unlock mask append (`0x506dcc`, then 12, 13).
pub const BONUS_CARS: [u8; 6] = [5, 4, 8, 3, 0xc, 0xd];

/// Item keys.
pub mod key {
    pub const PLAYERS: i32 = 1;
    pub const CAR_1: i32 = 2;
    pub const CAR_2: i32 = 3;
    pub const GEAR_1: i32 = 4;
    pub const GEAR_2: i32 = 5;
    pub const COUNTRY: i32 = 6;
    pub const CONTINUE: i32 = 7;
}

/// The three setup screens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetupKind {
    Championship,
    Rally,
    TimeTrial,
}

impl SetupKind {
    /// The event mode its enter callback writes into `record[0x180]`.
    pub const fn mode(self) -> u8 {
        match self {
            Self::Championship => 0,
            Self::Rally => 1,
            Self::TimeTrial => 2,
        }
    }

    /// The screen's title text id.
    pub const fn title_id(self) -> i32 {
        match self {
            Self::Championship => 0,
            Self::Rally => 1,
            Self::TimeTrial => 0x78,
        }
    }
}

fn difficulty(record: &Record) -> u8 {
    (record[0x20] >> 3) & 3
}

/// `FUN_004c5620(car)` over the record: at difficulty 2 every car, at 3
/// none; at 0 only the cars 1, 2, 7 and 11; at 1 the cars 0, 6, 9 and 10, the
/// cars 1, 2, 7 and 11 once `record[0x2b]` bit 0 is unlocked, and the rest
/// only in a time trial.
pub fn car_allowed(record: &Record, car: u8) -> bool {
    match difficulty(record) {
        0 => matches!(car, 1 | 2 | 7 | 11),
        1 => match car {
            0 | 6 | 9 | 10 => true,
            1 | 2 | 7 | 11 => record[0x2b] & 1 != 0,
            _ => record[0x180] & 3 == 2,
        },
        2 => true,
        _ => false,
    }
}

/// `FUN_004c4490(country)` over the record; `time_trial` is `DAT_00d4ce40`
/// (the country item's screen is the time-trial setup).
pub fn country_allowed(record: &Record, country: u8, time_trial: bool) -> bool {
    let rally_reach = record[0x2a] >> 4;
    let championship_reach = record[0x2a] & 0xf;
    if !time_trial {
        if country == 8 && record[0x2b] & 0xe > 8 {
            return true;
        }
        if country < rally_reach {
            return true;
        }
        if record[0x20] & 0x18 != 0 {
            return false;
        }
        return country < championship_reach;
    }
    if matches!(country, 8 | 9) && record[0x2b] & 0xe != 0 {
        return true;
    }
    if (country as i8) <= 3 && !matches!(country, 8 | 9) {
        return true;
    }
    country < championship_reach || country < rally_reach
}

/// The car list the enter body builds (`DAT_00508ec0`): the eight base cars,
/// then each bonus car whose bit is set in `(record[0x2c] & 0x3f)`, all six
/// when `record[0x30]` bit 3 is set.
pub fn car_list(record: &Record) -> Vec<u8> {
    let mut mask = record[0x2c] & 0x3f;
    if record[0x30] & 8 != 0 {
        mask |= 0xf;
    }
    let mut cars = BASE_CARS.to_vec();
    for (bit, &car) in BONUS_CARS.iter().enumerate() {
        if mask & (1 << bit) != 0 {
            cars.push(car);
        }
    }
    cars
}

/// The number of countries a rally or time trial can choose from
/// (`DAT_00509408`).
pub fn country_count(record: &Record, kind: SetupKind) -> u8 {
    let unlocked = record[0x2b] & 0xe;
    match kind {
        SetupKind::Rally => 8 + if unlocked > 8 { 2 } else { 0 },
        SetupKind::TimeTrial if unlocked == 0 => 8,
        SetupKind::TimeTrial => {
            if record[0x2f] & 0xf0 != 0 {
                10
            } else {
                9
            }
        }
        SetupKind::Championship => 0,
    }
}

fn value_item(text_id: i32, key: i32, count: i32, wrap: bool, draw: u32) -> MenuItem {
    let mut item = MenuItem::new(ItemKind::Value, text_id, draw);
    item.key = key;
    item.value = ValueWidget {
        value: 0,
        value_count: count,
        wrap,
    };
    item
}

/// One setup screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetupScreen {
    pub kind: SetupKind,
    pub menu: Menu,
    /// The car list the CAR items index (`DAT_00508ec0`).
    pub cars: Vec<u8>,
}

impl SetupScreen {
    /// The screen as `FUN_004be540` builds it. The value items' labels are
    /// executable strings (`CHOOSE_CAR` ...), shown as text id `-1`; the draw
    /// ids are the native item draw callbacks. `continue_to` is the name-entry
    /// screen.
    pub fn build(kind: SetupKind, parent: MenuId, continue_to: MenuId) -> Self {
        let mut menu = Menu::new(kind.title_id(), Some(parent), Some(0x00d9_0060));
        let continue_item = {
            let mut item = MenuItem::new(ItemKind::Submenu(continue_to), 0x18, 0x004c_9700);
            item.key = key::CONTINUE;
            item
        };
        menu.items = match kind {
            SetupKind::Championship => vec![
                value_item(-1, key::CAR_1, 0xe, false, 0x004c_56e0),
                value_item(-1, key::GEAR_1, 2, true, 0x004c_5840),
                continue_item,
            ],
            SetupKind::Rally | SetupKind::TimeTrial => vec![
                value_item(-1, key::PLAYERS, 6, true, 0x004c_9480),
                value_item(-1, key::CAR_1, 0xe, false, 0x004c_56e0),
                value_item(-1, key::GEAR_1, 2, true, 0x004c_5840),
                value_item(-1, key::CAR_2, 0xe, false, 0x004c_5780),
                value_item(-1, key::GEAR_2, 2, true, 0x004c_58c0),
                value_item(
                    -1,
                    key::COUNTRY,
                    if kind == SetupKind::TimeTrial { 10 } else { 8 },
                    false,
                    0x004c_45f0,
                ),
                continue_item,
            ],
        };
        // `aea60`: the Continue item is selected first.
        menu.forced_selection = Some(menu.items.len() - 1);
        Self {
            kind,
            menu,
            cars: Vec::new(),
        }
    }

    fn item_mut(&mut self, key: i32) -> Option<&mut MenuItem> {
        let index = self.menu.find(key)?;
        self.menu.items.get_mut(index)
    }

    fn value(&self, key: i32) -> i32 {
        self.menu
            .find(key)
            .map_or(-1, |index| self.menu.items[index].value.value)
    }

    fn set(&mut self, key: i32, value: i32, count: Option<i32>) {
        if let Some(item) = self.item_mut(key) {
            if let Some(count) = count {
                item.value.value_count = count;
            }
            item.value.value = value;
        }
    }

    /// The enter callback: write the mode, then `FUN_004c8c20` loads the
    /// players, cars, gearboxes and country, and the navigation waits for the
    /// slide-in. A chosen car that is not allowed moves to the first allowed.
    pub fn enter(&mut self, record: &mut Record) {
        record[0x180] = (record[0x180] & !3) | self.kind.mode();
        if self.kind == SetupKind::Championship {
            record[0x181] &= 0x0f;
        }
        self.cars = car_list(record);
        let count = self.cars.len() as i32;
        let index_of =
            |car: u8, cars: &[u8]| cars.iter().position(|&c| c == car).unwrap_or(0) as i32;
        let allowed_index = |index: i32, cars: &[u8]| {
            if cars
                .get(index as usize)
                .is_some_and(|&car| car_allowed(record, car))
            {
                index
            } else {
                cars.iter()
                    .position(|&car| car_allowed(record, car))
                    .map_or(index, |first| first as i32)
            }
        };
        self.set(key::PLAYERS, i32::from(record[0x182] & 7), None);
        let car_1 = allowed_index(index_of(record[0xbe] & 0xf, &self.cars), &self.cars);
        let car_2 = allowed_index(index_of(record[0x15a] & 0xf, &self.cars), &self.cars);
        self.set(key::CAR_1, car_1, Some(count));
        self.set(key::GEAR_1, i32::from((record[0xbe] >> 4) & 1), None);
        self.set(key::CAR_2, car_2, Some(count));
        self.set(key::GEAR_2, i32::from((record[0x15a] >> 4) & 1), None);
        let country = i32::from(record[0x181] & 0xf);
        let build_count = self
            .item_mut(key::COUNTRY)
            .map_or(0, |item| item.value.value_count);
        if self.kind != SetupKind::TimeTrial && build_count == 10 {
            self.set(key::COUNTRY, if country < 9 { country } else { 0 }, Some(9));
        } else {
            let count = i32::from(country_count(record, self.kind));
            self.set(
                key::COUNTRY,
                if country < count { country } else { 0 },
                Some(count),
            );
        }
        self.menu.nav = MenuNav::NONE;
    }

    /// The commit's slide-in check: once the slide finishes the navigation
    /// comes back.
    pub fn slide_finished(&mut self) {
        self.menu.nav = MenuNav::ALL;
    }

    /// Show or hide the second player's items by the players value, as the
    /// PLAYERS draw callback `0x4c9480` does each frame.
    pub fn apply_players(&mut self) {
        let players = self.value(key::PLAYERS);
        for (key, show, enable) in [
            (key::CAR_2, players <= 3, (1..=3).contains(&players)),
            (key::GEAR_2, players <= 3, (1..=3).contains(&players)),
            (key::COUNTRY, players <= 3, players <= 3),
        ] {
            if let Some(item) = self.item_mut(key) {
                item.visible = show;
                item.enabled = enable;
            }
        }
    }

    /// The Continue item's enable (`0x4c9700` reads the `DAT_0050941c` AND
    /// the car and country draws build): every enabled car and country
    /// passes.
    pub fn continue_enabled(&self, record: &Record) -> bool {
        let time_trial = self.kind == SetupKind::TimeTrial;
        let mut ok = true;
        for item in &self.menu.items {
            if !item.enabled {
                continue;
            }
            let value = item.value.value;
            ok &= match item.key {
                key::CAR_1 | key::CAR_2 => self
                    .cars
                    .get(value as usize)
                    .is_some_and(|&car| car_allowed(record, car)),
                key::COUNTRY => country_allowed(record, value as u8, time_trial),
                _ => true,
            };
        }
        ok
    }

    /// The post-input callback `0x4c90c0`, run when the step leaves the
    /// screen (Continue or back): write the selections into the record.
    pub fn post(&self, record: &mut Record) {
        let car = |key| {
            self.cars
                .get(self.value(key).max(0) as usize)
                .copied()
                .unwrap_or(0)
        };
        let gear = |key| (self.value(key) & 1) as u8;
        if record[0x180] & 3 == 0 {
            record[0x182] &= 0xf8;
            record[0xbe] = (record[0xbe] & 0xf0) | (car(key::CAR_1) & 0xf);
            record[0xbe] = (record[0xbe] & 0xef) | (gear(key::GEAR_1) << 4);
            record[0x181] &= 0xf0;
        } else {
            let players = self.value(key::PLAYERS);
            if (0..=5).contains(&players) {
                record[0x182] = (record[0x182] & 0xf8) | players as u8;
            }
            record[0xbe] = (record[0xbe] & 0xf0) | (car(key::CAR_1) & 0xf);
            record[0xbe] = (record[0xbe] & 0xef) | (gear(key::GEAR_1) << 4);
            record[0x15a] = (record[0x15a] & 0xf0) | (car(key::CAR_2) & 0xf);
            record[0x15a] = (record[0x15a] & 0xef) | (gear(key::GEAR_2) << 4);
            record[0x181] = (record[0x181] & 0xf0) | (self.value(key::COUNTRY) as u8 & 0xf);
        }
        let country = record[0x181] & 0xf;
        record[0x182] = (record[0x182] & 0xef) | (u8::from(matches!(country, 8 | 9)) << 4);
        record[0x182] = (record[0x182] & 0xf7) | (u8::from(country == 9) << 3);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> Box<Record> {
        let mut record = Box::new([0; LEGACY_CONFIG_SIZE]);
        crate::config_commit::apply_defaults(
            &mut record,
            crate::config_commit::DefaultsHost {
                token: 0,
                language: 0,
                large_memory: false,
            },
            None,
        );
        record
    }

    #[test]
    fn the_defaults_allow_the_easy_cars_and_the_first_country() {
        let record = record();
        // The default difficulty is 1 (`record[0x20]` bit 3).
        assert_eq!(difficulty(&record), 1);
        assert!(car_allowed(&record, 0) && car_allowed(&record, 6));
        assert!(!car_allowed(&record, 1), "needs the 0x2b unlock");
        assert!(!car_allowed(&record, 3), "time trial only");
        assert!(country_allowed(&record, 0, false));
        assert!(!country_allowed(&record, 1, false));
        assert!(country_allowed(&record, 3, true));
        assert!(!country_allowed(&record, 4, true));
        assert_eq!(car_list(&record), BASE_CARS.to_vec());
    }

    #[test]
    fn unlocks_extend_the_cars_and_countries() {
        let mut record = record();
        record[0x2c] = 0x05;
        assert_eq!(car_list(&record)[8..], [5, 8]);
        record[0x30] = 8;
        assert_eq!(car_list(&record)[8..], [5, 4, 8, 3]);
        record[0x2a] = 0x31;
        assert!(country_allowed(&record, 2, false));
        record[0x2b] = 0x0a;
        assert!(country_allowed(&record, 8, false));
        assert!(country_allowed(&record, 9, true));
        assert_eq!(country_count(&record, SetupKind::Rally), 10);
        assert_eq!(country_count(&record, SetupKind::TimeTrial), 9);
    }

    #[test]
    fn a_rally_setup_reads_and_writes_the_record() {
        let mut record = record();
        record[0xbe] = 0x16; // car 6, manual
        record[0x181] = 0x03;
        let mut screen = SetupScreen::build(SetupKind::Rally, MenuId(0), MenuId(2));
        screen.enter(&mut record);
        assert_eq!(record[0x180] & 3, 1);
        assert_eq!(screen.menu.nav, MenuNav::NONE);
        assert_eq!(screen.value(key::CAR_1), 1, "car 6 is second in the list");
        assert_eq!(screen.value(key::GEAR_1), 1);
        assert_eq!(screen.value(key::COUNTRY), 3);
        // Country 3 is locked for a rally by default, so Continue is off.
        assert!(!screen.continue_enabled(&record));
        screen.set(key::COUNTRY, 0, None);
        assert!(screen.continue_enabled(&record));
        screen.set(key::CAR_1, 3, None); // car 9
        screen.post(&mut record);
        assert_eq!(record[0xbe], 0x19);
        assert_eq!(record[0x181] & 0xf, 0);
    }

    #[test]
    fn a_championship_setup_forces_one_player_and_the_first_country() {
        let mut record = record();
        record[0x181] = 0x25;
        record[0x182] = 0x03;
        record[0xbe] = 0x01; // car 1 is not allowed by default
        let mut screen = SetupScreen::build(SetupKind::Championship, MenuId(0), MenuId(1));
        screen.enter(&mut record);
        assert_eq!(record[0x181], 0x05);
        assert_eq!(
            screen.value(key::CAR_1),
            0,
            "moves to the first allowed car"
        );
        screen.post(&mut record);
        assert_eq!((record[0x182] & 7, record[0x181]), (0, 0));
        assert_eq!(screen.menu.selected, 0);
        assert_eq!(screen.menu.forced_selection, Some(2));
    }

    #[test]
    fn the_players_value_shows_and_hides_the_second_car() {
        let mut screen = SetupScreen::build(SetupKind::TimeTrial, MenuId(0), MenuId(1));
        screen.apply_players();
        let car_2 = screen.menu.find(key::CAR_2).unwrap();
        assert!(screen.menu.items[car_2].visible && !screen.menu.items[car_2].enabled);
        screen.set(key::PLAYERS, 2, None);
        screen.apply_players();
        assert!(screen.menu.items[car_2].enabled);
        screen.set(key::PLAYERS, 4, None);
        screen.apply_players();
        let country = screen.menu.find(key::COUNTRY).unwrap();
        assert!(!screen.menu.items[country].visible && !screen.menu.items[country].enabled);
    }
}
