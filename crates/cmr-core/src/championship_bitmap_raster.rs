//! Indexed-surface preparation and persistent upload for championship logos.
//!
//! The enclosing frontend owns the active palette and explicitly selects the
//! index-copy policy. Car objects, car palettes, selection art,
//! fonts, text, draw planning, and frame execution stay outside this module.

use std::fmt;

use cmr_formats::riff_palette::LegacyPalette256;

use crate::championship_assets::{
    ChampionshipAssets, MANUFACTURER_LOGO_COUNT, ManufacturerLogo, ManufacturerLogoAsset,
};
use crate::championship_setup::CanonicalCar;
use crate::display::IndexedVideoBackend;
use crate::source_store::{SourceUploadError, UploadedIndexedBitmap, UploadedSourceStore};
use crate::surface::{IndexedSurface, PaletteMapping, SurfaceError, indexed_surface};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChampionshipLogoSurfaceAsset {
    pub identity: ManufacturerLogo,
    pub path: &'static str,
    pub surface: IndexedSurface,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChampionshipLogoSurfaceSet {
    logos: [ChampionshipLogoSurfaceAsset; MANUFACTURER_LOGO_COUNT],
}

impl ChampionshipLogoSurfaceSet {
    #[must_use]
    pub fn get(&self, logo: ManufacturerLogo) -> &ChampionshipLogoSurfaceAsset {
        &self.logos[logo.index()]
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = &ChampionshipLogoSurfaceAsset> {
        self.logos.iter()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChampionshipLogoSurfaceError {
    pub logo: ManufacturerLogo,
    pub path: &'static str,
    pub source: SurfaceError,
}

impl fmt::Display for ChampionshipLogoSurfaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "cannot prepare championship manufacturer logo {:?} at {}: {}",
            self.logo, self.path, self.source
        )
    }
}

impl std::error::Error for ChampionshipLogoSurfaceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// Converts all 11 unique decoded logos with caller-selected palette policy.
///
/// The decoded logo assets remain owned by `assets`; surface preparation clones
/// only their bounded bitmap values. No mapping mode is inferred here.
pub fn prepare_championship_logo_surfaces(
    assets: &ChampionshipAssets,
    active_palette: &LegacyPalette256,
    mapping: PaletteMapping,
) -> Result<ChampionshipLogoSurfaceSet, ChampionshipLogoSurfaceError> {
    prepare_logo_assets(assets.logos().cloned(), active_palette, mapping)
}

fn prepare_logo_assets(
    logos: impl IntoIterator<Item = ManufacturerLogoAsset>,
    active_palette: &LegacyPalette256,
    mapping: PaletteMapping,
) -> Result<ChampionshipLogoSurfaceSet, ChampionshipLogoSurfaceError> {
    let mut surfaces = Vec::with_capacity(MANUFACTURER_LOGO_COUNT);
    for asset in logos {
        let logo = asset.descriptor.identity;
        let path = asset.descriptor.path;
        let surface = indexed_surface(asset.bitmap, active_palette.clone(), mapping)
            .map_err(|source| ChampionshipLogoSurfaceError { logo, path, source })?;
        surfaces.push(ChampionshipLogoSurfaceAsset {
            identity: logo,
            path,
            surface,
        });
    }

    let logos = surfaces
        .try_into()
        .expect("championship assets provide exactly 11 unique manufacturer logos");
    Ok(ChampionshipLogoSurfaceSet { logos })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UploadedChampionshipLogoPool {
    logos: [UploadedIndexedBitmap; MANUFACTURER_LOGO_COUNT],
}

impl UploadedChampionshipLogoPool {
    #[cfg(test)]
    pub(crate) const fn test_repeated(bitmap: UploadedIndexedBitmap) -> Self {
        Self {
            logos: [bitmap; MANUFACTURER_LOGO_COUNT],
        }
    }
    #[must_use]
    pub const fn logo(&self, logo: ManufacturerLogo) -> UploadedIndexedBitmap {
        self.logos[logo.index()]
    }

    #[must_use]
    pub fn logo_for_car(&self, car: CanonicalCar) -> UploadedIndexedBitmap {
        self.logo(car.championship_descriptor().logo)
    }

    #[must_use]
    pub fn extent(&self, logo: ManufacturerLogo) -> (u32, u32) {
        self.logo(logo).extent()
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = UploadedIndexedBitmap> + '_ {
        self.logos.iter().copied()
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct ChampionshipLogoUploadError<E> {
    pub logo: ManufacturerLogo,
    pub path: &'static str,
    pub source: SourceUploadError<E>,
}

impl<E: fmt::Display> fmt::Display for ChampionshipLogoUploadError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "cannot upload championship manufacturer logo {:?} at {}: {}",
            self.logo, self.path, self.source
        )
    }
}

impl<E: std::error::Error + 'static> std::error::Error for ChampionshipLogoUploadError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// Uploads exactly 11 unique logo surfaces as one source-store transaction.
///
/// A failed upload rolls back only this batch; sources committed before the
/// transaction remain live in the shared store.
pub fn upload_championship_logos<B: IndexedVideoBackend>(
    store: &mut UploadedSourceStore<'_, B>,
    surfaces: ChampionshipLogoSurfaceSet,
) -> Result<UploadedChampionshipLogoPool, ChampionshipLogoUploadError<B::Error>> {
    let mut transaction = store.transaction();
    let mut uploaded = Vec::with_capacity(MANUFACTURER_LOGO_COUNT);
    for asset in surfaces.logos {
        let bitmap = transaction
            .upload_bitmap(&asset.surface)
            .map_err(|source| ChampionshipLogoUploadError {
                logo: asset.identity,
                path: asset.path,
                source,
            })?;
        uploaded.push(bitmap);
    }
    transaction.commit();

    Ok(UploadedChampionshipLogoPool {
        logos: uploaded
            .try_into()
            .expect("one uploaded source exists for every unique manufacturer logo"),
    })
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use cmr_formats::bitmap::{BitmapMetadata, BitmapPaletteEntry, IndexedBitmap};
    use cmr_formats::riff_palette::PaletteEntry;

    use super::*;
    use crate::blit::BlitPlan;
    use crate::championship_assets::{MANUFACTURER_LOGOS, ManufacturerLogoDescriptor};
    use crate::display::{CooperativeMode, DisplaySurfaces, IndexedMode, SurfaceTopology};

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum BackendError {
        Create(usize),
    }

    impl fmt::Display for BackendError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "{self:?}")
        }
    }

    impl std::error::Error for BackendError {}

    #[derive(Clone, Debug, Eq, PartialEq)]
    enum Event {
        Create {
            marker: u8,
            extent: (u32, u32, u32),
            handle: u8,
        },
        Destroy(u8),
    }

    #[derive(Default)]
    struct RecordingBackend {
        events: Vec<Event>,
        attempts: usize,
        fail_at: Option<usize>,
        unexpected_blit: Cell<bool>,
    }

    impl IndexedVideoBackend for RecordingBackend {
        type Error = BackendError;
        type Surface = u8;
        type Palette = u8;

        fn create_device(&mut self) -> Result<(), Self::Error> {
            unreachable!()
        }

        fn set_cooperative_mode(&mut self, _: CooperativeMode) -> Result<(), Self::Error> {
            unreachable!()
        }

        fn set_display_mode(&mut self, _: IndexedMode) -> Result<(), Self::Error> {
            unreachable!()
        }

        fn create_surfaces(
            &mut self,
            _: SurfaceTopology,
            _: IndexedMode,
        ) -> Result<DisplaySurfaces<Self::Surface>, Self::Error> {
            unreachable!()
        }

        fn create_palette(
            &mut self,
            _: &[PaletteEntry; 256],
        ) -> Result<Self::Palette, Self::Error> {
            unreachable!()
        }

        fn attach_palette(
            &mut self,
            _: Self::Surface,
            _: Self::Palette,
        ) -> Result<(), Self::Error> {
            unreachable!()
        }

        fn upload_palette(
            &mut self,
            _: Self::Palette,
            _: &[PaletteEntry; 256],
        ) -> Result<(), Self::Error> {
            unreachable!()
        }

        fn create_source(
            &mut self,
            surface: &IndexedSurface,
        ) -> Result<Self::Surface, Self::Error> {
            let attempt = self.attempts;
            self.attempts += 1;
            if self.fail_at == Some(attempt) {
                return Err(BackendError::Create(attempt));
            }
            let handle = 10 + attempt as u8;
            self.events.push(Event::Create {
                marker: surface.pixels[0],
                extent: (surface.width, surface.height, surface.pitch),
                handle,
            });
            Ok(handle)
        }

        fn blit(
            &mut self,
            _: Self::Surface,
            _: Self::Surface,
            _: BlitPlan,
        ) -> Result<(), Self::Error> {
            self.unexpected_blit.set(true);
            unreachable!()
        }

        fn destroy_source(&mut self, source: Self::Surface) {
            self.events.push(Event::Destroy(source));
        }
    }

    fn active_palette() -> LegacyPalette256 {
        LegacyPalette256 {
            entries: std::array::from_fn(|index| PaletteEntry {
                red: index as u8,
                green: 0,
                blue: 0,
                flags: 99,
            }),
        }
    }

    fn bitmap(marker: u8) -> IndexedBitmap {
        let width = 182u32;
        let height = 128u32;
        let row_stride = 184u32;
        let mut palette = vec![
            BitmapPaletteEntry {
                red: 0,
                green: 0,
                blue: 0,
                reserved: 0,
            };
            256
        ];
        for (index, entry) in palette.iter_mut().enumerate() {
            entry.red = index as u8;
        }
        let mut stored_pixels = Vec::with_capacity((row_stride * height) as usize);
        for _ in 0..height {
            stored_pixels.extend(std::iter::repeat_n(marker, width as usize));
            stored_pixels.extend([201, 202]);
        }
        IndexedBitmap {
            width,
            height,
            source_was_top_down: false,
            palette,
            pixels: vec![marker; (width * height) as usize],
            row_stride,
            stored_pixels,
            metadata: BitmapMetadata {
                reserved_1: 0,
                reserved_2: 0,
                x_pixels_per_meter: 0,
                y_pixels_per_meter: 0,
                important_colors: 0,
            },
        }
    }

    fn logo_assets() -> [ManufacturerLogoAsset; MANUFACTURER_LOGO_COUNT] {
        std::array::from_fn(|index| ManufacturerLogoAsset {
            descriptor: MANUFACTURER_LOGOS[index],
            bitmap: bitmap(20 + index as u8),
        })
    }

    fn prepare(mapping: PaletteMapping) -> ChampionshipLogoSurfaceSet {
        prepare_logo_assets(logo_assets(), &active_palette(), mapping).unwrap()
    }

    fn base_surface(marker: u8) -> IndexedSurface {
        IndexedSurface {
            width: 4,
            height: 1,
            pitch: 4,
            pixels: vec![marker; 4],
            palette: active_palette().entries,
        }
    }

    #[test]
    fn identity_preparation_preserves_aligned_extent_padding_and_active_palette() {
        let active = active_palette();
        let prepared =
            prepare_logo_assets(logo_assets(), &active, PaletteMapping::Identity).unwrap();

        assert_eq!(prepared.iter().len(), MANUFACTURER_LOGO_COUNT);
        for logo in ManufacturerLogo::ALL {
            let asset = prepared.get(logo);
            assert_eq!(asset.identity, logo);
            assert_eq!(asset.path, logo.descriptor().path);
            assert_eq!(
                (
                    asset.surface.width,
                    asset.surface.height,
                    asset.surface.pitch
                ),
                (184, 128, 184)
            );
            assert_eq!(&asset.surface.pixels[182..184], &[201, 202]);
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
    fn caller_explicitly_selects_identity_or_nearest_nonzero_mapping() {
        let mut logos = logo_assets();
        let subaru = &mut logos[ManufacturerLogo::Subaru.index()].bitmap;
        subaru.palette[1].red = 5;
        subaru.palette[201].red = 6;
        subaru.palette[202].red = 7;
        subaru.pixels.fill(1);
        for row in subaru.stored_pixels.as_chunks_mut::<184>().0 {
            row[..182].fill(1);
        }
        let identity =
            prepare_logo_assets(logos.clone(), &active_palette(), PaletteMapping::Identity)
                .unwrap();
        let remapped =
            prepare_logo_assets(logos, &active_palette(), PaletteMapping::NearestNonzero).unwrap();

        let identity = &identity.get(ManufacturerLogo::Subaru).surface.pixels;
        let remapped = &remapped.get(ManufacturerLogo::Subaru).surface.pixels;
        assert_eq!(&identity[..3], &[1, 1, 1]);
        assert_eq!(&identity[182..184], &[201, 202]);
        assert_eq!(&remapped[..3], &[5, 5, 5]);
        assert_eq!(&remapped[182..184], &[6, 7]);
    }

    #[test]
    fn surface_error_retains_logo_identity_and_path() {
        let mut logos = logo_assets();
        logos[ManufacturerLogo::Ford.index()].bitmap.palette.pop();
        assert_eq!(
            prepare_logo_assets(logos, &active_palette(), PaletteMapping::Identity),
            Err(ChampionshipLogoSurfaceError {
                logo: ManufacturerLogo::Ford,
                path: ManufacturerLogo::Ford.descriptor().path,
                source: SurfaceError::EmbeddedPaletteEntryCount(255),
            })
        );
    }

    #[test]
    fn upload_uses_eleven_unique_sources_and_car_aliases_share_one_handle() {
        let prepared = prepare(PaletteMapping::Identity);
        let mut backend = RecordingBackend::default();
        {
            let mut store = UploadedSourceStore::new(&mut backend);
            let pool = upload_championship_logos(&mut store, prepared).unwrap();
            assert_eq!(store.uploaded_source_count(), MANUFACTURER_LOGO_COUNT);
            assert_eq!(pool.iter().len(), MANUFACTURER_LOGO_COUNT);
            assert_eq!(pool.extent(ManufacturerLogo::Subaru), (184, 128));
            assert_eq!(
                pool.logo_for_car(CanonicalCar::Corolla).source_key(),
                pool.logo_for_car(CanonicalCar::Celica).source_key()
            );
            assert_eq!(
                pool.logo_for_car(CanonicalCar::Rs200).source_key(),
                pool.logo_for_car(CanonicalCar::Escort).source_key()
            );
        }
        assert_eq!(
            backend
                .events
                .iter()
                .filter(|event| matches!(event, Event::Create { .. }))
                .count(),
            MANUFACTURER_LOGO_COUNT
        );
    }

    #[test]
    fn upload_failure_rolls_back_only_logo_batch_and_preserves_path_context() {
        let prepared = prepare(PaletteMapping::Identity);
        let mut backend = RecordingBackend {
            fail_at: Some(6),
            ..RecordingBackend::default()
        };
        {
            let mut store = UploadedSourceStore::new(&mut backend);
            store.upload(&base_surface(90)).unwrap();
            let error = upload_championship_logos(&mut store, prepared).unwrap_err();
            assert_eq!(
                error,
                ChampionshipLogoUploadError {
                    logo: ManufacturerLogo::Mitsu,
                    path: ManufacturerLogo::Mitsu.descriptor().path,
                    source: SourceUploadError::Backend(BackendError::Create(6)),
                }
            );
            assert_eq!(store.uploaded_source_count(), 1);
            store.upload(&base_surface(91)).unwrap();
            assert_eq!(store.uploaded_source_count(), 2);
        }
        assert_eq!(
            backend
                .events
                .iter()
                .filter_map(|event| match event {
                    Event::Destroy(handle) => Some(*handle),
                    Event::Create { .. } => None,
                })
                .collect::<Vec<_>>(),
            vec![15, 14, 13, 12, 11, 17, 10]
        );
    }

    #[test]
    fn logo_pool_coexists_with_sources_committed_before_and_after_it() {
        let prepared = prepare(PaletteMapping::Identity);
        let mut backend = RecordingBackend::default();
        {
            let mut store = UploadedSourceStore::new(&mut backend);
            let before = store.upload_bitmap(&base_surface(90)).unwrap();
            let pool = upload_championship_logos(&mut store, prepared).unwrap();
            let after = store.upload_bitmap(&base_surface(91)).unwrap();

            assert_eq!(store.uploaded_source_count(), MANUFACTURER_LOGO_COUNT + 2);
            assert_eq!(before.extent(), (4, 1));
            assert_eq!(pool.extent(ManufacturerLogo::Reno), (184, 128));
            assert_eq!(after.extent(), (4, 1));
            assert_ne!(
                before.source_key(),
                pool.logo(ManufacturerLogo::Subaru).source_key()
            );
            assert_ne!(
                after.source_key(),
                pool.logo(ManufacturerLogo::M4r).source_key()
            );
        }
        assert_eq!(
            backend
                .events
                .iter()
                .filter(|event| matches!(event, Event::Destroy(_)))
                .count(),
            MANUFACTURER_LOGO_COUNT + 2
        );
    }

    #[test]
    fn descriptors_used_by_test_assets_remain_unique_and_ordered() {
        assert_eq!(
            logo_assets().map(|asset| asset.descriptor),
            MANUFACTURER_LOGOS
        );
        assert_eq!(
            MANUFACTURER_LOGOS[ManufacturerLogo::M4r.index()],
            ManufacturerLogoDescriptor {
                identity: ManufacturerLogo::M4r,
                path: "New_Fe/Images/carlogos/M4R.bmp",
            }
        );
    }
}
