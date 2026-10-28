//! Reconstruction of the per-tick car engine sound.
//!
//! Three recovered routines:
//!
//! - the caller `FUN_00488d60(p)`: runs once per in-race FSM handler call (one
//!   fixed simulation tick), right after Mechanics and the 3D-audio listener
//!   update. It routes between the reset-countdown branch, the ignition branch
//!   and normal running, and builds the six-word input block for the tick;
//! - the tick `FUN_00487e00(in, p)`: the Slow/Mid/Top rev-band layers, the
//!   slot-7 chatter/anti-lag/gear-clunk state machine and the top-band
//!   blow-off;
//! - the whine `FUN_00488ab0(p, in)`: the speed-driven whine layer with its
//!   gear-change dip.
//!
//! The helpers are reproduced alongside: the front/rear emitter volumes
//! `FUN_0047fa10`/`FUN_0047fac0`, the positional value `FUN_0047f680`/
//! `FUN_0047f6b0`, the "helium" pitch offset `FUN_0047ec00`, the master-scaled
//! volume `FUN_0047eb70`, and the throttle gate slew `FUN_00488a60`. The global
//! `rand()` (`FUN_004cfac0`) is [`GameLcg`]. Evidence:
//! `rea.official-eu.engine-sound-tick`. All of this is a description of
//! decompiler pseudocode, not original source.
//!
//! The `F`/`R` layers are a **front/rear emitter** pair, not a stereo pair: `F`
//! layers (0, 2, 5 and the whine 4) use the front emitter's proximity and
//! position, `R` layers (1, 3, 6 and the chatter 7) the rear emitter's. The
//! signed `±20%` term the volume helpers apply is a front/rear balance taken
//! from the camera heading against the car's forward axis (inference from the
//! listener routine `FUN_0047ec20`).
//!
//! The rev-band and whine tables are static `.data` in the shipped image; they
//! are passed in as [`EngineAudioTables`] (extracted from the user's own copy,
//! never committed). The listener geometry (`DAT_005ff9cc/d0/d4/dc/ec`) is an
//! input ([`EngineListener`]); its producer `FUN_0047ec20` is reconstructed
//! separately.

use crate::audio_mixer::Position3d;
use crate::game_rng::GameLcg;

/// The car waves' native rate (`0x2b11`), written to a muted layer.
pub const NATIVE_PITCH: u32 = 0x2b11;
/// Twice the native rate (`0x5622`): the chatter, pops, clunk and blow-off.
pub const DOUBLE_PITCH: u32 = 0x5622;
/// `QSWaveMixPlayEx` loop count for "forever".
pub const LOOP_FOREVER: u32 = 0xffff_ffff;
/// `QSWaveMixPlayEx` loop count for a single play.
pub const PLAY_ONCE: u32 = 0;
/// The ignition loop's start offset (`FUN_00495a80` block word 6).
pub const IGNITION_LOOP_START: u32 = 0x640;
/// The chatter envelope's full scale and its static default (`DAT_004f8b44`).
pub const ENVELOPE_FULL: i32 = 0x7fff;
/// Lower clamp of the audio RPM input.
pub const AUDIO_RPM_MIN: i32 = 2000;
/// Upper clamp of the audio RPM input.
pub const AUDIO_RPM_MAX: i32 = 0x2134;
/// Upper clamp of the whine speed input.
pub const AUDIO_SPEED_MAX: i32 = 0x104;

/// Wave-slot offsets from the car's wave base `DAT_00829558`, in loader order.
pub mod wave {
    pub const FS: u32 = 0;
    pub const RS: u32 = 1;
    pub const FM: u32 = 2;
    pub const RM: u32 = 3;
    pub const WHINE: u32 = 4;
    pub const FT: u32 = 5;
    pub const RT: u32 = 6;
    pub const CHATTER: u32 = 7;
    pub const POPLOOP1: u32 = 8;
    pub const IGNITION: u32 = 12;
    pub const GEAR: u32 = 13;
}

fn qmul(a: i32, b: i32) -> i32 {
    ((i64::from(a) * i64::from(b)) >> 16) as i32
}

/// One rev band (or the whine band): `{count, lo, hi, pitch[count+1],
/// volume[count+1]}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevBandTable {
    pub count: i32,
    pub lo: i32,
    pub hi: i32,
    pub pitch: Vec<u16>,
    pub volume: Vec<u8>,
}

/// The band a table lookup failed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineBand {
    Slow,
    Mid,
    Top,
    Whine,
}

/// Rejected engine tables or an index the tables cannot serve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineSoundError {
    /// A band's header is unusable (`hi <= lo` or a negative count).
    BadBandHeader {
        band: EngineBand,
        lo: i32,
        hi: i32,
        count: i32,
    },
    /// A band's arrays do not hold `count + 1` entries.
    BadBandLength {
        band: EngineBand,
        count: i32,
        pitch: usize,
        volume: usize,
    },
    /// The recovered arithmetic produced an index outside the band's arrays.
    /// The shipped code would read neighbouring `.data`; the reconstruction
    /// reports it instead.
    IndexOutOfTable { band: EngineBand, index: i32 },
}

impl std::fmt::Display for EngineSoundError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadBandHeader {
                band,
                lo,
                hi,
                count,
            } => {
                write!(
                    f,
                    "{band:?} band header lo={lo} hi={hi} count={count} is unusable"
                )
            }
            Self::BadBandLength {
                band,
                count,
                pitch,
                volume,
            } => write!(
                f,
                "{band:?} band count {count} needs {} entries, has pitch {pitch} volume {volume}",
                count + 1
            ),
            Self::IndexOutOfTable { band, index } => {
                write!(f, "{band:?} band index {index} is outside the table")
            }
        }
    }
}

impl std::error::Error for EngineSoundError {}

impl RevBandTable {
    fn validate(&self, band: EngineBand) -> Result<(), EngineSoundError> {
        if self.hi <= self.lo || self.count < 0 {
            return Err(EngineSoundError::BadBandHeader {
                band,
                lo: self.lo,
                hi: self.hi,
                count: self.count,
            });
        }
        let entries = self.count as usize + 1;
        if self.pitch.len() != entries || self.volume.len() != entries {
            return Err(EngineSoundError::BadBandLength {
                band,
                count: self.count,
                pitch: self.pitch.len(),
                volume: self.volume.len(),
            });
        }
        Ok(())
    }

    /// The recovered index `((x - lo) * count) / (hi - lo)` for `lo <= x <= hi`,
    /// else `-1`.
    pub fn index(&self, x: i32) -> i32 {
        if x < self.lo || x > self.hi {
            -1
        } else {
            (x - self.lo).wrapping_mul(self.count) / (self.hi - self.lo)
        }
    }

    fn pitch_at(&self, band: EngineBand, index: i32) -> Result<u16, EngineSoundError> {
        usize::try_from(index)
            .ok()
            .and_then(|i| self.pitch.get(i).copied())
            .ok_or(EngineSoundError::IndexOutOfTable { band, index })
    }

    fn volume_at(&self, band: EngineBand, index: i32) -> Result<i32, EngineSoundError> {
        usize::try_from(index)
            .ok()
            .and_then(|i| self.volume.get(i).copied())
            .map(i32::from)
            .ok_or(EngineSoundError::IndexOutOfTable { band, index })
    }
}

/// The static engine-sound tables shared by every car.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineAudioTables {
    /// `0x4f82e0`: volume only; its pitch array is never read.
    pub slow: RevBandTable,
    /// `0x4f8330`.
    pub mid: RevBandTable,
    /// `0x4f84c0`.
    pub top: RevBandTable,
    /// `0x4f8650`, indexed by road speed.
    pub whine: RevBandTable,
    /// `0x4f8b4c`: the upshift whine-dip sequence (entry 0 is never read).
    pub upshift_dip: [i32; 24],
}

