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
}
