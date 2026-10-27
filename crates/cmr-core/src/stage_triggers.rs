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
//! remain caller responsibilities. In particular, this module does not invent
//! a centre-point trigger test: the recovered geometry routines sample several
//! native car-bound points whose correspondence to the reconstructed
//! [`crate::race_state::CarState`] has not yet been proven.
//!
//! Static evidence (Ghidra pseudocode, not original source):
//! - loader reset: `ev_62b90e76060f94cdbb6eb452de97cb67e7f7b8f98f856b978cc57df3a30c8c54`;
//! - type-1 ordinal assignment: `ev_631f88315a111abb907ac74ceb2f1a565aae659483e2f51bd544a8beb42c3b30`;
//! - consumed-mask propagation: `ev_afaf198afe1d14266de5fc7f5635e0e2eea6f542e8a49ed755daace8e5af5a30`;
//! - event dispatch: `ev_e01ae13bfe6696b542ccfcfd54c06d8a56e4aa5cc2d58496a6eacc288dbc7853`.

use cmr_formats::stage::TriggerVolume;

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
            0 => TriggerDispatch::SplitCheckpoint { payload },
            1 => {
                let TriggerRuntimeState::Type1Ordinal(ordinal) = self.state else {
                    // `initialize_runtime_triggers` makes this unreachable. Keep
                    // classification total for values constructed inside tests
                    // or by future serializers without inventing an ordinal.
                    return TriggerDispatch::InvalidType1State;
                };
                TriggerDispatch::Finish { payload, ordinal }
            }
            2 => TriggerDispatch::Sound {
                event_code: 0,
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
            event_code => TriggerDispatch::Sound {
                event_code,
                payload,
                event_flag,
            },
        }
    }
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
    SplitCheckpoint {
        payload: i32,
    },
    Finish {
        payload: i32,
        ordinal: u8,
    },
    Sound {
        event_code: u8,
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
                TriggerDispatch::SplitCheckpoint { payload: 10 },
                TriggerDispatch::Finish {
                    payload: 11,
                    ordinal: 0,
                },
                TriggerDispatch::Sound {
                    event_code: 0,
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
                TriggerDispatch::Sound {
                    event_code: 0x13,
                    payload: 20,
                    event_flag: true,
                },
            ]
        );
    }
}
