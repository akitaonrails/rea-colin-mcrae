//! Required indexed 2D assets for the championship setup frame.
//!
//! Asset loading is separate from surface preparation. The caller explicitly
//! supplies the active palette and mapping for the info box; the recovered
//! Chequered preview always uses its loaded FCHE palette with identity indices.

use std::fmt;

use cmr_assets::{AssetStore, AssetStoreError};
use cmr_formats::{
    bitmap::{BitmapError, BitmapLimits, IndexedBitmap, parse_indexed_bitmap},
    riff_palette::{LegacyPalette256, PaletteError, parse_legacy_palette_256},
};

use crate::display::IndexedVideoBackend;
use crate::source_store::{SourceUploadError, UploadedIndexedBitmap, UploadedSourceStore};
use crate::surface::{IndexedSurface, PaletteMapping, SurfaceError, indexed_surface};

pub const CHEQUERED_FRAME_COUNT: usize = ChequeredFrame::ALL.len();

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u8)]
pub enum ChequeredFrame {
    Frame01,
    Frame02,
    Frame03,
    Frame04,
    Frame05,
    Frame06,
    Frame07,
    Frame08,
    Frame09,
    Frame10,
    Frame11,
    Frame12,
    Frame13,
    Frame14,
    Frame15,
}

impl ChequeredFrame {
    pub const ALL: [Self; 15] = [
        Self::Frame01,
        Self::Frame02,
        Self::Frame03,
        Self::Frame04,
        Self::Frame05,
        Self::Frame06,
        Self::Frame07,
        Self::Frame08,
        Self::Frame09,
        Self::Frame10,
        Self::Frame11,
        Self::Frame12,
        Self::Frame13,
        Self::Frame14,
        Self::Frame15,
    ];

    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    #[must_use]
    pub const fn descriptor(self) -> ChequeredFrameDescriptor {
        CHEQUERED_FRAME_DESCRIPTORS[self.index()]
    }
}

impl TryFrom<usize> for ChequeredFrame {
    type Error = usize;

    fn try_from(index: usize) -> Result<Self, Self::Error> {
        Self::ALL.get(index).copied().ok_or(index)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BitmapAssetDescriptor {
    pub path: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChequeredFrameDescriptor {
    pub frame: ChequeredFrame,
    pub path: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaletteAssetDescriptor {
    pub path: &'static str,
}

pub const INFO_BOX_DESCRIPTOR: BitmapAssetDescriptor = BitmapAssetDescriptor {
    path: "New_Fe/Images/Infobox/r_info.bmp",
};

pub const CHEQUERED_PALETTE_DESCRIPTOR: PaletteAssetDescriptor = PaletteAssetDescriptor {
    path: "New_Fe/Palettes/Flags/fche.pal",
};

pub const CHEQUERED_FRAME_DESCRIPTORS: [ChequeredFrameDescriptor; 15] = [
    ChequeredFrameDescriptor {
        frame: ChequeredFrame::Frame01,
        path: "New_Fe/Images/flag/cheq/cheq_001.bmp",
    },
    ChequeredFrameDescriptor {
        frame: ChequeredFrame::Frame02,
        path: "New_Fe/Images/flag/cheq/cheq_002.bmp",
    },
    ChequeredFrameDescriptor {
        frame: ChequeredFrame::Frame03,
        path: "New_Fe/Images/flag/cheq/cheq_003.bmp",
    },
    ChequeredFrameDescriptor {
        frame: ChequeredFrame::Frame04,
        path: "New_Fe/Images/flag/cheq/cheq_004.bmp",
    },
    ChequeredFrameDescriptor {
        frame: ChequeredFrame::Frame05,
        path: "New_Fe/Images/flag/cheq/cheq_005.bmp",
    },
    ChequeredFrameDescriptor {
        frame: ChequeredFrame::Frame06,
        path: "New_Fe/Images/flag/cheq/cheq_006.bmp",
    },
    ChequeredFrameDescriptor {
        frame: ChequeredFrame::Frame07,
        path: "New_Fe/Images/flag/cheq/cheq_007.bmp",
    },
    ChequeredFrameDescriptor {
        frame: ChequeredFrame::Frame08,
        path: "New_Fe/Images/flag/cheq/cheq_008.bmp",
    },
    ChequeredFrameDescriptor {
        frame: ChequeredFrame::Frame09,
        path: "New_Fe/Images/flag/cheq/cheq_009.bmp",
    },
    ChequeredFrameDescriptor {
        frame: ChequeredFrame::Frame10,
        path: "New_Fe/Images/flag/cheq/cheq_010.bmp",
    },
    ChequeredFrameDescriptor {
        frame: ChequeredFrame::Frame11,
        path: "New_Fe/Images/flag/cheq/cheq_011.bmp",
    },
    ChequeredFrameDescriptor {
        frame: ChequeredFrame::Frame12,
        path: "New_Fe/Images/flag/cheq/cheq_012.bmp",
    },
    ChequeredFrameDescriptor {
        frame: ChequeredFrame::Frame13,
        path: "New_Fe/Images/flag/cheq/cheq_013.bmp",
    },
    ChequeredFrameDescriptor {
        frame: ChequeredFrame::Frame14,
        path: "New_Fe/Images/flag/cheq/cheq_014.bmp",
    },
    ChequeredFrameDescriptor {
        frame: ChequeredFrame::Frame15,
        path: "New_Fe/Images/flag/cheq/cheq_015.bmp",
    },
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChampionshipFrameAssetRole {
    InfoBox,
    ChequeredFrame(ChequeredFrame),
    ChequeredPalette,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChequeredFrameAsset {
    pub descriptor: ChequeredFrameDescriptor,
    pub bitmap: IndexedBitmap,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChampionshipFrameAssets {
    pub info_box: IndexedBitmap,
    frames: [ChequeredFrameAsset; CHEQUERED_FRAME_COUNT],
    pub chequered_palette: LegacyPalette256,
}

impl ChampionshipFrameAssets {
    #[must_use]
    pub fn frame(&self, frame: ChequeredFrame) -> &ChequeredFrameAsset {
        &self.frames[frame.index()]
    }

    pub fn frames(&self) -> impl ExactSizeIterator<Item = &ChequeredFrameAsset> {
        self.frames.iter()
    }
}

#[derive(Debug)]
pub enum ChampionshipFrameAssetLoadError {
    Asset {
        role: ChampionshipFrameAssetRole,
        path: &'static str,
        source: AssetStoreError,
    },
    Bitmap {
        role: ChampionshipFrameAssetRole,
        path: &'static str,
        source: BitmapError,
    },
    Palette {
        path: &'static str,
        source: PaletteError,
    },
}

impl ChampionshipFrameAssetLoadError {
    #[must_use]
    pub const fn role(&self) -> ChampionshipFrameAssetRole {
        match self {
            Self::Asset { role, .. } | Self::Bitmap { role, .. } => *role,
            Self::Palette { .. } => ChampionshipFrameAssetRole::ChequeredPalette,
        }
    }

    #[must_use]
    pub const fn path(&self) -> &'static str {
        match self {
            Self::Asset { path, .. } | Self::Bitmap { path, .. } | Self::Palette { path, .. } => {
                path
            }
        }
    }
}

impl fmt::Display for ChampionshipFrameAssetLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Asset { role, path, source } => write!(
                formatter,
                "cannot load championship frame asset {role:?} at {path}: {source}"
            ),
            Self::Bitmap { role, path, source } => write!(
                formatter,
                "cannot decode championship frame bitmap {role:?} at {path}: {source}"
            ),
            Self::Palette { path, source } => write!(
                formatter,
                "cannot decode championship Chequered palette at {path}: {source}"
            ),
        }
    }
}

impl std::error::Error for ChampionshipFrameAssetLoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Asset { source, .. } => Some(source),
            Self::Bitmap { source, .. } => Some(source),
            Self::Palette { source, .. } => Some(source),
        }
    }
}

