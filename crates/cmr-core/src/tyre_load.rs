//! Per-wheel tyre loads and surface gains for the wheel-force producer.
//!
//! Observations (pseudocode and listing level):
//!
//! - Every tick the mechanics driver `FUN_00407690` (`0x4079ef..0x407a96`)
//!   reads three words per wheel from `DAT_004f777c`, indexed by the wheel's
//!   sign-extended surface word `ctl +0x42a + 2w` plus `car +0x758 * 120`:
//!   `ctl +0x278 + 4w = -word0`, `ctl +0x288 + 4w = -word1` and
//!   `ctl +0x298 + 4w = word2 + 0x10000`.
//! - Later in the same tick `FUN_004095e0` (after the slope grip
//!   `FUN_00408f50`) writes the mass share `+0x528` and a shared load
//!   `-qmul(+0x528 + 30.0, DAT_0050c5f0 . n)` with the car normal `n` at
//!   `+0x324`. It then walks the eight probes (surface word `ctl +0x42a + 2i`).
//!   A probe touches when its clear flag `+0x73a + i` is zero, or, for a
//!   wheel, when its contact-distance flag `+0x68c + 4i` is set. A touching
//!   probe adds `n * shared_load` to `car +0x3fc + 12i` unless all four
//!   wheels count as touching (`+0x742 == 4`). A touching wheel of an upright
//!   car (`ctl +0x404`) replaces the load local with
//!   `qmul(car +0x654 + 4i + 30.0, qmul(probe_normal_y, 0x8000))` and writes
//!   `ctl +0x36c + 4i = qmul(load, DAT_004f6f0c[s] + ctl +0x268 + 4i + ctl +0x278 + 4i)`
//!   and `ctl +0x35c + 4i` the same with `DAT_004f6b4c[s]`. Every other
//!   touching probe writes `ctl +0x31c + 4i = qmul(load, DAT_004f6f0c[s] + ctl +0x250)`
//!   and `ctl +0x33c + 4i = qmul(load, DAT_004f6b4c[s] + ctl +0x254)` with the
//!   current load local, which is the last upright wheel's load once one has
//!   run. A clear wheel zeroes its four gain words; a clear hull probe zeroes
//!   its two. The tail of the pass (the base-force projection) is
//!   [`crate::vehicle_dynamics::grounded_base_force`]; its `DAT_0050c5a8`
//!   vector is always scaled by a clamped-to-zero factor.
//! - The wheel-force producer `FUN_0040d810` reads the results: its first
//!   loop clamps the slip term to `ctl +0x36c + 4w`, and its second loop's
//!   candidates use the gains `ctl +0x36c/+0x35c + 4w` with the bases
//!   `DAT_004f6f10/DAT_004f6b50[s] + ctl +0x258 + 4w + ctl +0x288 + 4w`, plus
//!   the initializer constant `ctl +0x37c` on the rear wheels.
//!
//! Unknowns: no writer was found for `car +0x654..+0x660` or for control
//! `+0x258..+0x26c`; the reconstruction keeps them at their zero reset value.
//! No reader of `car +0x3fc..+0x458` was found; the support sums are kept for
//! when one is.

use std::fmt;

use cmr_formats::pe_tyre_tables::{TYRE_SURFACE_COUNT, TYRE_VARIANT_COUNT, TyreSurfaceTables};

use crate::fixed_point::{Fixed16, Vec3};
use crate::vehicle_dynamics::contact_count_scale;

/// The constant added to both loads (`0x1e0000`, 30.0).
pub const TYRE_LOAD_OFFSET: i32 = 0x1e_0000;
/// The initializer's `ctl +0x37c` (`0x407094`), added to the rear bases.
pub const REAR_SURFACE_BASE_37C: i32 = -0x4ccc;
/// The initializer's load direction `DAT_0050c5f0..f8` (`0x406ae0..0x406af0`).
pub const TYRE_LOAD_DIRECTION_C5F0: Vec3 = Vec3::new(
    Fixed16::from_raw(0),
    Fixed16::from_raw(-0x8000),
    Fixed16::from_raw(0),
);

