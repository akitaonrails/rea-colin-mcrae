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
//! On Return the name is first matched against the 26 cheat names
//! (`FUN_004c6880`: each string de-obfuscated by `FUN_004baf00` and compared
//! exactly); a match toggles its record bits and the screen is entered again
//! with the cheat-on or cheat-off cue. Then a valid `CAC` competition code
//! (`FUN_004c67a0`: `"CAC"`, four digits and the check character
//! `FUN_004ba510`) is not stored as a name either: its number goes to
//! `record.u16[0x28]` and the screen is entered again with the code cue. The
//! encoded cheat strings come from the user's executable through the static
//! tables. A description of disassembly, not original source. Evidence:
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

/// The cue a code or cheat plays as the screen is entered again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameCue {
    /// `FUN_004c99f0`: a valid CAC code.
    Code,
    /// `FUN_004c9940`: a cheat switched on (`DAT_00d8cee0 != 0`).
    CheatOn,
    /// `FUN_004c99c0`: a cheat switched off.
    CheatOff,
}

/// The number of cheat names `FUN_004c6880` tests.
pub const CHEAT_COUNT: usize = 26;

/// `FUN_004baf00(enc)`: copy to the terminator, then XOR each byte with
/// `0x33` while the index is below the (re-measured) length, so an encoded
/// `0x33` ends the string there.
pub fn decode_string(encoded: &[u8]) -> Vec<u8> {
    let length = encoded
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(encoded.len());
    let mut out = encoded[..length].to_vec();
    let mut i = 0;
    while i < out.iter().position(|&b| b == 0).unwrap_or(out.len()) {
        out[i] ^= 0x33;
        i += 1;
    }
    out.truncate(out.iter().position(|&b| b == 0).unwrap_or(out.len()));
    out
}

/// One cheat's record write: the first sets fields; every other toggles one
/// bit (whose new value picks the cue) and, for two, a second bit after it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CheatEffect {
    /// `img[0x2b] = img[0x2b] & 0xfb | 0x0a`, `img[0x2a] = img[0x2a] & 0x0f
    /// | 0x80`, `img[0xc95]` `& 0xfe | 2` then `& 0xf3 | 0x10`; always on.
    Unlock,
    Toggle {
        at: usize,
        mask: u8,
        also: Option<(usize, u8)>,
    },
}

const fn toggle(at: usize, mask: u8) -> CheatEffect {
    CheatEffect::Toggle {
        at,
        mask,
        also: None,
    }
}

/// `FUN_004c6880`'s effects, in test order.
const CHEAT_EFFECTS: [CheatEffect; CHEAT_COUNT] = [
    CheatEffect::Unlock,
    toggle(0x30, 0x08),
    toggle(0x2c, 0x40),
    toggle(0x2c, 0x80),
    toggle(0x2d, 0x01),
    toggle(0x2d, 0x02),
    toggle(0x2d, 0x04),
    toggle(0x2d, 0x08),
    toggle(0x2d, 0x10),
    toggle(0x2d, 0x20),
    toggle(0x2d, 0x40),
    toggle(0x2d, 0x80),
    toggle(0x2e, 0x01),
    toggle(0x2e, 0x02),
    toggle(0x2e, 0x08),
    toggle(0x2e, 0x20),
    toggle(0x2e, 0x80),
    toggle(0x2f, 0x02),
    toggle(0x2f, 0x04),
    CheatEffect::Toggle {
        at: 0x2f,
        mask: 0x08,
        also: Some((0x2c, 0x10)),
    },
    CheatEffect::Toggle {
        at: 0x30,
        mask: 0x02,
        also: Some((0x2c, 0x20)),
    },
    toggle(0x2f, 0x10),
    toggle(0x2f, 0x20),
    toggle(0x2f, 0x40),
    toggle(0x2f, 0x80),
    toggle(0x30, 0x04),
];

/// `FUN_004c6880(name)`: the first cheat whose decoded name equals `name`
/// applies its effect; `Some(on)` with `DAT_00d8cee0`, or `None`.
pub fn apply_cheat(name: &[u8], encoded: &[Vec<u8>], record: &mut Record) -> Option<bool> {
    let index = encoded
        .iter()
        .take(CHEAT_COUNT)
        .position(|cheat| decode_string(cheat) == name)?;
    Some(match CHEAT_EFFECTS[index] {
        CheatEffect::Unlock => {
            record[0x2b] = (record[0x2b] & 0xfb) | 0x0a;
            record[0x2a] = (record[0x2a] & 0x0f) | 0x80;
            record[0xc95] = (record[0xc95] & 0xfe) | 0x02;
            record[0xc95] = (record[0xc95] & 0xf3) | 0x10;
            true
        }
        CheatEffect::Toggle { at, mask, also } => {
            record[at] ^= mask;
            let on = record[at] & mask != 0;
            if let Some((at, mask)) = also {
                record[at] ^= mask;
            }
            on
        }
    })
}

/// What a key step decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameOutcome {
    /// Keep editing.
    Editing,
    /// `FUN_004af3a0(target)`; `commit` when the record was committed
    /// (`FUN_004b93f0`) first.
    Leave { target: ScreenId, commit: bool },
    /// `FUN_004af3a0(menu)`: the typed name was a code; this screen is
    /// entered again (its stored name comes back) with `cue`.
    Again { cue: NameCue },
}

