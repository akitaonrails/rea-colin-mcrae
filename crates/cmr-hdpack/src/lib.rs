//! The HD art pack (docs/UPSCALING.md): the images `cmr-hd` makes, stored
//! compactly for a download, and turned back into the BMPs the hook library
//! loads (`to_bmp`) when the launcher installs the pack.
//!
//! A pack is a folder with `x2/` and `x4/` sets and a `pack.txt` listing
//! them. Each image is named by its runtime name (`hd::name` of the game's
//! BMP) and is one of:
//! - `<name>.png`, 8-bit with a palette: a texture kept as palette indices
//!   by the game (the race HUD); its indices and palette are kept exactly.
//! - `<name>.png` or `<name>.webp` in colour: a 2D image. With an alpha
//!   channel it is keyed (transparent pixels become the game's key, pure
//!   black, and opaque ones are kept off pure black so the key still
//!   matches); without one it is drawn as is.

/// The file listing a pack; its first line is `HEADER`.
pub const LIST: &str = "pack.txt";
pub const HEADER: &str = "cmr-hd pack 1";
/// The two sets, by the screen they suit (see the launcher's `hd_set`).
pub const SETS: [&str; 2] = ["x2", "x4"];

/// The BMP file name and contents for a pack file.
pub fn to_bmp(file: &str, bytes: &[u8]) -> Result<(String, Vec<u8>), String> {
    let (stem, ext) = file
        .rsplit_once('.')
        .ok_or_else(|| format!("{file}: no extension"))?;
    let bmp = match ext {
        "png" => png_to_bmp(bytes),
        "webp" => colour_to_bmp(
            image::load_from_memory_with_format(bytes, image::ImageFormat::WebP)
                .map_err(|e| e.to_string())?,
        ),
        _ => return Err(format!("{file}: not a pack image")),
    }
    .map_err(|e| format!("{file}: {e}"))?;
    Ok((format!("{stem}.bmp"), bmp))
}

fn png_to_bmp(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let info = reader.info();
    if info.color_type == png::ColorType::Indexed && info.bit_depth == png::BitDepth::Eight {
        let (w, h) = (info.width, info.height);
        let palette: Vec<[u8; 3]> = info
            .palette
            .as_deref()
            .ok_or("an indexed PNG without a palette")?
            .as_chunks::<3>()
            .0
            .to_vec();
        let mut indices = vec![0; reader.output_buffer_size().ok_or("too large")?];
        reader.next_frame(&mut indices).map_err(|e| e.to_string())?;
        indices.truncate((w * h) as usize);
        return Ok(bmp8(w, h, &palette, &indices));
    }
    colour_to_bmp(
        image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
            .map_err(|e| e.to_string())?,
    )
}

fn colour_to_bmp(img: image::DynamicImage) -> Result<Vec<u8>, String> {
    let keyed = img.color().has_alpha();
    let rgba = img.to_rgba8();
    let rgb: Vec<[u8; 3]> = rgba
        .pixels()
        .map(|p| {
            let [r, g, b, a] = p.0;
            match (keyed, a < 128, [r, g, b]) {
                (true, true, _) => [0, 0, 0],
                // The nearest colour that isn't the key.
                (true, false, [0, 0, 0]) => [0, 0, 8],
                (_, _, c) => c,
            }
        })
        .collect();
    Ok(bmp24(rgba.width(), rgba.height(), &rgb))
}

fn header(w: u32, h: u32, bits: u16, colours: u32, row: usize) -> Vec<u8> {
    let image = row as u32 * h;
    let offset = 54 + 4 * colours;
    let mut b = Vec::with_capacity((offset + image) as usize);
    b.extend_from_slice(b"BM");
    b.extend_from_slice(&(offset + image).to_le_bytes());
    b.extend_from_slice(&[0; 4]);
    b.extend_from_slice(&offset.to_le_bytes());
    b.extend_from_slice(&40u32.to_le_bytes());
    b.extend_from_slice(&w.to_le_bytes());
    b.extend_from_slice(&h.to_le_bytes()); // bottom-up
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&bits.to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes()); // no compression
    b.extend_from_slice(&image.to_le_bytes());
    b.extend_from_slice(&[0; 8]); // resolution
    b.extend_from_slice(&colours.to_le_bytes());
    b.extend_from_slice(&colours.to_le_bytes());
    b
}

