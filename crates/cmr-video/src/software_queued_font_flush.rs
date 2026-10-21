//! Backend-neutral flush planning for the bounded queued-font compatibility
//! subset.
//!
//! Current championship glyph production has zero rotation and no viewport
//! override. This module preserves recovered lane/depth/terminal ordering for
//! that subset, but deliberately stops before quad geometry, render states,
//! filtering, blending, fallback blitting, and pixels.

use crate::{
    software_queued_font_argb4444::{
        SoftwareQueuedFontArgb4444Pool, SoftwareQueuedFontArgb4444Source,
    },
    software_queued_font_glyphs::SoftwareQueuedFontTerminal,
    software_queued_font_queue::{
        RoutedSoftwareQueuedFontGlyph, SoftwareQueuedFontLane, SoftwareQueuedFontLaneQueues,
    },
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SoftwareQueuedFontFlushTerminal {
    TexturedQuad,
    FallbackBlitter,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SoftwareQueuedFontFlushCommand {
    pub lane: SoftwareQueuedFontLane,
    pub slot: usize,
    pub record: RoutedSoftwareQueuedFontGlyph,
    depth_bits: u32,
    pub terminal: SoftwareQueuedFontFlushTerminal,
}

impl SoftwareQueuedFontFlushCommand {
    #[must_use]
    pub const fn depth(self) -> f32 {
        f32::from_bits(self.depth_bits)
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct SoftwareQueuedFontFlushPlan<'pool> {
    commands: Vec<SoftwareQueuedFontFlushCommand>,
    dropped: [usize; 3],
    pool: &'pool SoftwareQueuedFontArgb4444Pool,
}

impl<'pool> SoftwareQueuedFontFlushPlan<'pool> {
    #[must_use]
    pub fn from_queues(
        queues: &SoftwareQueuedFontLaneQueues,
        pool: &'pool SoftwareQueuedFontArgb4444Pool,
    ) -> Self {
        let mut commands = Vec::new();
        for lane in SoftwareQueuedFontLane::FLUSH_ORDER {
            commands.extend(queues.records(lane).iter().copied().enumerate().map(
                |(slot, record)| SoftwareQueuedFontFlushCommand {
                    lane,
                    slot,
                    record,
                    depth_bits: lane_depth(lane).to_bits(),
                    terminal: match record.producer.terminal {
                        SoftwareQueuedFontTerminal::TexturedQuad => {
                            SoftwareQueuedFontFlushTerminal::TexturedQuad
                        }
                        SoftwareQueuedFontTerminal::FallbackBlitter => {
                            SoftwareQueuedFontFlushTerminal::FallbackBlitter
                        }
                    },
                },
            ));
        }
        Self {
            commands,
            dropped: [
                queues.dropped_len(SoftwareQueuedFontLane::One),
                queues.dropped_len(SoftwareQueuedFontLane::Two),
                queues.dropped_len(SoftwareQueuedFontLane::Three),
            ],
            pool,
        }
    }

    #[must_use]
    pub fn commands(&self) -> &[SoftwareQueuedFontFlushCommand] {
        &self.commands
    }

    #[must_use]
    pub const fn dropped_len(&self, lane: SoftwareQueuedFontLane) -> usize {
        self.dropped[lane as usize - 1]
    }

    #[must_use]
    pub fn source_owner(
        &self,
        command: &SoftwareQueuedFontFlushCommand,
    ) -> &'pool SoftwareQueuedFontArgb4444Source {
        command.record.producer.source_owner(self.pool)
    }
}

#[must_use]
pub const fn lane_depth(lane: SoftwareQueuedFontLane) -> f32 {
    match lane {
        SoftwareQueuedFontLane::One => 0.0,
        SoftwareQueuedFontLane::Two => 0.5,
        SoftwareQueuedFontLane::Three => 0.999_999,
    }
}

/// A backend boundary for the recovered terminal split. `preflight` must not
/// produce rendering effects; implementations should validate every command
/// before the first submission.
pub trait SoftwareQueuedFontFlushSink {
    type Error;

    fn preflight(
        &mut self,
        command: &SoftwareQueuedFontFlushCommand,
        source: &SoftwareQueuedFontArgb4444Source,
    ) -> Result<(), Self::Error>;

    fn submit_textured_quad(
        &mut self,
        command: SoftwareQueuedFontFlushCommand,
        source: &SoftwareQueuedFontArgb4444Source,
    );

    fn submit_fallback_blitter(
        &mut self,
        command: SoftwareQueuedFontFlushCommand,
        source: &SoftwareQueuedFontArgb4444Source,
    );
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SoftwareQueuedFontFlushPhase {
    Preflight,
}

#[derive(Debug, Eq, PartialEq)]
pub struct SoftwareQueuedFontFlushError<E> {
    pub phase: SoftwareQueuedFontFlushPhase,
    pub lane: SoftwareQueuedFontLane,
    pub slot: usize,
    pub source: E,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SoftwareQueuedFontFlushOutcome {
    pub submitted: usize,
}

pub fn execute_software_queued_font_flush_plan<S: SoftwareQueuedFontFlushSink>(
    plan: SoftwareQueuedFontFlushPlan<'_>,
    sink: &mut S,
) -> Result<SoftwareQueuedFontFlushOutcome, SoftwareQueuedFontFlushError<S::Error>> {
    for command in &plan.commands {
        sink.preflight(command, plan.source_owner(command))
            .map_err(|source| SoftwareQueuedFontFlushError {
                phase: SoftwareQueuedFontFlushPhase::Preflight,
                lane: command.lane,
                slot: command.slot,
                source,
            })?;
    }

    let mut completed = 0;
    for command in plan.commands {
        let source = command.record.producer.source_owner(plan.pool);
        match command.terminal {
            SoftwareQueuedFontFlushTerminal::TexturedQuad => {
                sink.submit_textured_quad(command, source);
            }
            SoftwareQueuedFontFlushTerminal::FallbackBlitter => {
                sink.submit_fallback_blitter(command, source);
            }
        }
        completed += 1;
    }
    Ok(SoftwareQueuedFontFlushOutcome {
        submitted: completed,
    })
}

#[cfg(test)]
mod tests {
    use cmr_core::font_assets::{FontSet, FontStyle};
    use cmr_formats::font::{FontLimits, RectI16, RectI32, parse_legacy_font};

    use super::*;
    use crate::{
        software_queued_font_argb4444::prepare_software_queued_font_argb4444_pool,
        software_queued_font_glyphs::LaneNeutralQueuedFontGlyph,
        software_queued_font_queue::{
            SOFTWARE_QUEUED_FONT_LANE_CAPACITY, SoftwareQueuedFontByte3Policy,
            SoftwareQueuedFontPushOutcome,
        },
    };

    #[test]
    fn plan_preserves_lane_order_fifo_exact_depth_and_terminal() {
        let pool = pool();
        let mut queues = SoftwareQueuedFontLaneQueues::new();
        push(&mut queues, SoftwareQueuedFontLane::One, 10, false);
        push(&mut queues, SoftwareQueuedFontLane::Three, 30, true);
        push(&mut queues, SoftwareQueuedFontLane::Two, 20, false);
        push(&mut queues, SoftwareQueuedFontLane::Three, 31, false);
        push(&mut queues, SoftwareQueuedFontLane::One, 11, true);

        let lane_three_before = queues.records(SoftwareQueuedFontLane::Three).to_vec();
        let plan = SoftwareQueuedFontFlushPlan::from_queues(&queues, &pool);
        assert_eq!(
            plan.commands()
                .iter()
                .map(|command| (command.lane, command.slot, command.record.producer.code))
                .collect::<Vec<_>>(),
            [
                (SoftwareQueuedFontLane::Three, 0, 30),
                (SoftwareQueuedFontLane::Three, 1, 31),
                (SoftwareQueuedFontLane::Two, 0, 20),
                (SoftwareQueuedFontLane::One, 0, 10),
                (SoftwareQueuedFontLane::One, 1, 11),
            ]
        );
        assert_eq!(plan.commands()[0].depth().to_bits(), 0x3f7fffef);
        assert_eq!(plan.commands()[2].depth().to_bits(), 0x3f000000);
        assert_eq!(plan.commands()[3].depth().to_bits(), 0x00000000);
        assert_eq!(
            plan.commands()[0].terminal,
            SoftwareQueuedFontFlushTerminal::FallbackBlitter
        );
        assert_eq!(
            plan.commands()[1].terminal,
            SoftwareQueuedFontFlushTerminal::TexturedQuad
        );
        assert_eq!(
            queues.records(SoftwareQueuedFontLane::Three),
            lane_three_before
        );
        assert_eq!(
            plan.source_owner(&plan.commands()[0]).style(),
            FontStyle::Title
        );
    }

    #[test]
    fn preflights_every_command_before_dispatching_terminal_paths() {
        let pool = pool();
        let mut queues = SoftwareQueuedFontLaneQueues::new();
        push(&mut queues, SoftwareQueuedFontLane::One, 10, true);
        push(&mut queues, SoftwareQueuedFontLane::Three, 30, false);
        push(&mut queues, SoftwareQueuedFontLane::Two, 20, true);
        let plan = SoftwareQueuedFontFlushPlan::from_queues(&queues, &pool);
        let mut sink = RecordingSink::default();

        let outcome = execute_software_queued_font_flush_plan(plan, &mut sink).unwrap();
        assert_eq!(outcome.submitted, 3);
        assert_eq!(sink.events, ["p30", "p20", "p10", "t30", "f20", "f10"]);
    }

    #[test]
    fn preflight_failure_has_no_submission_and_reports_exact_command() {
        let pool = pool();
        let mut queues = SoftwareQueuedFontLaneQueues::new();
        push(&mut queues, SoftwareQueuedFontLane::Three, 30, false);
        push(&mut queues, SoftwareQueuedFontLane::Two, 20, false);

        let mut preflight = RecordingSink {
            fail_preflight: Some(20),
            ..RecordingSink::default()
        };
        let error = execute_software_queued_font_flush_plan(
            SoftwareQueuedFontFlushPlan::from_queues(&queues, &pool),
            &mut preflight,
        )
        .unwrap_err();
        assert_eq!(error.phase, SoftwareQueuedFontFlushPhase::Preflight);
        assert_eq!(error.lane, SoftwareQueuedFontLane::Two);
        assert_eq!(error.slot, 0);
        assert_eq!(error.source, 20);
        assert_eq!(preflight.events, ["p30", "p20"]);
    }

    #[test]
    fn plan_retains_capacity_drop_metadata_without_emitting_the_drop() {
        let pool = pool();
        let mut queues = SoftwareQueuedFontLaneQueues::new();
        for slot in 0..SOFTWARE_QUEUED_FONT_LANE_CAPACITY {
            push(
                &mut queues,
                SoftwareQueuedFontLane::Two,
                (slot % 251) as u8,
                false,
            );
        }
        assert_eq!(
            queues.push(
                SoftwareQueuedFontLane::Two,
                SoftwareQueuedFontByte3Policy::ForceFf,
                glyph(252, true),
            ),
            SoftwareQueuedFontPushOutcome::DroppedCapacity
        );
        let accepted = queues.records(SoftwareQueuedFontLane::Two).to_vec();
        let plan = SoftwareQueuedFontFlushPlan::from_queues(&queues, &pool);
        assert_eq!(plan.commands().len(), SOFTWARE_QUEUED_FONT_LANE_CAPACITY);
        assert_eq!(plan.dropped_len(SoftwareQueuedFontLane::Two), 1);
        assert_eq!(queues.records(SoftwareQueuedFontLane::Two), accepted);
        assert!(
            plan.commands()
                .iter()
                .all(|command| command.record.producer.code != 252)
        );
    }

    #[derive(Default)]
    struct RecordingSink {
        events: Vec<String>,
        fail_preflight: Option<u8>,
    }

    impl SoftwareQueuedFontFlushSink for RecordingSink {
        type Error = u8;

        fn preflight(
            &mut self,
            command: &SoftwareQueuedFontFlushCommand,
            source: &SoftwareQueuedFontArgb4444Source,
        ) -> Result<(), Self::Error> {
            assert_eq!(source.style(), command.record.producer.style);
            let code = command.record.producer.code;
            self.events.push(format!("p{code}"));
            (self.fail_preflight != Some(code))
                .then_some(())
                .ok_or(code)
        }

        fn submit_textured_quad(
            &mut self,
            command: SoftwareQueuedFontFlushCommand,
            source: &SoftwareQueuedFontArgb4444Source,
        ) {
            self.submit("t", command, source);
        }

        fn submit_fallback_blitter(
            &mut self,
            command: SoftwareQueuedFontFlushCommand,
            source: &SoftwareQueuedFontArgb4444Source,
        ) {
            self.submit("f", command, source);
        }
    }

    impl RecordingSink {
        fn submit(
            &mut self,
            prefix: &str,
            command: SoftwareQueuedFontFlushCommand,
            source: &SoftwareQueuedFontArgb4444Source,
        ) {
            assert_eq!(source.style(), command.record.producer.style);
            let code = command.record.producer.code;
            self.events.push(format!("{prefix}{code}"));
        }
    }

    fn push(
        queues: &mut SoftwareQueuedFontLaneQueues,
        lane: SoftwareQueuedFontLane,
        code: u8,
        fallback: bool,
    ) {
        assert!(matches!(
            queues.push(
                lane,
                SoftwareQueuedFontByte3Policy::PreserveCaller,
                glyph(code, fallback),
            ),
            SoftwareQueuedFontPushOutcome::Accepted { .. }
        ));
    }

    fn glyph(code: u8, fallback: bool) -> LaneNeutralQueuedFontGlyph {
        LaneNeutralQueuedFontGlyph {
            command_index: usize::from(code),
            blit_index: usize::from(code),
            code,
            glyph_index: code,
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
            caller_tuple: [1, 2, 3, 4],
            terminal: if fallback {
                SoftwareQueuedFontTerminal::FallbackBlitter
            } else {
                SoftwareQueuedFontTerminal::TexturedQuad
            },
        }
    }

    fn pool() -> SoftwareQueuedFontArgb4444Pool {
        let bytes = font_file();
        let fonts = FontSet::from_ordered_fonts(std::array::from_fn(|_| {
            parse_legacy_font(&bytes, FontLimits::default()).unwrap()
        }));
        prepare_software_queued_font_argb4444_pool(fonts).unwrap()
    }

    fn font_file() -> Vec<u8> {
        const PIXEL_OFFSET: usize = 0x436;
        const FONT_HEADER_SIZE: usize = 24;
        const GLYPH_SIZE: usize = 16;
        let trailer = PIXEL_OFFSET + 4;
        let mut bytes = vec![0; trailer + FONT_HEADER_SIZE + GLYPH_SIZE];
        bytes[0..2].copy_from_slice(b"BM");
        bytes[18..22].copy_from_slice(&1i32.to_le_bytes());
        bytes[22..26].copy_from_slice(&1i32.to_le_bytes());
        bytes[28..30].copy_from_slice(&8u16.to_le_bytes());
        for index in 0..256usize {
            let offset = 14 + 40 + index * 4;
            bytes[offset..offset + 4].copy_from_slice(&[index as u8, index as u8, index as u8, 0]);
        }
        bytes[PIXEL_OFFSET] = 1;
        bytes[trailer + 1] = 1;
        bytes[trailer + 4..trailer + 8].copy_from_slice(&1u32.to_le_bytes());
        bytes[trailer + 8..trailer + 12].copy_from_slice(&1u32.to_le_bytes());
        let glyph = trailer + FONT_HEADER_SIZE;
        bytes[glyph] = b'A';
        bytes[glyph + 6..glyph + 8].copy_from_slice(&1i16.to_le_bytes());
        bytes[glyph + 8..glyph + 10].copy_from_slice(&1i16.to_le_bytes());
        bytes
    }
}