/// A surface word or tyre variant outside the static tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TyreSurfaceIndexError {
    Surface { probe: usize, surface: i16 },
    Variant(i32),
}

impl fmt::Display for TyreSurfaceIndexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Surface { probe, surface } => write!(
                f,
                "probe {probe} surface word {surface} is outside the {TYRE_SURFACE_COUNT}-entry tables"
            ),
            Self::Variant(variant) => write!(
                f,
                "tyre variant {variant} is outside the {TYRE_VARIANT_COUNT} variant blocks"
            ),
        }
    }
}

impl std::error::Error for TyreSurfaceIndexError {}

fn surface_index(probe: usize, surface: i16) -> Result<usize, TyreSurfaceIndexError> {
    usize::try_from(surface)
        .ok()
        .filter(|&index| index < TYRE_SURFACE_COUNT)
        .ok_or(TyreSurfaceIndexError::Surface { probe, surface })
}

/// The per-wheel control words written from `DAT_004f777c`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WheelSurfaceTerms {
    pub term_278: [i32; 4],
    pub term_288: [i32; 4],
    pub term_298: [i32; 4],
}

/// The driver's `DAT_004f777c` loop for the four wheel surface words.
///
/// # Errors
///
/// Rejects a variant outside the eight blocks or a surface word outside the
/// 120 surfaces. The native indexes the flat table without a check; a sum
/// that stays inside the table but crosses a variant block is rejected here.
pub fn derive_wheel_surface_terms(
    surface_words: [i16; 4],
    variant_758: i32,
    tables: &TyreSurfaceTables,
) -> Result<WheelSurfaceTerms, TyreSurfaceIndexError> {
    let variant = usize::try_from(variant_758)
        .ok()
        .and_then(|variant| tables.wheel_terms_777c.get(variant))
        .ok_or(TyreSurfaceIndexError::Variant(variant_758))?;
    let mut terms = WheelSurfaceTerms::default();
    for (wheel, &surface) in surface_words.iter().enumerate() {
        let words = variant[surface_index(wheel, surface)?];
        terms.term_278[wheel] = -i32::from(words[0]);
        terms.term_288[wheel] = -i32::from(words[1]);
        terms.term_298[wheel] = i32::from(words[2]) + 0x1_0000;
    }
    Ok(terms)
}

/// The persistent control gain words `FUN_004095e0` writes. The control
/// clear loop zeroes `+0x31c..+0x358`; `+0x35c..+0x378` start at zero too.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TyreGains {
    pub gain_31c: [i32; 8],
    pub gain_33c: [i32; 8],
    pub gain_35c: [i32; 4],
    pub gain_36c: [i32; 4],
}

/// Inputs to one `FUN_004095e0` pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TyreLoadInputs<'a> {
    /// Touching probe count `+0x742` from the latest threshold scan.
    pub count_742: u8,
    /// Mass word `+0x48`.
    pub mass_48: Fixed16,
    /// Car normal `+0x324..+0x32c`.
    pub normal_324: Vec3,
    /// Upright flag `ctl +0x404`.
    pub upright_404: bool,
    /// Probe clear flags `+0x73a..+0x741`.
    pub flags_73a: [u8; 8],
    /// Wheel contact-distance flags `+0x68c..+0x698`.
    pub contact_68c: [bool; 4],
    /// The y word of each wheel probe's normal (`+0x340 + 12i`).
    pub probe_normal_y: [i32; 4],
    /// Unsourced per-wheel load words `car +0x654 + 4i`.
    pub wheel_load_654: [i32; 4],
    /// Surface words `ctl +0x42a + 2i`.
    pub surface_words: [i16; 8],
    /// Unsourced per-wheel terms `ctl +0x268 + 4i`.
    pub term_268: [i32; 4],
    /// The driver's `DAT_004f777c` terms `ctl +0x278 + 4i`.
    pub term_278: [i32; 4],
    /// The shared terms `ctl +0x250/+0x254`.
    pub shared_250: i32,
    pub shared_254: i32,
    pub tables: &'a TyreSurfaceTables,
}

