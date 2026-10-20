use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use cmr_assets::{
    AssetFile, AssetProfile, AssetStore, ReviewedProfile, StorageEncoding, import_source,
};
use cmr_core::blit::BlitPlan;
use cmr_core::championship_frame_assets::{
    CHEQUERED_FRAME_COUNT, CHEQUERED_FRAME_DESCRIPTORS, CHEQUERED_PALETTE_DESCRIPTOR,
    ChampionshipFrameAssetLoadError, ChampionshipFrameAssetRole, ChampionshipFrameUploadError,
    ChequeredFrame, INFO_BOX_DESCRIPTOR, load_championship_frame_assets,
    prepare_championship_frame_surfaces, upload_championship_frame_surfaces,
};
use cmr_core::display::{
    CooperativeMode, DisplaySurfaces, IndexedMode, IndexedVideoBackend, SurfaceTopology,
};
use cmr_core::source_store::{SourceUploadError, UploadedSourceStore};
use cmr_core::surface::{IndexedSurface, PaletteMapping};
use cmr_formats::bitmap::{BitmapError, BitmapLimits};
use cmr_formats::riff_palette::{LegacyPalette256, PaletteEntry, PaletteError};
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
            "cmr-championship-frame-assets-{}-{id}",
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
fn descriptors_preserve_recovered_paths_and_numeric_order() {
    assert_eq!(CHEQUERED_FRAME_COUNT, 15);
    assert_eq!(INFO_BOX_DESCRIPTOR.path, "New_Fe/Images/Infobox/r_info.bmp");
    assert_eq!(
        CHEQUERED_PALETTE_DESCRIPTOR.path,
        "New_Fe/Palettes/Flags/fche.pal"
    );
    assert_eq!(
        CHEQUERED_FRAME_DESCRIPTORS.map(|value| value.frame),
        ChequeredFrame::ALL
    );
    for (index, descriptor) in CHEQUERED_FRAME_DESCRIPTORS.into_iter().enumerate() {
        assert_eq!(
            descriptor.path,
            format!("New_Fe/Images/flag/cheq/cheq_{:03}.bmp", index + 1)
        );
        assert_eq!(ChequeredFrame::try_from(index), Ok(descriptor.frame));
    }
    assert_eq!(ChequeredFrame::try_from(15), Err(15));
}

#[test]
fn complete_store_loads_info_frames_and_palette() {
    let fixture = Fixture::new(&complete_files());
    let assets = load_championship_frame_assets(&fixture.store, BitmapLimits::default()).unwrap();

    assert_eq!((assets.info_box.width, assets.info_box.height), (320, 132));
    assert_eq!(assets.info_box.row_stride, 320);
    assert_eq!(assets.frames().len(), 15);
    for (index, frame) in ChequeredFrame::ALL.into_iter().enumerate() {
        let asset = assets.frame(frame);
        assert_eq!(asset.descriptor, CHEQUERED_FRAME_DESCRIPTORS[index]);
        assert_eq!((asset.bitmap.width, asset.bitmap.height), (182, 128));
        assert_eq!(asset.bitmap.row_stride, 184);
        assert_eq!(asset.bitmap.pixels[0], 20 + index as u8);
    }
    assert_eq!(assets.chequered_palette.entries[1].red, 77);
}

#[test]
fn missing_asset_retains_exact_role_and_path() {
    let descriptor = ChequeredFrame::Frame08.descriptor();
    let mut files = complete_files();
    files.remove(descriptor.path);
    let fixture = Fixture::new(&files);

    let error =
        load_championship_frame_assets(&fixture.store, BitmapLimits::default()).unwrap_err();
    assert!(matches!(
        error,
        ChampionshipFrameAssetLoadError::Asset {
            role: ChampionshipFrameAssetRole::ChequeredFrame(ChequeredFrame::Frame08),
            path,
            ..
        } if path == descriptor.path
    ));
    assert_eq!(
        error.role(),
        ChampionshipFrameAssetRole::ChequeredFrame(ChequeredFrame::Frame08)
    );
    assert_eq!(error.path(), descriptor.path);
}

