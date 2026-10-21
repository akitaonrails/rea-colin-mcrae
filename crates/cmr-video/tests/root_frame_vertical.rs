use cmr_core::deferred_palette::{DeferredPaletteCommitOutcome, PaletteArenaIndex};
use cmr_core::display::{CooperativeMode, DisplayExtent, IndexedMode};
use cmr_core::font_assets::{FONT_COUNT, FontStyle};
use cmr_core::frontend::{FrontendPhase, RootSelection};
use cmr_core::localization::Language;
use cmr_core::root_menu_assets::VisualVariant;
use cmr_core::root_menu_bitmap_assets::RootMenuBitmapSlot;
use cmr_core::root_menu_c3d_assets::RootMenuC3dSlot;
use cmr_core::root_menu_c3d_runtime::{
    C3dMode2CornerSource, C3dMode2LoadState, C3dSurfaceLayoutGates, RootMenuC3dMode2LoadStates,
};
use cmr_core::root_menu_visual::{
    ROOT_INITIAL_HORIZONTAL_TRANSFORM_Q16, ROOT_INITIAL_VERTICAL_TRANSFORM_Q16, RootRenderInput,
};
use cmr_core::startup::{StartupLanguage, StartupScreen};
use cmr_core::startup_display::{StartupDisplayRequest, initialize_startup_display};
use cmr_core::surface::PaletteMapping;
use cmr_formats::bitmap::BitmapLimits;
use cmr_formats::riff_palette::PaletteEntry;
use cmr_test_support::SyntheticRootFrameFixture;
use cmr_video::SoftwareIndexedBackend;
use cmr_video::software_root_menu_session::{
    ROOT_MENU_DISK_PALETTE_INDEX, SoftwareRootMenuRenderError, SoftwareRootMenuSessionRequest,
    with_software_root_menu_session,
};

const EXTENT: DisplayExtent = DisplayExtent {
    width: 640,
    height: 480,
};
const C3D_DISPLAY_INDEX: u8 = 37;

