use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use cmr_assets::{
    AssetFile, AssetProfile, AssetStore, ReviewedProfile, StorageEncoding, import_source,
};
use cmr_core::c3d_actor_rgb::C3dActorRgbState;
use cmr_core::c3d_type2_fog::{
    C3dType2FogError, C3dType2FogState, prepare_c3d_type2_pre_draw_vertices,
};
use cmr_core::championship_assets::{
    CHAMPIONSHIP_CARS, MANUFACTURER_LOGOS, load_championship_assets,
};
use cmr_core::championship_car_execute::{
    ChampionshipCarBatchExecutor, ChampionshipCarBatchPreflightError, ChampionshipCarPreDrawState,
};
use cmr_core::championship_car_frame_plan::plan_championship_car_frame;
use cmr_core::championship_car_scene::{
    ChampionshipCarSceneError, ChampionshipCarScenePreflightStage, execute_championship_car_scene,
};
use cmr_core::championship_car_transform::{ChampionshipCarInput, ChampionshipCarState};
use cmr_core::championship_car_visibility::ChampionshipCarVisibilityLatches;
use cmr_core::championship_frame_assets::{
    CHEQUERED_FRAME_DESCRIPTORS, CHEQUERED_PALETTE_DESCRIPTOR, ChequeredFrame, INFO_BOX_DESCRIPTOR,
    load_championship_frame_assets, prepare_championship_frame_surfaces,
    upload_championship_frame_surfaces,
};
use cmr_core::championship_setup::{
    ActivePreview, CAR_ROW_TAG, CanonicalCar, ChampionshipSetupRenderTranscript, GEAR_ROW_TAG,
    GearChoice, NEXT_ROW_TAG, RenderRow, RowValue, SetupRowText,
};
use cmr_core::championship_setup_frame::{
    ChampionshipSetupFrameResources, ChampionshipSetupFrameState,
};
use cmr_core::championship_setup_visual::{
    ChampionshipPreviewCache, ChampionshipSetupVisualInput, NextPreviewContext, PixelPoint,
    Q16Point, RowPresentation,
};
use cmr_core::championship_terminal_frame::{
    ChampionshipCarFrameResources, ChampionshipTerminalFrameError,
    ChampionshipTerminalFrameResources, ChampionshipTerminalFrameState,
    ChampionshipTerminalPaletteResources, execute_championship_terminal_frame,
};
use cmr_core::deferred_palette::{
    DeferredPaletteCommitOutcome, DeferredPaletteState, PaletteArenaIndex, PaletteGroupBases,
};
use cmr_core::display::{
    CooperativeMode, DisplayExtent, IndexedDisplay, IndexedMode, IndexedVideoBackend,
    StartupCopyOutcome, SurfaceTopology,
};
use cmr_core::font_assets::{FONT_COUNT, FontSet, FontStyle};
use cmr_core::font_raster::UploadedFontPool;
use cmr_core::frontend_c3d_resources::prepare_frontend_c3d_resources;
use cmr_core::frontend_c3d_route::FrontendC3dRouteState;
use cmr_core::frontend_c3d_scene::{
    C3dDepthClear, C3dScenePrimaryError, C3dSceneStage, execute_frontend_c3d_scene,
};
use cmr_core::localization::{IndexedTextTable, Language};
use cmr_core::root_menu_assets::VisualVariant;
use cmr_core::root_menu_c3d_assets::load_root_menu_c3d;
use cmr_core::root_menu_c3d_render_state::{
    C3dGraphicsBranch, ROOT_MENU_C3D_INITIAL_FILTER_REQUEST_CACHE,
    root_menu_c3d_filter_cache_outcome, root_menu_c3d_legacy_alpha_contract,
};
use cmr_core::root_menu_c3d_resources::C3dPreparedSelector;
use cmr_core::root_menu_c3d_transform::{ROOT_MENU_C3D_PROJECTION, ROOT_MENU_C3D_VIEW};
use cmr_core::root_menu_c3d_visibility::RootMenuC3dVisibilityLatches;
use cmr_core::source_store::UploadedSourceStore;
use cmr_core::startup_display::StartupDisplay;
use cmr_core::surface::PaletteMapping;
use cmr_formats::bitmap::BitmapLimits;
use cmr_formats::c3d::{
    C3D_ENTRY24_ENTRY32_ORDINAL_OFFSETS, C3D_FORMAT_VERSION, C3D_GROUP0_VALUES_OFFSET,
    C3D_GROUP1_ENTRY24_LEN, C3D_GROUP1_ENTRY32_LEN, C3D_GROUP1_FIXED_LEN, C3D_HEADER_LEN,
    C3D_MAGIC, C3dLimits,
};
use cmr_formats::font::{FontLimits, TextLayoutLimits, parse_legacy_font};
use cmr_formats::riff_palette::PaletteEntry;
use cmr_formats::stored_asset::DecodeLimits;
use cmr_video::c3d_software::prepare_software_frontend_c3d_bindings;
use cmr_video::c3d_software_raster::{
    SoftwareC3dRasterError, SoftwareC3dSplitRenderDepth, SoftwareC3dTransformState,
    SoftwareChampionshipCarRasterProfile, SoftwareChampionshipCarSplitOracleProfile,
    preflight_championship_car_indexed_copy_batch, preflight_championship_car_split_modulate_batch,
};
use cmr_video::c3d_software_split_backend::{
    SoftwareSplitC3dBackendError, SoftwareSplitC3dBindingExpectation,
    SoftwareSplitC3dOracleBackend, SoftwareSplitC3dOracleConfiguration,
    SoftwareSplitC3dRetainedBindingKind, prepare_software_split_frontend_c3d_bindings,
};
use cmr_video::legacy_c3d_pixel_format::{
    KnownRgbLayout, RawDdPixelFormat, SelectedNonAlphaTextureFormat,
};
use cmr_video::packed_rgb_target::FramebufferRgbFormat;
use cmr_video::selected_texture_conversion_oracle::SourceOwnedTextureConversionOraclePolicy;
use cmr_video::software_championship_session::{
    SoftwareChampionshipFrameRequest, SoftwareChampionshipRasterSelection,
    SoftwareChampionshipRenderError, SoftwareChampionshipSessionBuildError,
    SoftwareChampionshipSessionRequest, with_software_championship_session,
};
use cmr_video::{SoftwareC3dBackendError, SoftwareIndexedBackend};
use sha2::{Digest, Sha256};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

const EXTENT: DisplayExtent = DisplayExtent {
    width: 64,
    height: 48,
};
const Q16_ONE: i32 = 1 << 16;
const IDENTITY_BASIS: [i32; 9] = [Q16_ONE, 0, 0, 0, Q16_ONE, 0, 0, 0, Q16_ONE];
const RESOURCE_INDEX: u8 = 217;
const BOOTSTRAP_RED: u8 = 11;
const CAR_PALETTE_RED: u8 = 73;
const SECOND_CAR_PALETTE_RED: u8 = 83;