impl EngineAudioTables {
    /// Accept tables whose headers and array lengths are consistent.
    pub fn new(
        slow: RevBandTable,
        mid: RevBandTable,
        top: RevBandTable,
        whine: RevBandTable,
        upshift_dip: [i32; 24],
    ) -> Result<Self, EngineSoundError> {
        slow.validate(EngineBand::Slow)?;
        mid.validate(EngineBand::Mid)?;
        top.validate(EngineBand::Top)?;
        whine.validate(EngineBand::Whine)?;
        Ok(Self {
            slow,
            mid,
            top,
            whine,
            upshift_dip,
        })
    }

    /// `pitchT[index]`, including the unguarded `-1` read of the anti-lag
    /// branch: index `-1` reads the u16 just before the array, which is the
    /// high half of the Top header's `hi` word.
    fn top_pitch_unguarded(&self, index: i32) -> Result<u16, EngineSoundError> {
        if index == -1 {
            Ok((self.top.hi as u32 >> 16) as u16)
        } else {
            self.top.pitch_at(EngineBand::Top, index)
        }
    }
}

/// The per-player 3D-audio listener words `FUN_0047ec20` writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EngineListener {
    /// `DAT_005ff9cc`: Q16 dot of the camera heading with the car forward.
    pub heading_dot_9cc: i32,
    /// `DAT_005ff9dc[p]`: front emitter angle (`±0x400`).
    pub front_angle_9dc: i16,
    /// `DAT_005ff9d0[p]`: rear emitter angle.
    pub rear_angle_9d0: i16,
    /// `DAT_005ff9ec[p]`: front emitter proximity `0..=1000`.
    pub front_proximity_9ec: i32,
    /// `DAT_005ff9d4[p]`: rear emitter proximity.
    pub rear_proximity_9d4: i32,
}

/// The mixer operations the engine sound issues. Channels and waves are the
/// absolute ids (`DAT_0082955c + p*8 + n`, `DAT_00829558 + n`).
pub trait EngineVoices {
    /// `FUN_00495d10`: the channel is still playing.
    fn playing(&self, channel: u32) -> bool;
    /// `FUN_00495a30`: play a wave with a loop count.
    fn play(&mut self, wave: u32, channel: u32, loops: u32);
    /// `FUN_00495a80`: play forever, looping back to `loop_start` samples.
    fn play_with_loop_start(&mut self, wave: u32, channel: u32, loop_start: u32);
    /// `FUN_00495b10`.
    fn stop(&mut self, channel: u32);
    /// `FUN_00495b70` (raw, already master-scaled where the game scales it).
    fn set_volume(&mut self, channel: u32, volume: i32);
    /// `FUN_00495bd0`.
    fn set_frequency(&mut self, channel: u32, frequency: u32);
    /// `FUN_00495c30`.
    fn set_position(&mut self, channel: u32, position: Position3d);
}

/// Per-stage settings the loader and options provide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineSoundConfig {
    /// `DAT_0082955c`: the first engine channel.
    pub channel_base: u32,
    /// `DAT_00829558`: the car's first wave slot.
    pub wave_base: u32,
    /// `DAT_00829540`: the anti-lag family flag the loader derives from the car.
    pub antilag_family_540: bool,
    /// `cfg[0xb6 + p*0x9c] & 0xf`: the car nibble, selecting the RT loop start.
    pub car_nibble: u8,
    /// `cfg[0x25] & 4`: the pitch is raised by 2000.
    pub helium: bool,
    /// `DAT_004f653c`: the 0..100 effects master volume.
    pub master_percent: i32,
}

/// `FUN_004872b0`'s family switch: car nibbles `{1,2,5,7,0xb}` are the
/// anti-lag family. In front-end/replay mode (`DAT_00d3ca88 != 0`) the loader
/// forces the flag on (and the nibble to 2) instead.
pub fn antilag_family_for_nibble(nibble: u8) -> bool {
    matches!(nibble & 0xf, 1 | 2 | 5 | 7 | 0xb)
}

/// The car- and control-record words the caller reads for one player.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EngineSoundCar {
    /// `ctl+0x2e8`: smoothed engine speed.
    pub engine_speed_2e8: i32,
    /// `ctl+0x160`: the guarded reciprocal `2^32 / ctl+0x15c`.
    pub redline_reciprocal_160: i32,
    /// `car+0xec`: road speed.
    pub speed_ec: i32,
    /// `ctl+0x43a`: gear (signed byte).
    pub gear_43a: i8,
    /// `ctl+0x2d8`: throttle response (Q16).
    pub throttle_response_2d8: i32,
    /// `ctl+0x304`: raw throttle pedal.
    pub throttle_pedal_304: i32,
    /// `car+0x75c` word: the out-of-bounds countdown.
    pub reset_timer_75c: i16,
    /// `car+0x6e8`.
    pub idle_6e8: i32,
    /// `car+0x6ec` word: the idle/ignition countdown (the reset branch writes it).
    pub idle_countdown_6ec: i16,
    /// `car+0x764`: keeps the ignition cranking during the reset branch.
    pub ignition_764: i32,
}

/// The caller's six-word input block for the tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EngineSoundInput {
    /// Synthetic 2000..=0x2134 "RPM".
    pub rpm: i32,
    /// Road speed, 0..=0x104 (whine index).
    pub speed: i32,
    /// Gear; the tick rewrites values outside `0..=6` to 1.
    pub gear: i32,
    /// Throttle 0 or 100.
    pub throttle: i32,
    /// Whine mute requests (`in[4]`, `in[5]`), always 0 from this caller.
    pub mute: [i32; 2],
}

/// The caller's input block formulas.
pub fn engine_sound_input(car: &EngineSoundCar) -> EngineSoundInput {
    let rev = qmul(car.engine_speed_2e8, car.redline_reciprocal_160);
    let rpm = (qmul(rev, 0x1964_0000) >> 16) + AUDIO_RPM_MIN;
    let rpm = rpm.clamp(AUDIO_RPM_MIN, AUDIO_RPM_MAX);

    let hud = qmul(car.speed_ec, 0x43_1168);
    let ratio = ((i64::from(hud) << 16) / 0x9ef9) as i32;
    // Truncate toward zero, then clamp low before high.
    let speed = ((ratio + ((ratio >> 31) & 0xffff)) >> 16).clamp(0, AUDIO_SPEED_MAX);

    let throttle = qmul(car.throttle_response_2d8, 0x64_0000);
    let throttle = if throttle < 0x32 {
        0
    } else {
        throttle.min(100)
    };

    EngineSoundInput {
        rpm,
        speed,
        gear: i32::from(car.gear_43a),
        throttle,
        mute: [0, 0],
    }
}

/// Per-player BSS words of the tick (not reset by the stage init).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerEngineState {
    pub gear_clunk_534: i32,
    pub prev_pedal_560: i32,
    pub steps_568: i32,
    pub hold_570: i32,
    /// Written by the pulsed overrun, never read.
    pub unused_578: i32,
    pub blowoff_580: i32,
    pub blowoff_gear_588: i32,
    pub blowoff_rpm_590: i32,
    pub overrun_598: i32,
    pub antilag_5a0: i32,
    pub antilag_fired_5a8: i32,
    pub prev_gear_5b0: u8,
    /// `DAT_004f8b44[p]`: the chatter envelope.
    pub envelope_b44: i32,
    /// `DAT_004f8bac/bbc[p]`: the caller's RPM slews, never consumed.
    pub slew_bac: i32,
    pub slew_bbc: i32,
}

