//! Terminal root-menu C3D scene transaction and prepared-frame pipeline.
//!
//! Root callbacks only stage one optional draw plan. The scene transaction is
//! executed later at the terminal compose command, including on frames without
//! a selected root object. Projection remains an initialization-time contract.

use std::fmt;

use crate::{
    display::{DisplayExtent, IndexedVideoBackend},
    root_menu_assets::VisualVariant,
    root_menu_c3d_assets::{RootMenuC3dAngles, RootMenuC3dSlot},
    root_menu_c3d_execute::{
        RootMenuC3dBatchBackend, RootMenuC3dBatchExecutor, RootMenuC3dExecuteError,
    },
    root_menu_c3d_frame_plan::{
        RootMenuC3dFramePlan, RootMenuC3dFramePlanError, plan_root_menu_c3d_frame,
    },
    root_menu_c3d_runtime::RootMenuC3dMode2ResourceScaledRuntime,
    root_menu_c3d_topology::RootMenuC3dTopology,
    root_menu_c3d_transform::{
        C3dMatrixWords, C3dViewportWords, ROOT_MENU_C3D_VIEW, RootMenuC3dViewportError,
        root_menu_c3d_viewport,
    },
    root_menu_c3d_visibility::{RootMenuC3dVisibilityLatches, RootMenuC3dVisibilityMask},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RootMenuC3dDepthClear {
    Clear,
    Skip,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RootMenuC3dBindingState {
    Preserved,
    Changed,
}

/// Scene operations whose placement is recovered but whose API is backend-owned.
pub trait RootMenuC3dSceneBackend: IndexedVideoBackend {
    type SceneError;

    fn clear_root_c3d_depth(
        &mut self,
        destination: Self::Surface,
        extent: DisplayExtent,
    ) -> Result<(), Self::SceneError>;

    fn set_root_c3d_view(&mut self, view: C3dMatrixWords) -> Result<(), Self::SceneError>;

    fn set_root_c3d_viewport(&mut self, viewport: C3dViewportWords)
    -> Result<(), Self::SceneError>;

    fn begin_root_c3d_scene(&mut self, destination: Self::Surface) -> Result<(), Self::SceneError>;

    /// Restores the identity world transform after the complete scene traversal.
    fn reset_root_c3d_world(&mut self) -> Result<(), Self::SceneError>;

    /// Performs the still-opaque post-root scene tail.
    fn finish_root_c3d_scene_tail(&mut self) -> Result<RootMenuC3dBindingState, Self::SceneError>;

    fn end_root_c3d_scene(&mut self) -> Result<(), Self::SceneError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RootMenuC3dSceneStage {
    ClearDepth,
    View,
    Viewport,
    Begin,
    ResetWorld,
    Tail,
    End,
}

#[derive(Debug, Eq, PartialEq)]
pub enum RootMenuC3dScenePrimaryError<S, D> {
    Viewport(RootMenuC3dViewportError),
    Backend {
        stage: RootMenuC3dSceneStage,
        source: S,
    },
    Submit(D),
}

impl<S: fmt::Display, D: fmt::Display> fmt::Display for RootMenuC3dScenePrimaryError<S, D> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Viewport(source) => source.fmt(formatter),
            Self::Backend { stage, source } => {
                write!(
                    formatter,
                    "root-menu C3D scene {stage:?} stage failed: {source}"
                )
            }
            Self::Submit(source) => source.fmt(formatter),
        }
    }
}

impl<S, D> std::error::Error for RootMenuC3dScenePrimaryError<S, D>
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
pub struct RootMenuC3dSceneError<S, D> {
    pub primary: RootMenuC3dScenePrimaryError<S, D>,
    /// Best-effort `EndScene` failure after a post-begin primary failure.
    pub cleanup: Option<S>,
}

impl<S: fmt::Display, D: fmt::Display> fmt::Display for RootMenuC3dSceneError<S, D> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.primary.fmt(formatter)?;
        if let Some(cleanup) = &self.cleanup {
            write!(formatter, "; End cleanup also failed: {cleanup}")?;
        }
        Ok(())
    }
}

