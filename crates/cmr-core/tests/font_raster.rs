use std::collections::BTreeSet;

use cmr_core::blit::{BlitPlan, RectEdges};
use cmr_core::display::{
    CooperativeMode, DisplayExtent, DisplaySurfaces, IndexedMode, IndexedVideoBackend,
    SurfaceTopology,
};
use cmr_core::font_raster::{
    FontRasterError, FontRasterLayoutError, execute_text_plan, font_atlas_surface,
};
use cmr_core::surface::IndexedSurface;
use cmr_formats::font::{
    FontLimits, GlyphBlit, HorizontalAlignment, PointI32, RectI16, RectI32, TextAlignment,
    TextLayoutLimits, TextPlan, VerticalAlignment, layout_text, parse_legacy_font,
};
use cmr_formats::riff_palette::PaletteEntry;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RecordingError {
    Create,
    Blit(usize),
}

impl std::fmt::Display for RecordingError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for RecordingError {}

#[derive(Default)]
struct RecordingBackend {
    fail_create: bool,
    fail_blits: BTreeSet<usize>,
    created: Vec<IndexedSurface>,
    blits: Vec<(u8, u8, BlitPlan)>,
    destroyed: Vec<u8>,
}

impl IndexedVideoBackend for RecordingBackend {
    type Error = RecordingError;
    type Surface = u8;
    type Palette = u8;

    fn create_device(&mut self) -> Result<(), Self::Error> {
        unreachable!()
    }

    fn set_cooperative_mode(&mut self, _mode: CooperativeMode) -> Result<(), Self::Error> {
        unreachable!()
    }

    fn set_display_mode(&mut self, _mode: IndexedMode) -> Result<(), Self::Error> {
        unreachable!()
    }

    fn create_surfaces(
        &mut self,
        _topology: SurfaceTopology,
        _mode: IndexedMode,
    ) -> Result<DisplaySurfaces<Self::Surface>, Self::Error> {
        unreachable!()
    }

    fn create_palette(
        &mut self,
        _entries: &[PaletteEntry; 256],
    ) -> Result<Self::Palette, Self::Error> {
        unreachable!()
    }

    fn attach_palette(
        &mut self,
        _surface: Self::Surface,
        _palette: Self::Palette,
    ) -> Result<(), Self::Error> {
        unreachable!()
    }

    fn upload_palette(
        &mut self,
        _palette: Self::Palette,
        _entries: &[PaletteEntry; 256],
    ) -> Result<(), Self::Error> {
        unreachable!()
    }

    fn create_source(&mut self, surface: &IndexedSurface) -> Result<Self::Surface, Self::Error> {
        if self.fail_create {
            return Err(RecordingError::Create);
        }
        self.created.push(surface.clone());
        Ok(7)
    }

    fn blit(
        &mut self,
        source: Self::Surface,
        destination: Self::Surface,
        plan: BlitPlan,
    ) -> Result<(), Self::Error> {
        let index = self.blits.len();
        self.blits.push((source, destination, plan));
        if self.fail_blits.contains(&index) {
            Err(RecordingError::Blit(index))
        } else {
            Ok(())
        }
    }

    fn destroy_source(&mut self, source: Self::Surface) {
        self.destroyed.push(source);
    }
}

#[test]
fn uploads_atlas_once_and_executes_exact_index_zero_key_plans() {
    let font = font();
    let plan = text_plan(&font, b"AB", PointI32 { x: 1, y: 1 });
    let mut backend = RecordingBackend::default();

    let outcome = execute_text_plan(
        &mut backend,
        &font,
        &plan,
        9,
        DisplayExtent {
            width: 20,
            height: 10,
        },
    )
    .unwrap();

    assert_eq!(backend.created.len(), 1);
    assert_eq!(backend.blits.len(), 2);
    assert_eq!(backend.destroyed, [7]);
    assert_eq!(outcome.planned_glyphs, 2);
    assert_eq!(outcome.successful_blits, 2);
    assert_eq!(outcome.no_ops, 0);
    assert!(outcome.diagnostics.is_empty());
    for (source, destination, plan) in &backend.blits {
        assert_eq!((*source, *destination), (7, 9));
        assert_eq!(plan.legacy_flags(), 0x11);
        assert_eq!(plan.color_key.unwrap().low, 0);
        assert_eq!(plan.color_key.unwrap().high, 0);
    }
}

