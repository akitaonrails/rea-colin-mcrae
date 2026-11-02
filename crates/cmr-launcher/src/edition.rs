//! Identifies the user's `Rally.exe` by its SHA-256 (`rea_launcher::identify`).
//! Only the builds below are patched; anything else is refused with its
//! digest and the issue link ([`refusal`]), never patched by guesswork.

use rea_launcher::identify::{self, Known};

use crate::fixes::{COMMUNITY_EU_15, FixSet, OFFICIAL_EU_15};

/// Where players report a build the launcher doesn't know (the
/// unknown-edition issue form).
pub const REPORT_URL: &str =
    "https://github.com/akitaonrails/rea-colin-mcrae/issues/new?template=unknown-edition.yml";

/// A known executable and the in-memory fixes it needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edition {
    pub name: &'static str,
    pub fixes: &'static FixSet,
}

pub static EDITIONS: [Known<Edition>; 2] = [
    Known::new(
        "0eb6ea9652219b9a35c0a6134bf0cdf316dce21215b724cdfc10ba85eb95982a",
        Edition {
            name: "Official EU v1.5",
            fixes: &OFFICIAL_EU_15,
        },
    ),
    // The EU v1.5 executable from a community pack, already patched on
    // disk with the equivalent of F1-F5.
    Known::new(
        "46500c8e22ce3e943b6855eca63ae9288261162347ca76fa36cad77cbb19c0bb",
        Edition {
            name: "EU v1.5, community-patched",
            fixes: &COMMUNITY_EU_15,
        },
    ),
];

pub use rea_launcher::identify::{hex as to_hex, sha256};

/// The known edition of `exe`, or its SHA-256.
pub fn identify(exe: &[u8]) -> Result<&'static Edition, [u8; 32]> {
    identify::identify(exe, &EDITIONS)
}

/// The message for a `Rally.exe` the launcher doesn't know.
pub fn refusal(digest: &[u8; 32]) -> String {
    format!(
        "This Rally.exe is not a version the launcher knows yet, so it is not patched.\n\n\
         SHA-256: {}\n\n\
         Please report it, with that SHA-256, at\n{REPORT_URL}",
        to_hex(digest)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editions_have_distinct_hashes() {
        for (i, a) in EDITIONS.iter().enumerate() {
            for b in &EDITIONS[i + 1..] {
                assert_ne!(a.sha256, b.sha256, "{} / {}", a.value.name, b.value.name);
            }
        }
    }

    #[test]
    fn unknown_exe_reports_its_hash_and_where_to_report_it() {
        let err = identify(b"not a game").unwrap_err();
        assert_eq!(err, sha256(b"not a game"));
        let text = refusal(&err);
        assert!(text.contains(&to_hex(&err)), "{text}");
        assert!(text.contains(REPORT_URL), "{text}");
    }
}
