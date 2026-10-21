//! Headless presentation capture for the indexed software framebuffer.
//!
//! Capturing an owned indexed frame makes the completed compatibility output
//! observable without assigning native window, DirectDraw Flip, synchronization,
//! or windowed-copy semantics to this backend.

use std::fmt;

use cmr_core::{display::DisplayExtent, root_menu_frame::RootFrameHooks};
use cmr_formats::riff_palette::PaletteEntry;
use sha2::{Digest, Sha256};

use crate::{PaletteHandle, SoftwareIndexedBackend, SoftwareVideoError, SurfaceHandle};

/// Version tag prepended to the canonical indexed-frame digest stream.
pub const SOFTWARE_PRESENTED_FRAME_DIGEST_FORMAT: &[u8; 8] = b"CMRFRM01";

/// Stable checksums for one complete indexed software frame.
///
/// The canonical stream consists of [`SOFTWARE_PRESENTED_FRAME_DIGEST_FORMAT`],
/// little-endian `u16` width and height, little-endian `u32` pixel count, all
/// 256 palette entries as red/green/blue/flags bytes, and the row-major indexed
/// pixels. These checksums identify deterministic output; they do not
/// authenticate the frame or its asset provenance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SoftwarePresentedFrameDigest {
    pub extent: DisplayExtent,
    pub indexed_pixel_count: u32,
    pub indexed_pixels_sha256: String,
    pub palette_rgb_flags_sha256: String,
    pub canonical_frame_sha256: String,
}

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

    /// Computes stable component and canonical checksums for this frame.
    #[must_use]
    pub fn digest_summary(&self) -> SoftwarePresentedFrameDigest {
        let indexed_pixel_count = u32::from(self.extent.width) * u32::from(self.extent.height);
        debug_assert_eq!(self.pixels.len(), indexed_pixel_count as usize);

        let mut palette_digest = Sha256::new();
        for entry in self.palette.iter() {
            palette_digest.update([entry.red, entry.green, entry.blue, entry.flags]);
        }

        let mut canonical_digest = Sha256::new();
        canonical_digest.update(SOFTWARE_PRESENTED_FRAME_DIGEST_FORMAT);
        canonical_digest.update(self.extent.width.to_le_bytes());
        canonical_digest.update(self.extent.height.to_le_bytes());
        canonical_digest.update(indexed_pixel_count.to_le_bytes());
        for entry in self.palette.iter() {
            canonical_digest.update([entry.red, entry.green, entry.blue, entry.flags]);
        }
        canonical_digest.update(&self.pixels);

        SoftwarePresentedFrameDigest {
            extent: self.extent,
            indexed_pixel_count,
            indexed_pixels_sha256: sha256_hex(Sha256::digest(&self.pixels)),
            palette_rgb_flags_sha256: sha256_hex(palette_digest.finalize()),
            canonical_frame_sha256: sha256_hex(canonical_digest.finalize()),
        }
    }
}

fn sha256_hex(digest: impl AsRef<[u8]>) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";

    let digest = digest.as_ref();
    let mut encoded = String::with_capacity(digest.len() * 2);
    for &byte in digest {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
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

    #[test]
    fn digest_summary_matches_the_canonical_version_one_vector() {
        let palette = std::array::from_fn(|index| {
            let index = index as u8;
            PaletteEntry {
                red: index,
                green: 255 - index,
                blue: index ^ 0x5a,
                flags: index.rotate_left(1),
            }
        });
        let frame = SoftwarePresentedFrame {
            extent: DisplayExtent {
                width: 2,
                height: 2,
            },
            pixels: Box::from([0, 1, 2, 255]),
            palette: Box::new(palette),
        };

        assert_eq!(SOFTWARE_PRESENTED_FRAME_DIGEST_FORMAT, b"CMRFRM01");
        assert_eq!(
            frame.digest_summary(),
            SoftwarePresentedFrameDigest {
                extent: frame.extent(),
                indexed_pixel_count: 4,
                indexed_pixels_sha256:
                    "3d1f57c984978ef98a18378c8166c1cb8ede02c03eeb6aee7e2f121dfeee3e56".to_owned(),
                palette_rgb_flags_sha256:
                    "b11f77b1394c63690850b4f92ae5dd451be670d81969e31508b834c434cdf88d".to_owned(),
                canonical_frame_sha256:
                    "3316bd72a0814a72a34076a21cfaf97b9ae5f47dc95e32e820f1cf84f087a154".to_owned(),
            }
        );
    }

    #[test]
    fn digest_summary_matches_the_all_zero_boundary_vector() {
        let frame = SoftwarePresentedFrame {
            extent: DisplayExtent {
                width: 1,
                height: 1,
            },
            pixels: Box::from([0]),
            palette: Box::new(
                [PaletteEntry {
                    red: 0,
                    green: 0,
                    blue: 0,
                    flags: 0,
                }; 256],
            ),
        };

        assert_eq!(
            frame.digest_summary().canonical_frame_sha256,
            "fce9803d5b73c116ad1d6c650f2db4bd19aaea7285fe3f9355abaa9b5b07224c"
        );
    }

    #[test]
    fn canonical_digest_is_sensitive_to_extent_palette_and_pixels() {
        let palette = [PaletteEntry {
            red: 0,
            green: 0,
            blue: 0,
            flags: 0,
        }; 256];
        let baseline = SoftwarePresentedFrame {
            extent: DisplayExtent {
                width: 2,
                height: 2,
            },
            pixels: Box::from([0, 1, 2, 3]),
            palette: Box::new(palette),
        };
        let mut changed_extent = baseline.clone();
        changed_extent.extent = DisplayExtent {
            width: 1,
            height: 4,
        };
        let mut changed_palette = baseline.clone();
        changed_palette.palette[0].flags = 1;
        let mut changed_pixels = baseline.clone();
        changed_pixels.pixels[0] = 1;

        let baseline_digest = baseline.digest_summary();
        let extent_digest = changed_extent.digest_summary();
        let palette_digest = changed_palette.digest_summary();
        let pixels_digest = changed_pixels.digest_summary();

        assert_ne!(
            extent_digest.canonical_frame_sha256,
            baseline_digest.canonical_frame_sha256
        );
        assert_ne!(
            palette_digest.canonical_frame_sha256,
            baseline_digest.canonical_frame_sha256
        );
        assert_ne!(
            pixels_digest.canonical_frame_sha256,
            baseline_digest.canonical_frame_sha256
        );
        assert_eq!(
            extent_digest.indexed_pixels_sha256,
            baseline_digest.indexed_pixels_sha256
        );
        assert_eq!(
            palette_digest.indexed_pixels_sha256,
            baseline_digest.indexed_pixels_sha256
        );
        assert_eq!(
            pixels_digest.palette_rgb_flags_sha256,
            baseline_digest.palette_rgb_flags_sha256
        );
    }
}
