//! Reconstruction of the frontend menu sound playback.
//!
//! The menu UI loader `FUN_00468f50` registers four waves on a dedicated menu
//! channel in a fixed order (`FORWARD`, `BACKWARD`, `LEFTRIGH`, `UPDOWN` - slot
//! offsets 0..3; see [`crate::audio_assets::MENU_WAVE_FILES`]). The menu play
//! helpers (`FUN_00467ef0`, `FUN_00467dd0`, and sibling functions of the same
//! shape) each, when their gate argument is set, perform the same three steps on
//! the menu channel: set full volume via `FUN_0047eb70(channel, 0x7fff)`, stop
//! the channel (`QSWaveMixStopChannel`), and play `base_slot + offset` one-shot
//! (`QSWaveMixPlayEx(..., 0)`).
//!
//! This module reproduces that mechanism as a pure command emitter over the
//! [`crate::audio_mixer`] contract. See evidence
//! `rea.official-eu.audio-menu-play`.
//!
//! Observations: the stop-then-play-one-shot-at-full-volume sequence, directly
//! from the two carved helpers playing slot offset 0 (`FORWARD`) and offset 3
//! (`UPDOWN`). Inferences: the slot order from the loader makes offset 1
//! `BACKWARD` and offset 2 `LEFTRIGH`; by filename, `FORWARD`=confirm,
//! `BACKWARD`=cancel, `UPDOWN`=vertical navigation, `LEFTRIGH`=horizontal/toggle.
//! Unknowns: which frontend action drives each helper for the two offsets whose
//! helper functions Ghidra did not carve (1 and 2), and whether a later UI uses
//! the separate `BACK/SELECT/...` event names.

use crate::audio_mixer::{
    ChannelId, MixerCommand, PLAY_ONE_SHOT, WaveSlotId, scale_volume_by_master_percent,
};
use crate::frontend::FrontendInput;

/// Full channel volume the menu play helpers set before each sound (`0x7fff`),
/// before the master-volume scale.
pub const MENU_VOLUME: i32 = 0x7fff;

/// A menu UI sound, by its recovered slot offset within the menu wave group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuSound {
    /// `FORWARD.WAV` (offset 0): confirm/enter.
    Forward,
    /// `BACKWARD.WAV` (offset 1): cancel/back.
    Backward,
    /// `LEFTRIGH.WAV` (offset 2): horizontal change / toggle.
    LeftRight,
    /// `UPDOWN.WAV` (offset 3): vertical navigation.
    UpDown,
}

impl MenuSound {
    /// The recovered slot offset added to the menu base slot.
    pub fn slot_offset(self) -> u32 {
        match self {
            MenuSound::Forward => 0,
            MenuSound::Backward => 1,
            MenuSound::LeftRight => 2,
            MenuSound::UpDown => 3,
        }
    }
}

/// The recovered menu channel and the base slot of its four-wave group
/// (`DAT_0057c608` and `DAT_0057d88c`).
#[derive(Debug, Clone, Copy)]
pub struct MenuChannel {
    pub channel: ChannelId,
    pub base_slot: WaveSlotId,
}

/// The menu UI sound a recovered front-end input edge plays, mapping a
/// [`FrontendInput`] to the [`MenuSound`] the matching play helper emits: a
/// `confirm` plays `FORWARD` (the carved helper `FUN_00467ef0`, offset 0), a
/// vertical move (`next`/`previous`) plays `UPDOWN` (the carved helper
/// `FUN_00467dd0`, offset 3), and a `cancel` plays `BACKWARD` (offset 1, by
/// filename - the helper Ghidra did not carve). `None` when no edge fired.
/// `confirm` takes precedence over `cancel` over a move (only one edge normally
/// fires per tick). `LEFTRIGH` (offset 2) is a horizontal-change sound the
/// vertical root menu never triggers, so it is not produced here.
#[must_use]
pub fn menu_sound_for_input(input: FrontendInput) -> Option<MenuSound> {
    if input.confirm {
        Some(MenuSound::Forward)
    } else if input.cancel {
        Some(MenuSound::Backward)
    } else if input.next || input.previous {
        Some(MenuSound::UpDown)
    } else {
        None
    }
}

