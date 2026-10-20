use cmr_assets::{AssetIndex, AssetProfile};
use cmr_core::font_assets::FontStyle;

const PROFILE: &str = include_str!("../../../profiles/installed-eu-derived-assets-v2.json");

#[test]
fn every_recovered_font_path_resolves_in_the_reviewed_profile() {
    let profile: AssetProfile = serde_json::from_str(PROFILE).unwrap();
    let index = AssetIndex::from_profile(&profile).unwrap();

    for style in FontStyle::ALL {
        index.resolve(style.path()).unwrap();
    }
}
