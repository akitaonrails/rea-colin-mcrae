//! Image-comparison fixtures and tolerances for differential verification
//! (reconstruction item E11).
//!
//! The reconstruction order ends each subsystem with "verify behavior with
//! differential fixtures or controlled observations". The software raster
//! (`cmr-video`) emits canonical 8-bit-per-channel RGB pixels and, after the
//! framebuffer conversion, packed 16-bit words. A faithful differential check
//! against an oracle cannot demand bit-exactness through that conversion:
//! packing to a 5-6-5 (or 5-5-5) target quantizes each channel, and the
//! documented bilinear/dither policies add bounded per-pixel noise. This module
//! supplies the comparison primitive - a per-channel delta metric with an
//! explicit [`Tolerance`] - and a small set of source-owned synthetic fixtures
//! to exercise it. All fixtures are generated here; no game payload bytes are
//! copied.
//!
//! The comparator is deliberately independent of `cmr-video`: it takes plain
//! RGB pixel buffers so any backend (the compatibility software raster or a
//! later modern GPU backend) is checked against the same metric.

/// An owned 8-bit-per-channel RGB image: `width * height` pixels in row-major
/// order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgb888Image {
    width: usize,
    height: usize,
    pixels: Vec<[u8; 3]>,
}

/// Why two images could not be compared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompareError {
    /// The pixel buffer length does not match `width * height`.
    PixelCountMismatch {
        /// Pixels the dimensions require.
        expected: usize,
        /// Pixels actually supplied.
        actual: usize,
    },
    /// The two images have different dimensions.
    DimensionMismatch {
        /// `(width, height)` of the reference image.
        reference: (usize, usize),
        /// `(width, height)` of the candidate image.
        candidate: (usize, usize),
    },
}

impl Rgb888Image {
    /// Build an image, rejecting a pixel buffer whose length is not exactly
    /// `width * height`.
    pub fn new(width: usize, height: usize, pixels: Vec<[u8; 3]>) -> Result<Self, CompareError> {
        let expected = width * height;
        if pixels.len() != expected {
            return Err(CompareError::PixelCountMismatch {
                expected,
                actual: pixels.len(),
            });
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }

    /// Image width in pixels.
    pub fn width(&self) -> usize {
        self.width
    }

    /// Image height in pixels.
    pub fn height(&self) -> usize {
        self.height
    }

    /// The row-major pixel buffer.
    pub fn pixels(&self) -> &[[u8; 3]] {
        &self.pixels
    }
}

/// A per-channel comparison tolerance.
///
/// A pixel is an *outlier* when any of its three channels differs from the
/// reference by more than `per_channel_delta`. A comparison passes when the
/// fraction of outlier pixels is at most `max_outlier_fraction` (in `0.0..=1.0`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tolerance {
    /// The largest per-channel absolute difference a pixel may have without
    /// being counted as an outlier.
    pub per_channel_delta: u8,
    /// The fraction of outlier pixels the comparison still accepts.
    pub max_outlier_fraction: f64,
}

impl Tolerance {
    /// Require bit-exact pixels (every channel identical).
    pub const EXACT: Tolerance = Tolerance {
        per_channel_delta: 0,
        max_outlier_fraction: 0.0,
    };

    /// The tolerance for a canonical RGB oracle compared against a value that
    /// has round-tripped through a 5-6-5 (or 5-5-5) packed framebuffer. A
    /// 5-bit channel expanded by bit-replication differs from its 8-bit source
    /// by at most 7, so a per-channel delta of `8` covers quantization with no
    /// outliers (packing is deterministic).
    pub const RGB565_ROUNDTRIP: Tolerance = Tolerance {
        per_channel_delta: 8,
        max_outlier_fraction: 0.0,
    };

