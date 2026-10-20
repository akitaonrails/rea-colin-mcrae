//! Arena-scoped resource bindings and source-ordered root C3D batch execution.
//!
//! Binding values are backend-owned RAII handles created once for each interned
//! prepared resource. The executor retains the recovered selector cache across
//! frames. Scene setup, depth clear, terminal composition, and rasterization
//! remain separate boundaries.

use std::fmt;

use crate::{
    c3d_batch::{C3dDrawCommand, C3dDrawOrdinal},
    root_menu_c3d_frame_plan::{RootMenuC3dDrawCommand, RootMenuC3dFramePlan},
    root_menu_c3d_resources::{
        C3dPreparedResource, C3dPreparedResourceAccessError, C3dPreparedResourceId,
        C3dPreparedResourceProvenance, C3dPreparedSelector,
    },
    root_menu_c3d_topology::RootMenuC3dTopology,
    root_menu_c3d_transform::C3dMatrixWords,
};

/// Catalog resolution for one backend-neutral draw command.
///
/// `Missing` preserves the source `-1` selector. It does not imply a binding:
/// the recovered default material/texture-zero operation needs its own backend
/// contract and cannot be represented by an arbitrary arena handle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum C3dPreflightBinding<'catalog, H> {
    Resource {
        ordinal: C3dDrawOrdinal,
        resource_id: C3dPreparedResourceId,
        binding: &'catalog H,
    },
    Missing {
        ordinal: C3dDrawOrdinal,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum C3dBatchPreflightError {
    CatalogProvenanceMismatch,
    ResourceAccess {
        ordinal: C3dDrawOrdinal,
        resource_id: C3dPreparedResourceId,
        source: C3dPreparedResourceAccessError,
    },
}

impl fmt::Display for C3dBatchPreflightError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CatalogProvenanceMismatch => formatter
                .write_str("C3D draw commands and binding catalog belong to different arenas"),
            Self::ResourceAccess {
                ordinal,
                resource_id,
                source,
            } => write!(
                formatter,
                "C3D batch {} cannot access binding {}: {source}",
                ordinal.batch_index(),
                resource_id.index()
            ),
        }
    }
}

impl std::error::Error for C3dBatchPreflightError {}

/// Resolves all resource selectors before any backend operation is attempted.
///
/// Missing selectors are retained for a caller with an explicit default
/// binding operation; resource selectors must belong to this exact catalog.
pub fn preflight_c3d_draw_commands<'catalog, 'command, H>(
    catalog: &'catalog C3dBindingCatalog<H>,
    provenance: C3dPreparedResourceProvenance,
    commands: impl IntoIterator<Item = &'command C3dDrawCommand>,
) -> Result<Vec<C3dPreflightBinding<'catalog, H>>, C3dBatchPreflightError> {
    if provenance != catalog.provenance() {
        return Err(C3dBatchPreflightError::CatalogProvenanceMismatch);
    }
    commands
        .into_iter()
        .map(|command| match command.selector() {
            C3dPreparedSelector::Resource(resource_id) => catalog
                .binding(resource_id)
                .map(|binding| C3dPreflightBinding::Resource {
                    ordinal: command.ordinal(),
                    resource_id,
                    binding,
                })
                .map_err(|source| C3dBatchPreflightError::ResourceAccess {
                    ordinal: command.ordinal(),
                    resource_id,
                    source,
                }),
            C3dPreparedSelector::Missing => Ok(C3dPreflightBinding::Missing {
                ordinal: command.ordinal(),
            }),
        })
        .collect()
}

/// Persistent cache for the last completely bound selector state.
///
/// `Missing` represents the recovered material/texture-zero state. Keeping it
/// distinct from both an uninitialized cache and every arena resource is
/// required for `resource -> missing -> same resource` sequences.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct C3dSelectorBindingCache {
    cached: Option<C3dPreparedSelector>,
}

impl C3dSelectorBindingCache {
    #[must_use]
    pub const fn new() -> Self {
        Self { cached: None }
    }

    #[must_use]
    pub fn needs_binding(self, selector: C3dPreparedSelector) -> bool {
        self.cached != Some(selector)
    }

    /// Commits only after the backend has completed its full binding operation.
    pub fn commit(&mut self, selector: C3dPreparedSelector) {
        self.cached = Some(selector);
    }

    pub fn invalidate(&mut self) {
        self.cached = None;
    }
}

