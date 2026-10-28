//! Weather and environmental-effect behaviour (item E10).
//!
//! # Provenance
//!
//! Weather is not chosen from a top-level enum at the particle layer: it is
//! driven by the stage's country/surface set. The config byte
//! `DAT_00d3ca80 + 0x168` low nibble selects a country/surface index (0..=8),
//! and the surface-material ids laid down on the stage are run through
//! capability tables (`FUN_00424a40`) that accumulate per-effect enable
//! counters (snow, dust, spray, grass, leaves, gravel, ...) which gate the
//! particle texture loads. A scalar weather-type global `DAT_00592ef8`
//! (getter `FUN_0046b220`; `3`/`4` = rain/wet) plus an intensity drive the
//! sky/fog and the full-screen precipitation overlay; the lighting global
//! `DAT_00592f00` (getter `FUN_0046b240`; `8` = night) disables the shadows
//! and tyre marks and darkens the particles.
//!
//! The wheel particles and tyre marks are [`crate::car_particles`]; the car
//! shadow is [`crate::car_shadow`]. Fog is software distance-haze
//! (color/density/far setters), the engine-level analog of the C3D type-2
//! per-vertex fog ([`crate::c3d_type2_fog`]) - there is no DirectX fog
//! render-state path.
//!
//! ## What is reconstructed vs. left as game data
//!
//! This module reconstructs the state gates, the effect-enable aggregation
//! (capability tables supplied by the caller), the named tuning-constant
//! defaults and the fog constants. The per-surface capability tables
//! (`0x4ec0f8..0x4ec788`) and the exact weather/intensity -> fog selection
//! are game data and are not vendored.
//!
//! See evidence `rea.official-eu.weather-effects`.

/// Config byte offset selecting the country/surface index (`& 0xf`).
pub const COUNTRY_CONFIG_OFFSET: usize = 0x168;

/// The country index forced for the special/bonus stage (`DAT_00d3ca88 != 0`).
pub const SPECIAL_STAGE_COUNTRY: u8 = 7;

/// The country/surface index for a stage: the low nibble of the config byte,
/// or [`SPECIAL_STAGE_COUNTRY`] when the special-stage flag is set.
pub fn country_index(config_byte: u8, special_stage: bool) -> u8 {
    if special_stage {
        SPECIAL_STAGE_COUNTRY
    } else {
        config_byte & 0x0f
    }
}

/// Whether the weather type is a rain/wet type (`3` or `4`), which arms the
/// thunder ambience (H03), the rain/wet zone audio, and the fog/overlay.
pub fn is_wet_weather(weather_type: u32) -> bool {
    weather_type == 3 || weather_type == 4
}

/// Whether the stage's lighting (`FUN_0046b240`) is night (`8`): shadows
/// off, the particle alpha floored, no fog.
pub fn is_night(lighting: u32) -> bool {
    lighting == crate::stage_weather::NIGHT_LIGHTING
}

/// Accumulate one effect's enable counter over the surface ids present on a
/// stage, reproducing `FUN_00424a40`: for each distinct present surface id, add
/// that id's capability byte for the effect. A positive result enables the
/// effect (e.g. snow is enabled iff its counter is `> 0`). The capability table
/// is game data supplied by the caller.
pub fn accumulate_effect_enable(present_surface_ids: &[usize], capability: &[u8]) -> u32 {
    let mut seen = std::collections::BTreeSet::new();
    let mut total = 0u32;
    for &id in present_surface_ids {
        if seen.insert(id)
            && let Some(&byte) = capability.get(id)
        {
            total += byte as u32;
        }
    }
    total
}

pub const SPLASH_EFFECT_COUNTER: usize = 0;
pub const DUST_EFFECT_COUNTER: usize = 2;
pub const SNOW_EFFECT_COUNTER: usize = 5;

/// Weather gates whose global counter identities are statically recovered.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StageWeatherEffects {
    pub splash: bool,
    pub dust: bool,
    pub snow: bool,
}

impl StageWeatherEffects {
    #[must_use]
    pub fn from_counters(counters: &[u32; crate::drivetrain::SURFACE_CAPABILITY_COUNT]) -> Self {
        Self {
            splash: counters[SPLASH_EFFECT_COUNTER] > 0,
            dust: counters[DUST_EFFECT_COUNTER] > 0,
            snow: counters[SNOW_EFFECT_COUNTER] > 0,
        }
    }
}

/// Named defaults from the particle/track tuning block (`0x4ebe00`, stride
/// `0x28`, name + value; a text tuning file overrides these at load). These are
/// engine behavior constants, encoded like the drivetrain/grip constants.
pub mod tuning {
    use crate::fixed_point::Fixed16;

