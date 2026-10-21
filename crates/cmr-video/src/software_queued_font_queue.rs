//! Bounded semantic queue routing for the recovered alternate-font producer.
//!
//! This preserves the exact three independent lane capacities, FIFO order, and
//! byte-3 gate without claiming the native 52-byte record ABI or executing a
//! flush. Lane selection remains caller-explicit.

use crate::software_queued_font_glyphs::{
    LaneNeutralChampionshipQueuedText, LaneNeutralQueuedFontGlyph,
};

pub const SOFTWARE_QUEUED_FONT_LANE_CAPACITY: usize = 0x7ff;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum SoftwareQueuedFontLane {
    One = 1,
    Two = 2,
    Three = 3,
}

impl SoftwareQueuedFontLane {
    pub const FLUSH_ORDER: [Self; 3] = [Self::Three, Self::Two, Self::One];

    const fn index(self) -> usize {
        self as usize - 1
    }
}

/// Resolved result of the producer's recovered byte-3 runtime gate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SoftwareQueuedFontByte3Policy {
    PreserveCaller,
    ForceFf,
}

impl SoftwareQueuedFontByte3Policy {
    /// Applies the recovered renderer-mode setter coercion and then resolves
    /// the producer gate from the resulting mode and capability bit.
    #[must_use]
    pub const fn from_configured_gate(
        configured_renderer_mode: u8,
        alpha_texture_capability: bool,
    ) -> Self {
        Self::from_recovered_gate(
            recovered_effective_queued_font_renderer_mode(configured_renderer_mode),
            alpha_texture_capability,
        )
    }

    /// Reproduces `FUN_004aa040`: preserve only when effective mode is not 7
    /// and the recovered alpha-texture capability bit is present.
    #[must_use]
    pub const fn from_recovered_gate(
        effective_renderer_mode: u8,
        alpha_texture_capability: bool,
    ) -> Self {
        if effective_renderer_mode != 7 && alpha_texture_capability {
            Self::PreserveCaller
        } else {
            Self::ForceFf
        }
    }

    #[must_use]
    pub const fn apply(self, mut tuple: [u8; 4]) -> [u8; 4] {
        if matches!(self, Self::ForceFf) {
            tuple[3] = 0xff;
        }
        tuple
    }
}

/// The renderer-mode setter retains values `0..=7` and coerces larger config
/// bytes to mode 6 before the byte-3 gate reads the global.
#[must_use]
pub const fn recovered_effective_queued_font_renderer_mode(configured: u8) -> u8 {
    if configured <= 7 { configured } else { 6 }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RoutedSoftwareQueuedFontGlyph {
    pub producer: LaneNeutralQueuedFontGlyph,
    pub stored_tuple: [u8; 4],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SoftwareQueuedFontPushOutcome {
    Accepted { slot: usize },
    DroppedCapacity,
}

#[derive(Debug, Default, Eq, PartialEq)]
pub struct SoftwareQueuedFontLaneQueues {
    lanes: [Vec<RoutedSoftwareQueuedFontGlyph>; 3],
    dropped: [usize; 3],
}

impl SoftwareQueuedFontLaneQueues {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            lanes: [Vec::new(), Vec::new(), Vec::new()],
            dropped: [0; 3],
        }
    }

    pub fn push(
        &mut self,
        lane: SoftwareQueuedFontLane,
        policy: SoftwareQueuedFontByte3Policy,
        producer: LaneNeutralQueuedFontGlyph,
    ) -> SoftwareQueuedFontPushOutcome {
        let index = lane.index();
        let records = &mut self.lanes[index];
        if records.len() >= SOFTWARE_QUEUED_FONT_LANE_CAPACITY {
            self.dropped[index] += 1;
            return SoftwareQueuedFontPushOutcome::DroppedCapacity;
        }
        let slot = records.len();
        records.push(RoutedSoftwareQueuedFontGlyph {
            stored_tuple: policy.apply(producer.caller_tuple),
            producer,
        });
        SoftwareQueuedFontPushOutcome::Accepted { slot }
    }

    #[must_use]
    pub fn records(&self, lane: SoftwareQueuedFontLane) -> &[RoutedSoftwareQueuedFontGlyph] {
        &self.lanes[lane.index()]
    }

    #[must_use]
    pub fn dropped_len(&self, lane: SoftwareQueuedFontLane) -> usize {
        self.dropped[lane.index()]
    }
}

