//! A portable software PCM mixer backend for the recovered [`AudioMixer`]
//! contract.
//!
//! This is the first concrete backend (reconstruction-order step 5): it
//! implements the already-defined [`crate::audio_mixer::AudioMixer`] interface
//! and passes the same [`crate::audio_mixer::check_audio_mixer_contract`] as the
//! reference bookkeeping. It performs no I/O and no OS calls - it mixes
//! already-decoded PCM voices into an interleaved stereo output buffer, so it is
//! fully unit-testable without an audio device. A later platform backend (e.g.
//! SDL/cpal) wraps this: it fills the [`WaveSource`] by decoding the user's WAV
//! files and pumps [`SoftwareMixer::render`] into the device callback.
//!
//! **Faithfulness boundary.** The recovered contract passes volume, frequency,
//! and 3D position through verbatim because their QMixer unit scales are
//! documented unknowns (see `audio_mixer`). This backend therefore makes
//! explicit, labelled *design decisions* for how to realise them as sound; they
//! are not claims about the original mixer:
//!
//! - **Volume** is treated as a linear gain `clamp(volume, 0, 0x7fff) / 0x7fff`
//!   (so the recovered full-scale `0x7fff` is unity, `0x7d00`/`0x7d00`-style
//!   values are near-unity). The real QMixer curve is unknown.
//! - **Frequency** sets the playback step `frequency / output_rate`; a voice
//!   defaults to its wave's own sample rate (unity pitch) until `set_frequency`
//!   retunes it, matching how the engine model drives pitch.
//! - **3D position** is recorded per channel but not yet spatialised (equal
//!   left/right), because the coordinate space is unrecovered. Panning is left
//!   for when that is established, rather than invented here.
//!
//! Sample interpolation is nearest-neighbour; the output is interleaved stereo
//! 16-bit at [`MIXER_OUTPUT_RATE_HZ`].

use std::collections::HashMap;

use crate::audio_mixer::{
    AudioMixer, ChannelId, MIXER_CHANNEL_CAPACITY, MIXER_OUTPUT_RATE_HZ, MixerError,
    MixerSessionState, PLAY_LOOPING, Position3d, WaveFormat, WaveSlotId,
};

/// Full-scale volume (`0x7fff`), the gain denominator for this backend.
pub const FULL_SCALE_VOLUME: i32 = 0x7fff;

/// A decoded PCM wave: interleaved 16-bit samples plus the recovered format.
///
/// `samples` holds `format.channels`-interleaved frames (mono = one value per
/// frame, stereo = L,R per frame). 8-bit source material is expected to be
/// up-converted to 16-bit by the decoder that fills the [`WaveSource`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PcmWave {
    pub format: WaveFormat,
    pub samples: Vec<i16>,
}

impl PcmWave {
    /// Number of sample frames (samples divided by channel count).
    pub fn frame_count(&self) -> usize {
        let channels = self.format.channels.max(1) as usize;
        self.samples.len() / channels
    }

    /// The `(left, right)` pair at a frame index, mono expanded to both sides.
    /// Out-of-range indices return silence.
    fn frame(&self, index: usize) -> (i32, i32) {
        let channels = self.format.channels.max(1) as usize;
        if index >= self.frame_count() {
            return (0, 0);
        }
        if channels == 1 {
            let s = self.samples[index] as i32;
            (s, s)
        } else {
            let base = index * channels;
            (self.samples[base] as i32, self.samples[base + 1] as i32)
        }
    }
}

/// Resolves a logical wave name (the game's path) to decoded PCM. A platform
/// backend implements this by reading and decoding the user's WAV files; tests
/// use an in-memory map.
pub trait WaveSource {
    /// Fetch the decoded wave for a name, or `None` if it cannot be provided
    /// (the recovered play path treats such a slot as a silent no-op).
    fn fetch(&self, name: &str) -> Option<PcmWave>;
}

impl WaveSource for HashMap<String, PcmWave> {
    fn fetch(&self, name: &str) -> Option<PcmWave> {
        self.get(name).cloned()
    }
}

/// An empty wave source: every fetch is silent. Lets the mixer satisfy the
/// contract (which loads named waves with no payload) without a bank.
#[derive(Debug, Clone, Copy, Default)]
pub struct EmptyWaveSource;

impl WaveSource for EmptyWaveSource {
    fn fetch(&self, _name: &str) -> Option<PcmWave> {
        None
    }
}