    /// A tolerance that keeps the quantization per-channel bound but allows a
    /// bounded fraction of outlier pixels, for a dithered candidate.
    pub fn dithered(max_outlier_fraction: f64) -> Tolerance {
        Tolerance {
            per_channel_delta: 8,
            max_outlier_fraction,
        }
    }
}

/// The outcome of comparing two images under a [`Tolerance`].
#[derive(Debug, Clone, PartialEq)]
pub struct ComparisonReport {
    /// Total pixels compared.
    pub total_pixels: usize,
    /// Pixels whose per-channel delta exceeded the tolerance.
    pub outlier_pixels: usize,
    /// The largest single-channel absolute difference seen anywhere.
    pub max_channel_delta: u8,
    /// The `(x, y)` of the first outlier pixel in row-major order, if any.
    pub first_outlier: Option<(usize, usize)>,
    /// Whether the comparison passed the tolerance.
    pub within_tolerance: bool,
}

impl ComparisonReport {
    /// The fraction of pixels that were outliers (`0.0` for an empty image).
    pub fn outlier_fraction(&self) -> f64 {
        if self.total_pixels == 0 {
            0.0
        } else {
            self.outlier_pixels as f64 / self.total_pixels as f64
        }
    }
}

/// Compare a candidate image against a reference under a tolerance.
///
/// Fails fast on a dimension mismatch; otherwise walks every pixel, accumulates
/// the per-channel deltas, and reports the max delta, the outlier count, the
/// first outlier coordinate, and the pass/fail verdict.
pub fn compare_rgb888(
    reference: &Rgb888Image,
    candidate: &Rgb888Image,
    tolerance: Tolerance,
) -> Result<ComparisonReport, CompareError> {
    if (reference.width, reference.height) != (candidate.width, candidate.height) {
        return Err(CompareError::DimensionMismatch {
            reference: (reference.width, reference.height),
            candidate: (candidate.width, candidate.height),
        });
    }

    let mut outlier_pixels = 0usize;
    let mut max_channel_delta = 0u8;
    let mut first_outlier = None;

    for (index, (r, c)) in reference
        .pixels
        .iter()
        .zip(candidate.pixels.iter())
        .enumerate()
    {
        let mut pixel_max = 0u8;
        for channel in 0..3 {
            let delta = r[channel].abs_diff(c[channel]);
            pixel_max = pixel_max.max(delta);
        }
        max_channel_delta = max_channel_delta.max(pixel_max);
        if pixel_max > tolerance.per_channel_delta {
            outlier_pixels += 1;
            if first_outlier.is_none() {
                first_outlier = Some((index % reference.width, index / reference.width));
            }
        }
    }

    let total_pixels = reference.pixels.len();
    let outlier_fraction = if total_pixels == 0 {
        0.0
    } else {
        outlier_pixels as f64 / total_pixels as f64
    };

    Ok(ComparisonReport {
        total_pixels,
        outlier_pixels,
        max_channel_delta,
        first_outlier,
        within_tolerance: outlier_fraction <= tolerance.max_outlier_fraction,
    })
}

/// Expand an `n`-bit channel value (`n` in `1..=8`) to 8 bits by
/// bit-replication, the standard widening used when unpacking a 5-6-5 word to
/// canonical RGB (e.g. a 5-bit `0x1f` becomes `0xff`, not `0xf8`).
pub fn expand_channel(value: u8, bits: u8) -> u8 {
    debug_assert!((1..=8).contains(&bits), "bit width out of range");
    let value = value & ((1u16 << bits) - 1) as u8;
    let mut out = 0u16;
    let mut filled = 0u8;
    // Replicate the high bits downward until all 8 bits are filled.
    while filled < 8 {
        let shift = 8i16 - filled as i16 - bits as i16;
        out |= if shift >= 0 {
            (value as u16) << shift
        } else {
            (value as u16) >> (-shift)
        };
        filled += bits;
    }
    (out & 0xff) as u8
}