pub fn load_championship_frame_assets(
    store: &AssetStore,
    bitmap_limits: BitmapLimits,
) -> Result<ChampionshipFrameAssets, ChampionshipFrameAssetLoadError> {
    let info_box = load_bitmap(
        store,
        ChampionshipFrameAssetRole::InfoBox,
        INFO_BOX_DESCRIPTOR.path,
        bitmap_limits,
    )?;
    let mut frames = Vec::with_capacity(CHEQUERED_FRAME_COUNT);
    for descriptor in CHEQUERED_FRAME_DESCRIPTORS {
        let bitmap = load_bitmap(
            store,
            ChampionshipFrameAssetRole::ChequeredFrame(descriptor.frame),
            descriptor.path,
            bitmap_limits,
        )?;
        frames.push(ChequeredFrameAsset { descriptor, bitmap });
    }
    let palette_bytes = load_bytes(
        store,
        ChampionshipFrameAssetRole::ChequeredPalette,
        CHEQUERED_PALETTE_DESCRIPTOR.path,
    )?;
    let chequered_palette = parse_legacy_palette_256(&palette_bytes).map_err(|source| {
        ChampionshipFrameAssetLoadError::Palette {
            path: CHEQUERED_PALETTE_DESCRIPTOR.path,
            source,
        }
    })?;
    Ok(ChampionshipFrameAssets {
        info_box,
        frames: frames
            .try_into()
            .expect("one bitmap was loaded for every Chequered frame"),
        chequered_palette,
    })
}

fn load_bitmap(
    store: &AssetStore,
    role: ChampionshipFrameAssetRole,
    path: &'static str,
    limits: BitmapLimits,
) -> Result<IndexedBitmap, ChampionshipFrameAssetLoadError> {
    let bytes = load_bytes(store, role, path)?;
    parse_indexed_bitmap(&bytes, limits).map_err(|source| ChampionshipFrameAssetLoadError::Bitmap {
        role,
        path,
        source,
    })
}

