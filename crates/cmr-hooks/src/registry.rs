//! The game's registry key, answered from the player's own hive.
//!
//! `Rally.exe` reads its folders and first-run state from
//! `HKLM\SOFTWARE\Codemasters\Rally\1.00`, opening one level at a time:
//! `RegOpenKeyExA(HKLM, "SOFTWARE")`, then `Codemasters`, `Rally`, `1.00`
//! relative to each handle (`0x496740`, `0x496890`, `0x496b50`), and
//! `RegOpenKeyA(HKLM, "Software")` … to set `Initialised` (`0x4bf359`).
//! Without administrator rights HKLM can't be written, and whether Windows
//! shows the game a per-user VirtualStore copy instead depends on UAC
//! virtualization, which the launcher can neither control nor see (issue
//! #6). So the hook sends the game's first step to `HKCU\Software`: the
//! rest of the walk lands on `HKCU\Software\Codemasters\Rally\1.00`, the
//! key the launcher writes and reads back, writable by the game too.

/// `HKEY_CURRENT_USER`.
pub const HKCU: u32 = 0x8000_0001;
/// `HKEY_LOCAL_MACHINE`.
pub const HKLM: u32 = 0x8000_0002;

/// Where an open of `subkey` under the predefined key `root` goes instead:
/// `HKLM` + `SOFTWARE` (any case) becomes `HKCU` + `Software`; anything
/// else is left alone (`None`). `subkey` is the bytes before the NUL.
pub fn redirect(root: u32, subkey: &[u8]) -> Option<(u32, &'static [u8])> {
    (root == HKLM && subkey.eq_ignore_ascii_case(b"SOFTWARE")).then_some((HKCU, b"Software\0"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_games_first_step_goes_to_the_players_hive() {
        // Both spellings the game uses (0x5023c8 and 0x5080d0).
        for name in [&b"SOFTWARE"[..], b"Software"] {
            assert_eq!(redirect(HKLM, name), Some((HKCU, &b"Software\0"[..])));
        }
    }

    #[test]
    fn every_other_open_is_left_alone() {
        // The later steps are relative to the handle the first one gave.
        assert_eq!(redirect(HKLM, b"Codemasters"), None);
        // A longer path under HKLM, another root, another key.
        assert_eq!(redirect(HKLM, b"SOFTWARE\\Codemasters"), None);
        assert_eq!(redirect(HKCU, b"Software"), None);
        assert_eq!(redirect(HKLM, b"SYSTEM"), None);
        assert_eq!(redirect(HKLM, b""), None);
    }
}