/// Decode a packed little-endian 5-6-5 RGB word to canonical 8-bit RGB by
/// bit-replication, so a packed framebuffer can be compared against an 8-bit
/// oracle with [`Tolerance::RGB565_ROUNDTRIP`].
pub fn rgb565_to_canonical(word: u16) -> [u8; 3] {
    let r = ((word >> 11) & 0x1f) as u8;
    let g = ((word >> 5) & 0x3f) as u8;
    let b = (word & 0x1f) as u8;
    [
        expand_channel(r, 5),
        expand_channel(g, 6),
        expand_channel(b, 5),
    ]
}

/// Source-owned synthetic images for exercising the comparator. None of these
/// reproduce game content; they are deterministic test patterns.
pub mod fixtures {
    use super::Rgb888Image;

    /// A `width * height` horizontal red ramp (`red = x * 255 / (width-1)`),
    /// green/blue zero. A canonical oracle pattern with a known gradient.
    pub fn horizontal_red_ramp(width: usize, height: usize) -> Rgb888Image {
        let mut pixels = Vec::with_capacity(width * height);
        for _y in 0..height {
            for x in 0..width {
                let red = if width <= 1 {
                    0
                } else {
                    (x * 255 / (width - 1)) as u8
                };
                pixels.push([red, 0, 0]);
            }
        }
        Rgb888Image::new(width, height, pixels).expect("ramp dimensions are consistent")
    }

    /// A `width * height` two-color checkerboard of `a`/`b` cells.
    pub fn checkerboard(width: usize, height: usize, a: [u8; 3], b: [u8; 3]) -> Rgb888Image {
        let mut pixels = Vec::with_capacity(width * height);
        for y in 0..height {
            for x in 0..width {
                pixels.push(if (x + y) % 2 == 0 { a } else { b });
            }
        }
        Rgb888Image::new(width, height, pixels).expect("checker dimensions are consistent")
    }