#[test]
fn malformed_truncated_and_palette_errors_retain_boundaries() {
    let mut malformed = complete_files();
    malformed.get_mut(INFO_BOX_DESCRIPTOR.path).unwrap()[0..2].copy_from_slice(b"NO");
    let fixture = Fixture::new(&malformed);
    assert!(matches!(
        load_championship_frame_assets(&fixture.store, BitmapLimits::default()),
        Err(ChampionshipFrameAssetLoadError::Bitmap {
            role: ChampionshipFrameAssetRole::InfoBox,
            path,
            source: BitmapError::NotBitmap,
        }) if path == INFO_BOX_DESCRIPTOR.path
    ));

    let descriptor = ChequeredFrame::Frame04.descriptor();
    let mut truncated = complete_files();
    truncated.insert(descriptor.path, b"BM".to_vec());
    let fixture = Fixture::new(&truncated);
    assert!(matches!(
        load_championship_frame_assets(&fixture.store, BitmapLimits::default()),
        Err(ChampionshipFrameAssetLoadError::Bitmap {
            role: ChampionshipFrameAssetRole::ChequeredFrame(ChequeredFrame::Frame04),
            path,
            source: BitmapError::Truncated,
        }) if path == descriptor.path
    ));

    let mut bad_palette = complete_files();
    bad_palette.insert(CHEQUERED_PALETTE_DESCRIPTOR.path, b"NOPE".to_vec());
    let fixture = Fixture::new(&bad_palette);
    assert!(matches!(
        load_championship_frame_assets(&fixture.store, BitmapLimits::default()),
        Err(ChampionshipFrameAssetLoadError::Palette {
            path,
            source: PaletteError::Truncated,
        }) if path == CHEQUERED_PALETTE_DESCRIPTOR.path
    ));
}

#[test]
fn caller_bitmap_limits_apply_to_info_and_frames() {
    let fixture = Fixture::new(&complete_files());
    let mut frame_limited_files = complete_files();
    frame_limited_files.insert(INFO_BOX_DESCRIPTOR.path, synthetic_bitmap(1, 1, 9, false));
    let frame_limited_fixture = Fixture::new(&frame_limited_files);
    assert!(matches!(
        load_championship_frame_assets(
            &fixture.store,
            BitmapLimits {
                max_width: 319,
                ..BitmapLimits::default()
            }
        ),
        Err(ChampionshipFrameAssetLoadError::Bitmap {
            role: ChampionshipFrameAssetRole::InfoBox,
            source: BitmapError::LimitsExceeded,
            ..
        })
    ));

    assert!(matches!(
        load_championship_frame_assets(
            &frame_limited_fixture.store,
            BitmapLimits {
                max_width: 320,
                max_height: 127,
                ..BitmapLimits::default()
            }
        ),
        Err(ChampionshipFrameAssetLoadError::Bitmap {
            role: ChampionshipFrameAssetRole::ChequeredFrame(ChequeredFrame::Frame01),
            source: BitmapError::LimitsExceeded,
            ..
        })
    ));
}

#[test]
#[ignore = "requires a user-owned import selected with CMR_RUNTIME_DATA"]
fn reviewed_import_loads_championship_frame_assets() {
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

    let assets = load_championship_frame_assets(&store, BitmapLimits::default())
        .expect("reviewed import contains championship frame assets");
    assert_eq!((assets.info_box.width, assets.info_box.height), (320, 132));
    assert!(assets.frames().all(|asset| {
        asset.bitmap.width == 182 && asset.bitmap.height == 128 && asset.bitmap.row_stride == 184
    }));
    let prepared = prepare_championship_frame_surfaces(
        &assets,
        &assets.chequered_palette,
        PaletteMapping::Identity,
    )
    .expect("reviewed bitmaps prepare as indexed surfaces");
    assert_eq!(prepared.info_box.surface.width, 320);
    assert!(prepared.frames().all(|asset| {
        (
            asset.surface.width,
            asset.surface.height,
            asset.surface.pitch,
        ) == (184, 128, 184)
    }));
}

