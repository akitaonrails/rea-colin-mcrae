use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use cmr_assets::{
    AssetFile, AssetProfile, AssetStore, ReviewedProfile, StorageEncoding, import_source,
};
use cmr_core::display::{
    CooperativeMode, DisplayExtent, IndexedMode, IndexedVideoBackend, SurfaceTopology,
};
use cmr_core::font_assets::{FONT_COUNT, FontSet, FontStyle};
use cmr_core::font_raster::UploadedFontPool;
use cmr_core::frontend::{FrontendPhase, ROOT_FOOTER_TEXT_INDEX, RootSelection};
use cmr_core::localization::{IndexedTextTable, Language};
use cmr_core::root_menu_assets::VisualVariant;
use cmr_core::root_menu_bitmap_assets::{RootMenuBitmapSlot, load_root_menu_bitmaps};
use cmr_core::root_menu_bitmap_raster::{prepare_root_menu_surfaces, upload_root_menu_bitmaps};
use cmr_core::root_menu_c3d_assets::{RootMenuC3dAngles, RootMenuC3dSlot, load_root_menu_c3d};
use cmr_core::root_menu_c3d_execute::RootMenuC3dBatchExecutor;
use cmr_core::root_menu_c3d_runtime::{
    C3dMode2CornerSource, C3dMode2LoadState, C3dSurfaceLayoutGates, RootMenuC3dMode2LoadStates,
    materialize_root_menu_mode2_resource_scaled,
};
use cmr_core::root_menu_c3d_scene::{PreparedRootMenuC3dPipeline, RootMenuC3dDepthClear};
use cmr_core::root_menu_c3d_topology::prepare_root_menu_c3d_topology;
use cmr_core::root_menu_c3d_visibility::RootMenuC3dVisibilityLatches;
use cmr_core::root_menu_frame::{RootFrameResources, execute_root_frame};
use cmr_core::root_menu_visual::{
    ROOT_INITIAL_HORIZONTAL_TRANSFORM_Q16, ROOT_INITIAL_VERTICAL_TRANSFORM_Q16, RootRenderInput,
};
use cmr_core::source_store::UploadedSourceStore;
use cmr_formats::bitmap::BitmapLimits;
use cmr_formats::c3d::{
    C3D_ENTRY24_ENTRY32_ORDINAL_OFFSETS, C3D_FORMAT_VERSION, C3D_GROUP0_VALUES_OFFSET,
    C3D_GROUP1_ENTRY24_LEN, C3D_GROUP1_ENTRY32_LEN, C3D_GROUP1_FIXED_LEN, C3D_HEADER_LEN,
    C3D_MAGIC, C3dLimits,
};
use cmr_formats::font::{FontLimits, TextLayoutLimits, parse_legacy_font};
use cmr_formats::riff_palette::{LegacyPalette256, PaletteEntry};
use cmr_formats::stored_asset::DecodeLimits;
use cmr_video::SoftwareIndexedBackend;
use cmr_video::c3d_software::prepare_software_root_c3d_bindings;
use sha2::{Digest, Sha256};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

