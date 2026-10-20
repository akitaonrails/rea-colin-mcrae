//! Pure scheduling contract for the recovered root-menu animations.
//!
//! This module owns no clock and performs no interpolation. It emits typed
//! schedules and accepts sampled Q16 positions from a future animation driver.

use crate::{
    frontend::RootSelection,
    root_menu_visual::{CHROME_SPECS, ROOT_ENTRY_COUNT},
};

pub const Q16_WRAP: i32 = 0x1_0000;
pub const PRIMARY_DURATION: u8 = 8;
pub const SECONDARY_DURATION: u8 = 1;
pub const ENTER_DURATION: u8 = 1;
pub const SECONDARY_ENDPOINTS_Q16: [i32; ROOT_ENTRY_COUNT] =
    [0x1_0000, 0x1_0000, 0x1_0000, 0x8000, 0x8000, 0x8000, 0x8000];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum TweenType {
    Type2 = 2,
    Type3 = 3,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AnimationChannel {
    PrimaryChrome,
    Secondary,
    EntryVisibility,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SampledPosition {
    PrimaryChrome,
    Secondary,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AnimationSchedule {
    pub channel: AnimationChannel,
    pub tween: TweenType,
    pub duration: u8,
    pub from_q16: i32,
    pub to_q16: i32,
}

impl AnimationSchedule {
    const fn new(
        channel: AnimationChannel,
        tween: TweenType,
        duration: u8,
        from_q16: i32,
        to_q16: i32,
    ) -> Self {
        Self {
            channel,
            tween,
            duration,
            from_q16,
            to_q16,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RootMenuAnimationState {
    selection: RootSelection,
    represented_selection: RootSelection,
    primary_position_q16: i32,
    secondary_position_q16: i32,
    entry_visuals_suppressed: bool,
}

impl RootMenuAnimationState {
    /// Activates a newly entered root screen.
    ///
    /// The primary position is initialized from construction-time selection
    /// zero before the saved selection is applied. The secondary position is
    /// explicit because this activation routine did not establish its value.
    #[must_use]
    pub fn activate_new(saved_selection: RootSelection, secondary_position_q16: i32) -> Self {
        let zero = RootSelection::new(0).expect("zero is a valid root selection");
        Self {
            selection: saved_selection,
            represented_selection: zero,
            primary_position_q16: CHROME_SPECS[0].gate_q16,
            secondary_position_q16,
            entry_visuals_suppressed: false,
        }
    }

    #[must_use]
    pub const fn selection(&self) -> RootSelection {
        self.selection
    }

    #[must_use]
    pub const fn represented_selection(&self) -> RootSelection {
        self.represented_selection
    }

    #[must_use]
    pub const fn primary_position_q16(&self) -> i32 {
        self.primary_position_q16
    }

    #[must_use]
    pub const fn secondary_position_q16(&self) -> i32 {
        self.secondary_position_q16
    }

    #[must_use]
    pub const fn entry_visuals_suppressed(&self) -> bool {
        self.entry_visuals_suppressed
    }

    pub fn set_selection(&mut self, selection: RootSelection) {
        self.selection = selection;
    }

    /// Records a value produced by an external interpolation driver.
    pub fn apply_sample(&mut self, position: SampledPosition, position_q16: i32) {
        match position {
            SampledPosition::PrimaryChrome => self.primary_position_q16 = position_q16,
            SampledPosition::Secondary => self.secondary_position_q16 = position_q16,
        }
    }

    /// Emits selection-change schedules in the recovered order.
    #[must_use]
    pub fn steady_update(&mut self) -> Vec<AnimationSchedule> {
        if self.selection == self.represented_selection {
            return Vec::new();
        }

        let old = i32::from(self.represented_selection.index());
        let new = i32::from(self.selection.index());
        let difference = new - old;
        if difference > 1 {
            self.primary_position_q16 = self.primary_position_q16.wrapping_add(Q16_WRAP);
        } else if difference < -1 {
            self.primary_position_q16 = self.primary_position_q16.wrapping_sub(Q16_WRAP);
        }

        let selection = usize::from(self.selection.index());
        let schedules = vec![
            AnimationSchedule::new(
                AnimationChannel::PrimaryChrome,
                TweenType::Type2,
                PRIMARY_DURATION,
                self.primary_position_q16,
                CHROME_SPECS[selection].gate_q16,
            ),
            AnimationSchedule::new(
                AnimationChannel::Secondary,
                TweenType::Type2,
                SECONDARY_DURATION,
                self.secondary_position_q16,
                SECONDARY_ENDPOINTS_Q16[selection],
            ),
        ];
        self.represented_selection = self.selection;
        schedules
    }

    /// Begins the recovered enter transition and suppresses root entry visuals.
    #[must_use]
    pub fn begin_enter_transition(&mut self) -> [AnimationSchedule; 2] {
        self.entry_visuals_suppressed = true;
        [
            AnimationSchedule::new(
                AnimationChannel::Secondary,
                TweenType::Type3,
                ENTER_DURATION,
                self.secondary_position_q16,
                0,
            ),
            AnimationSchedule::new(
                AnimationChannel::EntryVisibility,
                TweenType::Type2,
                ENTER_DURATION,
                Q16_WRAP,
                0,
            ),
        ]
    }

    /// Performs the normal activation-side suppression reset.
    pub fn activate_normally(&mut self) {
        self.entry_visuals_suppressed = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selection(value: u8) -> RootSelection {
        RootSelection::new(value).unwrap()
    }

    #[test]
    fn new_activation_starts_at_zero_before_every_saved_selection() {
        for saved in 0..ROOT_ENTRY_COUNT as u8 {
            let state = RootMenuAnimationState::activate_new(selection(saved), 0x3456);
            assert_eq!(state.selection().index(), saved);
            assert_eq!(state.represented_selection().index(), 0);
            assert_eq!(state.primary_position_q16(), 0);
            assert_eq!(state.secondary_position_q16(), 0x3456);
            assert!(!state.entry_visuals_suppressed());
        }
    }

    #[test]
    fn first_steady_update_targets_every_saved_selection_endpoint() {
        for saved in 0..ROOT_ENTRY_COUNT as u8 {
            let mut state = RootMenuAnimationState::activate_new(selection(saved), -0x1234);
            let schedules = state.steady_update();
            if saved == 0 {
                assert!(schedules.is_empty());
                continue;
            }

            let expected_from = if saved > 1 { Q16_WRAP } else { 0 };
            assert_eq!(
                schedules,
                vec![
                    AnimationSchedule::new(
                        AnimationChannel::PrimaryChrome,
                        TweenType::Type2,
                        8,
                        expected_from,
                        CHROME_SPECS[usize::from(saved)].gate_q16,
                    ),
                    AnimationSchedule::new(
                        AnimationChannel::Secondary,
                        TweenType::Type2,
                        1,
                        -0x1234,
                        SECONDARY_ENDPOINTS_Q16[usize::from(saved)],
                    ),
                ]
            );
            assert_eq!(state.represented_selection().index(), saved);
            assert!(state.steady_update().is_empty());
        }
    }

    #[test]
    fn all_selection_pairs_use_exact_wrap_correction_and_endpoints() {
        const CURRENT_PRIMARY: i32 = 0x2468;
        const CURRENT_SECONDARY: i32 = -0x1357;

        for from in 0..ROOT_ENTRY_COUNT as u8 {
            for to in 0..ROOT_ENTRY_COUNT as u8 {
                let mut state = RootMenuAnimationState::activate_new(selection(from), 0);
                let _ = state.steady_update();
                state.apply_sample(SampledPosition::PrimaryChrome, CURRENT_PRIMARY);
                state.apply_sample(SampledPosition::Secondary, CURRENT_SECONDARY);
                state.set_selection(selection(to));

                let schedules = state.steady_update();
                if from == to {
                    assert!(schedules.is_empty(), "{from}->{to}");
                    continue;
                }

                let difference = i32::from(to) - i32::from(from);
                let expected_from = match difference {
                    2.. => CURRENT_PRIMARY.wrapping_add(Q16_WRAP),
                    ..=-2 => CURRENT_PRIMARY.wrapping_sub(Q16_WRAP),
                    _ => CURRENT_PRIMARY,
                };
                assert_eq!(
                    schedules,
                    vec![
                        AnimationSchedule::new(
                            AnimationChannel::PrimaryChrome,
                            TweenType::Type2,
                            PRIMARY_DURATION,
                            expected_from,
                            CHROME_SPECS[usize::from(to)].gate_q16,
                        ),
                        AnimationSchedule::new(
                            AnimationChannel::Secondary,
                            TweenType::Type2,
                            SECONDARY_DURATION,
                            CURRENT_SECONDARY,
                            SECONDARY_ENDPOINTS_Q16[usize::from(to)],
                        ),
                    ],
                    "{from}->{to}"
                );
            }
        }
    }

    #[test]
    fn wrap_correction_uses_legacy_wrapping_integer_arithmetic() {
        let mut forward = RootMenuAnimationState::activate_new(selection(0), 0);
        forward.apply_sample(SampledPosition::PrimaryChrome, i32::MAX);
        forward.set_selection(selection(2));
        assert_eq!(
            forward.steady_update()[0].from_q16,
            i32::MAX.wrapping_add(Q16_WRAP)
        );

        let mut backward = RootMenuAnimationState::activate_new(selection(6), 0);
        let _ = backward.steady_update();
        backward.apply_sample(SampledPosition::PrimaryChrome, i32::MIN);
        backward.set_selection(selection(0));
        assert_eq!(
            backward.steady_update()[0].from_q16,
            i32::MIN.wrapping_sub(Q16_WRAP)
        );
    }

    #[test]
    fn secondary_endpoint_table_is_exact() {
        assert_eq!(
            SECONDARY_ENDPOINTS_Q16,
            [0x1_0000, 0x1_0000, 0x1_0000, 0x8000, 0x8000, 0x8000, 0x8000]
        );
    }

    #[test]
    fn enter_transition_sets_suppression_and_emits_two_ordered_schedules() {
        let mut state = RootMenuAnimationState::activate_new(selection(4), 0x4567);
        assert_eq!(
            state.begin_enter_transition(),
            [
                AnimationSchedule::new(AnimationChannel::Secondary, TweenType::Type3, 1, 0x4567, 0,),
                AnimationSchedule::new(
                    AnimationChannel::EntryVisibility,
                    TweenType::Type2,
                    1,
                    0x1_0000,
                    0,
                ),
            ]
        );
        assert!(state.entry_visuals_suppressed());
    }

    #[test]
    fn normal_activation_clears_suppression_without_changing_positions() {
        let mut state = RootMenuAnimationState::activate_new(selection(5), 0x2222);
        state.apply_sample(SampledPosition::PrimaryChrome, 0x1111);
        let _ = state.begin_enter_transition();
        state.activate_normally();
        assert!(!state.entry_visuals_suppressed());
        assert_eq!(state.primary_position_q16(), 0x1111);
        assert_eq!(state.secondary_position_q16(), 0x2222);
        assert_eq!(state.selection().index(), 5);
    }

    #[test]
    fn sampling_updates_both_position_channels() {
        let mut state = RootMenuAnimationState::activate_new(selection(0), 2);
        state.apply_sample(SampledPosition::PrimaryChrome, 3);
        state.apply_sample(SampledPosition::Secondary, 4);
        assert_eq!(state.primary_position_q16(), 3);
        assert_eq!(state.secondary_position_q16(), 4);
        assert!(!state.entry_visuals_suppressed());
    }

    #[test]
    fn tween_discriminants_and_durations_preserve_numeric_protocol() {
        assert_eq!(TweenType::Type2 as u8, 2);
        assert_eq!(TweenType::Type3 as u8, 3);
        assert_eq!(PRIMARY_DURATION, 8);
        assert_eq!(SECONDARY_DURATION, 1);
        assert_eq!(ENTER_DURATION, 1);
    }
}
