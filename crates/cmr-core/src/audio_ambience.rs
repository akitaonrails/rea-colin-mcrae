//! Reconstruction of the crowd/weather ambience triggers.
//!
//! This translates the recovered ambience trigger ticks into pure functions that
//! emit the exact [`MixerCommand`] sequence each one issues, driving the shared
//! ambience channel through the [`crate::audio_mixer`] contract:
//!
//! - [`AmbienceController::abduct_tick`] reproduces `FUN_0041d270`.
//! - [`AmbienceController::crowd_or_storm_tick`] reproduces `FUN_0041cfe0`.
//!
//! The recovered control flow - the channel-busy checks, the stop/set-volume/play
//! ordering, the loop-vs-one-shot flags, and the three-flag arbitration (only the
//! crowd voice loops, and it is suppressed while a one-shot abduct or storm voice
//! holds the channel) - is reproduced faithfully. The numeric values the original
//! computes outside this logic (the crowd proximity volume and its curve, the
//! thunder selector/volume from `FUN_0048d080`, and the 0..100 master-volume
//! percentage `DAT_004f653c`) are supplied by the caller rather than invented
//! here; see evidence `rea.official-eu.audio-ambience-trigger` and
//! `rea.official-eu.audio-abduct-trigger`.

use crate::audio_mixer::{
    ChannelId, MixerCommand, PLAY_LOOPING, PLAY_ONE_SHOT, WaveSlotId,
    scale_volume_by_master_percent,
};
use crate::audio_weather_assets::ABDUCT_VOLUME;

/// The shared ambience channel and the base slot of its wave group, as recovered
/// (`DAT_0050d85c` and `DAT_0050d858`). Slot offsets: crowd `+0`, abduct `+1`,
/// thunder variants `+2..5`.
#[derive(Debug, Clone, Copy)]
pub struct AmbienceChannel {
    pub channel: ChannelId,
    pub base_slot: WaveSlotId,
}

impl AmbienceChannel {
    fn crowd_slot(&self) -> WaveSlotId {
        self.base_slot
    }

    fn abduct_slot(&self) -> WaveSlotId {
        self.base_slot.with_offset(1)
    }

    fn thunder_slot(&self, selector: u8) -> WaveSlotId {
        self.base_slot.with_offset(2 + selector as u32)
    }
}

/// The recovered ambience arbitration flags: `DAT_0050d864` (crowd playing),
/// `DAT_0050d860` (abduct active), `DAT_0050d868` (storm active).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AmbienceController {
    crowd_playing: bool,
    abduct_active: bool,
    storm_active: bool,
}

/// The ambience cue for a crowd-or-storm tick, matching the recovered branch on
/// the storm predicate `FUN_0048d070`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmbienceCue {
    /// No storm this tick. `Some(volume)` when at least one crowd object is in
    /// range (the volume is already through the recovered proximity/curve
    /// pipeline); `None` when none are in range.
    Crowd(Option<i32>),
    /// A storm is active: the thunder selector and volume from `FUN_0048d080`.
    Storm { selector: u8, volume: i32 },
}

impl AmbienceController {
    /// A fresh controller with every ambience voice idle.
    pub fn new() -> Self {
        Self::default()
    }

    pub fn crowd_playing(&self) -> bool {
        self.crowd_playing
    }
    pub fn abduct_active(&self) -> bool {
        self.abduct_active
    }
    pub fn storm_active(&self) -> bool {
        self.storm_active
    }

    /// Reproduce the abduct trigger `FUN_0041d270`.
    ///
    /// When abduct is idle it (stops any current voice, then) sets the channel to
    /// the master-scaled [`ABDUCT_VOLUME`] and plays the abduct slot one-shot,
    /// marking abduct active. When already active it clears the flag once the
    /// channel has finished. `channel_busy` is the recovered `0x495d10` result.
    pub fn abduct_tick(
        &mut self,
        ch: AmbienceChannel,
        channel_busy: bool,
        master_percent: i32,
    ) -> Vec<MixerCommand> {
        let mut commands = Vec::new();
        if !self.abduct_active {
            if channel_busy {
                commands.push(MixerCommand::StopChannel {
                    channel: ch.channel,
                });
            }
            commands.push(MixerCommand::SetVolume {
                channel: ch.channel,
                volume: scale_volume_by_master_percent(master_percent, ABDUCT_VOLUME),
            });
            commands.push(MixerCommand::Play {
                slot: ch.abduct_slot(),
                channel: ch.channel,
                flags: PLAY_ONE_SHOT,
            });
            self.abduct_active = true;
        } else if !channel_busy {
            self.abduct_active = false;
        }
        commands
    }

