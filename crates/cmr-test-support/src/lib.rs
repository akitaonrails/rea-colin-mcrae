//! Source-owned synthetic fixtures shared by integration tests.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cmr_assets::{
    AssetFile, AssetProfile, AssetStore, ReviewedProfile, StorageEncoding, import_source,
};
use cmr_core::font_assets::FontStyle;
use cmr_core::frontend::ROOT_FOOTER_TEXT_INDEX;
use cmr_core::localization::Language;
use cmr_core::root_menu_assets::{ROOT_MENU_DISK_PALETTE_PATH, VisualVariant};
use cmr_core::root_menu_bitmap_assets::RootMenuBitmapSlot;
use cmr_core::root_menu_c3d_assets::RootMenuC3dSlot;
use cmr_formats::c3d::{
    C3D_ENTRY24_ENTRY32_ORDINAL_OFFSETS, C3D_FORMAT_VERSION, C3D_GROUP0_VALUES_OFFSET,
    C3D_GROUP1_ENTRY24_LEN, C3D_GROUP1_ENTRY32_LEN, C3D_GROUP1_FIXED_LEN, C3D_HEADER_LEN,
    C3D_MAGIC,
};
use cmr_formats::riff_palette::PaletteEntry;
use cmr_formats::stored_asset::DecodeLimits;
use sha2::{Digest, Sha256};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

const C3D_DISPLAY_INDEX: u8 = 37;

/// An imported synthetic root-menu asset set whose temporary files live for
/// exactly as long as this value.
#[derive(Debug)]
pub struct SyntheticRootFrameFixture {
    _temporary_root: TestDirectory,
    runtime_root: PathBuf,
    reviewed_profile: ReviewedProfile,
    store: AssetStore,
    disk_palette: [PaletteEntry; 256],
    expected_center_marker: u8,
}

impl SyntheticRootFrameFixture {
    pub fn new() -> Self {
        let temporary_root = TestDirectory::new();
        let source = temporary_root.0.join("source");
        let disk_palette = active_palette();
        let (reviewed_profile, expected_center_marker) =
            write_reviewed_root_assets(&source, &disk_palette);
        let runtime_root = temporary_root.0.join("runtime-data");
        import_source(&source, &runtime_root, &reviewed_profile)
            .expect("import synthetic root-frame assets");
        let store = AssetStore::open(
            &runtime_root,
            &reviewed_profile,
            DecodeLimits {
                max_stored_bytes: 4 * 1024 * 1024,
                max_payload_bytes: 4 * 1024 * 1024,
            },
        )
        .expect("open synthetic root-frame asset store");

        Self {
            _temporary_root: temporary_root,
            runtime_root,
            reviewed_profile,
            store,
            disk_palette,
            expected_center_marker,
        }
    }

    pub fn store(&self) -> &AssetStore {
        &self.store
    }

    pub fn runtime_root(&self) -> &Path {
        &self.runtime_root
    }

    pub fn reviewed_profile(&self) -> &ReviewedProfile {
        &self.reviewed_profile
    }

    pub fn disk_palette(&self) -> &[PaletteEntry; 256] {
        &self.disk_palette
    }

    pub fn expected_center_marker(&self) -> u8 {
        self.expected_center_marker
    }

    pub fn marker(slot: RootMenuBitmapSlot) -> u8 {
        40 + slot.index() as u8
    }
}

impl Default for SyntheticRootFrameFixture {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "cmr-synthetic-root-frame-{}-{id}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create synthetic fixture directory");
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
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

fn write_reviewed_root_assets(
    source: &Path,
    disk_palette: &[PaletteEntry; 256],
) -> (ReviewedProfile, u8) {
    let mut files = BTreeMap::<&'static str, Vec<u8>>::new();
    let startup_palette = [PaletteEntry {
        red: 3,
        green: 2,
        blue: 1,
        flags: 0,
    }; 256];
    files.insert("ini/loading.bmp", indexed_bitmap(640, 480, 3));
    files.insert("ini/loading.pal", riff_palette(&startup_palette));
    files.insert(ROOT_MENU_DISK_PALETTE_PATH, riff_palette(disk_palette));
    files.insert(Language::English.frontend_path(), footer_table_bytes());
    for (index, style) in FontStyle::ALL.into_iter().enumerate() {
        files.insert(style.path(), synthetic_font(index as u8));
    }
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
        let value = SyntheticRootFrameFixture::marker(slot);
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
        fs::create_dir_all(destination.parent().expect("asset has parent"))
            .expect("create synthetic asset directory");
        fs::write(&destination, &bytes).expect("write synthetic asset");
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
        roots: vec!["New_Fe".to_owned(), "ini".to_owned()],
        empty_directories: Vec::new(),
        sentinels: Vec::new(),
        files: profile_files,
    };
    (
        ReviewedProfile::from_json(&serde_json::to_vec(&profile).expect("serialize profile"))
            .expect("review synthetic profile"),
        center_marker,
    )
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

fn footer_table_bytes() -> Vec<u8> {
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
    bytes
}

fn riff_palette(entries: &[PaletteEntry; 256]) -> Vec<u8> {
    let data_size = 4 + entries.len() * 4;
    let riff_size = 4 + 8 + data_size;
    let mut bytes = Vec::with_capacity(riff_size + 8);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(riff_size as u32).to_le_bytes());
    bytes.extend_from_slice(b"PAL ");
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&(data_size as u32).to_le_bytes());
    bytes.extend_from_slice(&0x0300u16.to_le_bytes());
    bytes.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for entry in entries {
        bytes.extend_from_slice(&[entry.red, entry.green, entry.blue, entry.flags]);
    }
    bytes
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
        write!(encoded, "{byte:02x}").expect("writing to a String cannot fail");
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_owns_a_reviewed_import_until_drop() {
        let runtime_root = {
            let fixture = SyntheticRootFrameFixture::new();

            assert!(fixture.runtime_root().join("import-state.json").is_file());
            assert_eq!(
                fixture.reviewed_profile().profile().id,
                "synthetic-root-frame-vertical"
            );
            assert_eq!(
                fixture.store().load("ini/loading.bmp").unwrap()[0..2],
                *b"BM"
            );
            fixture.runtime_root().to_owned()
        };
        assert!(!runtime_root.exists());
    }
}
