//! Concrete root-menu C3D integration for the indexed software framebuffer.
//!
//! This is the deterministic branch-zero compatibility oracle. It deliberately
//! keeps the still-opaque post-root scene tail as a no-op and does not claim
//! driver-exact Direct3D palette conversion or rasterization.

use std::fmt;

use cmr_core::{
    c3d_batch::C3dDrawCommand,
    c3d_type2_fog::C3dType2PreDrawVertex,
    championship_car_execute::{ChampionshipCarBatchBackend, ChampionshipCarPreDrawProfile},
    display::DisplayExtent,
    frontend_c3d_scene::{C3dBindingState, FrontendC3dSceneBackend},
    root_menu_c3d_execute::RootMenuC3dBatchBackend,
    root_menu_c3d_frame_plan::RootMenuC3dDrawCommand,
    root_menu_c3d_scene::{RootMenuC3dBindingState, RootMenuC3dSceneBackend},
    root_menu_c3d_transform::{
        C3dMatrixWords, C3dViewportWords, ROOT_MENU_C3D_PROJECTION, ROOT_MENU_C3D_VIEW,
    },
};

use crate::{
    SoftwareIndexedBackend, SoftwareVideoError, SurfaceHandle,
    c3d_software::SoftwareC3dSampledTexture,
    c3d_software_raster::{
        SoftwareC3dComposeError, SoftwareC3dDepthBuffer, SoftwareC3dIndexedQuantization,
        SoftwareC3dRasterError, SoftwareC3dTransformState, SoftwareChampionshipCarIndexedSource,
        SoftwareChampionshipCarRasterProfile, compose_championship_car_indexed_copy_no_blend,
        compose_root_c3d_indexed_copy_no_blend, preflight_championship_car_indexed_copy_batch,
        preflight_root_c3d_copy_batch,
        validate_championship_car_raster_profile as validate_championship_car_profile_tuple,
    },
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SoftwareC3dBackendError {
    InvalidSequence(&'static str),
    Video(SoftwareVideoError),
    DestinationIsNotBackBuffer {
        destination: SurfaceHandle,
    },
    DestinationExtentMismatch {
        expected: DisplayExtent,
        actual: DisplayExtent,
    },
    ViewportExtentMismatch {
        viewport: DisplayExtent,
        destination: DisplayExtent,
    },
    MissingDepthBuffer,
    MissingActivePalette,
    PaletteNotAttached,
    UnsupportedTextureColorKey(u8),
    ChampionshipCarProfileNotConfigured,
    ChampionshipCarFogStateMismatch {
        profile_enabled: bool,
        prepared_enabled: bool,
    },
    ChampionshipCarActorRgbStateMismatch {
        profile_enabled: bool,
        prepared_enabled: bool,
    },
    Raster(SoftwareC3dRasterError),
    Compose(SoftwareC3dComposeError),
}

impl fmt::Display for SoftwareC3dBackendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "software C3D backend failed: {self:?}")
    }
}

impl std::error::Error for SoftwareC3dBackendError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Video(source) => Some(source),
            Self::Raster(source) => Some(source),
            Self::Compose(source) => Some(source),
            _ => None,
        }
    }
}

#[derive(Default)]
pub(crate) struct SoftwareC3dBackendState {
    depth_destination: Option<SurfaceHandle>,
    depth: Option<SoftwareC3dDepthBuffer>,
    view: Option<C3dMatrixWords>,
    viewport: Option<C3dViewportWords>,
    world: Option<C3dMatrixWords>,
    scene_destination: Option<SurfaceHandle>,
    binding: Option<SoftwareC3dBinding>,
    championship_car_profile: Option<SoftwareChampionshipCarRasterProfile>,
}

#[derive(Clone)]
enum SoftwareC3dBinding {
    Resource(SoftwareC3dSampledTexture),
    ChampionshipDefault,
}

impl SoftwareC3dBackendState {
    pub(crate) fn configure_back_buffer(
        &mut self,
        destination: SurfaceHandle,
        extent: DisplayExtent,
    ) {
        self.depth_destination = Some(destination);
        self.depth = SoftwareC3dDepthBuffer::new_far([extent.width, extent.height]).ok();
        self.view = None;
        self.viewport = None;
        self.world = None;
        self.scene_destination = None;
        self.binding = None;
        self.championship_car_profile = None;
    }

