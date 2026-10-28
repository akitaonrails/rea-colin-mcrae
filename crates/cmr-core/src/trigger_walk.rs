//! The per-car trigger walk `FUN_00403a00`, run by the mechanics routine after
//! the driver and the static-scene collision (`0x43b327`), and the cell lists
//! the stage fix-up `FUN_004904e0` links the `.dat` records into.
//!
//! Observations (pseudocode level):
//!
//! - `FUN_004904e0` visits the records in file order and pushes each at the
//!   head of the list of the cell named by its `+0x28` word, so a cell walks
//!   its records in reverse file order.
//! - `FUN_00403a00(car, ctl, player)` clears `+0x6dc`, walks the list of the
//!   car's cell `+0x750` (the primary cell `FUN_0048fd20` of the
//!   post-integration centre, written at the end of the driver), skips a record
//!   whose byte `+0x2e` has the player's bit, and dispatches on the type byte:
//!   0 queues a co-driver call (while in control) and marks the record and its
//!   identical neighbours consumed (`FUN_00403ee0`); 1 writes the `.cat`
//!   cursor; 3 is replay-only; 4 and 5 do nothing; 6 sets the latch `+0x67c`;
//!   7..=0xb switch surfaces (`FUN_00403e70`); 0x15 arms the `+0x75c` timer;
//!   2 and every other type run the barrier response (`FUN_00401fd0`). It ends
//!   by copying `+0x6dc` to `+0x6e0`. The mechanics routine then snapshots the
//!   probes into `+0x15c` (`0x43b340`).
//! - The mode predicates: `FUN_00401000` is `+0x166 & 0x1c` in `{4, 0xc}`,
//!   `FUN_00401030` is `== 8` (both outside replays), the loop mode is
//!   `FUN_00485bb0`; the stage condition `DAT_00592f00` gates types 7..=0xb
//!   (`!= 8`) and 0x15 (`== 8`).

use cmr_formats::stage::TriggerVolume;

use crate::barrier_collision::{BarrierCar, BarrierScratch, barrier_end_posts, barrier_response};
use crate::car_reset::loop_reset_mode;
use crate::stage_triggers::{
    RuntimeTrigger, TriggerGeometryMode, TriggerStateError, current_points_intersect_open_y_span,
    trigger_geometry_contains,
};

/// The `.dat` records linked per cell, each list in walk order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TriggerCells {
    lists: Vec<Vec<usize>>,
}

/// A record names a cell outside the stage's cell table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TriggerCellOutOfRange {
    pub record: usize,
    pub cell: i32,
    pub cell_count: usize,
}

/// `FUN_004904e0`'s trigger linking.
///
/// # Errors
///
/// A record whose cell word is negative or not below `cell_count`.
pub fn link_trigger_cells(
    volumes: &[TriggerVolume],
    cell_count: usize,
) -> Result<TriggerCells, TriggerCellOutOfRange> {
    let mut lists = vec![Vec::new(); cell_count];
    for (record, volume) in volumes.iter().enumerate() {
        let cell = usize::try_from(volume.spatial_node_index)
            .ok()
            .filter(|&cell| cell < cell_count)
            .ok_or(TriggerCellOutOfRange {
                record,
                cell: volume.spatial_node_index,
                cell_count,
            })?;
        lists[cell].insert(0, record);
    }
    Ok(TriggerCells { lists })
}

impl TriggerCells {
    /// The records of one cell in walk order, or `None` for a cell outside
    /// the table (the native reads past it).
    #[must_use]
    pub fn cell(&self, cell: i16) -> Option<&[usize]> {
        usize::try_from(cell)
            .ok()
            .and_then(|cell| self.lists.get(cell))
            .map(Vec::as_slice)
    }
}

