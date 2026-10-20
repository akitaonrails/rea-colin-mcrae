//! Pure compatibility model for the recovered championship setup screen.
//!
//! Animation completion is supplied by the caller. This module intentionally
//! does not invent timing, perform asset I/O, or depend on a host renderer.

use std::{error::Error, fmt};

use crate::frontend::{
    CHAMPIONSHIP_NEXT_SCREEN_ID, CHAMPIONSHIP_SETUP_SCREEN_ID, ROOT_SCREEN_ID, ScreenId,
};

pub const TITLE_TEXT_INDEX: usize = 0x13;
pub const NEXT_ROW_TEXT_INDEX: usize = 0x18;
pub const ENTRY_ANIMATION_CHANNELS: usize = 7;
pub const CAR_ROW_TAG: u8 = 2;
pub const GEAR_ROW_TAG: u8 = 4;
pub const NEXT_ROW_TAG: u8 = 7;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SetupRowKind {
    ChooseCar,
    ChooseGear,
    Next,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SetupRowText {
    /// The shipped row is constructed from the literal `CHOOSE CAR` label.
    ChooseCar,
    /// The shipped row is constructed from the literal `CHOOSE GEAR` label.
    ChooseGear,
    Localized(usize),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SetupRowSpec {
    pub kind: SetupRowKind,
    pub tag: u8,
    pub text: SetupRowText,
    pub target: Option<ScreenId>,
}

pub const ROWS: [SetupRowSpec; 3] = [
    SetupRowSpec {
        kind: SetupRowKind::ChooseCar,
        tag: CAR_ROW_TAG,
        text: SetupRowText::ChooseCar,
        target: None,
    },
    SetupRowSpec {
        kind: SetupRowKind::ChooseGear,
        tag: GEAR_ROW_TAG,
        text: SetupRowText::ChooseGear,
        target: None,
    },
    SetupRowSpec {
        kind: SetupRowKind::Next,
        tag: NEXT_ROW_TAG,
        text: SetupRowText::Localized(NEXT_ROW_TEXT_INDEX),
        target: Some(CHAMPIONSHIP_NEXT_SCREEN_ID),
    },
];

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u8)]
pub enum CanonicalCar {
    Subaru,
    Ibiza,
    Skoda,
    Quattro,
    Rs200,
    Mk2,
    Mitsubis,
    Golf,
    Integral,
    Corolla,
    Escort,
    Megane,
    M4r,
    Celica,
}

impl CanonicalCar {
    pub const ALL: [Self; 14] = [
        Self::Subaru,
        Self::Ibiza,
        Self::Skoda,
        Self::Quattro,
        Self::Rs200,
        Self::Mk2,
        Self::Mitsubis,
        Self::Golf,
        Self::Integral,
        Self::Corolla,
        Self::Escort,
        Self::Megane,
        Self::M4r,
        Self::Celica,
    ];

    #[must_use]
    pub const fn index(self) -> u8 {
        self as u8
    }

