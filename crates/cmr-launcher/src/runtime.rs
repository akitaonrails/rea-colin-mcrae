//! The runtime folder: a private, writable copy of the user's game folder.
//!
//! The user's folder is never written. Files are copied (not linked) because
//! the game writes into its own folder (`save\`), and a hard link would write
//! straight through to the original. A manifest records what each copied
//! file looked like in the source, so later launches copy only what changed
//! there. Files under `save\` are copied once and then belong to the game.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const MANIFEST: &str = ".cmr-manifest";

/// `OpenFile` keeps the path in a 128-byte buffer.
pub const MAX_GAME_PATH: usize = 127;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    pub size: u64,
    pub mtime: u64,
}

/// Relative path (with `/` separators) to its stamp.
pub type Listing = BTreeMap<String, Stamp>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub copy: Vec<String>,
    /// The new manifest, written after the copies succeed.
    pub manifest: Listing,
}

fn is_save(rel: &str) -> bool {
    rel.split('/')
        .next()
        .is_some_and(|top| top.eq_ignore_ascii_case("save"))
}

/// Decides which files to copy.
///
/// - missing in the runtime folder: copy;
/// - changed in the source since the last copy (per the manifest): copy,
///   except save files, which the game owns once they exist.
pub fn plan(source: &Listing, runtime_has: &dyn Fn(&str) -> bool, manifest: &Listing) -> Plan {
    let mut copy = Vec::new();
    let mut next = Listing::new();
    for (rel, stamp) in source {
        let present = runtime_has(rel);
        let changed = manifest.get(rel) != Some(stamp);
        if !present || (changed && !is_save(rel)) {
            copy.push(rel.clone());
            next.insert(rel.clone(), *stamp);
        } else {
            next.insert(rel.clone(), *manifest.get(rel).unwrap_or(stamp));
        }
    }
    Plan {
        copy,
        manifest: next,
    }
}

pub fn parse_manifest(text: &str) -> Listing {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            let size = parts.next()?.parse().ok()?;
            let mtime = parts.next()?.parse().ok()?;
            let rel = parts.next()?.to_owned();
            Some((rel, Stamp { size, mtime }))
        })
        .collect()
}

pub fn format_manifest(listing: &Listing) -> String {
    listing
        .iter()
        .map(|(rel, s)| format!("{}\t{}\t{}\n", s.size, s.mtime, rel))
        .collect()
}

/// The longest path the game could open below `root`, in characters.
pub fn longest_path(root: &Path, source: &Listing) -> Option<(usize, String)> {
    let base = root
        .to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .chars()
        .count();
    source
        .keys()
        .map(|rel| (base + 1 + rel.chars().count(), rel.clone()))
        .max()
}

fn mtime_secs(meta: &fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs())
}

/// Lists every file below `root`.
pub fn list(root: &Path) -> io::Result<Listing> {
    fn walk(root: &Path, dir: &Path, out: &mut Listing) -> io::Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let meta = entry.metadata()?;
            if meta.is_dir() {
                walk(root, &path, out)?;
            } else if meta.is_file() {
                let rel = path
                    .strip_prefix(root)
                    .expect("walk stays below root")
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/");
                out.insert(
                    rel,
                    Stamp {
                        size: meta.len(),
                        mtime: mtime_secs(&meta),
                    },
                );
            }
        }
        Ok(())
    }
    let mut out = Listing::new();
    walk(root, root, &mut out)?;
    Ok(out)
}

pub fn native(root: &Path, rel: &str) -> PathBuf {
    rel.split('/')
        .fold(root.to_path_buf(), |p, part| p.join(part))
}

/// Refuses runtime folders that overlap the game folder.
pub fn check_separate(game: &Path, runtime: &Path) -> Result<(), String> {
    let g = fs::canonicalize(game).map_err(|e| format!("game folder {}: {e}", game.display()))?;
    let r = fs::canonicalize(runtime).unwrap_or_else(|_| runtime.to_path_buf());
    if r.starts_with(&g) || g.starts_with(&r) {
        return Err(format!(
            "the runtime folder {} must be outside the game folder {}",
            runtime.display(),
            game.display()
        ));
    }
    Ok(())
}

fn make_writable(path: &Path) -> io::Result<()> {
    let mut perm = fs::metadata(path)?.permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perm.set_readonly(false);
    fs::set_permissions(path, perm)
}

