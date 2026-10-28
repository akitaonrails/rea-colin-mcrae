//! The car reset: the dispatcher `FUN_00467210` and the fade-and-place
//! routine `FUN_00414850`, run per car at the end of the in-race mechanics
//! routine `FUN_0043afd0` (`0x43b517`, after `FUN_00417e20`, the ground pass
//! `FUN_00410a60` and `FUN_00415df0`).
//!
//! Observations (pseudocode level, instruction level where noted):
//!
//! - `FUN_00467210(player)` returns unless the car's latch `+0x67c` is set and
//!   either `DAT_0057b0ac[player] == 100` (set by the race-start init
//!   `FUN_00437510` on both of its branches) or `DAT_00d3ca88 != 0`.
//! - With no `.cat` nodes (`DAT_0057c448 == 0`) it resets the car where it is:
//!   position `+0xc8..+0xd0`, direction row 0 (`+0x258..+0x260`).
//! - Otherwise it picks a node by mode. The loop mode (`FUN_00485bb0`, config
//!   `DAT_00d3ca80 +0x166/+0x168` bits, only while `DAT_00d3ca88 == 0`) starts
//!   at `DAT_0057b030[player] + cursor` wrapped below the node count and walks
//!   back (wrapping) while the node lies within `0x50000` of the comparison car
//!   on both x and z; the cursor is only wrapped when negative. The rewind
//!   mode (`FUN_00401060`: `+0x166 & 0x1c == 0xc`, `DAT_00d3ca88 == 0`) walks
//!   back from the cursor the same way without wrapping (falling to node 1 when
//!   every node is near), then skips back over nodes whose byte `+0x19` is set,
//!   stopping at node 0. The plain mode only does that skip. Both write the
//!   cursor `DAT_0057ae70[player]`. The comparison car is car record 1 for
//!   player 0 and car record 0 otherwise (instruction level, `0x46733c` and
//!   `0x4674a3`).
//! - `FUN_00414850(car, ctl, position, direction, immediate)`: the first call
//!   arms `ctl +0x41c`, sets the countdown `ctl +0x424 = 0xa0000` and the fade
//!   `ctl +0x428 = 0`. Each later call steps the countdown by `-0x10000`, or
//!   with the penalty flag `ctl +0x460` by `-0x8000` while positive and
//!   `-0x18000` otherwise (a sound cue `FUN_0047fcd0(0x21c0000)` and the empty
//!   `FUN_00488d50` when it lands on zero). The fade byte is
//!   `255 * (1 - |countdown| / 10)` (doubled, saturating, with the penalty).
//!   Below `-0x9ffff` it clears the latch, the arm and the fade, and with the
//!   penalty clears it and sets car `+0x760`. The car is placed when the
//!   countdown reaches zero, or immediately when `immediate` is set.
//! - Placement: row 0 is the direction flattened onto x/z and normalized, row 1
//!   up, row 2 `(row0.z, 0, -row0.x)`; the car is put `100.0` below the node,
//!   the probes rebuilt (`FUN_00409c10`), the eight lanes queried
//!   (`FUN_00419a40`) and the ground normal derived (`FUN_00408fe0`); the basis
//!   is re-orthonormalized onto that normal, the probes rebuilt and requeried,
//!   the probes scanned (`FUN_004126b0`), the car lifted onto the ground
//!   (`FUN_00412750`), rescanned, and the contact flags republished
//!   (`FUN_0040d740`). The motion, collision, tumbling, rollover, drivetrain,
//!   pedal, steering and lane-handle words are then cleared and the gear set
//!   to 1 (see [`crate::race_state::RaceState::apply_reset_placement`]).
//!
//! Not reconstructed: the split-time HUD bookkeeping in `FUN_00467210`
//! (`DAT_00573a80`, `DAT_0050c584`, `DAT_005739e0`), the render matrices
//! `ctl[0]`/`ctl[1]`, and the sound cue. The camera snap (`DAT_00551560`, read
//! by the mechanics routine to call `FUN_00434870`) is reported to the caller.

