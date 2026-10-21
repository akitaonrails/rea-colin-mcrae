//! Source-owned logical RGB16 preparation for championship ordinary bitmaps.
//!
//! This pool converts the required R_INFO bitmap, 11 unique manufacturer
//! logos, and 15 Chequered frames in one deterministic, effect-free batch.
//! Every conversion uses the bitmap's embedded palette. Indexed upload
//! handles, active display palettes, FCHE substitution, native surface layout,
//! and presentation remain outside this boundary.

use std::fmt;

use cmr_core::{
    championship_assets::{ChampionshipAssets, MANUFACTURER_LOGO_COUNT, ManufacturerLogo},
    championship_frame_assets::{
        CHEQUERED_FRAME_COUNT, ChampionshipFrameAssets, ChequeredFrame, INFO_BOX_DESCRIPTOR,
    },
};
use cmr_formats::bitmap::IndexedBitmap;

use crate::ordinary_bitmap_rgb16::{
    OrdinaryBitmapLogicalRgb16Source, OrdinaryBitmapRgb16Error,
    convert_ordinary_bitmap_0x81_to_logical_rgb16,
};

pub const CHAMPIONSHIP_ORDINARY_RGB16_SOURCE_COUNT: usize =
    1 + MANUFACTURER_LOGO_COUNT + CHEQUERED_FRAME_COUNT;

/// Stable identity of an ordinary bitmap admitted to the championship RGB16
/// pool. The common selection bitmap is deliberately absent until its stretch
/// behavior has a recovered conversion contract.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ChampionshipOrdinaryRgb16Identity {
    InfoBox,
    ManufacturerLogo(ManufacturerLogo),
    ChequeredFrame(ChequeredFrame),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SoftwareChampionshipOrdinaryRgb16PoolError {
    pub identity: ChampionshipOrdinaryRgb16Identity,
    /// Original-media-relative descriptor path; never a resolved host path.
    pub path: &'static str,
    pub source: OrdinaryBitmapRgb16Error,
}

impl fmt::Display for SoftwareChampionshipOrdinaryRgb16PoolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "cannot prepare championship ordinary RGB16 source {:?} at {}: {}",
            self.identity, self.path, self.source
        )
    }
}

impl std::error::Error for SoftwareChampionshipOrdinaryRgb16PoolError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// Fixed-shape, backend-neutral ownership of the 27 converted championship
/// ordinary bitmaps.
///
/// Construction is transactional: no pool is returned unless all sources
/// validate and convert. The decoded user-owned assets remain borrowed by the
/// builder and are neither moved nor written elsewhere.
#[derive(Debug, Eq, PartialEq)]
pub struct SoftwareChampionshipOrdinaryRgb16Pool {
    display_format_discriminator: u8,
    info_box: OrdinaryBitmapLogicalRgb16Source,
    logos: [OrdinaryBitmapLogicalRgb16Source; MANUFACTURER_LOGO_COUNT],
    frames: [OrdinaryBitmapLogicalRgb16Source; CHEQUERED_FRAME_COUNT],
}

impl SoftwareChampionshipOrdinaryRgb16Pool {
    #[must_use]
    pub const fn display_format_discriminator(&self) -> u8 {
        self.display_format_discriminator
    }

    #[must_use]
    pub const fn info_box(&self) -> &OrdinaryBitmapLogicalRgb16Source {
        &self.info_box
    }

    #[must_use]
    pub fn logo(&self, logo: ManufacturerLogo) -> &OrdinaryBitmapLogicalRgb16Source {
        &self.logos[logo.index()]
    }

    #[must_use]
    pub fn frame(&self, frame: ChequeredFrame) -> &OrdinaryBitmapLogicalRgb16Source {
        &self.frames[frame.index()]
    }

