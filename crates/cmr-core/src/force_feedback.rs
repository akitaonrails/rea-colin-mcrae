//! Recovered force-feedback (FFB) effect semantics (item H08).
//!
//! # Provenance
//!
//! The Windows release drives force feedback through **DirectInput**, and that
//! is the sole reason `DINPUT.dll` / `DirectInputCreateA` are imported: in-race
//! *input* is WinMM (`joyGetPosEx`) + `GetAsyncKeyState` (see
//! [`crate::input_device`]), while DirectInput is instantiated only as the
//! force-feedback *output* path. The device is brought up by `FUN_004aba70`
//! (`DirectInputCreateA` -> `EnumDevices(DIDEVTYPE_JOYSTICK)` -> `CreateDevice`
//! -> `SetDataFormat(c_dfDIJoystick)` -> `SetCooperativeLevel(EXCLUSIVE|
//! BACKGROUND)` -> `Acquire`), stored by the bring-up `FUN_004ab920` behind the
//! device-present flag `DAT_00bb95a8`.
//!
//! Six effects are created once (`FUN_00432ce0`) into the effect-object array
//! `DAT_00bb9578[]` and thereafter only updated per frame via
//! `IDirectInputEffect::SetParameters`. The per-car effect-parameter block is
//! `DAT_00550040` (stride `0x88`), double-buffered (`FUN_004335d0` snapshots
//! current -> previous) so the per-frame commit `FUN_00433620` issues a
//! `SetParameters` call only for the fields that changed.
//!
//! The `DIEFFECT` size observed on the stack (`0x38`) is the **DirectX 7** form,
//! `DIENVELOPE` is `0x14`, `DICONDITION` is `0x18`.
//!
//! ## Separation of observation, inference, and design decision
//!
//! - **Observations** (literal pseudocode): the `0x2710` magnitude rescale, the
//!   `0x10000` game-magnitude clamp, the digital-wheel direction snaps
//!   `9000`/`27000`, the collision peak+decay-step accumulator and its clamp at
//!   `0`, the rumble sample-period floor `1000`, the self-centring `/2` clamp,
//!   the cornering pre-shift clamp `0x27100000`, the per-player FFB-enable bit
//!   `0x08` at config stride `0x9c`, the effect slot/kind table, the condition
//!   selector (`0xb`..`0xe`), and the DirectInput `DIEP_*` / `DIEFF_*` /
//!   cooperative-level ABI constants (these are public DirectInput SDK values,
//!   not game data).
//! - **Inference**: the continuous (analog) collision direction is a polar-angle
//!   conversion from the game's `atan2` LUT (full circle = `0x400` units); the
//!   LUT scale is modelled by [`polar_centidegrees`] but the exact unit was not
//!   transcribed with certainty, so only the boundary mapping is asserted.
//! - **Design decision**: the collision decay *step* source and the per-source
//!   raw-sum derivations are left as caller-supplied inputs; this module
//!   reconstructs the confidently-recovered *transforms*, not the unrecovered
//!   force-production wiring (the G02/G03 unknowns feed the raw sums here).
//!
//! No game data is vendored: the predefined-effect GUIDs are DirectInput SDK
//! constants and are represented only as effect *kinds*; the per-surface rumble
//! table values (`DAT_004edb70`) are runtime-loaded game data and are not copied.

/// DirectInput ABI constants (public DirectInput SDK values, reproduced for the
/// compatibility layer; not game data).
pub mod di {
    /// `DISCL_EXCLUSIVE`.
    pub const COOP_EXCLUSIVE: u32 = 0x1;
    /// `DISCL_BACKGROUND`.
    pub const COOP_BACKGROUND: u32 = 0x8;
    /// `SetCooperativeLevel` argument observed at `FUN_004aba70`
    /// (`EXCLUSIVE | BACKGROUND`).
    pub const COOP_LEVEL: u32 = COOP_EXCLUSIVE | COOP_BACKGROUND;

    /// `DIDEVTYPE_JOYSTICK`, the `EnumDevices` filter.
    pub const DEVTYPE_JOYSTICK: u32 = 4;
    /// `DISFFC_STOPALL`, the `SendForceFeedbackCommand` argument at bring-up.
    pub const DISFFC_STOPALL: u32 = 2;

