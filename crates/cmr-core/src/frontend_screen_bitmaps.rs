//! The front end's sub-screen bitmaps the builder `FUN_004be540` and the
//! screens' loaders keep as surfaces: the info box, the metal name panels,
//! the LCD name field, the stage-select LEDs and button, and the players'
//! pictures (paths `.work/frontend-setup-draw-notes.md` section 8). Each is an
//! 8-bit image over the front end's shared palette, uploaded as is (identity
//! mapping, as the root's bitmaps are). A missing image leaves its draws out.

use cmr_assets::AssetStore;
use cmr_formats::bitmap::{BitmapLimits, parse_indexed_bitmap};
use cmr_formats::riff_palette::LegacyPalette256;

use crate::display::IndexedVideoBackend;
use crate::source_store::{UploadedIndexedBitmap, UploadedSourceStore};
use crate::surface::{PaletteMapping, indexed_surface};

/// A sub-screen bitmap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenBitmap {
    /// `DAT_00d4cdb8`: `images\INFOBOX\R_INFO.BMP`.
    InfoRight,
    /// `DAT_00d486cc`: `images\METAL\NAME.BMP`.
    Name,
    /// `DAT_00d4ad94`: `images\METAL\NAMSMALL.BMP`.
    NameSmall,
    /// `DAT_00d43794`: `images\METAL\OPTION.BMP`.
    Option,
    /// `DAT_00d4cd90/98/94`: the LCD field's caps and middle.
    LcdLeft,
    LcdMiddle,
    LcdRight,
    /// `DAT_00d4cd84/88`: the stage-select LEDs.
    LightOn,
    LightOff,
    /// `DAT_00d4cdac/b0`: the stage-select button.
    DoneIn,
    DoneOut,
    /// `DAT_00d486d0`: `images\THUMBS\FRAME.BMP`.
    ThumbFrame,
    /// `DAT_00d41bb4`, `DAT_00d41714`, `DAT_00d40408`, `DAT_00d4a4f4`.
    OnePlayer,
    TwoPlayer,
    Host,
    Join,
}

impl ScreenBitmap {
    pub const ALL: [Self; 16] = [
        Self::InfoRight,
        Self::Name,
        Self::NameSmall,
        Self::Option,
        Self::LcdLeft,
        Self::LcdMiddle,
        Self::LcdRight,
        Self::LightOn,
        Self::LightOff,
        Self::DoneIn,
        Self::DoneOut,
        Self::ThumbFrame,
        Self::OnePlayer,
        Self::TwoPlayer,
        Self::Host,
        Self::Join,
    ];

    /// The imported path.
    pub const fn path(self) -> &'static str {
        match self {
            Self::InfoRight => "New_Fe/Images/Infobox/r_info.bmp",
            Self::Name => "New_Fe/Images/Metal/name.bmp",
            Self::NameSmall => "New_Fe/Images/Metal/namsmall.bmp",
            Self::Option => "New_Fe/Images/Metal/option.bmp",
            Self::LcdLeft => "New_Fe/Images/LCDPanel/LCD_lef.bmp",
            Self::LcdMiddle => "New_Fe/Images/LCDPanel/LCD_mid.bmp",
            Self::LcdRight => "New_Fe/Images/LCDPanel/LCD_rig.bmp",
            Self::LightOn => "New_Fe/Images/LEDs/Lighton.bmp",
            Self::LightOff => "New_Fe/Images/LEDs/Lightoff.bmp",
            Self::DoneIn => "New_Fe/Images/Button/Done_in.bmp",
            Self::DoneOut => "New_Fe/Images/Button/Done_out.bmp",
            Self::ThumbFrame => "New_Fe/Images/Thumbs/frame.bmp",
            Self::OnePlayer => "New_Fe/Images/Players/1play.BMP",
            Self::TwoPlayer => "New_Fe/Images/Players/2play.BMP",
            Self::Host => "New_Fe/Images/Players/host.bmp",
            Self::Join => "New_Fe/Images/Players/join.bmp",
        }
    }

    const fn index(self) -> usize {
        self as usize
    }
}

/// The uploaded sub-screen bitmaps.
#[derive(Clone, Debug, Default)]
pub struct ScreenBitmapPool {
    slots: Vec<Option<UploadedIndexedBitmap>>,
}

impl ScreenBitmapPool {
    /// Load and upload every bitmap that exists and parses.
    pub fn load<B: IndexedVideoBackend>(
        store: &AssetStore,
        palette: &LegacyPalette256,
        sources: &mut UploadedSourceStore<'_, B>,
    ) -> Self {
        let slots = ScreenBitmap::ALL
            .iter()
            .map(|bitmap| {
                let bytes = store.load(bitmap.path()).ok()?;
                let parsed = parse_indexed_bitmap(&bytes, BitmapLimits::default()).ok()?;
                let surface =
                    indexed_surface(parsed, palette.clone(), PaletteMapping::Identity).ok()?;
                sources.upload_bitmap(&surface).ok()
            })
            .collect();
        Self { slots }
    }

    /// The uploaded bitmap, when it loaded.
    pub fn get(&self, bitmap: ScreenBitmap) -> Option<UploadedIndexedBitmap> {
        self.slots.get(bitmap.index()).copied().flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bitmap_has_a_distinct_path_and_slot() {
        let mut paths: Vec<&str> = ScreenBitmap::ALL.iter().map(|b| b.path()).collect();
        paths.sort_unstable();
        paths.dedup();
        assert_eq!(paths.len(), ScreenBitmap::ALL.len());
        for (index, bitmap) in ScreenBitmap::ALL.iter().enumerate() {
            assert_eq!(bitmap.index(), index);
        }
        assert!(
            ScreenBitmapPool::default()
                .get(ScreenBitmap::Name)
                .is_none()
        );
    }
}
