//! Pure vertex preparation for the recovered queued-font textured-quad
//! subset.
//!
//! Championship text currently supplies zero rotation, no viewport override,
//! and terminal values `8` or `0x48`. Consequently this contract prepares
//! only the unrotated textured terminal and does not invent the native
//! record's optional UV-swap or render-state flags.

use cmr_core::font_assets::FontStyle;
use cmr_formats::font::{RectI16, RectI32};

use crate::{
    software_queued_font_argb4444::SoftwareQueuedFontArgb4444Source,
    software_queued_font_flush::{SoftwareQueuedFontFlushCommand, SoftwareQueuedFontFlushTerminal},
};

pub const SOFTWARE_QUEUED_FONT_SPECULAR: u32 = 0xff00_0000;
const MODE_ZERO_HIGH_UV_ADJUSTMENT: f32 = 0.001;

/// One recovered transformed/lit queued-font vertex, represented semantically
/// rather than as a native Direct3D ABI structure.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SoftwareQueuedFontQuadVertex {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub rhw: f32,
    pub diffuse: u32,
    pub specular: u32,
    pub u: f32,
    pub v: f32,
}

/// Four vertices in the recovered triangle-strip order: top-left, top-right,
/// bottom-left, bottom-right.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SoftwareQueuedFontQuad {
    pub vertices: [SoftwareQueuedFontQuadVertex; 4],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SoftwareQueuedFontQuadError {
    UnsupportedTerminal(SoftwareQueuedFontFlushTerminal),
    SourceStyleMismatch {
        record: FontStyle,
        owner: FontStyle,
    },
    InvalidSourceExtent([u32; 2]),
    InvalidSourceRectangle {
        rectangle: RectI16,
        extent: [u32; 2],
    },
    InvalidDestinationRectangle(RectI32),
    DestinationOverflow(RectI32),
}

impl std::fmt::Display for SoftwareQueuedFontQuadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "queued-font quad preparation failed: {self:?}")
    }
}

impl std::error::Error for SoftwareQueuedFontQuadError {}

/// Validates source ownership and containment without requiring the textured
/// terminal. The fallback blitter shares these source-memory safety rules.
pub fn validate_software_queued_font_source(
    command: &SoftwareQueuedFontFlushCommand,
    source: &SoftwareQueuedFontArgb4444Source,
) -> Result<[u32; 2], SoftwareQueuedFontQuadError> {
    let record_style = command.record.producer.style;
    if source.style() != record_style {
        return Err(SoftwareQueuedFontQuadError::SourceStyleMismatch {
            record: record_style,
            owner: source.style(),
        });
    }

    let extent = source.aligned_surface_extent();
    if extent[0] == 0 || extent[1] == 0 {
        return Err(SoftwareQueuedFontQuadError::InvalidSourceExtent(extent));
    }

    let source_rectangle = command.record.producer.source;
    let source_x = i32::from(source_rectangle.x);
    let source_y = i32::from(source_rectangle.y);
    let source_width = i32::from(source_rectangle.width);
    let source_height = i32::from(source_rectangle.height);
    let Some(source_right) = source_x.checked_add(source_width) else {
        return Err(SoftwareQueuedFontQuadError::InvalidSourceRectangle {
            rectangle: source_rectangle,
            extent,
        });
    };
    let Some(source_bottom) = source_y.checked_add(source_height) else {
        return Err(SoftwareQueuedFontQuadError::InvalidSourceRectangle {
            rectangle: source_rectangle,
            extent,
        });
    };
    if source_x < 0
        || source_y < 0
        || source_width <= 0
        || source_height <= 0
        || u32::try_from(source_right).map_or(true, |right| right > extent[0])
        || u32::try_from(source_bottom).map_or(true, |bottom| bottom > extent[1])
    {
        return Err(SoftwareQueuedFontQuadError::InvalidSourceRectangle {
            rectangle: source_rectangle,
            extent,
        });
    }
    Ok(extent)
}