/// Brings `runtime` up to date with `game`. Returns how many files were copied.
pub fn sync(game: &Path, runtime: &Path) -> io::Result<usize> {
    let source = list(game)?;
    fs::create_dir_all(runtime)?;
    let manifest_path = runtime.join(MANIFEST);
    let manifest = fs::read_to_string(&manifest_path)
        .map(|t| parse_manifest(&t))
        .unwrap_or_default();
    let plan = plan(&source, &|rel| native(runtime, rel).is_file(), &manifest);
    for rel in &plan.copy {
        let from = native(game, rel);
        let to = native(runtime, rel);
        if let Some(dir) = to.parent() {
            fs::create_dir_all(dir)?;
        }
        if to.exists() {
            make_writable(&to)?;
        }
        fs::copy(&from, &to)?;
        // Copies of read-only source files (e.g. from a CD) are read-only too.
        make_writable(&to)?;
        let stamp = source[rel];
        let file = fs::OpenOptions::new().write(true).open(&to)?;
        file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(stamp.mtime))?;
    }
    fs::write(&manifest_path, format_manifest(&plan.manifest))?;
    ensure_save_dir(runtime)?;
    Ok(plan.copy.len())
}

/// The game writes `save\cmRally.cfg` and `SAVE\SAVESLOT*.cms` with
/// `OpenFile`, which does not create folders; the 1998 installer made this
/// one. Any existing spelling is kept (Wine on Linux is case-sensitive on
/// disk).
fn ensure_save_dir(runtime: &Path) -> io::Result<()> {
    save_dir(runtime).map(|_| ())
}

/// The runtime folder's save folder, in whatever spelling it has; created
/// as `save` when missing.
pub fn save_dir(runtime: &Path) -> io::Result<PathBuf> {
    for entry in fs::read_dir(runtime)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() && entry.file_name().eq_ignore_ascii_case("save") {
            return Ok(entry.path());
        }
    }
    let dir = runtime.join("save");
    fs::create_dir(&dir)?;
    Ok(dir)
}

