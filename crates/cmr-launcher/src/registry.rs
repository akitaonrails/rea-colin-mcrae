//! The registry values the game reads at startup.
//!
//! The game reads `HKLM\SOFTWARE\Codemasters\Rally\1.00`. With the hook
//! library loaded (the usual case) it reads `HKCU\Software\Codemasters\
//! Rally\1.00` instead (`cmr-hooks`' `registry`), so no administrator
//! rights are needed and no machine-wide key can win. Without it, HKLM in
//! the 32-bit view (`WOW6432Node` on 64-bit Windows); the HKCU VirtualStore
//! is never written: whether the game reads it depends on UAC
//! virtualization, which the launcher can't see (issue #6). Missing
//! `Language` or `Hardware` makes the game read uninitialized memory, so
//! both always exist after a launch, and every write is read back.

pub const KEY: &str = r"SOFTWARE\Codemasters\Rally\1.00";

/// The key the game reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reads {
    /// `HKCU\Software\Codemasters\Rally\1.00`, through the hook library.
    User,
    /// `HKLM\SOFTWARE\Codemasters\Rally\1.00`, 32-bit view (no hook library).
    Machine,
}

impl Reads {
    /// [`Reads::User`] when the hook library loads with the game.
    pub fn of(hooks: bool) -> Reads {
        if hooks { Reads::User } else { Reads::Machine }
    }

    /// The key, for `launcher.log` and messages.
    pub fn describe(self) -> &'static str {
        match self {
            Reads::User => {
                r"HKCU\Software\Codemasters\Rally\1.00 (cmrhooks.dll gives it to the game)"
            }
            Reads::Machine => r"HKLM\SOFTWARE\Codemasters\Rally\1.00 (32-bit view)",
        }
    }
}

/// `values` with each game-owned value ([`When::IfMissing`]) taken from
/// `previous` (where the game read it before: an earlier launcher's HKLM or
/// VirtualStore copy) when it has one, so moving to the player's key keeps
/// the game's language and first-run state. Our paths are never carried.
pub fn carried_over(values: Vec<Value>, previous: impl Fn(&str) -> Option<String>) -> Vec<Value> {
    values
        .into_iter()
        .map(|mut v| {
            if v.when == When::IfMissing
                && let Some(old) = previous(v.name).filter(|old| !old.is_empty())
            {
                v.data = old;
            }
            v
        })
        .collect()
}

/// Reads back what the game will see (`read`, from `reads`' key) and
/// compares it with what was written: our paths exactly, the game's own
/// values present. The error is the message for the player.
pub fn verify(
    values: &[Value],
    reads: Reads,
    read: impl Fn(&str) -> Result<Option<String>, String>,
) -> Result<(), String> {
    let mut wrong = Vec::new();
    for v in values {
        match read(v.name) {
            Ok(Some(got)) if v.when == When::IfMissing && !got.is_empty() => {}
            Ok(Some(got)) if got == v.data => {}
            Ok(Some(got)) => wrong.push(format!("{} is \"{got}\", not \"{}\"", v.name, v.data)),
            Ok(None) => wrong.push(format!("{} is missing", v.name)),
            Err(e) => wrong.push(format!("{} can't be read: {e}", v.name)),
        }
    }
    if wrong.is_empty() {
        return Ok(());
    }
    Err(format!(
        "The game would not find its folders. It reads {}, and after writing it the launcher \
         reads back:\n\n{}\n\nThe game would crash at start-up (0xC0000005 at 0x49893a). \
         Please report this with launcher.log: \
         https://github.com/akitaonrails/rea-colin-mcrae/issues",
        reads.describe(),
        wrong.join("\n")
    ))
}

/// The message when the game reads HKLM (the hook library is missing) and
/// the launcher can't write it.
pub fn machine_denied() -> String {
    format!(
        "cmrhooks.dll is missing next to the launcher. Without it the game reads its folders \
         from {}, which only an administrator can write, and would crash at start-up. \
         Put the launcher's files back together (unpack the release again), or start the \
         launcher once as administrator.",
        Reads::Machine.describe()
    )
}

