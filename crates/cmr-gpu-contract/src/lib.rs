//! API-neutral contracts for GPU conformance and performance evidence.
//!
//! This crate deliberately contains no wgpu, SDL, window, or driver types.
//! Candidate backends receive the same validated indexed frame and normalize
//! readback into the same tightly packed opaque RGBA8 representation. Native
//! API initialization can remain asynchronous in each adapter; the per-frame
//! conformance operation is synchronous so tests can compare complete pixels.

use std::fmt;

use cmr_core::display::DisplayExtent;
use cmr_formats::riff_palette::PaletteEntry;

/// Palette entry count of the recovered 8-bit indexed display.
pub const INDEXED_PALETTE_LEN: usize = 256;
/// Bytes per pixel in canonical readback output.
pub const RGBA8_BYTES_PER_PIXEL: usize = 4;

/// A validated borrowed indexed frame shared by every candidate backend.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IndexedFrameView<'a> {
    extent: DisplayExtent,
    indices: &'a [u8],
    palette: &'a [PaletteEntry],
}

impl<'a> IndexedFrameView<'a> {
    /// Validates nonzero extent, exact `width * height` index storage, and the
    /// complete 256-entry palette required by an `u8` index.
    pub fn new(
        extent: DisplayExtent,
        indices: &'a [u8],
        palette: &'a [PaletteEntry],
    ) -> Result<Self, FrameContractError> {
        let pixel_count = checked_pixel_count(extent)?;
        if indices.len() != pixel_count {
            return Err(FrameContractError::InvalidIndexLength {
                expected: pixel_count,
                actual: indices.len(),
            });
        }
        if palette.len() != INDEXED_PALETTE_LEN {
            return Err(FrameContractError::InvalidPaletteLength {
                expected: INDEXED_PALETTE_LEN,
                actual: palette.len(),
            });
        }
        Ok(Self {
            extent,
            indices,
            palette,
        })
    }

    #[must_use]
    pub const fn extent(self) -> DisplayExtent {
        self.extent
    }

    #[must_use]
    pub const fn indices(self) -> &'a [u8] {
        self.indices
    }

    #[must_use]
    pub const fn palette(self) -> &'a [PaletteEntry] {
        self.palette
    }

    #[must_use]
    pub fn pixel_count(self) -> usize {
        self.indices.len()
    }

    /// Expands the frame into the canonical comparison representation.
    ///
    /// Palette flags are control/reserved data, not alpha. Every output pixel
    /// is therefore opaque regardless of its palette entry's flag byte.
    pub fn canonical_rgba8(self) -> Result<CanonicalRgba8Frame, FrameContractError> {
        let output_len = self
            .indices
            .len()
            .checked_mul(RGBA8_BYTES_PER_PIXEL)
            .ok_or(FrameContractError::SizeOverflow(self.extent))?;
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(output_len)
            .map_err(|_| FrameContractError::AllocationFailed { bytes: output_len })?;
        for &index in self.indices {
            let entry = self.palette[usize::from(index)];
            pixels.extend_from_slice(&[entry.red, entry.green, entry.blue, u8::MAX]);
        }
        Ok(CanonicalRgba8Frame {
            extent: self.extent,
            pixels: pixels.into_boxed_slice(),
        })
    }
}

/// A tightly packed, row-major, opaque RGBA8 conformance result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalRgba8Frame {
    extent: DisplayExtent,
    pixels: Box<[u8]>,
}

impl CanonicalRgba8Frame {
    #[must_use]
    pub const fn extent(&self) -> DisplayExtent {
        self.extent
    }

    #[must_use]
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    #[must_use]
    pub const fn row_pitch(&self) -> usize {
        self.extent.width as usize * RGBA8_BYTES_PER_PIXEL
    }
}

/// Byte ordering of a mapped four-channel GPU readback surface.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadbackFormat {
    Rgba8,
    Bgra8,
}