/// Builds the exact zero-rotation/no-viewport vertex inputs for one textured
/// queued-font command.
///
/// `effective_renderer_mode` is the already-resolved legacy renderer mode.
/// Only exact mode zero expands the high U and V edges by `0.001f`; low edges
/// are never adjusted. Source geometry is validated against the owned aligned
/// surface before division, making this pure preparation safe for later GPU or
/// software backends.
pub fn prepare_software_queued_font_quad(
    command: &SoftwareQueuedFontFlushCommand,
    source: &SoftwareQueuedFontArgb4444Source,
    effective_renderer_mode: u8,
) -> Result<SoftwareQueuedFontQuad, SoftwareQueuedFontQuadError> {
    if command.terminal != SoftwareQueuedFontFlushTerminal::TexturedQuad {
        return Err(SoftwareQueuedFontQuadError::UnsupportedTerminal(
            command.terminal,
        ));
    }

    let extent = validate_software_queued_font_source(command, source)?;
    let source_rectangle = command.record.producer.source;
    let source_x = i32::from(source_rectangle.x);
    let source_y = i32::from(source_rectangle.y);
    let source_width = i32::from(source_rectangle.width);
    let source_height = i32::from(source_rectangle.height);
    let source_right = source_x + source_width;
    let source_bottom = source_y + source_height;

    let destination = command.record.producer.destination;
    if destination.width <= 0 || destination.height <= 0 {
        return Err(SoftwareQueuedFontQuadError::InvalidDestinationRectangle(
            destination,
        ));
    }
    let destination_right = destination.x.checked_add(destination.width).ok_or(
        SoftwareQueuedFontQuadError::DestinationOverflow(destination),
    )?;
    let destination_bottom = destination.y.checked_add(destination.height).ok_or(
        SoftwareQueuedFontQuadError::DestinationOverflow(destination),
    )?;

    let width = extent[0] as f32;
    let height = extent[1] as f32;
    let low_u = source_x as f32 / width;
    let low_v = source_y as f32 / height;
    let adjustment = if effective_renderer_mode == 0 {
        MODE_ZERO_HIGH_UV_ADJUSTMENT
    } else {
        0.0
    };
    let high_u = source_right as f32 / width + adjustment;
    let high_v = source_bottom as f32 / height + adjustment;

    let [byte_0, byte_1, byte_2, byte_3] = command.record.stored_tuple;
    let diffuse = u32::from_be_bytes([byte_3, byte_0, byte_1, byte_2]);
    let vertex = |x, y, u, v| SoftwareQueuedFontQuadVertex {
        x,
        y,
        z: command.depth(),
        rhw: 1.0,
        diffuse,
        specular: SOFTWARE_QUEUED_FONT_SPECULAR,
        u,
        v,
    };

    Ok(SoftwareQueuedFontQuad {
        vertices: [
            vertex(destination.x as f32, destination.y as f32, low_u, low_v),
            vertex(
                destination_right as f32,
                destination.y as f32,
                high_u,
                low_v,
            ),
            vertex(
                destination.x as f32,
                destination_bottom as f32,
                low_u,
                high_v,
            ),
            vertex(
                destination_right as f32,
                destination_bottom as f32,
                high_u,
                high_v,
            ),
        ],
    })
}

#[cfg(test)]
mod tests {
    use cmr_core::font_assets::{FontSet, FontStyle};
    use cmr_formats::font::{FontLimits, parse_legacy_font};

    use super::*;
    use crate::{
        software_queued_font_argb4444::{
            SoftwareQueuedFontArgb4444Pool, prepare_software_queued_font_argb4444_pool,
        },
        software_queued_font_glyphs::{LaneNeutralQueuedFontGlyph, SoftwareQueuedFontTerminal},
        software_queued_font_queue::{
            SoftwareQueuedFontByte3Policy, SoftwareQueuedFontLane, SoftwareQueuedFontLaneQueues,
            SoftwareQueuedFontPushOutcome,
        },
    };