#[test]
fn preparation_uses_explicit_info_mapping_and_attaches_fche_to_aligned_frames() {
    let fixture = Fixture::new(&complete_files());
    let assets = load_championship_frame_assets(&fixture.store, BitmapLimits::default()).unwrap();
    let active = active_palette();

    let identity =
        prepare_championship_frame_surfaces(&assets, &active, PaletteMapping::Identity).unwrap();
    let remapped =
        prepare_championship_frame_surfaces(&assets, &active, PaletteMapping::NearestNonzero)
            .unwrap();
    assert_eq!(identity.info_box.surface.pixels[0], 9);
    assert_eq!(remapped.info_box.surface.pixels[0], 42);
    assert_eq!(remapped.info_box.surface.palette, active.entries);

    for frame in ChequeredFrame::ALL {
        let surface = &identity.frame(frame).surface;
        assert_eq!(
            (surface.width, surface.height, surface.pitch),
            (184, 128, 184)
        );
        assert_eq!(&surface.pixels[182..184], &[201, 201]);
        assert_eq!(surface.palette, assets.chequered_palette.entries);
    }
}

#[test]
fn upload_commits_info_and_frames_and_coexists_with_other_sources() {
    let fixture = Fixture::new(&complete_files());
    let assets = load_championship_frame_assets(&fixture.store, BitmapLimits::default()).unwrap();
    let prepared =
        prepare_championship_frame_surfaces(&assets, &active_palette(), PaletteMapping::Identity)
            .unwrap();
    let mut backend = RecordingBackend::default();
    {
        let mut store = UploadedSourceStore::new(&mut backend);
        let before = store.upload_bitmap(&base_surface(90)).unwrap();
        let pool = upload_championship_frame_surfaces(&mut store, prepared).unwrap();
        let after = store.upload_bitmap(&base_surface(91)).unwrap();
        assert_eq!(store.uploaded_source_count(), 18);
        assert_eq!(pool.info_box().extent(), (320, 132));
        assert_eq!(pool.frame(ChequeredFrame::Frame01).extent(), (184, 128));
        assert_eq!(pool.frames().len(), 15);
        assert_ne!(before.source_key(), pool.info_box().source_key());
        assert_ne!(
            after.source_key(),
            pool.frame(ChequeredFrame::Frame15).source_key()
        );
    }
    assert_eq!(backend.created, 18);
}

#[test]
fn upload_failure_rolls_back_only_the_frame_batch_with_role_and_path() {
    let fixture = Fixture::new(&complete_files());
    let assets = load_championship_frame_assets(&fixture.store, BitmapLimits::default()).unwrap();
    let prepared =
        prepare_championship_frame_surfaces(&assets, &active_palette(), PaletteMapping::Identity)
            .unwrap();
    let mut backend = RecordingBackend {
        fail_at: Some(5),
        ..RecordingBackend::default()
    };
    {
        let mut store = UploadedSourceStore::new(&mut backend);
        store.upload(&base_surface(90)).unwrap();
        let error = upload_championship_frame_surfaces(&mut store, prepared).unwrap_err();
        assert_eq!(
            error,
            ChampionshipFrameUploadError {
                role: ChampionshipFrameAssetRole::ChequeredFrame(ChequeredFrame::Frame04),
                path: ChequeredFrame::Frame04.descriptor().path,
                source: SourceUploadError::Backend(BackendError::Create(5)),
            }
        );
        assert_eq!(store.uploaded_source_count(), 1);
        store.upload(&base_surface(91)).unwrap();
        assert_eq!(store.uploaded_source_count(), 2);
    }
    assert_eq!(backend.destroyed, vec![4, 3, 2, 1, 6, 0]);
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
            id: "synthetic-championship-frame-assets".to_owned(),
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
                max_stored_bytes: 512 * 1024,
                max_payload_bytes: 512 * 1024,
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
    files.insert(
        INFO_BOX_DESCRIPTOR.path,
        synthetic_bitmap(320, 132, 9, false),
    );
    for (index, descriptor) in CHEQUERED_FRAME_DESCRIPTORS.into_iter().enumerate() {
        files.insert(
            descriptor.path,
            synthetic_bitmap(182, 128, 20 + index as u8, true),
        );
    }
    files.insert(CHEQUERED_PALETTE_DESCRIPTOR.path, synthetic_palette());
    files
}