/// Removes driver-owned row padding and channel-order differences from mapped
/// readback bytes, yielding the canonical tightly packed RGBA8 comparison form.
pub fn normalize_readback(
    extent: DisplayExtent,
    mapped: &[u8],
    bytes_per_row: usize,
    format: ReadbackFormat,
) -> Result<CanonicalRgba8Frame, FrameContractError> {
    let pixel_count = checked_pixel_count(extent)?;
    let tight_pitch = usize::from(extent.width)
        .checked_mul(RGBA8_BYTES_PER_PIXEL)
        .ok_or(FrameContractError::SizeOverflow(extent))?;
    if bytes_per_row < tight_pitch {
        return Err(FrameContractError::ReadbackPitchTooSmall {
            minimum: tight_pitch,
            actual: bytes_per_row,
        });
    }
    let mapped_len = bytes_per_row
        .checked_mul(usize::from(extent.height))
        .ok_or(FrameContractError::SizeOverflow(extent))?;
    if mapped.len() != mapped_len {
        return Err(FrameContractError::InvalidReadbackLength {
            expected: mapped_len,
            actual: mapped.len(),
        });
    }
    let output_len = pixel_count
        .checked_mul(RGBA8_BYTES_PER_PIXEL)
        .ok_or(FrameContractError::SizeOverflow(extent))?;
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(output_len)
        .map_err(|_| FrameContractError::AllocationFailed { bytes: output_len })?;
    for row in mapped.chunks_exact(bytes_per_row) {
        let (row_pixels, remainder) = row[..tight_pitch].as_chunks::<RGBA8_BYTES_PER_PIXEL>();
        debug_assert!(remainder.is_empty());
        for pixel in row_pixels {
            match format {
                ReadbackFormat::Rgba8 => pixels.extend_from_slice(pixel),
                ReadbackFormat::Bgra8 => {
                    pixels.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]])
                }
            }
        }
    }
    Ok(CanonicalRgba8Frame {
        extent,
        pixels: pixels.into_boxed_slice(),
    })
}

fn checked_pixel_count(extent: DisplayExtent) -> Result<usize, FrameContractError> {
    if extent.width == 0 || extent.height == 0 {
        return Err(FrameContractError::InvalidExtent(extent));
    }
    usize::from(extent.width)
        .checked_mul(usize::from(extent.height))
        .ok_or(FrameContractError::SizeOverflow(extent))
}

/// Validation/allocation failure at the shared GPU conformance boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FrameContractError {
    InvalidExtent(DisplayExtent),
    SizeOverflow(DisplayExtent),
    InvalidIndexLength { expected: usize, actual: usize },
    InvalidPaletteLength { expected: usize, actual: usize },
    ReadbackPitchTooSmall { minimum: usize, actual: usize },
    InvalidReadbackLength { expected: usize, actual: usize },
    AllocationFailed { bytes: usize },
}

impl fmt::Display for FrameContractError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "GPU frame contract failed: {self:?}")
    }
}

impl std::error::Error for FrameContractError {}

/// Adapter implementation being measured, separate from its selected graphics
/// API (for example, both candidates can select Vulkan).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackendKind {
    Wgpu,
    SdlGpu,
}

/// Native graphics API selected by a backend instance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GraphicsApi {
    Vulkan,
    Direct3d12,
    Metal,
    OpenGl,
    Software,
    Unknown,
}

/// Hardware class reported by the native API for adapter comparison.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeviceKind {
    Integrated,
    Discrete,
    Virtual,
    Cpu,
    Unknown,
}

/// Stable adapter identity included with conformance and benchmark evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackendInfo {
    pub backend: BackendKind,
    pub graphics_api: GraphicsApi,
    /// PCI-style vendor ID when the native API exposes one.
    pub vendor_id: Option<u32>,
    /// PCI-style device ID when the native API exposes one.
    pub device_id: Option<u32>,
    pub device_kind: DeviceKind,
    pub adapter_name: String,
    pub driver_name: String,
    pub driver_info: String,
}

