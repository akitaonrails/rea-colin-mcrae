//! Preparation boundary between decoded root-menu bitmaps and indexed video.
//!
//! The original loader preserves source indices and does not attach each BMP's
//! palette to its surface. Reconstruction therefore uses the caller's active
//! 256-entry display palette with identity index mapping. Preparation has no
//! backend effects; persistent upload is a separate resource-lifetime step.

use std::fmt;

use cmr_formats::riff_palette::LegacyPalette256;

use crate::localization::Language;
use crate::root_menu_assets::VisualVariant;
use crate::root_menu_bitmap_assets::{
    ROOT_MENU_BITMAP_REQUEST_COUNT, RootMenuBitmapIdentity, RootMenuBitmapLoadError,
    RootMenuBitmapParts, RootMenuBitmapSet, RootMenuBitmapSlot,
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

fn prepare_parts(
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
    use cmr_formats::bitmap::{BitmapError, BitmapMetadata, BitmapPaletteEntry, IndexedBitmap};
    use cmr_formats::riff_palette::PaletteEntry;

    use super::*;
    use crate::root_menu_bitmap_assets::RootMenuBitmapAsset;

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
}
