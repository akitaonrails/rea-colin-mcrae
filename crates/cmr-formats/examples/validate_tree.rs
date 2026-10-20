use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cmr_formats::stored_asset::{DecodeLimits, StoredAsset, decode_stored_asset};

const LIMITS: DecodeLimits = DecodeLimits {
    max_stored_bytes: 512 * 1024 * 1024,
    max_payload_bytes: 512 * 1024 * 1024,
};

fn main() -> ExitCode {
    let roots: Vec<PathBuf> = env::args_os().skip(1).map(PathBuf::from).collect();
    if roots.is_empty() {
        eprintln!("usage: cargo run -p cmr-formats --example validate_tree -- ROOT...");
        return ExitCode::from(2);
    }

    match validate_roots(&roots) {
        Ok(summary) => {
            println!(
                "files={} raw={} gzip_cmpr={} stored_bytes={} payload_bytes={}",
                summary.files,
                summary.raw,
                summary.gzip_cmpr,
                summary.stored_bytes,
                summary.payload_bytes
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
    raw: u64,
    gzip_cmpr: u64,
    stored_bytes: u64,
    payload_bytes: u64,
}

fn validate_roots(roots: &[PathBuf]) -> Result<Summary, String> {
    let mut summary = Summary::default();
    let mut pending = roots.to_vec();

    while let Some(path) = pending.pop() {
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;

        if metadata.file_type().is_symlink() {
            return Err(format!("refusing symbolic link: {}", path.display()));
        }

        if metadata.is_dir() {
            let entries = fs::read_dir(&path)
                .map_err(|error| format!("cannot list {}: {error}", path.display()))?;
            for entry in entries {
                let entry = entry
                    .map_err(|error| format!("cannot read entry in {}: {error}", path.display()))?;
                pending.push(entry.path());
            }
            continue;
        }

        if !metadata.is_file() {
            return Err(format!("refusing special file: {}", path.display()));
        }

        validate_file(&path, &mut summary)?;
    }

    Ok(summary)
}

fn validate_file(path: &Path, summary: &mut Summary) -> Result<(), String> {
    let stored =
        fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let decoded = decode_stored_asset(&stored, LIMITS)
        .map_err(|error| format!("invalid stored asset {}: {error}", path.display()))?;

    summary.files += 1;
    summary.stored_bytes += stored.len() as u64;
    match decoded {
        StoredAsset::Raw(_) => summary.raw += 1,
        StoredAsset::Cmpr(payload) => {
            summary.gzip_cmpr += 1;
            summary.payload_bytes += payload.len() as u64;
        }
    }
    Ok(())
}