use cmr_formats::stage::CatNode;

use crate::compiled_car_initializer::{
    CompiledEightLaneExecutionFailure, CompiledLaneConfigProvider, CompiledLaneGeometryProvider,
    CompiledPostLaneBasisInputs, CompiledPostLaneNormalInput, derive_compiled_post_lane_basis,
    derive_compiled_post_lane_global_normal, normalize_compiled_lane_vector,
};
use crate::fixed_point::{Fixed16, Vec3};
use crate::ground_pass::{
    GroundContact, ground_penetration_lift, query, rebuild_ground_probes, scan,
};
use crate::vehicle_dynamics::classify_contact_distance_lanes;

/// `DAT_0057b0ac[player]` once the race-start init has run.
pub const RACE_RUNNING_57B0AC: i32 = 100;
/// The countdown's starting value `ctl +0x424`.
pub const RESET_COUNTDOWN_START: i32 = 0xa_0000;
/// The countdown ends once it falls below this value.
pub const RESET_COUNTDOWN_END: i32 = -0x9_ffff;
/// The placement starts this far below the node before the ground lift.
pub const RESET_DROP_HEIGHT: i32 = 0x64_0000;
/// The x and z reach within which a node counts as near the comparison car.
pub const RESET_NODE_REACH: i32 = 0x5_0000;

/// The reset words in the control record.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResetFadeState {
    /// `ctl +0x41c`: a reset is running.
    pub active_41c: bool,
    /// `ctl +0x424`: the countdown, from `10.0` through zero to `-10.0`.
    pub countdown_424: i32,
    /// `ctl +0x428`: the screen fade level.
    pub fade_428: u8,
    /// `ctl +0x460`: the slow penalty reset.
    pub penalty_460: bool,
}

/// What one `FUN_00414850` call asks the caller to do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResetFadeStep {
    /// Place the car on the target this call.
    pub place: bool,
    /// The penalty countdown landed on zero (sound cue).
    pub penalty_cue: bool,
    /// The countdown ended: clear the latch `+0x67c`.
    pub finished: bool,
    /// A penalty reset ended: set car `+0x760`.
    pub penalty_done_760: bool,
}

/// The fade byte for a countdown value.
#[must_use]
pub fn reset_fade_level(countdown: i32, penalty: bool) -> u8 {
    let magnitude = countdown.wrapping_abs();
    let share = 0x1_0000_i32.wrapping_sub(qmul(magnitude, 0x1999));
    let scaled = qmul(share, 0xff_0000) as u32;
    let rounded = scaled.wrapping_add(((scaled as i32) >> 31) as u32 & 0xffff);
    let level = (rounded >> 16) as u8;
    if !penalty {
        level
    } else if level < 0x80 {
        level << 1
    } else {
        0xff
    }
}

/// One `FUN_00414850` countdown step.
pub fn step_reset_fade(state: &mut ResetFadeState, immediate: bool) -> ResetFadeStep {
    let mut step = ResetFadeStep::default();
    if !state.active_41c {
        state.active_41c = true;
        state.countdown_424 = RESET_COUNTDOWN_START;
        state.fade_428 = 0;
        step.place = immediate;
        return step;
    }
    let previous = state.countdown_424;
    if state.penalty_460 {
        let delta = if previous < 1 { -0x1_8000 } else { -0x8000 };
        state.countdown_424 = previous.wrapping_add(delta);
        step.penalty_cue = state.countdown_424 == 0;
    } else {
        state.countdown_424 = previous.wrapping_sub(0x1_0000);
    }
    let landed = state.countdown_424 == 0;
    state.fade_428 = reset_fade_level(state.countdown_424, state.penalty_460);
    if state.countdown_424 < RESET_COUNTDOWN_END {
        step.finished = true;
        state.active_41c = false;
        state.fade_428 = 0;
        if state.penalty_460 {
            state.penalty_460 = false;
            step.penalty_done_760 = true;
        }
    }
    step.place = landed || immediate;
    step
}