impl PlayerEngineState {
    /// Zero BSS plus the envelope's static default.
    pub const INITIAL: Self = Self {
        gear_clunk_534: 0,
        prev_pedal_560: 0,
        steps_568: 0,
        hold_570: 0,
        unused_578: 0,
        blowoff_580: 0,
        blowoff_gear_588: 0,
        blowoff_rpm_590: 0,
        overrun_598: 0,
        antilag_5a0: 0,
        antilag_fired_5a8: 0,
        prev_gear_5b0: 0,
        envelope_b44: ENVELOPE_FULL,
        slew_bac: 0,
        slew_bbc: 0,
    };
}

/// The engine-sound globals. The gate, dip and whine words are single globals
/// shared by both players (a split-screen quirk kept as recovered).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineSoundState {
    pub players: [PlayerEngineState; 2],
    pub gate_544: i32,
    pub gate_548: i32,
    pub dip_54c: i32,
    pub prev_gear_52c: i32,
    pub dip_sequence_530: i32,
    pub whine_ramp_53c: i32,
    pub whine_last_5b4: i32,
    pub camera_550: i32,
    pub ignition_5b8: i32,
}

impl Default for EngineSoundState {
    fn default() -> Self {
        Self::new()
    }
}

/// What the caller decided this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineSoundBranch {
    /// `car+0x75c != 0`: layers muted, ignition countdown on channel 7.
    ResetCountdown,
    /// `car+0x6e8 != 0 && car+0x6ec != 0`: ignition on channel 0.
    Ignition,
    /// Normal running: the tick ran.
    Running,
}

/// The caller's result: the branch and the (possibly rewritten) `car+0x6ec`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineSoundOutcome {
    pub branch: EngineSoundBranch,
    pub idle_countdown_6ec: i16,
}

struct Tick<'a, V: EngineVoices> {
    voices: &'a mut V,
    config: &'a EngineSoundConfig,
    listener: &'a EngineListener,
    interior: bool,
    player: u32,
}

impl<V: EngineVoices> Tick<'_, V> {
    fn ch(&self, n: u32) -> u32 {
        self.config.channel_base + self.player * 8 + n
    }

    fn wave(&self, n: u32) -> u32 {
        self.config.wave_base + n
    }

    fn playing(&self, n: u32) -> bool {
        self.voices.playing(self.ch(n))
    }

    /// `FUN_0047eb70`: master-scaled volume.
    fn volume(&mut self, n: u32, v: i32) {
        let scaled = self.config.master_percent.wrapping_mul(v) / 100;
        let ch = self.ch(n);
        self.voices.set_volume(ch, scaled);
    }

    /// `FUN_0047ec00(p, pitch) & 0xffff` then `FUN_00495bd0`.
    fn pitch(&mut self, n: u32, pitch: u32) {
        let pitch = if self.config.helium {
            pitch + 2000
        } else {
            pitch
        };
        let ch = self.ch(n);
        self.voices.set_frequency(ch, pitch & 0xffff);
    }

    fn raw_frequency(&mut self, n: u32, frequency: u32) {
        let ch = self.ch(n);
        self.voices.set_frequency(ch, frequency);
    }

    fn stop(&mut self, n: u32) {
        let ch = self.ch(n);
        self.voices.stop(ch);
    }

    fn play(&mut self, wave: u32, n: u32, loops: u32) {
        let (w, ch) = (self.wave(wave), self.ch(n));
        self.voices.play(w, ch, loops);
    }

    fn play_with_loop_start(&mut self, wave: u32, n: u32, loop_start: u32) {
        let (w, ch) = (self.wave(wave), self.ch(n));
        self.voices.play_with_loop_start(w, ch, loop_start);
    }

    fn front(&self, v: i32) -> i32 {
        emitter_volume(self.listener, self.interior, v, Emitter::Front)
    }

    fn rear(&self, v: i32) -> i32 {
        emitter_volume(self.listener, self.interior, v, Emitter::Rear)
    }

    fn position(&mut self, n: u32, x: i32) {
        let ch = self.ch(n);
        self.voices.set_position(ch, Position3d { x, y: 0, z: 0 });
    }
}

/// The overrun branches' pop: one of `POPLOOP1..4` once on an idle channel 7.
fn start_overrun_pop<V: EngineVoices>(t: &mut Tick<'_, V>, rng: &mut GameLcg) {
    if !t.playing(7) {
        let pop = wave::POPLOOP1 + (rng.next_raw() & 3);
        t.play(pop, 7, PLAY_ONCE);
    }
}

/// Which emitter a layer belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Emitter {
    Front,
    Rear,
}

/// `FUN_00434d00`: camera modes 1, 2 (cockpit) and 3 (showroom) are interior.
pub fn interior_camera(camera_mode: i32) -> bool {
    matches!(camera_mode, 1..=3)
}

/// `FUN_0047f680`/`FUN_0047f6b0`: the emitter's positional value, 0 inside the
/// car, else `(angle * 0x1680) >> 16` (±90).
pub fn emitter_position(listener: &EngineListener, interior: bool, emitter: Emitter) -> i32 {
    if interior {
        return 0;
    }
    let angle = match emitter {
        Emitter::Front => listener.front_angle_9dc,
        Emitter::Rear => listener.rear_angle_9d0,
    };
    (i32::from(angle) * 0x1680) >> 16
}

/// `FUN_0047fa10` (front) / `FUN_0047fac0` (rear).
pub fn emitter_volume(listener: &EngineListener, interior: bool, v: i32, emitter: Emitter) -> i32 {
    let proximity = match emitter {
        Emitter::Front => listener.front_proximity_9ec,
        Emitter::Rear => listener.rear_proximity_9d4,
    };
    let v1 = if proximity < 0x14e {
        v / 3
    } else {
        proximity.wrapping_mul(v) / 1000
    };
    if interior {
        return v1;
    }
    let t = v1.wrapping_mul(20) / 100;
    // ftol(t * 65536.0): exact in range, the x87 indefinite integer outside it.
    let f = i32::try_from(i64::from(t) << 16).unwrap_or(i32::MIN);
    let balance = qmul(listener.heading_dot_9cc, f) >> 16;
    let v2 = match emitter {
        Emitter::Front => v1.wrapping_sub(balance),
        Emitter::Rear => v1.wrapping_add(balance),
    };
    v2.clamp(0, 0x7fff)
}

/// The RT layer's loop start (`FUN_00495a80` block word 6), a code immediate
/// the tick selects by the car nibble.
pub fn rear_top_loop_start(car_nibble: u8) -> u32 {
    match car_nibble & 0xf {
        1 | 7 => 0x9b4b,
        2 | 0xb => 0x7d51,
        6 | 0xa => 0x8d13,
        _ => 0xa065,
    }
}

fn slew_500(state: &mut i32, target: i32) {
    let diff = target.wrapping_sub(*state);
    if diff.unsigned_abs() > 500 {
        *state += if target < *state { -500 } else { 500 };
    } else {
        *state = target;
    }
}

impl EngineSoundState {
    /// BSS plus static defaults, before any stage load.
    pub const fn new() -> Self {
        Self {
            players: [PlayerEngineState::INITIAL; 2],
            gate_544: 0,
            gate_548: 0,
            dip_54c: 0,
            prev_gear_52c: -1,
            dip_sequence_530: 0,
            whine_ramp_53c: 0,
            whine_last_5b4: 0,
            camera_550: 0,
            ignition_5b8: 0,
        }
    }

