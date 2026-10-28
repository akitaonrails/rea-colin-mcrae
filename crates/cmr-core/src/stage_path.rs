//! The stage path builder `FUN_0044d4c0`: the stage-relative base the stage
//! load (`FUN_00421da0`, state 4's render handler) appends each section
//! extension to, under `<data root>\game\`.
//!
//! - The special event (`DAT_00514478`): `<CC>\8\<CC>8` for the country.
//! - The rally school: `RALLSCHL\TRACKS\<EASY|MEDIUM|HARD>\TASK<t>\TASK<t>`
//!   from `cfg[0x6c0]` (level bits 3-4, task `(b >> 5) + 1`).
//! - Secret stages (`cfg[0x166] & 0x40`): one of four fixed bases by stage.
//! - Otherwise `<CC>\<n>\<CC><n>` with `n` the stage nibble plus 1.
//!
//! The country codes are the native order of the directory formats at
//! `0x4f0db8`/`0x4f0dd8`. The debug override (a command-line stage token) is
//! not modelled. A description of disassembly, not original source.
//! Evidence: `rea.official-eu.stage-path`.

use crate::event_flow::EventState;

/// The country directories in the native order.
pub const COUNTRY_CODES: [&str; 8] = ["NZ", "GR", "MC", "AU", "SW", "CO", "IN", "UK"];

/// The secret stage bases (`0x4f0f80`, `0x4f0f64`, `0x4f0f4c`, `0x4f0f38`).
pub const SECRET_STAGES: [&str; 4] = [
    "SECRET\\DANGER\\DANGER",
    "SECRET\\MILEHIGH\\MILEHIGH",
    "SECRET\\RETSIL\\RETSIL",
    "SECRET\\TROUT\\TROUT",
];

/// The rally-school levels (`0x4f0dac`).
pub const SCHOOL_LEVELS: [&str; 3] = ["EASY", "MEDIUM", "HARD"];

fn country_stage(country: u8, number: u8) -> Option<String> {
    let code = COUNTRY_CODES.get(usize::from(country))?;
    Some(format!("{code}\\{number}\\{code}{number}"))
}

/// `FUN_0044d4c0`: the stage base for the event, or `None` where the native
/// leaves the buffer untouched (a secret stage above 3) or would read past
/// its tables (a country above 7, a school level of 3). `school_6c0` is
/// `cfg[0x6c0]`.
pub fn stage_base(event: &EventState, school_6c0: u8) -> Option<String> {
    let country = event.cfg.cursor_168 & 0xf;
    if event.globals.special_514478 {
        return country_stage(country, 8);
    }
    if event.globals.school_d3ca88 {
        let level = SCHOOL_LEVELS.get(usize::from((school_6c0 >> 3) & 3))?;
        let task = (school_6c0 >> 5) + 1;
        return Some(format!("RALLSCHL\\TRACKS\\{level}\\TASK{task}\\TASK{task}"));
    }
    let stage = event.cfg.cursor_168 >> 4;
    if event.cfg.mode_166 & 0x40 != 0 {
        return SECRET_STAGES
            .get(usize::from(stage))
            .map(|base| (*base).to_owned());
    }
    country_stage(country, stage + 1)
}

/// The country index and stage number of a `<CC><n>` stage name, as the
/// `--stage` argument names it.
pub fn parse_stage_name(name: &str) -> Option<(u8, u8)> {
    let (code, number) = name.split_at(name.len().min(2));
    let country = COUNTRY_CODES
        .iter()
        .position(|candidate| candidate.eq_ignore_ascii_case(code))?;
    let number = number.parse::<u8>().ok().filter(|n| *n >= 1)?;
    Some((country as u8, number))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(mode: u8, cursor: u8) -> EventState {
        let mut event = EventState::default();
        event.cfg.mode_166 = mode;
        event.cfg.cursor_168 = cursor;
        event
    }

    #[test]
    fn a_country_stage_numbers_from_one() {
        assert_eq!(
            stage_base(&event(0, 0x03), 0).as_deref(),
            Some("AU\\1\\AU1")
        );
        assert_eq!(
            stage_base(&event(2, 0x57), 0).as_deref(),
            Some("UK\\6\\UK6")
        );
        assert_eq!(stage_base(&event(0, 0x08), 0), None);
    }

    #[test]
    fn the_special_event_school_and_secret_bases() {
        let mut special = event(0, 0x25);
        special.globals.special_514478 = true;
        assert_eq!(stage_base(&special, 0).as_deref(), Some("CO\\8\\CO8"));
        let mut school = event(0, 0);
        school.globals.school_d3ca88 = true;
        assert_eq!(
            stage_base(&school, (1 << 3) | (2 << 5)).as_deref(),
            Some("RALLSCHL\\TRACKS\\MEDIUM\\TASK3\\TASK3")
        );
        assert_eq!(stage_base(&school, 3 << 3), None);
        assert_eq!(
            stage_base(&event(0x40, 0x18), 0).as_deref(),
            Some("SECRET\\MILEHIGH\\MILEHIGH")
        );
        assert_eq!(stage_base(&event(0x40, 0x48), 0), None);
    }

    #[test]
    fn stage_names_parse_case_insensitively() {
        assert_eq!(parse_stage_name("AU1"), Some((3, 1)));
        assert_eq!(parse_stage_name("uk8"), Some((7, 8)));
        assert_eq!(parse_stage_name("AU0"), None);
        assert_eq!(parse_stage_name("XX1"), None);
    }
}