/// One complete non-deferred setup-text transcript plus its recovered bounded
/// semantic route, starting from empty lanes.
#[derive(Debug, Eq, PartialEq)]
pub struct RoutedChampionshipQueuedText {
    source: LaneNeutralChampionshipQueuedText,
    queues: SoftwareQueuedFontLaneQueues,
    lane: SoftwareQueuedFontLane,
    byte3_policy: SoftwareQueuedFontByte3Policy,
}

impl RoutedChampionshipQueuedText {
    #[must_use]
    pub const fn source(&self) -> &LaneNeutralChampionshipQueuedText {
        &self.source
    }

    #[must_use]
    pub const fn queues(&self) -> &SoftwareQueuedFontLaneQueues {
        &self.queues
    }

    #[must_use]
    pub const fn lane(&self) -> SoftwareQueuedFontLane {
        self.lane
    }

    #[must_use]
    pub const fn byte3_policy(&self) -> SoftwareQueuedFontByte3Policy {
        self.byte3_policy
    }
}

#[must_use]
pub fn route_lane_neutral_championship_queued_text(
    source: LaneNeutralChampionshipQueuedText,
    lane: SoftwareQueuedFontLane,
    byte3_policy: SoftwareQueuedFontByte3Policy,
) -> RoutedChampionshipQueuedText {
    let mut queues = SoftwareQueuedFontLaneQueues::new();
    for record in source.records().copied() {
        let _ = queues.push(lane, byte3_policy, record);
    }
    RoutedChampionshipQueuedText {
        source,
        queues,
        lane,
        byte3_policy,
    }
}

#[cfg(test)]
mod tests {
    use cmr_core::font_assets::FontStyle;
    use cmr_formats::font::{RectI16, RectI32};

    use super::*;
    use crate::software_queued_font_glyphs::SoftwareQueuedFontTerminal;

    #[test]
    fn recovered_mode_coercion_and_byte3_gate_are_exact() {
        for mode in 0..=6 {
            assert_eq!(
                SoftwareQueuedFontByte3Policy::from_recovered_gate(mode, true),
                SoftwareQueuedFontByte3Policy::PreserveCaller
            );
        }
        assert_eq!(
            SoftwareQueuedFontByte3Policy::from_recovered_gate(7, true),
            SoftwareQueuedFontByte3Policy::ForceFf
        );
        assert_eq!(
            SoftwareQueuedFontByte3Policy::from_recovered_gate(2, false),
            SoftwareQueuedFontByte3Policy::ForceFf
        );
        assert_eq!(recovered_effective_queued_font_renderer_mode(7), 7);
        assert_eq!(recovered_effective_queued_font_renderer_mode(8), 6);
        assert_eq!(recovered_effective_queued_font_renderer_mode(u8::MAX), 6);
        assert_eq!(
            SoftwareQueuedFontByte3Policy::from_configured_gate(8, true),
            SoftwareQueuedFontByte3Policy::PreserveCaller
        );
        assert_eq!(
            SoftwareQueuedFontByte3Policy::from_configured_gate(8, false),
            SoftwareQueuedFontByte3Policy::ForceFf
        );

        for caller_byte3 in [0, 0x7e, 0xff] {
            let tuple = [1, 2, 3, caller_byte3];
            assert_eq!(
                SoftwareQueuedFontByte3Policy::PreserveCaller.apply(tuple),
                tuple
            );
            assert_eq!(
                SoftwareQueuedFontByte3Policy::ForceFf.apply(tuple),
                [1, 2, 3, 0xff]
            );
        }
    }

