use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use cmr_assets::{
    AssetFile, AssetProfile, AssetStore, AssetStoreError, ImportSummary, ReviewedProfile,
    StorageEncoding, import_source,
};
use cmr_core::root_menu_assets::{RootMenuObjectIdentity, VisualVariant};
use cmr_core::root_menu_c3d_assets::{
    ROOT_MENU_C3D_REQUEST_COUNT, RootMenuC3dLoadError, RootMenuC3dSlot, load_root_menu_c3d,
};
use cmr_formats::c3d::{C3D_FORMAT_VERSION, C3D_HEADER_LEN, C3D_MAGIC, C3dError, C3dLimits};
use cmr_formats::stored_asset::{DecodeError, DecodeLimits};
use sha2::{Digest, Sha256};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("cmr-root-menu-c3d-{}-{id}", std::process::id()));
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
fn loads_all_six_objects_in_exact_selection_order() {
    let variant = VisualVariant::Normal;
    let fixture = Fixture::new(&complete_files(variant));

    let loaded = load_root_menu_c3d(&fixture.store, variant, C3dLimits::default());
    assert_eq!(loaded.variant(), variant);
    assert_eq!(loaded.loaded_len(), ROOT_MENU_C3D_REQUEST_COUNT);
    assert_eq!(loaded.diagnostic_len(), 0);
    assert_eq!(
        loaded
            .slots()
            .map(|(slot, asset)| (
                slot,
                asset.unwrap().identity,
                asset.unwrap().path,
                asset.unwrap().file.header().unknown_08[0],
            ))
            .collect::<Vec<_>>(),
        RootMenuC3dSlot::ALL
            .into_iter()
            .enumerate()
            .map(|(index, slot)| (
                slot,
                RootMenuObjectIdentity::ALL[index],
                RootMenuObjectIdentity::ALL[index].path(variant),
                index as u8,
            ))
            .collect::<Vec<_>>()
    );
}

#[test]
fn g_prefixed_variant_uses_its_six_distinct_paths() {
    let variant = VisualVariant::GPrefixed;
    let fixture = Fixture::new(&complete_files(variant));

    let loaded = load_root_menu_c3d(&fixture.store, variant, C3dLimits::default());
    assert_eq!(
        loaded.iter().map(|asset| asset.path).collect::<Vec<_>>(),
        RootMenuObjectIdentity::ALL
            .map(|identity| identity.path(variant))
            .to_vec()
    );
    for slot in RootMenuC3dSlot::ALL {
        assert_eq!(loaded.get(slot).unwrap().slot, slot);
        assert!(loaded.get(slot).unwrap().path.contains("/g_"));
    }
}

#[test]
fn missing_and_malformed_objects_leave_fixed_holes_and_loading_continues() {
    let variant = VisualVariant::Normal;
    let mut files = complete_files(variant);
    files.remove(RootMenuC3dSlot::Rally.path(variant));
    files
        .get_mut(RootMenuC3dSlot::Options.path(variant))
        .unwrap()[0..4]
        .copy_from_slice(b"NOPE");
    let fixture = Fixture::new(&files);

    let loaded = load_root_menu_c3d(&fixture.store, variant, C3dLimits::default());
    assert_eq!(loaded.loaded_len(), ROOT_MENU_C3D_REQUEST_COUNT - 2);
    assert!(loaded.get(RootMenuC3dSlot::Championship).is_some());
    assert!(loaded.get(RootMenuC3dSlot::Rally).is_none());
    assert!(loaded.get(RootMenuC3dSlot::Options).is_none());
    assert!(loaded.get(RootMenuC3dSlot::Load).is_some());

    let diagnostics = loaded.diagnostics().collect::<Vec<_>>();
    assert_eq!(diagnostics.len(), 2);
    assert!(matches!(
        diagnostics[0],
        RootMenuC3dLoadError::Asset {
            slot: RootMenuC3dSlot::Rally,
            identity: RootMenuObjectIdentity::Rally,
            path,
            ..
        } if *path == RootMenuC3dSlot::Rally.path(variant)
    ));
    assert!(matches!(
        diagnostics[1],
        RootMenuC3dLoadError::Decode {
            slot: RootMenuC3dSlot::Options,
            identity: RootMenuObjectIdentity::Options,
            path,
            source: C3dError::InvalidMagic(actual),
        } if *path == RootMenuC3dSlot::Options.path(variant) && actual == b"NOPE"
    ));
    assert_eq!(diagnostics[0].slot(), RootMenuC3dSlot::Rally);
    assert_eq!(diagnostics[1].identity(), RootMenuObjectIdentity::Options);
    assert_eq!(
        diagnostics[1].path(),
        RootMenuC3dSlot::Options.path(variant)
    );
}

#[test]
fn corrupt_wrapped_object_is_an_asset_diagnostic_and_later_slots_still_load() {
    let variant = VisualVariant::Normal;
    let mut files = complete_files(variant);
    let corrupt_slot = RootMenuC3dSlot::TimeTrial;
    files.insert(corrupt_slot.path(variant), vec![0x1f, 0x8b, 0x08, 0x00]);
    let fixture = Fixture::new_unchecked_import(&files, corrupt_slot.path(variant));

    let loaded = load_root_menu_c3d(&fixture.store, variant, C3dLimits::default());

    assert_eq!(loaded.loaded_len(), ROOT_MENU_C3D_REQUEST_COUNT - 1);
    assert!(loaded.get(corrupt_slot).is_none());
    assert!(loaded.get(RootMenuC3dSlot::RallySchool).is_some());
    assert!(loaded.get(RootMenuC3dSlot::Load).is_some());
    let diagnostics = loaded.diagnostics().collect::<Vec<_>>();
    assert_eq!(diagnostics.len(), 1);
    assert!(matches!(
        diagnostics[0],
        RootMenuC3dLoadError::Asset {
            slot: RootMenuC3dSlot::TimeTrial,
            identity: RootMenuObjectIdentity::TimeTrial,
            path,
            source: AssetStoreError::Decode {
                source: DecodeError::Gzip(_),
                ..
            },
        } if *path == corrupt_slot.path(variant)
    ));
}