impl<S, D> std::error::Error for RootMenuC3dSceneError<S, D>
where
    S: std::error::Error + 'static,
    D: std::error::Error + 'static,
{
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.primary)
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct RootMenuC3dSceneOutcome<O> {
    pub depth_cleared: bool,
    pub submission: Option<O>,
    pub binding_state: RootMenuC3dBindingState,
}

/// Executes one complete terminal scene transaction.
///
/// Viewport validation is effect-free. Once `Begin` succeeds, failures in the
/// submission or tail make one best-effort `End` call. The optional plan is
/// consumed on every path and therefore cannot leak into a later frame.
pub fn execute_root_menu_c3d_scene<B, P, O, D>(
    backend: &mut B,
    destination: B::Surface,
    extent: DisplayExtent,
    depth_clear: RootMenuC3dDepthClear,
    plan: Option<P>,
    submit: impl FnOnce(&mut B, P) -> Result<O, D>,
) -> Result<RootMenuC3dSceneOutcome<O>, RootMenuC3dSceneError<B::SceneError, D>>
where
    B: RootMenuC3dSceneBackend,
{
    let viewport = root_menu_c3d_viewport(extent).map_err(|source| RootMenuC3dSceneError {
        primary: RootMenuC3dScenePrimaryError::Viewport(source),
        cleanup: None,
    })?;
    if depth_clear == RootMenuC3dDepthClear::Clear {
        backend
            .clear_root_c3d_depth(destination, extent)
            .map_err(|source| scene_backend_error(RootMenuC3dSceneStage::ClearDepth, source))?;
    }
    backend
        .set_root_c3d_view(ROOT_MENU_C3D_VIEW)
        .map_err(|source| scene_backend_error(RootMenuC3dSceneStage::View, source))?;
    backend
        .set_root_c3d_viewport(viewport)
        .map_err(|source| scene_backend_error(RootMenuC3dSceneStage::Viewport, source))?;
    backend
        .begin_root_c3d_scene(destination)
        .map_err(|source| scene_backend_error(RootMenuC3dSceneStage::Begin, source))?;

    let submission = match plan {
        Some(plan) => match submit(backend, plan) {
            Ok(outcome) => Some(outcome),
            Err(source) => {
                return Err(post_begin_error(
                    backend,
                    RootMenuC3dScenePrimaryError::Submit(source),
                ));
            }
        },
        None => None,
    };
    if let Err(source) = backend.reset_root_c3d_world() {
        return Err(post_begin_error(
            backend,
            RootMenuC3dScenePrimaryError::Backend {
                stage: RootMenuC3dSceneStage::ResetWorld,
                source,
            },
        ));
    }
    let binding_state = match backend.finish_root_c3d_scene_tail() {
        Ok(state) => state,
        Err(source) => {
            return Err(post_begin_error(
                backend,
                RootMenuC3dScenePrimaryError::Backend {
                    stage: RootMenuC3dSceneStage::Tail,
                    source,
                },
            ));
        }
    };
    backend
        .end_root_c3d_scene()
        .map_err(|source| scene_backend_error(RootMenuC3dSceneStage::End, source))?;
    Ok(RootMenuC3dSceneOutcome {
        depth_cleared: depth_clear == RootMenuC3dDepthClear::Clear,
        submission,
        binding_state,
    })
}

fn scene_backend_error<S, D>(
    stage: RootMenuC3dSceneStage,
    source: S,
) -> RootMenuC3dSceneError<S, D> {
    RootMenuC3dSceneError {
        primary: RootMenuC3dScenePrimaryError::Backend { stage, source },
        cleanup: None,
    }
}

fn post_begin_error<B, D>(
    backend: &mut B,
    primary: RootMenuC3dScenePrimaryError<B::SceneError, D>,
) -> RootMenuC3dSceneError<B::SceneError, D>
where
    B: RootMenuC3dSceneBackend,
{
    RootMenuC3dSceneError {
        primary,
        cleanup: backend.end_root_c3d_scene().err(),
    }
}

