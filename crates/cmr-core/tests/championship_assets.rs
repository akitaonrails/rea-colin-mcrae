use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use cmr_assets::{
    AssetFile, AssetProfile, AssetStore, ReviewedProfile, StorageEncoding, import_source,
};
use cmr_core::championship_assets::{
    CHAMPIONSHIP_CAR_COUNT, CHAMPIONSHIP_CARS, ChampionshipAssetLoadError, ChampionshipAssetRole,
    MANUFACTURER_LOGO_COUNT, MANUFACTURER_LOGOS, ManufacturerLogo, load_championship_assets,
};
use cmr_core::championship_setup::CanonicalCar;
use cmr_formats::bitmap::{BitmapError, BitmapLimits};
use cmr_formats::c3d::{C3D_FORMAT_VERSION, C3D_MAGIC, C3dError, C3dLimits};
use cmr_formats::riff_palette::PaletteError;
use cmr_formats::stored_asset::DecodeLimits;
use sha2::{Digest, Sha256};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
const REVIEWED_PROFILE: &[u8] =
    include_bytes!("../../../profiles/installed-eu-derived-assets-v2.json");

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "cmr-championship-assets-{}-{id}",
            std::process::id()
        ));
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
fn descriptors_preserve_canonical_order_paths_and_logo_aliases() {
    assert_eq!(CHAMPIONSHIP_CAR_COUNT, 14);
    assert_eq!(MANUFACTURER_LOGO_COUNT, 11);
    assert_eq!(CHAMPIONSHIP_CARS.map(|entry| entry.car), CanonicalCar::ALL);
    assert_eq!(
        CHAMPIONSHIP_CARS.map(|entry| entry.logo),
        [
            ManufacturerLogo::Subaru,
            ManufacturerLogo::Seat,
            ManufacturerLogo::Skoda,
            ManufacturerLogo::Audi,
            ManufacturerLogo::Ford,
            ManufacturerLogo::Ford,
            ManufacturerLogo::Mitsu,
            ManufacturerLogo::Vw,
            ManufacturerLogo::Lancia,
            ManufacturerLogo::Toyo,
            ManufacturerLogo::Ford,
            ManufacturerLogo::Reno,
            ManufacturerLogo::M4r,
            ManufacturerLogo::Toyo,
        ]
    );
    assert_eq!(
        CHAMPIONSHIP_CARS[CanonicalCar::Skoda.index() as usize].object_path,
        "New_Fe/Objects/Cars/Skoda.C3D"
    );
    assert_eq!(
        CHAMPIONSHIP_CARS[CanonicalCar::Corolla.index() as usize].object_path,
        "New_Fe/Objects/Cars/Corolla.C3D"
    );
    assert_eq!(
        CHAMPIONSHIP_CARS[CanonicalCar::Megane.index() as usize].object_path,
        "New_Fe/Objects/Cars/Megane.C3D"
    );

    let unique_paths = MANUFACTURER_LOGOS
        .into_iter()
        .map(|entry| entry.path)
        .collect::<BTreeSet<_>>();
    assert_eq!(unique_paths.len(), MANUFACTURER_LOGO_COUNT);
    assert_eq!(
        CanonicalCar::Celica.championship_descriptor().logo,
        ManufacturerLogo::Toyo
    );
}

#[test]
fn complete_store_loads_canonical_cars_and_one_copy_of_each_logo() {
    let fixture = Fixture::new(&complete_files());
    let loaded = load_championship_assets(
        &fixture.store,
        C3dLimits::default(),
        BitmapLimits::default(),
    )
    .unwrap();

    assert_eq!(loaded.cars().len(), CHAMPIONSHIP_CAR_COUNT);
    assert_eq!(loaded.logos().len(), MANUFACTURER_LOGO_COUNT);
    for (index, car) in CanonicalCar::ALL.into_iter().enumerate() {
        let asset = loaded.car(car);
        assert_eq!(asset.descriptor, CHAMPIONSHIP_CARS[index]);
        assert_eq!(asset.object.header().unknown_08[0], index as u8);
        assert_eq!(asset.palette.entries[0].red, 50 + index as u8);
    }
    for (index, logo) in ManufacturerLogo::ALL.into_iter().enumerate() {
        let asset = loaded.logo(logo);
        assert_eq!(asset.descriptor, MANUFACTURER_LOGOS[index]);
        assert_eq!(asset.bitmap.pixels[0], 100 + index as u8);
    }

    assert!(std::ptr::eq(
        loaded.logo_for_car(CanonicalCar::Rs200),
        loaded.logo_for_car(CanonicalCar::Mk2)
    ));
    assert!(std::ptr::eq(
        loaded.logo_for_car(CanonicalCar::Mk2),
        loaded.logo_for_car(CanonicalCar::Escort)
    ));
    assert!(std::ptr::eq(
        loaded.logo_for_car(CanonicalCar::Corolla),
        loaded.logo_for_car(CanonicalCar::Celica)
    ));
}

