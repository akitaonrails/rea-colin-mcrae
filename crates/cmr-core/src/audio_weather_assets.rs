//! Recovered weather/crowd ambience wave inventory.
//!
//! The official-EU `Rally.exe` ambience loader `FUN_0041cd30` registers a fixed
//! set of crowd and weather waves through the mixer load wrapper
//! `FUN_004958e0` (see [`crate::audio_mixer`]). For each wave it builds a path
//! from a base sound directory (`FUN_00496730`) concatenated with a relative
//! name, then calls `FUN_004958e0(path, 0x2b11, 0x10, stereo)` - that is, 11025
//! Hz, 16-bit PCM, mono or stereo. Load order fixes the wave-slot ordinals.
//!
//! This module records the recovered inventory as metadata (relative paths and
//! formats, never payload bytes), the way the bitmap and font loaders record
//! their recovered asset names. It lets the reconstruction drive the recovered
//! load sequence against a user-provided source root without bundling assets.
//!
//! Observations (evidence `rea.official-eu.audio-weather-wave-inventory`):
//! - Unconditional loads: `sounds\crowd\crowd.wav` (mono), then
//!   `sounds\weather\abduct.wav` (stereo).
//! - Conditional loads, only when `FUN_0046b230()` returns `3`:
//!   `sounds\weather\clap1.wav`, `clap2.wav`, `rumble1.wav`, `rumble2.wav` (all
//!   mono).
//! - Every wave uses rate `0x2b11` (11025) and `0x10` (16) bits.
//!
//! Inferences: `FUN_0046b230()==3` is the highest weather/detail level, at which
//! the thunderclap and rumble ambience are added; the mixer output rate (22050
//! Hz) differs from these waves' 11025 Hz, so QMixer resamples them.
//!
//! Unknowns: the base sound directory contents (user-root dependent) and whether
//! payload comes from a loose file or a pack; the exact domain of
//! `FUN_0046b230` (assumed weather level); the remaining thirteen load-callers'
//! wave groups (engine, surface, UI, etc.) are not yet enumerated.

use crate::audio_mixer::{AudioMixer, MixerError, WaveFormat, WaveSlotId};

/// Recovered mixer wave sample rate for the ambience group (`0x2b11`).
pub const AMBIENCE_WAVE_RATE_HZ: u32 = 11_025;

/// Recovered mixer wave bit depth for the ambience group (`0x10`).
pub const AMBIENCE_WAVE_BITS: u16 = 16;

/// The recovered weather-detail level at which `FUN_0046b230()` enables the
/// conditional thunder/rumble ambience.
pub const WEATHER_DETAIL_WITH_STORM: i32 = 3;

/// A recovered ambience wave: its source-relative path and PCM parameters, plus
/// whether it loads unconditionally or only at the storm weather level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WeatherWave {
    /// Path relative to the recovered base sound directory, using the original
    /// backslash separators as stored in the binary.
    pub relative_path: &'static str,
    /// Recovered stereo flag passed to the load wrapper.
    pub stereo: bool,
    /// `true` when the wave loads only at [`WEATHER_DETAIL_WITH_STORM`].
    pub storm_only: bool,
}

impl WeatherWave {
    /// Build the recovered PCM format for this wave (11025 Hz / 16-bit).
    pub fn format(&self) -> Result<WaveFormat, MixerError> {
        WaveFormat::pcm(AMBIENCE_WAVE_RATE_HZ, AMBIENCE_WAVE_BITS, self.stereo)
    }
}