    #[must_use]
    pub fn get(
        &self,
        identity: ChampionshipOrdinaryRgb16Identity,
    ) -> &OrdinaryBitmapLogicalRgb16Source {
        match identity {
            ChampionshipOrdinaryRgb16Identity::InfoBox => self.info_box(),
            ChampionshipOrdinaryRgb16Identity::ManufacturerLogo(logo) => self.logo(logo),
            ChampionshipOrdinaryRgb16Identity::ChequeredFrame(frame) => self.frame(frame),
        }
    }

    /// Iterates in deterministic bounded-pool order: R_INFO, canonical
    /// unique-logo order, then numbered Chequered-frame order.
    pub fn iter(
        &self,
    ) -> impl ExactSizeIterator<
        Item = (
            ChampionshipOrdinaryRgb16Identity,
            &OrdinaryBitmapLogicalRgb16Source,
        ),
    > {
        std::array::from_fn::<_, CHAMPIONSHIP_ORDINARY_RGB16_SOURCE_COUNT, _>(|index| {
            if index == 0 {
                (ChampionshipOrdinaryRgb16Identity::InfoBox, self.info_box())
            } else if index <= MANUFACTURER_LOGO_COUNT {
                let logo = ManufacturerLogo::ALL[index - 1];
                (
                    ChampionshipOrdinaryRgb16Identity::ManufacturerLogo(logo),
                    self.logo(logo),
                )
            } else {
                let frame = ChequeredFrame::ALL[index - 1 - MANUFACTURER_LOGO_COUNT];
                (
                    ChampionshipOrdinaryRgb16Identity::ChequeredFrame(frame),
                    self.frame(frame),
                )
            }
        })
        .into_iter()
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        CHAMPIONSHIP_ORDINARY_RGB16_SOURCE_COUNT
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        false
    }
}

/// Converts every currently exact-size championship ordinary bitmap through
/// its embedded palette using one explicit display-format discriminator.
///
/// The conversion order is R_INFO, the 11 unique manufacturer logos in
/// [`ManufacturerLogo::ALL`] order, then the 15 numbered frames in
/// [`ChequeredFrame::ALL`] order. A failure reports the stable identity and
/// original-media-relative descriptor path of the rejected bitmap.
pub fn prepare_software_championship_ordinary_rgb16_pool(
    championship: &ChampionshipAssets,
    frames: &ChampionshipFrameAssets,
    display_format_discriminator: u8,
) -> Result<SoftwareChampionshipOrdinaryRgb16Pool, SoftwareChampionshipOrdinaryRgb16PoolError> {
    prepare_pool_from_sources(
        (INFO_BOX_DESCRIPTOR.path, &frames.info_box),
        ManufacturerLogo::ALL.into_iter().map(|logo| {
            let asset = championship.logo(logo);
            (logo, asset.descriptor.path, &asset.bitmap)
        }),
        ChequeredFrame::ALL.into_iter().map(|frame| {
            let asset = frames.frame(frame);
            (frame, asset.descriptor.path, &asset.bitmap)
        }),
        display_format_discriminator,
    )
}

fn prepare_pool_from_sources<'a>(
    info_box: (&'static str, &'a IndexedBitmap),
    logos: impl IntoIterator<Item = (ManufacturerLogo, &'static str, &'a IndexedBitmap)>,
    frames: impl IntoIterator<Item = (ChequeredFrame, &'static str, &'a IndexedBitmap)>,
    display_format_discriminator: u8,
) -> Result<SoftwareChampionshipOrdinaryRgb16Pool, SoftwareChampionshipOrdinaryRgb16PoolError> {
    let convert = |identity, path, bitmap: &IndexedBitmap| {
        convert_ordinary_bitmap_0x81_to_logical_rgb16(bitmap, display_format_discriminator).map_err(
            |source| SoftwareChampionshipOrdinaryRgb16PoolError {
                identity,
                path,
                source,
            },
        )
    };

    let info_box = convert(
        ChampionshipOrdinaryRgb16Identity::InfoBox,
        info_box.0,
        info_box.1,
    )?;
    let logos = logos
        .into_iter()
        .map(|(logo, path, bitmap)| {
            convert(
                ChampionshipOrdinaryRgb16Identity::ManufacturerLogo(logo),
                path,
                bitmap,
            )
        })
        .collect::<Result<Vec<_>, _>>()?
        .try_into()
        .expect("one RGB16 source was converted for every manufacturer logo");
    let frames = frames
        .into_iter()
        .map(|(frame, path, bitmap)| {
            convert(
                ChampionshipOrdinaryRgb16Identity::ChequeredFrame(frame),
                path,
                bitmap,
            )
        })
        .collect::<Result<Vec<_>, _>>()?
        .try_into()
        .expect("one RGB16 source was converted for every Chequered frame");

    Ok(SoftwareChampionshipOrdinaryRgb16Pool {
        display_format_discriminator,
        info_box,
        logos,
        frames,
    })
}

