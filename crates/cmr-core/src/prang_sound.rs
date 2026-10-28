//! Reconstruction of the collision ("prang") sounds: the bank
//! (`FUN_00469ac0`), the impact player `FUN_00469e80`, its severity
//! `FUN_0041f630`, the landing/rollover thump `FUN_0046a030` and the
//! breakage cue `FUN_0046a2a0`.
//!
//! The bank loads 48 waves (SWIPE1..4, COLLIS1..6, the material names,
//! ROLL1..5, HLIGHT1/2, SCREEN1, MIRROR from `sounds\prangs\`, then 1..12
//! from `sounds\accident\`) and opens five channels at the full volume. An
//! impact plays `SWIPE`/`COLLIS` by severity on channel 0 (no cooldown beyond
//! "still playing", but the volume and position are rewritten every call);
//! channel 1 gets a position and volume and is never played (its wave is only
//! formatted into a debug string). Landing after six or more ticks with all
//! wheels in the air, or a body probe touching, plays a `ROLL` thump on
//! channels 2/3 alternately. All state is player 0 only.
//!
//! A description of disassembly, not original source. Evidence:
//! `rea.official-eu.prang-sound`, `.work/surface-impact-audio-notes.md`.

use crate::engine_sound::EngineVoices;
use crate::game_rng::GameLcg;
use crate::legacy_math::sqrt_q16;

/// Slots: `SWIPE1` (0), `ROLL1` (`0x1b`), `HLIGHT1` (`0x20`), `SCREEN1`
/// (`0x22`), `MIRROR` (`0x23`).
pub const ROLL_SLOT: u32 = 0x1b;
pub const HLIGHT_SLOT: u32 = 0x20;
pub const SCREEN_SLOT: u32 = 0x22;
pub const MIRROR_SLOT: u32 = 0x23;
/// The number of waves the bank loads from `sounds\prangs\`.
pub const PRANG_WAVES: usize = 36;
/// The total the bank loads (the last 12 from `sounds\accident\`).
pub const BANK_WAVES: usize = 48;

fn qmul(a: i32, b: i32) -> i32 {
    ((i64::from(a) * i64::from(b)) >> 16) as i32
}

/// The bank's relative wave paths from the name table `0x4f40e0`.
pub fn prang_wave_paths(names: &[String]) -> Vec<String> {
    names
        .iter()
        .enumerate()
        .map(|(slot, name)| {
            let dir = if slot < PRANG_WAVES {
                "sounds\\prangs\\"
            } else {
                "sounds\\accident\\"
            };
            format!("{dir}{name}")
        })
        .collect()
}

/// `FUN_0041f630`'s severity: the legacy-table length of the impact vector,
/// `0` below `0x2624`, else `min(qmul(len - 0x2624, 0xf536), 0x10000)`. The
/// damage model it also runs is not reconstructed; it is what yields the
/// breakage kind, which therefore stays 0.
pub fn impact_severity(vector: [i32; 3]) -> i32 {
    let sum = vector.iter().fold(0_u32, |sum, &part| {
        sum.wrapping_add(qmul(part, part) as u32)
    });
    let length = sqrt_q16(sum) as i32;
    if length < 0x2624 {
        0
    } else {
        qmul(length - 0x2624, 0xf536).min(0x10000)
    }
}

/// `FUN_00401fd0`'s barrier kind to impact object type (jump table
/// `0x403448`); other kinds play no impact sound.
pub fn barrier_object_type(kind: u8) -> Option<i8> {
    Some(match kind {
        0xc => 2,
        0xd => 3,
        0xe => 5,
        0xf => 6,
        0x10 => 0xd,
        0x11 => 0x10,
        0x12 => 0,
        0x13 => 0xf,
        0x14 => 9,
        _ => return None,
    })
}

