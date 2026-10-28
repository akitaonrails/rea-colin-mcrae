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
//! Inferences: the `T`/`M`/`S` suffix is a rev band (top/mid/slow) and the
//! `F`/`R` prefix is the front/rear emitter of a band (the engine tick gives `F`
//! layers the front emitter's proximity and position and `R` layers the
//! rear's; see [`crate::engine_sound`] - an earlier reading of a left/right
//! stereo pair is corrected);
//! the per-car `<ABBR>` is a three-ish-character model code. The crossfade/pitch
//! rules are recovered in `crate::audio_engine`; the role of the shared
//! `OP1..OP5`/`POP2` pops remains unknown.

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

/// The 14 car wave slots in the recovered load order of `FUN_004872b0` (the
/// index is the slot offset from the car base slot), for the seven rally cars
/// (loader rows 0..=6). The kit cars (rows 7..=11) load [`KIT_CAR_POPS`] into
/// slots 7..=11 instead of `CHATTER`/`POPLOOP1..4`; see [`car_wave_names`].
pub const CAR_LOAD_SEQUENCE: [&str; 14] = [
    "FS", "RS", "FM", "RM", "WHINE", "FT", "RT", "CHATTER", "POPLOOP1", "POPLOOP2", "POPLOOP3",
    "POPLOOP4", "IGNITION", "GEAR",
];

/// The base directory every car wave name is appended to (`s_sounds_car_`,
/// after the install root `FUN_00496730`).
pub const CAR_SOUND_DIR: &str = r"sounds\car\";

/// A car wave path relative to the media source root: the loader prepends
/// the installation root `FUN_00496730` (`<CDPath>\game\`), so the
/// [`CAR_SOUND_DIR`] path sits under `game\`, as for the co-driver speech.
pub fn car_wave_source_path(name: &str) -> String {
    format!("game\\{CAR_SOUND_DIR}{name}")
}

/// Slots 7..=11 of the kit-car rows 7..=11 (`0x4f88a4` row table).
pub const KIT_CAR_POPS: [&str; 5] = ["POP1.WAV", "POP2.WAV", "POP3.WAV", "POP4.WAV", "POP5.WAV"];

/// The loader's per-row car (`0x4f88a4`, 12 rows of 14 name pointers): row 0
/// is SUBARU and row 11 is ESCORT, the reverse of [`CARS`].
pub fn car_for_row(row: u32) -> Option<&'static CarEngineSet> {
    let row = usize::try_from(row).ok()?;
    CARS.len().checked_sub(row + 1).map(|index| &CARS[index])
}

/// The 14 wave names of a loader row, relative to [`CAR_SOUND_DIR`], in slot
/// order: per-car layers as `<DIR>\<LAYER>-<ABBR>.WAV`, shared waves bare.
pub fn car_wave_names(row: u32) -> Option<[String; 14]> {
    let car = car_for_row(row)?;
    let kit = row >= 7;
    Some(std::array::from_fn(|slot| {
        match (slot, CAR_LOAD_SEQUENCE[slot]) {
            (7..=11, _) if kit => KIT_CAR_POPS[slot - 7].to_owned(),
            (_, layer @ ("FS" | "RS" | "FM" | "RM" | "FT" | "RT")) => car.layer_path(layer),
            (_, shared) => format!("{shared}.WAV"),
        }
    }))
}

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
    fn loader_rows_run_from_subaru_to_escort() {
        assert_eq!(car_for_row(0).unwrap().dir, "SUBARU");
        assert_eq!(car_for_row(11).unwrap().dir, "ESCORT");
        assert!(car_for_row(12).is_none());
        let rally = car_wave_names(0).unwrap();
        assert_eq!(rally[0], r"SUBARU\FS-SUB.WAV");
        assert_eq!(rally[4], "WHINE.WAV");
        assert_eq!(rally[7], "CHATTER.WAV");
        assert_eq!(rally[13], "GEAR.WAV");
        let kit = car_wave_names(11).unwrap();
        assert_eq!(kit[6], r"ESCORT\RT-ESC.WAV");
        assert_eq!(&kit[7..12], KIT_CAR_POPS.map(str::to_owned).as_slice());
        assert_eq!(kit[12], "IGNITION.WAV");
        assert!(car_wave_names(12).is_none());
        assert_eq!(
            car_wave_source_path("GEAR.WAV"),
            r"game\sounds\car\GEAR.WAV"
        );
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
