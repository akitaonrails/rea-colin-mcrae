use std::convert::Infallible;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use cmr_assets::{
    AssetFile, AssetProfile, AssetStore, ReviewedProfile, StorageEncoding, import_source,
};
use cmr_core::root_menu_assets::VisualVariant;
use cmr_core::root_menu_c3d_assets::{RootMenuC3dSlot, load_root_menu_c3d};
use cmr_core::root_menu_c3d_execute::bind_root_menu_c3d_resources;
use cmr_core::root_menu_c3d_runtime::{
    C3dMode2CornerSource, C3dMode2LoadState, C3dSurfaceLayoutGates, RootMenuC3dMode2LoadStates,
};
use cmr_core::root_menu_c3d_topology::prepare_root_menu_c3d_topology;
use cmr_formats::bitmap::{BitmapLimits, BitmapPaletteEntry};
use cmr_formats::c3d::{C3D_FORMAT_VERSION, C3D_MAGIC, C3dLimits};
use cmr_formats::stored_asset::DecodeLimits;
use cmr_video::c3d_software::{SoftwareC3dRetainedSource, SoftwareC3dSampledTexture};
use sha2::{Digest, Sha256};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("cmr-video-c3d-source-{}-{id}", std::process::id()));
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
fn real_preparation_binding_owns_normalized_source_after_inputs_drop() {
    let (catalog, resource_id) = {
        let temporary = TestDirectory::new();
        let source = temporary.0.join("source");
        let path = RootMenuC3dSlot::Championship.path(VisualVariant::Normal);
        let bytes = one_resource_c3d(&indexed_bitmap_4x4());
        let source_file = source.join(path);
        fs::create_dir_all(source_file.parent().unwrap()).unwrap();
        fs::write(&source_file, &bytes).unwrap();

        let profile = AssetProfile {
            schema_version: 1,
            id: "synthetic-video-c3d-source".to_owned(),
            roots: vec!["New_Fe".to_owned()],
            empty_directories: Vec::new(),
            sentinels: Vec::new(),
            files: vec![AssetFile {
                path: path.to_owned(),
                size: bytes.len() as u64,
                sha256: hex_sha256(&bytes),
                encoding: StorageEncoding::Raw,
            }],
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
        let assets = load_root_menu_c3d(&store, VisualVariant::Normal, C3dLimits::default());
        assert_eq!(assets.loaded_len(), 1);
        let topology = prepare_root_menu_c3d_topology(&assets, BitmapLimits::default()).unwrap();
        assert_eq!(topology.resource_len(), 1);
        let mut resource_id = None;
        let catalog = bind_root_menu_c3d_resources(&topology, |id, resource| {
            assert!(resource_id.replace(id).is_none());
            Ok::<_, Infallible>(SoftwareC3dRetainedSource::from_prepared(resource))
        })
        .unwrap();
        (catalog, resource_id.unwrap())
    };

    assert_eq!(catalog.len(), 1);
    let retained = catalog.binding(resource_id).unwrap();
    assert_eq!(retained.first_slot(), RootMenuC3dSlot::Championship);
    assert_eq!(retained.extent(), [4, 4]);
    assert_eq!(retained.source_dib_extent_low16(), [4, 4]);
    assert_eq!(retained.indices(), (0..16).collect::<Vec<_>>());
    assert_eq!(retained.palette().len(), 256);
    assert_eq!(
        retained.palette()[0],
        BitmapPaletteEntry {
            red: 2,
            green: 1,
            blue: 0,
            reserved: 0,
        }
    );
    assert_eq!(
        retained.palette()[255],
        BitmapPaletteEntry {
            red: 1,
            green: 0,
            blue: 255,
            reserved: 0,
        }
    );
    assert_eq!(retained.color_key_index(), None);
    assert_eq!(retained.indices()[0], 0);

    let sampled = SoftwareC3dSampledTexture::from_retained_root(
        retained.clone(),
        RootMenuC3dMode2LoadStates::splat(C3dMode2LoadState {
            prepared_resource_count: 0,
            corner_source: C3dMode2CornerSource::OrdinaryDefaults,
            surface_layout: C3dSurfaceLayoutGates::default(),
        }),
    )
    .unwrap();
    assert_eq!(sampled.extent(), [4, 4]);
    assert_eq!(sampled.indices(), (0..16).collect::<Vec<_>>());
    assert_eq!(sampled.palette(), retained.palette());
}

fn one_resource_c3d(bitmap: &[u8]) -> Vec<u8> {
    let mut input = vec![0; 32];
    input[0..4].copy_from_slice(&C3D_MAGIC);
    input[4..8].copy_from_slice(&C3D_FORMAT_VERSION.to_le_bytes());
    input[16..18].copy_from_slice(&1u16.to_le_bytes());
    for offset in [20, 24, 28] {
        input[offset..offset + 4].copy_from_slice(&32u32.to_le_bytes());
    }
    input.extend_from_slice(b"SOURCE\0\0\0\0\0\0");
    input.extend_from_slice(&(bitmap.len() as u32).to_le_bytes());
    input.extend_from_slice(bitmap);
    input
}

fn indexed_bitmap_4x4() -> Vec<u8> {
    const WIDTH: usize = 4;
    const HEIGHT: usize = 4;
    const PALETTE_BYTES: usize = 256 * 4;
    const PIXEL_OFFSET: usize = 14 + 40 + PALETTE_BYTES;
    const STORED_BYTES: usize = WIDTH * HEIGHT;
    let mut input = vec![0; PIXEL_OFFSET + STORED_BYTES];
    let file_size = input.len() as u32;
    input[0..2].copy_from_slice(b"BM");
    input[2..6].copy_from_slice(&file_size.to_le_bytes());
    input[10..14].copy_from_slice(&(PIXEL_OFFSET as u32).to_le_bytes());
    input[14..18].copy_from_slice(&40u32.to_le_bytes());
    input[18..22].copy_from_slice(&(WIDTH as i32).to_le_bytes());
    input[22..26].copy_from_slice(&(HEIGHT as i32).to_le_bytes());
    input[26..28].copy_from_slice(&1u16.to_le_bytes());
    input[28..30].copy_from_slice(&8u16.to_le_bytes());
    input[34..38].copy_from_slice(&(STORED_BYTES as u32).to_le_bytes());
    input[46..50].copy_from_slice(&256u32.to_le_bytes());
    for index in 0..256 {
        let start = 54 + index * 4;
        input[start] = index as u8;
        input[start + 1] = (index as u8).wrapping_add(1);
        input[start + 2] = (index as u8).wrapping_add(2);
        input[start + 3] = 0xa5;
    }
    input[PIXEL_OFFSET..PIXEL_OFFSET + 4].copy_from_slice(&[12, 13, 14, 15]);
    input[PIXEL_OFFSET + 4..PIXEL_OFFSET + 8].copy_from_slice(&[8, 9, 10, 11]);
    input[PIXEL_OFFSET + 8..PIXEL_OFFSET + 12].copy_from_slice(&[4, 5, 6, 7]);
    input[PIXEL_OFFSET + 12..PIXEL_OFFSET + 16].copy_from_slice(&[0, 1, 2, 3]);
    input
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
