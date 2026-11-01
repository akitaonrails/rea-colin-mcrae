//! The game's stored files: most are a gzip stream whose payload is `CMPR`,
//! a little-endian length, then the file itself (for images, an ordinary
//! Windows bitmap); the rest are stored as they are. The game reads both
//! through one loader (`FUN_00495ed0`).

use std::io::Read;

const GZIP: [u8; 2] = [0x1f, 0x8b];
const CMPR: &[u8; 4] = b"CMPR";

/// The file inside a stored asset. Errors name what is wrong.
pub fn unwrap(stored: &[u8]) -> Result<Vec<u8>, String> {
    if !stored.starts_with(&GZIP) {
        return Ok(stored.to_vec());
    }
    let mut payload = Vec::new();
    flate2::read::GzDecoder::new(stored)
        .read_to_end(&mut payload)
        .map_err(|e| format!("not a valid gzip stream: {e}"))?;
    if payload.len() < 8 || &payload[..4] != CMPR {
        return Err("the gzip payload doesn't start with CMPR".into());
    }
    let len = u32::from_le_bytes(payload[4..8].try_into().expect("4 bytes")) as usize;
    if payload.len() - 8 != len {
        return Err(format!(
            "CMPR says {len} bytes, {} follow",
            payload.len() - 8
        ));
    }
    Ok(payload.split_off(8))
}

/// The bitmaps stored inside a model file (a stage's horizon, `*.hzn`:
/// a `CM3D` model whose sky and ground textures are whole Windows bitmaps),
/// in file order. Only complete files with the usual 40-byte header count.
pub fn embedded_bitmaps(model: &[u8]) -> Vec<&[u8]> {
    let u32_at = |o: usize| u32::from_le_bytes(model[o..o + 4].try_into().expect("4 bytes"));
    let mut found = Vec::new();
    let mut at = 0;
    while at + 54 <= model.len() {
        let size = u32_at(at + 2) as usize;
        let is_bitmap = &model[at..at + 2] == b"BM"
            && u32_at(at + 6) == 0
            && u32_at(at + 14) == 40
            && model[at + 26..at + 28] == [1, 0]
            && matches!(model[at + 28], 8 | 24)
            && (1..=4096).contains(&u32_at(at + 18))
            && (1..=4096).contains(&u32_at(at + 22))
            && size > 54
            && at + size <= model.len();
        if is_bitmap {
            found.push(&model[at..at + size]);
            at += size;
        } else {
            at += 1;
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn stored(inner: &[u8]) -> Vec<u8> {
        let mut payload = CMPR.to_vec();
        payload.extend_from_slice(&(inner.len() as u32).to_le_bytes());
        payload.extend_from_slice(inner);
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(&payload).unwrap();
        gz.finish().unwrap()
    }

    #[test]
    fn unwraps_the_container() {
        assert_eq!(unwrap(&stored(b"BM rest")).unwrap(), b"BM rest");
    }

    #[test]
    fn plain_files_pass_through() {
        assert_eq!(unwrap(b"BM plain").unwrap(), b"BM plain");
    }

    #[test]
    fn rejects_damaged_containers() {
        let mut bad = stored(b"BM");
        let n = bad.len();
        bad[n - 9] ^= 0xff; // break the CRC area
        assert!(unwrap(&bad).is_err());
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(b"XXXX\x02\0\0\0BM").unwrap();
        assert!(unwrap(&gz.finish().unwrap()).is_err(), "not CMPR");
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(b"CMPR\x09\0\0\0BM").unwrap();
        assert!(unwrap(&gz.finish().unwrap()).is_err(), "wrong length");
    }

    #[test]
    fn bitmaps_inside_a_model() {
        // A 2x1 8-bit bitmap (2-entry palette, one padded row).
        let mut bmp = b"BM".to_vec();
        let size: u32 = 14 + 40 + 8 + 4;
        bmp.extend_from_slice(&size.to_le_bytes());
        bmp.extend_from_slice(&0u32.to_le_bytes());
        bmp.extend_from_slice(&(14u32 + 40 + 8).to_le_bytes());
        bmp.extend_from_slice(&40u32.to_le_bytes());
        bmp.extend_from_slice(&2u32.to_le_bytes());
        bmp.extend_from_slice(&1u32.to_le_bytes());
        bmp.extend_from_slice(&[1, 0, 8, 0]);
        bmp.extend_from_slice(&[0; 24]);
        bmp.extend_from_slice(&[0, 0, 0, 0, 255, 255, 255, 0]);
        bmp.extend_from_slice(&[0, 1, 0, 0]);
        assert_eq!(bmp.len(), size as usize);
        let mut model = b"CM3D BM not a bitmap ".to_vec();
        model.extend_from_slice(&bmp);
        model.extend_from_slice(b"mesh");
        model.extend_from_slice(&bmp);
        let found = embedded_bitmaps(&model);
        assert_eq!(found, [&bmp[..], &bmp[..]]);
        assert!(embedded_bitmaps(&bmp[..40]).is_empty(), "cut short");
    }
}
