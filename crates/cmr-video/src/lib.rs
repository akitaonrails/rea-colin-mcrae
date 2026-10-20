use std::fmt;

use cmr_core::blit::{BlitMethod, BlitPlan};
use cmr_core::display::{
    CooperativeMode, DisplaySurfaces, IndexedMode, IndexedVideoBackend, SurfaceTopology,
};
use cmr_core::display_contract::IndexedVideoBackendProbe;
use cmr_core::surface::IndexedSurface;
use cmr_formats::riff_palette::PaletteEntry;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SurfaceHandle(usize);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaletteHandle(usize);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SoftwareVideoError {
    InvalidSequence(&'static str),
    InvalidSurface(SurfaceHandle),
    InvalidPalette(PaletteHandle),
    InvalidRectangle,
    InvalidSourceLayout,
    UnsupportedColorKey,
    UnsupportedStretch,
}

impl fmt::Display for SoftwareVideoError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSequence(operation) => {
                write!(
                    formatter,
                    "software video operation out of sequence: {operation}"
                )
            }
            Self::InvalidSurface(handle) => write!(formatter, "invalid surface handle {handle:?}"),
            Self::InvalidPalette(handle) => write!(formatter, "invalid palette handle {handle:?}"),
            Self::InvalidRectangle => write!(formatter, "blit rectangle is outside a surface"),
            Self::InvalidSourceLayout => write!(formatter, "invalid indexed source layout"),
            Self::UnsupportedColorKey => {
                write!(
                    formatter,
                    "software compatibility backend only supports the recovered index-zero color key"
                )
            }
            Self::UnsupportedStretch => {
                write!(
                    formatter,
                    "software compatibility backend has no recovered stretch rule"
                )
            }
        }
    }
}

impl std::error::Error for SoftwareVideoError {}

#[derive(Clone, Debug)]
struct SoftwareSurface {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
    palette: Option<PaletteHandle>,
    temporary: bool,
}

/// Deterministic source-owned framebuffer used as a compatibility oracle.
/// It implements exact-size indexed copies only; scaling remains unsupported
/// until its DirectDraw sampling behavior is recovered.
#[derive(Default)]
pub struct SoftwareIndexedBackend {
    device_created: bool,
    cooperative: Option<CooperativeMode>,
    mode: Option<IndexedMode>,
    topology: Option<SurfaceTopology>,
    surfaces: Vec<Option<SoftwareSurface>>,
    visible_front: Option<SurfaceHandle>,
    back: Option<SurfaceHandle>,
    palette: Option<[PaletteEntry; 256]>,
    palette_uploaded: bool,
}

impl SoftwareIndexedBackend {
    pub fn visible_pixels(&self) -> Option<&[u8]> {
        self.visible_front.and_then(|handle| self.pixels(handle))
    }

    pub fn back_pixels(&self) -> Option<&[u8]> {
        self.back.and_then(|handle| self.pixels(handle))
    }

    pub fn active_palette(&self) -> Option<&[PaletteEntry; 256]> {
        self.palette.as_ref()
    }

    pub fn temporary_surface_count(&self) -> usize {
        self.surfaces
            .iter()
            .flatten()
            .filter(|surface| surface.temporary)
            .count()
    }

    fn pixels(&self, handle: SurfaceHandle) -> Option<&[u8]> {
        self.surfaces
            .get(handle.0)
            .and_then(Option::as_ref)
            .map(|surface| surface.pixels.as_slice())
    }

    fn surface(&self, handle: SurfaceHandle) -> Result<&SoftwareSurface, SoftwareVideoError> {
        self.surfaces
            .get(handle.0)
            .and_then(Option::as_ref)
            .ok_or(SoftwareVideoError::InvalidSurface(handle))
    }

    fn push_surface(&mut self, surface: SoftwareSurface) -> SurfaceHandle {
        let handle = SurfaceHandle(self.surfaces.len());
        self.surfaces.push(Some(surface));
        handle
    }
}

impl IndexedVideoBackendProbe for SoftwareIndexedBackend {
    fn probe_visible_pixels(&self) -> Option<&[u8]> {
        self.visible_pixels()
    }

    fn probe_back_pixels(&self) -> Option<&[u8]> {
        self.back_pixels()
    }

    fn probe_palette(&self) -> Option<&[PaletteEntry; 256]> {
        self.active_palette()
    }

    fn probe_temporary_surface_count(&self) -> usize {
        self.temporary_surface_count()
    }
}

