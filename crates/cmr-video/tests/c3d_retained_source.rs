use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use cmr_assets::{
    AssetFile, AssetProfile, AssetStore, ReviewedProfile, StorageEncoding, import_source,
};
use cmr_core::display::{
    CooperativeMode, DisplayExtent, IndexedMode, IndexedVideoBackend, SurfaceTopology,
};
use cmr_core::frontend_c3d_route::FrontendC3dRouteState;
use cmr_core::root_menu_assets::VisualVariant;
use cmr_core::root_menu_c3d_assets::{RootMenuC3dAngles, RootMenuC3dSlot, load_root_menu_c3d};
use cmr_core::root_menu_c3d_execute::RootMenuC3dBatchExecutor;
use cmr_core::root_menu_c3d_runtime::{
    C3dMode2CornerSource, C3dMode2LoadState, C3dSurfaceLayoutGates, RootMenuC3dMode2LoadStates,
    materialize_root_menu_mode2_resource_scaled,
};
use cmr_core::root_menu_c3d_scene::{
    PreparedRootMenuC3dPipeline, RootMenuC3dDepthClear, RootMenuC3dFramePipeline,
};
use cmr_core::root_menu_c3d_topology::prepare_root_menu_c3d_topology;
use cmr_core::root_menu_c3d_visibility::RootMenuC3dVisibilityLatches;
use cmr_formats::bitmap::{BitmapLimits, BitmapPaletteEntry};
use cmr_formats::c3d::{
    C3D_ENTRY24_ENTRY32_ORDINAL_OFFSETS, C3D_FORMAT_VERSION, C3D_GROUP0_VALUES_OFFSET,
    C3D_GROUP1_ENTRY24_LEN, C3D_GROUP1_ENTRY32_LEN, C3D_GROUP1_FIXED_LEN, C3D_HEADER_LEN,
    C3D_MAGIC, C3dLimits,
};
use cmr_formats::riff_palette::PaletteEntry;
use cmr_formats::stored_asset::DecodeLimits;
use cmr_video::SoftwareIndexedBackend;
use cmr_video::c3d_software::prepare_software_root_c3d_bindings;
use cmr_video::c3d_software_raster::{
    SoftwareC3dRasterError, SoftwareC3dTransformState, preflight_root_c3d_batch,
    preflight_root_c3d_copy_batch,
};
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
        let resource_id = topology.resources().next().unwrap().0;
        let catalog = prepare_software_root_c3d_bindings(
            &topology,
            RootMenuC3dMode2LoadStates::splat(C3dMode2LoadState {
                prepared_resource_count: 0,
                corner_source: C3dMode2CornerSource::OrdinaryDefaults,
                surface_layout: C3dSurfaceLayoutGates::default(),
            }),
        )
        .unwrap();
        (catalog, resource_id)
    };

    assert_eq!(catalog.len(), 1);
    let sampled = catalog.binding(resource_id).unwrap();
    let retained = sampled.source();
    assert_eq!(retained.first_slot(), Some(RootMenuC3dSlot::Championship));
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

    assert_eq!(sampled.extent(), [4, 4]);
    assert_eq!(sampled.indices(), (0..16).collect::<Vec<_>>());
    assert_eq!(sampled.palette(), retained.palette());
}