#[test]
fn prepared_type2_car_executes_resource_missing_resource_on_software_backend() {
    let temporary = TestDirectory::new();
    let store = synthetic_store(&temporary);
    let championship =
        load_championship_assets(&store, C3dLimits::default(), BitmapLimits::default()).unwrap();
    let root = load_root_menu_c3d(&store, VisualVariant::Normal, C3dLimits::default());
    let frontend =
        prepare_frontend_c3d_resources(&championship, &root, BitmapLimits::default()).unwrap();
    let topology = frontend
        .car_topology(&championship, CanonicalCar::Subaru)
        .unwrap();
    let runtime = frontend
        .materialize_car(CanonicalCar::Subaru, &topology)
        .unwrap();
    let catalog = prepare_software_frontend_c3d_bindings(&frontend).unwrap();
    let mut executor = ChampionshipCarBatchExecutor::new(&catalog);
    let mut car_state = ChampionshipCarState::new();
    let input = ChampionshipCarInput {
        f9: false,
        f10: false,
    };
    let logo_origin = PixelPoint::new(8, 8);

    let hidden = plan_championship_car_frame(
        &topology,
        &runtime,
        &mut car_state,
        CanonicalCar::Subaru,
        input,
        IDENTITY_BASIS,
        logo_origin,
        EXTENT,
    )
    .unwrap();
    assert!(hidden.draws().is_empty());
    let mut successful_car_state = car_state;
    let visible = plan_championship_car_frame(
        &topology,
        &runtime,
        &mut successful_car_state,
        CanonicalCar::Subaru,
        input,
        IDENTITY_BASIS,
        logo_origin,
        EXTENT,
    )
    .unwrap();
    assert_eq!(
        visible
            .draws()
            .iter()
            .map(|draw| draw.command().selector() == C3dPreparedSelector::Missing)
            .collect::<Vec<_>>(),
        [false, true, false]
    );
    let first_draw = &visible.draws()[0];
    let prepared_vertices = prepare_c3d_type2_pre_draw_vertices(
        first_draw
            .command()
            .vertices()
            .iter()
            .map(|vertex| vertex.words()),
        C3dType2FogState::DISABLED,
    )
    .unwrap();
    assert!(!prepared_vertices.is_empty());
    assert_eq!(
        preflight_championship_car_indexed_copy_batch(
            first_draw.command(),
            &prepared_vertices[..prepared_vertices.len() - 1],
            SoftwareC3dTransformState::new(
                first_draw.actor_world(),
                ROOT_MENU_C3D_VIEW,
                ROOT_MENU_C3D_PROJECTION,
                visible.viewport(),
            ),
            SoftwareChampionshipCarRasterProfile::MUTATION_DISABLED_ACTOR_RGB_DISABLED_UNIFIED_INDEXED_COPY_NO_BLEND,
        ),
        Err(SoftwareC3dRasterError::PreparedVertexCountMismatch {
            command: prepared_vertices.len(),
            prepared: prepared_vertices.len() - 1,
        })
    );
    assert!(
        preflight_championship_car_split_modulate_batch(
            first_draw.command(),
            &prepared_vertices,
            SoftwareC3dTransformState::new(
                first_draw.actor_world(),
                ROOT_MENU_C3D_VIEW,
                ROOT_MENU_C3D_PROJECTION,
                visible.viewport(),
            ),
            SoftwareChampionshipCarSplitOracleProfile::new(
                SoftwareC3dSplitRenderDepth::Bits32,
                false,
                false,
            ),
        )
        .is_ok()
    );
    let prepared = executor
        .prepare(visible, ChampionshipCarPreDrawState::DISABLED)
        .unwrap();
    let mut enabled_car_state = car_state;
    let next_visible = plan_championship_car_frame(
        &topology,
        &runtime,
        &mut car_state,
        CanonicalCar::Subaru,
        input,
        IDENTITY_BASIS,
        logo_origin,
        EXTENT,
    )
    .unwrap();
    let next_prepared = executor
        .prepare(next_visible, ChampionshipCarPreDrawState::DISABLED)
        .unwrap();

    let (mut backend, surfaces) = initialized_backend();
    let before_pixels = backend.back_pixels().unwrap().to_vec();
    let before_depth = backend.c3d_depth_values().unwrap().to_vec();
    assert_eq!(
        execute_championship_car_scene(
            &mut executor,
            &mut backend,
            surfaces.back,
            C3dDepthClear::Clear,
            prepared,
        ),
        Err(ChampionshipCarSceneError::BackendPreflight {
            stage: ChampionshipCarScenePreflightStage::BatchBackend,
            source: SoftwareC3dBackendError::ChampionshipCarProfileNotConfigured,
        })
    );
    assert_eq!(backend.back_pixels().unwrap(), before_pixels);
    assert_eq!(backend.c3d_depth_values().unwrap(), before_depth);

    let profile = SoftwareChampionshipCarRasterProfile::
        MUTATION_DISABLED_ACTOR_RGB_DISABLED_UNIFIED_INDEXED_COPY_NO_BLEND;
    backend
        .configure_championship_car_raster_profile(profile)
        .unwrap();
    let outcome = execute_championship_car_scene(
        &mut executor,
        &mut backend,
        surfaces.back,
        C3dDepthClear::Clear,
        next_prepared,
    )
    .unwrap();

    let submission = outcome.submission.unwrap();
    assert_eq!(submission.actor_worlds, 1);
    assert_eq!(submission.bindings, 3);
    assert_eq!(submission.draws, 3);
    let pixels = backend.back_pixels().unwrap();
    let depth = backend.c3d_depth_values().unwrap();
    assert_eq!(pixels[337], RESOURCE_INDEX);
    assert_eq!(depth[337].to_bits(), 0x3f43_d264);
    assert_eq!(pixels[0], 0);
    assert_eq!(depth[0], 1.0);
    let disabled_pixels = pixels.to_vec();
    let disabled_depth = depth.to_vec();

    let enabled_visible = plan_championship_car_frame(
        &topology,
        &runtime,
        &mut enabled_car_state,
        CanonicalCar::Subaru,
        input,
        IDENTITY_BASIS,
        logo_origin,
        EXTENT,
    )
    .unwrap();
    let enabled_state = ChampionshipCarPreDrawState {
        type2_fog: C3dType2FogState {
            enabled: true,
            near_q16: 10 << 16,
            far_q16: 80 << 16,
            far_level: 0x0033_0000,
            shared_distance_q16: 9 << 16,
            center_q16: [0; 3],
        },
        actor_rgb: cmr_core::c3d_actor_rgb::C3dActorRgbState::Disabled,
    };
    let mut enabled_executor = ChampionshipCarBatchExecutor::new(&catalog);
    let enabled_prepared = enabled_executor
        .prepare(enabled_visible, enabled_state)
        .unwrap();
    let (mut enabled_backend, enabled_surfaces) = initialized_backend();
    enabled_backend
        .configure_championship_car_raster_profile(
            SoftwareChampionshipCarRasterProfile::MUTATION_ENABLED_ACTOR_RGB_DISABLED_UNIFIED_INDEXED_COPY_NO_BLEND,
        )
        .unwrap();
    execute_championship_car_scene(
        &mut enabled_executor,
        &mut enabled_backend,
        enabled_surfaces.back,
        C3dDepthClear::Clear,
        enabled_prepared,
    )
    .unwrap();
    assert_eq!(enabled_backend.back_pixels().unwrap(), disabled_pixels);
    assert_eq!(enabled_backend.c3d_depth_values().unwrap(), disabled_depth);
}

