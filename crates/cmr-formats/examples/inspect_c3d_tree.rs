use std::env;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cmr_formats::c3d::{
    C3D_BLOB_RECORD_HEADER_LEN, C3D_GROUP0_VALUES_OFFSET, C3dError, C3dLimits, parse_c3d,
};
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
                "files={} group0_records={} group0_bytes={} group0_values={} group0_to_group1={} group0_to_group1_errors={} group1_records={} group1_bytes={} entries32={} entries24={} group1_to_group2={} group2_sentinels={} group1_to_group2_errors={} blob_records={} blob_bytes={} group0_reaches_group1={} group1_reaches_group2={} group0_exact_layout={} group1_exact_layout={} records_exhaust_payload={}",
                summary.files,
                summary.group0_records,
                summary.group0_bytes,
                summary.group0_values,
                summary.group0_to_group1,
                summary.group0_to_group1_errors,
                summary.group1_records,
                summary.group1_bytes,
                summary.entries32,
                summary.entries24,
                summary.group1_to_group2,
                summary.group2_sentinels,
                summary.group1_to_group2_errors,
                summary.blob_records,
                summary.blob_bytes,
                summary.group0_reaches_group1,
                summary.group1_reaches_group2,
                summary.group0_exact_layout,
                summary.group1_exact_layout,
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
    group0_records: u64,
    group0_bytes: u64,
    group0_values: u64,
    group0_to_group1: u64,
    group0_to_group1_errors: u64,
    group1_records: u64,
    group1_bytes: u64,
    entries32: u64,
    entries24: u64,
    group1_to_group2: u64,
    group2_sentinels: u64,
    group1_to_group2_errors: u64,
    blob_records: u64,
    blob_bytes: u64,
    group0_reaches_group1: u64,
    group1_reaches_group2: u64,
    group0_exact_layout: u64,
    group1_exact_layout: u64,
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
    checked_accumulate(
        &mut summary.group0_records,
        file_summary.group0_records,
        "C3D group-0 record count",
    )?;
    checked_accumulate(
        &mut summary.group0_bytes,
        file_summary.group0_bytes,
        "C3D group-0 byte count",
    )?;
    checked_accumulate(
        &mut summary.group0_values,
        file_summary.group0_values,
        "C3D group-0 value count",
    )?;
    checked_accumulate(
        &mut summary.group0_to_group1,
        file_summary.group0_to_group1,
        "C3D group-0-to-group-1 reference count",
    )?;
    checked_accumulate(
        &mut summary.group0_to_group1_errors,
        file_summary.group0_to_group1_errors,
        "C3D group-0-to-group-1 reference-error count",
    )?;
    checked_accumulate(
        &mut summary.group1_records,
        file_summary.group1_records,
        "C3D group-1 record count",
    )?;
    checked_accumulate(
        &mut summary.group1_bytes,
        file_summary.group1_bytes,
        "C3D group-1 byte count",
    )?;
    checked_accumulate(
        &mut summary.entries32,
        file_summary.entries32,
        "C3D 32-byte entry count",
    )?;
    checked_accumulate(
        &mut summary.entries24,
        file_summary.entries24,
        "C3D 24-byte entry count",
    )?;
    checked_accumulate(
        &mut summary.group1_to_group2,
        file_summary.group1_to_group2,
        "C3D group-1-to-group-2 reference count",
    )?;
    checked_accumulate(
        &mut summary.group2_sentinels,
        file_summary.group2_sentinels,
        "C3D group-2 sentinel count",
    )?;
    checked_accumulate(
        &mut summary.group1_to_group2_errors,
        file_summary.group1_to_group2_errors,
        "C3D group-1-to-group-2 reference-error count",
    )?;
    summary.blob_records = summary
        .blob_records
        .checked_add(file_summary.blob_records)
        .ok_or_else(|| "C3D blob-record count overflow".to_owned())?;
    summary.blob_bytes = summary
        .blob_bytes
        .checked_add(file_summary.blob_bytes)
        .ok_or_else(|| "C3D blob-byte count overflow".to_owned())?;
    for (aggregate, present, name) in [
        (
            &mut summary.group0_reaches_group1,
            file_summary.group0_reaches_group1,
            "C3D group-0 boundary count",
        ),
        (
            &mut summary.group1_reaches_group2,
            file_summary.group1_reaches_group2,
            "C3D group-1 boundary count",
        ),
    ] {
        if present {
            checked_accumulate(aggregate, 1, name)?;
        }
    }
    checked_accumulate(
        &mut summary.group0_exact_layout,
        file_summary.group0_exact_layout,
        "C3D group-0 exact-layout count",
    )?;
    checked_accumulate(
        &mut summary.group1_exact_layout,
        file_summary.group1_exact_layout,
        "C3D group-1 exact-layout count",
    )?;
    if file_summary.records_exhaust_payload {
        summary.records_exhaust_payload = summary
            .records_exhaust_payload
            .checked_add(1)
            .ok_or_else(|| "C3D exhaustion count overflow".to_owned())?;
    }
    Ok(())
}

