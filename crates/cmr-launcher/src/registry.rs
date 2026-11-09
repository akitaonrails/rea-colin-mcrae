//! The registry values the game reads at startup.
//!
//! Key: `HKLM\SOFTWARE\Codemasters\Rally\1.00` in the 32-bit view
//! (`WOW6432Node` on 64-bit Windows), or its HKCU VirtualStore twin when
//! HKLM is not writable without administrator rights (`rea_launcher_win32::reg`). Missing `Language` or `Hardware` makes
//! the game read uninitialized memory, so both always exist after a launch.

pub const KEY: &str = r"SOFTWARE\Codemasters\Rally\1.00";

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
}