#[test]
fn split_oracle_backend_preflights_and_owns_a_reusable_logical_frame() {
    let temporary = TestDirectory::new();
    let store = synthetic_store(&temporary);
    let championship =
        load_championship_assets(&store, C3dLimits::default(), BitmapLimits::default()).unwrap();
    let root = load_root_menu_c3d(&store, VisualVariant::Normal, C3dLimits::default());
    let frontend =
        prepare_frontend_c3d_resources(&championship, &root, BitmapLimits::default()).unwrap();
    let topology = frontend
        .car_topology(&championship, CanonicalCar::Subaru)
        .unwrap();
    let runtime = frontend
        .materialize_car(CanonicalCar::Subaru, &topology)
        .unwrap();

    let texture_descriptor =
        RawDdPixelFormat::from_dwords([32, 0x40, 0, 16, 0x7c00, 0x03e0, 0x001f, 0]);
    let catalog = prepare_software_split_frontend_c3d_bindings(
        &frontend,
        SelectedNonAlphaTextureFormat::OpaqueRgb16 {
            descriptor: texture_descriptor,
            layout: KnownRgbLayout::Rgb555,
        },
        SourceOwnedTextureConversionOraclePolicy::ExactPaletteOrNearestMaskNoDither,
    )
    .unwrap();
    let mut executor = ChampionshipCarBatchExecutor::new(&catalog);
    let mut car_state = ChampionshipCarState::new();
    let input = ChampionshipCarInput {
        f9: false,
        f10: false,
    };
    let logo_origin = PixelPoint::new(8, 8);
    let hidden = plan_championship_car_frame(
        &topology,
        &runtime,
        &mut car_state,
        CanonicalCar::Subaru,
        input,
        IDENTITY_BASIS,
        logo_origin,
        EXTENT,
    )
    .unwrap();
    assert!(hidden.draws().is_empty());
    let visible = plan_championship_car_frame(
        &topology,
        &runtime,
        &mut car_state,
        CanonicalCar::Subaru,
        input,
        IDENTITY_BASIS,
        logo_origin,
        EXTENT,
    )
    .unwrap();
    let bad_actor_rgb = [[255, 0, 0]];
    let bad_prepared = executor
        .prepare(
            visible,
            ChampionshipCarPreDrawState {
                type2_fog: C3dType2FogState::DISABLED,
                actor_rgb: C3dActorRgbState::Enabled(&bad_actor_rgb),
            },
        )
        .unwrap();
    let good_visible = plan_championship_car_frame(
        &topology,
        &runtime,
        &mut car_state,
        CanonicalCar::Subaru,
        input,
        IDENTITY_BASIS,
        logo_origin,
        EXTENT,
    )
    .unwrap();
    let prepared = executor
        .prepare(good_visible, ChampionshipCarPreDrawState::DISABLED)
        .unwrap();

    let framebuffer =
        FramebufferRgbFormat::from_runtime_surface_descriptor(RawDdPixelFormat::from_dwords([
            32, 0x40, 0, 16, 0xf800, 0x07e0, 0x001f, 0,
        ]))
        .unwrap();
    let profile = SoftwareChampionshipCarSplitOracleProfile::new(
        SoftwareC3dSplitRenderDepth::Bits32,
        false,
        false,
    );
    let bad_profile = SoftwareChampionshipCarSplitOracleProfile::new(
        SoftwareC3dSplitRenderDepth::Bits32,
        false,
        true,
    );
    let mut bad_backend = SoftwareSplitC3dOracleBackend::new(
        EXTENT,
        framebuffer,
        SoftwareSplitC3dOracleConfiguration {
            profile: bad_profile,
            alpha_contract: root_menu_c3d_legacy_alpha_contract(0),
            filter_cache_outcome: root_menu_c3d_filter_cache_outcome(
                C3dGraphicsBranch::Alternate,
                true,
                ROOT_MENU_C3D_INITIAL_FILTER_REQUEST_CACHE,
            ),
        },
    )
    .unwrap();
    let bad_handle = bad_backend.target_handle();
    let bad_words_before = bad_backend.logical_target().words().to_vec();
    let bad_depth_before = bad_backend.depth_values().to_vec();
    assert!(matches!(
        execute_championship_car_scene(
            &mut executor,
            &mut bad_backend,
            bad_handle,
            C3dDepthClear::Clear,
            bad_prepared,
        ),
        Err(ChampionshipCarSceneError::BackendPreflight {
            stage: ChampionshipCarScenePreflightStage::Batch { .. },
            source: cmr_video::c3d_software_split_backend::SoftwareSplitC3dBackendError::Compose(
                cmr_video::c3d_software_raster::SoftwareC3dComposeError::Raster(
                    SoftwareC3dRasterError::NonBlackAlternateDefaultDiffuse { .. }
                )
            ),
        })
    ));
    assert_eq!(bad_backend.logical_target().words(), bad_words_before);
    assert_eq!(bad_backend.depth_values(), bad_depth_before);

    let mut backend = SoftwareSplitC3dOracleBackend::new(
        EXTENT,
        framebuffer,
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
    let handle = backend.target_handle();
    let initial_rgb = [200, 100, 50];
    backend
        .replace_logical_target_from_canonical_rgb(
            [EXTENT.width, EXTENT.height],
            &vec![initial_rgb; usize::from(EXTENT.width) * usize::from(EXTENT.height)],
        )
        .unwrap();
    let before_words = backend.logical_target().words().to_vec();
    let before_depth = backend.depth_values().to_vec();

    let outcome = execute_championship_car_scene(
        &mut executor,
        &mut backend,
        handle,
        C3dDepthClear::Clear,
        prepared,
    )
    .unwrap();

    let submission = outcome.submission.unwrap();
    assert_eq!(submission.actor_worlds, 1);
    assert_eq!(submission.bindings, 3);
    assert_eq!(submission.draws, 3);
    assert_ne!(backend.logical_target().words(), before_words);
    assert_ne!(backend.depth_values(), before_depth);
    assert_eq!(backend.logical_target().format(), framebuffer);
    assert_eq!(backend.configuration().profile, profile);
    // The embedded source palette provides green 129. RGB555 conversion first
    // decodes that as 132, the retained 0x96 diffuse material modulates it to
    // 78, and independent RGB565 framebuffer packing yields 0x260.
    assert_eq!(backend.logical_target().words()[337], 0x0260);
    assert_eq!(
        backend.logical_target().words()[0],
        framebuffer.layout().pack_rgb8(initial_rgb)
    );
    assert_eq!(backend.depth_values()[337].to_bits(), 0x3f44_4536);
    let rendered_words = backend.logical_target().words().to_vec();
    let rendered_depth = backend.depth_values().to_vec();
    let skipped = execute_frontend_c3d_scene(
        &mut backend,
        handle,
        EXTENT,
        C3dDepthClear::Skip,
        None::<()>,
        |_, ()| Ok::<(), std::convert::Infallible>(()),
    )
    .unwrap_err();
    assert_eq!(
        skipped.primary,
        C3dScenePrimaryError::Backend {
            stage: C3dSceneStage::Preflight,
            source: SoftwareSplitC3dBackendError::DepthClearRequired,
        }
    );
    assert_eq!(backend.logical_target().words(), rendered_words);
    assert_eq!(backend.depth_values(), rendered_depth);

    let empty = execute_frontend_c3d_scene(
        &mut backend,
        handle,
        EXTENT,
        C3dDepthClear::Clear,
        None::<()>,
        |_, ()| Ok::<(), std::convert::Infallible>(()),
    )
    .unwrap();
    assert!(empty.depth_cleared);
    assert_eq!(backend.logical_target().words(), rendered_words);
    assert!(backend.depth_values().iter().all(|depth| *depth == 1.0));

    let mismatch_plan = plan_championship_car_frame(
        &topology,
        &runtime,
        &mut car_state,
        CanonicalCar::Subaru,
        input,
        IDENTITY_BASIS,
        logo_origin,
        EXTENT,
    )
    .unwrap();
    let mismatch_prepared = executor
        .prepare(mismatch_plan, ChampionshipCarPreDrawState::DISABLED)
        .unwrap();
    let mut fresh_backend =
        SoftwareSplitC3dOracleBackend::new(EXTENT, framebuffer, backend.configuration()).unwrap();
    let fresh_handle = fresh_backend.target_handle();
    let fresh_words = fresh_backend.logical_target().words().to_vec();
    let fresh_depth = fresh_backend.depth_values().to_vec();
    assert!(matches!(
        execute_championship_car_scene(
            &mut executor,
            &mut fresh_backend,
            fresh_handle,
            C3dDepthClear::Clear,
            mismatch_prepared,
        ),
        Err(ChampionshipCarSceneError::BackendPreflight {
            stage: ChampionshipCarScenePreflightStage::Batch { .. },
            source: SoftwareSplitC3dBackendError::RetainedBindingMismatch {
                expected: SoftwareSplitC3dBindingExpectation::Resource(_),
                retained: SoftwareSplitC3dRetainedBindingKind::None,
            },
        })
    ));
    assert_eq!(fresh_backend.logical_target().words(), fresh_words);
    assert_eq!(fresh_backend.depth_values(), fresh_depth);

    let reuse_plan = plan_championship_car_frame(
        &topology,
        &runtime,
        &mut car_state,
        CanonicalCar::Subaru,
        input,
        IDENTITY_BASIS,
        logo_origin,
        EXTENT,
    )
    .unwrap();
    let reuse_prepared = executor
        .prepare(reuse_plan, ChampionshipCarPreDrawState::DISABLED)
        .unwrap();
    let reused = execute_championship_car_scene(
        &mut executor,
        &mut backend,
        handle,
        C3dDepthClear::Clear,
        reuse_prepared,
    )
    .unwrap();
    let reused_submission = reused.submission.unwrap();
    assert_eq!(reused_submission.bindings, 2);
    assert_eq!(reused_submission.draws, 3);
}

#[test]
fn unsupported_profile_configuration_is_effect_free() {
    let (mut unconfigured, _) = initialized_backend();
    let before_pixels = unconfigured.back_pixels().unwrap().to_vec();
    let before_depth = unconfigured.c3d_depth_values().unwrap().to_vec();
    let unsupported = SoftwareChampionshipCarRasterProfile {
        alpha_blend: true,
        ..SoftwareChampionshipCarRasterProfile::
            MUTATION_DISABLED_ACTOR_RGB_DISABLED_UNIFIED_INDEXED_COPY_NO_BLEND
    };

    assert!(matches!(
        unconfigured.configure_championship_car_raster_profile(unsupported),
        Err(SoftwareC3dBackendError::Raster(
            SoftwareC3dRasterError::UnsupportedChampionshipCarProfile(profile)
        )) if profile == unsupported
    ));

    assert_eq!(
        unconfigured.validate_championship_car_raster_profile(),
        Err(SoftwareC3dBackendError::ChampionshipCarProfileNotConfigured)
    );
    assert_eq!(unconfigured.back_pixels().unwrap(), before_pixels);
    assert_eq!(unconfigured.c3d_depth_values().unwrap(), before_depth);
}

#[test]
fn terminal_frame_presents_visible_car_indices_under_initial_group4_palette() {
    let temporary = TestDirectory::new();
    let store = synthetic_store(&temporary);
    let championship =
        load_championship_assets(&store, C3dLimits::default(), BitmapLimits::default()).unwrap();
    let root = load_root_menu_c3d(&store, VisualVariant::Normal, C3dLimits::default());
    let frontend =
        prepare_frontend_c3d_resources(&championship, &root, BitmapLimits::default()).unwrap();
    let topology = frontend
        .car_topology(&championship, CanonicalCar::Subaru)
        .unwrap();
    let runtime = frontend
        .materialize_car(CanonicalCar::Subaru, &topology)
        .unwrap();
    let catalog = prepare_software_frontend_c3d_bindings(&frontend).unwrap();
    let mut executor = ChampionshipCarBatchExecutor::new(&catalog);

    let (mut backend, surfaces) = initialized_backend();
    backend
        .configure_championship_car_raster_profile(
            SoftwareChampionshipCarRasterProfile::
                MUTATION_DISABLED_ACTOR_RGB_DISABLED_UNIFIED_INDEXED_COPY_NO_BLEND,
        )
        .unwrap();
    let mut sources = UploadedSourceStore::new(&mut backend);

    let fonts = UploadedFontPool::upload(&mut sources, synthetic_fonts()).unwrap();
    let active_palette = &championship.car(CanonicalCar::Subaru).palette;
    let logos = cmr_core::championship_bitmap_raster::upload_championship_logos(
        &mut sources,
        cmr_core::championship_bitmap_raster::prepare_championship_logo_surfaces(
            &championship,
            active_palette,
            PaletteMapping::Identity,
        )
        .unwrap(),
    )
    .unwrap();
    let frame_assets = load_championship_frame_assets(&store, BitmapLimits::default()).unwrap();
    let frames = upload_championship_frame_surfaces(
        &mut sources,
        prepare_championship_frame_surfaces(
            &frame_assets,
            active_palette,
            PaletteMapping::Identity,
        )
        .unwrap(),
    )
    .unwrap();
    let text = IndexedTextTable::parse(&b"A\n".repeat(0xfe), 0xfe).unwrap();
    let transcript = championship_transcript();
    let input = championship_input(&transcript);
    let logo_origin = PixelPoint::new(8, 8);
    let mut car_transform = ChampionshipCarState::new();
    car_transform
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
    let mut preview_cache = ChampionshipPreviewCache::new();
    let mut car_visibility = ChampionshipCarVisibilityLatches::new();
    let mut root_visibility = RootMenuC3dVisibilityLatches::new();
    let route = FrontendC3dRouteState::new();
    let mut palette_state = DeferredPaletteState::new();
    let bootstrap = palette(BOOTSTRAP_RED);
    let committed = active_palette.entries;
    assert_ne!(
        bootstrap[RESOURCE_INDEX as usize],
        committed[RESOURCE_INDEX as usize]
    );
    let palette_arena = [
        committed,
        palette(21),
        palette(31),
        palette(41),
        palette(51),
    ];
    let mut renderer_cache = bootstrap;
    let mut global_cache = bootstrap;

    let outcome = execute_championship_terminal_frame(
        input,
        ChampionshipSetupFrameState::default(),
        ChampionshipCarInput {
            f9: false,
            f10: false,
        },
        ChampionshipTerminalFrameResources {
            setup: ChampionshipSetupFrameResources {
                text: &text,
                fonts: &fonts,
                // The recovered selection command is still deferred: its
                // 256x256 source is stretched to a 33-pixel-high row, while
                // the deterministic software oracle intentionally supports
                // copy blits only.
                selection: None,
                logos: &logos,
                frames: &frames,
            },
            car: Some(ChampionshipCarFrameResources {
                car: CanonicalCar::Subaru,
                topology: &topology,
                runtime: &runtime,
                base_basis: IDENTITY_BASIS,
                pre_draw_state:
                    cmr_core::championship_car_execute::ChampionshipCarPreDrawState::DISABLED,
            }),
        },
        ChampionshipTerminalFrameState {
            preview_cache: &mut preview_cache,
            car_transform: &mut car_transform,
            car_visibility: &mut car_visibility,
            root_visibility: &mut root_visibility,
            route: &route,
        },
        ChampionshipTerminalPaletteResources {
            bits_per_pixel: 8,
            group_bases: PaletteGroupBases {
                group_1: 1,
                group_2: 2,
                group_3: 3,
                group_4: 0,
                group_5: 4,
            },
            arena: &palette_arena,
            state: &mut palette_state,
            renderer_cache: &mut renderer_cache,
            global_cache: &mut global_cache,
        },
        &mut executor,
        &mut sources,
        surfaces.back,
        TextLayoutLimits::default(),
        C3dDepthClear::Clear,
    )
    .unwrap();

    assert_eq!(outcome.scene.submission.unwrap().draws, 3);
    assert_eq!(
        outcome.palette_commit,
        DeferredPaletteCommitOutcome::Committed(PaletteArenaIndex::new(0))
    );
    assert!(outcome.presentation_completed);
    let presented = sources.backend().presented_frame().unwrap();
    assert_eq!(presented.extent(), EXTENT);
    assert!(presented.pixels().contains(&RESOURCE_INDEX));
    assert_eq!(presented.palette(), &committed);
    assert_eq!(
        presented.palette()[RESOURCE_INDEX as usize].red,
        CAR_PALETTE_RED
    );
    assert_ne!(
        presented.palette()[RESOURCE_INDEX as usize],
        bootstrap[RESOURCE_INDEX as usize]
    );
}

#[test]
fn public_championship_session_retains_state_and_releases_uploaded_sources() {
    let temporary = TestDirectory::new();
    let store = synthetic_store(&temporary);
    let bootstrap = palette(BOOTSTRAP_RED);
    let startup = synthetic_startup(bootstrap);
    let request = SoftwareChampionshipSessionRequest::new(
        Language::English,
        VisualVariant::Normal,
        SoftwareChampionshipRasterSelection::supported_software_oracle_policy(),
        PaletteMapping::Identity,
        PaletteMapping::Identity,
    );

    let run = with_software_championship_session(&store, startup, request, |session| {
        let persistent_sources = session.uploaded_source_count();
        assert_eq!(
            persistent_sources,
            FONT_COUNT + MANUFACTURER_LOGOS.len() + 16
        );
        assert_eq!(session.backend().active_palette(), Some(&bootstrap));
        assert_eq!(session.renderer_palette_cache(), &bootstrap);
        assert_eq!(session.global_palette_cache(), &bootstrap);
        assert!(session.selection_is_deferred());
        assert!(session.backend().presented_frame().is_none());

        let transcript = championship_transcript();
        let initial_preview = session.preview_cache();
        let initial_transform = session.car_transform();
        let initial_pixels = session.backend().back_pixels().unwrap().to_vec();
        let initial_depth = session.backend().c3d_depth_values().unwrap().to_vec();
        let mut wrong_extent = championship_input(&transcript);
        wrong_extent.viewport.width -= 1;
        assert!(
            session
                .render(SoftwareChampionshipFrameRequest {
                    visual: wrong_extent,
                    setup: ChampionshipSetupFrameState::default(),
                    car_input: ChampionshipCarInput {
                        f9: false,
                        f10: false,
                    },
                    base_basis: IDENTITY_BASIS,
                })
                .is_err()
        );
        assert_eq!(session.preview_cache(), initial_preview);
        assert_eq!(session.car_transform(), initial_transform);
        assert_eq!(session.backend().back_pixels().unwrap(), initial_pixels);
        assert_eq!(session.backend().c3d_depth_values().unwrap(), initial_depth);
        assert!(session.backend().presented_frame().is_none());

        let first = session
            .render(SoftwareChampionshipFrameRequest {
                visual: championship_input(&transcript),
                setup: ChampionshipSetupFrameState::default(),
                car_input: ChampionshipCarInput {
                    f9: false,
                    f10: false,
                },
                base_basis: IDENTITY_BASIS,
            })
            .unwrap();
        assert_eq!(first.scene.submission.unwrap().draws, 0);
        assert_eq!(
            first.palette_commit,
            DeferredPaletteCommitOutcome::Committed(PaletteArenaIndex::new(0))
        );
        assert_eq!(session.uploaded_source_count(), persistent_sources);
        assert_ne!(session.preview_cache(), initial_preview);
        assert_ne!(session.car_transform(), initial_transform);

        let second = session
            .render(SoftwareChampionshipFrameRequest {
                visual: championship_input(&transcript),
                setup: ChampionshipSetupFrameState::default(),
                car_input: ChampionshipCarInput {
                    f9: false,
                    f10: false,
                },
                base_basis: IDENTITY_BASIS,
            })
            .unwrap();
        assert_eq!(second.scene.submission.unwrap().draws, 3);
        assert_eq!(
            second.palette_commit,
            DeferredPaletteCommitOutcome::NoPendingPalette
        );
        assert_eq!(session.uploaded_source_count(), persistent_sources);
        let presented = session.backend().presented_frame().unwrap();
        assert!(presented.pixels().contains(&RESOURCE_INDEX));
        assert_eq!(
            presented.palette()[RESOURCE_INDEX as usize].red,
            CAR_PALETTE_RED
        );

        let second_car = championship_transcript_for(CanonicalCar::Ibiza);
        let switched = session
            .render(SoftwareChampionshipFrameRequest {
                visual: championship_input(&second_car),
                setup: ChampionshipSetupFrameState::default(),
                car_input: ChampionshipCarInput {
                    f9: false,
                    f10: false,
                },
                base_basis: IDENTITY_BASIS,
            })
            .unwrap();
        assert_eq!(switched.scene.submission.unwrap().draws, 3);
        assert_eq!(
            switched.palette_commit,
            DeferredPaletteCommitOutcome::Committed(PaletteArenaIndex::new(1))
        );
        assert_eq!(
            session.backend().presented_frame().unwrap().palette()[RESOURCE_INDEX as usize].red,
            SECOND_CAR_PALETTE_RED
        );

        let before_next_transform = session.car_transform();
        let next = championship_next_transcript();
        let next_outcome = session
            .render(SoftwareChampionshipFrameRequest {
                visual: championship_input(&next),
                setup: ChampionshipSetupFrameState {
                    chequered_frame: Some(ChequeredFrame::Frame01),
                },
                car_input: ChampionshipCarInput {
                    f9: false,
                    f10: false,
                },
                base_basis: IDENTITY_BASIS,
            })
            .unwrap();
        assert_eq!(next_outcome.scene.submission, None);
        assert_eq!(
            next_outcome.palette_commit,
            DeferredPaletteCommitOutcome::Committed(PaletteArenaIndex::new(14))
        );
        assert_eq!(session.car_transform(), before_next_transform);
        assert_eq!(
            session.palette_state().current(),
            Some(PaletteArenaIndex::new(14))
        );
        assert_eq!(
            session.active_c3d_route(),
            Some(cmr_core::frontend_c3d_route::FrontendC3dRoute::ChampionshipCar)
        );
        assert!(
            CanonicalCar::ALL
                .into_iter()
                .all(|car| session.car_visibility().mask(car).raw() == 0)
        );
        assert!(
            cmr_core::root_menu_c3d_assets::RootMenuC3dSlot::ALL
                .into_iter()
                .all(|slot| session.root_visibility().mask(slot).raw() == 0)
        );
        assert_eq!(session.uploaded_source_count(), persistent_sources);
        persistent_sources
    })
    .unwrap();

    assert_eq!(run.output, FONT_COUNT + MANUFACTURER_LOGOS.len() + 16);
    assert_eq!(run.backend.uploaded_source_count(), 0);
    assert!(run.backend.presented_frame().is_some());
    assert_eq!(
        run.startup_copy,
        StartupCopyOutcome {
            front: Ok(()),
            back: Ok(()),
        }
    );
}

#[test]
fn championship_session_rejects_unsupported_profile_before_callback() {
    let temporary = TestDirectory::new();
    let store = synthetic_store(&temporary);
    let mut unsupported = SoftwareChampionshipCarRasterProfile::
        MUTATION_DISABLED_ACTOR_RGB_DISABLED_UNIFIED_INDEXED_COPY_NO_BLEND;
    unsupported.alpha_blend = true;
    let request = SoftwareChampionshipSessionRequest::new(
        Language::English,
        VisualVariant::Normal,
        SoftwareChampionshipRasterSelection::caller_asserted(
            unsupported,
            cmr_core::c3d_type2_fog::C3dType2FogState::DISABLED,
        ),
        PaletteMapping::Identity,
        PaletteMapping::Identity,
    );
    let result = with_software_championship_session::<()>(
        &store,
        synthetic_startup(palette(BOOTSTRAP_RED)),
        request,
        |_| panic!("unsupported profile must not enter the callback"),
    );
    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("unsupported profile unexpectedly built a session"),
    };

    assert!(matches!(
        error,
        SoftwareChampionshipSessionBuildError::RasterProfile(SoftwareC3dBackendError::Raster(
            SoftwareC3dRasterError::UnsupportedChampionshipCarProfile(_)
        ))
    ));
}

