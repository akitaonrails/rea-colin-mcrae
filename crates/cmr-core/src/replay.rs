//! Reconstruction of the replay trace format and the display-snapshot buffer
//! (the confidently-recovered G08 pieces).
//!
//! The game has two distinct replay subsystems:
//!
//! - the **authoritative input trace** (the "Replay" class, `FUN_0046d7b0` /
//!   `FUN_0046de30`): per-car, **input-based re-simulation**, saved to / loaded
//!   from `.SSR` files and also streamed for the network replay car. Each object
//!   is a `0x40`-byte header, a per-frame `0x151ec`-byte initial-state seed
//!   (Region A), then `nTicks` 4-byte packed control records (Region B). A record
//!   holds the quantized, XOR-delta-coded actuators (steer/throttle/brake by
//!   `>>10`, plus gear/flags); on playback the normal physics step re-runs over
//!   the decoded inputs (`FUN_0046cd70` / `FUN_0046dc20`). `nTicks` is `0x1d4c`
//!   (7500 = 300 s at 25 Hz) or `0x3a98` at the 50 Hz option. Files load through
//!   the shared loader `FUN_00495ed0`, whose optional archive container carries
//!   the `"CMPR"` magic + 4-byte size.
//! - the **render snapshot buffer** (`FUN_0046c7d0` / `FUN_0046ca70`, gated on the
//!   record flag): 24-byte keyframes (a position vec3 of `i32` plus two
//!   `3xi16` compressed orientation axes) captured every 4th subframe (6.25 Hz),
//!   up to `0x8ca` per car across 6 car lanes, interpolated into the render
//!   transforms for a smooth action replay. This never feeds physics.
//!
//! See evidence `rea.official-eu.sim-replay`.
//!
//! Observations: the object sizing/region strides, the `nTicks` values, the
//! `>>10` quantization + XOR-delta codec, the `.SSR` extension and `"CMPR"`
//! container magic, and the snapshot record shape/capacity/compression.
//! Inferences: Region A is the deterministic re-sim seed; track/car identity is
//! carried in the `.SSR` filename. The four record-byte assignments and Region-A
//! section layout are recovered. Unknowns (not reproduced): the explicit `.SSR`
//! write path and the network wire framing.

use crate::fixed_point::Fixed16;

/// Per-car input-trace ticks at 25 Hz (`0x1d4c` = 7500 = 300 s).
pub const REPLAY_TICKS_25HZ: u32 = 0x1d4c;

/// Per-car input-trace ticks at the 50 Hz option (`0x3a98` = 15000).
pub const REPLAY_TICKS_50HZ: u32 = 0x3a98;

/// Maximum replay duration in seconds (both rates: `ticks / rate`).
pub const REPLAY_MAX_DURATION_SECONDS: u32 = 300;

/// Bytes of the per-frame initial-state seed block (Region A), `0x151ec`.
pub const REPLAY_INITIAL_STATE_BYTES: usize = 0x151ec;

/// Dwords of the Region-A block (`0x151ec / 4 = 0x547b`).
pub const REPLAY_INITIAL_STATE_DWORDS: usize = 0x547b;

/// Region-A car-type block size (`0xcd0`).
pub const REPLAY_CAR_TYPE_BYTES: usize = 0xcd0;
/// Region-A control-block offset (`0xcd0`) and size (`0x464`).
pub const REPLAY_CONTROL_OFFSET: usize = 0xcd0;
pub const REPLAY_CONTROL_BYTES: usize = 0x464;
/// Region-A physics-block offset (`0x11f4`) and size (`0x768`).
pub const REPLAY_PHYSICS_OFFSET: usize = 0x11f4;
pub const REPLAY_PHYSICS_BYTES: usize = 0x768;
/// Region-A object-record offset (`0x195c`) and stride (`0xa0`).
pub const REPLAY_OBJECTS_OFFSET: usize = 0x195c;
pub const REPLAY_OBJECT_RECORD_BYTES: usize = 0xa0;
/// Region-A trailing state/timer offsets (`0x151dc`/`0x151e8`).
pub const REPLAY_TRAILING_STATE_OFFSET: usize = 0x151dc;
pub const REPLAY_TIMER_OFFSET: usize = 0x151e8;
/// Maximum object records before the trailing 12-byte state block (`0x1f4`).
pub const REPLAY_MAX_OBJECTS: usize = 0x1f4;

