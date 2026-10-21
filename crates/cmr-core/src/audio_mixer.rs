//! Reconstruction-owned audio mixer contract.
//!
//! This module defines the compatibility interface for the game's sound-effect
//! mixer, recovered from the official-EU `Rally.exe` QSound/QMixer module
//! (`0x4957d0`-`0x495d10`). It is deliberately backend-agnostic: the recovered
//! QMixer-faithful path and any later portable backend (for example an SDL
//! audio device) both implement [`AudioMixer`] and are checked by the same
//! contract in [`crate::audio_mixer`] tests.
//!
//! Recovery order note (`AGENTS.md`): this is the *interface* definition derived
//! from recovered behavior. It does not itself play audio. The recovered
//! bookkeeping - the init guard, the fixed channel cap, the incrementing wave
//! slot table, and the per-channel "wave last played here" record - is modelled
//! by [`MixerSessionState`] so the recovered control flow is testable before any
//! host device exists.
//!
//! Observations (see evidence `rea.official-eu.audio-wave-path`,
//! `rea.official-eu.audio-mixer-controls`):
//! - Init builds a 22050 Hz, 30-channel QSound session and guards every later
//!   operation behind an init flag.
//! - Wave load builds a PCM `WAVEFORMATEX` from `(name, rate, bits, stereo)`,
//!   registers it, and stores the handle plus source name in a slot keyed by an
//!   incrementing counter.
//! - Play looks up a slot handle and, when present, records the wave against the
//!   target channel before issuing `QSWaveMixPlayEx`.
//! - Volume, frequency, and 3D position are per-channel; stop and a channel-busy
//!   predicate round out the control surface.
//!
//! Inferences: the 30-channel cap bounds simultaneous voices; panning is 3D
//! spatialization, not a scalar pan; the busy predicate is false when the
//! session is uninitialized.
//!
//! Unknowns (carried as documented gaps, not invented behavior): the QMixer unit
//! ranges/scales for volume and frequency, the coordinate space/units of the 3D
//! position, the meaning of the `dwFlags` play argument, the exact wave-slot
//! table capacity, and the contents of the per-load size accumulator.

/// Mixer output sample rate, from init field `+0x08 = 0x5622`.
pub const MIXER_OUTPUT_RATE_HZ: u32 = 22_050;

/// Maximum simultaneous mixing channels, from init field `+0x14 = 0x1e`.
pub const MIXER_CHANNEL_CAPACITY: u32 = 30;

/// A mixing channel index, validated against [`MIXER_CHANNEL_CAPACITY`].
///
/// The recovered code indexes a per-channel "wave last played here" table and
/// passes the raw channel to every `QSWaveMix*` control call. A channel outside
/// the recovered cap is not a value the recovered session can represent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChannelId(u32);

impl ChannelId {
    /// Construct a channel index, rejecting values at or beyond the cap.
    pub fn new(index: u32) -> Result<Self, MixerError> {
        if index < MIXER_CHANNEL_CAPACITY {
            Ok(Self(index))
        } else {
            Err(MixerError::ChannelOutOfRange {
                channel: index,
                capacity: MIXER_CHANNEL_CAPACITY,
            })
        }
    }

    /// The raw channel index.
    pub fn index(self) -> u32 {
        self.0
    }
}

/// A registered wave-slot handle, keyed by the recovered incrementing counter
/// (`0xa1f848`). Slot `0` is the first wave loaded in a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WaveSlotId(u32);

impl WaveSlotId {
    /// The slot's ordinal within the session's load sequence.
    pub fn ordinal(self) -> u32 {
        self.0
    }
}

/// A recovered PCM wave format, built by the load wrapper `0x4958e0` from
/// `(name, sample_rate, bits_per_sample, stereo)`.
///
/// Mirrors the `WAVEFORMATEX` the game assembles: `wFormatTag = 1` (PCM),
/// `nChannels = 1 + stereo`, and the block/byte-rate fields derived from them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaveFormat {
    /// `nChannels`: 1 (mono) or 2 (stereo).
    pub channels: u16,
    /// `nSamplesPerSec`.
    pub sample_rate: u32,
    /// `wBitsPerSample`.
    pub bits_per_sample: u16,
    /// `nBlockAlign`: bytes per sample frame.
    pub block_align: u16,
    /// `nAvgBytesPerSec`: `block_align * sample_rate`.
    pub avg_bytes_per_sec: u32,
}

