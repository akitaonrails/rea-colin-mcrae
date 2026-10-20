use std::env;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cmr_formats::c3d::{C3D_BLOB_RECORD_HEADER_LEN, C3dError, C3dLimits, parse_c3d};
use cmr_formats::stored_asset::{DecodeLimits, decode_stored_asset};

const STORAGE_LIMITS: DecodeLimits = DecodeLimits {
    max_stored_bytes: 512 * 1024 * 1024,
    max_payload_bytes: 512 * 1024 * 1024,
};

fn main() -> ExitCode {
    let roots = env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    if roots.is_empty() {
        eprintln!("usage: cargo run -p cmr-formats --example inspect_c3d_tree -- ROOT...");
        return ExitCode::from(2);
    }

    match inspect_roots(&roots) {
        Ok(summary) => {
            println!(
                "files={} blob_records={} blob_bytes={} records_exhaust_payload={}",
                summary.files,
                summary.blob_records,
                summary.blob_bytes,
                summary.records_exhaust_payload
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

#[derive(Default)]
struct Summary {
    files: u64,
    blob_records: u64,
    blob_bytes: u64,
    records_exhaust_payload: u64,
}

fn inspect_roots(roots: &[PathBuf]) -> Result<Summary, String> {
    let mut summary = Summary::default();
    let mut pending = roots.to_vec();

    while let Some(path) = pending.pop() {
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
        if metadata.file_type().is_symlink() {
            return Err(format!("refusing symbolic link: {}", path.display()));
        }
        if metadata.is_dir() {
            for entry in fs::read_dir(&path)
                .map_err(|error| format!("cannot list {}: {error}", path.display()))?
            {
                pending.push(
                    entry
                        .map_err(|error| {
                            format!("cannot read entry in {}: {error}", path.display())
                        })?
                        .path(),
                );
            }
        } else if metadata.is_file() && has_c3d_extension(&path) {
            inspect_file(&path, &mut summary)?;
        } else if !metadata.is_file() {
            return Err(format!("refusing special file: {}", path.display()));
        }
    }
    if summary.files == 0 {
        Err("no C3D files found under the supplied roots".to_owned())
    } else {
        Ok(summary)
    }
}

fn has_c3d_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("c3d"))
}

fn inspect_file(path: &Path, summary: &mut Summary) -> Result<(), String> {
    let stored = read_bounded_file(path, STORAGE_LIMITS.max_stored_bytes)?;
    let decoded = decode_stored_asset(&stored, STORAGE_LIMITS)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let file_summary =
        inspect_payload(decoded.bytes()).map_err(|error| format!("{}: {error}", path.display()))?;

    summary.files = summary
        .files
        .checked_add(1)
        .ok_or_else(|| "C3D file count overflow".to_owned())?;
    summary.blob_records = summary
        .blob_records
        .checked_add(file_summary.blob_records)
        .ok_or_else(|| "C3D blob-record count overflow".to_owned())?;
    summary.blob_bytes = summary
        .blob_bytes
        .checked_add(file_summary.blob_bytes)
        .ok_or_else(|| "C3D blob-byte count overflow".to_owned())?;
    if file_summary.records_exhaust_payload {
        summary.records_exhaust_payload = summary
            .records_exhaust_payload
            .checked_add(1)
            .ok_or_else(|| "C3D exhaustion count overflow".to_owned())?;
    }
    Ok(())
}

fn read_bounded_file(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let metadata = fs::metadata(path).map_err(|error| format!("{}: {error}", path.display()))?;
    if metadata.len() > u64::try_from(limit).unwrap_or(u64::MAX) {
        return Err(format!(
            "{}: stored asset is {} bytes; limit is {limit}",
            path.display(),
            metadata.len()
        ));
    }

    let file = File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    read_bounded(file, limit).map_err(|error| format!("{}: {error}", path.display()))
}

fn read_bounded(reader: impl Read, limit: usize) -> io::Result<Vec<u8>> {
    let read_limit = u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1);
    let mut bytes = Vec::with_capacity(limit.min(64 * 1024));
    reader.take(read_limit).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("stored asset exceeded {limit} bytes while reading"),
        ))
    } else {
        Ok(bytes)
    }
}

