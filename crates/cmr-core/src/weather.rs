//! Weather, particle, shadow, and environmental-effect behavior (item E10).
//!
//! # Provenance
//!
//! Weather is not chosen from a top-level enum at the particle layer: it is
//! driven by the stage's country/surface set. The config byte
//! `DAT_00d3ca80 + 0x168` low nibble selects a country/surface index (0..=8),
//! and the surface-material ids laid down on the stage are run through
//! capability tables (`FUN_00424a40`) that accumulate per-effect enable
//! counters (snow, dust, spray, grass, leaves, gravel, ...). A scalar
//! weather-type global `DAT_00592ef8` (getter `FUN_0046b220`; `3`/`4` = rain/wet)
//! plus an intensity drive the sky/fog and the full-screen precipitation
//! overlay, and the detail-level global `DAT_00592f00` (getter `FUN_0046b240`;
//! `8` = lowest detail) throttles particles and disables shadows.
//!
//! Three particle mechanisms: a per-car simulation pool (`DAT_00524480`,
//! `0x28`-byte records, 100/car) walked by `FUN_00428bc0` into camera-facing,
//! distance-culled billboards; a billboard/decal emitter (`FUN_00424f40`,
//! `0x50`-byte records, 8 ring pools of 100 at `DAT_005144d0`); and the
//! tyre-mark / track-decay fade (`Track_Decay_Life`/`Track_Decay_RATE`). The car
//! shadow is a projected soft-edged polygon shadow (a 64x64 blob texture, grey
//! `(100,100,100,170)`) rebuilt each frame (`FUN_004a51f0`). Fog is software
//! distance-haze (color/density/far setters), the engine-level analog of the
//! C3D type-2 per-vertex fog ([`crate::c3d_type2_fog`]) - there is no DirectX
//! fog render-state path.
//!
//! ## What is reconstructed vs. left as game data
//!
//! This module reconstructs the state gates, the particle-enable aggregation
//! algorithm (capability tables supplied by the caller), the particle record
//! layout + type dispatch + spawn/size/alpha math, the named tuning-constant
//! defaults (engine behavior constants, like the drivetrain/grip constants),
//! and the shadow/fog constants. The per-surface capability tables
//! (`0x4ec0f8..0x4ec788`), the sprite-sheet image sets, and the exact
//! weather/intensity -> fog selection are game/asset data and are not vendored;
//! the texture image names are referenced only as evidence.
//!
//! See evidence `rea.official-eu.weather-effects`.

use crate::fixed_point::Fixed16;

/// Config byte offset selecting the country/surface index (`& 0xf`).
pub const COUNTRY_CONFIG_OFFSET: usize = 0x168;

/// The country index forced for the special/bonus stage (`DAT_00d3ca88 != 0`).
pub const SPECIAL_STAGE_COUNTRY: u8 = 7;

/// The detail-level value at which shadows are disabled and particles are
/// throttled to the fixed low-detail alpha (`FUN_0046b240() == 8`).
pub const LOW_DETAIL_LEVEL: u32 = 8;

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

