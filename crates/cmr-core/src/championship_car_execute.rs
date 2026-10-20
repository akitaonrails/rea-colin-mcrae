//! Actor-scoped execution of a planned championship-car C3D hierarchy.
//!
//! The frame planner establishes depth-first actor and source batch order. This
//! module resolves the complete plan against one frontend binding catalog before
//! touching a backend, then preserves that order without adding raster, palette,
//! or unresolved type-2 mutation behavior.

use std::fmt;

use crate::{
    c3d_batch::{C3dDrawCommand, C3dDrawOrdinal},
    championship_car_frame_plan::{ChampionshipCarActorDraw, ChampionshipCarFramePlan},
    root_menu_c3d_execute::{
        C3dBatchPreflightError, C3dBindingCatalog, C3dPreflightBinding, C3dSelectorBindingCache,
        preflight_c3d_draw_commands,
    },
    root_menu_c3d_resources::{
        C3dPreparedResourceAccessError, C3dPreparedResourceId, C3dPreparedResourceProvenance,
        C3dPreparedSelector,
    },
    root_menu_c3d_transform::{C3dMatrixWords, C3dViewportWords},
};

/// Backend operations at the recovered actor-scoped car submission boundary.
///
/// Resource bindings and the missing-selector default operation each encompass
/// their complete material and texture state. In particular, the default call
/// represents the recovered default-material plus texture-zero operation; it is
/// not an alias for an arena resource.
pub trait ChampionshipCarBatchBackend<H> {
    type Error;

    fn set_championship_car_actor_world(
        &mut self,
        actor_index: usize,
        world: C3dMatrixWords,
    ) -> Result<(), Self::Error>;

    fn bind_championship_car_resource(&mut self, binding: &H) -> Result<(), Self::Error>;

    fn bind_championship_car_default(&mut self) -> Result<(), Self::Error>;

    fn draw_championship_car_batch(&mut self, command: &C3dDrawCommand) -> Result<(), Self::Error>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChampionshipCarBatchStage {
    ActorWorld,
    ResourceBinding,
    DefaultBinding,
    Draw,
}

#[derive(Debug, Eq, PartialEq)]
pub struct ChampionshipCarBatchExecutionError<E> {
    pub actor_index: usize,
    pub ordinal: C3dDrawOrdinal,
    pub stage: ChampionshipCarBatchStage,
    pub selector: C3dPreparedSelector,
    pub source: E,
}

impl<E: fmt::Display> fmt::Display for ChampionshipCarBatchExecutionError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "championship-car C3D {:?} stage failed at actor {} batch {}: {}",
            self.stage,
            self.actor_index,
            self.ordinal.batch_index(),
            self.source
        )
    }
}

impl<E: std::error::Error + 'static> std::error::Error for ChampionshipCarBatchExecutionError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChampionshipCarBatchPreflightError {
    CatalogProvenanceMismatch,
    CatalogIdentityMismatch,
    ResourceAccess {
        actor_index: usize,
        ordinal: C3dDrawOrdinal,
        resource_id: C3dPreparedResourceId,
        source: C3dPreparedResourceAccessError,
    },
}

impl fmt::Display for ChampionshipCarBatchPreflightError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CatalogProvenanceMismatch => formatter.write_str(
                "championship-car C3D frame plan and binding catalog belong to different arenas",
            ),
            Self::CatalogIdentityMismatch => formatter.write_str(
                "prepared championship-car frame belongs to a different binding catalog",
            ),
            Self::ResourceAccess {
                actor_index,
                ordinal,
                resource_id,
                source,
            } => write!(
                formatter,
                "championship-car actor {actor_index} batch {} cannot access binding {}: {source}",
                ordinal.batch_index(),
                resource_id.index()
            ),
        }
    }
}

impl std::error::Error for ChampionshipCarBatchPreflightError {}

