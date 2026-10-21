use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use cmr_assets::{
    AssetFile, AssetProfile, AssetStore, ReviewedProfile, StorageEncoding, import_source,
};
use cmr_core::{
    blit::BlitDecision,
    championship_assets::{CHAMPIONSHIP_CARS, MANUFACTURER_LOGOS, load_championship_assets},
    championship_bitmap_raster::{
        UploadedChampionshipLogoPool, prepare_championship_logo_surfaces, upload_championship_logos,
    },
    championship_car_execute::{ChampionshipCarBatchExecutor, ChampionshipCarPreDrawState},
    championship_car_transform::{ChampionshipCarInput, ChampionshipCarState},
    championship_car_visibility::ChampionshipCarVisibilityLatches,
    championship_frame_assets::{
        CHEQUERED_FRAME_DESCRIPTORS, CHEQUERED_PALETTE_DESCRIPTOR, ChequeredFrame,
        INFO_BOX_DESCRIPTOR, UploadedChampionshipFramePool, load_championship_frame_assets,
        prepare_championship_frame_surfaces, upload_championship_frame_surfaces,
    },
    championship_setup::{
        ActivePreview, CAR_ROW_TAG, CanonicalCar, ChampionshipSetupRenderTranscript, GEAR_ROW_TAG,
        GearChoice, NEXT_ROW_TAG, RenderRow, RowValue, SetupRowText,
    },
    championship_setup_frame::{
        ChampionshipBitmapIdentity, ChampionshipSetupFrameResources, ChampionshipSetupFrameState,
        DeferredChampionshipCommand, PreparedChampionshipSetupCommand,
        prepare_championship_setup_frame,
    },
    championship_setup_visual::{
        ChampionshipPreviewCache, ChampionshipSetupVisualInput, NextPreviewContext, Q16Point,
        RowPresentation,
    },
    championship_terminal_frame::scan_championship_car_contract_for_resource,
    deferred_palette::{DeferredPaletteState, PaletteArenaIndex, PaletteGroupBases},
    display::{CooperativeMode, DisplayExtent, IndexedMode, IndexedVideoBackend, SurfaceTopology},
    font_assets::FontSet,
    font_raster::UploadedFontPool,
    frontend_c3d_resources::prepare_frontend_c3d_resources,
    frontend_c3d_route::{FrontendC3dRoute, FrontendC3dRouteState},
    frontend_c3d_scene::C3dDepthClear,
    localization::IndexedTextTable,
    root_menu_assets::VisualVariant,
    root_menu_c3d_assets::load_root_menu_c3d,
    root_menu_c3d_render_state::{
        C3dGraphicsBranch, ROOT_MENU_C3D_INITIAL_FILTER_REQUEST_CACHE,
        root_menu_c3d_filter_cache_outcome, root_menu_c3d_legacy_alpha_contract,
    },
    root_menu_c3d_visibility::RootMenuC3dVisibilityLatches,
    source_store::{UploadedIndexedBitmap, UploadedSourceStore},
    surface::{IndexedSurface, PaletteMapping},
};
use cmr_formats::{
    bitmap::BitmapLimits,
    c3d::{
        C3D_ENTRY24_ENTRY32_ORDINAL_OFFSETS, C3D_FORMAT_VERSION, C3D_GROUP0_VALUES_OFFSET,
        C3D_GROUP1_ENTRY24_LEN, C3D_GROUP1_ENTRY32_LEN, C3D_GROUP1_FIXED_LEN, C3D_HEADER_LEN,
        C3D_MAGIC, C3dLimits,
    },
    font::{FontLimits, TextLayoutLimits, parse_legacy_font},
    riff_palette::PaletteEntry,
    stored_asset::DecodeLimits,
};
use cmr_video::{
    SoftwareIndexedBackend,
    c3d_software_raster::{SoftwareC3dSplitRenderDepth, SoftwareChampionshipCarSplitOracleProfile},
    c3d_software_split_backend::{
        SoftwareSplitC3dOracleBackend, SoftwareSplitC3dOracleConfiguration,
        prepare_software_split_frontend_c3d_bindings,
    },
    legacy_c3d_pixel_format::{KnownRgbLayout, RawDdPixelFormat, SelectedNonAlphaTextureFormat},
    packed_rgb_target::FramebufferRgbFormat,
    selected_texture_conversion_oracle::SourceOwnedTextureConversionOraclePolicy,
    software_championship_alternate_terminal::{
        SoftwareChampionshipAlternateCarResources, SoftwareChampionshipAlternateTerminalError,
        SoftwareChampionshipAlternateTerminalState,
        execute_software_championship_alternate_terminal_frame,
    },
    software_championship_ordinary_rgb16::{
        SoftwareChampionshipOrdinaryRgb16Pool, prepare_software_championship_ordinary_rgb16_pool,
    },
    software_championship_setup_frame::{
        SoftwareChampionshipBitmapOutcome, SoftwareChampionshipSetupCommandOutcome,
        SoftwareChampionshipSetupDeferredCommand,
    },
};
use sha2::{Digest, Sha256};

