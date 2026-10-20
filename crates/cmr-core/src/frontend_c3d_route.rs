//! Shared ownership of the frontend C3D backend route.
//!
//! Root-menu and championship-car submission retain independent selector
//! caches, but both mutate the same backend material and texture state.  A
//! route transition therefore makes the cache owned by the route being
//! entered uncertain, while repeated frames on one route may keep it.

use std::cell::Cell;

/// Frontend path currently responsible for the shared C3D backend state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendC3dRoute {
    RootMenu,
    ChampionshipCar,
}

/// Session-scoped route state shared by every frontend C3D pipeline.
#[derive(Debug, Default)]
pub struct FrontendC3dRouteState {
    active: Cell<Option<FrontendC3dRoute>>,
}

impl FrontendC3dRouteState {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            active: Cell::new(None),
        }
    }

    /// Enters `route`, returning `true` when its selector cache is uncertain.
    ///
    /// The first entry is treated as a transition because backend binding
    /// state predating this session is unknown. Re-entering the active route
    /// is the only case in which a retained selector cache remains valid.
    pub fn enter(&self, route: FrontendC3dRoute) -> bool {
        let changed = self.active.get() != Some(route);
        self.active.set(Some(route));
        changed
    }

    #[must_use]
    pub fn active(&self) -> Option<FrontendC3dRoute> {
        self.active.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_entry_and_route_switches_require_invalidation() {
        let state = FrontendC3dRouteState::new();

        assert_eq!(state.active(), None);
        assert!(state.enter(FrontendC3dRoute::RootMenu));
        assert_eq!(state.active(), Some(FrontendC3dRoute::RootMenu));
        assert!(!state.enter(FrontendC3dRoute::RootMenu));
        assert!(state.enter(FrontendC3dRoute::ChampionshipCar));
        assert!(!state.enter(FrontendC3dRoute::ChampionshipCar));
        assert!(state.enter(FrontendC3dRoute::RootMenu));
    }
}
