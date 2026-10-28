//! Portable frontend/replay music backend (item H07, music half).
//!
//! The music path is just looping PCM wave playback, so the portable "backend"
//! is a controller that drives the recovered H05 music lifecycle
//! ([`crate::audio_music_cues`]) as [`MixerCommand`]s over the
//! [`crate::audio_mixer`] contract. No new device code is needed: the existing
//! [`crate::audio_render::SoftwareMixer`] and the SDL audio device already
//! execute these commands.
//!
//! The controller reproduces the recovered decisions: start frontend music when
//! the context gate opens (`FUN_00443b20`: set the configured volume, play
//! looping with loop count `0xffffffff`), and run the per-frame keep-alive
//! (`FUN_004c9d70`: restart the loop when the voice drains, advance the fade-in
//! ramp while below the target, else hold). The fade ramp's per-step volume
//! curve `FUN_004c97f0` is now recovered (`QSWaveMixSetVolume(channel,
//! counter * 20000 / 100)` = [`music_fade_volume`]), so the keep-alive emits the
//! ramped `SetVolume` and the ramp lands exactly on the configured
//! [`music_volume`] at the fade target. The explicit music-stop trigger site is
//! unrecovered, so [`MusicController::stop`] exposes the teardown capability
//! without asserting the game's exact trigger.
//!
//! See evidence `rea.official-eu.music-lifecycle` / `rea.official-eu.frontend-music-channel`.

use crate::audio_mixer::{ChannelId, MixerCommand, WaveSlotId};
use crate::audio_music_cues::{
    MUSIC_LOOP_COUNT, MusicKeepAlive, MusicTrack, frontend_music_active, music_fade_target,
    music_fade_volume, music_keepalive, music_volume,
};

/// Drives looping frontend/replay music over the mixer contract. Holds the
/// dedicated music channel + wave slot and the two config bytes that set the
/// playback volume (`DAT_00d3ca80 + 7`) and the fade-in target
/// (`0x00d3ac60 + 0xf`).
#[derive(Debug, Clone, Copy)]
pub struct MusicController {
    channel: ChannelId,
    slot: WaveSlotId,
    volume_config: u8,
    fade_config: u8,
    ramp: u32,
    active: bool,
}

impl MusicController {
    /// Create a controller for a channel/slot pair and the recovered config
    /// bytes. The caller registers the track's wave into `slot` (via the mixer
    /// session / file source) before starting.
    pub fn new(channel: ChannelId, slot: WaveSlotId, volume_config: u8, fade_config: u8) -> Self {
        Self {
            channel,
            slot,
            volume_config,
            fade_config,
            ramp: 0,
            active: false,
        }
    }

    /// The record's current music bytes (the sound options edit them).
    pub fn set_config(&mut self, volume_config: u8, fade_config: u8) {
        self.volume_config = volume_config;
        self.fade_config = fade_config;
    }

    /// The music channel.
    pub fn channel(&self) -> ChannelId {
        self.channel
    }

    /// The music wave slot.
    pub fn slot(&self) -> WaveSlotId {
        self.slot
    }

    /// Whether music is currently playing.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// The current fade-in ramp counter.
    pub fn fade_ramp(&self) -> u32 {
        self.ramp
    }

    /// The start sequence (`FUN_00443b20`): set the configured volume, then play
    /// the slot looping.
    fn start_commands(&self) -> Vec<MixerCommand> {
        vec![
            MixerCommand::SetVolume {
                channel: self.channel,
                volume: music_volume(self.volume_config),
            },
            MixerCommand::Play {
                slot: self.slot,
                channel: self.channel,
                flags: MUSIC_LOOP_COUNT,
            },
        ]
    }

    /// Enter/update a context. When the context gate opens (`DAT_00d3ca88 == 0`)
    /// and music is not already playing, begin frontend music and return the
    /// start commands; otherwise return no commands.
    pub fn on_context(&mut self, context: u32) -> Vec<MixerCommand> {
        if frontend_music_active(context) && !self.active {
            self.active = true;
            self.ramp = 0;
            self.start_commands()
        } else {
            Vec::new()
        }
    }

    /// One keep-alive frame (`FUN_004c9d70`): when the voice has drained, restart
    /// the loop (and reset the fade ramp); while the voice plays below the fade
    /// target, advance the ramp one step and set the recovered ramped volume
    /// ([`music_fade_volume`]); at or above the target, hold. The decision uses
    /// the controller's own ramp (`DAT_00d90088`), so the caller supplies only
    /// whether the voice is still playing (`QSWaveMixIsChannelDone == 0`).
    /// Returns no commands when music is inactive.
    pub fn keepalive(&mut self, voice_playing: bool) -> Vec<MixerCommand> {
        if !self.active {
            return Vec::new();
        }
        match music_keepalive(
            voice_playing,
            self.ramp as i32,
            music_fade_target(self.fade_config),
        ) {
            MusicKeepAlive::Restart => {
                self.ramp = 0;
                vec![MixerCommand::Play {
                    slot: self.slot,
                    channel: self.channel,
                    flags: MUSIC_LOOP_COUNT,
                }]
            }
            MusicKeepAlive::FadeIn => {
                self.ramp = self.ramp.saturating_add(1);
                vec![MixerCommand::SetVolume {
                    channel: self.channel,
                    volume: music_fade_volume(self.ramp as i32),
                }]
            }
            MusicKeepAlive::HoldFadedIn => Vec::new(),
        }
    }