    /// `DIEFF_OBJECTOFFSETS`.
    pub const DIEFF_OBJECTOFFSETS: u32 = 0x2;
    /// `DIEFF_POLAR`.
    pub const DIEFF_POLAR: u32 = 0x20;
    /// `DIEFF_CARTESIAN`.
    pub const DIEFF_CARTESIAN: u32 = 0x10;

    /// `DIEP_SAMPLEPERIOD`.
    pub const DIEP_SAMPLEPERIOD: u32 = 0x2;
    /// `DIEP_GAIN`.
    pub const DIEP_GAIN: u32 = 0x4;
    /// `DIEP_DIRECTION`.
    pub const DIEP_DIRECTION: u32 = 0x40;
    /// `DIEP_TYPESPECIFICPARAMS`.
    pub const DIEP_TYPESPECIFICPARAMS: u32 = 0x100;

    /// `DIEFFECT` size for the DirectX 7 form seen on the stack.
    pub const DIEFFECT_DX7_SIZE: u32 = 0x38;
    /// `DIENVELOPE` size.
    pub const DIENVELOPE_SIZE: u32 = 0x14;
    /// `DICONDITION` size.
    pub const DICONDITION_SIZE: u32 = 0x18;

    /// `DI_FFNOMINALMAX`: the nominal maximum for gain, magnitude, and
    /// saturation in DirectInput units.
    pub const FF_NOMINAL_MAX: i32 = 10000;

    /// `E_INVALIDARG`, returned by every effect wrapper when no FFB device is
    /// present (the whole chain becomes a silent no-op).
    pub const E_INVALIDARG: u32 = 0x8007_0057;
}

/// Game-internal magnitude ceiling: `1.0` in Q16.16 (impacts, grip, and rumble
/// magnitudes are clamped to this before being rescaled for DirectInput).
pub const GAME_MAGNITUDE_MAX: i32 = 0x1_0000;

/// Multiplier the game folds into the magnitude normalisation to map a
/// `0..=0x10000` Q16.16 magnitude onto DirectInput's `0..=10000` range:
/// `value * 0x2710 >> 16` (`0x2710 == 10000 == DI_FFNOMINALMAX`).
pub const DI_MAGNITUDE_SCALE: i32 = 0x2710;

/// A full circle expressed in DirectInput polar centidegrees (`360 * 100`).
pub const DI_FULL_CIRCLE_CENTIDEGREES: i32 = 36000;

/// Digital-wheel collision direction "push right" (90 degrees).
pub const DI_DIRECTION_RIGHT_CENTIDEGREES: i32 = 9000;

/// Digital-wheel collision direction "push left" (270 degrees).
pub const DI_DIRECTION_LEFT_CENTIDEGREES: i32 = 27000;

/// Full-circle unit count of the game's `atan2` LUT (`FUN_0048e100` ->
/// `DAT_00833380`); a power-of-two angle used for the analog collision
/// direction (inference; see module docs).
pub const ATAN2_LUT_FULL_CIRCLE: i32 = 0x400;

/// Minimum sample period (microseconds) the surface-rumble computer
/// (`FUN_00432e00`) allows before flooring; a rougher surface shortens the
/// period down to this floor.
pub const RUMBLE_PERIOD_FLOOR_US: i32 = 1000;

/// Pre-shift ceiling the cornering/lateral computer (`FUN_00433b30`) clamps its
/// scaled magnitude to; `>> 16` of this is exactly [`di::FF_NOMINAL_MAX`].
pub const CORNERING_SCALED_MAX: i32 = 0x2710_0000;

/// Per-player config stride inside the option struct `DAT_00d3ca80`.
pub const PER_PLAYER_CONFIG_STRIDE: usize = 0x9c;

/// Offset of the per-player FFB option byte within one player's config record.
pub const PER_PLAYER_CONFIG_OPTION_OFFSET: usize = 0x3c;

/// Bit of the per-player config option byte that enables force feedback
/// (`*(DAT_00d3ca80 + 0x3c + player*0x9c) & 8`).
pub const FFB_ENABLE_BIT: u8 = 0x08;

