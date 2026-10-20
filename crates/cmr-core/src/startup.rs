/// Languages present in the observed installation's startup assets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Language {
    English,
    French,
    German,
    Spanish,
}

impl Language {
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
pub fn select_startup_assets(screen: StartupScreen, language: Language) -> StartupAssets {
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
            Language::English,
            Language::French,
            Language::German,
            Language::Spanish,
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
            (Language::English, "ini/English/legal.bmp"),
            (Language::French, "ini/French/legal.bmp"),
            (Language::German, "ini/German/legal.bmp"),
            (Language::Spanish, "ini/Spanish/legal.bmp"),
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
}
