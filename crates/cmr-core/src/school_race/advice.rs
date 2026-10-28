//! The rally school instructor's speech (`0x473540..0x476030`): the advice
//! bank's layout, the per-line advice `FUN_00474830`, the per-frame
//! producers `FUN_00474c50` (impacts, crash, position, near end, wrong way,
//! failing, reversing, idle) and the speech helpers.
//!
//! Every speech is one wav of the advice bank, played on the advice channel
//! (`FUN_00475e90`); the host maps [`SchoolCue::Advice`] to its path with
//! [`advice_path`]. Most words here are zero-initialized globals no task
//! resets (only `FUN_00473540`'s tail rearms a few), so the [`Instructor`]
//! lives in [`super::SchoolStatics`] and outlives tasks as they do.
//!
//! Not modelled, because nothing observable reads them: the follow-up queue
//! (`FUN_00475ff0`/`FUN_00475f30`) and its counter `DAT_005fc068`, read only
//! by the dead `FUN_00474df0`; the speech count `DAT_005fc064`; the position
//! flag `DAT_005fbed4` and the lap memo `DAT_004f58e4`, read only by those.
//! A description of disassembly, not original source. Evidence:
//! `rea.official-eu.school-advice`.

use super::{SchoolCar, SchoolCue, SchoolRace, qmul};
use crate::game_rng::GameLcg;

/// The first bank index of each speech category (`0x4f57d8`): speed,
/// direction, sliding, position, brake, handbrake, damage 1/2/3, after
/// crash, sniper, failed, about to fail, objects, near end, close to cone,
/// wide of cone.
const GROUP_BASE: [i32; 17] = [
    0, 21, 31, 40, 42, 45, 48, 53, 56, 58, 61, 66, 73, 77, 83, 87, 88,
];
/// The per-line categories' first sub-list (`0x4f57ec`) and pool size
/// (`0x4f5804`).
const SUB_FIRST: [i32; 6] = [0, 3, 5, 8, 10, 13];
const POOL: [i32; 6] = [7, 5, 3, 1, 1, 1];
/// The sixteen judged sub-lists' wav counts (`0x4f581c`), in bank order.
const SUB_COUNT: [i32; 16] = [7, 7, 7, 5, 5, 3, 3, 3, 1, 1, 1, 1, 1, 1, 1, 1];
/// The judged sub-lists' directories (`0x4f58a0`).
const SUB_DIRS: [&str; 16] = [
    "speed\\toofast",
    "speed\\correct",
    "speed\\tooslow",
    "dirctn\\wrong",
    "dirctn\\correct",
    "sliding\\toomuch",
    "sliding\\correct",
    "sliding\\toolttle",
    "position\\wrong",
    "position\\correct",
    "brake\\toomuch",
    "brake\\correct",
    "brake\\toolttle",
    "hndbrake\\toomuch",
    "hndbrake\\correct",
    "hndbrake\\toolttle",
];

/// The bank file for an advice index, relative to the language's sounds
/// directory (`FUN_00473540`'s load order).
#[must_use]
pub fn advice_path(index: u8) -> Option<String> {
    let mut i = i32::from(index);
    for (dir, count) in SUB_DIRS.iter().zip(SUB_COUNT) {
        if i < count {
            return Some(format!("advice\\{dir}\\{i}.wav"));
        }
        i -= count;
    }
    // From index 48: `damage\damage%d.wav` then the `\%d.wav` folders.
    if i < 10 {
        return Some(format!("advice\\damage\\damage{i}.wav"));
    }
    i -= 10;
    let tail = [
        ("apcrash", 3),
        ("sniper", 5),
        ("failing", 11),
        ("objects", 6),
        ("nearend", 4),
        ("cones\\tooclose", 1),
        ("cones\\toowide", 3),
    ];
    for (dir, count) in tail {
        if i < count {
            return Some(format!("advice\\{dir}\\{i}.wav"));
        }
        i -= count;
    }
    None
}