/// The car-record words the pass writes besides the control gains.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TyreLoadEffects {
    /// Mass share `+0x528`.
    pub mass_share_528: Fixed16,
    /// The shared load before any wheel replaces it.
    pub shared_load: i32,
    /// Additions to the support sums `car +0x3fc + 12i`.
    pub support_3fc: [Vec3; 8],
}

/// Runs `FUN_004095e0`'s load and gain loop, updating `gains` in place.
///
/// # Errors
///
/// Rejects a touching probe whose surface word is outside the 120 surfaces,
/// before any gain of that probe is written.
pub fn apply_tyre_load_pass(
    gains: &mut TyreGains,
    input: &TyreLoadInputs<'_>,
) -> Result<TyreLoadEffects, TyreSurfaceIndexError> {
    let mass_share = contact_count_scale(input.mass_48, input.count_742);
    let along = TYRE_LOAD_DIRECTION_C5F0.dot(input.normal_324);
    let shared_load =
        qmul(mass_share.raw().wrapping_add(TYRE_LOAD_OFFSET), along.raw()).wrapping_neg();
    // The native routes the shared load through `qmul(load, 1.0)` first.
    let support = input
        .normal_324
        .scale(Fixed16::from_raw(qmul(shared_load, 0x1_0000)));
    let mut effects = TyreLoadEffects {
        mass_share_528: mass_share,
        shared_load,
        support_3fc: [Vec3::ZERO; 8],
    };
    let mut load = shared_load;
    for probe in 0..8 {
        let wheel = probe < 4;
        let touching = input.flags_73a[probe] == 0 || (wheel && input.contact_68c[probe]);
        if !touching {
            gains.gain_31c[probe] = 0;
            gains.gain_33c[probe] = 0;
            if wheel {
                gains.gain_35c[probe] = 0;
                gains.gain_36c[probe] = 0;
            }
            continue;
        }
        if input.count_742 != 4 {
            effects.support_3fc[probe] = support;
        }
        let surface = surface_index(probe, input.surface_words[probe])?;
        let factor_6f0c = input.tables.load_factor_6f0c[surface];
        let factor_6b4c = input.tables.load_factor_6b4c[surface];
        if wheel && input.upright_404 {
            let normal_share = qmul(input.probe_normal_y[probe], 0x8000);
            load = qmul(
                input.wheel_load_654[probe].wrapping_add(TYRE_LOAD_OFFSET),
                normal_share,
            );
            let terms = input.term_268[probe].wrapping_add(input.term_278[probe]);
            gains.gain_36c[probe] = qmul(load, factor_6f0c.wrapping_add(terms));
            gains.gain_35c[probe] = qmul(load, factor_6b4c.wrapping_add(terms));
        } else {
            gains.gain_31c[probe] = qmul(load, factor_6f0c.wrapping_add(input.shared_250));
            gains.gain_33c[probe] = qmul(load, factor_6b4c.wrapping_add(input.shared_254));
        }
    }
    Ok(effects)
}

/// The second-loop candidate bases for one wheel:
/// `(transition_base, alternate_base)` from `DAT_004f6f10/DAT_004f6b50`.
///
/// # Errors
///
/// Rejects a surface word outside the 120 surfaces.
pub fn wheel_candidate_bases(
    wheel: usize,
    surface_word: i16,
    term_258: i32,
    term_288: i32,
    tables: &TyreSurfaceTables,
) -> Result<(i32, i32), TyreSurfaceIndexError> {
    let surface = surface_index(wheel, surface_word)?;
    let mut shared = term_258.wrapping_add(term_288);
    if wheel >= 2 {
        shared = shared.wrapping_add(REAR_SURFACE_BASE_37C);
    }
    Ok((
        tables.candidate_base_6f10[surface].wrapping_add(shared),
        tables.candidate_base_6b50[surface].wrapping_add(shared),
    ))
}