const EXTENT: DisplayExtent = DisplayExtent {
    width: 640,
    height: 480,
};
const C3D_DISPLAY_INDEX: u8 = 37;

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("cmr-video-root-frame-{}-{id}", std::process::id()));
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
fn synthetic_root_frame_captures_representative_layers_twice_without_reuploading() {
    let temporary = TestDirectory::new();
    let source = temporary.0.join("source");
    let (reviewed, expected_center_marker) = write_reviewed_root_assets(&source);
    let imported = temporary.0.join("runtime-data");
    import_source(&source, &imported, &reviewed).unwrap();
    let store = AssetStore::open(
        &imported,
        &reviewed,
        DecodeLimits {
            max_stored_bytes: 4 * 1024 * 1024,
            max_payload_bytes: 4 * 1024 * 1024,
        },
    )
    .unwrap();

    let palette = active_palette();
    let bitmaps = load_root_menu_bitmaps(
        &store,
        Language::English,
        VisualVariant::Normal,
        BitmapLimits::default(),
    )
    .unwrap();
    assert_eq!(bitmaps.loaded_len(), RootMenuBitmapSlot::ALL.len());
    let prepared_bitmaps =
        prepare_root_menu_surfaces(bitmaps, &LegacyPalette256 { entries: palette }).unwrap();

    let c3d_assets = load_root_menu_c3d(&store, VisualVariant::Normal, C3dLimits::default());
    assert_eq!(c3d_assets.loaded_len(), 1);
    let topology = prepare_root_menu_c3d_topology(&c3d_assets, BitmapLimits::default()).unwrap();
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

    let (mut backend, back) = initialized_backend(&palette);
    let mut sources = UploadedSourceStore::new(&mut backend);
    let fonts = UploadedFontPool::upload(&mut sources, font_set()).unwrap();
    let uploaded_bitmaps = upload_root_menu_bitmaps(&mut sources, prepared_bitmaps)
        .unwrap()
        .pool;
    let persistent_source_count = sources.uploaded_source_count();
    assert_eq!(
        persistent_source_count,
        FONT_COUNT + RootMenuBitmapSlot::ALL.len()
    );
    let text = footer_table();
    assert!(sources.backend().presented_frame().is_none());

    for expected_angle in [8, 16] {
        let pipeline = PreparedRootMenuC3dPipeline::new(
            &topology,
            &runtime,
            &mut angles,
            &mut visibility,
            &mut executor,
            RootMenuC3dDepthClear::Clear,
        )
        .unwrap();
        let outcome = execute_root_frame(
            root_input(),
            RootFrameResources {
                text: &text,
                fonts: &fonts,
                bitmaps: &uploaded_bitmaps,
                c3d: pipeline,
            },
            &mut sources,
            back,
            TextLayoutLimits::default(),
        )
        .unwrap();

        assert_eq!(outcome.bitmap_blits, 5);
        assert_eq!(outcome.bitmap_no_ops, 0);
        assert!(outcome.footer.is_some());
        assert_eq!(outcome.c3d_commands, 1);
        assert!(outcome.scene_completed);
        assert!(outcome.presentation_completed);
        for slot in RootMenuC3dSlot::ALL {
            assert_eq!(
                visibility.mask(slot).raw(),
                0x00,
                "terminal scene must clear every root visibility latch"
            );
        }
        assert_eq!(angles.angle(RootMenuC3dSlot::Championship), expected_angle);
        assert_eq!(sources.uploaded_source_count(), persistent_source_count);

        let backend = sources.backend();
        let presented = backend.presented_frame().unwrap();
        assert_eq!(presented.extent(), EXTENT);
        assert_eq!(presented.palette(), &palette);
        assert_eq!(presented.pixels(), backend.back_pixels().unwrap());
        assert_eq!(presented.pixels()[0], marker(RootMenuBitmapSlot::Backdrop));
        assert_eq!(presented.pixels()[124 * 640 + 232], expected_center_marker);
        assert_eq!(
            presented.pixels()[470 * 640 + 320],
            FontStyle::GreenInfo as u8 + 1
        );
        assert!(presented.pixels().contains(&C3D_DISPLAY_INDEX));
        let depth = backend.c3d_depth_values().unwrap();
        assert!(depth.iter().any(|value| *value < 1.0));
        assert!(depth.contains(&1.0));
    }

    drop(sources);
    assert_eq!(backend.uploaded_source_count(), 0);
}