/// `FUN_00485bb0`: the looping-course node walk.
#[must_use]
pub fn loop_reset_mode(config_166: u8, config_168: u8) -> bool {
    if config_166 & 0x40 != 0 {
        return config_168 & 0xf0 != 0x20;
    }
    config_166 & 0x20 != 0 || config_168 & 0xf0 == 0x60
}

/// `FUN_00401060`: the rewind-by-distance node walk.
#[must_use]
pub fn rewind_reset_mode(config_166: u8, replay_d3ca88: bool) -> bool {
    config_166 & 0x1c == 0xc && !replay_d3ca88
}

/// The inputs `FUN_00467210` reads besides the car's latch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResetDispatchInput<'a> {
    pub nodes: &'a [CatNode],
    /// `DAT_0057ae70[player]`.
    pub cursor: i32,
    /// `DAT_0057b030[player]`, the loop mode's node offset.
    pub loop_offset_57b030: i32,
    /// `DAT_0057b0ac[player]`.
    pub race_state_57b0ac: i32,
    /// `DAT_00d3ca88`.
    pub replay_d3ca88: bool,
    /// `DAT_00d3ca80 +0x166/+0x168`.
    pub config_166: u8,
    pub config_168: u8,
    /// The x and z of the comparison car (record 1 for player 0).
    pub comparison_xz: [i32; 2],
    /// This car's position and row 0, for the node-less reset.
    pub position: Vec3,
    pub row0: Vec3,
}

/// Where `FUN_00467210` places the car.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResetTarget {
    pub position: [i32; 3],
    pub direction: [i32; 3],
    /// The new `DAT_0057ae70[player]`, when the dispatcher writes it.
    pub cursor: Option<i32>,
}

/// A node walk read past the node array, or the loop walk never left the
/// comparison car's reach (the native would not terminate).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResetNodeError {
    OutOfRange { index: i32, count: usize },
    LoopNeverLeavesReach,
}

fn node_at(nodes: &[CatNode], index: i32) -> Result<&CatNode, ResetNodeError> {
    usize::try_from(index)
        .ok()
        .and_then(|i| nodes.get(i))
        .ok_or(ResetNodeError::OutOfRange {
            index,
            count: nodes.len(),
        })
}

fn near(node: &CatNode, comparison_xz: [i32; 2]) -> bool {
    node.position[0]
        .wrapping_sub(comparison_xz[0])
        .wrapping_abs()
        <= RESET_NODE_REACH
        && node.position[2]
            .wrapping_sub(comparison_xz[1])
            .wrapping_abs()
            <= RESET_NODE_REACH
}

fn skip_excluded(nodes: &[CatNode], mut index: i32) -> Result<i32, ResetNodeError> {
    loop {
        if node_at(nodes, index)?.reserved == 0 {
            break;
        }
        index -= 1;
        if index <= 0 {
            break;
        }
    }
    Ok(index)
}

