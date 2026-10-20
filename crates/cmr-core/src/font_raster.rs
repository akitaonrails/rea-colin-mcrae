//! Backend-neutral execution of precomputed legacy bitmap-font glyph plans.

use std::fmt;

use cmr_formats::font::{
    BitmapFont, GlyphBlit, PointI32, RectI32, TextAlignment, TextLayoutError, TextLayoutLimits,
    TextPlan, layout_text,
};
use cmr_formats::riff_palette::PaletteEntry;

use crate::blit::{BlitDecision, ColorKey, RectI16, plan_legacy_blit};
use crate::display::{DisplayExtent, IndexedVideoBackend};
use crate::font_assets::{FONT_COUNT, FontSet, FontStyle};
use crate::source_store::{
    SourceAccessError, SourceBlitError, SourceKey, SourceUploadError, UploadedSourceStore,
};
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
    SourceAccess(SourceAccessError),
}

impl<E: fmt::Display> fmt::Display for FontRasterError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Atlas(error) => error.fmt(formatter),
            Self::Layout(error) => error.fmt(formatter),
            Self::CreateSource(error) => {
                write!(formatter, "cannot create font atlas source: {error}")
            }
            Self::SourceAccess(error) => write!(formatter, "cannot access font atlas: {error}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for FontRasterError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Atlas(error) => Some(error),
            Self::Layout(error) => Some(error),
            Self::CreateSource(error) => Some(error),
            Self::SourceAccess(error) => Some(error),
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
    /// Safe, per-glyph rejections discovered after the plan's fatal
    /// invariants were validated. Entries retain plan order.
    pub layout_diagnostics: Vec<FontRasterLayoutError>,
    /// Backend submission failures, retained separately for API compatibility.
    pub diagnostics: Vec<FontBlitDiagnostic<E>>,
}

#[derive(Debug, Eq, PartialEq)]
pub enum FontPoolUploadError<E> {
    Atlas {
        style: FontStyle,
        source: FontAtlasConversionError,
    },
    Capacity {
        style: FontStyle,
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
            Self::Capacity { style } => {
                write!(
                    formatter,
                    "cannot allocate a source key for {style:?} font atlas"
                )
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
            Self::Capacity { .. } => None,
        }
    }
}

/// Persistent uploaded form of the 16-font frontend pool.
///
/// Construction is transactional: every atlas is converted before store
/// mutation, and a failed upload releases only this pool's earlier sources in
/// reverse order. The shared store owns source lifetimes so this pool can
/// coexist with root-menu and later frontend resources.
pub struct UploadedFontPool {
    fonts: FontSet,
    sources: [SourceKey; FONT_COUNT],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PersistentTextPrepareError {
    Layout(TextLayoutError),
    Raster(FontRasterLayoutError),
}

impl fmt::Display for PersistentTextPrepareError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Layout(source) => source.fmt(formatter),
            Self::Raster(source) => source.fmt(formatter),
        }
    }
}

impl std::error::Error for PersistentTextPrepareError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Layout(source) => Some(source),
            Self::Raster(source) => Some(source),
        }
    }
}

/// Fully laid-out and fatally validated persistent-font work.
///
/// Fields remain private so execution cannot observe a plan different from the
/// one validated during the effect-free preparation phase.
#[derive(Debug)]
pub struct PreparedPersistentText {
    style: FontStyle,
    source: SourceKey,
    plan: TextPlan,
    validated: ValidatedTextPlan,
}

impl PreparedPersistentText {
    #[must_use]
    pub const fn style(&self) -> FontStyle {
        self.style
    }

