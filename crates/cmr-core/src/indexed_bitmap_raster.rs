//! Identity-neutral checked execution for persistent indexed bitmaps.
//!
//! Screen-specific planners retain their own identities and geometry policy.
//! This boundary validates a concrete source/destination pair before exposing
//! it to the recovered legacy clipping and dispatch decision.

use std::fmt;

use crate::blit::{BlitDecision, ColorKey, RectEdges, RectI16, plan_legacy_blit};
use crate::display::{DisplayExtent, IndexedVideoBackend};
use crate::source_store::{
    SourceAccessError, SourceBlitError, UploadedIndexedBitmap, UploadedSourceStore,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IndexedBitmapRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl IndexedBitmapRect {
    #[must_use]
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IndexedBitmapGeometryRole {
    Source,
    Destination,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IndexedBitmapPlanError {
    InvalidDestinationExtent(DisplayExtent),
    NonPositiveGeometry {
        role: IndexedBitmapGeometryRole,
        rectangle: IndexedBitmapRect,
    },
    GeometryOutOfI16Range {
        role: IndexedBitmapGeometryRole,
        rectangle: IndexedBitmapRect,
    },
    SourceOutOfBounds {
        rectangle: IndexedBitmapRect,
        extent: (u32, u32),
    },
    ClippedSourceOutOfBounds {
        source: RectEdges,
        extent: (u32, u32),
    },
}

impl fmt::Display for IndexedBitmapPlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDestinationExtent(extent) => write!(
                formatter,
                "indexed bitmap destination extent {}x{} is outside the positive i16 range",
                extent.width, extent.height
            ),
            Self::NonPositiveGeometry { role, rectangle } => write!(
                formatter,
                "indexed bitmap {role:?} rectangle {rectangle:?} has a nonpositive size"
            ),
            Self::GeometryOutOfI16Range { role, rectangle } => write!(
                formatter,
                "indexed bitmap {role:?} rectangle {rectangle:?} is outside the i16 range"
            ),
            Self::SourceOutOfBounds { rectangle, extent } => write!(
                formatter,
                "indexed bitmap source rectangle {rectangle:?} is outside {}x{}",
                extent.0, extent.1
            ),
            Self::ClippedSourceOutOfBounds { source, extent } => write!(
                formatter,
                "indexed bitmap clipped source edges {source:?} are empty or outside {}x{}",
                extent.0, extent.1
            ),
        }
    }
}

impl std::error::Error for IndexedBitmapPlanError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IndexedBitmapBlitOutcome {
    NoOp,
    Blitted,
}

#[derive(Debug, Eq, PartialEq)]
pub enum IndexedBitmapBlitError<E> {
    Plan(IndexedBitmapPlanError),
    Access(SourceAccessError),
    Backend(E),
}

impl<E: fmt::Display> fmt::Display for IndexedBitmapBlitError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Plan(source) => source.fmt(formatter),
            Self::Access(source) => source.fmt(formatter),
            Self::Backend(source) => write!(formatter, "indexed bitmap blit failed: {source}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for IndexedBitmapBlitError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Plan(source) => Some(source),
            Self::Access(source) => Some(source),
            Self::Backend(source) => Some(source),
        }
    }
}

/// Validates and plans one legacy indexed blit without backend effects.
pub fn plan_indexed_bitmap_blit(
    bitmap: UploadedIndexedBitmap,
    source: IndexedBitmapRect,
    destination: IndexedBitmapRect,
    destination_extent: DisplayExtent,
    color_key: Option<ColorKey>,
) -> Result<BlitDecision, IndexedBitmapPlanError> {
    let destination_width = positive_i16(destination_extent.width).ok_or(
        IndexedBitmapPlanError::InvalidDestinationExtent(destination_extent),
    )?;
    let destination_height = positive_i16(destination_extent.height).ok_or(
        IndexedBitmapPlanError::InvalidDestinationExtent(destination_extent),
    )?;
    let source_i16 = checked_rectangle(IndexedBitmapGeometryRole::Source, source)?;
    let destination_i16 = checked_rectangle(IndexedBitmapGeometryRole::Destination, destination)?;
    validate_source_bounds(source, bitmap.extent())?;

    let decision = plan_legacy_blit(
        source_i16,
        destination_i16,
        destination_width,
        destination_height,
        color_key,
    );
    if let BlitDecision::Blit(plan) = decision {
        validate_clipped_source(plan.source, bitmap.extent())?;
    }
    Ok(decision)
}