#[test]
#[ignore = "requires a user-owned import selected with CMR_RUNTIME_DATA"]
fn reviewed_import_loads_every_championship_asset() {
    let runtime_root = std::env::var_os("CMR_RUNTIME_DATA")
        .map(PathBuf::from)
        .expect("CMR_RUNTIME_DATA must name an imported runtime-data directory");
    let reviewed = ReviewedProfile::from_json(REVIEWED_PROFILE).expect("reviewed profile is valid");
    let store = AssetStore::open(
        &runtime_root,
        &reviewed,
        DecodeLimits {
            max_stored_bytes: 64 * 1024 * 1024,
            max_payload_bytes: 256 * 1024 * 1024,
        },
    )
    .expect("CMR_RUNTIME_DATA must contain a complete matching reviewed import");

    let loaded = load_championship_assets(&store, C3dLimits::default(), BitmapLimits::default())
        .expect("reviewed import contains every championship asset");
    assert_eq!(loaded.cars().len(), CHAMPIONSHIP_CAR_COUNT);
    assert_eq!(loaded.logos().len(), MANUFACTURER_LOGO_COUNT);
    assert_eq!(
        loaded
            .logo_for_car(CanonicalCar::Celica)
            .descriptor
            .identity,
        ManufacturerLogo::Toyo
    );
}

#[test]
fn missing_asset_reports_the_exact_role_and_source_relative_path() {
    let mut files = complete_files();
    let descriptor = CanonicalCar::Escort.championship_descriptor();
    files.remove(descriptor.palette_path);
    let fixture = Fixture::new(&files);

    let error = load_championship_assets(
        &fixture.store,
        C3dLimits::default(),
        BitmapLimits::default(),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        ChampionshipAssetLoadError::Asset {
            role: ChampionshipAssetRole::CarPalette(CanonicalCar::Escort),
            path,
            ..
        } if path == descriptor.palette_path
    ));
    assert_eq!(
        error.role(),
        ChampionshipAssetRole::CarPalette(CanonicalCar::Escort)
    );
    assert_eq!(error.path(), descriptor.palette_path);
}

#[test]
fn malformed_object_and_palette_report_their_typed_decode_boundaries() {
    let subaru = CanonicalCar::Subaru.championship_descriptor();

    let mut malformed_object = complete_files();
    malformed_object.get_mut(subaru.object_path).unwrap()[0..4].copy_from_slice(b"NOPE");
    let fixture = Fixture::new(&malformed_object);
    assert!(matches!(
        load_championship_assets(
            &fixture.store,
            C3dLimits::default(),
            BitmapLimits::default()
        ),
        Err(ChampionshipAssetLoadError::Object {
            car: CanonicalCar::Subaru,
            path,
            source: C3dError::InvalidMagic(actual),
        }) if path == subaru.object_path && actual == *b"NOPE"
    ));

    let mut malformed_palette = complete_files();
    malformed_palette.insert(subaru.palette_path, b"NOPE-not-riff".to_vec());
    let fixture = Fixture::new(&malformed_palette);
    assert!(matches!(
        load_championship_assets(
            &fixture.store,
            C3dLimits::default(),
            BitmapLimits::default()
        ),
        Err(ChampionshipAssetLoadError::Palette {
            car: CanonicalCar::Subaru,
            path,
            source: PaletteError::NotRiff,
        }) if path == subaru.palette_path
    ));
}