#[cfg(test)]
mod tests {
    use cmr_core::{
        championship_assets::MANUFACTURER_LOGOS,
        championship_frame_assets::CHEQUERED_FRAME_DESCRIPTORS,
    };
    use cmr_formats::bitmap::{BitmapMetadata, BitmapPaletteEntry};

    use super::*;
    use crate::ordinary_bitmap_rgb16::OrdinaryBitmapRgb16Layout;

    fn bitmap(marker: u8) -> IndexedBitmap {
        let mut palette = vec![entry(0, 0, 0); 256];
        palette[usize::from(marker)] =
            entry(marker, marker.wrapping_add(1), marker.wrapping_add(2));
        IndexedBitmap {
            width: 4,
            height: 1,
            source_was_top_down: false,
            palette,
            pixels: vec![marker; 4],
            row_stride: 4,
            stored_pixels: vec![marker; 4],
            metadata: BitmapMetadata {
                reserved_1: 0,
                reserved_2: 0,
                x_pixels_per_meter: 0,
                y_pixels_per_meter: 0,
                important_colors: 0,
            },
        }
    }

    const fn entry(red: u8, green: u8, blue: u8) -> BitmapPaletteEntry {
        BitmapPaletteEntry {
            red,
            green,
            blue,
            reserved: 0,
        }
    }

    fn sources() -> (
        IndexedBitmap,
        [IndexedBitmap; MANUFACTURER_LOGO_COUNT],
        [IndexedBitmap; CHEQUERED_FRAME_COUNT],
    ) {
        (
            bitmap(1),
            std::array::from_fn(|index| bitmap(index as u8 + 2)),
            std::array::from_fn(|index| bitmap(index as u8 + 32)),
        )
    }

