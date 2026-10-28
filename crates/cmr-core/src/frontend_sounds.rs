//! The front end's own sound bank and its cue callbacks.
//!
//! The loader `FUN_004c9a60` sets the master `DAT_00509424` to
//! `20 * (record[0xd] & 0x7f)` (`FUN_004c9860`), allocates the base slot
//! `DAT_00d90080` and three channels (the first is `DAT_00d90084`), and loads
//! 19 waves, each the install root, the prefix `0x4f6650` and its name, at
//! 22050 Hz with flag `0x10`. It then fills the menus' sound tables:
//! the root's `0xd90070` (move `0x4c9d10`, enter `0x4c9c90`), the setups'
//! `0xd90060` (move `0x4c9870`, value `0x4c98e0`, enter `0x4c9c90`, back
//! `0x4c9cd0`) and the name entries' and stage select's `0xd90050` (move and
//! value `0x4c9870`, enter `0x4c98e0`, back `0x4c9cd0`). Each cue, when its
//! success byte is set, stops the channel and plays base slot + k one-shot
//! on it (`0x4c9d10` k = 0, `0x4c9870` 1, `0x4c98e0` 2, `0x4c9c90` 3,
//! `0x4c9cd0` 4); either way it then sets the channel's volume to
//! `0x7fff * master / 100` (`FUN_004c98b0`). `FUN_004c9a20` (k = 5) sounds
//! as the name entries and the stage select slide in and out. A description
//! of disassembly, not original source. Evidence:
//! `rea.official-eu.frontend-sounds`.

use crate::audio_mixer::{ChannelId, MixerCommand, PLAY_ONE_SHOT, WaveSlotId};
use crate::descriptor_menu::MenuSound;

/// The root's sound table.
pub const ROOT_SOUNDS: u32 = 0x00d9_0070;
/// The setups' sound table.
pub const SETUP_SOUNDS: u32 = 0x00d9_0060;
/// The name entries' and stage select's sound table.
pub const ENTRY_SOUNDS: u32 = 0x00d9_0050;
/// `FUN_004c9a20`'s slot.
pub const SLIDE_CUE: u32 = 5;
/// The bank's waves.
pub const WAVE_COUNT: usize = 19;
/// The waves' rate.
pub const WAVE_RATE_HZ: u32 = 22_050;

/// The slot a table's callback plays, or `None` for a null callback.
pub const fn cue_slot(table: u32, sound: MenuSound) -> Option<u32> {
    match (table, sound) {
        (ROOT_SOUNDS, MenuSound::Move) => Some(0),
        (ROOT_SOUNDS, MenuSound::Enter) => Some(3),
        (SETUP_SOUNDS, MenuSound::Move)
        | (ENTRY_SOUNDS, MenuSound::Move | MenuSound::ValueChange) => Some(1),
        (SETUP_SOUNDS, MenuSound::ValueChange) | (ENTRY_SOUNDS, MenuSound::Enter) => Some(2),
        (SETUP_SOUNDS, MenuSound::Enter) => Some(3),
        (SETUP_SOUNDS | ENTRY_SOUNDS, MenuSound::Back) => Some(4),
        _ => None,
    }
}

/// `FUN_004c9860(20 * (record[0xd] & 0x7f))`: the master volume percent.
pub const fn master_percent(record_0d: u8) -> i32 {
    20 * (record_0d & 0x7f) as i32
}

/// The bank: the channel `DAT_00d90084` and the base slot `DAT_00d90080`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrontendBank {
    pub channel: ChannelId,
    pub base: WaveSlotId,
}

/// One cue: the stop and one-shot play when `ok`, then the volume.
pub fn cue(bank: FrontendBank, slot: u32, ok: bool, master: i32) -> Vec<MixerCommand> {
    let mut commands = Vec::new();
    if ok {
        commands.push(MixerCommand::StopChannel {
            channel: bank.channel,
        });
        commands.push(MixerCommand::Play {
            slot: bank.base.with_offset(slot),
            channel: bank.channel,
            flags: PLAY_ONE_SHOT,
        });
    }
    commands.push(MixerCommand::SetVolume {
        channel: bank.channel,
        volume: 0x7fff_i32.wrapping_mul(master) / 100,
    });
    commands
}

/// The bank's channel `k` (`FUN_004c9930(k)`), when in range.
fn channel(bank: FrontendBank, k: u32) -> Option<ChannelId> {
    ChannelId::new(bank.channel.index() + k).ok()
}

/// `FUN_004c98b0(channel, volume)`: the volume scaled by the master.
fn volume(channel: ChannelId, volume: i32, master: i32) -> MixerCommand {
    MixerCommand::SetVolume {
        channel,
        volume: volume.wrapping_mul(master) / 100,
    }
}

fn play(bank: FrontendBank, slot: u32, channel: ChannelId) -> MixerCommand {
    MixerCommand::Play {
        slot: bank.base.with_offset(slot),
        channel,
        flags: PLAY_ONE_SHOT,
    }
}

