//! One-frame visibility latches for the six root-menu C3D objects.
//!
//! The selected native actor is marked `0xff` after its transform update. The
//! terminal frame path clears every root actor back to zero after the scene
//! attempt and before deferred palette work and presentation.

use crate::root_menu_c3d_assets::{ROOT_MENU_C3D_REQUEST_COUNT, RootMenuC3dSlot};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RootMenuC3dVisibilityMask(u8);

impl RootMenuC3dVisibilityMask {
    pub const HIDDEN: Self = Self(0x00);
    pub const ALL_LAYERS: Self = Self(0xff);

    #[must_use]
    pub const fn raw(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RootMenuC3dVisibilityLatches {
    values: [RootMenuC3dVisibilityMask; ROOT_MENU_C3D_REQUEST_COUNT],
}

impl RootMenuC3dVisibilityLatches {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            values: [RootMenuC3dVisibilityMask::HIDDEN; ROOT_MENU_C3D_REQUEST_COUNT],
        }
    }

    #[must_use]
    pub const fn mask(&self, slot: RootMenuC3dSlot) -> RootMenuC3dVisibilityMask {
        self.values[slot.index()]
    }

    pub(crate) fn show_all_layers(&mut self, slot: RootMenuC3dSlot) {
        self.values[slot.index()] = RootMenuC3dVisibilityMask::ALL_LAYERS;
    }

    /// Consumes every one-frame root visibility latch after a terminal scene
    /// attempt, including when an alternate scene owner shares the backend.
    pub fn clear_all(&mut self) {
        self.values.fill(RootMenuC3dVisibilityMask::HIDDEN);
    }
}

impl Default for RootMenuC3dVisibilityLatches {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_slot_can_be_shown_then_all_slots_are_cleared() {
        let mut latches = RootMenuC3dVisibilityLatches::new();
        for slot in RootMenuC3dSlot::ALL {
            assert_eq!(latches.mask(slot).raw(), 0x00);
        }

        latches.show_all_layers(RootMenuC3dSlot::RallySchool);
        for slot in RootMenuC3dSlot::ALL {
            let expected = if slot == RootMenuC3dSlot::RallySchool {
                0xff
            } else {
                0x00
            };
            assert_eq!(latches.mask(slot).raw(), expected);
        }

        latches.clear_all();
        for slot in RootMenuC3dSlot::ALL {
            assert_eq!(latches.mask(slot), RootMenuC3dVisibilityMask::HIDDEN);
        }
    }
}