#[test]
fn championship_session_rejects_fog_profile_state_mismatch_before_callback() {
    let temporary = TestDirectory::new();
    let store = synthetic_store(&temporary);
    let request = SoftwareChampionshipSessionRequest::new(
        Language::English,
        VisualVariant::Normal,
        SoftwareChampionshipRasterSelection::caller_asserted(
            SoftwareChampionshipCarRasterProfile::MUTATION_ENABLED_ACTOR_RGB_DISABLED_UNIFIED_INDEXED_COPY_NO_BLEND,
            C3dType2FogState::DISABLED,
        ),
        PaletteMapping::Identity,
        PaletteMapping::Identity,
    );
    let result = with_software_championship_session::<()>(
        &store,
        synthetic_startup(palette(BOOTSTRAP_RED)),
        request,
        |_| panic!("mismatched fog state must not enter the callback"),
    );
    assert!(matches!(
        result,
        Err(SoftwareChampionshipSessionBuildError::RasterProfile(
            SoftwareC3dBackendError::ChampionshipCarFogStateMismatch {
                profile_enabled: true,
                prepared_enabled: false,
            }
        ))
    ));
}

#[test]
fn championship_session_accepts_explicit_actor_rgb_and_renders_visible_car() {
    let temporary = TestDirectory::new();
    let store = synthetic_store(&temporary);
    let actor_rgb_by_car = std::array::from_fn(|index| {
        vec![[index as u8 + 1, index as u8 + 2, index as u8 + 3]].into_boxed_slice()
    });
    let request = SoftwareChampionshipSessionRequest::new(
        Language::English,
        VisualVariant::Normal,
        SoftwareChampionshipRasterSelection::caller_asserted_with_actor_rgb(
            SoftwareChampionshipCarRasterProfile::MUTATION_DISABLED_ACTOR_RGB_ENABLED_UNIFIED_INDEXED_COPY_NO_BLEND,
            C3dType2FogState::DISABLED,
            actor_rgb_by_car,
        ),
        PaletteMapping::Identity,
        PaletteMapping::Identity,
    );

    let run = with_software_championship_session(
        &store,
        synthetic_startup(palette(BOOTSTRAP_RED)),
        request,
        |session| {
            let transcript = championship_transcript();
            let frame = || SoftwareChampionshipFrameRequest {
                visual: championship_input(&transcript),
                setup: ChampionshipSetupFrameState::default(),
                car_input: ChampionshipCarInput {
                    f9: false,
                    f10: false,
                },
                base_basis: IDENTITY_BASIS,
            };
            assert_eq!(
                session
                    .render(frame())
                    .unwrap()
                    .scene
                    .submission
                    .unwrap()
                    .draws,
                0
            );
            assert_eq!(
                session
                    .render(frame())
                    .unwrap()
                    .scene
                    .submission
                    .unwrap()
                    .draws,
                3
            );
            assert!(
                session
                    .backend()
                    .presented_frame()
                    .unwrap()
                    .pixels()
                    .contains(&RESOURCE_INDEX)
            );
        },
    )
    .unwrap();
    assert_eq!(run.backend.uploaded_source_count(), 0);
}