/// The prang channels and slots plus the per-tick words of the thump.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrangSoundState {
    /// `DAT_00592dc8[4]`: ticks each wheel has been airborne.
    pub airborne: [i32; 4],
    /// `DAT_00592ea0[4]` / `DAT_00592eb0[4]`: wheel landed / body touched.
    pub landed: [bool; 4],
    pub body_latch: [bool; 4],
    /// `DAT_004f41a0`: the thump channel, alternating 2 and 3.
    pub thump_channel: u32,
    /// `DAT_004f41a8`: the full volume.
    pub full_volume: i32,
}

impl PrangSoundState {
    /// `FUN_00469a50`, run at the end of the bank load.
    pub fn new(full_volume: i32) -> Self {
        Self {
            airborne: [0; 4],
            landed: [false; 4],
            body_latch: [false; 4],
            thump_channel: 2,
            full_volume,
        }
    }
}

/// The host side of one prang call: the mixer, the channel and slot bases,
/// the curve input and the master volume.
pub struct PrangVoices<'a, V: EngineVoices> {
    pub voices: &'a mut V,
    pub slot_base: u32,
    pub channel_base: u32,
    /// `DAT_005ff9e4[0]`.
    pub centre_proximity: i32,
    /// `DAT_004f653c`.
    pub master_percent: i32,
}

impl<V: EngineVoices> PrangVoices<'_, V> {
    fn curve(&self, v: i32) -> i32 {
        if self.centre_proximity > 0x14d {
            self.centre_proximity.wrapping_mul(v) / 1000
        } else {
            v / 3
        }
    }
    fn volume(&mut self, n: u32, v: i32) {
        let v = self.curve(v);
        let scaled = self.master_percent.wrapping_mul(v) / 100;
        self.voices.set_volume(self.channel_base + n, scaled);
    }
    fn play_once(&mut self, slot: u32, n: u32) {
        self.voices
            .play(self.slot_base + slot, self.channel_base + n, 0);
    }
    fn playing(&self, n: u32) -> bool {
        self.voices.playing(self.channel_base + n)
    }
    fn position(&mut self, n: u32, azimuth: i32) {
        self.voices.set_position(
            self.channel_base + n,
            crate::audio_mixer::Position3d {
                x: azimuth,
                y: 0,
                z: 0,
            },
        );
    }
}

/// `FUN_00469e80(severity, point, object_type)` with the point's azimuth
/// already computed (`FUN_0047f6e0`).
pub fn play_impact<V: EngineVoices>(
    state: &PrangSoundState,
    out: &mut PrangVoices<'_, V>,
    severity: i32,
    azimuth: i32,
    object_type: i8,
) {
    let severity = if severity == 0x10000 {
        0xfd70
    } else {
        severity
    };
    let bucket = (qmul(severity, 0x64_0000) >> 16) % 10;
    let sub = qmul(severity, 0xa_0000) >> 16;
    let full = state.full_volume;
    if !out.playing(0) {
        out.play_once(sub as u32, 0);
    }
    let volume = if bucket <= 10 {
        full.wrapping_mul(bucket) / 20 + full / 2
    } else {
        full
    };
    out.volume(0, volume);
    out.position(0, azimuth);
    if object_type != 0 && object_type < 0x11 {
        out.position(1, azimuth);
        let volume = if bucket <= 10 {
            full.wrapping_mul(bucket) / 10
        } else {
            full
        };
        out.volume(1, volume);
        // FUN_0046a650 only formats the material wave's name for debugging.
    }
}

/// `FUN_0046a2a0(kind, point)`: the headlight/screen/mirror breakage cue on
/// channel 4.
pub fn play_breakage<V: EngineVoices>(
    state: &PrangSoundState,
    out: &mut PrangVoices<'_, V>,
    rng: &mut GameLcg,
    kind: i32,
    azimuth: i32,
) {
    let slot = match kind {
        2 => SCREEN_SLOT,
        3 => MIRROR_SLOT,
        _ => HLIGHT_SLOT + rng.next_raw() % 2,
    };
    if !out.playing(4) {
        out.volume(4, state.full_volume);
        out.play_once(slot, 4);
    }
    out.position(4, azimuth);
}