/// `FUN_00467210`'s gate and target choice. `None` when no reset runs.
///
/// # Errors
///
/// A node index outside the array, or a loop walk that never ends.
pub fn select_reset_target(
    latch_67c: bool,
    input: &ResetDispatchInput,
) -> Result<Option<ResetTarget>, ResetNodeError> {
    if !latch_67c || (input.race_state_57b0ac != RACE_RUNNING_57B0AC && !input.replay_d3ca88) {
        return Ok(None);
    }
    let nodes = input.nodes;
    if nodes.is_empty() {
        return Ok(Some(ResetTarget {
            position: [input.position.x, input.position.y, input.position.z].map(Fixed16::raw),
            direction: [input.row0.x, input.row0.y, input.row0.z].map(Fixed16::raw),
            cursor: None,
        }));
    }
    let count = i32::try_from(nodes.len()).unwrap_or(i32::MAX);
    let (index, cursor) =
        if !input.replay_d3ca88 && loop_reset_mode(input.config_166, input.config_168) {
            let mut index = input.loop_offset_57b030.wrapping_add(input.cursor);
            while index > count - 1 {
                index -= count;
            }
            let mut steps = 0;
            while near(node_at(nodes, index)?, input.comparison_xz) {
                steps += 1;
                if steps > nodes.len() {
                    return Err(ResetNodeError::LoopNeverLeavesReach);
                }
                index -= 1;
                if index < 0 {
                    index += count;
                }
            }
            let cursor = (input.cursor < 0).then(|| input.cursor.wrapping_add(count));
            (index.max(0), cursor)
        } else {
            let mut index = input.cursor;
            if rewind_reset_mode(input.config_166, input.replay_d3ca88) {
                let mut left_reach = false;
                while index >= 0 {
                    if !near(node_at(nodes, index)?, input.comparison_xz) {
                        left_reach = true;
                        break;
                    }
                    index -= 1;
                }
                if !left_reach {
                    index = skip_excluded(nodes, 1)?;
                } else if index > 0 {
                    index = skip_excluded(nodes, index)?;
                }
            } else if index > 0 {
                index = skip_excluded(nodes, index)?;
            }
            let index = index.max(0);
            (index, Some(index))
        };
    let node = node_at(nodes, index)?;
    Ok(Some(ResetTarget {
        position: node.position,
        direction: node.direction,
        cursor,
    }))
}

/// The placed car, before the state clears.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResetPlacement {
    pub position: Vec3,
    pub basis: [Vec3; 3],
    pub normal: Vec3,
    pub ground: GroundContact,
    pub contact_flags: [bool; 4],
    pub contact_count: u8,
}

/// The geometric half of `FUN_00414850`'s placement.
///
/// # Errors
///
/// Propagates a ground-query dispatch or config failure.
pub fn place_on_target<P, C>(
    target: &ResetTarget,
    ground: GroundContact,
    providers: &mut [P; 8],
    config: &mut C,
) -> Result<ResetPlacement, CompiledEightLaneExecutionFailure<P::Error, C::Error>>
where
    P: CompiledLaneGeometryProvider,
    C: CompiledLaneConfigProvider,
{
    let mut ground = ground;
    let row0 = normalize_compiled_lane_vector([target.direction[0], 0, target.direction[2]]);
    let mut basis = [
        vec(row0),
        Vec3::new(Fixed16::ZERO, Fixed16::ONE, Fixed16::ZERO),
        vec([row0[2], 0, row0[0].wrapping_neg()]),
    ];
    let mut position = vec(target.position);
    position.y = Fixed16::from_raw(position.y.raw().wrapping_sub(RESET_DROP_HEIGHT));

    ground.probes = rebuild_ground_probes(&ground, basis, position);
    query(&mut ground, true, providers, config)?;
    let normal = derive_compiled_post_lane_global_normal(CompiledPostLaneNormalInput {
        source_vectors: ground.probes,
        lane_words: ground.lanes.lane_words,
        selector_words: basis.map(|row| row.y.raw()),
    })
    .final_published_normal;
    let rows = derive_compiled_post_lane_basis(CompiledPostLaneBasisInputs {
        vector_324: normal,
        vector_258: raw(basis[0]),
    });
    basis = [rows.vector_258, rows.vector_264, rows.vector_270].map(vec);

    ground.probes = rebuild_ground_probes(&ground, basis, position);
    query(&mut ground, true, providers, config)?;
    ground.thresholds = scan(&ground);
    let lift = ground_penetration_lift(
        ground.lanes.lane_words,
        &ground.probes,
        ground.thresholds.flags_73a,
    );
    position.y = Fixed16::from_raw(position.y.raw().wrapping_add(lift));
    for probe in &mut ground.probes {
        probe[1] = probe[1].wrapping_add(lift);
    }
    ground.thresholds = scan(&ground);
    let (contact_flags, contact_count) = classify_contact_distance_lanes(
        std::array::from_fn(|lane| ground.lanes.lane_words[lane]),
        std::array::from_fn(|lane| ground.probes[lane][1]),
    );
    Ok(ResetPlacement {
        position,
        basis,
        normal: vec(normal),
        ground,
        contact_flags,
        contact_count,
    })
}