    #[must_use]
    pub const fn plan(&self) -> &TextPlan {
        &self.plan
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct PersistentTextOutcome<E> {
    pub plan: TextPlan,
    pub raster: FontRasterOutcome<E>,
}

impl UploadedFontPool {
    pub fn upload<B: IndexedVideoBackend>(
        store: &mut UploadedSourceStore<'_, B>,
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

        let mut transaction = store.transaction();
        let mut sources = Vec::with_capacity(FONT_COUNT);
        for (style, atlas) in &atlases {
            match transaction.upload(atlas) {
                Ok(source) => sources.push(source),
                Err(SourceUploadError::Capacity) => {
                    return Err(FontPoolUploadError::Capacity { style: *style });
                }
                Err(SourceUploadError::Backend(source)) => {
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
        transaction.commit();
        Ok(Self { fonts, sources })
    }

    pub fn font(&self, style: FontStyle) -> &BitmapFont {
        self.fonts.get(style)
    }

    pub fn execute<B: IndexedVideoBackend>(
        &self,
        store: &mut UploadedSourceStore<'_, B>,
        style: FontStyle,
        plan: &TextPlan,
        destination: B::Surface,
        destination_extent: DisplayExtent,
    ) -> Result<FontRasterOutcome<B::Error>, FontRasterError<B::Error>> {
        let font = self.fonts.get(style);
        let source = self.sources[style as usize];
        let validated =
            validate_text_plan(font, plan, destination_extent).map_err(FontRasterError::Layout)?;
        execute_validated_text_plan_with(plan, validated, |blit_plan| {
            match store.blit(source, destination, blit_plan) {
                Ok(()) => Ok(()),
                Err(SourceBlitError::Backend(error)) => Err(BlitSubmissionError::Diagnostic(error)),
                Err(SourceBlitError::Access(error)) => Err(BlitSubmissionError::Fatal(error)),
            }
        })
        .map_err(FontRasterError::SourceAccess)
    }
}

/// Lays out raw bytes and validates every fatal glyph, source-geometry, and
/// destination invariant without touching the source store or backend.
pub fn prepare_persistent_text(
    fonts: &UploadedFontPool,
    text: &[u8],
    style: FontStyle,
    position: PointI32,
    legacy_flags: u8,
    destination_extent: DisplayExtent,
    limits: TextLayoutLimits,
) -> Result<PreparedPersistentText, PersistentTextPrepareError> {
    let font = fonts.font(style);
    let plan = layout_text(
        font,
        text,
        position,
        i32::from(destination_extent.width),
        TextAlignment::from_legacy_flags(legacy_flags),
        limits,
    )
    .map_err(PersistentTextPrepareError::Layout)?;
    let validated = validate_text_plan(font, &plan, destination_extent)
        .map_err(PersistentTextPrepareError::Raster)?;
    Ok(PreparedPersistentText {
        style,
        source: fonts.sources[style as usize],
        plan,
        validated,
    })
}

/// Executes one previously prepared plan through its persistent uploaded font.
///
/// Deferred out-of-atlas glyphs and backend blit failures remain ordered
/// diagnostics. Store provenance/lifetime failures remain fatal.
pub fn execute_prepared_persistent_text<B: IndexedVideoBackend>(
    store: &mut UploadedSourceStore<'_, B>,
    prepared: PreparedPersistentText,
    destination: B::Surface,
) -> Result<PersistentTextOutcome<B::Error>, SourceAccessError> {
    let PreparedPersistentText {
        style: _,
        source,
        plan,
        validated,
    } = prepared;
    let raster = execute_validated_text_plan_with(&plan, validated, |blit_plan| {
        match store.blit(source, destination, blit_plan) {
            Ok(()) => Ok(()),
            Err(SourceBlitError::Backend(error)) => Err(BlitSubmissionError::Diagnostic(error)),
            Err(SourceBlitError::Access(error)) => Err(BlitSubmissionError::Fatal(error)),
        }
    })?;
    Ok(PersistentTextOutcome { plan, raster })
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
    let validated =
        validate_text_plan(font, plan, destination_extent).map_err(FontRasterError::Layout)?;
    let atlas = font_atlas_surface(font).map_err(FontRasterError::Atlas)?;
    let source = backend
        .create_source(&atlas)
        .map_err(FontRasterError::CreateSource)?;
    let outcome = execute_validated_text_plan_with(plan, validated, |blit_plan| {
        backend
            .blit(source, destination, blit_plan)
            .map_err(BlitSubmissionError::<B::Error, std::convert::Infallible>::Diagnostic)
    })
    .unwrap_or_else(|never| match never {});
    backend.destroy_source(source);
    Ok(outcome)
}

fn validate_text_plan(
    font: &BitmapFont,
    plan: &TextPlan,
    destination_extent: DisplayExtent,
) -> Result<ValidatedTextPlan, FontRasterLayoutError> {
    let width = positive_i16(destination_extent.width).ok_or(
        FontRasterLayoutError::InvalidDestinationExtent(destination_extent),
    )?;
    let height = positive_i16(destination_extent.height).ok_or(
        FontRasterLayoutError::InvalidDestinationExtent(destination_extent),
    )?;
    let mut rejected_sources = Vec::new();
    for (blit_index, glyph) in plan.blits.iter().enumerate() {
        if let Some(diagnostic) = validate_glyph_source(font, blit_index, glyph)? {
            rejected_sources.push(diagnostic);
        }
        destination_rectangle(blit_index, glyph)?;
    }
    Ok(ValidatedTextPlan {
        width,
        height,
        rejected_sources,
    })
}

#[derive(Debug)]
struct ValidatedTextPlan {
    width: i16,
    height: i16,
    rejected_sources: Vec<FontRasterLayoutError>,
}

enum BlitSubmissionError<E, F> {
    Diagnostic(E),
    Fatal(F),
}

fn execute_validated_text_plan_with<E, F>(
    plan: &TextPlan,
    validated: ValidatedTextPlan,
    mut blit: impl FnMut(crate::blit::BlitPlan) -> Result<(), BlitSubmissionError<E, F>>,
) -> Result<FontRasterOutcome<E>, F> {
    let mut outcome = FontRasterOutcome {
        planned_glyphs: plan.blits.len(),
        no_ops: 0,
        successful_blits: 0,
        layout_diagnostics: Vec::new(),
        diagnostics: Vec::new(),
    };
    let mut rejected_sources = validated.rejected_sources.into_iter().peekable();
    for (blit_index, glyph) in plan.blits.iter().enumerate() {
        if rejected_sources
            .peek()
            .is_some_and(|diagnostic| diagnostic_blit_index(diagnostic) == blit_index)
        {
            outcome.layout_diagnostics.push(
                rejected_sources
                    .next()
                    .expect("the rejected source was just inspected"),
            );
            continue;
        }
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
            validated.width,
            validated.height,
            Some(INDEX_ZERO_COLOR_KEY),
        ) {
            BlitDecision::NoOp => outcome.no_ops += 1,
            BlitDecision::Blit(blit_plan) => match blit(blit_plan) {
                Ok(()) => outcome.successful_blits += 1,
                Err(BlitSubmissionError::Diagnostic(error)) => {
                    outcome.diagnostics.push(FontBlitDiagnostic {
                        blit_index,
                        code: glyph.code,
                        glyph_index: glyph.glyph_index,
                        source: error,
                    })
                }
                Err(BlitSubmissionError::Fatal(error)) => return Err(error),
            },
        }
    }
    Ok(outcome)
}

fn validate_glyph_source(
    font: &BitmapFont,
    blit_index: usize,
    glyph: &GlyphBlit,
) -> Result<Option<FontRasterLayoutError>, FontRasterLayoutError> {
    let source = glyph.source;
    let atlas = font.atlas();
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
    let out_of_range = FontRasterLayoutError::GlyphSourceOutOfRange {
        blit_index,
        code: glyph.code,
        source,
        atlas_width: atlas.width,
        atlas_height: atlas.height,
    };
    if source.x < 0 || source.y < 0 || source.width <= 0 || source.height <= 0 {
        return Err(out_of_range);
    }
    let source_right = i64::from(source.x) + i64::from(source.width);
    let source_bottom = i64::from(source.y) + i64::from(source.height);
    if source_right > i64::from(atlas.width) || source_bottom > i64::from(atlas.height) {
        return Ok(Some(out_of_range));
    }
    Ok(None)
}

fn diagnostic_blit_index(diagnostic: &FontRasterLayoutError) -> usize {
    match diagnostic {
        FontRasterLayoutError::GlyphSourceOutOfRange { blit_index, .. } => *blit_index,
        _ => unreachable!("only source-containment failures are deferred"),
    }
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
    fn persistent_text_preparation_preserves_raw_bytes_and_legacy_alignment_without_effects() {
        let mut backend = RecordingBackend::default();
        let mut store = UploadedSourceStore::new(&mut backend);
        let pool = UploadedFontPool::upload(&mut store, font_set()).unwrap();
        let effects_before = store.backend().events.len();
        let raw = [b'A', 0xff, b'A'];

        let prepared = prepare_persistent_text(
            &pool,
            &raw,
            FontStyle::GreenInfo,
            PointI32 { x: 10, y: 10 },
            0x33,
            DisplayExtent {
                width: 20,
                height: 10,
            },
            TextLayoutLimits::default(),
        )
        .unwrap();

        assert_eq!(prepared.style(), FontStyle::GreenInfo);
        assert_eq!(prepared.plan().lines[0].bytes, raw);
        assert_eq!(prepared.plan().lines[0].measured_width, 2);
        assert_eq!(prepared.plan().lines[0].origin, PointI32 { x: 8, y: 10 });
        assert_eq!(
            prepared
                .plan()
                .blits
                .iter()
                .map(|glyph| glyph.destination.x)
                .collect::<Vec<_>>(),
            [8, 9]
        );
        assert_eq!(store.backend().events.len(), effects_before);
        assert_eq!(store.backend().blit_attempts, 0);
    }

    #[test]
    fn early_and_late_fatal_glyphs_are_rejected_before_any_backend_effect() {
        let mut backend = RecordingBackend::default();
        let mut store = UploadedSourceStore::new(&mut backend);
        let pool = UploadedFontPool::upload(&mut store, font_set()).unwrap();
        let extent = DisplayExtent {
            width: 20,
            height: 10,
        };

        for (x, expected_index) in [(i32::from(i16::MAX) + 1, 0), (i32::from(i16::MAX), 1)] {
            let error = prepare_persistent_text(
                &pool,
                b"AA",
                FontStyle::Title,
                PointI32 { x, y: 1 },
                0,
                extent,
                TextLayoutLimits::default(),
            )
            .unwrap_err();
            assert!(matches!(
                error,
                PersistentTextPrepareError::Raster(
                    FontRasterLayoutError::GlyphDestinationOutOfRange { blit_index, .. }
                ) if blit_index == expected_index
            ));
        }
        assert_eq!(store.backend().blit_attempts, 0);
        assert!(matches!(
            prepare_persistent_text(
                &pool,
                b"A",
                FontStyle::Title,
                PointI32 { x: 1, y: 1 },
                0,
                DisplayExtent {
                    width: 0,
                    height: 10,
                },
                TextLayoutLimits::default(),
            ),
            Err(PersistentTextPrepareError::Layout(
                TextLayoutError::InvalidViewportWidth(0)
            ))
        ));
        assert_eq!(store.backend().blit_attempts, 0);
        for invalid_extent in [
            DisplayExtent {
                width: 20,
                height: 0,
            },
            DisplayExtent {
                width: i16::MAX as u16 + 1,
                height: 10,
            },
        ] {
            assert!(matches!(
                prepare_persistent_text(
                    &pool,
                    b"A",
                    FontStyle::Title,
                    PointI32 { x: 1, y: 1 },
                    0,
                    invalid_extent,
                    TextLayoutLimits::default(),
                ),
                Err(PersistentTextPrepareError::Raster(
                    FontRasterLayoutError::InvalidDestinationExtent(extent)
                )) if extent == invalid_extent
            ));
        }
        assert_eq!(store.backend().blit_attempts, 0);
    }

    #[test]
    fn prepared_text_rejects_foreign_and_released_font_sources_without_blitting() {
        let extent = DisplayExtent {
            width: 20,
            height: 10,
        };
        let mut first_backend = RecordingBackend::default();
        let mut first_store = UploadedSourceStore::new(&mut first_backend);
        let pool = UploadedFontPool::upload(&mut first_store, font_set()).unwrap();
        let prepared = prepare_persistent_text(
            &pool,
            b"A",
            FontStyle::Title,
            PointI32 { x: 1, y: 1 },
            0,
            extent,
            TextLayoutLimits::default(),
        )
        .unwrap();
        let mut foreign_backend = RecordingBackend::default();
        let mut foreign_store = UploadedSourceStore::new(&mut foreign_backend);
        assert!(matches!(
            execute_prepared_persistent_text(&mut foreign_store, prepared, 99),
            Err(SourceAccessError::ForeignKey(_))
        ));
        assert_eq!(foreign_store.backend().blit_attempts, 0);

        let prepared = prepare_persistent_text(
            &pool,
            b"A",
            FontStyle::Title,
            PointI32 { x: 1, y: 1 },
            0,
            extent,
            TextLayoutLimits::default(),
        )
        .unwrap();
        let released = pool.sources[FontStyle::Title as usize];
        first_store.release(released).unwrap();
        assert!(matches!(
            execute_prepared_persistent_text(&mut first_store, prepared, 99),
            Err(SourceAccessError::ReleasedKey(key)) if key == released
        ));
        assert_eq!(first_store.backend().blit_attempts, 0);
    }

    #[test]
    fn prepared_text_keeps_recoverable_glyph_and_backend_diagnostics_in_order() {
        let mut backend = RecordingBackend {
            fail_blits: [0].into_iter().collect(),
            ..RecordingBackend::default()
        };
        let mut store = UploadedSourceStore::new(&mut backend);
        let pool = UploadedFontPool::upload(&mut store, font_set_with_out_of_bounds_white_search())
            .unwrap();
        let prepared = prepare_persistent_text(
            &pool,
            b"WAA",
            FontStyle::WhiteSearch,
            PointI32 { x: 1, y: 1 },
            0,
            DisplayExtent {
                width: 20,
                height: 10,
            },
            TextLayoutLimits::default(),
        )
        .unwrap();

        let rendered = execute_prepared_persistent_text(&mut store, prepared, 99).unwrap();
        assert!(matches!(
            rendered.raster.layout_diagnostics.as_slice(),
            [FontRasterLayoutError::GlyphSourceOutOfRange {
                blit_index: 0,
                code: b'W',
                ..
            }]
        ));
        assert!(matches!(
            rendered.raster.diagnostics.as_slice(),
            [FontBlitDiagnostic {
                blit_index: 1,
                code: b'A',
                source: BackendError::Blit(0),
                ..
            }]
        ));
        assert_eq!(rendered.raster.planned_glyphs, 3);
        assert_eq!(rendered.raster.successful_blits, 1);
        assert_eq!(store.backend().blit_attempts, 2);
    }

    #[test]
    fn pool_uploads_in_executable_order_reuses_handles_and_drops_in_reverse() {
        let mut backend = RecordingBackend::default();
        {
            let mut store = UploadedSourceStore::new(&mut backend);
            let pool = UploadedFontPool::upload(&mut store, font_set()).unwrap();
            for (index, style) in FontStyle::ALL.into_iter().enumerate() {
                assert_eq!(pool.font(style).raw_header()[0], index as u8);
            }
            let style = FontStyle::GreenInfo;
            let plan = plan(pool.font(style));
            for _ in 0..2 {
                let outcome = pool
                    .execute(
                        &mut store,
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
            assert_eq!(store.backend().create_attempts, FONT_COUNT);
            assert!(
                store
                    .backend()
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
            let error = {
                let mut store = UploadedSourceStore::new(&mut backend);
                match UploadedFontPool::upload(&mut store, font_set()) {
                    Err(error) => error,
                    Ok(_) => panic!("upload {failed_index} unexpectedly succeeded"),
                }
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
    fn failed_font_transaction_preserves_existing_source_and_store_recovery() {
        let mut backend = RecordingBackend {
            fail_create_at: Some(3),
            ..RecordingBackend::default()
        };
        {
            let mut store = UploadedSourceStore::new(&mut backend);
            store.upload(&marker_surface(90)).unwrap();
            assert!(matches!(
                UploadedFontPool::upload(&mut store, font_set()),
                Err(FontPoolUploadError::CreateSource {
                    style,
                    source: BackendError::Create(3),
                }) if style == FontStyle::ALL[2]
            ));
            assert_eq!(store.uploaded_source_count(), 1);
            store.upload(&marker_surface(91)).unwrap();
            assert_eq!(store.uploaded_source_count(), 2);
        }
        assert_eq!(
            backend
                .events
                .iter()
                .filter_map(|event| match event {
                    Event::Destroy(handle) => Some(*handle),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            [12, 11, 14, 10]
        );
    }

    #[test]
    fn font_pool_coexists_with_other_sources_without_owning_their_lifetimes() {
        let mut backend = RecordingBackend::default();
        {
            let mut store = UploadedSourceStore::new(&mut backend);
            store.upload(&marker_surface(90)).unwrap();
            let pool = UploadedFontPool::upload(&mut store, font_set()).unwrap();
            store.upload(&marker_surface(91)).unwrap();
            let style = FontStyle::GreenInfo;
            let outcome = pool
                .execute(
                    &mut store,
                    style,
                    &plan(pool.font(style)),
                    99,
                    DisplayExtent {
                        width: 20,
                        height: 10,
                    },
                )
                .unwrap();
            assert_eq!(outcome.successful_blits, 1);
            assert_eq!(store.uploaded_source_count(), FONT_COUNT + 2);
            drop(pool);
            assert_eq!(store.uploaded_source_count(), FONT_COUNT + 2);
            assert!(
                store
                    .backend()
                    .events
                    .iter()
                    .all(|event| !matches!(event, Event::Destroy(_)))
            );
        }
        assert_eq!(
            backend
                .events
                .iter()
                .filter_map(|event| match event {
                    Event::Destroy(handle) => Some(*handle),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            (10..10 + FONT_COUNT as u8 + 2).rev().collect::<Vec<_>>()
        );
    }

    #[test]
    fn pool_used_with_another_store_returns_typed_access_error_without_blitting() {
        let mut first_backend = RecordingBackend::default();
        let mut second_backend = RecordingBackend::default();
        let mut first_store = UploadedSourceStore::new(&mut first_backend);
        let pool = UploadedFontPool::upload(&mut first_store, font_set()).unwrap();
        let style = FontStyle::Title;
        let mut second_store = UploadedSourceStore::new(&mut second_backend);
        assert!(matches!(
            pool.execute(
                &mut second_store,
                style,
                &plan(pool.font(style)),
                99,
                DisplayExtent {
                    width: 20,
                    height: 10,
                },
            ),
            Err(FontRasterError::SourceAccess(
                SourceAccessError::ForeignKey(_)
            ))
        ));
        assert_eq!(second_store.backend().blit_attempts, 0);
    }

    #[test]
    fn released_font_source_returns_typed_access_error_without_blitting() {
        let mut backend = RecordingBackend::default();
        let mut store = UploadedSourceStore::new(&mut backend);
        let pool = UploadedFontPool::upload(&mut store, font_set()).unwrap();
        let style = FontStyle::Title;
        let released = pool.sources[style as usize];
        store.release(released).unwrap();
        assert!(matches!(
            pool.execute(
                &mut store,
                style,
                &plan(pool.font(style)),
                99,
                DisplayExtent {
                    width: 20,
                    height: 10,
                },
            ),
            Err(FontRasterError::SourceAccess(
                SourceAccessError::ReleasedKey(key)
            )) if key == released
        ));
        assert_eq!(store.backend().blit_attempts, 0);
    }

    #[test]
    fn backend_blit_diagnostics_do_not_stop_later_glyphs_or_frames() {
        let mut backend = RecordingBackend {
            fail_blits: [0].into_iter().collect(),
            ..RecordingBackend::default()
        };
        let mut store = UploadedSourceStore::new(&mut backend);
        let pool = UploadedFontPool::upload(&mut store, font_set()).unwrap();
        let style = FontStyle::Title;
        let mut two_glyphs = plan(pool.font(style));
        two_glyphs.blits.push(two_glyphs.blits[0].clone());
        let first = pool
            .execute(
                &mut store,
                style,
                &two_glyphs,
                99,
                DisplayExtent {
                    width: 20,
                    height: 10,
                },
            )
            .unwrap();
        assert_eq!(first.successful_blits, 1);
        assert_eq!(first.diagnostics.len(), 1);
        let second = pool
            .execute(
                &mut store,
                style,
                &two_glyphs,
                99,
                DisplayExtent {
                    width: 20,
                    height: 10,
                },
            )
            .unwrap();
        assert_eq!(second.successful_blits, 2);
        assert!(second.diagnostics.is_empty());
    }

    #[test]
    fn invalid_plan_has_no_backend_effect_and_does_not_poison_the_pool() {
        let mut backend = RecordingBackend::default();
        {
            let mut store = UploadedSourceStore::new(&mut backend);
            let pool = UploadedFontPool::upload(&mut store, font_set()).unwrap();
            let style = FontStyle::Title;
            let mut invalid = plan(pool.font(style));
            invalid.blits[0].glyph_index = 1;
            assert!(matches!(
                pool.execute(
                    &mut store,
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
            assert_eq!(store.backend().blit_attempts, 0);

            let valid = plan(pool.font(style));
            assert_eq!(
                pool.execute(
                    &mut store,
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
    fn pool_admits_unused_out_of_atlas_metadata_and_guards_it_when_selected() {
        let mut backend = RecordingBackend::default();
        {
            let mut store = UploadedSourceStore::new(&mut backend);
            let pool =
                UploadedFontPool::upload(&mut store, font_set_with_out_of_bounds_white_search())
                    .unwrap();
            assert_eq!(store.backend().create_attempts, FONT_COUNT);

            let style = FontStyle::WhiteSearch;
            let outcome = pool
                .execute(
                    &mut store,
                    style,
                    &plan_text(pool.font(style), b"WA"),
                    99,
                    DisplayExtent {
                        width: 20,
                        height: 10,
                    },
                )
                .unwrap();
            assert_eq!(outcome.successful_blits, 1);
            assert!(matches!(
                outcome.layout_diagnostics.as_slice(),
                [FontRasterLayoutError::GlyphSourceOutOfRange {
                    blit_index: 0,
                    code: b'W',
                    atlas_width: 1,
                    atlas_height: 1,
                    ..
                }]
            ));
            assert_eq!(store.backend().blit_attempts, 1);
        }
        assert_eq!(backend.blit_attempts, 1);
    }

    #[test]
    fn unwind_drops_every_uploaded_handle_once() {
        let mut backend = RecordingBackend::default();
        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut store = UploadedSourceStore::new(&mut backend);
            let _pool = UploadedFontPool::upload(&mut store, font_set()).unwrap();
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

    fn font_set_with_out_of_bounds_white_search() -> FontSet {
        FontSet::from_ordered_fonts(std::array::from_fn(|index| {
            let input = if FontStyle::ALL[index] == FontStyle::WhiteSearch {
                synthetic_font_with_out_of_bounds_glyph(index as u8)
            } else {
                synthetic_font(index as u8)
            };
            parse_legacy_font(&input, FontLimits::default()).unwrap()
        }))
    }

    fn marker_surface(marker: u8) -> IndexedSurface {
        IndexedSurface {
            width: 1,
            height: 1,
            pitch: 1,
            pixels: vec![marker],
            palette: [PaletteEntry {
                red: 0,
                green: 0,
                blue: 0,
                flags: 0,
            }; 256],
        }
    }

    fn plan(font: &BitmapFont) -> TextPlan {
        plan_text(font, b"A")
    }

    fn plan_text(font: &BitmapFont, text: &[u8]) -> TextPlan {
        layout_text(
            font,
            text,
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

    fn synthetic_font_with_out_of_bounds_glyph(identity: u8) -> Vec<u8> {
        const PIXEL_OFFSET: usize = 0x436;
        const FONT_HEADER_SIZE: usize = 24;
        const GLYPH_SIZE: usize = 16;
        let trailer = PIXEL_OFFSET + 4;
        let mut input = synthetic_font(identity);
        input.resize(input.len() + GLYPH_SIZE, 0);
        input[trailer + 1] = 2;
        let glyph = trailer + FONT_HEADER_SIZE + GLYPH_SIZE;
        input[glyph] = b'W';
        input[glyph + 4..glyph + 6].copy_from_slice(&1i16.to_le_bytes());
        input[glyph + 6..glyph + 8].copy_from_slice(&1i16.to_le_bytes());
        input[glyph + 8..glyph + 10].copy_from_slice(&1i16.to_le_bytes());
        input
    }
}