#[test]
fn truncated_logo_reports_unique_identity_and_exact_path() {
    let descriptor = ManufacturerLogo::Ford.descriptor();
    let mut files = complete_files();
    files.insert(descriptor.path, b"BM".to_vec());
    let fixture = Fixture::new(&files);

    let error = load_championship_assets(
        &fixture.store,
        C3dLimits::default(),
        BitmapLimits::default(),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        ChampionshipAssetLoadError::Logo {
            logo: ManufacturerLogo::Ford,
            path,
            source: BitmapError::Truncated,
        } if path == descriptor.path
    ));
    assert_eq!(
        error.role(),
        ChampionshipAssetRole::ManufacturerLogo(ManufacturerLogo::Ford)
    );
    assert_eq!(error.path(), descriptor.path);
}

#[test]
fn caller_limits_are_forwarded_to_bounded_parsers() {
    let fixture = Fixture::new(&complete_files());
    assert!(matches!(
        load_championship_assets(
            &fixture.store,
            C3dLimits {
                max_payload_bytes: 31,
                ..C3dLimits::default()
            },
            BitmapLimits::default()
        ),
        Err(ChampionshipAssetLoadError::Object {
            car: CanonicalCar::Subaru,
            source: C3dError::PayloadTooLarge {
                actual: 64,
                limit: 31,
            },
            ..
        })
    ));

    assert!(matches!(
        load_championship_assets(
            &fixture.store,
            C3dLimits::default(),
            BitmapLimits {
                max_width: 3,
                ..BitmapLimits::default()
            }
        ),
        Err(ChampionshipAssetLoadError::Logo {
            logo: ManufacturerLogo::Subaru,
            source: BitmapError::LimitsExceeded,
            ..
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
        fs::create_dir(&source).unwrap();

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
            id: "synthetic-championship-assets".to_owned(),
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
                max_stored_bytes: 128 * 1024,
                max_payload_bytes: 128 * 1024,
            },
        )
        .unwrap();
        Self {
            _temporary: temporary,
            store,
        }
    }
}

fn complete_files() -> BTreeMap<&'static str, Vec<u8>> {
    let mut files = BTreeMap::new();
    for (index, descriptor) in CHAMPIONSHIP_CARS.into_iter().enumerate() {
        files.insert(descriptor.object_path, synthetic_c3d(index as u8));
        files.insert(descriptor.palette_path, synthetic_palette(50 + index as u8));
    }
    for (index, descriptor) in MANUFACTURER_LOGOS.into_iter().enumerate() {
        files.insert(descriptor.path, synthetic_bitmap(100 + index as u8));
    }
    files
}

fn synthetic_c3d(marker: u8) -> Vec<u8> {
    let mut input = vec![0; 64];
    input[0..4].copy_from_slice(&C3D_MAGIC);
    input[4..8].copy_from_slice(&C3D_FORMAT_VERSION.to_le_bytes());
    input[8..12].copy_from_slice(&[marker, 0xa1, 0xb2, 0xc3]);
    for (index, count) in [0u16, 0, 1].into_iter().enumerate() {
        let start = 12 + index * 2;
        input[start..start + 2].copy_from_slice(&count.to_le_bytes());
    }
    for (index, offset) in [32u32, 40, 48].into_iter().enumerate() {
        let start = 20 + index * 4;
        input[start..start + 4].copy_from_slice(&offset.to_le_bytes());
    }
    input
}

fn synthetic_palette(marker: u8) -> Vec<u8> {
    let data_size = 4 + 256 * 4;
    let file_size = 12 + 8 + data_size;
    let mut input = vec![0; file_size];
    input[0..4].copy_from_slice(b"RIFF");
    input[4..8].copy_from_slice(&((file_size - 8) as u32).to_le_bytes());
    input[8..12].copy_from_slice(b"PAL ");
    input[12..16].copy_from_slice(b"data");
    input[16..20].copy_from_slice(&(data_size as u32).to_le_bytes());
    input[20..22].copy_from_slice(&0x0300u16.to_le_bytes());
    input[22..24].copy_from_slice(&256u16.to_le_bytes());
    input[24] = marker;
    input
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
