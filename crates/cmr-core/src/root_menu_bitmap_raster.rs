//! Preparation boundary between decoded root-menu bitmaps and indexed video.
//!
//! The original loader preserves source indices and does not attach each BMP's
//! palette to its surface. Reconstruction therefore uses the caller's active
//! 256-entry display palette with identity index mapping. Preparation has no
//! backend effects; persistent upload is a separate resource-lifetime step.

use std::fmt;

use cmr_formats::riff_palette::LegacyPalette256;

use crate::blit::{BlitDecision, BlitPlan, ColorKey, RectI16, plan_legacy_blit};
use crate::display::{DisplayExtent, IndexedVideoBackend};
use crate::localization::Language;
use crate::root_menu_assets::VisualVariant;
use crate::root_menu_bitmap_assets::{
    ROOT_MENU_BITMAP_REQUEST_COUNT, RootMenuBitmapIdentity, RootMenuBitmapLoadError,
    RootMenuBitmapParts, RootMenuBitmapSet, RootMenuBitmapSlot,
};
use crate::root_menu_visual::{
    ANIMATION_BITMAP_SLOTS, CENTER_BITMAP_SLOTS, Point, ROOT_ENTRY_COUNT, Rect, RenderCommand,
};
use crate::source_store::{
    SourceAccessError, SourceBlitError, SourceKey, SourceUploadError, UploadedSourceStore,
};
use crate::surface::{IndexedSurface, PaletteMapping, SurfaceError, indexed_surface};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RootMenuSurfaceAsset {
    pub identity: RootMenuBitmapIdentity,
    pub path: &'static str,
    pub surface: IndexedSurface,
}

#[derive(Debug)]
pub enum RootMenuSurfaceDiagnostic {
    Load(RootMenuBitmapLoadError),
    Surface {
        slot: RootMenuBitmapSlot,
        identity: RootMenuBitmapIdentity,
        path: &'static str,
        source: SurfaceError,
    },
}

impl RootMenuSurfaceDiagnostic {
    #[must_use]
    pub const fn identity(&self) -> RootMenuBitmapIdentity {
        match self {
            Self::Load(error) => error.identity(),
            Self::Surface { identity, .. } => *identity,
        }
    }

    #[must_use]
    pub const fn path(&self) -> &'static str {
        match self {
            Self::Load(error) => error.path(),
            Self::Surface { path, .. } => path,
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum RootMenuSurfaceError {
    UnknownIdentity(RootMenuBitmapIdentity),
    CenterVariantMismatch {
        identity: RootMenuBitmapIdentity,
        expected: VisualVariant,
    },
    DuplicateSlot(RootMenuBitmapSlot),
    UnexpectedRequiredDiagnostic(RootMenuBitmapSlot),
    MissingSlot(RootMenuBitmapSlot),
    Surface {
        slot: RootMenuBitmapSlot,
        identity: RootMenuBitmapIdentity,
        path: &'static str,
        source: SurfaceError,
    },
}

impl fmt::Display for RootMenuSurfaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownIdentity(identity) => {
                write!(formatter, "unknown root-menu bitmap identity {identity:?}")
            }
            Self::CenterVariantMismatch { identity, expected } => write!(
                formatter,
                "root-menu bitmap {identity:?} does not match variant {expected:?}"
            ),
            Self::DuplicateSlot(slot) => {
                write!(formatter, "duplicate root-menu bitmap slot {slot:?}")
            }
            Self::UnexpectedRequiredDiagnostic(slot) => write!(
                formatter,
                "required root-menu bitmap slot {slot:?} was supplied as an optional diagnostic"
            ),
            Self::MissingSlot(slot) => write!(formatter, "missing root-menu bitmap slot {slot:?}"),
            Self::Surface {
                identity,
                path,
                source,
                ..
            } => write!(
                formatter,
                "cannot prepare root-menu bitmap {identity:?} at {path}: {source}"
            ),
        }
    }
}

impl std::error::Error for RootMenuSurfaceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Surface { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct RootMenuSurfaceSet {
    language: Language,
    center_variant: VisualVariant,
    slots: [Option<RootMenuSurfaceAsset>; ROOT_MENU_BITMAP_REQUEST_COUNT],
    optional_diagnostics: Vec<RootMenuSurfaceDiagnostic>,
}

impl RootMenuSurfaceSet {
    #[must_use]
    pub const fn language(&self) -> Language {
        self.language
    }

    #[must_use]
    pub const fn center_variant(&self) -> VisualVariant {
        self.center_variant
    }

    #[must_use]
    pub fn get(&self, slot: RootMenuBitmapSlot) -> Option<&RootMenuSurfaceAsset> {
        self.slots[slot.index()].as_ref()
    }

    pub fn loaded_assets(&self) -> impl Iterator<Item = &RootMenuSurfaceAsset> {
        self.slots.iter().flatten()
    }

    pub fn slots(
        &self,
    ) -> impl ExactSizeIterator<Item = (RootMenuBitmapSlot, Option<&RootMenuSurfaceAsset>)> {
        RootMenuBitmapSlot::ALL
            .into_iter()
            .map(|slot| (slot, self.get(slot)))
    }

    pub fn optional_diagnostics(
        &self,
    ) -> impl ExactSizeIterator<Item = &RootMenuSurfaceDiagnostic> {
        self.optional_diagnostics.iter()
    }

    #[must_use]
    pub fn loaded_len(&self) -> usize {
        self.slots.iter().flatten().count()
    }

    #[must_use]
    pub fn optional_diagnostic_len(&self) -> usize {
        self.optional_diagnostics.len()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct UploadedRootMenuBitmap {
    source: SourceKey,
    width: u32,
    height: u32,
}

/// Backend-free descriptor for the fixed 17-slot root-menu bitmap batch.
///
/// Optional slots remain holes and never shift later identities. The shared
/// source store owns every live backend resource and global teardown order.
#[derive(Debug)]
pub struct UploadedRootMenuBitmapPool {
    language: Language,
    center_variant: VisualVariant,
    slots: [Option<UploadedRootMenuBitmap>; ROOT_MENU_BITMAP_REQUEST_COUNT],
}

impl UploadedRootMenuBitmapPool {
    #[must_use]
    pub const fn language(&self) -> Language {
        self.language
    }

    #[must_use]
    pub const fn center_variant(&self) -> VisualVariant {
        self.center_variant
    }

    #[must_use]
    pub fn is_loaded(&self, slot: RootMenuBitmapSlot) -> bool {
        self.slots[slot.index()].is_some()
    }

    #[must_use]
    pub fn extent(&self, slot: RootMenuBitmapSlot) -> Option<(u32, u32)> {
        self.slots[slot.index()].map(|bitmap| (bitmap.width, bitmap.height))
    }

    pub fn blit<B: IndexedVideoBackend>(
        &self,
        store: &mut UploadedSourceStore<'_, B>,
        slot: RootMenuBitmapSlot,
        destination: B::Surface,
        plan: BlitPlan,
    ) -> Result<(), RootMenuBitmapBlitError<B::Error>> {
        let bitmap =
            self.slots[slot.index()].ok_or(RootMenuBitmapBlitError::MissingOptionalSlot(slot))?;
        store
            .blit(bitmap.source, destination, plan)
            .map_err(|error| match error {
                SourceBlitError::Access(source) => RootMenuBitmapBlitError::Access { slot, source },
                SourceBlitError::Backend(source) => {
                    RootMenuBitmapBlitError::Backend { slot, source }
                }
            })
    }
}

#[derive(Debug)]
pub enum RootMenuBitmapUploadDiagnostic<E> {
    Preparation {
        slot: RootMenuBitmapSlot,
        source: RootMenuSurfaceDiagnostic,
    },
    Upload {
        slot: RootMenuBitmapSlot,
        identity: RootMenuBitmapIdentity,
        path: &'static str,
        source: SourceUploadError<E>,
    },
}

impl<E> RootMenuBitmapUploadDiagnostic<E> {
    #[must_use]
    pub const fn slot(&self) -> RootMenuBitmapSlot {
        match self {
            Self::Preparation { slot, .. } | Self::Upload { slot, .. } => *slot,
        }
    }

    #[must_use]
    pub const fn identity(&self) -> RootMenuBitmapIdentity {
        match self {
            Self::Preparation { source, .. } => source.identity(),
            Self::Upload { identity, .. } => *identity,
        }
    }

    #[must_use]
    pub const fn path(&self) -> &'static str {
        match self {
            Self::Preparation { source, .. } => source.path(),
            Self::Upload { path, .. } => path,
        }
    }
}

#[derive(Debug)]
pub struct RootMenuBitmapUploadOutcome<E> {
    pub pool: UploadedRootMenuBitmapPool,
    pub optional_diagnostics: Vec<RootMenuBitmapUploadDiagnostic<E>>,
}

#[derive(Debug, Eq, PartialEq)]
pub struct RootMenuBitmapUploadError<E> {
    pub slot: RootMenuBitmapSlot,
    pub identity: RootMenuBitmapIdentity,
    pub path: &'static str,
    pub source: SourceUploadError<E>,
}

impl<E: fmt::Display> fmt::Display for RootMenuBitmapUploadError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "cannot upload required root-menu bitmap {:?} at {}: {}",
            self.identity, self.path, self.source
        )
    }
}

