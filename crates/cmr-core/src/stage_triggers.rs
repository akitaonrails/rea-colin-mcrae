//! Pure runtime contract for the stage `.dat` trigger table.
//!
//! The shipped loader resets byte `+0x2e` in every `0x30`-byte record, then
//! assigns ascending wrapping ordinals to type-1 records. During play the same
//! byte is a per-player consumed mask for the other event classes. Keeping that
//! mutable state separate from [`TriggerVolume`] preserves the source bytes and
//! avoids treating the unconsumed `+0x2f` byte as part of a `u16` mask.
//!
//! The event classification below is the side-effect-free portion of
//! `FUN_00403a00`. Its mode, race-state, geometry, and already-consumed gates
//! remain caller responsibilities. The recovered geometry contract uses the
//! native current and prior eight-point car bounds rather than substituting a
//! centre point.
//!
//! Static evidence (Ghidra pseudocode, not original source):
//! - loader reset: `ev_62b90e76060f94cdbb6eb452de97cb67e7f7b8f98f856b978cc57df3a30c8c54`;
//! - type-1 ordinal assignment: `ev_631f88315a111abb907ac74ceb2f1a565aae659483e2f51bd544a8beb42c3b30`;
//! - consumed-mask propagation: `ev_afaf198afe1d14266de5fc7f5635e0e2eea6f542e8a49ed755daace8e5af5a30`;
//! - event dispatch: `ev_e01ae13bfe6696b542ccfcfd54c06d8a56e4aa5cc2d58496a6eacc288dbc7853`.

use cmr_formats::stage::TriggerVolume;

#[cfg(test)]
const Q16_ONE: i32 = 0x1_0000;
const Q16_TWO: i32 = 0x2_0000;

/// Native selector passed to `FUN_00403980` by the trigger dispatcher.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TriggerGeometryMode {
    /// Native parameter zero: reject when the saved centre is on the positive
    /// side of the trigger plane.
    RequireSavedNonPositive,
    /// Native nonzero parameter: do not apply that directional precondition.
    Bidirectional,
}

/// Exact car-state inputs sampled by the native DAT geometry routines.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TriggerCarBounds {
    /// Physics state `+0xc8..+0xd0`.
    pub current_position: [i32; 3],
    /// Position snapshot at `+0x3e4..+0x3ec`, taken before the physics step.
    pub saved_position: [i32; 3],
    /// Physics state `+0x44`; native thickness is its Q16 product with 2.0.
    pub radius_q16: i32,
    /// Eight world-space points at `+0xfc..+0x158`.
    pub current_points: [[i32; 3]; 8],
    /// Previous trigger-query snapshot at `+0x15c..+0x1b8`.
    pub prior_points: [[i32; 3]; 8],
}

impl TriggerCarBounds {
    /// Reproduce the frame driver's post-query 24-dword copy.
    pub fn snapshot_current_points(&mut self) {
        self.prior_points = self.current_points;
    }
}

fn q16_mul(left: i32, right: i32) -> i32 {
    ((i64::from(left) * i64::from(right)) >> 16) as i32
}

fn dot_from_point(point: [i32; 3], probe: [i32; 3], normal: [i32; 3]) -> i32 {
    (0..3).fold(0_i32, |sum, component| {
        sum.wrapping_add(q16_mul(
            point[component].wrapping_sub(probe[component]),
            normal[component],
        ))
    })
}

fn inside_xz_segment(
    point_a: [i32; 3],
    point_b: [i32; 3],
    normal: [i32; 3],
    probe: [i32; 3],
) -> bool {
    let edge = |point: [i32; 3]| {
        q16_mul(normal[2].wrapping_neg(), point[0].wrapping_sub(probe[0]))
            .wrapping_add(q16_mul(normal[0], point[2].wrapping_sub(probe[2])))
    };
    let edge_a = edge(point_a);
    let edge_b = edge(point_b);
    edge_a == 0 || edge_b == 0 || edge_a.is_negative() != edge_b.is_negative()
}