fn rand_mod(rng: &mut GameLcg, n: i32) -> i32 {
    if n == 0 {
        return 0;
    }
    (rng.next_raw() as i32) % n
}

/// The instructor's words (`0x5fbe6c..0x5fc0c4`), zero at process start.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Instructor {
    /// The host's channels are playing (co-driver, intro, advice).
    host_speaking: bool,
    /// A speech started this frame (its channel is busy at once natively).
    said_this_frame: bool,
    /// `DAT_005fc07c`, `DAT_005fc080`: the last speech and the last major
    /// one (clock centiseconds).
    said_at: i32,
    major_at: i32,
    /// `DAT_005fc084`: the damage total when last speaking.
    damage_snapshot: i32,
    /// `DAT_005fc034`: the last line's advice was a correction.
    pub(super) corrected: bool,
    /// `DAT_005fc060`, `DAT_005fc040`, `DAT_005fbe88`, `DAT_005fbed0`: an
    /// impact speech pending its delay.
    impact_pending: bool,
    impact_cat: u8,
    impact_wav: i32,
    impact_at: i32,
    /// `DAT_005fc0ac`, `DAT_005fc0b0`: a crash seen, its after-crash speech
    /// due when the car recovers.
    crash_seen: bool,
    crash_pending: bool,
    /// `DAT_005fc0b4`: the near-end speech was said.
    near_end_said: bool,
    /// `DAT_005fc0b8`: the wrong-way pool was rearmed.
    wrong_way_rearmed: bool,
    /// `DAT_005fbeec..`, `DAT_005fc02c..`, `DAT_005fbec8`, `DAT_005fbee4`.
    wrong_used: [bool; 5],
    wrong_pool: [i8; 5],
    wrong_left: i8,
    failing_armed: bool,
    /// `DAT_005fbe6c..`, `DAT_005fbe7c..`, `DAT_005fc044`, `DAT_005fbef4`.
    fail_used: [bool; 11],
    fail_pool: [i8; 11],
    fail_left: i8,
    pub(super) fail_phase2: bool,
    /// `DAT_005fc0bc`, `DAT_005fc020`: the reversing nag's start.
    reverse_armed: bool,
    reverse_at: i32,
    /// `DAT_005fc0c4`, `DAT_005fc0c0`: the idle nag's start.
    idle_armed: bool,
    idle_at: i32,
}

impl Instructor {
    /// `FUN_00473540`'s tail at the stage load, run unless level 2 task
    /// 2 or later: the speech times and pending impact cleared, the two
    /// pools rearmed.
    pub fn start_task(&mut self, level: u8, task: u8) {
        if level == 2 && task > 1 {
            return;
        }
        self.major_at = 0;
        self.said_at = 0;
        self.impact_pending = false;
        self.rearm_wrong_way();
        self.rearm_failing();
    }

    /// The host's speech channels at the frame's start.
    pub fn begin_frame(&mut self, speaking: bool) {
        self.host_speaking = speaking;
        self.said_this_frame = false;
    }

    /// `FUN_00475cc0`: a speech channel is playing (then `FUN_00474da0`
    /// snapshots the damage).
    fn speaking(&mut self, damage: i32) -> bool {
        let busy = self.host_speaking || self.said_this_frame;
        if busy {
            self.damage_snapshot = damage;
        }
        busy
    }

    /// `FUN_00475d40`: within 4..8 s of the last speech.
    fn cooling(&self, now: i32, rng: &mut GameLcg) -> bool {
        let window = (rand_mod(rng, 5) + 4) * 100;
        now.wrapping_sub(self.said_at).wrapping_abs() < window
    }

    /// `FUN_00475d90`: within 1..2 s of the last major speech.
    fn cooling_major(&self, now: i32, rng: &mut GameLcg) -> bool {
        let window = (rand_mod(rng, 2) + 1) * 100;
        now.wrapping_sub(self.major_at).wrapping_abs() < window
    }