#[derive(Debug, Eq, PartialEq)]
struct FileSummary {
    blob_records: u64,
    blob_bytes: u64,
    records_exhaust_payload: bool,
}

fn inspect_payload(payload: &[u8]) -> Result<FileSummary, C3dError> {
    let parsed = parse_c3d(payload, C3dLimits::default())?;
    let mut blob_records = 0;
    let mut blob_bytes = 0;
    let mut end = usize::try_from(parsed.header().section_offsets[2])
        .expect("validated C3D position fits usize");
    for record in parsed.blob_records() {
        blob_records = u64::checked_add(blob_records, 1).expect("C3D record count fits u64");
        blob_bytes = u64::checked_add(
            blob_bytes,
            u64::try_from(record.blob().len()).expect("blob length fits u64"),
        )
        .expect("one C3D payload byte count fits u64");
        end = record.offset() + C3D_BLOB_RECORD_HEADER_LEN + record.blob().len();
    }
    Ok(FileSummary {
        blob_records,
        blob_bytes,
        records_exhaust_payload: end == parsed.bytes().len(),
    })
}

#[cfg(test)]
mod tests {
    use cmr_formats::c3d::{C3D_FORMAT_VERSION, C3D_MAGIC};

    use super::*;

    #[test]
    fn extension_filter_is_ascii_case_insensitive() {
        assert!(has_c3d_extension(Path::new("object.C3D")));
        assert!(has_c3d_extension(Path::new("object.c3d")));
        assert!(!has_c3d_extension(Path::new("object.c3dx")));
        assert!(!has_c3d_extension(Path::new("object")));
    }

    #[test]
    fn empty_scan_is_an_error_instead_of_a_successful_zero_summary() {
        assert_eq!(
            inspect_roots(&[]).err().as_deref(),
            Some("no C3D files found under the supplied roots")
        );
    }

    #[test]
    fn payload_summary_counts_records_bytes_and_exact_exhaustion() {
        let mut payload = vec![0; 52];
        payload[0..4].copy_from_slice(&C3D_MAGIC);
        payload[4..8].copy_from_slice(&C3D_FORMAT_VERSION.to_le_bytes());
        payload[16..18].copy_from_slice(&1u16.to_le_bytes());
        payload[28..32].copy_from_slice(&32u32.to_le_bytes());
        payload[32..44].copy_from_slice(b"SYNTHETIC\0\0\0");
        payload[44..48].copy_from_slice(&3u32.to_le_bytes());
        payload[48..51].copy_from_slice(&[1, 2, 3]);

        assert_eq!(
            inspect_payload(&payload).unwrap(),
            FileSummary {
                blob_records: 1,
                blob_bytes: 3,
                records_exhaust_payload: false,
            }
        );
        payload.pop();
        assert!(inspect_payload(&payload).unwrap().records_exhaust_payload);
    }

    #[test]
    fn zero_records_exhaust_only_when_their_base_is_at_payload_end() {
        let at_end = c3d_header_with_blob_group(0, 32, 32);
        assert!(inspect_payload(&at_end).unwrap().records_exhaust_payload);

        let before_end = c3d_header_with_blob_group(0, 32, 33);
        assert!(
            !inspect_payload(&before_end)
                .unwrap()
                .records_exhaust_payload
        );
    }

    #[test]
    fn bounded_reader_accepts_the_limit_and_rejects_one_byte_more() {
        let exact = read_bounded(io::Cursor::new([1, 2, 3, 4]), 4).unwrap();
        assert_eq!(exact, [1, 2, 3, 4]);

        let error = read_bounded(io::Cursor::new([1, 2, 3, 4, 5, 6]), 4).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            error.to_string(),
            "stored asset exceeded 4 bytes while reading"
        );
    }

    fn c3d_header_with_blob_group(count: u16, offset: u32, len: usize) -> Vec<u8> {
        let mut payload = vec![0; len];
        payload[0..4].copy_from_slice(&C3D_MAGIC);
        payload[4..8].copy_from_slice(&C3D_FORMAT_VERSION.to_le_bytes());
        payload[16..18].copy_from_slice(&count.to_le_bytes());
        payload[28..32].copy_from_slice(&offset.to_le_bytes());
        payload
    }
}