/// Plans and submits one checked persistent indexed-bitmap blit.
pub fn execute_indexed_bitmap_blit<B: IndexedVideoBackend>(
    store: &mut UploadedSourceStore<'_, B>,
    bitmap: UploadedIndexedBitmap,
    destination_surface: B::Surface,
    source: IndexedBitmapRect,
    destination: IndexedBitmapRect,
    destination_extent: DisplayExtent,
    color_key: Option<ColorKey>,
) -> Result<IndexedBitmapBlitOutcome, IndexedBitmapBlitError<B::Error>> {
    match plan_indexed_bitmap_blit(bitmap, source, destination, destination_extent, color_key)
        .map_err(IndexedBitmapBlitError::Plan)?
    {
        BlitDecision::NoOp => Ok(IndexedBitmapBlitOutcome::NoOp),
        BlitDecision::Blit(plan) => {
            bitmap
                .blit(store, destination_surface, plan)
                .map_err(|source| match source {
                    SourceBlitError::Access(source) => IndexedBitmapBlitError::Access(source),
                    SourceBlitError::Backend(source) => IndexedBitmapBlitError::Backend(source),
                })?;
            Ok(IndexedBitmapBlitOutcome::Blitted)
        }
    }
}

fn positive_i16(value: u16) -> Option<i16> {
    i16::try_from(value).ok().filter(|value| *value > 0)
}

fn checked_rectangle(
    role: IndexedBitmapGeometryRole,
    rectangle: IndexedBitmapRect,
) -> Result<RectI16, IndexedBitmapPlanError> {
    if rectangle.width <= 0 || rectangle.height <= 0 {
        return Err(IndexedBitmapPlanError::NonPositiveGeometry { role, rectangle });
    }
    let converted = || {
        Some(RectI16 {
            x: i16::try_from(rectangle.x).ok()?,
            y: i16::try_from(rectangle.y).ok()?,
            width: i16::try_from(rectangle.width).ok()?,
            height: i16::try_from(rectangle.height).ok()?,
        })
    };
    converted().ok_or(IndexedBitmapPlanError::GeometryOutOfI16Range { role, rectangle })
}

fn validate_source_bounds(
    rectangle: IndexedBitmapRect,
    extent: (u32, u32),
) -> Result<(), IndexedBitmapPlanError> {
    let right = rectangle.x.checked_add(rectangle.width);
    let bottom = rectangle.y.checked_add(rectangle.height);
    let within = rectangle.x >= 0
        && rectangle.y >= 0
        && right.is_some_and(|right| u32::try_from(right).is_ok_and(|right| right <= extent.0))
        && bottom
            .is_some_and(|bottom| u32::try_from(bottom).is_ok_and(|bottom| bottom <= extent.1));
    if !within {
        return Err(IndexedBitmapPlanError::SourceOutOfBounds { rectangle, extent });
    }
    Ok(())
}

