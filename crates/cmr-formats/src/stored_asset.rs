use std::fmt;
use std::io::{Cursor, Read};

use flate2::bufread::GzDecoder;

const GZIP_MAGIC: &[u8; 2] = b"\x1f\x8b";
const CMPR_MAGIC: &[u8; 4] = b"CMPR";
const CMPR_HEADER_LEN: usize = 8;

/// Resource limits applied before and during decoding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecodeLimits {
    pub max_stored_bytes: usize,
    pub max_payload_bytes: usize,
}

impl Default for DecodeLimits {
    fn default() -> Self {
        Self {
            max_stored_bytes: 64 * 1024 * 1024,
            max_payload_bytes: 256 * 1024 * 1024,
        }
    }
}

/// A validated stored asset.
#[derive(Debug, Eq, PartialEq)]
pub enum StoredAsset<'a> {
    /// The original bytes did not begin with gzip magic and pass through as-is.
    Raw(&'a [u8]),
    /// A gzip stream containing a validated `CMPR` envelope.
    Cmpr(Vec<u8>),
}

impl StoredAsset<'_> {
    pub fn bytes(&self) -> &[u8] {
        match self {
            Self::Raw(bytes) => bytes,
            Self::Cmpr(bytes) => bytes.as_slice(),
        }
    }
}

#[derive(Debug)]
pub enum DecodeError {
    StoredTooLarge { actual: usize, limit: usize },
    Gzip(std::io::Error),
    DecodedTooLarge { limit: usize },
    MissingCmprHeader,
    PayloadTooLarge { declared: usize, limit: usize },
    PayloadLengthMismatch { declared: usize, actual: usize },
    TrailingGzipData { bytes: usize },
}

impl fmt::Display for DecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StoredTooLarge { actual, limit } => {
                write!(
                    formatter,
                    "stored asset is {actual} bytes; limit is {limit}"
                )
            }
            Self::Gzip(error) => write!(formatter, "invalid gzip stream: {error}"),
            Self::DecodedTooLarge { limit } => {
                write!(
                    formatter,
                    "decoded CMPR envelope exceeds {} bytes",
                    limit + CMPR_HEADER_LEN
                )
            }
            Self::MissingCmprHeader => {
                write!(formatter, "gzip payload does not begin with a CMPR header")
            }
            Self::PayloadTooLarge { declared, limit } => {
                write!(
                    formatter,
                    "CMPR payload declares {declared} bytes; limit is {limit}"
                )
            }
            Self::PayloadLengthMismatch { declared, actual } => {
                write!(
                    formatter,
                    "CMPR payload declares {declared} bytes but contains {actual}"
                )
            }
            Self::TrailingGzipData { bytes } => {
                write!(formatter, "gzip member has {bytes} trailing bytes")
            }
        }
    }
}

impl std::error::Error for DecodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Gzip(error) => Some(error),
            _ => None,
        }
    }
}