fn load_bytes(
    store: &AssetStore,
    role: ChampionshipFrameAssetRole,
    path: &'static str,
) -> Result<Vec<u8>, ChampionshipFrameAssetLoadError> {
    store
        .load(path)
        .map_err(|source| ChampionshipFrameAssetLoadError::Asset { role, path, source })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChampionshipFrameSurfaceAsset {
    pub role: ChampionshipFrameAssetRole,
    pub path: &'static str,
    pub surface: IndexedSurface,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChampionshipFrameSurfaceSet {
    pub info_box: ChampionshipFrameSurfaceAsset,
    frames: [ChampionshipFrameSurfaceAsset; CHEQUERED_FRAME_COUNT],
}

impl ChampionshipFrameSurfaceSet {
    #[must_use]
    pub fn frame(&self, frame: ChequeredFrame) -> &ChampionshipFrameSurfaceAsset {
        &self.frames[frame.index()]
    }

    pub fn frames(&self) -> impl ExactSizeIterator<Item = &ChampionshipFrameSurfaceAsset> {
        self.frames.iter()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChampionshipFrameSurfaceError {
    pub role: ChampionshipFrameAssetRole,
    pub path: &'static str,
    pub source: SurfaceError,
}

impl fmt::Display for ChampionshipFrameSurfaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "cannot prepare championship frame asset {:?} at {}: {}",
            self.role, self.path, self.source
        )
    }
}

impl std::error::Error for ChampionshipFrameSurfaceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

pub fn prepare_championship_frame_surfaces(
    assets: &ChampionshipFrameAssets,
    info_active_palette: &LegacyPalette256,
    info_mapping: PaletteMapping,
) -> Result<ChampionshipFrameSurfaceSet, ChampionshipFrameSurfaceError> {
    let info_box = indexed_surface(
        assets.info_box.clone(),
        info_active_palette.clone(),
        info_mapping,
    )
    .map_err(|source| ChampionshipFrameSurfaceError {
        role: ChampionshipFrameAssetRole::InfoBox,
        path: INFO_BOX_DESCRIPTOR.path,
        source,
    })?;
    let mut frames = Vec::with_capacity(CHEQUERED_FRAME_COUNT);
    for asset in &assets.frames {
        let role = ChampionshipFrameAssetRole::ChequeredFrame(asset.descriptor.frame);
        let surface = indexed_surface(
            asset.bitmap.clone(),
            assets.chequered_palette.clone(),
            PaletteMapping::Identity,
        )
        .map_err(|source| ChampionshipFrameSurfaceError {
            role,
            path: asset.descriptor.path,
            source,
        })?;
        frames.push(ChampionshipFrameSurfaceAsset {
            role,
            path: asset.descriptor.path,
            surface,
        });
    }
    Ok(ChampionshipFrameSurfaceSet {
        info_box: ChampionshipFrameSurfaceAsset {
            role: ChampionshipFrameAssetRole::InfoBox,
            path: INFO_BOX_DESCRIPTOR.path,
            surface: info_box,
        },
        frames: frames
            .try_into()
            .expect("one surface was prepared for every Chequered frame"),
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UploadedChampionshipFramePool {
    info_box: UploadedIndexedBitmap,
    frames: [UploadedIndexedBitmap; CHEQUERED_FRAME_COUNT],
}

impl UploadedChampionshipFramePool {
    #[must_use]
    pub const fn info_box(&self) -> UploadedIndexedBitmap {
        self.info_box
    }

    #[must_use]
    pub const fn frame(&self, frame: ChequeredFrame) -> UploadedIndexedBitmap {
        self.frames[frame.index()]
    }

    pub fn frames(&self) -> impl ExactSizeIterator<Item = UploadedIndexedBitmap> + '_ {
        self.frames.iter().copied()
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct ChampionshipFrameUploadError<E> {
    pub role: ChampionshipFrameAssetRole,
    pub path: &'static str,
    pub source: SourceUploadError<E>,
}

impl<E: fmt::Display> fmt::Display for ChampionshipFrameUploadError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "cannot upload championship frame asset {:?} at {}: {}",
            self.role, self.path, self.source
        )
    }
}

impl<E: std::error::Error + 'static> std::error::Error for ChampionshipFrameUploadError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

pub fn upload_championship_frame_surfaces<B: IndexedVideoBackend>(
    store: &mut UploadedSourceStore<'_, B>,
    surfaces: ChampionshipFrameSurfaceSet,
) -> Result<UploadedChampionshipFramePool, ChampionshipFrameUploadError<B::Error>> {
    let mut transaction = store.transaction();
    let info_box = transaction
        .upload_bitmap(&surfaces.info_box.surface)
        .map_err(|source| ChampionshipFrameUploadError {
            role: surfaces.info_box.role,
            path: surfaces.info_box.path,
            source,
        })?;
    let mut frames = Vec::with_capacity(CHEQUERED_FRAME_COUNT);
    for asset in surfaces.frames {
        let uploaded = transaction
            .upload_bitmap(&asset.surface)
            .map_err(|source| ChampionshipFrameUploadError {
                role: asset.role,
                path: asset.path,
                source,
            })?;
        frames.push(uploaded);
    }
    transaction.commit();
    Ok(UploadedChampionshipFramePool {
        info_box,
        frames: frames
            .try_into()
            .expect("one source was uploaded for every Chequered frame"),
    })
}