    #[test]
    fn prepares_exact_strip_corners_colors_and_mode_zero_uvs() {
        let pool = pool();
        let mut queues = SoftwareQueuedFontLaneQueues::new();
        push(
            &mut queues,
            SoftwareQueuedFontLane::Three,
            SoftwareQueuedFontByte3Policy::PreserveCaller,
            glyph(
                FontStyle::Title,
                SoftwareQueuedFontTerminal::TexturedQuad,
                RectI16 {
                    x: 2,
                    y: 1,
                    width: 4,
                    height: 2,
                },
                RectI32 {
                    x: -4,
                    y: 7,
                    width: 8,
                    height: 9,
                },
            ),
        );
        let plan = crate::software_queued_font_flush::SoftwareQueuedFontFlushPlan::from_queues(
            &queues, &pool,
        );
        let command = &plan.commands()[0];

        let quad =
            prepare_software_queued_font_quad(command, plan.source_owner(command), 0).unwrap();

        assert_eq!(command.depth().to_bits(), 0x3f7f_ffef);
        assert_eq!(
            quad.vertices.map(|vertex| (vertex.x, vertex.y)),
            [(-4.0, 7.0), (4.0, 7.0), (-4.0, 16.0), (4.0, 16.0)]
        );
        assert_eq!(
            quad.vertices
                .map(|vertex| (vertex.u.to_bits(), vertex.v.to_bits())),
            [
                (0.25_f32.to_bits(), 0.25_f32.to_bits()),
                (0.751_f32.to_bits(), 0.25_f32.to_bits()),
                (0.25_f32.to_bits(), 0.751_f32.to_bits()),
                (0.751_f32.to_bits(), 0.751_f32.to_bits()),
            ]
        );
        for vertex in quad.vertices {
            assert_eq!(vertex.z.to_bits(), 0x3f7f_ffef);
            assert_eq!(vertex.rhw, 1.0);
            assert_eq!(vertex.diffuse, 0x0401_0203);
            assert_eq!(vertex.specular, SOFTWARE_QUEUED_FONT_SPECULAR);
            assert_eq!(vertex.specular, 0xff00_0000);
        }
    }

    #[test]
    fn nonzero_mode_does_not_adjust_either_high_uv_edge() {
        let pool = pool();
        let mut queues = SoftwareQueuedFontLaneQueues::new();
        push(
            &mut queues,
            SoftwareQueuedFontLane::Two,
            SoftwareQueuedFontByte3Policy::PreserveCaller,
            glyph(
                FontStyle::Title,
                SoftwareQueuedFontTerminal::TexturedQuad,
                RectI16 {
                    x: 2,
                    y: 1,
                    width: 4,
                    height: 2,
                },
                RectI32 {
                    x: 10,
                    y: 20,
                    width: 4,
                    height: 2,
                },
            ),
        );
        let plan = crate::software_queued_font_flush::SoftwareQueuedFontFlushPlan::from_queues(
            &queues, &pool,
        );
        let command = &plan.commands()[0];

        let quad =
            prepare_software_queued_font_quad(command, plan.source_owner(command), 1).unwrap();

        assert_eq!(quad.vertices[0].u.to_bits(), 0.25_f32.to_bits());
        assert_eq!(quad.vertices[0].v.to_bits(), 0.25_f32.to_bits());
        assert_eq!(quad.vertices[3].u.to_bits(), 0.75_f32.to_bits());
        assert_eq!(quad.vertices[3].v.to_bits(), 0.75_f32.to_bits());
        assert_eq!(quad.vertices[3].z.to_bits(), 0.5_f32.to_bits());
    }

    #[test]
    fn forced_byte_three_is_the_diffuse_high_byte() {
        let pool = pool();
        let mut queues = SoftwareQueuedFontLaneQueues::new();
        push(
            &mut queues,
            SoftwareQueuedFontLane::One,
            SoftwareQueuedFontByte3Policy::ForceFf,
            glyph(
                FontStyle::Title,
                SoftwareQueuedFontTerminal::TexturedQuad,
                RectI16 {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1,
                },
                RectI32 {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1,
                },
            ),
        );
        let plan = crate::software_queued_font_flush::SoftwareQueuedFontFlushPlan::from_queues(
            &queues, &pool,
        );
        let command = &plan.commands()[0];

        let quad =
            prepare_software_queued_font_quad(command, plan.source_owner(command), 7).unwrap();

        assert!(
            quad.vertices
                .iter()
                .all(|vertex| vertex.diffuse == 0xff01_0203)
        );
        assert!(quad.vertices.iter().all(|vertex| vertex.z == 0.0));
    }