/// The predefined DirectInput effect kinds the game creates. Represented as
/// kinds rather than GUIDs; the GUIDs are DirectInput SDK constants.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EffectKind {
    /// Periodic sine (buzz / rumble).
    PeriodicSine,
    /// Constant force (directional kick / lateral load).
    ConstantForce,
    /// Condition: spring (centring).
    ConditionSpring,
    /// Condition: damper (road feel / self-centring resistance).
    ConditionDamper,
}

/// Number of effect object slots the game creates once at reset
/// (`FUN_00432ce0`).
pub const EFFECT_SLOT_COUNT: usize = 6;

/// The effect kind created into each slot, in creation order
/// (`DAT_00bb9578[slot]`). Returns `None` for an out-of-range slot.
pub fn effect_slot_kind(slot: usize) -> Option<EffectKind> {
    Some(match slot {
        0 => EffectKind::PeriodicSine,
        1 => EffectKind::ConstantForce,
        2 => EffectKind::ConditionSpring,
        3 => EffectKind::ConditionDamper,
        4 => EffectKind::ConstantForce,
        5 => EffectKind::PeriodicSine,
        _ => return None,
    })
}

/// DirectInput `DIEFFECT.dwFlags` the game passes to `CreateEffect` for a given
/// effect kind: polar/object-offsets for periodic and constant force,
/// cartesian/object-offsets for conditions.
pub fn effect_create_flags(kind: EffectKind) -> u32 {
    match kind {
        EffectKind::PeriodicSine | EffectKind::ConstantForce => {
            di::DIEFF_POLAR | di::DIEFF_OBJECTOFFSETS
        }
        EffectKind::ConditionSpring | EffectKind::ConditionDamper => {
            di::DIEFF_CARTESIAN | di::DIEFF_OBJECTOFFSETS
        }
    }
}

/// `DIEFFECT.cAxes` for a given effect kind: 2 for polar periodic/constant
/// effects, 1 for a single-axis condition.
pub fn effect_axis_count(kind: EffectKind) -> u32 {
    match kind {
        EffectKind::PeriodicSine | EffectKind::ConstantForce => 2,
        EffectKind::ConditionSpring | EffectKind::ConditionDamper => 1,
    }
}

/// The condition type selected by the condition-creation helper
/// (`FUN_004ac8e0`): `0xb`=spring, `0xc`=inertia, `0xd`=damper, `0xe`=friction.
/// Returns `None` for an unrecognised selector.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConditionType {
    /// `0xb` -> `GUID_Spring`.
    Spring,
    /// `0xc` -> `GUID_Inertia`.
    Inertia,
    /// `0xd` -> `GUID_Damper`.
    Damper,
    /// `0xe` -> `GUID_Friction`.
    Friction,
}

/// Decode the condition-type selector argument of `FUN_004ac8e0`.
pub fn condition_type(selector: u32) -> Option<ConditionType> {
    Some(match selector {
        0xb => ConditionType::Spring,
        0xc => ConditionType::Inertia,
        0xd => ConditionType::Damper,
        0xe => ConditionType::Friction,
        _ => return None,
    })
}

/// Rescale a game-internal Q16.16 magnitude (`0..=0x10000`) into a DirectInput
/// gain/magnitude in `0..=10000` via the recovered `value * 0x2710 >> 16`,
/// clamping the input to `[0, 0x10000]` first (so the collision impact
/// `0..=0x10000` from [`crate::collision`] maps to a constant-force gain).
pub fn scale_magnitude(game_magnitude: i32) -> i32 {
    let clamped = game_magnitude.clamp(0, GAME_MAGNITUDE_MAX);
    ((clamped as i64 * DI_MAGNITUDE_SCALE as i64) >> 16) as i32
}

/// Clamp and shift a cornering/lateral pre-shift magnitude (`FUN_00433b30`):
/// the scaled value is clamped to `0x27100000` then `>> 16`, yielding a gain in
/// `0..=10000`.
pub fn clamp_cornering_gain(scaled_magnitude: i32) -> i32 {
    scaled_magnitude.clamp(0, CORNERING_SCALED_MAX) >> 16
}

