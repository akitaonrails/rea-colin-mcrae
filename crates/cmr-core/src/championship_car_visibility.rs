//! One-frame visibility latches for the 14 championship-preview cars.
//!
//! The selected native car is marked `0xff` after its transform update. The
//! terminal frame path clears every car back to zero after the scene attempt
//! and before deferred palette work and presentation.

use crate::championship_setup::CanonicalCar;

const CHAMPIONSHIP_CAR_COUNT: usize = CanonicalCar::ALL.len();

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChampionshipCarVisibilityMask(u8);

impl ChampionshipCarVisibilityMask {
    pub const HIDDEN: Self = Self(0x00);
    pub const ALL_LAYERS: Self = Self(0xff);

    #[must_use]
    pub const fn raw(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChampionshipCarVisibilityLatches {
    values: [ChampionshipCarVisibilityMask; CHAMPIONSHIP_CAR_COUNT],
}

impl ChampionshipCarVisibilityLatches {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            values: [ChampionshipCarVisibilityMask::HIDDEN; CHAMPIONSHIP_CAR_COUNT],
        }
    }

    #[must_use]
    pub const fn mask(&self, car: CanonicalCar) -> ChampionshipCarVisibilityMask {
        self.values[car.index() as usize]
    }

    pub fn show_all_layers(&mut self, car: CanonicalCar) {
        self.values[car.index() as usize] = ChampionshipCarVisibilityMask::ALL_LAYERS;
    }

    pub fn clear_all(&mut self) {
        self.values.fill(ChampionshipCarVisibilityMask::HIDDEN);
    }
}

impl Default for ChampionshipCarVisibilityLatches {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_canonical_car_starts_hidden_at_its_exact_index() {
        let latches = ChampionshipCarVisibilityLatches::new();

        for (expected_index, car) in CanonicalCar::ALL.into_iter().enumerate() {
            assert_eq!(car.index() as usize, expected_index);
            assert_eq!(latches.mask(car), ChampionshipCarVisibilityMask::HIDDEN);
            assert_eq!(latches.mask(car).raw(), 0x00);
        }
    }

    #[test]
    fn showing_each_canonical_car_changes_only_that_latch() {
        for selected in CanonicalCar::ALL {
            let mut latches = ChampionshipCarVisibilityLatches::new();
            latches.show_all_layers(selected);

            for car in CanonicalCar::ALL {
                let expected = if car == selected { 0xff } else { 0x00 };
                assert_eq!(latches.mask(car).raw(), expected);
            }
        }
    }

    #[test]
    fn clear_resets_all_canonical_car_latches() {
        let mut latches = ChampionshipCarVisibilityLatches::new();
        for car in CanonicalCar::ALL {
            latches.show_all_layers(car);
        }

        latches.clear_all();

        for car in CanonicalCar::ALL {
            assert_eq!(latches.mask(car), ChampionshipCarVisibilityMask::HIDDEN);
        }
    }
}
