use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cmr_assets::{
    AssetFile, AssetProfile, AssetStore, ReviewedProfile, StorageEncoding, import_source,
};
use cmr_core::championship_assets::{
    CHAMPIONSHIP_CARS, MANUFACTURER_LOGOS, ManufacturerLogo, load_championship_assets,
};
use cmr_core::championship_frame_assets::{
    CHEQUERED_FRAME_DESCRIPTORS, CHEQUERED_PALETTE_DESCRIPTOR, ChequeredFrame, INFO_BOX_DESCRIPTOR,
    load_championship_frame_assets,
};
use cmr_core::championship_setup::CanonicalCar;
use cmr_formats::bitmap::BitmapLimits;
use cmr_formats::c3d::{C3D_FORMAT_VERSION, C3D_MAGIC, C3dLimits};
use cmr_formats::stored_asset::DecodeLimits;
use cmr_video::ordinary_bitmap_rgb16::{OrdinaryBitmapRgb16Error, OrdinaryBitmapRgb16Layout};
use cmr_video::software_championship_ordinary_rgb16::{
    CHAMPIONSHIP_ORDINARY_RGB16_SOURCE_COUNT, ChampionshipOrdinaryRgb16Identity,
    SoftwareChampionshipOrdinaryRgb16Pool, prepare_software_championship_ordinary_rgb16_pool,
};
use sha2::{Digest, Sha256};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
const SOURCE_INDEX: u8 = 7;

#[test]
fn public_pool_maps_exact_27_source_identity_order_and_deduplicated_car_logos() {
    let fixture = Fixture::new(Fault::None);
    let (championship, frames) = fixture.load();
    let pool =
        prepare_software_championship_ordinary_rgb16_pool(&championship, &frames, 5).unwrap();

    assert_eq!(pool.len(), CHAMPIONSHIP_ORDINARY_RGB16_SOURCE_COUNT);
    assert_eq!(pool.len(), 27);
    assert!(!pool.is_empty());
    assert_eq!(pool.display_format_discriminator(), 5);

    let expected = std::iter::once(ChampionshipOrdinaryRgb16Identity::InfoBox)
        .chain(ManufacturerLogo::ALL.map(ChampionshipOrdinaryRgb16Identity::ManufacturerLogo))
        .chain(ChequeredFrame::ALL.map(ChampionshipOrdinaryRgb16Identity::ChequeredFrame))
        .collect::<Vec<_>>();
    assert_eq!(
        pool.iter()
            .map(|(identity, _)| identity)
            .collect::<Vec<_>>(),
        expected
    );
    for (identity, source) in pool.iter() {
        assert!(std::ptr::eq(source, pool.get(identity)));
    }

    for car in CanonicalCar::ALL {
        let logo = car.championship_descriptor().logo;
        assert!(std::ptr::eq(
            pool.logo(logo),
            pool.get(ChampionshipOrdinaryRgb16Identity::ManufacturerLogo(logo))
        ));
    }
    assert!(std::ptr::eq(
        pool.logo(CanonicalCar::Rs200.championship_descriptor().logo),
        pool.logo(CanonicalCar::Mk2.championship_descriptor().logo)
    ));
    assert!(std::ptr::eq(
        pool.logo(CanonicalCar::Mk2.championship_descriptor().logo),
        pool.logo(CanonicalCar::Escort.championship_descriptor().logo)
    ));
    assert!(std::ptr::eq(
        pool.logo(CanonicalCar::Corolla.championship_descriptor().logo),
        pool.logo(CanonicalCar::Celica.championship_descriptor().logo)
    ));
}

#[test]
fn public_pool_uses_each_embedded_palette_not_fche_and_keeps_one_layout() {
    let fixture = Fixture::new(Fault::None);
    let (championship, frames) = fixture.load();
    let rgb555 =
        prepare_software_championship_ordinary_rgb16_pool(&championship, &frames, 5).unwrap();
    let rgb565 =
        prepare_software_championship_ordinary_rgb16_pool(&championship, &frames, 6).unwrap();

    for (_, source) in rgb555.iter() {
        assert_eq!(source.display_format_discriminator(), 5);
        assert_eq!(source.layout(), OrdinaryBitmapRgb16Layout::Rgb555);
        assert_ne!(source.packed_color_key(), 0);
    }
    for (_, source) in rgb565.iter() {
        assert_eq!(source.display_format_discriminator(), 6);
        assert_eq!(source.layout(), OrdinaryBitmapRgb16Layout::Rgb565);
        assert_ne!(source.packed_color_key(), 0);
    }

    let info_rgb = embedded_source_rgb(5);
    let subaru_rgb = embedded_source_rgb(20 + ManufacturerLogo::Subaru.index() as u8);
    assert_eq!(rgb555.info_box().logical_words()[0], pack555(info_rgb));
    assert_eq!(
        rgb555.logo(ManufacturerLogo::Subaru).logical_words()[0],
        pack555(subaru_rgb)
    );
    assert_ne!(
        rgb555.info_box().logical_words()[0],
        rgb555.logo(ManufacturerLogo::Subaru).logical_words()[0],
        "the same source index must be expanded through each bitmap's own palette"
    );

    let first_frame_rgb = embedded_source_rgb(80);
    let fche = frames.chequered_palette.entries[usize::from(SOURCE_INDEX)];
    let fche_rgb = [fche.red, fche.green, fche.blue];
    assert_ne!(first_frame_rgb, fche_rgb);
    assert_eq!(
        rgb555.frame(ChequeredFrame::Frame01).logical_words()[0],
        pack555(first_frame_rgb)
    );
    assert_ne!(
        rgb555.frame(ChequeredFrame::Frame01).logical_words()[0],
        pack555(fche_rgb),
        "the external FCHE display palette must not replace the BMP's embedded palette"
    );
    assert_eq!(
        rgb565.frame(ChequeredFrame::Frame01).logical_words()[0],
        pack565(first_frame_rgb)
    );
}

