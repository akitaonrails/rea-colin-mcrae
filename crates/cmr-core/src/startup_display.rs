use std::fmt;

use cmr_assets::AssetStore;
use cmr_formats::bitmap::BitmapLimits;

use crate::display::{
    BackendFailure, CooperativeMode, DisplayInitError, IndexedDisplay, IndexedMode,
    IndexedVideoBackend, StartupCopyError, StartupCopyOutcome,
};
use crate::startup::{StartupLanguage, StartupScreen};
use crate::startup_surface::{
    StartupSurfaceError, load_startup_bitmap_surface, load_startup_palette,
};
use crate::surface::PaletteMapping;

#[derive(Debug)]
pub enum StartupDisplayError<E> {
    Asset(StartupSurfaceError),
    Initialize(DisplayInitError<E>),
    ApplyPalette(BackendFailure<E>),
    Copy(StartupCopyError<E>),
}

impl<E: fmt::Display> fmt::Display for StartupDisplayError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Asset(error) => error.fmt(formatter),
            Self::Initialize(error) => error.fmt(formatter),
            Self::ApplyPalette(error) => error.fmt(formatter),
            Self::Copy(error) => error.fmt(formatter),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for StartupDisplayError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Asset(error) => Some(error),
            Self::Initialize(error) => Some(error),
            Self::ApplyPalette(error) => Some(error),
            Self::Copy(error) => Some(error),
        }
    }
}

pub struct StartupDisplay<B: IndexedVideoBackend> {
    pub display: IndexedDisplay<B>,
    pub copy: StartupCopyOutcome<B::Error>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StartupDisplayRequest {
    pub mode: IndexedMode,
    pub cooperative: CooperativeMode,
    pub screen: StartupScreen,
    pub language: StartupLanguage,
    pub mapping: PaletteMapping,
    pub bitmap_limits: BitmapLimits,
}

/// Reconstructs the recovered startup transaction across verified assets and
/// the platform video boundary. The selected palette is attached during
/// display initialization and explicitly applied again before the bitmap is
/// loaded, then the temporary image is copied to visible-front and back.
pub fn initialize_startup_display<B: IndexedVideoBackend>(
    store: &AssetStore,
    backend: B,
    request: StartupDisplayRequest,
) -> Result<StartupDisplay<B>, StartupDisplayError<B::Error>> {
    let palette = load_startup_palette(store, request.screen, request.language)
        .map_err(StartupDisplayError::Asset)?;
    let mut display =
        IndexedDisplay::initialize(backend, request.mode, request.cooperative, &palette.entries)
            .map_err(StartupDisplayError::Initialize)?;
    display
        .upload_palette(&palette.entries)
        .map_err(StartupDisplayError::ApplyPalette)?;
    let surface = load_startup_bitmap_surface(
        store,
        request.screen,
        request.language,
        palette,
        request.mapping,
        request.bitmap_limits,
    )
    .map_err(StartupDisplayError::Asset)?;
    let copy = display
        .show_startup_surface(&surface)
        .map_err(StartupDisplayError::Copy)?;
    Ok(StartupDisplay { display, copy })
}
