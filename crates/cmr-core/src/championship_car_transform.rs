//! Recovered normal-frame transform and visibility state for the selected car.
//!
//! This models only the observed callback policy where its fourth argument is
//! one. The unresolved alternate branch is intentionally not configurable.

use crate::{
    championship_setup_visual::PixelPoint,
    display::DisplayExtent,
    root_menu_c3d_transform::{
        RootMenuC3dViewportError, apply_c3d_angle_tuple_q16, root_menu_c3d_viewport,
    },
};

pub const CHAMPIONSHIP_CAR_START_PITCH: i16 = -227;
pub const CHAMPIONSHIP_CAR_START_TRANSLATION_Q16: [i32; 3] = [0, 0x6666, 0x4_8000];
pub const CHAMPIONSHIP_CAR_FRAME_TRANSLATION_Q16: [i32; 3] = [0, 0x8000, 0x4_8000];
pub const CHAMPIONSHIP_CAR_SPIN_DELTA: u16 = 22;
pub const CHAMPIONSHIP_CAR_PITCH_DELTA: i16 = 11;
pub const CHAMPIONSHIP_CAR_PITCH_LOWER_GATE: i16 = -910;
pub const CHAMPIONSHIP_CAR_PITCH_UPPER_GATE: i16 = -113;

const Q16_ONE: i32 = 0x1_0000;
const INITIAL_ORIGIN: PixelPoint = PixelPoint { x: -1, y: -1 };
const LOGO_WIDTH_SCALE_Q16: i32 = 0x48f5;
const LOGO_HEIGHT_SCALE_Q16: i32 = 0x445a;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChampionshipCarInput {
    pub f9: bool,
    pub f10: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChampionshipCarFrame {
    pub local_transform_q16: [i32; 16],
    pub visible: bool,
    pub logo_extent: (i32, i32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChampionshipCarState {
    pitch: i16,
    spin: u16,
    previous_logo_origin: PixelPoint,
}

impl Default for ChampionshipCarState {
    fn default() -> Self {
        Self::new()
    }
}

impl ChampionshipCarState {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            pitch: CHAMPIONSHIP_CAR_START_PITCH,
            spin: 0,
            previous_logo_origin: INITIAL_ORIGIN,
        }
    }

    #[must_use]
    pub const fn pitch(self) -> i16 {
        self.pitch
    }

    #[must_use]
    pub const fn spin(self) -> u16 {
        self.spin
    }

    #[must_use]
    pub const fn previous_logo_origin(self) -> PixelPoint {
        self.previous_logo_origin
    }

    /// Returns the initial load-time transform before the normal callback
    /// replaces its translation and adds the wrapping spin stage.
    #[must_use]
    pub fn startup_local_transform_q16(base_basis: [i32; 9]) -> [i32; 16] {
        let basis =
            apply_c3d_angle_tuple_q16(base_basis, [CHAMPIONSHIP_CAR_START_PITCH as u16, 0, 0]);
        matrix_from_basis(basis, CHAMPIONSHIP_CAR_START_TRANSLATION_Q16)
    }

    /// Advances the observed normal (`param4=1`) callback exactly once.
    pub fn advance_normal_frame(
        &mut self,
        input: ChampionshipCarInput,
        base_basis: [i32; 9],
        logo_origin: PixelPoint,
        viewport: DisplayExtent,
    ) -> Result<ChampionshipCarFrame, RootMenuC3dViewportError> {
        // The same recovered signed-i16 rectangle constructor owns the root
        // and championship frontend passes. Validate before persistent state.
        let _ = root_menu_c3d_viewport(viewport)?;

        if input.f9 && self.pitch > CHAMPIONSHIP_CAR_PITCH_LOWER_GATE {
            self.pitch = self.pitch.wrapping_sub(CHAMPIONSHIP_CAR_PITCH_DELTA);
        } else if input.f10 && self.pitch < CHAMPIONSHIP_CAR_PITCH_UPPER_GATE {
            self.pitch = self.pitch.wrapping_add(CHAMPIONSHIP_CAR_PITCH_DELTA);
        }
        self.spin = self.spin.wrapping_add(CHAMPIONSHIP_CAR_SPIN_DELTA);

        let pitched = apply_c3d_angle_tuple_q16(base_basis, [self.pitch as u16, 0, 0]);
        let basis = apply_c3d_angle_tuple_q16(pitched, [0, self.spin, 0]);
        let logo_extent = championship_logo_extent(viewport);
        let visible = self.previous_logo_origin == logo_origin
            && fully_contained(logo_origin, logo_extent, viewport);
        self.previous_logo_origin = logo_origin;

        Ok(ChampionshipCarFrame {
            local_transform_q16: matrix_from_basis(basis, CHAMPIONSHIP_CAR_FRAME_TRANSLATION_Q16),
            visible,
            logo_extent,
        })
    }
}

#[must_use]
pub fn championship_logo_extent(viewport: DisplayExtent) -> (i32, i32) {
    (
        (i32::from(viewport.width) * LOGO_WIDTH_SCALE_Q16) >> 16,
        (i32::from(viewport.height) * LOGO_HEIGHT_SCALE_Q16) >> 16,
    )
}

fn fully_contained(origin: PixelPoint, extent: (i32, i32), viewport: DisplayExtent) -> bool {
    origin.x >= 0
        && origin.x.wrapping_add(extent.0) <= i32::from(viewport.width)
        && origin.y >= 0
        && origin.y.wrapping_add(extent.1) <= i32::from(viewport.height)
}

fn matrix_from_basis(basis: [i32; 9], translation: [i32; 3]) -> [i32; 16] {
    [
        basis[0],
        basis[1],
        basis[2],
        0,
        basis[3],
        basis[4],
        basis[5],
        0,
        basis[6],
        basis[7],
        basis[8],
        0,
        translation[0],
        translation[1],
        translation[2],
        Q16_ONE,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDENTITY: [i32; 9] = [Q16_ONE, 0, 0, 0, Q16_ONE, 0, 0, 0, Q16_ONE];
    const VIEWPORT: DisplayExtent = DisplayExtent {
        width: 640,
        height: 480,
    };
    const NO_KEYS: ChampionshipCarInput = ChampionshipCarInput {
        f9: false,
        f10: false,
    };

    #[test]
    fn startup_state_and_transform_match_observed_words() {
        let state = ChampionshipCarState::new();
        assert_eq!(state.pitch(), -227);
        assert_eq!(state.spin(), 0);
        assert_eq!(state.previous_logo_origin(), PixelPoint::new(-1, -1));
        let matrix = ChampionshipCarState::startup_local_transform_q16(IDENTITY);
        assert_eq!(&matrix[12..15], &CHAMPIONSHIP_CAR_START_TRANSLATION_Q16);
        assert_eq!(
            [matrix[3], matrix[7], matrix[11], matrix[15]],
            [0, 0, 0, Q16_ONE]
        );
    }

    #[test]
    fn startup_transform_keeps_fixed_pitch_after_mutable_state_advances() {
        let expected = ChampionshipCarState::startup_local_transform_q16(IDENTITY);
        let mut state = ChampionshipCarState::new();
        let _ = state
            .advance_normal_frame(
                ChampionshipCarInput {
                    f9: true,
                    f10: false,
                },
                IDENTITY,
                PixelPoint::new(20, 20),
                VIEWPORT,
            )
            .unwrap();
        assert_eq!(state.pitch(), -238);
        assert_eq!(
            ChampionshipCarState::startup_local_transform_q16(IDENTITY),
            expected
        );
    }

    #[test]
    fn first_frame_has_exact_golden_basis_and_frame_state() {
        let mut state = ChampionshipCarState::new();
        let frame = state
            .advance_normal_frame(NO_KEYS, IDENTITY, PixelPoint::new(100, 100), VIEWPORT)
            .unwrap();
        assert_eq!(state.pitch(), -227);
        assert_eq!(state.spin(), 22);
        assert_eq!(
            &frame.local_transform_q16[12..15],
            &CHAMPIONSHIP_CAR_FRAME_TRANSLATION_Q16
        );
        assert_eq!(frame.logo_extent, (182, 128));
        assert!(!frame.visible);
        assert_eq!(
            &frame.local_transform_q16[..12],
            &[
                65_496, -755, -2_080, 0, -2, 61_591, -22_362, 0, 2_210, 22_343, 61_555, 0
            ]
        );
    }

    #[test]
    fn key_precedence_and_signed_limits_match_comparisons() {
        let mut state = ChampionshipCarState::new();
        let _ = state
            .advance_normal_frame(
                ChampionshipCarInput {
                    f9: true,
                    f10: true,
                },
                IDENTITY,
                PixelPoint::new(0, 0),
                VIEWPORT,
            )
            .unwrap();
        assert_eq!(state.pitch(), -238);

        for (pitch, input, expected) in [
            (
                -909,
                ChampionshipCarInput {
                    f9: true,
                    f10: false,
                },
                -920,
            ),
            (
                -910,
                ChampionshipCarInput {
                    f9: true,
                    f10: false,
                },
                -910,
            ),
            (
                -114,
                ChampionshipCarInput {
                    f9: false,
                    f10: true,
                },
                -103,
            ),
            (
                -113,
                ChampionshipCarInput {
                    f9: false,
                    f10: true,
                },
                -113,
            ),
            (
                -910,
                ChampionshipCarInput {
                    f9: true,
                    f10: true,
                },
                -899,
            ),
        ] {
            let mut state = ChampionshipCarState {
                pitch,
                ..ChampionshipCarState::new()
            };
            let _ = state
                .advance_normal_frame(input, IDENTITY, PixelPoint::new(0, 0), VIEWPORT)
                .unwrap();
            assert_eq!(state.pitch(), expected);
        }
    }

    #[test]
    fn spin_uses_word_width_wrapping_addition() {
        let mut state = ChampionshipCarState {
            spin: u16::MAX - 5,
            ..ChampionshipCarState::new()
        };
        let _ = state
            .advance_normal_frame(NO_KEYS, IDENTITY, PixelPoint::new(0, 0), VIEWPORT)
            .unwrap();
        assert_eq!(state.spin(), 16);
    }

    #[test]
    fn pitch_then_spin_matches_nonidentity_two_stage_golden() {
        const NONIDENTITY_BASE: [i32; 9] = [0, 0, -Q16_ONE, 0, Q16_ONE, 0, Q16_ONE, 0, 0];
        let reversed = apply_c3d_angle_tuple_q16(
            apply_c3d_angle_tuple_q16(NONIDENTITY_BASE, [0, 22, 0]),
            [CHAMPIONSHIP_CAR_START_PITCH as u16, 0, 0],
        );
        let mut state = ChampionshipCarState::new();
        let actual = state
            .advance_normal_frame(NO_KEYS, NONIDENTITY_BASE, PixelPoint::new(0, 0), VIEWPORT)
            .unwrap();
        assert_eq!(
            actual.local_transform_q16[..12],
            [
                -2_080, -755, -65_498, 0, -22_362, 61_591, 0, 0, 61_555, 22_343, -2_213, 0,
            ]
        );
        assert_ne!(actual.local_transform_q16[..3], reversed[..3]);
    }

    #[test]
    fn invalid_signed_i16_viewports_leave_all_persistent_state_unchanged() {
        for width in [0, i16::MAX as u16 + 1, u16::MAX] {
            let mut state = ChampionshipCarState {
                pitch: -400,
                spin: 65_530,
                previous_logo_origin: PixelPoint::new(17, 19),
            };
            let before = state;
            let extent = DisplayExtent { width, height: 480 };
            assert_eq!(
                state.advance_normal_frame(
                    ChampionshipCarInput {
                        f9: true,
                        f10: true,
                    },
                    IDENTITY,
                    PixelPoint::new(123, 234),
                    extent,
                ),
                Err(RootMenuC3dViewportError { extent })
            );
            assert_eq!(state, before);
        }
    }

    #[test]
    fn visibility_requires_stable_origin_and_full_scaled_containment() {
        let mut state = ChampionshipCarState::new();
        assert!(
            !state
                .advance_normal_frame(NO_KEYS, IDENTITY, PixelPoint::new(10, 10), VIEWPORT)
                .unwrap()
                .visible
        );
        assert!(
            state
                .advance_normal_frame(NO_KEYS, IDENTITY, PixelPoint::new(10, 10), VIEWPORT)
                .unwrap()
                .visible
        );
        assert!(
            !state
                .advance_normal_frame(NO_KEYS, IDENTITY, PixelPoint::new(11, 10), VIEWPORT)
                .unwrap()
                .visible
        );
        assert!(
            !state
                .advance_normal_frame(NO_KEYS, IDENTITY, PixelPoint::new(500, 10), VIEWPORT)
                .unwrap()
                .visible
        );
        assert!(
            !state
                .advance_normal_frame(NO_KEYS, IDENTITY, PixelPoint::new(500, 10), VIEWPORT)
                .unwrap()
                .visible
        );
    }

    #[test]
    fn containment_includes_exact_edges_and_rejects_every_clipped_edge() {
        assert_eq!(championship_logo_extent(VIEWPORT), (182, 128));
        for origin in [
            PixelPoint::new(-1, 0),
            PixelPoint::new(0, -1),
            PixelPoint::new(459, 0),
            PixelPoint::new(0, 353),
        ] {
            assert!(!fully_contained(origin, (182, 128), VIEWPORT));
        }
        assert!(fully_contained(
            PixelPoint::new(458, 352),
            (182, 128),
            VIEWPORT
        ));
    }
}
