use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use cmr_assets::{
    AssetFile, AssetProfile, AssetStore, ReviewedProfile, StorageEncoding, import_source,
};
use cmr_core::c3d_stage_scene::StageScene;
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
    C3D_MAGIC, C3dLimits, parse_c3d,
};
use cmr_formats::riff_palette::PaletteEntry;
use cmr_formats::stored_asset::DecodeLimits;
use cmr_video::SoftwareIndexedBackend;
use cmr_video::c3d_software::prepare_software_root_c3d_bindings;
use cmr_video::c3d_software_raster::{
    SoftwareC3dDepthBuffer, SoftwareC3dIndexedQuantization, SoftwareC3dPreparedCopyBatch,
    SoftwareC3dRasterError, SoftwareC3dTransformState, compose_root_c3d_indexed_copy_no_blend,
    preflight_mesh_lod_prim, preflight_root_c3d_batch, preflight_root_c3d_copy_batch,
};
use cmr_video::software_stage_scene::{StageC3dTextures, compose_stage_scene_frame};
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

/// Bridge: an `assemble_lod_draw_group` prim (the recovered stage/car geometry
/// path) feeds the real cmr-video transform/clip/cull through the new
/// `preflight_mesh_lod_prim`, producing the same visible triangle the recovered
/// camera pipeline sees. This closes parse -> `c3d_mesh` assembly -> raster for a
/// node mesh, independent of the menu `RootMenuC3dDrawCommand` path.
#[test]
fn assembled_mesh_prim_drives_the_real_gameplay_transform() {
    let bitmap = indexed_bitmap_4x4();
    // The same single camera-facing triangle the gameplay test uses.
    let bytes = drawable_root_c3d(&bitmap, [0, 0, -(1 << 16)]);
    let file = parse_c3d(&bytes, C3dLimits::default()).unwrap();
    let record = file.group1_records().next().unwrap();

    // Assemble the LOD draw group from the parsed mesh record, keying prims on
    // each face's group-2 material ordinal (the recovered loader topology).
    let selectors: Vec<i16> = record
        .entries_24()
        .map(|face| face.group2_ordinal())
        .collect();
    let group = cmr_core::c3d_mesh::assemble_lod_draw_group(
        record,
        cmr_core::c3d_render::VERTEXTYPE_LVERTEX,
        &selectors,
    )
    .unwrap();
    assert_eq!(group.prims().len(), 1);
    let prim = &group.prims()[0];

    // Identity node, camera translated back along -Z so the model-z=0 triangle
    // sits at positive view-space depth (as in the gameplay command test).
    let identity_q16 = cmr_core::c3d_camera::MATRIX_IDENTITY_Q16;
    let mut camera_world = identity_q16;
    camera_world[14] = -(6 << 16);
    let state = || {
        SoftwareC3dTransformState::for_gameplay_scene(64, 48, &camera_world, &identity_q16).unwrap()
    };

    let batch = preflight_mesh_lod_prim(prim.unique_entries_32(), prim.indices(), state()).unwrap();
    let again = preflight_mesh_lod_prim(prim.unique_entries_32(), prim.indices(), state()).unwrap();

    // The one assembled triangle survives clipping and the back-face cull.
    assert_eq!(batch.source_triangle_count(), 1);
    assert_eq!(batch.triangles().len(), 1);
    assert_eq!(batch.counter_clockwise_triangle_count(), 0);
    assert_eq!(batch.fully_clipped_source_triangle_count(), 0);
    // Deterministic, and every prepared screen vertex is finite and in-bounds.
    assert_eq!(batch, again);
    for triangle in batch.triangles() {
        for vertex in triangle.vertices() {
            let [x, y, z] = vertex.position();
            assert!(x.is_finite() && y.is_finite() && z.is_finite());
            assert!((0.0..=64.0).contains(&x));
            assert!((0.0..=48.0).contains(&y));
        }
    }
}