    fn require_inactive(&self, operation: &'static str) -> Result<(), SoftwareC3dBackendError> {
        if self.scene_destination.is_some() {
            Err(SoftwareC3dBackendError::InvalidSequence(operation))
        } else {
            Ok(())
        }
    }

    fn require_active(
        &self,
        operation: &'static str,
    ) -> Result<SurfaceHandle, SoftwareC3dBackendError> {
        self.scene_destination
            .ok_or(SoftwareC3dBackendError::InvalidSequence(operation))
    }

    pub(crate) const fn scene_is_active(&self) -> bool {
        self.scene_destination.is_some()
    }
}

impl SoftwareIndexedBackend {
    /// Source-owned depth values used by the software C3D compatibility path.
    #[must_use]
    pub fn c3d_depth_values(&self) -> Option<&[f32]> {
        self.c3d.depth.as_ref().map(SoftwareC3dDepthBuffer::values)
    }

    /// Selects the championship raster contract from caller-owned runtime
    /// state. Frontend entry does not itself prove whether legacy fog mutation
    /// remains enabled, so the backend deliberately has no implicit profile.
    pub fn configure_championship_car_raster_profile(
        &mut self,
        profile: SoftwareChampionshipCarRasterProfile,
    ) -> Result<(), SoftwareC3dBackendError> {
        self.c3d
            .require_inactive("configure_championship_car_raster_profile")?;
        validate_championship_car_profile_tuple(profile)
            .map_err(SoftwareC3dBackendError::Raster)?;
        self.c3d.championship_car_profile = Some(profile);
        Ok(())
    }

    fn checked_c3d_destination(
        &self,
        destination: SurfaceHandle,
    ) -> Result<DisplayExtent, SoftwareC3dBackendError> {
        if self.back != Some(destination) || self.c3d.depth_destination != Some(destination) {
            return Err(SoftwareC3dBackendError::DestinationIsNotBackBuffer { destination });
        }
        let surface = self
            .surface(destination)
            .map_err(SoftwareC3dBackendError::Video)?;
        Ok(DisplayExtent {
            width: surface.width as u16,
            height: surface.height as u16,
        })
    }

    fn require_c3d_palette(
        &self,
        destination: SurfaceHandle,
    ) -> Result<[cmr_formats::riff_palette::PaletteEntry; 256], SoftwareC3dBackendError> {
        if !self.palette_uploaded {
            return Err(SoftwareC3dBackendError::MissingActivePalette);
        }
        let surface = self
            .surface(destination)
            .map_err(SoftwareC3dBackendError::Video)?;
        if surface.palette.is_none() {
            return Err(SoftwareC3dBackendError::PaletteNotAttached);
        }
        self.palette
            .ok_or(SoftwareC3dBackendError::MissingActivePalette)
    }

    /// Effect-free guard for terminal owners to call before depth clear or
    /// `BeginScene`.
    pub fn validate_championship_car_raster_profile(
        &self,
    ) -> Result<SoftwareChampionshipCarRasterProfile, SoftwareC3dBackendError> {
        let profile = self
            .c3d
            .championship_car_profile
            .ok_or(SoftwareC3dBackendError::ChampionshipCarProfileNotConfigured)?;
        validate_championship_car_profile_tuple(profile)
            .map_err(SoftwareC3dBackendError::Raster)?;
        Ok(profile)
    }
}

impl RootMenuC3dSceneBackend for SoftwareIndexedBackend {
    type SceneError = SoftwareC3dBackendError;

    fn clear_root_c3d_depth(
        &mut self,
        destination: Self::Surface,
        extent: DisplayExtent,
    ) -> Result<(), Self::SceneError> {
        self.c3d.require_inactive("clear_root_c3d_depth")?;
        let actual = self.checked_c3d_destination(destination)?;
        if actual != extent {
            return Err(SoftwareC3dBackendError::DestinationExtentMismatch {
                expected: extent,
                actual,
            });
        }
        let depth = self
            .c3d
            .depth
            .as_mut()
            .ok_or(SoftwareC3dBackendError::MissingDepthBuffer)?;
        if depth.extent() != [extent.width, extent.height] {
            return Err(SoftwareC3dBackendError::MissingDepthBuffer);
        }
        depth.clear_far();
        Ok(())
    }