/// A playing voice on a channel.
#[derive(Debug, Clone, Copy)]
struct Voice {
    /// Index into [`SoftwareMixer::waves`] (parallel to the slot table).
    wave_index: usize,
    /// Fractional read cursor, in source frames.
    cursor: f64,
    /// Frames advanced per output frame (`frequency / output_rate`).
    step: f64,
    /// Whether the voice loops at end-of-wave.
    looping: bool,
}

/// Per-channel mix state.
#[derive(Debug, Clone, Copy)]
struct Channel {
    volume: i32,
    frequency: Option<u32>,
    #[allow(dead_code)]
    position: Position3d,
    voice: Option<Voice>,
}

impl Default for Channel {
    fn default() -> Self {
        Self {
            // Unknown-default design decision: full scale until set_volume runs.
            volume: FULL_SCALE_VOLUME,
            frequency: None,
            position: Position3d { x: 0, y: 0, z: 0 },
            voice: None,
        }
    }
}

/// A software mixer realising the recovered contract over a [`WaveSource`].
#[derive(Debug, Clone)]
pub struct SoftwareMixer<B: WaveSource> {
    state: MixerSessionState,
    bank: B,
    /// Decoded wave per slot, parallel to the session slot table. `None` is a
    /// slot whose payload the bank could not supply (a silent no-op on play).
    waves: Vec<Option<PcmWave>>,
    channels: [Channel; MIXER_CHANNEL_CAPACITY as usize],
    output_rate: u32,
}

impl<B: WaveSource + Default> Default for SoftwareMixer<B> {
    fn default() -> Self {
        Self::with_source(B::default())
    }
}

impl<B: WaveSource> SoftwareMixer<B> {
    /// Build a mixer over the given wave source.
    pub fn with_source(bank: B) -> Self {
        Self {
            state: MixerSessionState::new(),
            bank,
            waves: Vec::new(),
            channels: [Channel::default(); MIXER_CHANNEL_CAPACITY as usize],
            output_rate: MIXER_OUTPUT_RATE_HZ,
        }
    }

    fn channel_mut(&mut self, channel: ChannelId) -> &mut Channel {
        &mut self.channels[channel.index() as usize]
    }

    /// Render `out.len() / 2` interleaved stereo frames, advancing every active
    /// voice. A finished one-shot voice clears itself; a looping voice wraps.
    pub fn render(&mut self, out: &mut [i16]) {
        let output_rate = self.output_rate as f64;
        for frame in out.chunks_mut(2) {
            let mut acc_l: i32 = 0;
            let mut acc_r: i32 = 0;
            for channel in &mut self.channels {
                let Some(voice) = channel.voice.as_mut() else {
                    continue;
                };
                let Some(Some(wave)) = self.waves.get(voice.wave_index) else {
                    channel.voice = None;
                    continue;
                };
                let frames = wave.frame_count();
                if frames == 0 {
                    channel.voice = None;
                    continue;
                }
                let index = voice.cursor as usize;
                let (l, r) = wave.frame(index);
                let gain = volume_gain(channel.volume);
                acc_l += (l as f32 * gain) as i32;
                acc_r += (r as f32 * gain) as i32;

                voice.cursor += if voice.step > 0.0 {
                    voice.step
                } else {
                    wave.format.sample_rate as f64 / output_rate
                };
                if voice.cursor as usize >= frames {
                    if voice.looping {
                        // Wrap, preserving the fractional overshoot.
                        voice.cursor %= frames as f64;
                    } else {
                        channel.voice = None;
                    }
                }
            }
            if frame.len() == 2 {
                frame[0] = clamp_i16(acc_l);
                frame[1] = clamp_i16(acc_r);
            }
        }
    }
}

/// Linear gain for a recovered volume value (design decision; see module docs).
fn volume_gain(volume: i32) -> f32 {
    volume.clamp(0, FULL_SCALE_VOLUME) as f32 / FULL_SCALE_VOLUME as f32
}

/// Saturating conversion of a mixed accumulator to 16-bit.
fn clamp_i16(sample: i32) -> i16 {
    sample.clamp(i16::MIN as i32, i16::MAX as i32) as i16
}