/// `FUN_004c67a0`: `"CAC"`, four digits and their check character
/// (`FUN_004ba510`), exactly eight bytes; the competition number.
pub fn cac_code(name: &[u8]) -> Option<u16> {
    let [b'C', b'A', b'C', digits @ .., check] = name else {
        return None;
    };
    if digits.len() != 4 || !digits.iter().chain([check]).all(u8::is_ascii_digit) {
        return None;
    }
    let number = digits
        .iter()
        .fold(0i32, |n, &d| n * 10 + i32::from(d - b'0'));
    (crate::hiscore_codes::cac_check(number) == *check).then_some(number as u16)
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
    /// The encoded cheat names (`FUN_004c6880`'s strings), in test order.
    cheats: Vec<Vec<u8>>,
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
            cheats: Vec::new(),
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

    /// The encoded cheat names from the user's executable.
    pub fn set_cheats(&mut self, cheats: Vec<Vec<u8>>) {
        self.cheats = cheats;
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

    /// Return: a cheat toggles its bits and a CAC code is kept as the
    /// competition number; otherwise the
    /// name and its initials go into the player's block, then either the
    /// second player's entry or the commit and the forward screen.
    fn store(&mut self, record: &mut Record) -> NameOutcome {
        let name = self.name().to_vec();
        if let Some(on) = apply_cheat(&name, &self.cheats, record) {
            return NameOutcome::Again {
                cue: if on {
                    NameCue::CheatOn
                } else {
                    NameCue::CheatOff
                },
            };
        }
        if let Some(number) = cac_code(self.name()) {
            record[0x28..0x2a].copy_from_slice(&number.to_le_bytes());
            return NameOutcome::Again { cue: NameCue::Code };
        }
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

    #[test]
    fn a_cac_code_sets_the_competition_and_keeps_the_name() {
        assert_eq!(cac_code(b"CAC12340"), Some(1234));
        assert_eq!(cac_code(b"CAC12341"), None, "wrong check");
        assert_eq!(cac_code(b"CAC1234"), None);
        assert_eq!(cac_code(b"CAC123400"), None);
        assert_eq!(cac_code(b"CAB12340"), None);
        // 3012's check is ';', which cannot be typed.
        assert_eq!(cac_code(b"CAC3012;"), None);
        let mut record = record(b"COLIN", 0);
        let mut entry = NameEntry::default();
        entry.enter(NameScreen::Championship, &record);
        for &key in b"CAC12340" {
            press(&mut entry, &mut record, key);
        }
        assert_eq!(
            press(&mut entry, &mut record, 0x0d),
            NameOutcome::Again { cue: NameCue::Code }
        );
        assert_eq!(u16::from_le_bytes([record[0x28], record[0x29]]), 1234);
        assert_eq!(&record[0x34..0x39], b"COLIN", "the name is not stored");
    }

    /// A synthetic encoded name (not one of the game's).
    fn encode(name: &[u8]) -> Vec<u8> {
        name.iter().map(|b| b ^ 0x33).chain([0]).collect()
    }

    #[test]
    fn the_decoder_stops_at_an_encoded_0x33() {
        assert_eq!(decode_string(&encode(b"ABC")), b"ABC");
        // 0x33 decodes to NUL, which ends the re-measured string.
        assert_eq!(decode_string(&[b'A' ^ 0x33, 0x33, b'C' ^ 0x33, 0]), b"A");
    }

    #[test]
    fn a_cheat_toggles_its_bits_and_keeps_the_name() {
        let mut cheats: Vec<Vec<u8>> = (0..CHEAT_COUNT)
            .map(|i| encode(format!("CHEAT{i}").as_bytes()))
            .collect();
        cheats[19] = encode(b"PAIRED");
        let mut record = record(b"COLIN", 0);
        // The unlock sets its fields and is always "on".
        assert_eq!(apply_cheat(b"CHEAT0", &cheats, &mut record), Some(true));
        assert_eq!(
            (record[0x2b], record[0x2a], record[0xc95]),
            (0x0a, 0x80, 0x12)
        );
        // A paired toggle flips its second bit after taking the first.
        assert_eq!(apply_cheat(b"PAIRED", &cheats, &mut record), Some(true));
        assert_eq!((record[0x2f], record[0x2c]), (0x08, 0x10));
        assert_eq!(apply_cheat(b"PAIRED", &cheats, &mut record), Some(false));
        assert_eq!((record[0x2f], record[0x2c]), (0, 0));
        assert_eq!(apply_cheat(b"NOPE", &cheats, &mut record), None);
        // Typed on the screen: the cue follows the new bit; the name is kept.
        let mut entry = NameEntry::default();
        entry.set_cheats(cheats);
        entry.enter(NameScreen::Rally, &record);
        for &key in b"CHEAT4" {
            press(&mut entry, &mut record, key);
        }
        assert_eq!(
            press(&mut entry, &mut record, 0x0d),
            NameOutcome::Again {
                cue: NameCue::CheatOn
            }
        );
        assert_eq!(record[0x2d], 0x01);
        assert_eq!(&record[0x34..0x39], b"COLIN");
    }
}
