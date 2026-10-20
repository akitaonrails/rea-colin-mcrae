//! Backend-neutral execution of precomputed legacy bitmap-font glyph plans.

use std::fmt;

use cmr_formats::font::{BitmapFont, GlyphBlit, RectI32, TextPlan};
use cmr_formats::riff_palette::PaletteEntry;

use crate::blit::{BlitDecision, ColorKey, RectI16, plan_legacy_blit};
use crate::display::{DisplayExtent, IndexedVideoBackend};
use crate::font_assets::{FONT_COUNT, FontSet, FontStyle};
use crate::surface::IndexedSurface;

const INDEX_ZERO_COLOR_KEY: ColorKey = ColorKey { low: 0, high: 0 };

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FontAtlasConversionError {
    PaletteEntryCount(usize),
    PixelCount { expected: u64, actual: usize },
}

impl fmt::Display for FontAtlasConversionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PaletteEntryCount(count) => {
                write!(
                    formatter,
                    "font atlas has {count} palette entries; expected 256"
                )
            }
            Self::PixelCount { expected, actual } => write!(
                formatter,
                "font atlas expects {expected} pixels, found {actual}"
            ),
        }
    }
}

impl std::error::Error for FontAtlasConversionError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FontRasterLayoutError {
    InvalidDestinationExtent(DisplayExtent),
    GlyphSourceOutOfRange {
        blit_index: usize,
        code: u8,
        source: cmr_formats::font::RectI16,
        atlas_width: u32,
        atlas_height: u32,
    },
    GlyphDimensionsDiffer {
        blit_index: usize,
        code: u8,
        source: cmr_formats::font::RectI16,
        destination: RectI32,
    },
    GlyphDoesNotMatchFont {
        blit_index: usize,
        code: u8,
        glyph_index: u8,
        source: cmr_formats::font::RectI16,
    },
    GlyphDestinationOutOfRange {
        blit_index: usize,
        code: u8,
        destination: RectI32,
    },
}

impl fmt::Display for FontRasterLayoutError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDestinationExtent(extent) => write!(
                formatter,
                "font destination extent {}x{} is outside the positive i16 range",
                extent.width, extent.height
            ),
            Self::GlyphSourceOutOfRange {
                blit_index,
                code,
                source,
                atlas_width,
                atlas_height,
            } => write!(
                formatter,
                "font glyph blit {blit_index} for byte {code:#04x} has source {source:?} outside the positive {atlas_width}x{atlas_height} atlas"
            ),
            Self::GlyphDimensionsDiffer {
                blit_index,
                code,
                source,
                destination,
            } => write!(
                formatter,
                "font glyph blit {blit_index} for byte {code:#04x} would stretch source {source:?} to destination {destination:?}"
            ),
            Self::GlyphDoesNotMatchFont {
                blit_index,
                code,
                glyph_index,
                source,
            } => write!(
                formatter,
                "font glyph blit {blit_index} for byte {code:#04x} and glyph index {glyph_index} with source {source:?} does not match the supplied font"
            ),
            Self::GlyphDestinationOutOfRange {
                blit_index,
                code,
                destination,
            } => write!(
                formatter,
                "font glyph blit {blit_index} for byte {code:#04x} has destination {destination:?} outside the i16 rectangle range"
            ),
        }
    }
}

impl std::error::Error for FontRasterLayoutError {}

#[derive(Debug, Eq, PartialEq)]
pub enum FontRasterError<E> {
    Atlas(FontAtlasConversionError),
    Layout(FontRasterLayoutError),
    CreateSource(E),
}

impl<E: fmt::Display> fmt::Display for FontRasterError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Atlas(error) => error.fmt(formatter),
            Self::Layout(error) => error.fmt(formatter),
            Self::CreateSource(error) => {
                write!(formatter, "cannot create font atlas source: {error}")
            }
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for FontRasterError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Atlas(error) => Some(error),
            Self::Layout(error) => Some(error),
            Self::CreateSource(error) => Some(error),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct FontBlitDiagnostic<E> {
    pub blit_index: usize,
    pub code: u8,
    pub glyph_index: u8,
    pub source: E,
}

#[derive(Debug, Eq, PartialEq)]
pub struct FontRasterOutcome<E> {
    pub planned_glyphs: usize,
    pub no_ops: usize,
    pub successful_blits: usize,
    pub diagnostics: Vec<FontBlitDiagnostic<E>>,
}

#[derive(Debug, Eq, PartialEq)]
pub enum FontPoolUploadError<E> {
    Atlas {
        style: FontStyle,
        source: FontAtlasConversionError,
    },
    CreateSource {
        style: FontStyle,
        source: E,
    },
}