    /// `FUN_00487250`, run at the end of the car sound loader: reset the shared
    /// words and set each active player's layer volumes (`FUN_00487d10`).
    /// The per-player words are left as they were.
    pub fn stage_init<V: EngineVoices>(
        &mut self,
        voices: &mut V,
        config: &EngineSoundConfig,
        two_players: bool,
    ) {
        self.gate_544 = 0;
        self.gate_548 = 0;
        self.dip_54c = 0;
        self.prev_gear_52c = -1;
        self.dip_sequence_530 = 0;
        self.whine_ramp_53c = 0;
        let listener = EngineListener::default();
        for player in 0..if two_players { 2 } else { 1 } {
            let mut tick = Tick {
                voices,
                config,
                listener: &listener,
                interior: false,
                player,
            };
            tick.volume(0, 0x7fff);
            for n in [1, 2, 3, 5, 6, 4] {
                tick.volume(n, 0);
            }
        }
    }

    /// `FUN_00488a60`: slew the shared throttle gate (+20 up, -10 down).
    pub fn slew_gate(&mut self, target: i32) {
        let g = self.gate_544;
        let g = if target - g > 20 {
            g + 20
        } else if g - target > 10 {
            g - 10
        } else {
            target
        };
        self.gate_544 = g;
        self.gate_548 = g;
    }

    /// The caller `FUN_00488d60(p)`.
    #[allow(clippy::too_many_arguments)]
    pub fn update<V: EngineVoices>(
        &mut self,
        voices: &mut V,
        config: &EngineSoundConfig,
        tables: &EngineAudioTables,
        rng: &mut GameLcg,
        player: u32,
        car: &EngineSoundCar,
        camera_mode: i32,
        listener: &EngineListener,
    ) -> Result<EngineSoundOutcome, EngineSoundError> {
        let mut tick = Tick {
            voices,
            config,
            listener,
            interior: interior_camera(camera_mode),
            player,
        };
        let mut countdown = car.idle_countdown_6ec;

        if car.reset_timer_75c != 0 {
            // FUN_00487210(ch, 0): volume 0 on each playing layer.
            for n in [0, 1, 2, 3, 5, 6, 4] {
                if tick.playing(n) {
                    tick.volume(n, 0);
                }
            }
            if countdown == 0 {
                if car.ignition_764 != 0 {
                    tick.play_with_loop_start(wave::IGNITION, 7, IGNITION_LOOP_START);
                    let v = tick.front(0x7fff);
                    tick.volume(7, v);
                    countdown = 0xd;
                }
            } else {
                countdown -= 1;
                if countdown == 0 {
                    if car.ignition_764 == 0 {
                        tick.stop(7);
                    } else {
                        countdown = 0xd;
                    }
                }
            }
            return Ok(EngineSoundOutcome {
                branch: EngineSoundBranch::ResetCountdown,
                idle_countdown_6ec: countdown,
            });
        }

        if car.idle_6e8 != 0 && countdown != 0 {
            if countdown == 0x17 || (countdown == 0xd && self.ignition_5b8 == 0) {
                if tick.playing(0) {
                    self.ignition_5b8 = 0;
                    tick.stop(0);
                }
            } else if !tick.playing(0) {
                self.ignition_5b8 = 1;
                tick.play_with_loop_start(wave::IGNITION, 0, IGNITION_LOOP_START);
                let v = tick.front(0x7fff);
                tick.volume(0, v);
            }
            return Ok(EngineSoundOutcome {
                branch: EngineSoundBranch::Ignition,
                idle_countdown_6ec: countdown,
            });
        }

        if self.ignition_5b8 != 0 {
            self.ignition_5b8 = 0;
            if tick.playing(0) {
                tick.stop(0);
            }
        }
        if !tick.playing(0) {
            for n in [
                wave::FS,
                wave::RS,
                wave::FM,
                wave::RM,
                wave::FT,
                wave::WHINE,
            ] {
                tick.play(n, n, LOOP_FOREVER);
            }
        }
        let mut input = engine_sound_input(car);
        let p = player as usize;
        slew_500(&mut self.players[p].slew_bbc, input.rpm);
        slew_500(&mut self.players[p].slew_bac, input.rpm);
        self.camera_550 = camera_mode;
        self.tick(&mut tick, tables, rng, &mut input, car.throttle_pedal_304)?;
        Ok(EngineSoundOutcome {
            branch: EngineSoundBranch::Running,
            idle_countdown_6ec: countdown,
        })
    }

    fn chase_camera(&self) -> bool {
        matches!(self.camera_550, 5 | 6)
    }

    /// The tick `FUN_00487e00`.
    fn tick<V: EngineVoices>(
        &mut self,
        t: &mut Tick<'_, V>,
        tables: &EngineAudioTables,
        rng: &mut GameLcg,
        input: &mut EngineSoundInput,
        pedal: i32,
    ) -> Result<(), EngineSoundError> {
        let p = t.player as usize;
        let front = emitter_position(t.listener, t.interior, Emitter::Front);
        let rear = emitter_position(t.listener, t.interior, Emitter::Rear);
        for n in [0, 2, 5, 4] {
            t.position(n, front);
        }
        for n in [1, 3, 6, 7] {
            t.position(n, rear);
        }

        let rpm = input.rpm;
        let gear = input.gear;
        if (gear == 1 || gear == 7) && i32::from(self.players[p].prev_gear_5b0) != gear {
            self.players[p].gear_clunk_534 = 1;
        }
        self.players[p].prev_gear_5b0 = gear as u8;
        if !(0..=6).contains(&gear) {
            input.gear = 1;
        }
        let family = t.config.antilag_family_540;
        if self.dip_54c != 0 {
            self.dip_54c -= 1;
        }
        let me = &mut self.players[p];
        if input.gear > 1 && !family {
            me.blowoff_gear_588 = 1;
        }
        if rpm > 5000 && !family {
            me.blowoff_rpm_590 = 1;
        }
        if rpm > 0x1194 {
            if family {
                if pedal != 0 {
                    me.antilag_5a0 = 1;
                    me.antilag_fired_5a8 = 0;
                }
            } else {
                me.overrun_598 = 1;
                if input.throttle != 0 {
                    me.envelope_b44 = ENVELOPE_FULL;
                    me.unused_578 = 0;
                    me.steps_568 = 0;
                    me.hold_570 = 0;
                }
            }
        }

        let i_s = tables.slow.index(rpm);
        let i_m = tables.mid.index(rpm);
        let i_t = tables.top.index(rpm);

        // Slow: volume only.
        if i_s == -1 {
            for n in [0, 1] {
                if t.playing(n) {
                    t.volume(n, 0);
                }
            }
        } else {
            let v = tables.slow.volume_at(EngineBand::Slow, i_s)? * 100;
            if t.playing(0) {
                let fv = t.front(v);
                t.volume(0, fv);
            }
            if t.playing(1) {
                let rv = t.rear(v);
                t.volume(1, rv);
            }
        }

        // Mid.
        if i_m == -1 {
            for n in [2, 3] {
                if t.playing(n) {
                    t.raw_frequency(n, NATIVE_PITCH);
                    t.volume(n, 0);
                }
            }
        } else {
            let pitch = u32::from(tables.mid.pitch_at(EngineBand::Mid, i_m)?);
            let v = tables.mid.volume_at(EngineBand::Mid, i_m)? * 100;
            if t.playing(2) {
                t.pitch(2, pitch);
                let fv = t.front(v);
                t.volume(2, fv);
            }
            if t.playing(3) {
                t.pitch(3, pitch);
                let rv = t.rear(v);
                t.volume(3, rv);
            }
        }

        self.slew_gate(input.throttle);
        self.chatter(t, tables, rng, input, rpm, i_t, pedal)?;
        self.top_band(t, tables, i_t)?;
        self.whine(t, tables, rng, input)?;
        self.players[p].prev_pedal_560 = pedal;
        Ok(())
    }