/// C3D pipeline accepted by the sequential root-frame dispatcher.
pub trait RootMenuC3dFramePipeline<B: IndexedVideoBackend> {
    type Plan;
    type PlanError;
    type SceneError;

    fn variant(&self) -> VisualVariant;

    fn plan(
        &mut self,
        slot: RootMenuC3dSlot,
        angle_delta: u16,
        extent: DisplayExtent,
    ) -> Result<Option<Self::Plan>, Self::PlanError>;

    fn execute_terminal_scene(
        &mut self,
        backend: &mut B,
        destination: B::Surface,
        extent: DisplayExtent,
        plan: Option<Self::Plan>,
    ) -> Result<(), Self::SceneError>;
}

/// Production pipeline joining prepared geometry, persistent angles/bindings,
/// and the terminal scene transaction.
pub struct PreparedRootMenuC3dPipeline<'a, 'asset, 'catalog, H> {
    topology: &'a RootMenuC3dTopology<'asset>,
    runtime: &'a RootMenuC3dMode2ResourceScaledRuntime,
    angles: &'a mut RootMenuC3dAngles,
    visibility: &'a mut RootMenuC3dVisibilityLatches,
    batches: &'a mut RootMenuC3dBatchExecutor<'catalog, H>,
    depth_clear: RootMenuC3dDepthClear,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreparedRootMenuC3dPipelineError {
    VariantMismatch,
    RuntimeProvenanceMismatch,
    BindingProvenanceMismatch,
}

impl fmt::Display for PreparedRootMenuC3dPipelineError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::VariantMismatch => {
                formatter.write_str("root-menu C3D topology and scaled runtime variants differ")
            }
            Self::RuntimeProvenanceMismatch => formatter
                .write_str("root-menu C3D topology and scaled runtime belong to different arenas"),
            Self::BindingProvenanceMismatch => formatter
                .write_str("root-menu C3D topology and binding catalog belong to different arenas"),
        }
    }
}

impl std::error::Error for PreparedRootMenuC3dPipelineError {}

impl<'a, 'asset, 'catalog, H> PreparedRootMenuC3dPipeline<'a, 'asset, 'catalog, H> {
    pub fn new(
        topology: &'a RootMenuC3dTopology<'asset>,
        runtime: &'a RootMenuC3dMode2ResourceScaledRuntime,
        angles: &'a mut RootMenuC3dAngles,
        visibility: &'a mut RootMenuC3dVisibilityLatches,
        batches: &'a mut RootMenuC3dBatchExecutor<'catalog, H>,
        depth_clear: RootMenuC3dDepthClear,
    ) -> Result<Self, PreparedRootMenuC3dPipelineError> {
        if topology.variant() != runtime.variant() {
            return Err(PreparedRootMenuC3dPipelineError::VariantMismatch);
        }
        if topology.provenance() != runtime.provenance() {
            return Err(PreparedRootMenuC3dPipelineError::RuntimeProvenanceMismatch);
        }
        if topology.provenance() != batches.provenance() {
            return Err(PreparedRootMenuC3dPipelineError::BindingProvenanceMismatch);
        }
        Ok(Self {
            topology,
            runtime,
            angles,
            visibility,
            batches,
            depth_clear,
        })
    }

    pub fn plan_frame(
        &mut self,
        slot: RootMenuC3dSlot,
        angle_delta: u16,
        extent: DisplayExtent,
    ) -> Result<Option<RootMenuC3dFramePlan>, RootMenuC3dFramePlanError> {
        let plan = plan_root_menu_c3d_frame(
            self.topology,
            self.runtime,
            self.angles,
            slot,
            angle_delta,
            extent,
        )?;
        if let Some(plan) = &plan {
            self.visibility.show_all_layers(plan.slot());
        }
        Ok(plan)
    }

    #[must_use]
    pub const fn visibility_mask(&self, slot: RootMenuC3dSlot) -> RootMenuC3dVisibilityMask {
        self.visibility.mask(slot)
    }
}

