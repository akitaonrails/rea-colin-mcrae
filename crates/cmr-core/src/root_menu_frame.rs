//! Sequential execution boundary for one recovered root-menu frame.
//!
//! Bitmap/footer behavior and C3D planning are implemented by recovered
//! compatibility code. A staged C3D plan is consumed by the always-run scene
//! transaction at the terminal `ComposeAndPresent` boundary. Its visibility
//! cleanup completes before the depth-gated deferred palette commit, which in
//! turn completes before presentation.

use std::fmt;

use cmr_formats::font::TextLayoutLimits;
use cmr_formats::riff_palette::PaletteEntry;

use crate::deferred_palette::{
    DeferredPaletteAdapter, DeferredPaletteCommitError, DeferredPaletteCommitOutcome,
    DeferredPaletteState, PaletteArenaIndex, commit_deferred_palette,
};
use crate::display::{DisplayExtent, IndexedVideoBackend};
use crate::font_raster::UploadedFontPool;
use crate::localization::IndexedTextTable;
use crate::root_menu_assets::VisualVariant;
use crate::root_menu_bitmap_raster::{
    RootBitmapCommandError, RootBitmapCommandOutcome, UploadedRootMenuBitmapPool,
    execute_root_bitmap_command,
};
use crate::root_menu_c3d_scene::RootMenuC3dFramePipeline;
use crate::root_menu_footer::{RootFooterError, RootFooterOutcome, render_root_footer};
use crate::root_menu_visual::{
    ROOT_FOOTER_FLAGS, ROOT_FOOTER_POSITION, RenderCommand, RootRenderInput, RootVisualAssetError,
    RootVisualAssets, root_render_transcript,
};
use crate::source_store::UploadedSourceStore;

/// Final presentation supplied by a platform/render adapter.
pub trait RootFrameHooks: IndexedVideoBackend {
    type FinalError;

    fn present_root_frame(
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
    pub scene_completed: bool,
    pub palette_commit: DeferredPaletteCommitOutcome,
    pub presentation_completed: bool,
}

/// Mutable renderer-owned state consumed by the terminal palette boundary.
///
/// The adapter that performs the upload is the frame backend; the arena,
/// recovered current/pending indexes, and the two native-equivalent software
/// caches remain explicit caller-owned renderer state.
pub struct RootFramePaletteResources<'a> {
    pub bits_per_pixel: u8,
    pub arena: &'a [[PaletteEntry; 256]],
    pub state: &'a mut DeferredPaletteState,
    pub renderer_cache: &'a mut [PaletteEntry; 256],
    pub global_cache: &'a mut [PaletteEntry; 256],
}

impl RootFramePaletteResources<'_> {
    /// `FUN_004c4920`'s request of arena entry `index`; an entry the arena
    /// did not load leaves the request out.
    pub fn request_loaded(&mut self, index: i32) {
        if usize::try_from(index).is_ok_and(|i| i < self.arena.len()) {
            self.state.request(PaletteArenaIndex::new(index));
        }
    }
}

pub struct RootFrameResources<'a, C> {
    pub text: &'a IndexedTextTable,
    pub fonts: &'a UploadedFontPool,
    pub bitmaps: &'a UploadedRootMenuBitmapPool,
    pub c3d: C,
}

pub type RootFrameExecutionResult<E, L, S, P> =
    Result<RootFrameOutcome<E>, RootFrameError<E, L, S, P>>;

#[derive(Debug, Eq, PartialEq)]
pub enum RootFrameError<E, L, S, P> {
    C3dVariantMismatch {
        bitmap_variant: VisualVariant,
        object_variant: VisualVariant,
    },
    VisualAssets(RootVisualAssetError),
    Bitmap {
        command_index: usize,
        source: RootBitmapCommandError<E>,
    },
    Footer {
        command_index: usize,
        source: RootFooterError<E>,
    },
    C3dPlan {
        command_index: usize,
        source: L,
    },
    C3dScene {
        command_index: usize,
        source: S,
    },
    PaletteCommit {
        command_index: usize,
        source: DeferredPaletteCommitError<E>,
    },
    Presentation {
        command_index: usize,
        source: P,
    },
}

