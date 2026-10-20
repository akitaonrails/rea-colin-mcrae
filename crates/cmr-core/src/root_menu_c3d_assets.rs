//! Fixed-slot loading for the six optional root-menu C3D objects.
//!
//! This module stops at bounded structural admission of the three counted C3D
//! groups. Runtime object creation and rendering remain unrecovered boundaries
//! and must not be represented by indexed-surface handles.

use std::fmt;

use cmr_assets::{AssetStore, AssetStoreError};
use cmr_formats::c3d::{C3dError, C3dFile, C3dLimits, parse_c3d};

use crate::{
    frontend::RootSelection,
    root_menu_assets::{RootMenuObjectIdentity, VisualVariant},
};

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

    /// Maps the six object-bearing root selections to their fixed C3D slots.
    /// The seventh root entry has no corresponding object.
    #[must_use]
    pub const fn for_selection(selection: RootSelection) -> Option<Self> {
        match selection.index() {
            0 => Some(Self::Championship),
            1 => Some(Self::Rally),
            2 => Some(Self::TimeTrial),
            3 => Some(Self::RallySchool),
            4 => Some(Self::Options),
            5 => Some(Self::Load),
            6 => None,
            _ => unreachable!(),
        }
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

/// Persistent wrapping angle state for the six root-menu C3D slots.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RootMenuC3dAngles {
    values: [u16; ROOT_MENU_C3D_REQUEST_COUNT],
}

impl RootMenuC3dAngles {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            values: [0; ROOT_MENU_C3D_REQUEST_COUNT],
        }
    }

    #[must_use]
    pub const fn angle(&self, slot: RootMenuC3dSlot) -> u16 {
        self.values[slot.index()]
    }

    pub(crate) fn advance_loaded_slot(&mut self, slot: RootMenuC3dSlot, delta: u16) -> (u16, u16) {
        let previous_angle = self.values[slot.index()];
        let resulting_angle = previous_angle.wrapping_add(delta);
        self.values[slot.index()] = resulting_angle;
        (previous_angle, resulting_angle)
    }

    /// Advances one loaded object's angle and returns its exact frame binding.
    /// An empty slot leaves every persistent angle unchanged.
    #[must_use]
    pub fn advance_frame_object<'asset>(
        &mut self,
        assets: &'asset RootMenuC3dSet,
        slot: RootMenuC3dSlot,
        delta: u16,
    ) -> Option<RootMenuC3dFrameObject<'asset>> {
        let asset = assets.get(slot)?;
        let (previous_angle, resulting_angle) = self.advance_loaded_slot(slot, delta);
        Some(RootMenuC3dFrameObject {
            asset,
            slot,
            previous_angle,
            resulting_angle,
            delta,
        })
    }
}

impl Default for RootMenuC3dAngles {
    fn default() -> Self {
        Self::new()
    }
}

/// One frame's binding between a loaded root-menu object and its angle step.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RootMenuC3dFrameObject<'asset> {
    asset: &'asset RootMenuC3dAsset,
    slot: RootMenuC3dSlot,
    previous_angle: u16,
    resulting_angle: u16,
    delta: u16,
}

