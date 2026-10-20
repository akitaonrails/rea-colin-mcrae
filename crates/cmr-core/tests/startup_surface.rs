use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use cmr_assets::{
    AssetFile, AssetProfile, AssetStore, ReviewedProfile, StorageEncoding, import_source,
};
use cmr_core::blit::BlitPlan;
use cmr_core::display::{
    CooperativeMode, DisplayExtent, DisplaySurfaces, IndexedMode, IndexedVideoBackend,
    SurfaceTopology,
};
use cmr_core::startup::{StartupLanguage, StartupScreen};
use cmr_core::startup_display::{
    StartupDisplayError, StartupDisplayRequest, initialize_startup_display,
};
use cmr_core::startup_surface::{StartupSurfaceError, load_startup_surface};
use cmr_core::surface::PaletteMapping;
use cmr_formats::bitmap::BitmapLimits;
use cmr_formats::stored_asset::DecodeLimits;
use sha2::{Digest, Sha256};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("cmr-startup-surface-{}-{id}", std::process::id()));
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
fn verified_import_decodes_and_maps_a_startup_surface_end_to_end() {
    let temporary = TestDirectory::new();
    let source = temporary.0.join("source");
    fs::create_dir(&source).unwrap();
    fs::create_dir(source.join("ini")).unwrap();

    let mut embedded = vec![[200, 200, 200, 0]; 256];
    embedded[0] = [255, 255, 255, 0];
    embedded[1] = [10, 10, 10, 0];
    embedded[2] = [30, 40, 50, 0];
    embedded[3] = [250, 250, 250, 0];
    let bitmap = bitmap(&embedded, &[0, 1, 2, 3]);

    let mut active = vec![[100, 100, 100, 0]; 256];
    active[0] = [255, 255, 255, 0];
    active[1] = [0, 10, 10, 0];
    active[2] = [20, 10, 10, 0];
    active[3] = [30, 40, 50, 0];
    active[4] = [250, 250, 250, 0];
    let palette = palette(&active);
    let malformed_legal_palette = b"not a RIFF palette";

    fs::write(source.join("ini/legal.pal"), malformed_legal_palette).unwrap();
    fs::write(source.join("ini/loading.bmp"), &bitmap).unwrap();
    fs::write(source.join("ini/loading.pal"), &palette).unwrap();
    let profile = AssetProfile {
        schema_version: 1,
        id: "synthetic-startup".to_owned(),
        roots: vec!["ini".to_owned()],
        empty_directories: Vec::new(),
        sentinels: Vec::new(),
        files: vec![
            asset("ini/legal.pal", malformed_legal_palette),
            asset("ini/loading.bmp", &bitmap),
            asset("ini/loading.pal", &palette),
        ],
    };
    let reviewed = ReviewedProfile::from_json(&serde_json::to_vec(&profile).unwrap()).unwrap();
    let imported = temporary.0.join("runtime-data");
    import_source(&source, &imported, &reviewed).unwrap();
    let store = AssetStore::open(
        &imported,
        &reviewed,
        DecodeLimits {
            max_stored_bytes: 2048,
            max_payload_bytes: 2048,
        },
    )
    .unwrap();

    let surface = load_startup_surface(
        &store,
        StartupScreen::Loading,
        StartupLanguage::English,
        PaletteMapping::NearestNonzero,
        BitmapLimits::default(),
    )
    .unwrap();

    assert_eq!((surface.width, surface.height, surface.pitch), (4, 1, 4));
    assert_eq!(surface.pixels, [0, 1, 3, 4]);
    assert_eq!(surface.palette[3].red, 30);
    assert_eq!(surface.palette[3].green, 40);
    assert_eq!(surface.palette[3].blue, 50);
    assert_eq!(surface.palette[3].flags, 0);

    assert!(matches!(
        load_startup_surface(
            &store,
            StartupScreen::Legal,
            StartupLanguage::English,
            PaletteMapping::NearestNonzero,
            BitmapLimits::default(),
        ),
        Err(StartupSurfaceError::Palette { path, .. }) if path == "ini/legal.pal"
    ));

    let startup = initialize_startup_display(
        &store,
        RecordingBackend::default(),
        StartupDisplayRequest {
            mode: IndexedMode {
                extent: DisplayExtent {
                    width: 4,
                    height: 1,
                },
                bits_per_pixel: 8,
            },
            cooperative: CooperativeMode::Windowed,
            screen: StartupScreen::Loading,
            language: StartupLanguage::English,
            mapping: PaletteMapping::NearestNonzero,
            bitmap_limits: BitmapLimits::default(),
        },
    )
    .unwrap();
    assert_eq!(startup.copy.front, Ok(()));
    assert_eq!(startup.copy.back, Ok(()));
    assert_eq!(
        startup.display.into_backend().events,
        [
            "device",
            "cooperative",
            "surfaces",
            "create-palette",
            "attach-front",
            "attach-back",
            "upload",
            "upload",
            "source",
            "blit-front",
            "blit-back",
            "destroy-source",
        ]
    );

    fs::remove_file(imported.join("ini/loading.bmp")).unwrap();
    let error = match initialize_startup_display(
        &store,
        RecordingBackend {
            fail_second_upload: true,
            ..RecordingBackend::default()
        },
        StartupDisplayRequest {
            mode: IndexedMode {
                extent: DisplayExtent {
                    width: 4,
                    height: 1,
                },
                bits_per_pixel: 8,
            },
            cooperative: CooperativeMode::Windowed,
            screen: StartupScreen::Loading,
            language: StartupLanguage::English,
            mapping: PaletteMapping::NearestNonzero,
            bitmap_limits: BitmapLimits::default(),
        },
    ) {
        Ok(_) => panic!("expected second palette upload to fail"),
        Err(error) => error,
    };
    assert!(matches!(error, StartupDisplayError::ApplyPalette(_)));
}