/// Whether the detail level is the lowest (`8`): shadows off, particle alpha
/// forced to the low-detail floor.
pub fn is_low_detail(detail_level: u32) -> bool {
    detail_level == LOW_DETAIL_LEVEL
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

    #[must_use]
    pub fn allows(self, effect: particles::ParticleEffect) -> bool {
        match effect {
            particles::ParticleEffect::Dust => self.dust,
            particles::ParticleEffect::Snow | particles::ParticleEffect::SnowCloud => self.snow,
            particles::ParticleEffect::Splash => self.splash,
            particles::ParticleEffect::Clods
            | particles::ParticleEffect::Leaves
            | particles::ParticleEffect::GravelDryMud => true,
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
    /// `Max_dust_size` (raw tuning value; scale uncertain).
    pub const MAX_DUST_SIZE: i32 = 2;
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

/// Particle subsystem: pool layout, type dispatch, and spawn/size/alpha math.
pub mod particles {
    use super::tuning::{FOLLOW_SIZE_GROWTH, INITIAL_FOLLOW_SIZE};
    use super::*;

    /// Stride of one per-car simulation record (`DAT_00524480`).
    pub const SIM_RECORD_STRIDE: usize = 0x28;
    /// Simulation records per car.
    pub const SIM_RECORDS_PER_CAR: usize = 100;
    /// Stride of one billboard/decal record (`DAT_005144d0`).
    pub const BILLBOARD_RECORD_STRIDE: usize = 0x50;
    /// Number of billboard ring pools.
    pub const BILLBOARD_POOLS: usize = 8;
    /// Billboard records per pool.
    pub const BILLBOARD_RECORDS_PER_POOL: usize = 100;

    /// The low-detail fixed particle alpha floor (`FUN_0046b240() == 8`).
    pub const LOW_DETAIL_ALPHA: u32 = 0x50;
    /// The low-detail forced initial animation frame.
    pub const LOW_DETAIL_FRAME: u8 = 0x32;

    /// Near/far distance-cull window for a billboard (Q16.16: 3.0 .. 100.0).
    pub const CULL_NEAR: Fixed16 = Fixed16::from_raw(0x3_0000);
    /// Far cull bound.
    pub const CULL_FAR: Fixed16 = Fixed16::from_raw(0x64_0000);

    /// A debris/precipitation particle class, selected by the record type byte
    /// (`+0x21` in the sim record / `+0x4a` in the billboard record).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ParticleEffect {
        /// Dust family (`0x01`/`0x06`/`0x09`/`0x16`/`0x18`).
        Dust,
        /// Clods (`0x02`).
        Clods,
        /// Falling/screen snow (`0x07`).
        Snow,
        /// Snow cloud (`0x08`).
        SnowCloud,
        /// Water splash / spray (`0x0b`).
        Splash,
        /// Leaves (`0x0c`).
        Leaves,
        /// Gravel / dry mud (`0x15`/`0x17`/`0x1a`).
        GravelDryMud,
    }

    /// Classify a particle type byte into its effect, reproducing the type
    /// dispatch in `FUN_00428bc0`.
    pub fn particle_effect(type_byte: u8) -> Option<ParticleEffect> {
        Some(match type_byte {
            0x01 | 0x06 | 0x09 | 0x16 | 0x18 => ParticleEffect::Dust,
            0x02 => ParticleEffect::Clods,
            0x07 => ParticleEffect::Snow,
            0x08 => ParticleEffect::SnowCloud,
            0x0b => ParticleEffect::Splash,
            0x0c => ParticleEffect::Leaves,
            0x15 | 0x17 | 0x1a => ParticleEffect::GravelDryMud,
            _ => return None,
        })
    }

    /// Apply the stage weather gates after the recovered type dispatch.
    pub fn gated_particle_effect(
        type_byte: u8,
        effects: StageWeatherEffects,
    ) -> Option<ParticleEffect> {
        particle_effect(type_byte).filter(|effect| effects.allows(*effect))
    }

    /// Type-selected sprite-table identity from `FUN_00424f40`.
    #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
    pub enum ParticleSpriteTable {
        #[default]
        Default,
        Type6,
        Type7,
        Type8,
        Type9,
        Type16,
        Type18,
        Type1b,
    }

    #[must_use]
    pub fn particle_sprite_table(type_byte: u8) -> ParticleSpriteTable {
        match type_byte {
            6 => ParticleSpriteTable::Type6,
            7 => ParticleSpriteTable::Type7,
            8 => ParticleSpriteTable::Type8,
            9 => ParticleSpriteTable::Type9,
            0x16 => ParticleSpriteTable::Type16,
            0x18 => ParticleSpriteTable::Type18,
            0x1b => ParticleSpriteTable::Type1b,
            _ => ParticleSpriteTable::Default,
        }
    }

    /// The recovered 80-byte spawn record; unknown scalar semantics retain offsets.
    #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
    pub struct ParticleRecord {
        pub position: [i32; 3],
        pub vector_24: [i32; 3],
        pub word_30: i32,
        pub word_34: i32,
        pub word_3c: i32,
        pub sprite_table: ParticleSpriteTable,
        pub word_46: u16,
        pub tuning_48: u16,
        pub type_byte: u8,
        pub frame: u8,
        pub flags: u8,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct ParticleSpawn {
        pub position: [i32; 3],
        pub vector_24: [i32; 3],
        pub word_30: i32,
        pub word_34: i32,
        pub word_46: u16,
        pub type_byte: u8,
        pub tuning: [u16; 2],
        pub random: u32,
        pub low_detail: bool,
        pub global_flag: bool,
    }

    #[derive(Debug)]
    pub struct ParticlePool {
        records: [ParticleRecord; BILLBOARD_RECORDS_PER_POOL],
        next: usize,
    }

    impl Default for ParticlePool {
        fn default() -> Self {
            Self {
                records: [ParticleRecord::default(); BILLBOARD_RECORDS_PER_POOL],
                next: 0,
            }
        }
    }

    impl ParticlePool {
        #[must_use]
        pub fn records(&self) -> &[ParticleRecord; BILLBOARD_RECORDS_PER_POOL] {
            &self.records
        }

        #[must_use]
        pub fn next_index(&self) -> usize {
            self.next
        }

        /// Overwrite the next slot, advance modulo 100, and report whether it wrapped.
        pub fn spawn(&mut self, input: ParticleSpawn) -> (bool, &ParticleRecord) {
            let index = self.next;
            let previous_flags = self.records[index].flags;
            self.next = (index + 1) % BILLBOARD_RECORDS_PER_POOL;
            let wrapped = self.next == 0;
            let tuning_48 = match input.type_byte {
                6 | 7 | 8 | 9 | 0x1b => input.tuning[1],
                _ => input.tuning[0],
            };
            self.records[index] = ParticleRecord {
                position: input.position,
                vector_24: input.vector_24,
                word_30: input.word_30,
                word_34: input.word_34,
                word_3c: 0,
                sprite_table: particle_sprite_table(input.type_byte),
                word_46: input.word_46,
                tuning_48,
                type_byte: input.type_byte,
                frame: initial_anim_frame(input.random, input.low_detail),
                flags: ((input.global_flag as u8) << 1) | (previous_flags & 0xfd) | 1,
            };
            (wrapped, &self.records[index])
        }
    }

    #[derive(Debug, Default)]
    pub struct ParticlePools([ParticlePool; BILLBOARD_POOLS]);

    impl ParticlePools {
        #[must_use]
        pub fn pool(&self, index: usize) -> Option<&ParticlePool> {
            self.0.get(index)
        }

        /// Spawn into one of the eight pools; an out-of-range pool is rejected safely.
        pub fn spawn(
            &mut self,
            pool: usize,
            input: ParticleSpawn,
        ) -> Option<(bool, &ParticleRecord)> {
            self.0.get_mut(pool).map(|pool| pool.spawn(input))
        }
    }

    /// The initial animation frame at spawn (`FUN_00424f40` / `FUN_004a96e0`):
    /// the average of the three low bytes of a random word, or the fixed
    /// low-detail frame when the detail level is lowest.
    pub fn initial_anim_frame(random: u32, low_detail: bool) -> u8 {
        if low_detail {
            return LOW_DETAIL_FRAME;
        }
        let a = random & 0xff;
        let b = (random >> 8) & 0xff;
        let c = (random >> 16) & 0xff;
        ((a + b + c) / 3) as u8
    }

    /// Whether a billboard at the given camera distance is within the cull
    /// window (`3.0 < dist < 100.0`), reproducing `FUN_00428bc0`'s LOD gate.
    pub fn is_visible(distance: Fixed16) -> bool {
        distance > CULL_NEAR && distance < CULL_FAR
    }

    /// The follow particle's size after `frames` of growth: the spawn size plus
    /// the per-frame growth, clamped to a caller-supplied maximum (the game's
    /// `Max_dust_size`, whose scale is uncertain).
    pub fn follow_size(frames: i32, max: Fixed16) -> Fixed16 {
        let grown =
            Fixed16::from_raw(INITIAL_FOLLOW_SIZE.raw() + FOLLOW_SIZE_GROWTH.raw() * frames.max(0));
        grown.min(max)
    }

    /// The rendered particle alpha, reproducing `FUN_00428bc0`:
    /// `min(0xff, pool_alpha * multiplier)`, floored to the low-detail value
    /// when the detail level is lowest. The multiplier is `3` (own viewport) or
    /// `1` (other), per the recovered `(config & ...)` select.
    pub fn particle_alpha(pool_alpha: u32, multiplier: u32, low_detail: bool) -> u32 {
        let scaled = (pool_alpha * multiplier).min(0xff);
        if low_detail {
            scaled.max(LOW_DETAIL_ALPHA)
        } else {
            scaled
        }
    }
}

/// Car-shadow subsystem (`FUN_004a4f20`/`FUN_004a51f0`, "Cm" shadows): a
/// projected, textured, soft-edged polygon shadow flattened to the ground.
pub mod shadow {
    /// Blob texture dimension (`images\shadpc.bmp` + alpha mask).
    pub const BLOB_SIZE: u32 = 64;
    /// Shadow RGBA: grey `(100,100,100)` at alpha `170` (~0.67).
    pub const SHADOW_RGBA: (u8, u8, u8, u8) = (100, 100, 100, 170);
    /// Soft-edge per-vertex alpha ramp written across a shadow polygon.
    pub const SOFT_EDGE_ALPHAS: [f32; 5] = [0.0, 0.3, 0.5, 0.7, 1.0];
    /// Ground-plane coordinate the shadow is flattened onto (Q16.16 -2.0).
    pub const GROUND_PLANE_Q16: i32 = -0x2_0000;

    /// A shadow entry's primitive shape (`piVar13[0x1b]` in `FUN_004a51f0`).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ShadowPrimitive {
        /// Type 0: a 4-vertex strip.
        Strip4,
        /// Type 1: indexed with the 11-entry index table (`DAT_00b73420`).
        Indexed11,
        /// Type 2: indexed with the 15-entry index table (`DAT_00b73890`).
        Indexed15,
    }

    /// Classify a shadow entry type byte (0/1/2).
    pub fn shadow_primitive(entry_type: u8) -> Option<ShadowPrimitive> {
        Some(match entry_type {
            0 => ShadowPrimitive::Strip4,
            1 => ShadowPrimitive::Indexed11,
            2 => ShadowPrimitive::Indexed15,
            _ => return None,
        })
    }

    /// The vertex/index count a shadow primitive draws.
    pub fn shadow_primitive_counts(primitive: ShadowPrimitive) -> (u32, u32) {
        match primitive {
            ShadowPrimitive::Strip4 => (4, 0),
            ShadowPrimitive::Indexed11 => (0, 11),
            ShadowPrimitive::Indexed15 => (0, 15),
        }
    }

    /// Convert a Q16.16 vertex coordinate to the float the shadow builder uses
    /// (`* 1/65536`).
    pub fn fixed_to_float(q16: i32) -> f32 {
        q16 as f32 / 65536.0
    }
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
        assert!(is_low_detail(8));
        assert!(!is_low_detail(2));
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
    fn particle_dispatch_applies_only_the_matching_weather_gates() {
        use particles::{ParticleEffect, gated_particle_effect};
        let disabled = StageWeatherEffects {
            splash: false,
            dust: false,
            snow: false,
        };
        assert_eq!(gated_particle_effect(0x01, disabled), None);
        assert_eq!(gated_particle_effect(0x07, disabled), None);
        assert_eq!(gated_particle_effect(0x08, disabled), None);
        assert_eq!(gated_particle_effect(0x0b, disabled), None);
        assert_eq!(
            gated_particle_effect(0x02, disabled),
            Some(ParticleEffect::Clods)
        );
        assert_eq!(
            gated_particle_effect(0x0c, disabled),
            Some(ParticleEffect::Leaves)
        );
        let enabled = StageWeatherEffects {
            splash: true,
            dust: true,
            snow: true,
        };
        assert_eq!(
            gated_particle_effect(0x01, enabled),
            Some(ParticleEffect::Dust)
        );
        assert_eq!(
            gated_particle_effect(0x07, enabled),
            Some(ParticleEffect::Snow)
        );
        assert_eq!(
            gated_particle_effect(0x0b, enabled),
            Some(ParticleEffect::Splash)
        );
        assert_eq!(gated_particle_effect(0x7f, enabled), None);
    }

    #[test]
    fn particle_sprite_table_and_tuning_follow_the_type_switch() {
        use particles::{ParticlePools, ParticleSpawn, ParticleSpriteTable, particle_sprite_table};
        let cases = [
            (1, ParticleSpriteTable::Default),
            (6, ParticleSpriteTable::Type6),
            (7, ParticleSpriteTable::Type7),
            (8, ParticleSpriteTable::Type8),
            (9, ParticleSpriteTable::Type9),
            (0x16, ParticleSpriteTable::Type16),
            (0x18, ParticleSpriteTable::Type18),
            (0x1b, ParticleSpriteTable::Type1b),
            (0x7f, ParticleSpriteTable::Default),
        ];
        for (type_byte, expected) in cases {
            assert_eq!(particle_sprite_table(type_byte), expected);
        }
        let input = ParticleSpawn {
            position: [1, 2, 3],
            vector_24: [4, 5, 6],
            word_30: 7,
            word_34: 8,
            word_46: 9,
            type_byte: 6,
            tuning: [10, 11],
            random: 0x0603,
            low_detail: false,
            global_flag: true,
        };
        let mut pools = ParticlePools::default();
        let (wrapped, record) = pools.spawn(0, input).unwrap();
        assert!(!wrapped);
        assert_eq!(record.position, [1, 2, 3]);
        assert_eq!(record.vector_24, [4, 5, 6]);
        assert_eq!((record.word_30, record.word_34, record.word_3c), (7, 8, 0));
        assert_eq!((record.word_46, record.tuning_48), (9, 11));
        assert_eq!((record.type_byte, record.frame, record.flags), (6, 3, 3));
        assert_eq!(pools.pool(0).unwrap().next_index(), 1);
        assert!(pools.spawn(8, input).is_none());
    }

    #[test]
    fn particle_pool_wraps_at_one_hundred_and_preserves_only_expected_flags() {
        use particles::{ParticlePools, ParticleSpawn};
        let input = ParticleSpawn {
            position: [0; 3],
            vector_24: [0; 3],
            word_30: 0,
            word_34: 0,
            word_46: 0,
            type_byte: 0x16,
            tuning: [10, 11],
            random: 0,
            low_detail: true,
            global_flag: true,
        };
        let mut pools = ParticlePools::default();
        for index in 0..100 {
            let (wrapped, record) = pools.spawn(0, input).unwrap();
            assert_eq!(wrapped, index == 99);
            assert_eq!(record.tuning_48, 10);
            assert_eq!(record.frame, 0x32);
        }
        assert_eq!(pools.pool(0).unwrap().next_index(), 0);
        let (_, record) = pools
            .spawn(
                0,
                ParticleSpawn {
                    global_flag: false,
                    ..input
                },
            )
            .unwrap();
        assert_eq!(record.flags, 1);
    }

    #[test]
    fn particle_type_dispatch_matches_recovery() {
        use particles::{ParticleEffect, particle_effect};
        assert_eq!(particle_effect(0x01), Some(ParticleEffect::Dust));
        assert_eq!(particle_effect(0x18), Some(ParticleEffect::Dust));
        assert_eq!(particle_effect(0x02), Some(ParticleEffect::Clods));
        assert_eq!(particle_effect(0x07), Some(ParticleEffect::Snow));
        assert_eq!(particle_effect(0x08), Some(ParticleEffect::SnowCloud));
        assert_eq!(particle_effect(0x0b), Some(ParticleEffect::Splash));
        assert_eq!(particle_effect(0x0c), Some(ParticleEffect::Leaves));
        assert_eq!(particle_effect(0x1a), Some(ParticleEffect::GravelDryMud));
        assert_eq!(particle_effect(0x7f), None);
    }

    #[test]
    fn initial_frame_averages_or_floors() {
        use particles::{LOW_DETAIL_FRAME, initial_anim_frame};
        // (0x0c + 0x06 + 0x03)/3 = 7.
        assert_eq!(initial_anim_frame(0x03_06_0c, false), 7);
        // Low detail forces the fixed frame regardless of the random word.
        assert_eq!(initial_anim_frame(0x03_06_0c, true), LOW_DETAIL_FRAME);
    }

    #[test]
    fn cull_window_and_size_growth() {
        use particles::{CULL_FAR, CULL_NEAR, follow_size, is_visible};
        assert!(!is_visible(Fixed16::from_raw(CULL_NEAR.raw()))); // boundary excluded
        assert!(is_visible(Fixed16::from_raw(0x10_0000))); // 16.0 inside
        assert!(!is_visible(Fixed16::from_raw(CULL_FAR.raw() + 1)));
        // Growth: spawn 0.2 + 0.0 at frame 0; grows with frames; clamps to max.
        let huge = Fixed16::from_raw(i32::MAX);
        assert_eq!(follow_size(0, huge).raw(), 0x3333);
        assert_eq!(follow_size(2, huge).raw(), 0x3333 + 0x51e * 2);
        let small = Fixed16::from_raw(0x3333);
        assert_eq!(follow_size(10, small).raw(), 0x3333); // clamped
    }

    #[test]
    fn particle_alpha_scales_and_floors() {
        use particles::{LOW_DETAIL_ALPHA, particle_alpha};
        assert_eq!(particle_alpha(50, 3, false), 150);
        assert_eq!(particle_alpha(100, 3, false), 0xff); // saturates
        assert_eq!(particle_alpha(10, 1, false), 10);
        // Low detail floors to 0x50.
        assert_eq!(particle_alpha(10, 1, true), LOW_DETAIL_ALPHA);
        assert_eq!(particle_alpha(100, 3, true), 0xff);
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
    fn shadow_primitives_and_conversion() {
        use shadow::{
            SHADOW_RGBA, ShadowPrimitive, fixed_to_float, shadow_primitive, shadow_primitive_counts,
        };
        assert_eq!(shadow_primitive(0), Some(ShadowPrimitive::Strip4));
        assert_eq!(shadow_primitive(1), Some(ShadowPrimitive::Indexed11));
        assert_eq!(shadow_primitive(2), Some(ShadowPrimitive::Indexed15));
        assert_eq!(shadow_primitive(3), None);
        assert_eq!(shadow_primitive_counts(ShadowPrimitive::Strip4), (4, 0));
        assert_eq!(shadow_primitive_counts(ShadowPrimitive::Indexed15), (0, 15));
        assert_eq!(SHADOW_RGBA, (100, 100, 100, 170));
        // 1.0 in Q16.16 -> 1.0f; -2.0 ground plane.
        assert_eq!(fixed_to_float(0x1_0000), 1.0);
        assert_eq!(fixed_to_float(shadow::GROUND_PLANE_Q16), -2.0);
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