    /// `FUN_00475e90(cat, judge, index)`.
    fn say(
        &mut self,
        cat: u8,
        judge: i16,
        index: i32,
        now: i32,
        damage: i32,
        out: &mut Vec<SchoolCue>,
    ) {
        out.push(SchoolCue::Advice(index as u8));
        self.said_this_frame = true;
        self.said_at = now;
        if (6..=0xd).contains(&cat) || (cat == 3 && judge == 0) {
            self.major_at = now;
        }
        self.damage_snapshot = damage;
    }

    /// `FUN_00475410(1)`.
    fn rearm_wrong_way(&mut self) {
        self.wrong_used = [false; 5];
        self.wrong_pool = [0, 1, 2, 3, 4];
        self.wrong_left = 5;
    }

    /// `FUN_00475410(0)`: "wrong direction" from a shuffled pool of five;
    /// with fewer than four left, the failing chain.
    fn wrong_way(
        &mut self,
        rng: &mut GameLcg,
        now: i32,
        damage: i32,
        out: &mut Vec<SchoolCue>,
    ) -> WrongWay {
        if self.wrong_left <= 3 {
            self.failing_armed = false;
            return WrongWay::Failing;
        }
        if !self.failing_armed {
            self.rearm_failing();
            self.failing_armed = true;
        }
        let k = self.wrong_pool[rand_mod(rng, i32::from(self.wrong_left)) as usize];
        self.say(1, 0, GROUP_BASE[1] + i32::from(k), now, damage, out);
        self.wrong_used[k as usize] = true;
        if self.wrong_left != 0 {
            self.wrong_left -= 1;
        }
        let mut j = 0;
        for e in 0..5 {
            if !self.wrong_used[e] {
                self.wrong_pool[j] = e as i8;
                j += 1;
            }
        }
        WrongWay::Said(true)
    }

    /// `FUN_004754f0(1)`.
    fn rearm_failing(&mut self) {
        self.fail_used = [false; 11];
        self.fail_pool = std::array::from_fn(|i| i as i8);
        // `DAT_005fc058` is never written, so 3 + 1.
        self.fail_left = 4;
    }
}

/// What `FUN_00475410(0)` did.
enum WrongWay {
    Said(bool),
    /// It handed over to `FUN_004754f0(0)`.
    Failing,
}

impl SchoolRace {
    /// `FUN_004754f0(0)`: "failing" from the first pool, then "about to
    /// fail", then the task fails; a failed task is abandoned (every mark
    /// 0). Returns whether it spoke.
    fn failing(
        &mut self,
        ins: &mut Instructor,
        car: &SchoolCar,
        rng: &mut GameLcg,
        out: &mut Vec<SchoolCue>,
    ) -> bool {
        if self.failed {
            self.abandoned = true;
            self.stop_marker();
            self.end(rng);
            // The failed end clears `DAT_005fbef4`.
            ins.fail_phase2 = false;
            return false;
        }
        let now = self.elapsed_cs();
        let (span, size_after) = if ins.fail_phase2 {
            if car.speed <= 0xfa0 {
                ins.fail_used[5..11].fill(false);
            }
            (4..7, 7)
        } else {
            (0..4, 4)
        };
        let k = ins.fail_pool[rand_mod(rng, i32::from(ins.fail_left)) as usize];
        ins.say(
            0xb,
            -1,
            GROUP_BASE[0xb] + i32::from(k),
            now,
            car.damage,
            out,
        );
        ins.fail_used[k as usize] = true;
        if ins.fail_left != 0 {
            ins.fail_left -= 1;
        }
        let mut j = 0;
        for e in span {
            if !ins.fail_used[e] {
                ins.fail_pool[j] = e as i8;
                j += 1;
            }
        }
        if size_after - i32::from(ins.fail_left) < 2 {
            return true;
        }
        if ins.fail_phase2 {
            self.failed = true;
        } else {
            ins.fail_phase2 = true;
            ins.fail_used[4..11].fill(false);
            ins.fail_left = 7;
            for (c, slot) in ins.fail_pool[4..11].iter_mut().enumerate() {
                *slot = (c + 4) as i8;
            }
        }
        true
    }

