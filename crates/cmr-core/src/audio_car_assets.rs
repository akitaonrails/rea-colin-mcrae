//! Recovered car engine sound model.
//!
//! The car sound loader `FUN_004872b0` (which references the `sounds\car\`
//! prefix 28 times) registers a per-car set of layered engine loops. Each wave
//! path has the recovered shape `<CAR_DIR>\<LAYER>-<ABBR>.WAV`, for example
//! `ESCORT\RT-ESC.WAV`, built under the `sounds\car\` base directory.
//!
//! Twelve cars each define six layer samples in the stored order
//! `RT, FT, RM, FM, RS, FS`, alongside shared engine sounds (gear change,
//! ignition, idle/backfire pops, transmission chatter and whine). This module
//! records that structure as metadata (relative paths, original backslash form -
//! never payload). See evidence `rea.official-eu.audio-wave-inventory`.
//!
//! Inferences: the `F`/`R` layer prefix is the on-power / off-power (overrun)
//! engine state and the `T`/`M`/`S` suffix a rev band, the classic six-loop
//! crossfaded engine model; the per-car `<ABBR>` is a three-ish-character model
//! code. Unknowns: the exact crossfade/pitch rules (the play sites are not yet
//! recovered), the sample rate/format of these waves, and the role of the shared
//! `OP1..OP5`/`POP2` pops.

/// One car's engine sound set: its directory name and the filename abbreviation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CarEngineSet {
    /// Directory under `sounds\car\`, e.g. `ESCORT`.
    pub dir: &'static str,
    /// Filename model code, e.g. `ESC` in `RT-ESC.WAV`.
    pub abbr: &'static str,
}

/// The six engine layer codes, in the recovered stored order.
pub const ENGINE_LAYERS: [&str; 6] = ["RT", "FT", "RM", "FM", "RS", "FS"];

/// The twelve cars with engine sound sets, in the recovered stored order.
pub const CARS: [CarEngineSet; 12] = [
    CarEngineSet {
        dir: "ESCORT",
        abbr: "ESC",
    },
    CarEngineSet {
        dir: "MEGANE",
        abbr: "MEG",
    },
    CarEngineSet {
        dir: "GOLF",
        abbr: "GOLF",
    },
    CarEngineSet {
        dir: "SEAT",
        abbr: "SEAT",
    },
    CarEngineSet {
        dir: "SKODA",
        abbr: "SKO",
    },
    CarEngineSet {
        dir: "AUDI",
        abbr: "AUD",
    },
    CarEngineSet {
        dir: "LANCIA",
        abbr: "LAN",
    },
    CarEngineSet {
        dir: "RS200",
        abbr: "200",
    },
    CarEngineSet {
        dir: "TOYOTA",
        abbr: "TOY",
    },
    CarEngineSet {
        dir: "COSWORTH",
        abbr: "COS",
    },
    CarEngineSet {
        dir: "MITSUB",
        abbr: "MIT",
    },
    CarEngineSet {
        dir: "SUBARU",
        abbr: "SUB",
    },
];

/// Shared (non-per-car) engine sound filenames.
pub const SHARED_ENGINE_SOUNDS: [&str; 8] = [
    "GEAR.WAV",
    "IGNITION.WAV",
    "POPLOOP1.WAV",
    "POPLOOP2.WAV",
    "POPLOOP3.WAV",
    "POPLOOP4.WAV",
    "CHATTER.WAV",
    "WHINE.WAV",
];

/// Recovered sample rate (`0x2b11` = 11025) and bit depth (`0x10` = 16, mono) of
/// every car wave: the loader `FUN_004872b0` uses the identical
/// `(path, 0x2b11, 0x10, 0)` tuple for all 14 loads.
pub const CAR_WAVE_RATE_HZ: u32 = 11_025;
pub const CAR_WAVE_BITS: u16 = 16;

/// The 14 car waves in the recovered load order of `FUN_004872b0` (the index is
/// the slot offset from the car base slot). The six per-car engine layers and
/// `WHINE` interleave with the shared chatter/pop-loop/ignition/gear waves;
/// slots 8..15 (the shared tail) load only when `DAT_00550c08 == 2`.
pub const CAR_LOAD_SEQUENCE: [&str; 14] = [
    "FS", "RS", "FM", "RM", "WHINE", "FT", "RT", "CHATTER", "POPLOOP1", "POPLOOP2", "POPLOOP3",
    "POPLOOP4", "IGNITION", "GEAR",
];