/// Self-centring / road-feel condition gain (`FUN_00433800`): the accumulated
/// raw sum is halved then clamped to `0..=10000`.
pub fn self_centering_gain(raw_sum: i32) -> i32 {
    (raw_sum / 2).clamp(0, di::FF_NOMINAL_MAX)
}

/// Surface-rumble sample period (`FUN_00432e00`): a rougher surface shortens the
/// period, floored at [`RUMBLE_PERIOD_FLOOR_US`].
pub fn rumble_period_us(raw_period_us: i32) -> i32 {
    raw_period_us.max(RUMBLE_PERIOD_FLOOR_US)
}

/// Digital/2-button-wheel collision direction snap (`FUN_004331b0` when the
/// per-player device marker reports a digital device): left -> 270 degrees,
/// right -> 90 degrees, in DirectInput polar centidegrees.
pub fn digital_collision_direction(push_left: bool) -> i32 {
    if push_left {
        DI_DIRECTION_LEFT_CENTIDEGREES
    } else {
        DI_DIRECTION_RIGHT_CENTIDEGREES
    }
}

/// Convert a full-circle angle in `full_circle_units` (for the game's `atan2`
/// LUT, [`ATAN2_LUT_FULL_CIRCLE`]) into DirectInput polar centidegrees,
/// wrapped into `[0, 36000)`.
///
/// Inference: this models the analog collision direction conversion; only the
/// boundary mapping (0 -> 0, quarter turn -> 9000, full turn -> wrap to 0) is
/// asserted, because the exact LUT unit was not transcribed with certainty.
pub fn polar_centidegrees(angle: i32, full_circle_units: i32) -> i32 {
    debug_assert!(full_circle_units > 0);
    let wrapped = angle.rem_euclid(full_circle_units);
    ((wrapped as i64 * DI_FULL_CIRCLE_CENTIDEGREES as i64) / full_circle_units as i64) as i32
}

/// A decaying collision force accumulator (`FUN_004330c0`/`FUN_004331b0` seed it,
/// `FUN_00433ac0` plays then decays it each frame): the current `magnitude` is
/// emitted as the live effect gain, then reduced by `step`, clamped at `0`, so a
/// collision produces a kick that ramps down over several frames.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DecayAccumulator {
    /// Current DirectInput gain (`0..=10000`), emitted this frame.
    pub magnitude: i32,
    /// Per-frame decay step subtracted after emitting.
    pub step: i32,
}

impl DecayAccumulator {
    /// Seed from a game-internal Q16.16 impact magnitude (rescaled to DI units)
    /// and an explicit per-frame decay step. The seeding of the step itself is a
    /// G04/unknown source, so it is supplied by the caller.
    pub fn seed(game_impact: i32, step: i32) -> Self {
        Self {
            magnitude: scale_magnitude(game_impact),
            step: step.max(0),
        }
    }

    /// Advance one frame: return the gain emitted this frame, then decay toward
    /// zero (clamped at `0`). Matches `FUN_00433ac0`'s copy-then-decay order.
    pub fn tick(&mut self) -> i32 {
        let emitted = self.magnitude;
        self.magnitude = (self.magnitude - self.step).max(0);
        emitted
    }

    /// Whether the accumulator has fully decayed (no further force to emit).
    pub fn is_idle(&self) -> bool {
        self.magnitude <= 0
    }
}

/// A `DICONDITION` as the game fills it for the centring/damper effects
/// (`FUN_004ac840`): symmetric positive/negative coefficients, full saturation,
/// no offset, no dead band.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Condition {
    /// `lOffset`.
    pub offset: i32,
    /// `lPositiveCoefficient`.
    pub positive_coefficient: i32,
    /// `lNegativeCoefficient`.
    pub negative_coefficient: i32,
    /// `dwPositiveSaturation`.
    pub positive_saturation: u32,
    /// `dwNegativeSaturation`.
    pub negative_saturation: u32,
    /// `lDeadBand`.
    pub dead_band: i32,
}

