//! Executable indexed-text slice for the recovered root-menu footer.

use std::fmt;

use cmr_formats::font::{
    PointI32, TextAlignment, TextLayoutError, TextLayoutLimits, TextPlan, layout_text,
};

use crate::display::{DisplayExtent, IndexedVideoBackend};
use crate::font_raster::{FontRasterError, FontRasterOutcome, UploadedFontPool};
use crate::frontend::{FrontendPhase, ROOT_FOOTER_TEXT_INDEX};
use crate::localization::IndexedTextTable;
use crate::root_menu_visual::{ROOT_FOOTER_FLAGS, ROOT_FOOTER_FONT_STYLE, ROOT_FOOTER_POSITION};
use crate::source_store::UploadedSourceStore;

#[derive(Debug, Eq, PartialEq)]
pub enum RootFooterError<E> {
    Layout(TextLayoutError),
    Raster(FontRasterError<E>),
}

impl<E: fmt::Display> fmt::Display for RootFooterError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Layout(error) => write!(formatter, "cannot lay out root footer: {error}"),
            Self::Raster(error) => write!(formatter, "cannot draw root footer: {error}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for RootFooterError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Layout(error) => Some(error),
            Self::Raster(error) => Some(error),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum RootFooterOutcome<E> {
    Skipped,
    Rendered {
        plan: TextPlan,
        raster: FontRasterOutcome<E>,
    },
}

/// Draws the localized root footer into the caller-selected active frame.
///
/// The original callback emits this text only in numeric frontend phase 1.
/// The destination handle and extent are explicit replacements for its active
/// display-context wrapper; no independent footer surface is created.
pub fn render_root_footer<B: IndexedVideoBackend>(
    phase: FrontendPhase,
    text: &IndexedTextTable,
    fonts: &UploadedFontPool,
    sources: &mut UploadedSourceStore<'_, B>,
    destination: B::Surface,
    destination_extent: DisplayExtent,
    limits: TextLayoutLimits,
) -> Result<RootFooterOutcome<B::Error>, RootFooterError<B::Error>> {
    if phase != FrontendPhase::Interactive {
        return Ok(RootFooterOutcome::Skipped);
    }

    let font = fonts.font(ROOT_FOOTER_FONT_STYLE);
    let plan = layout_text(
        font,
        text.lookup(ROOT_FOOTER_TEXT_INDEX),
        PointI32 {
            x: ROOT_FOOTER_POSITION.x,
            y: ROOT_FOOTER_POSITION.y,
        },
        i32::from(destination_extent.width),
        TextAlignment::from_legacy_flags(ROOT_FOOTER_FLAGS),
        limits,
    )
    .map_err(RootFooterError::Layout)?;
    let raster = fonts
        .execute(
            sources,
            ROOT_FOOTER_FONT_STYLE,
            &plan,
            destination,
            destination_extent,
        )
        .map_err(RootFooterError::Raster)?;
    Ok(RootFooterOutcome::Rendered { plan, raster })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use cmr_formats::font::{FontLimits, RectI32, parse_legacy_font};
    use cmr_formats::riff_palette::PaletteEntry;

    use super::*;
    use crate::blit::BlitPlan;
    use crate::display::{CooperativeMode, DisplaySurfaces, IndexedMode, SurfaceTopology};
    use crate::font_assets::{FONT_COUNT, FontSet, FontStyle};
    use crate::surface::IndexedSurface;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum BackendError {
        Blit,
    }

    impl fmt::Display for BackendError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "{self:?}")
        }
    }

    impl std::error::Error for BackendError {}

    #[derive(Default)]
    struct RecordingBackend {
        created: Vec<u8>,
        blits: Vec<(u8, u8, BlitPlan)>,
        destroyed: Vec<u8>,
        fail_blits: BTreeSet<usize>,
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
            let handle = 10 + self.created.len() as u8;
            self.created.push(surface.pixels[0]);
            Ok(handle)
        }

        fn blit(
            &mut self,
            source: Self::Surface,
            destination: Self::Surface,
            plan: BlitPlan,
        ) -> Result<(), Self::Error> {
            let blit_index = self.blits.len();
            self.blits.push((source, destination, plan));
            if self.fail_blits.contains(&blit_index) {
                Err(BackendError::Blit)
            } else {
                Ok(())
            }
        }

        fn destroy_source(&mut self, source: Self::Surface) {
            self.destroyed.push(source);
        }
    }

    #[test]
    fn interactive_footer_uses_exact_text_font_alignment_and_persistent_handle() {
        let table = table_with_footer(b"AB");
        let mut backend = RecordingBackend::default();
        {
            let mut sources = UploadedSourceStore::new(&mut backend);
            let fonts = UploadedFontPool::upload(&mut sources, font_set()).unwrap();
            for _ in 0..2 {
                let outcome = render_root_footer(
                    FrontendPhase::Interactive,
                    &table,
                    &fonts,
                    &mut sources,
                    99,
                    DisplayExtent {
                        width: 640,
                        height: 480,
                    },
                    TextLayoutLimits::default(),
                )
                .unwrap();
                let RootFooterOutcome::Rendered { plan, raster } = outcome else {
                    panic!("interactive footer was skipped");
                };
                assert_eq!(plan.lines.len(), 1);
                assert_eq!(plan.lines[0].bytes, b"AB");
                assert_eq!(plan.lines[0].origin, PointI32 { x: 317, y: 468 });
                assert_eq!(plan.block_height, 4);
                assert_eq!(
                    plan.blits
                        .iter()
                        .map(|blit| blit.destination)
                        .collect::<Vec<_>>(),
                    [
                        RectI32 {
                            x: 317,
                            y: 468,
                            width: 2,
                            height: 1,
                        },
                        RectI32 {
                            x: 320,
                            y: 468,
                            width: 2,
                            height: 1,
                        },
                    ]
                );
                assert_eq!(raster.successful_blits, 2);
                assert!(raster.diagnostics.is_empty());
            }
            assert_eq!(sources.backend().created.len(), FONT_COUNT);
            assert_eq!(sources.backend().blits.len(), 4);
            for (source, destination, plan) in &sources.backend().blits {
                assert_eq!((*source, *destination), (17, 99));
                assert_eq!(plan.legacy_flags(), 0x11);
            }
            assert!(sources.backend().destroyed.is_empty());
        }
        assert_eq!(backend.destroyed.len(), FONT_COUNT);
    }

    #[test]
    fn noninteractive_phases_skip_before_layout_or_backend_effects() {
        let oversized = table_with_footer(b"AB");
        let mut backend = RecordingBackend::default();
        {
            let mut sources = UploadedSourceStore::new(&mut backend);
            let fonts = UploadedFontPool::upload(&mut sources, font_set()).unwrap();
            for phase in [
                FrontendPhase::AdvanceToPending,
                FrontendPhase::AdvanceToInteractive,
                FrontendPhase::BeginTransition,
            ] {
                assert_eq!(
                    render_root_footer(
                        phase,
                        &oversized,
                        &fonts,
                        &mut sources,
                        99,
                        DisplayExtent {
                            width: 0,
                            height: 0,
                        },
                        TextLayoutLimits {
                            max_input_bytes: 0,
                            ..TextLayoutLimits::default()
                        },
                    )
                    .unwrap(),
                    RootFooterOutcome::Skipped
                );
            }
            assert!(sources.backend().blits.is_empty());
        }
    }

    #[test]
    fn layout_and_blit_failures_are_typed_and_do_not_replace_later_frames() {
        let table = table_with_footer(b"AB");
        let mut backend = RecordingBackend {
            fail_blits: [0, 1].into_iter().collect(),
            ..RecordingBackend::default()
        };
        {
            let mut sources = UploadedSourceStore::new(&mut backend);
            let fonts = UploadedFontPool::upload(&mut sources, font_set()).unwrap();
            assert!(matches!(
                render_root_footer(
                    FrontendPhase::Interactive,
                    &table,
                    &fonts,
                    &mut sources,
                    99,
                    DisplayExtent {
                        width: 640,
                        height: 480,
                    },
                    TextLayoutLimits {
                        max_input_bytes: 1,
                        ..TextLayoutLimits::default()
                    },
                ),
                Err(RootFooterError::Layout(
                    TextLayoutError::InputLimitExceeded {
                        actual: 2,
                        limit: 1,
                    }
                ))
            ));
            assert!(sources.backend().blits.is_empty());

            let RootFooterOutcome::Rendered { raster, .. } = render_root_footer(
                FrontendPhase::Interactive,
                &table,
                &fonts,
                &mut sources,
                99,
                DisplayExtent {
                    width: 640,
                    height: 480,
                },
                TextLayoutLimits::default(),
            )
            .unwrap() else {
                panic!("interactive footer was skipped");
            };
            assert_eq!(raster.successful_blits, 0);
            assert_eq!(raster.diagnostics.len(), 2);
            assert!(matches!(
                render_root_footer(
                    FrontendPhase::Interactive,
                    &table,
                    &fonts,
                    &mut sources,
                    99,
                    DisplayExtent {
                        width: 640,
                        height: 480,
                    },
                    TextLayoutLimits::default(),
                ),
                Ok(RootFooterOutcome::Rendered {
                    raster: FontRasterOutcome {
                        successful_blits: 2,
                        ..
                    },
                    ..
                })
            ));
        }
    }

    fn table_with_footer(footer: &[u8]) -> IndexedTextTable {
        let mut bytes = Vec::new();
        for index in 0..=ROOT_FOOTER_TEXT_INDEX {
            if index == ROOT_FOOTER_TEXT_INDEX {
                bytes.extend_from_slice(footer);
            } else {
                bytes.extend_from_slice(b"unused");
                bytes.push(b'0' + (index % 10) as u8);
            }
            if index != ROOT_FOOTER_TEXT_INDEX {
                bytes.push(b'\n');
            }
        }
        IndexedTextTable::parse(&bytes, ROOT_FOOTER_TEXT_INDEX + 1).unwrap()
    }

    fn font_set() -> FontSet {
        FontSet::from_ordered_fonts(std::array::from_fn(|index| {
            parse_legacy_font(&synthetic_font(index as u8), FontLimits::default()).unwrap()
        }))
    }

    fn synthetic_font(identity: u8) -> Vec<u8> {
        const PIXEL_OFFSET: usize = 0x436;
        const FONT_HEADER_SIZE: usize = 24;
        const GLYPH_SIZE: usize = 16;
        let trailer = PIXEL_OFFSET + 4;
        let mut input = vec![0; trailer + FONT_HEADER_SIZE + 2 * GLYPH_SIZE];
        input[0..2].copy_from_slice(b"BM");
        input[18..22].copy_from_slice(&4i32.to_le_bytes());
        input[22..26].copy_from_slice(&1i32.to_le_bytes());
        input[28..30].copy_from_slice(&8u16.to_le_bytes());
        input[PIXEL_OFFSET..PIXEL_OFFSET + 4].copy_from_slice(&[
            identity + 1,
            identity + 1,
            identity + 1,
            identity + 1,
        ]);
        input[trailer] = identity;
        input[trailer + 1] = 2;
        input[trailer + 4..trailer + 8].copy_from_slice(&4u32.to_le_bytes());
        input[trailer + 12..trailer + 16].copy_from_slice(&1u32.to_le_bytes());
        write_glyph(&mut input, trailer + FONT_HEADER_SIZE, b'A', 0);
        write_glyph(&mut input, trailer + FONT_HEADER_SIZE + GLYPH_SIZE, b'B', 2);
        input
    }

    fn write_glyph(input: &mut [u8], offset: usize, code: u8, x: i16) {
        input[offset] = code;
        input[offset + 2..offset + 4].copy_from_slice(&x.to_le_bytes());
        input[offset + 6..offset + 8].copy_from_slice(&2i16.to_le_bytes());
        input[offset + 8..offset + 10].copy_from_slice(&1i16.to_le_bytes());
    }

    #[test]
    fn constants_retain_the_observed_footer_contract() {
        assert_eq!(ROOT_FOOTER_TEXT_INDEX, 253);
        assert_eq!(ROOT_FOOTER_FONT_STYLE, FontStyle::GreenInfo);
        assert_eq!((ROOT_FOOTER_POSITION.x, ROOT_FOOTER_POSITION.y), (320, 470));
        assert_eq!(ROOT_FOOTER_FLAGS, 0x22);
    }
}
