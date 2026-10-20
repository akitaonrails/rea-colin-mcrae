//! Recovered native frame-completion operations.
//!
//! This module executes only the operations established for the original
//! steady-frame presentation routine. Fullscreen completion samples the back
//! buffer and invokes Flip on the primary chain. Windowed completion resolves
//! the client origin and synchronizes to vertical blank; the separate windowed
//! copy owner is still unrecovered and is intentionally absent from this API.
//!
//! This contract is also distinct from the software backend's headless frame
//! capture. Capturing an owned frame neither performs these native operations
//! nor makes a window visible.

use std::fmt;

use crate::window::{PresentationPlan, WindowMode, WindowRuntimeState, WindowedSynchronization};

/// Default word returned by the native sample helper when it cannot read a
/// supported pixel at its configured runtime coordinates.
pub const NATIVE_BACK_BUFFER_SAMPLE_DEFAULT: u32 = 0x0403_0201;

/// Result reported by the native vertical-blank wait itself.
///
/// `NativeFailure` is an observed result from an otherwise successful adapter
/// call. It is deliberately separate from [`NativePresentationOps::Error`],
/// which means the adapter could not perform or report the operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeVerticalBlankWait {
    Succeeded,
    NativeFailure,
}

/// Native operations used by the recovered frame-completion plan.
///
/// There is intentionally no windowed back-to-front copy operation here. The
/// retained evidence assigns that copy to another, still-unrecovered owner.
pub trait NativePresentationOps<S> {
    type Error;

    /// Returns the helper's sampled word using adapter-owned runtime sample
    /// coordinates, including [`NATIVE_BACK_BUFFER_SAMPLE_DEFAULT`] when the
    /// native surface operation itself fails. `Err` is reserved for an
    /// adapter/tool failure that prevents the operation from being performed
    /// or reported.
    fn sample_back_buffer_pixel(&mut self, back: S) -> Result<u32, Self::Error>;

    /// Invokes the native flip with the supplied numeric flags.
    ///
    /// The shipped routine ignores the Flip HRESULT. An adapter should
    /// therefore return `Ok(())` after a completed native invocation even when
    /// that HRESULT reports failure; `Err` is the reconstruction's typed
    /// fail-fast boundary for an adapter/tool failure.
    fn flip_primary(&mut self, numeric_flags: u32) -> Result<(), Self::Error>;
    fn resolve_client_origin(&mut self) -> Result<(), Self::Error>;
    fn wait_for_vertical_blank(
        &mut self,
        numeric_flags: u32,
    ) -> Result<NativeVerticalBlankWait, Self::Error>;
    fn poll_vga_blank_edge(&mut self, port: u16) -> Result<(), Self::Error>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativePresentationOutcome {
    FullscreenFlipInvoked,
    WindowedVerticalBlankWaitSucceeded,
    WindowedWaitFailureLatched,
    WindowedPolledVgaBlankEdge,
}

#[derive(Debug, Eq, PartialEq)]
pub enum NativePresentationError<E> {
    SampleBackBufferPixel(E),
    FlipPrimary(E),
    ResolveClientOrigin(E),
    WaitForVerticalBlank(E),
    PollVgaBlankEdge(E),
}

impl<E: fmt::Display> fmt::Display for NativePresentationError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SampleBackBufferPixel(source) => {
                write!(formatter, "cannot sample native back buffer: {source}")
            }
            Self::FlipPrimary(source) => write!(formatter, "cannot flip native primary: {source}"),
            Self::ResolveClientOrigin(source) => {
                write!(formatter, "cannot resolve native client origin: {source}")
            }
            Self::WaitForVerticalBlank(source) => {
                write!(
                    formatter,
                    "cannot invoke native vertical-blank wait: {source}"
                )
            }
            Self::PollVgaBlankEdge(source) => {
                write!(formatter, "cannot poll native VGA blank edge: {source}")
            }
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for NativePresentationError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match self {
            Self::SampleBackBufferPixel(source)
            | Self::FlipPrimary(source)
            | Self::ResolveClientOrigin(source)
            | Self::WaitForVerticalBlank(source)
            | Self::PollVgaBlankEdge(source) => source,
        })
    }
}

