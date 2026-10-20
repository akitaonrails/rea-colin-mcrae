use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use cmr_assets::{
    AssetFile, AssetProfile, AssetStore, ReviewedProfile, StorageEncoding, import_source,
};
use cmr_core::championship_assets::{
    CHAMPIONSHIP_CARS, MANUFACTURER_LOGOS, load_championship_assets,
};
use cmr_core::championship_car_execute::ChampionshipCarBatchExecutor;
use cmr_core::championship_car_frame_plan::plan_championship_car_frame;
use cmr_core::championship_car_scene::execute_championship_car_scene;
use cmr_core::championship_car_transform::{ChampionshipCarInput, ChampionshipCarState};
use cmr_core::championship_setup::CanonicalCar;
use cmr_core::championship_setup_visual::PixelPoint;
use cmr_core::display::{
    CooperativeMode, DisplayExtent, IndexedMode, IndexedVideoBackend, SurfaceTopology,
};
use cmr_core::frontend_c3d_resources::prepare_frontend_c3d_resources;
use cmr_core::frontend_c3d_scene::C3dDepthClear;
use cmr_core::root_menu_assets::VisualVariant;
use cmr_core::root_menu_c3d_assets::load_root_menu_c3d;
use cmr_core::root_menu_c3d_resources::C3dPreparedSelector;
use cmr_formats::bitmap::BitmapLimits;
use cmr_formats::c3d::{
    C3D_ENTRY24_ENTRY32_ORDINAL_OFFSETS, C3D_FORMAT_VERSION, C3D_GROUP0_VALUES_OFFSET,
    C3D_GROUP1_ENTRY24_LEN, C3D_GROUP1_ENTRY32_LEN, C3D_GROUP1_FIXED_LEN, C3D_HEADER_LEN,
    C3D_MAGIC, C3dLimits,
};
use cmr_formats::riff_palette::PaletteEntry;
use cmr_formats::stored_asset::DecodeLimits;
use cmr_video::c3d_software::prepare_software_frontend_c3d_bindings;
use cmr_video::c3d_software_raster::{
    SoftwareC3dRasterError, SoftwareChampionshipCarRasterProfile,
};
use cmr_video::{SoftwareC3dBackendError, SoftwareIndexedBackend};
use sha2::{Digest, Sha256};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

const EXTENT: DisplayExtent = DisplayExtent {
    width: 64,
    height: 48,
};
const Q16_ONE: i32 = 1 << 16;
const IDENTITY_BASIS: [i32; 9] = [Q16_ONE, 0, 0, 0, Q16_ONE, 0, 0, 0, Q16_ONE];
const RESOURCE_INDEX: u8 = 217;

