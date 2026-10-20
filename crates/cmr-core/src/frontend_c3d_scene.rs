//! Domain-neutral frontend C3D scene transaction.
//!
//! The recovered root-menu path and other frontend C3D consumers share this
//! ordering contract. Object traversal remains caller-owned: this module only
//! brackets that submission with the common scene state and cleanup behavior.

use std::fmt;

pub use crate::root_menu_c3d_transform::{
    C3dMatrixWords, C3dViewportWords, RootMenuC3dViewportError as C3dViewportError,
};
use crate::{
    display::DisplayExtent,
    root_menu_c3d_transform::{ROOT_MENU_C3D_VIEW, root_menu_c3d_viewport},
};

/// Exact recovered identity camera transform shared by frontend C3D scenes.
pub const C3D_IDENTITY_VIEW: C3dMatrixWords = ROOT_MENU_C3D_VIEW;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum C3dDepthClear {
    Clear,
    Skip,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum C3dBindingState {
    Preserved,
    Changed,
}

/// Backend-owned effects required by the common frontend scene transaction.
pub trait FrontendC3dSceneBackend {
    type Surface: Copy;
    type SceneError;

    /// Rejects destination, extent, or depth-policy state before the common
    /// scene transaction performs its first backend effect.
    ///
    /// This hook may retain diagnostics, but must be idempotent and must not
    /// produce externally observable render, binding, or selector-cache effects.
    fn preflight_c3d_scene(
        &mut self,
        _destination: Self::Surface,
        _extent: DisplayExtent,
        _depth_clear: C3dDepthClear,
    ) -> Result<(), Self::SceneError> {
        Ok(())
    }

    fn clear_c3d_depth(
        &mut self,
        destination: Self::Surface,
        extent: DisplayExtent,
    ) -> Result<(), Self::SceneError>;

    fn set_c3d_view(&mut self, view: C3dMatrixWords) -> Result<(), Self::SceneError>;

    fn set_c3d_viewport(&mut self, viewport: C3dViewportWords) -> Result<(), Self::SceneError>;

    fn begin_c3d_scene(&mut self, destination: Self::Surface) -> Result<(), Self::SceneError>;

    /// Restores the identity world transform after the complete traversal.
    fn reset_c3d_world(&mut self) -> Result<(), Self::SceneError>;

    /// Performs the still-opaque common post-traversal scene tail.
    fn finish_c3d_scene_tail(&mut self) -> Result<C3dBindingState, Self::SceneError>;

    fn end_c3d_scene(&mut self) -> Result<(), Self::SceneError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum C3dSceneStage {
    Preflight,
    ClearDepth,
    View,
    Viewport,
    Begin,
    ResetWorld,
    Tail,
    End,
}

#[derive(Debug, Eq, PartialEq)]
pub enum C3dScenePrimaryError<S, D> {
    Viewport(C3dViewportError),
    Backend { stage: C3dSceneStage, source: S },
    Submit(D),
}

impl<S: fmt::Display, D: fmt::Display> fmt::Display for C3dScenePrimaryError<S, D> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Viewport(source) => source.fmt(formatter),
            Self::Backend { stage, source } => {
                write!(
                    formatter,
                    "frontend C3D scene {stage:?} stage failed: {source}"
                )
            }
            Self::Submit(source) => source.fmt(formatter),
        }
    }
}

impl<S, D> std::error::Error for C3dScenePrimaryError<S, D>
where
    S: std::error::Error + 'static,
    D: std::error::Error + 'static,
{
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Viewport(source) => Some(source),
            Self::Backend { source, .. } => Some(source),
            Self::Submit(source) => Some(source),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct C3dSceneError<S, D> {
    pub primary: C3dScenePrimaryError<S, D>,
    /// Best-effort `EndScene` failure after a post-begin primary failure.
    pub cleanup: Option<S>,
}

impl<S: fmt::Display, D: fmt::Display> fmt::Display for C3dSceneError<S, D> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.primary.fmt(formatter)?;
        if let Some(cleanup) = &self.cleanup {
            write!(formatter, "; End cleanup also failed: {cleanup}")?;
        }
        Ok(())
    }
}

