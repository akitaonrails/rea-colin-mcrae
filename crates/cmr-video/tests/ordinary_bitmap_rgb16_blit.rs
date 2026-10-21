use cmr_core::blit::{
    BlitDecision, BlitMethod, BlitPlan, ColorKey, RectEdges, RectI16, plan_legacy_blit,
};
use cmr_core::{
    display::DisplayExtent,
    frontend_c3d_scene::{C3dDepthClear, C3dScenePrimaryError, execute_frontend_c3d_scene},
    root_menu_c3d_render_state::{
        C3dGraphicsBranch, ROOT_MENU_C3D_INITIAL_FILTER_REQUEST_CACHE,
        root_menu_c3d_filter_cache_outcome, root_menu_c3d_legacy_alpha_contract,
    },
};
use cmr_formats::bitmap::{BitmapMetadata, BitmapPaletteEntry, IndexedBitmap};
use cmr_video::{
    c3d_software_raster::{SoftwareC3dSplitRenderDepth, SoftwareChampionshipCarSplitOracleProfile},
    c3d_software_split_backend::{
        SoftwareSplitC3dBackendError, SoftwareSplitC3dOracleBackend,
        SoftwareSplitC3dOracleConfiguration,
    },
    legacy_c3d_pixel_format::RawDdPixelFormat,
    ordinary_bitmap_rgb16::{
        OrdinaryBitmapLogicalRgb16Source, OrdinaryBitmapRgb16Layout,
        convert_ordinary_bitmap_0x81_to_logical_rgb16,
    },
    ordinary_bitmap_rgb16_blit::{
        OrdinaryBitmapRgb16BlitError, compose_ordinary_bitmap_logical_rgb16_fast_blit,
        preflight_ordinary_bitmap_logical_rgb16_fast_blit,
    },
    packed_rgb_target::{FramebufferRgbFormat, LogicalMaskedRgbTarget},
};

#[test]
fn public_fast_blit_copies_packed_words_exactly_without_repacking() {
    let source = converted_source(4, 2, &[1, 2, 3, 4, 4, 3, 2, 1], 5);
    let mut target = target([4, 2], OrdinaryBitmapRgb16Layout::Rgb555);
    let plan = fast_plan(edges(0, 0, 4, 2), edges(0, 0, 4, 2), None);

    preflight_ordinary_bitmap_logical_rgb16_fast_blit(&source, &target, plan).unwrap();
    compose_ordinary_bitmap_logical_rgb16_fast_blit(&source, &mut target, plan).unwrap();

    assert_eq!(
        target.words(),
        source
            .logical_words()
            .iter()
            .copied()
            .map(u32::from)
            .collect::<Vec<_>>()
    );
}

#[test]
fn public_keyed_blit_uses_the_nonzero_packed_key_and_can_copy_aligned_padding() {
    // The fourth stored byte is DIB row alignment, but the recovered ordinary
    // conversion deliberately made it part of the logical source surface.
    let source = converted_source(3, 1, &[0, 1, 2, 4], 6);
    assert_ne!(source.packed_color_key(), 0);
    let mut target = target([4, 1], OrdinaryBitmapRgb16Layout::Rgb565);
    target
        .replace_from_canonical_rgb([4, 1], &[[255, 0, 255]; 4])
        .unwrap();
    let preserved = target.words()[0];
    let key = u32::from(source.packed_color_key());

    compose_ordinary_bitmap_logical_rgb16_fast_blit(
        &source,
        &mut target,
        fast_plan(
            edges(0, 0, 4, 1),
            edges(0, 0, 4, 1),
            Some(ColorKey {
                low: key,
                high: key,
            }),
        ),
    )
    .unwrap();

    assert_eq!(target.words()[0], preserved);
    assert_eq!(
        &target.words()[1..],
        &source.logical_words()[1..]
            .iter()
            .copied()
            .map(u32::from)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        target.words()[3],
        u32::from(source.logical_words()[3]),
        "the aligned padding word must be copied when explicitly selected"
    );
}

