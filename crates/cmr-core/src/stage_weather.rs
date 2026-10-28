//! The stage's weather, chosen when state 4 arms the race (`FUN_00449ba0` ->
//! `FUN_0046b030`): the weather type `DAT_00592ef8` (`FUN_0046b220`; 3 is
//! rain), the weather detail `DAT_00592efc` (`FUN_0046b230`; 3 adds the storm)
//! and the lighting `DAT_00592f00` (`FUN_0046b240`; 8 is night), plus the six
//! light colours `FUN_0046af60` sets from the lighting and hands to the
//! engine (`FUN_004a8630`).
//!
//! In the rally school each value comes from a 3x5 table by level and task;
//! the special event is fixed (type 2, detail 1, lighting 4); otherwise
//! `cfg[0x25]` bit 7 forces type 1 and night, bit 6 type 5, and the rest comes
//! from 9x7 tables by country and stage. The tables are static data of the
//! user's executable, extracted through a reviewed store; this module only
//! indexes them. A description of disassembly, not original source.
//! Evidence: `rea.official-eu.stage-weather`.

use crate::event_flow::EventState;

/// The six weather tables (`0x4f4538`, `0x4f4574`, `0x4f4670`, `0x4f46ac`,
/// `0x4f47a8`, `0x4f47e4`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StageWeatherTables {
    pub school_type: [u32; 15],
    pub stage_type: [u32; 63],
    pub school_detail: [u32; 15],
    pub stage_detail: [u32; 63],
    pub school_lighting: [u32; 15],
    pub stage_lighting: [u32; 63],
}

/// The brightness `FUN_0046b030` passes to `FUN_0046af60` by lighting.
pub const LIGHTING_BRIGHTNESS: [u8; 9] = [0xc8, 0x96, 0xb4, 0xd2, 0xf0, 0xd2, 0xb4, 0x8c, 0x46];

/// The night lighting (`FUN_0046b240() == 8`), which also loads the night
/// car models.
pub const NIGHT_LIGHTING: u32 = 8;

/// The weather of one stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StageWeather {
    /// `DAT_00592ef8`.
    pub weather_type: u32,
    /// `DAT_00592efc`.
    pub detail: u32,
    /// `DAT_00592f00`.
    pub lighting: u32,
}

/// The six RGBA light colours (`0x592ee0`, `0x592ee4`, `0x592ee8`,
/// `0x592eec`, `0x592ef0`, `0x592ef4`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StageLights {
    pub colors: [[u8; 4]; 6],
}

fn school_index(school_6c0: u8) -> usize {
    usize::from((school_6c0 >> 3) & 3) * 5 + usize::from(school_6c0 >> 5)
}

fn stage_index(country: u8, stage: u8) -> usize {
    usize::from(country) * 7 + usize::from(stage)
}

fn read(table: &[u32], index: usize) -> u32 {
    // The native indexes without a bound; past the table it reads the next
    // one's words, which this lookup does not reproduce.
    table.get(index).copied().unwrap_or(0)
}

/// `FUN_0046b030`'s three choices for the event's stage.
pub fn select_weather(tables: &StageWeatherTables, event: &EventState) -> StageWeather {
    let school_6c0 = event.cfg.school_6c0;
    let cfg_25 = event.cfg.options_025;
    if event.globals.school_d3ca88 {
        let index = school_index(school_6c0);
        return StageWeather {
            weather_type: read(&tables.school_type, index),
            detail: read(&tables.school_detail, index),
            lighting: read(&tables.school_lighting, index),
        };
    }
    if event.globals.special_514478 {
        return StageWeather {
            weather_type: 2,
            detail: 1,
            lighting: 4,
        };
    }
    let index = stage_index(event.cfg.cursor_168 & 0xf, event.cfg.cursor_168 >> 4);
    let weather_type = if cfg_25 & 0x80 != 0 {
        1
    } else if cfg_25 & 0x40 != 0 {
        5
    } else {
        read(&tables.stage_type, index)
    };
    let lighting = if cfg_25 & 0x80 != 0 {
        NIGHT_LIGHTING
    } else {
        read(&tables.stage_lighting, index)
    };
    StageWeather {
        weather_type,
        detail: read(&tables.stage_detail, index),
        lighting,
    }
}

