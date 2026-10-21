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
}