    #[test]
    fn uv_normalization_uses_aligned_prepared_surface_extent() {
        let pool = pool_with_extent(5, 4);
        assert_eq!(pool.source(FontStyle::Title).dib_extent(), [5, 4]);
        assert_eq!(
            pool.source(FontStyle::Title).aligned_surface_extent(),
            [8, 4]
        );
        let mut queues = SoftwareQueuedFontLaneQueues::new();
        push(
            &mut queues,
            SoftwareQueuedFontLane::Two,
            SoftwareQueuedFontByte3Policy::PreserveCaller,
            glyph(
                FontStyle::Title,
                SoftwareQueuedFontTerminal::TexturedQuad,
                RectI16 {
                    x: 6,
                    y: 0,
                    width: 2,
                    height: 1,
                },
                RectI32 {
                    x: 0,
                    y: 0,
                    width: 2,
                    height: 1,
                },
            ),
        );
        let plan = crate::software_queued_font_flush::SoftwareQueuedFontFlushPlan::from_queues(
            &queues, &pool,
        );
        let command = &plan.commands()[0];

        let quad =
            prepare_software_queued_font_quad(command, plan.source_owner(command), 1).unwrap();

        assert_eq!(quad.vertices[0].u.to_bits(), 0.75_f32.to_bits());
        assert_eq!(quad.vertices[1].u.to_bits(), 1.0_f32.to_bits());
    }

    #[test]
    fn rejects_fallback_and_style_mismatch_before_geometry() {
        let pool = pool();
        let mut queues = SoftwareQueuedFontLaneQueues::new();
        push(
            &mut queues,
            SoftwareQueuedFontLane::Two,
            SoftwareQueuedFontByte3Policy::PreserveCaller,
            glyph(
                FontStyle::Title,
                SoftwareQueuedFontTerminal::FallbackBlitter,
                RectI16 {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1,
                },
                RectI32 {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1,
                },
            ),
        );
        let plan = crate::software_queued_font_flush::SoftwareQueuedFontFlushPlan::from_queues(
            &queues, &pool,
        );
        let fallback = &plan.commands()[0];
        assert_eq!(
            prepare_software_queued_font_quad(fallback, plan.source_owner(fallback), 0),
            Err(SoftwareQueuedFontQuadError::UnsupportedTerminal(
                SoftwareQueuedFontFlushTerminal::FallbackBlitter
            ))
        );

        let mut queues = SoftwareQueuedFontLaneQueues::new();
        push(
            &mut queues,
            SoftwareQueuedFontLane::Two,
            SoftwareQueuedFontByte3Policy::PreserveCaller,
            glyph(
                FontStyle::Title,
                SoftwareQueuedFontTerminal::TexturedQuad,
                RectI16 {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1,
                },
                RectI32 {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1,
                },
            ),
        );
        let plan = crate::software_queued_font_flush::SoftwareQueuedFontFlushPlan::from_queues(
            &queues, &pool,
        );
        let command = &plan.commands()[0];
        assert_eq!(
            prepare_software_queued_font_quad(command, pool.source(FontStyle::Options), 0),
            Err(SoftwareQueuedFontQuadError::SourceStyleMismatch {
                record: FontStyle::Title,
                owner: FontStyle::Options,
            })
        );
    }