#[test]
fn prepared_type2_car_executes_resource_missing_resource_on_software_backend() {
    let temporary = TestDirectory::new();
    let store = synthetic_store(&temporary);
    let championship =
        load_championship_assets(&store, C3dLimits::default(), BitmapLimits::default()).unwrap();
    let root = load_root_menu_c3d(&store, VisualVariant::Normal, C3dLimits::default());
    let frontend =
        prepare_frontend_c3d_resources(&championship, &root, BitmapLimits::default()).unwrap();
    let topology = frontend
        .car_topology(&championship, CanonicalCar::Subaru)
        .unwrap();
    let runtime = frontend
        .materialize_car(CanonicalCar::Subaru, &topology)
        .unwrap();
    let catalog = prepare_software_frontend_c3d_bindings(&frontend).unwrap();
    let mut executor = ChampionshipCarBatchExecutor::new(&catalog);
    let mut car_state = ChampionshipCarState::new();
    let input = ChampionshipCarInput {
        f9: false,
        f10: false,
    };
    let logo_origin = PixelPoint::new(8, 8);

    let hidden = plan_championship_car_frame(
        &topology,
        &runtime,
        &mut car_state,
        CanonicalCar::Subaru,
        input,
        IDENTITY_BASIS,
        logo_origin,
        EXTENT,
    )
    .unwrap();
    assert!(hidden.draws().is_empty());
    let visible = plan_championship_car_frame(
        &topology,
        &runtime,
        &mut car_state,
        CanonicalCar::Subaru,
        input,
        IDENTITY_BASIS,
        logo_origin,
        EXTENT,
    )
    .unwrap();
    assert_eq!(
        visible
            .draws()
            .iter()
            .map(|draw| draw.command().selector() == C3dPreparedSelector::Missing)
            .collect::<Vec<_>>(),
        [false, true, false]
    );
    let prepared = executor.prepare(visible).unwrap();

    let (mut backend, surfaces) = initialized_backend();
    let profile = SoftwareChampionshipCarRasterProfile::
        MUTATION_DISABLED_ACTOR_RGB_DISABLED_UNIFIED_INDEXED_COPY_NO_BLEND;
    backend
        .configure_championship_car_raster_profile(profile)
        .unwrap();
    let outcome = execute_championship_car_scene(
        &mut executor,
        &mut backend,
        surfaces.back,
        C3dDepthClear::Clear,
        prepared,
    )
    .unwrap();

    let submission = outcome.submission.unwrap();
    assert_eq!(submission.actor_worlds, 1);
    assert_eq!(submission.bindings, 3);
    assert_eq!(submission.draws, 3);
    let pixels = backend.back_pixels().unwrap();
    let depth = backend.c3d_depth_values().unwrap();
    assert_eq!(pixels[337], RESOURCE_INDEX);
    assert_eq!(depth[337].to_bits(), 0x3f43_d264);
    assert_eq!(pixels[0], 0);
    assert_eq!(depth[0], 1.0);
}

#[test]
fn unsupported_and_unconfigured_profiles_reject_before_a_requested_depth_clear() {
    let (mut unconfigured, _) = initialized_backend();
    let before_pixels = unconfigured.back_pixels().unwrap().to_vec();
    let before_depth = unconfigured.c3d_depth_values().unwrap().to_vec();
    let unsupported = SoftwareChampionshipCarRasterProfile {
        fog_mutation: true,
        ..SoftwareChampionshipCarRasterProfile::
            MUTATION_DISABLED_ACTOR_RGB_DISABLED_UNIFIED_INDEXED_COPY_NO_BLEND
    };

    assert!(matches!(
        unconfigured.configure_championship_car_raster_profile(unsupported),
        Err(SoftwareC3dBackendError::Raster(
            SoftwareC3dRasterError::UnsupportedChampionshipCarProfile(profile)
        )) if profile == unsupported
    ));

    assert_eq!(
        unconfigured.validate_championship_car_raster_profile(),
        Err(SoftwareC3dBackendError::ChampionshipCarProfileNotConfigured)
    );
    assert_eq!(unconfigured.back_pixels().unwrap(), before_pixels);
    assert_eq!(unconfigured.c3d_depth_values().unwrap(), before_depth);
}

fn initialized_backend() -> (
    SoftwareIndexedBackend,
    cmr_core::display::DisplaySurfaces<cmr_video::SurfaceHandle>,
) {
    let mut backend = SoftwareIndexedBackend::default();
    backend.create_device().unwrap();
    backend
        .set_cooperative_mode(CooperativeMode::Windowed)
        .unwrap();
    let surfaces = backend
        .create_surfaces(
            SurfaceTopology::PrimaryAndOffscreen,
            IndexedMode {
                extent: EXTENT,
                bits_per_pixel: 8,
            },
        )
        .unwrap();
    let entries = std::array::from_fn(|index| PaletteEntry {
        red: index as u8,
        green: index as u8,
        blue: index as u8,
        flags: 0,
    });
    let palette = backend.create_palette(&entries).unwrap();
    backend
        .attach_palette(surfaces.visible_front, palette)
        .unwrap();
    backend.attach_palette(surfaces.back, palette).unwrap();
    backend.upload_palette(palette, &entries).unwrap();
    (backend, surfaces)
}