    /// Slot 7: overrun pops, gear clunk and anti-lag.
    #[allow(clippy::too_many_arguments)]
    fn chatter<V: EngineVoices>(
        &mut self,
        t: &mut Tick<'_, V>,
        tables: &EngineAudioTables,
        rng: &mut GameLcg,
        input: &EngineSoundInput,
        rpm: i32,
        i_t: i32,
        pedal: i32,
    ) -> Result<(), EngineSoundError> {
        let p = t.player as usize;
        let chase = self.chase_camera();
        if input.throttle == 0 && self.players[p].overrun_598 != 0 {
            if rpm < 0x157d {
                if rpm < 0x1195 {
                    self.players[p].overrun_598 = 0;
                    return Ok(());
                }
                start_overrun_pop(t, rng);
                let me = &mut self.players[p];
                let e = me.envelope_b44;
                if e == ENVELOPE_FULL {
                    if me.steps_568 == 0 {
                        me.hold_570 = (rng.next_raw() % 5) as i32 + 1;
                        me.steps_568 = (rng.next_raw() % 5) as i32 + 1;
                        me.unused_578 = (rng.next_raw() % 3) as i32 + 1;
                    } else {
                        me.envelope_b44 = ENVELOPE_FULL - ENVELOPE_FULL / me.steps_568;
                    }
                } else if e == 0 {
                    me.steps_568 = 0;
                    if me.hold_570 == 0 {
                        me.envelope_b44 = ENVELOPE_FULL;
                    } else {
                        me.hold_570 -= 1;
                    }
                } else if me.steps_568 == 0 {
                    me.envelope_b44 = 0;
                } else {
                    me.envelope_b44 = e - ENVELOPE_FULL / me.steps_568;
                    if me.envelope_b44 < 0 {
                        me.envelope_b44 = 0;
                    }
                }
                let e = me.envelope_b44;
                t.pitch(7, DOUBLE_PITCH);
                let rv = t.rear(e);
                t.volume(7, rv);
            } else {
                start_overrun_pop(t, rng);
                t.pitch(7, DOUBLE_PITCH);
                let e = self.players[p].envelope_b44;
                let e = if chase { e } else { e * 0x50 / 100 };
                let rv = t.rear(e);
                t.volume(7, rv);
            }
            return Ok(());
        }

        if self.players[p].gear_clunk_534 != 0 {
            if t.playing(7) {
                t.stop(7);
            }
            self.players[p].gear_clunk_534 = 0;
            t.pitch(7, DOUBLE_PITCH);
            t.position(7, 0);
            let fv = t.front(10000);
            t.volume(7, fv);
            t.play(wave::GEAR, 7, PLAY_ONCE);
            return Ok(());
        }

        if self.players[p].antilag_5a0 != 0 {
            if pedal < self.players[p].prev_pedal_560 || pedal == 0 {
                if self.players[p].antilag_fired_5a8 == 0 && !t.playing(7) {
                    let pop = wave::CHATTER + rng.next_raw() % 5;
                    t.play(pop, 7, PLAY_ONCE);
                    self.players[p].antilag_fired_5a8 = 1;
                }
                let rv = t.rear(10000);
                t.volume(7, rv);
                let pitch = u32::from(tables.top_pitch_unguarded(i_t)?);
                t.pitch(7, pitch);
            } else if t.playing(7) {
                let ch = t.ch(7);
                t.voices.set_volume(ch, 0);
                t.stop(7);
                self.players[p].antilag_5a0 = 0;
            }
        }
        Ok(())
    }

    /// The Top band (slots 5/6) and the blow-off.
    fn top_band<V: EngineVoices>(
        &mut self,
        t: &mut Tick<'_, V>,
        tables: &EngineAudioTables,
        i_t: i32,
    ) -> Result<(), EngineSoundError> {
        let p = t.player as usize;
        if i_t == -1 {
            if t.playing(5) {
                t.raw_frequency(5, NATIVE_PITCH);
                t.volume(5, 0);
                // Stops 6, not 5: channel 5 keeps looping at volume 0.
                t.stop(6);
            }
            if t.playing(6) {
                t.raw_frequency(6, NATIVE_PITCH);
                t.volume(6, 0);
                t.stop(6);
            }
            return Ok(());
        }

        if self.gate_548 == 0 {
            let me = self.players[p];
            if me.blowoff_580 == 0 {
                if t.playing(5) {
                    t.stop(5);
                }
                if me.blowoff_gear_588 != 0 && me.blowoff_rpm_590 != 0 {
                    t.pitch(5, DOUBLE_PITCH);
                    t.play(wave::CHATTER, 5, PLAY_ONCE);
                    let fv = t.front(if self.chase_camera() { 9000 } else { 10000 });
                    t.volume(5, fv);
                    let me = &mut self.players[p];
                    me.blowoff_580 = 1;
                    me.blowoff_gear_588 = 0;
                    me.blowoff_rpm_590 = 0;
                }
            }
            if t.playing(6) {
                t.stop(6);
            }
            return Ok(());
        }

        let mut v = tables.top.volume_at(EngineBand::Top, i_t)? * self.gate_548 / 100;
        if self.dip_54c != 0 {
            v = (v - 20).max(0);
        }
        if self.players[p].blowoff_580 != 0 {
            t.stop(5);
            self.players[p].blowoff_580 = 0;
        }
        let pitch = u32::from(tables.top.pitch_at(EngineBand::Top, i_t)?);
        if t.playing(5) {
            t.pitch(5, pitch);
            let fv = t.front(v * 100);
            t.volume(5, fv);
        } else {
            // The frequency and volume go to channel 6 before channel 5 starts.
            t.pitch(6, pitch);
            let rv = t.rear(v * 100);
            t.volume(6, rv);
            t.play(wave::FT, 5, LOOP_FOREVER);
        }
        t.pitch(6, pitch);
        let rv = t.rear(v * 100);
        t.volume(6, rv);
        if !t.playing(6) {
            let start = rear_top_loop_start(t.config.car_nibble);
            t.play_with_loop_start(wave::RT, 6, start);
        }
        Ok(())
    }