fn synthetic_bitmap(width: u32, height: u32, pixel: u8, frame_palette: bool) -> Vec<u8> {
    let palette_entries = 256usize;
    let stride = width.div_ceil(4) * 4;
    let image_size = stride * height;
    let pixel_offset = 14 + 40 + palette_entries * 4;
    let file_size = pixel_offset + image_size as usize;
    let mut input = vec![0; file_size];
    input[0..2].copy_from_slice(b"BM");
    input[2..6].copy_from_slice(&(file_size as u32).to_le_bytes());
    input[10..14].copy_from_slice(&(pixel_offset as u32).to_le_bytes());
    input[14..18].copy_from_slice(&40u32.to_le_bytes());
    input[18..22].copy_from_slice(&(width as i32).to_le_bytes());
    input[22..26].copy_from_slice(&(height as i32).to_le_bytes());
    input[26..28].copy_from_slice(&1u16.to_le_bytes());
    input[28..30].copy_from_slice(&8u16.to_le_bytes());
    input[34..38].copy_from_slice(&image_size.to_le_bytes());
    input[46..50].copy_from_slice(&256u32.to_le_bytes());
    if frame_palette {
        input[54 + 4] = 0;
        input[54 + 5] = 0;
        input[54 + 6] = 77;
    } else {
        input[54 + pixel as usize * 4 + 2] = 10;
    }
    for row in input[pixel_offset..].chunks_exact_mut(stride as usize) {
        row[..width as usize].fill(pixel);
        row[width as usize..].fill(201);
    }
    input
}

fn active_palette() -> LegacyPalette256 {
    LegacyPalette256 {
        entries: std::array::from_fn(|index| PaletteEntry {
            red: if index == 42 { 10 } else { 0 },
            green: 0,
            blue: 0,
            flags: 0,
        }),
    }
}

fn base_surface(marker: u8) -> IndexedSurface {
    IndexedSurface {
        width: 1,
        height: 1,
        pitch: 1,
        pixels: vec![marker],
        palette: active_palette().entries,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BackendError {
    Create(usize),
}

impl std::fmt::Display for BackendError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for BackendError {}

#[derive(Default)]
struct RecordingBackend {
    created: usize,
    destroyed: Vec<u8>,
    fail_at: Option<usize>,
}

impl IndexedVideoBackend for RecordingBackend {
    type Error = BackendError;
    type Surface = u8;
    type Palette = u8;

    fn create_device(&mut self) -> Result<(), Self::Error> {
        unreachable!()
    }
    fn set_cooperative_mode(&mut self, _: CooperativeMode) -> Result<(), Self::Error> {
        unreachable!()
    }
    fn set_display_mode(&mut self, _: IndexedMode) -> Result<(), Self::Error> {
        unreachable!()
    }
    fn create_surfaces(
        &mut self,
        _: SurfaceTopology,
        _: IndexedMode,
    ) -> Result<DisplaySurfaces<Self::Surface>, Self::Error> {
        unreachable!()
    }
    fn create_palette(&mut self, _: &[PaletteEntry; 256]) -> Result<Self::Palette, Self::Error> {
        unreachable!()
    }
    fn attach_palette(&mut self, _: Self::Surface, _: Self::Palette) -> Result<(), Self::Error> {
        unreachable!()
    }
    fn upload_palette(
        &mut self,
        _: Self::Palette,
        _: &[PaletteEntry; 256],
    ) -> Result<(), Self::Error> {
        unreachable!()
    }

    fn create_source(&mut self, _: &IndexedSurface) -> Result<Self::Surface, Self::Error> {
        let attempt = self.created;
        self.created += 1;
        if self.fail_at == Some(attempt) {
            return Err(BackendError::Create(attempt));
        }
        Ok(attempt as u8)
    }

    fn blit(&mut self, _: Self::Surface, _: Self::Surface, _: BlitPlan) -> Result<(), Self::Error> {
        unreachable!()
    }

    fn destroy_source(&mut self, source: Self::Surface) {
        self.destroyed.push(source);
    }
}

fn synthetic_palette() -> Vec<u8> {
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
    input[28] = 77;
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
