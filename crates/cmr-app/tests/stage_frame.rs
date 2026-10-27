//! End-to-end coverage for the headless stage-frame command: a synthetic stage
//! directory under a `--source`-style media root is read, loaded, walked,
//! texture-bound, composed, and bridged to the shared presentation type.
//!
//! The `.c3d` scene fixture is a tiny synthetic single-node, single-triangle
//! mesh with an inline `BM` bitmap group-2 blob (the shape the shipped frontend
//! C3D assets use). No game payload bytes are copied.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cmr_app::{
    StageFrameError, StageRenderResources, StageRenderStats, StageRendererChoice,
    StageRendererKind, render_stage_frame, run_headless_stage_frame,
};
use cmr_formats::c3d::{
    C3D_ENTRY24_ENTRY32_ORDINAL_OFFSETS, C3D_FORMAT_VERSION, C3D_GROUP0_VALUES_OFFSET,
    C3D_GROUP1_ENTRY24_LEN, C3D_GROUP1_ENTRY32_LEN, C3D_GROUP1_FIXED_LEN, C3D_HEADER_LEN,
    C3D_MAGIC,
};
use cmr_formats::stage::{
    C3D_EXTENSION, CAT_EXTENSION, CAT_NODE_STRIDE, OBJ_EXTENSION, SHT_EXTENSION,
};
use cmr_platform_sdl3::present_software_frame_once;

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

/// A temporary media root removed on drop.
struct MediaDir(PathBuf);

impl MediaDir {
    fn new() -> Self {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("cmr-stage-frame-{}-{id}", std::process::id()));
        fs::create_dir(&path).expect("create temporary media directory");
        Self(path)
    }

    fn root(&self) -> &Path {
        &self.0
    }

    fn write_section(&self, stage: &str, extension: &str, bytes: &[u8]) {
        fs::write(self.0.join(format!("{stage}{extension}")), bytes)
            .expect("write stage section file");
    }
}

impl Drop for MediaDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn composes_a_drawable_stage_from_a_source_root() {
    let media = MediaDir::new();
    media.write_section(
        "teststage",
        C3D_EXTENSION,
        &drawable_c3d(&indexed_bitmap_4x4(), [0, 0, -(1 << 16)]),
    );

    let output =
        run_headless_stage_frame(media.root(), "teststage", StageRendererChoice::Software).unwrap();

    assert_eq!(output.stage, "teststage");
    assert_eq!(output.node_count, 1);
    assert_eq!(output.draw_ops, 1);
    assert_eq!(output.textures, 1);
    assert_eq!(output.renderer, StageRendererKind::Software);
    let StageRenderStats::Software(stats) = output.stats else {
        panic!("the software renderer must report software statistics");
    };
    assert_eq!(stats.prims_composed, 1);
    assert_eq!(stats.unresolved_prims, 0);
    assert_eq!(stats.empty_lod_nodes, 0);
    assert!(stats.covered > 0);
    assert!(stats.depth_passed > 0);
    assert_eq!(output.frame_digest.extent.width, 640);
    assert_eq!(output.frame_digest.extent.height, 480);

    // The summary is path-free and deterministic across runs.
    let again =
        run_headless_stage_frame(media.root(), "teststage", StageRendererChoice::Software).unwrap();
    assert_eq!(output.stable_summary(), again.stable_summary());
    assert!(
        !output
            .stable_summary()
            .contains(media.root().to_str().unwrap())
    );
}

#[test]
#[ignore = "requires an SDL video driver; run with SDL_VIDEODRIVER=dummy"]
fn sdl_host_presents_a_composed_stage_frame() {
    let media = MediaDir::new();
    media.write_section(
        "windowed",
        C3D_EXTENSION,
        &drawable_c3d(&indexed_bitmap_4x4(), [0, 0, -(1 << 16)]),
    );

    // The windowed command's composition path, presented once (the live
    // `run_windowed_stage_frame` waits for the user to close the window).
    let rendered =
        render_stage_frame(media.root(), "windowed", StageRendererChoice::Software).unwrap();
    assert_eq!(rendered.node_count, 1);
    present_software_frame_once(&rendered.frame).unwrap();
}

#[test]
fn reads_an_optional_cat_sibling_without_failing() {
    let media = MediaDir::new();
    media.write_section(
        "withcat",
        C3D_EXTENSION,
        &drawable_c3d(&indexed_bitmap_4x4(), [0, 0, -(1 << 16)]),
    );
    // Two well-formed (all-zero) .cat nodes alongside the scene.
    media.write_section("withcat", CAT_EXTENSION, &[0u8; CAT_NODE_STRIDE * 2]);

    let output =
        run_headless_stage_frame(media.root(), "withcat", StageRendererChoice::Software).unwrap();
    assert_eq!(output.node_count, 1);
    let StageRenderStats::Software(stats) = output.stats else {
        panic!("the software renderer must report software statistics");
    };
    assert_eq!(stats.prims_composed, 1);
}

