//! Opt-in smoke test for the production software root-menu assembly.
//!
//! This test reads only a user-created import and is ignored by default:
//!
//! ```text
//! CMR_RUNTIME_DATA=/path/to/runtime-data cargo test -p cmr-video \
//!   --test reviewed_import_session_smoke -- --ignored --nocapture
//! ```

use std::path::PathBuf;

use cmr_assets::{AssetStore, ReviewedProfile};
use cmr_core::deferred_palette::{DeferredPaletteCommitOutcome, PaletteArenaIndex};
use cmr_core::display::{CooperativeMode, DisplayExtent, IndexedMode};
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
use cmr_formats::stored_asset::DecodeLimits;
use cmr_video::SoftwareIndexedBackend;
use cmr_video::software_root_menu_session::{
    ROOT_MENU_DISK_PALETTE_INDEX, SoftwareRootMenuSessionRequest, with_software_root_menu_session,
};

const REVIEWED_PROFILE: &[u8] =
    include_bytes!("../../../profiles/installed-eu-derived-assets-v2.json");
const EXTENT: DisplayExtent = DisplayExtent {
    width: 640,
    height: 480,
};

const CONSERVATIVE_MODE2_STATE: C3dMode2LoadState = C3dMode2LoadState {
    prepared_resource_count: 0,
    corner_source: C3dMode2CornerSource::OrdinaryDefaults,
    surface_layout: C3dSurfaceLayoutGates {
        quarter_if_large: false,
        force_quarter: false,
        half_if_large: false,
        expand_width_dominant: false,
    },
};

#[test]
#[ignore = "requires a user-owned import selected with CMR_RUNTIME_DATA"]
fn reviewed_import_renders_two_root_frames_and_releases_uploaded_sources() {
    let runtime_root = std::env::var_os("CMR_RUNTIME_DATA")
        .map(PathBuf::from)
        .expect("CMR_RUNTIME_DATA must name an imported runtime-data directory");
    let reviewed = ReviewedProfile::from_json(REVIEWED_PROFILE).expect("reviewed profile is valid");
    let store = AssetStore::open(
        &runtime_root,
        &reviewed,
        DecodeLimits {
            max_stored_bytes: 64 * 1024 * 1024,
            max_payload_bytes: 256 * 1024 * 1024,
        },
    )
    .expect("CMR_RUNTIME_DATA must contain a complete matching reviewed import");

    let startup = initialize_startup_display(
        &store,
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
    .expect("reviewed English loading art must initialize the software display");

    let c3d_load_states = RootMenuC3dMode2LoadStates::new([
        CONSERVATIVE_MODE2_STATE,
        CONSERVATIVE_MODE2_STATE,
        CONSERVATIVE_MODE2_STATE,
        CONSERVATIVE_MODE2_STATE,
        CONSERVATIVE_MODE2_STATE,
        CONSERVATIVE_MODE2_STATE,
    ]);
    let request = SoftwareRootMenuSessionRequest::new(
        Language::English,
        VisualVariant::Normal,
        c3d_load_states,
    );

    let run = with_software_root_menu_session(&store, startup, request, |session| {
        assert!(session.uploaded_source_count() > 0);
        assert!(session.backend().presented_frame().is_none());
        assert_eq!(
            session.palette_state().pending(),
            Some(ROOT_MENU_DISK_PALETTE_INDEX)
        );
        for slot in [
            RootMenuBitmapSlot::ChampionshipGlobe,
            RootMenuBitmapSlot::TimeTrialGlobe,
            RootMenuBitmapSlot::Cone,
            RootMenuBitmapSlot::Disk,
            RootMenuBitmapSlot::Quit,
            RootMenuBitmapSlot::Spanner,
        ] {
            assert_eq!(session.bitmap_extent(slot), Some((208, 229)));
        }

        let first = session
            .render(root_input())
            .expect("first root frame renders");
        assert!(first.bitmap_blits > 0);
        assert!(first.footer.is_some());
        assert_eq!(first.c3d_commands, 1);
        assert!(first.scene_completed);
        assert_eq!(
            first.palette_commit,
            DeferredPaletteCommitOutcome::Committed(PaletteArenaIndex::new(0))
        );
        assert!(first.presentation_completed);

        let second = session
            .render(root_input())
            .expect("second root frame renders");
        assert!(second.scene_completed);
        assert_eq!(
            second.palette_commit,
            DeferredPaletteCommitOutcome::NoPendingPalette
        );
        assert!(second.presentation_completed);
        assert_eq!(
            session.palette_state().current(),
            Some(ROOT_MENU_DISK_PALETTE_INDEX)
        );
        assert_eq!(session.palette_state().pending(), None);
        assert_eq!(
            session.renderer_palette_cache(),
            session.global_palette_cache()
        );
        assert_eq!(
            session.backend().active_palette(),
            Some(session.renderer_palette_cache())
        );
        for slot in RootMenuC3dSlot::ALL {
            assert_eq!(session.visibility().mask(slot).raw(), 0);
        }

        let presented = session
            .backend()
            .presented_frame()
            .expect("terminal presentation captures the frame");
        assert_eq!(presented.extent(), EXTENT);
        assert_eq!(presented.palette(), session.renderer_palette_cache());
        assert_eq!(
            presented.pixels(),
            session
                .backend()
                .back_pixels()
                .expect("root session has a back buffer")
        );
        assert_eq!(presented.pixels().len(), 640 * 480);
    })
    .expect("reviewed assets build the production software root-menu session");

    assert_eq!(run.startup_copy.front, Ok(()));
    assert_eq!(run.startup_copy.back, Ok(()));
    assert!(run.backend.presented_frame().is_some());
    assert_eq!(run.backend.uploaded_source_count(), 0);
}

fn root_input() -> RootRenderInput {
    RootRenderInput {
        phase: FrontendPhase::Interactive,
        selection: RootSelection::new(0).expect("root selection zero is valid"),
        animation_position_q16: 0,
        language: Language::English,
        viewport_extent: EXTENT,
        horizontal_transform_q16: ROOT_INITIAL_HORIZONTAL_TRANSFORM_Q16,
        vertical_transform_q16: ROOT_INITIAL_VERTICAL_TRANSFORM_Q16,
        entry_visuals_suppressed: false,
    }
}