    /// `Initial_dust_size` (Q16.16 ~0.2).
    pub const INITIAL_DUST_SIZE: Fixed16 = Fixed16::from_raw(0x3333);
    /// `Initial_follow_size` (Q16.16 ~0.2), the spawn size addend.
    pub const INITIAL_FOLLOW_SIZE: Fixed16 = Fixed16::from_raw(0x3333);
    /// `Follow_size_growth` (Q16.16), per-frame size growth.
    pub const FOLLOW_SIZE_GROWTH: Fixed16 = Fixed16::from_raw(0x51e);
    /// `Max_dust_size` (2.0 Q16.16), used directly by the native clamp.
    pub const MAX_DUST_SIZE: Fixed16 = Fixed16::from_raw(0x2_0000);
    /// `Draw_dirt_my_viewport`: particle budget for the own-car viewport.
    pub const DRAW_DIRT_MY_VIEWPORT: u32 = 100;
    /// `Draw_dirt_other_viewport`: particle budget for the other viewport.
    pub const DRAW_DIRT_OTHER_VIEWPORT: u32 = 30;
    /// `Gas_alpha_delta`: per-frame alpha decrement for the gas/dust class.
    pub const GAS_ALPHA_DELTA: u32 = 3;
    /// `Smoke_alpha_delta`.
    pub const SMOKE_ALPHA_DELTA: u32 = 2;
    /// `Dry_snow_alpha_delta`.
    pub const DRY_SNOW_ALPHA_DELTA: u32 = 2;
    /// `Loose_sand_alpha_delta`.
    pub const LOOSE_SAND_ALPHA_DELTA: u32 = 3;
    /// `Spray_wetness`.
    pub const SPRAY_WETNESS: u32 = 8;
    /// `Splash_wetness`.
    pub const SPLASH_WETNESS: u32 = 0x28;
    /// `Track_Decay_Life`: tyre-mark lifetime in frames.
    pub const TRACK_DECAY_LIFE: u32 = 0x4d8;
    /// `Track_Decay_RATE`: per-frame tyre-mark decay step.
    pub const TRACK_DECAY_RATE: u32 = 0x7c;
}

/// Sky / horizon / fog setup (`FUN_0048b960`, `FUN_00480d80`).
pub mod sky {
    use crate::fixed_point::Fixed16;

    /// Config bit (`DAT_00d3ca80 + 0x25 & 0x80`) that suppresses the `.hzn`
    /// horizon model when set.
    pub const HORIZON_SKIP_BIT: u8 = 0x80;

    /// The recovered fog density values (f32), rising with weather intensity.
    pub const FOG_DENSITIES: [f32; 4] = [0.2, 0.4, 0.6, 0.8];

    /// The recovered fog far-distance values (Q16.16): 20, 40, 60, 70, 80.
    pub const FOG_FAR_DISTANCES: [Fixed16; 5] = [
        Fixed16::from_raw(0x14_0000),
        Fixed16::from_raw(0x28_0000),
        Fixed16::from_raw(0x3c_0000),
        Fixed16::from_raw(0x46_0000),
        Fixed16::from_raw(0x50_0000),
    ];

    /// The recovered packed fog colors (ARGB): grey, bluish, and white.
    pub const FOG_COLOR_GREY: u32 = 0x7864_6464;
    /// Bluish fog color.
    pub const FOG_COLOR_BLUISH: u32 = 0x7864_492a;
    /// White fog color.
    pub const FOG_COLOR_WHITE: u32 = 0xffff_ffff;