#[test]
fn atlas_surface_preserves_normalized_indices_and_all_palette_metadata() {
    let font = font();
    let surface = font_atlas_surface(&font).unwrap();

    assert_eq!((surface.width, surface.height, surface.pitch), (4, 2, 4));
    assert_eq!(surface.pixels, [1, 2, 3, 4, 5, 6, 7, 8]);
    for (index, entry) in surface.palette.iter().enumerate() {
        let byte = index as u8;
        assert_eq!(entry.red, byte.wrapping_add(2));
        assert_eq!(entry.green, byte.wrapping_add(1));
        assert_eq!(entry.blue, byte);
        assert_eq!(entry.flags, 255 - byte);
    }
}

#[test]
fn clipping_adjusts_the_source_and_fully_outside_glyphs_are_no_ops() {
    let font = font();
    let plan = manual_plan(vec![
        glyph(0, b'A', 0, 0, 2, 1, -1, 0),
        glyph(1, b'B', 2, 0, 2, 1, 10, 0),
    ]);
    let mut backend = RecordingBackend::default();

    let outcome = execute_text_plan(
        &mut backend,
        &font,
        &plan,
        4,
        DisplayExtent {
            width: 10,
            height: 5,
        },
    )
    .unwrap();

    assert_eq!(outcome.no_ops, 1);
    assert_eq!(outcome.successful_blits, 1);
    assert_eq!(backend.blits.len(), 1);
    assert_eq!(backend.blits[0].2.source, edges(1, 0, 2, 1));
    assert_eq!(backend.blits[0].2.destination, edges(0, 0, 1, 1));
    assert_eq!(backend.destroyed, [7]);
}

#[test]
fn individual_blit_failures_are_ordered_and_do_not_stop_later_glyphs() {
    let font = font();
    let plan = manual_plan(vec![
        glyph(0, b'A', 0, 0, 2, 1, 0, 0),
        glyph(1, b'B', 2, 0, 2, 1, 2, 0),
        glyph(0, b'A', 0, 0, 2, 1, 4, 0),
    ]);
    let mut backend = RecordingBackend {
        fail_blits: BTreeSet::from([0, 2]),
        ..RecordingBackend::default()
    };

    let outcome = execute_text_plan(
        &mut backend,
        &font,
        &plan,
        3,
        DisplayExtent {
            width: 10,
            height: 5,
        },
    )
    .unwrap();

    assert_eq!(backend.blits.len(), 3);
    assert_eq!(backend.destroyed, [7]);
    assert_eq!(outcome.successful_blits, 1);
    assert_eq!(outcome.diagnostics.len(), 2);
    assert_eq!(outcome.diagnostics[0].blit_index, 0);
    assert_eq!(outcome.diagnostics[0].code, b'A');
    assert_eq!(outcome.diagnostics[0].source, RecordingError::Blit(0));
    assert_eq!(outcome.diagnostics[1].blit_index, 2);
    assert_eq!(outcome.diagnostics[1].source, RecordingError::Blit(2));
}

#[test]
fn source_creation_failure_makes_no_blits_or_destroy_call() {
    let font = font();
    let plan = text_plan(&font, b"A", PointI32 { x: 0, y: 0 });
    let mut backend = RecordingBackend {
        fail_create: true,
        ..RecordingBackend::default()
    };

    assert_eq!(
        execute_text_plan(
            &mut backend,
            &font,
            &plan,
            1,
            DisplayExtent {
                width: 10,
                height: 5,
            },
        ),
        Err(FontRasterError::CreateSource(RecordingError::Create))
    );
    assert!(backend.created.is_empty());
    assert!(backend.blits.is_empty());
    assert!(backend.destroyed.is_empty());
}