/// `FUN_004035a0` (directional) / `FUN_00403470` (bidirectional): the car's
/// centre lies within `2 * +0x44` of the trigger plane, and for the directional
/// test the saved centre is not on the positive side. Returns the facing flag
/// `DAT_0050c524` (saved centre on the non-negative side) when it passes.
#[must_use]
pub fn trigger_plane_side(
    volume: &TriggerVolume,
    car: &TriggerCarBounds,
    mode: TriggerGeometryMode,
) -> Option<bool> {
    let saved_side = dot_from_point(volume.point_a, car.saved_position, volume.normal_q16);
    if mode == TriggerGeometryMode::RequireSavedNonPositive && saved_side > 0 {
        return None;
    }
    let current_center_side =
        dot_from_point(volume.point_a, car.current_position, volume.normal_q16);
    let thickness = q16_mul(car.radius_q16, Q16_TWO);
    if current_center_side.wrapping_abs() > thickness {
        return None;
    }
    Some(saved_side >= 0)
}

/// `FUN_004036e0`'s per-probe plane depths and the two probe lists.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProbePlaneLists {
    /// `DAT_0050c548[i]`: `(point_a - probe) . normal` for every probe.
    pub depths: [i32; 8],
    /// `DAT_0050c528`/`DAT_0050c529`: probes with negative / non-negative depth.
    pub negative_total: u8,
    pub nonnegative_total: u8,
    /// `DAT_0050c538` (count `DAT_0050c52a`): admitted probes with negative depth.
    pub negative: [u8; 8],
    pub negative_count: u8,
    /// `DAT_0050c540` (count `DAT_0050c52b`): admitted probes with non-negative depth.
    pub nonnegative: [u8; 8],
    pub nonnegative_count: u8,
}

/// `FUN_004036e0`: classify each current probe against the trigger plane. A
/// probe is admitted when it lies within the segment's x/z span and, on the
/// side the facing flag does not expect, only while it is advancing relative
/// to its prior position.
#[must_use]
pub fn probe_plane_lists(
    volume: &TriggerVolume,
    car: &TriggerCarBounds,
    facing: bool,
) -> ProbePlaneLists {
    let mut lists = ProbePlaneLists {
        depths: [0; 8],
        negative_total: 0,
        nonnegative_total: 0,
        negative: [0; 8],
        negative_count: 0,
        nonnegative: [0; 8],
        nonnegative_count: 0,
    };
    for index in 0..8 {
        let depth = dot_from_point(volume.point_a, car.current_points[index], volume.normal_q16);
        lists.depths[index] = depth;
        if depth < 0 {
            lists.negative_total += 1;
        } else {
            lists.nonnegative_total += 1;
        }
        if !inside_xz_segment(
            volume.point_a,
            volume.point_b,
            volume.normal_q16,
            car.current_points[index],
        ) {
            continue;
        }
        if (depth < 0) == facing {
            let prior = dot_from_point(volume.point_a, car.prior_points[index], volume.normal_q16);
            let advancing = if depth < 1 {
                depth >= 0 || depth <= prior
            } else {
                prior <= depth
            };
            if !advancing {
                continue;
            }
        }
        if depth < 0 {
            lists.negative[usize::from(lists.negative_count)] = index as u8;
            lists.negative_count += 1;
        } else {
            lists.nonnegative[usize::from(lists.nonnegative_count)] = index as u8;
            lists.nonnegative_count += 1;
        }
    }
    lists
}

/// Exact side/crossing predicate `FUN_00403980`: the plane-side test, then
/// the probe lists; true when both lists are non-empty, or when every probe
/// is on the negative side of a plane the car faced.
#[must_use]
pub fn trigger_geometry_contains(
    volume: &TriggerVolume,
    car: &TriggerCarBounds,
    mode: TriggerGeometryMode,
) -> bool {
    let Some(facing) = trigger_plane_side(volume, car, mode) else {
        return false;
    };
    let lists = probe_plane_lists(volume, car, facing);
    match (lists.negative_count, lists.nonnegative_count) {
        (0, 0) => false,
        (n, p) if n != 0 && p != 0 => true,
        _ => lists.negative_total == 8 && facing,
    }
}

/// Evaluate against the old snapshot, then perform the native post-query copy.
pub fn trigger_geometry_contains_then_snapshot(
    volume: &TriggerVolume,
    car: &mut TriggerCarBounds,
    mode: TriggerGeometryMode,
) -> bool {
    let result = trigger_geometry_contains(volume, car, mode);
    car.snapshot_current_points();
    result
}

