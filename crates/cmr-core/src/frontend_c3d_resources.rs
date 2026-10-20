//! Frontend-session ownership of championship-car and root-menu C3D resources.
//!
//! The recovered fresh-session order is represented directly: all 14 required
//! cars are prepared with mode 8 first, then the six optional root objects are
//! prepared with mode 2 in the same arena. This module deliberately stops at
//! decoded indexed resources and selectors; mode-8 vertex mutation,
//! transforms, and rendering remain deferred.

use std::fmt;

use cmr_formats::bitmap::BitmapLimits;

use crate::{
    championship_assets::{CHAMPIONSHIP_CAR_COUNT, ChampionshipAssets},
    championship_setup::CanonicalCar,
    root_menu_assets::VisualVariant,
    root_menu_c3d_assets::{ROOT_MENU_C3D_REQUEST_COUNT, RootMenuC3dSet, RootMenuC3dSlot},
    root_menu_c3d_resources::{
        C3dEntryMaterialPolicy, C3dLoadPolicy, C3dPreparedArena, C3dPreparedArenaBuilder,
        C3dPreparedObject, C3dPreparedResource, C3dPreparedResourceAccessError,
        C3dPreparedResourceId, C3dPreparedResourceProvenance, C3dResourceAllocationPolicy,
        C3dResourceArenaIdentityExhausted, C3dResourcePreparationError, C3dSourceIdentity,
        C3dSurfaceLayoutGates, FrontendC3dOwner, RootMenuC3dPreparedObject,
        RootMenuC3dResourceDiagnostic, prepare_root_menu_c3d_objects,
    },
};

/// Resource count visible on entry to each recovered fresh-session car load.
pub const CHAMPIONSHIP_C3D_ENTRY_RESOURCE_SNAPSHOTS: [u32; CHAMPIONSHIP_CAR_COUNT] =
    [0, 4, 8, 12, 16, 20, 24, 28, 32, 36, 40, 44, 48, 51];

/// Exact resource prefix occupied by all 14 championship cars.
pub const CHAMPIONSHIP_C3D_RESOURCE_PREFIX_LEN: usize = 55;

/// Prepared C3D state owned for the lifetime of one frontend session.
///
/// Keeping this value alive across championship-screen visits preserves arena
/// IDs and prevents car resources from being renumbered or re-prepared.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrontendC3dPrepared {
    arena: C3dPreparedArena,
    cars: [C3dPreparedObject; CHAMPIONSHIP_CAR_COUNT],
    root_variant: VisualVariant,
    root_slots: [Option<RootMenuC3dPreparedObject>; ROOT_MENU_C3D_REQUEST_COUNT],
    root_diagnostics: Vec<RootMenuC3dResourceDiagnostic>,
}

impl FrontendC3dPrepared {
    #[must_use]
    pub const fn provenance(&self) -> C3dPreparedResourceProvenance {
        self.arena.provenance()
    }

    #[must_use]
    pub fn car(&self, car: CanonicalCar) -> &C3dPreparedObject {
        &self.cars[car.index() as usize]
    }

    pub fn cars(&self) -> impl ExactSizeIterator<Item = (CanonicalCar, &C3dPreparedObject)> {
        CanonicalCar::ALL
            .into_iter()
            .map(|car| (car, self.car(car)))
    }

    #[must_use]
    pub const fn root_variant(&self) -> VisualVariant {
        self.root_variant
    }

    #[must_use]
    pub fn root(&self, slot: RootMenuC3dSlot) -> Option<&RootMenuC3dPreparedObject> {
        self.root_slots[slot.index()].as_ref()
    }

    pub fn root_slots(
        &self,
    ) -> impl ExactSizeIterator<Item = (RootMenuC3dSlot, Option<&RootMenuC3dPreparedObject>)> {
        RootMenuC3dSlot::ALL
            .into_iter()
            .map(|slot| (slot, self.root(slot)))
    }

    #[must_use]
    pub fn loaded_root_len(&self) -> usize {
        self.root_slots.iter().flatten().count()
    }

    pub fn root_diagnostics(
        &self,
    ) -> impl ExactSizeIterator<Item = &RootMenuC3dResourceDiagnostic> {
        self.root_diagnostics.iter()
    }

    #[must_use]
    pub fn resource_len(&self) -> usize {
        self.arena.resource_len()
    }

    pub fn resources(
        &self,
    ) -> impl ExactSizeIterator<Item = (C3dPreparedResourceId, &C3dPreparedResource)> {
        self.arena.resources()
    }