#[test]
fn imported_root_type1_copy_ignores_normals_and_reuses_retained_binding() {
    let temporary = TestDirectory::new();
    let source = temporary.0.join("source");
    let slot = RootMenuC3dSlot::Championship;
    let path = slot.path(VisualVariant::Normal);
    let alternate_slot = RootMenuC3dSlot::Rally;
    let alternate_path = alternate_slot.path(VisualVariant::Normal);
    let bitmap = indexed_bitmap_4x4();
    let bytes = drawable_root_c3d(&bitmap, [0, -(1 << 16), 0]);
    let alternate_bytes = drawable_root_c3d(&bitmap, [3 << 16, 4 << 16, -(5 << 16)]);
    let source_file = source.join(path);
    fs::create_dir_all(source_file.parent().unwrap()).unwrap();
    fs::write(&source_file, &bytes).unwrap();
    fs::write(source.join(alternate_path), &alternate_bytes).unwrap();

    let profile = AssetProfile {
        schema_version: 1,
        id: "synthetic-video-c3d-scene".to_owned(),
        roots: vec!["New_Fe".to_owned()],
        empty_directories: Vec::new(),
        sentinels: Vec::new(),
        files: vec![
            AssetFile {
                path: path.to_owned(),
                size: bytes.len() as u64,
                sha256: hex_sha256(&bytes),
                encoding: StorageEncoding::Raw,
            },
            AssetFile {
                path: alternate_path.to_owned(),
                size: alternate_bytes.len() as u64,
                sha256: hex_sha256(&alternate_bytes),
                encoding: StorageEncoding::Raw,
            },
        ],
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
    let topology = prepare_root_menu_c3d_topology(&assets, BitmapLimits::default()).unwrap();
    let load_states = RootMenuC3dMode2LoadStates::splat(C3dMode2LoadState {
        prepared_resource_count: 0,
        corner_source: C3dMode2CornerSource::OrdinaryDefaults,
        surface_layout: C3dSurfaceLayoutGates::default(),
    });
    let runtime = materialize_root_menu_mode2_resource_scaled(&topology, load_states).unwrap();
    let catalog = prepare_software_root_c3d_bindings(&topology, load_states).unwrap();
    let mut executor = RootMenuC3dBatchExecutor::new(&catalog);
    let mut angles = RootMenuC3dAngles::new();
    let mut visibility = RootMenuC3dVisibilityLatches::new();
    let route_state = FrontendC3dRouteState::new();
    let mut pipeline = PreparedRootMenuC3dPipeline::new(
        &topology,
        &runtime,
        &mut angles,
        &mut visibility,
        &mut executor,
        &route_state,
        RootMenuC3dDepthClear::Clear,
    )
    .unwrap();

    let extent = DisplayExtent {
        width: 64,
        height: 48,
    };
    let mut active_palette = [PaletteEntry {
        red: 255,
        green: 255,
        blue: 255,
        flags: 0,
    }; 256];
    for (index, entry) in active_palette.iter_mut().take(16).enumerate() {
        *entry = PaletteEntry {
            red: index as u8 + 2,
            green: index as u8 + 1,
            blue: index as u8,
            flags: 0xa5,
        };
    }
    let mut backend = SoftwareIndexedBackend::default();
    backend.create_device().unwrap();
    backend
        .set_cooperative_mode(CooperativeMode::Windowed)
        .unwrap();
    let surfaces = backend
        .create_surfaces(
            SurfaceTopology::PrimaryAndOffscreen,
            IndexedMode {
                extent,
                bits_per_pixel: 8,
            },
        )
        .unwrap();
    let palette = backend.create_palette(&active_palette).unwrap();
    backend
        .attach_palette(surfaces.visible_front, palette)
        .unwrap();
    backend.attach_palette(surfaces.back, palette).unwrap();
    backend.upload_palette(palette, &active_palette).unwrap();

    let first = pipeline.plan_frame(slot, 0, extent).unwrap();
    let first_plan = first.as_ref().unwrap();
    assert_eq!(first_plan.commands().len(), 1);
    assert_eq!(
        preflight_root_c3d_batch(
            &first_plan.commands()[0],
            SoftwareC3dTransformState::new(
                first_plan.world(),
                first_plan.view(),
                first_plan.projection(),
                first_plan.viewport(),
            ),
        ),
        Err(SoftwareC3dRasterError::UnsupportedVertexLayout { vertex_type: 1 })
    );
    pipeline
        .execute_terminal_scene(&mut backend, surfaces.back, extent, first)
        .unwrap();

    let back = backend.back_pixels().unwrap();
    assert!(back.iter().any(|pixel| (1..=15).contains(pixel)));
    assert!(back.contains(&0));
    assert!(
        backend
            .visible_pixels()
            .unwrap()
            .iter()
            .all(|pixel| *pixel == 0)
    );
    let depth = backend.c3d_depth_values().unwrap();
    assert!(depth.iter().any(|value| *value < 1.0));
    assert!(depth.contains(&1.0));
    let first_pixels = back.to_vec();
    let first_depth = depth.to_vec();

    let second = pipeline.plan_frame(alternate_slot, 0, extent).unwrap();
    pipeline
        .execute_terminal_scene(&mut backend, surfaces.back, extent, second)
        .unwrap();
    assert_eq!(backend.back_pixels().unwrap(), first_pixels);
    assert_eq!(backend.c3d_depth_values().unwrap(), first_depth);

    let third = pipeline.plan_frame(slot, 0, extent).unwrap();
    pipeline
        .execute_terminal_scene(&mut backend, surfaces.back, extent, third)
        .unwrap();
    assert_eq!(backend.back_pixels().unwrap(), first_pixels);
}

/// End-to-end: a synthetic C3D mesh goes through the real resource-preparation
/// and frame pipeline, then the recovered gameplay-scene camera matrices
/// (`SoftwareC3dTransformState::for_gameplay_scene`) drive the software
/// transform/clip/cull instead of the fixed menu matrices. This is the E07
/// gameplay oracle exercised over the actual command path, not a hand-built
/// command.
#[test]
fn gameplay_camera_matrices_drive_the_real_scene_transform() {
    let temporary = TestDirectory::new();
    let source = temporary.0.join("source");
    let slot = RootMenuC3dSlot::Championship;
    let path = slot.path(VisualVariant::Normal);
    let bitmap = indexed_bitmap_4x4();
    // A single camera-facing triangle (normal toward -Z).
    let bytes = drawable_root_c3d(&bitmap, [0, 0, -(1 << 16)]);
    let source_file = source.join(path);
    fs::create_dir_all(source_file.parent().unwrap()).unwrap();
    fs::write(&source_file, &bytes).unwrap();

    let profile = AssetProfile {
        schema_version: 1,
        id: "synthetic-video-c3d-gameplay".to_owned(),
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
    let topology = prepare_root_menu_c3d_topology(&assets, BitmapLimits::default()).unwrap();
    let load_states = RootMenuC3dMode2LoadStates::splat(C3dMode2LoadState {
        prepared_resource_count: 0,
        corner_source: C3dMode2CornerSource::OrdinaryDefaults,
        surface_layout: C3dSurfaceLayoutGates::default(),
    });
    let runtime = materialize_root_menu_mode2_resource_scaled(&topology, load_states).unwrap();
    let catalog = prepare_software_root_c3d_bindings(&topology, load_states).unwrap();
    let mut executor = RootMenuC3dBatchExecutor::new(&catalog);
    let mut angles = RootMenuC3dAngles::new();
    let mut visibility = RootMenuC3dVisibilityLatches::new();
    let route_state = FrontendC3dRouteState::new();
    let mut pipeline = PreparedRootMenuC3dPipeline::new(
        &topology,
        &runtime,
        &mut angles,
        &mut visibility,
        &mut executor,
        &route_state,
        RootMenuC3dDepthClear::Clear,
    )
    .unwrap();

    let extent = DisplayExtent {
        width: 64,
        height: 48,
    };
    let plan = pipeline.plan_frame(slot, 0, extent).unwrap();
    let plan = plan.as_ref().unwrap();
    assert_eq!(plan.commands().len(), 1);
    let command = &plan.commands()[0];

    // Gameplay camera: identity node, camera translated back along -Z so the
    // triangle (at model z = 0) sits at positive view-space depth.
    let identity_q16 = cmr_core::c3d_camera::MATRIX_IDENTITY_Q16;
    let camera_at = |world_z: i32| {
        let mut m = identity_q16;
        m[14] = world_z;
        m
    };
    let gameplay_state = |camera_world: &[i32; 16]| {
        SoftwareC3dTransformState::for_gameplay_scene(
            extent.width,
            extent.height,
            camera_world,
            &identity_q16,
        )
        .unwrap()
    };

    let near_camera = camera_at(-(6 << 16));
    let far_camera = camera_at(-(60 << 16));

    // Integration: the gameplay matrices drive the real transform/clip/cull.
    let near = preflight_root_c3d_copy_batch(command, gameplay_state(&near_camera)).unwrap();
    let near_again = preflight_root_c3d_copy_batch(command, gameplay_state(&near_camera)).unwrap();
    let far = preflight_root_c3d_copy_batch(command, gameplay_state(&far_camera)).unwrap();

    // The one source triangle is seen by the pipeline under both cameras.
    assert_eq!(near.source_triangle_count(), 1);
    assert_eq!(far.source_triangle_count(), 1);
    // It is visible (survives clipping and the back-face cull) from the near camera.
    assert_eq!(near.triangles().len(), 1);
    assert_eq!(near.counter_clockwise_triangle_count(), 0);
    assert_eq!(near.fully_clipped_source_triangle_count(), 0);

    // Deterministic for identical input.
    assert_eq!(near, near_again);
    // The camera parameters actually drive the transform: a farther camera
    // projects the same triangle smaller, so the prepared geometry differs.
    assert_ne!(near, far);

    // Every prepared screen vertex is finite and inside the back buffer.
    for triangle in near.triangles() {
        for vertex in triangle.vertices() {
            let [x, y, z] = vertex.position();
            assert!(x.is_finite() && y.is_finite() && z.is_finite());
            assert!((0.0..=f32::from(extent.width)).contains(&x));
            assert!((0.0..=f32::from(extent.height)).contains(&y));
        }
    }

    // The near triangle's screen bounding box is larger than the far one (the
    // perspective divide shrinks distant geometry), when the far triangle is
    // also visible.
    if far.triangles().len() == 1 {
        let span = |batch: &cmr_video::c3d_software_raster::SoftwareC3dPreparedCopyBatch| {
            let verts = batch.triangles()[0].vertices();
            let xs = verts.iter().map(|v| v.position()[0]);
            let max = xs.clone().fold(f32::MIN, f32::max);
            let min = xs.fold(f32::MAX, f32::min);
            max - min
        };
        assert!(span(&near) > span(&far));
    }
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

fn drawable_root_c3d(bitmap: &[u8], normal_q16: [i32; 3]) -> Vec<u8> {
    let group0_len = C3D_GROUP0_VALUES_OFFSET + 2;
    let group1_len = C3D_GROUP1_FIXED_LEN + 3 * C3D_GROUP1_ENTRY32_LEN + C3D_GROUP1_ENTRY24_LEN;
    let group1_start = C3D_HEADER_LEN + group0_len;
    let blob_start = group1_start + group1_len;
    let mut input = vec![0; blob_start + 16 + bitmap.len()];
    input[0..4].copy_from_slice(&C3D_MAGIC);
    input[4..8].copy_from_slice(&C3D_FORMAT_VERSION.to_le_bytes());
    input[12..14].copy_from_slice(&1u16.to_le_bytes());
    input[14..16].copy_from_slice(&1u16.to_le_bytes());
    input[16..18].copy_from_slice(&1u16.to_le_bytes());
    for (index, offset) in [C3D_HEADER_LEN, group1_start, blob_start]
        .into_iter()
        .enumerate()
    {
        let field = 20 + index * 4;
        input[field..field + 4].copy_from_slice(&(offset as u32).to_le_bytes());
    }

    let group0 = C3D_HEADER_LEN;
    input[group0..group0 + 4].copy_from_slice(&(group0_len as u32).to_le_bytes());
    input[group0 + 4..group0 + 6].copy_from_slice(&(-1i16).to_le_bytes());
    for index in [0, 5, 10, 15] {
        let field = group0 + 8 + index * 4;
        input[field..field + 4].copy_from_slice(&(1i32 << 16).to_le_bytes());
    }
    input[group0 + 76] = 1;
    input[group0 + C3D_GROUP0_VALUES_OFFSET..group0 + C3D_GROUP0_VALUES_OFFSET + 2]
        .copy_from_slice(&0u16.to_le_bytes());

    let group1 = group1_start;
    input[group1 + 12..group1 + 16].copy_from_slice(&(group1_len as u32).to_le_bytes());
    input[group1 + 16..group1 + 18].copy_from_slice(&3u16.to_le_bytes());
    input[group1 + 18..group1 + 20].copy_from_slice(&1u16.to_le_bytes());
    let [normal_x, normal_y, normal_z] = normal_q16;
    let vertices: [[i32; 8]; 3] = [
        [
            -(4 << 16),
            -(3 << 16),
            0,
            0,
            -(1 << 16),
            normal_x,
            normal_y,
            normal_z,
        ],
        [-(4 << 16), 3 << 16, 0, 0, 0, normal_x, normal_y, normal_z],
        [
            4 << 16,
            -(3 << 16),
            0,
            1 << 16,
            -(1 << 16),
            normal_x,
            normal_y,
            normal_z,
        ],
    ];
    for (vertex_index, vertex) in vertices.iter().enumerate() {
        let start = group1 + C3D_GROUP1_FIXED_LEN + vertex_index * C3D_GROUP1_ENTRY32_LEN;
        for (component_index, component) in vertex.iter().enumerate() {
            let field = start + component_index * 4;
            input[field..field + 4].copy_from_slice(&component.to_le_bytes());
        }
    }
    let entry24 = group1 + C3D_GROUP1_FIXED_LEN + 3 * C3D_GROUP1_ENTRY32_LEN;
    for (corner, ordinal) in [0u16, 1, 2].into_iter().enumerate() {
        let field = entry24 + C3D_ENTRY24_ENTRY32_ORDINAL_OFFSETS[corner];
        input[field..field + 2].copy_from_slice(&ordinal.to_le_bytes());
    }
    input[entry24 + 8..entry24 + 10].copy_from_slice(&0i16.to_le_bytes());

    input[blob_start..blob_start + 12].copy_from_slice(b"SCENE-TEX!!!");
    input[blob_start + 12..blob_start + 16].copy_from_slice(&(bitmap.len() as u32).to_le_bytes());
    input[blob_start + 16..].copy_from_slice(bitmap);
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