impl BackendInfo {
    /// Whether both reports identify the same vendor/device model pair.
    ///
    /// A missing native ID never counts as a match; benchmark orchestration
    /// must report the model comparison as unverified instead of guessing from
    /// names. Equal IDs do not prove that two backends selected the same
    /// physical adapter when a system has multiple identical devices.
    #[must_use]
    pub fn same_vendor_device_pair(&self, other: &Self) -> bool {
        matches!(
            (
                self.vendor_id,
                self.device_id,
                other.vendor_id,
                other.device_id
            ),
            (Some(a_vendor), Some(a_device), Some(b_vendor), Some(b_device))
                if a_vendor == b_vendor && a_device == b_device
        )
    }
}

/// Capabilities that change which comparisons/timings are authoritative.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BackendCapabilities {
    pub sampled_r8_uint: bool,
    pub timestamp_queries: bool,
    pub headless_readback: bool,
    pub device_recovery: bool,
}

/// Per-frame timing returned beside completed readback.
///
/// The two required wall-clock intervals permit a fair comparison even when a
/// backend cannot expose GPU timestamp queries. `gpu_timestamp_ns` is only
/// supplementary and must not replace submit-to-completion measurement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameTiming {
    /// CPU wall time from per-frame allocation/upload preparation through
    /// encoding and successful submission.
    pub cpu_encode_submit_ns: u64,
    /// Wall time from successful submission until its completion fence/event.
    pub submit_to_completion_ns: u64,
    /// Native GPU timestamp delta when supported and calibrated.
    pub gpu_timestamp_ns: Option<u64>,
}

/// One complete synchronous conformance observation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConformanceFrame {
    pub rgba8: CanonicalRgba8Frame,
    pub timing: FrameTiming,
}

/// Common operation implemented by both native GPU candidates.
///
/// Construction is intentionally outside this trait so an adapter can perform
/// asynchronous instance/device setup without infecting this shared contract.
pub trait IndexedPaletteGpuBackend {
    type Error;

    fn info(&self) -> &BackendInfo;
    fn capabilities(&self) -> BackendCapabilities;
    fn render_readback(
        &mut self,
        frame: IndexedFrameView<'_>,
    ) -> Result<ConformanceFrame, Self::Error>;
}

/// Deterministic nearest-rank timing summary over non-empty nanosecond samples.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimingSummary {
    pub samples: usize,
    pub minimum_ns: u64,
    pub median_ns: u64,
    pub p95_ns: u64,
    pub p99_ns: u64,
    pub maximum_ns: u64,
}

impl TimingSummary {
    pub fn from_samples(samples: &[u64]) -> Result<Self, TimingSummaryError> {
        if samples.is_empty() {
            return Err(TimingSummaryError::Empty);
        }
        let mut ordered = samples.to_vec();
        ordered.sort_unstable();
        Ok(Self {
            samples: ordered.len(),
            minimum_ns: ordered[0],
            median_ns: nearest_rank(&ordered, 50),
            p95_ns: nearest_rank(&ordered, 95),
            p99_ns: nearest_rank(&ordered, 99),
            maximum_ns: ordered[ordered.len() - 1],
        })
    }
}

fn nearest_rank(ordered: &[u64], percentile: usize) -> u64 {
    debug_assert!(!ordered.is_empty());
    debug_assert!((1..=100).contains(&percentile));
    let rank = ordered.len().saturating_mul(percentile).div_ceil(100);
    ordered[rank.saturating_sub(1)]
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimingSummaryError {
    Empty,
}

impl fmt::Display for TimingSummaryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "cannot summarize an empty timing sample set")
    }
}

