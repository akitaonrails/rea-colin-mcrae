use cmr_formats::bitmap::{BitmapMetadata, BitmapPaletteEntry, IndexedBitmap};
use cmr_video::ordinary_bitmap_rgb16::{
    OrdinaryBitmapRgb16Error, OrdinaryBitmapRgb16Layout,
    convert_ordinary_bitmap_0x81_to_logical_rgb16,
};

#[test]
fn public_0x81_conversion_uses_exact_discriminator_and_complete_aligned_rows() {
    let bitmap = discriminating_bitmap();

    let rgb555 = convert_ordinary_bitmap_0x81_to_logical_rgb16(&bitmap, 5).unwrap();
    assert_eq!(rgb555.dib_extent(), [3, 2]);
    assert_eq!(rgb555.aligned_surface_extent(), [4, 2]);
    assert!(!rgb555.source_was_top_down());
    assert_eq!(rgb555.display_format_discriminator(), 5);
    assert_eq!(rgb555.layout(), OrdinaryBitmapRgb16Layout::Rgb555);
    assert_eq!(rgb555.stored_indices(), [0, 1, 2, 250, 2, 1, 0, 251]);
    assert_eq!(
        rgb555.logical_words(),
        [0x041f, 0, 0x7fff, 1, 0x7fff, 0, 0x041f, 0x0400]
    );
    assert_eq!(rgb555.packed_color_key(), 0x041f);

    for discriminator in [0, 4, 6, u8::MAX] {
        let rgb565 = convert_ordinary_bitmap_0x81_to_logical_rgb16(&bitmap, discriminator).unwrap();
        assert_eq!(rgb565.display_format_discriminator(), discriminator);
        assert_eq!(rgb565.layout(), OrdinaryBitmapRgb16Layout::Rgb565);
        assert_eq!(
            rgb565.logical_words(),
            [0x083f, 0x0020, 0xffff, 1, 0xffff, 0x0020, 0x083f, 0x0800]
        );
        assert_eq!(rgb565.packed_color_key(), 0x083f);
    }
}

#[test]
fn public_0x81_conversion_is_owned_by_each_bitmaps_embedded_palette() {
    let red = palette_variant(entry(255, 0, 0, 0x11));
    let blue = palette_variant(entry(0, 0, 255, 0xee));

    let red = convert_ordinary_bitmap_0x81_to_logical_rgb16(&red, 5).unwrap();
    let blue = convert_ordinary_bitmap_0x81_to_logical_rgb16(&blue, 5).unwrap();

    assert_eq!(red.stored_indices(), blue.stored_indices());
    assert_eq!(red.embedded_palette_rgb()[3], [255, 0, 0]);
    assert_eq!(blue.embedded_palette_rgb()[3], [0, 0, 255]);
    assert_eq!(red.logical_words(), [0x7c00; 4]);
    assert_eq!(blue.logical_words(), [0x001f; 4]);
    assert_ne!(red.logical_words(), blue.logical_words());
}

#[test]
fn public_0x81_key_is_embedded_entry_zero_after_the_selected_packing() {
    let mut bitmap = discriminating_bitmap();
    bitmap.palette[0] = entry(255, 129, 7, 0xff);

    let rgb555 = convert_ordinary_bitmap_0x81_to_logical_rgb16(&bitmap, 5).unwrap();
    let rgb565 = convert_ordinary_bitmap_0x81_to_logical_rgb16(&bitmap, 6).unwrap();

    assert_eq!(rgb555.packed_color_key(), 0x7e00);
    assert_eq!(rgb565.packed_color_key(), 0xfc00);
    assert_ne!(rgb555.packed_color_key(), 0);
    assert_ne!(rgb565.packed_color_key(), 0);
    assert_eq!(rgb555.logical_words()[0], rgb555.packed_color_key());
    assert_eq!(rgb565.logical_words()[0], rgb565.packed_color_key());
}

#[test]
fn public_0x81_validation_failures_return_no_output_and_leave_input_unchanged() {
    let valid = discriminating_bitmap();
    let expected = convert_ordinary_bitmap_0x81_to_logical_rgb16(&valid, 5).unwrap();

    let cases = [
        (
            mutate(&valid, |bitmap| bitmap.source_was_top_down = true),
            OrdinaryBitmapRgb16Error::UnsupportedTopDownSource,
        ),
        (
            mutate(&valid, |bitmap| bitmap.pixels[4] = 7),
            OrdinaryBitmapRgb16Error::LogicalStoredPixelMismatch {
                logical_offset: 4,
                stored_offset: 5,
                logical_index: 7,
                stored_index: 1,
            },
        ),
    ];

    for (malformed, error) in cases {
        let before = malformed.clone();
        assert_eq!(
            convert_ordinary_bitmap_0x81_to_logical_rgb16(&malformed, 5),
            Err(error)
        );
        assert_eq!(malformed, before);
    }

    assert_eq!(
        convert_ordinary_bitmap_0x81_to_logical_rgb16(&valid, 5),
        Ok(expected)
    );
}

fn discriminating_bitmap() -> IndexedBitmap {
    let mut palette = vec![entry(0, 0, 0, 0); 256];
    palette[0] = entry(8, 4, 248, 0x10);
    palette[1] = entry(0, 4, 0, 0x20);
    palette[2] = entry(248, 252, 248, 0x30);
    palette[250] = entry(0, 0, 8, 0x40);
    palette[251] = entry(8, 0, 0, 0x50);
    IndexedBitmap {
        width: 3,
        height: 2,
        source_was_top_down: false,
        palette,
        pixels: vec![0, 1, 2, 2, 1, 0],
        row_stride: 4,
        stored_pixels: vec![0, 1, 2, 250, 2, 1, 0, 251],
        metadata: metadata(),
    }
}

fn palette_variant(color: BitmapPaletteEntry) -> IndexedBitmap {
    let mut palette = vec![entry(1, 2, 3, 0); 256];
    palette[3] = color;
    IndexedBitmap {
        width: 4,
        height: 1,
        source_was_top_down: false,
        palette,
        pixels: vec![3; 4],
        row_stride: 4,
        stored_pixels: vec![3; 4],
        metadata: metadata(),
    }
}

fn mutate(source: &IndexedBitmap, mutate: impl FnOnce(&mut IndexedBitmap)) -> IndexedBitmap {
    let mut result = source.clone();
    mutate(&mut result);
    result
}

const fn entry(red: u8, green: u8, blue: u8, reserved: u8) -> BitmapPaletteEntry {
    BitmapPaletteEntry {
        red,
        green,
        blue,
        reserved,
    }
}

const fn metadata() -> BitmapMetadata {
    BitmapMetadata {
        reserved_1: 0,
        reserved_2: 0,
        x_pixels_per_meter: 0,
        y_pixels_per_meter: 0,
        important_colors: 0,
    }
}