    #[must_use]
    pub const fn object_name(self) -> &'static str {
        match self {
            Self::Subaru => "SUBARU",
            Self::Ibiza => "IBIZA",
            Self::Skoda => "SKODA",
            Self::Quattro => "QUATTRO",
            Self::Rs200 => "RS200",
            Self::Mk2 => "MK2",
            Self::Mitsubis => "MITSUBIS",
            Self::Golf => "GOLF",
            Self::Integral => "INTEGRAL",
            Self::Corolla => "COROLLA",
            Self::Escort => "ESCORT",
            Self::Megane => "MEGANE",
            Self::M4r => "M4R",
            Self::Celica => "CELICA",
        }
    }

    #[must_use]
    pub const fn logo_name(self) -> &'static str {
        match self {
            Self::Subaru => "SUBARU",
            Self::Ibiza => "SEAT",
            Self::Skoda => "SKODA",
            Self::Quattro => "AUDI",
            Self::Rs200 | Self::Mk2 | Self::Escort => "FORD",
            Self::Mitsubis => "MITSU",
            Self::Golf => "VW",
            Self::Integral => "LANCIA",
            Self::Corolla => "TOYO",
            Self::Megane => "RENO",
            Self::M4r => "M4R",
            Self::Celica => "TOYO",
        }
    }

    const fn from_index(index: u8) -> Option<Self> {
        if index < Self::ALL.len() as u8 {
            Some(Self::ALL[index as usize])
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GearChoice {
    BitClear,
    BitSet,
}

impl GearChoice {
    const fn from_profile(profile: u8) -> Self {
        if profile & 0x10 == 0 {
            Self::BitClear
        } else {
            Self::BitSet
        }
    }

    const fn bit(self) -> u8 {
        match self {
            Self::BitClear => 0,
            Self::BitSet => 0x10,
        }
    }

    const fn toggled(self) -> Self {
        match self {
            Self::BitClear => Self::BitSet,
            Self::BitSet => Self::BitClear,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SetupPhase {
    Entering,
    Interactive,
    Exiting { target: ScreenId },
}

/// One action after the generic menu dispatcher has arbitrated raw input bits.
///
/// The recovered evidence identifies all six bits individually, but does not
/// establish precedence when multiple bits arrive together. Keeping that
/// arbitration outside this screen model avoids inventing conflict behavior.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SetupAction {
    Left,
    Right,
    Up,
    Down,
    Enter,
    Escape,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SetupTransition {
    pub from: ScreenId,
    pub to: ScreenId,
    pub committed_profile_byte: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SetupExitCompletion {
    pub target: ScreenId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SetupActivationError {
    NoEligibleCars,
    IneligibleFallback { fallback: CanonicalCar },
}

impl fmt::Display for SetupActivationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoEligibleCars => {
                formatter.write_str("championship setup requires an eligible car")
            }
            Self::IneligibleFallback { fallback } => write!(
                formatter,
                "caller-supplied fallback car {} is not eligible",
                fallback.object_name()
            ),
        }
    }
}

impl Error for SetupActivationError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RowValue {
    Car(CanonicalCar),
    Gear(GearChoice),
    None,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RenderRow {
    pub tag: u8,
    pub text: SetupRowText,
    pub selected: bool,
    pub value: RowValue,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActivePreview {
    Car {
        canonical_car: CanonicalCar,
        object_name: &'static str,
        logo_name: &'static str,
    },
    Gear(GearChoice),
    Next,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChampionshipSetupRenderTranscript {
    pub title_text_index: usize,
    pub rows: [RenderRow; 3],
    pub highlight_row: usize,
    pub preview: ActivePreview,
    pub palette_group_4_requested: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChampionshipSetupState {
    eligible_cars: Vec<CanonicalCar>,
    selected_car: usize,
    gear: GearChoice,
    selected_row: usize,
    phase: SetupPhase,
    profile_byte: u8,
    exit_completed: bool,
    last_previewed_car: Option<CanonicalCar>,
}

impl ChampionshipSetupState {
    /// Activates the screen from profile state and an ordered canonical-car mask.
    ///
    /// The artifact establishes that an ineligible saved car is replaced, but
    /// the current evidence does not establish which eligible car wins. The
    /// caller therefore supplies that fallback explicitly.
    pub fn activate(
        profile_byte: u8,
        eligible_mask: [bool; CanonicalCar::ALL.len()],
        ineligible_saved_fallback: CanonicalCar,
    ) -> Result<Self, SetupActivationError> {
        let eligible_cars = CanonicalCar::ALL
            .into_iter()
            .zip(eligible_mask)
            .filter_map(|(car, eligible)| eligible.then_some(car))
            .collect::<Vec<_>>();
        if eligible_cars.is_empty() {
            return Err(SetupActivationError::NoEligibleCars);
        }
        let saved = CanonicalCar::from_index(profile_byte & 0x0f);
        let selected_car = if let Some(position) =
            saved.and_then(|saved| eligible_cars.iter().position(|&car| car == saved))
        {
            position
        } else {
            eligible_cars
                .iter()
                .position(|&car| car == ineligible_saved_fallback)
                .ok_or(SetupActivationError::IneligibleFallback {
                    fallback: ineligible_saved_fallback,
                })?
        };

        Ok(Self {
            eligible_cars,
            selected_car,
            gear: GearChoice::from_profile(profile_byte),
            selected_row: ROWS.len() - 1,
            phase: SetupPhase::Entering,
            profile_byte,
            exit_completed: false,
            last_previewed_car: None,
        })
    }

    #[must_use]
    pub const fn screen_id(&self) -> ScreenId {
        CHAMPIONSHIP_SETUP_SCREEN_ID
    }

    #[must_use]
    pub const fn parent(&self) -> ScreenId {
        ROOT_SCREEN_ID
    }

    #[must_use]
    pub const fn forward_target(&self) -> ScreenId {
        CHAMPIONSHIP_NEXT_SCREEN_ID
    }

    #[must_use]
    pub const fn phase(&self) -> SetupPhase {
        self.phase
    }

    #[must_use]
    pub const fn selected_row(&self) -> usize {
        self.selected_row
    }

    #[must_use]
    pub fn selected_car(&self) -> CanonicalCar {
        self.eligible_cars[self.selected_car]
    }

    #[must_use]
    pub const fn filtered_car_index(&self) -> usize {
        self.selected_car
    }

    #[must_use]
    pub const fn gear(&self) -> GearChoice {
        self.gear
    }

    #[must_use]
    pub fn eligible_cars(&self) -> &[CanonicalCar] {
        &self.eligible_cars
    }

    /// Completes entry only after all seven caller-observed channels finish.
    pub fn update_entry(&mut self, channels_complete: [bool; ENTRY_ANIMATION_CHANNELS]) {
        if self.phase == SetupPhase::Entering && channels_complete.into_iter().all(|done| done) {
            self.phase = SetupPhase::Interactive;
        }
    }

    /// Applies one already-arbitrated menu action. It is ignored outside Interactive.
    #[must_use]
    pub fn handle_action(&mut self, action: SetupAction) -> Option<SetupTransition> {
        if self.phase != SetupPhase::Interactive {
            return None;
        }

        match action {
            SetupAction::Up => {
                self.selected_row = (self.selected_row + ROWS.len() - 1) % ROWS.len()
            }
            SetupAction::Down => self.selected_row = (self.selected_row + 1) % ROWS.len(),
            SetupAction::Left => self.change_value(false),
            SetupAction::Right => self.change_value(true),
            SetupAction::Enter if ROWS[self.selected_row].target.is_some() => {
                return Some(self.begin_exit(CHAMPIONSHIP_NEXT_SCREEN_ID));
            }
            SetupAction::Escape => return Some(self.begin_exit(ROOT_SCREEN_ID)),
            SetupAction::Enter => {}
        }
        None
    }

    fn change_value(&mut self, forward: bool) {
        match ROWS[self.selected_row].kind {
            SetupRowKind::ChooseCar => {
                let count = self.eligible_cars.len();
                self.selected_car = if forward {
                    (self.selected_car + 1) % count
                } else {
                    (self.selected_car + count - 1) % count
                };
            }
            SetupRowKind::ChooseGear => self.gear = self.gear.toggled(),
            SetupRowKind::Next => {}
        }
    }

    fn begin_exit(&mut self, target: ScreenId) -> SetupTransition {
        let committed_profile_byte =
            (self.profile_byte & !0x1f) | self.selected_car().index() | self.gear.bit();
        let transition = SetupTransition {
            from: CHAMPIONSHIP_SETUP_SCREEN_ID,
            to: target,
            committed_profile_byte,
        };
        self.profile_byte = committed_profile_byte;
        self.phase = SetupPhase::Exiting { target };
        transition
    }

    /// Emits a distinct completion signal once all explicit exit inputs are true.
    #[must_use]
    pub fn complete_exit(
        &mut self,
        channels_complete: [bool; ENTRY_ANIMATION_CHANNELS],
        screen_tween_complete: bool,
    ) -> Option<SetupExitCompletion> {
        if self.exit_completed
            || !screen_tween_complete
            || !channels_complete.into_iter().all(|done| done)
        {
            return None;
        }
        let SetupPhase::Exiting { target } = self.phase else {
            return None;
        };
        self.exit_completed = true;
        Some(SetupExitCompletion { target })
    }

    #[must_use]
    pub fn render_transcript(&mut self) -> ChampionshipSetupRenderTranscript {
        let car = self.selected_car();
        let rows = [
            RenderRow {
                tag: CAR_ROW_TAG,
                text: SetupRowText::ChooseCar,
                selected: self.selected_row == 0,
                value: RowValue::Car(car),
            },
            RenderRow {
                tag: GEAR_ROW_TAG,
                text: SetupRowText::ChooseGear,
                selected: self.selected_row == 1,
                value: RowValue::Gear(self.gear),
            },
            RenderRow {
                tag: NEXT_ROW_TAG,
                text: SetupRowText::Localized(NEXT_ROW_TEXT_INDEX),
                selected: self.selected_row == 2,
                value: RowValue::None,
            },
        ];

        let (preview, palette_group_4_requested) = match self.selected_row {
            0 => {
                let changed = self.last_previewed_car != Some(car);
                self.last_previewed_car = Some(car);
                (
                    ActivePreview::Car {
                        canonical_car: car,
                        object_name: car.object_name(),
                        logo_name: car.logo_name(),
                    },
                    changed,
                )
            }
            1 => (ActivePreview::Gear(self.gear), false),
            _ => (ActivePreview::Next, false),
        };

        ChampionshipSetupRenderTranscript {
            title_text_index: TITLE_TEXT_INDEX,
            rows,
            highlight_row: self.selected_row,
            preview,
            palette_group_4_requested,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_ELIGIBLE: [bool; 14] = [true; 14];
    const COMPLETE: [bool; 7] = [true; 7];

    fn interactive(
        profile: u8,
        mask: [bool; 14],
        fallback: CanonicalCar,
    ) -> ChampionshipSetupState {
        let mut state = ChampionshipSetupState::activate(profile, mask, fallback).unwrap();
        state.update_entry(COMPLETE);
        state
    }

    #[test]
    fn constructor_preserves_rows_tags_initial_selection_and_targets() {
        let state =
            ChampionshipSetupState::activate(0, ALL_ELIGIBLE, CanonicalCar::Subaru).unwrap();
        assert_eq!(
            ROWS.iter().map(|row| row.tag).collect::<Vec<_>>(),
            vec![2, 4, 7]
        );
        assert_eq!(ROWS[2].text, SetupRowText::Localized(0x18));
        assert_eq!(state.selected_row(), 2);
        assert_eq!(state.screen_id().legacy_value(), 0x00d4_0900);
        assert_eq!(state.parent().legacy_value(), 0x00d4_8b80);
        assert_eq!(state.forward_target().legacy_value(), 0x00d4_5cf8);
    }

    #[test]
    fn activation_hydrates_eligible_saved_car_both_gears_and_explicit_fallback() {
        let mask = [
            true, false, false, true, false, false, false, false, false, false, false, false,
            false, true,
        ];
        for gear in [GearChoice::BitClear, GearChoice::BitSet] {
            let bit = if gear == GearChoice::BitSet { 0x10 } else { 0 };
            let state =
                ChampionshipSetupState::activate(3 | bit, mask, CanonicalCar::Subaru).unwrap();
            assert_eq!(
                state.eligible_cars(),
                &[
                    CanonicalCar::Subaru,
                    CanonicalCar::Quattro,
                    CanonicalCar::Celica
                ]
            );
            assert_eq!(state.filtered_car_index(), 1);
            assert_eq!(state.selected_car(), CanonicalCar::Quattro);
            assert_eq!(state.gear(), gear);
        }

        let state = ChampionshipSetupState::activate(2, mask, CanonicalCar::Celica).unwrap();
        assert_eq!(state.selected_car(), CanonicalCar::Celica);
        assert_eq!(state.filtered_car_index(), 2);

        let mut only_saved_is_eligible = [false; 14];
        only_saved_is_eligible[CanonicalCar::Quattro.index() as usize] = true;
        let state = ChampionshipSetupState::activate(
            CanonicalCar::Quattro.index(),
            only_saved_is_eligible,
            CanonicalCar::Subaru,
        )
        .unwrap();
        assert_eq!(state.selected_car(), CanonicalCar::Quattro);
    }

    #[test]
    fn activation_rejects_zero_eligible_cars_and_an_ineligible_fallback() {
        assert_eq!(
            ChampionshipSetupState::activate(0, [false; 14], CanonicalCar::Subaru),
            Err(SetupActivationError::NoEligibleCars)
        );
        let mut mask = [false; 14];
        mask[CanonicalCar::Celica.index() as usize] = true;
        assert_eq!(
            ChampionshipSetupState::activate(0, mask, CanonicalCar::Subaru),
            Err(SetupActivationError::IneligibleFallback {
                fallback: CanonicalCar::Subaru
            })
        );
    }

    #[test]
    fn car_and_row_navigation_wrap_at_both_boundaries() {
        let mask = [
            true, false, false, true, false, false, false, false, false, false, false, false,
            false, true,
        ];
        let mut state = interactive(0, mask, CanonicalCar::Subaru);
        assert_eq!(state.handle_action(SetupAction::Down), None);
        assert_eq!(state.selected_row(), 0);
        assert_eq!(state.handle_action(SetupAction::Left), None);
        assert_eq!(state.selected_car(), CanonicalCar::Celica);
        assert_eq!(state.handle_action(SetupAction::Right), None);
        assert_eq!(state.selected_car(), CanonicalCar::Subaru);
        assert_eq!(state.handle_action(SetupAction::Up), None);
        assert_eq!(state.selected_row(), 2);
        assert_eq!(state.handle_action(SetupAction::Up), None);
        assert_eq!(state.selected_row(), 1);
    }

    #[test]
    fn all_six_arbitrated_actions_are_gated_during_entry_and_exit() {
        for action in [
            SetupAction::Left,
            SetupAction::Right,
            SetupAction::Up,
            SetupAction::Down,
            SetupAction::Enter,
            SetupAction::Escape,
        ] {
            let mut entering =
                ChampionshipSetupState::activate(0, ALL_ELIGIBLE, CanonicalCar::Subaru).unwrap();
            assert_eq!(entering.handle_action(action), None);
            assert_eq!(entering.selected_row(), 2);
        }

        let mut state = interactive(0, ALL_ELIGIBLE, CanonicalCar::Subaru);
        let transition = state.handle_action(SetupAction::Escape).unwrap();
        for action in [
            SetupAction::Left,
            SetupAction::Right,
            SetupAction::Up,
            SetupAction::Down,
            SetupAction::Enter,
            SetupAction::Escape,
        ] {
            assert_eq!(state.handle_action(action), None);
        }
        assert_eq!(
            state.complete_exit(COMPLETE, true),
            Some(SetupExitCompletion {
                target: transition.to
            })
        );
        assert_eq!(state.complete_exit(COMPLETE, true), None);
    }

    #[test]
    fn gear_changes_in_both_directions_and_enter_only_routes_the_submenu_row() {
        let mut state = interactive(0, ALL_ELIGIBLE, CanonicalCar::Subaru);
        let _ = state.handle_action(SetupAction::Up);
        let _ = state.handle_action(SetupAction::Left);
        assert_eq!(state.gear(), GearChoice::BitSet);
        let _ = state.handle_action(SetupAction::Right);
        assert_eq!(state.gear(), GearChoice::BitClear);
        assert_eq!(state.handle_action(SetupAction::Enter), None);
        let _ = state.handle_action(SetupAction::Down);
        assert_eq!(
            state.handle_action(SetupAction::Enter).unwrap().to,
            CHAMPIONSHIP_NEXT_SCREEN_ID
        );
    }

    #[test]
    fn forward_and_escape_commit_canonical_low_nibble_and_gear_bit_once() {
        for target_forward in [true, false] {
            let mut state = interactive(0xe0, ALL_ELIGIBLE, CanonicalCar::Subaru);
            let _ = state.handle_action(SetupAction::Down); // row 0
            let _ = state.handle_action(SetupAction::Left); // Celica, canonical 13
            let _ = state.handle_action(SetupAction::Down); // row 1
            let _ = state.handle_action(SetupAction::Right); // bit 4 set
            if target_forward {
                let _ = state.handle_action(SetupAction::Down); // row 2
            }
            let transition = state
                .handle_action(if target_forward {
                    SetupAction::Enter
                } else {
                    SetupAction::Escape
                })
                .unwrap();
            assert_eq!(transition.committed_profile_byte, 0xfd);
            assert_eq!(
                transition.to,
                if target_forward {
                    CHAMPIONSHIP_NEXT_SCREEN_ID
                } else {
                    ROOT_SCREEN_ID
                }
            );
            assert_eq!(state.handle_action(SetupAction::Escape), None);
            assert_eq!(
                state.complete_exit(COMPLETE, true),
                Some(SetupExitCompletion {
                    target: transition.to
                })
            );
            assert_eq!(state.complete_exit(COMPLETE, true), None);
        }
    }

    #[test]
    fn entry_and_exit_require_every_explicit_completion_input() {
        let mut state =
            ChampionshipSetupState::activate(0, ALL_ELIGIBLE, CanonicalCar::Subaru).unwrap();
        let mut incomplete = COMPLETE;
        incomplete[6] = false;
        state.update_entry(incomplete);
        assert_eq!(state.phase(), SetupPhase::Entering);
        state.update_entry(COMPLETE);
        assert_eq!(state.phase(), SetupPhase::Interactive);
        let transition = state.handle_action(SetupAction::Escape).unwrap();
        assert_eq!(state.complete_exit(incomplete, true), None);
        assert_eq!(state.complete_exit(COMPLETE, false), None);
        assert_eq!(
            state.complete_exit(COMPLETE, true),
            Some(SetupExitCompletion {
                target: transition.to
            })
        );
        assert_eq!(state.complete_exit(COMPLETE, true), None);
    }

    #[test]
    fn render_transcript_tracks_each_active_row_mappings_and_palette_changes() {
        let mut state = interactive(1, ALL_ELIGIBLE, CanonicalCar::Subaru);
        let next = state.render_transcript();
        assert_eq!(next.title_text_index, 0x13);
        assert_eq!(next.highlight_row, 2);
        assert_eq!(next.preview, ActivePreview::Next);
        assert!(!next.palette_group_4_requested);
        assert_eq!(
            next.rows.iter().map(|row| row.tag).collect::<Vec<_>>(),
            vec![2, 4, 7]
        );

        let _ = state.handle_action(SetupAction::Down);
        let car = state.render_transcript();
        assert_eq!(car.highlight_row, 0);
        assert_eq!(
            car.preview,
            ActivePreview::Car {
                canonical_car: CanonicalCar::Ibiza,
                object_name: "IBIZA",
                logo_name: "SEAT"
            }
        );
        assert!(car.palette_group_4_requested);
        assert!(!state.render_transcript().palette_group_4_requested);

        let _ = state.handle_action(SetupAction::Right);
        let changed = state.render_transcript();
        assert_eq!(
            changed.preview,
            ActivePreview::Car {
                canonical_car: CanonicalCar::Skoda,
                object_name: "SKODA",
                logo_name: "SKODA"
            }
        );
        assert!(changed.palette_group_4_requested);

        let _ = state.handle_action(SetupAction::Down);
        let gear = state.render_transcript();
        assert_eq!(gear.preview, ActivePreview::Gear(GearChoice::BitClear));
        assert!(!gear.palette_group_4_requested);
        assert_eq!(gear.rows.iter().filter(|row| row.selected).count(), 1);
    }

    #[test]
    fn every_canonical_car_has_the_recovered_object_and_logo_mapping() {
        let objects = CanonicalCar::ALL.map(CanonicalCar::object_name);
        assert_eq!(
            objects,
            [
                "SUBARU", "IBIZA", "SKODA", "QUATTRO", "RS200", "MK2", "MITSUBIS", "GOLF",
                "INTEGRAL", "COROLLA", "ESCORT", "MEGANE", "M4R", "CELICA"
            ]
        );
        let logos = CanonicalCar::ALL.map(CanonicalCar::logo_name);
        assert_eq!(
            logos,
            [
                "SUBARU", "SEAT", "SKODA", "AUDI", "FORD", "FORD", "MITSU", "VW", "LANCIA", "TOYO",
                "FORD", "RENO", "M4R", "TOYO"
            ]
        );
    }
}