    fn set_root_c3d_view(&mut self, view: C3dMatrixWords) -> Result<(), Self::SceneError> {
        self.c3d.require_inactive("set_root_c3d_view")?;
        self.c3d.view = Some(view);
        Ok(())
    }

    fn set_root_c3d_viewport(
        &mut self,
        viewport: C3dViewportWords,
    ) -> Result<(), Self::SceneError> {
        self.c3d.require_inactive("set_root_c3d_viewport")?;
        self.c3d.viewport = Some(viewport);
        Ok(())
    }

    fn begin_root_c3d_scene(&mut self, destination: Self::Surface) -> Result<(), Self::SceneError> {
        self.c3d.require_inactive("begin_root_c3d_scene")?;
        let destination_extent = self.checked_c3d_destination(destination)?;
        self.require_c3d_palette(destination)?;
        if self.c3d.view.is_none() {
            return Err(SoftwareC3dBackendError::InvalidSequence(
                "begin_root_c3d_scene without view",
            ));
        }
        let viewport = self
            .c3d
            .viewport
            .ok_or(SoftwareC3dBackendError::InvalidSequence(
                "begin_root_c3d_scene without viewport",
            ))?;
        let words = viewport.words();
        let viewport_extent = DisplayExtent {
            width: u16::try_from(words[3]).map_err(|_| {
                SoftwareC3dBackendError::ViewportExtentMismatch {
                    viewport: DisplayExtent {
                        width: 0,
                        height: 0,
                    },
                    destination: destination_extent,
                }
            })?,
            height: u16::try_from(words[4]).map_err(|_| {
                SoftwareC3dBackendError::ViewportExtentMismatch {
                    viewport: DisplayExtent {
                        width: 0,
                        height: 0,
                    },
                    destination: destination_extent,
                }
            })?,
        };
        if viewport_extent != destination_extent {
            return Err(SoftwareC3dBackendError::ViewportExtentMismatch {
                viewport: viewport_extent,
                destination: destination_extent,
            });
        }
        let depth = self
            .c3d
            .depth
            .as_ref()
            .ok_or(SoftwareC3dBackendError::MissingDepthBuffer)?;
        if depth.extent() != [destination_extent.width, destination_extent.height] {
            return Err(SoftwareC3dBackendError::MissingDepthBuffer);
        }
        self.c3d.world = None;
        self.c3d.scene_destination = Some(destination);
        Ok(())
    }

    fn reset_root_c3d_world(&mut self) -> Result<(), Self::SceneError> {
        self.c3d.require_active("reset_root_c3d_world")?;
        self.c3d.world = Some(ROOT_MENU_C3D_VIEW);
        Ok(())
    }

    fn finish_root_c3d_scene_tail(&mut self) -> Result<RootMenuC3dBindingState, Self::SceneError> {
        self.c3d.require_active("finish_root_c3d_scene_tail")?;
        // The native tail remains opaque. The software oracle intentionally
        // performs no work and therefore preserves its retained texture.
        Ok(RootMenuC3dBindingState::Preserved)
    }

    fn end_root_c3d_scene(&mut self) -> Result<(), Self::SceneError> {
        self.c3d.require_active("end_root_c3d_scene")?;
        self.c3d.scene_destination = None;
        self.c3d.world = None;
        Ok(())
    }
}

impl FrontendC3dSceneBackend for SoftwareIndexedBackend {
    type Surface = SurfaceHandle;
    type SceneError = SoftwareC3dBackendError;

    fn clear_c3d_depth(
        &mut self,
        destination: Self::Surface,
        extent: DisplayExtent,
    ) -> Result<(), Self::SceneError> {
        self.clear_root_c3d_depth(destination, extent)
    }

    fn set_c3d_view(&mut self, view: C3dMatrixWords) -> Result<(), Self::SceneError> {
        self.set_root_c3d_view(view)
    }

    fn set_c3d_viewport(&mut self, viewport: C3dViewportWords) -> Result<(), Self::SceneError> {
        self.set_root_c3d_viewport(viewport)
    }

    fn begin_c3d_scene(&mut self, destination: Self::Surface) -> Result<(), Self::SceneError> {
        self.begin_root_c3d_scene(destination)
    }

    fn reset_c3d_world(&mut self) -> Result<(), Self::SceneError> {
        self.reset_root_c3d_world()
    }

