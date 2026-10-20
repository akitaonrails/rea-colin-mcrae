use std::env;
use std::error::Error;
use std::fs;
use std::path::PathBuf;

use cmr_assets::{AssetStore, ReviewedProfile};
use cmr_core::startup::{StartupLanguage, StartupScreen};
use cmr_core::startup_surface::load_startup_surface;
use cmr_core::surface::PaletteMapping;
use cmr_formats::bitmap::BitmapLimits;
use cmr_formats::stored_asset::DecodeLimits;

fn main() -> Result<(), Box<dyn Error>> {
    let (runtime_root, profile_path, screen, language) = arguments()?;
    let profile_document = fs::read(profile_path)?;
    let reviewed = ReviewedProfile::from_json(&profile_document)?;
    let store = AssetStore::open(
        &runtime_root,
        &reviewed,
        DecodeLimits {
            max_stored_bytes: 64 * 1024 * 1024,
            max_payload_bytes: 256 * 1024 * 1024,
        },
    )?;
    let identity = load_startup_surface(
        &store,
        screen,
        language,
        PaletteMapping::Identity,
        BitmapLimits::default(),
    )?;
    let nearest = load_startup_surface(
        &store,
        screen,
        language,
        PaletteMapping::NearestNonzero,
        BitmapLimits::default(),
    )?;
    let changed = identity
        .pixels
        .iter()
        .zip(&nearest.pixels)
        .filter(|(identity, nearest)| identity != nearest)
        .count();
    println!(
        "{}x{} pixels={} nearest-remapped={changed}",
        nearest.width,
        nearest.height,
        nearest.pixels.len()
    );
    Ok(())
}

fn arguments() -> Result<(PathBuf, PathBuf, StartupScreen, StartupLanguage), Box<dyn Error>> {
    let mut arguments = env::args_os();
    let executable = arguments.next().unwrap_or_default();
    let usage = || {
        format!(
            "usage: {} RUNTIME_ROOT PROFILE <loading|legal> <english|french|german|spanish>",
            PathBuf::from(&executable)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
        )
    };
    let Some(runtime_root) = arguments.next() else {
        return Err(usage().into());
    };
    let Some(profile) = arguments.next() else {
        return Err(usage().into());
    };
    let runtime_root = runtime_root.into();
    let profile = profile.into();
    let screen = match arguments.next().and_then(|value| value.into_string().ok()) {
        Some(value) if value.eq_ignore_ascii_case("loading") => StartupScreen::Loading,
        Some(value) if value.eq_ignore_ascii_case("legal") => StartupScreen::Legal,
        _ => return Err(usage().into()),
    };
    let language = match arguments.next().and_then(|value| value.into_string().ok()) {
        Some(value) if value.eq_ignore_ascii_case("english") => StartupLanguage::English,
        Some(value) if value.eq_ignore_ascii_case("french") => StartupLanguage::French,
        Some(value) if value.eq_ignore_ascii_case("german") => StartupLanguage::German,
        Some(value) if value.eq_ignore_ascii_case("spanish") => StartupLanguage::Spanish,
        _ => return Err(usage().into()),
    };
    if arguments.next().is_some() {
        return Err(usage().into());
    }
    Ok((runtime_root, profile, screen, language))
}
