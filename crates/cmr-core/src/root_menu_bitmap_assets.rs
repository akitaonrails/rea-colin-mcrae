//! Loading boundary for root-menu indexed bitmaps.
//!
//! Rendering and C3D decoding remain outside this module. Object identities
//! and paths continue to live in `root_menu_assets` for a later C3D slice.

use std::fmt;

use cmr_assets::{AssetStore, AssetStoreError};
use cmr_formats::bitmap::{BitmapError, BitmapLimits, IndexedBitmap, parse_indexed_bitmap};

use crate::localization::Language;
use crate::root_menu_assets::{
    CenterIconIdentity, ROOT_MENU_ANIMATION_FRAMES, ROOT_MENU_BACKDROP_PATH, ROOT_MENU_SELECT_PATH,
    VisualVariant, localized_root_menu_assets,
};

pub const ROOT_MENU_OPTIONAL_BITMAP_REQUEST_COUNT: usize = 2 + ROOT_MENU_ANIMATION_FRAMES;
pub const ROOT_MENU_REQUIRED_BITMAP_REQUEST_COUNT: usize = 2 + CenterIconIdentity::ALL.len();
pub const ROOT_MENU_BITMAP_REQUEST_COUNT: usize =
    ROOT_MENU_OPTIONAL_BITMAP_REQUEST_COUNT + ROOT_MENU_REQUIRED_BITMAP_REQUEST_COUNT;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RootMenuBitmapIdentity {
    MainSelected,
    MainShadow,
    AnimationFrame(u8),
    Backdrop,
    Selection,
    CenterIcon {
        identity: CenterIconIdentity,
        variant: VisualVariant,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RootMenuBitmapAsset {
    pub identity: RootMenuBitmapIdentity,
    pub path: &'static str,
    pub bitmap: IndexedBitmap,
}

#[derive(Debug)]
pub enum RootMenuBitmapLoadError {
    Asset {
        identity: RootMenuBitmapIdentity,
        path: &'static str,
        source: AssetStoreError,
    },
    Decode {
        identity: RootMenuBitmapIdentity,
        path: &'static str,
        source: BitmapError,
    },
}

impl RootMenuBitmapLoadError {
    pub const fn identity(&self) -> RootMenuBitmapIdentity {
        match self {
            Self::Asset { identity, .. } | Self::Decode { identity, .. } => *identity,
        }
    }

    pub const fn path(&self) -> &'static str {
        match self {
            Self::Asset { path, .. } | Self::Decode { path, .. } => path,
        }
    }
}

impl fmt::Display for RootMenuBitmapLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Asset {
                identity,
                path,
                source,
            } => write!(
                formatter,
                "cannot load root-menu bitmap {identity:?} at {path}: {source}"
            ),
            Self::Decode {
                identity,
                path,
                source,
            } => write!(
                formatter,
                "cannot decode root-menu bitmap {identity:?} at {path}: {source}"
            ),
        }
    }
}

impl std::error::Error for RootMenuBitmapLoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Asset { source, .. } => Some(source),
            Self::Decode { source, .. } => Some(source),
        }
    }
}

#[derive(Debug)]
pub struct RootMenuBitmapSet {
    pub language: Language,
    pub center_variant: VisualVariant,
    assets: Vec<RootMenuBitmapAsset>,
    optional_diagnostics: Vec<RootMenuBitmapLoadError>,
}

impl RootMenuBitmapSet {
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &RootMenuBitmapAsset> {
        self.assets.iter()
    }

    pub fn optional_diagnostics(&self) -> impl ExactSizeIterator<Item = &RootMenuBitmapLoadError> {
        self.optional_diagnostics.iter()
    }

    pub fn get(&self, identity: RootMenuBitmapIdentity) -> Option<&IndexedBitmap> {
        self.asset(identity).map(|asset| &asset.bitmap)
    }

    pub fn asset(&self, identity: RootMenuBitmapIdentity) -> Option<&RootMenuBitmapAsset> {
        self.assets.iter().find(|asset| asset.identity == identity)
    }

    pub fn loaded_len(&self) -> usize {
        self.assets.len()
    }

    pub fn optional_diagnostic_len(&self) -> usize {
        self.optional_diagnostics.len()
    }
}

/// Loads the root-menu bitmap requests in recovered order.
///
/// Localized main-option and animation bitmaps are optional. Their asset and
/// safe-decoder failures are retained as diagnostics while loading continues.
/// The backdrop, selection bitmap, and center icons are mandatory.
pub fn load_root_menu_bitmaps(
    store: &AssetStore,
    language: Language,
    center_variant: VisualVariant,
    limits: BitmapLimits,
) -> Result<RootMenuBitmapSet, RootMenuBitmapLoadError> {
    let localized = localized_root_menu_assets(language);
    let mut assets = Vec::with_capacity(ROOT_MENU_BITMAP_REQUEST_COUNT);
    let mut optional_diagnostics = Vec::new();

    let optional_requests = std::iter::once((
        RootMenuBitmapIdentity::MainSelected,
        localized.main_selected,
    ))
    .chain(std::iter::once((
        RootMenuBitmapIdentity::MainShadow,
        localized.main_shadow,
    )))
    .chain(
        localized
            .animation
            .into_iter()
            .enumerate()
            .map(|(index, path)| (RootMenuBitmapIdentity::AnimationFrame(index as u8), path)),
    );
    for (identity, path) in optional_requests {
        match load_bitmap(store, identity, path, limits) {
            Ok(asset) => assets.push(asset),
            Err(error) => optional_diagnostics.push(error),
        }
    }

    assets.push(load_bitmap(
        store,
        RootMenuBitmapIdentity::Backdrop,
        ROOT_MENU_BACKDROP_PATH,
        limits,
    )?);
    assets.push(load_bitmap(
        store,
        RootMenuBitmapIdentity::Selection,
        ROOT_MENU_SELECT_PATH,
        limits,
    )?);
    for identity in CenterIconIdentity::ALL {
        assets.push(load_bitmap(
            store,
            RootMenuBitmapIdentity::CenterIcon {
                identity,
                variant: center_variant,
            },
            identity.path(center_variant),
            limits,
        )?);
    }

    debug_assert_eq!(
        assets.len() + optional_diagnostics.len(),
        ROOT_MENU_BITMAP_REQUEST_COUNT
    );
    Ok(RootMenuBitmapSet {
        language,
        center_variant,
        assets,
        optional_diagnostics,
    })
}

fn load_bitmap(
    store: &AssetStore,
    identity: RootMenuBitmapIdentity,
    path: &'static str,
    limits: BitmapLimits,
) -> Result<RootMenuBitmapAsset, RootMenuBitmapLoadError> {
    let bytes = store
        .load(path)
        .map_err(|source| RootMenuBitmapLoadError::Asset {
            identity,
            path,
            source,
        })?;
    let bitmap =
        parse_indexed_bitmap(&bytes, limits).map_err(|source| RootMenuBitmapLoadError::Decode {
            identity,
            path,
            source,
        })?;
    Ok(RootMenuBitmapAsset {
        identity,
        path,
        bitmap,
    })
}