/// The recovered ambience waves in `FUN_0041cd30` load order. Index is the
/// relative wave-slot ordinal within this loader's sequence.
pub const WEATHER_WAVES: [WeatherWave; 6] = [
    WeatherWave {
        relative_path: r"sounds\crowd\crowd.wav",
        stereo: false,
        storm_only: false,
    },
    WeatherWave {
        relative_path: r"sounds\weather\abduct.wav",
        stereo: true,
        storm_only: false,
    },
    WeatherWave {
        relative_path: r"sounds\weather\clap1.wav",
        stereo: false,
        storm_only: true,
    },
    WeatherWave {
        relative_path: r"sounds\weather\clap2.wav",
        stereo: false,
        storm_only: true,
    },
    WeatherWave {
        relative_path: r"sounds\weather\rumble1.wav",
        stereo: false,
        storm_only: true,
    },
    WeatherWave {
        relative_path: r"sounds\weather\rumble2.wav",
        stereo: false,
        storm_only: true,
    },
];

/// The waves `FUN_0041cd30` loads at the given weather-detail level, in order.
///
/// Mirrors the recovered control flow: the first two waves always load; the
/// remaining four load only when the level equals [`WEATHER_DETAIL_WITH_STORM`].
pub fn waves_for_weather_detail(level: i32) -> impl Iterator<Item = &'static WeatherWave> {
    let storm = level == WEATHER_DETAIL_WITH_STORM;
    WEATHER_WAVES
        .iter()
        .filter(move |wave| storm || !wave.storm_only)
}