const EXTENT: DisplayExtent = DisplayExtent {
    width: 640,
    height: 480,
};
const Q16_ONE: i32 = 1 << 16;
const IDENTITY_BASIS: [i32; 9] = [Q16_ONE, 0, 0, 0, Q16_ONE, 0, 0, 0, Q16_ONE];
const RESOURCE_INDEX: u8 = 217;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TraceEvent {
    Deferred(usize),
    Bitmap(usize, ChampionshipBitmapIdentity),
    Coordinated(usize, DeferredKind),
    Scene,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DeferredKind {
    Transform,
    Latch,
    Palette,
}

#[test]
fn combined_terminal_preserves_setup_order_and_composes_visible_car_on_the_same_target() {
    with_fixture(|fixture| {
        let transcript = car_transcript();
        let mut preview_cache = ChampionshipPreviewCache::new();
        let prepared = fixture.prepare_setup(&transcript, preview_cache);
        let logo_origin =
            scan_championship_car_contract_for_resource(&prepared, Some(CanonicalCar::Subaru))
                .unwrap()
                .unwrap()
                .logo_origin;
        let info_probe = prepared
            .commands()
            .find_map(|command| match command {
                PreparedChampionshipSetupCommand::Bitmap {
                    identity: ChampionshipBitmapIdentity::InfoBox,
                    decision: BlitDecision::Blit(plan),
                    ..
                } => Some(
                    usize::try_from(plan.destination.bottom - 1).unwrap()
                        * usize::from(EXTENT.width)
                        + usize::try_from(plan.destination.right - 1).unwrap(),
                ),
                _ => None,
            })
            .unwrap();
        let mut transform = ChampionshipCarState::new();
        // The recovered state machine intentionally hides its first normal
        // frame; prime it so this scenario establishes the visible branch.
        transform
            .advance_normal_frame(
                ChampionshipCarInput {
                    f9: false,
                    f10: false,
                },
                IDENTITY_BASIS,
                logo_origin,
                EXTENT,
            )
            .unwrap();
        let transform_before = transform;
        let mut car_visibility = ChampionshipCarVisibilityLatches::new();
        let mut root_visibility = RootMenuC3dVisibilityLatches::new();
        let mut palette_state = DeferredPaletteState::new();
        let route = FrontendC3dRouteState::new();
        let target_before = fixture.backend.logical_target().words().to_vec();
        let depth_before = fixture.backend.depth_values().to_vec();

        let outcome = fixture
            .execute(
                prepared,
                &mut preview_cache,
                &mut transform,
                &mut car_visibility,
                &mut root_visibility,
                &mut palette_state,
                &route,
            )
            .unwrap();

        let mut trace = outcome
            .setup
            .commands
            .iter()
            .map(trace_setup)
            .collect::<Vec<_>>();
        trace.push(TraceEvent::Scene);
        assert_eq!(
            trace,
            [
                TraceEvent::Deferred(0),
                TraceEvent::Deferred(1),
                TraceEvent::Deferred(2),
                TraceEvent::Bitmap(3, ChampionshipBitmapIdentity::InfoBox),
                TraceEvent::Deferred(4),
                TraceEvent::Coordinated(5, DeferredKind::Transform),
                TraceEvent::Coordinated(6, DeferredKind::Latch),
                TraceEvent::Bitmap(
                    7,
                    ChampionshipBitmapIdentity::ManufacturerLogo(
                        cmr_core::championship_assets::ManufacturerLogo::Subaru,
                    ),
                ),
                TraceEvent::Coordinated(8, DeferredKind::Palette),
                TraceEvent::Deferred(9),
                TraceEvent::Deferred(10),
                TraceEvent::Scene,
            ]
        );
        let submission = outcome.scene.submission.unwrap();
        assert_eq!(submission.draws, 3);
        assert_ne!(fixture.backend.logical_target().words(), target_before);
        assert_ne!(fixture.backend.depth_values(), depth_before);
        assert!(
            fixture
                .backend
                .depth_values()
                .iter()
                .any(|depth| *depth < 1.0)
        );
        // The far corner of the info box is outside the small synthetic car
        // triangle, proving that setup and C3D share one persistent target.
        assert_ne!(fixture.backend.logical_target().words()[info_probe], 0);
        assert_ne!(preview_cache, ChampionshipPreviewCache::new());
        assert_ne!(transform, transform_before);
        assert_eq!(palette_state.pending(), Some(PaletteArenaIndex::new(0)));
        assert_eq!(
            outcome.palette_commit,
            cmr_core::deferred_palette::DeferredPaletteCommitOutcome::SkippedNonIndexedDepth
        );
        assert_eq!(route.active(), Some(FrontendC3dRoute::ChampionshipCar));
        assert!(
            CanonicalCar::ALL
                .into_iter()
                .all(|car| car_visibility.mask(car).raw() == 0)
        );
        assert!(
            cmr_core::root_menu_c3d_assets::RootMenuC3dSlot::ALL
                .into_iter()
                .all(|slot| root_visibility.mask(slot).raw() == 0)
        );
    });
}

#[test]
fn combined_terminal_hidden_then_repeated_visible_frames_keep_cache_and_binding_contracts() {
    with_fixture(|fixture| {
        let transcript = car_transcript();
        let mut preview_cache = ChampionshipPreviewCache::new();
        let mut transform = ChampionshipCarState::new();
        let mut car_visibility = ChampionshipCarVisibilityLatches::new();
        let mut root_visibility = RootMenuC3dVisibilityLatches::new();
        let mut palette_state = DeferredPaletteState::new();
        let route = FrontendC3dRouteState::new();

        let hidden = fixture
            .execute(
                fixture.prepare_setup(&transcript, preview_cache),
                &mut preview_cache,
                &mut transform,
                &mut car_visibility,
                &mut root_visibility,
                &mut palette_state,
                &route,
            )
            .unwrap();
        assert_eq!(hidden.scene.submission.unwrap().draws, 0);
        assert!(
            fixture
                .backend
                .depth_values()
                .iter()
                .all(|depth| *depth == 1.0)
        );
        assert!(hidden.setup.commands.iter().any(is_stretch));
        assert!(hidden.setup.commands.iter().any(is_info_text));
        assert!(hidden.setup.commands.iter().any(is_palette_request));

        let first_visible = fixture
            .execute(
                fixture.prepare_setup(&transcript, preview_cache),
                &mut preview_cache,
                &mut transform,
                &mut car_visibility,
                &mut root_visibility,
                &mut palette_state,
                &route,
            )
            .unwrap();
        assert_eq!(first_visible.scene.submission.unwrap().bindings, 3);
        assert!(!first_visible.setup.commands.iter().any(is_palette_request));

        let repeated = fixture
            .execute(
                fixture.prepare_setup(&transcript, preview_cache),
                &mut preview_cache,
                &mut transform,
                &mut car_visibility,
                &mut root_visibility,
                &mut palette_state,
                &route,
            )
            .unwrap();
        let submission = repeated.scene.submission.unwrap();
        assert_eq!(submission.draws, 3);
        assert_eq!(submission.bindings, 2);
        assert!(!repeated.setup.commands.iter().any(is_palette_request));
    });
}

#[test]
fn complete_preflight_failure_is_effect_free_and_a_valid_frame_recovers() {
    with_fixture(|fixture| {
        let transcript = car_transcript();
        let initial_cache = ChampionshipPreviewCache::new();
        let mut preview_cache = initial_cache;
        let mut transform = ChampionshipCarState::new();
        let transform_before = transform;
        let mut car_visibility = ChampionshipCarVisibilityLatches::new();
        let mut root_visibility = RootMenuC3dVisibilityLatches::new();
        let mut palette_state = DeferredPaletteState::new();
        let route = FrontendC3dRouteState::new();
        let words_before = fixture.backend.logical_target().words().to_vec();
        let depth_before = fixture.backend.depth_values().to_vec();
        let foreign_destination = split_backend(false).1;

        let error = fixture
            .execute_with_destination(
                fixture.prepare_setup(&transcript, preview_cache),
                foreign_destination,
                &mut preview_cache,
                &mut transform,
                &mut car_visibility,
                &mut root_visibility,
                &mut palette_state,
                &route,
            )
            .unwrap_err();
        assert!(matches!(
            error,
            SoftwareChampionshipAlternateTerminalError::Setup(_)
        ));
        assert_eq!(fixture.backend.logical_target().words(), words_before);
        assert_eq!(fixture.backend.depth_values(), depth_before);
        assert_eq!(preview_cache, initial_cache);
        assert_eq!(transform, transform_before);
        assert_eq!(palette_state.pending(), None);
        assert_eq!(route.active(), None);
        assert!(
            CanonicalCar::ALL
                .into_iter()
                .all(|car| car_visibility.mask(car).raw() == 0)
        );
        assert!(
            cmr_core::root_menu_c3d_assets::RootMenuC3dSlot::ALL
                .into_iter()
                .all(|slot| root_visibility.mask(slot).raw() == 0)
        );

        fixture
            .execute(
                fixture.prepare_setup(&transcript, preview_cache),
                &mut preview_cache,
                &mut transform,
                &mut car_visibility,
                &mut root_visibility,
                &mut palette_state,
                &route,
            )
            .unwrap();
        assert_ne!(fixture.backend.logical_target().words(), words_before);
    });
}

#[test]
fn c3d_preflight_failure_keeps_setup_state_uncommitted_and_recovers_on_same_owner() {
    with_fixture(|fixture| {
        let transcript = next_transcript();
        let initial_cache = ChampionshipPreviewCache::new();
        let mut preview_cache = initial_cache;
        let mut transform = ChampionshipCarState::new();
        let transform_before = transform;
        let mut car_visibility = ChampionshipCarVisibilityLatches::new();
        let mut root_visibility = RootMenuC3dVisibilityLatches::new();
        let mut palette_state = DeferredPaletteState::new();
        let route = FrontendC3dRouteState::new();
        let words_before = fixture.backend.logical_target().words().to_vec();
        let depth_before = fixture.backend.depth_values().to_vec();

        let rejected = fixture.prepare_offscreen_next_setup(&transcript, preview_cache);
        assert!(rejected.commands().all(|command| !matches!(
            command,
            PreparedChampionshipSetupCommand::Bitmap {
                decision: BlitDecision::Blit(_),
                ..
            }
        )));
        let error = fixture
            .execute_no_car_with_options(
                rejected,
                fixture.destination,
                C3dDepthClear::Skip,
                &mut preview_cache,
                &mut transform,
                &mut car_visibility,
                &mut root_visibility,
                &mut palette_state,
                &route,
            )
            .unwrap_err();
        assert!(matches!(
            error,
            SoftwareChampionshipAlternateTerminalError::Scene { .. }
        ));
        assert_eq!(fixture.backend.logical_target().words(), words_before);
        assert_eq!(fixture.backend.depth_values(), depth_before);
        assert_eq!(preview_cache, initial_cache);
        assert_eq!(transform, transform_before);
        assert_eq!(palette_state.pending(), None);
        // Like the indexed owner, route ownership is selected before C3D
        // preflight; no visual/setup state has committed yet.
        assert_eq!(route.active(), Some(FrontendC3dRoute::ChampionshipCar));

        let recovery = fixture
            .execute_no_car(
                fixture.prepare_offscreen_next_setup(&transcript, preview_cache),
                &mut preview_cache,
                &mut transform,
                &mut car_visibility,
                &mut root_visibility,
                &mut palette_state,
                &route,
            )
            .unwrap();
        assert_eq!(recovery.scene.submission, None);
        assert_eq!(fixture.backend.logical_target().words(), words_before);
        assert_eq!(fixture.backend.depth_values(), depth_before);
        assert_ne!(preview_cache, initial_cache);
        assert_eq!(palette_state.pending(), Some(PaletteArenaIndex::new(10)));
    });
}

#[test]
fn next_preview_blits_chequered_frame_then_runs_empty_common_scene_and_skips_palette_upload() {
    with_fixture(|fixture| {
        let car = car_transcript();
        let mut preview_cache = ChampionshipPreviewCache::new();
        let mut transform = ChampionshipCarState::new();
        let mut car_visibility = ChampionshipCarVisibilityLatches::new();
        let mut root_visibility = RootMenuC3dVisibilityLatches::new();
        let mut palette_state = DeferredPaletteState::new();
        let route = FrontendC3dRouteState::new();

        // Drive through the intentional hidden frame and one visible frame so
        // the Next frame has real depth to clear.
        for _ in 0..2 {
            fixture
                .execute(
                    fixture.prepare_setup(&car, preview_cache),
                    &mut preview_cache,
                    &mut transform,
                    &mut car_visibility,
                    &mut root_visibility,
                    &mut palette_state,
                    &route,
                )
                .unwrap();
        }
        assert!(
            fixture
                .backend
                .depth_values()
                .iter()
                .any(|depth| *depth < 1.0)
        );

        let next = next_transcript();
        let outcome = fixture
            .execute_no_car(
                fixture.prepare_next_setup(&next, preview_cache),
                &mut preview_cache,
                &mut transform,
                &mut car_visibility,
                &mut root_visibility,
                &mut palette_state,
                &route,
            )
            .unwrap();

        assert_eq!(
            outcome
                .setup
                .commands
                .iter()
                .map(trace_setup)
                .collect::<Vec<_>>(),
            [
                TraceEvent::Deferred(0),
                TraceEvent::Deferred(1),
                TraceEvent::Deferred(2),
                TraceEvent::Deferred(3),
                TraceEvent::Deferred(4),
                TraceEvent::Bitmap(5, ChampionshipBitmapIdentity::InfoBox),
                TraceEvent::Deferred(6),
                TraceEvent::Coordinated(7, DeferredKind::Palette),
                TraceEvent::Bitmap(
                    8,
                    ChampionshipBitmapIdentity::ChequeredFrame(ChequeredFrame::Frame04),
                ),
            ]
        );
        assert_eq!(outcome.scene.submission, None);
        assert!(outcome.scene.depth_cleared);
        assert!(
            fixture
                .backend
                .depth_values()
                .iter()
                .all(|depth| *depth == 1.0)
        );
        assert!(outcome.setup.commands.iter().any(|command| matches!(
            command,
            SoftwareChampionshipSetupCommandOutcome::ExecutedBitmap {
                identity: ChampionshipBitmapIdentity::ChequeredFrame(ChequeredFrame::Frame04),
                outcome: SoftwareChampionshipBitmapOutcome::Blitted,
                ..
            }
        )));
        assert_eq!(
            outcome.palette_commit,
            cmr_core::deferred_palette::DeferredPaletteCommitOutcome::SkippedNonIndexedDepth
        );
        assert_eq!(palette_state.pending(), Some(PaletteArenaIndex::new(10)));
        assert!(outcome.setup.commands.iter().any(is_stretch));
        assert!(outcome.setup.commands.iter().any(is_info_text));
        assert!(outcome.setup.commands.iter().any(is_palette_request));
    });
}

fn trace_setup(command: &SoftwareChampionshipSetupCommandOutcome) -> TraceEvent {
    match *command {
        SoftwareChampionshipSetupCommandOutcome::ExecutedBitmap {
            command_index,
            identity,
            outcome: SoftwareChampionshipBitmapOutcome::Blitted,
        } => TraceEvent::Bitmap(command_index, identity),
        SoftwareChampionshipSetupCommandOutcome::ExecutedBitmap { .. } => {
            panic!("fixture expects every ordinary bitmap to intersect")
        }
        SoftwareChampionshipSetupCommandOutcome::CoordinatedChampionship {
            command_index,
            command: DeferredChampionshipCommand::CarTransform(_),
        } => TraceEvent::Coordinated(command_index, DeferredKind::Transform),
        SoftwareChampionshipSetupCommandOutcome::CoordinatedChampionship {
            command_index,
            command: DeferredChampionshipCommand::CarVisibilityLatch(_),
        } => TraceEvent::Coordinated(command_index, DeferredKind::Latch),
        SoftwareChampionshipSetupCommandOutcome::CoordinatedChampionship {
            command_index,
            command: DeferredChampionshipCommand::PaletteRequest { .. },
        } => TraceEvent::Coordinated(command_index, DeferredKind::Palette),
        SoftwareChampionshipSetupCommandOutcome::CoordinatedChampionship { .. } => {
            panic!("terminal coordinates only transform, latch, and palette boundaries")
        }
        SoftwareChampionshipSetupCommandOutcome::Deferred { command_index, .. } => {
            TraceEvent::Deferred(command_index)
        }
    }
}

fn is_stretch(command: &SoftwareChampionshipSetupCommandOutcome) -> bool {
    matches!(
        command,
        SoftwareChampionshipSetupCommandOutcome::Deferred {
            command: SoftwareChampionshipSetupDeferredCommand::Bitmap {
                identity: ChampionshipBitmapIdentity::Selection,
                ..
            },
            ..
        }
    )
}

fn is_info_text(command: &SoftwareChampionshipSetupCommandOutcome) -> bool {
    matches!(
        command,
        SoftwareChampionshipSetupCommandOutcome::Deferred {
            command: SoftwareChampionshipSetupDeferredCommand::Championship(
                DeferredChampionshipCommand::InfoText(_)
            ),
            ..
        }
    )
}

fn is_palette_request(command: &SoftwareChampionshipSetupCommandOutcome) -> bool {
    matches!(
        command,
        SoftwareChampionshipSetupCommandOutcome::CoordinatedChampionship {
            command: DeferredChampionshipCommand::PaletteRequest { .. },
            ..
        }
    )
}

struct Fixture<'a> {
    topology: &'a cmr_core::c3d_topology::C3dObjectTopology<'a>,
    runtime: &'a cmr_core::root_menu_c3d_runtime::C3dResourceScaledObject,
    fonts: &'a UploadedFontPool,
    logos: &'a UploadedChampionshipLogoPool,
    frames: &'a UploadedChampionshipFramePool,
    selection: UploadedIndexedBitmap,
    text: IndexedTextTable,
    pool: &'a SoftwareChampionshipOrdinaryRgb16Pool,
    executor: ChampionshipCarBatchExecutor<
        'a,
        cmr_video::c3d_software_split_backend::SoftwareSplitC3dBinding,
    >,
    backend: SoftwareSplitC3dOracleBackend,
    destination: cmr_video::c3d_software_split_backend::SoftwareSplitC3dTargetHandle,
}