    /// `FUN_004756f0`: impacts, the crash, wheels off the line, the near
    /// end and the wrong way. Returns whether it spoke.
    fn speech_events(
        &mut self,
        ins: &mut Instructor,
        car: &SchoolCar,
        rng: &mut GameLcg,
        out: &mut Vec<SchoolCue>,
    ) -> bool {
        let now = self.elapsed_cs();
        let damage = car.damage;
        let mut said = false;
        if ins.impact_pending && !ins.speaking(damage) {
            if now.wrapping_sub(ins.impact_at).wrapping_abs() > 20 {
                ins.say(ins.impact_cat, -1, ins.impact_wav, now, damage, out);
                ins.impact_pending = false;
                return true;
            }
            return false;
        }
        if ins.cooling_major(now, rng) {
            return false;
        }
        if ins.crash_seen {
            if car.crashed {
                ins.crash_pending = true;
            } else {
                ins.crash_seen = false;
                if ins.crash_pending {
                    ins.crash_pending = false;
                    ins.impact_wav = rand_mod(rng, 3) + GROUP_BASE[9];
                    ins.say(9, -1, ins.impact_wav, now, damage, out);
                    said = true;
                }
            }
        }
        // `FUN_00476030`: the damage total rose since the last speech.
        if (damage > ins.damage_snapshot || car.object_contact) && !ins.crash_seen {
            if car.severity < 0x4ccc {
                let surface = if rand_mod(rng, 2) != 0 {
                    car.contact_surface
                } else {
                    0
                };
                (ins.impact_cat, ins.impact_wav) = match surface {
                    5 => (0xd, GROUP_BASE[0xd] + 2),
                    8 => (0xd, GROUP_BASE[0xd] + 1),
                    0xd => (0xd, rand_mod(rng, 3) + GROUP_BASE[0xd] + 3),
                    0x10 => (0xd, GROUP_BASE[0xd]),
                    _ => (6, GROUP_BASE[6] + rand_mod(rng, 5)),
                };
            } else if !car.crashed {
                ins.impact_cat = 7;
                ins.impact_wav = GROUP_BASE[7] + rand_mod(rng, 3);
            } else {
                ins.impact_cat = 8;
                ins.impact_wav = rand_mod(rng, 2) + GROUP_BASE[8];
            }
            ins.crash_seen = true;
            ins.impact_pending = true;
            ins.impact_at = now;
            return false;
        }
        if ins.speaking(damage) {
            return false;
        }
        if !said {
            let cooling = ins.cooling(now, rng);
            if !cooling && !ins.crash_seen && !self.loop_task {
                // All four wheels on surfaces 0x14..=0x1d: off the line.
                let off = car.surfaces.iter().all(|s| (0x14..=0x1d).contains(s));
                if off {
                    let _ = rng.next_raw();
                    ins.say(3, 0, GROUP_BASE[3], now, damage, out);
                    said = true;
                }
            }
            if !said && !ins.near_end_said {
                if self.task == 0 && self.level == 0 {
                    if now as u32 > 200 {
                        let wav = rand_mod(rng, 2) + GROUP_BASE[0xe] + 2;
                        ins.say(0xe, -1, wav, now, damage, out);
                        ins.near_end_said = true;
                    }
                } else if self.loop_task {
                    if 4 - (self.lap + 1) <= 0 {
                        ins.say(0xe, -1, GROUP_BASE[0xe] + 1, now, damage, out);
                        said = true;
                        ins.near_end_said = true;
                    }
                } else if car.cat_remaining < 0x14 {
                    ins.say(0xe, -1, GROUP_BASE[0xe], now, damage, out);
                    said = true;
                    ins.near_end_said = true;
                }
            }
        }
        if self.laps_done || ins.cooling(now, rng) {
            return said;
        }
        // `FUN_00475d00`.
        let wrong_way = if self.loop_task {
            self.wrong_way
        } else {
            car.reverse_count_5
        };
        if wrong_way && !said {
            let spoke = match ins.wrong_way(rng, now, damage, out) {
                WrongWay::Said(spoke) => spoke,
                WrongWay::Failing => self.failing(ins, car, rng, out),
            };
            ins.wrong_way_rearmed = false;
            return spoke;
        }
        if !ins.wrong_way_rearmed {
            ins.rearm_wrong_way();
            ins.wrong_way_rearmed = true;
        }
        said
    }

