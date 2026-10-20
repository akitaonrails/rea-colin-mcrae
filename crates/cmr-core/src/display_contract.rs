use std::fmt::Debug;

use cmr_formats::riff_palette::PaletteEntry;

use crate::display::{
    CooperativeMode, DisplayExtent, IndexedDisplay, IndexedMode, IndexedVideoBackend,
};
use crate::surface::IndexedSurface;

/// Read-only hooks used by the shared backend conformance suite. Every concrete
/// compatibility or modern video backend implements this in its test support
/// so the same observable contract runs against all of them.
pub trait IndexedVideoBackendProbe: IndexedVideoBackend + Default {
    fn probe_visible_pixels(&self) -> Option<&[u8]>;
    fn probe_back_pixels(&self) -> Option<&[u8]>;
    fn probe_palette(&self) -> Option<&[PaletteEntry; 256]>;
    fn probe_temporary_surface_count(&self) -> usize;
}

/// Shared backend contract. This intentionally tests only behavior already
/// recovered for every backend: both topology branches, palette replacement,
/// exact-size front/back copies, and temporary-source release.
pub fn check_indexed_video_backend<B>() -> Result<(), String>
where
    B: IndexedVideoBackendProbe,
    B::Error: Debug,
{
    check_mode::<B>(CooperativeMode::Windowed)?;
    check_mode::<B>(CooperativeMode::ExclusiveFullscreen)
}

fn check_mode<B>(cooperative: CooperativeMode) -> Result<(), String>
where
    B: IndexedVideoBackendProbe,
    B::Error: Debug,
{
    let initial_palette = palette(7);
    let replacement_palette = palette(19);
    let mode = IndexedMode {
        extent: DisplayExtent {
            width: 4,
            height: 1,
        },
        bits_per_pixel: 8,
    };
    let mut display = IndexedDisplay::initialize(B::default(), mode, cooperative, &initial_palette)
        .map_err(|error| format!("{cooperative:?} initialization failed: {error:?}"))?;
    display
        .upload_palette(&replacement_palette)
        .map_err(|error| format!("{cooperative:?} palette replacement failed: {error:?}"))?;
    let source = IndexedSurface {
        width: 4,
        height: 1,
        pitch: 4,
        pixels: vec![0, 1, 2, 3],
        palette: replacement_palette,
    };
    let outcome = display
        .show_startup_surface(&source)
        .map_err(|error| format!("{cooperative:?} startup copy failed: {error:?}"))?;
    outcome
        .front
        .map_err(|error| format!("{cooperative:?} visible copy failed: {error:?}"))?;
    outcome
        .back
        .map_err(|error| format!("{cooperative:?} back copy failed: {error:?}"))?;

    let backend = display.into_backend();
    if backend.probe_visible_pixels() != Some(source.pixels.as_slice()) {
        return Err(format!("{cooperative:?} visible pixels differ"));
    }
    if backend.probe_back_pixels() != Some(source.pixels.as_slice()) {
        return Err(format!("{cooperative:?} back pixels differ"));
    }
    if backend.probe_palette() != Some(&replacement_palette) {
        return Err(format!("{cooperative:?} active palette differs"));
    }
    if backend.probe_temporary_surface_count() != 0 {
        return Err(format!("{cooperative:?} temporary source leaked"));
    }
    Ok(())
}

fn palette(red: u8) -> [PaletteEntry; 256] {
    [PaletteEntry {
        red,
        green: 2,
        blue: 3,
        flags: 0,
    }; 256]
}