    pub fn resource(
        &self,
        id: C3dPreparedResourceId,
    ) -> Result<&C3dPreparedResource, C3dPreparedResourceAccessError> {
        self.arena.resource(id)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FrontendC3dPreparationError {
    ArenaIdentity(C3dResourceArenaIdentityExhausted),
    CarResourceCount {
        car: CanonicalCar,
        path: &'static str,
        expected: usize,
        actual: usize,
    },
    CarResource {
        car: CanonicalCar,
        path: &'static str,
        source: C3dResourcePreparationError,
    },
}

impl fmt::Display for FrontendC3dPreparationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ArenaIdentity(source) => source.fmt(formatter),
            Self::CarResourceCount {
                car,
                path,
                expected,
                actual,
            } => write!(
                formatter,
                "championship car {car:?} at {path} has {actual} embedded resources; recovered fresh-session contract requires {expected}"
            ),
            Self::CarResource { car, path, source } => write!(
                formatter,
                "cannot prepare championship car {car:?} at {path}: {source}"
            ),
        }
    }
}

impl std::error::Error for FrontendC3dPreparationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ArenaIdentity(source) => Some(source),
            Self::CarResource { source, .. } => Some(source),
            Self::CarResourceCount { .. } => None,
        }
    }
}

/// Prepares the recovered fresh frontend C3D session.
///
/// Cars are required and fail the complete operation. Root objects retain the
/// existing optional fixed-slot behavior: each failed root object rolls back
/// its own additions, records a diagnostic, and preparation continues.
pub fn prepare_frontend_c3d_resources(
    championship: &ChampionshipAssets,
    root: &RootMenuC3dSet,
    bitmap_limits: BitmapLimits,
) -> Result<FrontendC3dPrepared, FrontendC3dPreparationError> {
    let mut builder =
        C3dPreparedArenaBuilder::new().map_err(FrontendC3dPreparationError::ArenaIdentity)?;
    let mut cars = Vec::with_capacity(CHAMPIONSHIP_CAR_COUNT);

    for (index, car) in CanonicalCar::ALL.into_iter().enumerate() {
        let asset = championship.car(car);
        let expected = if car == CanonicalCar::M4r { 3 } else { 4 };
        let actual = asset.object.blob_records().len();
        // Source-owned fail-closed admission policy. The reviewed corpus
        // establishes these counts; static analysis does not establish how
        // the original executable handled a substituted car object.
        if actual != expected {
            return Err(FrontendC3dPreparationError::CarResourceCount {
                car,
                path: asset.descriptor.object_path,
                expected,
                actual,
            });
        }

        let policy = championship_mode8_policy(CHAMPIONSHIP_C3D_ENTRY_RESOURCE_SNAPSHOTS[index]);
        let prepared = builder
            .prepare_object(
                C3dSourceIdentity {
                    owner: FrontendC3dOwner::ChampionshipCar(car),
                    path: asset.descriptor.object_path.into(),
                },
                &asset.object,
                policy,
                bitmap_limits,
            )
            .map_err(|source| FrontendC3dPreparationError::CarResource {
                car,
                path: asset.descriptor.object_path,
                source,
            })?;
        cars.push(prepared);
    }

    debug_assert_eq!(builder.resource_len(), CHAMPIONSHIP_C3D_RESOURCE_PREFIX_LEN);
    let source_slots = RootMenuC3dSlot::ALL.map(|slot| root.get(slot));
    let (root_slots, root_diagnostics) =
        prepare_root_menu_c3d_objects(&mut builder, source_slots, bitmap_limits);

    Ok(FrontendC3dPrepared {
        arena: builder.finish(),
        cars: cars
            .try_into()
            .expect("one prepared object exists for every canonical car"),
        root_variant: root.variant(),
        root_slots,
        root_diagnostics,
    })
}

const fn championship_mode8_policy(entry_resource_count: u32) -> C3dLoadPolicy {
    C3dLoadPolicy {
        mode: 8,
        caller_option: 1,
        caller_flags: 0x18,
        surface_constructor_flags: 0x15,
        entry_resource_count,
        allocation: C3dResourceAllocationPolicy::AppendEveryResource,
        surface_layout: C3dSurfaceLayoutGates {
            quarter_if_large: false,
            force_quarter: false,
            half_if_large: false,
            expand_width_dominant: true,
        },
        material: C3dEntryMaterialPolicy::FixedUniformWord(0x9696_9696),
    }
}
