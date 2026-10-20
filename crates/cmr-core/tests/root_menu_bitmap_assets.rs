use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use cmr_assets::{
    AssetFile, AssetProfile, AssetStore, ReviewedProfile, StorageEncoding, import_source,
};
use cmr_core::localization::Language;
use cmr_core::root_menu_assets::{
    CenterIconIdentity, ROOT_MENU_BACKDROP_PATH, ROOT_MENU_SELECT_PATH, VisualVariant,
    localized_root_menu_assets,
};
use cmr_core::root_menu_bitmap_assets::{
    ROOT_MENU_BITMAP_REQUEST_COUNT, ROOT_MENU_OPTIONAL_BITMAP_REQUEST_COUNT,
    ROOT_MENU_REQUIRED_BITMAP_REQUEST_COUNT, RootMenuBitmapIdentity, RootMenuBitmapLoadError,
    RootMenuBitmapSlot, load_root_menu_bitmaps,
};
use cmr_core::root_menu_bitmap_raster::{RootMenuSurfaceDiagnostic, prepare_root_menu_surfaces};
use cmr_formats::bitmap::{BitmapError, BitmapLimits};
use cmr_formats::riff_palette::{LegacyPalette256, PaletteEntry};
use cmr_formats::stored_asset::DecodeLimits;
use sha2::{Digest, Sha256};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("cmr-root-menu-bitmaps-{}-{id}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn loads_all_seventeen_requests_in_recovered_order() {
    let language = Language::French;
    let variant = VisualVariant::GPrefixed;
    let fixture = Fixture::new(&complete_files(language, variant));

    let loaded =
        load_root_menu_bitmaps(&fixture.store, language, variant, BitmapLimits::default()).unwrap();
    assert_eq!(loaded.language(), language);
    assert_eq!(loaded.center_variant(), variant);
    assert_eq!(loaded.loaded_len(), ROOT_MENU_BITMAP_REQUEST_COUNT);
    assert_eq!(loaded.optional_diagnostic_len(), 0);

    let mut expected = vec![
        RootMenuBitmapIdentity::MainSelected,
        RootMenuBitmapIdentity::MainShadow,
    ];
    expected.extend((0..7).map(RootMenuBitmapIdentity::AnimationFrame));
    expected.extend([
        RootMenuBitmapIdentity::Backdrop,
        RootMenuBitmapIdentity::Selection,
    ]);
    expected.extend(
        CenterIconIdentity::ALL
            .map(|identity| RootMenuBitmapIdentity::CenterIcon { identity, variant }),
    );
    assert_eq!(
        loaded
            .iter()
            .map(|asset| asset.identity)
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(
        loaded
            .asset(RootMenuBitmapIdentity::Selection)
            .unwrap()
            .path,
        ROOT_MENU_SELECT_PATH
    );
}

#[test]
fn preparation_keeps_load_and_conversion_holes_in_canonical_slot_order() {
    let language = Language::English;
    let variant = VisualVariant::Normal;
    let localized = localized_root_menu_assets(language);
    let mut files = complete_files(language, variant);
    files.remove(localized.main_selected);
    files.get_mut(localized.main_shadow).unwrap()[18..22].copy_from_slice(&3i32.to_le_bytes());
    let fixture = Fixture::new(&files);
    let loaded =
        load_root_menu_bitmaps(&fixture.store, language, variant, BitmapLimits::default()).unwrap();

    let prepared = prepare_root_menu_surfaces(
        loaded,
        &LegacyPalette256 {
            entries: [PaletteEntry {
                red: 1,
                green: 2,
                blue: 3,
                flags: 4,
            }; 256],
        },
    )
    .unwrap();

    assert_eq!(prepared.language(), language);
    assert_eq!(prepared.center_variant(), variant);
    let slots = prepared.slots().collect::<Vec<_>>();
    assert_eq!(slots.len(), ROOT_MENU_BITMAP_REQUEST_COUNT);
    assert_eq!(slots[0], (RootMenuBitmapSlot::MainSelected, None));
    assert_eq!(slots[1], (RootMenuBitmapSlot::MainShadow, None));
    assert!(slots[RootMenuBitmapSlot::Backdrop.index()].1.is_some());
    let diagnostics = prepared.optional_diagnostics().collect::<Vec<_>>();
    assert!(matches!(
        diagnostics[0],
        RootMenuSurfaceDiagnostic::Load(RootMenuBitmapLoadError::Asset {
            identity: RootMenuBitmapIdentity::MainSelected,
            ..
        })
    ));
    assert!(matches!(
        diagnostics[1],
        RootMenuSurfaceDiagnostic::Surface {
            slot: RootMenuBitmapSlot::MainShadow,
            source: cmr_core::surface::SurfaceError::UnsupportedUnalignedWidth(3),
            ..
        }
    ));
}

#[test]
fn missing_and_malformed_optional_assets_are_diagnosed_and_loading_continues() {
    let language = Language::English;
    let variant = VisualVariant::Normal;
    let localized = localized_root_menu_assets(language);
    let mut files = complete_files(language, variant);
    files.remove(localized.main_selected);
    set_compression(files.get_mut(localized.animation[3]).unwrap(), 1);
    let fixture = Fixture::new(&files);

    let loaded =
        load_root_menu_bitmaps(&fixture.store, language, variant, BitmapLimits::default()).unwrap();
    assert_eq!(loaded.loaded_len(), ROOT_MENU_BITMAP_REQUEST_COUNT - 2);
    assert_eq!(loaded.optional_diagnostic_len(), 2);
    assert!(loaded.asset(RootMenuBitmapIdentity::MainSelected).is_none());
    assert!(
        loaded
            .asset(RootMenuBitmapIdentity::AnimationFrame(3))
            .is_none()
    );
    assert!(loaded.asset(RootMenuBitmapIdentity::Backdrop).is_some());
    assert!(loaded.asset(RootMenuBitmapIdentity::Selection).is_some());

    let diagnostics = loaded.optional_diagnostics().collect::<Vec<_>>();
    assert!(matches!(
        diagnostics[0],
        RootMenuBitmapLoadError::Asset {
            identity: RootMenuBitmapIdentity::MainSelected,
            path,
            ..
        } if *path == localized.main_selected
    ));
    assert!(matches!(
        diagnostics[1],
        RootMenuBitmapLoadError::Decode {
            identity: RootMenuBitmapIdentity::AnimationFrame(3),
            path,
            source: BitmapError::UnsupportedCompression(1),
        } if *path == localized.animation[3]
    ));
    assert_eq!(
        diagnostics[0].identity(),
        RootMenuBitmapIdentity::MainSelected
    );
    assert_eq!(diagnostics[1].path(), localized.animation[3]);
}

#[test]
fn italian_uses_distinct_paths_and_skips_missing_optionals_without_fallback() {
    let language = Language::Italian;
    let variant = VisualVariant::Normal;
    let fixture = Fixture::new(&mandatory_files(variant));

    let loaded =
        load_root_menu_bitmaps(&fixture.store, language, variant, BitmapLimits::default()).unwrap();
    assert_eq!(loaded.loaded_len(), ROOT_MENU_REQUIRED_BITMAP_REQUEST_COUNT);
    assert_eq!(
        loaded.optional_diagnostic_len(),
        ROOT_MENU_OPTIONAL_BITMAP_REQUEST_COUNT
    );
    assert!(
        loaded
            .optional_diagnostics()
            .all(|diagnostic| diagnostic.path().contains("/Italian/"))
    );
    assert!(
        loaded
            .optional_diagnostics()
            .all(|diagnostic| matches!(diagnostic, RootMenuBitmapLoadError::Asset { .. }))
    );
    assert!(loaded.iter().all(|asset| !asset.path.contains("/English/")));
    assert!(loaded.asset(RootMenuBitmapIdentity::Backdrop).is_some());
}

#[test]
fn missing_required_asset_fails_with_exact_context() {
    let language = Language::Spanish;
    let variant = VisualVariant::Normal;
    let mut files = complete_files(language, variant);
    files.remove(ROOT_MENU_SELECT_PATH);
    let fixture = Fixture::new(&files);

    assert!(matches!(
        load_root_menu_bitmaps(&fixture.store, language, variant, BitmapLimits::default(),),
        Err(RootMenuBitmapLoadError::Asset {
            identity: RootMenuBitmapIdentity::Selection,
            path: ROOT_MENU_SELECT_PATH,
            ..
        })
    ));
}

#[test]
fn malformed_required_asset_fails_with_exact_context() {
    let language = Language::German;
    let variant = VisualVariant::Normal;
    let identity = CenterIconIdentity::Cone;
    let malformed_path = identity.path(variant);
    let mut files = complete_files(language, variant);
    set_compression(files.get_mut(malformed_path).unwrap(), 1);
    let fixture = Fixture::new(&files);

    assert!(matches!(
        load_root_menu_bitmaps(
            &fixture.store,
            language,
            variant,
            BitmapLimits::default(),
        ),
        Err(RootMenuBitmapLoadError::Decode {
            identity: RootMenuBitmapIdentity::CenterIcon {
                identity: CenterIconIdentity::Cone,
                variant: VisualVariant::Normal,
            },
            path,
            source: BitmapError::UnsupportedCompression(1),
        }) if path == malformed_path
    ));
}

#[test]
fn limits_skip_an_oversized_optional_with_exact_context_and_continue() {
    let language = Language::English;
    let variant = VisualVariant::Normal;
    let oversized_path = localized_root_menu_assets(language).main_selected;
    let mut files = complete_files(language, variant);
    files.get_mut(oversized_path).unwrap()[18..22].copy_from_slice(&8i32.to_le_bytes());
    let fixture = Fixture::new(&files);

    let loaded = load_root_menu_bitmaps(
        &fixture.store,
        language,
        variant,
        BitmapLimits {
            max_width: 4,
            ..BitmapLimits::default()
        },
    )
    .unwrap();
    assert_eq!(loaded.loaded_len(), ROOT_MENU_BITMAP_REQUEST_COUNT - 1);
    assert!(matches!(
        loaded.optional_diagnostics().next().unwrap(),
        RootMenuBitmapLoadError::Decode {
            identity: RootMenuBitmapIdentity::MainSelected,
            path,
            source: BitmapError::LimitsExceeded,
        } if *path == oversized_path
    ));
    assert!(loaded.asset(RootMenuBitmapIdentity::Backdrop).is_some());
}

#[test]
fn limits_fail_at_first_required_bitmap_with_exact_context() {
    let language = Language::English;
    let variant = VisualVariant::Normal;
    let fixture = Fixture::new(&complete_files(language, variant));

    assert!(matches!(
        load_root_menu_bitmaps(
            &fixture.store,
            language,
            variant,
            BitmapLimits {
                max_width: 0,
                ..BitmapLimits::default()
            },
        ),
        Err(RootMenuBitmapLoadError::Decode {
            identity: RootMenuBitmapIdentity::Backdrop,
            path: ROOT_MENU_BACKDROP_PATH,
            source: BitmapError::LimitsExceeded,
        })
    ));
}

struct Fixture {
    _temporary: TestDirectory,
    store: AssetStore,
}

impl Fixture {
    fn new(files: &BTreeMap<&'static str, Vec<u8>>) -> Self {
        let temporary = TestDirectory::new();
        let source = temporary.0.join("source");
        fs::create_dir_all(source.join("New_Fe")).unwrap();

        let mut profile_files = Vec::with_capacity(files.len());
        for (path, bytes) in files {
            let destination = source.join(path);
            fs::create_dir_all(destination.parent().unwrap()).unwrap();
            fs::write(&destination, bytes).unwrap();
            profile_files.push(asset(path, bytes));
        }
        profile_files.sort_by(|left, right| left.path.cmp(&right.path));

        let profile = AssetProfile {
            schema_version: 1,
            id: "synthetic-root-menu-bitmaps".to_owned(),
            roots: vec!["New_Fe".to_owned()],
            empty_directories: Vec::new(),
            sentinels: Vec::new(),
            files: profile_files,
        };
        let reviewed = ReviewedProfile::from_json(&serde_json::to_vec(&profile).unwrap()).unwrap();
        let imported = temporary.0.join("runtime-data");
        import_source(&source, &imported, &reviewed).unwrap();
        let store = AssetStore::open(
            &imported,
            &reviewed,
            DecodeLimits {
                max_stored_bytes: 64 * 1024,
                max_payload_bytes: 64 * 1024,
            },
        )
        .unwrap();
        Self {
            _temporary: temporary,
            store,
        }
    }
}

fn complete_files(language: Language, variant: VisualVariant) -> BTreeMap<&'static str, Vec<u8>> {
    let localized = localized_root_menu_assets(language);
    let mut paths = vec![localized.main_selected, localized.main_shadow];
    paths.extend(localized.animation);
    paths.extend([ROOT_MENU_BACKDROP_PATH, ROOT_MENU_SELECT_PATH]);
    paths.extend(CenterIconIdentity::ALL.map(|identity| identity.path(variant)));
    files_for_paths(paths)
}

