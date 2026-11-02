//! The runtime folder: a private, writable copy of the user's game folder,
//! kept by the SDK's `rea_launcher::runtime` with this game's [`Policy`].
//!
//! The user's folder is never written. Files are copied (not linked): the
//! game writes into its own folder (`save\`), and this launcher has always
//! copied everything. A manifest (`.cmr-manifest`, the SDK's format, which
//! is the one this launcher wrote) records each copied file, so later
//! launches copy only what changed there. Files under `save\` are copied
//! once and then belong to the game; the player's saves folder is mirrored
//! with them before and after each run ([`saves_mirror`]).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use rea_launcher::paths;
use rea_launcher::runtime::{self as sdk, Listing, Policy, Stamp, SyncReport};
use rea_launcher::saves::{FirstRun, Mirror};

pub const MANIFEST: &str = ".cmr-manifest";

/// The saves mirror's record of the last sync (in the runtime folder).
pub const SAVES_STATE: &str = ".cmr-saves-state";

/// `OpenFile` keeps the path in a 128-byte buffer.
pub const MAX_GAME_PATH: usize = 127;

/// What the game does with its own files.
pub struct Cmr;

/// The game's own `save` folder, in any spelling.
fn is_save(rel: &str) -> bool {
    rel.split('/')
        .next()
        .is_some_and(|top| top.eq_ignore_ascii_case("save"))
}

impl Policy for Cmr {
    /// `save\cmRally.cfg` and `SAVE\SAVESLOT*.cms`: copied once, then the
    /// game's.
    fn game_written(&self, rel: &str, _source: Stamp) -> bool {
        is_save(rel)
    }
}

/// Brings `runtime` up to date with `game`, then makes sure the game's
/// save folder exists.
pub fn sync(game: &Path, runtime: &Path) -> io::Result<SyncReport> {
    let report = sdk::sync(game, runtime, &Cmr, MANIFEST)?;
    save_dir(runtime)?;
    Ok(report)
}