impl<B: WaveSource> AudioMixer for SoftwareMixer<B> {
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
        // Keep the decoded-wave table parallel to the slot table. A bank miss
        // registers a silent slot rather than failing the load.
        debug_assert_eq!(slot.ordinal() as usize, self.waves.len());
        self.waves.push(self.bank.fetch(name));
        Ok(slot)
    }

    fn play(
        &mut self,
        slot: WaveSlotId,
        channel: ChannelId,
        flags: u32,
    ) -> Result<(), Self::Error> {
        // Defer to the recovered bookkeeping for the init guard, unknown-slot
        // rejection, null-handle no-op, and the channel-wave record.
        if self.state.play(slot, channel, flags)?.is_none() {
            return Ok(());
        }
        let wave_index = slot.ordinal() as usize;
        // A registered slot with no decoded payload stays a silent no-op.
        let native_rate = match self.waves.get(wave_index) {
            Some(Some(wave)) if wave.frame_count() > 0 => wave.format.sample_rate,
            _ => return Ok(()),
        };
        let output_rate = self.output_rate as f64;
        let ch = self.channel_mut(channel);
        let frequency = ch.frequency.unwrap_or(native_rate);
        ch.voice = Some(Voice {
            wave_index,
            cursor: 0.0,
            step: frequency as f64 / output_rate,
            looping: flags == PLAY_LOOPING,
        });
        Ok(())
    }

    fn set_volume(&mut self, channel: ChannelId, volume: i32) -> Result<(), Self::Error> {
        self.state.set_volume(channel, volume)?;
        self.channel_mut(channel).volume = volume;
        Ok(())
    }

    fn set_frequency(&mut self, channel: ChannelId, frequency: u32) -> Result<(), Self::Error> {
        self.state.set_frequency(channel, frequency)?;
        let output_rate = self.output_rate as f64;
        let ch = self.channel_mut(channel);
        ch.frequency = Some(frequency);
        if let Some(voice) = ch.voice.as_mut() {
            voice.step = frequency as f64 / output_rate;
        }
        Ok(())
    }

    fn set_position(
        &mut self,
        channel: ChannelId,
        position: Position3d,
    ) -> Result<(), Self::Error> {
        self.state.set_position(channel, position)?;
        self.channel_mut(channel).position = position;
        Ok(())
    }

    fn stop(&mut self, channel: ChannelId) -> Result<(), Self::Error> {
        self.state.stop(channel)?;
        self.channel_mut(channel).voice = None;
        Ok(())
    }

    fn is_channel_busy(&self, channel: ChannelId) -> bool {
        if !self.state.is_initialized() {
            return false;
        }
        self.channels[channel.index() as usize].voice.is_some()
    }
}

