//! Terminal C3D scene transaction for one prepared championship-car frame.
//!
//! Catalog resolution is deliberately separate: callers must prepare the
//! single-use frame before this function can perform depth, camera, scene, or
//! draw effects.

use std::fmt;

use crate::{
    championship_car_execute::{
        ChampionshipCarBatchBackend, ChampionshipCarBatchExecutor, ChampionshipCarBatchOutcome,
        ChampionshipCarBatchPreflightError, ChampionshipCarExecuteError,
        PreparedChampionshipCarFrame,
    },
    display::DisplayExtent,
    frontend_c3d_scene::{
        C3dBindingState, C3dDepthClear, C3dSceneError, C3dSceneOutcome, C3dScenePrimaryError,
        C3dSceneStage, FrontendC3dSceneBackend, execute_frontend_c3d_scene,
    },
};

pub type ChampionshipCarSceneOutcome = C3dSceneOutcome<ChampionshipCarBatchOutcome>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChampionshipCarScenePreflightStage {
    BatchBackend,
}

#[derive(Debug, Eq, PartialEq)]
pub enum ChampionshipCarSceneError<E> {
    /// Effect-free rejection before depth clear or any other scene operation.
    Preflight(ChampionshipCarBatchPreflightError),
    /// Backend-specific runtime state rejected before any scene effect.
    BackendPreflight {
        stage: ChampionshipCarScenePreflightStage,
        source: E,
    },
    Scene(C3dSceneError<E, ChampionshipCarExecuteError<E>>),
}

impl<E: fmt::Display> fmt::Display for ChampionshipCarSceneError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Preflight(source) => source.fmt(formatter),
            Self::BackendPreflight { stage, source } => {
                write!(
                    formatter,
                    "championship-car {stage:?} preflight failed: {source}"
                )
            }
            Self::Scene(source) => source.fmt(formatter),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for ChampionshipCarSceneError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Preflight(source) => Some(source),
            Self::BackendPreflight { source, .. } => Some(source),
            Self::Scene(source) => Some(source),
        }
    }
}

type TerminalSceneResult<S, D, O> = Result<C3dSceneOutcome<O>, C3dSceneError<S, D>>;

/// Executes one already-preflighted championship-car frame as a complete scene.
///
/// The frame is consumed on every path. Cache state is retained only when no
/// submission or post-submission operation failed and the opaque tail reports
/// that bindings were preserved. Failures before `BeginScene` preserve it.
pub fn execute_championship_car_scene<'catalog, B, H>(
    executor: &mut ChampionshipCarBatchExecutor<'catalog, H>,
    backend: &mut B,
    destination: B::Surface,
    depth_clear: C3dDepthClear,
    prepared: PreparedChampionshipCarFrame<'catalog, H>,
) -> Result<ChampionshipCarSceneOutcome, ChampionshipCarSceneError<B::SceneError>>
where
    B: FrontendC3dSceneBackend
        + ChampionshipCarBatchBackend<H, Error = <B as FrontendC3dSceneBackend>::SceneError>,
{
    if let Err(source) = executor.validate_prepared(&prepared) {
        return Err(ChampionshipCarSceneError::Preflight(source));
    }
    backend
        .preflight_championship_car_scene()
        .map_err(|source| ChampionshipCarSceneError::BackendPreflight {
            stage: ChampionshipCarScenePreflightStage::BatchBackend,
            source,
        })?;
    let extent = prepared.extent();
    let (result, invalidate) = execute_terminal_scene(
        backend,
        destination,
        extent,
        depth_clear,
        prepared,
        |backend, prepared| executor.execute_prepared(backend, prepared),
    );

    if invalidate {
        executor.invalidate_bindings();
    }
    result.map_err(ChampionshipCarSceneError::Scene)
}