impl<E: fmt::Display> fmt::Display for FontPoolUploadError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Atlas { style, source } => {
                write!(formatter, "cannot convert {style:?} font atlas: {source}")
            }
            Self::CreateSource { style, source } => {
                write!(formatter, "cannot upload {style:?} font atlas: {source}")
            }
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for FontPoolUploadError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Atlas { source, .. } => Some(source),
            Self::CreateSource { source, .. } => Some(source),
        }
    }
}

/// Persistent uploaded form of the 16-font frontend pool.
///
/// Construction is transactional: every atlas is converted before backend
/// mutation, and a failed upload releases all earlier handles in reverse order.
/// The pool exclusively borrows its backend so its handles cannot outlive that
/// backend. Dropping it releases every handle once, also in reverse order.
pub struct UploadedFontPool<'a, B: IndexedVideoBackend> {
    backend: &'a mut B,
    fonts: FontSet,
    sources: [B::Surface; FONT_COUNT],
}

impl<'a, B: IndexedVideoBackend> UploadedFontPool<'a, B> {
    pub fn upload(
        backend: &'a mut B,
        fonts: FontSet,
    ) -> Result<Self, FontPoolUploadError<B::Error>> {
        let atlases = FontStyle::ALL
            .into_iter()
            .map(|style| {
                font_atlas_surface(fonts.get(style))
                    .map(|atlas| (style, atlas))
                    .map_err(|source| FontPoolUploadError::Atlas { style, source })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let mut sources = Vec::with_capacity(FONT_COUNT);
        for (style, atlas) in &atlases {
            match backend.create_source(atlas) {
                Ok(source) => sources.push(source),
                Err(source) => {
                    for uploaded in sources.into_iter().rev() {
                        backend.destroy_source(uploaded);
                    }
                    return Err(FontPoolUploadError::CreateSource {
                        style: *style,
                        source,
                    });
                }
            }
        }
        let sources = sources
            .try_into()
            .unwrap_or_else(|_| unreachable!("one source was uploaded for every font style"));
        Ok(Self {
            backend,
            fonts,
            sources,
        })
    }

    pub fn font(&self, style: FontStyle) -> &BitmapFont {
        self.fonts.get(style)
    }

    pub fn backend_mut(&mut self) -> &mut B {
        self.backend
    }

    pub fn execute(
        &mut self,
        style: FontStyle,
        plan: &TextPlan,
        destination: B::Surface,
        destination_extent: DisplayExtent,
    ) -> Result<FontRasterOutcome<B::Error>, FontRasterError<B::Error>> {
        let font = self.fonts.get(style);
        let source = self.sources[style as usize];
        let (width, height) =
            validate_text_plan(font, plan, destination_extent).map_err(FontRasterError::Layout)?;
        Ok(execute_validated_text_plan(
            self.backend,
            source,
            plan,
            destination,
            width,
            height,
        ))
    }
}

impl<B: IndexedVideoBackend> Drop for UploadedFontPool<'_, B> {
    fn drop(&mut self) {
        for source in self.sources.iter().rev().copied() {
            self.backend.destroy_source(source);
        }
    }
}

/// Converts a parsed font atlas without remapping its indices or palette.
pub fn font_atlas_surface(font: &BitmapFont) -> Result<IndexedSurface, FontAtlasConversionError> {
    let atlas = font.atlas();
    if atlas.palette.len() != 256 {
        return Err(FontAtlasConversionError::PaletteEntryCount(
            atlas.palette.len(),
        ));
    }
    let expected = u64::from(atlas.width) * u64::from(atlas.height);
    if u64::try_from(atlas.pixels.len()).ok() != Some(expected) {
        return Err(FontAtlasConversionError::PixelCount {
            expected,
            actual: atlas.pixels.len(),
        });
    }
    let palette = std::array::from_fn(|index| {
        let entry = atlas.palette[index];
        PaletteEntry {
            red: entry.red,
            green: entry.green,
            blue: entry.blue,
            flags: entry.reserved,
        }
    });
    Ok(IndexedSurface {
        width: atlas.width,
        height: atlas.height,
        pitch: atlas.width,
        pixels: atlas.pixels.clone(),
        palette,
    })
}

/// Uploads one atlas source and executes every visible glyph plan in order.
///
/// Individual backend blit failures are diagnostics and do not stop later
/// glyphs. A successfully created source is always destroyed after the plan.
/// This per-plan source lifetime is a deliberate compatibility bridge and
/// differs from the recovered executable's persistent startup font surfaces;
/// runtime integration should replace it with a persistent uploaded font pool.
pub fn execute_text_plan<B: IndexedVideoBackend>(
    backend: &mut B,
    font: &BitmapFont,
    plan: &TextPlan,
    destination: B::Surface,
    destination_extent: DisplayExtent,
) -> Result<FontRasterOutcome<B::Error>, FontRasterError<B::Error>> {
    let (width, height) =
        validate_text_plan(font, plan, destination_extent).map_err(FontRasterError::Layout)?;
    let atlas = font_atlas_surface(font).map_err(FontRasterError::Atlas)?;
    let source = backend
        .create_source(&atlas)
        .map_err(FontRasterError::CreateSource)?;
    let outcome = execute_validated_text_plan(backend, source, plan, destination, width, height);
    backend.destroy_source(source);
    Ok(outcome)
}

fn validate_text_plan(
    font: &BitmapFont,
    plan: &TextPlan,
    destination_extent: DisplayExtent,
) -> Result<(i16, i16), FontRasterLayoutError> {
    let width = positive_i16(destination_extent.width).ok_or(
        FontRasterLayoutError::InvalidDestinationExtent(destination_extent),
    )?;
    let height = positive_i16(destination_extent.height).ok_or(
        FontRasterLayoutError::InvalidDestinationExtent(destination_extent),
    )?;
    for (blit_index, glyph) in plan.blits.iter().enumerate() {
        validate_glyph_source(font, blit_index, glyph)?;
        destination_rectangle(blit_index, glyph)?;
    }
    Ok((width, height))
}

fn execute_validated_text_plan<B: IndexedVideoBackend>(
    backend: &mut B,
    source: B::Surface,
    plan: &TextPlan,
    destination: B::Surface,
    width: i16,
    height: i16,
) -> FontRasterOutcome<B::Error> {
    let mut outcome = FontRasterOutcome {
        planned_glyphs: plan.blits.len(),
        no_ops: 0,
        successful_blits: 0,
        diagnostics: Vec::new(),
    };
    for (blit_index, glyph) in plan.blits.iter().enumerate() {
        let destination_rectangle = destination_rectangle(blit_index, glyph)
            .expect("all glyph rectangles were validated before source creation");
        match plan_legacy_blit(
            RectI16 {
                x: glyph.source.x,
                y: glyph.source.y,
                width: glyph.source.width,
                height: glyph.source.height,
            },
            destination_rectangle,
            width,
            height,
            Some(INDEX_ZERO_COLOR_KEY),
        ) {
            BlitDecision::NoOp => outcome.no_ops += 1,
            BlitDecision::Blit(blit_plan) => match backend.blit(source, destination, blit_plan) {
                Ok(()) => outcome.successful_blits += 1,
                Err(error) => outcome.diagnostics.push(FontBlitDiagnostic {
                    blit_index,
                    code: glyph.code,
                    glyph_index: glyph.glyph_index,
                    source: error,
                }),
            },
        }
    }
    outcome
}

fn validate_glyph_source(
    font: &BitmapFont,
    blit_index: usize,
    glyph: &GlyphBlit,
) -> Result<(), FontRasterLayoutError> {
    let source = glyph.source;
    let atlas = font.atlas();
    let source_right = i64::from(source.x) + i64::from(source.width);
    let source_bottom = i64::from(source.y) + i64::from(source.height);
    if source.x < 0
        || source.y < 0
        || source.width <= 0
        || source.height <= 0
        || source_right > i64::from(atlas.width)
        || source_bottom > i64::from(atlas.height)
    {
        return Err(FontRasterLayoutError::GlyphSourceOutOfRange {
            blit_index,
            code: glyph.code,
            source,
            atlas_width: atlas.width,
            atlas_height: atlas.height,
        });
    }
    if i32::from(source.width) != glyph.destination.width
        || i32::from(source.height) != glyph.destination.height
    {
        return Err(FontRasterLayoutError::GlyphDimensionsDiffer {
            blit_index,
            code: glyph.code,
            source,
            destination: glyph.destination,
        });
    }
    let indexed = font.glyphs().get(usize::from(glyph.glyph_index));
    let selected = font.glyph(glyph.code);
    if indexed.is_none()
        || selected.is_none()
        || !std::ptr::eq(
            indexed.expect("checked above"),
            selected.expect("checked above"),
        )
        || indexed.expect("checked above").source != source
    {
        return Err(FontRasterLayoutError::GlyphDoesNotMatchFont {
            blit_index,
            code: glyph.code,
            glyph_index: glyph.glyph_index,
            source,
        });
    }
    Ok(())
}

fn positive_i16(value: u16) -> Option<i16> {
    let converted = i16::try_from(value).ok()?;
    (converted > 0).then_some(converted)
}

fn destination_rectangle(
    blit_index: usize,
    glyph: &GlyphBlit,
) -> Result<RectI16, FontRasterLayoutError> {
    let destination = glyph.destination;
    let rectangle = RectI16 {
        x: i16::try_from(destination.x).map_err(|_| layout_error(blit_index, glyph))?,
        y: i16::try_from(destination.y).map_err(|_| layout_error(blit_index, glyph))?,
        width: i16::try_from(destination.width).map_err(|_| layout_error(blit_index, glyph))?,
        height: i16::try_from(destination.height).map_err(|_| layout_error(blit_index, glyph))?,
    };
    if rectangle.width <= 0 || rectangle.height <= 0 {
        return Err(layout_error(blit_index, glyph));
    }
    Ok(rectangle)
}

fn layout_error(blit_index: usize, glyph: &GlyphBlit) -> FontRasterLayoutError {
    FontRasterLayoutError::GlyphDestinationOutOfRange {
        blit_index,
        code: glyph.code,
        destination: glyph.destination,
    }
}

#[cfg(test)]
mod pool_tests {
    use std::collections::BTreeSet;