#[test]
fn invalid_extent_and_glyph_ranges_fail_before_backend_creation() {
    let font = font();
    let valid = text_plan(&font, b"A", PointI32 { x: 0, y: 0 });
    for extent in [
        DisplayExtent {
            width: 0,
            height: 5,
        },
        DisplayExtent {
            width: 32_768,
            height: 5,
        },
    ] {
        let mut backend = RecordingBackend::default();
        assert!(matches!(
            execute_text_plan(&mut backend, &font, &valid, 1, extent),
            Err(FontRasterError::Layout(
                FontRasterLayoutError::InvalidDestinationExtent(found)
            )) if found == extent
        ));
        assert!(backend.created.is_empty());
    }

    let invalid = manual_plan(vec![glyph(0, b'A', 0, 0, 2, 1, 40_000, 0)]);
    let mut backend = RecordingBackend::default();
    assert!(matches!(
        execute_text_plan(
            &mut backend,
            &font,
            &invalid,
            1,
            DisplayExtent {
                width: 10,
                height: 5,
            },
        ),
        Err(FontRasterError::Layout(
            FontRasterLayoutError::GlyphDestinationOutOfRange {
                blit_index: 0,
                code: b'A',
                destination: RectI32 { x: 40_000, .. },
            }
        ))
    ));
    assert!(backend.created.is_empty());
    assert!(backend.blits.is_empty());
    assert!(backend.destroyed.is_empty());
}

#[test]
fn rejects_nonpositive_and_out_of_atlas_sources_before_backend_creation() {
    let font = font();
    for invalid in [
        glyph(0, b'A', 0, 0, 0, 1, 0, 0),
        glyph(0, b'A', -1, 0, 2, 1, 0, 0),
        glyph(0, b'A', 3, 0, 2, 1, 0, 0),
    ] {
        let mut backend = RecordingBackend::default();
        assert!(matches!(
            execute_text_plan(
                &mut backend,
                &font,
                &manual_plan(vec![invalid]),
                1,
                DisplayExtent {
                    width: 10,
                    height: 5,
                },
            ),
            Err(FontRasterError::Layout(
                FontRasterLayoutError::GlyphSourceOutOfRange {
                    blit_index: 0,
                    code: b'A',
                    atlas_width: 4,
                    atlas_height: 2,
                    ..
                }
            ))
        ));
        assert!(backend.created.is_empty());
        assert!(backend.blits.is_empty());
    }
}

#[test]
fn rejects_stretched_and_forged_glyph_plans_before_backend_creation() {
    let font = font();
    let mut stretched = glyph(0, b'A', 0, 0, 2, 1, 0, 0);
    stretched.destination.width = 3;
    let forged = glyph(1, b'A', 2, 0, 2, 1, 0, 0);

    let mut backend = RecordingBackend::default();
    assert!(matches!(
        execute_text_plan(
            &mut backend,
            &font,
            &manual_plan(vec![stretched]),
            1,
            DisplayExtent {
                width: 10,
                height: 5,
            },
        ),
        Err(FontRasterError::Layout(
            FontRasterLayoutError::GlyphDimensionsDiffer {
                blit_index: 0,
                code: b'A',
                ..
            }
        ))
    ));
    assert!(backend.created.is_empty());

    assert!(matches!(
        execute_text_plan(
            &mut backend,
            &font,
            &manual_plan(vec![forged]),
            1,
            DisplayExtent {
                width: 10,
                height: 5,
            },
        ),
        Err(FontRasterError::Layout(
            FontRasterLayoutError::GlyphDoesNotMatchFont {
                blit_index: 0,
                code: b'A',
                glyph_index: 1,
                ..
            }
        ))
    ));
    assert!(backend.created.is_empty());
    assert!(backend.blits.is_empty());
}