/// The global words the walk reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TriggerWalkConfig {
    /// The car's player index (`param_3`).
    pub player: u8,
    /// `DAT_00551ad8`, which replaces the mask player in the `FUN_00401030` mode.
    pub split_player_551ad8: u8,
    /// `DAT_00d3ca88`.
    pub replay: bool,
    /// `DAT_00d3ca80 +0x25/+0x166/+0x168`.
    pub config_25: u8,
    pub config_166: u8,
    pub config_168: u8,
    /// `DAT_00592f00`.
    pub condition_592f00: i32,
    /// `DAT_00551af0`.
    pub slow_motion: bool,
}

impl TriggerWalkConfig {
    /// Harness words for a single-player race: player 0, no replay, zero
    /// config bytes (unrecovered), and a stage condition other than 8 (the
    /// per-stage table `DAT_004f47e4` is not imported).
    #[must_use]
    pub const fn single_player_harness() -> Self {
        Self {
            player: 0,
            split_player_551ad8: 0,
            replay: false,
            config_25: 0,
            config_166: 0,
            config_168: 0,
            condition_592f00: 0,
            slow_motion: false,
        }
    }

    fn mode_401000(&self) -> bool {
        matches!(self.config_166 & 0x1c, 4 | 0xc) && !self.replay
    }

    fn mode_401030(&self) -> bool {
        self.config_166 & 0x1c == 8 && !self.replay
    }

    fn loop_mode(&self) -> bool {
        loop_reset_mode(self.config_166, self.config_168)
    }
}

/// What one record did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriggerEvent {
    /// `FUN_00444380(payload)`: a co-driver call.
    CodriverCall { record: usize, payload: i32 },
    /// `DAT_0057ae70[player] = payload`.
    NodeCursor { record: usize, payload: i32 },
    /// `+0x67c = 1`.
    ResetLatch { record: usize },
    /// `FUN_00403e70(index)`.
    SurfaceSwitch { record: usize, index: u8 },
    /// `+0x75c` armed with this many ticks.
    PenaltyTimer { record: usize, ticks: i16 },
    /// `FUN_00401fd0` touched the car; `impact` is set on a first-contact
    /// tick.
    Barrier {
        record: usize,
        kind: u8,
        impact: Option<crate::barrier_collision::BarrierImpact>,
    },
}

/// The walk's persistent car words besides the barrier car.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TriggerWalkState {
    /// `DAT_0050c574`, kept across calls.
    pub selected_probe_574: u8,
    /// `+0x6e0`: the last walk's `+0x6dc`.
    pub previous_contact_6e0: bool,
    /// `+0x75c`.
    pub penalty_timer_75c: i16,
}

/// `FUN_00403ee0`: mark the record and its identical neighbours (same type,
/// same end points on x/z) consumed for `player`.
fn consume_with_neighbours(
    triggers: &mut [RuntimeTrigger],
    record: usize,
    player: u8,
) -> Result<(), TriggerStateError> {
    let target = *triggers[record].volume();
    let same = |volume: &TriggerVolume| {
        volume.point_a[0] == target.point_a[0]
            && volume.point_a[2] == target.point_a[2]
            && volume.point_b[0] == target.point_b[0]
            && volume.point_b[2] == target.point_b[2]
    };
    let mut index = record;
    while index > 0 && triggers[index - 1].volume().raw_type == target.raw_type {
        if !same(triggers[index - 1].volume()) {
            break;
        }
        index -= 1;
        triggers[index].mark_consumed(player)?;
    }
    let mut index = record;
    while index + 1 < triggers.len() && triggers[index + 1].volume().raw_type == target.raw_type {
        if !same(triggers[index + 1].volume()) {
            break;
        }
        index += 1;
        triggers[index].mark_consumed(player)?;
    }
    triggers[record].mark_consumed(player)
}