/// Executes one recovered native frame-completion plan.
///
/// A native failure reported by the first windowed vertical-blank wait latches
/// the VGA fallback but does not poll during that same frame. The next call
/// derives a new plan from `runtime` and polls instead. Adapter errors are
/// returned as typed failures and do not alter the fallback latch. In the
/// fullscreen branch, the helper result is stored before Flip; the helper's
/// recovered native-failure default is still a successful sample result and
/// therefore still continues to Flip.
pub fn execute_native_presentation_plan<S, O>(
    runtime: &mut WindowRuntimeState,
    mode: WindowMode,
    back: S,
    sampled_back_buffer_pixel: &mut u32,
    operations: &mut O,
) -> Result<NativePresentationOutcome, NativePresentationError<O::Error>>
where
    O: NativePresentationOps<S>,
{
    match runtime.presentation_plan(mode) {
        PresentationPlan::Fullscreen {
            sample_back_buffer_pixel,
            flip_numeric_flags,
        } => {
            if sample_back_buffer_pixel {
                *sampled_back_buffer_pixel = operations
                    .sample_back_buffer_pixel(back)
                    .map_err(NativePresentationError::SampleBackBufferPixel)?;
            }
            operations
                .flip_primary(flip_numeric_flags)
                .map_err(NativePresentationError::FlipPrimary)?;
            Ok(NativePresentationOutcome::FullscreenFlipInvoked)
        }
        PresentationPlan::Windowed {
            resolve_client_origin,
            synchronization,
        } => {
            if resolve_client_origin {
                operations
                    .resolve_client_origin()
                    .map_err(NativePresentationError::ResolveClientOrigin)?;
            }
            match synchronization {
                WindowedSynchronization::WaitForVerticalBlank { numeric_flags } => match operations
                    .wait_for_vertical_blank(numeric_flags)
                    .map_err(NativePresentationError::WaitForVerticalBlank)?
                {
                    NativeVerticalBlankWait::Succeeded => {
                        Ok(NativePresentationOutcome::WindowedVerticalBlankWaitSucceeded)
                    }
                    NativeVerticalBlankWait::NativeFailure => {
                        runtime.record_vertical_blank_wait(false);
                        Ok(NativePresentationOutcome::WindowedWaitFailureLatched)
                    }
                },
                WindowedSynchronization::PollVgaBlankEdge { port } => {
                    operations
                        .poll_vga_blank_edge(port)
                        .map_err(NativePresentationError::PollVgaBlankEdge)?;
                    Ok(NativePresentationOutcome::WindowedPolledVgaBlankEdge)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Operation {
        SampleBackBufferPixel(u8),
        FlipPrimary(u32),
        ResolveClientOrigin,
        WaitForVerticalBlank(u32),
        PollVgaBlankEdge(u16),
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum OperationKind {
        SampleBackBufferPixel,
        FlipPrimary,
        ResolveClientOrigin,
        WaitForVerticalBlank,
        PollVgaBlankEdge,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct AdapterError(OperationKind);

    impl fmt::Display for AdapterError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "adapter failure at {:?}", self.0)
        }
    }

    impl std::error::Error for AdapterError {}

    struct RecordingOperations {
        calls: Vec<Operation>,
        fail_at: Option<OperationKind>,
        sample: u32,
        wait: NativeVerticalBlankWait,
    }

    impl Default for RecordingOperations {
        fn default() -> Self {
            Self {
                calls: Vec::new(),
                fail_at: None,
                sample: 0xa1b2_c3d4,
                wait: NativeVerticalBlankWait::Succeeded,
            }
        }
    }

    impl RecordingOperations {
        fn record(&mut self, operation: Operation) -> Result<(), AdapterError> {
            self.calls.push(operation);
            let kind = match operation {
                Operation::SampleBackBufferPixel(_) => OperationKind::SampleBackBufferPixel,
                Operation::FlipPrimary(_) => OperationKind::FlipPrimary,
                Operation::ResolveClientOrigin => OperationKind::ResolveClientOrigin,
                Operation::WaitForVerticalBlank(_) => OperationKind::WaitForVerticalBlank,
                Operation::PollVgaBlankEdge(_) => OperationKind::PollVgaBlankEdge,
            };
            if self.fail_at == Some(kind) {
                Err(AdapterError(kind))
            } else {
                Ok(())
            }
        }
    }

    impl NativePresentationOps<u8> for RecordingOperations {
        type Error = AdapterError;

        fn sample_back_buffer_pixel(&mut self, back: u8) -> Result<u32, Self::Error> {
            self.record(Operation::SampleBackBufferPixel(back))?;
            Ok(self.sample)
        }

        fn flip_primary(&mut self, numeric_flags: u32) -> Result<(), Self::Error> {
            self.record(Operation::FlipPrimary(numeric_flags))
        }

        fn resolve_client_origin(&mut self) -> Result<(), Self::Error> {
            self.record(Operation::ResolveClientOrigin)
        }

        fn wait_for_vertical_blank(
            &mut self,
            numeric_flags: u32,
        ) -> Result<NativeVerticalBlankWait, Self::Error> {
            self.record(Operation::WaitForVerticalBlank(numeric_flags))?;
            Ok(self.wait)
        }

        fn poll_vga_blank_edge(&mut self, port: u16) -> Result<(), Self::Error> {
            self.record(Operation::PollVgaBlankEdge(port))
        }
    }

    fn state() -> WindowRuntimeState {
        WindowRuntimeState {
            active: true,
            activation_dispatch_enabled: true,
            force_wait_once: false,
            exit_requested: false,
            activation_latched: true,
            rebuild_pending: false,
            deactivation_guard: true,
            clear_black_on_paint: false,
            vertical_blank_fallback: false,
        }
    }

    #[test]
    fn fullscreen_samples_back_then_flips_without_windowed_operations() {
        let mut runtime = state();
        let mut operations = RecordingOperations::default();
        let mut sampled = 0x1122_3344;

        let outcome = execute_native_presentation_plan(
            &mut runtime,
            WindowMode::Fullscreen,
            7,
            &mut sampled,
            &mut operations,
        )
        .unwrap();

        assert_eq!(outcome, NativePresentationOutcome::FullscreenFlipInvoked);
        assert_eq!(
            operations.calls,
            [
                Operation::SampleBackBufferPixel(7),
                Operation::FlipPrimary(1),
            ]
        );
        assert_eq!(sampled, 0xa1b2_c3d4);
        assert!(!runtime.vertical_blank_fallback);
    }

    #[test]
    fn native_sample_default_is_stored_and_still_continues_to_flip() {
        let mut runtime = state();
        let mut operations = RecordingOperations {
            sample: NATIVE_BACK_BUFFER_SAMPLE_DEFAULT,
            ..RecordingOperations::default()
        };
        let mut sampled = 0x1122_3344;

        assert_eq!(
            execute_native_presentation_plan(
                &mut runtime,
                WindowMode::Fullscreen,
                7,
                &mut sampled,
                &mut operations,
            ),
            Ok(NativePresentationOutcome::FullscreenFlipInvoked)
        );
        assert_eq!(sampled, NATIVE_BACK_BUFFER_SAMPLE_DEFAULT);
        assert_eq!(
            operations.calls,
            [
                Operation::SampleBackBufferPixel(7),
                Operation::FlipPrimary(1),
            ]
        );
    }

    #[test]
    fn sample_adapter_error_preserves_the_previous_sample_and_suppresses_flip() {
        let mut runtime = state();
        let mut operations = RecordingOperations {
            fail_at: Some(OperationKind::SampleBackBufferPixel),
            ..RecordingOperations::default()
        };
        let mut sampled = 0x1122_3344;

        assert_eq!(
            execute_native_presentation_plan(
                &mut runtime,
                WindowMode::Fullscreen,
                7,
                &mut sampled,
                &mut operations,
            ),
            Err(NativePresentationError::SampleBackBufferPixel(
                AdapterError(OperationKind::SampleBackBufferPixel)
            ))
        );
        assert_eq!(sampled, 0x1122_3344);
        assert_eq!(operations.calls, [Operation::SampleBackBufferPixel(7)]);
    }

    #[test]
    fn windowed_success_resolves_origin_then_waits_without_fullscreen_operations() {
        let mut runtime = state();
        let mut operations = RecordingOperations::default();
        let mut sampled = 0x1122_3344;

        let outcome = execute_native_presentation_plan(
            &mut runtime,
            WindowMode::Windowed,
            7,
            &mut sampled,
            &mut operations,
        )
        .unwrap();

        assert_eq!(
            outcome,
            NativePresentationOutcome::WindowedVerticalBlankWaitSucceeded
        );
        assert_eq!(
            operations.calls,
            [
                Operation::ResolveClientOrigin,
                Operation::WaitForVerticalBlank(1),
            ]
        );
        assert_eq!(sampled, 0x1122_3344);
        assert!(!runtime.vertical_blank_fallback);
    }

    #[test]
    fn native_wait_failure_latches_without_same_frame_poll_then_next_frame_polls() {
        let mut runtime = state();
        let mut operations = RecordingOperations {
            wait: NativeVerticalBlankWait::NativeFailure,
            ..RecordingOperations::default()
        };
        let mut sampled = 0x1122_3344;

        let first = execute_native_presentation_plan(
            &mut runtime,
            WindowMode::Windowed,
            7,
            &mut sampled,
            &mut operations,
        )
        .unwrap();

        assert_eq!(first, NativePresentationOutcome::WindowedWaitFailureLatched);
        assert_eq!(
            operations.calls,
            [
                Operation::ResolveClientOrigin,
                Operation::WaitForVerticalBlank(1),
            ],
            "the failing wait does not introduce an unobserved same-frame poll"
        );
        assert!(runtime.vertical_blank_fallback);

        operations.calls.clear();
        operations.wait = NativeVerticalBlankWait::Succeeded;
        let second = execute_native_presentation_plan(
            &mut runtime,
            WindowMode::Windowed,
            7,
            &mut sampled,
            &mut operations,
        )
        .unwrap();

        assert_eq!(
            second,
            NativePresentationOutcome::WindowedPolledVgaBlankEdge
        );
        assert_eq!(
            operations.calls,
            [
                Operation::ResolveClientOrigin,
                Operation::PollVgaBlankEdge(0x03da),
            ]
        );
        assert!(
            runtime.vertical_blank_fallback,
            "fallback latch remains set for subsequent plans"
        );
    }

    #[test]
    fn adapter_wait_error_is_typed_and_does_not_latch_native_failure() {
        let mut runtime = state();
        let mut operations = RecordingOperations {
            fail_at: Some(OperationKind::WaitForVerticalBlank),
            ..RecordingOperations::default()
        };
        let mut sampled = 0x1122_3344;

        assert_eq!(
            execute_native_presentation_plan(
                &mut runtime,
                WindowMode::Windowed,
                7,
                &mut sampled,
                &mut operations,
            ),
            Err(NativePresentationError::WaitForVerticalBlank(AdapterError(
                OperationKind::WaitForVerticalBlank
            )))
        );
        assert_eq!(
            operations.calls,
            [
                Operation::ResolveClientOrigin,
                Operation::WaitForVerticalBlank(1),
            ]
        );
        assert!(!runtime.vertical_blank_fallback);
    }

    #[test]
    fn flip_adapter_error_retains_the_newly_stored_sample() {
        let mut runtime = state();
        let mut operations = RecordingOperations {
            fail_at: Some(OperationKind::FlipPrimary),
            sample: 0x5566_7788,
            ..RecordingOperations::default()
        };
        let mut sampled = 0x1122_3344;

        assert_eq!(
            execute_native_presentation_plan(
                &mut runtime,
                WindowMode::Fullscreen,
                7,
                &mut sampled,
                &mut operations,
            ),
            Err(NativePresentationError::FlipPrimary(AdapterError(
                OperationKind::FlipPrimary
            )))
        );
        assert_eq!(sampled, 0x5566_7788);
        assert_eq!(
            operations.calls,
            [
                Operation::SampleBackBufferPixel(7),
                Operation::FlipPrimary(1),
            ]
        );
    }

    #[test]
    fn every_adapter_error_stops_at_its_exact_operation() {
        let cases = [
            (
                WindowMode::Fullscreen,
                false,
                OperationKind::SampleBackBufferPixel,
                NativePresentationError::SampleBackBufferPixel(AdapterError(
                    OperationKind::SampleBackBufferPixel,
                )),
                vec![Operation::SampleBackBufferPixel(7)],
            ),
            (
                WindowMode::Fullscreen,
                false,
                OperationKind::FlipPrimary,
                NativePresentationError::FlipPrimary(AdapterError(OperationKind::FlipPrimary)),
                vec![
                    Operation::SampleBackBufferPixel(7),
                    Operation::FlipPrimary(1),
                ],
            ),
            (
                WindowMode::Windowed,
                false,
                OperationKind::ResolveClientOrigin,
                NativePresentationError::ResolveClientOrigin(AdapterError(
                    OperationKind::ResolveClientOrigin,
                )),
                vec![Operation::ResolveClientOrigin],
            ),
            (
                WindowMode::Windowed,
                false,
                OperationKind::WaitForVerticalBlank,
                NativePresentationError::WaitForVerticalBlank(AdapterError(
                    OperationKind::WaitForVerticalBlank,
                )),
                vec![
                    Operation::ResolveClientOrigin,
                    Operation::WaitForVerticalBlank(1),
                ],
            ),
            (
                WindowMode::Windowed,
                true,
                OperationKind::PollVgaBlankEdge,
                NativePresentationError::PollVgaBlankEdge(AdapterError(
                    OperationKind::PollVgaBlankEdge,
                )),
                vec![
                    Operation::ResolveClientOrigin,
                    Operation::PollVgaBlankEdge(0x03da),
                ],
            ),
        ];

        for (mode, fallback, failed, expected_error, expected_calls) in cases {
            let mut runtime = state();
            runtime.vertical_blank_fallback = fallback;
            let mut operations = RecordingOperations {
                fail_at: Some(failed),
                ..RecordingOperations::default()
            };
            let mut sampled = 0x1122_3344;

            assert_eq!(
                execute_native_presentation_plan(
                    &mut runtime,
                    mode,
                    7,
                    &mut sampled,
                    &mut operations,
                ),
                Err(expected_error)
            );
            assert_eq!(operations.calls, expected_calls);
            assert_eq!(runtime.vertical_blank_fallback, fallback);
        }
    }
}