fn synthetic_store(temporary: &TestDirectory) -> AssetStore {
    let source = temporary.0.join("source");
    let mut files = BTreeMap::new();
    for descriptor in CHAMPIONSHIP_CARS {
        let resource_count = if descriptor.car == CanonicalCar::M4r {
            3
        } else {
            4
        };
        files.insert(
            descriptor.object_path,
            type2_car_c3d(&[0, -1, 0], resource_count),
        );
        files.insert(descriptor.palette_path, synthetic_palette());
    }
    for descriptor in MANUFACTURER_LOGOS {
        files.insert(descriptor.path, indexed_bitmap_4x4(3));
    }

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
        id: "synthetic-championship-car-software".to_owned(),
        roots: vec!["New_Fe".to_owned()],
        empty_directories: Vec::new(),
        sentinels: Vec::new(),
        files: profile_files,
    };
    let reviewed = ReviewedProfile::from_json(&serde_json::to_vec(&profile).unwrap()).unwrap();
    let imported = temporary.0.join("runtime-data");
    import_source(&source, &imported, &reviewed).unwrap();
    AssetStore::open(
        &imported,
        &reviewed,
        DecodeLimits {
            max_stored_bytes: 2 * 1024 * 1024,
            max_payload_bytes: 2 * 1024 * 1024,
        },
    )
    .unwrap()
}

fn type2_car_c3d(selectors: &[i16], resource_count: usize) -> Vec<u8> {
    let group0_len = C3D_GROUP0_VALUES_OFFSET + 2;
    let vertex_count = selectors.len() * 3;
    let group1_len = C3D_GROUP1_FIXED_LEN
        + vertex_count * C3D_GROUP1_ENTRY32_LEN
        + selectors.len() * C3D_GROUP1_ENTRY24_LEN;
    let group1_start = C3D_HEADER_LEN + group0_len;
    let blob_start = group1_start + group1_len;
    let bitmap = indexed_bitmap_4x4(RESOURCE_INDEX);
    let mut bytes = vec![0; blob_start + resource_count * (16 + bitmap.len())];
    bytes[..4].copy_from_slice(&C3D_MAGIC);
    bytes[4..8].copy_from_slice(&C3D_FORMAT_VERSION.to_le_bytes());
    bytes[12..14].copy_from_slice(&1u16.to_le_bytes());
    bytes[14..16].copy_from_slice(&1u16.to_le_bytes());
    bytes[16..18].copy_from_slice(&(resource_count as u16).to_le_bytes());
    for (index, offset) in [C3D_HEADER_LEN, group1_start, blob_start]
        .into_iter()
        .enumerate()
    {
        bytes[20 + index * 4..24 + index * 4].copy_from_slice(&(offset as u32).to_le_bytes());
    }

    let group0 = C3D_HEADER_LEN;
    bytes[group0..group0 + 4].copy_from_slice(&(group0_len as u32).to_le_bytes());
    bytes[group0 + 4..group0 + 6].copy_from_slice(&(-1i16).to_le_bytes());
    for index in [0, 5, 10, 15] {
        let field = group0 + 8 + index * 4;
        bytes[field..field + 4].copy_from_slice(&Q16_ONE.to_le_bytes());
    }
    bytes[group0 + 76] = 1;
    bytes[group0 + C3D_GROUP0_VALUES_OFFSET..group0 + C3D_GROUP0_VALUES_OFFSET + 2]
        .copy_from_slice(&0u16.to_le_bytes());

    let group1 = group1_start;
    bytes[group1 + 12..group1 + 16].copy_from_slice(&(group1_len as u32).to_le_bytes());
    bytes[group1 + 16..group1 + 18].copy_from_slice(&(vertex_count as u16).to_le_bytes());
    bytes[group1 + 18..group1 + 20].copy_from_slice(&(selectors.len() as u16).to_le_bytes());
    let triangle: [[i32; 8]; 3] = [
        [-Q16_ONE, -Q16_ONE, 0, 0, -Q16_ONE, 0, 0, 0],
        [-Q16_ONE, Q16_ONE, 0, 0, 0, 0, 0, 0],
        [Q16_ONE, -Q16_ONE, 0, Q16_ONE, -Q16_ONE, 0, 0, 0],
    ];
    for selector_index in 0..selectors.len() {
        for (corner, vertex) in triangle.iter().enumerate() {
            let vertex_index = selector_index * 3 + corner;
            let start = group1 + C3D_GROUP1_FIXED_LEN + vertex_index * C3D_GROUP1_ENTRY32_LEN;
            for (component_index, component) in vertex.iter().enumerate() {
                let field = start + component_index * 4;
                bytes[field..field + 4].copy_from_slice(&component.to_le_bytes());
            }
        }
    }
    let entries = group1 + C3D_GROUP1_FIXED_LEN + vertex_count * C3D_GROUP1_ENTRY32_LEN;
    for (entry_index, selector) in selectors.iter().enumerate() {
        let entry = entries + entry_index * C3D_GROUP1_ENTRY24_LEN;
        for (corner, ordinal_offset) in C3D_ENTRY24_ENTRY32_ORDINAL_OFFSETS.into_iter().enumerate()
        {
            let ordinal = (entry_index * 3 + corner) as u16;
            bytes[entry + ordinal_offset..entry + ordinal_offset + 2]
                .copy_from_slice(&ordinal.to_le_bytes());
        }
        bytes[entry + 8..entry + 10].copy_from_slice(&selector.to_le_bytes());
    }

    let mut offset = blob_start;
    for ordinal in 0..resource_count {
        let mut label = *b"CAR-TEX-000!";
        label[8] = b'0' + ordinal as u8;
        bytes[offset..offset + 12].copy_from_slice(&label);
        bytes[offset + 12..offset + 16].copy_from_slice(&(bitmap.len() as u32).to_le_bytes());
        bytes[offset + 16..offset + 16 + bitmap.len()].copy_from_slice(&bitmap);
        offset += 16 + bitmap.len();
    }
    bytes
}