/// `FUN_00403a00` over one cell. Returns the records' effects in walk order;
/// barrier pushes are already applied to `car`.
///
/// # Errors
///
/// A cell outside the table, or a player outside the consumed mask.
pub fn walk_triggers(
    triggers: &mut [RuntimeTrigger],
    cells: &TriggerCells,
    cell: i16,
    config: &TriggerWalkConfig,
    car: &mut BarrierCar,
    state: &mut TriggerWalkState,
) -> Result<Vec<TriggerEvent>, TriggerWalkError> {
    let records = cells
        .cell(cell)
        .ok_or(TriggerWalkError::CellOutOfRange(cell))?;
    let mask_player = if config.mode_401030() {
        config.split_player_551ad8
    } else {
        config.player
    };
    car.contact_6dc = false;
    let mut scratch = BarrierScratch {
        car_box: None,
        selected_probe_574: state.selected_probe_574,
    };
    let mut events = Vec::new();
    for &record in records {
        if !triggers[record].player_gate_open(mask_player)? {
            continue;
        }
        let volume = *triggers[record].volume();
        let contains =
            |car: &BarrierCar, mode| trigger_geometry_contains(&volume, &car.bounds, mode);
        match volume.raw_type {
            0 => {
                if (config.replay || config.config_25 & 8 == 0 || config.loop_mode())
                    && contains(car, TriggerGeometryMode::Bidirectional)
                {
                    if !car.out_of_control_680 {
                        events.push(TriggerEvent::CodriverCall {
                            record,
                            payload: volume.payload,
                        });
                    }
                    if config.replay
                        || !config.mode_401030()
                        || !config.loop_mode()
                        || config.player == 0
                    {
                        consume_with_neighbours(triggers, record, mask_player)?;
                    }
                }
            }
            1 => {
                if !config.replay
                    && !config.loop_mode()
                    && contains(car, TriggerGeometryMode::Bidirectional)
                {
                    events.push(TriggerEvent::NodeCursor {
                        record,
                        payload: volume.payload,
                    });
                }
            }
            3..=5 => {}
            6 => {
                if current_points_intersect_open_y_span(&volume, &car.bounds)
                    && contains(car, TriggerGeometryMode::Bidirectional)
                {
                    events.push(TriggerEvent::ResetLatch { record });
                }
            }
            7..=0x0b => {
                if (config.replay || !config.mode_401000())
                    && contains(car, TriggerGeometryMode::RequireSavedNonPositive)
                    && config.condition_592f00 != 8
                {
                    events.push(TriggerEvent::SurfaceSwitch {
                        record,
                        index: volume.raw_type - 7,
                    });
                }
            }
            0x15 => {
                if !config.replay
                    && !config.mode_401000()
                    && contains(car, TriggerGeometryMode::Bidirectional)
                    && state.penalty_timer_75c == 0
                    && config.condition_592f00 == 8
                {
                    let rate = i64::from(i32::from(config.slow_motion) + 1) * 0x19_0000;
                    let ticks = ((rate * 0xc_0000) >> 32) as i16;
                    state.penalty_timer_75c = ticks;
                    events.push(TriggerEvent::PenaltyTimer { record, ticks });
                }
            }
            raw_type => {
                if current_points_intersect_open_y_span(&volume, &car.bounds) {
                    if volume.event_flag != 0 {
                        barrier_end_posts(&volume, car, &mut scratch);
                    }
                    if barrier_response(&volume, car, &mut scratch) {
                        events.push(TriggerEvent::Barrier {
                            record,
                            kind: if raw_type == 2 { 0 } else { raw_type },
                            impact: car.impact.take(),
                        });
                    }
                }
            }
        }
    }
    state.selected_probe_574 = scratch.selected_probe_574;
    state.previous_contact_6e0 = car.contact_6dc;
    Ok(events)
}

/// The walk could not run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriggerWalkError {
    CellOutOfRange(i16),
    State(TriggerStateError),
}

