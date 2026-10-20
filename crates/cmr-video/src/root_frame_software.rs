//! Headless presentation capture for the indexed software framebuffer.
//!
//! Capturing an owned indexed frame makes the completed compatibility output
//! observable without assigning native window, DirectDraw Flip, synchronization,
//! or windowed-copy semantics to this backend.

use std::fmt;

use cmr_core::{display::DisplayExtent, root_menu_frame::RootFrameHooks};
use cmr_formats::riff_palette::PaletteEntry;

use crate::{PaletteHandle, SoftwareIndexedBackend, SoftwareVideoError, SurfaceHandle};

/// One immutable indexed frame emitted by the headless software oracle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SoftwarePresentedFrame {
    extent: DisplayExtent,
    pixels: Box<[u8]>,
    palette: Box<[PaletteEntry; 256]>,
}

impl SoftwarePresentedFrame {
    #[must_use]
    pub const fn extent(&self) -> DisplayExtent {
        self.extent
    }

    #[must_use]
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    #[must_use]
    pub fn palette(&self) -> &[PaletteEntry; 256] {
        &self.palette
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SoftwarePresentationError {
    InvalidSequence(&'static str),
    Video(SoftwareVideoError),
    DestinationIsNotBackBuffer {
        destination: SurfaceHandle,
    },
    DestinationExtentMismatch {
        expected: DisplayExtent,
        actual: DisplayExtent,
    },
    MissingActivePalette,
    PaletteNotAttached,
    InvalidPixelLength {
        expected: usize,
        actual: usize,
    },
}

impl fmt::Display for SoftwarePresentationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "software headless presentation failed: {self:?}")
    }
}

impl std::error::Error for SoftwarePresentationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Video(source) => Some(source),
            _ => None,
        }
    }
}

impl RootFrameHooks for SoftwareIndexedBackend {
    type FinalError = SoftwarePresentationError;