impl<E: std::error::Error + 'static> std::error::Error for RootMenuBitmapUploadError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum RootMenuBitmapBlitError<E> {
    MissingOptionalSlot(RootMenuBitmapSlot),
    Access {
        slot: RootMenuBitmapSlot,
        source: SourceAccessError,
    },
    Backend {
        slot: RootMenuBitmapSlot,
        source: E,
    },
}

impl<E: fmt::Display> fmt::Display for RootMenuBitmapBlitError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingOptionalSlot(slot) => {
                write!(formatter, "root-menu bitmap slot {slot:?} is not loaded")
            }
            Self::Access { slot, source } => {
                write!(
                    formatter,
                    "cannot access root-menu bitmap slot {slot:?}: {source}"
                )
            }
            Self::Backend { slot, source } => {
                write!(
                    formatter,
                    "cannot blit root-menu bitmap slot {slot:?}: {source}"
                )
            }
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for RootMenuBitmapBlitError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::MissingOptionalSlot(_) => None,
            Self::Access { source, .. } => Some(source),
            Self::Backend { source, .. } => Some(source),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RootBitmapCommandKind {
    Backdrop,
    ChromeShadow,
    ChromeAnimation,
    SelectedChrome,
    CenterIcon,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RootBitmapCommandOutcome {
    NotBitmap,
    NoOp,
    Blitted,
}

#[derive(Debug, Eq, PartialEq)]
pub enum RootBitmapCommandError<E> {
    EntryOutOfRange {
        command: RootBitmapCommandKind,
        index: u8,
    },
    UnexpectedSlot {
        command: RootBitmapCommandKind,
        actual: RootMenuBitmapSlot,
        expected: RootMenuBitmapSlot,
    },
    MissingSlot(RootMenuBitmapSlot),
    InvalidDestinationExtent(DisplayExtent),
    SourceOutOfBounds {
        slot: RootMenuBitmapSlot,
        source: Rect,
        extent: (u32, u32),
    },
    SourceExtentOutOfRange {
        slot: RootMenuBitmapSlot,
        extent: (u32, u32),
    },
    GeometryOutOfRange {
        slot: RootMenuBitmapSlot,
        source: Rect,
        destination: Point,
    },
    Access {
        slot: RootMenuBitmapSlot,
        source: SourceAccessError,
    },
    Backend {
        slot: RootMenuBitmapSlot,
        source: E,
    },
}

impl<E: fmt::Display> fmt::Display for RootBitmapCommandError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EntryOutOfRange { command, index } => {
                write!(
                    formatter,
                    "root bitmap {command:?} index {index} is out of range"
                )
            }
            Self::UnexpectedSlot {
                command,
                actual,
                expected,
            } => write!(
                formatter,
                "root bitmap {command:?} uses slot {actual:?}; expected {expected:?}"
            ),
            Self::MissingSlot(slot) => write!(formatter, "root bitmap slot {slot:?} is not loaded"),
            Self::InvalidDestinationExtent(extent) => write!(
                formatter,
                "root bitmap destination extent {}x{} is outside the positive i16 range",
                extent.width, extent.height
            ),
            Self::SourceOutOfBounds {
                slot,
                source,
                extent,
            } => write!(
                formatter,
                "root bitmap slot {slot:?} source {source:?} is outside {}x{}",
                extent.0, extent.1
            ),
            Self::SourceExtentOutOfRange { slot, extent } => write!(
                formatter,
                "root bitmap slot {slot:?} extent {}x{} is outside the positive u16 range",
                extent.0, extent.1
            ),
            Self::GeometryOutOfRange {
                slot,
                source,
                destination,
            } => write!(
                formatter,
                "root bitmap slot {slot:?} geometry {source:?} to {destination:?} is outside the i16 range"
            ),
            Self::Access { slot, source } => {
                write!(
                    formatter,
                    "cannot access root bitmap slot {slot:?}: {source}"
                )
            }
            Self::Backend { slot, source } => {
                write!(formatter, "cannot draw root bitmap slot {slot:?}: {source}")
            }
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for RootBitmapCommandError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Access { source, .. } => Some(source),
            Self::Backend { source, .. } => Some(source),
            _ => None,
        }
    }
}

const INDEX_ZERO_COLOR_KEY: ColorKey = ColorKey { low: 0, high: 0 };

/// Executes one bitmap command without consuming or reordering other root
/// transcript commands. Non-bitmap commands are classified as
/// [`RootBitmapCommandOutcome::NotBitmap`] so a sequential caller can dispatch
/// its retained copy elsewhere.
pub fn execute_root_bitmap_command<B: IndexedVideoBackend>(
    store: &mut UploadedSourceStore<'_, B>,
    pool: &UploadedRootMenuBitmapPool,
    command: RenderCommand,
    destination: B::Surface,
    destination_extent: DisplayExtent,
) -> Result<RootBitmapCommandOutcome, RootBitmapCommandError<B::Error>> {
    let Some(spec) = bitmap_command_spec(command)? else {
        return Ok(RootBitmapCommandOutcome::NotBitmap);
    };
    let extent = pool
        .extent(spec.slot)
        .ok_or(RootBitmapCommandError::MissingSlot(spec.slot))?;
    let source = spec.source.unwrap_or(Rect {
        x: 0,
        y: 0,
        width: u16::try_from(extent.0)
            .ok()
            .filter(|value| *value > 0)
            .ok_or(RootBitmapCommandError::SourceExtentOutOfRange {
                slot: spec.slot,
                extent,
            })?,
        height: u16::try_from(extent.1)
            .ok()
            .filter(|value| *value > 0)
            .ok_or(RootBitmapCommandError::SourceExtentOutOfRange {
                slot: spec.slot,
                extent,
            })?,
    });
    validate_source(spec.slot, source, extent)?;
    let destination_width = positive_i16(destination_extent.width).ok_or(
        RootBitmapCommandError::InvalidDestinationExtent(destination_extent),
    )?;
    let destination_height = positive_i16(destination_extent.height).ok_or(
        RootBitmapCommandError::InvalidDestinationExtent(destination_extent),
    )?;
    let source_rect = checked_rect(spec.slot, source, spec.destination)?;
    let destination_rect = RectI16 {
        x: i16::try_from(spec.destination.x).map_err(|_| geometry_error(spec, source))?,
        y: i16::try_from(spec.destination.y).map_err(|_| geometry_error(spec, source))?,
        width: source_rect.width,
        height: source_rect.height,
    };
    match plan_legacy_blit(
        source_rect,
        destination_rect,
        destination_width,
        destination_height,
        spec.color_key,
    ) {
        BlitDecision::NoOp => Ok(RootBitmapCommandOutcome::NoOp),
        BlitDecision::Blit(plan) => {
            pool.blit(store, spec.slot, destination, plan)
                .map_err(|error| match error {
                    RootMenuBitmapBlitError::MissingOptionalSlot(slot) => {
                        RootBitmapCommandError::MissingSlot(slot)
                    }
                    RootMenuBitmapBlitError::Access { slot, source } => {
                        RootBitmapCommandError::Access { slot, source }
                    }
                    RootMenuBitmapBlitError::Backend { slot, source } => {
                        RootBitmapCommandError::Backend { slot, source }
                    }
                })?;
            Ok(RootBitmapCommandOutcome::Blitted)
        }
    }
}