/// Checked offset of one Region-A object record.
#[must_use]
pub fn replay_object_record_offset(index: usize) -> Option<usize> {
    (index < REPLAY_MAX_OBJECTS)
        .then_some(REPLAY_OBJECTS_OFFSET + index * REPLAY_OBJECT_RECORD_BYTES)
}

/// Bytes of one per-tick control record (Region B element).
pub const REPLAY_TICK_RECORD_BYTES: usize = 4;

/// Bytes of the replay-object header prefix.
pub const REPLAY_OBJECT_HEADER_BYTES: usize = 0x40;

/// The per-frame constant used in the object-size formula (`0x151ee`, two bytes
/// over the Region-A block size).
pub const REPLAY_SIZING_PAD: usize = 0x151ee;

/// The `.SSR` replay/shadow file extension (uppercase, as the format literal
/// `"%s%s%c%d%c.SSR"` produces).
pub const REPLAY_FILE_EXTENSION: &str = ".SSR";

/// Archive-container magic for the shared loader (`"CMPR"`).
pub const CMPR_MAGIC: [u8; 4] = *b"CMPR";

/// Bytes of the `"CMPR"` container header (magic + 4-byte little-endian size).
pub const CMPR_HEADER_BYTES: usize = 8;

/// Bytes of one render-snapshot keyframe (pos `3xi32` + 2 x `3xi16`).
pub const SNAPSHOT_RECORD_BYTES: usize = 0x18;

/// Render-snapshot keyframes stored per car (`0x8ca` = 2250 ~ 360 s at 6.25 Hz).
pub const SNAPSHOT_CAPACITY_PER_CAR: usize = 0x8ca;

/// Render-snapshot car lanes.
pub const SNAPSHOT_CAR_LANES: usize = 6;

/// Total render-snapshot keyframe entries (`0x8ca * 6 = 0x34bc`).
pub const SNAPSHOT_BUFFER_ENTRIES: usize = SNAPSHOT_CAPACITY_PER_CAR * SNAPSHOT_CAR_LANES;

/// Right shift used to quantize an actuator into a record field (`>>10`).
pub const ACTUATOR_QUANT_SHIFT: u32 = 10;

/// Mask kept after quantization (6-bit field, `0x3f`).
pub const ACTUATOR_QUANT_MASK: u8 = 0x3f;

/// Orientation-axis compression scale applied before clamping (`0x8000`).
pub const ORIENTATION_COMPRESS_SCALE: i64 = 0x8000;

/// Orientation-axis compression clamp magnitude (`0x7ff9`, inside `i16` range).
pub const ORIENTATION_COMPRESS_CLAMP: i32 = 0x7ff9;

/// Per-subframe render-snapshot interpolation step in Q16.16 (`0x4000` = 0.25);
/// subframes `0..=3` give factors `0.0, 0.25, 0.5, 0.75`.
pub const SNAPSHOT_INTERP_STEP: i32 = 0x4000;

/// The per-car input-trace tick count for the active rate (`FUN_00422be0`:
/// `((double_rate)+1) * 0x1d4c`).
pub fn replay_ticks(double_rate: bool) -> u32 {
    if double_rate {
        REPLAY_TICKS_50HZ
    } else {
        REPLAY_TICKS_25HZ
    }
}

/// Serialized replay-object size in bytes, reproducing `FUN_0046d7b0`:
/// `(nTicks*4 + 0x151ee) * nFrames + 0x40`. The file loader validates the loaded
/// size against this exact value.
pub fn replay_object_size(n_ticks: u32, n_frames: u32) -> usize {
    (n_ticks as usize * REPLAY_TICK_RECORD_BYTES + REPLAY_SIZING_PAD) * n_frames as usize
        + REPLAY_OBJECT_HEADER_BYTES
}

/// Byte offset of the per-tick record stream (Region B) within the object:
/// after the header and the `nFrames` Region-A seed blocks.
pub fn replay_region_b_offset(n_frames: u32) -> usize {
    REPLAY_OBJECT_HEADER_BYTES + n_frames as usize * REPLAY_INITIAL_STATE_BYTES
}

/// Whether a loaded file's size matches the replay object it claims to be
/// (`FUN_0046ddc0`'s size check).
pub fn replay_file_size_valid(loaded_bytes: usize, n_ticks: u32, n_frames: u32) -> bool {
    loaded_bytes == replay_object_size(n_ticks, n_frames)
}

