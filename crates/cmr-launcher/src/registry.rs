//! The registry values the game reads at startup.
//!
//! Key: `HKLM\SOFTWARE\Codemasters\Rally\1.00` in the 32-bit view
//! (`WOW6432Node` on 64-bit Windows). Missing `Language` or `Hardware` makes
//! the game read uninitialized memory, so both always exist after a launch.

pub const KEY: &str = r"SOFTWARE\Codemasters\Rally\1.00";

/// Where a 32-bit, unmanifested process's HKLM\SOFTWARE writes and reads are
/// redirected when UAC virtualization is active: used when HKLM itself is
/// not writable without administrator rights.
pub const VIRTUAL_STORE_KEY: &str =
    r"Software\Classes\VirtualStore\MACHINE\SOFTWARE\WOW6432Node\Codemasters\Rally\1.00";

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