#[derive(Clone, Copy)]
struct BitmapCommandSpec {
    slot: RootMenuBitmapSlot,
    source: Option<Rect>,
    destination: Point,
    color_key: Option<ColorKey>,
}

fn bitmap_command_spec<E>(
    command: RenderCommand,
) -> Result<Option<BitmapCommandSpec>, RootBitmapCommandError<E>> {
    let keyed = Some(INDEX_ZERO_COLOR_KEY);
    let spec = match command {
        RenderCommand::Backdrop {
            slot,
            source,
            destination,
        } => checked_command_slot(
            RootBitmapCommandKind::Backdrop,
            slot,
            RootMenuBitmapSlot::Backdrop,
            source,
            destination,
            None,
        )?,
        RenderCommand::ChromeShadow {
            slot,
            entry,
            source,
            destination,
        } => {
            validate_entry(RootBitmapCommandKind::ChromeShadow, entry)?;
            checked_command_slot(
                RootBitmapCommandKind::ChromeShadow,
                slot,
                RootMenuBitmapSlot::MainShadow,
                source,
                destination,
                keyed,
            )?
        }
        RenderCommand::ChromeAnimation {
            slot,
            entry,
            source,
            destination,
        } => {
            let expected = ANIMATION_BITMAP_SLOTS
                .get(usize::from(entry))
                .copied()
                .ok_or(RootBitmapCommandError::EntryOutOfRange {
                    command: RootBitmapCommandKind::ChromeAnimation,
                    index: entry,
                })?;
            checked_command_slot(
                RootBitmapCommandKind::ChromeAnimation,
                slot,
                expected,
                source,
                destination,
                keyed,
            )?
        }
        RenderCommand::SelectedChrome {
            slot,
            entry,
            source,
            destination,
        } => {
            validate_entry(RootBitmapCommandKind::SelectedChrome, entry)?;
            checked_command_slot(
                RootBitmapCommandKind::SelectedChrome,
                slot,
                RootMenuBitmapSlot::MainSelected,
                source,
                destination,
                keyed,
            )?
        }
        RenderCommand::CenterIcon {
            slot,
            visual_index,
            destination,
        } => {
            let expected = CENTER_BITMAP_SLOTS
                .get(usize::from(visual_index))
                .copied()
                .ok_or(RootBitmapCommandError::EntryOutOfRange {
                    command: RootBitmapCommandKind::CenterIcon,
                    index: visual_index,
                })?;
            if slot != expected {
                return Err(RootBitmapCommandError::UnexpectedSlot {
                    command: RootBitmapCommandKind::CenterIcon,
                    actual: slot,
                    expected,
                });
            }
            BitmapCommandSpec {
                slot,
                source: None,
                destination,
                color_key: keyed,
            }
        }
        RenderCommand::FooterText { .. }
        | RenderCommand::C3dObject { .. }
        | RenderCommand::ComposeAndPresent => return Ok(None),
    };
    Ok(Some(spec))
}

fn validate_entry<E>(
    command: RootBitmapCommandKind,
    index: u8,
) -> Result<(), RootBitmapCommandError<E>> {
    if usize::from(index) >= ROOT_ENTRY_COUNT {
        return Err(RootBitmapCommandError::EntryOutOfRange { command, index });
    }
    Ok(())
}

fn checked_command_slot<E>(
    kind: RootBitmapCommandKind,
    actual: RootMenuBitmapSlot,
    expected: RootMenuBitmapSlot,
    source: Rect,
    destination: Point,
    color_key: Option<ColorKey>,
) -> Result<BitmapCommandSpec, RootBitmapCommandError<E>> {
    if actual != expected {
        return Err(RootBitmapCommandError::UnexpectedSlot {
            command: kind,
            actual,
            expected,
        });
    }
    Ok(BitmapCommandSpec {
        slot: actual,
        source: Some(source),
        destination,
        color_key,
    })
}

fn validate_source<E>(
    slot: RootMenuBitmapSlot,
    source: Rect,
    extent: (u32, u32),
) -> Result<(), RootBitmapCommandError<E>> {
    let right = i64::from(source.x) + i64::from(source.width);
    let bottom = i64::from(source.y) + i64::from(source.height);
    if source.x < 0
        || source.y < 0
        || source.width == 0
        || source.height == 0
        || right > i64::from(extent.0)
        || bottom > i64::from(extent.1)
    {
        return Err(RootBitmapCommandError::SourceOutOfBounds {
            slot,
            source,
            extent,
        });
    }
    Ok(())
}

fn checked_rect<E>(
    slot: RootMenuBitmapSlot,
    source: Rect,
    destination: Point,
) -> Result<RectI16, RootBitmapCommandError<E>> {
    Ok(RectI16 {
        x: i16::try_from(source.x).map_err(|_| RootBitmapCommandError::GeometryOutOfRange {
            slot,
            source,
            destination,
        })?,
        y: i16::try_from(source.y).map_err(|_| RootBitmapCommandError::GeometryOutOfRange {
            slot,
            source,
            destination,
        })?,
        width: i16::try_from(source.width).map_err(|_| {
            RootBitmapCommandError::GeometryOutOfRange {
                slot,
                source,
                destination,
            }
        })?,
        height: i16::try_from(source.height).map_err(|_| {
            RootBitmapCommandError::GeometryOutOfRange {
                slot,
                source,
                destination,
            }
        })?,
    })
}

fn geometry_error<E>(spec: BitmapCommandSpec, source: Rect) -> RootBitmapCommandError<E> {
    RootBitmapCommandError::GeometryOutOfRange {
        slot: spec.slot,
        source,
        destination: spec.destination,
    }
}

fn positive_i16(value: u16) -> Option<i16> {
    let value = i16::try_from(value).ok()?;
    (value > 0).then_some(value)
}