/// Build a `DICONDITION` for a centring/damper coefficient exactly as
/// `FUN_004ac840` does: offset 0, symmetric coefficients, both saturations at
/// `DI_FFNOMINALMAX`, dead band 0.
pub fn centering_condition(coefficient: i32) -> Condition {
    Condition {
        offset: 0,
        positive_coefficient: coefficient,
        negative_coefficient: coefficient,
        positive_saturation: di::FF_NOMINAL_MAX as u32,
        negative_saturation: di::FF_NOMINAL_MAX as u32,
        dead_band: 0,
    }
}

/// `DIEP_*` flag set for a periodic (sine/rumble) effect update, pushing only
/// the fields that changed (`FUN_00433620` -> `FUN_004abf50`/`004abd30`):
/// sample period and/or gain.
pub fn periodic_update_flags(gain_changed: bool, period_changed: bool) -> u32 {
    let mut flags = 0;
    if gain_changed {
        flags |= di::DIEP_GAIN;
    }
    if period_changed {
        flags |= di::DIEP_SAMPLEPERIOD;
    }
    flags
}

/// `DIEP_*` flag set for a constant-force effect update, pushing only the
/// fields that changed (`FUN_00433620` -> `FUN_004abed0`/`004abd90`/`004abd30`):
/// gain and/or direction (both -> `0x44`).
pub fn constant_force_update_flags(gain_changed: bool, direction_changed: bool) -> u32 {
    let mut flags = 0;
    if gain_changed {
        flags |= di::DIEP_GAIN;
    }
    if direction_changed {
        flags |= di::DIEP_DIRECTION;
    }
    flags
}

/// `DIEP_*` flag set for a condition effect update (`FUN_004ac840`): the whole
/// condition block is type-specific params.
pub fn condition_update_flags(coefficient_changed: bool) -> u32 {
    if coefficient_changed {
        di::DIEP_TYPESPECIFICPARAMS
    } else {
        0
    }
}

/// Whether force feedback is enabled for a player, given that player's config
/// option byte (`*(DAT_00d3ca80 + 0x3c + player*0x9c) & 8`).
pub fn ffb_enabled(player_config_option: u8) -> bool {
    player_config_option & FFB_ENABLE_BIT != 0
}

/// Byte offset of a player's FFB option within the option struct `DAT_00d3ca80`.
pub fn player_config_option_offset(player: usize) -> usize {
    PER_PLAYER_CONFIG_OPTION_OFFSET + player * PER_PLAYER_CONFIG_STRIDE
}

