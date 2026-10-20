use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use cmr_assets::{
    AssetFile, AssetProfile, AssetStore, ReviewedProfile, StorageEncoding, import_source,
};
use cmr_core::localization::{FRONTEND_TEXT_COUNT, GAME_TEXT_COUNT, Language, TextTableError};
use cmr_core::localization_assets::{
    LocalizationLoadError, load_frontend_text_table, load_game_text_table,
};
use cmr_formats::stored_asset::DecodeLimits;
use sha2::{Digest, Sha256};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "cmr-localization-assets-{}-{id}",
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
fn verified_store_loads_fixed_frontend_and_game_tables() {
    let frontend = text_table(b'F', FRONTEND_TEXT_COUNT);
    let game = text_table(b'G', GAME_TEXT_COUNT);
    let fixture = Fixture::new(&[
        ("Game/Language/italian.txt", game.as_slice()),
        ("New_Fe/Language/english.txt", frontend.as_slice()),
    ]);

    let frontend_table = load_frontend_text_table(&fixture.store, Language::Italian).unwrap();
    assert_eq!(frontend_table.len(), FRONTEND_TEXT_COUNT);
    assert_eq!(frontend_table.lookup(0), b"F0");
    assert_eq!(
        frontend_table.lookup(FRONTEND_TEXT_COUNT - 1),
        format!("F{}", FRONTEND_TEXT_COUNT - 1).as_bytes()
    );

    let game_table = load_game_text_table(&fixture.store, Language::Italian).unwrap();
    assert_eq!(game_table.len(), GAME_TEXT_COUNT);
    assert_eq!(game_table.lookup(0), b"G0");
    assert_eq!(
        game_table.lookup(GAME_TEXT_COUNT - 1),
        format!("G{}", GAME_TEXT_COUNT - 1).as_bytes()
    );
}

#[test]
fn loader_errors_retain_selected_path_and_parse_context() {
    let short_frontend = b"only-one-record";
    let fixture = Fixture::new(&[("New_Fe/Language/english.txt", short_frontend.as_slice())]);

    assert!(matches!(
        load_frontend_text_table(&fixture.store, Language::Italian),
        Err(LocalizationLoadError::TextTable { path, source: TextTableError::MissingRecord {
            requested: FRONTEND_TEXT_COUNT,
            parsed: 1,
            offset,
        } }) if path == "New_Fe/Language/english.txt" && offset == short_frontend.len()
    ));

    assert!(matches!(
        load_game_text_table(&fixture.store, Language::Italian),
        Err(LocalizationLoadError::Asset { path, .. })
            if path == "Game/Language/italian.txt"
    ));
}

struct Fixture {
    _temporary: TestDirectory,
    store: AssetStore,
}

impl Fixture {
    fn new(files: &[(&str, &[u8])]) -> Self {
        let temporary = TestDirectory::new();
        let source = temporary.0.join("source");
        fs::create_dir(&source).unwrap();
        fs::create_dir(source.join("Game")).unwrap();
        fs::create_dir(source.join("New_Fe")).unwrap();

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
            id: "synthetic-localization".to_owned(),
            roots: vec!["Game".to_owned(), "New_Fe".to_owned()],
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
                max_stored_bytes: 64 * 1024,
                max_payload_bytes: 64 * 1024,
            },
        )
        .unwrap();

        Self {
            _temporary: temporary,
            store,
        }
    }
}

fn text_table(prefix: u8, records: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    for index in 0..records {
        bytes.push(prefix);
        bytes.extend_from_slice(index.to_string().as_bytes());
        if index + 1 < records {
            bytes.extend_from_slice(b"\r\n");
        }
    }
    bytes
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
