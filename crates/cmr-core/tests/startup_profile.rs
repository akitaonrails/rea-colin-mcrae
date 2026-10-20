use cmr_assets::{AssetIndex, AssetProfile};
use cmr_core::startup::{Language, StartupScreen, select_startup_assets};

const PROFILE: &str = include_str!("../../../profiles/installed-eu-derived-assets-v2.json");

#[test]
fn every_recovered_startup_path_resolves_in_the_reviewed_profile() {
    let profile: AssetProfile = serde_json::from_str(PROFILE).unwrap();
    let index = AssetIndex::from_profile(&profile).unwrap();

    for language in [
        Language::English,
        Language::French,
        Language::German,
        Language::Spanish,
    ] {
        for screen in [StartupScreen::Loading, StartupScreen::Legal] {
            let assets = select_startup_assets(screen, language);
            index.resolve(assets.palette).unwrap();
            index.resolve(&assets.bitmap).unwrap();
        }
    }
}
