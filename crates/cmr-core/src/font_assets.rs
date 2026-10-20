use std::fmt;

use cmr_assets::{AssetStore, AssetStoreError};
use cmr_formats::font::{BitmapFont, FontError, FontLimits, parse_legacy_font};

pub const YELLOW_FONT_PATH: &str = "New_Fe/Fonts/yellow.fon";
pub const GREY_FONT_PATH: &str = "New_Fe/Fonts/grey.fon";
pub const WHITE_FONT_PATH: &str = "New_Fe/Fonts/white.fon";
pub const BLUE_TITLE_FONT_PATH: &str = "New_Fe/Fonts/bluetit.fon";
pub const WHITE_SEARCH_FONT_PATH: &str = "New_Fe/Fonts/whitsear.fon";
pub const WHITE_TITLE_FONT_PATH: &str = "New_Fe/Fonts/whittit.fon";
pub const RED_LCD_FONT_PATH: &str = "New_Fe/Fonts/red_lcd.fon";
pub const BLUE_INFO_FONT_PATH: &str = "New_Fe/Fonts/bl_info.fon";
pub const GREEN_INFO_FONT_PATH: &str = "New_Fe/Fonts/gr_info.fon";
pub const GREY_DYNAMIC_FONT_PATH: &str = "New_Fe/Fonts/grey_dy.fon";
pub const BLUE_DYNAMIC_FONT_PATH: &str = "New_Fe/Fonts/blue_dy.fon";
pub const RED_DYNAMIC_FONT_PATH: &str = "New_Fe/Fonts/red_dy.fon";
pub const OPTION_GREY_FONT_PATH: &str = "New_Fe/Fonts/opt_grey.fon";
pub const OPTIONS_FONT_PATH: &str = "New_Fe/Fonts/options.fon";
pub const OPTION_RED_FONT_PATH: &str = "New_Fe/Fonts/op_red.fon";
pub const TITLE_FONT_PATH: &str = "New_Fe/Fonts/title.fon";

/// Relative names passed to the recovered mode-2 loader. `AssetStore` instead
/// uses [`FontStyle::path`] because imported assets retain their `New_Fe` root.
pub const LEGACY_FONT_PATHS: [&str; FONT_COUNT] = [
    "FONTS/TITLE.FON",
    "FONTS/OP_RED.FON",
    "FONTS/OPTIONS.FON",
    "FONTS/OPT_GREY.FON",
    "FONTS/RED_DY.FON",
    "FONTS/BLUE_DY.FON",
    "FONTS/GREY_DY.FON",
    "FONTS/GR_INFO.FON",
    "FONTS/BL_INFO.FON",
    "FONTS/RED_LCD.FON",
    "FONTS/WHITTIT.FON",
    "FONTS/WHITSEAR.FON",
    "FONTS/BLUETIT.FON",
    "FONTS/WHITE.FON",
    "FONTS/GREY.FON",
    "FONTS/YELLOW.FON",
];

pub const FONT_COUNT: usize = 16;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u8)]
pub enum FontStyle {
    Title,
    OptionRed,
    Options,
    OptionGrey,
    RedDynamic,
    BlueDynamic,
    GreyDynamic,
    GreenInfo,
    BlueInfo,
    RedLcd,
    WhiteTitle,
    WhiteSearch,
    BlueTitle,
    White,
    Grey,
    Yellow,
}

impl FontStyle {
    pub const ALL: [Self; FONT_COUNT] = [
        Self::Title,
        Self::OptionRed,
        Self::Options,
        Self::OptionGrey,
        Self::RedDynamic,
        Self::BlueDynamic,
        Self::GreyDynamic,
        Self::GreenInfo,
        Self::BlueInfo,
        Self::RedLcd,
        Self::WhiteTitle,
        Self::WhiteSearch,
        Self::BlueTitle,
        Self::White,
        Self::Grey,
        Self::Yellow,
    ];