/// `FUN_00403e00` succeeds for dispatch when at least one current probe lies
/// strictly between the record's two Y endpoints. Equality is outside.
#[must_use]
pub fn current_points_intersect_open_y_span(
    volume: &TriggerVolume,
    car: &TriggerCarBounds,
) -> bool {
    let low = volume.point_a[1].min(volume.point_b[1]);
    let high = volume.point_a[1].max(volume.point_b[1]);
    car.current_points
        .iter()
        .any(|point| point[1] > low && point[1] < high)
}

/// Number of player bits available in the native one-byte consumed mask.
pub const TRIGGER_CONSUMED_PLAYER_CAPACITY: u8 = u8::BITS as u8;

/// Mutable meaning assigned to a trigger's native byte at `+0x2e` after load.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TriggerRuntimeState {
    /// Per-player fire-once bits for every trigger type except type 1.
    ConsumedMask(u8),
    /// Ascending, wrapping ordinal assigned to a type-1 record.
    Type1Ordinal(u8),
}

/// One immutable parsed record paired with its separately initialized state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimeTrigger {
    volume: TriggerVolume,
    state: TriggerRuntimeState,
}

impl RuntimeTrigger {
    /// The immutable record decoded from the user-owned `.dat` payload.
    #[must_use]
    pub const fn volume(&self) -> &TriggerVolume {
        &self.volume
    }

    /// Current mutable interpretation of native byte `+0x2e`.
    #[must_use]
    pub const fn state(&self) -> TriggerRuntimeState {
        self.state
    }

    /// The byte value read by the dispatcher's outer per-player bit gate.
    ///
    /// Type-1 records are intentionally included: the native dispatcher tests
    /// the ordinal byte before entering its type switch, even though that byte
    /// is not a consumable mask for type 1.
    #[must_use]
    pub const fn runtime_byte(&self) -> u8 {
        match self.state {
            TriggerRuntimeState::ConsumedMask(value) | TriggerRuntimeState::Type1Ordinal(value) => {
                value
            }
        }
    }

    /// Whether the native outer bit gate permits this record for `player`.
    pub fn player_gate_open(&self, player: u8) -> Result<bool, TriggerStateError> {
        Ok(self.runtime_byte() & player_bit(player)? == 0)
    }

    /// Whether a consumable trigger has already fired for `player`.
    pub fn consumed_by(&self, player: u8) -> Result<bool, TriggerStateError> {
        let bit = player_bit(player)?;
        match self.state {
            TriggerRuntimeState::ConsumedMask(mask) => Ok(mask & bit != 0),
            TriggerRuntimeState::Type1Ordinal(_) => Err(TriggerStateError::Type1NotConsumable),
        }
    }

    /// Mark a consumable trigger as fired for `player`, preserving other bits.
    pub fn mark_consumed(&mut self, player: u8) -> Result<(), TriggerStateError> {
        let bit = player_bit(player)?;
        match &mut self.state {
            TriggerRuntimeState::ConsumedMask(mask) => {
                *mask |= bit;
                Ok(())
            }
            TriggerRuntimeState::Type1Ordinal(_) => Err(TriggerStateError::Type1NotConsumable),
        }
    }

    /// Classify the record without applying geometry, mode, or race-state side effects.
    #[must_use]
    pub const fn dispatch(&self) -> TriggerDispatch {
        let payload = self.volume.payload;
        let event_flag = self.volume.event_flag != 0;
        match self.volume.raw_type {
            0 => TriggerDispatch::CodriverCall { payload },
            1 => {
                let TriggerRuntimeState::Type1Ordinal(ordinal) = self.state else {
                    // `initialize_runtime_triggers` makes this unreachable. Keep
                    // classification total for values constructed inside tests
                    // or by future serializers without inventing an ordinal.
                    return TriggerDispatch::InvalidType1State;
                };
                TriggerDispatch::NodeCursor { payload, ordinal }
            }
            2 => TriggerDispatch::Barrier {
                kind: 0,
                payload,
                event_flag,
            },
            3 => TriggerDispatch::RallySchool { payload },
            4 | 5 => TriggerDispatch::Inert,
            6 => TriggerDispatch::Kill,
            7..=0x0b => TriggerDispatch::SurfaceSwitch {
                index: self.volume.raw_type - 7,
            },
            0x15 => TriggerDispatch::OutOfBounds,
            kind => TriggerDispatch::Barrier {
                kind,
                payload,
                event_flag,
            },
        }
    }