#[derive(Default)]
struct RecordingBackend {
    events: Vec<&'static str>,
    uploads: usize,
    fail_second_upload: bool,
}

impl IndexedVideoBackend for RecordingBackend {
    type Error = &'static str;
    type Surface = u8;
    type Palette = u8;

    fn create_device(&mut self) -> Result<(), Self::Error> {
        self.events.push("device");
        Ok(())
    }

    fn set_cooperative_mode(&mut self, _mode: CooperativeMode) -> Result<(), Self::Error> {
        self.events.push("cooperative");
        Ok(())
    }

    fn set_display_mode(&mut self, _mode: IndexedMode) -> Result<(), Self::Error> {
        self.events.push("mode");
        Ok(())
    }

    fn create_surfaces(
        &mut self,
        _topology: SurfaceTopology,
        _mode: IndexedMode,
    ) -> Result<DisplaySurfaces<Self::Surface>, Self::Error> {
        self.events.push("surfaces");
        Ok(DisplaySurfaces {
            visible_front: 1,
            back: 2,
        })
    }

    fn create_palette(
        &mut self,
        _entries: &[cmr_formats::riff_palette::PaletteEntry; 256],
    ) -> Result<Self::Palette, Self::Error> {
        self.events.push("create-palette");
        Ok(9)
    }

    fn attach_palette(
        &mut self,
        surface: Self::Surface,
        _palette: Self::Palette,
    ) -> Result<(), Self::Error> {
        self.events.push(if surface == 1 {
            "attach-front"
        } else {
            "attach-back"
        });
        Ok(())
    }

    fn upload_palette(
        &mut self,
        _palette: Self::Palette,
        _entries: &[cmr_formats::riff_palette::PaletteEntry; 256],
    ) -> Result<(), Self::Error> {
        self.events.push("upload");
        self.uploads += 1;
        if self.fail_second_upload && self.uploads == 2 {
            Err("second upload failed")
        } else {
            Ok(())
        }
    }

    fn create_source(
        &mut self,
        _surface: &cmr_core::surface::IndexedSurface,
    ) -> Result<Self::Surface, Self::Error> {
        self.events.push("source");
        Ok(3)
    }

    fn blit(
        &mut self,
        _source: Self::Surface,
        destination: Self::Surface,
        _plan: BlitPlan,
    ) -> Result<(), Self::Error> {
        self.events.push(if destination == 1 {
            "blit-front"
        } else {
            "blit-back"
        });
        Ok(())
    }

    fn destroy_source(&mut self, _source: Self::Surface) {
        self.events.push("destroy-source");
    }
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
    let mut result = String::with_capacity(64);
    for byte in digest {
        write!(result, "{byte:02x}").unwrap();
    }
    result
}

fn bitmap(colors: &[[u8; 4]], pixels: &[u8]) -> Vec<u8> {
    let pixel_offset = 14 + 40 + colors.len() * 4;
    let file_size = pixel_offset + pixels.len();
    let mut result = vec![0u8; pixel_offset];
    result[0..2].copy_from_slice(b"BM");
    result[2..6].copy_from_slice(&(file_size as u32).to_le_bytes());
    result[10..14].copy_from_slice(&(pixel_offset as u32).to_le_bytes());
    result[14..18].copy_from_slice(&40u32.to_le_bytes());
    result[18..22].copy_from_slice(&4i32.to_le_bytes());
    result[22..26].copy_from_slice(&1i32.to_le_bytes());
    result[26..28].copy_from_slice(&1u16.to_le_bytes());
    result[28..30].copy_from_slice(&8u16.to_le_bytes());
    result[34..38].copy_from_slice(&(pixels.len() as u32).to_le_bytes());
    result[46..50].copy_from_slice(&(colors.len() as u32).to_le_bytes());
    for (index, [red, green, blue, reserved]) in colors.iter().copied().enumerate() {
        let start = 54 + index * 4;
        result[start..start + 4].copy_from_slice(&[blue, green, red, reserved]);
    }
    result.extend_from_slice(pixels);
    result
}

fn palette(colors: &[[u8; 4]]) -> Vec<u8> {
    let data_size = 4 + colors.len() * 4;
    let riff_size = 4 + 8 + data_size;
    let mut result = Vec::new();
    result.extend_from_slice(b"RIFF");
    result.extend_from_slice(&(riff_size as u32).to_le_bytes());
    result.extend_from_slice(b"PAL ");
    result.extend_from_slice(b"data");
    result.extend_from_slice(&(data_size as u32).to_le_bytes());
    result.extend_from_slice(&0x0300u16.to_le_bytes());
    result.extend_from_slice(&(colors.len() as u16).to_le_bytes());
    for color in colors {
        result.extend_from_slice(color);
    }
    result
}
