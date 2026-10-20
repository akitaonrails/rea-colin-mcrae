use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use cmr_assets::{
    AssetFile, AssetProfile, AssetStore, ReviewedProfile, StorageEncoding, import_source,
};
use cmr_core::font_assets::{FONT_COUNT, FontLoadError, FontStyle, load_font_set};
use cmr_formats::font::{FontError, FontLimits};
use cmr_formats::stored_asset::DecodeLimits;
use sha2::{Digest, Sha256};

const FIXED_PIXEL_OFFSET: usize = 0x436;
const FONT_HEADER_SIZE: usize = 24;

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("cmr-font-assets-{}-{id}", std::process::id()));
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
fn verified_store_loads_all_fonts_in_recovered_order() {
    let files = FontStyle::ALL
        .into_iter()
        .enumerate()
        .map(|(index, style)| (style.path(), synthetic_font(index as u8)))
        .collect::<Vec<_>>();
    let fixture = Fixture::new(&files);

    let fonts = load_font_set(&fixture.store, FontLimits::default()).unwrap();
    assert_eq!(fonts.len(), FONT_COUNT);
    assert!(!fonts.is_empty());
    assert_eq!(
        fonts.iter().map(|entry| entry.style).collect::<Vec<_>>(),
        FontStyle::ALL
    );
    for (index, style) in FontStyle::ALL.into_iter().enumerate() {
        assert_eq!(fonts.get(style).glyphs()[0].code, index as u8);
        assert_eq!(fonts.get(style).raw_header()[0], 0xa0 | index as u8);
    }
}

#[test]
fn loader_retains_missing_path_and_style_context() {
    let files = FontStyle::ALL
        .into_iter()
        .filter(|style| *style != FontStyle::WhiteTitle)
        .enumerate()
        .map(|(index, style)| (style.path(), synthetic_font(index as u8)))
        .collect::<Vec<_>>();
    let fixture = Fixture::new(&files);

    assert!(matches!(
        load_font_set(&fixture.store, FontLimits::default()),
        Err(FontLoadError::Asset {
            style: FontStyle::WhiteTitle,
            path: "New_Fe/Fonts/whittit.fon",
            ..
        })
    ));
}

#[test]
fn loader_retains_malformed_path_style_and_parser_context() {
    let files = FontStyle::ALL
        .into_iter()
        .enumerate()
        .map(|(index, style)| {
            let mut bytes = synthetic_font(index as u8);
            if style == FontStyle::White {
                bytes[28..30].copy_from_slice(&4u16.to_le_bytes());
            }
            (style.path(), bytes)
        })
        .collect::<Vec<_>>();
    let fixture = Fixture::new(&files);

    assert!(matches!(
        load_font_set(&fixture.store, FontLimits::default()),
        Err(FontLoadError::Parse {
            style: FontStyle::White,
            path: "New_Fe/Fonts/white.fon",
            source: FontError::UnsupportedBitDepth(4),
        })
    ));
}

#[test]
fn loader_applies_configured_font_limits_with_context() {
    let files = FontStyle::ALL
        .into_iter()
        .enumerate()
        .map(|(index, style)| (style.path(), synthetic_font(index as u8)))
        .collect::<Vec<_>>();
    let fixture = Fixture::new(&files);

    assert!(matches!(
        load_font_set(
            &fixture.store,
            FontLimits {
                max_glyphs: 0,
                ..FontLimits::default()
            }
        ),
        Err(FontLoadError::Parse {
            style: FontStyle::Yellow,
            path: "New_Fe/Fonts/yellow.fon",
            source: FontError::GlyphLimitExceeded { count: 1, limit: 0 },
        })
    ));
}

struct Fixture {
    _temporary: TestDirectory,
    store: AssetStore,
}

impl Fixture {
    fn new(files: &[(&str, Vec<u8>)]) -> Self {
        let temporary = TestDirectory::new();
        let source = temporary.0.join("source");
        fs::create_dir(&source).unwrap();

        let mut profile_files = Vec::with_capacity(files.len());
        for (path, bytes) in files {
            let portable_path = path.replace('\\', "/");
            let destination = source.join(&portable_path);
            fs::create_dir_all(destination.parent().unwrap()).unwrap();
            fs::write(&destination, bytes).unwrap();
            profile_files.push(asset(&portable_path, bytes));
        }
        profile_files.sort_by(|left, right| left.path.cmp(&right.path));

        let profile = AssetProfile {
            schema_version: 1,
            id: "synthetic-fonts".to_owned(),
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
                max_stored_bytes: 256 * 1024,
                max_payload_bytes: 256 * 1024,
            },
        )
        .unwrap();

        Self {
            _temporary: temporary,
            store,
        }
    }
}

fn synthetic_font(identity: u8) -> Vec<u8> {
    let width = 1u32;
    let height = 1u32;
    let stride = 4usize;
    let mut input = vec![0; FIXED_PIXEL_OFFSET + stride];
    input[18..22].copy_from_slice(&(width as i32).to_le_bytes());
    input[22..26].copy_from_slice(&(height as i32).to_le_bytes());
    input[28..30].copy_from_slice(&8u16.to_le_bytes());
    input[30..34].copy_from_slice(&0u32.to_le_bytes());
    input[FIXED_PIXEL_OFFSET] = identity;

    let mut header = [0; FONT_HEADER_SIZE];
    header[0] = 0xa0 | identity;
    header[1] = 1;
    header[4..8].copy_from_slice(&1u32.to_le_bytes());
    header[8..12].copy_from_slice(&1u32.to_le_bytes());
    input.extend_from_slice(&header);

    let mut glyph = [0; 16];
    glyph[0] = identity;
    glyph[6..8].copy_from_slice(&1i16.to_le_bytes());
    glyph[8..10].copy_from_slice(&1i16.to_le_bytes());
    input.extend_from_slice(&glyph);
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