    /// Geometry family selected by the native type switch before side effects.
    #[must_use]
    pub const fn spatial_gate(&self) -> TriggerSpatialGate {
        match self.volume.raw_type {
            0 | 1 | 3 | 0x15 => TriggerSpatialGate::BidirectionalPlane,
            2 => TriggerSpatialGate::OpenYSpan,
            4 | 5 => TriggerSpatialGate::Inert,
            6 => TriggerSpatialGate::OpenYSpanThenBidirectionalPlane,
            7..=0x0b => TriggerSpatialGate::DirectionalPlane,
            _ => TriggerSpatialGate::OpenYSpan,
        }
    }
}

/// Recovered spatial predicate selected by `FUN_00403a00` for a DAT type.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TriggerSpatialGate {
    /// Types 4/5 perform no work.
    Inert,
    /// `FUN_00403e00`: any current probe strictly inside the record's Y span.
    OpenYSpan,
    /// `FUN_00403980(1)`: plane/segment test without the saved-side prefilter.
    BidirectionalPlane,
    /// Type 6: open-Y success followed by `FUN_00403980(1)`.
    OpenYSpanThenBidirectionalPlane,
    /// Types 7..=0x0b: `FUN_00403980(0)` with the saved-side prefilter.
    DirectionalPlane,
}

impl TriggerSpatialGate {
    #[must_use]
    pub fn matches(self, volume: &TriggerVolume, car: &TriggerCarBounds) -> bool {
        match self {
            Self::Inert => false,
            Self::OpenYSpan => current_points_intersect_open_y_span(volume, car),
            Self::BidirectionalPlane => {
                trigger_geometry_contains(volume, car, TriggerGeometryMode::Bidirectional)
            }
            Self::OpenYSpanThenBidirectionalPlane => {
                current_points_intersect_open_y_span(volume, car)
                    && trigger_geometry_contains(volume, car, TriggerGeometryMode::Bidirectional)
            }
            Self::DirectionalPlane => {
                trigger_geometry_contains(volume, car, TriggerGeometryMode::RequireSavedNonPositive)
            }
        }
    }
}

/// A record that passed the native outer player-bit and spatial gates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TriggerSpatialCandidate {
    pub record_index: usize,
    pub dispatch: TriggerDispatch,
}

/// Evaluate every record against one shared prior-probe snapshot, then perform
/// the native post-dispatch copy exactly once.
///
/// Front-end/race-mode predicates and event side effects remain outside this
/// pure candidate pass. The result therefore says which records reached those
/// gates, not that every returned event fired.
pub fn spatial_candidates_then_snapshot(
    triggers: &[RuntimeTrigger],
    car: &mut TriggerCarBounds,
    player: u8,
) -> Result<Vec<TriggerSpatialCandidate>, TriggerStateError> {
    let mut candidates = Vec::new();
    for (record_index, trigger) in triggers.iter().enumerate() {
        if trigger.player_gate_open(player)?
            && trigger.spatial_gate().matches(trigger.volume(), car)
        {
            candidates.push(TriggerSpatialCandidate {
                record_index,
                dispatch: trigger.dispatch(),
            });
        }
    }
    car.snapshot_current_points();
    Ok(candidates)
}

/// Failure to access the runtime meaning of native byte `+0x2e`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TriggerStateError {
    /// Only eight player bits fit in the recovered byte.
    PlayerOutOfRange(u8),
    /// Type-1 records use the byte as an ordinal rather than a consumed mask.
    Type1NotConsumable,
}

fn player_bit(player: u8) -> Result<u8, TriggerStateError> {
    if player >= TRIGGER_CONSUMED_PLAYER_CAPACITY {
        return Err(TriggerStateError::PlayerOutOfRange(player));
    }
    Ok(1_u8 << player)
}

