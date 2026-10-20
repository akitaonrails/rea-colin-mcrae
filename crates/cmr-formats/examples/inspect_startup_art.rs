use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use cmr_formats::bitmap::{BitmapLimits, IndexedBitmap, parse_indexed_bitmap};
use cmr_formats::riff_palette::{RiffPalette, parse_legacy_palette_256, parse_riff_palette};

const BITMAP_PATHS: &[&str] = &[
    "loading.bmp",
    "loaded.bmp",
    "English/legal.bmp",
    "French/Legal.bmp",
    "German/legal.bmp",
    "Spanish/Legal.bmp",
];

const PALETTE_PATHS: &[&str] = &[
    "loading.pal",
    "legal.pal",
    "basepal.pal",
    "loadpal.pal",
    "schlpal.pal",
    "setrpal.pal",
];

fn main() -> Result<(), Box<dyn Error>> {
    let ini = exactly_one_argument()?;

    for relative in BITMAP_PATHS {
        let bitmap = read_bitmap(&ini, relative)?;
        println!(
            "bitmap {relative}: {}x{}, {} colors, {} pixels, source {}",
            bitmap.width,
            bitmap.height,
            bitmap.palette.len(),
            bitmap.pixels.len(),
            if bitmap.source_was_top_down {
                "top-down"
            } else {
                "bottom-up"
            }
        );
    }

    for relative in PALETTE_PATHS {
        let palette = read_palette(&ini, relative)?;
        let chunk_names = palette
            .ancillary_chunks
            .iter()
            .map(|chunk| String::from_utf8_lossy(&chunk.id).into_owned())
            .collect::<Vec<_>>()
            .join(",");
        println!(
            "palette {relative}: version {:#06x}, {} colors, {} extra data bytes, chunks [{}]",
            palette.version,
            palette.entries.len(),
            palette.data_extra.len(),
            chunk_names
        );
    }

    for (bitmap_path, palette_path) in [
        ("loading.bmp", "loading.pal"),
        ("loaded.bmp", "loadpal.pal"),
        ("English/legal.bmp", "legal.pal"),
        ("French/Legal.bmp", "legal.pal"),
        ("German/legal.bmp", "legal.pal"),
        ("Spanish/Legal.bmp", "legal.pal"),
    ] {
        let bitmap = read_bitmap(&ini, bitmap_path)?;
        let palette = read_palette(&ini, palette_path)?;
        let matching = bitmap
            .palette
            .iter()
            .zip(&palette.entries)
            .filter(|(embedded, external)| {
                embedded.red == external.red
                    && embedded.green == external.green
                    && embedded.blue == external.blue
            })
            .count();
        println!(
            "pair {bitmap_path} + {palette_path}: {matching}/{} RGB entries match",
            bitmap.palette.len().min(palette.entries.len())
        );
    }

    Ok(())
}

fn exactly_one_argument() -> Result<PathBuf, Box<dyn Error>> {
    let mut args = env::args_os();
    let executable = args.next().unwrap_or_default();
    let Some(ini) = args.next() else {
        return Err(format!(
            "usage: {} /path/to/CMRally/ini",
            Path::new(&executable)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
        )
        .into());
    };
    if args.next().is_some() {
        return Err("expected exactly one ini-directory argument".into());
    }
    Ok(ini.into())
}

fn read_bitmap(root: &Path, relative: &str) -> Result<IndexedBitmap, Box<dyn Error>> {
    let bytes = fs::read(root.join(relative))?;
    Ok(parse_indexed_bitmap(&bytes, BitmapLimits::default())?)
}

fn read_palette(root: &Path, relative: &str) -> Result<RiffPalette, Box<dyn Error>> {
    let bytes = fs::read(root.join(relative))?;
    parse_legacy_palette_256(&bytes)?;
    Ok(parse_riff_palette(&bytes)?)
}