    fn finish_c3d_scene_tail(&mut self) -> Result<C3dBindingState, Self::SceneError> {
        self.finish_root_c3d_scene_tail()
    }

    fn end_c3d_scene(&mut self) -> Result<(), Self::SceneError> {
        self.end_root_c3d_scene()
    }
}

impl RootMenuC3dBatchBackend<SoftwareC3dSampledTexture> for SoftwareIndexedBackend {
    type Error = SoftwareC3dBackendError;

    fn set_root_c3d_world(&mut self, world: C3dMatrixWords) -> Result<(), Self::Error> {
        self.c3d.require_active("set_root_c3d_world")?;
        self.c3d.world = Some(world);
        Ok(())
    }

    fn bind_root_c3d_resource(
        &mut self,
        binding: &SoftwareC3dSampledTexture,
    ) -> Result<(), Self::Error> {
        self.c3d.require_active("bind_root_c3d_resource")?;
        if let Some(index) = binding.color_key_index() {
            return Err(SoftwareC3dBackendError::UnsupportedTextureColorKey(index));
        }
        self.c3d.binding = Some(SoftwareC3dBinding::Resource(binding.clone()));
        Ok(())
    }

    fn draw_root_c3d_batch(&mut self, command: &RootMenuC3dDrawCommand) -> Result<(), Self::Error> {
        let destination = self.c3d.require_active("draw_root_c3d_batch")?;
        let world = self
            .c3d
            .world
            .ok_or(SoftwareC3dBackendError::InvalidSequence(
                "draw_root_c3d_batch without world",
            ))?;
        let view = self
            .c3d
            .view
            .ok_or(SoftwareC3dBackendError::InvalidSequence(
                "draw_root_c3d_batch without view",
            ))?;
        let viewport = self
            .c3d
            .viewport
            .ok_or(SoftwareC3dBackendError::InvalidSequence(
                "draw_root_c3d_batch without viewport",
            ))?;
        let texture = match self.c3d.binding.clone() {
            Some(SoftwareC3dBinding::Resource(texture)) => texture,
            _ => {
                return Err(SoftwareC3dBackendError::InvalidSequence(
                    "draw_root_c3d_batch without resource binding",
                ));
            }
        };
        let extent = self.checked_c3d_destination(destination)?;
        let palette = self.require_c3d_palette(destination)?;
        let prepared = preflight_root_c3d_copy_batch(
            command,
            SoftwareC3dTransformState::new(world, view, ROOT_MENU_C3D_PROJECTION, viewport),
        )
        .map_err(SoftwareC3dBackendError::Raster)?;

        let depth = self
            .c3d
            .depth
            .as_mut()
            .ok_or(SoftwareC3dBackendError::MissingDepthBuffer)?;
        let target = self
            .surfaces
            .get_mut(destination.0)
            .and_then(Option::as_mut)
            .ok_or(SoftwareC3dBackendError::Video(
                SoftwareVideoError::InvalidSurface(destination),
            ))?;
        compose_root_c3d_indexed_copy_no_blend(
            &prepared,
            &texture,
            depth,
            [extent.width, extent.height],
            &mut target.pixels,
            &palette,
            SoftwareC3dIndexedQuantization::NearestRgbL1LowestIndexNoDither,
        )
        .map_err(SoftwareC3dBackendError::Compose)?;
        Ok(())
    }
}

impl ChampionshipCarBatchBackend<SoftwareC3dSampledTexture> for SoftwareIndexedBackend {
    type Error = SoftwareC3dBackendError;

    fn preflight_championship_car_scene(
        &mut self,
        pre_draw_profile: ChampionshipCarPreDrawProfile,
    ) -> Result<(), Self::Error> {
        let profile = self.validate_championship_car_raster_profile()?;
        if profile.fog_mutation != pre_draw_profile.type2_fog_enabled {
            return Err(SoftwareC3dBackendError::ChampionshipCarFogStateMismatch {
                profile_enabled: profile.fog_mutation,
                prepared_enabled: pre_draw_profile.type2_fog_enabled,
            });
        }
        if profile.actor_rgb_override != pre_draw_profile.actor_rgb_enabled {
            return Err(
                SoftwareC3dBackendError::ChampionshipCarActorRgbStateMismatch {
                    profile_enabled: profile.actor_rgb_override,
                    prepared_enabled: pre_draw_profile.actor_rgb_enabled,
                },
            );
        }
        Ok(())
    }

