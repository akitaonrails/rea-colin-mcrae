//! Arena-scoped resource bindings and source-ordered root C3D batch execution.
//!
//! Binding values are backend-owned RAII handles created once for each interned
//! prepared resource. The executor retains the recovered selector cache across
//! frames. Scene setup, depth clear, terminal composition, and rasterization
//! remain separate boundaries.

use std::fmt;

use crate::{
    root_menu_c3d_frame_plan::{RootMenuC3dDrawCommand, RootMenuC3dFramePlan},
    root_menu_c3d_resources::{
        C3dPreparedResource, C3dPreparedResourceAccessError, C3dPreparedResourceId,
        C3dPreparedResourceProvenance, C3dPreparedSelector,
    },
    root_menu_c3d_topology::RootMenuC3dTopology,
    root_menu_c3d_transform::C3dMatrixWords,
};

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

/// Backend-owned bindings for every interned resource in one prepared arena.
///
/// `H` should own its backend lifetime (normally through RAII). If construction
/// fails, every handle already returned by the mapper is dropped with the
/// partial catalog.
#[derive(Debug, Eq, PartialEq)]
pub struct RootMenuC3dBindingCatalog<H> {
    provenance: C3dPreparedResourceProvenance,
    resource_ids: Vec<C3dPreparedResourceId>,
    bindings: Vec<H>,
}

impl<H> Drop for RootMenuC3dBindingCatalog<H> {
    fn drop(&mut self) {
        while let Some(binding) = self.bindings.pop() {
            drop(binding);
        }
    }
}

impl<H> RootMenuC3dBindingCatalog<H> {
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

#[derive(Debug)]
pub struct RootMenuC3dBindingBuildError<E> {
    pub resource_id: C3dPreparedResourceId,
    pub first_slot: crate::root_menu_c3d_assets::RootMenuC3dSlot,
    pub first_path: &'static str,
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
    let mut resource_ids = Vec::with_capacity(topology.resource_len());
    let mut bindings = ReverseDropVec::with_capacity(topology.resource_len());
    for (resource_id, resource) in topology.resources() {
        let binding = match bind(resource_id, resource) {
            Ok(binding) => binding,
            Err(source) => {
                return Err(RootMenuC3dBindingBuildError {
                    resource_id,
                    first_slot: resource.first_slot(),
                    first_path: resource.first_path(),
                    first_resource_ordinal: resource.first_resource_ordinal(),
                    source,
                });
            }
        };
        resource_ids.push(resource_id);
        bindings.push(binding);
    }
    Ok(RootMenuC3dBindingCatalog {
        provenance: topology.provenance(),
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
    cached: Option<C3dPreparedResourceId>,
}

impl<'catalog, H> RootMenuC3dBatchExecutor<'catalog, H> {
    #[must_use]
    pub const fn new(catalog: &'catalog RootMenuC3dBindingCatalog<H>) -> Self {
        Self {
            catalog,
            cached: None,
        }
    }

    /// Invalidates the selector cache after another render path may have
    /// changed shared backend material or texture state.
    pub fn invalidate_bindings(&mut self) {
        self.cached = None;
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
        if plan.provenance() != self.catalog.provenance() {
            return Err(RootMenuC3dExecuteError::Preflight(
                RootMenuC3dBatchPreflightError::CatalogProvenanceMismatch,
            ));
        }
        for command in plan.commands() {
            let C3dPreparedSelector::Resource(resource_id) = command.selector() else {
                return Err(RootMenuC3dExecuteError::Preflight(
                    RootMenuC3dBatchPreflightError::MissingSelector {
                        batch_index: command.batch_index(),
                    },
                ));
            };
            self.catalog.binding(resource_id).map_err(|source| {
                RootMenuC3dExecuteError::Preflight(RootMenuC3dBatchPreflightError::ResourceAccess {
                    batch_index: command.batch_index(),
                    resource_id,
                    source,
                })
            })?;
        }

        backend
            .set_root_c3d_world(plan.world())
            .map_err(|source| backend_error(RootMenuC3dBatchStage::World, None, None, source))?;
        let mut outcome = RootMenuC3dBatchOutcome {
            bindings: 0,
            draws: 0,
        };
        for command in plan.commands() {
            let C3dPreparedSelector::Resource(resource_id) = command.selector() else {
                unreachable!("preflight rejected missing exact-root selector")
            };
            let binding = self
                .catalog
                .binding(resource_id)
                .expect("preflight resolved every exact-root binding");
            if self.cached != Some(resource_id) {
                backend.bind_root_c3d_resource(binding).map_err(|source| {
                    backend_error(
                        RootMenuC3dBatchStage::Binding,
                        Some(command.batch_index()),
                        Some(resource_id),
                        source,
                    )
                })?;
                self.cached = Some(resource_id);
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