fn indexed_bitmap_4x4(pixel: u8) -> Vec<u8> {
    const WIDTH: usize = 4;
    const HEIGHT: usize = 4;
    const PIXEL_OFFSET: usize = 14 + 40 + 256 * 4;
    let mut bytes = vec![0; PIXEL_OFFSET + WIDTH * HEIGHT];
    let file_size = bytes.len() as u32;
    bytes[0..2].copy_from_slice(b"BM");
    bytes[2..6].copy_from_slice(&file_size.to_le_bytes());
    bytes[10..14].copy_from_slice(&(PIXEL_OFFSET as u32).to_le_bytes());
    bytes[14..18].copy_from_slice(&40u32.to_le_bytes());
    bytes[18..22].copy_from_slice(&(WIDTH as i32).to_le_bytes());
    bytes[22..26].copy_from_slice(&(HEIGHT as i32).to_le_bytes());
    bytes[26..28].copy_from_slice(&1u16.to_le_bytes());
    bytes[28..30].copy_from_slice(&8u16.to_le_bytes());
    bytes[34..38].copy_from_slice(&((WIDTH * HEIGHT) as u32).to_le_bytes());
    bytes[46..50].copy_from_slice(&256u32.to_le_bytes());
    bytes[PIXEL_OFFSET..].fill(pixel);
    bytes
}

fn synthetic_palette() -> Vec<u8> {
    let data_size = 4 + 256 * 4;
    let file_size = 12 + 8 + data_size;
    let mut bytes = vec![0; file_size];
    bytes[0..4].copy_from_slice(b"RIFF");
    bytes[4..8].copy_from_slice(&((file_size - 8) as u32).to_le_bytes());
    bytes[8..12].copy_from_slice(b"PAL ");
    bytes[12..16].copy_from_slice(b"data");
    bytes[16..20].copy_from_slice(&(data_size as u32).to_le_bytes());
    bytes[20..22].copy_from_slice(&0x0300u16.to_le_bytes());
    bytes[22..24].copy_from_slice(&256u16.to_le_bytes());
    bytes
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
            "cmr-video-championship-car-{}-{id}",
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