fn initialized_backend(
    entries: &[PaletteEntry; 256],
) -> (SoftwareIndexedBackend, cmr_video::SurfaceHandle) {
    let mut backend = SoftwareIndexedBackend::default();
    backend.create_device().unwrap();
    backend
        .set_cooperative_mode(CooperativeMode::ExclusiveFullscreen)
        .unwrap();
    backend
        .set_display_mode(IndexedMode {
            extent: EXTENT,
            bits_per_pixel: 8,
        })
        .unwrap();
    let surfaces = backend
        .create_surfaces(
            SurfaceTopology::FlippingChainWithOneBackBuffer,
            IndexedMode {
                extent: EXTENT,
                bits_per_pixel: 8,
            },
        )
        .unwrap();
    let palette = backend.create_palette(entries).unwrap();
    backend
        .attach_palette(surfaces.visible_front, palette)
        .unwrap();
    backend.attach_palette(surfaces.back, palette).unwrap();
    backend.upload_palette(palette, entries).unwrap();
    (backend, surfaces.back)
}

fn root_input() -> RootRenderInput {
    RootRenderInput {
        phase: FrontendPhase::Interactive,
        selection: RootSelection::new(0).unwrap(),
        animation_position_q16: 0,
        language: Language::English,
        viewport_extent: EXTENT,
        horizontal_transform_q16: ROOT_INITIAL_HORIZONTAL_TRANSFORM_Q16,
        vertical_transform_q16: ROOT_INITIAL_VERTICAL_TRANSFORM_Q16,
        entry_visuals_suppressed: false,
    }
}

fn active_palette() -> [PaletteEntry; 256] {
    let mut palette = [PaletteEntry {
        red: 255,
        green: 255,
        blue: 255,
        flags: 0,
    }; 256];
    palette[C3D_DISPLAY_INDEX as usize] = PaletteEntry {
        red: 11,
        green: 10,
        blue: 9,
        flags: 0xa5,
    };
    palette
}

fn write_reviewed_root_assets(source: &std::path::Path) -> (ReviewedProfile, u8) {
    let mut files = BTreeMap::<&'static str, Vec<u8>>::new();
    let mut center_marker = 0;
    for slot in RootMenuBitmapSlot::ALL {
        let path = slot.path(Language::English, VisualVariant::Normal);
        let (width, height) = match slot {
            RootMenuBitmapSlot::Backdrop => (1600, 960),
            RootMenuBitmapSlot::MainSelected | RootMenuBitmapSlot::MainShadow => (256, 256),
            RootMenuBitmapSlot::Animation0
            | RootMenuBitmapSlot::Animation1
            | RootMenuBitmapSlot::Animation2
            | RootMenuBitmapSlot::Animation3
            | RootMenuBitmapSlot::Animation4
            | RootMenuBitmapSlot::Animation5
            | RootMenuBitmapSlot::Animation6 => (400, 400),
            _ => (4, 1),
        };
        let value = marker(slot);
        files
            .entry(path)
            .or_insert_with(|| indexed_bitmap(width, height, value));
        if slot == RootMenuBitmapSlot::ChampionshipGlobe {
            center_marker = files[path][14 + 40 + 256 * 4];
        }
    }
    files.insert(
        RootMenuC3dSlot::Championship.path(VisualVariant::Normal),
        drawable_root_c3d(&indexed_bitmap(4, 4, 9)),
    );

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
        id: "synthetic-root-frame-vertical".to_owned(),
        roots: vec!["New_Fe".to_owned()],
        empty_directories: Vec::new(),
        sentinels: Vec::new(),
        files: profile_files,
    };
    (
        ReviewedProfile::from_json(&serde_json::to_vec(&profile).unwrap()).unwrap(),
        center_marker,
    )
}

fn marker(slot: RootMenuBitmapSlot) -> u8 {
    40 + slot.index() as u8
}