    /// `FUN_00475bb0`: reversing (wheels on the line, reverse gear) for
    /// more than 5 s.
    fn reversing_nag(
        &mut self,
        ins: &mut Instructor,
        car: &SchoolCar,
        out: &mut Vec<SchoolCue>,
    ) -> bool {
        let now = self.elapsed_cs();
        if !ins.reverse_armed {
            ins.reverse_at = now;
            ins.reverse_armed = true;
        }
        if now.wrapping_sub(ins.reverse_at) <= 500 {
            return false;
        }
        if car.surfaces.iter().any(|&s| s == 0xf || s == 0) || car.gear != 7 {
            return false;
        }
        ins.say(6, -1, GROUP_BASE[6] + 7, now, car.damage, out);
        ins.reverse_at = now;
        true
    }

    /// `FUN_00475c40(t, rearm)`: sitting below 0x4ccc for more than 500.
    pub(super) fn idle_nag(
        &self,
        ins: &mut Instructor,
        t: i32,
        rearm: bool,
        car: &SchoolCar,
        rng: &mut GameLcg,
        out: &mut Vec<SchoolCue>,
    ) -> bool {
        if car.speed >= 0x4ccc {
            ins.idle_armed = false;
            ins.idle_at = 0;
            return false;
        }
        if !ins.idle_armed || rearm {
            ins.idle_at = t;
            ins.idle_armed = true;
        }
        if t.wrapping_sub(ins.idle_at) as u32 <= 500 {
            return false;
        }
        let wav = GROUP_BASE[0xa] + rand_mod(rng, 5);
        ins.say(0xa, -1, wav, self.elapsed_cs(), car.damage, out);
        ins.idle_armed = false;
        ins.idle_at = 0;
        true
    }

    /// `FUN_00474c50`: the frame's speech, then the nags.
    pub(super) fn speech_frame(
        &mut self,
        ins: &mut Instructor,
        car: &SchoolCar,
        rng: &mut GameLcg,
        out: &mut Vec<SchoolCue>,
    ) {
        let said = self.speech_events(ins, car, rng, out);
        if ins.speaking(car.damage) || ins.cooling(self.elapsed_cs(), rng) || said {
            return;
        }
        if self.reversing_nag(ins, car, out) {
            return;
        }
        self.idle_nag(ins, self.elapsed_cs(), false, car, rng, out);
    }

    /// `FUN_00475d20`: the fourth loop done.
    pub(super) fn near_end_speech(
        &self,
        ins: &mut Instructor,
        car: &SchoolCar,
        out: &mut Vec<SchoolCue>,
    ) {
        ins.say(
            0xe,
            -1,
            GROUP_BASE[0xe] + 3,
            self.elapsed_cs(),
            car.damage,
            out,
        );
    }

    /// `FUN_004753c0`'s speech: too many cones.
    pub(super) fn cones_speech(
        &self,
        ins: &mut Instructor,
        car: &SchoolCar,
        rng: &mut GameLcg,
        out: &mut Vec<SchoolCue>,
    ) {
        let wav = GROUP_BASE[0xd] + 3 + rand_mod(rng, 3);
        ins.say(0xd, -1, wav, self.elapsed_cs(), car.damage, out);
    }

    /// `FUN_00475cc0` for the task FSM.
    pub(super) fn instructor_speaking(ins: &mut Instructor, car: &SchoolCar) -> bool {
        ins.speaking(car.damage)
    }

