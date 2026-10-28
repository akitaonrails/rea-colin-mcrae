//! The rally school's in-race text: the task's status line (`FUN_0046e460`,
//! queued first by the B handlers of states 5/6 `FUN_004231f0` and 9
//! `FUN_00423380`) and the loop counter (`FUN_0046e4f0`, queued by the
//! shared composite `FUN_00422ca0` in states 5, 7, 9 and 0xa while the task
//! is not settling at its finish, `0x422e2a`).
//!
//! Both go through the Q16 text wrapper `FUN_00497b40` in `title.fon`
//! (`DAT_00550800`). A description of disassembly, not original source.
//! Evidence: `rea.official-eu.school-text`, `.work/school-race-notes.md`.

use cmr_formats::font::{BitmapFont, TextLayoutError};

use crate::pause_menu_draw::{PauseFont, PauseTextRun, TITLE_COLOR, text_q16};
use crate::school_race::SchoolRace;

/// The white colour record `0x502498`.
pub const WHITE: [u8; 4] = [0xff, 0xff, 0xff, 0xff];

/// The status line's text ids: "TAKEN TOO LONG", "HIT TOO MANY CONES",
/// "STOP AT THE LINE".
const TOO_LONG: i32 = 0x113;
const TOO_MANY_CONES: i32 = 0x114;
const STOP_AT_THE_LINE: i32 = 0x124;
/// "LOOPS:".
const LOOPS: i32 = 0x115;

/// The status line's task-FSM text (`0x46e460`): 2 over time, 3 too many
/// cones, 4 the laps done unless already settling.
#[must_use]
pub fn status_text_id(race: &SchoolRace) -> Option<i32> {
    match race.fsm {
        2 => Some(TOO_LONG),
        3 => Some(TOO_MANY_CONES),
        4 if !race.finishing => Some(STOP_AT_THE_LINE),
        _ => None,
    }
}

/// The text one frame of `state` queues for the school task, in queue
/// order. `text` is the string lookup `FUN_0049b010`.
pub fn school_text_runs(
    race: &SchoolRace,
    state: u8,
    text: &dyn Fn(i32) -> Vec<u8>,
    font: &BitmapFont,
    width: u16,
    height: u16,
) -> Result<Vec<PauseTextRun>, TextLayoutError> {
    let run = |plan, color| PauseTextRun {
        font: PauseFont::Title,
        color,
        plan,
        clip: None,
    };
    let mut runs = Vec::new();
    if matches!(state, 5 | 6 | 9)
        && let Some(id) = status_text_id(race)
    {
        let (plan, _, _) = text_q16(font, &text(id), 0x8000, 0x3333, 0x22, width, height)?;
        runs.push(run(plan, TITLE_COLOR));
    }
    // `FUN_00471800` is the lap number `DAT_005fbe28 + 1`.
    let lap = race.laps() + 1;
    if matches!(state, 5 | 7 | 9 | 0xa)
        && !race.finishing
        && race.loop_task
        && lap <= 4
        && race.armed()
    {
        let (plan, _, label_width) =
            text_q16(font, &text(LOOPS), 0xccc, 0x12b0, 0x21, width, height)?;
        runs.push(run(plan, TITLE_COLOR));
        let count = format!("{}", 5 - lap);
        // `DAT_00a2006c / 2`, rounded toward zero.
        let x = 0xccc + label_width / 2;
        let (plan, _, _) = text_q16(font, count.as_bytes(), x, 0x1ced, 0x22, width, height)?;
        runs.push(run(plan, WHITE));
    }
    Ok(runs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::school_race::{SchoolRace, SchoolScoring};
    use cmr_formats::school_rsf::{RSF_BLOCK_SIZE, parse_rsf};

    fn race(level: u8, task: u8) -> SchoolRace {
        let reference = parse_rsf(&vec![0; RSF_BLOCK_SIZE]).unwrap();
        let scoring = SchoolScoring::from_blocks(&[0; 288], &[0; 288]).unwrap();
        SchoolRace::start(level, task, 10, reference, scoring)
    }

    #[test]
    fn the_status_line_follows_the_task_fsm() {
        let mut r = race(1, 2);
        assert_eq!(status_text_id(&r), None);
        r.fsm = 2;
        assert_eq!(status_text_id(&r), Some(0x113));
        r.fsm = 3;
        assert_eq!(status_text_id(&r), Some(0x114));
        r.fsm = 4;
        assert_eq!(status_text_id(&r), Some(0x124));
        r.finishing = true;
        assert_eq!(status_text_id(&r), None);
    }

    #[test]
    fn only_a_loop_task_counts_its_loops() {
        // Level 1 task 0 is a loop task; level 1 task 2 is not.
        assert!(race(1, 0).loop_task);
        assert!(!race(1, 2).loop_task);
    }

    #[test]
    fn a_loop_task_queues_its_status_then_the_counter() {
        let font = crate::pause_menu_draw::tests::font();
        let text = |id: i32| {
            if id == 0x115 {
                b"LOOPS".to_vec()
            } else {
                b"STOP".to_vec()
            }
        };
        let mut r = race(1, 0);
        r.fsm = 4;
        let runs = school_text_runs(&r, 5, &text, &font, 640, 480).unwrap();
        assert_eq!(runs.len(), 3);
        assert_eq!(runs[0].color, TITLE_COLOR);
        // Centred on 0x3333 of 480 lines (the glyphs are 4 high).
        assert_eq!(
            runs[0].plan.blits[0].destination.y,
            ((480 * 0x3333) >> 16) - 2
        );
        assert_eq!(runs[2].color, WHITE);
        // State 7 has no status line; a settling task shows neither.
        assert_eq!(
            school_text_runs(&r, 7, &text, &font, 640, 480)
                .unwrap()
                .len(),
            2
        );
        r.finishing = true;
        assert!(
            school_text_runs(&r, 5, &text, &font, 640, 480)
                .unwrap()
                .is_empty()
        );
    }
}