    fn set_championship_car_actor_world(
        &mut self,
        _actor_index: usize,
        world: C3dMatrixWords,
    ) -> Result<(), Self::Error> {
        self.c3d
            .require_active("set_championship_car_actor_world")?;
        self.c3d.world = Some(world);
        Ok(())
    }

    fn bind_championship_car_resource(
        &mut self,
        binding: &SoftwareC3dSampledTexture,
    ) -> Result<(), Self::Error> {
        self.c3d.require_active("bind_championship_car_resource")?;
        if let Some(index) = binding.color_key_index() {
            return Err(SoftwareC3dBackendError::UnsupportedTextureColorKey(index));
        }
        self.c3d.binding = Some(SoftwareC3dBinding::Resource(binding.clone()));
        Ok(())
    }

    fn bind_championship_car_default(&mut self) -> Result<(), Self::Error> {
        self.c3d.require_active("bind_championship_car_default")?;
        self.c3d.binding = Some(SoftwareC3dBinding::ChampionshipDefault);
        Ok(())
    }

    fn draw_championship_car_batch(
        &mut self,
        command: &C3dDrawCommand,
        vertices: &[C3dType2PreDrawVertex],
    ) -> Result<(), Self::Error> {
        let destination = self.c3d.require_active("draw_championship_car_batch")?;
        let profile = self.validate_championship_car_raster_profile()?;
        let world = self
            .c3d
            .world
            .ok_or(SoftwareC3dBackendError::InvalidSequence(
                "draw_championship_car_batch without actor world",
            ))?;
        let view = self
            .c3d
            .view
            .ok_or(SoftwareC3dBackendError::InvalidSequence(
                "draw_championship_car_batch without view",
            ))?;
        let viewport = self
            .c3d
            .viewport
            .ok_or(SoftwareC3dBackendError::InvalidSequence(
                "draw_championship_car_batch without viewport",
            ))?;
        let binding = self
            .c3d
            .binding
            .clone()
            .ok_or(SoftwareC3dBackendError::InvalidSequence(
                "draw_championship_car_batch without binding",
            ))?;
        let extent = self.checked_c3d_destination(destination)?;
        let prepared = preflight_championship_car_indexed_copy_batch(
            command,
            vertices,
            SoftwareC3dTransformState::new(world, view, ROOT_MENU_C3D_PROJECTION, viewport),
            profile,
        )
        .map_err(SoftwareC3dBackendError::Raster)?;

        let depth = self
            .c3d
            .depth
            .as_mut()
            .ok_or(SoftwareC3dBackendError::MissingDepthBuffer)?;
        let target = self
            .surfaces
            .get_mut(destination.0)
            .and_then(Option::as_mut)
            .ok_or(SoftwareC3dBackendError::Video(
                SoftwareVideoError::InvalidSurface(destination),
            ))?;
        let source = match &binding {
            SoftwareC3dBinding::Resource(texture) => {
                SoftwareChampionshipCarIndexedSource::UnifiedTexture(texture)
            }
            SoftwareC3dBinding::ChampionshipDefault => {
                SoftwareChampionshipCarIndexedSource::ReviewedCarPaletteMissingSelectorOpaqueBlackIndexZero
            }
        };
        compose_championship_car_indexed_copy_no_blend(
            &prepared,
            source,
            depth,
            [extent.width, extent.height],
            &mut target.pixels,
            profile,
        )
        .map_err(SoftwareC3dBackendError::Compose)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use cmr_core::{
        championship_car_execute::ChampionshipCarBatchBackend,
        display::{CooperativeMode, IndexedMode, IndexedVideoBackend, SurfaceTopology},
        frontend_c3d_scene::{C3dDepthClear, execute_frontend_c3d_scene},
        root_menu_c3d_scene::{RootMenuC3dDepthClear, execute_root_menu_c3d_scene},
        root_menu_c3d_transform::{ROOT_MENU_C3D_VIEW, root_menu_c3d_viewport},
    };
    use cmr_formats::riff_palette::PaletteEntry;

    use super::*;

    const EXTENT: DisplayExtent = DisplayExtent {
        width: 16,
        height: 12,
    };

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct InjectedSubmitFailure;

    impl fmt::Display for InjectedSubmitFailure {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("injected submit failure")
        }
    }