    /// `FUN_00471f30` then `FUN_00474830`: the line's advice from its
    /// errors `{speed, direction, sliding, distance, brake, handbrake,
    /// cones}`.
    pub(super) fn line_advice(
        &self,
        mut e: [i32; 7],
        ins: &mut Instructor,
        car: &SchoolCar,
        rng: &mut GameLcg,
        out: &mut Vec<SchoolCue>,
    ) {
        let loop_task = self.loop_task;
        // `FUN_00471f30`.
        e[3] /= 0xef42;
        e[2] /= 500;
        e[5] /= 100;
        e[0] /= if loop_task { 1000 } else { 200 };
        if loop_task {
            e[6] /= if e[6] > 0 { 8000 } else { 3000 };
        }
        let now = self.elapsed_cs();
        if ins.speaking(car.damage) || ins.cooling(now, rng) {
            return;
        }
        ins.corrected = false;
        let mut judge = [-1i32; 6];
        judge[0] = if e[0] > 40 {
            0
        } else if e[0] < -10 {
            2
        } else {
            1
        };
        if loop_task {
            judge[1] = -1;
            e[1] = 0;
        } else {
            let limit = if self.task as i8 > 0 { 340 } else { 20 };
            judge[1] = i32::from((-limit..=limit).contains(&e[1]));
        }
        e[1] /= 10;
        if loop_task {
            judge[3] = -1;
            e[3] = 0;
        } else {
            judge[3] = if e[3].wrapping_abs() <= 40 { 1 } else { -1 };
        }
        let slide = car.slide().wrapping_abs();
        judge[2] = if e[2] < -55 && e[0] > -5 {
            2
        } else if e[2] > 55 {
            0
        } else if slide > 30000 && e[2] >= -55 {
            1
        } else {
            e[2] = 0;
            -1
        };
        let thr = if loop_task { 45 } else { 30 };
        let judged = |v: &mut i32, pedal_gate: bool| {
            if *v > thr && pedal_gate {
                0
            } else if *v < -thr && pedal_gate {
                2
            } else if *v > -thr && *v < thr {
                1
            } else {
                *v = 0;
                -1
            }
        };
        let braking_gate = e[0] < 0;
        judge[4] = judged(&mut e[4], braking_gate);
        judge[5] = judged(&mut e[5], true);
        // `FUN_00475de0`: errors the player is already correcting.
        if e[5] < -30 && car.handbrake_demand > 0 {
            e[5] = 0;
        }
        if e[2] < -55 && slide < 1000 {
            e[2] = 0;
        }
        if !car.analog_throttle {
            if e[4] < -30 && car.brake_demand > 0 {
                e[4] = 0;
            }
            if e[0] < -10 && car.throttle > 0 {
                e[0] = 0;
            }
        }
        let mut worst = 0;
        let mut c = 0;
        for (i, v) in e[..6].iter().enumerate() {
            let a = v.wrapping_abs();
            if a > worst {
                worst = a;
                c = i;
            }
        }
        if loop_task {
            let wide = if self.task == 1 && self.level as i8 > 0 {
                10
            } else {
                40
            };
            if worst <= e[6].wrapping_abs() {
                if e[6] < -40 {
                    ins.corrected = true;
                    let _ = rng.next_raw();
                    ins.say(0xf, -1, GROUP_BASE[0xf], now, car.damage, out);
                    return;
                }
                if e[6] > wide {
                    ins.corrected = true;
                    let wav = rand_mod(rng, 3) + GROUP_BASE[0x10];
                    ins.say(0x10, -1, wav, now, car.damage, out);
                    return;
                }
            }
        }
        let j = judge[c];
        if j == 0 || j == 2 {
            ins.corrected = true;
        }
        let sub = SUB_FIRST[c] + j;
        let mut pool = POOL[c];
        if pool == 0 {
            return;
        }
        let mut pick = -1;
        if c == 0 {
            if slide > 0 && sub == 0 {
                pick = 6;
            }
            if loop_task && sub == 2 {
                pool = 5;
            }
        }
        let w = if pick == -1 {
            rand_mod(rng, pool)
        } else {
            pick
        };
        let base: i32 = SUB_COUNT.iter().take(sub.max(0) as usize).sum();
        if j != -1 {
            ins.say(c as u8, j as i16, base + w, now, car.damage, out);
        }
    }
}

