use std::fmt;

use cmr_formats::riff_palette::PaletteEntry;

use crate::blit::{BlitDecision, BlitPlan, RectI16, plan_legacy_blit};
use crate::surface::IndexedSurface;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DisplayExtent {
    pub width: u16,
    pub height: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IndexedMode {
    pub extent: DisplayExtent,
    pub bits_per_pixel: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CooperativeMode {
    Windowed,
    ExclusiveFullscreen,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SurfaceTopology {
    PrimaryAndOffscreen,
    FlippingChainWithOneBackBuffer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DisplaySurfaces<S> {
    pub visible_front: S,
    pub back: S,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IndexedFrameTarget<S> {
    pub mode: IndexedMode,
    pub visible_front: S,
    pub back: S,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisplayStage {
    CreateDevice,
    SetCooperativeMode,
    SetDisplayMode,
    CreateSurfaces,
    CreatePalette,
    AttachFrontPalette,
    AttachBackPalette,
    UploadPalette,
    CreateSource,
}

#[derive(Debug, Eq, PartialEq)]
pub struct BackendFailure<E> {
    pub stage: DisplayStage,
    pub source: E,
}

impl<E: fmt::Display> fmt::Display for BackendFailure<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "display backend failed at {:?}: {}",
            self.stage, self.source
        )
    }
}

impl<E: std::error::Error + 'static> std::error::Error for BackendFailure<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum DisplayInitError<E> {
    UnsupportedBitsPerPixel(u8),
    InvalidExtent(DisplayExtent),
    Backend(BackendFailure<E>),
}

impl<E: fmt::Display> fmt::Display for DisplayInitError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedBitsPerPixel(depth) => {
                write!(
                    formatter,
                    "indexed display does not support {depth}-bit mode"
                )
            }
            Self::InvalidExtent(extent) => write!(
                formatter,
                "invalid indexed display extent {}x{}",
                extent.width, extent.height
            ),
            Self::Backend(error) => error.fmt(formatter),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for DisplayInitError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Backend(error) => Some(error),
            _ => None,
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum StartupCopyError<E> {
    UnsupportedSourceExtent {
        width: u32,
        height: u32,
    },
    InvalidSourceLayout {
        expected_pixels: u64,
        actual_pixels: usize,
        width: u32,
        pitch: u32,
    },
    Backend(BackendFailure<E>),
}

impl<E: fmt::Display> fmt::Display for StartupCopyError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSourceExtent { width, height } => {
                write!(
                    formatter,
                    "startup surface extent {width}x{height} exceeds the recovered rectangle range"
                )
            }
            Self::InvalidSourceLayout {
                expected_pixels,
                actual_pixels,
                width,
                pitch,
            } => write!(
                formatter,
                "invalid startup surface layout: width {width}, pitch {pitch}, expected {expected_pixels} pixels, found {actual_pixels}"
            ),
            Self::Backend(error) => error.fmt(formatter),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for StartupCopyError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Backend(error) => Some(error),
            _ => None,
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct StartupCopyOutcome<E> {
    pub front: Result<(), E>,
    pub back: Result<(), E>,
}

/// Platform implementation for the recovered indexed-display subset.
///
/// The backend owns persistent device, surface, and palette resources and must
/// release them when dropped. Uploaded indexed sources remain valid until
/// [`IndexedVideoBackend::destroy_source`] or backend drop; callers use their
/// explicit lifetime to model both temporary startup images and persistent
/// frontend resources.
pub trait IndexedVideoBackend {
    type Error;
    type Surface: Copy;
    type Palette: Copy;

    fn create_device(&mut self) -> Result<(), Self::Error>;
    fn set_cooperative_mode(&mut self, mode: CooperativeMode) -> Result<(), Self::Error>;
    fn set_display_mode(&mut self, mode: IndexedMode) -> Result<(), Self::Error>;
    fn create_surfaces(
        &mut self,
        topology: SurfaceTopology,
        mode: IndexedMode,
    ) -> Result<DisplaySurfaces<Self::Surface>, Self::Error>;
    fn create_palette(
        &mut self,
        entries: &[PaletteEntry; 256],
    ) -> Result<Self::Palette, Self::Error>;
    fn attach_palette(
        &mut self,
        surface: Self::Surface,
        palette: Self::Palette,
    ) -> Result<(), Self::Error>;
    fn upload_palette(
        &mut self,
        palette: Self::Palette,
        entries: &[PaletteEntry; 256],
    ) -> Result<(), Self::Error>;
    fn create_source(&mut self, surface: &IndexedSurface) -> Result<Self::Surface, Self::Error>;
    fn blit(
        &mut self,
        source: Self::Surface,
        destination: Self::Surface,
        plan: BlitPlan,
    ) -> Result<(), Self::Error>;
    fn destroy_source(&mut self, source: Self::Surface);
}

pub struct IndexedDisplay<B: IndexedVideoBackend> {
    backend: B,
    mode: IndexedMode,
    front: B::Surface,
    back: B::Surface,
    palette: B::Palette,
}

impl<B: IndexedVideoBackend> IndexedDisplay<B> {
    pub fn initialize(
        mut backend: B,
        mode: IndexedMode,
        cooperative: CooperativeMode,
        entries: &[PaletteEntry; 256],
    ) -> Result<Self, DisplayInitError<B::Error>> {
        validate_mode(mode)?;
        call(&mut backend, DisplayStage::CreateDevice, |backend| {
            backend.create_device()
        })
        .map_err(DisplayInitError::Backend)?;
        call(&mut backend, DisplayStage::SetCooperativeMode, |backend| {
            backend.set_cooperative_mode(cooperative)
        })
        .map_err(DisplayInitError::Backend)?;
        if cooperative == CooperativeMode::ExclusiveFullscreen {
            call(&mut backend, DisplayStage::SetDisplayMode, |backend| {
                backend.set_display_mode(mode)
            })
            .map_err(DisplayInitError::Backend)?;
        }
        let topology = match cooperative {
            CooperativeMode::Windowed => SurfaceTopology::PrimaryAndOffscreen,
            CooperativeMode::ExclusiveFullscreen => SurfaceTopology::FlippingChainWithOneBackBuffer,
        };
        let surfaces = call(&mut backend, DisplayStage::CreateSurfaces, |backend| {
            backend.create_surfaces(topology, mode)
        })
        .map_err(DisplayInitError::Backend)?;
        let front = surfaces.visible_front;
        let back = surfaces.back;
        let palette = call(&mut backend, DisplayStage::CreatePalette, |backend| {
            backend.create_palette(entries)
        })
        .map_err(DisplayInitError::Backend)?;
        call(&mut backend, DisplayStage::AttachFrontPalette, |backend| {
            backend.attach_palette(front, palette)
        })
        .map_err(DisplayInitError::Backend)?;
        call(&mut backend, DisplayStage::AttachBackPalette, |backend| {
            backend.attach_palette(back, palette)
        })
        .map_err(DisplayInitError::Backend)?;
        call(&mut backend, DisplayStage::UploadPalette, |backend| {
            backend.upload_palette(palette, entries)
        })
        .map_err(DisplayInitError::Backend)?;

        Ok(Self {
            backend,
            mode,
            front,
            back,
            palette,
        })
    }

    pub fn upload_palette(
        &mut self,
        entries: &[PaletteEntry; 256],
    ) -> Result<(), BackendFailure<B::Error>> {
        call(&mut self.backend, DisplayStage::UploadPalette, |backend| {
            backend.upload_palette(self.palette, entries)
        })
    }

    /// Copies the source to the visible primary surface and then the back
    /// surface. Both legacy blit results were ignored, so both calls are always
    /// attempted and their diagnostics are returned together.
    pub fn show_startup_surface(
        &mut self,
        surface: &IndexedSurface,
    ) -> Result<StartupCopyOutcome<B::Error>, StartupCopyError<B::Error>> {
        let expected_pixels = u64::from(surface.width) * u64::from(surface.height);
        if surface.width == 0
            || surface.height == 0
            || surface.pitch != surface.width
            || u64::try_from(surface.pixels.len()).ok() != Some(expected_pixels)
        {
            return Err(StartupCopyError::InvalidSourceLayout {
                expected_pixels,
                actual_pixels: surface.pixels.len(),
                width: surface.width,
                pitch: surface.pitch,
            });
        }
        let source_width = i16::try_from(surface.width).map_err(|_| {
            StartupCopyError::UnsupportedSourceExtent {
                width: surface.width,
                height: surface.height,
            }
        })?;
        let source_height = i16::try_from(surface.height).map_err(|_| {
            StartupCopyError::UnsupportedSourceExtent {
                width: surface.width,
                height: surface.height,
            }
        })?;
        let destination_width = self.mode.extent.width as i16;
        let destination_height = self.mode.extent.height as i16;
        let plan = plan_legacy_blit(
            RectI16 {
                x: 0,
                y: 0,
                width: source_width,
                height: source_height,
            },
            RectI16 {
                x: 0,
                y: 0,
                width: destination_width,
                height: destination_height,
            },
            destination_width,
            destination_height,
            None,
        );
        let BlitDecision::Blit(plan) = plan else {
            unreachable!("validated positive full-display rectangle cannot be clipped out")
        };
        let source = call(&mut self.backend, DisplayStage::CreateSource, |backend| {
            backend.create_source(surface)
        })
        .map_err(StartupCopyError::Backend)?;
        let front = self.backend.blit(source, self.front, plan);
        let back = self.backend.blit(source, self.back, plan);
        self.backend.destroy_source(source);
        Ok(StartupCopyOutcome { front, back })
    }

    pub fn mode(&self) -> IndexedMode {
        self.mode
    }

    pub fn frame_target(&self) -> IndexedFrameTarget<B::Surface> {
        IndexedFrameTarget {
            mode: self.mode,
            visible_front: self.front,
            back: self.back,
        }
    }

    pub fn into_backend(self) -> B {
        self.backend
    }
}

fn validate_mode<E>(mode: IndexedMode) -> Result<(), DisplayInitError<E>> {
    if mode.bits_per_pixel != 8 {
        return Err(DisplayInitError::UnsupportedBitsPerPixel(
            mode.bits_per_pixel,
        ));
    }
    if mode.extent.width == 0
        || mode.extent.height == 0
        || mode.extent.width > i16::MAX as u16
        || mode.extent.height > i16::MAX as u16
    {
        return Err(DisplayInitError::InvalidExtent(mode.extent));
    }
    Ok(())
}

fn call<B, T>(
    backend: &mut B,
    stage: DisplayStage,
    operation: impl FnOnce(&mut B) -> Result<T, B::Error>,
) -> Result<T, BackendFailure<B::Error>>
where
    B: IndexedVideoBackend,
{
    operation(backend).map_err(|source| BackendFailure { stage, source })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cmr_formats::riff_palette::PaletteEntry;

    #[derive(Clone, Debug, Eq, PartialEq)]
    enum Event {
        CreateDevice,
        Cooperative(CooperativeMode),
        SetMode(IndexedMode),
        CreateSurfaces(SurfaceTopology, IndexedMode),
        CreatePalette(u8),
        Attach(u8, u8),
        Upload(u8, u8),
        CreateSource,
        Blit(u8, u8, BlitPlan),
        Destroy(u8),
    }

    #[derive(Default)]
    struct RecordingBackend {
        events: Vec<Event>,
        fail_stage: Option<DisplayStage>,
        fail_front_blit: bool,
        fail_back_blit: bool,
    }

    impl RecordingBackend {
        fn record(&mut self, stage: DisplayStage, event: Event) -> Result<(), &'static str> {
            self.events.push(event);
            if self.fail_stage == Some(stage) {
                Err("injected failure")
            } else {
                Ok(())
            }
        }
    }

    impl IndexedVideoBackend for RecordingBackend {
        type Error = &'static str;
        type Surface = u8;
        type Palette = u8;

        fn create_device(&mut self) -> Result<(), Self::Error> {
            self.record(DisplayStage::CreateDevice, Event::CreateDevice)
        }

        fn set_cooperative_mode(&mut self, mode: CooperativeMode) -> Result<(), Self::Error> {
            self.record(DisplayStage::SetCooperativeMode, Event::Cooperative(mode))
        }

        fn set_display_mode(&mut self, mode: IndexedMode) -> Result<(), Self::Error> {
            self.record(DisplayStage::SetDisplayMode, Event::SetMode(mode))
        }

        fn create_surfaces(
            &mut self,
            topology: SurfaceTopology,
            mode: IndexedMode,
        ) -> Result<DisplaySurfaces<Self::Surface>, Self::Error> {
            self.record(
                DisplayStage::CreateSurfaces,
                Event::CreateSurfaces(topology, mode),
            )?;
            Ok(DisplaySurfaces {
                visible_front: 1,
                back: 2,
            })
        }

        fn create_palette(
            &mut self,
            entries: &[PaletteEntry; 256],
        ) -> Result<Self::Palette, Self::Error> {
            self.record(
                DisplayStage::CreatePalette,
                Event::CreatePalette(entries[0].red),
            )?;
            Ok(9)
        }

        fn attach_palette(
            &mut self,
            surface: Self::Surface,
            palette: Self::Palette,
        ) -> Result<(), Self::Error> {
            let stage = if surface == 1 {
                DisplayStage::AttachFrontPalette
            } else {
                DisplayStage::AttachBackPalette
            };
            self.record(stage, Event::Attach(surface, palette))
        }

        fn upload_palette(
            &mut self,
            palette: Self::Palette,
            entries: &[PaletteEntry; 256],
        ) -> Result<(), Self::Error> {
            self.record(
                DisplayStage::UploadPalette,
                Event::Upload(palette, entries[0].red),
            )
        }

        fn create_source(
            &mut self,
            _surface: &IndexedSurface,
        ) -> Result<Self::Surface, Self::Error> {
            self.record(DisplayStage::CreateSource, Event::CreateSource)?;
            Ok(3)
        }

        fn blit(
            &mut self,
            source: Self::Surface,
            destination: Self::Surface,
            plan: BlitPlan,
        ) -> Result<(), Self::Error> {
            self.events.push(Event::Blit(source, destination, plan));
            if destination == 1 && self.fail_front_blit {
                Err("front blit failed")
            } else if destination == 2 && self.fail_back_blit {
                Err("back blit failed")
            } else {
                Ok(())
            }
        }

        fn destroy_source(&mut self, source: Self::Surface) {
            self.events.push(Event::Destroy(source));
        }
    }

    fn mode(width: u16, height: u16) -> IndexedMode {
        IndexedMode {
            extent: DisplayExtent { width, height },
            bits_per_pixel: 8,
        }
    }

    fn palette() -> [PaletteEntry; 256] {
        [PaletteEntry {
            red: 1,
            green: 2,
            blue: 3,
            flags: 0,
        }; 256]
    }

    fn surface(width: u32, height: u32) -> IndexedSurface {
        IndexedSurface {
            width,
            height,
            pitch: width,
            pixels: vec![0; usize::try_from(width * height).unwrap()],
            palette: palette(),
        }
    }

    #[test]
    fn windowed_initialization_has_recovered_order_and_shared_palette() {
        let display = IndexedDisplay::initialize(
            RecordingBackend::default(),
            mode(640, 480),
            CooperativeMode::Windowed,
            &palette(),
        )
        .unwrap();

        assert_eq!(
            display.into_backend().events,
            [
                Event::CreateDevice,
                Event::Cooperative(CooperativeMode::Windowed),
                Event::CreateSurfaces(SurfaceTopology::PrimaryAndOffscreen, mode(640, 480)),
                Event::CreatePalette(1),
                Event::Attach(1, 9),
                Event::Attach(2, 9),
                Event::Upload(9, 1),
            ]
        );
    }

    #[test]
    fn fullscreen_sets_mode_before_creating_a_flipping_chain() {
        let display = IndexedDisplay::initialize(
            RecordingBackend::default(),
            mode(800, 600),
            CooperativeMode::ExclusiveFullscreen,
            &palette(),
        )
        .unwrap();
        let events = display.into_backend().events;

        assert_eq!(
            events[1],
            Event::Cooperative(CooperativeMode::ExclusiveFullscreen)
        );
        assert_eq!(events[2], Event::SetMode(mode(800, 600)));
        assert_eq!(
            events[3],
            Event::CreateSurfaces(
                SurfaceTopology::FlippingChainWithOneBackBuffer,
                mode(800, 600)
            )
        );
    }

    #[test]
    fn frame_target_copies_exact_mode_and_surface_handles_without_backend_events() {
        let display = IndexedDisplay::initialize(
            RecordingBackend::default(),
            mode(800, 600),
            CooperativeMode::ExclusiveFullscreen,
            &palette(),
        )
        .unwrap();

        assert_eq!(
            display.frame_target(),
            IndexedFrameTarget {
                mode: mode(800, 600),
                visible_front: 1,
                back: 2,
            }
        );
        assert_eq!(display.into_backend().events.len(), 8);
    }

    #[test]
    fn visible_blit_failure_does_not_skip_back_copy_or_source_release() {
        let backend = RecordingBackend {
            fail_front_blit: true,
            ..RecordingBackend::default()
        };
        let mut display =
            IndexedDisplay::initialize(backend, mode(4, 1), CooperativeMode::Windowed, &palette())
                .unwrap();
        let outcome = display.show_startup_surface(&surface(4, 1)).unwrap();

        assert_eq!(outcome.front, Err("front blit failed"));
        assert_eq!(outcome.back, Ok(()));
        let events = display.into_backend().events;
        assert_eq!(events[7], Event::CreateSource);
        let Event::Blit(3, 1, front_plan) = events[8] else {
            panic!("expected visible-front blit")
        };
        assert_eq!(front_plan.legacy_flags(), 0x10);
        assert_eq!(events[9], Event::Blit(3, 2, front_plan));
        assert_eq!(events[10], Event::Destroy(3));
    }

    #[test]
    fn both_blit_diagnostics_are_retained_and_source_is_still_released() {
        let backend = RecordingBackend {
            fail_front_blit: true,
            fail_back_blit: true,
            ..RecordingBackend::default()
        };
        let mut display =
            IndexedDisplay::initialize(backend, mode(4, 1), CooperativeMode::Windowed, &palette())
                .unwrap();
        let outcome = display.show_startup_surface(&surface(4, 1)).unwrap();

        assert_eq!(outcome.front, Err("front blit failed"));
        assert_eq!(outcome.back, Err("back blit failed"));
        assert_eq!(
            display.into_backend().events.last(),
            Some(&Event::Destroy(3))
        );
    }

    #[test]
    fn source_creation_failure_performs_no_blit_or_release() {
        let backend = RecordingBackend {
            fail_stage: Some(DisplayStage::CreateSource),
            ..RecordingBackend::default()
        };
        let mut display =
            IndexedDisplay::initialize(backend, mode(4, 1), CooperativeMode::Windowed, &palette())
                .unwrap();
        let error = display.show_startup_surface(&surface(4, 1)).unwrap_err();

        assert!(matches!(
            error,
            StartupCopyError::Backend(BackendFailure {
                stage: DisplayStage::CreateSource,
                source: "injected failure"
            })
        ));
        assert_eq!(
            display.into_backend().events.last(),
            Some(&Event::CreateSource)
        );
    }

    #[test]
    fn display_extent_drives_stretch_planning_without_assuming_640_by_480() {
        let mut display = IndexedDisplay::initialize(
            RecordingBackend::default(),
            mode(8, 2),
            CooperativeMode::Windowed,
            &palette(),
        )
        .unwrap();
        display.show_startup_surface(&surface(4, 1)).unwrap();
        let events = display.into_backend().events;
        let Event::Blit(_, _, plan) = events[8] else {
            panic!("expected blit")
        };
        assert_eq!(plan.legacy_flags(), 0x0100_0000);
        assert_eq!(plan.destination.right, 8);
        assert_eq!(plan.destination.bottom, 2);
    }

    #[test]
    fn invalid_modes_and_initialization_failure_stop_before_later_operations() {
        let invalid = IndexedDisplay::initialize(
            RecordingBackend::default(),
            IndexedMode {
                extent: DisplayExtent {
                    width: 640,
                    height: 480,
                },
                bits_per_pixel: 16,
            },
            CooperativeMode::Windowed,
            &palette(),
        );
        assert!(matches!(
            invalid,
            Err(DisplayInitError::UnsupportedBitsPerPixel(16))
        ));

        let backend = RecordingBackend {
            fail_stage: Some(DisplayStage::AttachFrontPalette),
            ..RecordingBackend::default()
        };
        let failed = IndexedDisplay::initialize(
            backend,
            mode(640, 480),
            CooperativeMode::Windowed,
            &palette(),
        );
        let Err(DisplayInitError::Backend(failure)) = failed else {
            panic!("expected backend failure")
        };
        assert_eq!(failure.stage, DisplayStage::AttachFrontPalette);
    }

    #[test]
    fn rejects_invalid_mode_and_source_geometry_before_backend_calls() {
        for extent in [
            DisplayExtent {
                width: 0,
                height: 480,
            },
            DisplayExtent {
                width: 640,
                height: u16::MAX,
            },
        ] {
            assert!(matches!(
                IndexedDisplay::initialize(
                    RecordingBackend::default(),
                    IndexedMode {
                        extent,
                        bits_per_pixel: 8,
                    },
                    CooperativeMode::Windowed,
                    &palette(),
                ),
                Err(DisplayInitError::InvalidExtent(actual)) if actual == extent
            ));
        }

        let mut display = IndexedDisplay::initialize(
            RecordingBackend::default(),
            mode(4, 1),
            CooperativeMode::Windowed,
            &palette(),
        )
        .unwrap();
        let mut malformed = surface(4, 1);
        malformed.pixels.pop();
        assert!(matches!(
            display.show_startup_surface(&malformed),
            Err(StartupCopyError::InvalidSourceLayout {
                expected_pixels: 4,
                actual_pixels: 3,
                ..
            })
        ));
        assert_eq!(display.into_backend().events.len(), 7);
    }
}