#[derive(Debug, Eq, PartialEq)]
pub enum ChampionshipCarExecuteError<E> {
    Preflight(ChampionshipCarBatchPreflightError),
    Backend(ChampionshipCarBatchExecutionError<E>),
}

impl<E: fmt::Display> fmt::Display for ChampionshipCarExecuteError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Preflight(source) => source.fmt(formatter),
            Self::Backend(source) => source.fmt(formatter),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for ChampionshipCarExecuteError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Preflight(source) => Some(source),
            Self::Backend(source) => Some(source),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChampionshipCarBatchOutcome {
    pub actor_worlds: usize,
    pub bindings: usize,
    pub draws: usize,
}

/// Persistent selector-cache owner for championship-car C3D submission.
#[derive(Debug)]
pub struct ChampionshipCarBatchExecutor<'catalog, H> {
    catalog: &'catalog C3dBindingCatalog<H>,
    cache: C3dSelectorBindingCache,
}

/// A single-use championship-car frame whose complete catalog join succeeded.
///
/// Preparation performs no backend effects. Holding the resolved bindings here
/// lets the surrounding scene transaction reject stale or incomplete catalogs
/// before depth clear, camera setup, or `BeginScene`.
#[derive(Debug)]
pub struct PreparedChampionshipCarFrame<'catalog, H> {
    catalog: &'catalog C3dBindingCatalog<H>,
    plan: ChampionshipCarFramePlan,
    bindings: Vec<C3dPreflightBinding<'catalog, H>>,
}

impl<H> PreparedChampionshipCarFrame<'_, H> {
    #[must_use]
    pub const fn extent(&self) -> crate::display::DisplayExtent {
        self.plan.extent()
    }

    #[must_use]
    pub const fn viewport(&self) -> C3dViewportWords {
        self.plan.viewport()
    }

    #[must_use]
    pub const fn selected_parent_visible(&self) -> bool {
        self.plan.selected_parent_visible()
    }
}

impl<'catalog, H> ChampionshipCarBatchExecutor<'catalog, H> {
    #[must_use]
    pub const fn new(catalog: &'catalog C3dBindingCatalog<H>) -> Self {
        Self {
            catalog,
            cache: C3dSelectorBindingCache::new(),
        }
    }

    #[must_use]
    pub const fn provenance(&self) -> C3dPreparedResourceProvenance {
        self.catalog.provenance()
    }

    /// Invalidates the selector cache after another render path may have
    /// changed shared backend material or texture state.
    pub fn invalidate_bindings(&mut self) {
        self.cache.invalidate();
    }

    /// Resolves every selector in `plan` without touching a scene backend.
    pub fn prepare(
        &self,
        plan: ChampionshipCarFramePlan,
    ) -> Result<PreparedChampionshipCarFrame<'catalog, H>, ChampionshipCarBatchPreflightError> {
        let preflight = preflight_c3d_draw_commands(
            self.catalog,
            plan.provenance(),
            plan.draws().iter().map(|draw| draw.command()),
        );
        let bindings = finish_preflight(plan.draws(), preflight)?;
        Ok(PreparedChampionshipCarFrame {
            catalog: self.catalog,
            plan,
            bindings,
        })
    }

    /// Confirms that a prepared frame was resolved by this exact catalog.
    pub fn validate_prepared(
        &self,
        prepared: &PreparedChampionshipCarFrame<'catalog, H>,
    ) -> Result<(), ChampionshipCarBatchPreflightError> {
        if std::ptr::eq(self.catalog, prepared.catalog) {
            Ok(())
        } else {
            Err(ChampionshipCarBatchPreflightError::CatalogIdentityMismatch)
        }
    }

    /// Submits one frame that has already passed the complete catalog join.
    pub fn execute_prepared<B>(
        &mut self,
        backend: &mut B,
        prepared: PreparedChampionshipCarFrame<'catalog, H>,
    ) -> Result<ChampionshipCarBatchOutcome, ChampionshipCarExecuteError<B::Error>>
    where
        B: ChampionshipCarBatchBackend<H>,
    {
        self.validate_prepared(&prepared)
            .map_err(ChampionshipCarExecuteError::Preflight)?;
        execute_preflighted(
            backend,
            &mut self.cache,
            prepared.plan.draws(),
            &prepared.bindings,
        )
    }

    /// Executes one already-planned frame in its actor DFS and source batch order.
    ///
    /// The complete plan/catalog join is checked before the first backend
    /// effect. A successful complete binding commits the selector immediately,
    /// so it remains cached if the subsequent draw fails. A failed binding may
    /// have partially changed backend state and therefore invalidates the prior
    /// cache state.
    pub fn execute<B>(
        &mut self,
        backend: &mut B,
        plan: ChampionshipCarFramePlan,
    ) -> Result<ChampionshipCarBatchOutcome, ChampionshipCarExecuteError<B::Error>>
    where
        B: ChampionshipCarBatchBackend<H>,
    {
        let prepared = self
            .prepare(plan)
            .map_err(ChampionshipCarExecuteError::Preflight)?;
        self.execute_prepared(backend, prepared)
    }
}