/// Quantize a Q16.16 actuator value into its 6-bit record field (`(v >> 10) &
/// 0x3f`).
pub fn quantize_actuator(value: i32) -> u8 {
    ((value >> ACTUATOR_QUANT_SHIFT) & ACTUATOR_QUANT_MASK as i32) as u8
}

/// XOR-delta encode a quantized field against the previous record's field
/// (`(prev ^ cur) & 0x3f`).
pub fn delta_encode(previous: u8, current: u8) -> u8 {
    (previous ^ current) & ACTUATOR_QUANT_MASK
}

/// XOR-delta decode a record field given the previous one (the inverse of
/// [`delta_encode`], since XOR is self-inverse).
pub fn delta_decode(previous: u8, encoded: u8) -> u8 {
    (previous ^ encoded) & ACTUATOR_QUANT_MASK
}

/// Native control fields consumed by `FUN_0046cd70`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReplayActuators {
    pub steer: i32,
    pub steer_alternate: i32,
    pub throttle: i32,
    pub brake: i32,
    pub handbrake: bool,
    pub flag_310: bool,
    pub gear: i32,
    pub analog_steer: bool,
    pub analog_throttle: bool,
    pub analog_brake: bool,
}

/// One four-byte input-trace record.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReplayControlRecord(pub [u8; REPLAY_TICK_RECORD_BYTES]);

/// Encode one record; the double-XOR steer/gear sequences become their current
/// values, while the repeat counter alone retains history.
#[must_use]
pub fn encode_replay_control(
    input: ReplayActuators,
    previous: ReplayControlRecord,
    recording_active: bool,
) -> ReplayControlRecord {
    let analog_steer = input.analog_steer && recording_active;
    let analog_throttle = input.analog_throttle && recording_active;
    let analog_brake = input.analog_brake && recording_active;
    let mut steer_byte = (previous.0[3] & 0x7f) | u8::from(analog_steer) << 7;
    let steer_value = if input.steer < 1 {
        steer_byte = (previous.0[3] & 0x3f) | u8::from(analog_steer) << 7;
        if analog_steer {
            quantize_actuator(input.steer_alternate)
        } else {
            u8::from(input.steer_alternate != 0)
        }
    } else {
        steer_byte |= 0x40;
        if analog_steer {
            quantize_actuator(input.steer)
        } else {
            u8::from(input.steer != 0)
        }
    };
    steer_byte = (steer_byte & 0xc0) | (steer_value & 0x3f);
    let throttle = if analog_throttle {
        quantize_actuator(input.throttle)
    } else {
        u8::from(input.throttle != 0)
    };
    let brake = if analog_brake {
        quantize_actuator(input.brake)
    } else {
        u8::from(input.brake != 0)
    };
    let gear = (input.gear.wrapping_add(1) as u8) & 3;
    let repeat = previous.0[2].wrapping_add(1) & 0x3f;
    ReplayControlRecord([
        u8::from(analog_throttle) | u8::from(analog_brake) << 1 | (throttle & 0x3f) << 2,
        (brake & 0x3f) << 2 | gear,
        u8::from(input.handbrake) << 7 | u8::from(input.flag_310) << 6 | repeat,
        steer_byte,
    ])
}

/// Decoded actuator/mode values and the native repeat-counter outcome.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReplayDecodedControl {
    pub actuators: ReplayActuators,
    pub control_410: bool,
    pub control_414: bool,
    pub control_418: bool,
    pub next_repeat: u8,
    pub repeat_complete: bool,
}