/// Executes the terminal championship scene even when the current setup
/// preview has no car.  The empty case still performs the recovered common
/// scene bracket and tail, preventing a previous car frame from remaining on
/// screen when the selection moves to `Next`.
pub fn execute_optional_championship_car_scene<'catalog, B, H>(
    executor: &mut ChampionshipCarBatchExecutor<'catalog, H>,
    backend: &mut B,
    destination: B::Surface,
    extent: DisplayExtent,
    depth_clear: C3dDepthClear,
    prepared: Option<PreparedChampionshipCarFrame<'catalog, H>>,
) -> Result<ChampionshipCarSceneOutcome, ChampionshipCarSceneError<B::SceneError>>
where
    B: FrontendC3dSceneBackend
        + ChampionshipCarBatchBackend<H, Error = <B as FrontendC3dSceneBackend>::SceneError>,
{
    preflight_optional_championship_car_scene(executor, backend, prepared.as_ref())?;
    let scene_extent = prepared
        .as_ref()
        .map_or(extent, PreparedChampionshipCarFrame::extent);
    let result = execute_frontend_c3d_scene(
        backend,
        destination,
        scene_extent,
        depth_clear,
        prepared,
        |backend, prepared| executor.execute_prepared(backend, prepared),
    );
    if invalidates_selector_cache(&result) {
        executor.invalidate_bindings();
    }
    result.map_err(ChampionshipCarSceneError::Scene)
}

pub(crate) fn preflight_optional_championship_car_scene<'catalog, B, H>(
    executor: &ChampionshipCarBatchExecutor<'catalog, H>,
    backend: &mut B,
    prepared: Option<&PreparedChampionshipCarFrame<'catalog, H>>,
) -> Result<(), ChampionshipCarSceneError<B::SceneError>>
where
    B: FrontendC3dSceneBackend
        + ChampionshipCarBatchBackend<H, Error = <B as FrontendC3dSceneBackend>::SceneError>,
{
    let Some(frame) = prepared else {
        return Ok(());
    };
    executor
        .validate_prepared(frame)
        .map_err(ChampionshipCarSceneError::Preflight)?;
    backend
        .preflight_championship_car_scene()
        .map_err(|source| ChampionshipCarSceneError::BackendPreflight {
            stage: ChampionshipCarScenePreflightStage::BatchBackend,
            source,
        })
}

fn execute_terminal_scene<B, P, O, D>(
    backend: &mut B,
    destination: B::Surface,
    extent: DisplayExtent,
    depth_clear: C3dDepthClear,
    prepared: P,
    submit: impl FnOnce(&mut B, P) -> Result<O, D>,
) -> (TerminalSceneResult<B::SceneError, D, O>, bool)
where
    B: FrontendC3dSceneBackend,
{
    let result = execute_frontend_c3d_scene(
        backend,
        destination,
        extent,
        depth_clear,
        Some(prepared),
        submit,
    );
    let invalidate = invalidates_selector_cache(&result);
    (result, invalidate)
}