/// Every file below `root`.
pub fn list(root: &Path) -> io::Result<Listing> {
    sdk::list(root)
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

/// The runtime folder's save folder, in whatever spelling it has; created
/// as `save` when missing. The game writes `save\cmRally.cfg` and
/// `SAVE\SAVESLOT*.cms` with `OpenFile`, which does not create folders;
/// the 1998 installer made this one. Any existing spelling is kept (Wine
/// on Linux is case-sensitive on disk).
pub fn save_dir(runtime: &Path) -> io::Result<PathBuf> {
    if let Some(dir) = paths::find_ignoring_case(runtime, "save").filter(|d| d.is_dir()) {
        return Ok(dir);
    }
    let dir = runtime.join("save");
    fs::create_dir_all(&dir)
        .map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", dir.display())))?;
    Ok(dir)
}

/// The mirror between the runtime's save folder and the player's saves
/// folder. A file changed on one side since the last sync wins; with no
/// record yet (the first run, or saves from launcher 0.7 and older, which
/// kept none) the saves folder's copy wins, as those launchers did.
pub fn saves_mirror<'a>(rt_save: &'a Path, saves: &'a Path, state: &'a Path) -> Mirror<'a> {
    Mirror::folder(rt_save, saves, state, FirstRun::SavesWin)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    fn temp(name: &str) -> PathBuf {
        let p =
            std::env::temp_dir().join(format!("cmr-launcher-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        p
    }

    fn touch_later(path: &Path) {
        let f = fs::OpenOptions::new().write(true).open(path).unwrap();
        f.set_modified(SystemTime::now() + Duration::from_secs(10))
            .unwrap();
    }

    #[test]
    fn longest_path_counts_separator() {
        let l: Listing = [
            ("ab".to_string(), Stamp { size: 0, mtime: 0 }),
            ("Game/x".to_string(), Stamp { size: 0, mtime: 0 }),
        ]
        .into();
        assert_eq!(
            longest_path(Path::new("C:\\R\\"), &l),
            Some((11, "Game/x".into()))
        );
    }

    #[test]
    fn sync_copies_then_keeps_saves() {
        let game = temp("game");
        let rt = temp("rt");
        fs::create_dir_all(game.join("save")).unwrap();
        fs::write(game.join("Rally.exe"), b"exe").unwrap();
        fs::write(game.join("save/slot"), b"orig").unwrap();

        assert_eq!(sync(&game, &rt).unwrap().brought, 2);
        assert_eq!(sync(&game, &rt).unwrap().brought, 0);

        // The game writes its save; the source changes too.
        fs::write(rt.join("save/slot"), b"played").unwrap();
        fs::write(game.join("save/slot"), b"changed").unwrap();
        fs::write(game.join("Rally.exe"), b"exe2").unwrap();
        touch_later(&game.join("Rally.exe"));

        assert_eq!(sync(&game, &rt).unwrap().brought, 1);
        assert_eq!(fs::read(rt.join("save/slot")).unwrap(), b"played");
        assert_eq!(fs::read(rt.join("Rally.exe")).unwrap(), b"exe2");
        assert_eq!(fs::read(game.join("save/slot")).unwrap(), b"changed");

        fs::remove_dir_all(&game).unwrap();
        fs::remove_dir_all(&rt).unwrap();
    }

    /// Launcher 0.7 wrote the same manifest (`size\tmtime\trel`): an
    /// existing runtime copy is not copied again.
    #[test]
    fn a_runtime_from_launcher_0_7_is_kept() {
        let game = temp("old-game");
        let rt = temp("old-rt");
        fs::create_dir_all(&game).unwrap();
        fs::create_dir_all(&rt).unwrap();
        fs::write(game.join("Rally.exe"), b"exe").unwrap();
        fs::copy(game.join("Rally.exe"), rt.join("Rally.exe")).unwrap();
        let stamp = Stamp::of(&fs::metadata(game.join("Rally.exe")).unwrap());
        fs::write(
            rt.join(MANIFEST),
            format!("{}\t{}\tRally.exe\n", stamp.size, stamp.mtime),
        )
        .unwrap();
        assert_eq!(sync(&game, &rt).unwrap().brought, 0);
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
        assert_eq!(save_dir(&rt2).unwrap().file_name().unwrap(), "SAVE");

        for d in [game, rt, rt2] {
            fs::remove_dir_all(d).unwrap();
        }
    }

    /// The player's story: saves made with launcher 0.7 (no state record)
    /// win over the runtime's at the first launch, as they did; then the
    /// game writes a save and it reaches the saves folder; a session killed
    /// before the write-back is saved at the next launch, not lost.
    #[test]
    fn saves_follow_the_player_across_launches() {
        let rt = temp("mirror-rt");
        let saves = temp("mirror-saves");
        let rt_save = rt.join("save");
        let state = rt.join(SAVES_STATE);
        fs::create_dir_all(&rt_save).unwrap();
        fs::create_dir_all(&saves).unwrap();
        fs::write(rt_save.join("cmRally.cfg"), b"runtime").unwrap();
        fs::write(saves.join("cmRally.cfg"), b"saved").unwrap();
        fs::write(saves.join("SAVESLOT1.cms"), b"slot").unwrap();

        let mirror = saves_mirror(&rt_save, &saves, &state);
        mirror.before().unwrap();
        assert_eq!(fs::read(rt_save.join("cmRally.cfg")).unwrap(), b"saved");
        assert_eq!(fs::read(rt_save.join("SAVESLOT1.cms")).unwrap(), b"slot");

        // The game plays and writes.
        fs::write(rt_save.join("SAVESLOT1.cms"), b"slot after a rally").unwrap();
        assert_eq!(mirror.after().unwrap(), ["SAVESLOT1.cms"]);
        assert_eq!(
            fs::read(saves.join("SAVESLOT1.cms")).unwrap(),
            b"slot after a rally"
        );

        // A session ends before the write-back (killed): the next launch
        // keeps the runtime's newer save instead of overwriting it.
        fs::write(rt_save.join("SAVESLOT1.cms"), b"unsaved progress").unwrap();
        touch_later(&rt_save.join("SAVESLOT1.cms"));
        mirror.before().unwrap();
        assert_eq!(
            fs::read(saves.join("SAVESLOT1.cms")).unwrap(),
            b"unsaved progress"
        );

        // A new saves folder starts from what the game has.
        let fresh = temp("mirror-fresh");
        let state2 = rt.join(".other-state");
        saves_mirror(&rt_save, &fresh, &state2).before().unwrap();
        assert_eq!(fs::read(fresh.join("cmRally.cfg")).unwrap(), b"saved");

        for d in [rt, saves, fresh] {
            fs::remove_dir_all(d).unwrap();
        }
    }

    #[test]
    fn paths_with_spaces_apostrophes_and_accents_work() {
        let root = temp("José");
        let game = root.join("Jogos d'Ação").join("Colin McRae Rally");
        let rt = root.join("Runtime Pasta");
        fs::create_dir_all(game.join("Game")).unwrap();
        fs::write(game.join("Rally.exe"), b"exe").unwrap();
        fs::write(game.join("Game").join("pista ç.c3d"), b"x").unwrap();
        rea_launcher::paths::check_separate(&game, &rt).unwrap();
        assert_eq!(sync(&game, &rt).unwrap().brought, 2);
        assert!(rt.join("Game").join("pista ç.c3d").is_file());
        assert!(rea_launcher::paths::check_separate(&game, &game.join("rt")).is_err());
        fs::remove_dir_all(&root).unwrap();
    }
}
