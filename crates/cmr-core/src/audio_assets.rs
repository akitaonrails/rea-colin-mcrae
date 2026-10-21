//! Recovered sound asset namespace.
//!
//! The official-EU `Rally.exe` string table defines the game's complete sound
//! directory taxonomy under the `sounds\` root, plus a few fully-qualified wave
//! and music paths. This module records that recovered namespace as metadata
//! (relative paths, original backslash form - never payload bytes), giving the
//! reconstruction one source of truth for the audio asset layout as each loader
//! is recovered. See evidence `rea.official-eu.audio-sound-taxonomy`.
//!
//! Observations: the explicit wave/music files and the category directory
//! prefixes recovered from the string table (addresses in the evidence row).
//! Inferences: the directory prefixes are joined with a dynamically chosen name
//! (per language, per surface, per event) the way the ambience loader joins a
//! base path with a stored name; the `menu\` names are the frontend UI event
//! sounds. Unknowns: the filename/extension convention appended to each prefix,
//! and which loader populates each group (only the weather/crowd ambience loader
//! `FUN_0041cd30` is recovered so far).

/// The base sound directory prefix (`sounds\`).
pub const SOUND_ROOT: &str = r"sounds\";

/// Fully-qualified frontend menu music, relative to the install root.
pub const FRONTEND_MUSIC: &str = r"\new_fe\sounds\music\frontend.wav";

/// Fully-qualified replay music, relative to the install root.
pub const REPLAY_MUSIC: &str = r"\new_fe\sounds\replay\replay.wav";

/// Weather waves loaded outside the `FUN_0041cd30` ambience group.
pub const WEATHER_WET_OUT: &str = r"sounds\weather\wet_out.wav";
pub const WEATHER_RAIN_IN: &str = r"sounds\weather\rain_in.wav";

/// Category directory prefixes under [`SOUND_ROOT`]. Each is concatenated with a
/// dynamically chosen name at load time.
pub mod dirs {
    pub const CAR: &str = r"sounds\car\";
    pub const MENU: &str = r"sounds\menu\";
    pub const ACCIDENT: &str = r"sounds\accident\";
    pub const PRANGS: &str = r"sounds\prangs\";
    pub const NEWSURFS: &str = r"sounds\newsurfs\";
    pub const CROWD: &str = r"sounds\crowd\";
    pub const WEATHER: &str = r"sounds\weather\";
}

/// Recovered sample rate (`0x5622` = 22050) and bit depth (`0x10` = 16, mono) of
/// the menu UI waves loaded by `FUN_00468f50`.
pub const MENU_WAVE_RATE_HZ: u32 = 22_050;
pub const MENU_WAVE_BITS: u16 = 16;

/// The four menu UI waves the loader `FUN_00468f50` registers, in load order
/// (which fixes their slot offsets). Each is `sounds\menu\<name>`, 22050 Hz /
/// 16-bit mono, played on a dedicated menu channel.
///
/// Inferred association with the reconstructed frontend actions: `UPDOWN`
/// vertical navigation, `LEFTRIGH` horizontal navigation/toggle, `FORWARD`
/// confirm, `BACKWARD` cancel. The exact action-to-slot mapping needs the play
/// sites and is not yet recovered.
pub const MENU_WAVE_FILES: [&str; 4] = [
    r"sounds\menu\FORWARD.WAV",
    r"sounds\menu\BACKWARD.WAV",
    r"sounds\menu\LEFTRIGH.WAV",
    r"sounds\menu\UPDOWN.WAV",
];

/// A separate set of `sounds\menu\` event-name strings present in the binary
/// (`BACK`, `SELECT`, `TOGGLE`, `UPDOWN`, `WHEEL`). These are observed as string
/// literals only; they are not the waves the recovered loader `FUN_00468f50`
/// registers (those are [`MENU_WAVE_FILES`]), and the loader/role that consumes
/// them is not yet established.
pub const MENU_EVENT_SOUND_NAMES: [&str; 5] = [
    r"sounds\menu\BACK",
    r"sounds\menu\SELECT",
    r"sounds\menu\TOGGLE",
    r"sounds\menu\UPDOWN",
    r"sounds\menu\WHEEL",
];

/// The recovered per-language speech (co-driver pacenote) directory prefixes
/// under `sounds\speech\`, plus the `helium` novelty voice.
pub const SPEECH_LANGUAGE_DIRS: [&str; 6] = [
    r"sounds\speech\english\",
    r"sounds\speech\french\",
    r"sounds\speech\german\",
    r"sounds\speech\spanish\",
    r"sounds\speech\italian\",
    r"sounds\speech\helium\",
];

/// The recovered rally-school (tutorial) speech directory prefixes per language.
pub const SCHOOL_SPEECH_DIRS: [&str; 5] = [
    r"rallschl\sounds\english\",
    r"rallschl\sounds\french\",
    r"rallschl\sounds\german\",
    r"rallschl\sounds\spanish\",
    r"rallschl\sounds\italian\",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loaded_menu_waves_are_four_mono_files_under_the_menu_dir() {
        assert_eq!(MENU_WAVE_FILES.len(), 4);
        assert_eq!(MENU_WAVE_FILES[0], r"sounds\menu\FORWARD.WAV");
        assert_eq!(MENU_WAVE_FILES[3], r"sounds\menu\UPDOWN.WAV");
        for file in MENU_WAVE_FILES {
            assert!(file.starts_with(dirs::MENU));
            assert!(file.ends_with(".WAV"));
        }
        assert_eq!(MENU_WAVE_RATE_HZ, 22_050);
        assert_eq!(MENU_WAVE_BITS, 16);
    }

    #[test]
    fn menu_event_names_are_a_distinct_five_string_set() {
        assert_eq!(MENU_EVENT_SOUND_NAMES.len(), 5);
        for name in MENU_EVENT_SOUND_NAMES {
            assert!(name.starts_with(dirs::MENU));
        }
        // They are not the loaded waves.
        for name in MENU_EVENT_SOUND_NAMES {
            assert!(!MENU_WAVE_FILES.contains(&name));
        }
    }

    #[test]
    fn speech_dirs_cover_the_recovered_languages() {
        assert_eq!(SPEECH_LANGUAGE_DIRS.len(), 6);
        assert_eq!(SCHOOL_SPEECH_DIRS.len(), 5);
        assert!(SPEECH_LANGUAGE_DIRS.contains(&r"sounds\speech\english\"));
        assert!(SPEECH_LANGUAGE_DIRS.contains(&r"sounds\speech\helium\"));
    }

    #[test]
    fn explicit_music_and_weather_paths_are_recorded() {
        assert!(FRONTEND_MUSIC.ends_with(r"frontend.wav"));
        assert!(REPLAY_MUSIC.ends_with(r"replay.wav"));
        assert_eq!(WEATHER_WET_OUT, r"sounds\weather\wet_out.wav");
        assert_eq!(WEATHER_RAIN_IN, r"sounds\weather\rain_in.wav");
    }

    #[test]
    fn category_dirs_live_under_the_sound_root() {
        for dir in [
            dirs::CAR,
            dirs::MENU,
            dirs::ACCIDENT,
            dirs::PRANGS,
            dirs::NEWSURFS,
            dirs::CROWD,
            dirs::WEATHER,
        ] {
            assert!(dir.starts_with(SOUND_ROOT));
        }
    }
}