#[test]
fn public_fast_blit_requires_matching_555_or_565_layouts() {
    for (discriminator, source_layout, other_layout) in [
        (
            5,
            OrdinaryBitmapRgb16Layout::Rgb555,
            OrdinaryBitmapRgb16Layout::Rgb565,
        ),
        (
            6,
            OrdinaryBitmapRgb16Layout::Rgb565,
            OrdinaryBitmapRgb16Layout::Rgb555,
        ),
    ] {
        let source = converted_source(4, 1, &[1, 2, 3, 4], discriminator);
        let plan = fast_plan(edges(0, 0, 4, 1), edges(0, 0, 4, 1), None);

        let mut matching = target([4, 1], source_layout);
        compose_ordinary_bitmap_logical_rgb16_fast_blit(&source, &mut matching, plan).unwrap();
        assert_eq!(
            matching.words(),
            source
                .logical_words()
                .iter()
                .copied()
                .map(u32::from)
                .collect::<Vec<_>>()
        );

        let mut mismatching = target([4, 1], other_layout);
        mismatching
            .replace_from_canonical_rgb([4, 1], &[[17, 31, 47]; 4])
            .unwrap();
        let before = mismatching.clone();
        assert!(matches!(
            compose_ordinary_bitmap_logical_rgb16_fast_blit(
                &source,
                &mut mismatching,
                plan
            ),
            Err(OrdinaryBitmapRgb16BlitError::DestinationLayoutMismatch {
                source: actual,
                ..
            }) if actual == source_layout
        ));
        assert_eq!(mismatching, before);
    }
}

#[test]
fn public_fast_blit_accepts_the_clipped_equal_size_plan() {
    let source = converted_source(4, 2, &[1, 2, 3, 4, 4, 3, 2, 1], 6);
    let mut target = target([3, 2], OrdinaryBitmapRgb16Layout::Rgb565);
    let BlitDecision::Blit(plan) =
        plan_legacy_blit(rect(0, 0, 4, 2), rect(-1, 0, 4, 2), 3, 2, None)
    else {
        panic!("a partially visible copy must remain a blit");
    };
    assert_eq!(plan.method, BlitMethod::Fast);

    compose_ordinary_bitmap_logical_rgb16_fast_blit(&source, &mut target, plan).unwrap();

    assert_eq!(
        target.words(),
        [
            source.logical_words()[1],
            source.logical_words()[2],
            source.logical_words()[3],
            source.logical_words()[5],
            source.logical_words()[6],
            source.logical_words()[7],
        ]
        .map(u32::from)
    );
}

#[test]
fn sequential_fast_blits_preserve_submission_order() {
    let first = converted_source(4, 1, &[1, 1, 1, 1], 5);
    let second = converted_source(4, 1, &[2, 3, 4, 2], 5);
    let mut target = target([4, 1], OrdinaryBitmapRgb16Layout::Rgb555);

    compose_ordinary_bitmap_logical_rgb16_fast_blit(
        &first,
        &mut target,
        fast_plan(edges(0, 0, 4, 1), edges(0, 0, 4, 1), None),
    )
    .unwrap();
    compose_ordinary_bitmap_logical_rgb16_fast_blit(
        &second,
        &mut target,
        fast_plan(edges(1, 0, 3, 1), edges(0, 0, 2, 1), None),
    )
    .unwrap();

    assert_eq!(
        target.words(),
        [
            second.logical_words()[1],
            second.logical_words()[2],
            first.logical_words()[2],
            first.logical_words()[3],
        ]
        .map(u32::from)
    );
}

