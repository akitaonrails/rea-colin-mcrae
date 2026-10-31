//! `cmr-hd pack`: the HD set's runtime BMPs stored compactly for a
//! download (the format is `cmr_hdpack`'s, which also turns them back).
//! 8-bit textures become palette PNGs (exact); colour images become a
//! lossless PNG, or a near-lossless WebP when that is much smaller (the
//! upscaled pictures and panels, about 7x).

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// WebP quality for colour images: no visible change (41 dB measured).
const QUALITY: f32 = 92.0;

pub fn pack(hd: &Path, out: &Path, keyed: &BTreeMap<String, bool>) -> Result<(), String> {
    let mut list = format!("{}\n", cmr_hdpack::HEADER);
    let mut total = 0;
    for set in cmr_hdpack::SETS {
        let dir = out.join(set);
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let mut names: Vec<_> = fs::read_dir(hd.join(set))
            .map_err(|e| format!("{}: {e}", hd.join(set).display()))?
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .filter(|n| n.ends_with(".bmp"))
            .collect();
        names.sort();
        for name in names {
            let stem = name.trim_end_matches(".bmp");
            let bmp = fs::read(hd.join(set).join(&name)).map_err(|e| e.to_string())?;
            let (file, bytes) = encode(stem, &bmp, keyed.get(stem).copied().unwrap_or(false))
                .map_err(|e| format!("{set}/{name}: {e}"))?;
            // Checked: the pack gives back the same pixels.
            let back = cmr_hdpack::to_bmp(&file, &bytes)?.1;
            if file.ends_with(".png") && pixels(&back)? != pixels(&bmp)? {
                return Err(format!("{set}/{name}: the pack changed its pixels"));
            }
            total += bytes.len();
            fs::write(dir.join(&file), &bytes).map_err(|e| e.to_string())?;
            list.push_str(&format!("{set}/{file}\n"));
        }
    }
    fs::write(out.join(cmr_hdpack::LIST), list).map_err(|e| e.to_string())?;
    println!(
        "pack written to {} ({} MB)",
        out.display(),
        total / 1_000_000
    );
    Ok(())
}

/// A pack file (name, bytes) for one runtime BMP.
fn encode(stem: &str, bmp: &[u8], keyed: bool) -> Result<(String, Vec<u8>), String> {
    let u32_at = |o: usize| u32::from_le_bytes(bmp[o..o + 4].try_into().unwrap());
    if bmp.len() < 54 || &bmp[..2] != b"BM" {
        return Err("not a BMP".into());
    }
    let (w, h) = (u32_at(18), u32_at(22));
    let bits = u16::from_le_bytes([bmp[28], bmp[29]]);
    if bits == 8 {
        let palette = crate::palette::of_bmp(bmp).ok_or("no palette")?;
        let (offset, row) = (u32_at(10) as usize, (w as usize + 3) & !3);
        let mut indices = Vec::with_capacity((w * h) as usize);
        for y in (0..h as usize).rev() {
            let at = offset + y * row;
            indices.extend_from_slice(bmp.get(at..at + w as usize).ok_or("short BMP")?);
        }
        let mut out = Vec::new();
        let mut e = png::Encoder::new(&mut out, w, h);
        e.set_color(png::ColorType::Indexed);
        e.set_depth(png::BitDepth::Eight);
        e.set_compression(png::Compression::High);
        e.set_palette(palette.concat());
        e.write_header()
            .and_then(|mut wr| wr.write_image_data(&indices))
            .map_err(|e| e.to_string())?;
        return Ok((format!("{stem}.png"), out));
    }
    let img = image::load_from_memory_with_format(bmp, image::ImageFormat::Bmp)
        .map_err(|e| e.to_string())?;
    let (png_bytes, webp_bytes) = if keyed {
        let mut rgba = img.to_rgba8();
        crate::key::to_alpha(&mut rgba);
        let webp = webp::Encoder::from_rgba(&rgba, w, h)
            .encode(QUALITY)
            .to_vec();
        (png_of(image::DynamicImage::ImageRgba8(rgba))?, webp)
    } else {
        let rgb = img.to_rgb8();
        let webp = webp::Encoder::from_rgb(&rgb, w, h).encode(QUALITY).to_vec();
        (png_of(image::DynamicImage::ImageRgb8(rgb))?, webp)
    };
    Ok(if webp_bytes.len() * 10 < png_bytes.len() * 7 {
        (format!("{stem}.webp"), webp_bytes)
    } else {
        (format!("{stem}.png"), png_bytes)
    })
}

fn png_of(img: image::DynamicImage) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

/// A BMP's pixels as RGBA, to compare two encodings.
fn pixels(bmp: &[u8]) -> Result<Vec<u8>, String> {
    Ok(
        image::load_from_memory_with_format(bmp, image::ImageFormat::Bmp)
            .map_err(|e| e.to_string())?
            .to_rgba8()
            .into_raw(),
    )
}