/// Map the car-selection nibble - the low 4 bits of the config byte at
/// `DAT_00d3ca80 + 0xb6` - to the loader's per-car row index, reproducing the
/// recovered `switch` in `FUN_004872b0`. Unlisted values (0 and 12..15) select
/// row 0 (the loader's default, whose base symbols are SUBARU).
pub fn car_row_index_for_selector(selector_nibble: u8) -> u32 {
    match selector_nibble & 0x0f {
        1 => 8,
        2 => 7,
        3 => 6,
        4 => 4,
        5 => 0xb,
        6 => 1,
        7 => 9,
        8 => 5,
        9 => 3,
        0xa => 2,
        0xb => 10,
        _ => 0,
    }
}

impl CarEngineSet {
    /// The recovered relative path of one engine layer, e.g.
    /// `ESCORT\RT-ESC.WAV`. `layer` is one of [`ENGINE_LAYERS`].
    pub fn layer_path(&self, layer: &str) -> String {
        format!(r"{}\{}-{}.WAV", self.dir, layer, self.abbr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twelve_cars_and_six_layers_are_recovered() {
        assert_eq!(CARS.len(), 12);
        assert_eq!(ENGINE_LAYERS.len(), 6);
        assert_eq!(CARS[0].dir, "ESCORT");
        assert_eq!(CARS[11].dir, "SUBARU");
    }

    #[test]
    fn layer_paths_match_the_recovered_filenames() {
        assert_eq!(CARS[0].layer_path("RT"), r"ESCORT\RT-ESC.WAV");
        assert_eq!(CARS[7].layer_path("FS"), r"RS200\FS-200.WAV");
        assert_eq!(CARS[2].layer_path("RM"), r"GOLF\RM-GOLF.WAV");
    }

    #[test]
    fn every_car_builds_six_distinct_layer_paths() {
        for car in &CARS {
            let mut paths: Vec<String> = ENGINE_LAYERS.iter().map(|l| car.layer_path(l)).collect();
            paths.sort();
            paths.dedup();
            assert_eq!(paths.len(), 6, "{} has duplicate layer paths", car.dir);
        }
    }

    #[test]
    fn shared_engine_sounds_are_recorded() {
        assert!(SHARED_ENGINE_SOUNDS.contains(&"IGNITION.WAV"));
        assert!(SHARED_ENGINE_SOUNDS.contains(&"GEAR.WAV"));
        assert_eq!(SHARED_ENGINE_SOUNDS.len(), 8);
    }

    #[test]
    fn car_waves_are_11025_hz_16_bit() {
        assert_eq!(CAR_WAVE_RATE_HZ, 11_025);
        assert_eq!(CAR_WAVE_BITS, 16);
    }

    #[test]
    fn load_sequence_matches_the_recovered_order() {
        assert_eq!(CAR_LOAD_SEQUENCE.len(), 14);
        assert_eq!(CAR_LOAD_SEQUENCE[0], "FS");
        assert_eq!(CAR_LOAD_SEQUENCE[4], "WHINE");
        assert_eq!(CAR_LOAD_SEQUENCE[7], "CHATTER");
        assert_eq!(CAR_LOAD_SEQUENCE[13], "GEAR");
    }

    #[test]
    fn selector_remap_matches_the_recovered_switch() {
        // Listed cases.
        assert_eq!(car_row_index_for_selector(1), 8);
        assert_eq!(car_row_index_for_selector(5), 0xb);
        assert_eq!(car_row_index_for_selector(0xb), 10);
        assert_eq!(car_row_index_for_selector(0xa), 2);
        // Default (0 and 12..15, plus the high nibble ignored).
        assert_eq!(car_row_index_for_selector(0), 0);
        assert_eq!(car_row_index_for_selector(12), 0);
        assert_eq!(car_row_index_for_selector(0xf), 0);
        assert_eq!(car_row_index_for_selector(0x31), 8); // high nibble masked off
    }
}