/// The words `FUN_0046a030` reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThumpInput {
    /// `cfg[0x26] & 2`.
    pub effects_off: bool,
    /// `ctl +0x404`.
    pub upright_404: bool,
    /// `car +0x680`.
    pub out_of_control_680: bool,
    /// `car +0x73a..+0x741`: 1 means no contact (wheels 0..3, body 4..7).
    pub contact: [u8; 8],
}

impl PrangSoundState {
    fn next_thump_channel(&mut self) {
        self.thump_channel = if self.thump_channel == 3 {
            2
        } else {
            self.thump_channel + 1
        };
    }

    /// `FUN_0046a030`. Returns whether it set `car +0x704`.
    pub fn thump<V: EngineVoices>(
        &mut self,
        out: &mut PrangVoices<'_, V>,
        rng: &mut GameLcg,
        input: &ThumpInput,
    ) -> bool {
        if input.effects_off {
            return false;
        }
        let full = self.full_volume;
        let mut set_704 = false;
        if input.upright_404 && !input.out_of_control_680 {
            for wheel in 0..4 {
                if input.contact[wheel] == 1 {
                    self.airborne[wheel] += 1;
                    self.landed[wheel] = false;
                    continue;
                }
                if !self.landed[wheel] && self.airborne.iter().all(|&ticks| ticks > 5) {
                    set_704 = true;
                    let channel = self.thump_channel;
                    if !out.playing(channel) {
                        let slot = ROLL_SLOT + rng.next_raw() % 5;
                        out.play_once(slot, channel);
                    }
                    let ticks = self.airborne[wheel];
                    let volume = if ticks > 0x23 {
                        full
                    } else {
                        full / 3 + (ticks - 5).wrapping_mul(full) / 45
                    };
                    out.volume(channel, volume);
                    self.next_thump_channel();
                    self.landed[wheel] = true;
                }
                self.airborne[wheel] = 0;
            }
        }
        for probe in 4..8 {
            let latch = &mut self.body_latch[probe - 4];
            if input.contact[probe] == 1 {
                *latch = false;
            } else if !*latch {
                set_704 = true;
                let channel = self.thump_channel;
                if !out.playing(channel) {
                    let slot = ROLL_SLOT + rng.next_raw() % 5;
                    out.play_once(slot, channel);
                    out.volume(channel, full);
                }
                self.next_thump_channel();
                self.body_latch[probe - 4] = true;
            }
        }
        set_704
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_mixer::Position3d;
    use std::collections::BTreeSet;

    #[derive(Default)]
    struct Voices {
        playing: BTreeSet<u32>,
        plays: Vec<(u32, u32)>,
        volumes: Vec<(u32, i32)>,
        positions: Vec<(u32, i32)>,
    }

    impl EngineVoices for Voices {
        fn playing(&self, channel: u32) -> bool {
            self.playing.contains(&channel)
        }
        fn play(&mut self, wave: u32, channel: u32, _loops: u32) {
            self.playing.insert(channel);
            self.plays.push((wave, channel));
        }
        fn play_with_loop_start(&mut self, wave: u32, channel: u32, _loop_start: u32) {
            self.play(wave, channel, 0);
        }
        fn stop(&mut self, channel: u32) {
            self.playing.remove(&channel);
        }
        fn set_volume(&mut self, channel: u32, volume: i32) {
            self.volumes.push((channel, volume));
        }
        fn set_frequency(&mut self, _channel: u32, _frequency: u32) {}
        fn set_position(&mut self, channel: u32, position: Position3d) {
            self.positions.push((channel, position.x));
        }
        fn wave_busy(&self, _wave: u32) -> bool {
            false
        }
    }

    fn out(voices: &mut Voices) -> PrangVoices<'_, Voices> {
        PrangVoices {
            voices,
            slot_base: 100,
            channel_base: 20,
            centre_proximity: 1000,
            master_percent: 100,
        }
    }