impl<E: fmt::Display, L: fmt::Display, S: fmt::Display, P: fmt::Display> fmt::Display
    for RootFrameError<E, L, S, P>
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::C3dVariantMismatch {
                bitmap_variant,
                object_variant,
            } => write!(
                formatter,
                "root-menu bitmap variant {bitmap_variant:?} does not match C3D variant {object_variant:?}"
            ),
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
            Self::C3dPlan {
                command_index,
                source,
            } => write!(
                formatter,
                "root frame C3D plan at command {command_index} failed: {source}"
            ),
            Self::C3dScene {
                command_index,
                source,
            } => write!(
                formatter,
                "root frame C3D scene at terminal command {command_index} failed: {source}"
            ),
            Self::PaletteCommit {
                command_index,
                source,
            } => write!(
                formatter,
                "root frame palette commit at terminal command {command_index} failed: {source}"
            ),
            Self::Presentation {
                command_index,
                source,
            } => write!(
                formatter,
                "root frame presentation at command {command_index} failed: {source}"
            ),
        }
    }
}

impl<E, L, S, P> std::error::Error for RootFrameError<E, L, S, P>
where
    E: std::error::Error + 'static,
    L: std::error::Error + 'static,
    S: std::error::Error + 'static,
    P: std::error::Error + 'static,
{
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::C3dVariantMismatch { .. } => None,
            Self::VisualAssets(source) => Some(source),
            Self::Bitmap { source, .. } => Some(source),
            Self::Footer { source, .. } => Some(source),
            Self::C3dPlan { source, .. } => Some(source),
            Self::C3dScene { source, .. } => Some(source),
            Self::PaletteCommit { source, .. } => Some(source),
            Self::Presentation { source, .. } => Some(source),
        }
    }
}

/// Generates and executes one canonical root-menu transcript in exact order.
///
/// Fatal bitmap, footer-layout/source, C3D-plan/scene, palette-commit, and
/// presentation failures stop the frame. Individual footer glyph blit failures
/// retain the recovered behavior: they remain ordered diagnostics and do not
/// suppress later commands.
pub fn execute_root_frame<B, C>(
    input: RootRenderInput,
    mut resources: RootFrameResources<'_, C>,
    mut palette: RootFramePaletteResources<'_>,
    sources: &mut UploadedSourceStore<'_, B>,
    destination: B::Surface,
    text_limits: TextLayoutLimits,
) -> RootFrameExecutionResult<
    <B as IndexedVideoBackend>::Error,
    C::PlanError,
    C::SceneError,
    B::FinalError,
