//! Reconstruction of the music and start-of-stage countdown play paths.
//!
//! Two non-engine playback cues recovered together:
//!
//! - **Music** (frontend/replay): each is a single wave loaded *whole* (not
//!   streamed) through the managed-handle loader variant `FUN_004959f0` and
//!   played **looping** (`QSWaveMixPlayEx` dwFlags `0xffffffff`) on its own
//!   slot/channel pair. Frontend start is `FUN_00443b20` (one-shot init from the
//!   frontend init `FUN_00437ac0`); a second instance `FUN_004c9d70` is a
//!   per-frame keep-alive that re-starts the loop when the channel reports done
//!   and ramps a fade-in. Replay music (`replay.wav`) and the setup-screen
//!   `SETUP.WAV` build their paths inline and load through the same
//!   `FUN_004959f0` variant. All are gated by the sound-disabled flag
//!   `DAT_00d3ca88`.
//! - **Start-of-stage countdown** (`5-4-3-2-1-GO`): preloader `FUN_0046a740`
//!   ("PreSnd") loads six `countdwn\` waves into consecutive slots from a base
//!   (`DAT_00592ed8`) with the base loader `FUN_004958e0(path, 0x2b11, 0x10, 0)`
//!   (11025 Hz / 16-bit / mono), under `<speech base>` + `<language dir>` +
//!   `countdwn\`. In rally-school mode (`DAT_005f1e20 >= 2`) it additionally
//!   preloads the `intro2\` set. Play helper `FUN_0046af10(index)` plays slot
//!   `base + index` **one-shot** (`dwFlags 0`) on a shared SFX channel; the
//!   cadence driver is `FUN_00439b10`.
//!
//! The managed-handle loader `FUN_004959f0` differs from the base loader
//! `FUN_004958e0` by taking fixed format arguments and recording the returned
//! wave handle in a persistent table (stride `0x43`, counter `DAT_00a1f848`,
//! mixer handle `DAT_00a1f840`, ready flag `DAT_00a1f838`) - the "long-lived
//! looped wave" path, vs. the base "bulk SFX/speech slot" path.
//!
//! Observations: music is a single looped wave via `FUN_004959f0`; the two
//! music instances and their distinct globals; the countdown file set, its
//! 11025/16/mono format, one-shot play of slot `base+index`, and the `intro2\`
//! tutorial set. Inferences: `DAT_00d3ca88` is the sound-disabled gate; the two
//! music instances are the attract/menu variants. Unknowns (not reproduced):
//! the music/fade volume scaling (literal constants only, units unknown), the
//! countdown index->value slot mapping and the `FUN_00439b10` tick cadence, and
//! the replay/SETUP control flow (their enclosing functions were not bound by
//! the decompiler). See evidence `rea.official-eu.audio-music-countdown`.

use crate::audio_mixer::{PLAY_LOOPING, PLAY_ONE_SHOT};
use crate::audio_pacenote::SpeechLanguage;

/// Music plays on a sustained loop.
pub const MUSIC_PLAY_FLAGS: u32 = PLAY_LOOPING;

/// A looped music track, each loaded whole through the managed-handle loader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MusicTrack {
    /// Frontend/menu music (`FUN_00443b20` / keep-alive `FUN_004c9d70`).
    Frontend,
    /// Replay music.
    Replay,
}

impl MusicTrack {
    /// The recovered wave path (relative to the installation root), reusing the
    /// catalogued constants in [`crate::audio_assets`].
    pub fn path(self) -> &'static str {
        match self {
            MusicTrack::Frontend => crate::audio_assets::FRONTEND_MUSIC,
            MusicTrack::Replay => crate::audio_assets::REPLAY_MUSIC,
        }
    }
}

/// The directory (under the selected speech-language dir) holding the
/// start-of-stage countdown waves (`countdwn\`).
pub const COUNTDOWN_DIR: &str = r"countdwn\";

/// Recovered sample rate/bit depth/channel layout shared by the countdown and
/// `intro2\` waves (base loader `FUN_004958e0(path, 0x2b11, 0x10, 0)`).
pub const COUNTDOWN_WAVE_RATE_HZ: u32 = 11_025;
pub const COUNTDOWN_WAVE_BITS: u16 = 16;

/// Countdown waves always play one-shot.
pub const COUNTDOWN_PLAY_FLAGS: u32 = PLAY_ONE_SHOT;

/// The six `countdwn\` waves, as observed in `.rdata` (the spoken count-in
/// `5-4-3-2-1-GO`). The exact slot offset each receives within the bank (the
/// index `FUN_0046af10` plays) was not confidently recovered, so no
/// index->value mapping is asserted here.
pub const COUNTDOWN_WAVES: [&str; 6] = ["go!.wav", "1.wav", "2.wav", "3.wav", "4.wav", "5.wav"];

/// The `intro2\` countdown set additionally preloaded in rally-school/tutorial
/// mode (`DAT_005f1e20 >= 2`).
pub const INTRO2_WAVES: [&str; 4] = [
    r"intro2\three.wav",
    r"intro2\two.wav",
    r"intro2\one.wav",
    r"intro2\go.wav",
];

/// Build the countdown wave path relative to the source root, reproducing the
/// loader's concatenation `<language dir>` + `countdwn\` + `<filename>`
/// (e.g. `sounds\speech\english\countdwn\go!.wav`). The absolute path the game
/// forms additionally prepends the installation root.
pub fn countdown_relative_path(language: SpeechLanguage, filename: &str) -> String {
    format!("{}{COUNTDOWN_DIR}{filename}", language.dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn music_tracks_resolve_to_catalogued_paths() {
        assert_eq!(
            MusicTrack::Frontend.path(),
            crate::audio_assets::FRONTEND_MUSIC
        );
        assert_eq!(MusicTrack::Replay.path(), crate::audio_assets::REPLAY_MUSIC);
        assert!(MusicTrack::Frontend.path().ends_with(r"frontend.wav"));
        assert!(MusicTrack::Replay.path().ends_with(r"replay.wav"));
        // Music loops; countdown is one-shot.
        assert_eq!(MUSIC_PLAY_FLAGS, PLAY_LOOPING);
        assert_eq!(COUNTDOWN_PLAY_FLAGS, PLAY_ONE_SHOT);
    }

    #[test]
    fn countdown_set_matches_recovery() {
        assert_eq!(COUNTDOWN_WAVES.len(), 6);
        assert_eq!(COUNTDOWN_WAVES[0], "go!.wav");
        assert_eq!(
            &COUNTDOWN_WAVES[1..],
            &["1.wav", "2.wav", "3.wav", "4.wav", "5.wav"]
        );
        assert_eq!(COUNTDOWN_WAVE_RATE_HZ, 11_025);
        assert_eq!(COUNTDOWN_WAVE_BITS, 16);
        assert_eq!(INTRO2_WAVES.len(), 4);
        assert_eq!(INTRO2_WAVES[3], r"intro2\go.wav");
    }

    #[test]
    fn countdown_path_matches_the_countdwn_shape() {
        assert_eq!(
            countdown_relative_path(SpeechLanguage::English, "go!.wav"),
            r"sounds\speech\english\countdwn\go!.wav"
        );
        assert_eq!(
            countdown_relative_path(SpeechLanguage::Italian, "3.wav"),
            r"sounds\speech\italian\countdwn\3.wav"
        );
    }
}