/// `FUN_0046af60(LIGHTING_BRIGHTNESS[lighting])`: every colour takes the
/// brightness, then the third and fourth become 0 by day and they and the
/// sixth `0x32` at night; every alpha is `0xff`. A lighting past the table yields `None` (the native reads past its
/// local array).
pub fn stage_lights(weather: StageWeather) -> Option<StageLights> {
    let level = *LIGHTING_BRIGHTNESS.get(weather.lighting as usize)?;
    let night = if weather.lighting == NIGHT_LIGHTING {
        0x32
    } else {
        0
    };
    let lit = [level, level, level, 0xff];
    let dim = [night, night, night, 0xff];
    let last = if weather.lighting == NIGHT_LIGHTING {
        dim
    } else {
        lit
    };
    Some(StageLights {
        colors: [lit, lit, dim, dim, lit, last],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tables() -> StageWeatherTables {
        let ramp15 = std::array::from_fn(|i| 100 + i as u32);
        let ramp63 = std::array::from_fn(|i| 200 + i as u32);
        StageWeatherTables {
            school_type: ramp15,
            stage_type: ramp63,
            school_detail: ramp15.map(|v| v + 1000),
            stage_detail: ramp63.map(|v| v + 1000),
            school_lighting: ramp15.map(|v| v + 2000),
            stage_lighting: ramp63.map(|v| v + 2000),
        }
    }

    #[test]
    fn a_stage_reads_its_country_and_stage_row() {
        let mut event = EventState::default();
        event.cfg.cursor_168 = 0x23; // country 3, stage 2
        let weather = select_weather(&tables(), &event);
        assert_eq!(
            weather,
            StageWeather {
                weather_type: 200 + 23,
                detail: 1200 + 23,
                lighting: 2200 + 23,
            }
        );
        event.cfg.options_025 = 0x80;
        let forced = select_weather(&tables(), &event);
        assert_eq!((forced.weather_type, forced.lighting), (1, 8));
        event.cfg.options_025 = 0x40;
        assert_eq!(select_weather(&tables(), &event).weather_type, 5);
    }

    #[test]
    fn the_school_and_the_special_event() {
        let mut school = EventState::default();
        school.globals.school_d3ca88 = true;
        school.cfg.school_6c0 = (2 << 3) | (4 << 5);
        school.cfg.options_025 = 0x80;
        let weather = select_weather(&tables(), &school);
        assert_eq!(weather.weather_type, 100 + 14);
        assert_eq!(weather.lighting, 2100 + 14);
        let mut special = EventState::default();
        special.globals.special_514478 = true;
        assert_eq!(
            select_weather(&tables(), &special),
            StageWeather {
                weather_type: 2,
                detail: 1,
                lighting: 4
            }
        );
    }

    #[test]
    fn the_lights_follow_the_lighting() {
        let day = stage_lights(StageWeather {
            weather_type: 0,
            detail: 0,
            lighting: 4,
        })
        .unwrap();
        assert_eq!(day.colors[0], [0xf0, 0xf0, 0xf0, 0xff]);
        assert_eq!(day.colors[2], [0, 0, 0, 0xff]);
        let night = stage_lights(StageWeather {
            weather_type: 0,
            detail: 0,
            lighting: 8,
        })
        .unwrap();
        assert_eq!(night.colors[4], [0x46, 0x46, 0x46, 0xff]);
        assert_eq!(night.colors[3], [0x32, 0x32, 0x32, 0xff]);
        assert_eq!(night.colors[5], [0x32, 0x32, 0x32, 0xff]);
        assert_eq!(day.colors[5], [0xf0, 0xf0, 0xf0, 0xff]);
        assert_eq!(
            stage_lights(StageWeather {
                weather_type: 0,
                detail: 0,
                lighting: 9
            }),
            None
        );
    }
}