struct ReverseDropVec<T>(Vec<T>);

impl<T> ReverseDropVec<T> {
    fn with_capacity(capacity: usize) -> Self {
        Self(Vec::with_capacity(capacity))
    }

    fn push(&mut self, value: T) {
        self.0.push(value);
    }

    fn into_vec(mut self) -> Vec<T> {
        std::mem::take(&mut self.0)
    }
}

impl<T> Drop for ReverseDropVec<T> {
    fn drop(&mut self) {
        while let Some(value) = self.0.pop() {
            drop(value);
        }
    }
}

/// Backend-owned bindings for every prepared resource in one C3D arena.
///
/// `H` should own its backend lifetime (normally through RAII). If construction
/// fails, every handle already returned by the mapper is dropped with the
/// partial catalog.
#[derive(Debug, Eq, PartialEq)]
pub struct C3dBindingCatalog<H> {
    provenance: C3dPreparedResourceProvenance,
    resource_ids: Vec<C3dPreparedResourceId>,
    bindings: Vec<H>,
}

impl<H> Drop for C3dBindingCatalog<H> {
    fn drop(&mut self) {
        while let Some(binding) = self.bindings.pop() {
            drop(binding);
        }
    }
}

impl<H> C3dBindingCatalog<H> {
    #[must_use]
    pub const fn provenance(&self) -> C3dPreparedResourceProvenance {
        self.provenance
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.bindings.len()
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    pub fn binding(&self, id: C3dPreparedResourceId) -> Result<&H, C3dPreparedResourceAccessError> {
        if id.provenance() != self.provenance {
            return Err(C3dPreparedResourceAccessError::Foreign);
        }
        if id.index() >= self.bindings.len() {
            return Err(C3dPreparedResourceAccessError::Unknown {
                index: id.index(),
                resource_count: self.bindings.len(),
            });
        }
        // IDs do not expose their arena payload, so retain and compare the
        // exact topology-issued ID rather than trusting its public index.
        self.resource_ids
            .get(id.index())
            .filter(|expected| **expected == id)
            .and_then(|_| self.bindings.get(id.index()))
            .ok_or(C3dPreparedResourceAccessError::Foreign)
    }
}

/// Compatibility name retained for the isolated root-menu pipeline.
pub type RootMenuC3dBindingCatalog<H> = C3dBindingCatalog<H>;

#[derive(Debug)]
pub struct C3dBindingBuildError<E> {
    pub resource_id: C3dPreparedResourceId,
    pub origin: crate::root_menu_c3d_resources::C3dPreparedResourceOrigin,
    pub source: E,
}

impl<E: fmt::Display> fmt::Display for C3dBindingBuildError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "cannot bind C3D resource {} from {:?} ({}) ordinal {}: {}",
            self.resource_id.index(),
            self.origin.source().owner,
            self.origin.source().path,
            self.origin.source_resource_ordinal(),
            self.source
        )
    }
}

impl<E: std::error::Error + 'static> std::error::Error for C3dBindingBuildError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

#[derive(Debug)]
pub struct RootMenuC3dBindingBuildError<E> {
    pub resource_id: C3dPreparedResourceId,
    pub first_slot: crate::root_menu_c3d_assets::RootMenuC3dSlot,
    pub first_path: Box<str>,
    pub first_resource_ordinal: u16,
    pub source: E,
}

impl<E: fmt::Display> fmt::Display for RootMenuC3dBindingBuildError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "cannot bind root-menu C3D resource {} from {:?} ({}) ordinal {}: {}",
            self.resource_id.index(),
            self.first_slot,
            self.first_path,
            self.first_resource_ordinal,
            self.source
        )
    }
}

impl<E: std::error::Error + 'static> std::error::Error for RootMenuC3dBindingBuildError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// Maps every interned resource exactly once in arena order.
pub fn bind_root_menu_c3d_resources<H, E>(
    topology: &RootMenuC3dTopology<'_>,
    mut bind: impl FnMut(C3dPreparedResourceId, &C3dPreparedResource) -> Result<H, E>,
) -> Result<RootMenuC3dBindingCatalog<H>, RootMenuC3dBindingBuildError<E>> {
    bind_prepared_c3d_resources(topology.provenance(), topology.resources(), &mut bind).map_err(
        |error| RootMenuC3dBindingBuildError {
            resource_id: error.resource_id,
            first_slot: match error.origin.source().owner {
                crate::root_menu_c3d_resources::FrontendC3dOwner::RootMenu(slot) => slot,
                crate::root_menu_c3d_resources::FrontendC3dOwner::ChampionshipCar(_) => {
                    unreachable!("root topology contains only root-owned C3D resources")
                }
            },
            first_path: error.origin.source().path.clone(),
            first_resource_ordinal: error.origin.source_resource_ordinal(),
            source: error.source,
        },
    )
}