impl<B, H> RootMenuC3dFramePipeline<B> for PreparedRootMenuC3dPipeline<'_, '_, '_, H>
where
    B: RootMenuC3dSceneBackend + RootMenuC3dBatchBackend<H>,
{
    type Plan = RootMenuC3dFramePlan;
    type PlanError = RootMenuC3dFramePlanError;
    type SceneError = RootMenuC3dSceneError<
        B::SceneError,
        RootMenuC3dExecuteError<<B as RootMenuC3dBatchBackend<H>>::Error>,
    >;

    fn variant(&self) -> VisualVariant {
        self.topology.variant()
    }

    fn plan(
        &mut self,
        slot: RootMenuC3dSlot,
        angle_delta: u16,
        extent: DisplayExtent,
    ) -> Result<Option<Self::Plan>, Self::PlanError> {
        self.plan_frame(slot, angle_delta, extent)
    }

    fn execute_terminal_scene(
        &mut self,
        backend: &mut B,
        destination: B::Surface,
        extent: DisplayExtent,
        plan: Option<Self::Plan>,
    ) -> Result<(), Self::SceneError> {
        let result = execute_root_menu_c3d_scene(
            backend,
            destination,
            extent,
            self.depth_clear,
            plan,
            |backend, plan| self.batches.execute(backend, plan),
        );
        // The native path clears after the scene returns. Clearing after typed
        // reconstruction failures as well prevents stale one-frame latches.
        self.visibility.clear_all();
        match &result {
            Ok(outcome) if outcome.binding_state == RootMenuC3dBindingState::Changed => {
                self.batches.invalidate_bindings();
            }
            Err(error)
                if matches!(
                    error.primary,
                    RootMenuC3dScenePrimaryError::Submit(_)
                        | RootMenuC3dScenePrimaryError::Backend {
                            stage: RootMenuC3dSceneStage::Tail | RootMenuC3dSceneStage::End,
                            ..
                        }
                ) =>
            {
                self.batches.invalidate_bindings();
            }
            _ => {}
        }
        result.map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use cmr_formats::riff_palette::PaletteEntry;

    use super::*;
    use crate::{
        blit::BlitPlan,
        display::{CooperativeMode, DisplaySurfaces, IndexedMode, SurfaceTopology},
        surface::IndexedSurface,
    };

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct SceneFailure(RootMenuC3dSceneStage);

    impl fmt::Display for SceneFailure {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "{:?}", self.0)
        }
    }

    impl std::error::Error for SceneFailure {}

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
        failures: Vec<RootMenuC3dSceneStage>,
        tail_state: RootMenuC3dBindingState,
    }

    impl Backend {
        fn new() -> Self {
            Self {
                events: Vec::new(),
                failures: Vec::new(),
                tail_state: RootMenuC3dBindingState::Preserved,
            }
        }

        fn effect(
            &mut self,
            stage: RootMenuC3dSceneStage,
            event: Event,
        ) -> Result<(), SceneFailure> {
            self.events.push(event);
            if self.failures.contains(&stage) {
                Err(SceneFailure(stage))
            } else {
                Ok(())
            }
        }
    }

    impl IndexedVideoBackend for Backend {
        type Error = SceneFailure;
        type Surface = u8;
        type Palette = u8;

        fn create_device(&mut self) -> Result<(), Self::Error> {
            unreachable!()
        }
        fn set_cooperative_mode(&mut self, _: CooperativeMode) -> Result<(), Self::Error> {
            unreachable!()
        }
        fn set_display_mode(&mut self, _: IndexedMode) -> Result<(), Self::Error> {
            unreachable!()
        }
        fn create_surfaces(
            &mut self,
            _: SurfaceTopology,
            _: IndexedMode,
        ) -> Result<DisplaySurfaces<u8>, Self::Error> {
            unreachable!()
        }
        fn create_palette(&mut self, _: &[PaletteEntry; 256]) -> Result<u8, Self::Error> {
            unreachable!()
        }
        fn attach_palette(&mut self, _: u8, _: u8) -> Result<(), Self::Error> {
            unreachable!()
        }
        fn upload_palette(&mut self, _: u8, _: &[PaletteEntry; 256]) -> Result<(), Self::Error> {
            unreachable!()
        }
        fn create_source(&mut self, _: &IndexedSurface) -> Result<u8, Self::Error> {
            unreachable!()
        }
        fn blit(&mut self, _: u8, _: u8, _: BlitPlan) -> Result<(), Self::Error> {
            unreachable!()
        }
        fn destroy_source(&mut self, _: u8) {
            unreachable!()
        }
    }

    impl RootMenuC3dSceneBackend for Backend {
        type SceneError = SceneFailure;

        fn clear_root_c3d_depth(
            &mut self,
            _: u8,
            _: DisplayExtent,
        ) -> Result<(), Self::SceneError> {
            self.effect(RootMenuC3dSceneStage::ClearDepth, Event::Clear)
        }
        fn set_root_c3d_view(&mut self, _: C3dMatrixWords) -> Result<(), Self::SceneError> {
            self.effect(RootMenuC3dSceneStage::View, Event::View)
        }
        fn set_root_c3d_viewport(&mut self, _: C3dViewportWords) -> Result<(), Self::SceneError> {
            self.effect(RootMenuC3dSceneStage::Viewport, Event::Viewport)
        }
        fn begin_root_c3d_scene(&mut self, _: u8) -> Result<(), Self::SceneError> {
            self.effect(RootMenuC3dSceneStage::Begin, Event::Begin)
        }
        fn reset_root_c3d_world(&mut self) -> Result<(), Self::SceneError> {
            self.effect(RootMenuC3dSceneStage::ResetWorld, Event::ResetWorld)
        }
        fn finish_root_c3d_scene_tail(
            &mut self,
        ) -> Result<RootMenuC3dBindingState, Self::SceneError> {
            self.effect(RootMenuC3dSceneStage::Tail, Event::Tail)?;
            Ok(self.tail_state)
        }
        fn end_root_c3d_scene(&mut self) -> Result<(), Self::SceneError> {
            self.effect(RootMenuC3dSceneStage::End, Event::End)
        }
    }

    const EXTENT: DisplayExtent = DisplayExtent {
        width: 640,
        height: 480,
    };

    #[test]
    fn submitted_plan_runs_only_inside_complete_terminal_scene() {
        let mut backend = Backend::new();
        backend.tail_state = RootMenuC3dBindingState::Changed;
        let outcome = execute_root_menu_c3d_scene(
            &mut backend,
            7,
            EXTENT,
            RootMenuC3dDepthClear::Clear,
            Some(42_u8),
            |backend, plan| {
                backend.events.push(Event::Submit(plan));
                Ok::<_, SubmitFailure>(plan + 1)
            },
        )
        .unwrap();
        assert_eq!(outcome.submission, Some(43));
        assert!(outcome.depth_cleared);
        assert_eq!(outcome.binding_state, RootMenuC3dBindingState::Changed);
        assert_eq!(
            backend.events,
            [
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
    fn absent_plan_still_runs_scene_and_skip_policy_omits_only_clear() {
        let mut backend = Backend::new();
        let outcome = execute_root_menu_c3d_scene(
            &mut backend,
            7,
            EXTENT,
            RootMenuC3dDepthClear::Skip,
            None::<u8>,
            |_, _| Ok::<_, SubmitFailure>(()),
        )
        .unwrap();
        assert_eq!(outcome.submission, None);
        assert!(!outcome.depth_cleared);
        assert_eq!(
            backend.events,
            [
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
    fn invalid_viewport_fails_before_every_backend_effect() {
        let mut backend = Backend::new();
        let error = execute_root_menu_c3d_scene(
            &mut backend,
            7,
            DisplayExtent {
                width: 0,
                height: 480,
            },
            RootMenuC3dDepthClear::Clear,
            None::<u8>,
            |_, _| Ok::<_, SubmitFailure>(()),
        )
        .unwrap_err();
        assert!(matches!(
            error.primary,
            RootMenuC3dScenePrimaryError::Viewport(_)
        ));
        assert!(error.cleanup.is_none());
        assert!(backend.events.is_empty());
    }

    #[test]
    fn every_pre_begin_failure_stops_without_end_cleanup() {
        for stage in [
            RootMenuC3dSceneStage::ClearDepth,
            RootMenuC3dSceneStage::View,
            RootMenuC3dSceneStage::Viewport,
            RootMenuC3dSceneStage::Begin,
        ] {
            let mut backend = Backend::new();
            backend.failures.push(stage);
            let error = execute_root_menu_c3d_scene(
                &mut backend,
                7,
                EXTENT,
                RootMenuC3dDepthClear::Clear,
                Some(1_u8),
                |backend, plan| {
                    backend.events.push(Event::Submit(plan));
                    Ok::<_, SubmitFailure>(())
                },
            )
            .unwrap_err();
            assert!(matches!(
                error.primary,
                RootMenuC3dScenePrimaryError::Backend { stage: actual, .. } if actual == stage
            ));
            assert!(error.cleanup.is_none());
            assert!(!backend.events.contains(&Event::End));
            assert!(!backend.events.contains(&Event::Submit(1)));
        }
    }

    #[test]
    fn post_begin_submit_failure_attempts_end_and_retains_cleanup_failure() {
        let mut backend = Backend::new();
        backend.failures.push(RootMenuC3dSceneStage::End);
        let error = execute_root_menu_c3d_scene(
            &mut backend,
            7,
            EXTENT,
            RootMenuC3dDepthClear::Clear,
            Some(9_u8),
            |backend, plan| {
                backend.events.push(Event::Submit(plan));
                Err::<(), _>(SubmitFailure)
            },
        )
        .unwrap_err();
        assert_eq!(
            error.primary,
            RootMenuC3dScenePrimaryError::Submit(SubmitFailure)
        );
        assert_eq!(
            error.cleanup,
            Some(SceneFailure(RootMenuC3dSceneStage::End))
        );
        assert_eq!(backend.events.last(), Some(&Event::End));
        assert!(!backend.events.contains(&Event::ResetWorld));
    }

    #[test]
    fn reset_and_tail_failures_cleanup_but_normal_end_failure_is_not_retried() {
        for stage in [
            RootMenuC3dSceneStage::ResetWorld,
            RootMenuC3dSceneStage::Tail,
        ] {
            let mut backend = Backend::new();
            backend.failures.push(stage);
            let error = execute_root_menu_c3d_scene(
                &mut backend,
                7,
                EXTENT,
                RootMenuC3dDepthClear::Clear,
                Some(3_u8),
                |backend, plan| {
                    backend.events.push(Event::Submit(plan));
                    Ok::<_, SubmitFailure>(())
                },
            )
            .unwrap_err();
            assert!(matches!(
                error.primary,
                RootMenuC3dScenePrimaryError::Backend { stage: actual, .. } if actual == stage
            ));
            assert_eq!(
                backend
                    .events
                    .iter()
                    .filter(|event| **event == Event::End)
                    .count(),
                1
            );
        }

        let mut backend = Backend::new();
        backend.failures.push(RootMenuC3dSceneStage::End);
        let error = execute_root_menu_c3d_scene(
            &mut backend,
            7,
            EXTENT,
            RootMenuC3dDepthClear::Clear,
            None::<u8>,
            |_, _| Ok::<_, SubmitFailure>(()),
        )
        .unwrap_err();
        assert!(matches!(
            error.primary,
            RootMenuC3dScenePrimaryError::Backend {
                stage: RootMenuC3dSceneStage::End,
                ..
            }
        ));
        assert!(error.cleanup.is_none());
        assert_eq!(
            backend
                .events
                .iter()
                .filter(|event| **event == Event::End)
                .count(),
            1
        );
    }
}