    pub const fn path(self) -> &'static str {
        match self {
            Self::Yellow => YELLOW_FONT_PATH,
            Self::Grey => GREY_FONT_PATH,
            Self::White => WHITE_FONT_PATH,
            Self::BlueTitle => BLUE_TITLE_FONT_PATH,
            Self::WhiteSearch => WHITE_SEARCH_FONT_PATH,
            Self::WhiteTitle => WHITE_TITLE_FONT_PATH,
            Self::RedLcd => RED_LCD_FONT_PATH,
            Self::BlueInfo => BLUE_INFO_FONT_PATH,
            Self::GreenInfo => GREEN_INFO_FONT_PATH,
            Self::GreyDynamic => GREY_DYNAMIC_FONT_PATH,
            Self::BlueDynamic => BLUE_DYNAMIC_FONT_PATH,
            Self::RedDynamic => RED_DYNAMIC_FONT_PATH,
            Self::OptionGrey => OPTION_GREY_FONT_PATH,
            Self::Options => OPTIONS_FONT_PATH,
            Self::OptionRed => OPTION_RED_FONT_PATH,
            Self::Title => TITLE_FONT_PATH,
        }
    }

    pub const fn legacy_path(self) -> &'static str {
        LEGACY_FONT_PATHS[self as usize]
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FontAsset {
    pub style: FontStyle,
    pub font: BitmapFont,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FontSet {
    fonts: [FontAsset; FONT_COUNT],
}

impl FontSet {
    pub fn get(&self, style: FontStyle) -> &BitmapFont {
        &self.fonts[style as usize].font
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = &FontAsset> {
        self.fonts.iter()
    }

    pub const fn len(&self) -> usize {
        FONT_COUNT
    }

    pub const fn is_empty(&self) -> bool {
        false
    }
}

#[derive(Debug)]
pub enum FontLoadError {
    Asset {
        style: FontStyle,
        path: &'static str,
        source: AssetStoreError,
    },
    Parse {
        style: FontStyle,
        path: &'static str,
        source: FontError,
    },
}

impl fmt::Display for FontLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Asset {
                style,
                path,
                source,
            } => write!(formatter, "cannot load {style:?} font {path}: {source}"),
            Self::Parse {
                style,
                path,
                source,
            } => write!(formatter, "cannot parse {style:?} font {path}: {source}"),
        }
    }
}

impl std::error::Error for FontLoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Asset { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
        }
    }
}

/// Loads all recovered frontend fonts in the original initialization order.
pub fn load_font_set(store: &AssetStore, limits: FontLimits) -> Result<FontSet, FontLoadError> {
    let mut fonts = Vec::with_capacity(FONT_COUNT);
    for style in FontStyle::ALL {
        let path = style.path();
        let bytes = store.load(path).map_err(|source| FontLoadError::Asset {
            style,
            path,
            source,
        })?;
        let font = parse_legacy_font(&bytes, limits).map_err(|source| FontLoadError::Parse {
            style,
            path,
            source,
        })?;
        fonts.push(FontAsset { style, font });
    }
    let fonts = fonts
        .try_into()
        .expect("one font was loaded for every recovered style");
    Ok(FontSet { fonts })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_follows_the_recovered_startup_load_order() {
        assert_eq!(
            FontStyle::ALL,
            [
                FontStyle::Title,
                FontStyle::OptionRed,
                FontStyle::Options,
                FontStyle::OptionGrey,
                FontStyle::RedDynamic,
                FontStyle::BlueDynamic,
                FontStyle::GreyDynamic,
                FontStyle::GreenInfo,
                FontStyle::BlueInfo,
                FontStyle::RedLcd,
                FontStyle::WhiteTitle,
                FontStyle::WhiteSearch,
                FontStyle::BlueTitle,
                FontStyle::White,
                FontStyle::Grey,
                FontStyle::Yellow,
            ]
        );
    }

    #[test]
    fn registry_preserves_recovered_order_and_both_path_domains() {
        assert_eq!(
            FontStyle::ALL.map(FontStyle::legacy_path),
            [
                "FONTS/TITLE.FON",
                "FONTS/OP_RED.FON",
                "FONTS/OPTIONS.FON",
                "FONTS/OPT_GREY.FON",
                "FONTS/RED_DY.FON",
                "FONTS/BLUE_DY.FON",
                "FONTS/GREY_DY.FON",
                "FONTS/GR_INFO.FON",
                "FONTS/BL_INFO.FON",
                "FONTS/RED_LCD.FON",
                "FONTS/WHITTIT.FON",
                "FONTS/WHITSEAR.FON",
                "FONTS/BLUETIT.FON",
                "FONTS/WHITE.FON",
                "FONTS/GREY.FON",
                "FONTS/YELLOW.FON",
            ]
        );
        assert_eq!(
            FontStyle::ALL.map(FontStyle::path),
            [
                "New_Fe/Fonts/title.fon",
                "New_Fe/Fonts/op_red.fon",
                "New_Fe/Fonts/options.fon",
                "New_Fe/Fonts/opt_grey.fon",
                "New_Fe/Fonts/red_dy.fon",
                "New_Fe/Fonts/blue_dy.fon",
                "New_Fe/Fonts/grey_dy.fon",
                "New_Fe/Fonts/gr_info.fon",
                "New_Fe/Fonts/bl_info.fon",
                "New_Fe/Fonts/red_lcd.fon",
                "New_Fe/Fonts/whittit.fon",
                "New_Fe/Fonts/whitsear.fon",
                "New_Fe/Fonts/bluetit.fon",
                "New_Fe/Fonts/white.fon",
                "New_Fe/Fonts/grey.fon",
                "New_Fe/Fonts/yellow.fon",
            ]
        );
    }
}