impl<S, D> std::error::Error for C3dSceneError<S, D>
where
    S: std::error::Error + 'static,
    D: std::error::Error + 'static,
{
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.primary)
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct C3dSceneOutcome<O> {
    pub depth_cleared: bool,
    pub submission: Option<O>,
    pub binding_state: C3dBindingState,
}

/// Executes one complete frontend C3D scene transaction.
///
/// Viewport validation is effect-free. Once `Begin` succeeds, failures in the
/// submission or tail make one best-effort `End` call. The optional plan is
/// consumed on every path and therefore cannot leak into a later frame.
pub fn execute_frontend_c3d_scene<B, P, O, D>(
    backend: &mut B,
    destination: B::Surface,
    extent: DisplayExtent,
    depth_clear: C3dDepthClear,
    plan: Option<P>,
    submit: impl FnOnce(&mut B, P) -> Result<O, D>,
) -> Result<C3dSceneOutcome<O>, C3dSceneError<B::SceneError, D>>
where
    B: FrontendC3dSceneBackend,
{
    let viewport = root_menu_c3d_viewport(extent).map_err(|source| C3dSceneError {
        primary: C3dScenePrimaryError::Viewport(source),
        cleanup: None,
    })?;
    backend
        .preflight_c3d_scene(destination, extent, depth_clear)
        .map_err(|source| scene_backend_error(C3dSceneStage::Preflight, source))?;
    if depth_clear == C3dDepthClear::Clear {
        backend
            .clear_c3d_depth(destination, extent)
            .map_err(|source| scene_backend_error(C3dSceneStage::ClearDepth, source))?;
    }
    backend
        .set_c3d_view(C3D_IDENTITY_VIEW)
        .map_err(|source| scene_backend_error(C3dSceneStage::View, source))?;
    backend
        .set_c3d_viewport(viewport)
        .map_err(|source| scene_backend_error(C3dSceneStage::Viewport, source))?;
    backend
        .begin_c3d_scene(destination)
        .map_err(|source| scene_backend_error(C3dSceneStage::Begin, source))?;

    let submission = match plan {
        Some(plan) => match submit(backend, plan) {
            Ok(outcome) => Some(outcome),
            Err(source) => {
                return Err(post_begin_error(
                    backend,
                    C3dScenePrimaryError::Submit(source),
                ));
            }
        },
        None => None,
    };
    if let Err(source) = backend.reset_c3d_world() {
        return Err(post_begin_error(
            backend,
            C3dScenePrimaryError::Backend {
                stage: C3dSceneStage::ResetWorld,
                source,
            },
        ));
    }
    let binding_state = match backend.finish_c3d_scene_tail() {
        Ok(state) => state,
        Err(source) => {
            return Err(post_begin_error(
                backend,
                C3dScenePrimaryError::Backend {
                    stage: C3dSceneStage::Tail,
                    source,
                },
            ));
        }
    };
    backend
        .end_c3d_scene()
        .map_err(|source| scene_backend_error(C3dSceneStage::End, source))?;
    Ok(C3dSceneOutcome {
        depth_cleared: depth_clear == C3dDepthClear::Clear,
        submission,
        binding_state,
    })
}

fn scene_backend_error<S, D>(stage: C3dSceneStage, source: S) -> C3dSceneError<S, D> {
    C3dSceneError {
        primary: C3dScenePrimaryError::Backend { stage, source },
        cleanup: None,
    }
}

