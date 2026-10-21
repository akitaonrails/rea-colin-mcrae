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

use crate::audio_mixer::{MixerError, WaveFormat};

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
}