#[test]
fn championship_session_rejects_actor_rgb_count_before_callback() {
    let temporary = TestDirectory::new();
    let store = synthetic_store(&temporary);
    let mut actor_rgb_by_car = std::array::from_fn(|_| vec![[0x28; 3]].into_boxed_slice());
    actor_rgb_by_car[CanonicalCar::Subaru.index() as usize] = Vec::new().into_boxed_slice();
    let request = SoftwareChampionshipSessionRequest::new(
        Language::English,
        VisualVariant::Normal,
        SoftwareChampionshipRasterSelection::caller_asserted_with_actor_rgb(
            SoftwareChampionshipCarRasterProfile::MUTATION_DISABLED_ACTOR_RGB_ENABLED_UNIFIED_INDEXED_COPY_NO_BLEND,
            C3dType2FogState::DISABLED,
            actor_rgb_by_car,
        ),
        PaletteMapping::Identity,
        PaletteMapping::Identity,
    );

    let result = with_software_championship_session::<()>(
        &store,
        synthetic_startup(palette(BOOTSTRAP_RED)),
        request,
        |_| panic!("bad actor-RGB cardinality must not enter the callback"),
    );
    assert!(matches!(
        result,
        Err(SoftwareChampionshipSessionBuildError::ActorRgbCount {
            car: CanonicalCar::Subaru,
            expected: 1,
            actual: 0,
        })
    ));
}

