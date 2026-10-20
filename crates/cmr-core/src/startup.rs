use std::fmt;

use crate::localization::Language;

/// Languages present in the observed installation's startup assets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartupLanguage {
    English,
    French,
    German,
    Spanish,
}

impl StartupLanguage {
    fn asset_directory(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::French => "French",
            Self::German => "German",
            Self::Spanish => "Spanish",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnsupportedStartupLanguage(pub Language);

impl fmt::Display for UnsupportedStartupLanguage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "reviewed asset profile has no {:?} startup artwork",
            self.0
        )
    }
}

impl std::error::Error for UnsupportedStartupLanguage {}

impl TryFrom<Language> for StartupLanguage {
    type Error = UnsupportedStartupLanguage;

    fn try_from(language: Language) -> Result<Self, Self::Error> {
        match language {
            Language::English => Ok(Self::English),
            Language::French => Ok(Self::French),
            Language::German => Ok(Self::German),
            Language::Spanish => Ok(Self::Spanish),
            Language::Italian => Err(UnsupportedStartupLanguage(language)),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartupScreen {
    Loading,
    Legal,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartupAssets {
    pub palette: &'static str,
    pub bitmap: String,
}

/// Selects the logical startup artwork paths recovered from the canonical EU
/// executable. Filesystem casing is resolved by the asset layer.
pub fn select_startup_assets(screen: StartupScreen, language: StartupLanguage) -> StartupAssets {
    match screen {
        StartupScreen::Loading => StartupAssets {
            palette: "ini/loading.pal",
            bitmap: "ini/loading.bmp".to_owned(),
        },
        StartupScreen::Legal => StartupAssets {
            palette: "ini/legal.pal",
            bitmap: format!("ini/{}/legal.bmp", language.asset_directory()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loading_art_is_not_localized() {
        for language in [
            StartupLanguage::English,
            StartupLanguage::French,
            StartupLanguage::German,
            StartupLanguage::Spanish,
        ] {
            assert_eq!(
                select_startup_assets(StartupScreen::Loading, language),
                StartupAssets {
                    palette: "ini/loading.pal",
                    bitmap: "ini/loading.bmp".to_owned(),
                }
            );
        }
    }

    #[test]
    fn legal_art_selects_observed_language_directory() {
        let cases = [
            (StartupLanguage::English, "ini/English/legal.bmp"),
            (StartupLanguage::French, "ini/French/legal.bmp"),
            (StartupLanguage::German, "ini/German/legal.bmp"),
            (StartupLanguage::Spanish, "ini/Spanish/legal.bmp"),
        ];
        for (language, bitmap) in cases {
            assert_eq!(
                select_startup_assets(StartupScreen::Legal, language),
                StartupAssets {
                    palette: "ini/legal.pal",
                    bitmap: bitmap.to_owned(),
                }
            );
        }
    }

    #[test]
    fn shared_language_conversion_rejects_missing_italian_startup_assets() {
        for (language, expected) in [
            (Language::English, StartupLanguage::English),
            (Language::French, StartupLanguage::French),
            (Language::German, StartupLanguage::German),
            (Language::Spanish, StartupLanguage::Spanish),
        ] {
            assert_eq!(StartupLanguage::try_from(language), Ok(expected));
        }
        assert_eq!(
            StartupLanguage::try_from(Language::Italian),
            Err(UnsupportedStartupLanguage(Language::Italian))
        );
        assert_eq!(
            UnsupportedStartupLanguage(Language::Italian).to_string(),
            "reviewed asset profile has no Italian startup artwork"
        );
    }
}