fn font() -> cmr_formats::font::BitmapFont {
    parse_legacy_font(&synthetic_font(), FontLimits::default()).unwrap()
}

fn text_plan(font: &cmr_formats::font::BitmapFont, text: &[u8], anchor: PointI32) -> TextPlan {
    layout_text(
        font,
        text,
        anchor,
        100,
        TextAlignment {
            horizontal: HorizontalAlignment::Left,
            vertical: VerticalAlignment::Top,
        },
        TextLayoutLimits::default(),
    )
    .unwrap()
}

fn manual_plan(blits: Vec<GlyphBlit>) -> TextPlan {
    TextPlan {
        lines: Vec::new(),
        blits,
        block_height: 1,
        next_y: 1,
    }
}

#[allow(clippy::too_many_arguments)]
fn glyph(
    glyph_index: u8,
    code: u8,
    source_x: i16,
    source_y: i16,
    width: i16,
    height: i16,
    destination_x: i32,
    destination_y: i32,
) -> GlyphBlit {
    GlyphBlit {
        code,
        glyph_index,
        source: RectI16 {
            x: source_x,
            y: source_y,
            width,
            height,
        },
        destination: RectI32 {
            x: destination_x,
            y: destination_y,
            width: i32::from(width),
            height: i32::from(height),
        },
    }
}

fn edges(left: i32, top: i32, right: i32, bottom: i32) -> RectEdges {
    RectEdges {
        left,
        top,
        right,
        bottom,
    }
}

fn synthetic_font() -> Vec<u8> {
    const PIXEL_OFFSET: usize = 0x436;
    const WIDTH: usize = 4;
    const HEIGHT: usize = 2;
    const FONT_HEADER_SIZE: usize = 24;
    const GLYPH_SIZE: usize = 16;
    let trailer = PIXEL_OFFSET + WIDTH * HEIGHT;
    let mut input = vec![0; trailer + FONT_HEADER_SIZE + 2 * GLYPH_SIZE];
    input[0..2].copy_from_slice(b"BM");
    input[18..22].copy_from_slice(&(WIDTH as i32).to_le_bytes());
    input[22..26].copy_from_slice(&(HEIGHT as i32).to_le_bytes());
    input[28..30].copy_from_slice(&8u16.to_le_bytes());
    for index in 0..256 {
        let byte = index as u8;
        let offset = 54 + index * 4;
        input[offset..offset + 4].copy_from_slice(&[
            byte,
            byte.wrapping_add(1),
            byte.wrapping_add(2),
            255 - byte,
        ]);
    }
    // Legacy BMP rows are bottom-up; the parser normalizes them.
    input[PIXEL_OFFSET..PIXEL_OFFSET + 4].copy_from_slice(&[5, 6, 7, 8]);
    input[PIXEL_OFFSET + 4..PIXEL_OFFSET + 8].copy_from_slice(&[1, 2, 3, 4]);
    input[trailer + 1] = 2;
    input[trailer + 4..trailer + 8].copy_from_slice(&1u32.to_le_bytes());

    write_glyph(&mut input, trailer + FONT_HEADER_SIZE, b'A', 0, 0, 2, 1);
    write_glyph(
        &mut input,
        trailer + FONT_HEADER_SIZE + GLYPH_SIZE,
        b'B',
        2,
        0,
        2,
        1,
    );
    input
}

fn write_glyph(input: &mut [u8], offset: usize, code: u8, x: i16, y: i16, width: i16, height: i16) {
    input[offset] = code;
    input[offset + 2..offset + 4].copy_from_slice(&x.to_le_bytes());
    input[offset + 4..offset + 6].copy_from_slice(&y.to_le_bytes());
    input[offset + 6..offset + 8].copy_from_slice(&width.to_le_bytes());
    input[offset + 8..offset + 10].copy_from_slice(&height.to_le_bytes());
}