impl IndexedVideoBackend for SoftwareIndexedBackend {
    type Error = SoftwareVideoError;
    type Surface = SurfaceHandle;
    type Palette = PaletteHandle;

    fn create_device(&mut self) -> Result<(), Self::Error> {
        if self.device_created {
            return Err(SoftwareVideoError::InvalidSequence("create_device"));
        }
        self.device_created = true;
        Ok(())
    }

    fn set_cooperative_mode(&mut self, mode: CooperativeMode) -> Result<(), Self::Error> {
        if !self.device_created || self.cooperative.is_some() {
            return Err(SoftwareVideoError::InvalidSequence("set_cooperative_mode"));
        }
        self.cooperative = Some(mode);
        Ok(())
    }

    fn set_display_mode(&mut self, mode: IndexedMode) -> Result<(), Self::Error> {
        if self.cooperative != Some(CooperativeMode::ExclusiveFullscreen) || self.mode.is_some() {
            return Err(SoftwareVideoError::InvalidSequence("set_display_mode"));
        }
        self.mode = Some(mode);
        Ok(())
    }

    fn create_surfaces(
        &mut self,
        topology: SurfaceTopology,
        mode: IndexedMode,
    ) -> Result<DisplaySurfaces<Self::Surface>, Self::Error> {
        if self.cooperative.is_none() || self.visible_front.is_some() {
            return Err(SoftwareVideoError::InvalidSequence("create_surfaces"));
        }
        let expected_topology = match self.cooperative {
            Some(CooperativeMode::Windowed) => SurfaceTopology::PrimaryAndOffscreen,
            Some(CooperativeMode::ExclusiveFullscreen) => {
                SurfaceTopology::FlippingChainWithOneBackBuffer
            }
            None => unreachable!(),
        };
        if topology != expected_topology {
            return Err(SoftwareVideoError::InvalidSequence(
                "create_surfaces topology",
            ));
        }
        if self.mode.is_some_and(|selected| selected != mode) {
            return Err(SoftwareVideoError::InvalidSequence("create_surfaces mode"));
        }
        self.mode = Some(mode);
        self.topology = Some(topology);
        let width = usize::from(mode.extent.width);
        let height = usize::from(mode.extent.height);
        let make_surface = || SoftwareSurface {
            width,
            height,
            pixels: vec![0; width * height],
            palette: None,
            temporary: false,
        };
        let visible_front = self.push_surface(make_surface());
        let back = self.push_surface(make_surface());
        self.visible_front = Some(visible_front);
        self.back = Some(back);
        Ok(DisplaySurfaces {
            visible_front,
            back,
        })
    }

    fn create_palette(
        &mut self,
        entries: &[PaletteEntry; 256],
    ) -> Result<Self::Palette, Self::Error> {
        if self.visible_front.is_none() || self.palette.is_some() {
            return Err(SoftwareVideoError::InvalidSequence("create_palette"));
        }
        self.palette = Some(*entries);
        self.palette_uploaded = false;
        Ok(PaletteHandle(0))
    }

    fn attach_palette(
        &mut self,
        surface: Self::Surface,
        palette: Self::Palette,
    ) -> Result<(), Self::Error> {
        if palette != PaletteHandle(0) || self.palette.is_none() {
            return Err(SoftwareVideoError::InvalidPalette(palette));
        }
        let selected = self
            .surfaces
            .get_mut(surface.0)
            .and_then(Option::as_mut)
            .ok_or(SoftwareVideoError::InvalidSurface(surface))?;
        selected.palette = Some(palette);
        Ok(())
    }

    fn upload_palette(
        &mut self,
        palette: Self::Palette,
        entries: &[PaletteEntry; 256],
    ) -> Result<(), Self::Error> {
        if palette != PaletteHandle(0) || self.palette.is_none() {
            return Err(SoftwareVideoError::InvalidPalette(palette));
        }
        let both_attached = [self.visible_front, self.back]
            .into_iter()
            .flatten()
            .all(|handle| {
                self.surface(handle)
                    .is_ok_and(|surface| surface.palette == Some(palette))
            });
        if !both_attached {
            return Err(SoftwareVideoError::InvalidSequence("upload_palette"));
        }
        self.palette = Some(*entries);
        self.palette_uploaded = true;
        Ok(())
    }

