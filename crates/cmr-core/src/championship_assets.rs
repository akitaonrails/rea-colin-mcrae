//! Required asset bundle for the recovered championship car-selection screen.
//!
//! Font, localization, selection-surface, palette scheduling, and rendering
//! ownership remain with the enclosing frontend session. This module only
//! admits the 14 car objects, their 14 palettes, and the 11 unique logos.

use std::fmt;

use cmr_assets::{AssetStore, AssetStoreError};
use cmr_formats::{
    bitmap::{BitmapError, BitmapLimits, IndexedBitmap, parse_indexed_bitmap},
    c3d::{C3dError, C3dFile, C3dLimits, parse_c3d},
    riff_palette::{LegacyPalette256, PaletteError, parse_legacy_palette_256},
};

use crate::championship_setup::CanonicalCar;

pub const CHAMPIONSHIP_CAR_COUNT: usize = CanonicalCar::ALL.len();
pub const MANUFACTURER_LOGO_COUNT: usize = ManufacturerLogo::ALL.len();

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u8)]
pub enum ManufacturerLogo {
    Subaru,
    Seat,
    Skoda,
    Audi,
    Ford,
    Mitsu,
    Vw,
    Lancia,
    Toyo,
    Reno,
    M4r,
}

impl ManufacturerLogo {
    pub const ALL: [Self; 11] = [
        Self::Subaru,
        Self::Seat,
        Self::Skoda,
        Self::Audi,
        Self::Ford,
        Self::Mitsu,
        Self::Vw,
        Self::Lancia,
        Self::Toyo,
        Self::Reno,
        Self::M4r,
    ];

    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    #[must_use]
    pub const fn descriptor(self) -> ManufacturerLogoDescriptor {
        MANUFACTURER_LOGOS[self.index()]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ManufacturerLogoDescriptor {
    pub identity: ManufacturerLogo,
    pub path: &'static str,
}

pub const MANUFACTURER_LOGOS: [ManufacturerLogoDescriptor; 11] = [
    ManufacturerLogoDescriptor {
        identity: ManufacturerLogo::Subaru,
        path: "New_Fe/Images/carlogos/Subaru.bmp",
    },
    ManufacturerLogoDescriptor {
        identity: ManufacturerLogo::Seat,
        path: "New_Fe/Images/carlogos/Seat.bmp",
    },
    ManufacturerLogoDescriptor {
        identity: ManufacturerLogo::Skoda,
        path: "New_Fe/Images/carlogos/Skoda.bmp",
    },
    ManufacturerLogoDescriptor {
        identity: ManufacturerLogo::Audi,
        path: "New_Fe/Images/carlogos/Audi.bmp",
    },
    ManufacturerLogoDescriptor {
        identity: ManufacturerLogo::Ford,
        path: "New_Fe/Images/carlogos/Ford.bmp",
    },
    ManufacturerLogoDescriptor {
        identity: ManufacturerLogo::Mitsu,
        path: "New_Fe/Images/carlogos/Mitsu.bmp",
    },
    ManufacturerLogoDescriptor {
        identity: ManufacturerLogo::Vw,
        path: "New_Fe/Images/carlogos/Vw.bmp",
    },
    ManufacturerLogoDescriptor {
        identity: ManufacturerLogo::Lancia,
        path: "New_Fe/Images/carlogos/Lancia.bmp",
    },
    ManufacturerLogoDescriptor {
        identity: ManufacturerLogo::Toyo,
        path: "New_Fe/Images/carlogos/Toyo.bmp",
    },
    ManufacturerLogoDescriptor {
        identity: ManufacturerLogo::Reno,
        path: "New_Fe/Images/carlogos/Reno.bmp",
    },
    ManufacturerLogoDescriptor {
        identity: ManufacturerLogo::M4r,
        path: "New_Fe/Images/carlogos/M4R.bmp",
    },
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChampionshipCarDescriptor {
    pub car: CanonicalCar,
    pub object_path: &'static str,
    pub palette_path: &'static str,
    pub logo: ManufacturerLogo,
}

pub const CHAMPIONSHIP_CARS: [ChampionshipCarDescriptor; CHAMPIONSHIP_CAR_COUNT] = [
    ChampionshipCarDescriptor {
        car: CanonicalCar::Subaru,
        object_path: "New_Fe/Objects/Cars/Subaru.c3d",
        palette_path: "New_Fe/Palettes/Cars/Subaru.pal",
        logo: ManufacturerLogo::Subaru,
    },
    ChampionshipCarDescriptor {
        car: CanonicalCar::Ibiza,
        object_path: "New_Fe/Objects/Cars/Ibiza.c3d",
        palette_path: "New_Fe/Palettes/Cars/Ibiza.pal",
        logo: ManufacturerLogo::Seat,
    },
    ChampionshipCarDescriptor {
        car: CanonicalCar::Skoda,
        object_path: "New_Fe/Objects/Cars/Skoda.C3D",
        palette_path: "New_Fe/Palettes/Cars/Skoda.pal",
        logo: ManufacturerLogo::Skoda,
    },
    ChampionshipCarDescriptor {
        car: CanonicalCar::Quattro,
        object_path: "New_Fe/Objects/Cars/Quattro.c3d",
        palette_path: "New_Fe/Palettes/Cars/Quattro.pal",
        logo: ManufacturerLogo::Audi,
    },
    ChampionshipCarDescriptor {
        car: CanonicalCar::Rs200,
        object_path: "New_Fe/Objects/Cars/RS200.c3d",
        palette_path: "New_Fe/Palettes/Cars/RS200.pal",
        logo: ManufacturerLogo::Ford,
    },
    ChampionshipCarDescriptor {
        car: CanonicalCar::Mk2,
        object_path: "New_Fe/Objects/Cars/MK2.c3d",
        palette_path: "New_Fe/Palettes/Cars/MK2.pal",
        logo: ManufacturerLogo::Ford,
    },
    ChampionshipCarDescriptor {
        car: CanonicalCar::Mitsubis,
        object_path: "New_Fe/Objects/Cars/Mitsubis.c3d",
        palette_path: "New_Fe/Palettes/Cars/Mitsubis.pal",
        logo: ManufacturerLogo::Mitsu,
    },
    ChampionshipCarDescriptor {
        car: CanonicalCar::Golf,
        object_path: "New_Fe/Objects/Cars/Golf.c3d",
        palette_path: "New_Fe/Palettes/Cars/Golf.pal",
        logo: ManufacturerLogo::Vw,
    },
    ChampionshipCarDescriptor {
        car: CanonicalCar::Integral,
        object_path: "New_Fe/Objects/Cars/Integral.c3d",
        palette_path: "New_Fe/Palettes/Cars/Integral.pal",
        logo: ManufacturerLogo::Lancia,
    },
    ChampionshipCarDescriptor {
        car: CanonicalCar::Corolla,
        object_path: "New_Fe/Objects/Cars/Corolla.C3D",
        palette_path: "New_Fe/Palettes/Cars/Corolla.pal",
        logo: ManufacturerLogo::Toyo,
    },
    ChampionshipCarDescriptor {
        car: CanonicalCar::Escort,
        object_path: "New_Fe/Objects/Cars/Escort.c3d",
        palette_path: "New_Fe/Palettes/Cars/Escort.pal",
        logo: ManufacturerLogo::Ford,
    },
    ChampionshipCarDescriptor {
        car: CanonicalCar::Megane,
        object_path: "New_Fe/Objects/Cars/Megane.C3D",
        palette_path: "New_Fe/Palettes/Cars/Megane.pal",
        logo: ManufacturerLogo::Reno,
    },
    ChampionshipCarDescriptor {
        car: CanonicalCar::M4r,
        object_path: "New_Fe/Objects/Cars/M4R.c3d",
        palette_path: "New_Fe/Palettes/Cars/M4R.pal",
        logo: ManufacturerLogo::M4r,
    },
    ChampionshipCarDescriptor {
        car: CanonicalCar::Celica,
        object_path: "New_Fe/Objects/Cars/Celica.c3d",
        palette_path: "New_Fe/Palettes/Cars/Celica.pal",
        logo: ManufacturerLogo::Toyo,
    },
];

impl CanonicalCar {
    #[must_use]
    pub const fn championship_descriptor(self) -> ChampionshipCarDescriptor {
        CHAMPIONSHIP_CARS[self.index() as usize]
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChampionshipCarAsset {
    pub descriptor: ChampionshipCarDescriptor,
    pub object: C3dFile,
    pub palette: LegacyPalette256,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManufacturerLogoAsset {
    pub descriptor: ManufacturerLogoDescriptor,
    pub bitmap: IndexedBitmap,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChampionshipAssets {
    cars: [ChampionshipCarAsset; CHAMPIONSHIP_CAR_COUNT],
    logos: [ManufacturerLogoAsset; MANUFACTURER_LOGO_COUNT],
}

impl ChampionshipAssets {
    #[must_use]
    pub fn car(&self, car: CanonicalCar) -> &ChampionshipCarAsset {
        &self.cars[car.index() as usize]
    }

    pub fn cars(&self) -> impl ExactSizeIterator<Item = &ChampionshipCarAsset> {
        self.cars.iter()
    }

    #[must_use]
    pub fn logo(&self, logo: ManufacturerLogo) -> &ManufacturerLogoAsset {
        &self.logos[logo.index()]
    }

    #[must_use]
    pub fn logo_for_car(&self, car: CanonicalCar) -> &ManufacturerLogoAsset {
        self.logo(self.car(car).descriptor.logo)
    }

    pub fn logos(&self) -> impl ExactSizeIterator<Item = &ManufacturerLogoAsset> {
        self.logos.iter()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChampionshipAssetRole {
    CarObject(CanonicalCar),
    CarPalette(CanonicalCar),
    ManufacturerLogo(ManufacturerLogo),
}

#[derive(Debug)]
pub enum ChampionshipAssetLoadError {
    Asset {
        role: ChampionshipAssetRole,
        path: &'static str,
        source: AssetStoreError,
    },
    Object {
        car: CanonicalCar,
        path: &'static str,
        source: C3dError,
    },
    Palette {
        car: CanonicalCar,
        path: &'static str,
        source: PaletteError,
    },
    Logo {
        logo: ManufacturerLogo,
        path: &'static str,
        source: BitmapError,
    },
}

impl ChampionshipAssetLoadError {
    #[must_use]
    pub const fn role(&self) -> ChampionshipAssetRole {
        match self {
            Self::Asset { role, .. } => *role,
            Self::Object { car, .. } => ChampionshipAssetRole::CarObject(*car),
            Self::Palette { car, .. } => ChampionshipAssetRole::CarPalette(*car),
            Self::Logo { logo, .. } => ChampionshipAssetRole::ManufacturerLogo(*logo),
        }
    }

    #[must_use]
    pub const fn path(&self) -> &'static str {
        match self {
            Self::Asset { path, .. }
            | Self::Object { path, .. }
            | Self::Palette { path, .. }
            | Self::Logo { path, .. } => path,
        }
    }
}

impl fmt::Display for ChampionshipAssetLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Asset { role, path, source } => {
                write!(
                    formatter,
                    "cannot load championship asset {role:?} at {path}: {source}"
                )
            }
            Self::Object { car, path, source } => {
                write!(
                    formatter,
                    "cannot decode championship car {car:?} at {path}: {source}"
                )
            }
            Self::Palette { car, path, source } => write!(
                formatter,
                "cannot decode championship car palette {car:?} at {path}: {source}"
            ),
            Self::Logo { logo, path, source } => write!(
                formatter,
                "cannot decode championship manufacturer logo {logo:?} at {path}: {source}"
            ),
        }
    }
}

impl std::error::Error for ChampionshipAssetLoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Asset { source, .. } => Some(source),
            Self::Object { source, .. } => Some(source),
            Self::Palette { source, .. } => Some(source),
            Self::Logo { source, .. } => Some(source),
        }
    }
}

/// Loads the mandatory championship car-selection assets in canonical order.
///
/// Manufacturer logos are loaded once per unique identity, then referenced by
/// the per-car descriptor. Any missing or malformed required asset aborts the
/// bundle without substituting a different car or manufacturer.
pub fn load_championship_assets(
    store: &AssetStore,
    c3d_limits: C3dLimits,
    bitmap_limits: BitmapLimits,
) -> Result<ChampionshipAssets, ChampionshipAssetLoadError> {
    let mut cars = Vec::with_capacity(CHAMPIONSHIP_CAR_COUNT);
    for descriptor in CHAMPIONSHIP_CARS {
        let object_bytes = load_bytes(
            store,
            ChampionshipAssetRole::CarObject(descriptor.car),
            descriptor.object_path,
        )?;
        let object = parse_c3d(&object_bytes, c3d_limits).map_err(|source| {
            ChampionshipAssetLoadError::Object {
                car: descriptor.car,
                path: descriptor.object_path,
                source,
            }
        })?;

        let palette_bytes = load_bytes(
            store,
            ChampionshipAssetRole::CarPalette(descriptor.car),
            descriptor.palette_path,
        )?;
        let palette = parse_legacy_palette_256(&palette_bytes).map_err(|source| {
            ChampionshipAssetLoadError::Palette {
                car: descriptor.car,
                path: descriptor.palette_path,
                source,
            }
        })?;
        cars.push(ChampionshipCarAsset {
            descriptor,
            object,
            palette,
        });
    }

    let mut logos = Vec::with_capacity(MANUFACTURER_LOGO_COUNT);
    for descriptor in MANUFACTURER_LOGOS {
        let bytes = load_bytes(
            store,
            ChampionshipAssetRole::ManufacturerLogo(descriptor.identity),
            descriptor.path,
        )?;
        let bitmap = parse_indexed_bitmap(&bytes, bitmap_limits).map_err(|source| {
            ChampionshipAssetLoadError::Logo {
                logo: descriptor.identity,
                path: descriptor.path,
                source,
            }
        })?;
        logos.push(ManufacturerLogoAsset { descriptor, bitmap });
    }

    Ok(ChampionshipAssets {
        cars: cars
            .try_into()
            .expect("one championship asset was loaded for every canonical car"),
        logos: logos
            .try_into()
            .expect("one championship logo was loaded for every unique identity"),
    })
}

fn load_bytes(
    store: &AssetStore,
    role: ChampionshipAssetRole,
    path: &'static str,
) -> Result<Vec<u8>, ChampionshipAssetLoadError> {
    store
        .load(path)
        .map_err(|source| ChampionshipAssetLoadError::Asset { role, path, source })
}
