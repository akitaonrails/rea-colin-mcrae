use std::fmt;

use cmr_assets::{AssetStore, AssetStoreError};
use cmr_formats::bitmap::{BitmapError, BitmapLimits, parse_indexed_bitmap};
use cmr_formats::riff_palette::{LegacyPalette256, PaletteError, parse_legacy_palette_256};

use crate::startup::{Language, StartupScreen, select_startup_assets};
use crate::surface::{IndexedSurface, PaletteMapping, SurfaceError, indexed_surface};

#[derive(Debug)]
pub enum StartupSurfaceError {
    Asset {
        path: String,
        source: AssetStoreError,
    },
    Bitmap {
        path: String,
        source: BitmapError,
    },
    Palette {
        path: String,
        source: PaletteError,
    },
    Surface(SurfaceError),
}

impl fmt::Display for StartupSurfaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Asset { path, source } => write!(formatter, "cannot load {path}: {source}"),
            Self::Bitmap { path, source } => write!(formatter, "cannot parse {path}: {source}"),
            Self::Palette { path, source } => write!(formatter, "cannot parse {path}: {source}"),
            Self::Surface(error) => write!(formatter, "cannot create startup surface: {error}"),
        }
    }
}

impl std::error::Error for StartupSurfaceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Asset { source, .. } => Some(source),
            Self::Bitmap { source, .. } => Some(source),
            Self::Palette { source, .. } => Some(source),
            Self::Surface(error) => Some(error),
        }
    }
}

/// Loads and decodes one selected startup surface from a verified user import.
/// The mapping choice remains explicit until the recovered adapter-derived
/// global field has a trustworthy domain name and runtime contract.
pub fn load_startup_surface(
    store: &AssetStore,
    screen: StartupScreen,
    language: Language,
    mapping: PaletteMapping,
    bitmap_limits: BitmapLimits,
) -> Result<IndexedSurface, StartupSurfaceError> {
    let palette = load_startup_palette(store, screen, language)?;
    load_startup_bitmap_surface(store, screen, language, palette, mapping, bitmap_limits)
}

/// Loads and validates the selected 256-entry palette independently so a
/// display controller can apply it before touching the bitmap, as observed.
pub fn load_startup_palette(
    store: &AssetStore,
    screen: StartupScreen,
    language: Language,
) -> Result<LegacyPalette256, StartupSurfaceError> {
    let selected = select_startup_assets(screen, language);
    let palette_bytes =
        store
            .load(selected.palette)
            .map_err(|source| StartupSurfaceError::Asset {
                path: selected.palette.to_owned(),
                source,
            })?;
    parse_legacy_palette_256(&palette_bytes).map_err(|source| StartupSurfaceError::Palette {
        path: selected.palette.to_owned(),
        source,
    })
}

/// Loads the selected bitmap after its active palette has already been chosen.
pub fn load_startup_bitmap_surface(
    store: &AssetStore,
    screen: StartupScreen,
    language: Language,
    palette: LegacyPalette256,
    mapping: PaletteMapping,
    bitmap_limits: BitmapLimits,
) -> Result<IndexedSurface, StartupSurfaceError> {
    let selected = select_startup_assets(screen, language);
    let bitmap_bytes =
        store
            .load(&selected.bitmap)
            .map_err(|source| StartupSurfaceError::Asset {
                path: selected.bitmap.clone(),
                source,
            })?;
    let bitmap = parse_indexed_bitmap(&bitmap_bytes, bitmap_limits).map_err(|source| {
        StartupSurfaceError::Bitmap {
            path: selected.bitmap,
            source,
        }
    })?;
    indexed_surface(bitmap, palette, mapping).map_err(StartupSurfaceError::Surface)
}