pub(crate) fn bind_prepared_c3d_resources<'resource, H, E>(
    provenance: C3dPreparedResourceProvenance,
    resources: impl ExactSizeIterator<Item = (C3dPreparedResourceId, &'resource C3dPreparedResource)>,
    bind: &mut impl FnMut(C3dPreparedResourceId, &C3dPreparedResource) -> Result<H, E>,
) -> Result<C3dBindingCatalog<H>, C3dBindingBuildError<E>> {
    let mut resource_ids = Vec::with_capacity(resources.len());
    let mut bindings = ReverseDropVec::with_capacity(resources.len());
    for (resource_id, resource) in resources {
        let binding = bind(resource_id, resource).map_err(|source| C3dBindingBuildError {
            resource_id,
            origin: resource.origin().clone(),
            source,
        })?;
        resource_ids.push(resource_id);
        bindings.push(binding);
    }
    Ok(C3dBindingCatalog {
        provenance,
        resource_ids,
        bindings: bindings.into_vec(),
    })
}

/// Backend operations established at the branch-neutral root batch boundary.
///
/// A binding owns both material and texture policy. The legacy unified branch
/// binds material then texture, while its alternate branch can bind texture
/// alone; concrete backend profiles preserve their selected internal order.
pub trait RootMenuC3dBatchBackend<H> {
    type Error;

    fn set_root_c3d_world(&mut self, world: C3dMatrixWords) -> Result<(), Self::Error>;

    fn bind_root_c3d_resource(&mut self, binding: &H) -> Result<(), Self::Error>;

    fn draw_root_c3d_batch(&mut self, command: &RootMenuC3dDrawCommand) -> Result<(), Self::Error>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RootMenuC3dBatchStage {
    World,
    Binding,
    Draw,
}

#[derive(Debug, Eq, PartialEq)]
pub struct RootMenuC3dBatchExecutionError<E> {
    pub stage: RootMenuC3dBatchStage,
    pub batch_index: Option<usize>,
    pub resource_id: Option<C3dPreparedResourceId>,
    pub source: E,
}

impl<E: fmt::Display> fmt::Display for RootMenuC3dBatchExecutionError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "root-menu C3D {:?} stage failed", self.stage)?;
        if let Some(batch_index) = self.batch_index {
            write!(formatter, " at batch {batch_index}")?;
        }
        write!(formatter, ": {}", self.source)
    }
}

impl<E: std::error::Error + 'static> std::error::Error for RootMenuC3dBatchExecutionError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RootMenuC3dBatchPreflightError {
    CatalogProvenanceMismatch,
    ResourceAccess {
        batch_index: usize,
        resource_id: C3dPreparedResourceId,
        source: C3dPreparedResourceAccessError,
    },
    MissingSelector {
        batch_index: usize,
    },
}

impl fmt::Display for RootMenuC3dBatchPreflightError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CatalogProvenanceMismatch => formatter.write_str(
                "root-menu C3D frame plan and binding catalog belong to different arenas",
            ),
            Self::ResourceAccess {
                batch_index,
                resource_id,
                source,
            } => write!(
                formatter,
                "root-menu C3D batch {batch_index} cannot access binding {}: {source}",
                resource_id.index()
            ),
            Self::MissingSelector { batch_index } => write!(
                formatter,
                "root-menu C3D exact-root batch {batch_index} has a missing selector"
            ),
        }
    }
}

impl std::error::Error for RootMenuC3dBatchPreflightError {}

#[derive(Debug, Eq, PartialEq)]
pub enum RootMenuC3dExecuteError<E> {
    Preflight(RootMenuC3dBatchPreflightError),
    Backend(RootMenuC3dBatchExecutionError<E>),
}

impl<E: fmt::Display> fmt::Display for RootMenuC3dExecuteError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Preflight(source) => source.fmt(formatter),
            Self::Backend(source) => source.fmt(formatter),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for RootMenuC3dExecuteError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Preflight(source) => Some(source),
            Self::Backend(source) => Some(source),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RootMenuC3dBatchOutcome {
    pub bindings: usize,
    pub draws: usize,
}