#[test]
fn public_pool_middle_and_last_failures_publish_no_replacement_and_keep_relative_context() {
    let baseline_fixture = Fixture::new(Fault::None);
    let (baseline_championship, baseline_frames) = baseline_fixture.load();
    let published = prepare_software_championship_ordinary_rgb16_pool(
        &baseline_championship,
        &baseline_frames,
        5,
    )
    .unwrap();
    let before = fingerprint(&published);

    for (fault, expected_identity, expected_path) in [
        (
            Fault::Logo(ManufacturerLogo::Mitsu),
            ChampionshipOrdinaryRgb16Identity::ManufacturerLogo(ManufacturerLogo::Mitsu),
            ManufacturerLogo::Mitsu.descriptor().path,
        ),
        (
            Fault::Frame(ChequeredFrame::Frame15),
            ChampionshipOrdinaryRgb16Identity::ChequeredFrame(ChequeredFrame::Frame15),
            ChequeredFrame::Frame15.descriptor().path,
        ),
    ] {
        let failed_fixture = Fixture::new(fault);
        let (championship, frames) = failed_fixture.load();
        let error = prepare_software_championship_ordinary_rgb16_pool(&championship, &frames, 5)
            .unwrap_err();

        assert_eq!(error.identity, expected_identity);
        assert_eq!(error.path, expected_path);
        assert_eq!(
            error.source,
            OrdinaryBitmapRgb16Error::UnsupportedTopDownSource
        );
        assert!(Path::new(error.path).is_relative());
        assert!(!error.path.contains(baseline_fixture.root_string().as_str()));
        assert!(!error.path.contains(failed_fixture.root_string().as_str()));
        assert_eq!(fingerprint(&published), before);
    }
}

fn fingerprint(
    pool: &SoftwareChampionshipOrdinaryRgb16Pool,
) -> Vec<(ChampionshipOrdinaryRgb16Identity, u16, u16)> {
    pool.iter()
        .map(|(identity, source)| {
            (
                identity,
                source.packed_color_key(),
                source.logical_words()[0],
            )
        })
        .collect()
}

#[derive(Clone, Copy)]
enum Fault {
    None,
    Logo(ManufacturerLogo),
    Frame(ChequeredFrame),
}

struct Fixture {
    root: TestDirectory,
    store: AssetStore,
}

impl Fixture {
    fn new(fault: Fault) -> Self {
        let root = TestDirectory::new();
        let source = root.0.join("source");
        let files = complete_files(fault);
        let mut profile_files = Vec::with_capacity(files.len());
        for (path, bytes) in files {
            let destination = source.join(path);
            fs::create_dir_all(destination.parent().unwrap()).unwrap();
            fs::write(&destination, &bytes).unwrap();
            profile_files.push(AssetFile {
                path: path.to_owned(),
                size: bytes.len() as u64,
                sha256: hex_sha256(&bytes),
                encoding: StorageEncoding::Raw,
            });
        }
        profile_files.sort_by(|left, right| left.path.cmp(&right.path));
        let profile = AssetProfile {
            schema_version: 1,
            id: "synthetic-championship-ordinary-rgb16".to_owned(),
            roots: vec!["New_Fe".to_owned()],
            empty_directories: Vec::new(),
            sentinels: Vec::new(),
            files: profile_files,
        };
        let reviewed = ReviewedProfile::from_json(&serde_json::to_vec(&profile).unwrap()).unwrap();
        let imported = root.0.join("runtime-data");
        import_source(&source, &imported, &reviewed).unwrap();
        let store = AssetStore::open(
            &imported,
            &reviewed,
            DecodeLimits {
                max_stored_bytes: 2 * 1024 * 1024,
                max_payload_bytes: 2 * 1024 * 1024,
            },
        )
        .unwrap();
        Self { root, store }
    }

