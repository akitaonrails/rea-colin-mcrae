//! Reviewed logical paths for the recovered root-menu visuals.
//!
//! Paths are rooted for [`cmr_assets::AssetStore`]. They deliberately keep the
//! normal and `g_` filename variants distinct rather than assigning an
//! unverified rendering meaning to the prefix.

use crate::localization::Language;

pub const ROOT_MENU_BACKDROP_PATH: &str = "New_Fe/Images/Backdrop/Main.bmp";
pub const ROOT_MENU_SELECT_PATH: &str = "New_Fe/Images/select/select.bmp";
pub const ROOT_MENU_DISK_PALETTE_PATH: &str = "New_Fe/Palettes/disk.pal";
pub const ROOT_MENU_ANIMATION_FRAMES: usize = 7;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalizedRootMenuAssets {
    pub main_selected: &'static str,
    pub main_shadow: &'static str,
    pub animation: [&'static str; ROOT_MENU_ANIMATION_FRAMES],
}

const ENGLISH_ASSETS: LocalizedRootMenuAssets = LocalizedRootMenuAssets {
    main_selected: "New_Fe/Images/MainOpts/English/main_sel.BMP",
    main_shadow: "New_Fe/Images/MainOpts/English/main_shd.BMP",
    animation: [
        "New_Fe/Images/MainOpts/English/Anim/english0.bmp",
        "New_Fe/Images/MainOpts/English/Anim/english1.bmp",
        "New_Fe/Images/MainOpts/English/Anim/english2.bmp",
        "New_Fe/Images/MainOpts/English/Anim/english3.bmp",
        "New_Fe/Images/MainOpts/English/Anim/english4.bmp",
        "New_Fe/Images/MainOpts/English/Anim/english5.bmp",
        "New_Fe/Images/MainOpts/English/Anim/english6.bmp",
    ],
};

const FRENCH_ASSETS: LocalizedRootMenuAssets = LocalizedRootMenuAssets {
    main_selected: "New_Fe/Images/MainOpts/French/main_sel.BMP",
    main_shadow: "New_Fe/Images/MainOpts/French/main_shd.BMP",
    animation: [
        "New_Fe/Images/MainOpts/French/Anim/french0.bmp",
        "New_Fe/Images/MainOpts/French/Anim/french1.bmp",
        "New_Fe/Images/MainOpts/French/Anim/french2.bmp",
        "New_Fe/Images/MainOpts/French/Anim/french3.bmp",
        "New_Fe/Images/MainOpts/French/Anim/french4.bmp",
        "New_Fe/Images/MainOpts/French/Anim/french5.bmp",
        "New_Fe/Images/MainOpts/French/Anim/french6.bmp",
    ],
};

const GERMAN_ASSETS: LocalizedRootMenuAssets = LocalizedRootMenuAssets {
    main_selected: "New_Fe/Images/MainOpts/German/main_sel.BMP",
    main_shadow: "New_Fe/Images/MainOpts/German/main_shd.BMP",
    animation: [
        "New_Fe/Images/MainOpts/German/Anim/german0.bmp",
        "New_Fe/Images/MainOpts/German/Anim/german1.bmp",
        "New_Fe/Images/MainOpts/German/Anim/german2.bmp",
        "New_Fe/Images/MainOpts/German/Anim/german3.bmp",
        "New_Fe/Images/MainOpts/German/Anim/german4.bmp",
        "New_Fe/Images/MainOpts/German/Anim/german5.bmp",
        "New_Fe/Images/MainOpts/German/Anim/german6.bmp",
    ],
};

const SPANISH_ASSETS: LocalizedRootMenuAssets = LocalizedRootMenuAssets {
    main_selected: "New_Fe/Images/MainOpts/Spanish/main_sel.BMP",
    main_shadow: "New_Fe/Images/MainOpts/Spanish/main_shd.BMP",
    animation: [
        "New_Fe/Images/MainOpts/Spanish/Anim/spanish0.bmp",
        "New_Fe/Images/MainOpts/Spanish/Anim/spanish1.bmp",
        "New_Fe/Images/MainOpts/Spanish/Anim/spanish2.bmp",
        "New_Fe/Images/MainOpts/Spanish/Anim/spanish3.bmp",
        "New_Fe/Images/MainOpts/Spanish/Anim/spanish4.bmp",
        "New_Fe/Images/MainOpts/Spanish/Anim/spanish5.bmp",
        "New_Fe/Images/MainOpts/Spanish/Anim/spanish6.bmp",
    ],
};

const ITALIAN_ASSETS: LocalizedRootMenuAssets = LocalizedRootMenuAssets {
    main_selected: "New_Fe/Images/MainOpts/Italian/main_sel.BMP",
    main_shadow: "New_Fe/Images/MainOpts/Italian/main_shd.BMP",
    animation: [
        "New_Fe/Images/MainOpts/Italian/Anim/italian0.bmp",
        "New_Fe/Images/MainOpts/Italian/Anim/italian1.bmp",
        "New_Fe/Images/MainOpts/Italian/Anim/italian2.bmp",
        "New_Fe/Images/MainOpts/Italian/Anim/italian3.bmp",
        "New_Fe/Images/MainOpts/Italian/Anim/italian4.bmp",
        "New_Fe/Images/MainOpts/Italian/Anim/italian5.bmp",
        "New_Fe/Images/MainOpts/Italian/Anim/italian6.bmp",
    ],
};

/// Selects the exact root-menu artwork row requested by the recovered loader.
/// The reviewed local corpus lacks the Italian row; callers must report that
/// missing user-owned data rather than silently substituting another language.
pub const fn localized_root_menu_assets(language: Language) -> LocalizedRootMenuAssets {
    match language {
        Language::English => ENGLISH_ASSETS,
        Language::French => FRENCH_ASSETS,
        Language::German => GERMAN_ASSETS,
        Language::Spanish => SPANISH_ASSETS,
        Language::Italian => ITALIAN_ASSETS,
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum VisualVariant {
    Normal,
    GPrefixed,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CenterIconIdentity {
    Cone,
    Disk,
    ChampionshipGlobe,
    TimeTrialGlobe,
    Quit,
    Spanner,
}

impl CenterIconIdentity {
    /// Atlas/object-slot order consumed by `root_menu_visual`.
    pub const ALL: [Self; 6] = [
        Self::ChampionshipGlobe,
        Self::TimeTrialGlobe,
        Self::Cone,
        Self::Spanner,
        Self::Disk,
        Self::Quit,
    ];

    pub const fn path(self, variant: VisualVariant) -> &'static str {
        match (self, variant) {
            (Self::Cone, VisualVariant::Normal) => "New_Fe/Images/CenIcon/cone.bmp",
            (Self::Cone, VisualVariant::GPrefixed) => "New_Fe/Images/CenIcon/g_cone.bmp",
            (Self::Disk, VisualVariant::Normal) => "New_Fe/Images/CenIcon/disk.bmp",
            (Self::Disk, VisualVariant::GPrefixed) => "New_Fe/Images/CenIcon/g_disk.bmp",
            (Self::ChampionshipGlobe, VisualVariant::Normal) => {
                "New_Fe/Images/CenIcon/Globe_ch.bmp"
            }
            (Self::ChampionshipGlobe, VisualVariant::GPrefixed) => {
                "New_Fe/Images/CenIcon/g_globe.bmp"
            }
            (Self::TimeTrialGlobe, VisualVariant::Normal) => "New_Fe/Images/CenIcon/Globe_ti.bmp",
            (Self::TimeTrialGlobe, VisualVariant::GPrefixed) => "New_Fe/Images/CenIcon/g_globe.bmp",
            (Self::Quit, VisualVariant::Normal) => "New_Fe/Images/CenIcon/quit.bmp",
            (Self::Quit, VisualVariant::GPrefixed) => "New_Fe/Images/CenIcon/g_quit.bmp",
            (Self::Spanner, VisualVariant::Normal) => "New_Fe/Images/CenIcon/spanner.bmp",
            (Self::Spanner, VisualVariant::GPrefixed) => "New_Fe/Images/CenIcon/g_span.bmp",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RootMenuObjectIdentity {
    Rally,
    Championship,
    TimeTrial,
    RallySchool,
    Load,
    Options,
}

impl RootMenuObjectIdentity {
    /// Selection order for entries 0 through 5; entry 6 has no C3D object.
    pub const ALL: [Self; 6] = [
        Self::Championship,
        Self::Rally,
        Self::TimeTrial,
        Self::RallySchool,
        Self::Options,
        Self::Load,
    ];

    pub const fn path(self, variant: VisualVariant) -> &'static str {
        match (self, variant) {
            (Self::Rally, VisualVariant::Normal) => "New_Fe/Objects/MainMenu/Rally.C3D",
            (Self::Rally, VisualVariant::GPrefixed) => "New_Fe/Objects/MainMenu/g_rally.C3D",
            (Self::Championship, VisualVariant::Normal) => "New_Fe/Objects/MainMenu/Champ.C3D",
            (Self::Championship, VisualVariant::GPrefixed) => "New_Fe/Objects/MainMenu/g_champ.C3D",
            (Self::TimeTrial, VisualVariant::Normal) => "New_Fe/Objects/MainMenu/Time.C3D",
            (Self::TimeTrial, VisualVariant::GPrefixed) => "New_Fe/Objects/MainMenu/g_time.C3D",
            (Self::RallySchool, VisualVariant::Normal) => "New_Fe/Objects/MainMenu/Rschool.C3D",
            (Self::RallySchool, VisualVariant::GPrefixed) => {
                "New_Fe/Objects/MainMenu/g_rschool.C3D"
            }
            (Self::Load, VisualVariant::Normal) => "New_Fe/Objects/MainMenu/Load.C3D",
            (Self::Load, VisualVariant::GPrefixed) => "New_Fe/Objects/MainMenu/g_load.C3D",
            (Self::Options, VisualVariant::Normal) => "New_Fe/Objects/MainMenu/Options.c3d",
            (Self::Options, VisualVariant::GPrefixed) => "New_Fe/Objects/MainMenu/g_options.C3D",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn italian_retains_its_observed_distinct_artwork_paths() {
        let assets = localized_root_menu_assets(Language::Italian);
        assert_eq!(
            assets.main_selected,
            "New_Fe/Images/MainOpts/Italian/main_sel.BMP"
        );
        assert_eq!(
            assets.main_shadow,
            "New_Fe/Images/MainOpts/Italian/main_shd.BMP"
        );
        for (index, path) in assets.animation.into_iter().enumerate() {
            assert!(path.ends_with(&format!("/italian{index}.bmp")));
        }
    }

    #[test]
    fn localized_rows_retain_language_specific_frame_names_and_order() {
        let cases = [
            (Language::English, "English", "english"),
            (Language::French, "French", "french"),
            (Language::German, "German", "german"),
            (Language::Spanish, "Spanish", "spanish"),
            (Language::Italian, "Italian", "italian"),
        ];
        for (language, directory, stem) in cases {
            let assets = localized_root_menu_assets(language);
            assert_eq!(
                assets.main_selected,
                format!("New_Fe/Images/MainOpts/{directory}/main_sel.BMP")
            );
            assert_eq!(
                assets.main_shadow,
                format!("New_Fe/Images/MainOpts/{directory}/main_shd.BMP")
            );
            for (index, path) in assets.animation.into_iter().enumerate() {
                assert_eq!(
                    path,
                    format!("New_Fe/Images/MainOpts/{directory}/Anim/{stem}{index}.bmp")
                );
            }
        }
    }

    #[test]
    fn exact_center_and_object_paths_follow_recovered_slot_order() {
        assert_eq!(
            CenterIconIdentity::ALL.map(|identity| identity.path(VisualVariant::Normal)),
            [
                "New_Fe/Images/CenIcon/Globe_ch.bmp",
                "New_Fe/Images/CenIcon/Globe_ti.bmp",
                "New_Fe/Images/CenIcon/cone.bmp",
                "New_Fe/Images/CenIcon/spanner.bmp",
                "New_Fe/Images/CenIcon/disk.bmp",
                "New_Fe/Images/CenIcon/quit.bmp",
            ]
        );
        assert_eq!(
            CenterIconIdentity::ALL.map(|identity| identity.path(VisualVariant::GPrefixed)),
            [
                "New_Fe/Images/CenIcon/g_globe.bmp",
                "New_Fe/Images/CenIcon/g_globe.bmp",
                "New_Fe/Images/CenIcon/g_cone.bmp",
                "New_Fe/Images/CenIcon/g_span.bmp",
                "New_Fe/Images/CenIcon/g_disk.bmp",
                "New_Fe/Images/CenIcon/g_quit.bmp",
            ]
        );
        assert_eq!(
            RootMenuObjectIdentity::ALL.map(|identity| identity.path(VisualVariant::Normal)),
            [
                "New_Fe/Objects/MainMenu/Champ.C3D",
                "New_Fe/Objects/MainMenu/Rally.C3D",
                "New_Fe/Objects/MainMenu/Time.C3D",
                "New_Fe/Objects/MainMenu/Rschool.C3D",
                "New_Fe/Objects/MainMenu/Options.c3d",
                "New_Fe/Objects/MainMenu/Load.C3D",
            ]
        );
        assert_eq!(
            RootMenuObjectIdentity::ALL.map(|identity| identity.path(VisualVariant::GPrefixed)),
            [
                "New_Fe/Objects/MainMenu/g_champ.C3D",
                "New_Fe/Objects/MainMenu/g_rally.C3D",
                "New_Fe/Objects/MainMenu/g_time.C3D",
                "New_Fe/Objects/MainMenu/g_rschool.C3D",
                "New_Fe/Objects/MainMenu/g_options.C3D",
                "New_Fe/Objects/MainMenu/g_load.C3D",
            ]
        );
    }

    #[test]
    fn globe_identities_share_only_the_observed_g_prefixed_asset() {
        assert_ne!(
            CenterIconIdentity::ChampionshipGlobe.path(VisualVariant::Normal),
            CenterIconIdentity::TimeTrialGlobe.path(VisualVariant::Normal)
        );
        assert_eq!(
            CenterIconIdentity::ChampionshipGlobe.path(VisualVariant::GPrefixed),
            CenterIconIdentity::TimeTrialGlobe.path(VisualVariant::GPrefixed)
        );
    }

    #[test]
    fn normal_and_g_prefixed_object_paths_remain_distinct() {
        for identity in RootMenuObjectIdentity::ALL {
            assert_ne!(
                identity.path(VisualVariant::Normal),
                identity.path(VisualVariant::GPrefixed)
            );
        }
    }

    #[test]
    fn identity_arrays_follow_recovered_visual_and_selection_order() {
        assert_eq!(
            CenterIconIdentity::ALL,
            [
                CenterIconIdentity::ChampionshipGlobe,
                CenterIconIdentity::TimeTrialGlobe,
                CenterIconIdentity::Cone,
                CenterIconIdentity::Spanner,
                CenterIconIdentity::Disk,
                CenterIconIdentity::Quit,
            ]
        );
        assert_eq!(
            RootMenuObjectIdentity::ALL,
            [
                RootMenuObjectIdentity::Championship,
                RootMenuObjectIdentity::Rally,
                RootMenuObjectIdentity::TimeTrial,
                RootMenuObjectIdentity::RallySchool,
                RootMenuObjectIdentity::Options,
                RootMenuObjectIdentity::Load,
            ]
        );
    }
}