    fn create_source(&mut self, surface: &IndexedSurface) -> Result<Self::Surface, Self::Error> {
        if !self.palette_uploaded {
            return Err(SoftwareVideoError::InvalidSequence("create_source"));
        }
        let expected = u64::from(surface.width) * u64::from(surface.height);
        if surface.width == 0
            || surface.height == 0
            || surface.pitch != surface.width
            || u64::try_from(surface.pixels.len()).ok() != Some(expected)
        {
            return Err(SoftwareVideoError::InvalidSourceLayout);
        }
        Ok(self.push_surface(SoftwareSurface {
            width: surface.width as usize,
            height: surface.height as usize,
            pixels: surface.pixels.clone(),
            palette: None,
            temporary: true,
        }))
    }

    fn blit(
        &mut self,
        source: Self::Surface,
        destination: Self::Surface,
        plan: BlitPlan,
    ) -> Result<(), Self::Error> {
        if plan.method == BlitMethod::Stretch {
            return Err(SoftwareVideoError::UnsupportedStretch);
        }
        if plan
            .color_key
            .is_some_and(|key| key.low != 0 || key.high != 0)
        {
            return Err(SoftwareVideoError::UnsupportedColorKey);
        }
        let source_surface = self.surface(source)?.clone();
        let destination_surface = self
            .surfaces
            .get_mut(destination.0)
            .and_then(Option::as_mut)
            .ok_or(SoftwareVideoError::InvalidSurface(destination))?;
        let Some(source_width) = plan.source.right.checked_sub(plan.source.left) else {
            return Err(SoftwareVideoError::InvalidRectangle);
        };
        let Some(source_height) = plan.source.bottom.checked_sub(plan.source.top) else {
            return Err(SoftwareVideoError::InvalidRectangle);
        };
        let Some(destination_width) = plan.destination.right.checked_sub(plan.destination.left)
        else {
            return Err(SoftwareVideoError::InvalidRectangle);
        };
        let Some(destination_height) = plan.destination.bottom.checked_sub(plan.destination.top)
        else {
            return Err(SoftwareVideoError::InvalidRectangle);
        };
        if source_width <= 0
            || source_height <= 0
            || destination_width <= 0
            || destination_height <= 0
            || source_width != destination_width
            || source_height != destination_height
            || plan.source.left < 0
            || plan.source.top < 0
            || plan.destination.left < 0
            || plan.destination.top < 0
            || plan.source.right > source_surface.width as i32
            || plan.source.bottom > source_surface.height as i32
            || plan.destination.right > destination_surface.width as i32
            || plan.destination.bottom > destination_surface.height as i32
        {
            return Err(SoftwareVideoError::InvalidRectangle);
        }
        for row in 0..source_height as usize {
            let source_start =
                (plan.source.top as usize + row) * source_surface.width + plan.source.left as usize;
            let destination_start = (plan.destination.top as usize + row)
                * destination_surface.width
                + plan.destination.left as usize;
            let count = source_width as usize;
            let source_row = &source_surface.pixels[source_start..source_start + count];
            let destination_row =
                &mut destination_surface.pixels[destination_start..destination_start + count];
            if plan.color_key.is_some() {
                for (source_pixel, destination_pixel) in
                    source_row.iter().zip(destination_row.iter_mut())
                {
                    if *source_pixel != 0 {
                        *destination_pixel = *source_pixel;
                    }
                }
            } else {
                destination_row.copy_from_slice(source_row);
            }
        }
        Ok(())
    }