/// A [`SoftwareMixer`] over an in-memory name->wave map.
pub type MapMixer = SoftwareMixer<HashMap<String, PcmWave>>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_mixer::{PLAY_ONE_SHOT, check_audio_mixer_contract};

    fn mono(samples: &[i16], rate: u32) -> PcmWave {
        PcmWave {
            format: WaveFormat::pcm(rate, 16, false).unwrap(),
            samples: samples.to_vec(),
        }
    }

    fn bank(entries: &[(&str, PcmWave)]) -> HashMap<String, PcmWave> {
        entries
            .iter()
            .map(|(n, w)| ((*n).to_owned(), w.clone()))
            .collect()
    }

    fn ch(index: u32) -> ChannelId {
        ChannelId::new(index).unwrap()
    }

    #[test]
    fn empty_backend_satisfies_the_shared_contract() {
        check_audio_mixer_contract::<MapMixer>().unwrap();
    }

    #[test]
    fn one_shot_plays_each_sample_then_stops() {
        let mut mixer =
            SoftwareMixer::with_source(bank(&[("beep", mono(&[100, 200, 300], 22_050))]));
        mixer.initialize().unwrap();
        let slot = mixer.load_wave("beep", 22_050, 16, false).unwrap();
        mixer.play(slot, ch(0), PLAY_ONE_SHOT).unwrap();
        assert!(mixer.is_channel_busy(ch(0)));

        let mut out = [0i16; 8]; // 4 stereo frames
        mixer.render(&mut out);
        // Native rate == output rate -> step 1.0 -> one source frame per output.
        assert_eq!(out, [100, 100, 200, 200, 300, 300, 0, 0]);
        // The one-shot finished during the 4th frame.
        assert!(!mixer.is_channel_busy(ch(0)));
    }

    #[test]
    fn looping_voice_wraps_and_stays_busy() {
        let mut mixer = SoftwareMixer::with_source(bank(&[("lp", mono(&[10, 20], 22_050))]));
        mixer.initialize().unwrap();
        let slot = mixer.load_wave("lp", 22_050, 16, false).unwrap();
        mixer.play(slot, ch(1), PLAY_LOOPING).unwrap();

        let mut out = [0i16; 10]; // 5 frames
        mixer.render(&mut out);
        assert_eq!(out, [10, 10, 20, 20, 10, 10, 20, 20, 10, 10]);
        assert!(mixer.is_channel_busy(ch(1)));
    }

    #[test]
    fn volume_scales_the_output_as_linear_gain() {
        let mut mixer = SoftwareMixer::with_source(bank(&[("s", mono(&[100], 22_050))]));
        mixer.initialize().unwrap();
        let slot = mixer.load_wave("s", 22_050, 16, false).unwrap();
        mixer.set_volume(ch(0), FULL_SCALE_VOLUME / 2).unwrap();
        mixer.play(slot, ch(0), PLAY_LOOPING).unwrap();

        let mut out = [0i16; 2];
        mixer.render(&mut out);
        // 100 * (16383/32767) = 49.998 -> truncated to 49.
        assert_eq!(out[0], 49);
    }

    #[test]
    fn set_frequency_changes_the_playback_step() {
        let mut mixer = SoftwareMixer::with_source(bank(&[("s", mono(&[10, 20, 30, 40], 22_050))]));
        mixer.initialize().unwrap();
        let slot = mixer.load_wave("s", 22_050, 16, false).unwrap();
        // Double the output rate -> step 2.0 -> every other source frame.
        mixer.set_frequency(ch(0), 44_100).unwrap();
        mixer.play(slot, ch(0), PLAY_ONE_SHOT).unwrap();

        let mut out = [0i16; 6]; // 3 frames
        mixer.render(&mut out);
        assert_eq!(out, [10, 10, 30, 30, 0, 0]);
        assert!(!mixer.is_channel_busy(ch(0)));
    }

    #[test]
    fn two_channels_mix_additively() {
        let mut mixer = SoftwareMixer::with_source(bank(&[
            ("a", mono(&[100], 22_050)),
            ("b", mono(&[50], 22_050)),
        ]));
        mixer.initialize().unwrap();
        let a = mixer.load_wave("a", 22_050, 16, false).unwrap();
        let b = mixer.load_wave("b", 22_050, 16, false).unwrap();
        mixer.play(a, ch(0), PLAY_LOOPING).unwrap();
        mixer.play(b, ch(1), PLAY_LOOPING).unwrap();

        let mut out = [0i16; 2];
        mixer.render(&mut out);
        assert_eq!(out[0], 150);
    }

    #[test]
    fn stop_silences_the_channel() {
        let mut mixer = SoftwareMixer::with_source(bank(&[("lp", mono(&[10, 20], 22_050))]));
        mixer.initialize().unwrap();
        let slot = mixer.load_wave("lp", 22_050, 16, false).unwrap();
        mixer.play(slot, ch(2), PLAY_LOOPING).unwrap();
        mixer.stop(ch(2)).unwrap();
        assert!(!mixer.is_channel_busy(ch(2)));

        let mut out = [0i16; 4];
        mixer.render(&mut out);
        assert_eq!(out, [0, 0, 0, 0]);
    }

    #[test]
    fn a_registered_slot_with_no_payload_is_a_silent_no_op() {
        // No bank entry for the name: the load registers a slot, play is silent.
        let mut mixer = MapMixer::default();
        mixer.initialize().unwrap();
        let slot = mixer.load_wave("missing", 22_050, 16, false).unwrap();
        mixer.play(slot, ch(0), PLAY_LOOPING).unwrap();
        assert!(!mixer.is_channel_busy(ch(0)));
        let mut out = [0i16; 4];
        mixer.render(&mut out);
        assert_eq!(out, [0, 0, 0, 0]);
    }

    #[test]
    fn stereo_waves_keep_their_channels() {
        let mut mixer = SoftwareMixer::with_source(bank(&[(
            "st",
            PcmWave {
                format: WaveFormat::pcm(22_050, 16, true).unwrap(),
                samples: vec![10, 20], // one stereo frame: L=10, R=20
            },
        )]));
        mixer.initialize().unwrap();
        let slot = mixer.load_wave("st", 22_050, 16, true).unwrap();
        mixer.play(slot, ch(0), PLAY_LOOPING).unwrap();
        let mut out = [0i16; 2];
        mixer.render(&mut out);
        assert_eq!(out, [10, 20]);
    }
}
