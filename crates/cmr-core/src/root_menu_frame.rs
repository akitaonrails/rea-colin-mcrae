//! Sequential execution boundary for one recovered root-menu frame.
//!
//! Bitmap and footer behavior is implemented by recovered compatibility code.
//! C3D-command handling and the final `ComposeAndPresent` tail remain explicit
//! injected boundaries until those subsystems are recovered.

use std::fmt;

use cmr_formats::font::TextLayoutLimits;

use crate::display::{DisplayExtent, IndexedVideoBackend};
use crate::font_raster::UploadedFontPool;
use crate::localization::IndexedTextTable;
use crate::root_menu_bitmap_raster::{
    RootBitmapCommandError, RootBitmapCommandOutcome, UploadedRootMenuBitmapPool,
    execute_root_bitmap_command,
};
use crate::root_menu_footer::{RootFooterError, RootFooterOutcome, render_root_footer};
use crate::root_menu_visual::{
    CENTER_VISUAL_COUNT, ROOT_FOOTER_FLAGS, ROOT_FOOTER_POSITION, RenderCommand, RootRenderInput,
    RootVisualAssetError, RootVisualAssets, VisualHandle, root_render_transcript,
};
use crate::source_store::UploadedSourceStore;

/// Unrecovered root-frame operations supplied by a platform/render adapter.
///
/// Both calls receive the same destination token and extent used by indexed
/// drawing. This keeps ordering and target identity explicit without claiming
/// how C3D composition or presentation is implemented. Those target arguments
/// are reconstruction context, not recovered C3D call parameters.
pub trait RootFrameHooks: IndexedVideoBackend {
    type C3dError;
    type FinalError;

    fn execute_c3d_command(
        &mut self,
        destination: Self::Surface,
        destination_extent: DisplayExtent,
        object: VisualHandle,
        selection: u8,
        angle_delta: u16,
    ) -> Result<(), Self::C3dError>;

    fn finalize_compose_and_present(
        &mut self,
        destination: Self::Surface,
        destination_extent: DisplayExtent,
    ) -> Result<(), Self::FinalError>;
}

#[derive(Debug, Eq, PartialEq)]
pub struct RootFrameOutcome<E> {
    pub transcript: Vec<RenderCommand>,
    pub bitmap_blits: usize,
    pub bitmap_no_ops: usize,
    pub footer: Option<RootFooterOutcome<E>>,
    pub c3d_commands: usize,
    pub final_hook_completed: bool,
}

#[derive(Clone, Copy)]
pub struct RootFrameResources<'a> {
    pub text: &'a IndexedTextTable,
    pub fonts: &'a UploadedFontPool,
    pub bitmaps: &'a UploadedRootMenuBitmapPool,
    pub c3d_objects: [Option<VisualHandle>; CENTER_VISUAL_COUNT],
}

pub type RootFrameExecutionResult<E, C, P> = Result<RootFrameOutcome<E>, RootFrameError<E, C, P>>;

#[derive(Debug, Eq, PartialEq)]
pub enum RootFrameError<E, C, P> {
    VisualAssets(RootVisualAssetError),
    Bitmap {
        command_index: usize,
        source: RootBitmapCommandError<E>,
    },
    Footer {
        command_index: usize,
        source: RootFooterError<E>,
    },
    C3dCommand {
        command_index: usize,
        source: C,
    },
    Finalization {
        command_index: usize,
        source: P,
    },
}

impl<E: fmt::Display, C: fmt::Display, P: fmt::Display> fmt::Display for RootFrameError<E, C, P> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::VisualAssets(source) => {
                write!(formatter, "cannot bind root frame visual assets: {source}")
            }
            Self::Bitmap {
                command_index,
                source,
            } => write!(
                formatter,
                "root frame command {command_index} failed: {source}"
            ),
            Self::Footer {
                command_index,
                source,
            } => write!(
                formatter,
                "root frame footer command {command_index} failed: {source}"
            ),
            Self::C3dCommand {
                command_index,
                source,
            } => write!(
                formatter,
                "root frame C3D-command hook {command_index} failed: {source}"
            ),
            Self::Finalization {
                command_index,
                source,
            } => write!(
                formatter,
                "root frame final hook at command {command_index} failed: {source}"
            ),
        }
    }
}