/// Persistent selector-cache owner for root C3D submission.
#[derive(Debug)]
pub struct RootMenuC3dBatchExecutor<'catalog, H> {
    catalog: &'catalog RootMenuC3dBindingCatalog<H>,
    /// `None` is the exact-root equivalent of the legacy `-2` uninitialized
    /// cache. Source `-1` is not represented because the planner rejects it.
    cache: C3dSelectorBindingCache,
}

impl<'catalog, H> RootMenuC3dBatchExecutor<'catalog, H> {
    #[must_use]
    pub const fn new(catalog: &'catalog RootMenuC3dBindingCatalog<H>) -> Self {
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

    /// Consumes one frame plan. The selector cache commits only after the
    /// backend's complete resource binding succeeds, and persists after draw
    /// failure. Typed fail-fast errors are reconstruction safety policy; the
    /// legacy graphics return values were ignored.
    pub fn execute<B>(
        &mut self,
        backend: &mut B,
        plan: RootMenuC3dFramePlan,
    ) -> Result<RootMenuC3dBatchOutcome, RootMenuC3dExecuteError<B::Error>>
    where
        B: RootMenuC3dBatchBackend<H>,
    {
        let bindings = preflight_c3d_draw_commands(
            self.catalog,
            plan.provenance(),
            plan.commands().iter().map(RootMenuC3dDrawCommand::command),
        )
        .map_err(|error| {
            RootMenuC3dExecuteError::Preflight(match error {
                C3dBatchPreflightError::CatalogProvenanceMismatch => {
                    RootMenuC3dBatchPreflightError::CatalogProvenanceMismatch
                }
                C3dBatchPreflightError::ResourceAccess {
                    ordinal,
                    resource_id,
                    source,
                } => RootMenuC3dBatchPreflightError::ResourceAccess {
                    batch_index: ordinal.batch_index(),
                    resource_id,
                    source,
                },
            })
        })?;
        for binding in &bindings {
            if let C3dPreflightBinding::Missing { ordinal } = binding {
                return Err(RootMenuC3dExecuteError::Preflight(
                    RootMenuC3dBatchPreflightError::MissingSelector {
                        batch_index: ordinal.batch_index(),
                    },
                ));
            }
        }

        backend
            .set_root_c3d_world(plan.world())
            .map_err(|source| backend_error(RootMenuC3dBatchStage::World, None, None, source))?;
        let mut outcome = RootMenuC3dBatchOutcome {
            bindings: 0,
            draws: 0,
        };
        for (command, binding) in plan.commands().iter().zip(bindings) {
            let C3dPreflightBinding::Resource {
                resource_id,
                binding,
                ..
            } = binding
            else {
                unreachable!("preflight rejected missing exact-root selector")
            };
            let selector = C3dPreparedSelector::Resource(resource_id);
            if self.cache.needs_binding(selector) {
                backend.bind_root_c3d_resource(binding).map_err(|source| {
                    backend_error(
                        RootMenuC3dBatchStage::Binding,
                        Some(command.batch_index()),
                        Some(resource_id),
                        source,
                    )
                })?;
                self.cache.commit(selector);
                outcome.bindings += 1;
            }
            backend.draw_root_c3d_batch(command).map_err(|source| {
                backend_error(
                    RootMenuC3dBatchStage::Draw,
                    Some(command.batch_index()),
                    Some(resource_id),
                    source,
                )
            })?;
            outcome.draws += 1;
        }
        Ok(outcome)
    }
}

fn backend_error<E>(
    stage: RootMenuC3dBatchStage,
    batch_index: Option<usize>,
    resource_id: Option<C3dPreparedResourceId>,
    source: E,
) -> RootMenuC3dExecuteError<E> {
    RootMenuC3dExecuteError::Backend(RootMenuC3dBatchExecutionError {
        stage,
        batch_index,
        resource_id,
        source,
    })
}

#[cfg(test)]
mod catalog_tests {
    use super::*;
    use crate::{
        c3d_batch::{C3dDrawCommand, C3dDrawOrdinal},
        root_menu_c3d_resources::{
            C3dPreparedArenaBuilder, C3dPreparedSelector, C3dVertexMutationBranch,
        },
        root_menu_c3d_runtime::{C3dMode2ResourceScaledVertex, C3dOpaqueSideVector},
    };

