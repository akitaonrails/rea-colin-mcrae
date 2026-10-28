//! The name-entry screens (`0xd45cf8` championship, `0xd47d90` rally,
//! `0xd46fb0` time trial): menus with no items whose commit callback runs
//! the text-entry key handler `FUN_004c64a0` once both slide-ins have
//! finished.
//!
//! The enter callbacks (`0x4c59d0`/`0x4c5ab0`/`0x4c5c70`) copy the player's
//! stored name (`record + 0x34 + 0x9c p`) into the 12-byte edit buffer
//! `0xd8cebc`, arm the clear-on-first-key flag `DAT_00d8cec8`, and choose the
//! back and forward screens by the players value (`record[0x182] & 7`) and
//! the player index `DAT_00d8cee8`. The key handler appends space, `A`-`Z`,
//! `0`-`9` and the `.` key while fewer than 11 characters are typed, deletes
//! the last one on Delete (VK `0x2e`) or Backspace, goes back on Escape (or
//! pad bit `0x20`), and on Return (or pad bit `0x10`) stores the name and its
//! initials and either moves on to the second player's entry or commits the
//! record (`FUN_004b93f0`) and goes forward. Input is taken only after a
//! frame with no key (`DAT_00d8cef0`), so the Return that opened the screen
//! is ignored.
//!
//! Not reconstructed: the cheat names `FUN_004c6880` (obfuscated strings in
//! the executable) and the `CAC` record codes `FUN_004c67a0` (checksum
//! `FUN_004ba510`); a typed code is stored as a name. A description of
//! disassembly, not original source. Evidence:
//! `rea.official-eu.frontend-name-entry`.

use crate::config::LEGACY_CONFIG_SIZE;
use crate::frontend::{
    CHAMPIONSHIP_NEXT_SCREEN_ID, CHAMPIONSHIP_SETUP_SCREEN_ID, NETWORK_SCREEN_ID,
    RACE_SENTINEL_SCREEN_ID, RALLY_NAME_SCREEN_ID, RALLY_SCREEN_ID, STAGE_SELECT_SCREEN_ID,
    ScreenId, TIME_TRIAL_NAME_SCREEN_ID, TIME_TRIAL_SCREEN_ID,
};

type Record = [u8; LEGACY_CONFIG_SIZE];

/// The edit buffer's size (`0xd8cebc`, 12 bytes with the NUL).
pub const NAME_BUFFER: usize = 12;
/// `VkKeyScanA('.') & 0xff` on the US layout (`VK_OEM_PERIOD`).
pub const VK_PERIOD: u8 = 0xbe;

/// One step's input: the repeated key `DAT_00d4cd70` (a virtual-key code, 0
/// for none) and the pad bits (`0x10` confirm, `0x20` back).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NameKey {
    pub vk: u8,
    pub pad: u32,
}

/// Which name entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameScreen {
    Championship,
    Rally,
    TimeTrial,
}

impl NameScreen {
    pub const fn screen_id(self) -> ScreenId {
        match self {
            Self::Championship => CHAMPIONSHIP_NEXT_SCREEN_ID,
            Self::Rally => RALLY_NAME_SCREEN_ID,
            Self::TimeTrial => TIME_TRIAL_NAME_SCREEN_ID,
        }
    }

    fn setup(self) -> ScreenId {
        match self {
            Self::Championship => CHAMPIONSHIP_SETUP_SCREEN_ID,
            Self::Rally => RALLY_SCREEN_ID,
            Self::TimeTrial => TIME_TRIAL_SCREEN_ID,
        }
    }

    fn race(self) -> ScreenId {
        match self {
            Self::TimeTrial => STAGE_SELECT_SCREEN_ID,
            _ => RACE_SENTINEL_SCREEN_ID,
        }
    }
}

/// What a key step decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameOutcome {
    /// Keep editing.
    Editing,
    /// `FUN_004af3a0(target)`; `commit` when the record was committed
    /// (`FUN_004b93f0`) first.
    Leave { target: ScreenId, commit: bool },
}