    fn present_root_frame(
        &mut self,
        destination: Self::Surface,
        destination_extent: DisplayExtent,
    ) -> Result<(), Self::FinalError> {
        if self.c3d.scene_is_active() {
            return Err(SoftwarePresentationError::InvalidSequence(
                "present_root_frame during active C3D scene",
            ));
        }
        if self.back != Some(destination) {
            return Err(SoftwarePresentationError::DestinationIsNotBackBuffer { destination });
        }
        if !self.palette_uploaded {
            return Err(SoftwarePresentationError::MissingActivePalette);
        }
        let palette = self
            .palette
            .ok_or(SoftwarePresentationError::MissingActivePalette)?;
        let frame = {
            let surface = self
                .surface(destination)
                .map_err(SoftwarePresentationError::Video)?;
            let actual = DisplayExtent {
                width: surface.width as u16,
                height: surface.height as u16,
            };
            if actual != destination_extent {
                return Err(SoftwarePresentationError::DestinationExtentMismatch {
                    expected: destination_extent,
                    actual,
                });
            }
            if surface.palette != Some(PaletteHandle(0)) {
                return Err(SoftwarePresentationError::PaletteNotAttached);
            }
            let expected =
                usize::from(destination_extent.width) * usize::from(destination_extent.height);
            if surface.pixels.len() != expected {
                return Err(SoftwarePresentationError::InvalidPixelLength {
                    expected,
                    actual: surface.pixels.len(),
                });
            }
            SoftwarePresentedFrame {
                extent: destination_extent,
                pixels: surface.pixels.clone().into_boxed_slice(),
                palette: Box::new(palette),
            }
        };
        self.presented_frame = Some(frame);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use cmr_core::{
        display::{
            CooperativeMode, DisplaySurfaces, IndexedMode, IndexedVideoBackend, SurfaceTopology,
        },
        root_menu_c3d_scene::RootMenuC3dSceneBackend,
        root_menu_c3d_transform::{ROOT_MENU_C3D_VIEW, root_menu_c3d_viewport},
    };

    use super::*;

    const EXTENT: DisplayExtent = DisplayExtent {
        width: 4,
        height: 2,
    };

    fn palette(seed: u8) -> [PaletteEntry; 256] {
        std::array::from_fn(|index| PaletteEntry {
            red: seed.wrapping_add(index as u8),
            green: seed,
            blue: index as u8,
            flags: (index as u8) ^ seed,
        })
    }

    fn surfaces_only() -> (SoftwareIndexedBackend, DisplaySurfaces<SurfaceHandle>) {
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
        (backend, surfaces)
    }

    fn initialized_backend() -> (
        SoftwareIndexedBackend,
        DisplaySurfaces<SurfaceHandle>,
        PaletteHandle,
    ) {
        let (mut backend, surfaces) = surfaces_only();
        let entries = palette(3);
        let palette = backend.create_palette(&entries).unwrap();
        backend
            .attach_palette(surfaces.visible_front, palette)
            .unwrap();
        backend.attach_palette(surfaces.back, palette).unwrap();
        backend.upload_palette(palette, &entries).unwrap();
        (backend, surfaces, palette)
    }

    fn set_back_pixels(backend: &mut SoftwareIndexedBackend, back: SurfaceHandle, pixels: &[u8]) {
        backend.surfaces[back.0].as_mut().unwrap().pixels = pixels.to_vec();
    }

    #[test]
    fn capture_owns_pixels_and_palette_without_mutating_surfaces() {
        let (mut backend, surfaces, palette_handle) = initialized_backend();
        let first_pixels = [1, 2, 3, 4, 5, 6, 7, 8];
        set_back_pixels(&mut backend, surfaces.back, &first_pixels);
        let front_before = backend.visible_pixels().unwrap().to_vec();
        let back_before = backend.back_pixels().unwrap().to_vec();

        backend.present_root_frame(surfaces.back, EXTENT).unwrap();

        let captured = backend.presented_frame().unwrap().clone();
        assert_eq!(captured.extent(), EXTENT);
        assert_eq!(captured.pixels(), first_pixels);
        assert_eq!(captured.palette(), &palette(3));
        assert_eq!(backend.visible_pixels().unwrap(), front_before);
        assert_eq!(backend.back_pixels().unwrap(), back_before);

        set_back_pixels(&mut backend, surfaces.back, &[9; 8]);
        backend
            .upload_palette(palette_handle, &palette(17))
            .unwrap();
        assert_eq!(captured.pixels(), first_pixels);
        assert_eq!(captured.palette(), &palette(3));
        assert_eq!(backend.presented_frame(), Some(&captured));

        backend.present_root_frame(surfaces.back, EXTENT).unwrap();
        assert_eq!(backend.presented_frame().unwrap().pixels(), &[9; 8]);
        assert_eq!(backend.presented_frame().unwrap().palette(), &palette(17));
        assert_eq!(backend.visible_pixels().unwrap(), front_before);
        assert_eq!(backend.back_pixels().unwrap(), &[9; 8]);
    }

    #[test]
    fn every_capture_validation_error_preserves_prior_snapshot_and_surfaces() {
        let (mut backend, surfaces, _) = initialized_backend();
        let baseline_pixels = [11, 12, 13, 14, 15, 16, 17, 18];
        set_back_pixels(&mut backend, surfaces.back, &baseline_pixels);
        backend.present_root_frame(surfaces.back, EXTENT).unwrap();
        let baseline = backend.presented_frame().unwrap().clone();
        let front_before = backend.visible_pixels().unwrap().to_vec();
        let back_before = backend.back_pixels().unwrap().to_vec();

        assert!(matches!(
            backend.present_root_frame(surfaces.visible_front, EXTENT),
            Err(SoftwarePresentationError::DestinationIsNotBackBuffer { .. })
        ));
        assert!(matches!(
            backend.present_root_frame(
                surfaces.back,
                DisplayExtent {
                    width: EXTENT.width - 1,
                    height: EXTENT.height,
                }
            ),
            Err(SoftwarePresentationError::DestinationExtentMismatch { .. })
        ));

        backend.set_root_c3d_view(ROOT_MENU_C3D_VIEW).unwrap();
        backend
            .set_root_c3d_viewport(root_menu_c3d_viewport(EXTENT).unwrap())
            .unwrap();
        backend.begin_root_c3d_scene(surfaces.back).unwrap();
        assert_eq!(
            backend.present_root_frame(surfaces.back, EXTENT),
            Err(SoftwarePresentationError::InvalidSequence(
                "present_root_frame during active C3D scene"
            ))
        );
        backend.end_root_c3d_scene().unwrap();

        backend.surfaces[surfaces.back.0].as_mut().unwrap().palette = None;
        assert_eq!(
            backend.present_root_frame(surfaces.back, EXTENT),
            Err(SoftwarePresentationError::PaletteNotAttached)
        );
        backend.surfaces[surfaces.back.0].as_mut().unwrap().palette = Some(PaletteHandle(0));

        backend.surfaces[surfaces.back.0]
            .as_mut()
            .unwrap()
            .pixels
            .pop();
        assert_eq!(
            backend.present_root_frame(surfaces.back, EXTENT),
            Err(SoftwarePresentationError::InvalidPixelLength {
                expected: 8,
                actual: 7,
            })
        );
        backend.surfaces[surfaces.back.0]
            .as_mut()
            .unwrap()
            .pixels
            .push(18);

        assert_eq!(backend.presented_frame(), Some(&baseline));
        assert_eq!(backend.visible_pixels().unwrap(), front_before);
        assert_eq!(backend.back_pixels().unwrap(), back_before);
    }

    #[test]
    fn capture_requires_an_uploaded_active_palette() {
        let (mut backend, surfaces) = surfaces_only();
        assert_eq!(
            backend.present_root_frame(surfaces.back, EXTENT),
            Err(SoftwarePresentationError::MissingActivePalette)
        );
        assert!(backend.presented_frame().is_none());
    }
}
