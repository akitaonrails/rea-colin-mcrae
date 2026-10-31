//! Installing the HD art pack that comes with the launcher (`hd` next to
//! it, made by `cmr-hd pack`): its compact images become the BMPs the hook
//! library loads, once, in the runtime folder (`<runtime>/hd/<id>/x2` and
//! `x4`). The id is the pack list's digest, so a new pack installs afresh
//! and an old one is removed.

use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// Where the pack `pack` is installed under `root`, installing it first if
/// needed. Returns the folder holding the `x2` and `x4` sets.
pub fn install(pack: &Path, root: &Path) -> Result<PathBuf, String> {
    let list = fs::read_to_string(pack.join(cmr_hdpack::LIST))
        .map_err(|e| format!("{}: {e}", pack.join(cmr_hdpack::LIST).display()))?;
    let mut lines = list.lines();
    if lines.next() != Some(cmr_hdpack::HEADER) {
        return Err(format!("{}: not an HD art pack", pack.display()));
    }
    let id: String = Sha256::digest(list.as_bytes())[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let dest = root.join(&id);
    let done = dest.join("installed");
    if done.is_file() {
        return Ok(dest);
    }
    // Older packs (and an interrupted install) go.
    if let Ok(entries) = fs::read_dir(root) {
        for e in entries.flatten() {
            let _ = fs::remove_dir_all(e.path());
        }
    }
    for set in cmr_hdpack::SETS {
        fs::create_dir_all(dest.join(set)).map_err(|e| e.to_string())?;
    }
    let files: Vec<&str> = lines.filter(|l| !l.is_empty()).collect();
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get());
    let chunk = files.len().div_ceil(workers).max(1);
    std::thread::scope(|scope| {
        let jobs: Vec<_> = files
            .chunks(chunk)
            .map(|part| scope.spawn(|| part.iter().try_for_each(|f| unpack(pack, &dest, f))))
            .collect();
        jobs.into_iter().try_for_each(|j| {
            j.join()
                .unwrap_or_else(|_| Err("an unpacking thread failed".into()))
        })
    })?;
    fs::write(&done, format!("{} files\n", files.len())).map_err(|e| e.to_string())?;
    Ok(dest)
}

/// One pack file (`set/name.ext`) to its BMP.
fn unpack(pack: &Path, dest: &Path, file: &str) -> Result<(), String> {
    let (set, name) = file
        .split_once('/')
        .filter(|(set, name)| cmr_hdpack::SETS.contains(set) && !name.contains(['/', '\\']))
        .ok_or_else(|| format!("{file}: not a pack entry"))?;
    let bytes = fs::read(pack.join(set).join(name)).map_err(|e| format!("{file}: {e}"))?;
    let (bmp_name, bmp) = cmr_hdpack::to_bmp(name, &bytes)?;
    fs::write(dest.join(set).join(bmp_name), bmp).map_err(|e| format!("{file}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(rgb: [u8; 3]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut e = png::Encoder::new(&mut out, 1, 1);
        e.set_color(png::ColorType::Rgb);
        e.write_header().unwrap().write_image_data(&rgb).unwrap();
        out
    }

    #[test]
    fn installs_once_and_replaces_older_packs() {
        let tmp = std::env::temp_dir().join(format!("cmr-hdpack-{}", std::process::id()));
        let (pack, root) = (tmp.join("pack"), tmp.join("installed"));
        let _ = fs::remove_dir_all(&tmp);
        for set in cmr_hdpack::SETS {
            fs::create_dir_all(pack.join(set)).unwrap();
            fs::write(pack.join(set).join("0123456789abcdef.png"), png([1, 2, 3])).unwrap();
        }
        let list = |extra: &str| {
            format!(
                "{}\nx2/0123456789abcdef.png\nx4/0123456789abcdef.png\n{extra}",
                cmr_hdpack::HEADER
            )
        };
        fs::write(pack.join(cmr_hdpack::LIST), list("")).unwrap();
        let first = install(&pack, &root).unwrap();
        let bmp = fs::read(first.join("x4").join("0123456789abcdef.bmp")).unwrap();
        assert_eq!(bmp, cmr_hdpack::bmp24(1, 1, &[[1, 2, 3]]));
        // Installed: not done again.
        fs::remove_file(first.join("x2").join("0123456789abcdef.bmp")).unwrap();
        assert_eq!(install(&pack, &root).unwrap(), first);
        assert!(!first.join("x2").join("0123456789abcdef.bmp").exists());
        // A new pack replaces it.
        fs::write(pack.join(cmr_hdpack::LIST), list("\n")).unwrap();
        let second = install(&pack, &root).unwrap();
        assert_ne!(second, first);
        assert!(!first.exists() && second.join("x2").join("0123456789abcdef.bmp").is_file());
        // Entries can't leave the pack.
        fs::write(pack.join(cmr_hdpack::LIST), list("x2/../../evil.png\n")).unwrap();
        assert!(install(&pack, &root).is_err());
        assert!(install(&tmp, &root).is_err());
        let _ = fs::remove_dir_all(&tmp);
    }
}