fn post_begin_error<B, D>(
    backend: &mut B,
    primary: C3dScenePrimaryError<B::SceneError, D>,
) -> C3dSceneError<B::SceneError, D>
where
    B: FrontendC3dSceneBackend,
{
    C3dSceneError {
        primary,
        cleanup: backend.end_c3d_scene().err(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct Failure(C3dSceneStage);

    impl fmt::Display for Failure {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "{:?}", self.0)
        }
    }

    impl std::error::Error for Failure {}

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct SubmitFailure;

    impl fmt::Display for SubmitFailure {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("submit")
        }
    }

    impl std::error::Error for SubmitFailure {}

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Event {
        Preflight,
        Clear,
        View,
        Viewport,
        Begin,
        Submit(u8),
        ResetWorld,
        Tail,
        End,
    }

    struct Backend {
        events: Vec<Event>,
        failures: Vec<C3dSceneStage>,
        tail_state: C3dBindingState,
        view: Option<C3dMatrixWords>,
        viewport: Option<C3dViewportWords>,
    }

    impl Backend {
        fn new() -> Self {
            Self {
                events: Vec::new(),
                failures: Vec::new(),
                tail_state: C3dBindingState::Preserved,
                view: None,
                viewport: None,
            }
        }

        fn effect(&mut self, stage: C3dSceneStage, event: Event) -> Result<(), Failure> {
            self.events.push(event);
            if self.failures.contains(&stage) {
                Err(Failure(stage))
            } else {
                Ok(())
            }
        }
    }

    impl FrontendC3dSceneBackend for Backend {
        type Surface = u8;
        type SceneError = Failure;

        fn preflight_c3d_scene(
            &mut self,
            _: Self::Surface,
            _: DisplayExtent,
            _: C3dDepthClear,
        ) -> Result<(), Self::SceneError> {
            self.effect(C3dSceneStage::Preflight, Event::Preflight)
        }

        fn clear_c3d_depth(
            &mut self,
            _: Self::Surface,
            _: DisplayExtent,
        ) -> Result<(), Self::SceneError> {
            self.effect(C3dSceneStage::ClearDepth, Event::Clear)
        }

        fn set_c3d_view(&mut self, view: C3dMatrixWords) -> Result<(), Self::SceneError> {
            self.view = Some(view);
            self.effect(C3dSceneStage::View, Event::View)
        }

        fn set_c3d_viewport(&mut self, viewport: C3dViewportWords) -> Result<(), Self::SceneError> {
            self.viewport = Some(viewport);
            self.effect(C3dSceneStage::Viewport, Event::Viewport)
        }

        fn begin_c3d_scene(&mut self, _: Self::Surface) -> Result<(), Self::SceneError> {
            self.effect(C3dSceneStage::Begin, Event::Begin)
        }

        fn reset_c3d_world(&mut self) -> Result<(), Self::SceneError> {
            self.effect(C3dSceneStage::ResetWorld, Event::ResetWorld)
        }

        fn finish_c3d_scene_tail(&mut self) -> Result<C3dBindingState, Self::SceneError> {
            self.effect(C3dSceneStage::Tail, Event::Tail)?;
            Ok(self.tail_state)
        }

        fn end_c3d_scene(&mut self) -> Result<(), Self::SceneError> {
            self.effect(C3dSceneStage::End, Event::End)
        }
    }

    const EXTENT: DisplayExtent = DisplayExtent {
        width: 640,
        height: 480,
    };

    #[test]
    fn common_transaction_orders_every_effect_and_returns_submission() {
        let mut backend = Backend::new();
        backend.tail_state = C3dBindingState::Changed;

        let outcome = execute_frontend_c3d_scene(
            &mut backend,
            7,
            EXTENT,
            C3dDepthClear::Clear,
            Some(42_u8),
            |backend, plan| {
                backend.events.push(Event::Submit(plan));
                Ok::<_, SubmitFailure>(plan + 1)
            },
        )
        .unwrap();

        assert_eq!(outcome.submission, Some(43));
        assert!(outcome.depth_cleared);
        assert_eq!(outcome.binding_state, C3dBindingState::Changed);
        assert_eq!(backend.view, Some(C3D_IDENTITY_VIEW));
        assert_eq!(
            backend.viewport,
            Some(root_menu_c3d_viewport(EXTENT).unwrap())
        );
        assert_eq!(
            backend.events,
            [
                Event::Preflight,
                Event::Clear,
                Event::View,
                Event::Viewport,
                Event::Begin,
                Event::Submit(42),
                Event::ResetWorld,
                Event::Tail,
                Event::End,
            ]
        );
    }

    #[test]
    fn skip_policy_omits_only_depth_clear_and_absent_plan_still_brackets_scene() {
        let mut backend = Backend::new();
        let outcome = execute_frontend_c3d_scene(
            &mut backend,
            7,
            EXTENT,
            C3dDepthClear::Skip,
            None::<u8>,
            |_, _| Ok::<_, SubmitFailure>(()),
        )
        .unwrap();

        assert!(!outcome.depth_cleared);
        assert_eq!(outcome.submission, None);
        assert_eq!(
            backend.events,
            [
                Event::Preflight,
                Event::View,
                Event::Viewport,
                Event::Begin,
                Event::ResetWorld,
                Event::Tail,
                Event::End,
            ]
        );
    }

    #[test]
    fn invalid_viewport_is_effect_free() {
        let mut backend = Backend::new();
        let error = execute_frontend_c3d_scene(
            &mut backend,
            7,
            DisplayExtent {
                width: 0,
                height: 480,
            },
            C3dDepthClear::Clear,
            None::<u8>,
            |_, _| Ok::<_, SubmitFailure>(()),
        )
        .unwrap_err();

        assert!(matches!(error.primary, C3dScenePrimaryError::Viewport(_)));
        assert!(error.cleanup.is_none());
        assert!(backend.events.is_empty());
    }

    #[test]
    fn preflight_rejection_precedes_skip_empty_scene_effects() {
        let mut backend = Backend::new();
        backend.failures.push(C3dSceneStage::Preflight);

        let error = execute_frontend_c3d_scene(
            &mut backend,
            7,
            EXTENT,
            C3dDepthClear::Skip,
            None::<u8>,
            |_, _| Ok::<_, SubmitFailure>(()),
        )
        .unwrap_err();

        assert_eq!(
            error.primary,
            C3dScenePrimaryError::Backend {
                stage: C3dSceneStage::Preflight,
                source: Failure(C3dSceneStage::Preflight),
            }
        );
        assert!(error.cleanup.is_none());
        assert_eq!(backend.events, [Event::Preflight]);
        assert!(backend.view.is_none());
        assert!(backend.viewport.is_none());
    }

    #[test]
    fn injected_failures_preserve_begin_boundary_and_cleanup_contract() {
        for stage in [
            C3dSceneStage::Preflight,
            C3dSceneStage::ClearDepth,
            C3dSceneStage::View,
            C3dSceneStage::Viewport,
            C3dSceneStage::Begin,
            C3dSceneStage::ResetWorld,
            C3dSceneStage::Tail,
            C3dSceneStage::End,
        ] {
            let mut backend = Backend::new();
            backend.failures.push(stage);
            let error = execute_frontend_c3d_scene(
                &mut backend,
                7,
                EXTENT,
                C3dDepthClear::Clear,
                Some(1_u8),
                |backend, plan| {
                    backend.events.push(Event::Submit(plan));
                    Ok::<_, SubmitFailure>(())
                },
            )
            .unwrap_err();

            assert!(matches!(
                error.primary,
                C3dScenePrimaryError::Backend { stage: actual, .. } if actual == stage
            ));
            let end_count = backend
                .events
                .iter()
                .filter(|event| **event == Event::End)
                .count();
            let expected_end_count = usize::from(matches!(
                stage,
                C3dSceneStage::ResetWorld | C3dSceneStage::Tail | C3dSceneStage::End
            ));
            assert_eq!(end_count, expected_end_count, "failed stage: {stage:?}");
            assert!(error.cleanup.is_none());
        }

        let mut backend = Backend::new();
        backend.failures.push(C3dSceneStage::End);
        let error = execute_frontend_c3d_scene(
            &mut backend,
            7,
            EXTENT,
            C3dDepthClear::Skip,
            Some(1_u8),
            |backend, plan| {
                backend.events.push(Event::Submit(plan));
                Err::<(), _>(SubmitFailure)
            },
        )
        .unwrap_err();
        assert_eq!(error.primary, C3dScenePrimaryError::Submit(SubmitFailure));
        assert_eq!(error.cleanup, Some(Failure(C3dSceneStage::End)));
        assert_eq!(backend.events.last(), Some(&Event::End));
    }
}