    /// Return a copy of `image` with every channel of every pixel pushed by
    /// `delta` (saturating), to synthesize a candidate that differs uniformly.
    pub fn shifted(image: &Rgb888Image, delta: i16) -> Rgb888Image {
        let pixels = image
            .pixels()
            .iter()
            .map(|p| {
                let mut out = *p;
                for channel in &mut out {
                    *channel = (*channel as i16 + delta).clamp(0, 255) as u8;
                }
                out
            })
            .collect();
        Rgb888Image::new(image.width(), image.height(), pixels).expect("shift preserves dimensions")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_inconsistent_pixel_count() {
        let err = Rgb888Image::new(2, 2, vec![[0, 0, 0]; 3]).unwrap_err();
        assert_eq!(
            err,
            CompareError::PixelCountMismatch {
                expected: 4,
                actual: 3
            }
        );
    }

    #[test]
    fn dimension_mismatch_fails_before_pixel_walk() {
        let a = fixtures::horizontal_red_ramp(4, 2);
        let b = fixtures::horizontal_red_ramp(2, 4);
        let err = compare_rgb888(&a, &b, Tolerance::EXACT).unwrap_err();
        assert_eq!(
            err,
            CompareError::DimensionMismatch {
                reference: (4, 2),
                candidate: (2, 4),
            }
        );
    }

    #[test]
    fn identical_images_pass_exact() {
        let a = fixtures::horizontal_red_ramp(8, 4);
        let report = compare_rgb888(&a, &a, Tolerance::EXACT).unwrap();
        assert_eq!(report.total_pixels, 32);
        assert_eq!(report.outlier_pixels, 0);
        assert_eq!(report.max_channel_delta, 0);
        assert_eq!(report.first_outlier, None);
        assert!(report.within_tolerance);
    }

    #[test]
    fn exact_rejects_any_difference_and_reports_first_outlier() {
        let a = fixtures::checkerboard(3, 2, [10, 20, 30], [40, 50, 60]);
        // Nudge a single interior pixel (x=1, y=1 -> index 4).
        let mut pixels = a.pixels().to_vec();
        pixels[4][1] += 5;
        let b = Rgb888Image::new(3, 2, pixels).unwrap();
        let report = compare_rgb888(&a, &b, Tolerance::EXACT).unwrap();
        assert_eq!(report.outlier_pixels, 1);
        assert_eq!(report.max_channel_delta, 5);
        assert_eq!(report.first_outlier, Some((1, 1)));
        assert!(!report.within_tolerance);
        assert!((report.outlier_fraction() - 1.0 / 6.0).abs() < 1e-12);
    }

    #[test]
    fn quantization_within_roundtrip_tolerance_passes() {
        let reference = fixtures::horizontal_red_ramp(16, 4);
        // A uniform +7 shift is at the 5-bit replication bound: not an outlier.
        let candidate = fixtures::shifted(&reference, 7);
        let report = compare_rgb888(&reference, &candidate, Tolerance::RGB565_ROUNDTRIP).unwrap();
        assert_eq!(report.max_channel_delta, 7);
        assert_eq!(report.outlier_pixels, 0);
        assert!(report.within_tolerance);
        // One more step pushes every pixel over the bound.
        let over = fixtures::shifted(&reference, 9);
        let report = compare_rgb888(&reference, &over, Tolerance::RGB565_ROUNDTRIP).unwrap();
        assert!(report.outlier_pixels > 0);
        assert!(!report.within_tolerance);
    }

    #[test]
    fn dithered_tolerance_accepts_bounded_outlier_fraction() {
        let reference = fixtures::checkerboard(4, 4, [100, 100, 100], [100, 100, 100]);
        let mut pixels = reference.pixels().to_vec();
        // Make 2 of 16 pixels large outliers (fraction 0.125).
        pixels[0] = [200, 200, 200];
        pixels[1] = [200, 200, 200];
        let candidate = Rgb888Image::new(4, 4, pixels).unwrap();
        // 0.125 > 0.1 fails, but 0.125 <= 0.2 passes.
        assert!(
            !compare_rgb888(&reference, &candidate, Tolerance::dithered(0.1))
                .unwrap()
                .within_tolerance
        );
        assert!(
            compare_rgb888(&reference, &candidate, Tolerance::dithered(0.2))
                .unwrap()
                .within_tolerance
        );
    }

    #[test]
    fn channel_expansion_replicates_bits() {
        // Full and empty values map to full and empty 8-bit.
        assert_eq!(expand_channel(0x1f, 5), 0xff);
        assert_eq!(expand_channel(0, 5), 0);
        assert_eq!(expand_channel(0x3f, 6), 0xff);
        // 5-bit 0x10 (16/31) -> 0x84 by replication (not 0x80 truncation).
        assert_eq!(expand_channel(0x10, 5), 0x84);
        // 8-bit is identity.
        assert_eq!(expand_channel(0xab, 8), 0xab);
    }

    #[test]
    fn rgb565_decode_matches_component_expansion() {
        // White and black round-trip exactly.
        assert_eq!(rgb565_to_canonical(0xffff), [0xff, 0xff, 0xff]);
        assert_eq!(rgb565_to_canonical(0x0000), [0, 0, 0]);
        // Pure channels light only their component.
        assert_eq!(rgb565_to_canonical(0xf800), [0xff, 0, 0]);
        assert_eq!(rgb565_to_canonical(0x07e0), [0, 0xff, 0]);
        assert_eq!(rgb565_to_canonical(0x001f), [0, 0, 0xff]);
    }

    #[test]
    fn empty_image_compares_cleanly() {
        let empty = Rgb888Image::new(0, 0, vec![]).unwrap();
        let report = compare_rgb888(&empty, &empty, Tolerance::EXACT).unwrap();
        assert_eq!(report.total_pixels, 0);
        assert_eq!(report.outlier_fraction(), 0.0);
        assert!(report.within_tolerance);
    }
}