    impl std::error::Error for InjectedSubmitFailure {}

    fn initialized_backend() -> (
        SoftwareIndexedBackend,
        cmr_core::display::DisplaySurfaces<SurfaceHandle>,
    ) {
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
        let entries = [PaletteEntry {
            red: 0,
            green: 0,
            blue: 0,
            flags: 0,
        }; 256];
        let palette = backend.create_palette(&entries).unwrap();
        backend
            .attach_palette(surfaces.visible_front, palette)
            .unwrap();
        backend.attach_palette(surfaces.back, palette).unwrap();
        backend.upload_palette(palette, &entries).unwrap();
        (backend, surfaces)
    }

    #[test]
    fn empty_scene_obeys_core_order_without_touching_color() {
        let (mut backend, surfaces) = initialized_backend();
        let before_front = backend.visible_pixels().unwrap().to_vec();
        let before_back = backend.back_pixels().unwrap().to_vec();

        let outcome = execute_root_menu_c3d_scene(
            &mut backend,
            surfaces.back,
            EXTENT,
            RootMenuC3dDepthClear::Clear,
            None::<()>,
            |_, ()| Ok::<(), Infallible>(()),
        )
        .unwrap();

        assert!(outcome.depth_cleared);
        assert_eq!(outcome.submission, None);
        assert_eq!(outcome.binding_state, RootMenuC3dBindingState::Preserved);
        assert_eq!(backend.visible_pixels().unwrap(), before_front);
        assert_eq!(backend.back_pixels().unwrap(), before_back);
        assert!(
            backend
                .c3d_depth_values()
                .unwrap()
                .iter()
                .all(|value| *value == 1.0)
        );

        execute_root_menu_c3d_scene(
            &mut backend,
            surfaces.back,
            EXTENT,
            RootMenuC3dDepthClear::Skip,
            None::<()>,
            |_, ()| Ok::<(), Infallible>(()),
        )
        .unwrap();
    }

    #[test]
    fn invalid_clear_and_begin_boundaries_are_effect_free_and_recoverable() {
        let (mut backend, surfaces) = initialized_backend();
        let before_color = backend.back_pixels().unwrap().to_vec();
        let before_depth = backend.c3d_depth_values().unwrap().to_vec();

        assert!(matches!(
            backend.clear_root_c3d_depth(surfaces.visible_front, EXTENT),
            Err(SoftwareC3dBackendError::DestinationIsNotBackBuffer { .. })
        ));
        assert!(matches!(
            backend.clear_root_c3d_depth(
                surfaces.back,
                DisplayExtent {
                    width: EXTENT.width - 1,
                    height: EXTENT.height,
                }
            ),
            Err(SoftwareC3dBackendError::DestinationExtentMismatch { .. })
        ));
        assert_eq!(backend.back_pixels().unwrap(), before_color);
        assert_eq!(backend.c3d_depth_values().unwrap(), before_depth);

        assert_eq!(
            backend.begin_root_c3d_scene(surfaces.back),
            Err(SoftwareC3dBackendError::InvalidSequence(
                "begin_root_c3d_scene without view"
            ))
        );
        assert_eq!(backend.back_pixels().unwrap(), before_color);
        assert_eq!(backend.c3d_depth_values().unwrap(), before_depth);
        backend.set_root_c3d_view(ROOT_MENU_C3D_VIEW).unwrap();
        backend
            .set_root_c3d_viewport(root_menu_c3d_viewport(EXTENT).unwrap())
            .unwrap();
        backend.begin_root_c3d_scene(surfaces.back).unwrap();
        assert_eq!(
            backend.begin_root_c3d_scene(surfaces.back),
            Err(SoftwareC3dBackendError::InvalidSequence(
                "begin_root_c3d_scene"
            ))
        );
        backend.reset_root_c3d_world().unwrap();
        assert_eq!(
            backend.finish_root_c3d_scene_tail().unwrap(),
            RootMenuC3dBindingState::Preserved
        );
        backend.end_root_c3d_scene().unwrap();

        execute_root_menu_c3d_scene(
            &mut backend,
            surfaces.back,
            EXTENT,
            RootMenuC3dDepthClear::Clear,
            None::<()>,
            |_, ()| Ok::<(), Infallible>(()),
        )
        .unwrap();
    }