fn checked_accumulate(target: &mut u64, value: u64, name: &str) -> Result<(), String> {
    *target = target
        .checked_add(value)
        .ok_or_else(|| format!("{name} overflow"))?;
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
    group0_records: u64,
    group0_bytes: u64,
    group0_values: u64,
    group0_to_group1: u64,
    group0_to_group1_errors: u64,
    group1_records: u64,
    group1_bytes: u64,
    entries32: u64,
    entries24: u64,
    group1_to_group2: u64,
    group2_sentinels: u64,
    group1_to_group2_errors: u64,
    blob_records: u64,
    blob_bytes: u64,
    group0_reaches_group1: bool,
    group1_reaches_group2: bool,
    group0_exact_layout: u64,
    group1_exact_layout: u64,
    records_exhaust_payload: bool,
}

fn inspect_payload(payload: &[u8]) -> Result<FileSummary, C3dError> {
    let parsed = parse_c3d(payload, C3dLimits::default())?;
    let mut group0_records = 0;
    let mut group0_bytes = 0;
    let mut group0_values = 0;
    let mut group0_exact_layout = 0;
    let mut group0_end = usize::try_from(parsed.header().section_offsets[0])
        .expect("validated C3D position fits usize");
    for record in parsed.group0_records() {
        group0_records += 1;
        group0_bytes += record.bytes().len() as u64;
        group0_values += u64::from(record.value_count());
        let used = C3D_GROUP0_VALUES_OFFSET + usize::from(record.value_count()) * 2;
        if record.bytes().len() == ((used + 3) & !3) {
            group0_exact_layout += 1;
        }
        group0_end = record.offset() + record.bytes().len();
    }
    let mut group0_to_group1 = 0;
    let mut group0_to_group1_errors = 0;
    for reference in parsed.resolve_group1_references() {
        if reference.is_ok() {
            group0_to_group1 += 1;
        } else {
            group0_to_group1_errors += 1;
        }
    }

    let mut group1_records = 0;
    let mut group1_bytes = 0;
    let mut entries32 = 0;
    let mut entries24 = 0;
    let mut group1_exact_layout = 0;
    let mut group1_end = usize::try_from(parsed.header().section_offsets[1])
        .expect("validated C3D position fits usize");
    for record in parsed.group1_records() {
        group1_records += 1;
        group1_bytes += record.bytes().len() as u64;
        entries32 += u64::from(record.entry32_count());
        entries24 += u64::from(record.entry24_count());
        if record.trailing_bytes().is_empty() {
            group1_exact_layout += 1;
        }
        group1_end = record.offset() + record.bytes().len();
    }
    let mut group1_to_group2 = 0;
    let mut group2_sentinels = 0;
    let mut group1_to_group2_errors = 0;
    for reference in parsed.resolve_group2_references() {
        match reference {
            Ok(reference) if reference.target().is_some() => group1_to_group2 += 1,
            Ok(_) => group2_sentinels += 1,
            Err(_) => group1_to_group2_errors += 1,
        }
    }

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
        group0_records,
        group0_bytes,
        group0_values,
        group0_to_group1,
        group0_to_group1_errors,
        group1_records,
        group1_bytes,
        entries32,
        entries24,
        group1_to_group2,
        group2_sentinels,
        group1_to_group2_errors,
        blob_records,
        blob_bytes,
        group0_reaches_group1: group0_end
            == usize::try_from(parsed.header().section_offsets[1])
                .expect("validated C3D position fits usize"),
        group1_reaches_group2: group1_end
            == usize::try_from(parsed.header().section_offsets[2])
                .expect("validated C3D position fits usize"),
        group0_exact_layout,
        group1_exact_layout,
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
        payload[20..24].copy_from_slice(&32u32.to_le_bytes());
        payload[24..28].copy_from_slice(&32u32.to_le_bytes());
        payload[28..32].copy_from_slice(&32u32.to_le_bytes());
        payload[32..44].copy_from_slice(b"SYNTHETIC\0\0\0");
        payload[44..48].copy_from_slice(&3u32.to_le_bytes());
        payload[48..51].copy_from_slice(&[1, 2, 3]);

        assert_eq!(
            inspect_payload(&payload).unwrap(),
            FileSummary {
                group0_records: 0,
                group0_bytes: 0,
                group0_values: 0,
                group0_to_group1: 0,
                group0_to_group1_errors: 0,
                group1_records: 0,
                group1_bytes: 0,
                entries32: 0,
                entries24: 0,
                group1_to_group2: 0,
                group2_sentinels: 0,
                group1_to_group2_errors: 0,
                blob_records: 1,
                blob_bytes: 3,
                group0_reaches_group1: true,
                group1_reaches_group2: true,
                group0_exact_layout: 0,
                group1_exact_layout: 0,
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
    fn payload_summary_uses_all_three_production_iterators() {
        let mut payload = c3d_header_with_blob_group(1, 160, 176);
        payload[12..14].copy_from_slice(&1u16.to_le_bytes());
        payload[14..16].copy_from_slice(&1u16.to_le_bytes());
        payload[20..24].copy_from_slice(&32u32.to_le_bytes());
        payload[24..28].copy_from_slice(&112u32.to_le_bytes());
        payload[32..36].copy_from_slice(&80u32.to_le_bytes());
        payload[124..128].copy_from_slice(&48u32.to_le_bytes());

        assert_eq!(
            inspect_payload(&payload).unwrap(),
            FileSummary {
                group0_records: 1,
                group0_bytes: 80,
                group0_values: 0,
                group0_to_group1: 0,
                group0_to_group1_errors: 0,
                group1_records: 1,
                group1_bytes: 48,
                entries32: 0,
                entries24: 0,
                group1_to_group2: 0,
                group2_sentinels: 0,
                group1_to_group2_errors: 0,
                blob_records: 1,
                blob_bytes: 0,
                group0_reaches_group1: true,
                group1_reaches_group2: true,
                group0_exact_layout: 1,
                group1_exact_layout: 1,
                records_exhaust_payload: true,
            }
        );
    }

    #[test]
    fn payload_summary_counts_resolved_sentinel_and_invalid_references() {
        let mut payload = vec![0; 228];
        payload[0..4].copy_from_slice(&C3D_MAGIC);
        payload[4..8].copy_from_slice(&C3D_FORMAT_VERSION.to_le_bytes());
        payload[12..14].copy_from_slice(&1u16.to_le_bytes());
        payload[14..16].copy_from_slice(&1u16.to_le_bytes());
        payload[16..18].copy_from_slice(&1u16.to_le_bytes());
        payload[20..24].copy_from_slice(&32u32.to_le_bytes());
        payload[24..28].copy_from_slice(&116u32.to_le_bytes());
        payload[28..32].copy_from_slice(&212u32.to_le_bytes());

        payload[32..36].copy_from_slice(&84u32.to_le_bytes());
        payload[108] = 1;
        payload[112..114].copy_from_slice(&0u16.to_le_bytes());

        payload[116..128].copy_from_slice(b"GROUP1-REFS!");
        payload[128..132].copy_from_slice(&96u32.to_le_bytes());
        payload[134..136].copy_from_slice(&2u16.to_le_bytes());
        payload[172..174].copy_from_slice(&0i16.to_le_bytes());
        payload[196..198].copy_from_slice(&(-1i16).to_le_bytes());
        payload[212..224].copy_from_slice(b"GROUP2-ZERO!");

        let summary = inspect_payload(&payload).unwrap();
        assert_eq!(summary.group0_to_group1, 1);
        assert_eq!(summary.group0_to_group1_errors, 0);
        assert_eq!(summary.group1_to_group2, 1);
        assert_eq!(summary.group2_sentinels, 1);
        assert_eq!(summary.group1_to_group2_errors, 0);

        payload[112..114].copy_from_slice(&1u16.to_le_bytes());
        payload[172..174].copy_from_slice(&(-2i16).to_le_bytes());
        let invalid = inspect_payload(&payload).unwrap();
        assert_eq!(invalid.group0_to_group1, 0);
        assert_eq!(invalid.group0_to_group1_errors, 1);
        assert_eq!(invalid.group1_to_group2, 0);
        assert_eq!(invalid.group2_sentinels, 1);
        assert_eq!(invalid.group1_to_group2_errors, 1);
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