/// Copies every file below `from` into `to` (creating folders), skipping
/// files whose copy already has the same size and modification time.
/// Returns how many files were copied. Used to keep the player's save
/// folder (`saves` setting) and the runtime's `save` folder in step.
pub fn mirror(from: &Path, to: &Path) -> io::Result<usize> {
    if !from.is_dir() {
        return Ok(0);
    }
    let mut copied = 0;
    for (rel, stamp) in list(from)? {
        let src = native(from, &rel);
        let dst = native(to, &rel);
        let same = fs::metadata(&dst)
            .map(|m| m.len() == stamp.size && mtime_secs(&m) == stamp.mtime)
            .unwrap_or(false);
        if same {
            continue;
        }
        if let Some(dir) = dst.parent() {
            fs::create_dir_all(dir)?;
        }
        if dst.exists() {
            make_writable(&dst)?;
        }
        fs::copy(&src, &dst)?;
        make_writable(&dst)?;
        let file = fs::OpenOptions::new().write(true).open(&dst)?;
        file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(stamp.mtime))?;
        copied += 1;
    }
    Ok(copied)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stamp(size: u64, mtime: u64) -> Stamp {
        Stamp { size, mtime }
    }

    fn listing(items: &[(&str, Stamp)]) -> Listing {
        items.iter().map(|(r, s)| (r.to_string(), *s)).collect()
    }

    #[test]
    fn first_sync_copies_everything() {
        let src = listing(&[
            ("Rally.exe", stamp(1, 1)),
            ("save/cmRally.cfg", stamp(2, 2)),
        ]);
        let p = plan(&src, &|_| false, &Listing::new());
        assert_eq!(p.copy, ["Rally.exe", "save/cmRally.cfg"]);
        assert_eq!(p.manifest, src);
    }

    #[test]
    fn unchanged_source_copies_nothing() {
        let src = listing(&[("Rally.exe", stamp(1, 1))]);
        let p = plan(&src, &|_| true, &src);
        assert!(p.copy.is_empty());
    }

    #[test]
    fn changed_game_file_is_recopied_but_save_is_kept() {
        let old = listing(&[("Game/a.c3d", stamp(1, 1)), ("SAVE/slot.cms", stamp(1, 1))]);
        let new = listing(&[("Game/a.c3d", stamp(1, 9)), ("SAVE/slot.cms", stamp(5, 9))]);
        let p = plan(&new, &|_| true, &old);
        assert_eq!(p.copy, ["Game/a.c3d"]);
        // The kept save file stays recorded at its old source stamp.
        assert_eq!(p.manifest["SAVE/slot.cms"], stamp(1, 1));
    }

    #[test]
    fn deleted_runtime_file_is_restored() {
        let src = listing(&[("save/x", stamp(1, 1))]);
        let p = plan(&src, &|_| false, &src);
        assert_eq!(p.copy, ["save/x"]);
    }

    #[test]
    fn manifest_round_trips_with_spaces() {
        let l = listing(&[("Game/Images/a b.txt", stamp(10, 20))]);
        assert_eq!(parse_manifest(&format_manifest(&l)), l);
    }

    #[test]
    fn longest_path_counts_separator() {
        let l = listing(&[("ab", stamp(0, 0)), ("Game/x", stamp(0, 0))]);
        assert_eq!(
            longest_path(Path::new("C:\\R\\"), &l),
            Some((11, "Game/x".into()))
        );
    }

    fn temp(name: &str) -> PathBuf {
        let p =
            std::env::temp_dir().join(format!("cmr-launcher-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        p
    }

    #[test]
    fn sync_copies_then_keeps_saves() {
        let game = temp("game");
        let rt = temp("rt");
        fs::create_dir_all(game.join("save")).unwrap();
        fs::write(game.join("Rally.exe"), b"exe").unwrap();
        fs::write(game.join("save/slot"), b"orig").unwrap();

        assert_eq!(sync(&game, &rt).unwrap(), 2);
        assert_eq!(sync(&game, &rt).unwrap(), 0);

        // The game writes its save; the source changes too.
        fs::write(rt.join("save/slot"), b"played").unwrap();
        fs::write(game.join("save/slot"), b"changed").unwrap();
        fs::write(game.join("Rally.exe"), b"exe2").unwrap();
        let f = fs::OpenOptions::new()
            .write(true)
            .open(game.join("Rally.exe"))
            .unwrap();
        f.set_modified(SystemTime::now() + Duration::from_secs(10))
            .unwrap();
        drop(f);

        assert_eq!(sync(&game, &rt).unwrap(), 1);
        assert_eq!(fs::read(rt.join("save/slot")).unwrap(), b"played");
        assert_eq!(fs::read(rt.join("Rally.exe")).unwrap(), b"exe2");
        assert_eq!(fs::read(game.join("save/slot")).unwrap(), b"changed");

        fs::remove_dir_all(&game).unwrap();
        fs::remove_dir_all(&rt).unwrap();
    }

    #[test]
    fn sync_creates_a_missing_save_folder_once() {
        let game = temp("nosave");
        let rt = temp("nosave-rt");
        fs::create_dir_all(&game).unwrap();
        fs::write(game.join("Rally.exe"), b"exe").unwrap();
        sync(&game, &rt).unwrap();
        assert!(rt.join("save").is_dir());
        sync(&game, &rt).unwrap();

        // An existing folder in another spelling is reused.
        let rt2 = temp("nosave-rt2");
        fs::create_dir_all(rt2.join("SAVE")).unwrap();
        sync(&game, &rt2).unwrap();
        let dirs: Vec<_> = fs::read_dir(&rt2)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().unwrap().is_dir())
            .map(|e| e.file_name())
            .collect();
        assert_eq!(dirs, ["SAVE"]);

        for d in [game, rt, rt2] {
            fs::remove_dir_all(d).unwrap();
        }
    }

    #[test]
    fn mirror_copies_new_and_changed_files_only() {
        let a = temp("mirror-a");
        let b = temp("mirror-b");
        fs::create_dir_all(&a).unwrap();
        fs::write(a.join("cmRally.cfg"), b"one").unwrap();
        fs::write(a.join("SAVESLOT1.cms"), b"slot").unwrap();
        assert_eq!(mirror(&a, &b).unwrap(), 2);
        assert_eq!(mirror(&a, &b).unwrap(), 0, "unchanged files are skipped");
        fs::write(a.join("cmRally.cfg"), b"two!").unwrap();
        assert_eq!(mirror(&a, &b).unwrap(), 1);
        assert_eq!(fs::read(b.join("cmRally.cfg")).unwrap(), b"two!");
        assert_eq!(
            mirror(&temp("mirror-none"), &b).unwrap(),
            0,
            "no source folder"
        );
        for d in [a, b] {
            fs::remove_dir_all(d).unwrap();
        }
    }

    #[test]
    fn save_dir_finds_any_spelling() {
        let rt = temp("savedir");
        fs::create_dir_all(rt.join("SAVE")).unwrap();
        assert_eq!(save_dir(&rt).unwrap().file_name().unwrap(), "SAVE");
        fs::remove_dir_all(&rt).unwrap();
    }

    #[test]
    fn overlapping_folders_are_refused() {
        let game = temp("overlap");
        fs::create_dir_all(&game).unwrap();
        assert!(check_separate(&game, &game.join("rt")).is_err());
        assert!(check_separate(&game, &temp("elsewhere")).is_ok());
        fs::remove_dir_all(&game).unwrap();
    }
}