/// An 8-bit BMP: `indices` row by row from the top, a palette of up to 256.
pub fn bmp8(w: u32, h: u32, palette: &[[u8; 3]], indices: &[u8]) -> Vec<u8> {
    let row = (w as usize + 3) & !3;
    let mut b = header(w, h, 8, 256, row);
    for i in 0..256 {
        let [r, g, bl] = palette.get(i).copied().unwrap_or([0; 3]);
        b.extend_from_slice(&[bl, g, r, 0]);
    }
    for y in (0..h as usize).rev() {
        let line = &indices[y * w as usize..(y + 1) * w as usize];
        b.extend_from_slice(line);
        b.resize(b.len() + row - line.len(), 0);
    }
    b
}

/// A 24-bit BMP: `rgb` row by row from the top.
pub fn bmp24(w: u32, h: u32, rgb: &[[u8; 3]]) -> Vec<u8> {
    let row = (3 * w as usize + 3) & !3;
    let mut b = header(w, h, 24, 0, row);
    for y in (0..h as usize).rev() {
        let start = b.len();
        for [r, g, bl] in &rgb[y * w as usize..(y + 1) * w as usize] {
            b.extend_from_slice(&[*bl, *g, *r]);
        }
        b.resize(start + row, 0);
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexed_pngs_keep_their_indices_and_palette() {
        let palette = [[0, 0, 0], [255, 0, 0], [10, 20, 30]];
        let indices = [0u8, 1, 2, 1, 0, 2];
        let mut png_bytes = Vec::new();
        {
            let mut e = png::Encoder::new(&mut png_bytes, 3, 2);
            e.set_color(png::ColorType::Indexed);
            e.set_depth(png::BitDepth::Eight);
            e.set_palette(palette.concat());
            e.write_header()
                .unwrap()
                .write_image_data(&indices)
                .unwrap();
        }
        let (name, bmp) = to_bmp("abc.png", &png_bytes).unwrap();
        assert_eq!(name, "abc.bmp");
        assert_eq!(bmp, bmp8(3, 2, &palette, &indices));
        // Header: size, offset, bits; the bottom row first.
        assert_eq!(
            u32::from_le_bytes(bmp[2..6].try_into().unwrap()) as usize,
            bmp.len()
        );
        assert_eq!(
            u32::from_le_bytes(bmp[10..14].try_into().unwrap()),
            54 + 1024
        );
        assert_eq!(bmp[28], 8);
        assert_eq!(&bmp[1078..1081], [1, 0, 2]);
        assert_eq!(&bmp[1082..1085], [0, 1, 2]);
    }

    #[test]
    fn keyed_images_get_the_games_key() {
        let img = image::RgbaImage::from_raw(
            2,
            1,
            vec![9, 9, 9, 0, 0, 0, 0, 255], // transparent, opaque black
        )
        .unwrap();
        let mut png_bytes = Vec::new();
        img.write_to(
            &mut std::io::Cursor::new(&mut png_bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
        let (_, bmp) = to_bmp("k.png", &png_bytes).unwrap();
        assert_eq!(bmp, bmp24(2, 1, &[[0, 0, 0], [0, 0, 8]]));
        // Without alpha, black stays black.
        let rgb = image::RgbImage::from_raw(1, 1, vec![0, 0, 0]).unwrap();
        let mut png_bytes = Vec::new();
        rgb.write_to(
            &mut std::io::Cursor::new(&mut png_bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
        assert_eq!(
            to_bmp("p.png", &png_bytes).unwrap().1,
            bmp24(1, 1, &[[0, 0, 0]])
        );
        assert!(to_bmp("x.gif", &[]).is_err());
    }

    #[test]
    fn rows_are_padded_to_four_bytes() {
        let bmp = bmp24(1, 2, &[[1, 2, 3], [4, 5, 6]]);
        assert_eq!(bmp.len(), 54 + 8);
        assert_eq!(&bmp[54..58], [6, 5, 4, 0]);
    }
}