    /// Stop music and reset the controller (teardown). The exact in-game trigger
    /// site is an H05 unknown; this provides the capability for the
    /// frontend -> race transition.
    pub fn stop(&mut self) -> Vec<MixerCommand> {
        self.active = false;
        self.ramp = 0;
        vec![MixerCommand::StopChannel {
            channel: self.channel,
        }]
    }

    /// The track selected by a context value. Only the frontend context is
    /// recovered (the replay-context trigger is an H05 unknown), so a non-zero
    /// context yields `None`.
    pub fn track_for_context(context: u32) -> Option<MusicTrack> {
        if frontend_music_active(context) {
            Some(MusicTrack::Frontend)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn controller() -> MusicController {
        // Volume config 100 -> 100*400000/100 = 400000; fade 50 -> 50*0x14 = 1000.
        MusicController::new(
            ChannelId::new(5).unwrap(),
            WaveSlotId::from_ordinal(12),
            100,
            50,
        )
    }

    #[test]
    fn enters_frontend_context_once() {
        let mut c = controller();
        let start = c.on_context(0);
        assert_eq!(
            start,
            vec![
                MixerCommand::SetVolume {
                    channel: ChannelId::new(5).unwrap(),
                    volume: 400_000,
                },
                MixerCommand::Play {
                    slot: WaveSlotId::from_ordinal(12),
                    channel: ChannelId::new(5).unwrap(),
                    flags: MUSIC_LOOP_COUNT,
                },
            ]
        );
        assert!(c.is_active());
        // Re-entering the same context does not restart it.
        assert!(c.on_context(0).is_empty());
    }

    #[test]
    fn non_frontend_context_does_not_start() {
        let mut c = controller();
        assert!(c.on_context(1).is_empty());
        assert!(!c.is_active());
    }

    #[test]
    fn keepalive_restarts_on_drain_and_ramps_while_playing() {
        let mut c = controller();
        c.on_context(0);
        // Fade target is 50 * 0x14 = 1000, reached once the ramp counter hits it.
        // Below target while playing: advance the ramp, set the ramped volume.
        assert_eq!(
            c.keepalive(true),
            vec![MixerCommand::SetVolume {
                channel: ChannelId::new(5).unwrap(),
                volume: music_fade_volume(1),
            }]
        );
        assert_eq!(c.fade_ramp(), 1);
        assert_eq!(
            c.keepalive(true),
            vec![MixerCommand::SetVolume {
                channel: ChannelId::new(5).unwrap(),
                volume: music_fade_volume(2),
            }]
        );
        assert_eq!(c.fade_ramp(), 2);
        // Drained: restart the loop, reset the ramp.
        let restart = c.keepalive(false);
        assert_eq!(
            restart,
            vec![MixerCommand::Play {
                slot: WaveSlotId::from_ordinal(12),
                channel: ChannelId::new(5).unwrap(),
                flags: MUSIC_LOOP_COUNT,
            }]
        );
        assert_eq!(c.fade_ramp(), 0);
    }

    #[test]
    fn keepalive_holds_once_the_ramp_reaches_the_fade_target() {
        // Fade config 1 -> target counter 1*0x14 = 20. Volume config matched to
        // 1 so the ramp lands exactly on the configured full volume at target.
        let mut c = MusicController::new(
            ChannelId::new(5).unwrap(),
            WaveSlotId::from_ordinal(12),
            1,
            1,
        );
        c.on_context(0);
        let target = music_fade_target(1);
        assert_eq!(target, 20);
        // Ramp up to the target; each step sets the ramped volume.
        for step in 1..=target {
            assert_eq!(
                c.keepalive(true),
                vec![MixerCommand::SetVolume {
                    channel: ChannelId::new(5).unwrap(),
                    volume: music_fade_volume(step),
                }]
            );
        }
        assert_eq!(c.fade_ramp(), target as u32);
        // At the target: hold, no further command, ramp pinned.
        assert!(c.keepalive(true).is_empty());
        assert_eq!(c.fade_ramp(), target as u32);
        // The ramp landed on the configured full volume.
        assert_eq!(music_fade_volume(target), music_volume(1));
    }

    #[test]
    fn keepalive_is_inert_when_inactive() {
        let mut c = controller();
        assert!(c.keepalive(false).is_empty());
        assert_eq!(c.fade_ramp(), 0);
    }

    #[test]
    fn stop_emits_stop_and_deactivates() {
        let mut c = controller();
        c.on_context(0);
        let stop = c.stop();
        assert_eq!(
            stop,
            vec![MixerCommand::StopChannel {
                channel: ChannelId::new(5).unwrap(),
            }]
        );
        assert!(!c.is_active());
        // After a stop, a fresh context entry starts again.
        assert!(!c.on_context(0).is_empty());
    }

    #[test]
    fn track_selection_is_frontend_only() {
        assert_eq!(
            MusicController::track_for_context(0),
            Some(MusicTrack::Frontend)
        );
        assert_eq!(MusicController::track_for_context(1), None);
    }
}