impl WaveFormat {
    /// Build a PCM format exactly as the load wrapper does.
    ///
    /// Rejects a non-byte-aligned bit depth and a zero sample rate with a useful
    /// error, matching the project rule that malformed input is refused rather
    /// than silently coerced. The recovered callers only ever pass 8- or 16-bit
    /// depths, for which `block_align = channels * bits/8`.
    pub fn pcm(sample_rate: u32, bits_per_sample: u16, stereo: bool) -> Result<Self, MixerError> {
        if sample_rate == 0 {
            return Err(MixerError::InvalidWaveFormat {
                reason: "sample rate is zero",
            });
        }
        if bits_per_sample == 0 || !bits_per_sample.is_multiple_of(8) {
            return Err(MixerError::InvalidWaveFormat {
                reason: "bits per sample is not a non-zero multiple of 8",
            });
        }
        let channels: u16 = if stereo { 2 } else { 1 };
        let bytes_per_sample = bits_per_sample / 8;
        let block_align = channels * bytes_per_sample;
        let avg_bytes_per_sec = block_align as u32 * sample_rate;
        Ok(Self {
            channels,
            sample_rate,
            bits_per_sample,
            block_align,
            avg_bytes_per_sec,
        })
    }
}

/// A recovered 3D source position passed to `QSWaveMixSetPosition`.
///
/// The field order `{x, y, z}` is recovered; the coordinate space and units are
/// unknown, so the components are carried verbatim as signed integers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position3d {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

/// A single backend operation the recovered mixer control flow decides to issue.
///
/// [`MixerSessionState`] produces these from the recovered guards and
/// bookkeeping; an [`AudioMixer`] backend performs them. Separating the decision
/// from the device keeps the recovered logic testable without audio hardware,
/// the same way the frontend emits effects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MixerCommand {
    /// Initialize the session: 22050 Hz output, `MIXER_CHANNEL_CAPACITY` voices.
    Initialize {
        output_rate_hz: u32,
        channel_capacity: u32,
    },
    /// Register a wave (`QSWaveMixOpenWave`) into the given slot.
    OpenWave {
        slot: WaveSlotId,
        format: WaveFormat,
    },
    /// Play a registered slot on a channel (`QSWaveMixPlayEx`).
    Play {
        slot: WaveSlotId,
        channel: ChannelId,
        flags: u32,
    },
    /// Set per-channel volume (`QSWaveMixSetVolume`).
    SetVolume { channel: ChannelId, volume: i32 },
    /// Set per-channel playback frequency (`QSWaveMixSetFrequency`).
    SetFrequency { channel: ChannelId, frequency: u32 },
    /// Set per-channel 3D position (`QSWaveMixSetPosition`).
    SetPosition {
        channel: ChannelId,
        position: Position3d,
    },
    /// Stop a channel (`QSWaveMixStopChannel`).
    StopChannel { channel: ChannelId },
}

/// Errors from the recovered mixer contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MixerError {
    /// An operation was requested before the session was initialized. The
    /// recovered code gates every load/play/control call behind the init flag
    /// `0xa1f838`.
    NotInitialized,
    /// A channel index is at or beyond [`MIXER_CHANNEL_CAPACITY`].
    ChannelOutOfRange { channel: u32, capacity: u32 },
    /// A referenced wave slot was never registered in this session.
    UnknownSlot { slot: u32 },
    /// A wave format could not be built from the requested parameters.
    InvalidWaveFormat { reason: &'static str },
}

impl std::fmt::Display for MixerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MixerError::NotInitialized => write!(f, "audio mixer session is not initialized"),
            MixerError::ChannelOutOfRange { channel, capacity } => {
                write!(f, "channel {channel} is out of range (capacity {capacity})")
            }
            MixerError::UnknownSlot { slot } => write!(f, "wave slot {slot} is not registered"),
            MixerError::InvalidWaveFormat { reason } => {
                write!(f, "invalid wave format: {reason}")
            }
        }
    }
}