#[test]
fn unsupported_modes_keys_and_malformed_rectangles_are_atomic() {
    let source = converted_source(4, 1, &[0, 1, 2, 3], 6);
    let mut target = target([4, 1], OrdinaryBitmapRgb16Layout::Rgb565);
    target
        .replace_from_canonical_rgb([4, 1], &[[13, 29, 61]; 4])
        .unwrap();
    let before = target.clone();
    let key = u32::from(source.packed_color_key());
    let full = edges(0, 0, 4, 1);
    let cases = [
        (
            BlitPlan {
                source: full,
                destination: full,
                method: BlitMethod::Stretch,
                color_key: None,
            },
            OrdinaryBitmapRgb16BlitError::UnsupportedStretch,
        ),
        (
            fast_plan(
                full,
                full,
                Some(ColorKey {
                    low: key,
                    high: key + 1,
                }),
            ),
            OrdinaryBitmapRgb16BlitError::UnsupportedColorKeyRange(ColorKey {
                low: key,
                high: key + 1,
            }),
        ),
        (
            fast_plan(
                full,
                full,
                Some(ColorKey {
                    low: key + 1,
                    high: key + 1,
                }),
            ),
            OrdinaryBitmapRgb16BlitError::UnsupportedColorKey {
                expected: source.packed_color_key(),
                actual: key + 1,
            },
        ),
        (
            fast_plan(edges(-1, 0, 3, 1), full, None),
            OrdinaryBitmapRgb16BlitError::InvalidSourceRectangle(edges(-1, 0, 3, 1)),
        ),
        (
            fast_plan(full, edges(1, 0, 5, 1), None),
            OrdinaryBitmapRgb16BlitError::InvalidDestinationRectangle(edges(1, 0, 5, 1)),
        ),
        (
            fast_plan(edges(0, 0, 3, 1), full, None),
            OrdinaryBitmapRgb16BlitError::SourceDestinationSizeMismatch {
                source: [3, 1],
                destination: [4, 1],
            },
        ),
    ];

    for (plan, expected) in cases {
        assert_eq!(
            preflight_ordinary_bitmap_logical_rgb16_fast_blit(&source, &target, plan),
            Err(expected)
        );
        assert_eq!(
            compose_ordinary_bitmap_logical_rgb16_fast_blit(&source, &mut target, plan),
            Err(expected)
        );
        assert_eq!(target, before);
    }
}

#[test]
fn split_backend_rejects_foreign_targets_and_blits_during_an_active_scene() {
    const EXTENT: DisplayExtent = DisplayExtent {
        width: 4,
        height: 1,
    };
    let source = converted_source(4, 1, &[1, 2, 3, 4], 6);
    let plan = fast_plan(edges(0, 0, 4, 1), edges(0, 0, 4, 1), None);
    let mut backend = split_backend(EXTENT);
    let own_handle = backend.target_handle();
    let foreign_handle = split_backend(EXTENT).target_handle();
    backend
        .replace_logical_target_from_canonical_rgb([4, 1], &[[13, 29, 61]; 4])
        .unwrap();
    let before = backend.logical_target().clone();

    assert_eq!(
        backend.preflight_ordinary_bitmap_logical_rgb16_fast_blit(foreign_handle, &source, plan,),
        Err(SoftwareSplitC3dBackendError::ForeignTarget {
            expected: own_handle,
            actual: foreign_handle,
        })
    );
    assert_eq!(
        backend.compose_ordinary_bitmap_logical_rgb16_fast_blit(foreign_handle, &source, plan,),
        Err(SoftwareSplitC3dBackendError::ForeignTarget {
            expected: own_handle,
            actual: foreign_handle,
        })
    );
    assert_eq!(backend.logical_target(), &before);

    let error = execute_frontend_c3d_scene(
        &mut backend,
        own_handle,
        EXTENT,
        C3dDepthClear::Clear,
        Some(()),
        |active, ()| {
            active.compose_ordinary_bitmap_logical_rgb16_fast_blit(own_handle, &source, plan)
        },
    )
    .unwrap_err();
    assert_eq!(
        error.primary,
        C3dScenePrimaryError::Submit(SoftwareSplitC3dBackendError::InvalidSequence(
            "compose_ordinary_bitmap_logical_rgb16_fast_blit",
        ))
    );
    assert_eq!(error.cleanup, None);
    assert_eq!(backend.logical_target(), &before);

    // EndScene cleanup after the rejected submission restores the inactive
    // state, so the same source and handle can subsequently compose.
    backend
        .compose_ordinary_bitmap_logical_rgb16_fast_blit(own_handle, &source, plan)
        .unwrap();
    assert_eq!(
        backend.logical_target().words(),
        source
            .logical_words()
            .iter()
            .copied()
            .map(u32::from)
            .collect::<Vec<_>>()
    );
}

