//! Identifies the user's `Rally.exe` by its SHA-256.

use sha2::{Digest, Sha256};

use crate::fixes::{COMMUNITY_EU_15, FixSet, OFFICIAL_EU_15};

/// A known executable and the in-memory fixes it needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edition {
    pub name: &'static str,
    pub sha256: [u8; 32],
    pub fixes: &'static FixSet,
}

const fn hex32(s: &str) -> [u8; 32] {
    let b = s.as_bytes();
    assert!(b.len() == 64);
    let mut out = [0u8; 32];
    let mut i = 0;
    while i < 32 {
        out[i] = (nibble(b[2 * i]) << 4) | nibble(b[2 * i + 1]);
        i += 1;
    }
    out
}

const fn nibble(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        _ => panic!("lowercase hex expected"),
    }
}

pub static EDITIONS: &[Edition] = &[
    Edition {
        name: "Official EU v1.5",
        sha256: hex32("0eb6ea9652219b9a35c0a6134bf0cdf316dce21215b724cdfc10ba85eb95982a"),
        fixes: &OFFICIAL_EU_15,
    },
    Edition {
        // The EU v1.5 executable from a community pack, already patched on
        // disk with the equivalent of F1-F5.
        name: "EU v1.5, community-patched",
        sha256: hex32("46500c8e22ce3e943b6855eca63ae9288261162347ca76fa36cad77cbb19c0bb"),
        fixes: &COMMUNITY_EU_15,
    },
];

pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

pub fn to_hex(digest: &[u8; 32]) -> String {
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn identify(exe: &[u8]) -> Result<&'static Edition, [u8; 32]> {
    let digest = sha256(exe);
    EDITIONS.iter().find(|e| e.sha256 == digest).ok_or(digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips() {
        let h = "0eb6ea9652219b9a35c0a6134bf0cdf316dce21215b724cdfc10ba85eb95982a";
        assert_eq!(to_hex(&hex32(h)), h);
    }

    #[test]
    fn editions_have_distinct_hashes() {
        for (i, a) in EDITIONS.iter().enumerate() {
            for b in &EDITIONS[i + 1..] {
                assert_ne!(a.sha256, b.sha256, "{} / {}", a.name, b.name);
            }
        }
    }

    #[test]
    fn unknown_exe_reports_its_hash() {
        let err = identify(b"not a game").unwrap_err();
        assert_eq!(err, sha256(b"not a game"));
    }
}