/// The name-entry state, shared by the three screens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameEntry {
    /// `0xd8cebc`.
    buffer: [u8; NAME_BUFFER],
    /// `DAT_00d8cec8`: the first key clears the buffer.
    clear_on_key: bool,
    /// `DAT_00d8cef0`: a frame without a key has been seen.
    armed: bool,
    /// `DAT_00d8cee4`: the player being named.
    pub player: usize,
    /// `DAT_00d8cee8`: the player the next entry names.
    pub next_player: usize,
    /// `DAT_00d8ced4`, `DAT_00d8cecc`.
    back: ScreenId,
    forward: ScreenId,
}

impl Default for NameEntry {
    fn default() -> Self {
        Self {
            buffer: [0; NAME_BUFFER],
            clear_on_key: false,
            armed: false,
            player: 0,
            next_player: 0,
            back: CHAMPIONSHIP_SETUP_SCREEN_ID,
            forward: RACE_SENTINEL_SCREEN_ID,
        }
    }
}

impl NameEntry {
    /// The typed name (the buffer up to its NUL).
    pub fn name(&self) -> &[u8] {
        let length = self
            .buffer
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(NAME_BUFFER);
        &self.buffer[..length]
    }

    /// The enter callback: name player `DAT_00d8cee8` from the record and
    /// choose the targets. Championship: back to its setup, forward to the
    /// race. Rally and time trial by players: 0 one player (forward to the
    /// race, or the stage select); 4/5 network (forward to the network
    /// screen); otherwise player 1 re-enters this screen and player 2 goes on.
    pub fn enter(&mut self, screen: NameScreen, record: &Record) {
        self.player = self.next_player;
        let base = 0x34 + 0x9c * self.player;
        let stored = &record[base..base + NAME_BUFFER];
        let length = stored
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(NAME_BUFFER - 1);
        self.buffer = [0; NAME_BUFFER];
        self.buffer[..length.min(NAME_BUFFER - 1)]
            .copy_from_slice(&stored[..length.min(NAME_BUFFER - 1)]);
        self.clear_on_key = true;
        let own = screen.screen_id();
        (self.back, self.forward) = match screen {
            NameScreen::Championship => (screen.setup(), RACE_SENTINEL_SCREEN_ID),
            _ => match record[0x182] & 7 {
                0 => (screen.setup(), screen.race()),
                4 | 5 => (screen.setup(), NETWORK_SCREEN_ID),
                _ if self.player == 0 => (screen.setup(), own),
                _ => (own, screen.race()),
            },
        };
    }

    /// `FUN_004c6450`: append while fewer than 11 characters are typed.
    fn append(&mut self, byte: u8) {
        let length = self.name().len();
        if length < NAME_BUFFER - 1 {
            self.buffer[length] = byte;
            for slot in &mut self.buffer[length + 1..] {
                *slot = 0;
            }
        }
    }

    /// `FUN_004c64a0`: one step of the key handler.
    pub fn key(&mut self, key: NameKey, record: &mut Record) -> NameOutcome {
        if !self.armed && key.vk != 0 {
            return NameOutcome::Editing;
        }
        self.armed = true;
        let vk = key.vk;
        if vk != 0x1b && vk != 0x0d && vk != 0 && self.clear_on_key {
            self.clear_on_key = false;
            self.buffer = [0; NAME_BUFFER];
        }
        if vk == b' ' || vk.is_ascii_uppercase() || vk.is_ascii_digit() {
            self.append(vk);
        }
        if vk == VK_PERIOD {
            self.append(b'.');
        }
        if vk == 0x2e || vk == 8 {
            let length = self.name().len();
            if length > 0 {
                self.buffer[length - 1] = 0;
            }
        }
        let mut outcome = NameOutcome::Editing;
        if vk == 0x1b || key.pad & 0x20 != 0 {
            self.armed = false;
            self.next_player = 0;
            outcome = NameOutcome::Leave {
                target: self.back,
                commit: false,
            };
        }
        if vk == 0x0d || key.pad & 0x10 != 0 {
            self.armed = false;
            outcome = self.store(record);
        }
        outcome
    }

