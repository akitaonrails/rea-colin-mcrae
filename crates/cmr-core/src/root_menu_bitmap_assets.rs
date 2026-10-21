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

/// Fixed request slot used by the original root-menu bitmap batch.
///
/// Slots never compact when an optional bitmap is absent. In particular, the
/// two G-prefixed globe requests remain separate even though they resolve to
/// the same logical path in that variant.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u8)]
pub enum RootMenuBitmapSlot {
    MainSelected,
    MainShadow,
    Animation0,
    Animation1,
    Animation2,
    Animation3,
    Animation4,
    Animation5,
    Animation6,
    Backdrop,
    Selection,
    ChampionshipGlobe,
    TimeTrialGlobe,
    Cone,
    Spanner,
    Disk,
    Quit,
}

impl RootMenuBitmapSlot {
    pub const ALL: [Self; ROOT_MENU_BITMAP_REQUEST_COUNT] = [
        Self::MainSelected,
        Self::MainShadow,
        Self::Animation0,
        Self::Animation1,
        Self::Animation2,
        Self::Animation3,
        Self::Animation4,
        Self::Animation5,
        Self::Animation6,
        Self::Backdrop,
        Self::Selection,
        Self::ChampionshipGlobe,
        Self::TimeTrialGlobe,
        Self::Cone,
        Self::Spanner,
        Self::Disk,
        Self::Quit,
    ];

    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    #[must_use]
    pub const fn is_optional(self) -> bool {
        self.index() < ROOT_MENU_OPTIONAL_BITMAP_REQUEST_COUNT
    }

    #[must_use]
    pub const fn identity(self, variant: VisualVariant) -> RootMenuBitmapIdentity {
        match self {
            Self::MainSelected => RootMenuBitmapIdentity::MainSelected,
            Self::MainShadow => RootMenuBitmapIdentity::MainShadow,
            Self::Animation0 => RootMenuBitmapIdentity::AnimationFrame(0),
            Self::Animation1 => RootMenuBitmapIdentity::AnimationFrame(1),
            Self::Animation2 => RootMenuBitmapIdentity::AnimationFrame(2),
            Self::Animation3 => RootMenuBitmapIdentity::AnimationFrame(3),
            Self::Animation4 => RootMenuBitmapIdentity::AnimationFrame(4),
            Self::Animation5 => RootMenuBitmapIdentity::AnimationFrame(5),
            Self::Animation6 => RootMenuBitmapIdentity::AnimationFrame(6),
            Self::Backdrop => RootMenuBitmapIdentity::Backdrop,
            Self::Selection => RootMenuBitmapIdentity::Selection,
            Self::ChampionshipGlobe => RootMenuBitmapIdentity::CenterIcon {
                identity: CenterIconIdentity::ChampionshipGlobe,
                variant,
            },
            Self::TimeTrialGlobe => RootMenuBitmapIdentity::CenterIcon {
                identity: CenterIconIdentity::TimeTrialGlobe,
                variant,
            },
            Self::Cone => RootMenuBitmapIdentity::CenterIcon {
                identity: CenterIconIdentity::Cone,
                variant,
            },
            Self::Spanner => RootMenuBitmapIdentity::CenterIcon {
                identity: CenterIconIdentity::Spanner,
                variant,
            },
            Self::Disk => RootMenuBitmapIdentity::CenterIcon {
                identity: CenterIconIdentity::Disk,
                variant,
            },
            Self::Quit => RootMenuBitmapIdentity::CenterIcon {
                identity: CenterIconIdentity::Quit,
                variant,
            },
        }
    }

