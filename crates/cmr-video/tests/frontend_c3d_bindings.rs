use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use cmr_assets::{
    AssetFile, AssetProfile, AssetStore, ReviewedProfile, StorageEncoding, import_source,
};
use cmr_core::{
    championship_assets::{CHAMPIONSHIP_CARS, MANUFACTURER_LOGOS, load_championship_assets},
    frontend_c3d_resources::{
        CHAMPIONSHIP_C3D_RESOURCE_PREFIX_LEN, prepare_frontend_c3d_resources,
    },
    root_menu_assets::VisualVariant,
    root_menu_c3d_assets::{RootMenuC3dSlot, load_root_menu_c3d},
    root_menu_c3d_resources::FrontendC3dOwner,
};
use cmr_formats::{
    bitmap::BitmapLimits,
    c3d::{C3D_FORMAT_VERSION, C3D_MAGIC, C3dLimits},
    stored_asset::DecodeLimits,
};
use cmr_video::c3d_software::prepare_software_frontend_c3d_bindings;
use sha2::{Digest, Sha256};

const SHARED_LABEL: [u8; 12] = *b"SHARED-CAR!!";
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

#[test]
fn aggregate_catalog_maps_all_cars_and_a_root_to_car_alias() {
    let fixture = Fixture::new(&complete_files());
    let championship = load_championship_assets(
        &fixture.store,
        C3dLimits::default(),
        BitmapLimits::default(),
    )
    .unwrap();
    let root = load_root_menu_c3d(&fixture.store, VisualVariant::Normal, C3dLimits::default());
    let prepared =
        prepare_frontend_c3d_resources(&championship, &root, BitmapLimits::default()).unwrap();
    let car_id = prepared.cars().next().unwrap().1.ordinal_resources()[0];
    let root_id = prepared
        .root(RootMenuC3dSlot::Championship)
        .unwrap()
        .ordinal_resources()[0];

    assert_eq!(
        prepared.resource_len(),
        CHAMPIONSHIP_C3D_RESOURCE_PREFIX_LEN
    );
    assert_eq!(root_id, car_id);
    assert_eq!(
        prepared.resource(root_id).unwrap().origin().source().owner,
        FrontendC3dOwner::ChampionshipCar(cmr_core::championship_setup::CanonicalCar::Subaru)
    );

    let catalog = prepare_software_frontend_c3d_bindings(&prepared).unwrap();
    assert_eq!(catalog.provenance(), prepared.provenance());
    assert_eq!(catalog.len(), CHAMPIONSHIP_C3D_RESOURCE_PREFIX_LEN);
    let texture = catalog.binding(root_id).unwrap();
    assert_eq!(
        texture.source().owner(),
        prepared.resource(car_id).unwrap().origin().source().owner
    );
    assert_eq!(texture.extent(), [4, 4]);
}

fn complete_files() -> BTreeMap<&'static str, Vec<u8>> {
    let mut files = BTreeMap::new();
    for descriptor in CHAMPIONSHIP_CARS {
        let count = if descriptor.car == cmr_core::championship_setup::CanonicalCar::M4r {
            3
        } else {
            4
        };
        let blobs = (0..count)
            .map(|_| (SHARED_LABEL, indexed_bmp(7)))
            .collect::<Vec<_>>();
        files.insert(descriptor.object_path, c3d_with_blobs(&blobs));
        files.insert(descriptor.palette_path, synthetic_palette(1));
    }
    for descriptor in MANUFACTURER_LOGOS {
        files.insert(descriptor.path, indexed_bmp(2));
    }
    files.insert(
        RootMenuC3dSlot::Championship.path(VisualVariant::Normal),
        c3d_with_blobs(&[(SHARED_LABEL, indexed_bmp(7))]),
    );
    files
}

fn c3d_with_blobs(blobs: &[([u8; 12], Vec<u8>)]) -> Vec<u8> {
    let total_len = 32 + blobs.iter().map(|(_, blob)| 16 + blob.len()).sum::<usize>();
    let mut bytes = vec![0; total_len];
    bytes[0..4].copy_from_slice(&C3D_MAGIC);
    bytes[4..8].copy_from_slice(&C3D_FORMAT_VERSION.to_le_bytes());
    bytes[16..18].copy_from_slice(&(blobs.len() as u16).to_le_bytes());
    for index in 0..3 {
        bytes[20 + index * 4..24 + index * 4].copy_from_slice(&32u32.to_le_bytes());
    }
    let mut offset = 32;
    for (label, blob) in blobs {
        bytes[offset..offset + 12].copy_from_slice(label);
        bytes[offset + 12..offset + 16].copy_from_slice(&(blob.len() as u32).to_le_bytes());
        bytes[offset + 16..offset + 16 + blob.len()].copy_from_slice(blob);
        offset += 16 + blob.len();
    }
    bytes
}

fn indexed_bmp(pixel: u8) -> Vec<u8> {
    const PIXEL_OFFSET: usize = 14 + 40 + 256 * 4;
    const PIXEL_LEN: usize = 4 * 4;
    let mut bytes = vec![0; PIXEL_OFFSET + PIXEL_LEN];
    let file_len = bytes.len() as u32;
    bytes[0..2].copy_from_slice(b"BM");
    bytes[2..6].copy_from_slice(&file_len.to_le_bytes());
    bytes[10..14].copy_from_slice(&(PIXEL_OFFSET as u32).to_le_bytes());
    bytes[14..18].copy_from_slice(&40u32.to_le_bytes());
    bytes[18..22].copy_from_slice(&4i32.to_le_bytes());
    bytes[22..26].copy_from_slice(&4i32.to_le_bytes());
    bytes[26..28].copy_from_slice(&1u16.to_le_bytes());
    bytes[28..30].copy_from_slice(&8u16.to_le_bytes());
    bytes[34..38].copy_from_slice(&(PIXEL_LEN as u32).to_le_bytes());
    bytes[46..50].copy_from_slice(&256u32.to_le_bytes());
    bytes[PIXEL_OFFSET..].fill(pixel);
    bytes
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
            profile_files.push(AssetFile {
                path: (*path).to_owned(),
                size: bytes.len() as u64,
                sha256: hex_sha256(bytes),
                encoding: StorageEncoding::Raw,
            });
        }
        profile_files.sort_by(|left, right| left.path.cmp(&right.path));
        let profile = AssetProfile {
            schema_version: 1,
            id: "synthetic-frontend-c3d-bindings".to_owned(),
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
                max_stored_bytes: 2 * 1024 * 1024,
                max_payload_bytes: 2 * 1024 * 1024,
            },
        )
        .unwrap();
        Self {
            _temporary: temporary,
            store,
        }
    }
}

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "cmr-video-frontend-c3d-bindings-{}-{id}",
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

fn hex_sha256(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(encoded, "{byte:02x}").unwrap();
    }
    encoded
}