/// Whether the per-car FFB dispatch (`FUN_00433750`) runs this frame: the
/// DirectInput device must be present (`DAT_00bb95a8`), the session's FFB must be
/// active (`DAT_00550484`), and the player's FFB option bit must be set. When
/// false, every effect wrapper short-circuits to [`di::E_INVALIDARG`] and the
/// game runs with no forces.
pub fn ffb_active(device_present: bool, session_active: bool, player_enabled: bool) -> bool {
    device_present && session_active && player_enabled
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magnitude_scales_full_range_to_di_units() {
        // 0 -> 0, full Q16.16 (0x10000) -> DI_FFNOMINALMAX (10000).
        assert_eq!(scale_magnitude(0), 0);
        assert_eq!(scale_magnitude(GAME_MAGNITUDE_MAX), di::FF_NOMINAL_MAX);
        // Half magnitude -> half nominal (floored).
        assert_eq!(scale_magnitude(0x8000), 5000);
        // Out-of-range clamps rather than overflowing.
        assert_eq!(scale_magnitude(-1), 0);
        assert_eq!(scale_magnitude(0x20_0000), di::FF_NOMINAL_MAX);
    }

    #[test]
    fn cornering_gain_clamps_before_shift() {
        assert_eq!(clamp_cornering_gain(0), 0);
        assert_eq!(
            clamp_cornering_gain(CORNERING_SCALED_MAX),
            di::FF_NOMINAL_MAX
        );
        // Beyond the pre-shift ceiling still lands on the nominal max.
        assert_eq!(clamp_cornering_gain(i32::MAX), di::FF_NOMINAL_MAX);
        assert_eq!(clamp_cornering_gain(-5), 0);
        // A mid value is a plain >>16 of the clamped input.
        assert_eq!(clamp_cornering_gain(0x1388_0000), 0x1388);
    }

    #[test]
    fn self_centering_halves_and_clamps() {
        assert_eq!(self_centering_gain(0), 0);
        assert_eq!(self_centering_gain(10000), 5000);
        assert_eq!(self_centering_gain(40000), di::FF_NOMINAL_MAX);
        assert_eq!(self_centering_gain(-10), 0);
    }

    #[test]
    fn rumble_period_is_floored() {
        assert_eq!(rumble_period_us(0), RUMBLE_PERIOD_FLOOR_US);
        assert_eq!(rumble_period_us(500), RUMBLE_PERIOD_FLOOR_US);
        assert_eq!(rumble_period_us(36006), 36006);
    }

    #[test]
    fn digital_direction_snaps_left_and_right() {
        assert_eq!(
            digital_collision_direction(false),
            DI_DIRECTION_RIGHT_CENTIDEGREES
        );
        assert_eq!(
            digital_collision_direction(true),
            DI_DIRECTION_LEFT_CENTIDEGREES
        );
    }

    #[test]
    fn polar_conversion_maps_lut_boundaries() {
        // Boundary mapping only (the analog direction is an inference).
        assert_eq!(polar_centidegrees(0, ATAN2_LUT_FULL_CIRCLE), 0);
        assert_eq!(
            polar_centidegrees(ATAN2_LUT_FULL_CIRCLE / 4, ATAN2_LUT_FULL_CIRCLE),
            9000
        );
        assert_eq!(
            polar_centidegrees(ATAN2_LUT_FULL_CIRCLE / 2, ATAN2_LUT_FULL_CIRCLE),
            18000
        );
        // A full turn (and multiples) wraps back to zero.
        assert_eq!(
            polar_centidegrees(ATAN2_LUT_FULL_CIRCLE, ATAN2_LUT_FULL_CIRCLE),
            0
        );
        assert_eq!(
            polar_centidegrees(
                ATAN2_LUT_FULL_CIRCLE + ATAN2_LUT_FULL_CIRCLE / 4,
                ATAN2_LUT_FULL_CIRCLE
            ),
            9000
        );
        // Negative angles wrap into range (three-quarter turn).
        assert_eq!(
            polar_centidegrees(-ATAN2_LUT_FULL_CIRCLE / 4, ATAN2_LUT_FULL_CIRCLE),
            27000
        );
    }

    #[test]
    fn decay_accumulator_emits_then_ramps_to_zero() {
        // Seed from a half-magnitude impact (5000 DI units) with a 2000/frame step.
        let mut acc = DecayAccumulator::seed(0x8000, 2000);
        assert_eq!(acc.magnitude, 5000);
        assert!(!acc.is_idle());
        // Copy-then-decay: first tick emits the peak, then subtracts the step.
        assert_eq!(acc.tick(), 5000);
        assert_eq!(acc.tick(), 3000);
        assert_eq!(acc.tick(), 1000);
        // Clamps at zero rather than going negative.
        assert_eq!(acc.tick(), 0);
        assert_eq!(acc.tick(), 0);
        assert!(acc.is_idle());
    }

    #[test]
    fn decay_seed_clamps_impact_and_step() {
        // Over-range impact clamps to the nominal max; negative step floors at 0.
        let acc = DecayAccumulator::seed(0x40_0000, -5);
        assert_eq!(acc.magnitude, di::FF_NOMINAL_MAX);
        assert_eq!(acc.step, 0);
    }

    #[test]
    fn effect_slot_table_matches_creation_order() {
        assert_eq!(effect_slot_kind(0), Some(EffectKind::PeriodicSine));
        assert_eq!(effect_slot_kind(1), Some(EffectKind::ConstantForce));
        assert_eq!(effect_slot_kind(2), Some(EffectKind::ConditionSpring));
        assert_eq!(effect_slot_kind(3), Some(EffectKind::ConditionDamper));
        assert_eq!(effect_slot_kind(4), Some(EffectKind::ConstantForce));
        assert_eq!(effect_slot_kind(5), Some(EffectKind::PeriodicSine));
        // Out-of-range slot is rejected, not wrapped.
        assert_eq!(effect_slot_kind(EFFECT_SLOT_COUNT), None);
        assert_eq!(effect_slot_kind(99), None);
    }

    #[test]
    fn create_flags_and_axes_split_polar_from_cartesian() {
        assert_eq!(
            effect_create_flags(EffectKind::PeriodicSine),
            di::DIEFF_POLAR | di::DIEFF_OBJECTOFFSETS
        );
        assert_eq!(effect_create_flags(EffectKind::PeriodicSine), 0x22);
        assert_eq!(
            effect_create_flags(EffectKind::ConstantForce),
            di::DIEFF_POLAR | di::DIEFF_OBJECTOFFSETS
        );
        assert_eq!(
            effect_create_flags(EffectKind::ConditionSpring),
            di::DIEFF_CARTESIAN | di::DIEFF_OBJECTOFFSETS
        );
        assert_eq!(effect_create_flags(EffectKind::ConditionDamper), 0x12);
        assert_eq!(effect_axis_count(EffectKind::ConstantForce), 2);
        assert_eq!(effect_axis_count(EffectKind::ConditionDamper), 1);
    }

    #[test]
    fn condition_selector_decodes_known_types() {
        assert_eq!(condition_type(0xb), Some(ConditionType::Spring));
        assert_eq!(condition_type(0xc), Some(ConditionType::Inertia));
        assert_eq!(condition_type(0xd), Some(ConditionType::Damper));
        assert_eq!(condition_type(0xe), Some(ConditionType::Friction));
        assert_eq!(condition_type(0), None);
        assert_eq!(condition_type(0xf), None);
    }

    #[test]
    fn centering_condition_is_symmetric_and_saturated() {
        let c = centering_condition(1234);
        assert_eq!(c.offset, 0);
        assert_eq!(c.positive_coefficient, 1234);
        assert_eq!(c.negative_coefficient, 1234);
        assert_eq!(c.positive_saturation, di::FF_NOMINAL_MAX as u32);
        assert_eq!(c.negative_saturation, di::FF_NOMINAL_MAX as u32);
        assert_eq!(c.dead_band, 0);
    }

    #[test]
    fn update_flags_push_only_changed_fields() {
        // Periodic: gain-only (4), period-only (2), both (6), neither (0).
        assert_eq!(periodic_update_flags(true, false), di::DIEP_GAIN);
        assert_eq!(periodic_update_flags(false, true), di::DIEP_SAMPLEPERIOD);
        assert_eq!(periodic_update_flags(true, true), 0x6);
        assert_eq!(periodic_update_flags(false, false), 0);
        // Constant force: gain-only (4), dir-only (0x40), both (0x44), neither (0).
        assert_eq!(constant_force_update_flags(true, false), di::DIEP_GAIN);
        assert_eq!(constant_force_update_flags(false, true), di::DIEP_DIRECTION);
        assert_eq!(constant_force_update_flags(true, true), 0x44);
        assert_eq!(constant_force_update_flags(false, false), 0);
        // Condition: whole block is type-specific params.
        assert_eq!(condition_update_flags(true), di::DIEP_TYPESPECIFICPARAMS);
        assert_eq!(condition_update_flags(false), 0);
    }

    #[test]
    fn enable_gate_requires_device_session_and_player_bit() {
        assert!(ffb_enabled(FFB_ENABLE_BIT));
        assert!(ffb_enabled(0xff));
        assert!(!ffb_enabled(0));
        assert!(!ffb_enabled(0x04));

        assert!(ffb_active(true, true, true));
        assert!(!ffb_active(false, true, true));
        assert!(!ffb_active(true, false, true));
        assert!(!ffb_active(true, true, false));
    }

    #[test]
    fn player_config_offsets_stride_correctly() {
        assert_eq!(player_config_option_offset(0), 0x3c);
        assert_eq!(player_config_option_offset(1), 0x3c + 0x9c);
        assert_eq!(player_config_option_offset(2), 0x3c + 0x9c * 2);
    }
}