#[test]
fn synthetic_root_frame_captures_type1_copy_layers_twice_without_reuploading() {
    let fixture = SyntheticRootFrameFixture::new();
    let disk_palette = *fixture.disk_palette();
    let expected_center_marker = fixture.expected_center_marker();
    let bootstrap_palette = disk_palette.map(|_| PaletteEntry {
        red: 0,
        green: 0,
        blue: 0,
        flags: 0,
    });
    let startup = initialize_startup_display(
        fixture.store(),
        SoftwareIndexedBackend::default(),
        StartupDisplayRequest {
            mode: IndexedMode {
                extent: EXTENT,
                bits_per_pixel: 8,
            },
            cooperative: CooperativeMode::ExclusiveFullscreen,
            screen: StartupScreen::Loading,
            language: StartupLanguage::English,
            mapping: PaletteMapping::Identity,
            bitmap_limits: BitmapLimits::default(),
        },
    )
    .unwrap();
    let load_states = RootMenuC3dMode2LoadStates::splat(C3dMode2LoadState {
        prepared_resource_count: 0,
        corner_source: C3dMode2CornerSource::OrdinaryDefaults,
        surface_layout: C3dSurfaceLayoutGates::default(),
    });
    let request =
        SoftwareRootMenuSessionRequest::new(Language::English, VisualVariant::Normal, load_states);
    let run = with_software_root_menu_session(fixture.store(), startup, request, |session| {
        let persistent_source_count = session.uploaded_source_count();
        assert_eq!(
            persistent_source_count,
            FONT_COUNT + RootMenuBitmapSlot::ALL.len()
        );
        assert!(session.bitmap_diagnostics().is_empty());
        assert_eq!(session.c3d_load_diagnostics().len(), 5);
        assert_eq!(session.c3d_preparation_diagnostic_count(), 0);
        assert!(session.backend().presented_frame().is_none());
        assert_eq!(session.backend().active_palette(), Some(&bootstrap_palette));
        assert_eq!(session.palette_state().current(), None);
        assert_eq!(
            session.palette_state().pending(),
            Some(ROOT_MENU_DISK_PALETTE_INDEX)
        );
        assert_eq!(session.renderer_palette_cache(), &bootstrap_palette);
        assert_eq!(session.global_palette_cache(), &bootstrap_palette);

        let mut wrong_language = root_input();
        wrong_language.language = Language::French;
        assert!(matches!(
            session.render(wrong_language),
            Err(SoftwareRootMenuRenderError::LanguageMismatch {
                expected: Language::English,
                actual: Language::French,
            })
        ));
        let mut wrong_extent = root_input();
        wrong_extent.viewport_extent.width -= 1;
        assert!(matches!(
            session.render(wrong_extent),
            Err(SoftwareRootMenuRenderError::ExtentMismatch { .. })
        ));
        assert!(session.backend().presented_frame().is_none());
        assert_eq!(session.uploaded_source_count(), persistent_source_count);
        assert_eq!(session.palette_state().current(), None);
        assert_eq!(
            session.palette_state().pending(),
            Some(ROOT_MENU_DISK_PALETTE_INDEX)
        );

        for (frame_index, expected_angle, expected_palette_commit) in [
            (
                0,
                8,
                DeferredPaletteCommitOutcome::Committed(PaletteArenaIndex::new(0)),
            ),
            (1, 16, DeferredPaletteCommitOutcome::NoPendingPalette),
        ] {
            let outcome = session.render(root_input()).unwrap();
            assert_eq!(outcome.bitmap_blits, 5);
            assert_eq!(outcome.bitmap_no_ops, 0);
            assert!(outcome.footer.is_some());
            assert_eq!(outcome.c3d_commands, 1);
            assert!(outcome.scene_completed);
            assert_eq!(outcome.palette_commit, expected_palette_commit);
            assert!(outcome.presentation_completed);
            for slot in RootMenuC3dSlot::ALL {
                assert_eq!(
                    session.visibility().mask(slot).raw(),
                    0x00,
                    "terminal scene must clear every root visibility latch"
                );
            }
            assert_eq!(
                session.angles().angle(RootMenuC3dSlot::Championship),
                expected_angle
            );
            assert_eq!(session.uploaded_source_count(), persistent_source_count);

            let backend = session.backend();
            let presented = backend.presented_frame().unwrap();
            assert_eq!(presented.extent(), EXTENT);
            assert_eq!(presented.palette(), &disk_palette);
            assert_ne!(presented.palette(), &bootstrap_palette);
            assert_eq!(session.renderer_palette_cache(), &disk_palette);
            assert_eq!(session.global_palette_cache(), &disk_palette);
            assert_eq!(presented.pixels(), backend.back_pixels().unwrap());
            assert_eq!(
                presented.pixels()[0],
                SyntheticRootFrameFixture::marker(RootMenuBitmapSlot::Backdrop)
            );
            assert_eq!(presented.pixels()[124 * 640 + 232], expected_center_marker);
            assert_eq!(
                presented.pixels()[470 * 640 + 320],
                FontStyle::GreenInfo as u8 + 1
            );
            assert_eq!(
                presented.pixels().contains(&C3D_DISPLAY_INDEX),
                frame_index == 1,
                "the first scene uses the zero-RGB bootstrap palette; later scenes use disk.pal"
            );
            let depth = backend.c3d_depth_values().unwrap();
            assert!(depth.iter().any(|value| *value < 1.0));
            assert!(depth.contains(&1.0));
        }
        assert_eq!(
            session.palette_state().current(),
            Some(ROOT_MENU_DISK_PALETTE_INDEX)
        );
        assert_eq!(session.palette_state().pending(), None);
    })
    .unwrap();

    assert_eq!(run.startup_copy.front, Ok(()));
    assert_eq!(run.startup_copy.back, Ok(()));
    assert_eq!(run.backend.uploaded_source_count(), 0);
}

fn root_input() -> RootRenderInput {
    RootRenderInput {
        phase: FrontendPhase::Interactive,
        selection: RootSelection::new(0).unwrap(),
        animation_position_q16: 0,
        language: Language::English,
        viewport_extent: EXTENT,
        horizontal_transform_q16: ROOT_INITIAL_HORIZONTAL_TRANSFORM_Q16,
        vertical_transform_q16: ROOT_INITIAL_VERTICAL_TRANSFORM_Q16,
        entry_visuals_suppressed: false,
    }
}