impl std::error::Error for MixerError {}

/// The compatibility interface for the sound-effect mixer.
///
/// The recovered QMixer path and any later portable backend implement this. The
/// method set is the recovered control surface; semantics are defined by the
/// shared contract test [`check_audio_mixer_contract`]. Volume, frequency, and
/// position units are passed through verbatim because their QMixer scales are
/// not yet recovered.
pub trait AudioMixer {
    type Error: std::fmt::Debug;

    /// Initialize a 22050 Hz, 30-channel session. Idempotent per the recovered
    /// init-flag guard: a second call is a no-op.
    fn initialize(&mut self) -> Result<(), Self::Error>;

    /// Register a wave and return its slot id. Fails if not initialized.
    fn load_wave(
        &mut self,
        name: &str,
        sample_rate: u32,
        bits_per_sample: u16,
        stereo: bool,
    ) -> Result<WaveSlotId, Self::Error>;

    /// Play a registered slot on a channel. A missing slot handle is a recovered
    /// no-op (the game guards on a non-null handle); an out-of-range channel is
    /// rejected.
    fn play(&mut self, slot: WaveSlotId, channel: ChannelId, flags: u32)
    -> Result<(), Self::Error>;

    fn set_volume(&mut self, channel: ChannelId, volume: i32) -> Result<(), Self::Error>;
    fn set_frequency(&mut self, channel: ChannelId, frequency: u32) -> Result<(), Self::Error>;
    fn set_position(&mut self, channel: ChannelId, position: Position3d)
    -> Result<(), Self::Error>;
    fn stop(&mut self, channel: ChannelId) -> Result<(), Self::Error>;

    /// Channel-busy predicate (`0x495d10`): true while the channel is still
    /// playing, false when done or when the session is uninitialized.
    fn is_channel_busy(&self, channel: ChannelId) -> bool;
}

/// Pure reconstruction-owned bookkeeping for a mixer session.
///
/// This reproduces the recovered state and guards without touching audio
/// hardware: the init flag, the incrementing wave-slot table (handle presence
/// plus source name), and the per-channel "wave last played here" record
/// (`0xa0bd88[channel]`). Each method returns the [`MixerCommand`] the recovered
/// code would issue, or the recovered no-op/error. A backend drives this and
/// executes the returned command.
#[derive(Debug, Clone, Default)]
pub struct MixerSessionState {
    initialized: bool,
    /// One entry per loaded wave, in load order. The source name is retained to
    /// mirror the recovered slot record; `registered` models a non-null handle.
    slots: Vec<WaveSlot>,
    /// The wave last played on each channel, mirroring `0xa0bd88[channel]`.
    /// `None` until a slot has played on that channel.
    channel_waves: [Option<WaveSlotId>; MIXER_CHANNEL_CAPACITY as usize],
}

/// A recovered wave-slot record: a registered handle plus the source name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaveSlot {
    pub name: String,
    pub format: WaveFormat,
    /// Models the stored `QSWaveMixOpenWave` handle; `false` would be a null
    /// handle (a load the recovered play path skips).
    pub registered: bool,
}

impl MixerSessionState {
    /// A fresh, uninitialized session.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the init-flag guard is set.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// The number of waves loaded so far (the recovered slot counter).
    pub fn slot_count(&self) -> u32 {
        self.slots.len() as u32
    }

    /// The wave last played on a channel, or `None`.
    pub fn channel_wave(&self, channel: ChannelId) -> Option<WaveSlotId> {
        self.channel_waves[channel.index() as usize]
    }

    /// Look up a loaded slot record.
    pub fn slot(&self, slot: WaveSlotId) -> Option<&WaveSlot> {
        self.slots.get(slot.ordinal() as usize)
    }

    /// Initialize the session. Idempotent: a repeat call returns the same
    /// command without clearing loaded slots, matching the recovered init-flag
    /// short-circuit.
    pub fn initialize(&mut self) -> MixerCommand {
        self.initialized = true;
        MixerCommand::Initialize {
            output_rate_hz: MIXER_OUTPUT_RATE_HZ,
            channel_capacity: MIXER_CHANNEL_CAPACITY,
        }
    }