#[test]
fn championship_session_carries_enabled_fog_state_to_visible_car_submission() {
    let temporary = TestDirectory::new();
    let store = synthetic_store(&temporary);
    let fog = C3dType2FogState {
        enabled: true,
        near_q16: 10 << 16,
        far_q16: 80 << 16,
        far_level: 0x0033_0000,
        shared_distance_q16: 9 << 16,
        center_q16: [0; 3],
    };
    let request = SoftwareChampionshipSessionRequest::new(
        Language::English,
        VisualVariant::Normal,
        SoftwareChampionshipRasterSelection::caller_asserted(
            SoftwareChampionshipCarRasterProfile::MUTATION_ENABLED_ACTOR_RGB_DISABLED_UNIFIED_INDEXED_COPY_NO_BLEND,
            fog,
        ),
        PaletteMapping::Identity,
        PaletteMapping::Identity,
    );

    let run = with_software_championship_session(
        &store,
        synthetic_startup(palette(BOOTSTRAP_RED)),
        request,
        |session| {
            assert_eq!(session.request().raster_selection.type2_fog, fog);
            let transcript = championship_transcript();
            for expected_draws in [0, 3] {
                let outcome = session
                    .render(SoftwareChampionshipFrameRequest {
                        visual: championship_input(&transcript),
                        setup: ChampionshipSetupFrameState::default(),
                        car_input: ChampionshipCarInput {
                            f9: false,
                            f10: false,
                        },
                        base_basis: IDENTITY_BASIS,
                    })
                    .unwrap();
                assert_eq!(outcome.scene.submission.unwrap().draws, expected_draws);
            }
            assert!(
                session
                    .backend()
                    .presented_frame()
                    .unwrap()
                    .pixels()
                    .contains(&RESOURCE_INDEX)
            );
        },
    )
    .unwrap();
    assert_eq!(run.backend.uploaded_source_count(), 0);
}