/// `FUN_004c9940`, a cheat switched on: channels 2 and 1 stop, the
/// language's spoken line `speech` (`0x509430[img[0x20] & 7]`) plays on
/// channel 2 at full volume and CHEATON (17) on channel 1 at `0x2710`.
/// Both slots are raw ids in the native; they name these waves when the
/// bank is the first allocation (base 0), which this plays relative to the
/// bank (inference).
pub fn cheat_on_cue(bank: FrontendBank, speech: u32, master: i32) -> Vec<MixerCommand> {
    let (Some(one), Some(two)) = (channel(bank, 1), channel(bank, 2)) else {
        return Vec::new();
    };
    vec![
        MixerCommand::StopChannel { channel: two },
        MixerCommand::StopChannel { channel: one },
        play(bank, speech, two),
        volume(two, 0x7fff, master),
        play(bank, 0x11, one),
        volume(one, 0x2710, master),
    ]
}

/// `FUN_004c99c0`, a cheat switched off: CHEATOFF (18, a raw id like the
/// cheat-on slots) on channel 1.
pub fn cheat_off_cue(bank: FrontendBank, master: i32) -> Vec<MixerCommand> {
    let Some(one) = channel(bank, 1) else {
        return Vec::new();
    };
    vec![play(bank, 0x12, one), volume(one, 0x7fff, master)]
}

/// `FUN_004c99f0`, a valid CAC code: every mixer channel stops
/// (`FUN_00495b30`, the music's too), then COMPO (bank + 16) plays on the
/// bank's channel.
pub fn code_cue(bank: FrontendBank, master: i32) -> Vec<MixerCommand> {
    let mut commands: Vec<MixerCommand> = (0..crate::audio_mixer::MIXER_CHANNEL_CAPACITY)
        .filter_map(|index| ChannelId::new(index).ok())
        .map(|channel| MixerCommand::StopChannel { channel })
        .collect();
    commands.push(play(bank, 0x10, bank.channel));
    commands.push(volume(bank.channel, 0x7fff, master));
    commands
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tables_map_their_callbacks_to_slots() {
        assert_eq!(cue_slot(ROOT_SOUNDS, MenuSound::Move), Some(0));
        assert_eq!(cue_slot(ROOT_SOUNDS, MenuSound::Back), None);
        assert_eq!(cue_slot(SETUP_SOUNDS, MenuSound::ValueChange), Some(2));
        assert_eq!(cue_slot(SETUP_SOUNDS, MenuSound::Enter), Some(3));
        assert_eq!(cue_slot(ENTRY_SOUNDS, MenuSound::ValueChange), Some(1));
        assert_eq!(cue_slot(ENTRY_SOUNDS, MenuSound::Enter), Some(2));
        assert_eq!(cue_slot(ENTRY_SOUNDS, MenuSound::Back), Some(4));
        assert_eq!(cue_slot(0x57e698, MenuSound::Move), None);
    }

    #[test]
    fn a_cue_plays_then_always_sets_the_volume() {
        let bank = FrontendBank {
            channel: ChannelId::new(3).unwrap(),
            base: WaveSlotId::from_ordinal(10),
        };
        let played = cue(bank, 2, true, master_percent(5));
        assert_eq!(played.len(), 3);
        assert!(
            matches!(played[1], MixerCommand::Play { slot, .. } if slot == WaveSlotId::from_ordinal(12))
        );
        assert_eq!(
            played[2],
            MixerCommand::SetVolume {
                channel: bank.channel,
                volume: 0x7fff
            }
        );
        let blocked = cue(bank, 2, false, master_percent(2));
        assert_eq!(
            blocked,
            vec![MixerCommand::SetVolume {
                channel: bank.channel,
                volume: 0x7fff * 40 / 100
            }]
        );
    }

    #[test]
    fn the_cheat_and_code_cues() {
        let bank = FrontendBank {
            channel: ChannelId::new(21).unwrap(),
            base: WaveSlotId::from_ordinal(40),
        };
        let on = cheat_on_cue(bank, 6, 50);
        assert_eq!(on.len(), 6);
        assert!(matches!(on[2], MixerCommand::Play { slot, channel, .. }
            if slot == WaveSlotId::from_ordinal(46) && channel.index() == 23));
        assert_eq!(
            on[5],
            MixerCommand::SetVolume {
                channel: ChannelId::new(22).unwrap(),
                volume: 5000
            }
        );
        let off = cheat_off_cue(bank, 100);
        assert!(
            matches!(off[0], MixerCommand::Play { slot, .. } if slot == WaveSlotId::from_ordinal(58))
        );
        let code = code_cue(bank, 100);
        assert_eq!(
            code.len(),
            crate::audio_mixer::MIXER_CHANNEL_CAPACITY as usize + 2
        );
        assert!(matches!(code[30], MixerCommand::Play { slot, channel, .. }
            if slot == WaveSlotId::from_ordinal(56) && channel.index() == 21));
    }
}