/// Decodes the storage wrapper used by the observed game corpus.
///
/// Files without gzip magic pass through unchanged. Gzip files must contain one
/// member whose decoded bytes are `CMPR`, a little-endian `u32` payload length,
/// and exactly that many payload bytes.
pub fn decode_stored_asset(
    input: &[u8],
    limits: DecodeLimits,
) -> Result<StoredAsset<'_>, DecodeError> {
    if input.len() > limits.max_stored_bytes {
        return Err(DecodeError::StoredTooLarge {
            actual: input.len(),
            limit: limits.max_stored_bytes,
        });
    }

    if !input.starts_with(GZIP_MAGIC) {
        return Ok(StoredAsset::Raw(input));
    }

    let maximum_decoded = limits.max_payload_bytes.saturating_add(CMPR_HEADER_LEN);
    let read_limit = maximum_decoded.saturating_add(1);
    let cursor = Cursor::new(input);
    let mut decoder = GzDecoder::new(cursor);
    let mut decoded = Vec::with_capacity(read_limit.min(64 * 1024));
    decoder
        .by_ref()
        .take(read_limit as u64)
        .read_to_end(&mut decoded)
        .map_err(DecodeError::Gzip)?;

    if decoded.len() > maximum_decoded {
        return Err(DecodeError::DecodedTooLarge {
            limit: limits.max_payload_bytes,
        });
    }

    // Reading to the member's end validates its trailer, including the CRC.
    // The BufRead decoder leaves bytes after the first member unconsumed.
    let consumed = decoder.get_ref().position() as usize;
    if consumed != input.len() {
        return Err(DecodeError::TrailingGzipData {
            bytes: input.len() - consumed,
        });
    }

    if decoded.len() < CMPR_HEADER_LEN || !decoded.starts_with(CMPR_MAGIC) {
        return Err(DecodeError::MissingCmprHeader);
    }

    let declared = u32::from_le_bytes(decoded[4..8].try_into().expect("fixed-size slice")) as usize;
    if declared > limits.max_payload_bytes {
        return Err(DecodeError::PayloadTooLarge {
            declared,
            limit: limits.max_payload_bytes,
        });
    }

    let payload = decoded.split_off(CMPR_HEADER_LEN);
    if payload.len() != declared {
        return Err(DecodeError::PayloadLengthMismatch {
            declared,
            actual: payload.len(),
        });
    }

    Ok(StoredAsset::Cmpr(payload))
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::Compression;
    use flate2::write::GzEncoder;

    use super::*;

    const LIMITS: DecodeLimits = DecodeLimits {
        max_stored_bytes: 1024,
        max_payload_bytes: 64,
    };

    fn gzip(bytes: &[u8]) -> Vec<u8> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }

    fn cmpr(declared: u32, payload: &[u8]) -> Vec<u8> {
        let mut envelope = Vec::from(CMPR_MAGIC.as_slice());
        envelope.extend_from_slice(&declared.to_le_bytes());
        envelope.extend_from_slice(payload);
        gzip(&envelope)
    }

    #[test]
    fn raw_bytes_pass_through_without_copying() {
        let input = b"RIFF synthetic fixture";
        let decoded = decode_stored_asset(input, LIMITS).unwrap();
        assert_eq!(decoded, StoredAsset::Raw(input));
        assert!(std::ptr::eq(decoded.bytes().as_ptr(), input.as_ptr()));
    }

    #[test]
    fn unwraps_valid_gzip_cmpr() {
        let input = cmpr(5, b"hello");
        assert_eq!(
            decode_stored_asset(&input, LIMITS).unwrap(),
            StoredAsset::Cmpr(b"hello".to_vec())
        );
    }

    #[test]
    fn rejects_gzip_without_cmpr_envelope() {
        let error = decode_stored_asset(&gzip(b"ordinary gzip"), LIMITS).unwrap_err();
        assert!(matches!(error, DecodeError::MissingCmprHeader));
    }

    #[test]
    fn rejects_declared_length_mismatch() {
        let error = decode_stored_asset(&cmpr(4, b"five!"), LIMITS).unwrap_err();
        assert!(matches!(
            error,
            DecodeError::PayloadLengthMismatch {
                declared: 4,
                actual: 5
            }
        ));
    }

    #[test]
    fn rejects_corrupt_gzip_crc() {
        let mut input = cmpr(5, b"hello");
        let crc_byte = input.len() - 8;
        input[crc_byte] ^= 0xff;
        let error = decode_stored_asset(&input, LIMITS).unwrap_err();
        assert!(matches!(error, DecodeError::Gzip(_)));
    }

    #[test]
    fn rejects_trailing_data_or_second_member() {
        let mut input = cmpr(5, b"hello");
        input.extend_from_slice(&gzip(b"second member"));
        let error = decode_stored_asset(&input, LIMITS).unwrap_err();
        assert!(matches!(error, DecodeError::TrailingGzipData { .. }));
    }

    #[test]
    fn rejects_payload_over_limit_without_unbounded_output() {
        let input = cmpr(65, &[0; 65]);
        let error = decode_stored_asset(&input, LIMITS).unwrap_err();
        assert!(matches!(error, DecodeError::DecodedTooLarge { limit: 64 }));
    }

    #[test]
    fn rejects_stored_input_over_limit_before_decoding() {
        let limits = DecodeLimits {
            max_stored_bytes: 4,
            ..LIMITS
        };
        let error = decode_stored_asset(b"12345", limits).unwrap_err();
        assert!(matches!(
            error,
            DecodeError::StoredTooLarge {
                actual: 5,
                limit: 4
            }
        ));
    }
}