#[test]
fn championship_session_carries_enabled_fog_parameters_to_car_preparation() {
    let temporary = TestDirectory::new();
    let store = synthetic_store(&temporary);
    let fog = C3dType2FogState {
        enabled: true,
        near_q16: 0,
        far_q16: 1,
        far_level: 0x0033_0000,
        shared_distance_q16: 1,
        center_q16: [7, 8, 9],
    };
    let request = SoftwareChampionshipSessionRequest::new(
        Language::English,
        VisualVariant::Normal,
        SoftwareChampionshipRasterSelection::caller_asserted(
            SoftwareChampionshipCarRasterProfile::MUTATION_ENABLED_ACTOR_RGB_DISABLED_UNIFIED_INDEXED_COPY_NO_BLEND,
            fog,
        ),
        PaletteMapping::Identity,
        PaletteMapping::Identity,
    );

    with_software_championship_session(
        &store,
        synthetic_startup(palette(BOOTSTRAP_RED)),
        request,
        |session| {
            let transcript = championship_transcript();
            let frame = || SoftwareChampionshipFrameRequest {
                visual: championship_input(&transcript),
                setup: ChampionshipSetupFrameState::default(),
                car_input: ChampionshipCarInput {
                    f9: false,
                    f10: false,
                },
                base_basis: IDENTITY_BASIS,
            };
            assert_eq!(
                session
                    .render(frame())
                    .unwrap()
                    .scene
                    .submission
                    .unwrap()
                    .draws,
                0
            );

            let error = session.render(frame()).unwrap_err();
            assert!(matches!(
                error,
                SoftwareChampionshipRenderError::Frame(source)
                    if matches!(
                        *source,
                        ChampionshipTerminalFrameError::CarPrepare {
                            source: ChampionshipCarBatchPreflightError::Type2Fog {
                                source: C3dType2FogError::SlopeOverflow {
                                    near_q16: 0,
                                    far_q16: 1,
                                },
                                ..
                            },
                            ..
                        }
                    )
            ));
        },
    )
    .unwrap();
}

fn synthetic_startup(bootstrap: [PaletteEntry; 256]) -> StartupDisplay<SoftwareIndexedBackend> {
    let display = IndexedDisplay::initialize(
        SoftwareIndexedBackend::default(),
        IndexedMode {
            extent: EXTENT,
            bits_per_pixel: 8,
        },
        CooperativeMode::ExclusiveFullscreen,
        &bootstrap,
    )
    .unwrap();
    StartupDisplay {
        display,
        copy: StartupCopyOutcome {
            front: Ok(()),
            back: Ok(()),
        },
    }
}

fn initialized_backend() -> (
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
    let entries = palette(BOOTSTRAP_RED);
    let palette = backend.create_palette(&entries).unwrap();
    backend
        .attach_palette(surfaces.visible_front, palette)
        .unwrap();
    backend.attach_palette(surfaces.back, palette).unwrap();
    backend.upload_palette(palette, &entries).unwrap();
    (backend, surfaces)
}