    /// Load and register a wave, returning the new slot id and the open command.
    /// Guarded by the init flag.
    pub fn load_wave(
        &mut self,
        name: &str,
        sample_rate: u32,
        bits_per_sample: u16,
        stereo: bool,
    ) -> Result<(WaveSlotId, MixerCommand), MixerError> {
        if !self.initialized {
            return Err(MixerError::NotInitialized);
        }
        let format = WaveFormat::pcm(sample_rate, bits_per_sample, stereo)?;
        let slot = WaveSlotId(self.slots.len() as u32);
        self.slots.push(WaveSlot {
            name: name.to_owned(),
            format,
            registered: true,
        });
        Ok((slot, MixerCommand::OpenWave { slot, format }))
    }

    /// Play a slot on a channel. Guarded by the init flag. A slot with no
    /// registered handle is a recovered no-op (`Ok(None)`); an unknown slot
    /// ordinal is rejected.
    pub fn play(
        &mut self,
        slot: WaveSlotId,
        channel: ChannelId,
        flags: u32,
    ) -> Result<Option<MixerCommand>, MixerError> {
        if !self.initialized {
            return Err(MixerError::NotInitialized);
        }
        let record = self
            .slots
            .get(slot.ordinal() as usize)
            .ok_or(MixerError::UnknownSlot {
                slot: slot.ordinal(),
            })?;
        if !record.registered {
            return Ok(None);
        }
        self.channel_waves[channel.index() as usize] = Some(slot);
        Ok(Some(MixerCommand::Play {
            slot,
            channel,
            flags,
        }))
    }

    /// Set per-channel volume. Guarded by the init flag.
    pub fn set_volume(&self, channel: ChannelId, volume: i32) -> Result<MixerCommand, MixerError> {
        self.require_init()?;
        Ok(MixerCommand::SetVolume { channel, volume })
    }

    /// Set per-channel frequency. Guarded by the init flag.
    pub fn set_frequency(
        &self,
        channel: ChannelId,
        frequency: u32,
    ) -> Result<MixerCommand, MixerError> {
        self.require_init()?;
        Ok(MixerCommand::SetFrequency { channel, frequency })
    }

    /// Set per-channel 3D position. Guarded by the init flag.
    pub fn set_position(
        &self,
        channel: ChannelId,
        position: Position3d,
    ) -> Result<MixerCommand, MixerError> {
        self.require_init()?;
        Ok(MixerCommand::SetPosition { channel, position })
    }

    /// Stop a channel. Guarded by the init flag.
    pub fn stop(&self, channel: ChannelId) -> Result<MixerCommand, MixerError> {
        self.require_init()?;
        Ok(MixerCommand::StopChannel { channel })
    }

    fn require_init(&self) -> Result<(), MixerError> {
        if self.initialized {
            Ok(())
        } else {
            Err(MixerError::NotInitialized)
        }
    }
}