fn indexed_bitmap(width: u32, height: u32, pixel: u8) -> Vec<u8> {
    let pixel_offset = 14 + 40 + 256 * 4;
    let stride = (width as usize).div_ceil(4) * 4;
    let image_len = stride * height as usize;
    let mut bytes = vec![0; pixel_offset + image_len];
    bytes[0..2].copy_from_slice(b"BM");
    let file_len = bytes.len() as u32;
    bytes[2..6].copy_from_slice(&file_len.to_le_bytes());
    bytes[10..14].copy_from_slice(&(pixel_offset as u32).to_le_bytes());
    bytes[14..18].copy_from_slice(&40u32.to_le_bytes());
    bytes[18..22].copy_from_slice(&(width as i32).to_le_bytes());
    bytes[22..26].copy_from_slice(&(height as i32).to_le_bytes());
    bytes[26..28].copy_from_slice(&1u16.to_le_bytes());
    bytes[28..30].copy_from_slice(&8u16.to_le_bytes());
    bytes[34..38].copy_from_slice(&(image_len as u32).to_le_bytes());
    bytes[46..50].copy_from_slice(&256u32.to_le_bytes());
    for index in 0..256 {
        let start = 54 + index * 4;
        bytes[start] = index as u8;
        bytes[start + 1] = (index as u8).wrapping_add(1);
        bytes[start + 2] = (index as u8).wrapping_add(2);
    }
    for row in bytes[pixel_offset..].chunks_exact_mut(stride) {
        row[..width as usize].fill(pixel);
    }
    bytes
}

fn font_set() -> FontSet {
    FontSet::from_ordered_fonts(std::array::from_fn(|index| {
        parse_legacy_font(&synthetic_font(index as u8), FontLimits::default()).unwrap()
    }))
}

fn synthetic_font(identity: u8) -> Vec<u8> {
    const PIXEL_OFFSET: usize = 0x436;
    const FONT_HEADER_SIZE: usize = 24;
    const GLYPH_SIZE: usize = 16;
    let trailer = PIXEL_OFFSET + 4;
    let mut input = vec![0; trailer + FONT_HEADER_SIZE + GLYPH_SIZE];
    input[0..2].copy_from_slice(b"BM");
    input[18..22].copy_from_slice(&1i32.to_le_bytes());
    input[22..26].copy_from_slice(&1i32.to_le_bytes());
    input[28..30].copy_from_slice(&8u16.to_le_bytes());
    input[PIXEL_OFFSET] = identity + 1;
    input[trailer] = identity;
    input[trailer + 1] = 1;
    input[trailer + 4..trailer + 8].copy_from_slice(&1u32.to_le_bytes());
    let glyph = trailer + FONT_HEADER_SIZE;
    input[glyph] = b'A';
    input[glyph + 6..glyph + 8].copy_from_slice(&1i16.to_le_bytes());
    input[glyph + 8..glyph + 10].copy_from_slice(&1i16.to_le_bytes());
    input
}

fn footer_table() -> IndexedTextTable {
    let mut bytes = Vec::new();
    for index in 0..=ROOT_FOOTER_TEXT_INDEX {
        bytes.extend_from_slice(if index == ROOT_FOOTER_TEXT_INDEX {
            b"A"
        } else {
            b"unused"
        });
        if index != ROOT_FOOTER_TEXT_INDEX {
            bytes.push(b'\n');
        }
    }
    IndexedTextTable::parse(&bytes, ROOT_FOOTER_TEXT_INDEX + 1).unwrap()
}

fn drawable_root_c3d(bitmap: &[u8]) -> Vec<u8> {
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
    let vertices: [[i32; 8]; 3] = [
        [-(4 << 16), -(3 << 16), 0, 0, -(1 << 16), 0, 0, 0],
        [-(4 << 16), 3 << 16, 0, 0, -(1 << 16), 0, 0, 0],
        [4 << 16, -(3 << 16), 0, 0, -(1 << 16), 0, 0, 0],
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
    input[blob_start..blob_start + 12].copy_from_slice(b"FRAME-TEX!!!");
    input[blob_start + 12..blob_start + 16].copy_from_slice(&(bitmap.len() as u32).to_le_bytes());
    input[blob_start + 16..].copy_from_slice(bitmap);
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