    /// Reproduce the crowd/storm trigger `FUN_0041cfe0`.
    ///
    /// The caller passes the recovered storm predicate's outcome as the [`AmbienceCue`]
    /// and the current channel-busy state. The crowd branch only acts while
    /// neither storm nor abduct holds the channel.
    pub fn crowd_or_storm_tick(
        &mut self,
        ch: AmbienceChannel,
        cue: AmbienceCue,
        channel_busy: bool,
        master_percent: i32,
    ) -> Vec<MixerCommand> {
        let mut commands = Vec::new();
        match cue {
            AmbienceCue::Crowd(crowd_volume) => {
                // Recovered guard: crowd plays only when d868==0 && d860==0.
                if self.storm_active || self.abduct_active {
                    return commands;
                }
                match crowd_volume {
                    Some(volume) => {
                        commands.push(MixerCommand::SetVolume {
                            channel: ch.channel,
                            volume: scale_volume_by_master_percent(master_percent, volume),
                        });
                        // Already looping: keep the voice, do not restart.
                        if channel_busy {
                            return commands;
                        }
                        commands.push(MixerCommand::Play {
                            slot: ch.crowd_slot(),
                            channel: ch.channel,
                            flags: PLAY_LOOPING,
                        });
                        self.crowd_playing = true;
                    }
                    None => {
                        if channel_busy {
                            commands.push(MixerCommand::StopChannel {
                                channel: ch.channel,
                            });
                        }
                    }
                }
            }
            AmbienceCue::Storm { selector, volume } => {
                let play_thunder = |commands: &mut Vec<MixerCommand>| {
                    commands.push(MixerCommand::SetVolume {
                        channel: ch.channel,
                        volume: scale_volume_by_master_percent(master_percent, volume),
                    });
                    commands.push(MixerCommand::Play {
                        slot: ch.thunder_slot(selector),
                        channel: ch.channel,
                        flags: PLAY_ONE_SHOT,
                    });
                };
                if self.crowd_playing {
                    if channel_busy {
                        commands.push(MixerCommand::StopChannel {
                            channel: ch.channel,
                        });
                    }
                    self.crowd_playing = false;
                    self.storm_active = true;
                    play_thunder(&mut commands);
                } else if !self.storm_active {
                    self.storm_active = true;
                    play_thunder(&mut commands);
                } else if !channel_busy {
                    play_thunder(&mut commands);
                }
            }
        }
        commands
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel() -> AmbienceChannel {
        AmbienceChannel {
            channel: ChannelId::new(4).unwrap(),
            base_slot: WaveSlotId::from_ordinal(10),
        }
    }

    #[test]
    fn abduct_first_tick_stops_sets_volume_and_plays_one_shot() {
        let ch = channel();
        let mut c = AmbienceController::new();
        let commands = c.abduct_tick(ch, true, 100);
        assert_eq!(
            commands,
            vec![
                MixerCommand::StopChannel {
                    channel: ch.channel
                },
                MixerCommand::SetVolume {
                    channel: ch.channel,
                    volume: 0x7fff,
                },
                MixerCommand::Play {
                    slot: WaveSlotId::from_ordinal(11),
                    channel: ch.channel,
                    flags: PLAY_ONE_SHOT,
                },
            ]
        );
        assert!(c.abduct_active());
    }

    #[test]
    fn abduct_without_busy_channel_skips_the_stop() {
        let ch = channel();
        let mut c = AmbienceController::new();
        let commands = c.abduct_tick(ch, false, 100);
        assert_eq!(commands.len(), 2);
        assert!(matches!(commands[0], MixerCommand::SetVolume { .. }));
    }

    #[test]
    fn abduct_clears_once_the_channel_finishes() {
        let ch = channel();
        let mut c = AmbienceController::new();
        c.abduct_tick(ch, false, 100); // starts the one-shot, abduct_active set
        // Still playing: no commands, flag stays set.
        assert!(c.abduct_tick(ch, true, 100).is_empty());
        assert!(c.abduct_active());
        // Finished: flag clears, no commands this tick.
        assert!(c.abduct_tick(ch, false, 100).is_empty());
        assert!(!c.abduct_active());
    }

    #[test]
    fn abduct_volume_is_master_scaled() {
        let ch = channel();
        let mut c = AmbienceController::new();
        let commands = c.abduct_tick(ch, false, 50);
        assert_eq!(
            commands[0],
            MixerCommand::SetVolume {
                channel: ch.channel,
                volume: 0x7fff / 2,
            }
        );
    }

    #[test]
    fn crowd_in_range_starts_a_looping_voice() {
        let ch = channel();
        let mut c = AmbienceController::new();
        let commands = c.crowd_or_storm_tick(ch, AmbienceCue::Crowd(Some(1000)), false, 100);
        assert_eq!(
            commands,
            vec![
                MixerCommand::SetVolume {
                    channel: ch.channel,
                    volume: 1000,
                },
                MixerCommand::Play {
                    slot: WaveSlotId::from_ordinal(10),
                    channel: ch.channel,
                    flags: PLAY_LOOPING,
                },
            ]
        );
        assert!(c.crowd_playing());
    }

    #[test]
    fn crowd_already_looping_only_updates_volume() {
        let ch = channel();
        let mut c = AmbienceController::new();
        let commands = c.crowd_or_storm_tick(ch, AmbienceCue::Crowd(Some(500)), true, 100);
        assert_eq!(commands.len(), 1);
        assert!(matches!(commands[0], MixerCommand::SetVolume { .. }));
    }

    #[test]
    fn crowd_out_of_range_stops_a_busy_channel() {
        let ch = channel();
        let mut c = AmbienceController::new();
        let busy = c.crowd_or_storm_tick(ch, AmbienceCue::Crowd(None), true, 100);
        assert_eq!(
            busy,
            vec![MixerCommand::StopChannel {
                channel: ch.channel
            }]
        );
        let idle = c.crowd_or_storm_tick(ch, AmbienceCue::Crowd(None), false, 100);
        assert!(idle.is_empty());
    }

    #[test]
    fn crowd_is_suppressed_while_abduct_or_storm_holds_the_channel() {
        let ch = channel();
        let mut c = AmbienceController::new();
        c.abduct_tick(ch, false, 100); // sets abduct_active
        assert!(
            c.crowd_or_storm_tick(ch, AmbienceCue::Crowd(Some(1000)), false, 100)
                .is_empty()
        );
    }

    #[test]
    fn storm_takes_over_from_the_crowd_and_plays_the_selected_variant() {
        let ch = channel();
        let mut c = AmbienceController::new();
        c.crowd_or_storm_tick(ch, AmbienceCue::Crowd(Some(1000)), false, 100); // crowd playing
        let commands = c.crowd_or_storm_tick(
            ch,
            AmbienceCue::Storm {
                selector: 2,
                volume: 2000,
            },
            true,
            100,
        );
        assert_eq!(
            commands,
            vec![
                MixerCommand::StopChannel {
                    channel: ch.channel
                },
                MixerCommand::SetVolume {
                    channel: ch.channel,
                    volume: 2000,
                },
                MixerCommand::Play {
                    // base 10 + 2 + selector 2 = 14 (rumble1).
                    slot: WaveSlotId::from_ordinal(14),
                    channel: ch.channel,
                    flags: PLAY_ONE_SHOT,
                },
            ]
        );
        assert!(c.storm_active() && !c.crowd_playing());
    }

    #[test]
    fn storm_replays_only_once_the_one_shot_finishes() {
        let ch = channel();
        let mut c = AmbienceController::new();
        let cue = AmbienceCue::Storm {
            selector: 0,
            volume: 500,
        };
        // First storm tick (no crowd): sets storm_active and plays.
        assert_eq!(c.crowd_or_storm_tick(ch, cue, false, 100).len(), 2);
        assert!(c.storm_active());
        // Still playing: nothing.
        assert!(c.crowd_or_storm_tick(ch, cue, true, 100).is_empty());
        // Finished: replays.
        assert_eq!(c.crowd_or_storm_tick(ch, cue, false, 100).len(), 2);
    }
}