fn invalidates_selector_cache<S, D, O>(
    result: &Result<C3dSceneOutcome<O>, C3dSceneError<S, D>>,
) -> bool {
    match result {
        Ok(outcome) => outcome.binding_state == C3dBindingState::Changed,
        Err(error) => match &error.primary {
            C3dScenePrimaryError::Viewport(_) => false,
            C3dScenePrimaryError::Backend { stage, .. } => !matches!(
                stage,
                C3dSceneStage::ClearDepth
                    | C3dSceneStage::View
                    | C3dSceneStage::Viewport
                    | C3dSceneStage::Begin
            ),
            C3dScenePrimaryError::Submit(_) => true,
        },
    }
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use super::*;
    use crate::{
        c3d_batch::C3dDrawCommand,
        championship_car_frame_plan::tests::{
            scene_test_fixture, scene_test_fixtures_with_selectors,
        },
        frontend_c3d_scene::{C3dMatrixWords, C3dViewportError, C3dViewportWords},
        root_menu_c3d_execute::bind_prepared_c3d_resources,
    };

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Event {
        Preflight,
        Clear,
        View,
        Viewport,
        Begin,
        ActorWorld(usize),
        Resource,
        Draw(usize),
        Default,
        ResetWorld,
        Tail,
        End,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum TestError {
        Profile,
        Resource,
    }

    impl std::fmt::Display for TestError {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(formatter, "{self:?}")
        }
    }

    impl std::error::Error for TestError {}

    struct Backend {
        events: Vec<Event>,
        fail_preflight: bool,
        fail_resource: bool,
        tail_state: C3dBindingState,
    }

    impl Default for Backend {
        fn default() -> Self {
            Self {
                events: Vec::new(),
                fail_preflight: false,
                fail_resource: false,
                tail_state: C3dBindingState::Preserved,
            }
        }
    }

    impl FrontendC3dSceneBackend for Backend {
        type Surface = u8;
        type SceneError = TestError;

        fn clear_c3d_depth(
            &mut self,
            _: Self::Surface,
            _: DisplayExtent,
        ) -> Result<(), Self::SceneError> {
            self.events.push(Event::Clear);
            Ok(())
        }

        fn set_c3d_view(&mut self, _: C3dMatrixWords) -> Result<(), Self::SceneError> {
            self.events.push(Event::View);
            Ok(())
        }

        fn set_c3d_viewport(&mut self, _: C3dViewportWords) -> Result<(), Self::SceneError> {
            self.events.push(Event::Viewport);
            Ok(())
        }

        fn begin_c3d_scene(&mut self, _: Self::Surface) -> Result<(), Self::SceneError> {
            self.events.push(Event::Begin);
            Ok(())
        }

        fn reset_c3d_world(&mut self) -> Result<(), Self::SceneError> {
            self.events.push(Event::ResetWorld);
            Ok(())
        }

        fn finish_c3d_scene_tail(&mut self) -> Result<C3dBindingState, Self::SceneError> {
            self.events.push(Event::Tail);
            Ok(self.tail_state)
        }

        fn end_c3d_scene(&mut self) -> Result<(), Self::SceneError> {
            self.events.push(Event::End);
            Ok(())
        }
    }

    impl ChampionshipCarBatchBackend<u8> for Backend {
        type Error = TestError;

        fn preflight_championship_car_scene(&mut self) -> Result<(), Self::Error> {
            self.events.push(Event::Preflight);
            if self.fail_preflight {
                Err(TestError::Profile)
            } else {
                Ok(())
            }
        }

        fn set_championship_car_actor_world(
            &mut self,
            actor_index: usize,
            _: C3dMatrixWords,
        ) -> Result<(), Self::Error> {
            self.events.push(Event::ActorWorld(actor_index));
            Ok(())
        }

        fn bind_championship_car_resource(&mut self, _: &u8) -> Result<(), Self::Error> {
            self.events.push(Event::Resource);
            if self.fail_resource {
                self.fail_resource = false;
                return Err(TestError::Resource);
            }
            Ok(())
        }

        fn bind_championship_car_default(&mut self) -> Result<(), Self::Error> {
            self.events.push(Event::Default);
            Ok(())
        }

        fn draw_championship_car_batch(
            &mut self,
            command: &C3dDrawCommand,
        ) -> Result<(), Self::Error> {
            self.events
                .push(Event::Draw(usize::from(command.ordinal().group1_ordinal())));
            Ok(())
        }
    }

    const EXTENT: DisplayExtent = DisplayExtent {
        width: 640,
        height: 480,
    };

    #[test]
    fn terminal_scene_orders_brackets_around_actor_bind_and_draw_submission() {
        let mut backend = Backend::default();
        let (outcome, invalidate) = execute_terminal_scene(
            &mut backend,
            4,
            EXTENT,
            C3dDepthClear::Clear,
            (),
            |backend, ()| {
                backend.events.push(Event::ActorWorld(0));
                backend.events.push(Event::Resource);
                backend.events.push(Event::Draw(0));
                backend.events.push(Event::Default);
                backend.events.push(Event::Draw(1));
                Ok::<_, Infallible>(2_usize)
            },
        );

        assert_eq!(
            backend.events,
            [
                Event::Clear,
                Event::View,
                Event::Viewport,
                Event::Begin,
                Event::ActorWorld(0),
                Event::Resource,
                Event::Draw(0),
                Event::Default,
                Event::Draw(1),
                Event::ResetWorld,
                Event::Tail,
                Event::End,
            ]
        );
        assert_eq!(outcome.unwrap().submission, Some(2));
        assert!(!invalidate);
    }

    #[test]
    fn backend_preflight_rejection_is_typed_and_precedes_all_scene_effects() {
        let (plan, arena) = scene_test_fixture();
        let catalog =
            bind_prepared_c3d_resources(arena.provenance(), arena.resources(), &mut |_, _| {
                Ok::<_, Infallible>(7_u8)
            })
            .unwrap();
        let mut executor = ChampionshipCarBatchExecutor::new(&catalog);
        let prepared = executor.prepare(plan).unwrap();
        let mut backend = Backend {
            fail_preflight: true,
            ..Backend::default()
        };

        let error = execute_championship_car_scene(
            &mut executor,
            &mut backend,
            4,
            C3dDepthClear::Clear,
            prepared,
        )
        .unwrap_err();

        assert_eq!(
            error,
            ChampionshipCarSceneError::BackendPreflight {
                stage: ChampionshipCarScenePreflightStage::BatchBackend,
                source: TestError::Profile,
            }
        );
        assert_eq!(backend.events, [Event::Preflight]);
    }

    #[test]
    fn optional_empty_car_scene_still_runs_the_complete_terminal_bracket() {
        let (_, arena) = scene_test_fixture();
        let catalog =
            bind_prepared_c3d_resources(arena.provenance(), arena.resources(), &mut |_, _| {
                Ok::<_, Infallible>(7_u8)
            })
            .unwrap();
        let mut executor = ChampionshipCarBatchExecutor::new(&catalog);
        let mut backend = Backend::default();

        let outcome = execute_optional_championship_car_scene(
            &mut executor,
            &mut backend,
            4,
            EXTENT,
            C3dDepthClear::Clear,
            None,
        )
        .unwrap();

        assert_eq!(
            backend.events,
            [
                Event::Clear,
                Event::View,
                Event::Viewport,
                Event::Begin,
                Event::ResetWorld,
                Event::Tail,
                Event::End,
            ]
        );
        assert_eq!(outcome.submission, None);
    }

    #[test]
    fn real_prepared_scene_preserves_exact_terminal_order_including_missing_default() {
        let (plan, arena) = scene_test_fixture();
        let catalog =
            bind_prepared_c3d_resources(arena.provenance(), arena.resources(), &mut |_, _| {
                Ok::<_, Infallible>(7_u8)
            })
            .unwrap();
        let mut executor = ChampionshipCarBatchExecutor::new(&catalog);
        let prepared = executor.prepare(plan).unwrap();
        let mut backend = Backend::default();

        let outcome = execute_championship_car_scene(
            &mut executor,
            &mut backend,
            4,
            C3dDepthClear::Clear,
            prepared,
        )
        .unwrap();

        assert_eq!(
            backend.events,
            [
                Event::Preflight,
                Event::Clear,
                Event::View,
                Event::Viewport,
                Event::Begin,
                Event::ActorWorld(0),
                Event::Resource,
                Event::Draw(0),
                Event::ActorWorld(1),
                Event::Default,
                Event::Draw(1),
                Event::ResetWorld,
                Event::Tail,
                Event::End,
            ]
        );
        assert_eq!(
            outcome.submission,
            Some(ChampionshipCarBatchOutcome {
                actor_worlds: 2,
                bindings: 2,
                draws: 2,
            })
        );
    }

    #[test]
    fn preparation_and_catalog_brand_rejections_are_effect_free() {
        let (plan, arena) = scene_test_fixture();
        let incomplete = bind_prepared_c3d_resources(
            arena.provenance(),
            arena.resources().take(0),
            &mut |_, _| Ok::<_, Infallible>(1_u8),
        )
        .unwrap();
        let incomplete_executor = ChampionshipCarBatchExecutor::new(&incomplete);
        assert!(incomplete_executor.prepare(plan).is_err());

        let (plan, arena) = scene_test_fixture();
        let first =
            bind_prepared_c3d_resources(arena.provenance(), arena.resources(), &mut |_, _| {
                Ok::<_, Infallible>(1_u8)
            })
            .unwrap();
        let second =
            bind_prepared_c3d_resources(arena.provenance(), arena.resources(), &mut |_, _| {
                Ok::<_, Infallible>(2_u8)
            })
            .unwrap();
        let prepared = ChampionshipCarBatchExecutor::new(&first)
            .prepare(plan)
            .unwrap();
        let mut second_executor = ChampionshipCarBatchExecutor::new(&second);
        let mut backend = Backend::default();

        let error = execute_championship_car_scene(
            &mut second_executor,
            &mut backend,
            4,
            C3dDepthClear::Clear,
            prepared,
        )
        .unwrap_err();
        assert!(matches!(
            error,
            ChampionshipCarSceneError::Preflight(
                ChampionshipCarBatchPreflightError::CatalogIdentityMismatch
            )
        ));
        assert!(backend.events.is_empty());
    }

    #[test]
    fn real_scene_preserves_cache_only_until_changed_or_failed_submission() {
        let (plans, arena) = scene_test_fixtures_with_selectors(4, &[0]);
        let catalog =
            bind_prepared_c3d_resources(arena.provenance(), arena.resources(), &mut |_, _| {
                Ok::<_, Infallible>(7_u8)
            })
            .unwrap();
        let mut executor = ChampionshipCarBatchExecutor::new(&catalog);
        let mut prepared = plans
            .into_iter()
            .map(|plan| executor.prepare(plan).unwrap())
            .collect::<Vec<_>>()
            .into_iter();
        let mut backend = Backend::default();

        execute_championship_car_scene(
            &mut executor,
            &mut backend,
            4,
            C3dDepthClear::Skip,
            prepared.next().unwrap(),
        )
        .unwrap();
        assert_eq!(
            backend
                .events
                .iter()
                .filter(|event| **event == Event::Resource)
                .count(),
            1
        );

        backend.events.clear();
        backend.tail_state = C3dBindingState::Changed;
        execute_championship_car_scene(
            &mut executor,
            &mut backend,
            4,
            C3dDepthClear::Skip,
            prepared.next().unwrap(),
        )
        .unwrap();
        assert!(!backend.events.contains(&Event::Resource));

        backend.events.clear();
        backend.tail_state = C3dBindingState::Preserved;
        backend.fail_resource = true;
        assert!(
            execute_championship_car_scene(
                &mut executor,
                &mut backend,
                4,
                C3dDepthClear::Skip,
                prepared.next().unwrap(),
            )
            .is_err()
        );
        assert!(backend.events.contains(&Event::Resource));

        backend.events.clear();
        execute_championship_car_scene(
            &mut executor,
            &mut backend,
            4,
            C3dDepthClear::Skip,
            prepared.next().unwrap(),
        )
        .unwrap();
        assert!(backend.events.contains(&Event::Resource));
    }

    fn successful(state: C3dBindingState) -> Result<C3dSceneOutcome<()>, C3dSceneError<(), ()>> {
        Ok(C3dSceneOutcome {
            depth_cleared: true,
            submission: Some(()),
            binding_state: state,
        })
    }

    fn backend_failure(stage: C3dSceneStage) -> Result<C3dSceneOutcome<()>, C3dSceneError<(), ()>> {
        Err(C3dSceneError {
            primary: C3dScenePrimaryError::Backend { stage, source: () },
            cleanup: None,
        })
    }

    #[test]
    fn cache_policy_preserves_only_certain_pre_begin_and_successful_preserved_paths() {
        assert!(!invalidates_selector_cache(&successful(
            C3dBindingState::Preserved
        )));
        assert!(invalidates_selector_cache(&successful(
            C3dBindingState::Changed
        )));

        for stage in [
            C3dSceneStage::ClearDepth,
            C3dSceneStage::View,
            C3dSceneStage::Viewport,
            C3dSceneStage::Begin,
        ] {
            assert!(!invalidates_selector_cache(&backend_failure(stage)));
        }
        for stage in [
            C3dSceneStage::ResetWorld,
            C3dSceneStage::Tail,
            C3dSceneStage::End,
        ] {
            assert!(invalidates_selector_cache(&backend_failure(stage)));
        }

        let submit: Result<C3dSceneOutcome<()>, C3dSceneError<(), ()>> = Err(C3dSceneError {
            primary: C3dScenePrimaryError::Submit(()),
            cleanup: Some(()),
        });
        assert!(invalidates_selector_cache(&submit));

        let viewport: Result<C3dSceneOutcome<()>, C3dSceneError<(), Infallible>> =
            Err(C3dSceneError {
                primary: C3dScenePrimaryError::Viewport(C3dViewportError {
                    extent: DisplayExtent {
                        width: 0,
                        height: 480,
                    },
                }),
                cleanup: None,
            });
        assert!(!invalidates_selector_cache(&viewport));
    }
}