    /// Complete inputs to the software fog setters.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct FogSettings {
        pub color: u32,
        pub density: f32,
        pub far: Fixed16,
        pub source_base: u32,
    }

    /// Select fog from weather type, intensity, country and the lighting.
    /// Weather 5 always uses density 0.8/far 70; weather 3 uses intensity rows
    /// 0..2, weather 4 rows 1..3. Other cases or night disable fog.
    #[must_use]
    pub fn fog_settings(
        weather_type: u32,
        intensity: u32,
        country: u8,
        night: bool,
    ) -> Option<FogSettings> {
        let color = match country {
            3 | 6 => FOG_COLOR_BLUISH,
            2 | 4 if weather_type == 4 => FOG_COLOR_WHITE,
            _ if weather_type == 4 => FOG_COLOR_WHITE,
            _ => FOG_COLOR_GREY,
        };
        if weather_type == 5 {
            return Some(FogSettings {
                color,
                density: FOG_DENSITIES[3],
                far: FOG_FAR_DISTANCES[3],
                source_base: 0x50_0000,
            });
        }
        if night || !(1..=3).contains(&intensity) {
            return None;
        }
        let row = usize::try_from(intensity - 1).ok()?;
        match weather_type {
            3 => Some(FogSettings {
                color,
                density: FOG_DENSITIES[row],
                far: FOG_FAR_DISTANCES[row],
                source_base: 0x8c_0000,
            }),
            4 => Some(FogSettings {
                color,
                density: FOG_DENSITIES[row + 1],
                far: FOG_FAR_DISTANCES[row + 1],
                source_base: 0x50_0000,
            }),
            _ => None,
        }
    }

    /// The full-screen precipitation overlay tint for wet weather
    /// (`IMAGES\SNOW.BMP`), RGBA `(100,100,100,255)`.
    pub const PRECIP_OVERLAY_RGBA: (u8, u8, u8, u8) = (100, 100, 100, 255);

    /// Whether the `.hzn` horizon model is loaded for a given config byte (it is
    /// suppressed when [`HORIZON_SKIP_BIT`] is set).
    pub fn loads_horizon(config_byte: u8) -> bool {
        config_byte & HORIZON_SKIP_BIT == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn country_index_masks_or_forces_special() {
        assert_eq!(country_index(0x05, false), 5);
        assert_eq!(country_index(0xf8, false), 8);
        assert_eq!(country_index(0x05, true), SPECIAL_STAGE_COUNTRY);
    }

    #[test]
    fn weather_and_detail_gates() {
        assert!(is_wet_weather(3));
        assert!(is_wet_weather(4));
        assert!(!is_wet_weather(0));
        assert!(!is_wet_weather(5));
        assert!(is_night(8));
        assert!(!is_night(2));
    }

    #[test]
    fn effect_enable_accumulates_distinct_surfaces() {
        // capability[id] = per-surface enable weight for this effect.
        let capability = [0u8, 1, 0, 2, 0, 3];
        // Surfaces 1, 3, 5 present (1 is listed twice - counted once).
        let present = [1usize, 3, 5, 1];
        assert_eq!(accumulate_effect_enable(&present, &capability), 1 + 2 + 3);
        // No enabling surfaces -> 0 (effect disabled).
        assert_eq!(accumulate_effect_enable(&[0, 2, 4], &capability), 0);
        // Out-of-range ids are ignored.
        assert_eq!(accumulate_effect_enable(&[99], &capability), 0);
    }

    #[test]
    fn stage_weather_gates_use_the_recovered_counter_indices() {
        let mut counters = [0; crate::drivetrain::SURFACE_CAPABILITY_COUNT];
        assert_eq!(
            StageWeatherEffects::from_counters(&counters),
            StageWeatherEffects {
                splash: false,
                dust: false,
                snow: false,
            }
        );
        counters[SPLASH_EFFECT_COUNTER] = 1;
        counters[DUST_EFFECT_COUNTER] = 2;
        counters[SNOW_EFFECT_COUNTER] = 3;
        counters[1] = 99;
        let effects = StageWeatherEffects::from_counters(&counters);
        assert!(effects.splash && effects.dust && effects.snow);
    }

    #[test]
    fn tuning_defaults_are_byte_exact() {
        assert_eq!(tuning::TRACK_DECAY_LIFE, 0x4d8);
        assert_eq!(tuning::TRACK_DECAY_RATE, 0x7c);
        assert_eq!(tuning::DRAW_DIRT_MY_VIEWPORT, 100);
        assert_eq!(tuning::DRAW_DIRT_OTHER_VIEWPORT, 30);
        assert_eq!(tuning::INITIAL_FOLLOW_SIZE.raw(), 0x3333);
        assert_eq!(tuning::FOLLOW_SIZE_GROWTH.raw(), 0x51e);
    }

    #[test]
    fn fog_selection_covers_weather_intensity_country_and_detail() {
        use sky::*;
        let wet = fog_settings(3, 2, 3, false).unwrap();
        assert_eq!(wet.color, FOG_COLOR_BLUISH);
        assert_eq!(wet.density, 0.4);
        assert_eq!(wet.far, FOG_FAR_DISTANCES[1]);
        assert_eq!(wet.source_base, 0x8c_0000);

        let rain = fog_settings(4, 3, 2, false).unwrap();
        assert_eq!(rain.color, FOG_COLOR_WHITE);
        assert_eq!(rain.density, 0.8);
        assert_eq!(rain.far, FOG_FAR_DISTANCES[3]);
        assert_eq!(rain.source_base, 0x50_0000);

        let override_settings = fog_settings(5, 0, 6, true).unwrap();
        assert_eq!(override_settings.color, FOG_COLOR_BLUISH);
        assert_eq!(override_settings.density, 0.8);
        assert_eq!(override_settings.far, FOG_FAR_DISTANCES[3]);
        assert!(fog_settings(4, 0, 0, false).is_none());
        assert!(fog_settings(3, 2, 0, true).is_none());
        assert!(fog_settings(2, 2, 0, false).is_none());
        assert_eq!(tuning::MAX_DUST_SIZE.raw(), 0x2_0000);
    }

    #[test]
    fn sky_horizon_gate_and_fog_constants() {
        use sky::{FOG_COLOR_GREY, FOG_FAR_DISTANCES, loads_horizon};
        assert!(loads_horizon(0x00));
        assert!(loads_horizon(0x7f));
        assert!(!loads_horizon(0x80));
        assert_eq!(FOG_COLOR_GREY, 0x7864_6464);
        // Far distances are 20.0 .. 80.0 in Q16.16.
        assert_eq!(FOG_FAR_DISTANCES[0].raw(), 0x14_0000);
        assert_eq!(FOG_FAR_DISTANCES[4].raw(), 0x50_0000);
    }
}