fn qmul(left: i32, right: i32) -> i32 {
    Fixed16::from_raw(left).mul(Fixed16::from_raw(right)).raw()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tables() -> TyreSurfaceTables {
        TyreSurfaceTables {
            candidate_base_6b50: std::array::from_fn(|i| 0x1000 + i as i32),
            candidate_base_6f10: std::array::from_fn(|i| 0x2000 + i as i32),
            load_factor_6b4c: std::array::from_fn(|i| 0x8000 + i as i32 * 0x100),
            load_factor_6f0c: std::array::from_fn(|i| 0x4000 + i as i32 * 0x100),
            wheel_terms_777c: std::array::from_fn(|v| {
                std::array::from_fn(|s| [(v * 10 + s) as u16, 0x300 + s as u16, 0xfff0])
            }),
            roll_limits_768c: [[0; 2]; 120],
        }
    }

    fn up() -> Vec3 {
        Vec3::new(Fixed16::ZERO, Fixed16::ONE, Fixed16::ZERO)
    }

    fn inputs(tables: &TyreSurfaceTables) -> TyreLoadInputs<'_> {
        TyreLoadInputs {
            count_742: 3,
            mass_48: Fixed16::from_int(3),
            normal_324: up(),
            upright_404: true,
            flags_73a: [0, 0, 0, 1, 0, 1, 1, 1],
            contact_68c: [false; 4],
            probe_normal_y: [0x1_0000, 0x8000, 0x1_0000, 0x1_0000],
            wheel_load_654: [0; 4],
            surface_words: [1, 2, 3, 4, 5, 6, 7, 8],
            term_268: [0; 4],
            term_278: [-0x100, 0, 0, 0],
            shared_250: 0x10,
            shared_254: 0x20,
            tables,
        }
    }

    #[test]
    fn wheel_terms_negate_the_first_two_words_and_offset_the_third() {
        let t = tables();
        let terms = derive_wheel_surface_terms([0, 1, 119, 5], 2, &t).unwrap();
        assert_eq!(terms.term_278, [-20, -21, -139, -25]);
        assert_eq!(terms.term_288, [-0x300, -0x301, -0x377, -0x305]);
        assert_eq!(terms.term_298, [0x1_fff0; 4]);
        assert_eq!(
            derive_wheel_surface_terms([0, 120, 0, 0], 0, &t),
            Err(TyreSurfaceIndexError::Surface {
                probe: 1,
                surface: 120
            })
        );
        assert_eq!(
            derive_wheel_surface_terms([0, 0, -1, 0], 0, &t),
            Err(TyreSurfaceIndexError::Surface {
                probe: 2,
                surface: -1
            })
        );
        for variant in [-1, 8] {
            assert_eq!(
                derive_wheel_surface_terms([0; 4], variant, &t),
                Err(TyreSurfaceIndexError::Variant(variant))
            );
        }
    }

    #[test]
    fn upright_wheels_take_the_normal_scaled_load_and_hull_probes_reuse_it() {
        let t = tables();
        let mut gains = TyreGains {
            gain_31c: [7; 8],
            gain_33c: [7; 8],
            gain_35c: [7; 4],
            gain_36c: [7; 4],
        };
        let effects = apply_tyre_load_pass(&mut gains, &inputs(&t)).unwrap();
        // Share 3.0 / 3; shared load -qmul(31.0, -0.5) = 15.5.
        assert_eq!(effects.mass_share_528, Fixed16::ONE);
        assert_eq!(effects.shared_load, 0xf_8000);
        let support = Vec3::new(Fixed16::ZERO, Fixed16::from_raw(0xf_8000), Fixed16::ZERO);
        assert_eq!(effects.support_3fc[0], support);
        assert_eq!(effects.support_3fc[3], Vec3::ZERO);
        assert_eq!(effects.support_3fc[4], support);
        // Wheel 0: load 30.0 * 0.5 = 15.0; factor (0x4100 - 0x100) and 0x8100 - 0x100.
        assert_eq!(gains.gain_36c[0], qmul(0xf_0000, 0x4000));
        assert_eq!(gains.gain_35c[0], qmul(0xf_0000, 0x8000));
        // Wheel 1 at half the normal: load 7.5.
        assert_eq!(gains.gain_36c[1], qmul(0x7_8000, 0x4200));
        assert_eq!(gains.gain_35c[1], qmul(0x7_8000, 0x8200));
        // Wheels keep their shared-load words when upright.
        assert_eq!(gains.gain_31c[0], 7);
        // Wheel 3 is clear: all four of its words are zeroed.
        assert_eq!(
            (
                gains.gain_31c[3],
                gains.gain_33c[3],
                gains.gain_35c[3],
                gains.gain_36c[3]
            ),
            (0, 0, 0, 0)
        );
        // Hull probe 4 reuses wheel 2's load (15.0), not the shared 15.5.
        assert_eq!(gains.gain_31c[4], qmul(0xf_0000, 0x4500 + 0x10));
        assert_eq!(gains.gain_33c[4], qmul(0xf_0000, 0x8500 + 0x20));
        assert_eq!((gains.gain_31c[5], gains.gain_33c[5]), (0, 0));
    }

    #[test]
    fn a_tipped_car_writes_the_shared_load_words_and_contact_flags_count() {
        let t = tables();
        let mut gains = TyreGains::default();
        let mut input = inputs(&t);
        input.upright_404 = false;
        input.contact_68c = [false, false, false, true];
        input.count_742 = 4;
        let effects = apply_tyre_load_pass(&mut gains, &input).unwrap();
        assert_eq!(effects.support_3fc, [Vec3::ZERO; 8]);
        let shared = effects.shared_load;
        assert_eq!(effects.mass_share_528, Fixed16::from_raw(0xc000));
        // Wheel 3 is flagged clear but its contact-distance flag counts.
        for probe in [0, 1, 2, 3, 4] {
            let surface = probe + 1;
            assert_eq!(
                gains.gain_31c[probe],
                qmul(shared, 0x4000 + surface as i32 * 0x100 + 0x10)
            );
        }
        assert_eq!(gains.gain_35c, [0; 4]);
        assert_eq!(gains.gain_36c, [0; 4]);
    }

    #[test]
    fn an_airborne_car_has_no_mass_share_and_rejects_bad_touching_surfaces() {
        let t = tables();
        let mut gains = TyreGains::default();
        let mut input = inputs(&t);
        input.count_742 = 0;
        input.flags_73a = [1; 8];
        let effects = apply_tyre_load_pass(&mut gains, &input).unwrap();
        assert_eq!(effects.mass_share_528, Fixed16::ZERO);
        assert_eq!(effects.shared_load, 0xf_0000);
        assert_eq!(gains, TyreGains::default());
        // A clear probe's surface word is never read.
        input.surface_words[7] = 500;
        apply_tyre_load_pass(&mut gains, &input).unwrap();
        input.flags_73a[7] = 0;
        assert_eq!(
            apply_tyre_load_pass(&mut gains, &input),
            Err(TyreSurfaceIndexError::Surface {
                probe: 7,
                surface: 500
            })
        );
    }

    #[test]
    fn candidate_bases_add_the_rear_constant_only_behind() {
        let t = tables();
        assert_eq!(
            wheel_candidate_bases(1, 3, 0, -0x300, &t),
            Ok((0x2003 - 0x300, 0x1003 - 0x300))
        );
        assert_eq!(
            wheel_candidate_bases(2, 3, 5, -0x300, &t),
            Ok((0x2003 + 5 - 0x300 - 0x4ccc, 0x1003 + 5 - 0x300 - 0x4ccc))
        );
        assert_eq!(
            wheel_candidate_bases(0, 120, 0, 0, &t),
            Err(TyreSurfaceIndexError::Surface {
                probe: 0,
                surface: 120
            })
        );
    }
}