    #[must_use]
    pub const fn path(self, language: Language, variant: VisualVariant) -> &'static str {
        let localized = localized_root_menu_assets(language);
        match self {
            Self::MainSelected => localized.main_selected,
            Self::MainShadow => localized.main_shadow,
            Self::Animation0 => localized.animation[0],
            Self::Animation1 => localized.animation[1],
            Self::Animation2 => localized.animation[2],
            Self::Animation3 => localized.animation[3],
            Self::Animation4 => localized.animation[4],
            Self::Animation5 => localized.animation[5],
            Self::Animation6 => localized.animation[6],
            Self::Backdrop => ROOT_MENU_BACKDROP_PATH,
            Self::Selection => ROOT_MENU_SELECT_PATH,
            Self::ChampionshipGlobe => CenterIconIdentity::ChampionshipGlobe.path(variant),
            Self::TimeTrialGlobe => CenterIconIdentity::TimeTrialGlobe.path(variant),
            Self::Cone => CenterIconIdentity::Cone.path(variant),
            Self::Spanner => CenterIconIdentity::Spanner.path(variant),
            Self::Disk => CenterIconIdentity::Disk.path(variant),
            Self::Quit => CenterIconIdentity::Quit.path(variant),
        }
    }

    #[must_use]
    pub const fn from_identity(identity: RootMenuBitmapIdentity) -> Option<Self> {
        match identity {
            RootMenuBitmapIdentity::MainSelected => Some(Self::MainSelected),
            RootMenuBitmapIdentity::MainShadow => Some(Self::MainShadow),
            RootMenuBitmapIdentity::AnimationFrame(0) => Some(Self::Animation0),
            RootMenuBitmapIdentity::AnimationFrame(1) => Some(Self::Animation1),
            RootMenuBitmapIdentity::AnimationFrame(2) => Some(Self::Animation2),
            RootMenuBitmapIdentity::AnimationFrame(3) => Some(Self::Animation3),
            RootMenuBitmapIdentity::AnimationFrame(4) => Some(Self::Animation4),
            RootMenuBitmapIdentity::AnimationFrame(5) => Some(Self::Animation5),
            RootMenuBitmapIdentity::AnimationFrame(6) => Some(Self::Animation6),
            RootMenuBitmapIdentity::AnimationFrame(_) => None,
            RootMenuBitmapIdentity::Backdrop => Some(Self::Backdrop),
            RootMenuBitmapIdentity::Selection => Some(Self::Selection),
            RootMenuBitmapIdentity::CenterIcon {
                identity: CenterIconIdentity::ChampionshipGlobe,
                ..
            } => Some(Self::ChampionshipGlobe),
            RootMenuBitmapIdentity::CenterIcon {
                identity: CenterIconIdentity::TimeTrialGlobe,
                ..
            } => Some(Self::TimeTrialGlobe),
            RootMenuBitmapIdentity::CenterIcon {
                identity: CenterIconIdentity::Cone,
                ..
            } => Some(Self::Cone),
            RootMenuBitmapIdentity::CenterIcon {
                identity: CenterIconIdentity::Spanner,
                ..
            } => Some(Self::Spanner),
            RootMenuBitmapIdentity::CenterIcon {
                identity: CenterIconIdentity::Disk,
                ..
            } => Some(Self::Disk),
            RootMenuBitmapIdentity::CenterIcon {
                identity: CenterIconIdentity::Quit,
                ..
            } => Some(Self::Quit),
        }
    }
}

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
    language: Language,
    center_variant: VisualVariant,
    assets: Vec<RootMenuBitmapAsset>,
    optional_diagnostics: Vec<RootMenuBitmapLoadError>,
}

impl RootMenuBitmapSet {
    #[must_use]
    pub const fn language(&self) -> Language {
        self.language
    }

    #[must_use]
    pub const fn center_variant(&self) -> VisualVariant {
        self.center_variant
    }

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

    pub(crate) fn into_parts(self) -> RootMenuBitmapParts {
        RootMenuBitmapParts {
            language: self.language,
            center_variant: self.center_variant,
            assets: self.assets,
            optional_diagnostics: self.optional_diagnostics,
        }
    }
}

pub(crate) struct RootMenuBitmapParts {
    pub language: Language,
    pub center_variant: VisualVariant,
    pub assets: Vec<RootMenuBitmapAsset>,
    pub optional_diagnostics: Vec<RootMenuBitmapLoadError>,
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

/// Loads only the common selection bitmap through the same verified-store and
/// safe-decoder boundary used by the complete root-menu bitmap batch.
///
/// This is intended for owners that need the recovered Selection geometry but
/// do not own the rest of the root-menu 2D resources.
pub fn load_root_menu_selection_bitmap(
    store: &AssetStore,
    limits: BitmapLimits,
) -> Result<RootMenuBitmapAsset, RootMenuBitmapLoadError> {
    load_bitmap(
        store,
        RootMenuBitmapIdentity::Selection,
        ROOT_MENU_SELECT_PATH,
        limits,
    )
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