fn palette(red: u8) -> [PaletteEntry; 256] {
    std::array::from_fn(|index| PaletteEntry {
        red,
        green: index as u8,
        blue: index as u8,
        flags: 0,
    })
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

fn championship_transcript() -> ChampionshipSetupRenderTranscript {
    championship_transcript_for(CanonicalCar::Subaru)
}

fn championship_transcript_for(car: CanonicalCar) -> ChampionshipSetupRenderTranscript {
    ChampionshipSetupRenderTranscript {
        title_text_index: 0x13,
        rows: [
            RenderRow {
                tag: CAR_ROW_TAG,
                text: SetupRowText::ChooseCar,
                selected: true,
                value: RowValue::Car(car),
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
            canonical_car: car,
            object_name: car.object_name(),
            logo_name: car.logo_name(),
        },
    }
}

fn championship_next_transcript() -> ChampionshipSetupRenderTranscript {
    ChampionshipSetupRenderTranscript {
        title_text_index: 0x13,
        rows: [
            RenderRow {
                tag: CAR_ROW_TAG,
                text: SetupRowText::ChooseCar,
                selected: false,
                value: RowValue::Car(CanonicalCar::Ibiza),
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

fn championship_input<'a>(
    transcript: &'a ChampionshipSetupRenderTranscript,
) -> ChampionshipSetupVisualInput<'a> {
    ChampionshipSetupVisualInput {
        viewport: EXTENT,
        frontend_offset_q16: Q16Point::new(0, 0),
        row_anchors_q16: [
            Q16Point::new(0x2000, 0x2000),
            Q16Point::new(0x2000, 0x4000),
            Q16Point::new(0x2000, 0x6000),
        ],
        // Combined with the recovered +0x3999 horizontal logo adjustment,
        // this maps to the stable in-bounds origin (8, 8) at 64x48.
        preview_anchor_q16: Q16Point::new(-0x1999, 0x2aab),
        rows: [RowPresentation {
            visible: true,
            enabled: true,
        }; 3],
        selected_text_widths: [1; 3],
        transcript,
        next_context: NextPreviewContext {
            state_byte: 0,
            mode_byte: 0,
        },
    }
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
        files.insert(
            descriptor.palette_path,
            synthetic_palette(match descriptor.car {
                CanonicalCar::Subaru => CAR_PALETTE_RED,
                CanonicalCar::Ibiza => SECOND_CAR_PALETTE_RED,
                _ => 0,
            }),
        );
    }
    for descriptor in MANUFACTURER_LOGOS {
        files.insert(descriptor.path, indexed_bitmap(320, 256, 3));
    }
    files.insert(INFO_BOX_DESCRIPTOR.path, indexed_bitmap(320, 132, 0));
    files.insert(CHEQUERED_PALETTE_DESCRIPTOR.path, synthetic_palette(0));
    for descriptor in CHEQUERED_FRAME_DESCRIPTORS {
        files.insert(descriptor.path, indexed_bitmap(182, 128, 0));
    }
    files.insert(Language::English.frontend_path(), b"A\n".repeat(0xfe));
    for (index, style) in FontStyle::ALL.into_iter().enumerate() {
        files.insert(style.path(), synthetic_font(index as u8));
    }

    let mut profile_files = Vec::with_capacity(files.len());
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
    let profile = AssetProfile {
        schema_version: 1,
        id: "synthetic-championship-car-software".to_owned(),
        roots: vec!["New_Fe".to_owned()],
        empty_directories: Vec::new(),
        sentinels: Vec::new(),
        files: profile_files,
    };
    let reviewed = ReviewedProfile::from_json(&serde_json::to_vec(&profile).unwrap()).unwrap();
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
    let bitmap = indexed_bitmap_4x4(RESOURCE_INDEX);
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
    let triangle: [[i32; 8]; 3] = [
        [-Q16_ONE, -Q16_ONE, 0, 0, -Q16_ONE, 0, 0, 0],
        [-Q16_ONE, Q16_ONE, 0, 0, 0, 0, 0, 0],
        [Q16_ONE, -Q16_ONE, 0, Q16_ONE, -Q16_ONE, 0, 0, 0],
    ];
    for (selector_index, selector) in selectors.iter().enumerate() {
        for (corner, vertex) in triangle.iter().enumerate() {
            let vertex_index = selector_index * 3 + corner;
            let start = group1 + C3D_GROUP1_FIXED_LEN + vertex_index * C3D_GROUP1_ENTRY32_LEN;
            for (component_index, component) in vertex.iter().enumerate() {
                let field = start + component_index * 4;
                let component = if component_index == 4 {
                    if *selector >= 0 {
                        u32::MAX as i32
                    } else {
                        0xff00_0000_u32 as i32
                    }
                } else {
                    *component
                };
                bytes[field..field + 4].copy_from_slice(&component.to_le_bytes());
            }
        }
    }
    let entries = group1 + C3D_GROUP1_FIXED_LEN + vertex_count * C3D_GROUP1_ENTRY32_LEN;
    for (entry_index, selector) in selectors.iter().enumerate() {
        let entry = entries + entry_index * C3D_GROUP1_ENTRY24_LEN;
        for (corner, ordinal_offset) in C3D_ENTRY24_ENTRY32_ORDINAL_OFFSETS.into_iter().enumerate()
        {
            let ordinal = (entry_index * 3 + corner) as u16;
            bytes[entry + ordinal_offset..entry + ordinal_offset + 2]
                .copy_from_slice(&ordinal.to_le_bytes());
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

fn indexed_bitmap_4x4(pixel: u8) -> Vec<u8> {
    let mut bitmap = indexed_bitmap(4, 4, pixel);
    let palette_entry = 14 + 40 + usize::from(pixel) * 4;
    bitmap[palette_entry..palette_entry + 4].copy_from_slice(&[0, 129, 0, 0]);
    bitmap
}

fn indexed_bitmap(width: usize, height: usize, pixel: u8) -> Vec<u8> {
    const PIXEL_OFFSET: usize = 14 + 40 + 256 * 4;
    let row_stride = width.div_ceil(4) * 4;
    let mut bytes = vec![0; PIXEL_OFFSET + row_stride * height];
    let file_size = bytes.len() as u32;
    bytes[0..2].copy_from_slice(b"BM");
    bytes[2..6].copy_from_slice(&file_size.to_le_bytes());
    bytes[10..14].copy_from_slice(&(PIXEL_OFFSET as u32).to_le_bytes());
    bytes[14..18].copy_from_slice(&40u32.to_le_bytes());
    bytes[18..22].copy_from_slice(&(width as i32).to_le_bytes());
    bytes[22..26].copy_from_slice(&(height as i32).to_le_bytes());
    bytes[26..28].copy_from_slice(&1u16.to_le_bytes());
    bytes[28..30].copy_from_slice(&8u16.to_le_bytes());
    bytes[34..38].copy_from_slice(&((row_stride * height) as u32).to_le_bytes());
    bytes[46..50].copy_from_slice(&256u32.to_le_bytes());
    bytes[PIXEL_OFFSET..].fill(pixel);
    bytes
}

fn synthetic_palette(red: u8) -> Vec<u8> {
    let data_size = 4 + 256 * 4;
    let file_size = 12 + 8 + data_size;
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
        bytes[entry] = red;
        bytes[entry + 1] = index as u8;
        bytes[entry + 2] = index as u8;
    }
    bytes
}

fn hex_sha256(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut encoded = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        write!(encoded, "{byte:02x}").unwrap();
    }
    encoded
}

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "cmr-video-championship-car-{}-{id}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