fn converted_source(
    width: u32,
    height: u32,
    stored_pixels: &[u8],
    discriminator: u8,
) -> OrdinaryBitmapLogicalRgb16Source {
    let row_stride = (width + 3) & !3;
    assert_eq!(stored_pixels.len(), (row_stride * height) as usize);
    let pixels = stored_pixels
        .chunks_exact(row_stride as usize)
        .flat_map(|row| row[..width as usize].iter().copied())
        .collect();
    let bitmap = IndexedBitmap {
        width,
        height,
        source_was_top_down: false,
        palette: discriminating_palette(),
        pixels,
        row_stride,
        stored_pixels: stored_pixels.to_vec(),
        metadata: BitmapMetadata {
            reserved_1: 0,
            reserved_2: 0,
            x_pixels_per_meter: 0,
            y_pixels_per_meter: 0,
            important_colors: 0,
        },
    };
    convert_ordinary_bitmap_0x81_to_logical_rgb16(&bitmap, discriminator).unwrap()
}

fn discriminating_palette() -> Vec<BitmapPaletteEntry> {
    let mut palette = vec![entry(0, 0, 0); 256];
    // Entry zero is deliberately nonzero: keying must compare the packed word,
    // not the original palette index or numeric zero.
    palette[0] = entry(8, 132, 248);
    palette[1] = entry(248, 0, 0);
    palette[2] = entry(0, 252, 0);
    palette[3] = entry(0, 0, 248);
    palette[4] = entry(248, 252, 248);
    palette
}

const fn entry(red: u8, green: u8, blue: u8) -> BitmapPaletteEntry {
    BitmapPaletteEntry {
        red,
        green,
        blue,
        reserved: 0,
    }
}

fn target(extent: [u16; 2], layout: OrdinaryBitmapRgb16Layout) -> LogicalMaskedRgbTarget {
    let masks = match layout {
        OrdinaryBitmapRgb16Layout::Rgb555 => [0x7c00, 0x03e0, 0x001f],
        OrdinaryBitmapRgb16Layout::Rgb565 => [0xf800, 0x07e0, 0x001f],
    };
    let format =
        FramebufferRgbFormat::from_runtime_surface_descriptor(RawDdPixelFormat::from_dwords([
            32, 0x40, 0, 16, masks[0], masks[1], masks[2], 0,
        ]))
        .unwrap();
    LogicalMaskedRgbTarget::new(extent, format).unwrap()
}

fn split_backend(extent: DisplayExtent) -> SoftwareSplitC3dOracleBackend {
    SoftwareSplitC3dOracleBackend::new(
        extent,
        framebuffer_format(OrdinaryBitmapRgb16Layout::Rgb565),
        SoftwareSplitC3dOracleConfiguration {
            profile: SoftwareChampionshipCarSplitOracleProfile::new(
                SoftwareC3dSplitRenderDepth::Bits32,
                false,
                false,
            ),
            alpha_contract: root_menu_c3d_legacy_alpha_contract(0),
            filter_cache_outcome: root_menu_c3d_filter_cache_outcome(
                C3dGraphicsBranch::Alternate,
                true,
                ROOT_MENU_C3D_INITIAL_FILTER_REQUEST_CACHE,
            ),
        },
    )
    .unwrap()
}

fn framebuffer_format(layout: OrdinaryBitmapRgb16Layout) -> FramebufferRgbFormat {
    let masks = match layout {
        OrdinaryBitmapRgb16Layout::Rgb555 => [0x7c00, 0x03e0, 0x001f],
        OrdinaryBitmapRgb16Layout::Rgb565 => [0xf800, 0x07e0, 0x001f],
    };
    FramebufferRgbFormat::from_runtime_surface_descriptor(RawDdPixelFormat::from_dwords([
        32, 0x40, 0, 16, masks[0], masks[1], masks[2], 0,
    ]))
    .unwrap()
}

const fn fast_plan(
    source: RectEdges,
    destination: RectEdges,
    color_key: Option<ColorKey>,
) -> BlitPlan {
    BlitPlan {
        source,
        destination,
        method: BlitMethod::Fast,
        color_key,
    }
}

const fn edges(left: i32, top: i32, right: i32, bottom: i32) -> RectEdges {
    RectEdges {
        left,
        top,
        right,
        bottom,
    }
}

const fn rect(x: i16, y: i16, width: i16, height: i16) -> RectI16 {
    RectI16 {
        x,
        y,
        width,
        height,
    }
}