fn qmul(left: i32, right: i32) -> i32 {
    Fixed16::from_raw(left).mul(Fixed16::from_raw(right)).raw()
}

fn raw(vector: Vec3) -> [i32; 3] {
    [vector.x.raw(), vector.y.raw(), vector.z.raw()]
}

fn vec(words: [i32; 3]) -> Vec3 {
    Vec3::new(
        Fixed16::from_raw(words[0]),
        Fixed16::from_raw(words[1]),
        Fixed16::from_raw(words[2]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(x: i32, z: i32, excluded: u8) -> CatNode {
        CatNode {
            position: [x, 0x1_0000, z],
            direction: [0, 0, 0x1_0000],
            split_flag: 0,
            reserved: excluded,
            seq_index: 0,
        }
    }

    fn input<'a>(nodes: &'a [CatNode], cursor: i32) -> ResetDispatchInput<'a> {
        ResetDispatchInput {
            nodes,
            cursor,
            loop_offset_57b030: 0,
            race_state_57b0ac: RACE_RUNNING_57B0AC,
            replay_d3ca88: false,
            config_166: 0,
            config_168: 0,
            comparison_xz: [0, 0],
            position: Vec3::ZERO,
            row0: Vec3::new(Fixed16::ONE, Fixed16::ZERO, Fixed16::ZERO),
        }
    }

    #[test]
    fn the_countdown_places_at_zero_and_ends_below_minus_ten() {
        let mut state = ResetFadeState::default();
        let first = step_reset_fade(&mut state, false);
        assert!(!first.place);
        assert!(state.active_41c);
        assert_eq!(state.countdown_424, RESET_COUNTDOWN_START);
        let mut placed_at = None;
        let mut finished_at = None;
        for tick in 1..=30 {
            let step = step_reset_fade(&mut state, false);
            if step.place {
                assert!(placed_at.is_none());
                placed_at = Some(tick);
                assert_eq!(state.fade_428, 0xff);
            }
            if step.finished {
                finished_at = Some(tick);
                break;
            }
        }
        assert_eq!(placed_at, Some(10));
        // -10.0 (`-0xa0000`) is already below `-0x9ffff`.
        assert_eq!(finished_at, Some(20));
        assert!(!state.active_41c);
        assert_eq!(state.fade_428, 0);
    }

    #[test]
    fn an_immediate_reset_places_on_the_arming_call() {
        let mut state = ResetFadeState::default();
        assert!(step_reset_fade(&mut state, true).place);
    }

    #[test]
    fn the_penalty_countdown_is_slower_cues_at_zero_and_reports_its_end() {
        let mut state = ResetFadeState {
            penalty_460: true,
            ..ResetFadeState::default()
        };
        step_reset_fade(&mut state, false);
        let mut cue_ticks = Vec::new();
        let mut end = None;
        for tick in 1..=40 {
            let step = step_reset_fade(&mut state, false);
            if step.penalty_cue {
                assert!(step.place);
                cue_ticks.push(tick);
            }
            if step.finished {
                assert!(step.penalty_done_760);
                end = Some(tick);
                break;
            }
        }
        // 20 half-steps reach zero, then 1.5-unit steps reach -10.5.
        assert_eq!(cue_ticks, [20]);
        assert_eq!(end, Some(27));
        assert!(!state.penalty_460);
    }

    #[test]
    fn the_fade_level_is_full_at_zero_and_empty_at_ten() {
        assert_eq!(reset_fade_level(0, false), 0xff);
        assert_eq!(reset_fade_level(0xa_0000, false), 0);
        assert_eq!(reset_fade_level(-0x5_0000, false), 0x7f);
        assert_eq!(reset_fade_level(-0x5_0000, true), 0xfe);
        assert_eq!(reset_fade_level(0, true), 0xff);
    }

    #[test]
    fn the_mode_predicates_follow_the_config_bits() {
        assert!(!loop_reset_mode(0, 0));
        assert!(loop_reset_mode(0x20, 0));
        assert!(loop_reset_mode(0, 0x60));
        assert!(loop_reset_mode(0x40, 0x10));
        assert!(!loop_reset_mode(0x40, 0x20));
        assert!(rewind_reset_mode(0x0c, false));
        assert!(!rewind_reset_mode(0x0c, true));
        assert!(!rewind_reset_mode(0x1c, false));
    }

    #[test]
    fn no_reset_runs_without_the_latch_or_a_running_race() {
        let nodes = [node(0, 0, 0)];
        assert_eq!(select_reset_target(false, &input(&nodes, 0)), Ok(None));
        let mut waiting = input(&nodes, 0);
        waiting.race_state_57b0ac = 0;
        assert_eq!(select_reset_target(true, &waiting), Ok(None));
        waiting.replay_d3ca88 = true;
        assert!(select_reset_target(true, &waiting).unwrap().is_some());
    }

    #[test]
    fn a_stage_without_nodes_resets_the_car_where_it_is() {
        let mut here = input(&[], 0);
        here.position = vec([1, 2, 3]);
        let target = select_reset_target(true, &here).unwrap().unwrap();
        assert_eq!(target.position, [1, 2, 3]);
        assert_eq!(target.direction, [0x1_0000, 0, 0]);
        assert_eq!(target.cursor, None);
    }

    #[test]
    fn the_plain_mode_skips_back_over_excluded_nodes_but_not_past_node_zero() {
        let nodes = [node(0, 0, 1), node(0, 0, 0), node(0, 0, 1), node(0, 0, 1)];
        let target = select_reset_target(true, &input(&nodes, 3))
            .unwrap()
            .unwrap();
        assert_eq!(target.cursor, Some(1));
        let only_excluded = [node(0, 0, 1), node(0, 0, 1)];
        let target = select_reset_target(true, &input(&only_excluded, 1))
            .unwrap()
            .unwrap();
        assert_eq!(target.cursor, Some(0));
        // A cursor at node 0 is not checked at all.
        let target = select_reset_target(true, &input(&nodes, 0))
            .unwrap()
            .unwrap();
        assert_eq!(target.cursor, Some(0));
    }

    #[test]
    fn the_rewind_mode_walks_back_out_of_the_comparison_reach() {
        let nodes = [
            node(0, 0, 0),
            node(0x10_0000, 0, 0),
            node(0x20_0000, 0, 0),
            node(0x20_0000, 0x4_0000, 0),
        ];
        let mut rewind = input(&nodes, 3);
        rewind.config_166 = 0x0c;
        rewind.comparison_xz = [0x20_0000, 0];
        let target = select_reset_target(true, &rewind).unwrap().unwrap();
        assert_eq!(target.cursor, Some(1));
        assert_eq!(target.position, nodes[1].position);
        // Every node near: the walk falls to node 1.
        let close = [node(0, 0, 0), node(0, 0, 0)];
        let mut all_near = input(&close, 1);
        all_near.config_166 = 0x0c;
        let target = select_reset_target(true, &all_near).unwrap().unwrap();
        assert_eq!(target.cursor, Some(1));
        let single = [node(0, 0, 0)];
        let mut one = input(&single, 0);
        one.config_166 = 0x0c;
        assert_eq!(
            select_reset_target(true, &one),
            Err(ResetNodeError::OutOfRange { index: 1, count: 1 })
        );
    }

    #[test]
    fn the_loop_mode_wraps_and_keeps_the_cursor() {
        let nodes = [node(0x30_0000, 0, 0), node(0, 0, 1), node(0, 0, 0)];
        let mut looped = input(&nodes, 2);
        looped.config_166 = 0x20;
        looped.loop_offset_57b030 = 3;
        let target = select_reset_target(true, &looped).unwrap().unwrap();
        // 2 + 3 wraps to 2, then 2 and 1 are near; node 0 is far.
        assert_eq!(target.position, nodes[0].position);
        assert_eq!(target.cursor, None);
        looped.cursor = -1;
        looped.loop_offset_57b030 = 1;
        let target = select_reset_target(true, &looped).unwrap().unwrap();
        assert_eq!(target.cursor, Some(2));
        let near_only = [node(0, 0, 0), node(0, 0, 0)];
        let mut stuck = input(&near_only, 0);
        stuck.config_166 = 0x20;
        assert_eq!(
            select_reset_target(true, &stuck),
            Err(ResetNodeError::LoopNeverLeavesReach)
        );
    }

    struct FlatGround {
        height: i32,
    }

    impl CompiledLaneGeometryProvider for FlatGround {
        type Error = ();
        fn validate(&mut self, _source: [i32; 3], _handle: i16) -> Result<bool, ()> {
            Ok(true)
        }
        fn search(&mut self, _source: [i32; 3], _fallback: i32) -> Result<Option<i16>, ()> {
            Ok(Some(0))
        }
        fn build(
            &mut self,
            _source: [i32; 3],
            _handle: i16,
            _fallback: i32,
        ) -> Result<crate::compiled_car_initializer::CompiledLaneBuildCompletion, ()> {
            Ok(
                crate::compiled_car_initializer::CompiledLaneBuildCompletion::Built {
                    lane_word: self.height,
                    type_word: 0,
                    output_vector: [0, 0x1_0000, 0],
                },
            )
        }
    }

    fn flat(height: i32) -> [FlatGround; 8] {
        std::array::from_fn(|_| FlatGround { height })
    }

    fn ground() -> GroundContact {
        use crate::compiled_car_initializer::{
            CompiledEightLaneExecutionState, CompiledLaneThresholdEffects,
            CompiledSetupPoseLaneInputs,
        };
        GroundContact {
            lanes: CompiledEightLaneExecutionState {
                handles: [5; 8],
                type_words: [0; 8],
                output_vectors: [[0; 3]; 8],
                lane_words: [0; 8],
                accumulator_words: [0x3_0000; 8],
                latch_67c: 1,
            },
            lane_inputs: CompiledSetupPoseLaneInputs {
                gate_words: [0; 8],
                drive_words: [0; 8],
                first_four_subtractions: [0; 4],
            },
            half_words: [0x1_0000, 0x8000, 0x2_0000],
            deformation: [0x1_0000; 3],
            probes: [[0; 3]; 8],
            thresholds: CompiledLaneThresholdEffects {
                flags_73a: [1; 8],
                flag_743: 1,
                count_742: 0,
                indices_744: [0; 8],
            },
            wheel_reach_648_644: [0; 2],
        }
    }

    fn config() -> impl FnMut(usize) -> Result<u8, std::convert::Infallible> {
        |_lane| Ok(0)
    }

    #[test]
    fn placement_faces_the_node_direction_and_settles_on_the_ground() {
        let height = 0x2_0000;
        let target = ResetTarget {
            position: [0x40_0000, height + 0x5_0000, -0x10_0000],
            direction: [0, 0x7_0000, 0x2_0000],
            cursor: Some(3),
        };
        let placed = place_on_target(&target, ground(), &mut flat(height), &mut config()).unwrap();
        // The legacy normalization lands a few LSBs off 1.0.
        let unit = |word: i32| (0xfff0..=0x1_0010).contains(&word);
        let [n, r0, r1, r2] = [
            placed.normal,
            placed.basis[0],
            placed.basis[1],
            placed.basis[2],
        ]
        .map(raw);
        assert!(n[0] == 0 && unit(n[1]) && n[2] == 0, "{n:?}");
        assert_eq!(r1, n);
        assert!(r0[0] == 0 && r0[1] == 0 && unit(r0[2]), "{r0:?}");
        assert!(unit(-r2[0]) && r2[1] == 0 && r2[2] == 0, "{r2:?}");
        assert_eq!(placed.position.x.raw(), 0x40_0000);
        assert_eq!(placed.position.z.raw(), -0x10_0000);
        // Dropped 100 units, then lifted so the lowest probe meets the ground.
        let lowest = placed.ground.probes.iter().map(|p| p[1]).min().unwrap();
        assert_eq!(lowest, height);
        assert!(placed.position.y.raw() < target.position[1]);
        assert_eq!(placed.ground.thresholds.flag_743, 0);
        assert!(placed.contact_count > 0);
    }

    #[test]
    fn the_race_state_fades_places_and_ends_the_reset() {
        use crate::race_state::{CarState, RaceState};
        let height = 0x2_0000;
        let nodes = [node(0x10_0000, 0x10_0000, 0), node(0x20_0000, 0x20_0000, 1)];
        let mut car = CarState::at_rest();
        car.ground = Some(ground());
        car.position = vec([0x7_0000, -0x50_0000, 0x7_0000]);
        car.velocity = vec([0x3_0000, -0x8_0000, 0]);
        car.motion.angular_velocity = vec([0x100, 0x200, 0x300]);
        car.collision.response_state_680 = true;
        car.four_wheel.gearbox.gear_43a = 4;
        car.four_wheel.speed_ec = 0x8_0000;
        let mut race = RaceState::new(car);
        let mut dispatch = input(&nodes, 1);
        dispatch.row0 = Vec3::ZERO;
        let mut placed_tick = None;
        for tick in 0..=20 {
            let outcome = race
                .run_reset(&dispatch, &mut flat(height), &mut config())
                .unwrap();
            // The excluded node 1 skips back to node 0.
            assert_eq!(outcome.cursor, Some(0));
            if outcome.placed {
                placed_tick = Some(tick);
                assert!(outcome.camera_reset);
                assert_eq!(outcome.fade_428, 0xff);
                let car = race.car();
                assert_eq!(car.position.x.raw(), 0x10_0000);
                assert_eq!(car.velocity, Vec3::ZERO);
                assert_eq!(car.motion.angular_velocity, Vec3::ZERO);
                assert!(!car.collision.response_state_680);
                assert_eq!(car.four_wheel.gearbox.gear_43a, 1);
                assert_eq!(car.four_wheel.speed_ec, 0);
                let ground = car.ground.unwrap();
                assert_eq!(ground.lanes.handles, [0; 8]);
                assert_eq!(ground.lanes.accumulator_words, [0; 8]);
                // The latch stays set until the fade-in ends.
                assert_eq!(ground.lanes.latch_67c, 1);
            }
        }
        assert_eq!(placed_tick, Some(10));
        assert_eq!(race.car().ground.unwrap().lanes.latch_67c, 0);
        assert!(!race.car().four_wheel.reset.active_41c);
        // With the latch clear the dispatcher does nothing.
        let idle = race
            .run_reset(&dispatch, &mut flat(height), &mut config())
            .unwrap();
        assert_eq!(idle, crate::race_state::ResetOutcome::default());
    }

    #[test]
    fn a_cursor_past_the_nodes_is_reported() {
        let nodes = [node(0, 0, 0)];
        assert_eq!(
            select_reset_target(true, &input(&nodes, 1)),
            Err(ResetNodeError::OutOfRange { index: 1, count: 1 })
        );
    }
}