impl<'asset> RootMenuC3dFrameObject<'asset> {
    #[must_use]
    pub const fn asset(&self) -> &'asset RootMenuC3dAsset {
        self.asset
    }

    #[must_use]
    pub const fn slot(&self) -> RootMenuC3dSlot {
        self.slot
    }

    #[must_use]
    pub const fn previous_angle(&self) -> u16 {
        self.previous_angle
    }

    #[must_use]
    pub const fn resulting_angle(&self) -> u16 {
        self.resulting_angle
    }

    #[must_use]
    pub const fn delta(&self) -> u16 {
        self.delta
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

/// Builds a proprietary-free fixed-slot set for this crate's unit tests.
#[cfg(test)]
pub(crate) fn synthetic_root_menu_c3d_set(
    presence: [bool; ROOT_MENU_C3D_REQUEST_COUNT],
    variant: VisualVariant,
) -> RootMenuC3dSet {
    let slots = std::array::from_fn(|index| {
        presence[index].then(|| {
            let slot = RootMenuC3dSlot::ALL[index];
            RootMenuC3dAsset {
                slot,
                identity: slot.identity(),
                path: slot.path(variant),
                file: parse_c3d(&synthetic_c3d_bytes(index as u8), C3dLimits::default())
                    .expect("test C3D must parse"),
            }
        })
    });
    RootMenuC3dSet {
        variant,
        slots,
        diagnostics: Vec::new(),
    }
}

#[cfg(test)]
fn synthetic_c3d_bytes(marker: u8) -> Vec<u8> {
    use cmr_formats::c3d::{C3D_FORMAT_VERSION, C3D_MAGIC};

    let mut input = vec![0; 64];
    input[0..4].copy_from_slice(&C3D_MAGIC);
    input[4..8].copy_from_slice(&C3D_FORMAT_VERSION.to_le_bytes());
    input[8] = marker;
    for (index, count) in [0u16, 0, 1].into_iter().enumerate() {
        let start = 12 + index * 2;
        input[start..start + 2].copy_from_slice(&count.to_le_bytes());
    }
    for (index, offset) in [32u32, 40, 48].into_iter().enumerate() {
        let start = 20 + index * 4;
        input[start..start + 4].copy_from_slice(&offset.to_le_bytes());
    }
    input
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_root_selection_maps_explicitly_to_its_object_slot() {
        for (value, expected) in RootMenuC3dSlot::ALL.into_iter().enumerate() {
            let selection = RootSelection::new(value as u8).unwrap();
            assert_eq!(RootMenuC3dSlot::for_selection(selection), Some(expected));
        }
        assert_eq!(
            RootMenuC3dSlot::for_selection(RootSelection::new(6).unwrap()),
            None
        );
    }

    #[test]
    fn angles_start_at_zero_and_advance_independently() {
        let assets = set_with_asset(RootMenuC3dSlot::Rally);
        let mut angles = RootMenuC3dAngles::new();
        assert!(
            RootMenuC3dSlot::ALL
                .into_iter()
                .all(|slot| angles.angle(slot) == 0)
        );

        angles
            .advance_frame_object(&assets, RootMenuC3dSlot::Rally, 9)
            .unwrap();

        for slot in RootMenuC3dSlot::ALL {
            assert_eq!(
                angles.angle(slot),
                if slot == RootMenuC3dSlot::Rally { 9 } else { 0 }
            );
        }
    }

    #[test]
    fn missing_slot_returns_none_without_mutating_any_angle() {
        let assets = set_with_asset(RootMenuC3dSlot::Championship);
        let mut angles = RootMenuC3dAngles::new();
        angles
            .advance_frame_object(&assets, RootMenuC3dSlot::Championship, 17)
            .unwrap();
        let before = angles.clone();

        assert_eq!(
            angles.advance_frame_object(&assets, RootMenuC3dSlot::Rally, u16::MAX),
            None
        );
        assert_eq!(angles, before);
    }

    #[test]
    fn angle_advancement_wraps_at_u16_boundary() {
        let slot = RootMenuC3dSlot::Options;
        let assets = set_with_asset(slot);
        let mut angles = RootMenuC3dAngles::new();
        angles
            .advance_frame_object(&assets, slot, u16::MAX)
            .unwrap();

        let binding = angles.advance_frame_object(&assets, slot, 2).unwrap();

        assert_eq!(binding.previous_angle(), u16::MAX);
        assert_eq!(binding.resulting_angle(), 1);
        assert_eq!(binding.delta(), 2);
        assert_eq!(angles.angle(slot), 1);
    }

    #[test]
    fn frame_binding_borrows_the_exact_asset_and_retains_the_slot() {
        let slot = RootMenuC3dSlot::TimeTrial;
        let assets = set_with_asset(slot);
        let expected_asset = assets.get(slot).unwrap();
        let mut angles = RootMenuC3dAngles::new();

        let binding = angles.advance_frame_object(&assets, slot, 8).unwrap();

        assert!(std::ptr::eq(binding.asset(), expected_asset));
        assert_eq!(binding.slot(), slot);
        assert_eq!(binding.previous_angle(), 0);
        assert_eq!(binding.resulting_angle(), 8);
        assert_eq!(binding.delta(), 8);
    }

    fn set_with_asset(slot: RootMenuC3dSlot) -> RootMenuC3dSet {
        let mut presence = [false; ROOT_MENU_C3D_REQUEST_COUNT];
        presence[slot.index()] = true;
        synthetic_root_menu_c3d_set(presence, VisualVariant::Normal)
    }
}