    /// The whine `FUN_00488ab0` (channel 4).
    fn whine<V: EngineVoices>(
        &mut self,
        t: &mut Tick<'_, V>,
        tables: &EngineAudioTables,
        rng: &mut GameLcg,
        input: &EngineSoundInput,
    ) -> Result<(), EngineSoundError> {
        let speed = input.speed;
        let gear = input.gear;
        let mute = input.mute[0] != 0 || input.mute[1] != 0;
        let mut dip = 0;
        if gear != self.prev_gear_52c {
            self.dip_54c = 2;
            if gear > self.prev_gear_52c {
                self.dip_sequence_530 = if rng.next_raw() > 0x5ffe { 0 } else { 0x19 };
            }
        }
        if gear > 0 && gear < 5 {
            self.dip_sequence_530 += 1;
            if self.dip_sequence_530 < 0x18 {
                dip = tables.upshift_dip[self.dip_sequence_530 as usize];
            }
        }
        self.prev_gear_52c = gear;

        let band = &tables.whine;
        if speed < band.hi && gear != 0 {
            self.whine_ramp_53c = if input.throttle == 100 {
                (self.whine_ramp_53c + 10).min(100)
            } else {
                0
            };
            let mut index = if speed < band.lo {
                -1
            } else {
                (speed - band.lo).wrapping_mul(band.count) / (band.hi - band.lo)
            };
            let audible = if gear < 5 {
                index -= index.wrapping_mul(dip) / 100;
                index > -2
            } else {
                dip = 0;
                true
            };
            if audible && index != -1 {
                if t.playing(4) {
                    let pitch = u32::from(band.pitch_at(EngineBand::Whine, index)?);
                    t.pitch(4, pitch);
                    if mute {
                        return Ok(());
                    }
                    let mut v = band.volume_at(EngineBand::Whine, index)? - dip;
                    if v < 0 {
                        v = 10;
                    }
                    let mut v = self.whine_ramp_53c * v / 100;
                    if self.dip_54c != 0 {
                        v = (v - 20).max(0);
                    }
                    let s = if self.chase_camera() {
                        v * 0x2d
                    } else {
                        v * 0x37
                    };
                    let fv = t.front(s << 1);
                    t.volume(4, fv);
                    self.whine_last_5b4 = v;
                } else if self.whine_last_5b4 != 0 {
                    // The volume write that follows is unreachable: channel 4
                    // was just found idle.
                    self.whine_last_5b4 = (self.whine_last_5b4 - 10).max(0);
                }
                return Ok(());
            }
        }
        if t.playing(4) {
            t.raw_frequency(4, NATIVE_PITCH);
            t.volume(4, 0);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Op {
        Play(u32, u32, u32),
        PlayFrom(u32, u32, u32),
        Stop(u32),
        Volume(u32, i32),
        Frequency(u32, u32),
        Position(u32, i32),
    }

    #[derive(Default)]
    struct Recorder {
        playing: BTreeSet<u32>,
        ops: Vec<Op>,
    }

    impl Recorder {
        fn non_position(&self) -> Vec<Op> {
            self.ops
                .iter()
                .filter(|op| !matches!(op, Op::Position(..)))
                .cloned()
                .collect()
        }
    }

    impl EngineVoices for Recorder {
        fn playing(&self, channel: u32) -> bool {
            self.playing.contains(&channel)
        }
        fn play(&mut self, wave: u32, channel: u32, loops: u32) {
            self.playing.insert(channel);
            self.ops.push(Op::Play(wave, channel, loops));
        }
        fn play_with_loop_start(&mut self, wave: u32, channel: u32, loop_start: u32) {
            self.playing.insert(channel);
            self.ops.push(Op::PlayFrom(wave, channel, loop_start));
        }
        fn stop(&mut self, channel: u32) {
            self.playing.remove(&channel);
            self.ops.push(Op::Stop(channel));
        }
        fn set_volume(&mut self, channel: u32, volume: i32) {
            self.ops.push(Op::Volume(channel, volume));
        }
        fn set_frequency(&mut self, channel: u32, frequency: u32) {
            self.ops.push(Op::Frequency(channel, frequency));
        }
        fn set_position(&mut self, channel: u32, position: Position3d) {
            self.ops.push(Op::Position(channel, position.x));
        }
    }

    fn band(count: i32, lo: i32, hi: i32, pitch0: u16, vol: u8) -> RevBandTable {
        let n = count as usize + 1;
        RevBandTable {
            count,
            lo,
            hi,
            pitch: (0..n).map(|i| pitch0 + i as u16).collect(),
            volume: vec![vol; n],
        }
    }

    /// Synthetic tables (not the shipped values).
    fn tables() -> EngineAudioTables {
        EngineAudioTables::new(
            band(10, 2000, 4000, 100, 50),
            band(10, 3000, 6000, 200, 60),
            band(10, 4000, 0x2134, 300, 70),
            band(20, 10, 210, 400, 80),
            [0; 24],
        )
        .unwrap()
    }

    fn config() -> EngineSoundConfig {
        EngineSoundConfig {
            channel_base: 10,
            wave_base: 100,
            antilag_family_540: false,
            car_nibble: 3,
            helium: false,
            master_percent: 100,
        }
    }

    /// A close proximity listener behind the car (no balance term).
    fn listener() -> EngineListener {
        EngineListener {
            heading_dot_9cc: 0,
            front_angle_9dc: 0,
            rear_angle_9d0: 0,
            front_proximity_9ec: 1000,
            rear_proximity_9d4: 1000,
        }
    }

    fn rpm_car(rpm: i32) -> EngineSoundCar {
        // rev fraction = (rpm - 2000) / 6500 in Q16 via engine_speed * 1.0.
        let fraction = ((i64::from(rpm - 2000) << 16) / 6500) as i32 + 1;
        EngineSoundCar {
            engine_speed_2e8: fraction,
            redline_reciprocal_160: 0x10000,
            ..EngineSoundCar::default()
        }
    }

    #[test]
    fn input_block_follows_the_caller_formulas() {
        let car = EngineSoundCar {
            engine_speed_2e8: 0x8000,
            redline_reciprocal_160: 0x10000,
            speed_ec: 0,
            gear_43a: -1,
            throttle_response_2d8: 1,
            ..EngineSoundCar::default()
        };
        let input = engine_sound_input(&car);
        assert_eq!(input.rpm, 2000 + 3250);
        assert_eq!(input.speed, 0);
        assert_eq!(input.gear, -1);
        // Any positive response is already 100 after the Q16 multiply.
        assert_eq!(input.throttle, 100);
        let off = engine_sound_input(&EngineSoundCar {
            throttle_response_2d8: 0,
            ..car
        });
        assert_eq!(off.throttle, 0);
        let full = engine_sound_input(&EngineSoundCar {
            throttle_response_2d8: 0x10000,
            engine_speed_2e8: 0x20000,
            ..car
        });
        assert_eq!(full.throttle, 100);
        assert_eq!(full.rpm, AUDIO_RPM_MAX);
        let low = engine_sound_input(&EngineSoundCar {
            engine_speed_2e8: -0x10000,
            ..car
        });
        assert_eq!(low.rpm, AUDIO_RPM_MIN);
    }

    #[test]
    fn speed_input_truncates_and_clamps() {
        // hud = qmul(speed, 0x431168); speed = hud / 0x9ef9 in Q16.
        let car = |speed_ec| EngineSoundCar {
            speed_ec,
            ..EngineSoundCar::default()
        };
        assert_eq!(engine_sound_input(&car(-0x100000)).speed, 0);
        assert_eq!(engine_sound_input(&car(0x7fff_ffff)).speed, AUDIO_SPEED_MAX);
        let hud = qmul(0x10000, 0x43_1168);
        let expect = (((i64::from(hud) << 16) / 0x9ef9) >> 16) as i32;
        assert_eq!(engine_sound_input(&car(0x10000)).speed, expect);
    }

    #[test]
    fn band_index_is_minus_one_outside_the_window() {
        let b = band(10, 2000, 4000, 0, 0);
        assert_eq!(b.index(1999), -1);
        assert_eq!(b.index(4001), -1);
        assert_eq!(b.index(2000), 0);
        assert_eq!(b.index(4000), 10);
        assert_eq!(b.index(3000), 5);
    }

    #[test]
    fn tables_reject_inconsistent_bands() {
        let mut short = band(10, 0, 10, 0, 0);
        short.volume.pop();
        let t = tables();
        assert!(matches!(
            EngineAudioTables::new(
                short,
                t.mid.clone(),
                t.top.clone(),
                t.whine.clone(),
                [0; 24]
            ),
            Err(EngineSoundError::BadBandLength {
                band: EngineBand::Slow,
                ..
            })
        ));
        assert!(matches!(
            EngineAudioTables::new(
                t.slow.clone(),
                band(1, 5, 5, 0, 0),
                t.top.clone(),
                t.whine.clone(),
                [0; 24]
            ),
            Err(EngineSoundError::BadBandHeader {
                band: EngineBand::Mid,
                ..
            })
        ));
    }

    #[test]
    fn top_pitch_minus_one_reads_the_header_high_half() {
        let mut t = tables();
        t.top.hi = 0x0003_2134;
        assert_eq!(t.top_pitch_unguarded(-1), Ok(3));
        assert_eq!(t.top_pitch_unguarded(0), Ok(300));
        assert!(t.top_pitch_unguarded(-2).is_err());
    }

    #[test]
    fn emitter_volume_divides_far_sources_by_three_and_clamps() {
        let mut l = listener();
        l.front_proximity_9ec = 0x14d;
        assert_eq!(emitter_volume(&l, true, 9000, Emitter::Front), 3000);
        l.front_proximity_9ec = 500;
        assert_eq!(emitter_volume(&l, true, 9000, Emitter::Front), 4500);
        // Balance: heading dot 1.0 removes 20% from F and adds it to R.
        l.heading_dot_9cc = 0x10000;
        l.rear_proximity_9d4 = 500;
        assert_eq!(emitter_volume(&l, false, 9000, Emitter::Front), 4500 - 900);
        assert_eq!(emitter_volume(&l, false, 9000, Emitter::Rear), 4500 + 900);
        // Clamped to 0..=0x7fff outside the car only.
        l.rear_proximity_9d4 = 1000;
        assert_eq!(emitter_volume(&l, false, 30000, Emitter::Rear), 0x7fff);
        assert_eq!(emitter_volume(&l, true, 40000, Emitter::Rear), 40000);
    }

    #[test]
    fn emitter_position_is_zero_inside_and_scaled_outside() {
        let mut l = listener();
        l.front_angle_9dc = 0x400;
        l.rear_angle_9d0 = -0x400;
        assert_eq!(emitter_position(&l, false, Emitter::Front), 90);
        assert_eq!(emitter_position(&l, false, Emitter::Rear), -90);
        assert_eq!(emitter_position(&l, true, Emitter::Front), 0);
        assert!(interior_camera(1) && interior_camera(3) && !interior_camera(5));
    }

    #[test]
    fn gate_slews_up_by_twenty_and_down_by_ten() {
        let mut s = EngineSoundState::new();
        s.slew_gate(100);
        assert_eq!(s.gate_548, 20);
        for _ in 0..4 {
            s.slew_gate(100);
        }
        assert_eq!(s.gate_548, 100);
        s.slew_gate(0);
        assert_eq!(s.gate_548, 90);
        s.gate_544 = 5;
        s.slew_gate(0);
        assert_eq!((s.gate_544, s.gate_548), (0, 0));
    }

    #[test]
    fn loop_start_and_family_follow_the_car_nibble() {
        assert_eq!(rear_top_loop_start(1), 0x9b4b);
        assert_eq!(rear_top_loop_start(0xb), 0x7d51);
        assert_eq!(rear_top_loop_start(0xa), 0x8d13);
        assert_eq!(rear_top_loop_start(0), 0xa065);
        assert!(antilag_family_for_nibble(5) && !antilag_family_for_nibble(3));
    }

    fn run(
        state: &mut EngineSoundState,
        voices: &mut Recorder,
        rng: &mut GameLcg,
        car: &EngineSoundCar,
    ) -> EngineSoundOutcome {
        state
            .update(voices, &config(), &tables(), rng, 0, car, 5, &listener())
            .unwrap()
    }

    #[test]
    fn running_starts_six_loops_and_leaves_rt_and_chatter() {
        let mut state = EngineSoundState::new();
        let mut voices = Recorder::default();
        let mut rng = GameLcg::new(1);
        let out = run(&mut state, &mut voices, &mut rng, &rpm_car(3000));
        assert_eq!(out.branch, EngineSoundBranch::Running);
        let plays: Vec<_> = voices
            .ops
            .iter()
            .filter(|op| matches!(op, Op::Play(..)))
            .take(6)
            .cloned()
            .collect();
        assert_eq!(
            plays,
            vec![
                Op::Play(100, 10, LOOP_FOREVER),
                Op::Play(101, 11, LOOP_FOREVER),
                Op::Play(102, 12, LOOP_FOREVER),
                Op::Play(103, 13, LOOP_FOREVER),
                Op::Play(105, 15, LOOP_FOREVER),
                Op::Play(104, 14, LOOP_FOREVER),
            ]
        );
        assert!(!voices.playing(16) && !voices.playing(17));
    }

    #[test]
    fn slow_and_mid_bands_scale_their_volume_and_pitch() {
        let mut state = EngineSoundState::new();
        let mut voices = Recorder::default();
        let mut rng = GameLcg::new(1);
        let car = rpm_car(3000);
        run(&mut state, &mut voices, &mut rng, &car);
        let rpm = engine_sound_input(&car).rpm;
        let t = tables();
        let i_m = t.mid.index(rpm) as u32;
        let ops = voices.non_position();
        assert!(ops.contains(&Op::Volume(10, 5000)));
        assert!(ops.contains(&Op::Volume(11, 5000)));
        assert!(ops.contains(&Op::Frequency(12, 200 + i_m)));
        assert!(ops.contains(&Op::Volume(13, 6000)));
    }

    #[test]
    fn out_of_window_mid_returns_to_native_pitch() {
        let mut state = EngineSoundState::new();
        let mut voices = Recorder::default();
        let mut rng = GameLcg::new(1);
        run(&mut state, &mut voices, &mut rng, &rpm_car(2100));
        let ops = voices.non_position();
        assert!(ops.contains(&Op::Frequency(12, NATIVE_PITCH)));
        assert!(ops.contains(&Op::Volume(12, 0)));
    }

    #[test]
    fn helium_raises_every_table_pitch_by_two_thousand() {
        let mut state = EngineSoundState::new();
        let mut voices = Recorder::default();
        let mut rng = GameLcg::new(1);
        let mut cfg = config();
        cfg.helium = true;
        let car = rpm_car(3000);
        state
            .update(
                &mut voices,
                &cfg,
                &tables(),
                &mut rng,
                0,
                &car,
                5,
                &listener(),
            )
            .unwrap();
        let i_m = tables().mid.index(engine_sound_input(&car).rpm) as u32;
        assert!(voices.ops.contains(&Op::Frequency(12, 2200 + i_m)));
    }

    #[test]
    fn reset_countdown_mutes_layers_and_cranks_the_ignition() {
        let mut state = EngineSoundState::new();
        let mut voices = Recorder::default();
        voices.playing.extend([10, 11]);
        let mut rng = GameLcg::new(1);
        let car = EngineSoundCar {
            reset_timer_75c: 5,
            ignition_764: 1,
            ..EngineSoundCar::default()
        };
        let out = run(&mut state, &mut voices, &mut rng, &car);
        assert_eq!(out.branch, EngineSoundBranch::ResetCountdown);
        assert_eq!(out.idle_countdown_6ec, 0xd);
        assert_eq!(
            voices.ops,
            vec![
                Op::Volume(10, 0),
                Op::Volume(11, 0),
                Op::PlayFrom(112, 17, IGNITION_LOOP_START),
                Op::Volume(17, 0x7fff),
            ]
        );
        // Counting down to zero without the ignition flag stops channel 7.
        let mut voices = Recorder::default();
        let car = EngineSoundCar {
            reset_timer_75c: 5,
            idle_countdown_6ec: 1,
            ..EngineSoundCar::default()
        };
        let out = run(&mut state, &mut voices, &mut rng, &car);
        assert_eq!(out.idle_countdown_6ec, 0);
        assert_eq!(voices.ops, vec![Op::Stop(17)]);
    }

    #[test]
    fn ignition_branch_plays_then_stops_on_ch0() {
        let mut state = EngineSoundState::new();
        let mut voices = Recorder::default();
        let mut rng = GameLcg::new(1);
        let mut car = EngineSoundCar {
            idle_6e8: 1,
            idle_countdown_6ec: 0x20,
            ..EngineSoundCar::default()
        };
        let out = run(&mut state, &mut voices, &mut rng, &car);
        assert_eq!(out.branch, EngineSoundBranch::Ignition);
        assert_eq!(state.ignition_5b8, 1);
        assert_eq!(voices.ops[0], Op::PlayFrom(112, 10, IGNITION_LOOP_START));
        car.idle_countdown_6ec = 0x17;
        run(&mut state, &mut voices, &mut rng, &car);
        assert_eq!(state.ignition_5b8, 0);
        assert_eq!(voices.ops.last(), Some(&Op::Stop(10)));
    }

    #[test]
    fn entering_first_gear_plays_the_clunk() {
        let mut state = EngineSoundState::new();
        let mut voices = Recorder::default();
        let mut rng = GameLcg::new(1);
        let mut car = rpm_car(3000);
        car.gear_43a = 1;
        run(&mut state, &mut voices, &mut rng, &car);
        assert!(voices.ops.contains(&Op::Play(113, 17, PLAY_ONCE)));
        assert!(voices.ops.contains(&Op::Position(17, 0)));
        assert_eq!(state.players[0].gear_clunk_534, 0);
        // Staying in gear 1 does not clunk again.
        voices.ops.clear();
        run(&mut state, &mut voices, &mut rng, &car);
        assert!(!voices.ops.contains(&Op::Play(113, 17, PLAY_ONCE)));
    }

    #[test]
    fn lifting_at_high_revs_fires_the_blowoff_on_ch5() {
        let mut state = EngineSoundState::new();
        let mut voices = Recorder::default();
        let mut rng = GameLcg::new(1);
        let mut car = rpm_car(6000);
        car.gear_43a = 3;
        car.throttle_response_2d8 = 0x10000;
        for _ in 0..5 {
            run(&mut state, &mut voices, &mut rng, &car);
        }
        assert_eq!(state.gate_548, 100);
        assert!(voices.playing(16));
        car.throttle_response_2d8 = 0;
        voices.ops.clear();
        for _ in 0..10 {
            run(&mut state, &mut voices, &mut rng, &car);
        }
        assert_eq!(state.gate_548, 0);
        assert!(voices.ops.contains(&Op::Play(107, 15, PLAY_ONCE)));
        assert!(voices.ops.contains(&Op::Volume(15, 9000)));
        assert_eq!(state.players[0].blowoff_580, 1);
        assert!(!voices.playing(16));
    }

    #[test]
    fn rt_loop_start_comes_from_the_car_nibble() {
        let mut state = EngineSoundState::new();
        let mut voices = Recorder::default();
        let mut rng = GameLcg::new(1);
        let mut car = rpm_car(6000);
        car.throttle_response_2d8 = 0x10000;
        run(&mut state, &mut voices, &mut rng, &car);
        assert!(voices.ops.contains(&Op::PlayFrom(106, 16, 0xa065)));
    }

    #[test]
    fn overrun_pops_step_the_envelope() {
        let mut state = EngineSoundState::new();
        let mut voices = Recorder::default();
        let mut rng = GameLcg::new(7);
        let mut car = rpm_car(5000);
        car.throttle_response_2d8 = 0x10000;
        run(&mut state, &mut voices, &mut rng, &car);
        assert_eq!(state.players[0].overrun_598, 1);
        car.throttle_response_2d8 = 0;
        run(&mut state, &mut voices, &mut rng, &car);
        let me = state.players[0];
        assert!((1..=5).contains(&me.steps_568) && (1..=5).contains(&me.hold_570));
        assert_eq!(me.envelope_b44, ENVELOPE_FULL);
        assert!(
            voices
                .ops
                .iter()
                .any(|op| matches!(op, Op::Play(108..=111, 17, PLAY_ONCE)))
        );
        run(&mut state, &mut voices, &mut rng, &car);
        assert_eq!(
            state.players[0].envelope_b44,
            ENVELOPE_FULL - ENVELOPE_FULL / me.steps_568
        );
        // Below 4501 the overrun disarms.
        run(&mut state, &mut voices, &mut rng, &rpm_car(4000));
        assert_eq!(state.players[0].overrun_598, 0);
    }

    #[test]
    fn antilag_pops_on_lift_and_cuts_on_reapply() {
        let mut state = EngineSoundState::new();
        let mut voices = Recorder::default();
        let mut rng = GameLcg::new(3);
        let mut cfg = config();
        cfg.antilag_family_540 = true;
        let mut car = rpm_car(5000);
        car.throttle_pedal_304 = 0xffbe;
        let t = tables();
        let l = listener();
        state
            .update(&mut voices, &cfg, &t, &mut rng, 0, &car, 5, &l)
            .unwrap();
        assert_eq!(state.players[0].antilag_5a0, 1);
        car.throttle_pedal_304 = 0x8000;
        state
            .update(&mut voices, &cfg, &t, &mut rng, 0, &car, 5, &l)
            .unwrap();
        assert_eq!(state.players[0].antilag_fired_5a8, 1);
        assert!(
            voices
                .ops
                .iter()
                .any(|op| matches!(op, Op::Play(107..=111, 17, PLAY_ONCE)))
        );
        assert!(voices.ops.contains(&Op::Volume(17, 10000)));
        // Re-applying with channel 7 still playing cuts it and disarms. The
        // rpm stays above 4500, so the arming check re-arms right after.
        car.throttle_pedal_304 = 0xffbe;
        car.engine_speed_2e8 = rpm_car(4000).engine_speed_2e8;
        state
            .update(&mut voices, &cfg, &t, &mut rng, 0, &car, 5, &l)
            .unwrap();
        assert_eq!(state.players[0].antilag_5a0, 0);
        assert!(!voices.playing(17));
    }

    #[test]
    fn whine_follows_speed_and_ramps_with_throttle() {
        let mut state = EngineSoundState::new();
        let mut voices = Recorder::default();
        let mut rng = GameLcg::new(1);
        let mut car = rpm_car(3000);
        car.gear_43a = 5;
        car.throttle_response_2d8 = 0x10000;
        car.speed_ec = 0xed2f;
        run(&mut state, &mut voices, &mut rng, &car);
        let speed = engine_sound_input(&car).speed;
        let w = &tables().whine;
        let index = w.index(speed) as u32;
        assert!(voices.ops.contains(&Op::Frequency(14, 400 + index)));
        assert_eq!(state.whine_ramp_53c, 10);
        // Gear change dip: v = ramp 10 * 80 / 100 = 8, minus 20 -> 0.
        assert_eq!(state.dip_54c, 2);
        assert_eq!(state.whine_last_5b4, 0);
        // Neutral mutes the whine to the native rate.
        voices.ops.clear();
        car.gear_43a = 0;
        run(&mut state, &mut voices, &mut rng, &car);
        assert!(voices.ops.contains(&Op::Frequency(14, NATIVE_PITCH)));
        assert!(voices.ops.contains(&Op::Volume(14, 0)));
    }

    #[test]
    fn out_of_range_gear_is_rewritten_to_one_for_the_whine() {
        let mut state = EngineSoundState::new();
        let mut voices = Recorder::default();
        let mut rng = GameLcg::new(1);
        let mut car = rpm_car(3000);
        car.gear_43a = 7;
        run(&mut state, &mut voices, &mut rng, &car);
        assert_eq!(state.players[0].prev_gear_5b0, 7);
        assert_eq!(state.prev_gear_52c, 1);
    }

    #[test]
    fn stage_init_resets_shared_words_only() {
        let mut state = EngineSoundState::new();
        state.gate_548 = 50;
        state.prev_gear_52c = 3;
        state.players[0].overrun_598 = 1;
        let mut voices = Recorder::default();
        state.stage_init(&mut voices, &config(), false);
        assert_eq!((state.gate_548, state.prev_gear_52c), (0, -1));
        assert_eq!(state.players[0].overrun_598, 1);
        assert_eq!(voices.ops[0], Op::Volume(10, 0x7fff));
        assert_eq!(voices.ops.len(), 7);
    }
}
