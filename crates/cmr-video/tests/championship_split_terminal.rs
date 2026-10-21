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
    blit::{BlitDecision, BlitMethod, ColorKey},
    championship_assets::{CHAMPIONSHIP_CARS, MANUFACTURER_LOGOS, load_championship_assets},
    championship_car_execute::{ChampionshipCarBatchExecutor, ChampionshipCarPreDrawState},
    championship_car_transform::{ChampionshipCarInput, ChampionshipCarState},
    championship_car_visibility::ChampionshipCarVisibilityLatches,
    championship_frame_assets::{
        CHEQUERED_FRAME_DESCRIPTORS, CHEQUERED_PALETTE_DESCRIPTOR, ChequeredFrame,
        INFO_BOX_DESCRIPTOR, load_championship_frame_assets,
    },
    championship_setup::{
        ActivePreview, CAR_ROW_TAG, CanonicalCar, ChampionshipSetupRenderTranscript, GEAR_ROW_TAG,
        GearChoice, NEXT_ROW_TAG, RenderRow, RowValue, SetupRowText,
    },
    championship_setup_frame::{
        ChampionshipBitmapIdentity, ChampionshipSetupFrameState, DeferredChampionshipCommand,
        PreparedChampionshipAlternateSetupFrame, PreparedChampionshipSetupCommand,
        prepare_championship_setup_frame_from_source_extents,
    },
    championship_setup_visual::{
        ChampionshipPreviewCache, ChampionshipSetupVisualInput, NextPreviewContext, Q16Point,
        RowPresentation, SetupTextRole,
    },
    championship_terminal_frame::scan_championship_car_contract_for_resource,
    deferred_palette::{
        DeferredPaletteCommitOutcome, DeferredPaletteState, PaletteArenaIndex, PaletteGroupBases,
    },
    display::DisplayExtent,
    frontend_c3d_resources::prepare_frontend_c3d_resources,
    frontend_c3d_route::{FrontendC3dRoute, FrontendC3dRouteState},
    frontend_c3d_scene::C3dDepthClear,
    root_menu_assets::{ROOT_MENU_SELECT_PATH, VisualVariant},
    root_menu_c3d_assets::load_root_menu_c3d,
    root_menu_c3d_render_state::{
        C3dGraphicsBranch, ROOT_MENU_C3D_INITIAL_FILTER_REQUEST_CACHE,
        root_menu_c3d_filter_cache_outcome, root_menu_c3d_legacy_alpha_contract,
    },
    root_menu_c3d_visibility::RootMenuC3dVisibilityLatches,
};
use cmr_formats::{
    bitmap::{BitmapLimits, IndexedBitmap, parse_indexed_bitmap},
    c3d::{
        C3D_ENTRY24_ENTRY32_ORDINAL_OFFSETS, C3D_FORMAT_VERSION, C3D_GROUP0_VALUES_OFFSET,
        C3D_GROUP1_ENTRY24_LEN, C3D_GROUP1_ENTRY32_LEN, C3D_GROUP1_FIXED_LEN, C3D_HEADER_LEN,
        C3D_MAGIC, C3dLimits,
    },
    stored_asset::DecodeLimits,
};
use cmr_video::{
    c3d_software_raster::{SoftwareC3dSplitRenderDepth, SoftwareChampionshipCarSplitOracleProfile},
    c3d_software_split_backend::{
        SoftwareSplitC3dOracleBackend, SoftwareSplitC3dOracleConfiguration,
        prepare_software_split_frontend_c3d_bindings,
    },
    legacy_c3d_pixel_format::{KnownRgbLayout, RawDdPixelFormat, SelectedNonAlphaTextureFormat},
    packed_rgb_target::FramebufferRgbFormat,
    selected_texture_conversion_oracle::SourceOwnedTextureConversionOraclePolicy,
    software_championship_alternate_session::{
        SoftwareChampionshipAlternateFrameRequest, SoftwareChampionshipAlternateRasterState,
        SoftwareChampionshipAlternateRenderError, SoftwareChampionshipAlternateSessionBuildError,
        SoftwareChampionshipAlternateSessionRequest, with_software_championship_alternate_session,
    },
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
        prepare_software_championship_alternate_setup_frame,
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

#[test]
fn alternate_session_repeats_car_and_next_frames_without_indexed_ownership() {
    let temporary = TestDirectory::new();
    let store = synthetic_store(&temporary);
    let request = alternate_session_request(5, KnownRgbLayout::Rgb555);

    let run = with_software_championship_alternate_session(&store, request, |session| {
        assert_eq!(session.ordinary_source_count(), 27);
        assert!(session.selection_is_loaded());
        assert!(session.selection_is_deferred());
        let initial_preview = session.preview_cache();
        let initial_transform = session.car_transform();
        let initial_words = session.backend().logical_target().words().to_vec();
        let initial_depth = session.backend().depth_values().to_vec();

        let car = car_transcript();
        let mut wrong_extent = visual_input(&car);
        wrong_extent.viewport.width -= 1;
        assert!(matches!(
            session.render(SoftwareChampionshipAlternateFrameRequest {
                visual: wrong_extent,
                setup: ChampionshipSetupFrameState::default(),
                car_input: ChampionshipCarInput {
                    f9: false,
                    f10: false,
                },
                base_basis: IDENTITY_BASIS,
            }),
            Err(SoftwareChampionshipAlternateRenderError::ExtentMismatch {
                expected: EXTENT,
                actual: DisplayExtent {
                    width: 639,
                    height: 480,
                },
            })
        ));
        assert_eq!(session.preview_cache(), initial_preview);
        assert_eq!(session.car_transform(), initial_transform);
        assert_eq!(session.backend().logical_target().words(), initial_words);
        assert_eq!(session.backend().depth_values(), initial_depth);

        let first = session
            .render(SoftwareChampionshipAlternateFrameRequest {
                visual: visual_input(&car),
                setup: ChampionshipSetupFrameState::default(),
                car_input: ChampionshipCarInput {
                    f9: false,
                    f10: false,
                },
                base_basis: IDENTITY_BASIS,
            })
            .unwrap();
        assert_eq!(first.scene.submission.unwrap().draws, 0);
        assert!(first.setup.commands.iter().any(is_stretch));
        assert_eq!(
            first.palette_commit,
            DeferredPaletteCommitOutcome::SkippedNonIndexedDepth
        );
        assert_ne!(session.preview_cache(), initial_preview);
        assert_ne!(session.car_transform(), initial_transform);

        let second = session
            .render(SoftwareChampionshipAlternateFrameRequest {
                visual: visual_input(&car),
                setup: ChampionshipSetupFrameState::default(),
                car_input: ChampionshipCarInput {
                    f9: false,
                    f10: false,
                },
                base_basis: IDENTITY_BASIS,
            })
            .unwrap();
        let second_submission = second.scene.submission.unwrap();
        assert_eq!(second_submission.draws, 3);
        assert_eq!(second_submission.bindings, 3);
        assert_eq!(session.ordinary_source_count(), 27);

        let repeated = session
            .render(SoftwareChampionshipAlternateFrameRequest {
                visual: visual_input(&car),
                setup: ChampionshipSetupFrameState::default(),
                car_input: ChampionshipCarInput {
                    f9: false,
                    f10: false,
                },
                base_basis: IDENTITY_BASIS,
            })
            .unwrap();
        let repeated_submission = repeated.scene.submission.unwrap();
        assert_eq!(repeated_submission.draws, 3);
        assert_eq!(repeated_submission.bindings, 2);

        let before_next_transform = session.car_transform();
        let next = next_transcript();
        let next_outcome = session
            .render(SoftwareChampionshipAlternateFrameRequest {
                visual: visual_input(&next),
                setup: ChampionshipSetupFrameState {
                    chequered_frame: Some(ChequeredFrame::Frame04),
                },
                car_input: ChampionshipCarInput {
                    f9: false,
                    f10: false,
                },
                base_basis: IDENTITY_BASIS,
            })
            .unwrap();
        assert_eq!(next_outcome.scene.submission, None);
        assert_eq!(session.car_transform(), before_next_transform);
        assert_eq!(
            session.palette_state().pending(),
            Some(PaletteArenaIndex::new(14))
        );
        assert_eq!(
            session.active_c3d_route(),
            Some(FrontendC3dRoute::ChampionshipCar)
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
        session.ordinary_source_count()
    })
    .unwrap();

    assert_eq!(run.output, 27);
    assert!(
        run.backend
            .logical_target()
            .words()
            .iter()
            .any(|word| *word != 0)
    );
    assert!(run.backend.depth_values().iter().all(|depth| *depth == 1.0));
}

#[test]
fn alternate_session_rejects_ordinary_framebuffer_layout_before_callback() {
    let temporary = TestDirectory::new();
    let store = synthetic_store(&temporary);
    let request = alternate_session_request(5, KnownRgbLayout::Rgb565);
    let mut entered = false;

    let result = with_software_championship_alternate_session(&store, request, |_| {
        entered = true;
    });

    assert!(!entered);
    assert!(matches!(
        result,
        Err(
            SoftwareChampionshipAlternateSessionBuildError::OrdinaryFramebufferLayoutMismatch {
                expected: KnownRgbLayout::Rgb555,
                actual: Some(KnownRgbLayout::Rgb565),
            }
        )
    ));
}

#[test]
fn alternate_session_builds_and_renders_rgb565_sources_into_rgb565_target() {
    let temporary = TestDirectory::new();
    let store = synthetic_store(&temporary);
    let request = alternate_session_request(6, KnownRgbLayout::Rgb565);

    let run = with_software_championship_alternate_session(&store, request, |session| {
        let next = next_transcript();
        let outcome = session
            .render(SoftwareChampionshipAlternateFrameRequest {
                visual: visual_input(&next),
                setup: ChampionshipSetupFrameState {
                    chequered_frame: Some(ChequeredFrame::Frame04),
                },
                car_input: ChampionshipCarInput {
                    f9: false,
                    f10: false,
                },
                base_basis: IDENTITY_BASIS,
            })
            .unwrap();

        assert_eq!(outcome.scene.submission, None);
        assert_eq!(
            outcome.palette_commit,
            DeferredPaletteCommitOutcome::SkippedNonIndexedDepth
        );
        assert_eq!(session.ordinary_source_count(), 27);
    })
    .unwrap();

    assert_eq!(
        run.backend
            .logical_target()
            .format()
            .layout()
            .known_layout(),
        Some(KnownRgbLayout::Rgb565)
    );
    assert!(
        run.backend
            .logical_target()
            .words()
            .iter()
            .any(|word| *word != 0)
    );
}

#[test]
fn alternate_session_rejects_pre_draw_profile_mismatch_before_callback() {
    let temporary = TestDirectory::new();
    let store = synthetic_store(&temporary);
    let mut request = alternate_session_request(5, KnownRgbLayout::Rgb555);
    request.raster_state.type2_fog = cmr_core::c3d_type2_fog::C3dType2FogState {
        enabled: true,
        ..cmr_core::c3d_type2_fog::C3dType2FogState::DISABLED
    };
    let mut entered = false;

    let result = with_software_championship_alternate_session(&store, request, |_| {
        entered = true;
    });

    assert!(!entered);
    assert!(matches!(
        result,
        Err(SoftwareChampionshipAlternateSessionBuildError::RasterProfile(
            cmr_video::c3d_software_split_backend::SoftwareSplitC3dBackendError::ChampionshipCarFogStateMismatch {
                profile_enabled: false,
                prepared_enabled: true,
            }
        ))
    ));
}

fn alternate_session_request(
    discriminator: u8,
    framebuffer_layout: KnownRgbLayout,
) -> SoftwareChampionshipAlternateSessionRequest {
    let masks = match framebuffer_layout {
        KnownRgbLayout::Rgb555 => [0x7c00, 0x03e0, 0x001f],
        KnownRgbLayout::Rgb565 => [0xf800, 0x07e0, 0x001f],
    };
    let framebuffer_format =
        FramebufferRgbFormat::from_runtime_surface_descriptor(RawDdPixelFormat::from_dwords([
            32, 0x40, 0, 16, masks[0], masks[1], masks[2], 0,
        ]))
        .unwrap();
    let texture_descriptor =
        RawDdPixelFormat::from_dwords([32, 0x40, 0, 16, 0x7c00, 0x03e0, 0x001f, 0]);
    let profile = SoftwareChampionshipCarSplitOracleProfile::new(
        SoftwareC3dSplitRenderDepth::Bits32,
        false,
        false,
    );
    SoftwareChampionshipAlternateSessionRequest::new(
        VisualVariant::Normal,
        EXTENT,
        framebuffer_format,
        discriminator,
        SelectedNonAlphaTextureFormat::OpaqueRgb16 {
            descriptor: texture_descriptor,
            layout: KnownRgbLayout::Rgb555,
        },
        SoftwareSplitC3dOracleConfiguration {
            profile,
            alpha_contract: root_menu_c3d_legacy_alpha_contract(0),
            filter_cache_outcome: root_menu_c3d_filter_cache_outcome(
                C3dGraphicsBranch::Alternate,
                true,
                ROOT_MENU_C3D_INITIAL_FILTER_REQUEST_CACHE,
            ),
        },
        SoftwareChampionshipAlternateRasterState::disabled(),
    )
}

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PreparedKind {
    Text(SetupTextRole),
    Bitmap(ChampionshipBitmapIdentity, BlitMethod, Option<ColorKey>),
    NoOp(ChampionshipBitmapIdentity),
    InfoText,
    Transform,
    Latch,
    Palette(u8, u8),
    NextPreview,
    SelectionDeferred,
}

#[test]
fn alternate_preparation_matches_shared_indexed_kernel_for_cold_and_warm_transcripts() {
    with_source_fixture(|_, _, _, pool, selection| {
        for (transcript, state, expected_cold, expected_warm) in [
            (
                car_transcript(),
                ChampionshipSetupFrameState::default(),
                vec![
                    PreparedKind::Text(SetupTextRole::Title),
                    selection_kind(),
                    row_text(cmr_core::championship_setup::SetupRowKind::ChooseCar),
                    opaque_fast(ChampionshipBitmapIdentity::InfoBox),
                    PreparedKind::InfoText,
                    PreparedKind::Transform,
                    PreparedKind::Latch,
                    opaque_fast(ChampionshipBitmapIdentity::ManufacturerLogo(
                        cmr_core::championship_assets::ManufacturerLogo::Subaru,
                    )),
                    PreparedKind::Palette(4, 0),
                    row_text(cmr_core::championship_setup::SetupRowKind::ChooseGear),
                    row_text(cmr_core::championship_setup::SetupRowKind::Next),
                ],
                vec![
                    PreparedKind::Text(SetupTextRole::Title),
                    selection_kind(),
                    row_text(cmr_core::championship_setup::SetupRowKind::ChooseCar),
                    opaque_fast(ChampionshipBitmapIdentity::InfoBox),
                    PreparedKind::InfoText,
                    PreparedKind::Transform,
                    PreparedKind::Latch,
                    opaque_fast(ChampionshipBitmapIdentity::ManufacturerLogo(
                        cmr_core::championship_assets::ManufacturerLogo::Subaru,
                    )),
                    row_text(cmr_core::championship_setup::SetupRowKind::ChooseGear),
                    row_text(cmr_core::championship_setup::SetupRowKind::Next),
                ],
            ),
            (
                gear_transcript(),
                ChampionshipSetupFrameState::default(),
                vec![
                    PreparedKind::Text(SetupTextRole::Title),
                    row_text(cmr_core::championship_setup::SetupRowKind::ChooseCar),
                    selection_kind(),
                    row_text(cmr_core::championship_setup::SetupRowKind::ChooseGear),
                    opaque_fast(ChampionshipBitmapIdentity::InfoBox),
                    PreparedKind::InfoText,
                    PreparedKind::Transform,
                    PreparedKind::Latch,
                    opaque_fast(ChampionshipBitmapIdentity::ManufacturerLogo(
                        cmr_core::championship_assets::ManufacturerLogo::Subaru,
                    )),
                    PreparedKind::Palette(4, 0),
                    row_text(cmr_core::championship_setup::SetupRowKind::Next),
                ],
                vec![
                    PreparedKind::Text(SetupTextRole::Title),
                    row_text(cmr_core::championship_setup::SetupRowKind::ChooseCar),
                    selection_kind(),
                    row_text(cmr_core::championship_setup::SetupRowKind::ChooseGear),
                    opaque_fast(ChampionshipBitmapIdentity::InfoBox),
                    PreparedKind::InfoText,
                    PreparedKind::Transform,
                    PreparedKind::Latch,
                    opaque_fast(ChampionshipBitmapIdentity::ManufacturerLogo(
                        cmr_core::championship_assets::ManufacturerLogo::Subaru,
                    )),
                    row_text(cmr_core::championship_setup::SetupRowKind::Next),
                ],
            ),
            (
                next_transcript(),
                ChampionshipSetupFrameState {
                    chequered_frame: Some(ChequeredFrame::Frame04),
                },
                vec![
                    PreparedKind::Text(SetupTextRole::Title),
                    row_text(cmr_core::championship_setup::SetupRowKind::ChooseCar),
                    row_text(cmr_core::championship_setup::SetupRowKind::ChooseGear),
                    selection_kind(),
                    row_text(cmr_core::championship_setup::SetupRowKind::Next),
                    opaque_fast(ChampionshipBitmapIdentity::InfoBox),
                    PreparedKind::InfoText,
                    PreparedKind::Palette(1, 9),
                    opaque_fast(ChampionshipBitmapIdentity::ChequeredFrame(
                        ChequeredFrame::Frame04,
                    )),
                ],
                vec![
                    PreparedKind::Text(SetupTextRole::Title),
                    row_text(cmr_core::championship_setup::SetupRowKind::ChooseCar),
                    row_text(cmr_core::championship_setup::SetupRowKind::ChooseGear),
                    selection_kind(),
                    row_text(cmr_core::championship_setup::SetupRowKind::Next),
                    opaque_fast(ChampionshipBitmapIdentity::InfoBox),
                    PreparedKind::InfoText,
                    opaque_fast(ChampionshipBitmapIdentity::ChequeredFrame(
                        ChequeredFrame::Frame04,
                    )),
                ],
            ),
        ] {
            let mut cache = ChampionshipPreviewCache::new();
            assert_preparation_parity(&transcript, state, cache, pool, selection, &expected_cold);

            prepare_software_championship_alternate_setup_frame(
                visual_input(&transcript),
                state,
                cache,
                pool,
                Some(selection),
            )
            .unwrap()
            .execute_after_successful_preflight(&mut cache, |_| {
                Ok::<_, std::convert::Infallible>(())
            })
            .unwrap();

            assert_preparation_parity(&transcript, state, cache, pool, selection, &expected_warm);
        }
    });
}

#[test]
fn alternate_preparation_uses_aligned_pool_extents_and_keeps_opaque_keys_unset() {
    with_source_fixture(|_, _, _, pool, selection| {
        assert_ne!(pool.info_box().packed_color_key(), 0);
        assert!(pool.info_box().aligned_surface_extent()[0] > pool.info_box().dib_extent()[0]);
        let prepared = prepare_software_championship_alternate_setup_frame(
            visual_input(&car_transcript()),
            ChampionshipSetupFrameState::default(),
            ChampionshipPreviewCache::new(),
            pool,
            Some(selection),
        )
        .unwrap();
        for command in prepared.commands() {
            let PreparedChampionshipSetupCommand::Bitmap {
                identity,
                decision: BlitDecision::Blit(plan),
                ..
            } = command
            else {
                continue;
            };
            match identity {
                ChampionshipBitmapIdentity::Selection => {
                    assert_eq!(plan.method, BlitMethod::Stretch);
                    assert_eq!(plan.color_key, Some(ColorKey { low: 0, high: 0 }));
                    assert!(plan.source.right <= i32::try_from(selection.row_stride).unwrap());
                    assert!(plan.source.bottom <= i32::try_from(selection.height).unwrap());
                }
                ChampionshipBitmapIdentity::InfoBox => {
                    assert_eq!(plan.method, BlitMethod::Fast);
                    assert_eq!(plan.color_key, None);
                    let extent = pool.info_box().aligned_surface_extent();
                    assert!(plan.source.right <= i32::try_from(extent[0]).unwrap());
                    assert!(plan.source.bottom <= i32::try_from(extent[1]).unwrap());
                }
                ChampionshipBitmapIdentity::ManufacturerLogo(logo) => {
                    assert_eq!(plan.method, BlitMethod::Fast);
                    assert_eq!(plan.color_key, None);
                    let extent = pool.logo(logo).aligned_surface_extent();
                    assert!(plan.source.right <= i32::try_from(extent[0]).unwrap());
                    assert!(plan.source.bottom <= i32::try_from(extent[1]).unwrap());
                }
                ChampionshipBitmapIdentity::ChequeredFrame(_) => unreachable!(),
            }
        }
    });
}

#[test]
fn alternate_preparation_preserves_clipping_noop_and_deferred_selection_without_effects() {
    with_source_fixture(|_, _, _, pool, selection| {
        let transcript = car_transcript();
        let cache = ChampionshipPreviewCache::new();
        let mut clipped_input = visual_input(&transcript);
        clipped_input.frontend_offset_q16 = Q16Point::new(-0x2000, -0x2000);
        let clipped = prepare_software_championship_alternate_setup_frame(
            clipped_input,
            ChampionshipSetupFrameState::default(),
            cache,
            pool,
            Some(selection),
        )
        .unwrap();
        assert!(clipped.commands().any(|command| matches!(
            command,
            PreparedChampionshipSetupCommand::Bitmap {
                decision: BlitDecision::Blit(plan),
                ..
            } if plan.destination.left == 0 || plan.destination.top == 0
        )));

        let mut offscreen_input = visual_input(&transcript);
        offscreen_input.frontend_offset_q16 = Q16Point::new(1000 << 16, 1000 << 16);
        let offscreen = prepare_software_championship_alternate_setup_frame(
            offscreen_input,
            ChampionshipSetupFrameState::default(),
            cache,
            pool,
            Some(selection),
        )
        .unwrap();
        assert_eq!(
            offscreen
                .commands()
                .filter(|command| matches!(
                    command,
                    PreparedChampionshipSetupCommand::Bitmap {
                        decision: BlitDecision::NoOp,
                        ..
                    }
                ))
                .count(),
            3
        );

        let deferred = prepare_software_championship_alternate_setup_frame(
            visual_input(&transcript),
            ChampionshipSetupFrameState::default(),
            cache,
            pool,
            None,
        )
        .unwrap();
        assert!(deferred.commands().any(|command| matches!(
            command,
            PreparedChampionshipSetupCommand::Deferred {
                command: DeferredChampionshipCommand::Selection(_),
                ..
            }
        )));
    });
}

fn selection_kind() -> PreparedKind {
    PreparedKind::Bitmap(
        ChampionshipBitmapIdentity::Selection,
        BlitMethod::Stretch,
        Some(ColorKey { low: 0, high: 0 }),
    )
}

fn opaque_fast(identity: ChampionshipBitmapIdentity) -> PreparedKind {
    PreparedKind::Bitmap(identity, BlitMethod::Fast, None)
}

fn row_text(kind: cmr_core::championship_setup::SetupRowKind) -> PreparedKind {
    PreparedKind::Text(SetupTextRole::Row(kind))
}

fn assert_preparation_parity(
    transcript: &ChampionshipSetupRenderTranscript,
    state: ChampionshipSetupFrameState,
    cache: ChampionshipPreviewCache,
    pool: &SoftwareChampionshipOrdinaryRgb16Pool,
    selection: &IndexedBitmap,
    expected: &[PreparedKind],
) {
    let alternate = prepare_software_championship_alternate_setup_frame(
        visual_input(transcript),
        state,
        cache,
        pool,
        Some(selection),
    )
    .unwrap();
    let shared = prepare_championship_setup_frame_from_source_extents(
        visual_input(transcript),
        state,
        cache,
        |identity| match identity {
            ChampionshipBitmapIdentity::Selection => Some((selection.row_stride, selection.height)),
            ChampionshipBitmapIdentity::InfoBox => {
                let [width, height] = pool.info_box().aligned_surface_extent();
                Some((width, height))
            }
            ChampionshipBitmapIdentity::ManufacturerLogo(logo) => {
                let [width, height] = pool.logo(logo).aligned_surface_extent();
                Some((width, height))
            }
            ChampionshipBitmapIdentity::ChequeredFrame(frame) => {
                let [width, height] = pool.frame(frame).aligned_surface_extent();
                Some((width, height))
            }
        },
    )
    .unwrap();

    let alternate_commands = alternate.commands().collect::<Vec<_>>();
    assert_eq!(alternate_commands, shared.commands().collect::<Vec<_>>());
    assert_eq!(prepared_kinds(&alternate_commands), expected);
}

fn prepared_kinds(commands: &[PreparedChampionshipSetupCommand]) -> Vec<PreparedKind> {
    commands
        .iter()
        .copied()
        .enumerate()
        .map(|(expected_index, command)| match command {
            PreparedChampionshipSetupCommand::Text {
                command_index,
                command,
            } => {
                assert_eq!(command_index, expected_index);
                PreparedKind::Text(command.role)
            }
            PreparedChampionshipSetupCommand::Bitmap {
                command_index,
                identity,
                decision,
            } => {
                assert_eq!(command_index, expected_index);
                match decision {
                    BlitDecision::NoOp => PreparedKind::NoOp(identity),
                    BlitDecision::Blit(plan) => {
                        PreparedKind::Bitmap(identity, plan.method, plan.color_key)
                    }
                }
            }
            PreparedChampionshipSetupCommand::Deferred {
                command_index,
                command,
            } => {
                assert_eq!(command_index, expected_index);
                match command {
                    DeferredChampionshipCommand::Selection(_) => PreparedKind::SelectionDeferred,
                    DeferredChampionshipCommand::InfoText(_) => PreparedKind::InfoText,
                    DeferredChampionshipCommand::CarTransform(_) => PreparedKind::Transform,
                    DeferredChampionshipCommand::CarVisibilityLatch(_) => PreparedKind::Latch,
                    DeferredChampionshipCommand::PaletteRequest { group, item } => {
                        PreparedKind::Palette(group, item)
                    }
                    DeferredChampionshipCommand::NextPreview(_) => PreparedKind::NextPreview,
                }
            }
        })
        .collect()
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
    selection: IndexedBitmap,
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
    ) -> PreparedChampionshipAlternateSetupFrame {
        prepare_software_championship_alternate_setup_frame(
            visual_input(transcript),
            ChampionshipSetupFrameState::default(),
            cache,
            self.pool,
            Some(&self.selection),
        )
        .unwrap()
    }

    fn prepare_next_setup(
        &self,
        transcript: &ChampionshipSetupRenderTranscript,
        cache: ChampionshipPreviewCache,
    ) -> PreparedChampionshipAlternateSetupFrame {
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
    ) -> PreparedChampionshipAlternateSetupFrame {
        let mut input = visual_input(transcript);
        input.frontend_offset_q16 = Q16Point::new(1000 << 16, 1000 << 16);
        self.prepare_setup_from_input(input, ChampionshipSetupFrameState::default(), cache)
    }

    fn prepare_setup_from_input(
        &self,
        input: ChampionshipSetupVisualInput<'_>,
        state: ChampionshipSetupFrameState,
        cache: ChampionshipPreviewCache,
    ) -> PreparedChampionshipAlternateSetupFrame {
        prepare_software_championship_alternate_setup_frame(
            input,
            state,
            cache,
            self.pool,
            Some(&self.selection),
        )
        .unwrap()
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_no_car(
        &mut self,
        prepared: PreparedChampionshipAlternateSetupFrame,
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
        prepared: PreparedChampionshipAlternateSetupFrame,
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
        prepared: PreparedChampionshipAlternateSetupFrame,
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
        prepared: PreparedChampionshipAlternateSetupFrame,
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
        prepared: PreparedChampionshipAlternateSetupFrame,
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
    with_source_fixture(|store, championship, _frame_assets, pool, selection| {
        let root = load_root_menu_c3d(store, VisualVariant::Normal, C3dLimits::default());
        let frontend =
            prepare_frontend_c3d_resources(championship, &root, BitmapLimits::default()).unwrap();
        let topology = frontend
            .car_topology(championship, CanonicalCar::Subaru)
            .unwrap();
        let runtime = frontend
            .materialize_car(CanonicalCar::Subaru, &topology)
            .unwrap();
        let descriptor =
            RawDdPixelFormat::from_dwords([32, 0x40, 0, 16, 0x7c00, 0x03e0, 0x001f, 0]);
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
        let (backend, destination) = split_backend(false);
        let mut fixture = Fixture {
            topology: &topology,
            runtime: &runtime,
            selection: selection.clone(),
            pool,
            executor,
            backend,
            destination,
        };
        run(&mut fixture);
    });
}

fn with_source_fixture(
    run: impl FnOnce(
        &AssetStore,
        &cmr_core::championship_assets::ChampionshipAssets,
        &cmr_core::championship_frame_assets::ChampionshipFrameAssets,
        &SoftwareChampionshipOrdinaryRgb16Pool,
        &IndexedBitmap,
    ),
) {
    let temporary = TestDirectory::new();
    let store = synthetic_store(&temporary);
    let championship =
        load_championship_assets(&store, C3dLimits::default(), BitmapLimits::default()).unwrap();
    let frame_assets = load_championship_frame_assets(&store, BitmapLimits::default()).unwrap();
    let pool =
        prepare_software_championship_ordinary_rgb16_pool(&championship, &frame_assets, 5).unwrap();
    let selection =
        parse_indexed_bitmap(&indexed_bitmap(256, 256, 1), BitmapLimits::default()).unwrap();
    run(&store, &championship, &frame_assets, &pool, &selection);
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

fn gear_transcript() -> ChampionshipSetupRenderTranscript {
    let mut transcript = car_transcript();
    transcript.rows[0].selected = false;
    transcript.rows[1].selected = true;
    transcript.highlight_row = 1;
    transcript.preview = ActivePreview::Gear {
        gear: GearChoice::BitClear,
        canonical_car: CanonicalCar::Subaru,
        object_name: CanonicalCar::Subaru.object_name(),
        logo_name: CanonicalCar::Subaru.logo_name(),
    };
    transcript
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
        files.insert(descriptor.path, indexed_bitmap(321, 256, 3));
    }
    files.insert(INFO_BOX_DESCRIPTOR.path, indexed_bitmap(321, 132, 2));
    files.insert(CHEQUERED_PALETTE_DESCRIPTOR.path, synthetic_palette());
    for descriptor in CHEQUERED_FRAME_DESCRIPTORS {
        files.insert(descriptor.path, indexed_bitmap(183, 128, 4));
    }
    files.insert(ROOT_MENU_SELECT_PATH, indexed_bitmap(256, 256, 1));
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
    bytes[54..58].copy_from_slice(&[9, 17, 25, 0]);
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