    use cmr_formats::font::{
        FontLimits, HorizontalAlignment, PointI32, TextAlignment, TextLayoutLimits, layout_text,
        parse_legacy_font,
    };

    use super::*;
    use crate::display::{CooperativeMode, DisplaySurfaces, IndexedMode, SurfaceTopology};

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum BackendError {
        Create(usize),
        Blit(usize),
    }

    impl fmt::Display for BackendError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "{self:?}")
        }
    }

    impl std::error::Error for BackendError {}

    #[derive(Clone, Debug, Eq, PartialEq)]
    enum Event {
        Create { marker: u8, handle: u8 },
        Blit { source: u8, destination: u8 },
        Destroy(u8),
    }

    #[derive(Default)]
    struct RecordingBackend {
        events: Vec<Event>,
        create_attempts: usize,
        fail_create_at: Option<usize>,
        fail_blits: BTreeSet<usize>,
        blit_attempts: usize,
    }

    impl IndexedVideoBackend for RecordingBackend {
        type Error = BackendError;
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

        fn create_source(
            &mut self,
            surface: &IndexedSurface,
        ) -> Result<Self::Surface, Self::Error> {
            let index = self.create_attempts;
            self.create_attempts += 1;
            if self.fail_create_at == Some(index) {
                return Err(BackendError::Create(index));
            }
            let handle = 10 + index as u8;
            self.events.push(Event::Create {
                marker: surface.pixels[0],
                handle,
            });
            Ok(handle)
        }

        fn blit(
            &mut self,
            source: Self::Surface,
            destination: Self::Surface,
            _plan: crate::blit::BlitPlan,
        ) -> Result<(), Self::Error> {
            let index = self.blit_attempts;
            self.blit_attempts += 1;
            self.events.push(Event::Blit {
                source,
                destination,
            });
            if self.fail_blits.contains(&index) {
                Err(BackendError::Blit(index))
            } else {
                Ok(())
            }
        }

        fn destroy_source(&mut self, source: Self::Surface) {
            self.events.push(Event::Destroy(source));
        }
    }

    #[test]
    fn pool_uploads_in_executable_order_reuses_handles_and_drops_in_reverse() {
        let mut backend = RecordingBackend::default();
        {
            let mut pool = UploadedFontPool::upload(&mut backend, font_set()).unwrap();
            for (index, style) in FontStyle::ALL.into_iter().enumerate() {
                assert_eq!(pool.font(style).raw_header()[0], index as u8);
            }
            let style = FontStyle::GreenInfo;
            let plan = plan(pool.font(style));
            for _ in 0..2 {
                let outcome = pool
                    .execute(
                        style,
                        &plan,
                        99,
                        DisplayExtent {
                            width: 20,
                            height: 10,
                        },
                    )
                    .unwrap();
                assert_eq!(outcome.successful_blits, 1);
            }
            assert_eq!(pool.backend_mut().create_attempts, FONT_COUNT);
            assert!(
                pool.backend_mut()
                    .events
                    .iter()
                    .all(|event| !matches!(event, Event::Destroy(_)))
            );
        }

        assert_eq!(
            &backend.events[..FONT_COUNT],
            &(0..FONT_COUNT)
                .map(|index| Event::Create {
                    marker: index as u8 + 1,
                    handle: 10 + index as u8,
                })
                .collect::<Vec<_>>()
        );
        assert_eq!(
            &backend.events[FONT_COUNT..FONT_COUNT + 2],
            &[
                Event::Blit {
                    source: 17,
                    destination: 99,
                },
                Event::Blit {
                    source: 17,
                    destination: 99,
                },
            ]
        );
        assert_eq!(
            &backend.events[FONT_COUNT + 2..],
            &(10..10 + FONT_COUNT as u8)
                .rev()
                .map(Event::Destroy)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn every_upload_failure_rolls_back_only_prior_handles_in_reverse() {
        for failed_index in 0..FONT_COUNT {
            let mut backend = RecordingBackend {
                fail_create_at: Some(failed_index),
                ..RecordingBackend::default()
            };
            let error = match UploadedFontPool::upload(&mut backend, font_set()) {
                Err(error) => error,
                Ok(_) => panic!("upload {failed_index} unexpectedly succeeded"),
            };
            assert!(matches!(
                error,
                FontPoolUploadError::CreateSource {
                    style,
                    source: BackendError::Create(index),
                } if style == FontStyle::ALL[failed_index] && index == failed_index
            ));
            assert_eq!(backend.create_attempts, failed_index + 1);
            let destroyed = backend
                .events
                .iter()
                .filter_map(|event| match event {
                    Event::Destroy(handle) => Some(*handle),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(
                destroyed,
                (10..10 + failed_index as u8).rev().collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn invalid_plan_has_no_backend_effect_and_does_not_poison_the_pool() {
        let mut backend = RecordingBackend::default();
        {
            let mut pool = UploadedFontPool::upload(&mut backend, font_set()).unwrap();
            let style = FontStyle::Title;
            let mut invalid = plan(pool.font(style));
            invalid.blits[0].glyph_index = 1;
            assert!(matches!(
                pool.execute(
                    style,
                    &invalid,
                    99,
                    DisplayExtent {
                        width: 20,
                        height: 10,
                    },
                ),
                Err(FontRasterError::Layout(
                    FontRasterLayoutError::GlyphDoesNotMatchFont { .. }
                ))
            ));
            assert_eq!(pool.backend_mut().blit_attempts, 0);

            let valid = plan(pool.font(style));
            assert_eq!(
                pool.execute(
                    style,
                    &valid,
                    99,
                    DisplayExtent {
                        width: 20,
                        height: 10,
                    },
                )
                .unwrap()
                .successful_blits,
                1
            );
        }
        assert_eq!(backend.blit_attempts, 1);
    }

    #[test]
    fn unwind_drops_every_uploaded_handle_once() {
        let mut backend = RecordingBackend::default();
        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _pool = UploadedFontPool::upload(&mut backend, font_set()).unwrap();
            panic!("injected unwind");
        }));
        assert!(unwind.is_err());
        assert_eq!(
            backend
                .events
                .iter()
                .filter(|event| matches!(event, Event::Destroy(_)))
                .count(),
            FONT_COUNT
        );
    }

    fn font_set() -> FontSet {
        FontSet::from_ordered_fonts(std::array::from_fn(|index| {
            parse_legacy_font(&synthetic_font(index as u8), FontLimits::default()).unwrap()
        }))
    }

    fn plan(font: &BitmapFont) -> TextPlan {
        layout_text(
            font,
            b"A",
            PointI32 { x: 1, y: 1 },
            20,
            TextAlignment {
                horizontal: HorizontalAlignment::Left,
                vertical: cmr_formats::font::VerticalAlignment::Top,
            },
            TextLayoutLimits::default(),
        )
        .unwrap()
    }

    fn synthetic_font(identity: u8) -> Vec<u8> {
        const PIXEL_OFFSET: usize = 0x436;
        const FONT_HEADER_SIZE: usize = 24;
        const GLYPH_SIZE: usize = 16;
        let trailer = PIXEL_OFFSET + 4;
        let mut input = vec![0; trailer + FONT_HEADER_SIZE + GLYPH_SIZE];
        input[0..2].copy_from_slice(b"BM");
        input[18..22].copy_from_slice(&1i32.to_le_bytes());
        input[22..26].copy_from_slice(&1i32.to_le_bytes());
        input[28..30].copy_from_slice(&8u16.to_le_bytes());
        input[PIXEL_OFFSET] = identity + 1;
        input[trailer] = identity;
        input[trailer + 1] = 1;
        let glyph = trailer + FONT_HEADER_SIZE;
        input[glyph] = b'A';
        input[glyph + 6..glyph + 8].copy_from_slice(&1i16.to_le_bytes());
        input[glyph + 8..glyph + 10].copy_from_slice(&1i16.to_le_bytes());
        input
    }
}