impl From<TriggerStateError> for TriggerWalkError {
    fn from(error: TriggerStateError) -> Self {
        Self::State(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixed_point::{Fixed16, Vec3};
    use crate::stage_triggers::{TriggerCarBounds, initialize_runtime_triggers};

    const ONE: i32 = 0x1_0000;

    fn record(raw_type: u8, cell: i32, x: i32) -> TriggerVolume {
        TriggerVolume {
            point_a: [x, -10 * ONE, -50 * ONE],
            point_b: [x, 10 * ONE, 50 * ONE],
            normal_q16: [ONE, 0, 0],
            payload: 7,
            spatial_node_index: cell,
            raw_type,
            event_flag: 1,
            raw_state: 0,
            unknown_2f: 0,
        }
    }

    fn probes(center: [i32; 3]) -> [[i32; 3]; 8] {
        std::array::from_fn(|index| {
            let sx = if index & 1 == 0 { ONE } else { -ONE };
            let sy = if index & 2 == 0 { -ONE / 2 } else { ONE / 2 };
            let sz = if index & 4 == 0 { 2 * ONE } else { -2 * ONE };
            [center[0] + sx, center[1] + sy, center[2] + sz]
        })
    }

    fn vec(words: [i32; 3]) -> Vec3 {
        Vec3::new(
            Fixed16::from_raw(words[0]),
            Fixed16::from_raw(words[1]),
            Fixed16::from_raw(words[2]),
        )
    }

    /// A car crossing `x = 0` toward -x.
    fn crossing_car() -> BarrierCar {
        let center = [ONE / 2, 0, 0];
        let saved = [3 * ONE, 0, 0];
        BarrierCar {
            bounds: TriggerCarBounds {
                current_position: center,
                saved_position: saved,
                radius_q16: 3 * ONE,
                current_points: probes(center),
                prior_points: probes(saved),
            },
            reach_20: 2 * ONE,
            velocity: [-ONE / 2, 0, 0],
            basis: [vec([ONE, 0, 0]), vec([0, ONE, 0]), vec([0, 0, ONE])],
            half_words: [ONE, ONE / 2, 2 * ONE],
            impact_3f0: [0; 3],
            moment_3d8: Vec3::ZERO,
            out_of_control_680: false,
            recovery_688: false,
            contact_6dc: false,
            previous_contact_6e0: false,
            impact: None,
        }
    }

    #[test]
    fn linking_pushes_each_record_at_the_head_of_its_cell() {
        let volumes = [record(2, 1, 0), record(2, 0, 0), record(2, 1, 0)];
        let cells = link_trigger_cells(&volumes, 3).unwrap();
        assert_eq!(cells.cell(0), Some(&[1][..]));
        assert_eq!(cells.cell(1), Some(&[2, 0][..]));
        assert_eq!(cells.cell(2), Some(&[][..]));
        assert_eq!(cells.cell(3), None);
        assert_eq!(cells.cell(-1), None);
        assert_eq!(
            link_trigger_cells(&[record(2, 3, 0)], 3),
            Err(TriggerCellOutOfRange {
                record: 0,
                cell: 3,
                cell_count: 3
            })
        );
    }

    fn walk(
        volumes: &[TriggerVolume],
        car: &mut BarrierCar,
    ) -> (Vec<TriggerEvent>, Vec<RuntimeTrigger>, TriggerWalkState) {
        let cells = link_trigger_cells(volumes, 1).unwrap();
        let mut triggers = initialize_runtime_triggers(volumes);
        let mut state = TriggerWalkState::default();
        let events = walk_triggers(
            &mut triggers,
            &cells,
            0,
            &TriggerWalkConfig::single_player_harness(),
            car,
            &mut state,
        )
        .unwrap();
        (events, triggers, state)
    }

    #[test]
    fn a_barrier_record_pushes_the_car_and_sets_the_contact_words() {
        let mut car = crossing_car();
        let (events, _, state) = walk(&[record(2, 0, 0)], &mut car);
        let [
            TriggerEvent::Barrier {
                record: 0,
                kind: 0,
                impact: Some(impact),
            },
        ] = events.as_slice()
        else {
            panic!("one first-contact barrier event: {events:?}");
        };
        // The severity vector is the velocity change pushed out of the wall.
        assert!(impact.severity_vector[0] > 0);
        assert!(car.contact_6dc && state.previous_contact_6e0);
        // A second contact walk reports no impact.
        let mut again = crossing_car();
        again.previous_contact_6e0 = true;
        let (events, _, _) = walk(&[record(2, 0, 0)], &mut again);
        assert!(matches!(
            events.as_slice(),
            [TriggerEvent::Barrier { impact: None, .. }]
        ));
        assert!(car.velocity[0] > 0);
        // An unlisted type is a barrier of its own kind.
        let mut car = crossing_car();
        let (events, _, _) = walk(&[record(0x13, 0, 0)], &mut car);
        assert!(matches!(
            events.as_slice(),
            [TriggerEvent::Barrier {
                record: 0,
                kind: 0x13,
                ..
            }]
        ));
    }

    #[test]
    fn a_walk_without_contact_clears_the_contact_words() {
        let mut car = crossing_car();
        car.contact_6dc = true;
        let (events, _, state) = walk(&[record(2, 0, 40 * ONE)], &mut car);
        assert!(events.is_empty());
        assert!(!car.contact_6dc && !state.previous_contact_6e0);
    }

    #[test]
    fn crossing_records_report_their_effects_in_walk_order() {
        let volumes = [record(1, 0, 0), record(6, 0, 0), record(0, 0, 0)];
        let mut car = crossing_car();
        let (events, triggers, _) = walk(&volumes, &mut car);
        assert_eq!(
            events,
            [
                TriggerEvent::CodriverCall {
                    record: 2,
                    payload: 7
                },
                TriggerEvent::ResetLatch { record: 1 },
                TriggerEvent::NodeCursor {
                    record: 0,
                    payload: 7
                },
            ]
        );
        // The co-driver record is consumed for player 0 and skipped next time.
        assert_eq!(triggers[2].consumed_by(0), Ok(true));
        assert!(!triggers[2].player_gate_open(0).unwrap());
    }

    #[test]
    fn consuming_a_call_marks_identical_neighbours_only() {
        let mut volumes = [
            record(0, 0, 0),
            record(0, 0, 0),
            record(0, 0, 0),
            record(0, 0, 0),
        ];
        volumes[0].point_b[2] += 1;
        volumes[3].raw_type = 2;
        let mut triggers = initialize_runtime_triggers(&volumes);
        consume_with_neighbours(&mut triggers, 2, 0).unwrap();
        assert_eq!(triggers[0].consumed_by(0), Ok(false));
        assert_eq!(triggers[1].consumed_by(0), Ok(true));
        assert_eq!(triggers[2].consumed_by(0), Ok(true));
        assert_eq!(triggers[3].consumed_by(0), Ok(false));
    }

    #[test]
    fn an_out_of_control_car_does_not_queue_a_call_but_still_consumes_it() {
        let mut car = crossing_car();
        car.out_of_control_680 = true;
        let (events, triggers, _) = walk(&[record(0, 0, 0)], &mut car);
        assert!(events.is_empty());
        assert_eq!(triggers[0].consumed_by(0), Ok(true));
    }

    #[test]
    fn the_stage_condition_gates_surface_switches_and_the_penalty_timer() {
        let directional = TriggerVolume {
            event_flag: 1,
            ..record(9, 0, 0)
        };
        let mut car = crossing_car();
        let (events, _, _) = walk(&[directional], &mut car);
        assert_eq!(
            events,
            [TriggerEvent::SurfaceSwitch {
                record: 0,
                index: 2
            }]
        );
        // The penalty timer arms only under condition 8.
        let mut car = crossing_car();
        let (events, _, state) = walk(&[record(0x15, 0, 0)], &mut car);
        assert!(events.is_empty());
        assert_eq!(state.penalty_timer_75c, 0);
        let cells = link_trigger_cells(&[record(0x15, 0, 0)], 1).unwrap();
        let mut triggers = initialize_runtime_triggers(&[record(0x15, 0, 0)]);
        let mut state = TriggerWalkState::default();
        let config = TriggerWalkConfig {
            condition_592f00: 8,
            ..TriggerWalkConfig::single_player_harness()
        };
        let mut car = crossing_car();
        let events =
            walk_triggers(&mut triggers, &cells, 0, &config, &mut car, &mut state).unwrap();
        assert_eq!(
            events,
            [TriggerEvent::PenaltyTimer {
                record: 0,
                ticks: 300
            }]
        );
        assert_eq!(state.penalty_timer_75c, 300);
        // Already running: not re-armed.
        let mut car = crossing_car();
        state.penalty_timer_75c = 5;
        let events =
            walk_triggers(&mut triggers, &cells, 0, &config, &mut car, &mut state).unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn the_race_state_walk_pushes_latches_and_snapshots_the_probes() {
        use crate::compiled_car_initializer::{
            CompiledEightLaneExecutionState, CompiledLaneThresholdEffects,
            CompiledSetupPoseLaneInputs,
        };
        use crate::ground_pass::{GroundContact, rebuild_ground_probes};
        use crate::race_state::{CarState, RaceState};
        let ground = GroundContact {
            lanes: CompiledEightLaneExecutionState {
                handles: [0; 8],
                type_words: [0; 8],
                output_vectors: [[0; 3]; 8],
                lane_words: [0; 8],
                accumulator_words: [0; 8],
                latch_67c: 0,
            },
            lane_inputs: CompiledSetupPoseLaneInputs {
                gate_words: [0; 8],
                drive_words: [0; 8],
                first_four_subtractions: [0; 4],
            },
            half_words: [ONE, ONE / 2, 2 * ONE],
            deformation: [0x4000; 3],
            probes: [[0; 3]; 8],
            thresholds: CompiledLaneThresholdEffects {
                flags_73a: [0; 8],
                flag_743: 0,
                count_742: 0,
                indices_744: [0; 8],
            },
            wheel_reach_648_644: [0; 2],
            reach_20: 2 * ONE,
            radius_44: 3 * ONE,
        };
        let basis = [vec([ONE, 0, 0]), vec([0, ONE, 0]), vec([0, 0, ONE])];
        let undeformed = GroundContact {
            deformation: [0; 3],
            ..ground
        };
        let saved = vec([3 * ONE, 0, 0]);
        let mut car = CarState::at_rest();
        car.position = vec([ONE / 2, 0, 0]);
        car.velocity = vec([-ONE / 2, 0, 0]);
        car.motion.basis = basis;
        car.ground = Some(ground);
        car.four_wheel.saved_position_3e4 = saved;
        car.four_wheel.trigger_prior_15c = rebuild_ground_probes(&undeformed, basis, saved);
        let volumes = [record(2, 0, 0), record(6, 0, 0)];
        let mut triggers = initialize_runtime_triggers(&volumes);
        let cells = link_trigger_cells(&volumes, 1).unwrap();
        let mut race = RaceState::new(car);
        let events = race
            .run_trigger_walk(
                &mut triggers,
                &cells,
                &[Vec3::ZERO],
                &TriggerWalkConfig::single_player_harness(),
            )
            .unwrap();
        assert!(matches!(
            events.as_slice(),
            [
                TriggerEvent::ResetLatch { record: 1 },
                TriggerEvent::Barrier {
                    record: 0,
                    kind: 0,
                    ..
                },
            ]
        ));
        let after = race.car();
        assert_eq!(after.ground.unwrap().lanes.latch_67c, 1);
        assert!(after.position.x > car.position.x);
        assert!(after.velocity.x > Fixed16::ZERO);
        assert!(after.four_wheel.barrier_contact_6dc);
        assert!(after.four_wheel.trigger_walk.previous_contact_6e0);
        // The snapshot is the walked (undeformed, pushed) probes.
        assert_eq!(
            after.four_wheel.trigger_prior_15c,
            rebuild_ground_probes(&undeformed, basis, after.position)
        );
    }

    #[test]
    fn a_cell_outside_the_table_is_reported() {
        let cells = link_trigger_cells(&[], 1).unwrap();
        let mut state = TriggerWalkState::default();
        assert_eq!(
            walk_triggers(
                &mut [],
                &cells,
                4,
                &TriggerWalkConfig::single_player_harness(),
                &mut crossing_car(),
                &mut state,
            ),
            Err(TriggerWalkError::CellOutOfRange(4))
        );
    }
}
