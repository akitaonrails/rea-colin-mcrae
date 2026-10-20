use cmr_assets::{AssetIndex, AssetProfile};
use cmr_core::localization::Language;
use cmr_core::root_menu_assets::{
    CenterIconIdentity, ROOT_MENU_BACKDROP_PATH, ROOT_MENU_SELECT_PATH, RootMenuObjectIdentity,
    VisualVariant, localized_root_menu_assets,
};

const PROFILE: &str = include_str!("../../../profiles/installed-eu-derived-assets-v2.json");

#[test]
fn present_root_menu_visuals_resolve_and_missing_italian_row_is_explicit() {
    let profile: AssetProfile = serde_json::from_str(PROFILE).unwrap();
    let index = AssetIndex::from_profile(&profile).unwrap();

    index.resolve(ROOT_MENU_BACKDROP_PATH).unwrap();
    index.resolve(ROOT_MENU_SELECT_PATH).unwrap();

    for language in [
        Language::English,
        Language::French,
        Language::German,
        Language::Spanish,
    ] {
        let assets = localized_root_menu_assets(language);
        index.resolve(assets.main_selected).unwrap();
        index.resolve(assets.main_shadow).unwrap();
        for animation in assets.animation {
            index.resolve(animation).unwrap();
        }
    }

    let italian = localized_root_menu_assets(Language::Italian);
    assert!(index.resolve(italian.main_selected).is_err());
    assert!(index.resolve(italian.main_shadow).is_err());
    for animation in italian.animation {
        assert!(index.resolve(animation).is_err());
    }

    for variant in [VisualVariant::Normal, VisualVariant::GPrefixed] {
        for identity in CenterIconIdentity::ALL {
            index.resolve(identity.path(variant)).unwrap();
        }
        for identity in RootMenuObjectIdentity::ALL {
            index.resolve(identity.path(variant)).unwrap();
        }
    }
}
