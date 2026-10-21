//! SDL3 audio-device backend for the recovered mixer.
//!
//! This is the thin platform wrapper that turns the tested, host-agnostic
//! [`cmr_core::audio_render::SoftwareMixer`] into live sound: it opens a
//! 22050 Hz, stereo, signed-16-bit playback stream (the recovered output rate)
//! and, in the SDL audio callback, locks a shared mixer and pumps
//! [`SoftwareMixer::render`](cmr_core::audio_render::SoftwareMixer::render) into
//! the requested buffer. The mixing, decoding, and wave sourcing are already
//! covered by `cmr-core`/`cmr-formats` contract and unit tests; only this device
//! glue is runtime-only and therefore not unit-tested (it needs an audio
//! device, like the rest of the SDL host).
//!
//! The game drives the recovered audio subsystems (ambience, menu, engine,
//! pacenote, music) against the shared mixer handle returned by
//! [`AudioHost::mixer`]; the callback thread reads whatever those writes left.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use cmr_core::audio_mixer::{AudioMixer, MIXER_OUTPUT_RATE_HZ};
use cmr_core::audio_render::{FileWaveSource, SoftwareMixer};
use sdl3::AudioSubsystem;
use sdl3::Sdl;
use sdl3::audio::{AudioCallback, AudioFormat, AudioSpec, AudioStream, AudioStreamWithCallback};

use crate::SdlHostError;

/// Output channel count for the stereo device the mixer renders into.
const OUTPUT_CHANNELS: i32 = 2;

/// A mixer shared between the game thread (which issues load/play/control) and
/// the SDL audio callback thread (which renders). The mixer's `WaveSource` is a
/// [`FileWaveSource`] rooted at the user's original-media directory.
pub type SharedSoftwareMixer = Arc<Mutex<SoftwareMixer<FileWaveSource>>>;

/// The SDL audio callback: renders the shared mixer into the device buffer.
struct MixerCallback {
    mixer: SharedSoftwareMixer,
}

impl AudioCallback<i16> for MixerCallback {
    fn callback(&mut self, stream: &mut AudioStream, requested: i32) {
        // `requested` is the number of interleaved i16 samples SDL wants next.
        let len = requested.max(0) as usize;
        let mut buffer = vec![0i16; len];
        if let Ok(mut mixer) = self.mixer.lock() {
            mixer.render(&mut buffer);
        }
        // A short write underruns to silence rather than erroring the stream.
        let _ = stream.put_data_i16(&buffer);
    }
}

/// An open SDL audio device playing the shared software mixer.
///
/// Dropping it stops and closes the stream. The audio subsystem and stream are
/// kept alive for the host's lifetime.
pub struct AudioHost {
    mixer: SharedSoftwareMixer,
    _stream: AudioStreamWithCallback<MixerCallback>,
    _audio: AudioSubsystem,
}

impl AudioHost {
    /// Open the audio device and start the mixer callback. `source_root` is the
    /// user-selected original-media directory (`--source` / `CMR_ORIGINAL_DIR`);
    /// the mixer reads and decodes its WAVs in place.
    pub fn open(sdl: &Sdl, source_root: impl Into<PathBuf>) -> Result<Self, SdlHostError> {
        let audio = sdl
            .audio()
            .map_err(|error| SdlHostError::sdl("audio subsystem", error))?;

        let mut mixer = SoftwareMixer::with_source(FileWaveSource::new(source_root));
        // Establish the recovered 22050 Hz / 30-channel session up front.
        mixer
            .initialize()
            .map_err(|error| SdlHostError::sdl("audio session init", error))?;
        let mixer = Arc::new(Mutex::new(mixer));

        let spec = AudioSpec {
            freq: Some(MIXER_OUTPUT_RATE_HZ as i32),
            channels: Some(OUTPUT_CHANNELS),
            format: Some(AudioFormat::s16_sys()),
        };
        let stream = audio
            .open_playback_stream::<MixerCallback, i16>(
                &spec,
                MixerCallback {
                    mixer: mixer.clone(),
                },
            )
            .map_err(|error| SdlHostError::sdl("audio playback stream", error))?;
        stream
            .resume()
            .map_err(|error| SdlHostError::sdl("audio stream resume", error))?;

        Ok(Self {
            mixer,
            _stream: stream,
            _audio: audio,
        })
    }

    /// A clone of the shared mixer handle the game drives (load/play/control).
    pub fn mixer(&self) -> SharedSoftwareMixer {
        self.mixer.clone()
    }
}