/// `--check`'s registry verdict: ready when the game will see our values
/// once Play writes them. `machine_matches`: HKLM already holds them.
pub fn check(reads: Reads, wine: bool, machine_matches: bool) -> Result<(), String> {
    match reads {
        Reads::User => Ok(()),
        Reads::Machine if wine || machine_matches => Ok(()),
        Reads::Machine => Err(machine_denied()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum When {
    /// Our value, written on every launch.
    Always,
    /// The game's own setting: seeded once, then left to the game.
    IfMissing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Value {
    pub name: &'static str,
    pub data: String,
    pub when: When,
}

pub const LANGUAGES: [&str; 5] = ["ENGLISH", "FRENCH", "GERMAN", "SPANISH", "ITALIAN"];

/// The longest folder path the game reads back. It reads each path value
/// into a 100-byte buffer (`FUN_00496b50`, the terminating NUL included);
/// a longer one comes back empty, the game falls back to its developers'
/// folder `c:\codemast\cmrally`, can't open `ini\loading.pal` there and
/// dereferences null: 0xC0000005 at `0x49893a` before anything is drawn
/// (issue #5, reproduced under Wine with a 103-character runtime folder).
pub const MAX_PATH_CHARS: usize = 99;

/// The runtime folder to write into the registry: `long` when the game can
/// read it back (at most [`MAX_PATH_CHARS`]), else `short`, Windows' 8.3
/// name for the same folder, when that fits. ASCII-only paths are
/// preferred: the game reads the values through the ANSI code page, so a
/// short name is used for a long path with other characters when there is
/// one. The error is the message for the player.
pub fn root_for(long: &str, short: Option<&str>) -> Result<String, String> {
    let long = long.trim_end_matches('\\');
    let short = short
        .map(|s| s.trim_end_matches('\\'))
        .filter(|s| s.is_ascii() && s.chars().count() <= MAX_PATH_CHARS);
    let fits = long.chars().count() <= MAX_PATH_CHARS;
    match (fits, short) {
        (true, Some(short)) if !long.is_ascii() => Ok(short.to_owned()),
        (true, _) => Ok(long.to_owned()),
        (false, Some(short)) => Ok(short.to_owned()),
        (false, None) => Err(format!(
            "The launcher's copy of the game is in a folder whose path is {} characters long:\n\n\
             {long}\n\n\
             Colin McRae Rally can only read a folder path of up to {MAX_PATH_CHARS} characters, \
             and Windows has no short name for this one. Choose a shorter folder for the \
             launcher's copy (the runtime setting, or --runtime), for example C:\\Games\\CMR.",
            long.chars().count()
        )),
    }
}

/// The values for a runtime folder at `root` (a Windows path).
pub fn values(root: &str, language: &str) -> Vec<Value> {
    let root = root.trim_end_matches('\\').to_owned();
    let path = |name| Value {
        name,
        data: root.clone(),
        when: When::Always,
    };
    vec![
        path("CDPath"),
        path("HDPath"),
        path("AVIPath"),
        path("WAVPath"),
        Value {
            name: "Language",
            data: language.to_owned(),
            when: When::IfMissing,
        },
        // The detail tier the first-run defaults start from; the community
        // packs seed 5.
        Value {
            name: "Hardware",
            data: "5".into(),
            when: When::IfMissing,
        },
        // "0" makes the game apply its first-run defaults and set "1" itself.
        Value {
            name: "Initialised",
            data: "0".into(),
            when: When::IfMissing,
        },
    ]
}

/// Writes the game's values where it reads them and reads them back
/// (issue #6). HKLM is never written without the rights for it, and no
/// VirtualStore copy the game may not see is made.
#[cfg(windows)]
pub fn write(reads: Reads, values: Vec<Value>) -> Result<(), String> {
    use rea_launcher_win32::reg::Key;
    let written = match reads {
        // The game's own values from where an earlier launcher put them.
        Reads::User => {
            let before = Key::read_machine(KEY).ok();
            carried_over(values.clone(), |name| {
                before.as_ref().and_then(|b| b.string(name).ok().flatten())
            })
        }
        Reads::Machine => values.clone(),
    };
    let key = Key::create(hive(reads), KEY).map_err(|e| {
        if reads == Reads::Machine && e.is_access_denied() {
            machine_denied()
        } else {
            e.to_string()
        }
    })?;
    for v in &written {
        match v.when {
            When::Always => key.set_string(v.name, &v.data)?,
            When::IfMissing => {
                key.set_string_if_missing(v.name, &v.data)?;
            }
        }
    }
    drop(key);
    read_back(reads, &values)
}

#[cfg(windows)]
fn hive(reads: Reads) -> rea_launcher_win32::reg::Hive {
    use rea_launcher_win32::reg::Hive;
    match reads {
        Reads::User => Hive::CurrentUser,
        Reads::Machine => Hive::LocalMachine,
    }
}

/// [`verify`] against the key the game reads, opened read-only.
#[cfg(windows)]
pub fn read_back(reads: Reads, values: &[Value]) -> Result<(), String> {
    let key = rea_launcher_win32::reg::Key::open_read(hive(reads), KEY)?;
    verify(values, reads, |name| match &key {
        Some(key) => key.string(name).map_err(|e| e.to_string()),
        None => Ok(None),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_point_at_the_runtime_root_without_trailing_backslash() {
        let v = values(r"C:\CMR\Rally\", "ENGLISH");
        for name in ["CDPath", "HDPath", "AVIPath", "WAVPath"] {
            let value = v.iter().find(|x| x.name == name).unwrap();
            assert_eq!(value.data, r"C:\CMR\Rally");
            assert_eq!(value.when, When::Always);
        }
    }

    #[test]
    fn a_path_the_game_can_read_back_is_kept() {
        let long = r"C:\Users\player\AppData\Local\CMRLauncher\Rally";
        assert_eq!(root_for(long, None).unwrap(), long);
        assert_eq!(
            root_for(&format!("{long}\\"), Some(r"C:\SHORT~1")).unwrap(),
            long
        );
        // Exactly the limit fits.
        let at_limit = format!(r"C:\{}", "a".repeat(MAX_PATH_CHARS - 3));
        assert_eq!(root_for(&at_limit, None).unwrap(), at_limit);
    }

    #[test]
    fn a_path_longer_than_the_game_reads_uses_the_short_name() {
        // Issue #5: 100+ characters came back empty and the game crashed.
        let long = format!(r"C:\Games\{}\_runtime\CMR", "x".repeat(90));
        assert!(long.len() > MAX_PATH_CHARS);
        assert_eq!(
            root_for(&long, Some(r"C:\Games\XXXXXX~1\_RUNTI~1\CMR")).unwrap(),
            r"C:\Games\XXXXXX~1\_RUNTI~1\CMR"
        );
        // No usable short name: a message, never a path the game can't read.
        let err = root_for(&long, None).unwrap_err();
        assert!(
            err.contains(&format!("{} characters", long.len())) && err.contains("--runtime"),
            "{err}"
        );
        assert!(
            root_for(&long, Some(&long)).is_err(),
            "a short name as long as the path doesn't help"
        );
    }

    #[test]
    fn a_non_ascii_path_prefers_the_short_name_but_keeps_working_without_one() {
        let accented = r"C:\Users\José\AppData\Local\CMRLauncher\Rally";
        assert_eq!(
            root_for(
                accented,
                Some(r"C:\Users\JOS~1\AppData\Local\CMRLAU~1\Rally")
            )
            .unwrap(),
            r"C:\Users\JOS~1\AppData\Local\CMRLAU~1\Rally"
        );
        // As before: the game reads it through the ANSI code page.
        assert_eq!(root_for(accented, None).unwrap(), accented);
    }

    #[test]
    fn game_owned_values_are_only_seeded() {
        let v = values(r"C:\R", "GERMAN");
        for name in ["Language", "Hardware", "Initialised"] {
            assert_eq!(
                v.iter().find(|x| x.name == name).unwrap().when,
                When::IfMissing
            );
        }
        assert_eq!(
            v.iter().find(|x| x.name == "Language").unwrap().data,
            "GERMAN"
        );
    }

    fn read_from<'a>(
        pairs: &'a [(&'a str, &'a str)],
    ) -> impl Fn(&str) -> Result<Option<String>, String> + 'a {
        move |name| {
            Ok(pairs
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, v)| v.to_string()))
        }
    }

    #[test]
    fn with_the_hook_library_the_game_reads_the_players_own_key() {
        // Issue #6: no HKLM, no VirtualStore, no elevation.
        assert_eq!(Reads::of(true), Reads::User);
        assert_eq!(Reads::of(false), Reads::Machine);
        assert!(
            Reads::User
                .describe()
                .starts_with(r"HKCU\Software\Codemasters\Rally\1.00")
        );
        assert!(
            Reads::Machine
                .describe()
                .starts_with(r"HKLM\SOFTWARE\Codemasters\Rally\1.00")
        );
    }

    #[test]
    fn the_games_own_values_carry_over_from_where_it_read_them_before() {
        let previous = |name: &str| match name {
            "Language" => Some("FRENCH".to_string()),
            "Initialised" => Some("1".to_string()),
            // An old launcher's path never wins over the current one.
            "CDPath" => Some(r"D:\old".to_string()),
            "Hardware" => Some(String::new()),
            _ => None,
        };
        let v = carried_over(values(r"C:\R", "ENGLISH"), previous);
        let get = |name| v.iter().find(|x| x.name == name).unwrap().data.as_str();
        assert_eq!(get("Language"), "FRENCH");
        assert_eq!(get("Initialised"), "1");
        assert_eq!(get("CDPath"), r"C:\R");
        // Empty is missing: the default stays.
        assert_eq!(get("Hardware"), "5");
    }

    #[test]
    fn a_read_back_that_matches_is_ready() {
        let v = values(r"C:\R", "ENGLISH");
        let pairs = [
            ("CDPath", r"C:\R"),
            ("HDPath", r"C:\R"),
            ("AVIPath", r"C:\R"),
            ("WAVPath", r"C:\R"),
            // The game's own values may differ from the seeds.
            ("Language", "GERMAN"),
            ("Hardware", "3"),
            ("Initialised", "1"),
        ];
        assert_eq!(verify(&v, Reads::User, read_from(&pairs)), Ok(()));
    }

    #[test]
    fn a_read_back_that_differs_or_is_missing_says_what_and_where() {
        let v = values(r"C:\R", "ENGLISH");
        // Issue #6's stale key: a folder from an old install.
        let stale = [
            ("CDPath", r"c:\codemast\cmrally"),
            ("HDPath", r"C:\R"),
            ("AVIPath", r"C:\R"),
            ("WAVPath", r"C:\R"),
            ("Language", "ENGLISH"),
            ("Hardware", "5"),
            ("Initialised", "1"),
        ];
        let err = verify(&v, Reads::Machine, read_from(&stale)).unwrap_err();
        assert!(
            err.contains("CDPath") && err.contains(r"c:\codemast\cmrally"),
            "{err}"
        );
        assert!(
            err.contains(r"HKLM\SOFTWARE\Codemasters\Rally\1.00"),
            "{err}"
        );
        // Nothing there at all.
        let err = verify(&v, Reads::User, read_from(&[])).unwrap_err();
        assert!(err.contains("CDPath") && err.contains("missing"), "{err}");
        // A read that fails is not a match.
        let err = verify(&v, Reads::User, |_| Err("error 5".to_string())).unwrap_err();
        assert!(err.contains("error 5"), "{err}");
    }

    #[test]
    fn check_is_ready_only_when_the_game_will_see_the_values() {
        // The hook answers from the player's key: nothing machine-wide needed.
        assert_eq!(check(Reads::User, false, false), Ok(()));
        // Wine: the prefix's HKLM is writable.
        assert_eq!(check(Reads::Machine, true, false), Ok(()));
        // Windows without the hook library: only a key that already matches.
        assert_eq!(check(Reads::Machine, false, true), Ok(()));
        let err = check(Reads::Machine, false, false).unwrap_err();
        assert!(
            err.contains("cmrhooks.dll") && err.contains("administrator"),
            "{err}"
        );
    }

    /// Under Wine (`sdk/scripts/wine-test`, its own prefix): the player's
    /// key is written and read back, the game's own values come over from
    /// HKLM where an earlier launcher left them, and HKLM is not touched.
    #[cfg(windows)]
    #[test]
    fn the_players_key_is_written_read_back_and_takes_the_old_values() {
        use rea_launcher_win32::reg::{Hive, Key};
        let names = [
            "CDPath",
            "HDPath",
            "AVIPath",
            "WAVPath",
            "Language",
            "Hardware",
            "Initialised",
        ];
        let machine = Key::create(Hive::LocalMachine, KEY).unwrap();
        let user = Key::create(Hive::CurrentUser, KEY).unwrap();
        for name in names {
            machine.delete_value(name).unwrap();
            user.delete_value(name).unwrap();
        }
        machine.set_string("Language", "FRENCH").unwrap();
        machine.set_string("CDPath", r"D:\old").unwrap();

        write(Reads::User, values(r"C:\R", "ENGLISH")).unwrap();
        assert_eq!(user.string("CDPath").unwrap().as_deref(), Some(r"C:\R"));
        assert_eq!(user.string("Language").unwrap().as_deref(), Some("FRENCH"));
        assert_eq!(user.string("Initialised").unwrap().as_deref(), Some("0"));
        assert_eq!(
            machine.string("CDPath").unwrap().as_deref(),
            Some(r"D:\old")
        );
        assert_eq!(read_back(Reads::User, &values(r"C:\R", "ENGLISH")), Ok(()));

        // The game's own change survives the next launch.
        user.set_string("Initialised", "1").unwrap();
        write(Reads::User, values(r"C:\R2", "ENGLISH")).unwrap();
        assert_eq!(user.string("Initialised").unwrap().as_deref(), Some("1"));
        assert_eq!(user.string("WAVPath").unwrap().as_deref(), Some(r"C:\R2"));

        // HKLM still holds a stale folder: read back, it is not ready.
        let err = read_back(Reads::Machine, &values(r"C:\R2", "ENGLISH")).unwrap_err();
        assert!(err.contains(r"D:\old"), "{err}");
        // Without the hook library the game reads HKLM; Wine lets us write it.
        write(Reads::Machine, values(r"C:\R2", "ENGLISH")).unwrap();
        assert_eq!(machine.string("CDPath").unwrap().as_deref(), Some(r"C:\R2"));
    }
}