/// Decode a record and advance the independent repeat counter.
#[must_use]
pub fn decode_replay_control(
    record: ReplayControlRecord,
    previous_repeat: u8,
) -> ReplayDecodedControl {
    let steer_byte = record.0[3];
    let steer = if steer_byte & 0x80 == 0 {
        if steer_byte & 0x3f == 0 { 0 } else { -66 }
    } else {
        i32::from(steer_byte & 0x3f) << 10
    };
    let (steer, steer_alternate) = if steer_byte & 0x40 == 0 {
        (0, steer)
    } else {
        (steer, 0)
    };
    let throttle = if record.0[0] & 1 == 0 {
        if record.0[0] & 0xfc == 0 { 0 } else { -66 }
    } else {
        i32::from(record.0[0] >> 2) << 10
    };
    let brake = if record.0[0] & 2 == 0 {
        if record.0[1] & 0xfc == 0 { 0 } else { -66 }
    } else {
        i32::from(record.0[1] >> 2) << 10
    };
    let next_repeat = previous_repeat.wrapping_add(1);
    let repeat_complete = next_repeat >= record.0[2] & 0x3f;
    ReplayDecodedControl {
        actuators: ReplayActuators {
            steer,
            steer_alternate,
            throttle,
            brake,
            handbrake: record.0[2] & 0x80 != 0,
            flag_310: record.0[2] & 0x40 != 0,
            gear: i32::from(record.0[1] & 3) - 1,
            analog_steer: steer_byte & 0x80 != 0,
            analog_throttle: record.0[0] & 1 != 0,
            analog_brake: record.0[0] & 2 != 0,
        },
        control_410: steer_byte & 0x80 != 0,
        control_414: record.0[0] & 1 != 0,
        control_418: record.0[0] & 2 != 0,
        next_repeat: if repeat_complete { 0 } else { next_repeat },
        repeat_complete,
    }
}

/// Whether the network replay consumer accepts a record instead of suppressing
/// it as a duplicate. Repeat `0` suppresses unconditionally; `0x3f` always accepts.
#[must_use]
pub fn network_replay_record_accepted(
    previous: ReplayControlRecord,
    current: ReplayControlRecord,
) -> bool {
    let repeat = current.0[2] & 0x3f;
    if repeat == 0 {
        return false;
    }
    if repeat == 0x3f {
        return true;
    }
    let throttle = previous.0[0] ^ current.0[0];
    let brake_gear = previous.0[1] ^ current.0[1];
    let steer = previous.0[3] ^ current.0[3];
    let flags = previous.0[2] ^ current.0[2];
    !(throttle & 0xfc == 0
        && brake_gear & 0xfc == 0
        && steer & 0x7f == 0
        && flags & 0xc0 == 0
        && brake_gear & 3 == 0
        && throttle & 3 == 0
        && steer & 0x80 == 0)
}

/// Parse a `"CMPR"` container header, returning the payload size, or `None` if
/// the magic is absent or the slice is too short.
pub fn parse_cmpr_header(bytes: &[u8]) -> Option<u32> {
    if bytes.len() < CMPR_HEADER_BYTES || bytes[0..4] != CMPR_MAGIC {
        return None;
    }
    Some(u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]))
}

/// Flat index of a render-snapshot keyframe for `car` at `tick`
/// (`[car*0x8ca + tick]`), or `None` if either is out of range.
pub fn snapshot_record_index(car: usize, tick: usize) -> Option<usize> {
    if car < SNAPSHOT_CAR_LANES && tick < SNAPSHOT_CAPACITY_PER_CAR {
        Some(car * SNAPSHOT_CAPACITY_PER_CAR + tick)
    } else {
        None
    }
}

/// Compress one orientation-axis component into the `i16` stored in a snapshot
/// keyframe, reproducing `FUN_0046c710`: scale by `0x8000` (Q16.16) and clamp to
/// `±0x7ff9`.
pub fn compress_orientation(component: Fixed16) -> i16 {
    let scaled = (component.raw() as i64 * ORIENTATION_COMPRESS_SCALE) >> 16;
    scaled.clamp(
        -(ORIENTATION_COMPRESS_CLAMP as i64),
        ORIENTATION_COMPRESS_CLAMP as i64,
    ) as i16
}