#[test]
fn a_missing_scene_section_is_a_load_error() {
    let media = MediaDir::new();
    // Only a .cat sibling, no mandatory .c3d scene.
    media.write_section("noscene", CAT_EXTENSION, &[0u8; CAT_NODE_STRIDE]);

    let error = run_headless_stage_frame(media.root(), "noscene", StageRendererChoice::Software)
        .unwrap_err();
    assert_eq!(error.kind(), "load");
    assert!(matches!(error, StageFrameError::Load(_)));
}

#[test]
fn a_malformed_scene_is_a_load_error() {
    let media = MediaDir::new();
    media.write_section("broken", C3D_EXTENSION, b"not a c3d scene at all");

    let error = run_headless_stage_frame(media.root(), "broken", StageRendererChoice::Software)
        .unwrap_err();
    assert_eq!(error.kind(), "load");
}

#[test]
#[ignore = "requires a Vulkan adapter; run explicitly for the GPU stage-renderer contract"]
fn gpu_and_software_render_the_same_synthetic_layered_stage_frame() {
    let media = MediaDir::new();
    let scene = drawable_c3d(&indexed_bitmap_4x4(), [0, 0, -(1 << 16)]);
    media.write_section("gpucompare", C3D_EXTENSION, &scene);
    media.write_section("gpucompare", OBJ_EXTENSION, &scene);
    media.write_section("gpucompare", SHT_EXTENSION, &scene);

    let software =
        render_stage_frame(media.root(), "gpucompare", StageRendererChoice::Software).unwrap();
    let gpu = render_stage_frame(media.root(), "gpucompare", StageRendererChoice::Gpu).unwrap();

    assert_eq!(software.renderer, StageRendererKind::Software);
    assert_eq!(gpu.renderer, StageRendererKind::Gpu);
    // Both renderers are held to the same source-owned layered fixture
    // contract. OBJ/SHT source index zero is keyed, later equal-depth geometry
    // wins, and the host receives the same active-palette index frame.
    assert_eq!(gpu.frame.extent(), software.frame.extent());
    assert_eq!(gpu.frame.pixels(), software.frame.pixels());
    assert_eq!(gpu.frame.palette(), software.frame.palette());
    let (StageRenderStats::Gpu(gpu_stats), StageRenderStats::Software(software_stats)) =
        (gpu.stats, software.stats)
    else {
        panic!("each renderer must report its own statistics");
    };
    // The prepared adapter retains every drawable prim the software composer
    // rasterizes, and the same unresolved-slot accounting.
    assert_eq!(gpu_stats.draws, software_stats.prims_composed);
    assert_eq!(gpu_stats.unresolved_prims, software_stats.unresolved_prims);
    assert_eq!(
        gpu_stats.unique_textures,
        software_stats.empty_lod_nodes + 3
    );

    // A second camera pose proves the persistent scene takes its VIEW matrix
    // per frame rather than baking the placeholder camera at upload time.
    let mut moved = [0i32; 16];
    moved[0] = 1 << 16;
    moved[5] = 1 << 16;
    moved[10] = 1 << 16;
    moved[12] = -(1 << 16);
    moved[14] = -(5 << 16);
    moved[15] = 1 << 16;
    let mut software_resources =
        StageRenderResources::load_with(media.root(), "gpucompare", StageRendererChoice::Software)
            .unwrap();
    let mut gpu_resources =
        StageRenderResources::load_with(media.root(), "gpucompare", StageRendererChoice::Gpu)
            .unwrap();
    let software_moved = software_resources.compose(&moved).unwrap();
    let gpu_moved = gpu_resources.compose(&moved).unwrap();
    assert_eq!(gpu_moved.renderer, StageRendererKind::Gpu);
    assert_eq!(gpu_moved.frame.pixels(), software_moved.frame.pixels());
}

/// A minimal CM3D scene: one root group-0 node selecting one group-1 LOD mesh of
/// a single camera-facing triangle, textured by one inline `BM` group-2 blob.
fn drawable_c3d(bitmap: &[u8], normal_q16: [i32; 3]) -> Vec<u8> {
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

/// A tiny 4x4, 8bpp `BM` bitmap with a full 256-entry palette.
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