    #[test]
    fn post_begin_submission_failure_ends_scene_and_backend_is_reusable() {
        let (mut backend, surfaces) = initialized_backend();
        let before_color = backend.back_pixels().unwrap().to_vec();
        let before_depth = backend.c3d_depth_values().unwrap().to_vec();

        let error = execute_root_menu_c3d_scene(
            &mut backend,
            surfaces.back,
            EXTENT,
            RootMenuC3dDepthClear::Skip,
            Some(()),
            |backend, ()| {
                backend.set_root_c3d_world(ROOT_MENU_C3D_VIEW).unwrap();
                Err::<(), _>(InjectedSubmitFailure)
            },
        )
        .unwrap_err();
        assert!(matches!(
            error.primary,
            cmr_core::root_menu_c3d_scene::RootMenuC3dScenePrimaryError::Submit(
                InjectedSubmitFailure
            )
        ));
        assert_eq!(error.cleanup, None);
        assert_eq!(backend.back_pixels().unwrap(), before_color);
        assert_eq!(backend.c3d_depth_values().unwrap(), before_depth);
        assert_eq!(
            backend.end_root_c3d_scene(),
            Err(SoftwareC3dBackendError::InvalidSequence(
                "end_root_c3d_scene"
            ))
        );

        execute_root_menu_c3d_scene(
            &mut backend,
            surfaces.back,
            EXTENT,
            RootMenuC3dDepthClear::Clear,
            None::<()>,
            |_, ()| Ok::<(), Infallible>(()),
        )
        .unwrap();
    }

    #[test]
    fn championship_default_binding_is_distinct_and_scene_scoped() {
        let (mut backend, surfaces) = initialized_backend();
        let initial_color = backend.back_pixels().unwrap().to_vec();
        let initial_depth = backend.c3d_depth_values().unwrap().to_vec();
        assert_eq!(
            backend.validate_championship_car_raster_profile(),
            Err(SoftwareC3dBackendError::ChampionshipCarProfileNotConfigured)
        );
        let supported = SoftwareChampionshipCarRasterProfile::MUTATION_DISABLED_ACTOR_RGB_DISABLED_UNIFIED_INDEXED_COPY_NO_BLEND;
        let unsupported = SoftwareChampionshipCarRasterProfile {
            alpha_blend: true,
            ..supported
        };
        assert!(matches!(
            backend.configure_championship_car_raster_profile(unsupported),
            Err(SoftwareC3dBackendError::Raster(
                SoftwareC3dRasterError::UnsupportedChampionshipCarProfile(profile)
            )) if profile == unsupported
        ));
        assert_eq!(
            backend.validate_championship_car_raster_profile(),
            Err(SoftwareC3dBackendError::ChampionshipCarProfileNotConfigured)
        );
        assert!(!backend.c3d.scene_is_active());
        assert_eq!(backend.back_pixels().unwrap(), initial_color);
        assert_eq!(backend.c3d_depth_values().unwrap(), initial_depth);
        backend
            .configure_championship_car_raster_profile(supported)
            .unwrap();
        assert_eq!(
            backend.validate_championship_car_raster_profile(),
            Ok(supported)
        );
        let enabled_state = ChampionshipCarPreDrawProfile {
            type2_fog_enabled: true,
            actor_rgb_enabled: false,
        };
        assert_eq!(
            ChampionshipCarBatchBackend::<SoftwareC3dSampledTexture>::preflight_championship_car_scene(
                &mut backend,
                enabled_state,
            ),
            Err(SoftwareC3dBackendError::ChampionshipCarFogStateMismatch {
                profile_enabled: false,
                prepared_enabled: true,
            })
        );
        let before_color = backend.back_pixels().unwrap().to_vec();
        let before_depth = backend.c3d_depth_values().unwrap().to_vec();

        execute_frontend_c3d_scene(
            &mut backend,
            surfaces.back,
            EXTENT,
            C3dDepthClear::Skip,
            Some(()),
            |backend, ()| {
                assert_eq!(
                    backend.configure_championship_car_raster_profile(
                        unsupported,
                    ),
                    Err(SoftwareC3dBackendError::InvalidSequence(
                        "configure_championship_car_raster_profile"
                    ))
                );
                ChampionshipCarBatchBackend::<SoftwareC3dSampledTexture>::bind_championship_car_default(
                    backend,
                )?;
                assert!(matches!(
                    backend.c3d.binding,
                    Some(SoftwareC3dBinding::ChampionshipDefault)
                ));
                Ok::<(), SoftwareC3dBackendError>(())
            },
        )
        .unwrap();

        assert_eq!(backend.back_pixels().unwrap(), before_color);
        assert_eq!(backend.c3d_depth_values().unwrap(), before_depth);
        assert_eq!(
            ChampionshipCarBatchBackend::<SoftwareC3dSampledTexture>::bind_championship_car_default(
                &mut backend,
            ),
            Err(SoftwareC3dBackendError::InvalidSequence(
                "bind_championship_car_default"
            ))
        );
    }