impl SchoolCar {
    /// `car +0xd4 . car +0x270`.
    fn slide(&self) -> i32 {
        qmul(self.velocity[0], self.lateral[0])
            .wrapping_add(qmul(self.velocity[1], self.lateral[1]))
            .wrapping_add(qmul(self.velocity[2], self.lateral[2]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bank_layout_follows_the_load_order() {
        assert_eq!(advice_path(0).unwrap(), "advice\\speed\\toofast\\0.wav");
        assert_eq!(advice_path(21).unwrap(), "advice\\dirctn\\wrong\\0.wav");
        assert_eq!(advice_path(40).unwrap(), "advice\\position\\wrong\\0.wav");
        assert_eq!(
            advice_path(47).unwrap(),
            "advice\\hndbrake\\toolttle\\0.wav"
        );
        assert_eq!(advice_path(55).unwrap(), "advice\\damage\\damage7.wav");
        assert_eq!(advice_path(58).unwrap(), "advice\\apcrash\\0.wav");
        assert_eq!(advice_path(66).unwrap(), "advice\\failing\\0.wav");
        assert_eq!(advice_path(80).unwrap(), "advice\\objects\\3.wav");
        assert_eq!(advice_path(86).unwrap(), "advice\\nearend\\3.wav");
        assert_eq!(advice_path(87).unwrap(), "advice\\cones\\tooclose\\0.wav");
        assert_eq!(advice_path(90).unwrap(), "advice\\cones\\toowide\\2.wav");
        assert_eq!(advice_path(91), None);
        // The category bases line up with the sub-list counts.
        let judged: i32 = SUB_COUNT.iter().sum();
        assert_eq!(judged, GROUP_BASE[6]);
        for (c, first) in SUB_FIRST.iter().enumerate() {
            let base: i32 = SUB_COUNT.iter().take(*first as usize).sum();
            assert_eq!(base, GROUP_BASE[c]);
        }
    }

    fn reference_race(level: u8, task: u8) -> SchoolRace {
        let reference =
            cmr_formats::school_rsf::parse_rsf(&vec![0; cmr_formats::school_rsf::RSF_BLOCK_SIZE])
                .unwrap();
        let scoring = super::super::SchoolScoring::from_blocks(&[0; 288], &[0; 288]).unwrap();
        SchoolRace::start(level, task, 10, reference, scoring)
    }

    fn advice(out: &[SchoolCue]) -> Vec<u8> {
        out.iter()
            .map(|c| match c {
                SchoolCue::Advice(i) => *i,
            })
            .collect()
    }

    #[test]
    fn a_fast_line_is_told_too_fast_and_a_quiet_spell_follows() {
        let mut race = reference_race(1, 2);
        race.race_start();
        // 20 s into the run, past any cool-down from 0.
        race.now = 20 << 16;
        let mut ins = Instructor::default();
        ins.start_task(1, 2);
        let car = SchoolCar::default();
        let mut rng = GameLcg::new(3);
        let mut out = Vec::new();
        // 60 km/h-ish too fast after the /200 scale; everything else fine.
        let e = [200 * 60, 0, 0, 0, 0, 0, 0];
        race.line_advice(e, &mut ins, &car, &mut rng, &mut out);
        let said = advice(&out);
        assert_eq!(said.len(), 1);
        assert!(said[0] < 7, "speed\\toofast");
        assert!(ins.corrected);
        // The next line inside the cool-down says nothing.
        ins.begin_frame(false);
        race.line_advice(e, &mut ins, &car, &mut rng, &mut out);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn a_correct_line_is_praised_from_the_correct_list() {
        let mut race = reference_race(1, 2);
        race.race_start();
        race.now = 20 << 16;
        let mut ins = Instructor::default();
        let mut rng = GameLcg::new(9);
        let mut out = Vec::new();
        // Only the direction is off, and within the task's 340: correct.
        let e = [0, 300, 0, 0, 0, 0, 0];
        race.line_advice(e, &mut ins, &SchoolCar::default(), &mut rng, &mut out);
        let said = advice(&out);
        assert_eq!(said.len(), 1);
        assert!((26..31).contains(&said[0]), "dirctn\\correct");
        assert!(!ins.corrected);
    }

    #[test]
    fn the_wait_for_the_throttle_nags_when_it_times_out() {
        // Level 2 waits 2 s.
        let mut race = reference_race(2, 0);
        assert_eq!(race.fsm, 0);
        let mut statics = super::super::SchoolStatics::default();
        let mut rng = GameLcg::new(5);
        let mut said = Vec::new();
        for _ in 0..80 {
            statics.instructor.begin_frame(false);
            said.extend(advice(&race.tick(
                &SchoolCar::default(),
                false,
                &mut rng,
                &mut statics,
            )));
            if race.fsm != 0 {
                break;
            }
        }
        assert_eq!(race.fsm, 1, "the clock started at the timeout");
        assert_eq!(said.len(), 1);
        assert!((61..66).contains(&said[0]), "sniper");
    }

    #[test]
    fn a_non_loop_task_says_near_end_once_twenty_nodes_out() {
        let mut race = reference_race(1, 2);
        race.race_start();
        race.now = 30 << 16;
        let mut ins = Instructor::default();
        let mut rng = GameLcg::new(11);
        let mut out = Vec::new();
        let mut car = SchoolCar {
            cat_remaining: 25,
            speed: 0x8000,
            ..SchoolCar::default()
        };
        race.speech_frame(&mut ins, &car, &mut rng, &mut out);
        assert!(out.is_empty());
        car.cat_remaining = 19;
        race.now = 40 << 16;
        ins.begin_frame(false);
        race.speech_frame(&mut ins, &car, &mut rng, &mut out);
        assert_eq!(advice(&out), [83]);
        race.now = 60 << 16;
        ins.begin_frame(false);
        race.speech_frame(&mut ins, &car, &mut rng, &mut out);
        assert_eq!(advice(&out), [83], "only once");
    }

    #[test]
    fn going_the_wrong_way_ends_in_a_failed_task() {
        let mut race = reference_race(1, 2);
        race.race_start();
        let mut ins = Instructor::default();
        ins.start_task(1, 2);
        let mut rng = GameLcg::new(13);
        let mut out = Vec::new();
        let car = SchoolCar {
            reverse_count_5: true,
            cat_remaining: 100,
            speed: 0x8000,
            ..SchoolCar::default()
        };
        for second in 0..200 {
            race.now = (10 + second * 10) << 16;
            ins.begin_frame(false);
            race.speech_frame(&mut ins, &car, &mut rng, &mut out);
            if race.done {
                break;
            }
        }
        assert!(race.done, "failed then abandoned");
        assert_eq!(race.marks(), [0; 4], "every mark 0");
        let said = advice(&out);
        assert!(
            said.iter().take(2).all(|i| (21..26).contains(i)),
            "wrong way"
        );
        assert!(said[2..].iter().all(|i| (66..77).contains(i)), "failing");
    }

    #[test]
    fn the_wrong_way_pool_hands_over_to_failing_after_two() {
        let mut ins = Instructor::default();
        ins.start_task(1, 0);
        let mut rng = GameLcg::new(7);
        let mut out = Vec::new();
        for _ in 0..2 {
            assert!(matches!(
                ins.wrong_way(&mut rng, 0, 0, &mut out),
                WrongWay::Said(true)
            ));
        }
        assert!(matches!(
            ins.wrong_way(&mut rng, 0, 0, &mut out),
            WrongWay::Failing
        ));
        assert_eq!(out.len(), 2);
        let mut seen: Vec<u8> = out
            .iter()
            .map(|c| match c {
                SchoolCue::Advice(i) => *i,
            })
            .collect();
        seen.dedup();
        assert_eq!(seen.len(), 2, "two different wrong-way wavs");
        assert!(seen.iter().all(|i| (21..26).contains(i)));
    }
}
