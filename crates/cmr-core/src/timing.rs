//! Platform-neutral reconstruction of the frontend timing rules.
//!
//! The original WinMM and Win32 calls are represented as plans and explicit
//! transitions. Host timer creation, threads, and clock reads remain backend
//! responsibilities.

pub const INITIAL_TIMER_DELAY_MS: u32 = 33;
pub const COMPATIBILITY_BUCKET_MS: u32 = 40;
pub const Q16_ONE: u32 = 65_536;
pub const TWENTY_SECOND_DEADLINE_THRESHOLD_MS: u32 = 19_999;
pub const SIXTY_SECOND_DEADLINE_THRESHOLD_MS: u32 = 59_999;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogicalRate {
    Hz25,
    Hz50,
}

impl LogicalRate {
    pub const fn from_fast_flag(fast: bool) -> Self {
        if fast { Self::Hz50 } else { Self::Hz25 }
    }

    pub const fn hz(self) -> u32 {
        match self {
            Self::Hz25 => 25,
            Self::Hz50 => 50,
        }
    }

    pub const fn period_ms(self) -> u32 {
        1_000 / self.hz()
    }
}

pub const fn is_async_state(state: i32) -> bool {
    state >= 5 && state <= 10
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HighLevelStepPlan {
    StartAsyncTimer { rate: LogicalRate },
    AsyncTimerOwnsTicks,
    RunSynchronousTick,
    RunBucketedCompatibilityTick,
}

/// Reconstructs the high-level scheduling branch selected by the frontend.
pub const fn high_level_step_plan(
    state: i32,
    compatibility_mode: bool,
    timer_started: bool,
    fast_rate: bool,
) -> HighLevelStepPlan {
    if compatibility_mode {
        if is_async_state(state) {
            HighLevelStepPlan::RunBucketedCompatibilityTick
        } else {
            HighLevelStepPlan::RunSynchronousTick
        }
    } else if !is_async_state(state) {
        HighLevelStepPlan::RunSynchronousTick
    } else if timer_started {
        HighLevelStepPlan::AsyncTimerOwnsTicks
    } else {
        HighLevelStepPlan::StartAsyncTimer {
            rate: LogicalRate::from_fast_flag(fast_rate),
        }
    }
}

pub const fn bucket_changed_40ms(before_ms: u32, after_ms: u32) -> bool {
    before_ms / COMPATIBILITY_BUCKET_MS != after_ms / COMPATIBILITY_BUCKET_MS
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CompatibilityStepPlan {
    pub steps: usize,
    /// More timestamp pairs are needed because every supplied step crossed a
    /// bucket and the original loop would execute again.
    pub needs_another_step: bool,
}

/// Evaluates scripted `(before, after)` timestamps for the compatibility
/// do/while loop. Each pair represents one completed high-level tick.
pub fn compatibility_step_plan(step_windows: &[(u32, u32)]) -> CompatibilityStepPlan {
    for (index, &(before, after)) in step_windows.iter().enumerate() {
        if !bucket_changed_40ms(before, after) {
            return CompatibilityStepPlan {
                steps: index + 1,
                needs_another_step: false,
            };
        }
    }

    CompatibilityStepPlan {
        steps: step_windows.len(),
        needs_another_step: true,
    }
}

/// Counts samples through the first absolute 40-ms bucket change. `None`
/// means the supplied script ended while the original busy-wait would remain.
pub fn compatibility_wait_samples(start_ms: u32, samples: &[u32]) -> Option<usize> {
    samples
        .iter()
        .position(|&sample| bucket_changed_40ms(start_ms, sample))
        .map(|index| index + 1)
}

/// Exact low-32-bit QPC subtraction used by the recovered executable.
///
/// The `-1` on equality and wrap is intentional legacy behavior.
const fn legacy_low_qpc_delta_unadjusted(previous: u32, now: u32) -> u32 {
    if now > previous {
        now - previous
    } else {
        now.wrapping_sub(previous).wrapping_sub(1)
    }
}

pub const fn legacy_low_qpc_delta(previous: u32, now: u32) -> u32 {
    let delta = legacy_low_qpc_delta_unadjusted(previous, now);
    if delta == 0 { 1 } else { delta }
}

/// Low-QPC subtraction used inside the timer callback. Unlike the standalone
/// scaling helper, this path does not promote a zero delta to one.
pub const fn legacy_callback_qpc_delta(previous: u32, now: u32) -> u32 {
    legacy_low_qpc_delta_unadjusted(previous, now)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimingInputError {
    ZeroFrequency,
    ZeroTargetTicks,
    InvalidTicksPerMillisecond,
    InvalidPeriod,
    AccumulatorOverflow,
}

impl std::fmt::Display for TimingInputError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "invalid timing input: {self:?}")
    }
}