/// Shared contract check run against every [`AudioMixer`] implementation.
///
/// This encodes only recovered, backend-independent behavior: the init guard on
/// load/play/control, the fixed channel cap, the incrementing slot ids, the
/// missing-handle play no-op, and the uninitialized busy predicate. A later
/// portable backend must pass exactly this.
pub fn check_audio_mixer_contract<M: AudioMixer + Default>() -> Result<(), String> {
    let mut mixer = M::default();
    let channel = ChannelId::new(0).map_err(|e| format!("channel 0 rejected: {e}"))?;

    // Before init, the busy predicate is false and loads/plays/controls fail.
    if mixer.is_channel_busy(channel) {
        return Err("uninitialized session reported a busy channel".into());
    }
    if mixer
        .load_wave("pre", MIXER_OUTPUT_RATE_HZ, 16, false)
        .is_ok()
    {
        return Err("load before init was accepted".into());
    }

    mixer
        .initialize()
        .map_err(|e| format!("initialize failed: {e:?}"))?;

    // Slots are handed out in load order from zero.
    let slot0 = mixer
        .load_wave("engine", MIXER_OUTPUT_RATE_HZ, 16, false)
        .map_err(|e| format!("first load failed: {e:?}"))?;
    let slot1 = mixer
        .load_wave("skid", MIXER_OUTPUT_RATE_HZ, 8, true)
        .map_err(|e| format!("second load failed: {e:?}"))?;
    if slot0.ordinal() != 0 || slot1.ordinal() != 1 {
        return Err(format!(
            "slot ids not sequential: {} then {}",
            slot0.ordinal(),
            slot1.ordinal()
        ));
    }

    // A registered slot plays; the whole control surface accepts a valid channel.
    mixer
        .play(slot0, channel, 0)
        .map_err(|e| format!("play failed: {e:?}"))?;
    mixer
        .set_volume(channel, 0)
        .map_err(|e| format!("set_volume failed: {e:?}"))?;
    mixer
        .set_frequency(channel, MIXER_OUTPUT_RATE_HZ)
        .map_err(|e| format!("set_frequency failed: {e:?}"))?;
    mixer
        .set_position(channel, Position3d { x: 0, y: 0, z: 0 })
        .map_err(|e| format!("set_position failed: {e:?}"))?;
    mixer
        .stop(channel)
        .map_err(|e| format!("stop failed: {e:?}"))?;

    // The channel cap is enforced at the boundary.
    if ChannelId::new(MIXER_CHANNEL_CAPACITY).is_ok() {
        return Err("channel at capacity was accepted".into());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_ids_are_bounded_by_the_recovered_cap() {
        assert_eq!(ChannelId::new(0).unwrap().index(), 0);
        assert_eq!(ChannelId::new(29).unwrap().index(), 29);
        assert_eq!(
            ChannelId::new(30),
            Err(MixerError::ChannelOutOfRange {
                channel: 30,
                capacity: 30,
            })
        );
    }

    #[test]
    fn pcm_format_matches_the_recovered_waveformatex() {
        // Mono 16-bit at the mixer rate.
        let mono = WaveFormat::pcm(22_050, 16, false).unwrap();
        assert_eq!(mono.channels, 1);
        assert_eq!(mono.block_align, 2);
        assert_eq!(mono.avg_bytes_per_sec, 2 * 22_050);
        // Stereo 8-bit.
        let stereo = WaveFormat::pcm(11_025, 8, true).unwrap();
        assert_eq!(stereo.channels, 2);
        assert_eq!(stereo.block_align, 2);
        assert_eq!(stereo.avg_bytes_per_sec, 2 * 11_025);
    }

    #[test]
    fn pcm_format_rejects_malformed_parameters() {
        assert!(matches!(
            WaveFormat::pcm(0, 16, false),
            Err(MixerError::InvalidWaveFormat { .. })
        ));
        assert!(matches!(
            WaveFormat::pcm(22_050, 0, false),
            Err(MixerError::InvalidWaveFormat { .. })
        ));
        assert!(matches!(
            WaveFormat::pcm(22_050, 12, false),
            Err(MixerError::InvalidWaveFormat { .. })
        ));
    }

    #[test]
    fn operations_are_gated_behind_init() {
        let mut state = MixerSessionState::new();
        let channel = ChannelId::new(3).unwrap();
        assert_eq!(
            state.load_wave("x", 22_050, 16, false).unwrap_err(),
            MixerError::NotInitialized
        );
        assert_eq!(
            state.play(WaveSlotId(0), channel, 0).unwrap_err(),
            MixerError::NotInitialized
        );
        assert_eq!(
            state.set_volume(channel, 0).unwrap_err(),
            MixerError::NotInitialized
        );
        assert!(!state.is_initialized());
    }

    #[test]
    fn init_emits_the_recovered_session_parameters() {
        let mut state = MixerSessionState::new();
        assert_eq!(
            state.initialize(),
            MixerCommand::Initialize {
                output_rate_hz: 22_050,
                channel_capacity: 30,
            }
        );
        assert!(state.is_initialized());
    }

    #[test]
    fn init_is_idempotent_and_keeps_loaded_slots() {
        let mut state = MixerSessionState::new();
        state.initialize();
        state.load_wave("engine", 22_050, 16, false).unwrap();
        state.initialize();
        assert_eq!(state.slot_count(), 1);
    }

    #[test]
    fn loads_get_sequential_slots_and_retain_the_name() {
        let mut state = MixerSessionState::new();
        state.initialize();
        let (slot0, open0) = state.load_wave("engine", 22_050, 16, false).unwrap();
        let (slot1, _) = state.load_wave("skid", 22_050, 8, true).unwrap();
        assert_eq!(slot0.ordinal(), 0);
        assert_eq!(slot1.ordinal(), 1);
        assert_eq!(state.slot_count(), 2);
        assert_eq!(state.slot(slot0).unwrap().name, "engine");
        assert!(matches!(open0, MixerCommand::OpenWave { slot, .. } if slot == slot0));
    }

    #[test]
    fn play_records_the_wave_against_the_channel() {
        let mut state = MixerSessionState::new();
        state.initialize();
        let (slot, _) = state.load_wave("engine", 22_050, 16, false).unwrap();
        let channel = ChannelId::new(5).unwrap();
        assert_eq!(state.channel_wave(channel), None);
        let command = state.play(slot, channel, 0).unwrap();
        assert_eq!(
            command,
            Some(MixerCommand::Play {
                slot,
                channel,
                flags: 0,
            })
        );
        assert_eq!(state.channel_wave(channel), Some(slot));
    }

    #[test]
    fn play_rejects_an_unknown_slot() {
        let mut state = MixerSessionState::new();
        state.initialize();
        let channel = ChannelId::new(0).unwrap();
        assert_eq!(
            state.play(WaveSlotId(7), channel, 0).unwrap_err(),
            MixerError::UnknownSlot { slot: 7 }
        );
    }

    #[test]
    fn play_is_a_no_op_for_an_unregistered_handle() {
        let mut state = MixerSessionState::new();
        state.initialize();
        // Force a null-handle slot the way a failed OpenWave would leave one.
        state.slots.push(WaveSlot {
            name: "silent".into(),
            format: WaveFormat::pcm(22_050, 16, false).unwrap(),
            registered: false,
        });
        let channel = ChannelId::new(1).unwrap();
        assert_eq!(state.play(WaveSlotId(0), channel, 0).unwrap(), None);
        assert_eq!(state.channel_wave(channel), None);
    }

    #[test]
    fn control_commands_carry_the_channel_and_payload() {
        let mut state = MixerSessionState::new();
        state.initialize();
        let channel = ChannelId::new(2).unwrap();
        assert_eq!(
            state.set_volume(channel, -600).unwrap(),
            MixerCommand::SetVolume {
                channel,
                volume: -600,
            }
        );
        assert_eq!(
            state.set_frequency(channel, 44_100).unwrap(),
            MixerCommand::SetFrequency {
                channel,
                frequency: 44_100,
            }
        );
        let position = Position3d { x: 1, y: -2, z: 3 };
        assert_eq!(
            state.set_position(channel, position).unwrap(),
            MixerCommand::SetPosition { channel, position }
        );
        assert_eq!(
            state.stop(channel).unwrap(),
            MixerCommand::StopChannel { channel }
        );
    }

    /// A minimal in-memory backend proving the contract is satisfiable and
    /// exercising [`check_audio_mixer_contract`]. The recovered QMixer path and
    /// a later portable backend run this same check.
    #[derive(Default)]
    struct FakeMixer {
        state: MixerSessionState,
        busy: std::collections::BTreeSet<u32>,
    }

    impl AudioMixer for FakeMixer {
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
            Ok(slot)
        }

        fn play(
            &mut self,
            slot: WaveSlotId,
            channel: ChannelId,
            flags: u32,
        ) -> Result<(), Self::Error> {
            if self.state.play(slot, channel, flags)?.is_some() {
                self.busy.insert(channel.index());
            }
            Ok(())
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
            self.busy.remove(&channel.index());
            self.state.stop(channel).map(drop)
        }

        fn is_channel_busy(&self, channel: ChannelId) -> bool {
            self.busy.contains(&channel.index())
        }
    }

    #[test]
    fn fake_backend_satisfies_the_shared_contract() {
        check_audio_mixer_contract::<FakeMixer>().unwrap();
    }
}