/// Reproduce a menu play helper: set the full master-scaled volume, stop the
/// channel, then play the selected sound one-shot. The stop makes a new UI sound
/// interrupt the previous one on the shared menu channel.
pub fn play_menu_sound(
    menu: MenuChannel,
    sound: MenuSound,
    master_percent: i32,
) -> Vec<MixerCommand> {
    vec![
        MixerCommand::SetVolume {
            channel: menu.channel,
            volume: scale_volume_by_master_percent(master_percent, MENU_VOLUME),
        },
        MixerCommand::StopChannel {
            channel: menu.channel,
        },
        MixerCommand::Play {
            slot: menu.base_slot.with_offset(sound.slot_offset()),
            channel: menu.channel,
            flags: PLAY_ONE_SHOT,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu() -> MenuChannel {
        MenuChannel {
            channel: ChannelId::new(7).unwrap(),
            base_slot: WaveSlotId::from_ordinal(20),
        }
    }

    #[test]
    fn input_edges_map_to_the_recovered_menu_sounds() {
        let none = FrontendInput::default();
        assert_eq!(menu_sound_for_input(none), None);
        assert_eq!(
            menu_sound_for_input(FrontendInput {
                confirm: true,
                ..none
            }),
            Some(MenuSound::Forward)
        );
        assert_eq!(
            menu_sound_for_input(FrontendInput {
                cancel: true,
                ..none
            }),
            Some(MenuSound::Backward)
        );
        assert_eq!(
            menu_sound_for_input(FrontendInput { next: true, ..none }),
            Some(MenuSound::UpDown)
        );
        assert_eq!(
            menu_sound_for_input(FrontendInput {
                previous: true,
                ..none
            }),
            Some(MenuSound::UpDown)
        );
        // Confirm wins over a simultaneous cancel and move.
        assert_eq!(
            menu_sound_for_input(FrontendInput {
                confirm: true,
                cancel: true,
                next: true,
                previous: true,
            }),
            Some(MenuSound::Forward)
        );
        // Cancel wins over a move.
        assert_eq!(
            menu_sound_for_input(FrontendInput {
                cancel: true,
                next: true,
                ..none
            }),
            Some(MenuSound::Backward)
        );
    }

    #[test]
    fn slot_offsets_match_the_recovered_load_order() {
        assert_eq!(MenuSound::Forward.slot_offset(), 0);
        assert_eq!(MenuSound::Backward.slot_offset(), 1);
        assert_eq!(MenuSound::LeftRight.slot_offset(), 2);
        assert_eq!(MenuSound::UpDown.slot_offset(), 3);
    }

    #[test]
    fn forward_play_matches_the_recovered_helper_sequence() {
        // FUN_00467ef0: full volume, stop, play base+0 one-shot.
        let commands = play_menu_sound(menu(), MenuSound::Forward, 100);
        assert_eq!(
            commands,
            vec![
                MixerCommand::SetVolume {
                    channel: ChannelId::new(7).unwrap(),
                    volume: 0x7fff,
                },
                MixerCommand::StopChannel {
                    channel: ChannelId::new(7).unwrap(),
                },
                MixerCommand::Play {
                    slot: WaveSlotId::from_ordinal(20),
                    channel: ChannelId::new(7).unwrap(),
                    flags: PLAY_ONE_SHOT,
                },
            ]
        );
    }

    #[test]
    fn updown_plays_slot_offset_three() {
        // FUN_00467dd0: play base+3 one-shot.
        let commands = play_menu_sound(menu(), MenuSound::UpDown, 100);
        assert_eq!(
            commands[2],
            MixerCommand::Play {
                slot: WaveSlotId::from_ordinal(23),
                channel: ChannelId::new(7).unwrap(),
                flags: PLAY_ONE_SHOT,
            }
        );
    }

    #[test]
    fn volume_is_master_scaled() {
        let commands = play_menu_sound(menu(), MenuSound::Backward, 50);
        assert_eq!(
            commands[0],
            MixerCommand::SetVolume {
                channel: ChannelId::new(7).unwrap(),
                volume: 0x7fff / 2,
            }
        );
    }
}