fn validate_clipped_source(
    source: RectEdges,
    extent: (u32, u32),
) -> Result<(), IndexedBitmapPlanError> {
    let within = source.left >= 0
        && source.top >= 0
        && source.left < source.right
        && source.top < source.bottom
        && u32::try_from(source.right).is_ok_and(|right| right <= extent.0)
        && u32::try_from(source.bottom).is_ok_and(|bottom| bottom <= extent.1);
    if !within {
        return Err(IndexedBitmapPlanError::ClippedSourceOutOfBounds { source, extent });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use cmr_formats::riff_palette::PaletteEntry;

    use super::*;
    use crate::blit::{BlitMethod, BlitPlan, RectEdges};
    use crate::display::{CooperativeMode, DisplaySurfaces, IndexedMode, SurfaceTopology};
    use crate::surface::IndexedSurface;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum BackendError {
        Blit,
    }

    impl fmt::Display for BackendError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "{self:?}")
        }
    }

    impl std::error::Error for BackendError {}

    #[derive(Default)]
    struct RecordingBackend {
        events: Vec<(&'static str, u8)>,
        fail_blit: bool,
    }

    impl IndexedVideoBackend for RecordingBackend {
        type Error = BackendError;
        type Surface = u8;
        type Palette = u8;

        fn create_device(&mut self) -> Result<(), Self::Error> {
            unreachable!()
        }

        fn set_cooperative_mode(&mut self, _: CooperativeMode) -> Result<(), Self::Error> {
            unreachable!()
        }

        fn set_display_mode(&mut self, _: IndexedMode) -> Result<(), Self::Error> {
            unreachable!()
        }

        fn create_surfaces(
            &mut self,
            _: SurfaceTopology,
            _: IndexedMode,
        ) -> Result<DisplaySurfaces<Self::Surface>, Self::Error> {
            unreachable!()
        }

        fn create_palette(
            &mut self,
            _: &[PaletteEntry; 256],
        ) -> Result<Self::Palette, Self::Error> {
            unreachable!()
        }

        fn attach_palette(
            &mut self,
            _: Self::Surface,
            _: Self::Palette,
        ) -> Result<(), Self::Error> {
            unreachable!()
        }

        fn upload_palette(
            &mut self,
            _: Self::Palette,
            _: &[PaletteEntry; 256],
        ) -> Result<(), Self::Error> {
            unreachable!()
        }

        fn create_source(&mut self, _: &IndexedSurface) -> Result<Self::Surface, Self::Error> {
            let handle = 10 + self.events.len() as u8;
            self.events.push(("create", handle));
            Ok(handle)
        }

        fn blit(
            &mut self,
            source: Self::Surface,
            _: Self::Surface,
            _: BlitPlan,
        ) -> Result<(), Self::Error> {
            self.events.push(("blit", source));
            if self.fail_blit {
                Err(BackendError::Blit)
            } else {
                Ok(())
            }
        }

        fn destroy_source(&mut self, source: Self::Surface) {
            self.events.push(("destroy", source));
        }
    }

    const VIEWPORT: DisplayExtent = DisplayExtent {
        width: 640,
        height: 480,
    };

    fn surface(width: u32, height: u32) -> IndexedSurface {
        IndexedSurface {
            width,
            height,
            pitch: width,
            pixels: vec![1; (width * height) as usize],
            palette: [PaletteEntry {
                red: 0,
                green: 0,
                blue: 0,
                flags: 0,
            }; 256],
        }
    }

    fn uploaded(
        backend: &mut RecordingBackend,
        width: u32,
        height: u32,
    ) -> (
        UploadedSourceStore<'_, RecordingBackend>,
        UploadedIndexedBitmap,
    ) {
        let mut store = UploadedSourceStore::new(backend);
        let bitmap = store.upload_bitmap(&surface(width, height)).unwrap();
        (store, bitmap)
    }

    #[test]
    fn aligned_logo_draw_uses_logical_182_columns_of_184_column_surface() {
        let mut backend = RecordingBackend::default();
        let (mut store, bitmap) = uploaded(&mut backend, 184, 128);
        let key = ColorKey { low: 0, high: 0 };
        let decision = plan_indexed_bitmap_blit(
            bitmap,
            IndexedBitmapRect::new(0, 0, 182, 128),
            IndexedBitmapRect::new(40, 50, 182, 128),
            VIEWPORT,
            Some(key),
        )
        .unwrap();
        assert_eq!(
            decision,
            BlitDecision::Blit(BlitPlan {
                source: RectEdges {
                    left: 0,
                    top: 0,
                    right: 182,
                    bottom: 128,
                },
                destination: RectEdges {
                    left: 40,
                    top: 50,
                    right: 222,
                    bottom: 178,
                },
                method: BlitMethod::Fast,
                color_key: Some(key),
            })
        );
        assert_eq!(
            execute_indexed_bitmap_blit(
                &mut store,
                bitmap,
                99,
                IndexedBitmapRect::new(0, 0, 182, 128),
                IndexedBitmapRect::new(40, 50, 182, 128),
                VIEWPORT,
                Some(key),
            ),
            Ok(IndexedBitmapBlitOutcome::Blitted)
        );
        assert_eq!(store.backend().events, [("create", 10), ("blit", 10)]);
    }

    #[test]
    fn stretch_method_survives_destination_clipping_and_shifts_source_literally() {
        let mut backend = RecordingBackend::default();
        let (store, bitmap) = uploaded(&mut backend, 256, 256);
        assert_eq!(
            plan_indexed_bitmap_blit(
                bitmap,
                IndexedBitmapRect::new(0, 0, 256, 256),
                IndexedBitmapRect::new(-10, -4, 180, 33),
                DisplayExtent {
                    width: 160,
                    height: 25,
                },
                None,
            ),
            Ok(BlitDecision::Blit(BlitPlan {
                source: RectEdges {
                    left: 10,
                    top: 4,
                    right: 246,
                    bottom: 252,
                },
                destination: RectEdges {
                    left: 0,
                    top: 0,
                    right: 160,
                    bottom: 25,
                },
                method: BlitMethod::Stretch,
                color_key: None,
            }))
        );
        assert_eq!(store.backend().events, [("create", 10)]);
    }

    #[test]
    fn post_clip_horizontal_and_vertical_source_escape_is_rejected_without_submission() {
        for (source, destination, expected) in [
            (
                IndexedBitmapRect::new(0, 0, 100, 1),
                IndexedBitmapRect::new(-101, 0, 200, 1),
                RectEdges {
                    left: 101,
                    top: 0,
                    right: 100,
                    bottom: 1,
                },
            ),
            (
                IndexedBitmapRect::new(0, 0, 1, 100),
                IndexedBitmapRect::new(0, -101, 1, 200),
                RectEdges {
                    left: 0,
                    top: 101,
                    right: 1,
                    bottom: 100,
                },
            ),
        ] {
            let mut backend = RecordingBackend::default();
            let (mut store, bitmap) = uploaded(&mut backend, 100, 100);
            assert_eq!(
                execute_indexed_bitmap_blit(
                    &mut store,
                    bitmap,
                    99,
                    source,
                    destination,
                    VIEWPORT,
                    None,
                ),
                Err(IndexedBitmapBlitError::Plan(
                    IndexedBitmapPlanError::ClippedSourceOutOfBounds {
                        source: expected,
                        extent: (100, 100),
                    }
                ))
            );
            assert_eq!(store.backend().events, [("create", 10)]);
        }
    }

    #[test]
    fn fully_clipped_destination_is_a_no_op_without_backend_submission() {
        let mut backend = RecordingBackend::default();
        let (mut store, bitmap) = uploaded(&mut backend, 4, 4);
        assert_eq!(
            execute_indexed_bitmap_blit(
                &mut store,
                bitmap,
                99,
                IndexedBitmapRect::new(0, 0, 4, 4),
                IndexedBitmapRect::new(640, 0, 4, 4),
                VIEWPORT,
                None,
            ),
            Ok(IndexedBitmapBlitOutcome::NoOp)
        );
        assert_eq!(store.backend().events, [("create", 10)]);
    }

    #[test]
    fn every_invalid_destination_extent_fails_before_backend_submission() {
        for extent in [
            DisplayExtent {
                width: 0,
                height: 1,
            },
            DisplayExtent {
                width: 1,
                height: 0,
            },
            DisplayExtent {
                width: i16::MAX as u16 + 1,
                height: 1,
            },
            DisplayExtent {
                width: 1,
                height: i16::MAX as u16 + 1,
            },
        ] {
            let mut backend = RecordingBackend::default();
            let (mut store, bitmap) = uploaded(&mut backend, 4, 4);
            assert_eq!(
                execute_indexed_bitmap_blit(
                    &mut store,
                    bitmap,
                    99,
                    IndexedBitmapRect::new(0, 0, 1, 1),
                    IndexedBitmapRect::new(0, 0, 1, 1),
                    extent,
                    None,
                ),
                Err(IndexedBitmapBlitError::Plan(
                    IndexedBitmapPlanError::InvalidDestinationExtent(extent)
                ))
            );
            assert_eq!(store.backend().events, [("create", 10)]);
        }
    }

    #[test]
    fn every_nonpositive_source_and_destination_size_is_rejected() {
        for (role, source, destination) in [
            (
                IndexedBitmapGeometryRole::Source,
                IndexedBitmapRect::new(0, 0, 0, 1),
                IndexedBitmapRect::new(0, 0, 1, 1),
            ),
            (
                IndexedBitmapGeometryRole::Source,
                IndexedBitmapRect::new(0, 0, 1, -1),
                IndexedBitmapRect::new(0, 0, 1, 1),
            ),
            (
                IndexedBitmapGeometryRole::Destination,
                IndexedBitmapRect::new(0, 0, 1, 1),
                IndexedBitmapRect::new(0, 0, 0, 1),
            ),
            (
                IndexedBitmapGeometryRole::Destination,
                IndexedBitmapRect::new(0, 0, 1, 1),
                IndexedBitmapRect::new(0, 0, 1, -1),
            ),
        ] {
            let mut backend = RecordingBackend::default();
            let (mut store, bitmap) = uploaded(&mut backend, 4, 4);
            let rectangle = match role {
                IndexedBitmapGeometryRole::Source => source,
                IndexedBitmapGeometryRole::Destination => destination,
            };
            assert_eq!(
                execute_indexed_bitmap_blit(
                    &mut store,
                    bitmap,
                    99,
                    source,
                    destination,
                    VIEWPORT,
                    None,
                ),
                Err(IndexedBitmapBlitError::Plan(
                    IndexedBitmapPlanError::NonPositiveGeometry { role, rectangle }
                ))
            );
            assert_eq!(store.backend().events, [("create", 10)]);
        }
    }

    #[test]
    fn every_rectangle_field_outside_i16_is_rejected() {
        let base = IndexedBitmapRect::new(0, 0, 1, 1);
        for (role, rectangle) in [
            (
                IndexedBitmapGeometryRole::Source,
                IndexedBitmapRect::new(i16::MIN as i32 - 1, 0, 1, 1),
            ),
            (
                IndexedBitmapGeometryRole::Source,
                IndexedBitmapRect::new(0, i16::MAX as i32 + 1, 1, 1),
            ),
            (
                IndexedBitmapGeometryRole::Source,
                IndexedBitmapRect::new(0, 0, i16::MAX as i32 + 1, 1),
            ),
            (
                IndexedBitmapGeometryRole::Source,
                IndexedBitmapRect::new(0, 0, 1, i16::MAX as i32 + 1),
            ),
            (
                IndexedBitmapGeometryRole::Destination,
                IndexedBitmapRect::new(i16::MAX as i32 + 1, 0, 1, 1),
            ),
            (
                IndexedBitmapGeometryRole::Destination,
                IndexedBitmapRect::new(0, i16::MIN as i32 - 1, 1, 1),
            ),
            (
                IndexedBitmapGeometryRole::Destination,
                IndexedBitmapRect::new(0, 0, i16::MAX as i32 + 1, 1),
            ),
            (
                IndexedBitmapGeometryRole::Destination,
                IndexedBitmapRect::new(0, 0, 1, i16::MAX as i32 + 1),
            ),
        ] {
            let mut backend = RecordingBackend::default();
            let (mut store, bitmap) = uploaded(&mut backend, 4, 4);
            let (source, destination) = match role {
                IndexedBitmapGeometryRole::Source => (rectangle, base),
                IndexedBitmapGeometryRole::Destination => (base, rectangle),
            };
            assert_eq!(
                execute_indexed_bitmap_blit(
                    &mut store,
                    bitmap,
                    99,
                    source,
                    destination,
                    VIEWPORT,
                    None,
                ),
                Err(IndexedBitmapBlitError::Plan(
                    IndexedBitmapPlanError::GeometryOutOfI16Range { role, rectangle }
                ))
            );
            assert_eq!(store.backend().events, [("create", 10)]);
        }
    }

    #[test]
    fn every_source_edge_is_checked_against_uploaded_extent() {
        for rectangle in [
            IndexedBitmapRect::new(-1, 0, 1, 1),
            IndexedBitmapRect::new(0, -1, 1, 1),
            IndexedBitmapRect::new(3, 0, 2, 1),
            IndexedBitmapRect::new(0, 3, 1, 2),
            IndexedBitmapRect::new(i16::MAX as i32, 0, 1, 1),
            IndexedBitmapRect::new(0, i16::MAX as i32, 1, 1),
        ] {
            let mut backend = RecordingBackend::default();
            let (mut store, bitmap) = uploaded(&mut backend, 4, 4);
            assert_eq!(
                execute_indexed_bitmap_blit(
                    &mut store,
                    bitmap,
                    99,
                    rectangle,
                    IndexedBitmapRect::new(0, 0, 1, 1),
                    VIEWPORT,
                    None,
                ),
                Err(IndexedBitmapBlitError::Plan(
                    IndexedBitmapPlanError::SourceOutOfBounds {
                        rectangle,
                        extent: (4, 4),
                    }
                ))
            );
            assert_eq!(store.backend().events, [("create", 10)]);
        }
    }

    #[test]
    fn foreign_released_and_backend_failures_remain_distinct() {
        let mut first_backend = RecordingBackend::default();
        let foreign = {
            let (store, bitmap) = uploaded(&mut first_backend, 4, 4);
            drop(store);
            bitmap
        };
        let mut second_backend = RecordingBackend::default();
        let (mut second, local) = uploaded(&mut second_backend, 4, 4);
        let args = (
            99,
            IndexedBitmapRect::new(0, 0, 1, 1),
            IndexedBitmapRect::new(0, 0, 1, 1),
            VIEWPORT,
            None,
        );
        assert!(matches!(
            execute_indexed_bitmap_blit(
                &mut second,
                foreign,
                args.0,
                args.1,
                args.2,
                args.3,
                args.4,
            ),
            Err(IndexedBitmapBlitError::Access(
                SourceAccessError::ForeignKey(key)
            )) if key == foreign.source_key()
        ));
        second.release(local.source_key()).unwrap();
        assert!(matches!(
            execute_indexed_bitmap_blit(
                &mut second,
                local,
                args.0,
                args.1,
                args.2,
                args.3,
                args.4,
            ),
            Err(IndexedBitmapBlitError::Access(
                SourceAccessError::ReleasedKey(key)
            )) if key == local.source_key()
        ));

        let live = second.upload_bitmap(&surface(4, 4)).unwrap();
        second.backend_mut().fail_blit = true;
        assert_eq!(
            execute_indexed_bitmap_blit(&mut second, live, args.0, args.1, args.2, args.3, args.4,),
            Err(IndexedBitmapBlitError::Backend(BackendError::Blit))
        );
    }
}