/// Side-effect-free event class selected by the trigger dispatcher.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TriggerDispatch {
    /// Type 0: a co-driver call; `FUN_00444380` queues the payload's note id.
    CodriverCall {
        payload: i32,
    },
    /// Type 1: writes the `.cat` cursor `DAT_0057ae70[player] = payload`.
    NodeCursor {
        payload: i32,
        ordinal: u8,
    },
    /// Type 2 and every unlisted type: a barrier wall, `FUN_00401fd0(kind)`
    /// with `kind` 0 for type 2 and the type itself otherwise.
    Barrier {
        kind: u8,
        payload: i32,
        event_flag: bool,
    },
    RallySchool {
        payload: i32,
    },
    Inert,
    Kill,
    SurfaceSwitch {
        index: u8,
    },
    OutOfBounds,
    /// Defensive result for a type-1 record paired with the wrong state kind.
    InvalidType1State,
}

/// Initialize runtime state without mutating the parsed records.
///
/// Type-1 ordinals use native byte wrapping. Every other record starts with an
/// empty consumed mask, irrespective of the two raw tail bytes preserved by the
/// format decoder.
#[must_use]
pub fn initialize_runtime_triggers(volumes: &[TriggerVolume]) -> Vec<RuntimeTrigger> {
    let mut next_type1_ordinal = 0_u8;
    volumes
        .iter()
        .copied()
        .map(|volume| {
            let state = if volume.raw_type == 1 {
                let ordinal = next_type1_ordinal;
                next_type1_ordinal = next_type1_ordinal.wrapping_add(1);
                TriggerRuntimeState::Type1Ordinal(ordinal)
            } else {
                TriggerRuntimeState::ConsumedMask(0)
            };
            RuntimeTrigger { volume, state }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn volume(raw_type: u8, payload: i32) -> TriggerVolume {
        TriggerVolume {
            point_a: [1, 2, 3],
            point_b: [4, 5, 6],
            normal_q16: [0x10000, 0, 0],
            payload,
            spatial_node_index: 9,
            raw_type,
            event_flag: 1,
            raw_state: 0xf7,
            unknown_2f: 0xbf,
        }
    }

    #[test]
    fn initialization_ignores_raw_tail_and_preserves_the_parsed_record() {
        let parsed = [volume(0, 10), volume(1, 20), volume(6, 30), volume(1, 40)];
        let runtime = initialize_runtime_triggers(&parsed);

        assert_eq!(runtime[0].state(), TriggerRuntimeState::ConsumedMask(0));
        assert_eq!(runtime[1].state(), TriggerRuntimeState::Type1Ordinal(0));
        assert_eq!(runtime[2].state(), TriggerRuntimeState::ConsumedMask(0));
        assert_eq!(runtime[3].state(), TriggerRuntimeState::Type1Ordinal(1));
        assert_eq!(runtime[0].volume(), &parsed[0]);
        assert_eq!(parsed[0].raw_state, 0xf7);
        assert_eq!(parsed[0].unknown_2f, 0xbf);
    }

    #[test]
    fn type1_ordinals_wrap_like_the_native_byte() {
        let parsed = vec![volume(1, 0); 257];
        let runtime = initialize_runtime_triggers(&parsed);
        assert_eq!(runtime[255].state(), TriggerRuntimeState::Type1Ordinal(255));
        assert_eq!(runtime[256].state(), TriggerRuntimeState::Type1Ordinal(0));
    }

    #[test]
    fn consumed_mask_accepts_boundary_players_and_rejects_player_eight() {
        let mut trigger = initialize_runtime_triggers(&[volume(0, 0)])[0];
        trigger.mark_consumed(0).unwrap();
        trigger.mark_consumed(7).unwrap();
        assert!(trigger.consumed_by(0).unwrap());
        assert!(!trigger.consumed_by(1).unwrap());
        assert!(trigger.consumed_by(7).unwrap());
        assert_eq!(
            trigger.state(),
            TriggerRuntimeState::ConsumedMask(0b1000_0001)
        );
        assert_eq!(
            trigger.mark_consumed(8),
            Err(TriggerStateError::PlayerOutOfRange(8))
        );
    }

    #[test]
    fn type1_ordinal_cannot_be_misused_as_a_consumed_mask() {
        let mut trigger = initialize_runtime_triggers(&[volume(1, 0)])[0];
        assert_eq!(
            trigger.consumed_by(0),
            Err(TriggerStateError::Type1NotConsumable)
        );
        assert_eq!(
            trigger.mark_consumed(0),
            Err(TriggerStateError::Type1NotConsumable)
        );
    }

    #[test]
    fn dispatch_classifies_every_recovered_event_family() {
        let parsed = [
            volume(0, 10),
            volume(1, 11),
            volume(2, 12),
            volume(3, 13),
            volume(4, 14),
            volume(5, 15),
            volume(6, 16),
            volume(7, 17),
            volume(0x0b, 18),
            volume(0x15, 19),
            volume(0x13, 20),
        ];
        let runtime = initialize_runtime_triggers(&parsed);
        assert_eq!(
            runtime
                .iter()
                .map(RuntimeTrigger::dispatch)
                .collect::<Vec<_>>(),
            vec![
                TriggerDispatch::CodriverCall { payload: 10 },
                TriggerDispatch::NodeCursor {
                    payload: 11,
                    ordinal: 0,
                },
                TriggerDispatch::Barrier {
                    kind: 0,
                    payload: 12,
                    event_flag: true,
                },
                TriggerDispatch::RallySchool { payload: 13 },
                TriggerDispatch::Inert,
                TriggerDispatch::Inert,
                TriggerDispatch::Kill,
                TriggerDispatch::SurfaceSwitch { index: 0 },
                TriggerDispatch::SurfaceSwitch { index: 4 },
                TriggerDispatch::OutOfBounds,
                TriggerDispatch::Barrier {
                    kind: 0x13,
                    payload: 20,
                    event_flag: true,
                },
            ]
        );
    }

    #[test]
    fn spatial_gate_mapping_matches_the_dispatcher_type_switch() {
        let parsed = [
            volume(0, 0),
            volume(1, 0),
            volume(2, 0),
            volume(3, 0),
            volume(4, 0),
            volume(5, 0),
            volume(6, 0),
            volume(7, 0),
            volume(0x0b, 0),
            volume(0x15, 0),
            volume(0x13, 0),
        ];
        let runtime = initialize_runtime_triggers(&parsed);
        assert_eq!(
            runtime
                .iter()
                .map(RuntimeTrigger::spatial_gate)
                .collect::<Vec<_>>(),
            vec![
                TriggerSpatialGate::BidirectionalPlane,
                TriggerSpatialGate::BidirectionalPlane,
                TriggerSpatialGate::OpenYSpan,
                TriggerSpatialGate::BidirectionalPlane,
                TriggerSpatialGate::Inert,
                TriggerSpatialGate::Inert,
                TriggerSpatialGate::OpenYSpanThenBidirectionalPlane,
                TriggerSpatialGate::DirectionalPlane,
                TriggerSpatialGate::DirectionalPlane,
                TriggerSpatialGate::BidirectionalPlane,
                TriggerSpatialGate::OpenYSpan,
            ]
        );
    }

    #[test]
    fn outer_player_gate_reads_type1_ordinal_as_the_native_byte() {
        let runtime = initialize_runtime_triggers(&[volume(1, 0), volume(1, 0)]);
        assert_eq!(runtime[0].runtime_byte(), 0);
        assert_eq!(runtime[1].runtime_byte(), 1);
        assert!(runtime[0].player_gate_open(0).unwrap());
        assert!(!runtime[1].player_gate_open(0).unwrap());
        assert!(runtime[1].player_gate_open(1).unwrap());
        assert_eq!(
            runtime[0].player_gate_open(8),
            Err(TriggerStateError::PlayerOutOfRange(8))
        );
    }

    fn geometry_volume() -> TriggerVolume {
        TriggerVolume {
            point_a: [0, 0, 0],
            point_b: [0, 0, 10 * Q16_ONE],
            normal_q16: [Q16_ONE, 0, 0],
            ..volume(0, 0)
        }
    }

    fn point(x: i32, y: i32, z: i32) -> [i32; 3] {
        [x, y, z]
    }

    fn crossing_bounds() -> TriggerCarBounds {
        TriggerCarBounds {
            current_position: [0; 3],
            saved_position: [-Q16_ONE, 0, 0],
            radius_q16: Q16_ONE,
            current_points: [point(Q16_ONE, 0, 5 * Q16_ONE); 8],
            prior_points: [point(-Q16_ONE, 0, 5 * Q16_ONE); 8],
        }
    }

    #[test]
    fn candidate_pass_shares_prior_points_and_snapshots_once_after_the_table() {
        let plane = geometry_volume();
        let open_y = TriggerVolume {
            point_a: [0, -Q16_ONE, 0],
            point_b: [0, Q16_ONE, 10 * Q16_ONE],
            raw_type: 2,
            ..geometry_volume()
        };
        let inert = TriggerVolume {
            raw_type: 4,
            ..geometry_volume()
        };
        let runtime = initialize_runtime_triggers(&[plane, inert, open_y]);
        let mut car = crossing_bounds();
        let prior = car.prior_points;

        let candidates = spatial_candidates_then_snapshot(&runtime, &mut car, 0).unwrap();

        assert_eq!(
            candidates,
            vec![
                TriggerSpatialCandidate {
                    record_index: 0,
                    dispatch: TriggerDispatch::CodriverCall { payload: 0 },
                },
                TriggerSpatialCandidate {
                    record_index: 2,
                    dispatch: TriggerDispatch::Barrier {
                        kind: 0,
                        payload: 0,
                        event_flag: true,
                    },
                },
            ]
        );
        assert_ne!(prior, car.current_points);
        assert_eq!(car.prior_points, car.current_points);
    }

    #[test]
    fn geometry_detects_whole_hull_crossing_and_straddling() {
        let volume = geometry_volume();
        let mut car = crossing_bounds();
        assert!(trigger_geometry_contains(
            &volume,
            &car,
            TriggerGeometryMode::Bidirectional
        ));

        for point in &mut car.current_points[4..] {
            point[0] = -Q16_ONE;
        }
        assert!(trigger_geometry_contains(
            &volume,
            &car,
            TriggerGeometryMode::Bidirectional
        ));
    }

    #[test]
    fn geometry_preserves_direction_thickness_and_segment_boundaries() {
        let volume = geometry_volume();
        let mut car = crossing_bounds();
        assert!(!trigger_geometry_contains(
            &volume,
            &car,
            TriggerGeometryMode::RequireSavedNonPositive
        ));

        car.current_position[0] = 2 * Q16_ONE;
        assert!(trigger_geometry_contains(
            &volume,
            &car,
            TriggerGeometryMode::Bidirectional
        ));
        car.current_position[0] += 1;
        assert!(!trigger_geometry_contains(
            &volume,
            &car,
            TriggerGeometryMode::Bidirectional
        ));

        car.current_position = [0; 3];
        for point in &mut car.current_points {
            point[2] = -1;
        }
        assert!(!trigger_geometry_contains(
            &volume,
            &car,
            TriggerGeometryMode::Bidirectional
        ));
        for point in &mut car.current_points {
            point[2] = 0;
        }
        assert!(trigger_geometry_contains(
            &volume,
            &car,
            TriggerGeometryMode::Bidirectional
        ));
    }

    #[test]
    fn geometry_uses_prior_points_before_the_post_query_snapshot() {
        let volume = geometry_volume();
        let mut car = crossing_bounds();
        car.prior_points = [point(2 * Q16_ONE, 0, 5 * Q16_ONE); 8];

        assert!(!trigger_geometry_contains_then_snapshot(
            &volume,
            &mut car,
            TriggerGeometryMode::Bidirectional
        ));
        assert_eq!(car.prior_points, car.current_points);
        assert!(trigger_geometry_contains(
            &volume,
            &car,
            TriggerGeometryMode::Bidirectional
        ));
    }

    #[test]
    fn geometry_keeps_native_wrapping_abs_minimum_boundary() {
        let volume = geometry_volume();
        let mut car = crossing_bounds();
        car.current_position[0] = i32::MIN;
        car.radius_q16 = 0;
        assert!(trigger_geometry_contains(
            &volume,
            &car,
            TriggerGeometryMode::Bidirectional
        ));
    }

    #[test]
    fn open_y_span_excludes_endpoints_in_either_endpoint_order() {
        let mut volume = geometry_volume();
        volume.point_a[1] = -Q16_ONE;
        volume.point_b[1] = Q16_ONE;
        let mut car = crossing_bounds();
        assert!(current_points_intersect_open_y_span(&volume, &car));
        for point in &mut car.current_points {
            point[1] = Q16_ONE;
        }
        assert!(!current_points_intersect_open_y_span(&volume, &car));
        std::mem::swap(&mut volume.point_a[1], &mut volume.point_b[1]);
        car.current_points[3][1] = 0;
        assert!(current_points_intersect_open_y_span(&volume, &car));
    }
}