>
where
    B: RootFrameHooks + DeferredPaletteAdapter<Error = <B as IndexedVideoBackend>::Error>,
    C: RootMenuC3dFramePipeline<B>,
{
    if resources.bitmaps.center_variant() != resources.c3d.variant() {
        return Err(RootFrameError::C3dVariantMismatch {
            bitmap_variant: resources.bitmaps.center_variant(),
            object_variant: resources.c3d.variant(),
        });
    }
    let visuals = RootVisualAssets::from_bitmap_pool(resources.bitmaps)
        .map_err(RootFrameError::VisualAssets)?;
    let transcript = root_render_transcript(input, &visuals);
    let mut bitmap_blits = 0;
    let mut bitmap_no_ops = 0;
    let mut footer = None;
    let mut c3d_commands = 0;
    let mut staged_c3d = None;
    let mut scene_completed = false;
    let mut palette_commit = DeferredPaletteCommitOutcome::NoPendingPalette;
    let mut presentation_completed = false;

    for (command_index, command) in transcript.iter().copied().enumerate() {
        match command {
            RenderCommand::Backdrop { .. }
            | RenderCommand::ChromeShadow { .. }
            | RenderCommand::ChromeAnimation { .. }
            | RenderCommand::SelectedChrome { .. }
            | RenderCommand::CenterIcon { .. } => {
                // The root item draw `0x4cf590` requests the selected icon's
                // palette (`FUN_004c4920(icon, 2)`, arena base 11) before its
                // blit; an arena without the icon palettes leaves it out.
                if let RenderCommand::CenterIcon { visual_index, .. } = command {
                    palette.request_loaded(
                        crate::frontend_screen_bitmaps::palette::ICONS + i32::from(visual_index),
                    );
                }
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
            RenderCommand::C3dObject { slot, angle_delta } => {
                if staged_c3d.is_some() {
                    unreachable!("canonical transcript emitted multiple root C3D commands");
                }
                staged_c3d = resources
                    .c3d
                    .plan(slot, angle_delta, input.viewport_extent)
                    .map_err(|source| RootFrameError::C3dPlan {
                        command_index,
                        source,
                    })?;
                c3d_commands += usize::from(staged_c3d.is_some());
            }
            RenderCommand::ComposeAndPresent => {
                if command_index + 1 != transcript.len() {
                    unreachable!("canonical transcript emitted nonterminal final command");
                }
                let backend = sources.backend_mut();
                resources
                    .c3d
                    .execute_terminal_scene(
                        backend,
                        destination,
                        input.viewport_extent,
                        staged_c3d.take(),
                    )
                    .map_err(|source| RootFrameError::C3dScene {
                        command_index,
                        source,
                    })?;
                scene_completed = true;
                palette_commit = commit_deferred_palette(
                    palette.state,
                    palette.bits_per_pixel,
                    palette.arena,
                    palette.renderer_cache,
                    palette.global_cache,
                    backend,
                )
                .map_err(|source| RootFrameError::PaletteCommit {
                    command_index,
                    source,
                })?;
                backend
                    .present_root_frame(destination, input.viewport_extent)
                    .map_err(|source| RootFrameError::Presentation {
                        command_index,
                        source,
                    })?;
                presentation_completed = true;
            }
        }
    }

    if !presentation_completed {
        unreachable!("canonical transcript omitted its final command");
    }
    Ok(RootFrameOutcome {
        transcript,
        bitmap_blits,
        bitmap_no_ops,
        footer,
        c3d_commands,
        scene_completed,
        palette_commit,
        presentation_completed,
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
    use crate::deferred_palette::PaletteArenaIndex;
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
    use crate::root_menu_c3d_assets::{RootMenuC3dAngles, RootMenuC3dSet};
    use crate::root_menu_c3d_assets::{RootMenuC3dSlot, synthetic_root_menu_c3d_set};
    use crate::root_menu_visual::{
        C3D_ANGLE_DELTA, ROOT_INITIAL_HORIZONTAL_TRANSFORM_Q16, ROOT_INITIAL_VERTICAL_TRANSFORM_Q16,
    };
    use crate::surface::IndexedSurface;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum BackendError {
        Blit(u8),
        Palette,
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
            slot: RootMenuC3dSlot,
            asset_marker: u8,
            previous_angle: u16,
            resulting_angle: u16,
            delta: u16,
        },
        Scene {
            destination: u16,
            extent: DisplayExtent,
            has_plan: bool,
        },
        Present {
            destination: u16,
            extent: DisplayExtent,
        },
        PaletteUpload(u8),
        Destroy(u8),
    }

    struct RecordingBackend {
        events: Rc<RefCell<Vec<Event>>>,
        source_markers: Vec<u8>,
        create_attempts: usize,
        fail_markers: Rc<RefCell<BTreeSet<u8>>>,
        fail_c3d: Rc<Cell<bool>>,
        fail_palette: Rc<Cell<bool>>,
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
                fail_palette: Rc::new(Cell::new(false)),
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
        type FinalError = HookError;

        fn present_root_frame(
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

    impl DeferredPaletteAdapter for RecordingBackend {
        type Error = BackendError;

        fn upload_palette(&mut self, entries: &[PaletteEntry; 256]) -> Result<(), Self::Error> {
            self.events
                .borrow_mut()
                .push(Event::PaletteUpload(entries[0].red));
            if self.fail_palette.get() {
                Err(BackendError::Palette)
            } else {
                Ok(())
            }
        }
    }

    #[derive(Debug, Eq, PartialEq)]
    struct TestPlan {
        slot: RootMenuC3dSlot,
        asset_marker: u8,
        previous_angle: u16,
        resulting_angle: u16,
        delta: u16,
    }

    struct RecordingC3dPipeline<'a> {
        assets: &'a RootMenuC3dSet,
        angles: &'a mut RootMenuC3dAngles,
        fail_plan: bool,
    }

    impl RootMenuC3dFramePipeline<RecordingBackend> for RecordingC3dPipeline<'_> {
        type Plan = TestPlan;
        type PlanError = HookError;
        type SceneError = HookError;

        fn variant(&self) -> VisualVariant {
            self.assets.variant()
        }

        fn plan(
            &mut self,
            slot: RootMenuC3dSlot,
            angle_delta: u16,
            _extent: DisplayExtent,
        ) -> Result<Option<Self::Plan>, Self::PlanError> {
            if self.fail_plan {
                return Err(HookError::C3d);
            }
            Ok(self
                .angles
                .advance_frame_object(self.assets, slot, angle_delta)
                .map(|object| TestPlan {
                    slot: object.slot(),
                    asset_marker: object.asset().file.header().unknown_08[0],
                    previous_angle: object.previous_angle(),
                    resulting_angle: object.resulting_angle(),
                    delta: object.delta(),
                }))
        }

        fn execute_terminal_scene(
            &mut self,
            backend: &mut RecordingBackend,
            destination: u16,
            extent: DisplayExtent,
            plan: Option<Self::Plan>,
        ) -> Result<(), Self::SceneError> {
            backend.events.borrow_mut().push(Event::Scene {
                destination,
                extent,
                has_plan: plan.is_some(),
            });
            if let Some(plan) = plan {
                backend.events.borrow_mut().push(Event::C3d {
                    destination,
                    extent,
                    slot: plan.slot,
                    asset_marker: plan.asset_marker,
                    previous_angle: plan.previous_angle,
                    resulting_angle: plan.resulting_angle,
                    delta: plan.delta,
                });
            }
            if backend.fail_c3d.get() {
                Err(HookError::C3d)
            } else {
                Ok(())
            }
        }
    }

    // Most dispatcher tests exercise non-indexed terminal behavior. Keep their
    // setup compact while still routing through the real palette boundary.
    fn execute_root_frame<'a>(
        input: RootRenderInput,
        resources: RootFrameResources<'a, RecordingC3dPipeline<'a>>,
        sources: &mut UploadedSourceStore<'_, RecordingBackend>,
        destination: u16,
        text_limits: TextLayoutLimits,
    ) -> RootFrameExecutionResult<BackendError, HookError, HookError, HookError> {
        let mut state = DeferredPaletteState::new();
        let empty = PaletteEntry {
            red: 0,
            green: 0,
            blue: 0,
            flags: 0,
        };
        let mut renderer_cache = [empty; 256];
        let mut global_cache = [empty; 256];
        super::execute_root_frame(
            input,
            resources,
            RootFramePaletteResources {
                bits_per_pixel: 32,
                arena: &[],
                state: &mut state,
                renderer_cache: &mut renderer_cache,
                global_cache: &mut global_cache,
            },
            sources,
            destination,
            text_limits,
        )
    }

    #[test]
    fn dispatches_complete_interactive_transcript_in_exact_order_without_reuploading() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut backend = RecordingBackend::new(Rc::clone(&events));
        {
            let mut sources = UploadedSourceStore::new(&mut backend);
            let fonts = UploadedFontPool::upload(&mut sources, font_set()).unwrap();
            let bitmaps = bitmap_pool(&mut sources, &[]);
            let c3d_assets = synthetic_root_menu_c3d_set([true; 6], VisualVariant::Normal);
            let mut c3d_angles = RootMenuC3dAngles::new();
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
                        &c3d_assets,
                        &mut c3d_angles,
                    ),
                    &mut sources,
                    99,
                    TextLayoutLimits::default(),
                )
                .unwrap();
                assert_eq!(outcome.bitmap_blits, 5);
                assert_eq!(outcome.bitmap_no_ops, 0);
                assert_eq!(outcome.c3d_commands, 1);
                assert!(outcome.scene_completed);
                assert!(outcome.presentation_completed);
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
                    scene_event(true),
                    c3d_event(0, 8),
                    present_event(),
                    Event::Blit(root_marker(RootMenuBitmapSlot::Backdrop)),
                    Event::Blit(root_marker(RootMenuBitmapSlot::MainShadow)),
                    Event::Blit(root_marker(RootMenuBitmapSlot::Animation0)),
                    Event::Blit(root_marker(RootMenuBitmapSlot::MainSelected)),
                    Event::Blit(font_marker(FontStyle::GreenInfo)),
                    Event::Blit(root_marker(RootMenuBitmapSlot::ChampionshipGlobe)),
                    scene_event(true),
                    c3d_event(8, 16),
                    present_event(),
                ]
            );
            assert_eq!(c3d_angles.angle(RootMenuC3dSlot::Championship), 16);
        }
    }

    #[test]
    fn terminal_scene_precedes_depth_eight_palette_commit_and_presentation() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut backend = RecordingBackend::new(Rc::clone(&events));
        let fail_palette = Rc::clone(&backend.fail_palette);
        {
            let mut sources = UploadedSourceStore::new(&mut backend);
            let fonts = UploadedFontPool::upload(&mut sources, font_set()).unwrap();
            let bitmaps = bitmap_pool(&mut sources, &[]);
            let c3d_assets = synthetic_root_menu_c3d_set([true; 6], VisualVariant::Normal);
            let mut c3d_angles = RootMenuC3dAngles::new();
            let arena = [palette(0x6b), palette(0x9d)];
            let mut state = DeferredPaletteState::new();
            assert_eq!(
                state.request(PaletteArenaIndex::new(0)),
                crate::deferred_palette::PaletteRequestOutcome::PendingUpdated
            );
            let mut renderer_cache = palette(0);
            let mut global_cache = palette(0);
            events.borrow_mut().clear();

            let outcome = super::execute_root_frame(
                interactive_input(),
                frame_resources(
                    &footer_table(b"A"),
                    &fonts,
                    &bitmaps,
                    &c3d_assets,
                    &mut c3d_angles,
                ),
                RootFramePaletteResources {
                    bits_per_pixel: 8,
                    arena: &arena,
                    state: &mut state,
                    renderer_cache: &mut renderer_cache,
                    global_cache: &mut global_cache,
                },
                &mut sources,
                99,
                TextLayoutLimits::default(),
            )
            .unwrap();

            assert_eq!(
                outcome.palette_commit,
                DeferredPaletteCommitOutcome::Committed(PaletteArenaIndex::new(0))
            );
            assert_eq!(state.current(), Some(PaletteArenaIndex::new(0)));
            assert_eq!(state.pending(), None);
            assert_eq!(renderer_cache, arena[0]);
            assert_eq!(global_cache, arena[0]);
            let recorded = events.borrow();
            assert_eq!(
                &recorded[recorded.len() - 4..],
                [
                    scene_event(true),
                    c3d_event(0, 8),
                    Event::PaletteUpload(0x6b),
                    present_event(),
                ]
            );
            drop(recorded);

            assert_eq!(
                state.request(PaletteArenaIndex::new(1)),
                crate::deferred_palette::PaletteRequestOutcome::PendingUpdated
            );
            fail_palette.set(true);
            events.borrow_mut().clear();
            let error = super::execute_root_frame(
                interactive_input(),
                frame_resources(
                    &footer_table(b"A"),
                    &fonts,
                    &bitmaps,
                    &c3d_assets,
                    &mut c3d_angles,
                ),
                RootFramePaletteResources {
                    bits_per_pixel: 8,
                    arena: &arena,
                    state: &mut state,
                    renderer_cache: &mut renderer_cache,
                    global_cache: &mut global_cache,
                },
                &mut sources,
                99,
                TextLayoutLimits::default(),
            )
            .unwrap_err();

            assert!(matches!(
                error,
                RootFrameError::PaletteCommit {
                    command_index: 7,
                    source: DeferredPaletteCommitError::Adapter(BackendError::Palette),
                }
            ));
            assert_eq!(state.current(), Some(PaletteArenaIndex::new(0)));
            assert_eq!(state.pending(), Some(PaletteArenaIndex::new(1)));
            assert_eq!(renderer_cache, arena[0]);
            assert_eq!(global_cache, arena[0]);
            let recorded = events.borrow();
            assert_eq!(
                &recorded[recorded.len() - 3..],
                [
                    scene_event(true),
                    c3d_event(8, 16),
                    Event::PaletteUpload(0x9d),
                ]
            );
            assert!(
                !recorded
                    .iter()
                    .any(|event| matches!(event, Event::Present { .. }))
            );
            drop(recorded);

            events.borrow_mut().clear();
            let outcome = super::execute_root_frame(
                interactive_input(),
                frame_resources(
                    &footer_table(b"A"),
                    &fonts,
                    &bitmaps,
                    &c3d_assets,
                    &mut c3d_angles,
                ),
                RootFramePaletteResources {
                    bits_per_pixel: 16,
                    arena: &arena,
                    state: &mut state,
                    renderer_cache: &mut renderer_cache,
                    global_cache: &mut global_cache,
                },
                &mut sources,
                99,
                TextLayoutLimits::default(),
            )
            .unwrap();
            assert_eq!(
                outcome.palette_commit,
                DeferredPaletteCommitOutcome::SkippedNonIndexedDepth
            );
            assert_eq!(state.current(), Some(PaletteArenaIndex::new(0)));
            assert_eq!(state.pending(), Some(PaletteArenaIndex::new(1)));
            assert_eq!(renderer_cache, arena[0]);
            assert_eq!(global_cache, arena[0]);
            let recorded = events.borrow();
            assert_eq!(
                &recorded[recorded.len() - 3..],
                [scene_event(true), c3d_event(16, 24), present_event()]
            );
            assert!(
                !recorded
                    .iter()
                    .any(|event| matches!(event, Event::PaletteUpload(_)))
            );
        }
    }

    #[test]
    fn frame_binding_keeps_selection_slots_and_angles_independent() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut backend = RecordingBackend::new(Rc::clone(&events));
        {
            let mut sources = UploadedSourceStore::new(&mut backend);
            let fonts = UploadedFontPool::upload(&mut sources, font_set()).unwrap();
            let bitmaps = bitmap_pool(&mut sources, &[]);
            let c3d_assets = synthetic_root_menu_c3d_set([true; 6], VisualVariant::Normal);
            let mut c3d_angles = RootMenuC3dAngles::new();
            events.borrow_mut().clear();

            execute_root_frame(
                interactive_input(),
                frame_resources(
                    &footer_table(b"A"),
                    &fonts,
                    &bitmaps,
                    &c3d_assets,
                    &mut c3d_angles,
                ),
                &mut sources,
                99,
                TextLayoutLimits::default(),
            )
            .unwrap();
            let mut rally_input = interactive_input();
            rally_input.selection = RootSelection::new(1).unwrap();
            for _ in 0..2 {
                execute_root_frame(
                    rally_input,
                    frame_resources(
                        &footer_table(b"A"),
                        &fonts,
                        &bitmaps,
                        &c3d_assets,
                        &mut c3d_angles,
                    ),
                    &mut sources,
                    99,
                    TextLayoutLimits::default(),
                )
                .unwrap();
            }

            let bindings: Vec<_> = events
                .borrow()
                .iter()
                .filter_map(|event| match event {
                    Event::C3d {
                        slot,
                        asset_marker,
                        previous_angle,
                        resulting_angle,
                        ..
                    } => Some((*slot, *asset_marker, *previous_angle, *resulting_angle)),
                    _ => None,
                })
                .collect();
            assert_eq!(
                bindings,
                [
                    (RootMenuC3dSlot::Championship, 0, 0, 8),
                    (RootMenuC3dSlot::Rally, 1, 0, 8),
                    (RootMenuC3dSlot::Rally, 1, 8, 16),
                ]
            );
            assert_eq!(c3d_angles.angle(RootMenuC3dSlot::Championship), 8);
            assert_eq!(c3d_angles.angle(RootMenuC3dSlot::Rally), 16);
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
            let c3d_assets = synthetic_root_menu_c3d_set([false; 6], VisualVariant::Normal);
            let mut c3d_angles = RootMenuC3dAngles::new();
            events.borrow_mut().clear();
            let mut input = interactive_input();
            input.horizontal_transform_q16 = 0x10000;
            input.vertical_transform_q16 = 0x8000;
            let outcome = execute_root_frame(
                input,
                frame_resources(
                    &footer_table(b"A"),
                    &fonts,
                    &bitmaps,
                    &c3d_assets,
                    &mut c3d_angles,
                ),
                &mut sources,
                99,
                TextLayoutLimits::default(),
            )
            .unwrap();
            assert_eq!(outcome.bitmap_blits, 2);
            assert_eq!(outcome.bitmap_no_ops, 1);
            assert_eq!(outcome.c3d_commands, 0);
            assert_eq!(c3d_angles.angle(RootMenuC3dSlot::Championship), 0);
            assert_eq!(
                events.borrow().as_slice(),
                [
                    Event::Blit(root_marker(RootMenuBitmapSlot::Backdrop)),
                    Event::Blit(root_marker(RootMenuBitmapSlot::MainSelected)),
                    Event::Blit(font_marker(FontStyle::GreenInfo)),
                    scene_event(false),
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
            let c3d_assets = synthetic_root_menu_c3d_set([true; 6], VisualVariant::Normal);
            let mut c3d_angles = RootMenuC3dAngles::new();
            events.borrow_mut().clear();
            assert!(matches!(
                execute_root_frame(
                    interactive_input(),
                    frame_resources(
                        &footer_table(b"A"),
                        &fonts,
                        &bitmaps,
                        &c3d_assets,
                        &mut c3d_angles,
                    ),
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
            assert_eq!(c3d_angles.angle(RootMenuC3dSlot::Championship), 0);
        }
    }

    #[test]
    fn bitmap_and_c3d_variant_mismatch_fails_before_any_frame_effect() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut backend = RecordingBackend::new(Rc::clone(&events));
        {
            let mut sources = UploadedSourceStore::new(&mut backend);
            let fonts = UploadedFontPool::upload(&mut sources, font_set()).unwrap();
            let bitmaps = bitmap_pool(&mut sources, &[]);
            let c3d_assets = synthetic_root_menu_c3d_set([true; 6], VisualVariant::GPrefixed);
            let mut c3d_angles = RootMenuC3dAngles::new();
            events.borrow_mut().clear();

            assert!(matches!(
                execute_root_frame(
                    interactive_input(),
                    frame_resources(
                        &footer_table(b"A"),
                        &fonts,
                        &bitmaps,
                        &c3d_assets,
                        &mut c3d_angles,
                    ),
                    &mut sources,
                    99,
                    TextLayoutLimits::default(),
                ),
                Err(RootFrameError::C3dVariantMismatch {
                    bitmap_variant: VisualVariant::Normal,
                    object_variant: VisualVariant::GPrefixed,
                })
            ));
            assert!(events.borrow().is_empty());
            assert_eq!(c3d_angles.angle(RootMenuC3dSlot::Championship), 0);
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
            let c3d_assets = synthetic_root_menu_c3d_set([true; 6], VisualVariant::Normal);
            let mut c3d_angles = RootMenuC3dAngles::new();
            events.borrow_mut().clear();
            let outcome = execute_root_frame(
                interactive_input(),
                frame_resources(
                    &footer_table(b"AA"),
                    &fonts,
                    &bitmaps,
                    &c3d_assets,
                    &mut c3d_angles,
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
                    scene_event(true),
                    c3d_event(0, 8),
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
                    &c3d_assets,
                    &mut c3d_angles,
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
            assert_eq!(c3d_angles.angle(RootMenuC3dSlot::Championship), 8);
        }
    }

    #[test]
    fn foreign_font_pool_is_typed_and_stops_before_center_scene_and_presentation() {
        let font_events = Rc::new(RefCell::new(Vec::new()));
        let mut font_backend = RecordingBackend::new(font_events);
        let mut font_sources = UploadedSourceStore::new(&mut font_backend);
        let fonts = UploadedFontPool::upload(&mut font_sources, font_set()).unwrap();

        let frame_events = Rc::new(RefCell::new(Vec::new()));
        let mut frame_backend = RecordingBackend::new(Rc::clone(&frame_events));
        {
            let mut frame_sources = UploadedSourceStore::new(&mut frame_backend);
            let bitmaps = bitmap_pool(&mut frame_sources, &[]);
            let c3d_assets = synthetic_root_menu_c3d_set([true; 6], VisualVariant::Normal);
            let mut c3d_angles = RootMenuC3dAngles::new();
            frame_events.borrow_mut().clear();
            let error = execute_root_frame(
                interactive_input(),
                frame_resources(
                    &footer_table(b"A"),
                    &fonts,
                    &bitmaps,
                    &c3d_assets,
                    &mut c3d_angles,
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
            assert_eq!(c3d_angles.angle(RootMenuC3dSlot::Championship), 0);
        }
    }

    #[test]
    fn fatal_bitmap_scene_and_presentation_failures_are_indexed_and_fail_fast() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut backend = RecordingBackend::new(Rc::clone(&events));
        let fail_markers = Rc::clone(&backend.fail_markers);
        let fail_c3d = Rc::clone(&backend.fail_c3d);
        let fail_final = Rc::clone(&backend.fail_final);
        {
            let mut sources = UploadedSourceStore::new(&mut backend);
            let fonts = UploadedFontPool::upload(&mut sources, font_set()).unwrap();
            let bitmaps = bitmap_pool(&mut sources, &[]);
            let c3d_assets = synthetic_root_menu_c3d_set([true; 6], VisualVariant::Normal);
            let mut c3d_angles = RootMenuC3dAngles::new();
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
                        &c3d_assets,
                        &mut c3d_angles,
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
            assert_eq!(c3d_angles.angle(RootMenuC3dSlot::Championship), 0);
            fail_markers.borrow_mut().clear();

            events.borrow_mut().clear();
            assert!(matches!(
                execute_root_frame(
                    interactive_input(),
                    RootFrameResources {
                        text: &footer_table(b"A"),
                        fonts: &fonts,
                        bitmaps: &bitmaps,
                        c3d: RecordingC3dPipeline {
                            assets: &c3d_assets,
                            angles: &mut c3d_angles,
                            fail_plan: true,
                        },
                    },
                    &mut sources,
                    99,
                    TextLayoutLimits::default(),
                ),
                Err(RootFrameError::C3dPlan {
                    command_index: 6,
                    source: HookError::C3d,
                })
            ));
            assert!(
                !events
                    .borrow()
                    .iter()
                    .any(|event| matches!(event, Event::Scene { .. } | Event::Present { .. }))
            );
            assert_eq!(c3d_angles.angle(RootMenuC3dSlot::Championship), 0);

            events.borrow_mut().clear();
            fail_c3d.set(true);
            assert!(matches!(
                execute_root_frame(
                    interactive_input(),
                    frame_resources(
                        &footer_table(b"A"),
                        &fonts,
                        &bitmaps,
                        &c3d_assets,
                        &mut c3d_angles,
                    ),
                    &mut sources,
                    99,
                    TextLayoutLimits::default(),
                ),
                Err(RootFrameError::C3dScene {
                    command_index: 7,
                    source: HookError::C3d
                })
            ));
            assert!(
                !events
                    .borrow()
                    .iter()
                    .any(|event| matches!(event, Event::Present { .. }))
            );
            assert_eq!(c3d_angles.angle(RootMenuC3dSlot::Championship), 8);
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
                        &c3d_assets,
                        &mut c3d_angles,
                    ),
                    &mut sources,
                    99,
                    TextLayoutLimits::default(),
                ),
                Err(RootFrameError::Presentation {
                    command_index: 7,
                    source: HookError::Present
                })
            ));
            assert!(matches!(
                events.borrow().last(),
                Some(Event::Present { .. })
            ));
            assert!(events.borrow().contains(&c3d_event(8, 16)));
            assert_eq!(c3d_angles.angle(RootMenuC3dSlot::Championship), 16);
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
            let c3d_assets = synthetic_root_menu_c3d_set([true; 6], VisualVariant::Normal);
            let mut c3d_angles = RootMenuC3dAngles::new();
            let outcome = execute_root_frame(
                input,
                frame_resources(
                    &footer_table(b"A"),
                    &fonts,
                    &bitmaps,
                    &c3d_assets,
                    &mut c3d_angles,
                ),
                &mut sources,
                99,
                TextLayoutLimits::default(),
            )
            .unwrap();
            assert_eq!(outcome.bitmap_blits, 1);
            assert!(outcome.footer.is_none());
            assert_eq!(outcome.c3d_commands, 0);
            assert_eq!(c3d_angles.angle(RootMenuC3dSlot::Championship), 0);
            assert_eq!(
                events.borrow().as_slice(),
                [
                    Event::Blit(root_marker(RootMenuBitmapSlot::Backdrop)),
                    scene_event(false),
                    present_event(),
                ]
            );
        }
    }

    fn frame_resources<'a>(
        text: &'a IndexedTextTable,
        fonts: &'a UploadedFontPool,
        bitmaps: &'a UploadedRootMenuBitmapPool,
        c3d_assets: &'a RootMenuC3dSet,
        c3d_angles: &'a mut RootMenuC3dAngles,
    ) -> RootFrameResources<'a, RecordingC3dPipeline<'a>> {
        RootFrameResources {
            text,
            fonts,
            bitmaps,
            c3d: RecordingC3dPipeline {
                assets: c3d_assets,
                angles: c3d_angles,
                fail_plan: false,
            },
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

    fn c3d_event(previous_angle: u16, resulting_angle: u16) -> Event {
        Event::C3d {
            destination: 99,
            extent: DisplayExtent {
                width: 640,
                height: 480,
            },
            slot: RootMenuC3dSlot::Championship,
            asset_marker: RootMenuC3dSlot::Championship.index() as u8,
            previous_angle,
            resulting_angle,
            delta: C3D_ANGLE_DELTA,
        }
    }

    fn scene_event(has_plan: bool) -> Event {
        Event::Scene {
            destination: 99,
            extent: DisplayExtent {
                width: 640,
                height: 480,
            },
            has_plan,
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
        let row_stride = (width + 3) & !3;
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
            row_stride,
            stored_pixels: vec![root_marker(slot); (row_stride * height) as usize],
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

    fn palette(red: u8) -> [PaletteEntry; 256] {
        [PaletteEntry {
            red,
            green: 2,
            blue: 3,
            flags: 0,
        }; 256]
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