    fn load(
        &self,
    ) -> (
        cmr_core::championship_assets::ChampionshipAssets,
        cmr_core::championship_frame_assets::ChampionshipFrameAssets,
    ) {
        (
            load_championship_assets(&self.store, C3dLimits::default(), BitmapLimits::default())
                .unwrap(),
            load_championship_frame_assets(&self.store, BitmapLimits::default()).unwrap(),
        )
    }

    fn root_string(&self) -> String {
        self.root.0.to_string_lossy().into_owned()
    }
}

fn complete_files(fault: Fault) -> BTreeMap<&'static str, Vec<u8>> {
    let mut files = BTreeMap::new();
    for (index, descriptor) in CHAMPIONSHIP_CARS.into_iter().enumerate() {
        files.insert(descriptor.object_path, synthetic_c3d(index as u8));
        files.insert(descriptor.palette_path, synthetic_palette([0, 0, 0]));
    }
    for (index, descriptor) in MANUFACTURER_LOGOS.into_iter().enumerate() {
        files.insert(
            descriptor.path,
            synthetic_bitmap(
                320,
                256,
                20 + index as u8,
                matches!(fault, Fault::Logo(logo) if logo == descriptor.identity),
            ),
        );
    }
    files.insert(
        INFO_BOX_DESCRIPTOR.path,
        synthetic_bitmap(320, 132, 5, false),
    );
    files.insert(
        CHEQUERED_PALETTE_DESCRIPTOR.path,
        synthetic_palette([240, 0, 240]),
    );
    for (index, descriptor) in CHEQUERED_FRAME_DESCRIPTORS.into_iter().enumerate() {
        files.insert(
            descriptor.path,
            synthetic_bitmap(
                182,
                128,
                80 + index as u8,
                matches!(fault, Fault::Frame(frame) if frame == descriptor.frame),
            ),
        );
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

fn synthetic_bitmap(width: u32, height: u32, palette_seed: u8, top_down: bool) -> Vec<u8> {
    const PALETTE_ENTRIES: usize = 256;
    const PIXEL_OFFSET: usize = 14 + 40 + PALETTE_ENTRIES * 4;
    let stride = width.div_ceil(4) * 4;
    let image_size = stride * height;
    let mut input = vec![0; PIXEL_OFFSET + image_size as usize];
    let file_size = input.len() as u32;
    input[0..2].copy_from_slice(b"BM");
    input[2..6].copy_from_slice(&file_size.to_le_bytes());
    input[10..14].copy_from_slice(&(PIXEL_OFFSET as u32).to_le_bytes());
    input[14..18].copy_from_slice(&40u32.to_le_bytes());
    input[18..22].copy_from_slice(&(width as i32).to_le_bytes());
    let signed_height = if top_down {
        -(height as i32)
    } else {
        height as i32
    };
    input[22..26].copy_from_slice(&signed_height.to_le_bytes());
    input[26..28].copy_from_slice(&1u16.to_le_bytes());
    input[28..30].copy_from_slice(&8u16.to_le_bytes());
    input[34..38].copy_from_slice(&image_size.to_le_bytes());
    input[46..50].copy_from_slice(&(PALETTE_ENTRIES as u32).to_le_bytes());
    set_bitmap_palette_entry(
        &mut input,
        0,
        [
            palette_seed.wrapping_add(64),
            palette_seed.wrapping_add(32),
            palette_seed.wrapping_add(8),
        ],
    );
    set_bitmap_palette_entry(&mut input, SOURCE_INDEX, embedded_source_rgb(palette_seed));
    for row in input[PIXEL_OFFSET..].chunks_exact_mut(stride as usize) {
        row[..width as usize].fill(SOURCE_INDEX);
        row[width as usize..].fill(0);
    }
    input
}

fn set_bitmap_palette_entry(bytes: &mut [u8], index: u8, rgb: [u8; 3]) {
    let offset = 14 + 40 + usize::from(index) * 4;
    bytes[offset..offset + 4].copy_from_slice(&[rgb[2], rgb[1], rgb[0], 0]);
}

const fn embedded_source_rgb(seed: u8) -> [u8; 3] {
    [seed, seed.wrapping_add(16), seed.wrapping_add(32)]
}

fn synthetic_palette(source_rgb: [u8; 3]) -> Vec<u8> {
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
    let entry = 24 + usize::from(SOURCE_INDEX) * 4;
    input[entry..entry + 4].copy_from_slice(&[source_rgb[0], source_rgb[1], source_rgb[2], 0]);
    input
}

const fn pack555([red, green, blue]: [u8; 3]) -> u16 {
    ((red as u16 >> 3) << 10) | ((green as u16 >> 3) << 5) | (blue as u16 >> 3)
}

const fn pack565([red, green, blue]: [u8; 3]) -> u16 {
    ((red as u16 >> 3) << 11) | ((green as u16 >> 2) << 5) | (blue as u16 >> 3)
}

fn hex_sha256(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut encoded = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        write!(encoded, "{byte:02x}").unwrap();
    }
    encoded
}

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "cmr-championship-ordinary-rgb16-{}-{id}",
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