fn finish_preflight<'catalog, H, D: ActorDrawAccess>(
    draws: &[D],
    result: Result<Vec<C3dPreflightBinding<'catalog, H>>, C3dBatchPreflightError>,
) -> Result<Vec<C3dPreflightBinding<'catalog, H>>, ChampionshipCarBatchPreflightError> {
    result.map_err(|error| map_preflight_error(draws, error))
}

fn map_preflight_error<D: ActorDrawAccess>(
    draws: &[D],
    error: C3dBatchPreflightError,
) -> ChampionshipCarBatchPreflightError {
    match error {
        C3dBatchPreflightError::CatalogProvenanceMismatch => {
            ChampionshipCarBatchPreflightError::CatalogProvenanceMismatch
        }
        C3dBatchPreflightError::ResourceAccess {
            ordinal,
            resource_id,
            source,
        } => {
            let actor_index = draws
                .iter()
                .find(|draw| draw.command().ordinal() == ordinal)
                .map_or(ordinal.group0_record_index(), |draw| draw.actor_index());
            ChampionshipCarBatchPreflightError::ResourceAccess {
                actor_index,
                ordinal,
                resource_id,
                source,
            }
        }
    }
}

trait ActorDrawAccess {
    fn actor_index(&self) -> usize;
    fn actor_world(&self) -> C3dMatrixWords;
    fn command(&self) -> &C3dDrawCommand;
}

impl ActorDrawAccess for ChampionshipCarActorDraw {
    fn actor_index(&self) -> usize {
        self.actor_index()
    }

    fn actor_world(&self) -> C3dMatrixWords {
        self.actor_world()
    }

    fn command(&self) -> &C3dDrawCommand {
        self.command()
    }
}