impl std::error::Error for TimingInputError {}

/// Converts a low-QPC delta to the executable's clamped Q16 scale.
///
/// A zero frequency is outside the recovered successful-initialization path.
pub fn legacy_q16_scale(delta: u32, frequency: u32) -> Result<u32, TimingInputError> {
    if frequency == 0 {
        return Err(TimingInputError::ZeroFrequency);
    }
    // FILD loads both integers exactly, FDIV is rounded only by the following
    // FSTP-to-f32, then the reloaded f32 is multiplied in x87 precision.
    let ratio = (f64::from(delta) / f64::from(frequency)) as f32;
    let scaled = (f64::from(ratio) * f64::from(Q16_ONE)).round_ties_even();
    Ok(if scaled <= 0.0 {
        0
    } else if scaled >= f64::from(Q16_ONE) {
        Q16_ONE
    } else {
        scaled as u32
    })
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DriftCorrection {
    accumulated_ticks: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriftPlan {
    pub next_delay_ms: u32,
    pub accumulated_ticks: i32,
}

impl DriftCorrection {
    pub const fn accumulated_ticks(self) -> i32 {
        self.accumulated_ticks
    }

    /// Applies one callback's low-QPC observation and chooses the next
    /// one-shot delay. Comparisons are strict, matching the executable.
    pub fn observe(
        &mut self,
        previous_counter: u32,
        now_counter: u32,
        target_ticks: u32,
        ticks_per_ms: u32,
        period_ms: u32,
    ) -> Result<DriftPlan, TimingInputError> {
        if target_ticks == 0 {
            return Err(TimingInputError::ZeroTargetTicks);
        }
        if ticks_per_ms == 0 || ticks_per_ms > i32::MAX as u32 / 2 {
            return Err(TimingInputError::InvalidTicksPerMillisecond);
        }
        if period_ms <= 1 || period_ms == u32::MAX {
            return Err(TimingInputError::InvalidPeriod);
        }

        let elapsed = legacy_callback_qpc_delta(previous_counter, now_counter);
        let error_ticks = elapsed.wrapping_sub(target_ticks) as i32;
        let Some(mut accumulated_ticks) = self.accumulated_ticks.checked_add(error_ticks) else {
            return Err(TimingInputError::AccumulatorOverflow);
        };

        let tick_ms = ticks_per_ms as i32;
        let tolerance = tick_ms * 2;
        if accumulated_ticks > tolerance || accumulated_ticks < -tolerance {
            accumulated_ticks = 0;
        }

        let next_delay_ms = if accumulated_ticks > tick_ms {
            accumulated_ticks -= tick_ms;
            period_ms - 1
        } else if accumulated_ticks < -tick_ms {
            accumulated_ticks += tick_ms;
            period_ms + 1
        } else {
            period_ms
        };

        self.accumulated_ticks = accumulated_ticks;
        Ok(DriftPlan {
            next_delay_ms,
            accumulated_ticks,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimerOperation {
    BeginPeriod(u32),
    SetOneShot { delay_ms: u32 },
    EndPeriod(u32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimerStartState {
    BeginResolution { candidate: usize },
    SetInitialEvent { resolution_ms: u32 },
    Running { resolution_ms: u32 },
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimerStartEvent {
    ResolutionResult(bool),
    InitialEventResult(bool),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimerTransition {
    pub state: TimerStartState,
    pub operation: Option<TimerOperation>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimerStartMachine {
    state: TimerStartState,
}

impl Default for TimerStartMachine {
    fn default() -> Self {
        Self::new()
    }
}

impl TimerStartMachine {
    const RESOLUTIONS_MS: [u32; 3] = [1, 2, 3];

    pub const fn new() -> Self {
        Self {
            state: TimerStartState::BeginResolution { candidate: 0 },
        }
    }

    pub const fn state(self) -> TimerStartState {
        self.state
    }

    pub const fn initial_operation() -> TimerOperation {
        TimerOperation::BeginPeriod(Self::RESOLUTIONS_MS[0])
    }

    pub fn advance(&mut self, event: TimerStartEvent) -> Option<TimerTransition> {
        let (state, operation) = match (self.state, event) {
            (
                TimerStartState::BeginResolution { candidate },
                TimerStartEvent::ResolutionResult(true),
            ) => {
                let resolution_ms = Self::RESOLUTIONS_MS[candidate];
                (
                    TimerStartState::SetInitialEvent { resolution_ms },
                    Some(TimerOperation::SetOneShot {
                        delay_ms: INITIAL_TIMER_DELAY_MS,
                    }),
                )
            }
            (
                TimerStartState::BeginResolution { candidate },
                TimerStartEvent::ResolutionResult(false),
            ) if candidate + 1 < Self::RESOLUTIONS_MS.len() => {
                let candidate = candidate + 1;
                (
                    TimerStartState::BeginResolution { candidate },
                    Some(TimerOperation::BeginPeriod(Self::RESOLUTIONS_MS[candidate])),
                )
            }
            (TimerStartState::BeginResolution { .. }, TimerStartEvent::ResolutionResult(false)) => {
                (TimerStartState::Failed, None)
            }
            (
                TimerStartState::SetInitialEvent { resolution_ms },
                TimerStartEvent::InitialEventResult(true),
            ) => (TimerStartState::Running { resolution_ms }, None),
            (
                TimerStartState::SetInitialEvent { resolution_ms },
                TimerStartEvent::InitialEventResult(false),
            ) => (
                TimerStartState::Failed,
                Some(TimerOperation::EndPeriod(resolution_ms)),
            ),
            _ => return None,
        };

        self.state = state;
        Some(TimerTransition { state, operation })
    }

    pub const fn stop_operation(self) -> Option<TimerOperation> {
        match self.state {
            TimerStartState::Running { resolution_ms } => {
                Some(TimerOperation::EndPeriod(resolution_ms))
            }
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FocusTimingPlan {
    pub run_foreground_frame: bool,
    pub async_tick_eligible: bool,
}

pub const fn focus_timing_plan(focused: bool, async_timer_started: bool) -> FocusTimingPlan {
    FocusTimingPlan {
        run_foreground_frame: focused,
        async_tick_eligible: async_timer_started,
    }
}

pub const fn unsigned_elapsed_ms(start_ms: u32, now_ms: u32) -> u32 {
    now_ms.wrapping_sub(start_ms)
}

pub const fn elapsed_exceeds(start_ms: u32, now_ms: u32, threshold_ms: u32) -> bool {
    unsigned_elapsed_ms(start_ms, now_ms) > threshold_ms
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_rates_have_recovered_periods() {
        assert_eq!(LogicalRate::from_fast_flag(false).hz(), 25);
        assert_eq!(LogicalRate::from_fast_flag(false).period_ms(), 40);
        assert_eq!(LogicalRate::from_fast_flag(true).hz(), 50);
        assert_eq!(LogicalRate::from_fast_flag(true).period_ms(), 20);
    }

    #[test]
    fn async_timer_is_lazy_and_limited_to_states_five_through_ten() {
        assert_eq!(
            high_level_step_plan(4, false, false, false),
            HighLevelStepPlan::RunSynchronousTick
        );
        assert_eq!(
            high_level_step_plan(5, false, false, false),
            HighLevelStepPlan::StartAsyncTimer {
                rate: LogicalRate::Hz25
            }
        );
        assert_eq!(
            high_level_step_plan(10, false, false, true),
            HighLevelStepPlan::StartAsyncTimer {
                rate: LogicalRate::Hz50
            }
        );
        assert_eq!(
            high_level_step_plan(11, false, false, false),
            HighLevelStepPlan::RunSynchronousTick
        );
        assert_eq!(
            high_level_step_plan(4, false, true, false),
            HighLevelStepPlan::RunSynchronousTick
        );
        assert_eq!(
            high_level_step_plan(5, false, true, false),
            HighLevelStepPlan::AsyncTimerOwnsTicks
        );
        for state in [i32::MIN, 4, 11, i32::MAX] {
            assert_eq!(
                high_level_step_plan(state, false, true, true),
                HighLevelStepPlan::RunSynchronousTick
            );
        }
    }

    #[test]
    fn compatibility_mode_uses_bucketed_steps_only_in_timed_states() {
        assert_eq!(
            high_level_step_plan(5, true, false, false),
            HighLevelStepPlan::RunBucketedCompatibilityTick
        );
        assert_eq!(
            high_level_step_plan(11, true, true, true),
            HighLevelStepPlan::RunSynchronousTick
        );
    }

    #[test]
    fn absolute_forty_ms_bucket_edges_and_wrap_are_preserved() {
        assert!(!bucket_changed_40ms(0, 39));
        assert!(bucket_changed_40ms(39, 40));
        assert!(!bucket_changed_40ms(40, 79));
        assert!(bucket_changed_40ms(79, 80));
        assert!(bucket_changed_40ms(u32::MAX, 0));
    }

    #[test]
    fn compatibility_step_script_repeats_only_after_crossing_a_bucket() {
        assert_eq!(
            compatibility_step_plan(&[(39, 40), (40, 79)]),
            CompatibilityStepPlan {
                steps: 2,
                needs_another_step: false
            }
        );
        assert_eq!(
            compatibility_step_plan(&[(39, 40), (79, 80)]),
            CompatibilityStepPlan {
                steps: 2,
                needs_another_step: true
            }
        );
        assert_eq!(
            compatibility_step_plan(&[]),
            CompatibilityStepPlan {
                steps: 0,
                needs_another_step: true
            }
        );
    }

    #[test]
    fn compatibility_wait_stops_at_first_new_bucket() {
        assert_eq!(compatibility_wait_samples(40, &[41, 79, 80, 81]), Some(3));
        assert_eq!(compatibility_wait_samples(40, &[41, 79]), None);
        assert_eq!(compatibility_wait_samples(u32::MAX, &[0]), Some(1));
    }

    #[test]
    fn low_qpc_delta_preserves_equality_and_wrap_quirks() {
        assert_eq!(legacy_low_qpc_delta(100, 101), 1);
        assert_eq!(legacy_low_qpc_delta(100, 100), u32::MAX);
        assert_eq!(legacy_low_qpc_delta(0xffff_fff0, 0x10), 31);
        assert_eq!(legacy_low_qpc_delta(100, 102), 2);
        assert_eq!(legacy_low_qpc_delta(u32::MAX, 0), 1);
        assert_eq!(legacy_callback_qpc_delta(u32::MAX, 0), 0);
    }

    #[test]
    fn q16_scale_rounds_and_clamps() {
        assert_eq!(legacy_q16_scale(0, 65_536), Ok(0));
        assert_eq!(legacy_q16_scale(1, 65_536), Ok(1));
        assert_eq!(legacy_q16_scale(50, 100), Ok(32_768));
        assert_eq!(legacy_q16_scale(100, 100), Ok(Q16_ONE));
        assert_eq!(legacy_q16_scale(u32::MAX, 100), Ok(Q16_ONE));
    }

    #[test]
    fn q16_scale_rejects_frequency_zero() {
        assert_eq!(legacy_q16_scale(1, 0), Err(TimingInputError::ZeroFrequency));
    }

    #[test]
    fn drift_correction_uses_strict_one_ms_thresholds() {
        let mut exact = DriftCorrection::default();
        assert_eq!(
            exact.observe(0, 1_000, 1_000, 100, 40),
            Ok(DriftPlan {
                next_delay_ms: 40,
                accumulated_ticks: 0
            })
        );

        let mut at_threshold = DriftCorrection::default();
        assert_eq!(
            at_threshold.observe(0, 1_100, 1_000, 100, 40),
            Ok(DriftPlan {
                next_delay_ms: 40,
                accumulated_ticks: 100
            })
        );

        let mut late = DriftCorrection::default();
        assert_eq!(
            late.observe(0, 1_101, 1_000, 100, 40),
            Ok(DriftPlan {
                next_delay_ms: 39,
                accumulated_ticks: 1
            })
        );

        let mut early = DriftCorrection::default();
        assert_eq!(
            early.observe(0, 899, 1_000, 100, 40),
            Ok(DriftPlan {
                next_delay_ms: 41,
                accumulated_ticks: -1
            })
        );
    }

    #[test]
    fn drift_correction_preserves_strict_two_ms_reset_boundaries() {
        let mut positive_boundary = DriftCorrection::default();
        assert_eq!(
            positive_boundary.observe(0, 1_200, 1_000, 100, 40),
            Ok(DriftPlan {
                next_delay_ms: 39,
                accumulated_ticks: 100
            })
        );

        let mut negative_boundary = DriftCorrection::default();
        assert_eq!(
            negative_boundary.observe(0, 800, 1_000, 100, 40),
            Ok(DriftPlan {
                next_delay_ms: 41,
                accumulated_ticks: -100
            })
        );
    }

    #[test]
    fn extreme_drift_resets_before_delay_selection() {
        let mut correction = DriftCorrection::default();
        assert_eq!(
            correction.observe(0, 1_201, 1_000, 100, 40),
            Ok(DriftPlan {
                next_delay_ms: 40,
                accumulated_ticks: 0
            })
        );
    }

    #[test]
    fn drift_correction_rejects_invalid_zero_and_extreme_inputs_without_mutation() {
        let invalid_cases = [
            ((0, 1, 0, 1, 40), TimingInputError::ZeroTargetTicks),
            (
                (0, 1, 1, 0, 40),
                TimingInputError::InvalidTicksPerMillisecond,
            ),
            (
                (0, 1, 1, i32::MAX as u32 / 2 + 1, 40),
                TimingInputError::InvalidTicksPerMillisecond,
            ),
            ((0, 1, 1, 1, 0), TimingInputError::InvalidPeriod),
            ((0, 1, 1, 1, 1), TimingInputError::InvalidPeriod),
            ((0, 1, 1, 1, u32::MAX), TimingInputError::InvalidPeriod),
        ];

        for ((previous, now, target, ticks_per_ms, period), expected) in invalid_cases {
            let mut correction = DriftCorrection::default();
            assert_eq!(
                correction.observe(previous, now, target, ticks_per_ms, period),
                Err(expected)
            );
            assert_eq!(correction.accumulated_ticks(), 0);
        }
    }

    #[test]
    fn drift_correction_reports_accumulator_overflow_without_mutation() {
        let mut positive = DriftCorrection {
            accumulated_ticks: i32::MAX,
        };
        assert_eq!(
            positive.observe(0, 2, 1, 1, 40),
            Err(TimingInputError::AccumulatorOverflow)
        );
        assert_eq!(positive.accumulated_ticks(), i32::MAX);

        let mut negative = DriftCorrection {
            accumulated_ticks: i32::MIN,
        };
        assert_eq!(
            negative.observe(0, 1, 2, 1, 40),
            Err(TimingInputError::AccumulatorOverflow)
        );
        assert_eq!(negative.accumulated_ticks(), i32::MIN);
    }

    #[test]
    fn drift_correction_handles_callback_zero_delta_without_panicking() {
        let mut correction = DriftCorrection::default();
        assert_eq!(
            correction.observe(u32::MAX, 0, 1, 1, 40),
            Ok(DriftPlan {
                next_delay_ms: 40,
                accumulated_ticks: -1
            })
        );
    }

    #[test]
    fn drift_correction_accepts_safe_delay_and_tick_extremes() {
        let mut minimum_delay = DriftCorrection::default();
        assert_eq!(
            minimum_delay.observe(0, 3, 1, 1, 2),
            Ok(DriftPlan {
                next_delay_ms: 1,
                accumulated_ticks: 1
            })
        );

        let mut maximum_delay = DriftCorrection::default();
        assert_eq!(
            maximum_delay.observe(0, 1, 3, 1, u32::MAX - 1),
            Ok(DriftPlan {
                next_delay_ms: u32::MAX,
                accumulated_ticks: -1
            })
        );

        let mut maximum_tick_scale = DriftCorrection::default();
        assert_eq!(
            maximum_tick_scale.observe(0, 1, 1, i32::MAX as u32 / 2, 40),
            Ok(DriftPlan {
                next_delay_ms: 40,
                accumulated_ticks: 0
            })
        );
    }

    #[test]
    fn timer_resolution_falls_back_then_starts_one_shot() {
        let mut machine = TimerStartMachine::new();
        assert_eq!(
            TimerStartMachine::initial_operation(),
            TimerOperation::BeginPeriod(1)
        );
        assert_eq!(
            machine.advance(TimerStartEvent::ResolutionResult(false)),
            Some(TimerTransition {
                state: TimerStartState::BeginResolution { candidate: 1 },
                operation: Some(TimerOperation::BeginPeriod(2))
            })
        );
        assert_eq!(
            machine.advance(TimerStartEvent::ResolutionResult(true)),
            Some(TimerTransition {
                state: TimerStartState::SetInitialEvent { resolution_ms: 2 },
                operation: Some(TimerOperation::SetOneShot {
                    delay_ms: INITIAL_TIMER_DELAY_MS
                })
            })
        );
        assert_eq!(
            machine.advance(TimerStartEvent::InitialEventResult(true)),
            Some(TimerTransition {
                state: TimerStartState::Running { resolution_ms: 2 },
                operation: None
            })
        );
        assert_eq!(machine.stop_operation(), Some(TimerOperation::EndPeriod(2)));
    }

    #[test]
    fn timer_resolution_accepts_first_or_third_candidate() {
        let mut first = TimerStartMachine::new();
        assert_eq!(
            first.advance(TimerStartEvent::ResolutionResult(true)),
            Some(TimerTransition {
                state: TimerStartState::SetInitialEvent { resolution_ms: 1 },
                operation: Some(TimerOperation::SetOneShot {
                    delay_ms: INITIAL_TIMER_DELAY_MS
                })
            })
        );

        let mut third = TimerStartMachine::new();
        third.advance(TimerStartEvent::ResolutionResult(false));
        third.advance(TimerStartEvent::ResolutionResult(false));
        assert_eq!(
            third.advance(TimerStartEvent::ResolutionResult(true)),
            Some(TimerTransition {
                state: TimerStartState::SetInitialEvent { resolution_ms: 3 },
                operation: Some(TimerOperation::SetOneShot {
                    delay_ms: INITIAL_TIMER_DELAY_MS
                })
            })
        );
    }

    #[test]
    fn timer_start_cleans_up_after_event_failure() {
        let mut machine = TimerStartMachine::new();
        machine.advance(TimerStartEvent::ResolutionResult(true));
        assert_eq!(
            machine.advance(TimerStartEvent::InitialEventResult(false)),
            Some(TimerTransition {
                state: TimerStartState::Failed,
                operation: Some(TimerOperation::EndPeriod(1))
            })
        );
    }

    #[test]
    fn timer_start_fails_after_all_three_resolution_attempts() {
        let mut machine = TimerStartMachine::new();
        assert!(
            machine
                .advance(TimerStartEvent::ResolutionResult(false))
                .is_some()
        );
        assert!(
            machine
                .advance(TimerStartEvent::ResolutionResult(false))
                .is_some()
        );
        assert_eq!(
            machine.advance(TimerStartEvent::ResolutionResult(false)),
            Some(TimerTransition {
                state: TimerStartState::Failed,
                operation: None
            })
        );
        assert_eq!(machine.stop_operation(), None);
    }

    #[test]
    fn timer_machine_rejects_out_of_order_results() {
        let mut machine = TimerStartMachine::new();
        assert_eq!(
            machine.advance(TimerStartEvent::InitialEventResult(true)),
            None
        );
        assert_eq!(
            machine.state(),
            TimerStartState::BeginResolution { candidate: 0 }
        );
    }

    #[test]
    fn focus_only_gates_foreground_frames() {
        assert_eq!(
            focus_timing_plan(false, true),
            FocusTimingPlan {
                run_foreground_frame: false,
                async_tick_eligible: true
            }
        );
        assert_eq!(
            focus_timing_plan(true, false),
            FocusTimingPlan {
                run_foreground_frame: true,
                async_tick_eligible: false
            }
        );
    }

    #[test]
    fn unsigned_deadlines_preserve_strict_boundaries_and_wrap() {
        assert!(!elapsed_exceeds(
            1_000,
            20_999,
            TWENTY_SECOND_DEADLINE_THRESHOLD_MS
        ));
        assert!(elapsed_exceeds(
            1_000,
            21_000,
            TWENTY_SECOND_DEADLINE_THRESHOLD_MS
        ));
        assert!(!elapsed_exceeds(
            1_000,
            60_999,
            SIXTY_SECOND_DEADLINE_THRESHOLD_MS
        ));
        assert!(elapsed_exceeds(
            1_000,
            61_000,
            SIXTY_SECOND_DEADLINE_THRESHOLD_MS
        ));
        assert!(!elapsed_exceeds(10, 510, 500));
        assert!(elapsed_exceeds(10, 511, 500));
        assert!(!elapsed_exceeds(10, 140, 130));
        assert!(elapsed_exceeds(10, 141, 130));
        assert_eq!(unsigned_elapsed_ms(u32::MAX - 5, 4), 10);
    }
}