impl std::error::Error for TimingSummaryError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette() -> [PaletteEntry; INDEXED_PALETTE_LEN] {
        std::array::from_fn(|index| PaletteEntry {
            red: index as u8,
            green: (255 - index) as u8,
            blue: (index as u8).wrapping_mul(3),
            flags: (index as u8).wrapping_mul(17),
        })
    }

    #[test]
    fn canonical_oracle_maps_zero_and_255_and_ignores_flags_as_alpha() {
        let palette = palette();
        let view = IndexedFrameView::new(
            DisplayExtent {
                width: 2,
                height: 1,
            },
            &[0, 255],
            &palette,
        )
        .unwrap();

        let rgba = view.canonical_rgba8().unwrap();
        assert_eq!(
            rgba.pixels(),
            &[
                palette[0].red,
                palette[0].green,
                palette[0].blue,
                255,
                palette[255].red,
                palette[255].green,
                palette[255].blue,
                255,
            ]
        );
    }

    #[test]
    fn palette_only_mutation_changes_only_pixels_using_that_entry() {
        let indices = [1, 2, 1];
        let extent = DisplayExtent {
            width: 3,
            height: 1,
        };
        let before_palette = palette();
        let mut after_palette = before_palette;
        after_palette[1] = PaletteEntry {
            red: 9,
            green: 8,
            blue: 7,
            flags: 0,
        };
        let before = IndexedFrameView::new(extent, &indices, &before_palette)
            .unwrap()
            .canonical_rgba8()
            .unwrap();
        let after = IndexedFrameView::new(extent, &indices, &after_palette)
            .unwrap()
            .canonical_rgba8()
            .unwrap();

        assert_ne!(&before.pixels()[0..4], &after.pixels()[0..4]);
        assert_eq!(&before.pixels()[4..8], &after.pixels()[4..8]);
        assert_ne!(&before.pixels()[8..12], &after.pixels()[8..12]);
        assert_eq!(&after.pixels()[0..4], &[9, 8, 7, 255]);
    }

    #[test]
    fn odd_width_padded_bgra_readback_normalizes_without_padding() {
        let extent = DisplayExtent {
            width: 3,
            height: 2,
        };
        let mapped = [
            3, 2, 1, 255, 6, 5, 4, 254, 9, 8, 7, 253, 0xaa, 0xaa, 0xaa, 0xaa, 12, 11, 10, 252, 15,
            14, 13, 251, 18, 17, 16, 250, 0xbb, 0xbb, 0xbb, 0xbb,
        ];

        let normalized = normalize_readback(extent, &mapped, 16, ReadbackFormat::Bgra8).unwrap();

        assert_eq!(normalized.row_pitch(), 12);
        assert_eq!(
            normalized.pixels(),
            &[
                1, 2, 3, 255, 4, 5, 6, 254, 7, 8, 9, 253, 10, 11, 12, 252, 13, 14, 15, 251, 16, 17,
                18, 250,
            ]
        );
    }

    #[test]
    fn rgba_readback_preserves_channels_and_alpha() {
        let extent = DisplayExtent {
            width: 1,
            height: 1,
        };
        let normalized =
            normalize_readback(extent, &[1, 2, 3, 4], 4, ReadbackFormat::Rgba8).unwrap();
        assert_eq!(normalized.pixels(), &[1, 2, 3, 4]);
    }

    #[test]
    fn frame_view_rejects_zero_extent_and_wrong_index_or_palette_lengths() {
        let palette = palette();
        assert_eq!(
            IndexedFrameView::new(
                DisplayExtent {
                    width: 0,
                    height: 1,
                },
                &[],
                &palette,
            ),
            Err(FrameContractError::InvalidExtent(DisplayExtent {
                width: 0,
                height: 1,
            }))
        );
        assert!(matches!(
            IndexedFrameView::new(
                DisplayExtent {
                    width: 2,
                    height: 2,
                },
                &[0; 3],
                &palette,
            ),
            Err(FrameContractError::InvalidIndexLength {
                expected: 4,
                actual: 3
            })
        ));
        assert!(matches!(
            IndexedFrameView::new(
                DisplayExtent {
                    width: 1,
                    height: 1,
                },
                &[0],
                &palette[..255],
            ),
            Err(FrameContractError::InvalidPaletteLength {
                expected: 256,
                actual: 255
            })
        ));
    }

    #[test]
    fn readback_rejects_narrow_pitch_and_nonexact_storage() {
        let extent = DisplayExtent {
            width: 3,
            height: 2,
        };
        assert_eq!(
            normalize_readback(extent, &[0; 24], 11, ReadbackFormat::Rgba8),
            Err(FrameContractError::ReadbackPitchTooSmall {
                minimum: 12,
                actual: 11,
            })
        );
        assert_eq!(
            normalize_readback(extent, &[0; 31], 16, ReadbackFormat::Rgba8),
            Err(FrameContractError::InvalidReadbackLength {
                expected: 32,
                actual: 31,
            })
        );
        assert_eq!(
            normalize_readback(extent, &[0; 33], 16, ReadbackFormat::Rgba8),
            Err(FrameContractError::InvalidReadbackLength {
                expected: 32,
                actual: 33,
            })
        );
    }

    #[test]
    fn timing_summary_uses_deterministic_nearest_rank_percentiles() {
        let samples: Vec<u64> = (1..=100).rev().collect();
        let summary = TimingSummary::from_samples(&samples).unwrap();
        assert_eq!(
            summary,
            TimingSummary {
                samples: 100,
                minimum_ns: 1,
                median_ns: 50,
                p95_ns: 95,
                p99_ns: 99,
                maximum_ns: 100,
            }
        );
        assert_eq!(
            TimingSummary::from_samples(&[]),
            Err(TimingSummaryError::Empty)
        );
    }

    #[test]
    fn single_timing_sample_populates_every_rank() {
        let summary = TimingSummary::from_samples(&[42]).unwrap();
        assert_eq!(summary.minimum_ns, 42);
        assert_eq!(summary.median_ns, 42);
        assert_eq!(summary.p95_ns, 42);
        assert_eq!(summary.p99_ns, 42);
        assert_eq!(summary.maximum_ns, 42);
    }

    fn backend_info(vendor_id: Option<u32>, device_id: Option<u32>) -> BackendInfo {
        BackendInfo {
            backend: BackendKind::Wgpu,
            graphics_api: GraphicsApi::Vulkan,
            vendor_id,
            device_id,
            device_kind: DeviceKind::Discrete,
            adapter_name: "synthetic adapter".into(),
            driver_name: "synthetic driver".into(),
            driver_info: "test-only".into(),
        }
    }

    #[test]
    fn vendor_device_pair_match_requires_equal_present_ids() {
        let device = backend_info(Some(0x1002), Some(0x73bf));
        let same_via_other_backend = BackendInfo {
            backend: BackendKind::SdlGpu,
            ..device.clone()
        };
        assert!(device.same_vendor_device_pair(&same_via_other_backend));
        assert!(!device.same_vendor_device_pair(&backend_info(Some(0x10de), Some(0x73bf))));
        assert!(!device.same_vendor_device_pair(&backend_info(Some(0x1002), Some(0x1234))));
        assert!(!device.same_vendor_device_pair(&backend_info(None, Some(0x73bf))));
        assert!(!device.same_vendor_device_pair(&backend_info(Some(0x1002), None)));
    }

    #[test]
    fn timing_keeps_required_wall_intervals_separate_from_optional_gpu_timestamp() {
        let timing = FrameTiming {
            cpu_encode_submit_ns: 11,
            submit_to_completion_ns: 22,
            gpu_timestamp_ns: Some(17),
        };
        assert_eq!(timing.cpu_encode_submit_ns, 11);
        assert_eq!(timing.submit_to_completion_ns, 22);
        assert_eq!(timing.gpu_timestamp_ns, Some(17));
    }
}