impl Fixture<'_> {
    fn prepare_setup(
        &self,
        transcript: &ChampionshipSetupRenderTranscript,
        cache: ChampionshipPreviewCache,
    ) -> cmr_core::championship_setup_frame::PreparedChampionshipSetupFrame {
        prepare_championship_setup_frame(
            visual_input(transcript),
            ChampionshipSetupFrameState::default(),
            ChampionshipSetupFrameResources {
                text: &self.text,
                fonts: self.fonts,
                selection: Some(self.selection),
                logos: self.logos,
                frames: self.frames,
            },
            cache,
            TextLayoutLimits::default(),
        )
        .unwrap()
    }

    fn prepare_next_setup(
        &self,
        transcript: &ChampionshipSetupRenderTranscript,
        cache: ChampionshipPreviewCache,
    ) -> cmr_core::championship_setup_frame::PreparedChampionshipSetupFrame {
        self.prepare_setup_from_input(
            visual_input(transcript),
            ChampionshipSetupFrameState {
                chequered_frame: Some(ChequeredFrame::Frame04),
            },
            cache,
        )
    }

    fn prepare_offscreen_next_setup(
        &self,
        transcript: &ChampionshipSetupRenderTranscript,
        cache: ChampionshipPreviewCache,
    ) -> cmr_core::championship_setup_frame::PreparedChampionshipSetupFrame {
        let mut input = visual_input(transcript);
        input.frontend_offset_q16 = Q16Point::new(1000 << 16, 1000 << 16);
        self.prepare_setup_from_input(input, ChampionshipSetupFrameState::default(), cache)
    }

    fn prepare_setup_from_input(
        &self,
        input: ChampionshipSetupVisualInput<'_>,
        state: ChampionshipSetupFrameState,
        cache: ChampionshipPreviewCache,
    ) -> cmr_core::championship_setup_frame::PreparedChampionshipSetupFrame {
        prepare_championship_setup_frame(
            input,
            state,
            ChampionshipSetupFrameResources {
                text: &self.text,
                fonts: self.fonts,
                selection: Some(self.selection),
                logos: self.logos,
                frames: self.frames,
            },
            cache,
            TextLayoutLimits::default(),
        )
        .unwrap()
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_no_car(
        &mut self,
        prepared: cmr_core::championship_setup_frame::PreparedChampionshipSetupFrame,
        cache: &mut ChampionshipPreviewCache,
        transform: &mut ChampionshipCarState,
        car_visibility: &mut ChampionshipCarVisibilityLatches,
        root_visibility: &mut RootMenuC3dVisibilityLatches,
        palette_state: &mut DeferredPaletteState,
        route: &FrontendC3dRouteState,
    ) -> Result<
        cmr_video::software_championship_alternate_terminal::SoftwareChampionshipAlternateTerminalOutcome,
        SoftwareChampionshipAlternateTerminalError,
    >{
        self.execute_no_car_with_options(
            prepared,
            self.destination,
            C3dDepthClear::Clear,
            cache,
            transform,
            car_visibility,
            root_visibility,
            palette_state,
            route,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_no_car_with_options(
        &mut self,
        prepared: cmr_core::championship_setup_frame::PreparedChampionshipSetupFrame,
        destination: cmr_video::c3d_software_split_backend::SoftwareSplitC3dTargetHandle,
        depth_clear: C3dDepthClear,
        cache: &mut ChampionshipPreviewCache,
        transform: &mut ChampionshipCarState,
        car_visibility: &mut ChampionshipCarVisibilityLatches,
        root_visibility: &mut RootMenuC3dVisibilityLatches,
        palette_state: &mut DeferredPaletteState,
        route: &FrontendC3dRouteState,
    ) -> Result<
        cmr_video::software_championship_alternate_terminal::SoftwareChampionshipAlternateTerminalOutcome,
        SoftwareChampionshipAlternateTerminalError,
    >{
        execute_software_championship_alternate_terminal_frame(
            prepared,
            EXTENT,
            ChampionshipCarInput {
                f9: false,
                f10: false,
            },
            None,
            SoftwareChampionshipAlternateTerminalState {
                preview_cache: cache,
                car_transform: transform,
                car_visibility,
                root_visibility,
                palette_state,
                palette_group_bases: PaletteGroupBases {
                    group_1: 1,
                    group_2: 2,
                    group_3: 3,
                    group_4: 0,
                    group_5: 4,
                },
                route,
            },
            &mut self.executor,
            self.pool,
            &mut self.backend,
            destination,
            depth_clear,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn execute(
        &mut self,
        prepared: cmr_core::championship_setup_frame::PreparedChampionshipSetupFrame,
        cache: &mut ChampionshipPreviewCache,
        transform: &mut ChampionshipCarState,
        car_visibility: &mut ChampionshipCarVisibilityLatches,
        root_visibility: &mut RootMenuC3dVisibilityLatches,
        palette_state: &mut DeferredPaletteState,
        route: &FrontendC3dRouteState,
    ) -> Result<cmr_video::software_championship_alternate_terminal::SoftwareChampionshipAlternateTerminalOutcome, SoftwareChampionshipAlternateTerminalError>{
        self.execute_with_options(
            prepared,
            self.destination,
            C3dDepthClear::Clear,
            cache,
            transform,
            car_visibility,
            root_visibility,
            palette_state,
            route,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_with_destination(
        &mut self,
        prepared: cmr_core::championship_setup_frame::PreparedChampionshipSetupFrame,
        destination: cmr_video::c3d_software_split_backend::SoftwareSplitC3dTargetHandle,
        cache: &mut ChampionshipPreviewCache,
        transform: &mut ChampionshipCarState,
        car_visibility: &mut ChampionshipCarVisibilityLatches,
        root_visibility: &mut RootMenuC3dVisibilityLatches,
        palette_state: &mut DeferredPaletteState,
        route: &FrontendC3dRouteState,
    ) -> Result<cmr_video::software_championship_alternate_terminal::SoftwareChampionshipAlternateTerminalOutcome, SoftwareChampionshipAlternateTerminalError>{
        self.execute_with_options(
            prepared,
            destination,
            C3dDepthClear::Clear,
            cache,
            transform,
            car_visibility,
            root_visibility,
            palette_state,
            route,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_with_options(
        &mut self,
        prepared: cmr_core::championship_setup_frame::PreparedChampionshipSetupFrame,
        destination: cmr_video::c3d_software_split_backend::SoftwareSplitC3dTargetHandle,
        depth_clear: C3dDepthClear,
        cache: &mut ChampionshipPreviewCache,
        transform: &mut ChampionshipCarState,
        car_visibility: &mut ChampionshipCarVisibilityLatches,
        root_visibility: &mut RootMenuC3dVisibilityLatches,
        palette_state: &mut DeferredPaletteState,
        route: &FrontendC3dRouteState,
    ) -> Result<
        cmr_video::software_championship_alternate_terminal::SoftwareChampionshipAlternateTerminalOutcome,
        SoftwareChampionshipAlternateTerminalError,
    >{
        execute_software_championship_alternate_terminal_frame(
            prepared,
            EXTENT,
            ChampionshipCarInput {
                f9: false,
                f10: false,
            },
            Some(SoftwareChampionshipAlternateCarResources {
                car: CanonicalCar::Subaru,
                topology: self.topology,
                runtime: self.runtime,
                base_basis: IDENTITY_BASIS,
                pre_draw_state: ChampionshipCarPreDrawState::DISABLED,
            }),
            SoftwareChampionshipAlternateTerminalState {
                preview_cache: cache,
                car_transform: transform,
                car_visibility,
                root_visibility,
                palette_state,
                palette_group_bases: PaletteGroupBases {
                    group_1: 1,
                    group_2: 2,
                    group_3: 3,
                    group_4: 0,
                    group_5: 4,
                },
                route,
            },
            &mut self.executor,
            self.pool,
            &mut self.backend,
            destination,
            depth_clear,
        )
    }
}

fn with_fixture(run: impl FnOnce(&mut Fixture<'_>)) {
    let temporary = TestDirectory::new();
    let store = synthetic_store(&temporary);
    let championship =
        load_championship_assets(&store, C3dLimits::default(), BitmapLimits::default()).unwrap();
    let frame_assets = load_championship_frame_assets(&store, BitmapLimits::default()).unwrap();
    let pool =
        prepare_software_championship_ordinary_rgb16_pool(&championship, &frame_assets, 5).unwrap();
    let root = load_root_menu_c3d(&store, VisualVariant::Normal, C3dLimits::default());
    let frontend =
        prepare_frontend_c3d_resources(&championship, &root, BitmapLimits::default()).unwrap();
    let topology = frontend
        .car_topology(&championship, CanonicalCar::Subaru)
        .unwrap();
    let runtime = frontend
        .materialize_car(CanonicalCar::Subaru, &topology)
        .unwrap();
    let descriptor = RawDdPixelFormat::from_dwords([32, 0x40, 0, 16, 0x7c00, 0x03e0, 0x001f, 0]);
    let catalog = prepare_software_split_frontend_c3d_bindings(
        &frontend,
        SelectedNonAlphaTextureFormat::OpaqueRgb16 {
            descriptor,
            layout: KnownRgbLayout::Rgb555,
        },
        SourceOwnedTextureConversionOraclePolicy::ExactPaletteOrNearestMaskNoDither,
    )
    .unwrap();
    let executor = ChampionshipCarBatchExecutor::new(&catalog);

    let active_palette = &championship.car(CanonicalCar::Subaru).palette;
    let (mut indexed, _surfaces) = initialized_indexed_backend();
    let mut uploaded = UploadedSourceStore::new(&mut indexed);
    let fonts = UploadedFontPool::upload(&mut uploaded, synthetic_fonts()).unwrap();
    let logos = upload_championship_logos(
        &mut uploaded,
        prepare_championship_logo_surfaces(&championship, active_palette, PaletteMapping::Identity)
            .unwrap(),
    )
    .unwrap();
    let frames = upload_championship_frame_surfaces(
        &mut uploaded,
        prepare_championship_frame_surfaces(
            &frame_assets,
            active_palette,
            PaletteMapping::Identity,
        )
        .unwrap(),
    )
    .unwrap();
    let selection = uploaded
        .upload_bitmap(&IndexedSurface {
            width: 256,
            height: 256,
            pitch: 256,
            pixels: vec![1; 256 * 256],
            palette: active_palette.entries,
        })
        .unwrap();
    let text = IndexedTextTable::parse(&b"A\n".repeat(0xfe), 0xfe).unwrap();
    let (backend, destination) = split_backend(false);
    let mut fixture = Fixture {
        topology: &topology,
        runtime: &runtime,
        fonts: &fonts,
        logos: &logos,
        frames: &frames,
        selection,
        text,
        pool: &pool,
        executor,
        backend,
        destination,
    };
    run(&mut fixture);
    drop(fixture);
    drop(uploaded);
}

fn initialized_indexed_backend() -> (
    SoftwareIndexedBackend,
    cmr_core::display::DisplaySurfaces<cmr_video::SurfaceHandle>,
) {
    let mut backend = SoftwareIndexedBackend::default();
    backend.create_device().unwrap();
    backend
        .set_cooperative_mode(CooperativeMode::Windowed)
        .unwrap();
    let surfaces = backend
        .create_surfaces(
            SurfaceTopology::PrimaryAndOffscreen,
            IndexedMode {
                extent: EXTENT,
                bits_per_pixel: 8,
            },
        )
        .unwrap();
    let entries = [PaletteEntry {
        red: 0,
        green: 0,
        blue: 0,
        flags: 0,
    }; 256];
    let palette = backend.create_palette(&entries).unwrap();
    backend
        .attach_palette(surfaces.visible_front, palette)
        .unwrap();
    backend.attach_palette(surfaces.back, palette).unwrap();
    backend.upload_palette(palette, &entries).unwrap();
    (backend, surfaces)
}

fn split_backend(
    rgb565: bool,
) -> (
    SoftwareSplitC3dOracleBackend,
    cmr_video::c3d_software_split_backend::SoftwareSplitC3dTargetHandle,
) {
    let masks = if rgb565 {
        [0xf800, 0x07e0, 0x001f]
    } else {
        [0x7c00, 0x03e0, 0x001f]
    };
    let format =
        FramebufferRgbFormat::from_runtime_surface_descriptor(RawDdPixelFormat::from_dwords([
            32, 0x40, 0, 16, masks[0], masks[1], masks[2], 0,
        ]))
        .unwrap();
    let profile = SoftwareChampionshipCarSplitOracleProfile::new(
        SoftwareC3dSplitRenderDepth::Bits32,
        false,
        false,
    );
    let backend = SoftwareSplitC3dOracleBackend::new(
        EXTENT,
        format,
        SoftwareSplitC3dOracleConfiguration {
            profile,
            alpha_contract: root_menu_c3d_legacy_alpha_contract(0),
            filter_cache_outcome: root_menu_c3d_filter_cache_outcome(
                C3dGraphicsBranch::Alternate,
                true,
                ROOT_MENU_C3D_INITIAL_FILTER_REQUEST_CACHE,
            ),
        },
    )
    .unwrap();
    let destination = backend.target_handle();
    (backend, destination)
}

fn visual_input(
    transcript: &ChampionshipSetupRenderTranscript,
) -> ChampionshipSetupVisualInput<'_> {
    ChampionshipSetupVisualInput {
        viewport: EXTENT,
        frontend_offset_q16: Q16Point::new(0, 0),
        row_anchors_q16: [
            Q16Point::new(0x2000, 0x2000),
            Q16Point::new(0x2000, 0x4000),
            Q16Point::new(0x2000, 0x6000),
        ],
        preview_anchor_q16: Q16Point::new(0x2000, 0x4000),
        rows: [RowPresentation {
            visible: true,
            enabled: true,
        }; 3],
        selected_text_widths: [100, 80, 60],
        transcript,
        next_context: NextPreviewContext {
            state_byte: 0,
            mode_byte: 0,
        },
    }
}

fn car_transcript() -> ChampionshipSetupRenderTranscript {
    ChampionshipSetupRenderTranscript {
        title_text_index: 0x13,
        rows: [
            RenderRow {
                tag: CAR_ROW_TAG,
                text: SetupRowText::ChooseCar,
                selected: true,
                value: RowValue::Car(CanonicalCar::Subaru),
            },
            RenderRow {
                tag: GEAR_ROW_TAG,
                text: SetupRowText::ChooseGear,
                selected: false,
                value: RowValue::Gear(GearChoice::BitClear),
            },
            RenderRow {
                tag: NEXT_ROW_TAG,
                text: SetupRowText::Localized(0x18),
                selected: false,
                value: RowValue::None,
            },
        ],
        highlight_row: 0,
        preview: ActivePreview::Car {
            canonical_car: CanonicalCar::Subaru,
            object_name: CanonicalCar::Subaru.object_name(),
            logo_name: CanonicalCar::Subaru.logo_name(),
        },
    }
}

fn next_transcript() -> ChampionshipSetupRenderTranscript {
    ChampionshipSetupRenderTranscript {
        title_text_index: 0x13,
        rows: [
            RenderRow {
                tag: CAR_ROW_TAG,
                text: SetupRowText::ChooseCar,
                selected: false,
                value: RowValue::Car(CanonicalCar::Subaru),
            },
            RenderRow {
                tag: GEAR_ROW_TAG,
                text: SetupRowText::ChooseGear,
                selected: false,
                value: RowValue::Gear(GearChoice::BitClear),
            },
            RenderRow {
                tag: NEXT_ROW_TAG,
                text: SetupRowText::Localized(0x18),
                selected: true,
                value: RowValue::None,
            },
        ],
        highlight_row: 2,
        preview: ActivePreview::Next,
    }
}

fn synthetic_fonts() -> FontSet {
    FontSet::from_ordered_fonts(std::array::from_fn(|index| {
        parse_legacy_font(&synthetic_font(index as u8), FontLimits::default()).unwrap()
    }))
}

fn synthetic_font(identity: u8) -> Vec<u8> {
    const TABLE_OFFSET: usize = 0x436;
    const HEADER_LEN: usize = 24;
    let mut bytes = vec![0; TABLE_OFFSET + 4 + HEADER_LEN + 16];
    bytes[0..2].copy_from_slice(b"BM");
    bytes[18..22].copy_from_slice(&1i32.to_le_bytes());
    bytes[22..26].copy_from_slice(&1i32.to_le_bytes());
    bytes[28..30].copy_from_slice(&8u16.to_le_bytes());
    bytes[TABLE_OFFSET] = identity + 1;
    let table = TABLE_OFFSET + 4;
    bytes[table] = identity;
    bytes[table + 1] = 1;
    bytes[table + 4..table + 8].copy_from_slice(&1u32.to_le_bytes());
    bytes[table + 12..table + 16].copy_from_slice(&1u32.to_le_bytes());
    bytes[table + HEADER_LEN] = b'A';
    bytes[table + HEADER_LEN + 6..table + HEADER_LEN + 8].copy_from_slice(&1i16.to_le_bytes());
    bytes[table + HEADER_LEN + 8..table + HEADER_LEN + 10].copy_from_slice(&1i16.to_le_bytes());
    bytes
}

fn synthetic_store(temporary: &TestDirectory) -> AssetStore {
    let source = temporary.0.join("source");
    let mut files = BTreeMap::new();
    for descriptor in CHAMPIONSHIP_CARS {
        let resource_count = if descriptor.car == CanonicalCar::M4r {
            3
        } else {
            4
        };
        files.insert(
            descriptor.object_path,
            type2_car_c3d(&[0, -1, 0], resource_count),
        );
        files.insert(descriptor.palette_path, synthetic_palette());
    }
    for descriptor in MANUFACTURER_LOGOS {
        files.insert(descriptor.path, indexed_bitmap(320, 256, 3));
    }
    files.insert(INFO_BOX_DESCRIPTOR.path, indexed_bitmap(320, 132, 2));
    files.insert(CHEQUERED_PALETTE_DESCRIPTOR.path, synthetic_palette());
    for descriptor in CHEQUERED_FRAME_DESCRIPTORS {
        files.insert(descriptor.path, indexed_bitmap(182, 128, 4));
    }
    let mut profile_files = Vec::new();
    for (path, bytes) in files {
        let destination = source.join(path);
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::write(&destination, &bytes).unwrap();
        profile_files.push(AssetFile {
            path: path.to_owned(),
            size: bytes.len() as u64,
            sha256: hex_sha256(&bytes),
            encoding: StorageEncoding::Raw,
        });
    }
    profile_files.sort_by(|left, right| left.path.cmp(&right.path));
    let reviewed = ReviewedProfile::from_json(
        &serde_json::to_vec(&AssetProfile {
            schema_version: 1,
            id: "synthetic-championship-split-terminal".to_owned(),
            roots: vec!["New_Fe".to_owned()],
            empty_directories: Vec::new(),
            sentinels: Vec::new(),
            files: profile_files,
        })
        .unwrap(),
    )
    .unwrap();
    let imported = temporary.0.join("runtime-data");
    import_source(&source, &imported, &reviewed).unwrap();
    AssetStore::open(
        &imported,
        &reviewed,
        DecodeLimits {
            max_stored_bytes: 2 * 1024 * 1024,
            max_payload_bytes: 2 * 1024 * 1024,
        },
    )
    .unwrap()
}

fn type2_car_c3d(selectors: &[i16], resource_count: usize) -> Vec<u8> {
    let group0_len = C3D_GROUP0_VALUES_OFFSET + 2;
    let vertex_count = selectors.len() * 3;
    let group1_len = C3D_GROUP1_FIXED_LEN
        + vertex_count * C3D_GROUP1_ENTRY32_LEN
        + selectors.len() * C3D_GROUP1_ENTRY24_LEN;
    let group1_start = C3D_HEADER_LEN + group0_len;
    let blob_start = group1_start + group1_len;
    let bitmap = indexed_bitmap(4, 4, RESOURCE_INDEX);
    let mut bytes = vec![0; blob_start + resource_count * (16 + bitmap.len())];
    bytes[..4].copy_from_slice(&C3D_MAGIC);
    bytes[4..8].copy_from_slice(&C3D_FORMAT_VERSION.to_le_bytes());
    bytes[12..14].copy_from_slice(&1u16.to_le_bytes());
    bytes[14..16].copy_from_slice(&1u16.to_le_bytes());
    bytes[16..18].copy_from_slice(&(resource_count as u16).to_le_bytes());
    for (index, offset) in [C3D_HEADER_LEN, group1_start, blob_start]
        .into_iter()
        .enumerate()
    {
        bytes[20 + index * 4..24 + index * 4].copy_from_slice(&(offset as u32).to_le_bytes());
    }
    let group0 = C3D_HEADER_LEN;
    bytes[group0..group0 + 4].copy_from_slice(&(group0_len as u32).to_le_bytes());
    bytes[group0 + 4..group0 + 6].copy_from_slice(&(-1i16).to_le_bytes());
    for index in [0, 5, 10, 15] {
        let field = group0 + 8 + index * 4;
        bytes[field..field + 4].copy_from_slice(&Q16_ONE.to_le_bytes());
    }
    bytes[group0 + 76] = 1;
    bytes[group0 + C3D_GROUP0_VALUES_OFFSET..group0 + C3D_GROUP0_VALUES_OFFSET + 2]
        .copy_from_slice(&0u16.to_le_bytes());
    let group1 = group1_start;
    bytes[group1 + 12..group1 + 16].copy_from_slice(&(group1_len as u32).to_le_bytes());
    bytes[group1 + 16..group1 + 18].copy_from_slice(&(vertex_count as u16).to_le_bytes());
    bytes[group1 + 18..group1 + 20].copy_from_slice(&(selectors.len() as u16).to_le_bytes());
    let triangle = [
        [-Q16_ONE, -Q16_ONE, 0, 0, -Q16_ONE, 0, 0, 0],
        [-Q16_ONE, Q16_ONE, 0, 0, 0, 0, 0, 0],
        [Q16_ONE, -Q16_ONE, 0, Q16_ONE, -Q16_ONE, 0, 0, 0],
    ];
    for (selector_index, selector) in selectors.iter().enumerate() {
        for (corner, vertex) in triangle.iter().enumerate() {
            let start = group1
                + C3D_GROUP1_FIXED_LEN
                + (selector_index * 3 + corner) * C3D_GROUP1_ENTRY32_LEN;
            for (component_index, component) in vertex.iter().enumerate() {
                let value = if component_index == 4 && *selector >= 0 {
                    -1
                } else if component_index == 4 {
                    0xff00_0000_u32 as i32
                } else {
                    *component
                };
                bytes[start + component_index * 4..start + component_index * 4 + 4]
                    .copy_from_slice(&value.to_le_bytes());
            }
        }
    }
    let entries = group1 + C3D_GROUP1_FIXED_LEN + vertex_count * C3D_GROUP1_ENTRY32_LEN;
    for (entry_index, selector) in selectors.iter().enumerate() {
        let entry = entries + entry_index * C3D_GROUP1_ENTRY24_LEN;
        for (corner, offset) in C3D_ENTRY24_ENTRY32_ORDINAL_OFFSETS.into_iter().enumerate() {
            bytes[entry + offset..entry + offset + 2]
                .copy_from_slice(&((entry_index * 3 + corner) as u16).to_le_bytes());
        }
        bytes[entry + 8..entry + 10].copy_from_slice(&selector.to_le_bytes());
    }
    let mut offset = blob_start;
    for ordinal in 0..resource_count {
        let mut label = *b"CAR-TEX-000!";
        label[8] = b'0' + ordinal as u8;
        bytes[offset..offset + 12].copy_from_slice(&label);
        bytes[offset + 12..offset + 16].copy_from_slice(&(bitmap.len() as u32).to_le_bytes());
        bytes[offset + 16..offset + 16 + bitmap.len()].copy_from_slice(&bitmap);
        offset += 16 + bitmap.len();
    }
    bytes
}

fn indexed_bitmap(width: usize, height: usize, pixel: u8) -> Vec<u8> {
    const PIXEL_OFFSET: usize = 14 + 40 + 256 * 4;
    let row_stride = width.div_ceil(4) * 4;
    let mut bytes = vec![0; PIXEL_OFFSET + row_stride * height];
    bytes[0..2].copy_from_slice(b"BM");
    let file_size = bytes.len() as u32;
    bytes[2..6].copy_from_slice(&file_size.to_le_bytes());
    bytes[10..14].copy_from_slice(&(PIXEL_OFFSET as u32).to_le_bytes());
    bytes[14..18].copy_from_slice(&40u32.to_le_bytes());
    bytes[18..22].copy_from_slice(&(width as i32).to_le_bytes());
    bytes[22..26].copy_from_slice(&(height as i32).to_le_bytes());
    bytes[26..28].copy_from_slice(&1u16.to_le_bytes());
    bytes[28..30].copy_from_slice(&8u16.to_le_bytes());
    bytes[34..38].copy_from_slice(&((row_stride * height) as u32).to_le_bytes());
    bytes[46..50].copy_from_slice(&256u32.to_le_bytes());
    for index in 0..256 {
        let entry = 14 + 40 + index * 4;
        bytes[entry..entry + 4].copy_from_slice(&[
            index as u8,
            (index * 3) as u8,
            (index * 5) as u8,
            0,
        ]);
    }
    bytes[PIXEL_OFFSET..].fill(pixel);
    bytes
}

fn synthetic_palette() -> Vec<u8> {
    let data_size = 4 + 256 * 4;
    let file_size = 20 + data_size;
    let mut bytes = vec![0; file_size];
    bytes[0..4].copy_from_slice(b"RIFF");
    bytes[4..8].copy_from_slice(&((file_size - 8) as u32).to_le_bytes());
    bytes[8..12].copy_from_slice(b"PAL ");
    bytes[12..16].copy_from_slice(b"data");
    bytes[16..20].copy_from_slice(&(data_size as u32).to_le_bytes());
    bytes[20..22].copy_from_slice(&0x0300u16.to_le_bytes());
    bytes[22..24].copy_from_slice(&256u16.to_le_bytes());
    for index in 0..256 {
        let entry = 24 + index * 4;
        bytes[entry..entry + 4].copy_from_slice(&[index as u8, index as u8, index as u8, 0]);
    }
    bytes
}

fn hex_sha256(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut encoded = String::new();
    for byte in Sha256::digest(bytes) {
        write!(encoded, "{byte:02x}").unwrap();
    }
    encoded
}

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("cmr-split-terminal-{}-{id}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