    fn command(batch_index: usize, selector: C3dPreparedSelector) -> C3dDrawCommand {
        C3dDrawCommand::new(
            C3dDrawOrdinal::new(0, 0, 0, batch_index),
            selector,
            C3dVertexMutationBranch::Type2,
            Box::<[u16]>::default(),
            Box::<[C3dMode2ResourceScaledVertex]>::default(),
            Box::<[C3dOpaqueSideVector]>::default(),
            Box::<[u16]>::default(),
        )
    }

    #[test]
    fn catalog_distinguishes_unknown_same_arena_ids_from_foreign_ids() {
        let known = C3dPreparedResourceId::new_for_test(0);
        let unknown = C3dPreparedResourceId::new_for_test(1);
        let catalog = C3dBindingCatalog {
            provenance: known.provenance(),
            resource_ids: vec![known],
            bindings: vec![7u8],
        };

        assert_eq!(catalog.binding(known), Ok(&7));
        assert_eq!(
            catalog.binding(unknown),
            Err(C3dPreparedResourceAccessError::Unknown {
                index: 1,
                resource_count: 1,
            })
        );
    }

    #[test]
    fn neutral_preflight_resolves_resources_and_preserves_missing_selectors() {
        let known = C3dPreparedResourceId::new_for_test(0);
        let catalog = C3dBindingCatalog {
            provenance: known.provenance(),
            resource_ids: vec![known],
            bindings: vec![7u8],
        };
        let commands = [
            command(2, C3dPreparedSelector::Resource(known)),
            command(3, C3dPreparedSelector::Missing),
        ];

        let bindings =
            preflight_c3d_draw_commands(&catalog, known.provenance(), commands.iter()).unwrap();

        assert_eq!(
            bindings,
            [
                C3dPreflightBinding::Resource {
                    ordinal: C3dDrawOrdinal::new(0, 0, 0, 2),
                    resource_id: known,
                    binding: &7,
                },
                C3dPreflightBinding::Missing {
                    ordinal: C3dDrawOrdinal::new(0, 0, 0, 3),
                },
            ]
        );
    }

    #[test]
    fn neutral_preflight_rejects_provenance_and_catalog_access_before_execution() {
        let known = C3dPreparedResourceId::new_for_test(0);
        let unknown = C3dPreparedResourceId::new_for_test(1);
        let catalog = C3dBindingCatalog {
            provenance: known.provenance(),
            resource_ids: vec![known],
            bindings: vec![7u8],
        };
        let first_allocated = C3dPreparedArenaBuilder::new().unwrap().finish();
        let second_allocated = C3dPreparedArenaBuilder::new().unwrap().finish();
        let foreign = if first_allocated.provenance() != known.provenance() {
            first_allocated
        } else {
            second_allocated
        };

        assert_eq!(
            preflight_c3d_draw_commands(
                &catalog,
                foreign.provenance(),
                [command(0, C3dPreparedSelector::Resource(known))].iter(),
            ),
            Err(C3dBatchPreflightError::CatalogProvenanceMismatch)
        );
        assert_eq!(
            preflight_c3d_draw_commands(
                &catalog,
                known.provenance(),
                [command(4, C3dPreparedSelector::Resource(unknown))].iter(),
            ),
            Err(C3dBatchPreflightError::ResourceAccess {
                ordinal: C3dDrawOrdinal::new(0, 0, 0, 4),
                resource_id: unknown,
                source: C3dPreparedResourceAccessError::Unknown {
                    index: 1,
                    resource_count: 1,
                },
            })
        );
    }

    #[test]
    fn neutral_binding_cache_commits_only_explicitly_and_invalidates() {
        let first = C3dPreparedResourceId::new_for_test(0);
        let second = C3dPreparedResourceId::new_for_test(1);
        let mut cache = C3dSelectorBindingCache::new();

        let first = C3dPreparedSelector::Resource(first);
        let second = C3dPreparedSelector::Resource(second);
        assert!(cache.needs_binding(first));
        cache.commit(first);
        assert!(!cache.needs_binding(first));
        assert!(cache.needs_binding(C3dPreparedSelector::Missing));
        cache.commit(C3dPreparedSelector::Missing);
        assert!(!cache.needs_binding(C3dPreparedSelector::Missing));
        assert!(cache.needs_binding(first));
        cache.commit(first);
        assert!(cache.needs_binding(second));
        cache.invalidate();
        assert!(cache.needs_binding(first));
    }
}