fn mandatory_files(variant: VisualVariant) -> BTreeMap<&'static str, Vec<u8>> {
    let mut paths = vec![ROOT_MENU_BACKDROP_PATH, ROOT_MENU_SELECT_PATH];
    paths.extend(CenterIconIdentity::ALL.map(|identity| identity.path(variant)));
    files_for_paths(paths)
}

fn files_for_paths(paths: Vec<&'static str>) -> BTreeMap<&'static str, Vec<u8>> {
    paths
        .into_iter()
        .enumerate()
        .map(|(index, path)| (path, synthetic_bitmap(index as u8)))
        .collect()
}

fn set_compression(bitmap: &mut [u8], compression: u32) {
    bitmap[30..34].copy_from_slice(&compression.to_le_bytes());
}

fn synthetic_bitmap(pixel: u8) -> Vec<u8> {
    let palette_entries = 256;
    let pixel_offset = 14 + 40 + palette_entries * 4;
    let file_size = pixel_offset + 4;
    let mut input = vec![0; file_size];
    input[0..2].copy_from_slice(b"BM");
    input[2..6].copy_from_slice(&(file_size as u32).to_le_bytes());
    input[10..14].copy_from_slice(&(pixel_offset as u32).to_le_bytes());
    input[14..18].copy_from_slice(&40u32.to_le_bytes());
    input[18..22].copy_from_slice(&4i32.to_le_bytes());
    input[22..26].copy_from_slice(&1i32.to_le_bytes());
    input[26..28].copy_from_slice(&1u16.to_le_bytes());
    input[28..30].copy_from_slice(&8u16.to_le_bytes());
    input[34..38].copy_from_slice(&4u32.to_le_bytes());
    input[46..50].copy_from_slice(&(palette_entries as u32).to_le_bytes());
    input[pixel_offset..pixel_offset + 4].fill(pixel);
    input
}

fn asset(path: &str, bytes: &[u8]) -> AssetFile {
    AssetFile {
        path: path.to_owned(),
        size: bytes.len() as u64,
        sha256: hex_sha256(bytes),
        encoding: StorageEncoding::Raw,
    }
}

fn hex_sha256(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(encoded, "{byte:02x}").unwrap();
    }
    encoded
}