    #[test]
    fn routes_three_independent_fifo_lanes_and_rewrites_each_record() {
        let mut queues = SoftwareQueuedFontLaneQueues::new();
        for (lane, marker, policy) in [
            (
                SoftwareQueuedFontLane::One,
                10,
                SoftwareQueuedFontByte3Policy::PreserveCaller,
            ),
            (
                SoftwareQueuedFontLane::Three,
                30,
                SoftwareQueuedFontByte3Policy::ForceFf,
            ),
            (
                SoftwareQueuedFontLane::Two,
                20,
                SoftwareQueuedFontByte3Policy::PreserveCaller,
            ),
            (
                SoftwareQueuedFontLane::Three,
                31,
                SoftwareQueuedFontByte3Policy::PreserveCaller,
            ),
            (
                SoftwareQueuedFontLane::One,
                11,
                SoftwareQueuedFontByte3Policy::ForceFf,
            ),
        ] {
            let slot = queues.records(lane).len();
            assert_eq!(
                queues.push(lane, policy, glyph(marker)),
                SoftwareQueuedFontPushOutcome::Accepted { slot }
            );
        }

        assert_eq!(codes(queues.records(SoftwareQueuedFontLane::One)), [10, 11]);
        assert_eq!(codes(queues.records(SoftwareQueuedFontLane::Two)), [20]);
        assert_eq!(
            codes(queues.records(SoftwareQueuedFontLane::Three)),
            [30, 31]
        );
        assert_eq!(
            queues.records(SoftwareQueuedFontLane::Three)[0].stored_tuple,
            [1, 2, 3, 0xff]
        );
        assert_eq!(
            queues.records(SoftwareQueuedFontLane::Three)[1].stored_tuple,
            [1, 2, 3, 31]
        );
        assert_eq!(
            SoftwareQueuedFontLane::FLUSH_ORDER,
            [
                SoftwareQueuedFontLane::Three,
                SoftwareQueuedFontLane::Two,
                SoftwareQueuedFontLane::One,
            ]
        );
    }

    #[test]
    fn each_lane_accepts_exactly_2047_then_silently_drops() {
        let mut queues = SoftwareQueuedFontLaneQueues::new();
        for lane in [
            SoftwareQueuedFontLane::One,
            SoftwareQueuedFontLane::Two,
            SoftwareQueuedFontLane::Three,
        ] {
            for slot in 0..SOFTWARE_QUEUED_FONT_LANE_CAPACITY {
                assert_eq!(
                    queues.push(
                        lane,
                        SoftwareQueuedFontByte3Policy::PreserveCaller,
                        glyph((slot % 251) as u8),
                    ),
                    SoftwareQueuedFontPushOutcome::Accepted { slot }
                );
            }
            let last = *queues.records(lane).last().unwrap();
            assert_eq!(
                queues.push(lane, SoftwareQueuedFontByte3Policy::ForceFf, glyph(252),),
                SoftwareQueuedFontPushOutcome::DroppedCapacity
            );
            assert_eq!(
                queues.records(lane).len(),
                SOFTWARE_QUEUED_FONT_LANE_CAPACITY
            );
            assert_eq!(*queues.records(lane).last().unwrap(), last);
            assert_eq!(queues.dropped_len(lane), 1);
        }
    }

    fn codes(records: &[RoutedSoftwareQueuedFontGlyph]) -> Vec<u8> {
        records.iter().map(|record| record.producer.code).collect()
    }

    fn glyph(marker: u8) -> LaneNeutralQueuedFontGlyph {
        LaneNeutralQueuedFontGlyph {
            command_index: usize::from(marker),
            blit_index: usize::from(marker),
            code: marker,
            glyph_index: marker,
            style: FontStyle::Title,
            source: RectI16 {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
            destination: RectI32 {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
            caller_tuple: [1, 2, 3, marker],
            terminal: SoftwareQueuedFontTerminal::TexturedQuad,
        }
    }
}
