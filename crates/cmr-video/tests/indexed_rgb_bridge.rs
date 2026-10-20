use cmr_core::{
    blit::{BlitDecision, BlitMethod, BlitPlan, ColorKey, RectEdges, RectI16, plan_legacy_blit},
    display::{CooperativeMode, DisplayExtent, IndexedDisplay, IndexedMode, IndexedVideoBackend},
    surface::IndexedSurface,
};
use cmr_formats::riff_palette::PaletteEntry;
use cmr_video::{
    SoftwareIndexedBackend,
    legacy_c3d_pixel_format::RawDdPixelFormat,
    packed_rgb_target::{FramebufferRgbFormat, LogicalMaskedRgbTarget},
};

const EXTENT: DisplayExtent = DisplayExtent {
    width: 4,
    height: 1,
};

#[test]
fn completed_indexed_keyed_composition_bridges_through_the_explicit_palette() {
    let palette = discriminating_palette();
    let display = indexed_display(&palette);
    let target = display.frame_target();
    let mut backend = display.into_backend();
    let full_row = edges(0, 4);

    let background = backend
        .create_source(&surface([9, 9, 9, 9]))
        .expect("create opaque background");
    backend
        .blit(
            background,
            target.back,
            BlitPlan {
                source: full_row,
                destination: full_row,
                method: BlitMethod::Fast,
                color_key: None,
            },
        )
        .expect("compose opaque background");

    let overlay = backend
        .create_source(&surface([0, 1, 0, 2]))
        .expect("create keyed overlay");
    backend
        .blit(
            overlay,
            target.back,
            BlitPlan {
                source: full_row,
                destination: full_row,
                method: BlitMethod::Fast,
                color_key: Some(ColorKey { low: 0, high: 0 }),
            },
        )
        .expect("compose keyed overlay");

    assert_eq!(backend.back_pixels(), Some([9, 1, 9, 2].as_slice()));
    let active_palette = *backend.active_palette().expect("active display palette");
    let mut logical = LogicalMaskedRgbTarget::new([4, 1], framebuffer_rgb565()).unwrap();
    logical
        .replace_from_indexed_palette(
            [4, 1],
            backend.back_pixels().expect("completed indexed frame"),
            &active_palette,
        )
        .expect("bridge completed indexed frame");

    // Index zero in the overlay preserves the prior blue index 9, while its
    // nonzero red and green indices replace the background at pixels 1 and 3.
    assert_eq!(logical.words(), &[0x001f, 0xf800, 0x001f, 0x07e0]);
}

#[test]
fn clipped_copy_bridges_shifted_indices_and_fully_clipped_work_is_a_no_op() {
    let palette = discriminating_palette();
    let display = indexed_display(&palette);
    let target = display.frame_target();
    let mut backend = display.into_backend();
    let full_row = edges(0, 4);

    let background = backend.create_source(&surface([9; 4])).unwrap();
    backend
        .blit(
            background,
            target.back,
            BlitPlan {
                source: full_row,
                destination: full_row,
                method: BlitMethod::Fast,
                color_key: None,
            },
        )
        .unwrap();

    let source = backend.create_source(&surface([1, 2, 3, 4])).unwrap();
    let BlitDecision::Blit(clipped) = plan_legacy_blit(
        rect(0, 4),
        rect(-1, 4),
        EXTENT.width as i16,
        EXTENT.height as i16,
        None,
    ) else {
        panic!("partially visible copy must remain a blit");
    };
    backend.blit(source, target.back, clipped).unwrap();
    assert_eq!(backend.back_pixels(), Some([2, 3, 4, 9].as_slice()));

    assert_eq!(
        plan_legacy_blit(
            rect(0, 4),
            rect(4, 4),
            EXTENT.width as i16,
            EXTENT.height as i16,
            None,
        ),
        BlitDecision::NoOp
    );

    let active_palette = *backend.active_palette().unwrap();
    let mut logical = LogicalMaskedRgbTarget::new([4, 1], framebuffer_rgb565()).unwrap();
    logical
        .replace_from_indexed_palette([4, 1], backend.back_pixels().unwrap(), &active_palette)
        .unwrap();
    assert_eq!(logical.words(), &[0x07e0, 0xffe0, 0xf81f, 0x001f]);
}

fn indexed_display(palette: &[PaletteEntry; 256]) -> IndexedDisplay<SoftwareIndexedBackend> {
    IndexedDisplay::initialize(
        SoftwareIndexedBackend::default(),
        IndexedMode {
            extent: EXTENT,
            bits_per_pixel: 8,
        },
        CooperativeMode::Windowed,
        palette,
    )
    .unwrap()
}

fn surface(pixels: [u8; 4]) -> IndexedSurface {
    IndexedSurface {
        width: 4,
        height: 1,
        pitch: 4,
        pixels: pixels.into(),
        // Uploaded-source palettes are intentionally irrelevant to the bridge:
        // conversion uses the explicitly supplied active display palette.
        palette: [PaletteEntry {
            red: 17,
            green: 19,
            blue: 23,
            flags: 29,
        }; 256],
    }
}

fn discriminating_palette() -> [PaletteEntry; 256] {
    let mut palette = [PaletteEntry {
        red: 0,
        green: 0,
        blue: 0,
        flags: 0,
    }; 256];
    palette[1].red = 255;
    palette[2].green = 255;
    palette[3] = PaletteEntry {
        red: 255,
        green: 255,
        blue: 0,
        flags: 31,
    };
    palette[4] = PaletteEntry {
        red: 255,
        green: 0,
        blue: 255,
        flags: 47,
    };
    palette[9].blue = 255;
    palette
}

const fn edges(left: i32, right: i32) -> RectEdges {
    RectEdges {
        left,
        top: 0,
        right,
        bottom: 1,
    }
}

const fn rect(x: i16, width: i16) -> RectI16 {
    RectI16 {
        x,
        y: 0,
        width,
        height: 1,
    }
}

fn framebuffer_rgb565() -> FramebufferRgbFormat {
    FramebufferRgbFormat::from_runtime_surface_descriptor(RawDdPixelFormat::from_dwords([
        32, 0x40, 0, 16, 0xf800, 0x07e0, 0x001f, 0,
    ]))
    .unwrap()
}