    fn destroy_source(&mut self, source: Self::Surface) {
        if let Some(slot) = self.surfaces.get_mut(source.0)
            && slot.as_ref().is_some_and(|surface| surface.temporary)
        {
            *slot = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cmr_core::display::{DisplayExtent, IndexedDisplay};
    use cmr_core::display_contract::check_indexed_video_backend;

    fn palette(red: u8) -> [PaletteEntry; 256] {
        [PaletteEntry {
            red,
            green: 2,
            blue: 3,
            flags: 0,
        }; 256]
    }

    fn surface(width: u32, pixels: Vec<u8>) -> IndexedSurface {
        IndexedSurface {
            width,
            height: 1,
            pitch: width,
            pixels,
            palette: palette(7),
        }
    }

    #[test]
    fn satisfies_shared_indexed_video_backend_contract() {
        check_indexed_video_backend::<SoftwareIndexedBackend>().unwrap();
    }

    #[test]
    fn unknown_stretch_rule_fails_both_copies_without_leaking_source() {
        let mut display = IndexedDisplay::initialize(
            SoftwareIndexedBackend::default(),
            IndexedMode {
                extent: DisplayExtent {
                    width: 8,
                    height: 1,
                },
                bits_per_pixel: 8,
            },
            CooperativeMode::Windowed,
            &palette(7),
        )
        .unwrap();
        let outcome = display
            .show_startup_surface(&surface(4, vec![0, 1, 2, 3]))
            .unwrap();
        assert_eq!(outcome.front, Err(SoftwareVideoError::UnsupportedStretch));
        assert_eq!(outcome.back, Err(SoftwareVideoError::UnsupportedStretch));
        assert_eq!(display.into_backend().temporary_surface_count(), 0);
    }

    #[test]
    fn malformed_fast_rectangle_returns_error_without_indexing() {
        let display = IndexedDisplay::initialize(
            SoftwareIndexedBackend::default(),
            IndexedMode {
                extent: DisplayExtent {
                    width: 4,
                    height: 1,
                },
                bits_per_pixel: 8,
            },
            CooperativeMode::Windowed,
            &palette(7),
        )
        .unwrap();
        let mut backend = display.into_backend();
        let destination = backend.visible_front.unwrap();
        let mut short = surface(4, vec![0, 1, 2, 3]);
        short.pixels.pop();
        assert_eq!(
            backend.create_source(&short),
            Err(SoftwareVideoError::InvalidSourceLayout)
        );
        let mut bad_pitch = surface(4, vec![0, 1, 2, 3]);
        bad_pitch.pitch = 8;
        assert_eq!(
            backend.create_source(&bad_pitch),
            Err(SoftwareVideoError::InvalidSourceLayout)
        );
        let source = backend
            .create_source(&surface(4, vec![0, 1, 2, 3]))
            .unwrap();
        let error = backend
            .blit(
                source,
                destination,
                BlitPlan {
                    source: cmr_core::blit::RectEdges {
                        left: 3,
                        top: 0,
                        right: 1,
                        bottom: 1,
                    },
                    destination: cmr_core::blit::RectEdges {
                        left: 3,
                        top: 0,
                        right: 1,
                        bottom: 1,
                    },
                    method: BlitMethod::Fast,
                    color_key: None,
                },
            )
            .unwrap_err();
        assert_eq!(error, SoftwareVideoError::InvalidRectangle);
        for (source_edges, destination_edges) in [
            (
                cmr_core::blit::RectEdges {
                    left: i32::MIN,
                    top: 0,
                    right: i32::MAX,
                    bottom: 1,
                },
                cmr_core::blit::RectEdges {
                    left: 0,
                    top: 0,
                    right: 4,
                    bottom: 1,
                },
            ),
            (
                cmr_core::blit::RectEdges {
                    left: 0,
                    top: 0,
                    right: 4,
                    bottom: 1,
                },
                cmr_core::blit::RectEdges {
                    left: i32::MIN,
                    top: 0,
                    right: i32::MAX,
                    bottom: 1,
                },
            ),
        ] {
            assert_eq!(
                backend.blit(
                    source,
                    destination,
                    BlitPlan {
                        source: source_edges,
                        destination: destination_edges,
                        method: BlitMethod::Fast,
                        color_key: None,
                    },
                ),
                Err(SoftwareVideoError::InvalidRectangle)
            );
        }
        let color_key_error = backend
            .blit(
                source,
                destination,
                BlitPlan {
                    source: cmr_core::blit::RectEdges {
                        left: 0,
                        top: 0,
                        right: 4,
                        bottom: 1,
                    },
                    destination: cmr_core::blit::RectEdges {
                        left: 0,
                        top: 0,
                        right: 4,
                        bottom: 1,
                    },
                    method: BlitMethod::Fast,
                    color_key: Some(cmr_core::blit::ColorKey { low: 1, high: 1 }),
                },
            )
            .unwrap_err();
        assert_eq!(color_key_error, SoftwareVideoError::UnsupportedColorKey);
    }

    #[test]
    fn index_zero_color_key_preserves_destination_pixels() {
        let display = IndexedDisplay::initialize(
            SoftwareIndexedBackend::default(),
            IndexedMode {
                extent: DisplayExtent {
                    width: 4,
                    height: 1,
                },
                bits_per_pixel: 8,
            },
            CooperativeMode::Windowed,
            &palette(7),
        )
        .unwrap();
        let mut backend = display.into_backend();
        let destination = backend.visible_front.unwrap();
        let destination_edges = cmr_core::blit::RectEdges {
            left: 0,
            top: 0,
            right: 4,
            bottom: 1,
        };

        let background = backend
            .create_source(&surface(4, vec![9, 9, 9, 9]))
            .unwrap();
        backend
            .blit(
                background,
                destination,
                BlitPlan {
                    source: destination_edges,
                    destination: destination_edges,
                    method: BlitMethod::Fast,
                    color_key: None,
                },
            )
            .unwrap();

        let glyph = backend
            .create_source(&surface(4, vec![0, 1, 0, 2]))
            .unwrap();
        backend
            .blit(
                glyph,
                destination,
                BlitPlan {
                    source: destination_edges,
                    destination: destination_edges,
                    method: BlitMethod::Fast,
                    color_key: Some(cmr_core::blit::ColorKey { low: 0, high: 0 }),
                },
            )
            .unwrap();

        assert_eq!(backend.visible_pixels(), Some([9, 1, 9, 2].as_slice()));
    }

    #[test]
    fn clipped_index_zero_key_uses_shifted_source_and_rejects_other_ranges() {
        let display = IndexedDisplay::initialize(
            SoftwareIndexedBackend::default(),
            IndexedMode {
                extent: DisplayExtent {
                    width: 4,
                    height: 1,
                },
                bits_per_pixel: 8,
            },
            CooperativeMode::Windowed,
            &palette(7),
        )
        .unwrap();
        let mut backend = display.into_backend();
        let destination = backend.visible_front.unwrap();
        let glyph = backend
            .create_source(&surface(4, vec![0, 1, 0, 2]))
            .unwrap();

        backend
            .blit(
                glyph,
                destination,
                BlitPlan {
                    source: cmr_core::blit::RectEdges {
                        left: 1,
                        top: 0,
                        right: 4,
                        bottom: 1,
                    },
                    destination: cmr_core::blit::RectEdges {
                        left: 0,
                        top: 0,
                        right: 3,
                        bottom: 1,
                    },
                    method: BlitMethod::Fast,
                    color_key: Some(cmr_core::blit::ColorKey { low: 0, high: 0 }),
                },
            )
            .unwrap();
        assert_eq!(backend.visible_pixels(), Some([1, 0, 2, 0].as_slice()));

        for color_key in [
            cmr_core::blit::ColorKey { low: 0, high: 1 },
            cmr_core::blit::ColorKey { low: 1, high: 0 },
        ] {
            assert_eq!(
                backend.blit(
                    glyph,
                    destination,
                    BlitPlan {
                        source: cmr_core::blit::RectEdges {
                            left: 0,
                            top: 0,
                            right: 4,
                            bottom: 1,
                        },
                        destination: cmr_core::blit::RectEdges {
                            left: 0,
                            top: 0,
                            right: 4,
                            bottom: 1,
                        },
                        method: BlitMethod::Fast,
                        color_key: Some(color_key),
                    },
                ),
                Err(SoftwareVideoError::UnsupportedColorKey)
            );
        }
    }

    #[test]
    fn rejects_out_of_sequence_backend_transitions() {
        let mode = IndexedMode {
            extent: DisplayExtent {
                width: 4,
                height: 1,
            },
            bits_per_pixel: 8,
        };
        let other_mode = IndexedMode {
            extent: DisplayExtent {
                width: 8,
                height: 1,
            },
            bits_per_pixel: 8,
        };
        let entries = palette(7);

        let mut backend = SoftwareIndexedBackend::default();
        assert_eq!(
            backend.set_cooperative_mode(CooperativeMode::Windowed),
            Err(SoftwareVideoError::InvalidSequence("set_cooperative_mode"))
        );
        backend.create_device().unwrap();
        assert_eq!(
            backend.create_device(),
            Err(SoftwareVideoError::InvalidSequence("create_device"))
        );
        backend
            .set_cooperative_mode(CooperativeMode::Windowed)
            .unwrap();
        assert_eq!(
            backend.set_cooperative_mode(CooperativeMode::Windowed),
            Err(SoftwareVideoError::InvalidSequence("set_cooperative_mode"))
        );
        assert_eq!(
            backend.set_display_mode(mode),
            Err(SoftwareVideoError::InvalidSequence("set_display_mode"))
        );
        assert_eq!(
            backend.create_surfaces(SurfaceTopology::FlippingChainWithOneBackBuffer, mode),
            Err(SoftwareVideoError::InvalidSequence(
                "create_surfaces topology"
            ))
        );
        let surfaces = backend
            .create_surfaces(SurfaceTopology::PrimaryAndOffscreen, mode)
            .unwrap();
        assert_eq!(
            backend.create_surfaces(SurfaceTopology::PrimaryAndOffscreen, mode),
            Err(SoftwareVideoError::InvalidSequence("create_surfaces"))
        );
        let palette_handle = backend.create_palette(&entries).unwrap();
        assert_eq!(
            backend.create_palette(&entries),
            Err(SoftwareVideoError::InvalidSequence("create_palette"))
        );
        assert_eq!(
            backend.upload_palette(palette_handle, &entries),
            Err(SoftwareVideoError::InvalidSequence("upload_palette"))
        );
        backend
            .attach_palette(surfaces.visible_front, palette_handle)
            .unwrap();
        assert_eq!(
            backend.upload_palette(palette_handle, &entries),
            Err(SoftwareVideoError::InvalidSequence("upload_palette"))
        );
        assert_eq!(
            backend.create_source(&surface(4, vec![0, 1, 2, 3])),
            Err(SoftwareVideoError::InvalidSequence("create_source"))
        );
        backend
            .attach_palette(surfaces.back, palette_handle)
            .unwrap();
        backend.upload_palette(palette_handle, &entries).unwrap();

        let mut no_surfaces = SoftwareIndexedBackend::default();
        no_surfaces.create_device().unwrap();
        no_surfaces
            .set_cooperative_mode(CooperativeMode::Windowed)
            .unwrap();
        assert_eq!(
            no_surfaces.create_palette(&entries),
            Err(SoftwareVideoError::InvalidSequence("create_palette"))
        );

        let mut fullscreen = SoftwareIndexedBackend::default();
        fullscreen.create_device().unwrap();
        fullscreen
            .set_cooperative_mode(CooperativeMode::ExclusiveFullscreen)
            .unwrap();
        fullscreen.set_display_mode(mode).unwrap();
        assert_eq!(
            fullscreen
                .create_surfaces(SurfaceTopology::FlippingChainWithOneBackBuffer, other_mode,),
            Err(SoftwareVideoError::InvalidSequence("create_surfaces mode"))
        );
    }

    #[test]
    fn rejects_zero_excess_and_vertical_overflow_inputs() {
        let display = IndexedDisplay::initialize(
            SoftwareIndexedBackend::default(),
            IndexedMode {
                extent: DisplayExtent {
                    width: 4,
                    height: 1,
                },
                bits_per_pixel: 8,
            },
            CooperativeMode::Windowed,
            &palette(7),
        )
        .unwrap();
        let mut backend = display.into_backend();
        let destination = backend.visible_front.unwrap();
        for invalid in [
            IndexedSurface {
                width: 0,
                height: 1,
                pitch: 0,
                pixels: Vec::new(),
                palette: palette(7),
            },
            IndexedSurface {
                width: 4,
                height: 0,
                pitch: 4,
                pixels: Vec::new(),
                palette: palette(7),
            },
            surface(4, vec![0, 1, 2, 3, 4]),
        ] {
            assert_eq!(
                backend.create_source(&invalid),
                Err(SoftwareVideoError::InvalidSourceLayout)
            );
        }

        let source = backend
            .create_source(&surface(4, vec![0, 1, 2, 3]))
            .unwrap();
        for (source_edges, destination_edges) in [
            (
                cmr_core::blit::RectEdges {
                    left: 0,
                    top: i32::MIN,
                    right: 4,
                    bottom: i32::MAX,
                },
                cmr_core::blit::RectEdges {
                    left: 0,
                    top: 0,
                    right: 4,
                    bottom: 1,
                },
            ),
            (
                cmr_core::blit::RectEdges {
                    left: 0,
                    top: 0,
                    right: 4,
                    bottom: 1,
                },
                cmr_core::blit::RectEdges {
                    left: 0,
                    top: i32::MIN,
                    right: 4,
                    bottom: i32::MAX,
                },
            ),
        ] {
            assert_eq!(
                backend.blit(
                    source,
                    destination,
                    BlitPlan {
                        source: source_edges,
                        destination: destination_edges,
                        method: BlitMethod::Fast,
                        color_key: None,
                    },
                ),
                Err(SoftwareVideoError::InvalidRectangle)
            );
        }
    }
}