/// E07 Z-buffer oracle: two synthetic C3D objects at different scene depths are
/// composited into one shared framebuffer + depth buffer through the real
/// rasterizer (`compose_root_c3d_indexed_copy_no_blend`). The shipped renderer
/// resolves occlusion with a hardware Z-buffer, **not** a painter's triangle
/// sort (`rea.official-eu.c3d-rasterizer`/`c3d-scene-graph`), so the final image
/// and depth must be **independent of submission order**, each pixel must retain
/// the nearest surface (combined depth == elementwise `min` of each object's
/// solo depth), and the nearer object must occlude the farther where they
/// overlap. The two objects share one command but use node world matrices at
/// different Z, so both project to nearly the same screen triangle at strictly
/// different depths.
#[test]
fn gameplay_zbuffer_resolves_occlusion_independent_of_draw_order() {
    let temporary = TestDirectory::new();
    let source = temporary.0.join("source");
    let slot = RootMenuC3dSlot::Championship;
    let path = slot.path(VisualVariant::Normal);
    let bitmap = indexed_bitmap_4x4();
    let bytes = drawable_root_c3d(&bitmap, [0, 0, -(1 << 16)]);
    let source_file = source.join(path);
    fs::create_dir_all(source_file.parent().unwrap()).unwrap();
    fs::write(&source_file, &bytes).unwrap();

    let profile = AssetProfile {
        schema_version: 1,
        id: "synthetic-video-c3d-zbuffer".to_owned(),
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
    let resource_id = topology.resources().next().unwrap().0;
    let texture = catalog.binding(resource_id).unwrap();
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
    let command = &plan.commands()[0];

    // One camera held back along -Z; the two objects differ only by a node
    // world-matrix Z translation, so the far object is uniformly deeper and
    // projects to nearly the same screen triangle (guaranteeing overlap).
    let identity_q16 = cmr_core::c3d_camera::MATRIX_IDENTITY_Q16;
    let camera = {
        let mut m = identity_q16;
        m[14] = -(6 << 16);
        m
    };
    let node_at = |depth_units: i32| {
        let mut m = identity_q16;
        m[14] = depth_units << 16;
        m
    };
    let state = |node: &[i32; 16]| {
        SoftwareC3dTransformState::for_gameplay_scene(extent.width, extent.height, &camera, node)
            .unwrap()
    };
    let near = preflight_root_c3d_copy_batch(command, state(&node_at(0))).unwrap();
    let far = preflight_root_c3d_copy_batch(command, state(&node_at(8))).unwrap();

    // Both objects are visible and front-facing.
    assert_eq!(near.triangles().len(), 1);
    assert_eq!(far.triangles().len(), 1);
    assert_eq!(near.counter_clockwise_triangle_count(), 0);
    assert_eq!(far.counter_clockwise_triangle_count(), 0);

    let extent_arr = [extent.width, extent.height];
    let pixel_count = usize::from(extent.width) * usize::from(extent.height);

    // Composite a sequence of batches into a fresh shared framebuffer + depth
    // buffer (no clear between draws), returning the pixels, the depth values,
    // and the cumulative depth pass/reject counts.
    let render = |order: &[&SoftwareC3dPreparedCopyBatch]| {
        let mut depth = SoftwareC3dDepthBuffer::new_far(extent_arr).unwrap();
        let mut target = vec![0u8; pixel_count];
        let mut passed = 0usize;
        let mut rejected = 0usize;
        for batch in order {
            let outcome = compose_root_c3d_indexed_copy_no_blend(
                batch,
                texture,
                &mut depth,
                extent_arr,
                &mut target,
                &active_palette_16(),
                SoftwareC3dIndexedQuantization::NearestRgbL1LowestIndexNoDither,
            )
            .unwrap();
            passed += outcome.depth_passed();
            rejected += outcome.depth_rejected();
        }
        (target, depth.values().to_vec(), passed, rejected)
    };

    let near_solo = render(&[&near]);
    let far_solo = render(&[&far]);
    let near_then_far = render(&[&near, &far]);
    let far_then_near = render(&[&far, &near]);
    let near_then_far_again = render(&[&near, &far]);

    // Each object alone covers some pixels; the nearer one covers more (it is
    // larger on screen) and leaves background.
    assert!(near_solo.2 > 0 && far_solo.2 > 0);
    assert!(near_solo.2 > far_solo.2);
    assert!(near_then_far.1.iter().any(|&d| d < 1.0));
    assert!(near_then_far.1.contains(&1.0));
    // The two solo images genuinely differ (the far object is smaller).
    assert_ne!(near_solo.0, far_solo.0);

    // Z-buffer defining property: the final image and depth are identical
    // regardless of submission order - a painter's sort would not guarantee it.
    assert_eq!(near_then_far.0, far_then_near.0);
    assert_eq!(near_then_far.1, far_then_near.1);

    // Deterministic for identical input.
    assert_eq!(near_then_far.0, near_then_far_again.0);
    assert_eq!(near_then_far.1, near_then_far_again.1);

    // Each pixel retains the nearest surface: the combined depth is the
    // elementwise minimum of the two solo depth buffers.
    for i in 0..pixel_count {
        assert_eq!(near_then_far.1[i], near_solo.1[i].min(far_solo.1[i]));
    }

    // Where both objects cover a pixel, the near one is strictly in front and
    // its texel wins in the combined image.
    let mut overlap = 0usize;
    for i in 0..pixel_count {
        if near_solo.1[i] < 1.0 && far_solo.1[i] < 1.0 {
            overlap += 1;
            assert!(near_solo.1[i] < far_solo.1[i]);
            assert_eq!(near_then_far.0[i], near_solo.0[i]);
        }
    }
    assert!(
        overlap > 0,
        "the two objects must overlap for an occlusion test"
    );

    // Occlusion actually happened: drawing the far object after the near one
    // rejects its occluded fragments, so fewer fragments pass than the two
    // objects would pass independently.
    assert!(near_then_far.3 > 0);
    assert!(near_then_far.2 < near_solo.2 + far_solo.2);
}

/// E08 stage scene session: a parsed C3D scene is walked by
/// `cmr_core::c3d_stage_scene::StageScene` and composited to pixels by
/// `cmr_video::software_stage_scene::compose_stage_scene_frame`, with the prim's
/// group-2 material ordinal resolved to a texture by a caller-supplied closure.
/// The result must match the direct single-prim path (same camera, same identity
/// node world), proving the scene walk adds no distortion, and the deferred
/// texture resolver must be honoured (an unresolved ordinal composites nothing).
#[test]
fn stage_scene_frame_composites_a_walked_node_like_the_direct_prim_path() {
    let temporary = TestDirectory::new();
    let source = temporary.0.join("source");
    let slot = RootMenuC3dSlot::Championship;
    let path = slot.path(VisualVariant::Normal);
    let bitmap = indexed_bitmap_4x4();
    let bytes = drawable_root_c3d(&bitmap, [0, 0, -(1 << 16)]);
    let source_file = source.join(path);
    fs::create_dir_all(source_file.parent().unwrap()).unwrap();
    fs::write(&source_file, &bytes).unwrap();

    let profile = AssetProfile {
        schema_version: 1,
        id: "synthetic-video-c3d-stage-scene".to_owned(),
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
    let catalog = prepare_software_root_c3d_bindings(&topology, load_states).unwrap();
    let resource_id = topology.resources().next().unwrap().0;
    let texture = catalog.binding(resource_id).unwrap();

    // Walk the same parsed scene through the backend-neutral StageScene.
    let file = parse_c3d(&bytes, C3dLimits::default()).unwrap();
    let identity_q16 = cmr_core::c3d_camera::MATRIX_IDENTITY_Q16;
    let scene = StageScene::from_c3d(
        &file,
        &identity_q16,
        cmr_core::c3d_render::VERTEXTYPE_LVERTEX,
    )
    .unwrap();
    assert_eq!(scene.node_count(), 1);
    assert_eq!(scene.draw_order().len(), 1);

    let mut camera = identity_q16;
    camera[14] = -(6 << 16);
    let extent = [64u16, 48u16];
    let palette = active_palette_16();

    let frame = compose_stage_scene_frame(
        &scene,
        &camera,
        extent,
        &palette,
        SoftwareC3dIndexedQuantization::NearestRgbL1LowestIndexNoDither,
        |ordinal| (ordinal == 0).then_some(texture),
    )
    .unwrap();

    // The single prim composited, nothing skipped, and it covered the target.
    assert_eq!(frame.stats().prims_composed, 1);
    assert_eq!(frame.stats().unresolved_prims, 0);
    assert_eq!(frame.stats().empty_lod_nodes, 0);
    assert!(frame.stats().depth_passed > 0);
    assert!(frame.stats().covered > 0);
    assert!(frame.pixels().iter().any(|&index| index != 0));
    assert!(frame.depth().values().iter().any(|&d| d < 1.0));
    assert!(frame.depth().values().contains(&1.0));

    // Determinism: recomposing yields the identical frame.
    let again = compose_stage_scene_frame(
        &scene,
        &camera,
        extent,
        &palette,
        SoftwareC3dIndexedQuantization::NearestRgbL1LowestIndexNoDither,
        |ordinal| (ordinal == 0).then_some(texture),
    )
    .unwrap();
    assert_eq!(frame.pixels(), again.pixels());
    assert_eq!(frame.depth().values(), again.depth().values());

    // The frame bridges to the shared presentation type the platform host
    // consumes, preserving the indexed pixels and palette and expanding to RGBA8.
    let presented = frame.to_presented_frame().unwrap();
    assert_eq!(
        presented.extent(),
        DisplayExtent {
            width: 64,
            height: 48
        }
    );
    assert_eq!(presented.pixels(), frame.pixels());
    assert_eq!(presented.palette(), frame.palette());
    let rgba = presented.expand_rgba8().unwrap();
    assert_eq!(rgba.extent(), presented.extent());

    // Faithfulness: the walked frame equals the direct single-prim path with the
    // same camera and identity node world.
    let record = file.group1_records().next().unwrap();
    let selectors: Vec<i16> = record
        .entries_24()
        .map(|face| face.group2_ordinal())
        .collect();
    let group = cmr_core::c3d_mesh::assemble_lod_draw_group(
        record,
        cmr_core::c3d_render::VERTEXTYPE_LVERTEX,
        &selectors,
    )
    .unwrap();
    let prim = &group.prims()[0];
    let direct_state =
        SoftwareC3dTransformState::for_gameplay_scene(extent[0], extent[1], &camera, &identity_q16)
            .unwrap();
    let direct_batch =
        preflight_mesh_lod_prim(prim.unique_entries_32(), prim.indices(), direct_state).unwrap();
    let mut direct_depth = SoftwareC3dDepthBuffer::new_far(extent).unwrap();
    let mut direct_pixels = vec![0u8; usize::from(extent[0]) * usize::from(extent[1])];
    compose_root_c3d_indexed_copy_no_blend(
        &direct_batch,
        texture,
        &mut direct_depth,
        extent,
        &mut direct_pixels,
        &palette,
        SoftwareC3dIndexedQuantization::NearestRgbL1LowestIndexNoDither,
    )
    .unwrap();
    assert_eq!(frame.pixels(), direct_pixels.as_slice());
    assert_eq!(frame.depth().values(), direct_depth.values());

    // The deferred texture resolver is honoured: an unresolved ordinal composites
    // nothing and leaves the frame at its cleared background.
    let empty = compose_stage_scene_frame(
        &scene,
        &camera,
        extent,
        &palette,
        SoftwareC3dIndexedQuantization::NearestRgbL1LowestIndexNoDither,
        |_ordinal| None,
    )
    .unwrap();
    assert_eq!(empty.stats().prims_composed, 0);
    assert_eq!(empty.stats().unresolved_prims, 1);
    assert!(empty.pixels().iter().all(|&index| index == 0));
    assert!(empty.depth().values().iter().all(|&d| d == 1.0));

    // The stage's OWN group-2 texture binding resolves the face ordinal to a
    // sampled texture and composites to the identical frame as the root-menu
    // catalog path (both decode the same inline BM blob at full extent).
    let stage_textures = StageC3dTextures::prepare(&file, BitmapLimits::default()).unwrap();
    assert_eq!(stage_textures.len(), 1);
    assert!(stage_textures.texture_for_ordinal(0).is_some());
    assert!(stage_textures.texture_for_ordinal(-1).is_none());
    assert!(stage_textures.texture_for_ordinal(1).is_none());
    let bound = compose_stage_scene_frame(
        &scene,
        &camera,
        extent,
        &palette,
        SoftwareC3dIndexedQuantization::NearestRgbL1LowestIndexNoDither,
        |ordinal| stage_textures.texture_for_ordinal(ordinal),
    )
    .unwrap();
    assert_eq!(bound.stats().prims_composed, 1);
    assert_eq!(bound.pixels(), frame.pixels());
    assert_eq!(bound.depth().values(), frame.depth().values());
}

/// The real shipped stage/car group-2 blobs are the C3D engine inline texture
/// format, not `BM` bitmaps. `StageC3dTextures::prepare` must decode an inline
/// paletted engine blob positionally into its ordinal slot - the arena-backed
/// frontend prepare path rejected every such blob as `UnsupportedResource`.
#[test]
fn stage_textures_decode_an_inline_engine_paletted_blob_positionally() {
    let file = parse_c3d(
        &one_resource_c3d(&engine_paletted_blob()),
        C3dLimits::default(),
    )
    .unwrap();
    let textures = StageC3dTextures::prepare(&file, BitmapLimits::default()).unwrap();

    assert_eq!(textures.len(), 1);
    assert!(textures.texture_for_ordinal(-1).is_none());
    assert!(textures.texture_for_ordinal(1).is_none());
    let texture = textures
        .texture_for_ordinal(0)
        .expect("ordinal 0 is the decoded engine paletted texture");
    assert_eq!(texture.extent(), [4, 2]);
    assert_eq!(texture.indices(), [1, 2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(texture.palette().len(), 16);
    assert_eq!(
        texture.palette()[1],
        BitmapPaletteEntry {
            red: 248,
            green: 0,
            blue: 0,
            reserved: 0,
        }
    );
}

/// A 16-entry indexed display palette (the rest white), matching the synthetic
/// `indexed_bitmap_4x4` source colors used by the pixel-path tests.
fn active_palette_16() -> [PaletteEntry; 256] {
    let mut palette = [PaletteEntry {
        red: 255,
        green: 255,
        blue: 255,
        flags: 0,
    }; 256];
    for (index, entry) in palette.iter_mut().take(16).enumerate() {
        *entry = PaletteEntry {
            red: index as u8 + 2,
            green: index as u8 + 1,
            blue: index as u8,
            flags: 0xa5,
        };
    }
    palette
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

/// A 16-colour inline-palette engine paletted blob: pixel extent 4x2, indices
/// `[1..=8]`, palette entry 1 red and 2 green (`X1R5G5B5`). Mirrors the unit
/// fixture in `c3d_software`, built through the recovered layout offsets.
fn engine_paletted_blob() -> Vec<u8> {
    const PALETTED_TAG: u8 = 0x10;
    let palette_entries = 16usize;
    let descriptor = 0x14 + palette_entries * 2;
    let data_start = descriptor + 0x0c;
    let (stored_width, stored_height) = (1u16, 2u16);
    let row_bytes = 2 * stored_width as usize;
    let mut blob = vec![0u8; data_start + row_bytes * stored_height as usize];
    blob[0] = PALETTED_TAG;
    blob[0x04] = 0x08; // 16-colour + inline palette
    blob[0x16..0x18].copy_from_slice(&0x001fu16.to_le_bytes()); // palette[1] = red
    blob[0x18..0x1a].copy_from_slice(&0x03e0u16.to_le_bytes()); // palette[2] = green
    blob[descriptor + 0x08..descriptor + 0x0a].copy_from_slice(&stored_width.to_le_bytes());
    blob[descriptor + 0x0a..descriptor + 0x0c].copy_from_slice(&stored_height.to_le_bytes());
    // Two pixels per byte, low nibble first: row0 [1,2,3,4], row1 [5,6,7,8].
    blob[data_start..data_start + 4].copy_from_slice(&[0x21, 0x43, 0x65, 0x87]);
    blob
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