impl<E, C, P> std::error::Error for RootFrameError<E, C, P>
where
    E: std::error::Error + 'static,
    C: std::error::Error + 'static,
    P: std::error::Error + 'static,
{
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::VisualAssets(source) => Some(source),
            Self::Bitmap { source, .. } => Some(source),
            Self::Footer { source, .. } => Some(source),
            Self::C3dCommand { source, .. } => Some(source),
            Self::Finalization { source, .. } => Some(source),
        }
    }
}

/// Generates and executes one canonical root-menu transcript in exact order.
///
/// Fatal bitmap, footer-layout/source, C3D-command-hook, and final-hook failures
/// stop the frame. Individual footer glyph blit failures retain the recovered
/// behavior: they remain ordered diagnostics and do not suppress later
/// commands.
pub fn execute_root_frame<B>(
    input: RootRenderInput,
    resources: RootFrameResources<'_>,
    sources: &mut UploadedSourceStore<'_, B>,
    destination: B::Surface,
    text_limits: TextLayoutLimits,
) -> RootFrameExecutionResult<B::Error, B::C3dError, B::FinalError>
where
    B: RootFrameHooks,
{
    let visuals = RootVisualAssets::from_bitmap_pool(resources.bitmaps, resources.c3d_objects)
        .map_err(RootFrameError::VisualAssets)?;
    let transcript = root_render_transcript(input, &visuals);
    let mut bitmap_blits = 0;
    let mut bitmap_no_ops = 0;
    let mut footer = None;
    let mut c3d_commands = 0;
    let mut final_hook_completed = false;

    for (command_index, command) in transcript.iter().copied().enumerate() {
        match command {
            RenderCommand::Backdrop { .. }
            | RenderCommand::ChromeShadow { .. }
            | RenderCommand::ChromeAnimation { .. }
            | RenderCommand::SelectedChrome { .. }
            | RenderCommand::CenterIcon { .. } => {
                match execute_root_bitmap_command(
                    sources,
                    resources.bitmaps,
                    command,
                    destination,
                    input.viewport_extent,
                )
                .map_err(|source| RootFrameError::Bitmap {
                    command_index,
                    source,
                })? {
                    RootBitmapCommandOutcome::Blitted => bitmap_blits += 1,
                    RootBitmapCommandOutcome::NoOp => bitmap_no_ops += 1,
                    RootBitmapCommandOutcome::NotBitmap => {
                        unreachable!("matched bitmap command must classify as bitmap")
                    }
                }
            }
            RenderCommand::FooterText {
                text_index,
                position,
                flags,
            } => {
                if text_index != crate::frontend::ROOT_FOOTER_TEXT_INDEX
                    || position != ROOT_FOOTER_POSITION
                    || flags != ROOT_FOOTER_FLAGS
                {
                    unreachable!("canonical transcript emitted a noncanonical footer command");
                }
                let outcome = render_root_footer(
                    input.phase,
                    resources.text,
                    resources.fonts,
                    sources,
                    destination,
                    input.viewport_extent,
                    text_limits,
                )
                .map_err(|source| RootFrameError::Footer {
                    command_index,
                    source,
                })?;
                if matches!(outcome, RootFooterOutcome::Skipped) {
                    unreachable!("canonical transcript emitted a footer outside phase 1");
                }
                footer = Some(outcome);
            }
            RenderCommand::C3dObject {
                object,
                selection,
                angle_delta,
            } => {
                sources
                    .backend_mut()
                    .execute_c3d_command(
                        destination,
                        input.viewport_extent,
                        object,
                        selection,
                        angle_delta,
                    )
                    .map_err(|source| RootFrameError::C3dCommand {
                        command_index,
                        source,
                    })?;
                c3d_commands += 1;
            }
            RenderCommand::ComposeAndPresent => {
                if command_index + 1 != transcript.len() {
                    unreachable!("canonical transcript emitted nonterminal final command");
                }
                sources
                    .backend_mut()
                    .finalize_compose_and_present(destination, input.viewport_extent)
                    .map_err(|source| RootFrameError::Finalization {
                        command_index,
                        source,
                    })?;
                final_hook_completed = true;
            }
        }
    }

    if !final_hook_completed {
        unreachable!("canonical transcript omitted its final command");
    }
    Ok(RootFrameOutcome {
        transcript,
        bitmap_blits,
        bitmap_no_ops,
        footer,
        c3d_commands,
        final_hook_completed,
    })
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::collections::BTreeSet;
    use std::rc::Rc;

    use cmr_assets::AssetStoreError;
    use cmr_formats::bitmap::{BitmapMetadata, BitmapPaletteEntry, IndexedBitmap};
    use cmr_formats::font::{FontLimits, parse_legacy_font};
    use cmr_formats::riff_palette::{LegacyPalette256, PaletteEntry};

    use super::*;
    use crate::blit::BlitPlan;
    use crate::display::{CooperativeMode, DisplaySurfaces, IndexedMode, SurfaceTopology};
    use crate::font_assets::{FONT_COUNT, FontSet, FontStyle};
    use crate::frontend::{FrontendPhase, ROOT_FOOTER_TEXT_INDEX, RootSelection};
    use crate::localization::Language;
    use crate::root_menu_assets::VisualVariant;
    use crate::root_menu_bitmap_assets::{
        ROOT_MENU_BITMAP_REQUEST_COUNT, RootMenuBitmapAsset, RootMenuBitmapLoadError,
        RootMenuBitmapParts, RootMenuBitmapSlot,
    };
    use crate::root_menu_bitmap_raster::{prepare_parts, upload_root_menu_bitmaps};
    use crate::root_menu_visual::{
        C3D_ANGLE_DELTA, ROOT_INITIAL_HORIZONTAL_TRANSFORM_Q16, ROOT_INITIAL_VERTICAL_TRANSFORM_Q16,
    };
    use crate::surface::IndexedSurface;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum BackendError {
        Blit(u8),
    }

    impl fmt::Display for BackendError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "{self:?}")
        }
    }

    impl std::error::Error for BackendError {}

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum HookError {
        C3d,
        Present,
    }

    impl fmt::Display for HookError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "{self:?}")
        }
    }

    impl std::error::Error for HookError {}

    #[derive(Clone, Debug, Eq, PartialEq)]
    enum Event {
        Create(u8),
        Blit(u8),
        C3d {
            destination: u16,
            extent: DisplayExtent,
            object: VisualHandle,
            selection: u8,
            angle_delta: u16,
        },
        Present {
            destination: u16,
            extent: DisplayExtent,
        },
        Destroy(u8),
    }

    struct RecordingBackend {
        events: Rc<RefCell<Vec<Event>>>,
        source_markers: Vec<u8>,
        create_attempts: usize,
        fail_markers: Rc<RefCell<BTreeSet<u8>>>,
        fail_c3d: Rc<Cell<bool>>,
        fail_final: Rc<Cell<bool>>,
    }

    impl RecordingBackend {
        fn new(events: Rc<RefCell<Vec<Event>>>) -> Self {
            Self {
                events,
                source_markers: Vec::new(),
                create_attempts: 0,
                fail_markers: Rc::new(RefCell::new(BTreeSet::new())),
                fail_c3d: Rc::new(Cell::new(false)),
                fail_final: Rc::new(Cell::new(false)),
            }
        }
    }

    impl IndexedVideoBackend for RecordingBackend {
        type Error = BackendError;
        type Surface = u16;
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
            self.create_attempts += 1;
            let marker = surface.pixels[0];
            self.events.borrow_mut().push(Event::Create(marker));
            self.source_markers.push(marker);
            Ok(self.source_markers.len() as u16)
        }

        fn blit(
            &mut self,
            source: Self::Surface,
            _destination: Self::Surface,
            _plan: BlitPlan,
        ) -> Result<(), Self::Error> {
            let marker = self.source_markers[usize::from(source) - 1];
            self.events.borrow_mut().push(Event::Blit(marker));
            if self.fail_markers.borrow().contains(&marker) {
                Err(BackendError::Blit(marker))
            } else {
                Ok(())
            }
        }

        fn destroy_source(&mut self, source: Self::Surface) {
            let marker = self.source_markers[usize::from(source) - 1];
            self.events.borrow_mut().push(Event::Destroy(marker));
        }
    }

    impl RootFrameHooks for RecordingBackend {
        type C3dError = HookError;
        type FinalError = HookError;

        fn execute_c3d_command(
            &mut self,
            destination: u16,
            extent: DisplayExtent,
            object: VisualHandle,
            selection: u8,
            angle_delta: u16,
        ) -> Result<(), Self::C3dError> {
            self.events.borrow_mut().push(Event::C3d {
                destination,
                extent,
                object,
                selection,
                angle_delta,
            });
            if self.fail_c3d.get() {
                Err(HookError::C3d)
            } else {
                Ok(())
            }
        }

        fn finalize_compose_and_present(
            &mut self,
            destination: u16,
            extent: DisplayExtent,
        ) -> Result<(), Self::FinalError> {
            self.events.borrow_mut().push(Event::Present {
                destination,
                extent,
            });
            if self.fail_final.get() {
                Err(HookError::Present)
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn dispatches_complete_interactive_transcript_in_exact_order_without_reuploading() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut backend = RecordingBackend::new(Rc::clone(&events));
        {
            let mut sources = UploadedSourceStore::new(&mut backend);
            let fonts = UploadedFontPool::upload(&mut sources, font_set()).unwrap();
            let bitmaps = bitmap_pool(&mut sources, &[]);
            let create_attempts = sources.backend().create_attempts;
            let source_count = sources.uploaded_source_count();
            events.borrow_mut().clear();

            for _ in 0..2 {
                let outcome = execute_root_frame(
                    interactive_input(),
                    frame_resources(
                        &footer_table(b"A"),
                        &fonts,
                        &bitmaps,
                        [Some(VisualHandle::new(7).unwrap()); 6],
                    ),
                    &mut sources,
                    99,
                    TextLayoutLimits::default(),
                )
                .unwrap();
                assert_eq!(outcome.bitmap_blits, 5);
                assert_eq!(outcome.bitmap_no_ops, 0);
                assert_eq!(outcome.c3d_commands, 1);
                assert!(outcome.final_hook_completed);
                assert!(matches!(
                    outcome.footer,
                    Some(RootFooterOutcome::Rendered { .. })
                ));
            }
            assert_eq!(sources.backend().create_attempts, create_attempts);
            assert_eq!(sources.uploaded_source_count(), source_count);
            assert_eq!(source_count, FONT_COUNT + ROOT_MENU_BITMAP_REQUEST_COUNT);
            assert_eq!(
                events.borrow().as_slice(),
                [
                    Event::Blit(root_marker(RootMenuBitmapSlot::Backdrop)),
                    Event::Blit(root_marker(RootMenuBitmapSlot::MainShadow)),
                    Event::Blit(root_marker(RootMenuBitmapSlot::Animation0)),
                    Event::Blit(root_marker(RootMenuBitmapSlot::MainSelected)),
                    Event::Blit(font_marker(FontStyle::GreenInfo)),
                    Event::Blit(root_marker(RootMenuBitmapSlot::ChampionshipGlobe)),
                    c3d_event(),
                    present_event(),
                    Event::Blit(root_marker(RootMenuBitmapSlot::Backdrop)),
                    Event::Blit(root_marker(RootMenuBitmapSlot::MainShadow)),
                    Event::Blit(root_marker(RootMenuBitmapSlot::Animation0)),
                    Event::Blit(root_marker(RootMenuBitmapSlot::MainSelected)),
                    Event::Blit(font_marker(FontStyle::GreenInfo)),
                    Event::Blit(root_marker(RootMenuBitmapSlot::ChampionshipGlobe)),
                    c3d_event(),
                    present_event(),
                ]
            );
        }
    }

    #[test]
    fn optional_omissions_and_clipped_center_preserve_later_work() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut backend = RecordingBackend::new(Rc::clone(&events));
        {
            let mut sources = UploadedSourceStore::new(&mut backend);
            let fonts = UploadedFontPool::upload(&mut sources, font_set()).unwrap();
            let bitmaps = bitmap_pool(
                &mut sources,
                &[
                    RootMenuBitmapSlot::MainShadow,
                    RootMenuBitmapSlot::Animation0,
                ],
            );
            events.borrow_mut().clear();
            let mut input = interactive_input();
            input.horizontal_transform_q16 = 0x10000;
            input.vertical_transform_q16 = 0x8000;
            let outcome = execute_root_frame(
                input,
                frame_resources(&footer_table(b"A"), &fonts, &bitmaps, [None; 6]),
                &mut sources,
                99,
                TextLayoutLimits::default(),
            )
            .unwrap();
            assert_eq!(outcome.bitmap_blits, 2);
            assert_eq!(outcome.bitmap_no_ops, 1);
            assert_eq!(outcome.c3d_commands, 0);
            assert_eq!(
                events.borrow().as_slice(),
                [
                    Event::Blit(root_marker(RootMenuBitmapSlot::Backdrop)),
                    Event::Blit(root_marker(RootMenuBitmapSlot::MainSelected)),
                    Event::Blit(font_marker(FontStyle::GreenInfo)),
                    present_event(),
                ]
            );
        }
    }

    #[test]
    fn invalid_visual_binding_fails_before_any_frame_effect() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut backend = RecordingBackend::new(Rc::clone(&events));
        {
            let mut sources = UploadedSourceStore::new(&mut backend);
            let fonts = UploadedFontPool::upload(&mut sources, font_set()).unwrap();
            let bitmaps = bitmap_pool_with_backdrop_extent(&mut sources, &[], (65_536, 1));
            events.borrow_mut().clear();
            assert!(matches!(
                execute_root_frame(
                    interactive_input(),
                    frame_resources(&footer_table(b"A"), &fonts, &bitmaps, [None; 6]),
                    &mut sources,
                    99,
                    TextLayoutLimits::default(),
                ),
                Err(RootFrameError::VisualAssets(
                    RootVisualAssetError::BackdropExtentOutOfRange {
                        width: 65_536,
                        height: 1,
                    }
                ))
            ));
            assert!(events.borrow().is_empty());
        }
    }

    #[test]
    fn footer_blit_diagnostics_continue_but_layout_failure_stops_frame() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut backend = RecordingBackend::new(Rc::clone(&events));
        backend
            .fail_markers
            .borrow_mut()
            .insert(font_marker(FontStyle::GreenInfo));
        {
            let mut sources = UploadedSourceStore::new(&mut backend);
            let fonts = UploadedFontPool::upload(&mut sources, font_set()).unwrap();
            let bitmaps = bitmap_pool(&mut sources, &[]);
            events.borrow_mut().clear();
            let outcome = execute_root_frame(
                interactive_input(),
                frame_resources(
                    &footer_table(b"AA"),
                    &fonts,
                    &bitmaps,
                    [Some(VisualHandle::new(7).unwrap()); 6],
                ),
                &mut sources,
                99,
                TextLayoutLimits::default(),
            )
            .unwrap();
            let Some(RootFooterOutcome::Rendered { raster, .. }) = outcome.footer else {
                panic!("footer was not rendered");
            };
            assert_eq!(
                raster
                    .diagnostics
                    .iter()
                    .map(|diagnostic| (diagnostic.blit_index, diagnostic.source))
                    .collect::<Vec<_>>(),
                [
                    (0, BackendError::Blit(font_marker(FontStyle::GreenInfo))),
                    (1, BackendError::Blit(font_marker(FontStyle::GreenInfo))),
                ]
            );
            assert_eq!(
                events.borrow().as_slice(),
                [
                    Event::Blit(root_marker(RootMenuBitmapSlot::Backdrop)),
                    Event::Blit(root_marker(RootMenuBitmapSlot::MainShadow)),
                    Event::Blit(root_marker(RootMenuBitmapSlot::Animation0)),
                    Event::Blit(root_marker(RootMenuBitmapSlot::MainSelected)),
                    Event::Blit(font_marker(FontStyle::GreenInfo)),
                    Event::Blit(font_marker(FontStyle::GreenInfo)),
                    Event::Blit(root_marker(RootMenuBitmapSlot::ChampionshipGlobe)),
                    c3d_event(),
                    present_event(),
                ]
            );

            events.borrow_mut().clear();
            let error = execute_root_frame(
                interactive_input(),
                frame_resources(
                    &footer_table(b"AA"),
                    &fonts,
                    &bitmaps,
                    [Some(VisualHandle::new(7).unwrap()); 6],
                ),
                &mut sources,
                99,
                TextLayoutLimits {
                    max_input_bytes: 0,
                    ..TextLayoutLimits::default()
                },
            )
            .unwrap_err();
            assert!(matches!(
                error,
                RootFrameError::Footer {
                    command_index: 4,
                    ..
                }
            ));
            assert_eq!(
                events.borrow().as_slice(),
                [
                    Event::Blit(root_marker(RootMenuBitmapSlot::Backdrop)),
                    Event::Blit(root_marker(RootMenuBitmapSlot::MainShadow)),
                    Event::Blit(root_marker(RootMenuBitmapSlot::Animation0)),
                    Event::Blit(root_marker(RootMenuBitmapSlot::MainSelected)),
                ]
            );
        }
    }

    #[test]
    fn foreign_font_pool_is_typed_and_stops_before_center_and_final_hooks() {
        let font_events = Rc::new(RefCell::new(Vec::new()));
        let mut font_backend = RecordingBackend::new(font_events);
        let mut font_sources = UploadedSourceStore::new(&mut font_backend);
        let fonts = UploadedFontPool::upload(&mut font_sources, font_set()).unwrap();

        let frame_events = Rc::new(RefCell::new(Vec::new()));
        let mut frame_backend = RecordingBackend::new(Rc::clone(&frame_events));
        {
            let mut frame_sources = UploadedSourceStore::new(&mut frame_backend);
            let bitmaps = bitmap_pool(&mut frame_sources, &[]);
            frame_events.borrow_mut().clear();
            let error = execute_root_frame(
                interactive_input(),
                frame_resources(
                    &footer_table(b"A"),
                    &fonts,
                    &bitmaps,
                    [Some(VisualHandle::new(7).unwrap()); 6],
                ),
                &mut frame_sources,
                99,
                TextLayoutLimits::default(),
            )
            .unwrap_err();
            assert!(matches!(
                error,
                RootFrameError::Footer {
                    command_index: 4,
                    source: RootFooterError::Raster(
                        crate::font_raster::FontRasterError::SourceAccess(_)
                    ),
                }
            ));
            assert_eq!(
                frame_events.borrow().as_slice(),
                [
                    Event::Blit(root_marker(RootMenuBitmapSlot::Backdrop)),
                    Event::Blit(root_marker(RootMenuBitmapSlot::MainShadow)),
                    Event::Blit(root_marker(RootMenuBitmapSlot::Animation0)),
                    Event::Blit(root_marker(RootMenuBitmapSlot::MainSelected)),
                ]
            );
        }
    }

    #[test]
    fn fatal_bitmap_c3d_and_final_hook_failures_are_indexed_and_fail_fast() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut backend = RecordingBackend::new(Rc::clone(&events));
        let fail_markers = Rc::clone(&backend.fail_markers);
        let fail_c3d = Rc::clone(&backend.fail_c3d);
        let fail_final = Rc::clone(&backend.fail_final);
        {
            let mut sources = UploadedSourceStore::new(&mut backend);
            let fonts = UploadedFontPool::upload(&mut sources, font_set()).unwrap();
            let bitmaps = bitmap_pool(&mut sources, &[]);
            fail_markers
                .borrow_mut()
                .insert(root_marker(RootMenuBitmapSlot::Backdrop));
            events.borrow_mut().clear();
            assert!(matches!(
                execute_root_frame(
                    interactive_input(),
                    frame_resources(
                        &footer_table(b"A"),
                        &fonts,
                        &bitmaps,
                        [Some(VisualHandle::new(7).unwrap()); 6],
                    ),
                    &mut sources,
                    99,
                    TextLayoutLimits::default(),
                ),
                Err(RootFrameError::Bitmap {
                    command_index: 0,
                    source: RootBitmapCommandError::Backend { .. }
                })
            ));
            assert_eq!(events.borrow().len(), 1);
            fail_markers.borrow_mut().clear();

            events.borrow_mut().clear();
            fail_c3d.set(true);
            assert!(matches!(
                execute_root_frame(
                    interactive_input(),
                    frame_resources(
                        &footer_table(b"A"),
                        &fonts,
                        &bitmaps,
                        [Some(VisualHandle::new(7).unwrap()); 6],
                    ),
                    &mut sources,
                    99,
                    TextLayoutLimits::default(),
                ),
                Err(RootFrameError::C3dCommand {
                    command_index: 6,
                    source: HookError::C3d
                })
            ));
            assert!(
                !events
                    .borrow()
                    .iter()
                    .any(|event| matches!(event, Event::Present { .. }))
            );
            fail_c3d.set(false);

            events.borrow_mut().clear();
            fail_final.set(true);
            assert!(matches!(
                execute_root_frame(
                    interactive_input(),
                    frame_resources(
                        &footer_table(b"A"),
                        &fonts,
                        &bitmaps,
                        [Some(VisualHandle::new(7).unwrap()); 6],
                    ),
                    &mut sources,
                    99,
                    TextLayoutLimits::default(),
                ),
                Err(RootFrameError::Finalization {
                    command_index: 7,
                    source: HookError::Present
                })
            ));
            assert!(matches!(
                events.borrow().last(),
                Some(Event::Present { .. })
            ));
        }
    }

    #[test]
    fn noninteractive_suppressed_frame_does_not_invent_chrome_footer_or_c3d() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut backend = RecordingBackend::new(Rc::clone(&events));
        {
            let mut sources = UploadedSourceStore::new(&mut backend);
            let fonts = UploadedFontPool::upload(&mut sources, font_set()).unwrap();
            let bitmaps = bitmap_pool(&mut sources, &[]);
            events.borrow_mut().clear();
            let mut input = interactive_input();
            input.phase = FrontendPhase::BeginTransition;
            input.entry_visuals_suppressed = true;
            let outcome = execute_root_frame(
                input,
                frame_resources(
                    &footer_table(b"A"),
                    &fonts,
                    &bitmaps,
                    [Some(VisualHandle::new(7).unwrap()); 6],
                ),
                &mut sources,
                99,
                TextLayoutLimits::default(),
            )
            .unwrap();
            assert_eq!(outcome.bitmap_blits, 1);
            assert!(outcome.footer.is_none());
            assert_eq!(outcome.c3d_commands, 0);
            assert_eq!(
                events.borrow().as_slice(),
                [
                    Event::Blit(root_marker(RootMenuBitmapSlot::Backdrop)),
                    present_event(),
                ]
            );
        }
    }

    fn frame_resources<'a>(
        text: &'a IndexedTextTable,
        fonts: &'a UploadedFontPool,
        bitmaps: &'a UploadedRootMenuBitmapPool,
        c3d_objects: [Option<VisualHandle>; CENTER_VISUAL_COUNT],
    ) -> RootFrameResources<'a> {
        RootFrameResources {
            text,
            fonts,
            bitmaps,
            c3d_objects,
        }
    }

    fn interactive_input() -> RootRenderInput {
        RootRenderInput {
            phase: FrontendPhase::Interactive,
            selection: RootSelection::new(0).unwrap(),
            animation_position_q16: 0,
            language: Language::English,
            viewport_extent: DisplayExtent {
                width: 640,
                height: 480,
            },
            horizontal_transform_q16: ROOT_INITIAL_HORIZONTAL_TRANSFORM_Q16,
            vertical_transform_q16: ROOT_INITIAL_VERTICAL_TRANSFORM_Q16,
            entry_visuals_suppressed: false,
        }
    }

    fn c3d_event() -> Event {
        Event::C3d {
            destination: 99,
            extent: DisplayExtent {
                width: 640,
                height: 480,
            },
            object: VisualHandle::new(7).unwrap(),
            selection: 0,
            angle_delta: C3D_ANGLE_DELTA,
        }
    }

    fn present_event() -> Event {
        Event::Present {
            destination: 99,
            extent: DisplayExtent {
                width: 640,
                height: 480,
            },
        }
    }

    fn bitmap_pool(
        sources: &mut UploadedSourceStore<'_, RecordingBackend>,
        missing: &[RootMenuBitmapSlot],
    ) -> UploadedRootMenuBitmapPool {
        bitmap_pool_with_backdrop_extent(sources, missing, (1600, 960))
    }

    fn bitmap_pool_with_backdrop_extent(
        sources: &mut UploadedSourceStore<'_, RecordingBackend>,
        missing: &[RootMenuBitmapSlot],
        backdrop_extent: (u32, u32),
    ) -> UploadedRootMenuBitmapPool {
        let parts = RootMenuBitmapParts {
            language: Language::English,
            center_variant: VisualVariant::Normal,
            assets: RootMenuBitmapSlot::ALL
                .into_iter()
                .filter(|slot| !missing.contains(slot))
                .map(|slot| RootMenuBitmapAsset {
                    identity: slot.identity(VisualVariant::Normal),
                    path: slot.path(Language::English, VisualVariant::Normal),
                    bitmap: bitmap_for(slot, backdrop_extent),
                })
                .collect(),
            optional_diagnostics: missing
                .iter()
                .copied()
                .map(|slot| {
                    assert!(slot.is_optional());
                    RootMenuBitmapLoadError::Asset {
                        identity: slot.identity(VisualVariant::Normal),
                        path: slot.path(Language::English, VisualVariant::Normal),
                        source: AssetStoreError::InvalidImportState {
                            field: "synthetic missing optional",
                        },
                    }
                })
                .collect(),
        };
        let prepared = prepare_parts(parts, &legacy_palette()).unwrap();
        upload_root_menu_bitmaps(sources, prepared).unwrap().pool
    }

    fn bitmap_for(slot: RootMenuBitmapSlot, backdrop_extent: (u32, u32)) -> IndexedBitmap {
        let (width, height) = match slot {
            RootMenuBitmapSlot::Backdrop => backdrop_extent,
            RootMenuBitmapSlot::MainSelected | RootMenuBitmapSlot::MainShadow => (256, 256),
            RootMenuBitmapSlot::Animation0
            | RootMenuBitmapSlot::Animation1
            | RootMenuBitmapSlot::Animation2
            | RootMenuBitmapSlot::Animation3
            | RootMenuBitmapSlot::Animation4
            | RootMenuBitmapSlot::Animation5
            | RootMenuBitmapSlot::Animation6 => (400, 400),
            _ => (4, 1),
        };
        IndexedBitmap {
            width,
            height,
            source_was_top_down: false,
            palette: vec![
                BitmapPaletteEntry {
                    red: 0,
                    green: 0,
                    blue: 0,
                    reserved: 0,
                };
                256
            ],
            pixels: vec![root_marker(slot); (width * height) as usize],
            metadata: BitmapMetadata {
                reserved_1: 0,
                reserved_2: 0,
                x_pixels_per_meter: 0,
                y_pixels_per_meter: 0,
                important_colors: 0,
            },
        }
    }

    fn root_marker(slot: RootMenuBitmapSlot) -> u8 {
        40 + slot.index() as u8
    }

    fn font_marker(style: FontStyle) -> u8 {
        style as u8 + 1
    }

    fn legacy_palette() -> LegacyPalette256 {
        LegacyPalette256 {
            entries: [PaletteEntry {
                red: 0,
                green: 0,
                blue: 0,
                flags: 0,
            }; 256],
        }
    }

    fn footer_table(footer: &[u8]) -> IndexedTextTable {
        let mut bytes = Vec::new();
        for index in 0..=ROOT_FOOTER_TEXT_INDEX {
            bytes.extend_from_slice(if index == ROOT_FOOTER_TEXT_INDEX {
                footer
            } else {
                b"unused"
            });
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
        let mut input = vec![0; trailer + FONT_HEADER_SIZE + GLYPH_SIZE];
        input[0..2].copy_from_slice(b"BM");
        input[18..22].copy_from_slice(&1i32.to_le_bytes());
        input[22..26].copy_from_slice(&1i32.to_le_bytes());
        input[28..30].copy_from_slice(&8u16.to_le_bytes());
        input[PIXEL_OFFSET] = identity + 1;
        input[trailer] = identity;
        input[trailer + 1] = 1;
        input[trailer + 4..trailer + 8].copy_from_slice(&1u32.to_le_bytes());
        input[trailer + 12..trailer + 16].copy_from_slice(&1u32.to_le_bytes());
        let glyph = trailer + FONT_HEADER_SIZE;
        input[glyph] = b'A';
        input[glyph + 6..glyph + 8].copy_from_slice(&1i16.to_le_bytes());
        input[glyph + 8..glyph + 10].copy_from_slice(&1i16.to_le_bytes());
        input
    }
}