/// Uploads prepared root-menu surfaces into one shared transaction.
///
/// Optional upload failures remain fixed-slot diagnostics and upload continues.
/// A required failure rolls back every source created by this batch while
/// leaving resources committed before the transaction untouched.
pub fn upload_root_menu_bitmaps<B: IndexedVideoBackend>(
    store: &mut UploadedSourceStore<'_, B>,
    surfaces: RootMenuSurfaceSet,
) -> Result<RootMenuBitmapUploadOutcome<B::Error>, RootMenuBitmapUploadError<B::Error>> {
    let RootMenuSurfaceSet {
        language,
        center_variant,
        slots: prepared_slots,
        optional_diagnostics: preparation_diagnostics,
    } = surfaces;
    let mut diagnostics = preparation_diagnostics
        .into_iter()
        .map(|source| {
            let slot = RootMenuBitmapSlot::from_identity(source.identity())
                .expect("prepared diagnostics have checked root-menu identities");
            (
                slot.index(),
                RootMenuBitmapUploadDiagnostic::Preparation { slot, source },
            )
        })
        .collect::<Vec<_>>();
    let mut transaction = store.transaction();
    let mut slots = [None; ROOT_MENU_BITMAP_REQUEST_COUNT];

    for slot in RootMenuBitmapSlot::ALL {
        let Some(asset) = prepared_slots[slot.index()].as_ref() else {
            continue;
        };
        match transaction.upload(&asset.surface) {
            Ok(source) => {
                slots[slot.index()] = Some(UploadedRootMenuBitmap {
                    source,
                    width: asset.surface.width,
                    height: asset.surface.height,
                });
            }
            Err(source) if slot.is_optional() => diagnostics.push((
                slot.index(),
                RootMenuBitmapUploadDiagnostic::Upload {
                    slot,
                    identity: asset.identity,
                    path: asset.path,
                    source,
                },
            )),
            Err(source) => {
                return Err(RootMenuBitmapUploadError {
                    slot,
                    identity: asset.identity,
                    path: asset.path,
                    source,
                });
            }
        }
    }

    diagnostics.sort_by_key(|(index, _)| *index);
    transaction.commit();
    Ok(RootMenuBitmapUploadOutcome {
        pool: UploadedRootMenuBitmapPool {
            language,
            center_variant,
            slots,
        },
        optional_diagnostics: diagnostics
            .into_iter()
            .map(|(_, diagnostic)| diagnostic)
            .collect(),
    })
}

/// Converts every decoded root-menu bitmap without remapping palette indices.
///
/// Optional request failures remain ordered diagnostics and fixed-slot holes.
/// A required conversion failure returns immediately. All 17 loader outcomes
/// are checked so malformed internal sets cannot silently shift identities.
pub fn prepare_root_menu_surfaces(
    bitmaps: RootMenuBitmapSet,
    active_palette: &LegacyPalette256,
) -> Result<RootMenuSurfaceSet, RootMenuSurfaceError> {
    prepare_parts(bitmaps.into_parts(), active_palette)
}

pub(crate) fn prepare_parts(
    parts: RootMenuBitmapParts,
    active_palette: &LegacyPalette256,
) -> Result<RootMenuSurfaceSet, RootMenuSurfaceError> {
    let mut slots = std::array::from_fn(|_| None);
    let mut seen = [false; ROOT_MENU_BITMAP_REQUEST_COUNT];
    let mut diagnostics = Vec::new();

    for diagnostic in parts.optional_diagnostics {
        let identity = diagnostic.identity();
        let slot = checked_slot(identity, parts.center_variant)?;
        if !slot.is_optional() {
            return Err(RootMenuSurfaceError::UnexpectedRequiredDiagnostic(slot));
        }
        if seen[slot.index()] {
            return Err(RootMenuSurfaceError::DuplicateSlot(slot));
        }
        seen[slot.index()] = true;
        diagnostics.push((slot.index(), RootMenuSurfaceDiagnostic::Load(diagnostic)));
    }

    for asset in parts.assets {
        let slot = checked_slot(asset.identity, parts.center_variant)?;
        if seen[slot.index()] {
            return Err(RootMenuSurfaceError::DuplicateSlot(slot));
        }
        seen[slot.index()] = true;
        match indexed_surface(
            asset.bitmap,
            active_palette.clone(),
            PaletteMapping::Identity,
        ) {
            Ok(surface) => {
                slots[slot.index()] = Some(RootMenuSurfaceAsset {
                    identity: asset.identity,
                    path: asset.path,
                    surface,
                });
            }
            Err(source) if slot.is_optional() => diagnostics.push((
                slot.index(),
                RootMenuSurfaceDiagnostic::Surface {
                    slot,
                    identity: asset.identity,
                    path: asset.path,
                    source,
                },
            )),
            Err(source) => {
                return Err(RootMenuSurfaceError::Surface {
                    slot,
                    identity: asset.identity,
                    path: asset.path,
                    source,
                });
            }
        }
    }

    for slot in RootMenuBitmapSlot::ALL {
        if !seen[slot.index()] {
            return Err(RootMenuSurfaceError::MissingSlot(slot));
        }
    }
    diagnostics.sort_by_key(|(index, _)| *index);

    Ok(RootMenuSurfaceSet {
        language: parts.language,
        center_variant: parts.center_variant,
        slots,
        optional_diagnostics: diagnostics
            .into_iter()
            .map(|(_, diagnostic)| diagnostic)
            .collect(),
    })
}