/// Replay the recovered `FUN_0041cd30` load sequence through a mixer backend.
///
/// For the given weather-detail level, this loads each selected wave in the
/// recovered order, returning the assigned slot ids. It reproduces the recovered
/// path construction - the base sound directory (`FUN_00496730`, supplied here as
/// `base_path`) concatenated with each wave's stored relative name - without
/// imposing any host path semantics; `base_path` is joined exactly as the
/// recovered `strcpy`+`strcat` does. The mixer must already be initialized; a
/// load before init surfaces as the backend's error.
pub fn load_weather_ambience<M: AudioMixer>(
    mixer: &mut M,
    base_path: &str,
    weather_detail: i32,
) -> Result<Vec<WaveSlotId>, M::Error> {
    let mut slots = Vec::new();
    for wave in waves_for_weather_detail(weather_detail) {
        let name = format!("{base_path}{}", wave.relative_path);
        let slot = mixer.load_wave(
            &name,
            AMBIENCE_WAVE_RATE_HZ,
            AMBIENCE_WAVE_BITS,
            wave.stereo,
        )?;
        slots.push(slot);
    }
    Ok(slots)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_recovered_wave_builds_a_valid_format() {
        for wave in &WEATHER_WAVES {
            let format = wave.format().expect("recovered wave format is valid");
            assert_eq!(format.sample_rate, 11_025);
            assert_eq!(format.bits_per_sample, 16);
            assert_eq!(format.channels, if wave.stereo { 2 } else { 1 });
        }
    }

    #[test]
    fn load_order_and_flags_match_recovery() {
        assert_eq!(WEATHER_WAVES[0].relative_path, r"sounds\crowd\crowd.wav");
        assert!(!WEATHER_WAVES[0].stereo && !WEATHER_WAVES[0].storm_only);
        assert_eq!(WEATHER_WAVES[1].relative_path, r"sounds\weather\abduct.wav");
        assert!(WEATHER_WAVES[1].stereo && !WEATHER_WAVES[1].storm_only);
        // The storm group is exactly the last four, all mono.
        for wave in &WEATHER_WAVES[2..] {
            assert!(
                wave.storm_only,
                "{} should be storm-only",
                wave.relative_path
            );
            assert!(!wave.stereo);
        }
    }

    #[test]
    fn low_weather_detail_loads_only_the_unconditional_pair() {
        let loaded: Vec<_> = waves_for_weather_detail(0)
            .map(|w| w.relative_path)
            .collect();
        assert_eq!(
            loaded,
            [r"sounds\crowd\crowd.wav", r"sounds\weather\abduct.wav"]
        );
    }

    #[test]
    fn storm_weather_detail_loads_all_six_in_order() {
        let loaded: Vec<_> = waves_for_weather_detail(WEATHER_DETAIL_WITH_STORM)
            .map(|w| w.relative_path)
            .collect();
        assert_eq!(loaded.len(), 6);
        assert_eq!(loaded[5], r"sounds\weather\rumble2.wav");
    }

    use crate::audio_mixer::{ChannelId, MixerSessionState, Position3d};

    /// A mixer backend that records each load's resolved name and parameters,
    /// backed by the recovered session bookkeeping for slot ids.
    #[derive(Default)]
    struct RecordingMixer {
        state: MixerSessionState,
        loads: Vec<(String, u32, u16, bool)>,
    }

    impl AudioMixer for RecordingMixer {
        type Error = MixerError;

        fn initialize(&mut self) -> Result<(), Self::Error> {
            self.state.initialize();
            Ok(())
        }

        fn load_wave(
            &mut self,
            name: &str,
            sample_rate: u32,
            bits_per_sample: u16,
            stereo: bool,
        ) -> Result<WaveSlotId, Self::Error> {
            let (slot, _) = self
                .state
                .load_wave(name, sample_rate, bits_per_sample, stereo)?;
            self.loads
                .push((name.to_owned(), sample_rate, bits_per_sample, stereo));
            Ok(slot)
        }

        fn play(
            &mut self,
            slot: WaveSlotId,
            channel: ChannelId,
            flags: u32,
        ) -> Result<(), Self::Error> {
            self.state.play(slot, channel, flags).map(drop)
        }
        fn set_volume(&mut self, channel: ChannelId, volume: i32) -> Result<(), Self::Error> {
            self.state.set_volume(channel, volume).map(drop)
        }
        fn set_frequency(&mut self, channel: ChannelId, frequency: u32) -> Result<(), Self::Error> {
            self.state.set_frequency(channel, frequency).map(drop)
        }
        fn set_position(
            &mut self,
            channel: ChannelId,
            position: Position3d,
        ) -> Result<(), Self::Error> {
            self.state.set_position(channel, position).map(drop)
        }
        fn stop(&mut self, channel: ChannelId) -> Result<(), Self::Error> {
            self.state.stop(channel).map(drop)
        }
        fn is_channel_busy(&self, _channel: ChannelId) -> bool {
            false
        }
    }

    #[test]
    fn driver_replays_the_recovered_storm_sequence_with_concatenated_paths() {
        let mut mixer = RecordingMixer::default();
        mixer.initialize().unwrap();
        let slots =
            load_weather_ambience(&mut mixer, r"C:\game\", WEATHER_DETAIL_WITH_STORM).unwrap();
        // Sequential slot ids from zero, one per selected wave.
        assert_eq!(
            slots.iter().map(|s| s.ordinal()).collect::<Vec<_>>(),
            [0, 1, 2, 3, 4, 5]
        );
        // The recovered base+relative concatenation and per-wave parameters.
        assert_eq!(
            mixer.loads[0],
            (r"C:\game\sounds\crowd\crowd.wav".into(), 11_025, 16, false)
        );
        assert_eq!(
            mixer.loads[1],
            (
                r"C:\game\sounds\weather\abduct.wav".into(),
                11_025,
                16,
                true
            )
        );
        assert_eq!(
            mixer.loads[5],
            (
                r"C:\game\sounds\weather\rumble2.wav".into(),
                11_025,
                16,
                false
            )
        );
    }

    #[test]
    fn driver_loads_only_the_pair_below_the_storm_level() {
        let mut mixer = RecordingMixer::default();
        mixer.initialize().unwrap();
        let slots = load_weather_ambience(&mut mixer, "", 1).unwrap();
        assert_eq!(slots.len(), 2);
        assert_eq!(mixer.loads.len(), 2);
    }

    #[test]
    fn driver_surfaces_a_load_before_init() {
        let mut mixer = RecordingMixer::default();
        let error = load_weather_ambience(&mut mixer, "", 0).unwrap_err();
        assert_eq!(error, MixerError::NotInitialized);
        assert!(mixer.loads.is_empty());
    }
}