/// The render-snapshot interpolation factor for a subframe `0..=3`
/// (`subframe * 0.25`), reproducing `FUN_0046ca70`'s sub-frame blend.
pub fn snapshot_interp_factor(subframe: u32) -> Fixed16 {
    Fixed16::from_raw((subframe & 3) as i32 * SNAPSHOT_INTERP_STEP)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_counts_and_duration_match_recovery() {
        assert_eq!(replay_ticks(false), 0x1d4c);
        assert_eq!(replay_ticks(true), 0x3a98);
        // 7500 ticks / 25 Hz = 300 s; 15000 / 50 Hz = 300 s.
        assert_eq!(REPLAY_TICKS_25HZ / 25, REPLAY_MAX_DURATION_SECONDS);
        assert_eq!(REPLAY_TICKS_50HZ / 50, REPLAY_MAX_DURATION_SECONDS);
        // Region A is 0x547b dwords = 0x151ec bytes.
        assert_eq!(REPLAY_INITIAL_STATE_DWORDS * 4, REPLAY_INITIAL_STATE_BYTES);
    }

    #[test]
    fn object_size_and_region_layout_match_the_loader_check() {
        let size = replay_object_size(REPLAY_TICKS_25HZ, 1);
        // (7500*4 + 0x151ee) * 1 + 0x40.
        assert_eq!(size, (7500 * 4 + 0x151ee) + 0x40);
        // The loader validates the loaded size against the same formula.
        assert!(replay_file_size_valid(size, REPLAY_TICKS_25HZ, 1));
        assert!(!replay_file_size_valid(size + 1, REPLAY_TICKS_25HZ, 1));
        // Region B starts after the header and the single Region-A seed block.
        assert_eq!(
            replay_region_b_offset(1),
            REPLAY_OBJECT_HEADER_BYTES + REPLAY_INITIAL_STATE_BYTES
        );
    }

    #[test]
    fn region_a_sections_and_object_bounds_match_the_copy_routines() {
        assert_eq!(REPLAY_CAR_TYPE_BYTES, REPLAY_CONTROL_OFFSET);
        assert_eq!(REPLAY_CONTROL_OFFSET + REPLAY_CONTROL_BYTES, 0x1134);
        assert_eq!(
            REPLAY_PHYSICS_OFFSET + REPLAY_PHYSICS_BYTES,
            REPLAY_OBJECTS_OFFSET
        );
        assert_eq!(replay_object_record_offset(0), Some(REPLAY_OBJECTS_OFFSET));
        assert_eq!(
            replay_object_record_offset(REPLAY_MAX_OBJECTS - 1),
            Some(REPLAY_TRAILING_STATE_OFFSET - REPLAY_OBJECT_RECORD_BYTES)
        );
        assert_eq!(replay_object_record_offset(REPLAY_MAX_OBJECTS), None);
        assert_eq!(REPLAY_TIMER_OFFSET + 4, REPLAY_INITIAL_STATE_BYTES);
    }

    #[test]
    fn actuator_quantize_and_delta_round_trip() {
        // >>10 then 6-bit mask: 0x1_0000 >> 10 = 0x40, & 0x3f = 0.
        assert_eq!(quantize_actuator(0x1_0000), 0);
        // 0x8c00 >> 10 = 0x23, & 0x3f = 0x23.
        assert_eq!(quantize_actuator(0x8c00), 0x23);
        assert_eq!(quantize_actuator(0), 0);
        // XOR-delta is self-inverse: decode(prev, encode(prev, cur)) == cur.
        let prev = 0x2a;
        let cur = 0x15;
        let encoded = delta_encode(prev, cur);
        assert_eq!(delta_decode(prev, encoded), cur);
        // A field equal to the previous encodes as zero (the delta's purpose).
        assert_eq!(delta_encode(0x3f, 0x3f), 0);
    }

    #[test]
    fn analog_control_record_is_byte_exact_and_round_trips() {
        let input = ReplayActuators {
            steer: 0x8c00,
            steer_alternate: 0,
            throttle: 0x4c00,
            brake: 0x2c00,
            handbrake: true,
            flag_310: true,
            gear: 2,
            analog_steer: true,
            analog_throttle: true,
            analog_brake: true,
        };
        let record =
            encode_replay_control(input, ReplayControlRecord([0x55, 0x02, 0x10, 0x20]), true);
        assert_eq!(record.0, [0x4f, 0x2f, 0xd1, 0xe3]);
        let decoded = decode_replay_control(record, 0x10);
        assert_eq!(decoded.actuators, input);
        assert!(decoded.control_410);
        assert!(decoded.control_414);
        assert!(decoded.control_418);
        assert!(decoded.repeat_complete);
        assert_eq!(decoded.next_repeat, 0);
    }

    #[test]
    fn digital_controls_decode_to_minus_66_and_repeat_uses_history() {
        let input = ReplayActuators {
            steer: 0,
            steer_alternate: 1,
            throttle: 1,
            brake: 1,
            handbrake: true,
            flag_310: true,
            gear: -1,
            analog_steer: true,
            analog_throttle: true,
            analog_brake: true,
        };
        let record = encode_replay_control(input, ReplayControlRecord::default(), false);
        assert_eq!(record.0, [0x04, 0x04, 0xc1, 0x01]);
        let decoded = decode_replay_control(record, 0);
        assert_eq!(decoded.actuators.steer, 0);
        assert_eq!(decoded.actuators.steer_alternate, -66);
        assert_eq!(decoded.actuators.throttle, -66);
        assert_eq!(decoded.actuators.brake, -66);
        assert_eq!(decoded.actuators.gear, -1);
        assert!(!decoded.control_410);
        assert!(!decoded.control_414);
        assert!(!decoded.control_418);
        assert!(decoded.repeat_complete);
        let two_ticks = ReplayControlRecord([0, 0, 2, 0]);
        let first = decode_replay_control(two_ticks, 0);
        assert!(!first.repeat_complete);
        assert_eq!(first.next_repeat, 1);
        let second = decode_replay_control(two_ticks, first.next_repeat);
        assert!(second.repeat_complete);
        assert_eq!(second.next_repeat, 0);
    }

    #[test]
    fn network_replay_suppresses_only_duplicate_records() {
        let previous = ReplayControlRecord([0x4f, 0x2f, 0xd1, 0xe3]);
        assert!(!network_replay_record_accepted(previous, previous));
        assert!(!network_replay_record_accepted(
            previous,
            ReplayControlRecord([0x4f, 0x2f, 0xd2, 0xe3])
        ));
        assert!(network_replay_record_accepted(
            previous,
            ReplayControlRecord([0x4f, 0x2f, 0xff, 0xe3])
        ));
        assert!(network_replay_record_accepted(
            previous,
            ReplayControlRecord([0x50, 0x2f, 0xd1, 0xe3])
        ));
        assert!(network_replay_record_accepted(
            previous,
            ReplayControlRecord([0x4f, 0x2f, 0xd1, 0xe2])
        ));
    }

    #[test]
    fn cmpr_header_parse_rejects_bad_magic_and_short_input() {
        let mut header = Vec::from(CMPR_MAGIC);
        header.extend_from_slice(&0x1234u32.to_le_bytes());
        assert_eq!(parse_cmpr_header(&header), Some(0x1234));
        // Wrong magic.
        let bad = *b"ZZZZ\x00\x00\x00\x00";
        assert_eq!(parse_cmpr_header(&bad), None);
        // Too short.
        assert_eq!(parse_cmpr_header(b"CMP"), None);
    }

    #[test]
    fn snapshot_indexing_and_capacity() {
        assert_eq!(SNAPSHOT_BUFFER_ENTRIES, 0x34bc);
        assert_eq!(snapshot_record_index(0, 0), Some(0));
        assert_eq!(snapshot_record_index(1, 5), Some(0x8ca + 5));
        assert_eq!(snapshot_record_index(5, 0x8c9), Some(5 * 0x8ca + 0x8c9));
        // Out of range.
        assert_eq!(snapshot_record_index(6, 0), None);
        assert_eq!(snapshot_record_index(0, 0x8ca), None);
    }

    #[test]
    fn orientation_compression_scales_and_clamps() {
        // 0.5 (0x8000) -> (0x8000*0x8000)>>16 = 0x4000.
        assert_eq!(compress_orientation(Fixed16::from_raw(0x8000)), 0x4000);
        // 1.0 would scale to 0x8000 but clamps to 0x7ff9 (fits i16).
        assert_eq!(compress_orientation(Fixed16::ONE), 0x7ff9);
        // -1.0 clamps to -0x7ff9.
        assert_eq!(compress_orientation(-Fixed16::ONE), -0x7ff9);
        // Zero stays zero.
        assert_eq!(compress_orientation(Fixed16::ZERO), 0);
    }

    #[test]
    fn snapshot_interp_factor_steps_in_quarters() {
        assert_eq!(snapshot_interp_factor(0), Fixed16::ZERO);
        assert_eq!(snapshot_interp_factor(1), Fixed16::from_raw(0x4000)); // 0.25
        assert_eq!(snapshot_interp_factor(2), Fixed16::from_raw(0x8000)); // 0.50
        assert_eq!(snapshot_interp_factor(3), Fixed16::from_raw(0xc000)); // 0.75
        // Subframe wraps modulo 4.
        assert_eq!(snapshot_interp_factor(4), Fixed16::ZERO);
    }
}