    fn prepare<'a>(
        info: &'a IndexedBitmap,
        logos: &'a [IndexedBitmap; MANUFACTURER_LOGO_COUNT],
        frames: &'a [IndexedBitmap; CHEQUERED_FRAME_COUNT],
        discriminator: u8,
    ) -> Result<SoftwareChampionshipOrdinaryRgb16Pool, SoftwareChampionshipOrdinaryRgb16PoolError>
    {
        prepare_pool_from_sources(
            (INFO_BOX_DESCRIPTOR.path, info),
            ManufacturerLogo::ALL.into_iter().map(|logo| {
                (
                    logo,
                    MANUFACTURER_LOGOS[logo.index()].path,
                    &logos[logo.index()],
                )
            }),
            ChequeredFrame::ALL.into_iter().map(|frame| {
                (
                    frame,
                    CHEQUERED_FRAME_DESCRIPTORS[frame.index()].path,
                    &frames[frame.index()],
                )
            }),
            discriminator,
        )
    }

    #[test]
    fn fixed_pool_maps_every_identity_in_deterministic_order() {
        let (info, logos, frames) = sources();
        let pool = prepare(&info, &logos, &frames, 5).unwrap();

        assert_eq!(pool.len(), CHAMPIONSHIP_ORDINARY_RGB16_SOURCE_COUNT);
        assert!(!pool.is_empty());
        assert_eq!(pool.display_format_discriminator(), 5);
        let identities = pool
            .iter()
            .map(|(identity, _)| identity)
            .collect::<Vec<_>>();
        let expected = std::iter::once(ChampionshipOrdinaryRgb16Identity::InfoBox)
            .chain(ManufacturerLogo::ALL.map(ChampionshipOrdinaryRgb16Identity::ManufacturerLogo))
            .chain(ChequeredFrame::ALL.map(ChampionshipOrdinaryRgb16Identity::ChequeredFrame))
            .collect::<Vec<_>>();
        assert_eq!(identities, expected);
        for (identity, source) in pool.iter() {
            assert!(std::ptr::eq(source, pool.get(identity)));
            assert_eq!(source.layout(), OrdinaryBitmapRgb16Layout::Rgb555);
        }
        assert_eq!(pool.info_box().stored_indices()[0], 1);
        for logo in ManufacturerLogo::ALL {
            assert_eq!(pool.logo(logo).stored_indices()[0], logo.index() as u8 + 2);
        }
        for frame in ChequeredFrame::ALL {
            assert_eq!(
                pool.frame(frame).stored_indices()[0],
                frame.index() as u8 + 32
            );
        }
    }

    #[test]
    fn every_source_uses_its_embedded_palette_and_explicit_565_discriminator() {
        let (mut info, logos, frames) = sources();
        info.palette[1] = entry(255, 129, 31);
        let pool = prepare(&info, &logos, &frames, 4).unwrap();

        assert_eq!(pool.info_box().layout(), OrdinaryBitmapRgb16Layout::Rgb565);
        assert_eq!(pool.info_box().logical_words(), [0xfc03; 4]);
        assert_eq!(pool.info_box().embedded_palette_rgb()[1], [255, 129, 31]);
    }

    #[test]
    fn middle_failure_reports_stable_identity_and_relative_path_without_a_pool() {
        let (info, mut logos, frames) = sources();
        let failed = ManufacturerLogo::Mitsu;
        logos[failed.index()].source_was_top_down = true;

        let error = prepare(&info, &logos, &frames, 5).unwrap_err();

        assert_eq!(
            error,
            SoftwareChampionshipOrdinaryRgb16PoolError {
                identity: ChampionshipOrdinaryRgb16Identity::ManufacturerLogo(failed),
                path: failed.descriptor().path,
                source: OrdinaryBitmapRgb16Error::UnsupportedTopDownSource,
            }
        );
        assert!(!error.path.starts_with('/'));
    }

    #[test]
    fn last_failure_reports_numbered_frame_context() {
        let (info, logos, mut frames) = sources();
        let failed = ChequeredFrame::Frame15;
        frames[failed.index()].palette.clear();

        let error = prepare(&info, &logos, &frames, 5).unwrap_err();

        assert_eq!(
            error.identity,
            ChampionshipOrdinaryRgb16Identity::ChequeredFrame(failed)
        );
        assert_eq!(error.path, failed.descriptor().path);
        assert_eq!(
            error.source,
            OrdinaryBitmapRgb16Error::InvalidEmbeddedPaletteEntryCount(0)
        );
    }

    #[test]
    fn simultaneous_failures_choose_the_first_deterministic_pool_identity() {
        let (mut info, mut logos, mut frames) = sources();
        info.source_was_top_down = true;
        logos[ManufacturerLogo::Subaru.index()].palette.clear();
        frames[ChequeredFrame::Frame01.index()].palette.clear();

        let error = prepare(&info, &logos, &frames, 5).unwrap_err();

        assert_eq!(
            error,
            SoftwareChampionshipOrdinaryRgb16PoolError {
                identity: ChampionshipOrdinaryRgb16Identity::InfoBox,
                path: INFO_BOX_DESCRIPTOR.path,
                source: OrdinaryBitmapRgb16Error::UnsupportedTopDownSource,
            }
        );
    }
}
