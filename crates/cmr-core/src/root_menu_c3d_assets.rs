//! Fixed-slot loading for the six optional root-menu C3D objects.
//!
//! This module stops at bounded file and opaque-blob admission. Runtime object
//! creation and rendering remain unrecovered boundaries and must not be
//! represented by indexed-surface handles.

use std::fmt;

use cmr_assets::{AssetStore, AssetStoreError};
use cmr_formats::c3d::{C3dError, C3dFile, C3dLimits, parse_c3d};

use crate::root_menu_assets::{RootMenuObjectIdentity, VisualVariant};

pub const ROOT_MENU_C3D_REQUEST_COUNT: usize = RootMenuObjectIdentity::ALL.len();

/// Selection-order slot. This is explicit because the identity enum's
/// declaration order is not the recovered request order.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u8)]
pub enum RootMenuC3dSlot {
    Championship,
    Rally,
    TimeTrial,
    RallySchool,
    Options,
    Load,
}

impl RootMenuC3dSlot {
    pub const ALL: [Self; ROOT_MENU_C3D_REQUEST_COUNT] = [
        Self::Championship,
        Self::Rally,
        Self::TimeTrial,
        Self::RallySchool,
        Self::Options,
        Self::Load,
    ];

    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    #[must_use]
    pub const fn identity(self) -> RootMenuObjectIdentity {
        match self {
            Self::Championship => RootMenuObjectIdentity::Championship,
            Self::Rally => RootMenuObjectIdentity::Rally,
            Self::TimeTrial => RootMenuObjectIdentity::TimeTrial,
            Self::RallySchool => RootMenuObjectIdentity::RallySchool,
            Self::Options => RootMenuObjectIdentity::Options,
            Self::Load => RootMenuObjectIdentity::Load,
        }
    }

    #[must_use]
    pub const fn path(self, variant: VisualVariant) -> &'static str {
        self.identity().path(variant)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RootMenuC3dAsset {
    pub slot: RootMenuC3dSlot,
    pub identity: RootMenuObjectIdentity,
    pub path: &'static str,
    pub file: C3dFile,
}

#[derive(Debug)]
pub enum RootMenuC3dLoadError {
    Asset {
        slot: RootMenuC3dSlot,
        identity: RootMenuObjectIdentity,
        path: &'static str,
        source: AssetStoreError,
    },
    Decode {
        slot: RootMenuC3dSlot,
        identity: RootMenuObjectIdentity,
        path: &'static str,
        source: C3dError,
    },
}

impl RootMenuC3dLoadError {
    #[must_use]
    pub const fn slot(&self) -> RootMenuC3dSlot {
        match self {
            Self::Asset { slot, .. } | Self::Decode { slot, .. } => *slot,
        }
    }

    #[must_use]
    pub const fn identity(&self) -> RootMenuObjectIdentity {
        match self {
            Self::Asset { identity, .. } | Self::Decode { identity, .. } => *identity,
        }
    }

    #[must_use]
    pub const fn path(&self) -> &'static str {
        match self {
            Self::Asset { path, .. } | Self::Decode { path, .. } => path,
        }
    }
}

impl fmt::Display for RootMenuC3dLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Asset {
                slot,
                identity,
                path,
                source,
            } => write!(
                formatter,
                "cannot load root-menu C3D slot {slot:?} ({identity:?}) at {path}: {source}"
            ),
            Self::Decode {
                slot,
                identity,
                path,
                source,
            } => write!(
                formatter,
                "cannot decode root-menu C3D slot {slot:?} ({identity:?}) at {path}: {source}"
            ),
        }
    }
}

impl std::error::Error for RootMenuC3dLoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Asset { source, .. } => Some(source),
            Self::Decode { source, .. } => Some(source),
        }
    }
}

/// Six stable optional slots plus failures in request order.
#[derive(Debug)]
pub struct RootMenuC3dSet {
    variant: VisualVariant,
    slots: [Option<RootMenuC3dAsset>; ROOT_MENU_C3D_REQUEST_COUNT],
    diagnostics: Vec<RootMenuC3dLoadError>,
}

impl RootMenuC3dSet {
    #[must_use]
    pub const fn variant(&self) -> VisualVariant {
        self.variant
    }

    pub fn slots(
        &self,
    ) -> impl ExactSizeIterator<Item = (RootMenuC3dSlot, Option<&RootMenuC3dAsset>)> {
        RootMenuC3dSlot::ALL
            .into_iter()
            .map(|slot| (slot, self.slots[slot.index()].as_ref()))
    }

    pub fn iter(&self) -> impl Iterator<Item = &RootMenuC3dAsset> {
        self.slots.iter().flatten()
    }

    #[must_use]
    pub fn get(&self, slot: RootMenuC3dSlot) -> Option<&RootMenuC3dAsset> {
        self.slots[slot.index()].as_ref()
    }

    pub fn diagnostics(&self) -> impl ExactSizeIterator<Item = &RootMenuC3dLoadError> {
        self.diagnostics.iter()
    }

    #[must_use]
    pub fn loaded_len(&self) -> usize {
        self.iter().count()
    }

    #[must_use]
    pub fn diagnostic_len(&self) -> usize {
        self.diagnostics.len()
    }
}

/// Attempts all six recovered root-menu object requests in selection order.
///
/// Every request is optional in the shipped loader. Missing, rejected, or
/// malformed objects leave their exact slot empty and loading continues.
#[must_use]
pub fn load_root_menu_c3d(
    store: &AssetStore,
    variant: VisualVariant,
    limits: C3dLimits,
) -> RootMenuC3dSet {
    let mut slots = std::array::from_fn(|_| None);
    let mut diagnostics = Vec::new();

    for slot in RootMenuC3dSlot::ALL {
        let identity = slot.identity();
        let path = slot.path(variant);
        match load_object(store, slot, identity, path, limits) {
            Ok(asset) => slots[slot.index()] = Some(asset),
            Err(error) => diagnostics.push(error),
        }
    }

    debug_assert_eq!(
        slots.iter().flatten().count() + diagnostics.len(),
        ROOT_MENU_C3D_REQUEST_COUNT
    );
    RootMenuC3dSet {
        variant,
        slots,
        diagnostics,
    }
}

fn load_object(
    store: &AssetStore,
    slot: RootMenuC3dSlot,
    identity: RootMenuObjectIdentity,
    path: &'static str,
    limits: C3dLimits,
) -> Result<RootMenuC3dAsset, RootMenuC3dLoadError> {
    let bytes = store
        .load(path)
        .map_err(|source| RootMenuC3dLoadError::Asset {
            slot,
            identity,
            path,
            source,
        })?;
    let file = parse_c3d(&bytes, limits).map_err(|source| RootMenuC3dLoadError::Decode {
        slot,
        identity,
        path,
        source,
    })?;
    Ok(RootMenuC3dAsset {
        slot,
        identity,
        path,
        file,
    })
}