fn execute_preflighted<B, H, D>(
    backend: &mut B,
    cache: &mut C3dSelectorBindingCache,
    draws: &[D],
    bindings: &[C3dPreflightBinding<'_, H>],
) -> Result<ChampionshipCarBatchOutcome, ChampionshipCarExecuteError<B::Error>>
where
    B: ChampionshipCarBatchBackend<H>,
    D: ActorDrawAccess,
{
    debug_assert_eq!(draws.len(), bindings.len());
    let mut outcome = ChampionshipCarBatchOutcome {
        actor_worlds: 0,
        bindings: 0,
        draws: 0,
    };
    let mut current_actor = None;

    for (draw, binding) in draws.iter().zip(bindings) {
        let command = draw.command();
        let ordinal = command.ordinal();
        let actor_index = draw.actor_index();
        let selector = command.selector();

        if current_actor != Some(actor_index) {
            backend
                .set_championship_car_actor_world(actor_index, draw.actor_world())
                .map_err(|source| {
                    backend_error(
                        actor_index,
                        ordinal,
                        ChampionshipCarBatchStage::ActorWorld,
                        selector,
                        source,
                    )
                })?;
            current_actor = Some(actor_index);
            outcome.actor_worlds += 1;
        }

        if cache.needs_binding(selector) {
            match binding {
                C3dPreflightBinding::Resource {
                    resource_id,
                    binding,
                    ..
                } => {
                    debug_assert_eq!(selector, C3dPreparedSelector::Resource(*resource_id));
                    if let Err(source) = backend.bind_championship_car_resource(binding) {
                        cache.invalidate();
                        return Err(backend_error(
                            actor_index,
                            ordinal,
                            ChampionshipCarBatchStage::ResourceBinding,
                            selector,
                            source,
                        ));
                    }
                }
                C3dPreflightBinding::Missing { .. } => {
                    debug_assert_eq!(selector, C3dPreparedSelector::Missing);
                    if let Err(source) = backend.bind_championship_car_default() {
                        cache.invalidate();
                        return Err(backend_error(
                            actor_index,
                            ordinal,
                            ChampionshipCarBatchStage::DefaultBinding,
                            selector,
                            source,
                        ));
                    }
                }
            }
            cache.commit(selector);
            outcome.bindings += 1;
        }

        backend
            .draw_championship_car_batch(command)
            .map_err(|source| {
                backend_error(
                    actor_index,
                    ordinal,
                    ChampionshipCarBatchStage::Draw,
                    selector,
                    source,
                )
            })?;
        outcome.draws += 1;
    }

    Ok(outcome)
}

fn backend_error<E>(
    actor_index: usize,
    ordinal: C3dDrawOrdinal,
    stage: ChampionshipCarBatchStage,
    selector: C3dPreparedSelector,
    source: E,
) -> ChampionshipCarExecuteError<E> {
    ChampionshipCarExecuteError::Backend(ChampionshipCarBatchExecutionError {
        actor_index,
        ordinal,
        stage,
        selector,
        source,
    })
}

#[cfg(test)]
mod tests {
    use cmr_formats::{
        bitmap::BitmapLimits,
        c3d::{C3D_FORMAT_VERSION, C3D_HEADER_LEN, C3D_MAGIC, C3dLimits, parse_c3d},
    };

    use super::*;
    use crate::{
        c3d_batch::C3dDrawOrdinal,
        frontend_c3d_resources::championship_mode8_policy,
        root_menu_c3d_execute::bind_prepared_c3d_resources,
        root_menu_c3d_resources::{
            C3dPreparedArenaBuilder, C3dSourceIdentity, C3dVertexMutationBranch, FrontendC3dOwner,
        },
        root_menu_c3d_runtime::{C3dMode2ResourceScaledVertex, C3dOpaqueSideVector},
    };

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Event {
        World(usize, u32),
        Resource(u8),
        Default,
        Draw {
            actor: usize,
            batch: usize,
            vertex_type: u32,
            primitive: u32,
            flags: u32,
        },
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum TestError {
        World,
        Resource,
        Default,
        Draw,
    }

    impl fmt::Display for TestError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "{self:?}")
        }
    }

    impl std::error::Error for TestError {}

    #[derive(Default)]
    struct RecordingBackend {
        events: Vec<Event>,
        current_actor: Option<usize>,
        fail: Option<TestError>,
    }

    impl ChampionshipCarBatchBackend<u8> for RecordingBackend {
        type Error = TestError;

        fn set_championship_car_actor_world(
            &mut self,
            actor_index: usize,
            world: C3dMatrixWords,
        ) -> Result<(), Self::Error> {
            if self.fail == Some(TestError::World) {
                self.fail = None;
                return Err(TestError::World);
            }
            self.current_actor = Some(actor_index);
            self.events
                .push(Event::World(actor_index, world.words()[12]));
            Ok(())
        }

        fn bind_championship_car_resource(&mut self, binding: &u8) -> Result<(), Self::Error> {
            if self.fail == Some(TestError::Resource) {
                self.fail = None;
                return Err(TestError::Resource);
            }
            self.events.push(Event::Resource(*binding));
            Ok(())
        }

        fn bind_championship_car_default(&mut self) -> Result<(), Self::Error> {
            if self.fail == Some(TestError::Default) {
                self.fail = None;
                return Err(TestError::Default);
            }
            self.events.push(Event::Default);
            Ok(())
        }

        fn draw_championship_car_batch(
            &mut self,
            command: &C3dDrawCommand,
        ) -> Result<(), Self::Error> {
            if self.fail == Some(TestError::Draw) {
                self.fail = None;
                return Err(TestError::Draw);
            }
            self.events.push(Event::Draw {
                actor: self.current_actor.unwrap(),
                batch: command.batch_index(),
                vertex_type: command.vertex_type(),
                primitive: command.primitive_code(),
                flags: command.draw_flags(),
            });
            Ok(())
        }
    }

    struct TestDraw {
        actor: usize,
        world: C3dMatrixWords,
        command: C3dDrawCommand,
    }

    impl ActorDrawAccess for TestDraw {
        fn actor_index(&self) -> usize {
            self.actor
        }

        fn actor_world(&self) -> C3dMatrixWords {
            self.world
        }

        fn command(&self) -> &C3dDrawCommand {
            &self.command
        }
    }

    fn command(actor: usize, batch: usize, selector: C3dPreparedSelector) -> TestDraw {
        let mut world = [0; 16];
        world[12] = (actor as f32 + 0.5).to_bits();
        TestDraw {
            actor,
            world: C3dMatrixWords::from_words(world),
            command: C3dDrawCommand::new(
                C3dDrawOrdinal::new(actor, 0, actor as u16, batch),
                selector,
                C3dVertexMutationBranch::Type2,
                Box::<[u16]>::default(),
                Box::<[C3dMode2ResourceScaledVertex]>::default(),
                Box::<[C3dOpaqueSideVector]>::default(),
                Box::<[u16]>::default(),
            ),
        }
    }

    enum TestBinding<'a> {
        Resource {
            resource_id: C3dPreparedResourceId,
            binding: &'a u8,
        },
        Missing,
    }

    fn run(
        backend: &mut RecordingBackend,
        cache: &mut C3dSelectorBindingCache,
        draws: &[TestDraw],
        bindings: &[TestBinding<'_>],
    ) -> Result<ChampionshipCarBatchOutcome, ChampionshipCarExecuteError<TestError>> {
        let bindings = bindings
            .iter()
            .zip(draws)
            .map(|(binding, draw)| match binding {
                TestBinding::Resource {
                    resource_id,
                    binding,
                } => C3dPreflightBinding::Resource {
                    ordinal: draw.command.ordinal(),
                    resource_id: *resource_id,
                    binding: *binding,
                },
                TestBinding::Missing => C3dPreflightBinding::Missing {
                    ordinal: draw.command.ordinal(),
                },
            })
            .collect::<Vec<_>>();

        execute_preflighted(backend, cache, draws, &bindings)
    }

    #[test]
    fn executes_actor_dfs_order_and_exact_type2_commands_with_a_missing_a_transitions() {
        let resource = C3dPreparedResourceId::new_for_test(0);
        let draws = [
            command(4, 10, C3dPreparedSelector::Resource(resource)),
            command(4, 11, C3dPreparedSelector::Missing),
            command(7, 12, C3dPreparedSelector::Resource(resource)),
        ];
        let handle = 91;
        let bindings = [
            TestBinding::Resource {
                resource_id: resource,
                binding: &handle,
            },
            TestBinding::Missing,
            TestBinding::Resource {
                resource_id: resource,
                binding: &handle,
            },
        ];
        let mut backend = RecordingBackend::default();
        let mut cache = C3dSelectorBindingCache::new();

        assert_eq!(
            run(&mut backend, &mut cache, &draws, &bindings).unwrap(),
            ChampionshipCarBatchOutcome {
                actor_worlds: 2,
                bindings: 3,
                draws: 3,
            }
        );
        assert_eq!(
            backend.events,
            [
                Event::World(4, 4.5_f32.to_bits()),
                Event::Resource(91),
                Event::Draw {
                    actor: 4,
                    batch: 10,
                    vertex_type: 2,
                    primitive: 4,
                    flags: 8,
                },
                Event::Default,
                Event::Draw {
                    actor: 4,
                    batch: 11,
                    vertex_type: 2,
                    primitive: 4,
                    flags: 8,
                },
                Event::World(7, 7.5_f32.to_bits()),
                Event::Resource(91),
                Event::Draw {
                    actor: 7,
                    batch: 12,
                    vertex_type: 2,
                    primitive: 4,
                    flags: 8,
                },
            ]
        );
    }

    #[test]
    fn repeated_missing_and_cross_actor_same_selector_reuse_the_persistent_cache() {
        let resource = C3dPreparedResourceId::new_for_test(0);
        let handle = 3;
        let missing = [
            command(0, 0, C3dPreparedSelector::Missing),
            command(0, 1, C3dPreparedSelector::Missing),
        ];
        let same_resource = [
            command(1, 2, C3dPreparedSelector::Resource(resource)),
            command(2, 3, C3dPreparedSelector::Resource(resource)),
        ];
        let resource_bindings = [
            TestBinding::Resource {
                resource_id: resource,
                binding: &handle,
            },
            TestBinding::Resource {
                resource_id: resource,
                binding: &handle,
            },
        ];
        let mut backend = RecordingBackend::default();
        let mut cache = C3dSelectorBindingCache::new();

        run(
            &mut backend,
            &mut cache,
            &missing,
            &[TestBinding::Missing, TestBinding::Missing],
        )
        .unwrap();
        run(&mut backend, &mut cache, &same_resource, &resource_bindings).unwrap();
        assert_eq!(
            backend
                .events
                .iter()
                .filter(|event| matches!(event, Event::Default))
                .count(),
            1
        );
        assert_eq!(
            backend
                .events
                .iter()
                .filter(|event| matches!(event, Event::Resource(3)))
                .count(),
            1
        );

        backend.events.clear();
        run(&mut backend, &mut cache, &same_resource, &resource_bindings).unwrap();
        assert!(
            !backend
                .events
                .iter()
                .any(|event| matches!(event, Event::Resource(_)))
        );
        cache.invalidate();
        backend.events.clear();
        run(&mut backend, &mut cache, &same_resource, &resource_bindings).unwrap();
        assert!(backend.events.contains(&Event::Resource(3)));
    }

    #[test]
    fn binding_failure_does_not_commit_but_draw_failure_keeps_completed_binding() {
        let resource = C3dPreparedResourceId::new_for_test(0);
        let handle = 8;
        let draws = [command(5, 9, C3dPreparedSelector::Resource(resource))];
        let bindings = [TestBinding::Resource {
            resource_id: resource,
            binding: &handle,
        }];
        let mut cache = C3dSelectorBindingCache::new();
        let mut failing_bind = RecordingBackend {
            fail: Some(TestError::Resource),
            ..RecordingBackend::default()
        };
        let error = run(&mut failing_bind, &mut cache, &draws, &bindings).unwrap_err();
        assert_eq!(
            error,
            ChampionshipCarExecuteError::Backend(ChampionshipCarBatchExecutionError {
                actor_index: 5,
                ordinal: draws[0].command.ordinal(),
                stage: ChampionshipCarBatchStage::ResourceBinding,
                selector: C3dPreparedSelector::Resource(resource),
                source: TestError::Resource,
            })
        );

        let mut failing_draw = RecordingBackend {
            fail: Some(TestError::Draw),
            ..RecordingBackend::default()
        };
        assert_eq!(
            run(&mut failing_draw, &mut cache, &draws, &bindings),
            Err(ChampionshipCarExecuteError::Backend(
                ChampionshipCarBatchExecutionError {
                    actor_index: 5,
                    ordinal: draws[0].command.ordinal(),
                    stage: ChampionshipCarBatchStage::Draw,
                    selector: C3dPreparedSelector::Resource(resource),
                    source: TestError::Draw,
                }
            ))
        );
        assert!(failing_draw.events.contains(&Event::Resource(8)));

        let mut retry = RecordingBackend::default();
        run(&mut retry, &mut cache, &draws, &bindings).unwrap();
        assert!(
            !retry
                .events
                .iter()
                .any(|event| matches!(event, Event::Resource(_)))
        );

        let missing = [command(6, 10, C3dPreparedSelector::Missing)];
        let mut failing_default = RecordingBackend {
            fail: Some(TestError::Default),
            ..RecordingBackend::default()
        };
        assert_eq!(
            run(
                &mut failing_default,
                &mut cache,
                &missing,
                &[TestBinding::Missing]
            ),
            Err(ChampionshipCarExecuteError::Backend(
                ChampionshipCarBatchExecutionError {
                    actor_index: 6,
                    ordinal: missing[0].command.ordinal(),
                    stage: ChampionshipCarBatchStage::DefaultBinding,
                    selector: C3dPreparedSelector::Missing,
                    source: TestError::Default,
                }
            ))
        );
        let mut default_retry = RecordingBackend::default();
        run(
            &mut default_retry,
            &mut cache,
            &missing,
            &[TestBinding::Missing],
        )
        .unwrap();
        assert!(default_retry.events.contains(&Event::Default));
    }

    #[test]
    fn failed_binding_invalidates_a_previously_cached_selector() {
        let first = C3dPreparedResourceId::new_for_test(0);
        let second = C3dPreparedResourceId::new_for_test(1);
        let first_handle = 11;
        let second_handle = 22;
        let first_draw = [command(0, 0, C3dPreparedSelector::Resource(first))];
        let second_draw = [command(0, 1, C3dPreparedSelector::Resource(second))];
        let first_binding = [TestBinding::Resource {
            resource_id: first,
            binding: &first_handle,
        }];
        let second_binding = [TestBinding::Resource {
            resource_id: second,
            binding: &second_handle,
        }];
        let mut cache = C3dSelectorBindingCache::new();

        run(
            &mut RecordingBackend::default(),
            &mut cache,
            &first_draw,
            &first_binding,
        )
        .unwrap();
        let mut failed = RecordingBackend {
            fail: Some(TestError::Resource),
            ..RecordingBackend::default()
        };
        assert!(run(&mut failed, &mut cache, &second_draw, &second_binding).is_err());

        let mut retry_first = RecordingBackend::default();
        run(&mut retry_first, &mut cache, &first_draw, &first_binding).unwrap();
        assert!(retry_first.events.contains(&Event::Resource(first_handle)));
    }

    #[test]
    fn catalog_preflight_errors_are_complete_before_backend_effects() {
        let file = parse_c3d(&two_resource_c3d(), C3dLimits::default()).unwrap();
        let mut builder = C3dPreparedArenaBuilder::new().unwrap();
        let object = builder
            .prepare_object(
                C3dSourceIdentity {
                    owner: FrontendC3dOwner::ChampionshipCar(
                        crate::championship_setup::CanonicalCar::Subaru,
                    ),
                    path: "Cars/Test.c3d".into(),
                },
                &file,
                championship_mode8_policy(0),
                BitmapLimits::default(),
            )
            .unwrap();
        let arena = builder.finish();
        let known = object.ordinal_resources()[0];
        let resource = object.ordinal_resources()[1];
        let catalog = bind_prepared_c3d_resources(
            arena.provenance(),
            arena.resources().take(1),
            &mut |id, _| Ok::<_, std::convert::Infallible>(id.index() as u8),
        )
        .unwrap();
        let ordinal = C3dDrawOrdinal::new(6, 0, 6, 14);
        let draws = [command(6, 14, C3dPreparedSelector::Resource(resource))];
        let backend = RecordingBackend::default();

        let foreign = C3dPreparedArenaBuilder::new().unwrap().finish();
        let cases = [
            preflight_c3d_draw_commands(
                &catalog,
                foreign.provenance(),
                draws.iter().map(|draw| &draw.command),
            ),
            preflight_c3d_draw_commands(
                &catalog,
                arena.provenance(),
                draws.iter().map(|draw| &draw.command),
            ),
        ];
        assert_eq!(catalog.binding(known), Ok(&0));
        assert_eq!(
            cases[0],
            Err(C3dBatchPreflightError::CatalogProvenanceMismatch)
        );
        assert_eq!(
            cases[1],
            Err(C3dBatchPreflightError::ResourceAccess {
                ordinal,
                resource_id: resource,
                source: C3dPreparedResourceAccessError::Unknown {
                    index: 1,
                    resource_count: 1,
                },
            })
        );
        for result in cases {
            assert!(finish_preflight(&draws, result).is_err());
            assert!(backend.events.is_empty());
        }
        assert_eq!(draws[0].command.ordinal(), ordinal);
    }

    fn two_resource_c3d() -> Vec<u8> {
        let bitmaps = [indexed_bmp(1), indexed_bmp(2)];
        let blob_start = C3D_HEADER_LEN;
        let total_len = blob_start
            + bitmaps
                .iter()
                .map(|bitmap| 16 + bitmap.len())
                .sum::<usize>();
        let mut bytes = vec![0; total_len];
        bytes[0..4].copy_from_slice(&C3D_MAGIC);
        bytes[4..8].copy_from_slice(&C3D_FORMAT_VERSION.to_le_bytes());
        bytes[16..18].copy_from_slice(&2_u16.to_le_bytes());
        for (index, offset) in [blob_start as u32; 3].into_iter().enumerate() {
            let start = 20 + index * 4;
            bytes[start..start + 4].copy_from_slice(&offset.to_le_bytes());
        }
        let mut offset = blob_start;
        for (index, bitmap) in bitmaps.iter().enumerate() {
            let label = if index == 0 {
                *b"EXEC-FIRST!!"
            } else {
                *b"EXEC-SECOND!"
            };
            bytes[offset..offset + 12].copy_from_slice(&label);
            bytes[offset + 12..offset + 16].copy_from_slice(&(bitmap.len() as u32).to_le_bytes());
            bytes[offset + 16..offset + 16 + bitmap.len()].copy_from_slice(bitmap);
            offset += 16 + bitmap.len();
        }
        bytes
    }

    fn indexed_bmp(pixel: u8) -> Vec<u8> {
        let palette_len = 256 * 4;
        let pixel_offset = 14 + 40 + palette_len;
        let file_len = pixel_offset + 4;
        let mut bytes = vec![0; file_len];
        bytes[0..2].copy_from_slice(b"BM");
        bytes[2..6].copy_from_slice(&(file_len as u32).to_le_bytes());
        bytes[10..14].copy_from_slice(&(pixel_offset as u32).to_le_bytes());
        bytes[14..18].copy_from_slice(&40_u32.to_le_bytes());
        bytes[18..22].copy_from_slice(&1_i32.to_le_bytes());
        bytes[22..26].copy_from_slice(&1_i32.to_le_bytes());
        bytes[26..28].copy_from_slice(&1_u16.to_le_bytes());
        bytes[28..30].copy_from_slice(&8_u16.to_le_bytes());
        bytes[34..38].copy_from_slice(&4_u32.to_le_bytes());
        bytes[46..50].copy_from_slice(&256_u32.to_le_bytes());
        bytes[pixel_offset] = pixel;
        bytes
    }
}