    #[test]
    fn paths_split_between_prangs_and_accident() {
        let names: Vec<String> = (0..48).map(|i| format!("N{i}.WAV")).collect();
        let paths = prang_wave_paths(&names);
        assert_eq!(paths[0], "sounds\\prangs\\N0.WAV");
        assert_eq!(paths[35], "sounds\\prangs\\N35.WAV");
        assert_eq!(paths[36], "sounds\\accident\\N36.WAV");
    }

    #[test]
    fn severity_thresholds_and_clamps() {
        assert_eq!(impact_severity([0x2000, 0, 0]), 0);
        let s = impact_severity([0x8000, 0, 0]);
        assert!(s > 0 && s < 0x10000);
        assert_eq!(impact_severity([0x40000, 0, 0]), 0x10000);
    }

    #[test]
    fn an_impact_picks_its_wave_by_severity_and_positions_both_channels() {
        let state = PrangSoundState::new(0x7d00);
        let mut voices = Voices::default();
        play_impact(&state, &mut out(&mut voices), 0x10000, 30, 3);
        // 0x10000 -> 0xfd70: sub-wave 9 (COLLIS6); bucket (0xfd70 * 100 >> 16)
        // % 10 = 98 % 10 = 8.
        assert_eq!(voices.plays, vec![(109, 20)]);
        assert_eq!(voices.volumes[0], (20, 0x7d00 * 8 / 20 + 0x7d00 / 2));
        assert_eq!(voices.volumes[1], (21, 0x7d00 * 8 / 10));
        assert_eq!(voices.positions, vec![(20, 30), (21, 30)]);
        // A busy channel is not restarted, but its volume and position are.
        play_impact(&state, &mut out(&mut voices), 0x1000, -10, 0);
        assert_eq!(voices.plays.len(), 1);
        assert_eq!(voices.positions.last(), Some(&(20, -10)));
    }

    #[test]
    fn barrier_kinds_map_to_object_types() {
        assert_eq!(barrier_object_type(0xd), Some(3));
        assert_eq!(barrier_object_type(0x12), Some(0));
        assert_eq!(barrier_object_type(0), None);
        assert_eq!(barrier_object_type(0x15), None);
    }

    #[test]
    fn landing_after_six_airborne_ticks_thumps_the_first_wheel_only() {
        let mut state = PrangSoundState::new(0x7d00);
        let mut voices = Voices::default();
        let mut rng = GameLcg::new(1);
        let air = ThumpInput {
            effects_off: false,
            upright_404: true,
            out_of_control_680: false,
            contact: [1, 1, 1, 1, 0, 0, 0, 0],
        };
        // Body probes touching from the start latch once.
        state.thump(&mut out(&mut voices), &mut rng, &air);
        voices.plays.clear();
        voices.playing.clear();
        for _ in 0..5 {
            state.thump(&mut out(&mut voices), &mut rng, &air);
        }
        assert!(voices.plays.is_empty());
        let land = ThumpInput {
            contact: [0, 0, 1, 1, 0, 0, 0, 0],
            ..air
        };
        assert!(state.thump(&mut out(&mut voices), &mut rng, &land));
        assert_eq!(voices.plays.len(), 1, "wheel 1 sees wheel 0 already reset");
        assert!((0x1b + 100..0x20 + 100).contains(&voices.plays[0].0));
        assert_eq!(state.thump_channel, 3);
        assert!(state.landed[0] && !state.landed[1]);
    }

    #[test]
    fn breakage_kinds_pick_screen_mirror_or_a_headlight() {
        let state = PrangSoundState::new(0x7d00);
        let mut voices = Voices::default();
        let mut rng = GameLcg::new(1);
        play_breakage(&state, &mut out(&mut voices), &mut rng, 2, 0);
        assert_eq!(voices.plays, vec![(100 + SCREEN_SLOT, 24)]);
        voices.playing.clear();
        play_breakage(&state, &mut out(&mut voices), &mut rng, 7, 0);
        assert!([100 + HLIGHT_SLOT, 101 + HLIGHT_SLOT].contains(&voices.plays[1].0));
    }
}