    #[test]
    fn rejects_every_unsafe_source_edge_and_destination_overflow() {
        let invalid_sources = [
            RectI16 {
                x: -1,
                y: 0,
                width: 1,
                height: 1,
            },
            RectI16 {
                x: 0,
                y: -1,
                width: 1,
                height: 1,
            },
            RectI16 {
                x: 0,
                y: 0,
                width: 0,
                height: 1,
            },
            RectI16 {
                x: 0,
                y: 0,
                width: 1,
                height: 0,
            },
            RectI16 {
                x: 7,
                y: 0,
                width: 2,
                height: 1,
            },
            RectI16 {
                x: 0,
                y: 3,
                width: 1,
                height: 2,
            },
        ];
        for source_rectangle in invalid_sources {
            let (pool, queues) = one_queue(glyph(
                FontStyle::Title,
                SoftwareQueuedFontTerminal::TexturedQuad,
                source_rectangle,
                RectI32 {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1,
                },
            ));
            let plan = crate::software_queued_font_flush::SoftwareQueuedFontFlushPlan::from_queues(
                &queues, &pool,
            );
            let command = &plan.commands()[0];
            assert_eq!(
                prepare_software_queued_font_quad(command, plan.source_owner(command), 0),
                Err(SoftwareQueuedFontQuadError::InvalidSourceRectangle {
                    rectangle: source_rectangle,
                    extent: [8, 4],
                })
            );
        }

        let (pool, queues) = one_queue(glyph(
            FontStyle::Title,
            SoftwareQueuedFontTerminal::TexturedQuad,
            RectI16 {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
            RectI32 {
                x: i32::MAX,
                y: 0,
                width: 1,
                height: 1,
            },
        ));
        let plan = crate::software_queued_font_flush::SoftwareQueuedFontFlushPlan::from_queues(
            &queues, &pool,
        );
        let command = &plan.commands()[0];
        assert!(matches!(
            prepare_software_queued_font_quad(command, plan.source_owner(command), 0),
            Err(SoftwareQueuedFontQuadError::DestinationOverflow(_))
        ));
    }

    fn one_queue(
        glyph: LaneNeutralQueuedFontGlyph,
    ) -> (SoftwareQueuedFontArgb4444Pool, SoftwareQueuedFontLaneQueues) {
        let pool = pool();
        let mut queues = SoftwareQueuedFontLaneQueues::new();
        push(
            &mut queues,
            SoftwareQueuedFontLane::Two,
            SoftwareQueuedFontByte3Policy::PreserveCaller,
            glyph,
        );
        (pool, queues)
    }

    fn push(
        queues: &mut SoftwareQueuedFontLaneQueues,
        lane: SoftwareQueuedFontLane,
        policy: SoftwareQueuedFontByte3Policy,
        glyph: LaneNeutralQueuedFontGlyph,
    ) {
        assert_eq!(
            queues.push(lane, policy, glyph),
            SoftwareQueuedFontPushOutcome::Accepted { slot: 0 }
        );
    }

    fn glyph(
        style: FontStyle,
        terminal: SoftwareQueuedFontTerminal,
        source: RectI16,
        destination: RectI32,
    ) -> LaneNeutralQueuedFontGlyph {
        LaneNeutralQueuedFontGlyph {
            command_index: 0,
            blit_index: 0,
            code: b'A',
            glyph_index: 0,
            style,
            source,
            destination,
            caller_tuple: [1, 2, 3, 4],
            terminal,
        }
    }

    fn pool() -> SoftwareQueuedFontArgb4444Pool {
        pool_with_extent(8, 4)
    }

    fn pool_with_extent(width: i32, height: i32) -> SoftwareQueuedFontArgb4444Pool {
        let bytes = font_file(width, height);
        let fonts = FontSet::from_ordered_fonts(std::array::from_fn(|_| {
            parse_legacy_font(&bytes, FontLimits::default()).unwrap()
        }));
        prepare_software_queued_font_argb4444_pool(fonts).unwrap()
    }

    fn font_file(width: i32, height: i32) -> Vec<u8> {
        const PIXEL_OFFSET: usize = 0x436;
        const FONT_HEADER_SIZE: usize = 24;
        const GLYPH_SIZE: usize = 16;
        let stride = usize::try_from((width + 3) & !3).unwrap();
        let pixel_bytes = stride * usize::try_from(height).unwrap();
        let trailer = PIXEL_OFFSET + pixel_bytes;
        let mut bytes = vec![0; trailer + FONT_HEADER_SIZE + GLYPH_SIZE];
        bytes[0..2].copy_from_slice(b"BM");
        bytes[18..22].copy_from_slice(&width.to_le_bytes());
        bytes[22..26].copy_from_slice(&height.to_le_bytes());
        bytes[28..30].copy_from_slice(&8u16.to_le_bytes());
        for index in 0..256usize {
            let offset = 14 + 40 + index * 4;
            bytes[offset..offset + 4].copy_from_slice(&[index as u8, index as u8, index as u8, 0]);
        }
        bytes[PIXEL_OFFSET..trailer].fill(1);
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