    /// Return: the name and its initials go into the player's block, then
    /// either the second player's entry or the commit and the forward screen.
    fn store(&mut self, record: &mut Record) -> NameOutcome {
        let base = 0x34 + 0x9c * self.player;
        let length = self.name().len();
        record[base..base + length + 1].copy_from_slice(&self.buffer[..length + 1]);
        record[base + 0xc..base + 0xf].copy_from_slice(&self.buffer[..3]);
        record[base + 0xf] = 0;
        let players = record[0x182] & 7;
        let another = (!matches!(players, 0 | 4 | 5) || self.player != 0)
            && (players == 0 || self.player != 1);
        self.next_player = usize::from(another);
        NameOutcome::Leave {
            target: self.forward,
            commit: !another,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(name: &[u8], players: u8) -> Box<Record> {
        let mut record = Box::new([0u8; LEGACY_CONFIG_SIZE]);
        record[0x34..0x34 + name.len()].copy_from_slice(name);
        record[0x182] = players;
        record
    }

    fn press(entry: &mut NameEntry, record: &mut Record, vk: u8) -> NameOutcome {
        entry.key(NameKey { vk: 0, pad: 0 }, record);
        entry.key(NameKey { vk, pad: 0 }, record)
    }

    #[test]
    fn typing_replaces_the_stored_name_and_return_commits() {
        let mut record = record(b"COLIN", 0);
        let mut entry = NameEntry::default();
        entry.enter(NameScreen::Championship, &record);
        assert_eq!(entry.name(), b"COLIN");
        // The Return still held from the setup is ignored until a free frame.
        assert_eq!(
            entry.key(NameKey { vk: 0x0d, pad: 0 }, &mut record),
            NameOutcome::Editing
        );
        // The first letter clears the old name.
        press(&mut entry, &mut record, b'A');
        press(&mut entry, &mut record, b'B');
        press(&mut entry, &mut record, VK_PERIOD);
        press(&mut entry, &mut record, b'7');
        press(&mut entry, &mut record, 8);
        assert_eq!(entry.name(), b"AB.");
        let outcome = press(&mut entry, &mut record, 0x0d);
        assert_eq!(
            outcome,
            NameOutcome::Leave {
                target: RACE_SENTINEL_SCREEN_ID,
                commit: true
            }
        );
        assert_eq!(&record[0x34..0x38], b"AB.\0");
        assert_eq!(&record[0x40..0x44], b"AB.\0");
    }

    #[test]
    fn the_name_holds_eleven_characters_and_escape_goes_back() {
        let mut record = record(b"", 0);
        let mut entry = NameEntry::default();
        entry.enter(NameScreen::Rally, &record);
        for _ in 0..14 {
            press(&mut entry, &mut record, b'Z');
        }
        assert_eq!(entry.name().len(), 11);
        assert_eq!(
            press(&mut entry, &mut record, 0x1b),
            NameOutcome::Leave {
                target: RALLY_SCREEN_ID,
                commit: false
            }
        );
    }

    #[test]
    fn two_players_name_in_turn() {
        let mut record = record(b"ONE", 2);
        record[0x34 + 0x9c..0x34 + 0x9c + 3].copy_from_slice(b"TWO");
        let mut entry = NameEntry::default();
        entry.enter(NameScreen::TimeTrial, &record);
        assert_eq!(
            press(&mut entry, &mut record, 0x0d),
            NameOutcome::Leave {
                target: TIME_TRIAL_NAME_SCREEN_ID,
                commit: false
            }
        );
        entry.enter(NameScreen::TimeTrial, &record);
        assert_eq!((entry.player, entry.name()), (1, &b"TWO"[..]));
        assert_eq!(
            press(&mut entry, &mut record, 0x0d),
            NameOutcome::Leave {
                target: STAGE_SELECT_SCREEN_ID,
                commit: true
            }
        );
        // Player 2's back target is this screen (player 1's entry).
        let mut network = record.clone();
        network[0x182] = 4;
        let mut entry = NameEntry::default();
        entry.enter(NameScreen::Rally, &network);
        assert_eq!(
            press(&mut entry, &mut network, 0x0d),
            NameOutcome::Leave {
                target: NETWORK_SCREEN_ID,
                commit: true
            }
        );
    }
}