#[test]
fn all_six_optional_failures_are_retained_in_request_order() {
    let fixture = Fixture::new(&BTreeMap::new());

    let loaded = load_root_menu_c3d(&fixture.store, VisualVariant::Normal, C3dLimits::default());
    assert_eq!(loaded.loaded_len(), 0);
    assert_eq!(loaded.diagnostic_len(), ROOT_MENU_C3D_REQUEST_COUNT);
    assert!(loaded.slots().all(|(_, asset)| asset.is_none()));
    assert_eq!(
        loaded
            .diagnostics()
            .map(RootMenuC3dLoadError::slot)
            .collect::<Vec<_>>(),
        RootMenuC3dSlot::ALL
    );
}

#[test]
fn parser_limit_rejects_each_object_without_compacting_slots() {
    let variant = VisualVariant::Normal;
    let fixture = Fixture::new(&complete_files(variant));

    let loaded = load_root_menu_c3d(
        &fixture.store,
        variant,
        C3dLimits {
            max_payload_bytes: C3D_HEADER_LEN - 1,
            ..C3dLimits::default()
        },
    );
    assert_eq!(loaded.loaded_len(), 0);
    assert_eq!(loaded.diagnostic_len(), ROOT_MENU_C3D_REQUEST_COUNT);
    for (index, diagnostic) in loaded.diagnostics().enumerate() {
        assert_eq!(diagnostic.slot(), RootMenuC3dSlot::ALL[index]);
        assert!(matches!(
            diagnostic,
            RootMenuC3dLoadError::Decode {
                source: C3dError::PayloadTooLarge {
                    actual: 64,
                    limit: 31,
                },
                ..
            }
        ));
    }
}

struct Fixture {
    _temporary: TestDirectory,
    store: AssetStore,
}

impl Fixture {
    fn new(files: &BTreeMap<&'static str, Vec<u8>>) -> Self {
        let temporary = TestDirectory::new();
        let source = temporary.0.join("source");
        fs::create_dir_all(source.join("New_Fe")).unwrap();

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
            id: "synthetic-root-menu-c3d".to_owned(),
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

    /// Builds a self-consistent import record around bytes that the real
    /// importer would reject. This isolates the load-time storage failure
    /// boundary without weakening importer verification in production code.
    fn new_unchecked_import(
        files: &BTreeMap<&'static str, Vec<u8>>,
        wrapped_path: &'static str,
    ) -> Self {
        let temporary = TestDirectory::new();
        let imported = temporary.0.join("runtime-data");
        fs::create_dir(&imported).unwrap();

        let mut profile_files = Vec::with_capacity(files.len());
        let mut stored_bytes = 0u64;
        for (path, bytes) in files {
            let destination = imported.join(path);
            fs::create_dir_all(destination.parent().unwrap()).unwrap();
            fs::write(&destination, bytes).unwrap();
            stored_bytes += bytes.len() as u64;
            let mut file = asset(path, bytes);
            if *path == wrapped_path {
                file.encoding = StorageEncoding::GzipCmpr;
            }
            profile_files.push(file);
        }
        profile_files.sort_by(|left, right| left.path.cmp(&right.path));

        let profile = AssetProfile {
            schema_version: 1,
            id: "synthetic-root-menu-c3d-corrupt-wrapper".to_owned(),
            roots: vec!["New_Fe".to_owned()],
            empty_directories: Vec::new(),
            sentinels: Vec::new(),
            files: profile_files,
        };
        let reviewed = ReviewedProfile::from_json(&serde_json::to_vec(&profile).unwrap()).unwrap();
        let state = ImportSummary {
            schema_version: 1,
            profile: profile.id.clone(),
            profile_sha256: reviewed.sha256().to_owned(),
            files: files.len() as u64,
            stored_bytes,
        };
        fs::write(
            imported.join("import-state.json"),
            serde_json::to_vec(&state).unwrap(),
        )
        .unwrap();
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

fn complete_files(variant: VisualVariant) -> BTreeMap<&'static str, Vec<u8>> {
    RootMenuC3dSlot::ALL
        .into_iter()
        .enumerate()
        .map(|(index, slot)| (slot.path(variant), synthetic_c3d(index as u8)))
        .collect()
}

fn synthetic_c3d(marker: u8) -> Vec<u8> {
    let mut input = vec![0; 64];
    input[0..4].copy_from_slice(&C3D_MAGIC);
    input[4..8].copy_from_slice(&C3D_FORMAT_VERSION.to_le_bytes());
    input[8..12].copy_from_slice(&[marker, 0xa1, 0xb2, 0xc3]);
    for (index, count) in [1u16, 1, 1].into_iter().enumerate() {
        let start = 12 + index * 2;
        input[start..start + 2].copy_from_slice(&count.to_le_bytes());
    }
    for (index, offset) in [32u32, 40, 48].into_iter().enumerate() {
        let start = 20 + index * 4;
        input[start..start + 4].copy_from_slice(&offset.to_le_bytes());
    }
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
