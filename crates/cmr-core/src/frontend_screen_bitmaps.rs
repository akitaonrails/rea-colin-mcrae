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

/// Load, parse and upload one 8-bit image with the identity mapping, or
/// `None` when it is missing or malformed.
fn upload<B: IndexedVideoBackend>(
    store: &AssetStore,
    palette: &LegacyPalette256,
    sources: &mut UploadedSourceStore<'_, B>,
    path: &str,
) -> Option<UploadedIndexedBitmap> {
    let bytes = store.load(path).ok()?;
    let parsed = parse_indexed_bitmap(&bytes, BitmapLimits::default()).ok()?;
    let surface = indexed_surface(parsed, palette.clone(), PaletteMapping::Identity).ok()?;
    sources.upload_bitmap(&surface).ok()
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
            .map(|bitmap| upload(store, palette, sources, bitmap.path()))
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

/// The front end's palette arena (`FUN_004c46f0`): disk.pal at 0, the ten
/// flag groups from 1 (`DAT_00d4ce54`), the six root icons from 11
/// (`DAT_00d4ce50`; the unlocked set with `record[0x2b] & 1`), one at 17
/// (`DAT_00d4ce5c`), the fourteen car logos from 18 (`DAT_00d4ce58`) and one
/// at 32 (`DAT_00d8ce68`). `FUN_004c4820(id, kind)` adds the kind's base.
pub mod palette {
    pub const FLAGS: i32 = 1;
    pub const ICONS: i32 = 11;
    pub const SINGLE_17: i32 = 17;
    pub const CARS: i32 = 18;
    pub const SINGLE_32: i32 = 32;
    pub const COUNT: usize = 33;
}

/// The arena's paths after disk.pal (indices 1..33), from the executable's
/// tables in the static tables: the flag palettes, the root icons' (the
/// unlocked set with `record[0x2b] & 1`), `0x5088b4`, the car logos' and
/// `0x50895c`. Each is relative to the front end's root (`new_fe\`).
pub fn arena_paths(
    flags: &[Vec<u8>],
    icons: &[Vec<u8>],
    single_17: &[u8],
    cars: &[Vec<u8>],
    single_32: &[u8],
) -> Vec<String> {
    let path = |bytes: &[u8]| format!("new_fe\\{}", String::from_utf8_lossy(bytes));
    let mut paths: Vec<String> = flags.iter().map(|p| path(p)).collect();
    paths.extend(icons.iter().map(|p| path(p)));
    paths.push(path(single_17));
    paths.extend(cars.iter().map(|p| path(p)));
    paths.push(path(single_32));
    paths
}

/// A flag or logo image of the previews.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenImage {
    /// `DAT_00d494bc[15 group + frame]`: frame `0..15` of flag group `0..10`.
    Flag { group: u8, frame: u8 },
    /// `DAT_00d4123c[car]`: the car's manufacturer logo.
    Logo { car: u8 },
}

/// The uploaded flag frames and car logos.
#[derive(Clone, Debug, Default)]
pub struct ScreenImagePool {
    flags: Vec<Option<UploadedIndexedBitmap>>,
    logos: Vec<Option<UploadedIndexedBitmap>>,
}

impl ScreenImagePool {
    /// Load the 150 flag frames (`FUN_004c4ad0`: the prefix `0x508cec`, the
    /// group's name and `"%.3d.bmp"` of 1..=15, under the front end's root)
    /// and the 14 car logos.
    pub fn load<B: IndexedVideoBackend>(
        store: &AssetStore,
        palette: &LegacyPalette256,
        sources: &mut UploadedSourceStore<'_, B>,
        flag_prefix: &[u8],
        flag_groups: &[Vec<u8>],
        flag_format: &[u8],
    ) -> Self {
        let mut flags = Vec::with_capacity(150);
        for group in flag_groups {
            for frame in 1..=15i64 {
                let name = crate::c_format::c_format(
                    flag_format,
                    &[crate::c_format::FormatArg::Int(frame)],
                );
                let mut path = b"new_fe\\".to_vec();
                path.extend_from_slice(flag_prefix);
                path.extend_from_slice(group);
                path.extend(name);
                flags.push(upload(
                    store,
                    palette,
                    sources,
                    &String::from_utf8_lossy(&path),
                ));
            }
        }
        let logos = crate::championship_assets::CHAMPIONSHIP_CARS
            .iter()
            .map(|car| upload(store, palette, sources, car.logo.descriptor().path))
            .collect();
        Self { flags, logos }
    }

    /// The uploaded image, when it loaded.
    pub fn get(&self, image: ScreenImage) -> Option<UploadedIndexedBitmap> {
        match image {
            ScreenImage::Flag { group, frame } => self
                .flags
                .get(usize::from(group) * 15 + usize::from(frame))
                .copied()
                .flatten(),
            ScreenImage::Logo { car } => self.logos.get(usize::from(car)).copied().flatten(),
        }
    }
}

#[cfg(test)]
mod palette_tests {
    use super::*;

    #[test]
    fn the_arena_paths_keep_the_native_order() {
        let flags: Vec<Vec<u8>> = (0..10).map(|i| format!("F{i}").into_bytes()).collect();
        let icons: Vec<Vec<u8>> = (0..6).map(|i| format!("I{i}").into_bytes()).collect();
        let cars: Vec<Vec<u8>> = (0..14).map(|i| format!("C{i}").into_bytes()).collect();
        let paths = arena_paths(&flags, &icons, b"S17", &cars, b"S32");
        assert_eq!(paths.len(), palette::COUNT - 1);
        assert_eq!(paths[(palette::FLAGS - 1) as usize], "new_fe\\F0");
        assert_eq!(paths[(palette::ICONS - 1) as usize], "new_fe\\I0");
        assert_eq!(paths[(palette::SINGLE_17 - 1) as usize], "new_fe\\S17");
        assert_eq!(paths[(palette::CARS - 1) as usize], "new_fe\\C0");
        assert_eq!(paths[(palette::SINGLE_32 - 1) as usize], "new_fe\\S32");
        assert!(
            ScreenImagePool::default()
                .get(ScreenImage::Logo { car: 0 })
                .is_none()
        );
    }
}