fn checked_slot(
    identity: RootMenuBitmapIdentity,
    expected_variant: VisualVariant,
) -> Result<RootMenuBitmapSlot, RootMenuSurfaceError> {
    if let RootMenuBitmapIdentity::CenterIcon { variant, .. } = identity
        && variant != expected_variant
    {
        return Err(RootMenuSurfaceError::CenterVariantMismatch {
            identity,
            expected: expected_variant,
        });
    }
    RootMenuBitmapSlot::from_identity(identity)
        .ok_or(RootMenuSurfaceError::UnknownIdentity(identity))
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use cmr_formats::bitmap::{BitmapError, BitmapMetadata, BitmapPaletteEntry, IndexedBitmap};
    use cmr_formats::font::{
        FontLimits, HorizontalAlignment, PointI32, TextAlignment, TextLayoutLimits, layout_text,
        parse_legacy_font,
    };
    use cmr_formats::riff_palette::PaletteEntry;

    use super::*;
    use crate::blit::{BlitDecision, RectI16, plan_legacy_blit};
    use crate::display::{CooperativeMode, DisplaySurfaces, IndexedMode, SurfaceTopology};
    use crate::font_assets::{FONT_COUNT, FontSet, FontStyle};
    use crate::font_raster::UploadedFontPool;
    use crate::root_menu_bitmap_assets::RootMenuBitmapAsset;
    use crate::root_menu_c3d_assets::RootMenuC3dSlot;
    use crate::root_menu_visual::RootVisualAssets;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum BackendError {
        Create(usize),
        Blit,
    }

    impl fmt::Display for BackendError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "{self:?}")
        }
    }

    impl std::error::Error for BackendError {}

    #[derive(Clone, Debug, Eq, PartialEq)]
    enum Event {
        Create { marker: u8, handle: u8 },
        Blit { source: u8, destination: u8 },
        Destroy(u8),
    }

    #[derive(Default)]
    struct RecordingBackend {
        events: Vec<Event>,
        blits: Vec<BlitPlan>,
        create_attempts: usize,
        fail_create_at: Option<usize>,
        fail_blit: Cell<bool>,
    }

    impl IndexedVideoBackend for RecordingBackend {
        type Error = BackendError;
        type Surface = u8;
        type Palette = u8;

        fn create_device(&mut self) -> Result<(), Self::Error> {
            unreachable!()
        }

        fn set_cooperative_mode(&mut self, _mode: CooperativeMode) -> Result<(), Self::Error> {
            unreachable!()
        }

        fn set_display_mode(&mut self, _mode: IndexedMode) -> Result<(), Self::Error> {
            unreachable!()
        }

        fn create_surfaces(
            &mut self,
            _topology: SurfaceTopology,
            _mode: IndexedMode,
        ) -> Result<DisplaySurfaces<Self::Surface>, Self::Error> {
            unreachable!()
        }

        fn create_palette(
            &mut self,
            _entries: &[PaletteEntry; 256],
        ) -> Result<Self::Palette, Self::Error> {
            unreachable!()
        }

        fn attach_palette(
            &mut self,
            _surface: Self::Surface,
            _palette: Self::Palette,
        ) -> Result<(), Self::Error> {
            unreachable!()
        }

        fn upload_palette(
            &mut self,
            _palette: Self::Palette,
            _entries: &[PaletteEntry; 256],
        ) -> Result<(), Self::Error> {
            unreachable!()
        }

        fn create_source(
            &mut self,
            surface: &IndexedSurface,
        ) -> Result<Self::Surface, Self::Error> {
            let index = self.create_attempts;
            self.create_attempts += 1;
            if self.fail_create_at == Some(index) {
                return Err(BackendError::Create(index));
            }
            let handle = 10 + index as u8;
            self.events.push(Event::Create {
                marker: surface.pixels[0],
                handle,
            });
            Ok(handle)
        }

        fn blit(
            &mut self,
            source: Self::Surface,
            destination: Self::Surface,
            plan: BlitPlan,
        ) -> Result<(), Self::Error> {
            self.blits.push(plan);
            self.events.push(Event::Blit {
                source,
                destination,
            });
            if self.fail_blit.get() {
                Err(BackendError::Blit)
            } else {
                Ok(())
            }
        }

        fn destroy_source(&mut self, source: Self::Surface) {
            self.events.push(Event::Destroy(source));
        }
    }

    fn palette() -> LegacyPalette256 {
        LegacyPalette256 {
            entries: std::array::from_fn(|index| PaletteEntry {
                red: index as u8,
                green: 255 - index as u8,
                blue: 17,
                flags: 99,
            }),
        }
    }

    fn bitmap(marker: u8) -> IndexedBitmap {
        IndexedBitmap {
            width: 4,
            height: 1,
            source_was_top_down: false,
            palette: vec![
                BitmapPaletteEntry {
                    red: 1,
                    green: 2,
                    blue: 3,
                    reserved: 4,
                };
                256
            ],
            pixels: vec![marker; 4],
            metadata: BitmapMetadata {
                reserved_1: 0,
                reserved_2: 0,
                x_pixels_per_meter: 0,
                y_pixels_per_meter: 0,
                important_colors: 0,
            },
        }
    }

    fn complete_parts(variant: VisualVariant) -> RootMenuBitmapParts {
        let language = Language::English;
        RootMenuBitmapParts {
            language,
            center_variant: variant,
            assets: RootMenuBitmapSlot::ALL
                .into_iter()
                .map(|slot| RootMenuBitmapAsset {
                    identity: slot.identity(variant),
                    path: slot.path(language, variant),
                    bitmap: bitmap(slot.index() as u8),
                })
                .collect(),
            optional_diagnostics: Vec::new(),
        }
    }

    #[test]
    fn complete_preparation_preserves_fixed_slots_indices_and_active_palette() {
        let active = palette();
        let prepared = prepare_parts(complete_parts(VisualVariant::GPrefixed), &active).unwrap();

        assert_eq!(prepared.loaded_len(), ROOT_MENU_BITMAP_REQUEST_COUNT);
        assert_eq!(prepared.optional_diagnostic_len(), 0);
        for slot in RootMenuBitmapSlot::ALL {
            let asset = prepared.get(slot).unwrap();
            assert_eq!(asset.identity, slot.identity(VisualVariant::GPrefixed));
            assert_eq!(
                asset.path,
                slot.path(Language::English, VisualVariant::GPrefixed)
            );
            assert_eq!(asset.surface.pixels, vec![slot.index() as u8; 4]);
            assert_eq!(
                (
                    asset.surface.width,
                    asset.surface.height,
                    asset.surface.pitch
                ),
                (4, 1, 4)
            );
            assert!(
                asset
                    .surface
                    .palette
                    .iter()
                    .zip(active.entries)
                    .all(|(actual, expected)| actual.red == expected.red
                        && actual.green == expected.green
                        && actual.blue == expected.blue
                        && actual.flags == 0)
            );
        }
    }

    #[test]
    fn optional_surface_failure_is_an_ordered_hole_without_shifting_required_slots() {
        let mut parts = complete_parts(VisualVariant::Normal);
        parts.assets[RootMenuBitmapSlot::MainShadow.index()]
            .bitmap
            .palette
            .pop();
        let prepared = prepare_parts(parts, &palette()).unwrap();

        assert!(prepared.get(RootMenuBitmapSlot::MainShadow).is_none());
        assert_eq!(prepared.loaded_len(), ROOT_MENU_BITMAP_REQUEST_COUNT - 1);
        assert!(matches!(
            prepared.optional_diagnostics().next().unwrap(),
            RootMenuSurfaceDiagnostic::Surface {
                slot: RootMenuBitmapSlot::MainShadow,
                identity: RootMenuBitmapIdentity::MainShadow,
                source: SurfaceError::EmbeddedPaletteEntryCount(255),
                ..
            }
        ));
        assert_eq!(
            prepared
                .get(RootMenuBitmapSlot::Backdrop)
                .unwrap()
                .surface
                .pixels[0],
            RootMenuBitmapSlot::Backdrop.index() as u8
        );
    }

    #[test]
    fn required_surface_failure_retains_slot_identity_path_and_cause() {
        let mut parts = complete_parts(VisualVariant::Normal);
        parts.assets[RootMenuBitmapSlot::Backdrop.index()]
            .bitmap
            .width = 3;
        parts.assets[RootMenuBitmapSlot::Backdrop.index()]
            .bitmap
            .pixels
            .pop();

        assert!(matches!(
            prepare_parts(parts, &palette()),
            Err(RootMenuSurfaceError::Surface {
                slot: RootMenuBitmapSlot::Backdrop,
                identity: RootMenuBitmapIdentity::Backdrop,
                path: crate::root_menu_assets::ROOT_MENU_BACKDROP_PATH,
                source: SurfaceError::UnsupportedUnalignedWidth(3),
            })
        ));
    }

    #[test]
    fn forged_identity_sets_fail_without_indexing_or_silent_compaction() {
        let mut unknown = complete_parts(VisualVariant::Normal);
        unknown.assets[0].identity = RootMenuBitmapIdentity::AnimationFrame(7);
        assert!(matches!(
            prepare_parts(unknown, &palette()),
            Err(RootMenuSurfaceError::UnknownIdentity(
                RootMenuBitmapIdentity::AnimationFrame(7)
            ))
        ));

        let mut wrong_variant = complete_parts(VisualVariant::Normal);
        wrong_variant.assets[RootMenuBitmapSlot::Cone.index()].identity =
            RootMenuBitmapIdentity::CenterIcon {
                identity: crate::root_menu_assets::CenterIconIdentity::Cone,
                variant: VisualVariant::GPrefixed,
            };
        assert!(matches!(
            prepare_parts(wrong_variant, &palette()),
            Err(RootMenuSurfaceError::CenterVariantMismatch {
                expected: VisualVariant::Normal,
                ..
            })
        ));

        let mut duplicate = complete_parts(VisualVariant::Normal);
        duplicate.assets[1].identity = RootMenuBitmapIdentity::MainSelected;
        assert!(matches!(
            prepare_parts(duplicate, &palette()),
            Err(RootMenuSurfaceError::DuplicateSlot(
                RootMenuBitmapSlot::MainSelected
            ))
        ));

        let mut missing = complete_parts(VisualVariant::Normal);
        missing.assets.remove(RootMenuBitmapSlot::Selection.index());
        assert!(matches!(
            prepare_parts(missing, &palette()),
            Err(RootMenuSurfaceError::MissingSlot(
                RootMenuBitmapSlot::Selection
            ))
        ));

        let mut required_diagnostic = complete_parts(VisualVariant::Normal);
        required_diagnostic
            .optional_diagnostics
            .push(RootMenuBitmapLoadError::Decode {
                identity: RootMenuBitmapIdentity::Backdrop,
                path: crate::root_menu_assets::ROOT_MENU_BACKDROP_PATH,
                source: BitmapError::NotBitmap,
            });
        assert!(matches!(
            prepare_parts(required_diagnostic, &palette()),
            Err(RootMenuSurfaceError::UnexpectedRequiredDiagnostic(
                RootMenuBitmapSlot::Backdrop
            ))
        ));
    }

    #[test]
    fn complete_upload_preserves_all_fixed_slots_and_global_store_lifetime() {
        let prepared = prepare_parts(complete_parts(VisualVariant::GPrefixed), &palette()).unwrap();
        let mut backend = RecordingBackend::default();
        {
            let mut store = UploadedSourceStore::new(&mut backend);
            let uploaded = upload_root_menu_bitmaps(&mut store, prepared).unwrap();
            assert!(uploaded.optional_diagnostics.is_empty());
            assert_eq!(uploaded.pool.language(), Language::English);
            assert_eq!(uploaded.pool.center_variant(), VisualVariant::GPrefixed);
            for slot in RootMenuBitmapSlot::ALL {
                assert!(uploaded.pool.is_loaded(slot));
                assert_eq!(uploaded.pool.extent(slot), Some((4, 1)));
            }
            uploaded
                .pool
                .blit(
                    &mut store,
                    RootMenuBitmapSlot::ChampionshipGlobe,
                    99,
                    one_pixel_blit(),
                )
                .unwrap();
            uploaded
                .pool
                .blit(
                    &mut store,
                    RootMenuBitmapSlot::TimeTrialGlobe,
                    99,
                    one_pixel_blit(),
                )
                .unwrap();
            assert_eq!(
                store.uploaded_source_count(),
                ROOT_MENU_BITMAP_REQUEST_COUNT
            );
            assert_eq!(
                &store.backend().events[ROOT_MENU_BITMAP_REQUEST_COUNT..],
                &[
                    Event::Blit {
                        source: 21,
                        destination: 99,
                    },
                    Event::Blit {
                        source: 22,
                        destination: 99,
                    },
                ]
            );
            discard_root_pool(uploaded.pool);
            assert_eq!(
                store.uploaded_source_count(),
                ROOT_MENU_BITMAP_REQUEST_COUNT
            );
        }
        assert_eq!(
            backend
                .events
                .iter()
                .filter_map(|event| match event {
                    Event::Destroy(handle) => Some(*handle),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            (10..10 + ROOT_MENU_BITMAP_REQUEST_COUNT as u8)
                .rev()
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn preparation_and_upload_failures_are_ordered_optional_holes() {
        let mut parts = complete_parts(VisualVariant::Normal);
        parts.assets[RootMenuBitmapSlot::MainSelected.index()]
            .bitmap
            .palette
            .pop();
        let prepared = prepare_parts(parts, &palette()).unwrap();
        let mut backend = RecordingBackend {
            fail_create_at: Some(1),
            ..RecordingBackend::default()
        };
        let mut store = UploadedSourceStore::new(&mut backend);
        let uploaded = upload_root_menu_bitmaps(&mut store, prepared).unwrap();

        assert!(!uploaded.pool.is_loaded(RootMenuBitmapSlot::MainSelected));
        assert!(uploaded.pool.is_loaded(RootMenuBitmapSlot::MainShadow));
        assert!(!uploaded.pool.is_loaded(RootMenuBitmapSlot::Animation0));
        assert!(uploaded.pool.is_loaded(RootMenuBitmapSlot::Animation1));
        let visual_assets = RootVisualAssets::from_bitmap_pool(&uploaded.pool).unwrap();
        assert!(!visual_assets.selected_chrome_loaded);
        assert!(visual_assets.shadow_loaded);
        assert!(!visual_assets.animations_loaded[0]);
        assert!(visual_assets.animations_loaded[1]);
        assert_eq!(
            store.uploaded_source_count(),
            ROOT_MENU_BITMAP_REQUEST_COUNT - 2
        );
        assert_eq!(
            uploaded
                .optional_diagnostics
                .iter()
                .map(RootMenuBitmapUploadDiagnostic::slot)
                .collect::<Vec<_>>(),
            [
                RootMenuBitmapSlot::MainSelected,
                RootMenuBitmapSlot::Animation0,
            ]
        );
        assert!(matches!(
            &uploaded.optional_diagnostics[0],
            RootMenuBitmapUploadDiagnostic::Preparation {
                slot: RootMenuBitmapSlot::MainSelected,
                ..
            }
        ));
        assert!(matches!(
            &uploaded.optional_diagnostics[1],
            RootMenuBitmapUploadDiagnostic::Upload {
                slot: RootMenuBitmapSlot::Animation0,
                identity: RootMenuBitmapIdentity::AnimationFrame(0),
                source: SourceUploadError::Backend(BackendError::Create(1)),
                ..
            }
        ));
        assert_eq!(
            uploaded.pool.blit(
                &mut store,
                RootMenuBitmapSlot::Animation0,
                99,
                one_pixel_blit(),
            ),
            Err(RootMenuBitmapBlitError::MissingOptionalSlot(
                RootMenuBitmapSlot::Animation0
            ))
        );
    }

    #[test]
    fn required_upload_failure_rolls_back_only_root_batch_and_store_recovers() {
        let prepared = prepare_parts(complete_parts(VisualVariant::Normal), &palette()).unwrap();
        let mut backend = RecordingBackend {
            fail_create_at: Some(11),
            ..RecordingBackend::default()
        };
        {
            let mut store = UploadedSourceStore::new(&mut backend);
            store.upload(&marker_surface(90)).unwrap();
            assert!(matches!(
                upload_root_menu_bitmaps(&mut store, prepared),
                Err(RootMenuBitmapUploadError {
                    slot: RootMenuBitmapSlot::Selection,
                    identity: RootMenuBitmapIdentity::Selection,
                    source: SourceUploadError::Backend(BackendError::Create(11)),
                    ..
                })
            ));
            assert_eq!(store.uploaded_source_count(), 1);
            store.upload(&marker_surface(91)).unwrap();
            assert_eq!(store.uploaded_source_count(), 2);
        }
        assert_eq!(
            backend
                .events
                .iter()
                .filter_map(|event| match event {
                    Event::Destroy(handle) => Some(*handle),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            (11..=20).rev().chain([22, 10]).collect::<Vec<_>>()
        );
    }

    #[test]
    fn wrong_store_and_backend_blit_errors_remain_typed() {
        let prepared = prepare_parts(complete_parts(VisualVariant::Normal), &palette()).unwrap();
        let mut first_backend = RecordingBackend::default();
        let mut first_store = UploadedSourceStore::new(&mut first_backend);
        let uploaded = upload_root_menu_bitmaps(&mut first_store, prepared).unwrap();
        let mut second_backend = RecordingBackend {
            fail_blit: Cell::new(true),
            ..RecordingBackend::default()
        };
        let mut second_store = UploadedSourceStore::new(&mut second_backend);
        assert!(matches!(
            uploaded.pool.blit(
                &mut second_store,
                RootMenuBitmapSlot::Backdrop,
                99,
                one_pixel_blit(),
            ),
            Err(RootMenuBitmapBlitError::Access {
                slot: RootMenuBitmapSlot::Backdrop,
                source: SourceAccessError::ForeignKey(_),
            })
        ));
        assert!(second_store.backend().events.is_empty());

        first_store.backend().fail_blit.set(true);
        assert_eq!(
            uploaded.pool.blit(
                &mut first_store,
                RootMenuBitmapSlot::Backdrop,
                99,
                one_pixel_blit(),
            ),
            Err(RootMenuBitmapBlitError::Backend {
                slot: RootMenuBitmapSlot::Backdrop,
                source: BackendError::Blit,
            })
        );
    }

    #[test]
    fn font_and_root_pools_coexist_and_reuse_one_store_across_frames() {
        let prepared = prepare_parts(complete_parts(VisualVariant::Normal), &palette()).unwrap();
        let mut backend = RecordingBackend::default();
        {
            let mut store = UploadedSourceStore::new(&mut backend);
            let fonts = UploadedFontPool::upload(&mut store, font_set()).unwrap();
            let root = upload_root_menu_bitmaps(&mut store, prepared).unwrap().pool;
            assert_eq!(
                store.uploaded_source_count(),
                FONT_COUNT + ROOT_MENU_BITMAP_REQUEST_COUNT
            );

            let style = FontStyle::Title;
            let plan = layout_text(
                fonts.font(style),
                b"A",
                PointI32 { x: 0, y: 0 },
                4,
                TextAlignment {
                    horizontal: HorizontalAlignment::Left,
                    vertical: cmr_formats::font::VerticalAlignment::Top,
                },
                TextLayoutLimits::default(),
            )
            .unwrap();
            for _ in 0..2 {
                assert_eq!(
                    fonts
                        .execute(
                            &mut store,
                            style,
                            &plan,
                            99,
                            crate::display::DisplayExtent {
                                width: 4,
                                height: 1,
                            },
                        )
                        .unwrap()
                        .successful_blits,
                    1
                );
                root.blit(
                    &mut store,
                    RootMenuBitmapSlot::Backdrop,
                    99,
                    one_pixel_blit(),
                )
                .unwrap();
            }
            assert_eq!(
                store.backend().create_attempts,
                FONT_COUNT + ROOT_MENU_BITMAP_REQUEST_COUNT
            );
            discard_pools(fonts, root);
            assert_eq!(
                store.uploaded_source_count(),
                FONT_COUNT + ROOT_MENU_BITMAP_REQUEST_COUNT
            );
        }
        assert_eq!(
            backend
                .events
                .iter()
                .filter(|event| matches!(event, Event::Destroy(_)))
                .count(),
            FONT_COUNT + ROOT_MENU_BITMAP_REQUEST_COUNT
        );
    }

    #[test]
    fn command_executor_derives_exact_slots_geometry_and_color_keys() {
        let prepared = prepare_parts(complete_parts(VisualVariant::Normal), &palette()).unwrap();
        let mut backend = RecordingBackend::default();
        let mut store = UploadedSourceStore::new(&mut backend);
        let pool = upload_root_menu_bitmaps(&mut store, prepared).unwrap().pool;
        let commands = [
            RenderCommand::Backdrop {
                slot: RootMenuBitmapSlot::Backdrop,
                source: Rect::new(1, 0, 2, 1),
                destination: Point::new(2, 3),
            },
            RenderCommand::ChromeShadow {
                slot: RootMenuBitmapSlot::MainShadow,
                entry: 0,
                source: Rect::new(0, 0, 1, 1),
                destination: Point::new(1, 2),
            },
            RenderCommand::ChromeAnimation {
                slot: RootMenuBitmapSlot::Animation2,
                entry: 2,
                source: Rect::new(0, 0, 1, 1),
                destination: Point::new(2, 2),
            },
            RenderCommand::SelectedChrome {
                slot: RootMenuBitmapSlot::MainSelected,
                entry: 0,
                source: Rect::new(0, 0, 1, 1),
                destination: Point::new(3, 2),
            },
            RenderCommand::CenterIcon {
                slot: RootMenuBitmapSlot::Cone,
                visual_index: 2,
                destination: Point::new(4, 2),
            },
        ];
        for command in commands {
            assert_eq!(
                execute_root_bitmap_command(
                    &mut store,
                    &pool,
                    command,
                    99,
                    DisplayExtent {
                        width: 10,
                        height: 10,
                    },
                ),
                Ok(RootBitmapCommandOutcome::Blitted)
            );
        }
        assert_eq!(
            store
                .backend()
                .blits
                .iter()
                .map(|plan| plan.legacy_flags())
                .collect::<Vec<_>>(),
            [0x10, 0x11, 0x11, 0x11, 0x11]
        );
        assert_eq!(store.backend().blits[0].color_key, None);
        assert!(
            store.backend().blits[1..]
                .iter()
                .all(|plan| { plan.color_key == Some(ColorKey { low: 0, high: 0 }) })
        );
        assert_eq!(
            store.backend().blits[0].source,
            crate::blit::RectEdges {
                left: 1,
                top: 0,
                right: 3,
                bottom: 1,
            }
        );
        assert_eq!(
            store.backend().blits[4].source,
            crate::blit::RectEdges {
                left: 0,
                top: 0,
                right: 4,
                bottom: 1,
            }
        );
    }

    #[test]
    fn command_executor_clips_or_no_ops_without_reuploading() {
        let prepared = prepare_parts(complete_parts(VisualVariant::Normal), &palette()).unwrap();
        let mut backend = RecordingBackend::default();
        let mut store = UploadedSourceStore::new(&mut backend);
        let pool = upload_root_menu_bitmaps(&mut store, prepared).unwrap().pool;
        let source_count = store.uploaded_source_count();
        assert_eq!(
            execute_root_bitmap_command(
                &mut store,
                &pool,
                RenderCommand::SelectedChrome {
                    slot: RootMenuBitmapSlot::MainSelected,
                    entry: 0,
                    source: Rect::new(0, 0, 2, 1),
                    destination: Point::new(-1, 0),
                },
                99,
                DisplayExtent {
                    width: 4,
                    height: 1,
                },
            ),
            Ok(RootBitmapCommandOutcome::Blitted)
        );
        assert_eq!(
            store.backend().blits.last().unwrap().source,
            crate::blit::RectEdges {
                left: 1,
                top: 0,
                right: 2,
                bottom: 1,
            }
        );
        let prior_blits = store.backend().blits.len();
        assert_eq!(
            execute_root_bitmap_command(
                &mut store,
                &pool,
                RenderCommand::SelectedChrome {
                    slot: RootMenuBitmapSlot::MainSelected,
                    entry: 0,
                    source: Rect::new(0, 0, 1, 1),
                    destination: Point::new(-1, 0),
                },
                99,
                DisplayExtent {
                    width: 4,
                    height: 1,
                },
            ),
            Ok(RootBitmapCommandOutcome::NoOp)
        );
        assert_eq!(store.backend().blits.len(), prior_blits);
        assert_eq!(store.uploaded_source_count(), source_count);
    }

    #[test]
    fn command_executor_rejects_forged_commands_before_backend_effects() {
        let prepared = prepare_parts(complete_parts(VisualVariant::Normal), &palette()).unwrap();
        let mut backend = RecordingBackend::default();
        let mut store = UploadedSourceStore::new(&mut backend);
        let pool = upload_root_menu_bitmaps(&mut store, prepared).unwrap().pool;
        let extent = DisplayExtent {
            width: 4,
            height: 1,
        };
        let cases = [
            execute_root_bitmap_command(
                &mut store,
                &pool,
                RenderCommand::SelectedChrome {
                    slot: RootMenuBitmapSlot::MainShadow,
                    entry: 0,
                    source: Rect::new(0, 0, 1, 1),
                    destination: Point::new(0, 0),
                },
                99,
                extent,
            ),
            execute_root_bitmap_command(
                &mut store,
                &pool,
                RenderCommand::ChromeAnimation {
                    slot: RootMenuBitmapSlot::Animation0,
                    entry: 7,
                    source: Rect::new(0, 0, 1, 1),
                    destination: Point::new(0, 0),
                },
                99,
                extent,
            ),
            execute_root_bitmap_command(
                &mut store,
                &pool,
                RenderCommand::ChromeShadow {
                    slot: RootMenuBitmapSlot::MainShadow,
                    entry: 7,
                    source: Rect::new(0, 0, 1, 1),
                    destination: Point::new(0, 0),
                },
                99,
                extent,
            ),
            execute_root_bitmap_command(
                &mut store,
                &pool,
                RenderCommand::SelectedChrome {
                    slot: RootMenuBitmapSlot::MainSelected,
                    entry: 7,
                    source: Rect::new(0, 0, 1, 1),
                    destination: Point::new(0, 0),
                },
                99,
                extent,
            ),
            execute_root_bitmap_command(
                &mut store,
                &pool,
                RenderCommand::Backdrop {
                    slot: RootMenuBitmapSlot::Backdrop,
                    source: Rect::new(4, 0, 1, 1),
                    destination: Point::new(0, 0),
                },
                99,
                extent,
            ),
            execute_root_bitmap_command(
                &mut store,
                &pool,
                RenderCommand::Backdrop {
                    slot: RootMenuBitmapSlot::Backdrop,
                    source: Rect::new(0, 0, 1, 1),
                    destination: Point::new(i32::MAX, 0),
                },
                99,
                extent,
            ),
            execute_root_bitmap_command(
                &mut store,
                &pool,
                RenderCommand::Backdrop {
                    slot: RootMenuBitmapSlot::Backdrop,
                    source: Rect::new(0, 0, 1, 1),
                    destination: Point::new(0, 0),
                },
                99,
                DisplayExtent {
                    width: 0,
                    height: 1,
                },
            ),
        ];
        assert!(matches!(
            cases[0],
            Err(RootBitmapCommandError::UnexpectedSlot { .. })
        ));
        assert!(matches!(
            cases[1],
            Err(RootBitmapCommandError::EntryOutOfRange { .. })
        ));
        assert!(matches!(
            cases[2],
            Err(RootBitmapCommandError::EntryOutOfRange {
                command: RootBitmapCommandKind::ChromeShadow,
                ..
            })
        ));
        assert!(matches!(
            cases[3],
            Err(RootBitmapCommandError::EntryOutOfRange {
                command: RootBitmapCommandKind::SelectedChrome,
                ..
            })
        ));
        assert!(matches!(
            cases[4],
            Err(RootBitmapCommandError::SourceOutOfBounds { .. })
        ));
        assert!(matches!(
            cases[5],
            Err(RootBitmapCommandError::GeometryOutOfRange { .. })
        ));
        assert!(matches!(
            cases[6],
            Err(RootBitmapCommandError::InvalidDestinationExtent(_))
        ));
        assert!(store.backend().blits.is_empty());
    }

    #[test]
    fn command_executor_returns_nonbitmap_missing_access_and_backend_outcomes() {
        let mut parts = complete_parts(VisualVariant::Normal);
        parts.assets[RootMenuBitmapSlot::MainSelected.index()]
            .bitmap
            .palette
            .pop();
        let prepared = prepare_parts(parts, &palette()).unwrap();
        let mut first_backend = RecordingBackend::default();
        let mut first_store = UploadedSourceStore::new(&mut first_backend);
        let pool = upload_root_menu_bitmaps(&mut first_store, prepared)
            .unwrap()
            .pool;
        let extent = DisplayExtent {
            width: 4,
            height: 1,
        };
        for command in [
            RenderCommand::FooterText {
                text_index: 253,
                position: Point::new(320, 470),
                flags: 0x22,
            },
            RenderCommand::C3dObject {
                slot: RootMenuC3dSlot::Championship,
                angle_delta: 8,
            },
            RenderCommand::ComposeAndPresent,
        ] {
            assert_eq!(
                execute_root_bitmap_command(&mut first_store, &pool, command, 99, extent,),
                Ok(RootBitmapCommandOutcome::NotBitmap)
            );
        }
        assert!(matches!(
            execute_root_bitmap_command(
                &mut first_store,
                &pool,
                RenderCommand::SelectedChrome {
                    slot: RootMenuBitmapSlot::MainSelected,
                    entry: 0,
                    source: Rect::new(0, 0, 1, 1),
                    destination: Point::new(0, 0),
                },
                99,
                extent,
            ),
            Err(RootBitmapCommandError::MissingSlot(
                RootMenuBitmapSlot::MainSelected
            ))
        ));

        let backdrop = RenderCommand::Backdrop {
            slot: RootMenuBitmapSlot::Backdrop,
            source: Rect::new(0, 0, 1, 1),
            destination: Point::new(0, 0),
        };
        let mut second_backend = RecordingBackend::default();
        let mut second_store = UploadedSourceStore::new(&mut second_backend);
        assert!(matches!(
            execute_root_bitmap_command(&mut second_store, &pool, backdrop, 99, extent),
            Err(RootBitmapCommandError::Access {
                slot: RootMenuBitmapSlot::Backdrop,
                source: SourceAccessError::ForeignKey(_),
            })
        ));
        first_store.backend().fail_blit.set(true);
        assert_eq!(
            execute_root_bitmap_command(&mut first_store, &pool, backdrop, 99, extent),
            Err(RootBitmapCommandError::Backend {
                slot: RootMenuBitmapSlot::Backdrop,
                source: BackendError::Blit,
            })
        );
    }

    fn marker_surface(marker: u8) -> IndexedSurface {
        IndexedSurface {
            width: 1,
            height: 1,
            pitch: 1,
            pixels: vec![marker],
            palette: [PaletteEntry {
                red: 0,
                green: 0,
                blue: 0,
                flags: 0,
            }; 256],
        }
    }

    fn discard_root_pool(_pool: UploadedRootMenuBitmapPool) {}

    fn discard_pools(_fonts: UploadedFontPool, _root: UploadedRootMenuBitmapPool) {}

    fn font_set() -> FontSet {
        let font = parse_legacy_font(&synthetic_font(), FontLimits::default()).unwrap();
        FontSet::from_ordered_fonts(std::array::from_fn(|_| font.clone()))
    }

    fn synthetic_font() -> Vec<u8> {
        const PIXEL_OFFSET: usize = 0x436;
        const FONT_HEADER_SIZE: usize = 24;
        const GLYPH_SIZE: usize = 16;
        let trailer = PIXEL_OFFSET + 4;
        let mut input = vec![0; trailer + FONT_HEADER_SIZE + GLYPH_SIZE];
        input[0..2].copy_from_slice(b"BM");
        input[18..22].copy_from_slice(&4i32.to_le_bytes());
        input[22..26].copy_from_slice(&1i32.to_le_bytes());
        input[28..30].copy_from_slice(&8u16.to_le_bytes());
        input[PIXEL_OFFSET..PIXEL_OFFSET + 4].fill(7);
        input[trailer + 1] = 1;
        input[trailer + 4..trailer + 8].copy_from_slice(&4u32.to_le_bytes());
        input[trailer + 12..trailer + 16].copy_from_slice(&1u32.to_le_bytes());
        let glyph = trailer + FONT_HEADER_SIZE;
        input[glyph] = b'A';
        input[glyph + 6..glyph + 8].copy_from_slice(&1i16.to_le_bytes());
        input[glyph + 8..glyph + 10].copy_from_slice(&1i16.to_le_bytes());
        input
    }

    fn one_pixel_blit() -> BlitPlan {
        let BlitDecision::Blit(plan) = plan_legacy_blit(
            RectI16 {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
            RectI16 {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
            1,
            1,
            None,
        ) else {
            unreachable!()
        };
        plan
    }
}