    #[test]
    fn championship_copy_profiles_accept_matching_pre_draw_state() {
        let (mut backend, _) = initialized_backend();
        for (profile, state) in [
            (
                SoftwareChampionshipCarRasterProfile::MUTATION_DISABLED_ACTOR_RGB_DISABLED_UNIFIED_INDEXED_COPY_NO_BLEND,
                ChampionshipCarPreDrawProfile {
                    type2_fog_enabled: false,
                    actor_rgb_enabled: false,
                },
            ),
            (
                SoftwareChampionshipCarRasterProfile::MUTATION_ENABLED_ACTOR_RGB_DISABLED_UNIFIED_INDEXED_COPY_NO_BLEND,
                ChampionshipCarPreDrawProfile {
                    type2_fog_enabled: true,
                    actor_rgb_enabled: false,
                },
            ),
            (
                SoftwareChampionshipCarRasterProfile::MUTATION_DISABLED_ACTOR_RGB_ENABLED_UNIFIED_INDEXED_COPY_NO_BLEND,
                ChampionshipCarPreDrawProfile {
                    type2_fog_enabled: false,
                    actor_rgb_enabled: true,
                },
            ),
            (
                SoftwareChampionshipCarRasterProfile::MUTATION_ENABLED_ACTOR_RGB_ENABLED_UNIFIED_INDEXED_COPY_NO_BLEND,
                ChampionshipCarPreDrawProfile {
                    type2_fog_enabled: true,
                    actor_rgb_enabled: true,
                },
            ),
        ] {
            backend
                .configure_championship_car_raster_profile(profile)
                .unwrap();
            assert_eq!(
                ChampionshipCarBatchBackend::<SoftwareC3dSampledTexture>::preflight_championship_car_scene(
                    &mut backend,
                    state,
                ),
                Ok(())
            );
        }
    }

    #[test]
    fn actor_rgb_profile_state_mismatches_are_effect_free() {
        let (mut backend, _) = initialized_backend();
        let before_color = backend.back_pixels().unwrap().to_vec();
        let before_depth = backend.c3d_depth_values().unwrap().to_vec();

        for (profile, prepared_enabled) in [
            (
                SoftwareChampionshipCarRasterProfile::MUTATION_DISABLED_ACTOR_RGB_DISABLED_UNIFIED_INDEXED_COPY_NO_BLEND,
                true,
            ),
            (
                SoftwareChampionshipCarRasterProfile::MUTATION_DISABLED_ACTOR_RGB_ENABLED_UNIFIED_INDEXED_COPY_NO_BLEND,
                false,
            ),
        ] {
            backend
                .configure_championship_car_raster_profile(profile)
                .unwrap();
            assert_eq!(
                ChampionshipCarBatchBackend::<SoftwareC3dSampledTexture>::preflight_championship_car_scene(
                    &mut backend,
                    ChampionshipCarPreDrawProfile {
                        type2_fog_enabled: false,
                        actor_rgb_enabled: prepared_enabled,
                    },
                ),
                Err(
                    SoftwareC3dBackendError::ChampionshipCarActorRgbStateMismatch {
                        profile_enabled: profile.actor_rgb_override,
                        prepared_enabled,
                    }
                )
            );
            assert!(!backend.c3d.scene_is_active());
            assert_eq!(backend.back_pixels().unwrap(), before_color);
            assert_eq!(backend.c3d_depth_values().unwrap(), before_depth);
        }
    }
}
