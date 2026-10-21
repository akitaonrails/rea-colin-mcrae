//! Standalone logical-RGB backend for the bounded alternate C3D oracle.
//!
//! This backend deliberately does not implement the indexed display contract.
//! It owns one logical masked-word target and one source-owned depth buffer,
//! keeping their lifetime and nominal handle separate from indexed surfaces.
//! The target is not a native surface or presentation buffer: byte order, row
//! pitch, driver conversion, dithering, and display presentation remain open.

use std::{
    fmt,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use cmr_core::{
    blit::BlitPlan,
    c3d_batch::C3dDrawCommand,
    c3d_type2_fog::C3dType2PreDrawVertex,
    championship_car_execute::{
        ChampionshipCarBatchBackend, ChampionshipCarBatchPreflight, ChampionshipCarBindingPlan,
        ChampionshipCarPreDrawProfile, ChampionshipCarResolvedBinding,
    },
    display::DisplayExtent,
    frontend_c3d_resources::{FrontendC3dPrepared, bind_frontend_c3d_resources},
    frontend_c3d_scene::{C3dBindingState, C3dDepthClear, FrontendC3dSceneBackend},
    root_menu_c3d_execute::{C3dBindingBuildError, C3dBindingCatalog},
    root_menu_c3d_render_state::{
        C3dFilterDeviceAction, RootMenuC3dFilterCacheOutcome, RootMenuC3dLegacyAlphaContract,
        root_menu_c3d_legacy_alpha_contract,
    },
    root_menu_c3d_resources::C3dPreparedResourceId,
    root_menu_c3d_transform::{
        C3dMatrixWords, C3dViewportWords, ROOT_MENU_C3D_PROJECTION, RootMenuC3dViewportError,
        root_menu_c3d_viewport,
    },
};
use cmr_formats::riff_palette::PaletteEntry;

use crate::{
    c3d_software::{SoftwareC3dSampledTexture, SoftwareC3dSamplingError},
    c3d_software_raster::{
        SoftwareC3dComposeError, SoftwareC3dDepthBuffer, SoftwareC3dRasterError,
        SoftwareC3dTransformState, SoftwareChampionshipCarSplitOracleProfile,
        SoftwareChampionshipCarSplitSource,
        compose_championship_car_split_modulate_logical_masked_rgb_oracle,
        preflight_championship_car_split_modulate_batch,
        preflight_championship_car_split_modulate_logical_masked_rgb_oracle,
        validate_championship_car_split_modulate_no_alpha_oracle_profile,
    },
    legacy_c3d_pixel_format::SelectedNonAlphaTextureFormat,
    ordinary_bitmap_rgb16::OrdinaryBitmapLogicalRgb16Source,
    ordinary_bitmap_rgb16_blit::{
        OrdinaryBitmapRgb16BlitError, compose_ordinary_bitmap_logical_rgb16_fast_blit,
        preflight_ordinary_bitmap_logical_rgb16_fast_blit,
    },
    ordinary_bitmap_rgb16_stretch::{
        OrdinaryBitmapRgb16StretchError,
        compose_ordinary_bitmap_logical_rgb16_center_nearest_stretch,
        preflight_ordinary_bitmap_logical_rgb16_center_nearest_stretch,
    },
    packed_rgb_target::{
        FramebufferRgbFormat, LogicalMaskedRgbTarget, LogicalMaskedRgbTargetError,
    },
    selected_texture_conversion_oracle::{
        SourceOwnedLogicalSelectedTexture, SourceOwnedTextureConversionOracleError,
        SourceOwnedTextureConversionOraclePolicy, convert_selected_texture_source_owned_oracle,
    },
};

static NEXT_SPLIT_TARGET_OWNER: AtomicU64 = AtomicU64::new(1);

fn allocate_split_target_owner() -> Result<u64, SoftwareSplitC3dBackendError> {
    let mut current = NEXT_SPLIT_TARGET_OWNER.load(Ordering::Relaxed);
    loop {
        let next = current
            .checked_add(1)
            .ok_or(SoftwareSplitC3dBackendError::TargetIdentityExhausted)?;
        match NEXT_SPLIT_TARGET_OWNER.compare_exchange_weak(
            current,
            next,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return Ok(current),
            Err(actual) => current = actual,
        }
    }
}

/// Owner-scoped nominal handle for one backend's logical target.
///
/// Its private field prevents callers from manufacturing a target or confusing
/// it with an indexed [`crate::SurfaceHandle`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SoftwareSplitC3dTargetHandle(u64);

/// Cheaply retained binding for one already-converted selected texture.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SoftwareSplitC3dBinding(Arc<SourceOwnedLogicalSelectedTexture>);

impl SoftwareSplitC3dBinding {
    #[must_use]
    pub fn texture(&self) -> &SourceOwnedLogicalSelectedTexture {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SoftwareSplitC3dBindingBuildError {
    Sampling(SoftwareC3dSamplingError),
    UnsupportedTextureColorKey(u8),
    Conversion(SourceOwnedTextureConversionOracleError),
}

impl fmt::Display for SoftwareSplitC3dBindingBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "cannot build split C3D oracle binding: {self:?}")
    }
}

impl std::error::Error for SoftwareSplitC3dBindingBuildError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sampling(source) => Some(source),
            Self::Conversion(source) => Some(source),
            Self::UnsupportedTextureColorKey(_) => None,
        }
    }
}

/// Converts every frontend-arena source before a scene can perform effects.
///
/// The single selected format applies to the whole catalog. Construction is
/// transactional through [`bind_frontend_c3d_resources`]: a failed resource
/// drops every earlier binding and returns no partial catalog.
pub fn prepare_software_split_frontend_c3d_bindings(
    prepared: &FrontendC3dPrepared,
    selected_format: SelectedNonAlphaTextureFormat,
    conversion_policy: SourceOwnedTextureConversionOraclePolicy,
) -> Result<
    C3dBindingCatalog<SoftwareSplitC3dBinding>,
    C3dBindingBuildError<SoftwareSplitC3dBindingBuildError>,
> {
    bind_frontend_c3d_resources(prepared, |_, resource| {
        let sampled = SoftwareC3dSampledTexture::from_prepared_frontend(resource)
            .map_err(SoftwareSplitC3dBindingBuildError::Sampling)?;
        if let Some(index) = sampled.color_key_index() {
            return Err(SoftwareSplitC3dBindingBuildError::UnsupportedTextureColorKey(index));
        }
        convert_selected_texture_source_owned_oracle(&sampled, selected_format, conversion_policy)
            .map(|texture| SoftwareSplitC3dBinding(Arc::new(texture)))
            .map_err(SoftwareSplitC3dBindingBuildError::Conversion)
    })
}

/// Evidence-bounded configuration retained by the split oracle backend.
///
/// The alpha and filter fields retain recovered legacy state. `profile` also
/// names the bounded software-oracle policy used where native output remains
/// unresolved; it is not wholly a recovered driver configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SoftwareSplitC3dOracleConfiguration {
    pub profile: SoftwareChampionshipCarSplitOracleProfile,
    pub alpha_contract: RootMenuC3dLegacyAlphaContract,
    pub filter_cache_outcome: RootMenuC3dFilterCacheOutcome,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SoftwareSplitC3dBackendError {
    InvalidSequence(&'static str),
    TargetIdentityExhausted,
    ForeignTarget {
        expected: SoftwareSplitC3dTargetHandle,
        actual: SoftwareSplitC3dTargetHandle,
    },
    RetainedBindingMismatch {
        expected: SoftwareSplitC3dBindingExpectation,
        retained: SoftwareSplitC3dRetainedBindingKind,
    },
    UnsupportedAlphaContract(RootMenuC3dLegacyAlphaContract),
    FilterRequestMismatch {
        expected: u32,
        actual: RootMenuC3dFilterCacheOutcome,
    },
    DestinationExtentMismatch {
        expected: DisplayExtent,
        actual: DisplayExtent,
    },
    ViewportExtentMismatch {
        viewport: DisplayExtent,
        destination: DisplayExtent,
    },
    DepthClearRequired,
    ChampionshipCarFogStateMismatch {
        profile_enabled: bool,
        prepared_enabled: bool,
    },
    ChampionshipCarActorRgbStateMismatch {
        profile_enabled: bool,
        prepared_enabled: bool,
    },
    OrdinaryBitmapBlit(OrdinaryBitmapRgb16BlitError),
    OrdinaryBitmapStretch(OrdinaryBitmapRgb16StretchError),
    Target(LogicalMaskedRgbTargetError),
    Viewport(RootMenuC3dViewportError),
    Raster(SoftwareC3dRasterError),
    Compose(SoftwareC3dComposeError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SoftwareSplitC3dBindingExpectation {
    Resource(C3dPreparedResourceId),
    Missing,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SoftwareSplitC3dRetainedBindingKind {
    Resource,
    Missing,
    None,
}

impl fmt::Display for SoftwareSplitC3dBackendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "software split C3D oracle backend failed: {self:?}"
        )
    }
}

impl std::error::Error for SoftwareSplitC3dBackendError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Target(source) => Some(source),
            Self::OrdinaryBitmapBlit(source) => Some(source),
            Self::OrdinaryBitmapStretch(source) => Some(source),
            Self::Viewport(source) => Some(source),
            Self::Raster(source) => Some(source),
            Self::Compose(source) => Some(source),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
enum SoftwareSplitC3dBoundSource {
    Converted(SoftwareSplitC3dBinding),
    BlackDiffuseDefault,
}

/// Headless owner of one alternate C3D logical target and its depth buffer.
#[derive(Debug)]
pub struct SoftwareSplitC3dOracleBackend {
    target_handle: SoftwareSplitC3dTargetHandle,
    extent: DisplayExtent,
    target: LogicalMaskedRgbTarget,
    depth: SoftwareC3dDepthBuffer,
    configuration: SoftwareSplitC3dOracleConfiguration,
    preflight_viewport: C3dViewportWords,
    view: Option<C3dMatrixWords>,
    viewport: Option<C3dViewportWords>,
    world: Option<C3dMatrixWords>,
    scene_active: bool,
    depth_cleared_for_next_scene: bool,
    binding: Option<SoftwareSplitC3dBoundSource>,
}

impl SoftwareSplitC3dOracleBackend {
    pub fn new(
        extent: DisplayExtent,
        framebuffer_format: FramebufferRgbFormat,
        configuration: SoftwareSplitC3dOracleConfiguration,
    ) -> Result<Self, SoftwareSplitC3dBackendError> {
        validate_championship_car_split_modulate_no_alpha_oracle_profile(configuration.profile)
            .map_err(SoftwareSplitC3dBackendError::Raster)?;
        if configuration.alpha_contract != root_menu_c3d_legacy_alpha_contract(0) {
            return Err(SoftwareSplitC3dBackendError::UnsupportedAlphaContract(
                configuration.alpha_contract,
            ));
        }
        let expected_filter = configuration.profile.requested_filter as u32;
        if configuration.filter_cache_outcome.requested_filter != expected_filter
            || configuration
                .filter_cache_outcome
                .resulting_filter_request_cache
                != expected_filter
            || matches!(
                configuration.filter_cache_outcome.device_action,
                C3dFilterDeviceAction::Submit { submitted_value }
                    if submitted_value != 1 && submitted_value != expected_filter
            )
        {
            return Err(SoftwareSplitC3dBackendError::FilterRequestMismatch {
                expected: expected_filter,
                actual: configuration.filter_cache_outcome,
            });
        }
        let target = LogicalMaskedRgbTarget::new([extent.width, extent.height], framebuffer_format)
            .map_err(SoftwareSplitC3dBackendError::Target)?;
        let depth = SoftwareC3dDepthBuffer::new_far([extent.width, extent.height])
            .map_err(SoftwareSplitC3dBackendError::Raster)?;
        let preflight_viewport =
            root_menu_c3d_viewport(extent).map_err(SoftwareSplitC3dBackendError::Viewport)?;
        let target_owner = allocate_split_target_owner()?;
        let target_handle = SoftwareSplitC3dTargetHandle(target_owner);
        Ok(Self {
            target_handle,
            extent,
            target,
            depth,
            configuration,
            preflight_viewport,
            view: None,
            viewport: None,
            world: None,
            scene_active: false,
            depth_cleared_for_next_scene: false,
            binding: None,
        })
    }

    #[must_use]
    pub const fn target_handle(&self) -> SoftwareSplitC3dTargetHandle {
        self.target_handle
    }

    #[must_use]
    pub const fn extent(&self) -> DisplayExtent {
        self.extent
    }

    #[must_use]
    pub const fn logical_target(&self) -> &LogicalMaskedRgbTarget {
        &self.target
    }

    #[must_use]
    pub fn depth_values(&self) -> &[f32] {
        self.depth.values()
    }

    #[must_use]
    pub const fn configuration(&self) -> SoftwareSplitC3dOracleConfiguration {
        self.configuration
    }

    /// Effect-free validation for an ordinary-bitmap logical RGB16 blit into
    /// this backend's owned target.
    ///
    /// The nominal handle is owner-scoped, and composition is available only
    /// while no C3D scene is active. The underlying validator additionally
    /// requires an exact-size `Fast` plan, in-bounds rectangles, a matching
    /// RGB555/RGB565 layout, and either no key or the recovered packed key.
    pub fn preflight_ordinary_bitmap_logical_rgb16_fast_blit(
        &self,
        destination: SoftwareSplitC3dTargetHandle,
        source: &OrdinaryBitmapLogicalRgb16Source,
        plan: BlitPlan,
    ) -> Result<(), SoftwareSplitC3dBackendError> {
        self.require_inactive("preflight_ordinary_bitmap_logical_rgb16_fast_blit")?;
        self.validate_target(destination)?;
        preflight_ordinary_bitmap_logical_rgb16_fast_blit(source, &self.target, plan)
            .map_err(SoftwareSplitC3dBackendError::OrdinaryBitmapBlit)
    }

    /// Composes one validated ordinary-bitmap logical RGB16 blit into the
    /// owned target while no C3D scene is active.
    ///
    /// All owner, sequence, layout, key, and rectangle checks complete before
    /// the first target word changes.
    pub fn compose_ordinary_bitmap_logical_rgb16_fast_blit(
        &mut self,
        destination: SoftwareSplitC3dTargetHandle,
        source: &OrdinaryBitmapLogicalRgb16Source,
        plan: BlitPlan,
    ) -> Result<(), SoftwareSplitC3dBackendError> {
        self.require_inactive("compose_ordinary_bitmap_logical_rgb16_fast_blit")?;
        self.validate_target(destination)?;
        compose_ordinary_bitmap_logical_rgb16_fast_blit(source, &mut self.target, plan)
            .map_err(SoftwareSplitC3dBackendError::OrdinaryBitmapBlit)
    }

    /// Effect-free validation for the bounded Selection Stretch primitive.
    ///
    /// The application-side rectangles and numeric-zero request key are
    /// recovered. Center-nearest sampling is an explicitly source-owned
    /// deterministic policy and does not claim DirectDraw driver pixel parity.
    pub fn preflight_ordinary_bitmap_logical_rgb16_center_nearest_stretch(
        &self,
        destination: SoftwareSplitC3dTargetHandle,
        source: &OrdinaryBitmapLogicalRgb16Source,
        plan: BlitPlan,
    ) -> Result<(), SoftwareSplitC3dBackendError> {
        self.require_inactive("preflight_ordinary_bitmap_logical_rgb16_center_nearest_stretch")?;
        self.validate_target(destination)?;
        preflight_ordinary_bitmap_logical_rgb16_center_nearest_stretch(source, &self.target, plan)
            .map_err(SoftwareSplitC3dBackendError::OrdinaryBitmapStretch)
    }

    /// Composes one preflight-valid Selection Stretch into the owned target.
    pub fn compose_ordinary_bitmap_logical_rgb16_center_nearest_stretch(
        &mut self,
        destination: SoftwareSplitC3dTargetHandle,
        source: &OrdinaryBitmapLogicalRgb16Source,
        plan: BlitPlan,
    ) -> Result<(), SoftwareSplitC3dBackendError> {
        self.require_inactive("compose_ordinary_bitmap_logical_rgb16_center_nearest_stretch")?;
        self.validate_target(destination)?;
        compose_ordinary_bitmap_logical_rgb16_center_nearest_stretch(source, &mut self.target, plan)
            .map_err(SoftwareSplitC3dBackendError::OrdinaryBitmapStretch)
    }

    /// Replaces the owned logical color target while no scene is active.
    ///
    /// This is a source-owned initialization seam for later 2D composition;
    /// it does not describe a native lock, byte layout, pitch, or blit.
    pub fn replace_logical_target_from_canonical_rgb(
        &mut self,
        source_extent: [u16; 2],
        source_pixels: &[[u8; 3]],
    ) -> Result<(), SoftwareSplitC3dBackendError> {
        self.require_inactive("replace_logical_target_from_canonical_rgb")?;
        self.target
            .replace_from_canonical_rgb(source_extent, source_pixels)
            .map_err(SoftwareSplitC3dBackendError::Target)
    }

    /// Replaces the owned logical target from a completed indexed frame.
    ///
    /// `palette` is the caller-selected effective presentation palette. The
    /// bridge does not infer legacy palette timing or replay individual 2D
    /// operations, so their established clipping, keying, and order remain in
    /// the completed indexed frame. Conversion is allowed only outside a C3D
    /// scene and is atomic on validation failure.
    pub fn replace_logical_target_from_indexed_palette(
        &mut self,
        source_extent: [u16; 2],
        source_indices: &[u8],
        palette: &[PaletteEntry; 256],
    ) -> Result<(), SoftwareSplitC3dBackendError> {
        self.require_inactive("replace_logical_target_from_indexed_palette")?;
        self.target
            .replace_from_indexed_palette(source_extent, source_indices, palette)
            .map_err(SoftwareSplitC3dBackendError::Target)
    }

    fn require_inactive(
        &self,
        operation: &'static str,
    ) -> Result<(), SoftwareSplitC3dBackendError> {
        if self.scene_active {
            Err(SoftwareSplitC3dBackendError::InvalidSequence(operation))
        } else {
            Ok(())
        }
    }

    fn require_active(&self, operation: &'static str) -> Result<(), SoftwareSplitC3dBackendError> {
        if self.scene_active {
            Ok(())
        } else {
            Err(SoftwareSplitC3dBackendError::InvalidSequence(operation))
        }
    }

    fn validate_target(
        &self,
        target: SoftwareSplitC3dTargetHandle,
    ) -> Result<(), SoftwareSplitC3dBackendError> {
        if target == self.target_handle {
            Ok(())
        } else {
            Err(SoftwareSplitC3dBackendError::ForeignTarget {
                expected: self.target_handle,
                actual: target,
            })
        }
    }

    fn validate_pre_draw_profile(
        &self,
        prepared: ChampionshipCarPreDrawProfile,
    ) -> Result<(), SoftwareSplitC3dBackendError> {
        let profile = self.configuration.profile.raster;
        if profile.fog_mutation != prepared.type2_fog_enabled {
            return Err(
                SoftwareSplitC3dBackendError::ChampionshipCarFogStateMismatch {
                    profile_enabled: profile.fog_mutation,
                    prepared_enabled: prepared.type2_fog_enabled,
                },
            );
        }
        if profile.actor_rgb_override != prepared.actor_rgb_enabled {
            return Err(
                SoftwareSplitC3dBackendError::ChampionshipCarActorRgbStateMismatch {
                    profile_enabled: profile.actor_rgb_override,
                    prepared_enabled: prepared.actor_rgb_enabled,
                },
            );
        }
        Ok(())
    }

    fn retained_binding_kind(&self) -> SoftwareSplitC3dRetainedBindingKind {
        match &self.binding {
            Some(SoftwareSplitC3dBoundSource::Converted(_)) => {
                SoftwareSplitC3dRetainedBindingKind::Resource
            }
            Some(SoftwareSplitC3dBoundSource::BlackDiffuseDefault) => {
                SoftwareSplitC3dRetainedBindingKind::Missing
            }
            None => SoftwareSplitC3dRetainedBindingKind::None,
        }
    }

    fn validate_retained_binding(
        &self,
        batch: &ChampionshipCarBatchPreflight<'_, '_, SoftwareSplitC3dBinding>,
    ) -> Result<(), SoftwareSplitC3dBackendError> {
        if batch.binding_plan != ChampionshipCarBindingPlan::ReuseRetained {
            return Ok(());
        }
        let matches = match (&batch.binding, &self.binding) {
            (
                ChampionshipCarResolvedBinding::Resource { binding, .. },
                Some(SoftwareSplitC3dBoundSource::Converted(retained)),
            ) => Arc::ptr_eq(&binding.0, &retained.0),
            (
                ChampionshipCarResolvedBinding::Missing,
                Some(SoftwareSplitC3dBoundSource::BlackDiffuseDefault),
            ) => true,
            _ => false,
        };
        if matches {
            return Ok(());
        }
        let expected = match &batch.binding {
            ChampionshipCarResolvedBinding::Resource { resource_id, .. } => {
                SoftwareSplitC3dBindingExpectation::Resource(*resource_id)
            }
            ChampionshipCarResolvedBinding::Missing => SoftwareSplitC3dBindingExpectation::Missing,
        };
        Err(SoftwareSplitC3dBackendError::RetainedBindingMismatch {
            expected,
            retained: self.retained_binding_kind(),
        })
    }

    fn viewport_extent(
        viewport: C3dViewportWords,
    ) -> Result<DisplayExtent, SoftwareSplitC3dBackendError> {
        let words = viewport.words();
        let width = u16::try_from(words[3]).map_err(|_| {
            SoftwareSplitC3dBackendError::ViewportExtentMismatch {
                viewport: DisplayExtent {
                    width: 0,
                    height: 0,
                },
                destination: DisplayExtent {
                    width: 0,
                    height: 0,
                },
            }
        })?;
        let height = u16::try_from(words[4]).map_err(|_| {
            SoftwareSplitC3dBackendError::ViewportExtentMismatch {
                viewport: DisplayExtent { width, height: 0 },
                destination: DisplayExtent {
                    width: 0,
                    height: 0,
                },
            }
        })?;
        Ok(DisplayExtent { width, height })
    }
}

impl FrontendC3dSceneBackend for SoftwareSplitC3dOracleBackend {
    type Surface = SoftwareSplitC3dTargetHandle;
    type SceneError = SoftwareSplitC3dBackendError;

    fn preflight_c3d_scene(
        &mut self,
        destination: Self::Surface,
        extent: DisplayExtent,
        depth_clear: C3dDepthClear,
    ) -> Result<(), Self::SceneError> {
        self.require_inactive("preflight_c3d_scene")?;
        self.validate_target(destination)?;
        if extent != self.extent {
            return Err(SoftwareSplitC3dBackendError::DestinationExtentMismatch {
                expected: self.extent,
                actual: extent,
            });
        }
        if depth_clear != C3dDepthClear::Clear {
            return Err(SoftwareSplitC3dBackendError::DepthClearRequired);
        }
        Ok(())
    }

    fn clear_c3d_depth(
        &mut self,
        destination: Self::Surface,
        extent: DisplayExtent,
    ) -> Result<(), Self::SceneError> {
        self.require_inactive("clear_c3d_depth")?;
        self.validate_target(destination)?;
        if extent != self.extent {
            return Err(SoftwareSplitC3dBackendError::DestinationExtentMismatch {
                expected: self.extent,
                actual: extent,
            });
        }
        self.depth.clear_far();
        self.depth_cleared_for_next_scene = true;
        Ok(())
    }

    fn set_c3d_view(&mut self, view: C3dMatrixWords) -> Result<(), Self::SceneError> {
        self.require_inactive("set_c3d_view")?;
        self.view = Some(view);
        Ok(())
    }

    fn set_c3d_viewport(&mut self, viewport: C3dViewportWords) -> Result<(), Self::SceneError> {
        self.require_inactive("set_c3d_viewport")?;
        self.viewport = Some(viewport);
        Ok(())
    }

    fn begin_c3d_scene(&mut self, destination: Self::Surface) -> Result<(), Self::SceneError> {
        self.require_inactive("begin_c3d_scene")?;
        self.validate_target(destination)?;
        if !self.depth_cleared_for_next_scene {
            return Err(SoftwareSplitC3dBackendError::DepthClearRequired);
        }
        if self.view.is_none() {
            self.depth_cleared_for_next_scene = false;
            return Err(SoftwareSplitC3dBackendError::InvalidSequence(
                "begin_c3d_scene without view",
            ));
        }
        let viewport = match self.viewport {
            Some(viewport) => viewport,
            None => {
                self.depth_cleared_for_next_scene = false;
                return Err(SoftwareSplitC3dBackendError::InvalidSequence(
                    "begin_c3d_scene without viewport",
                ));
            }
        };
        let viewport_extent = Self::viewport_extent(viewport).map_err(|_| {
            self.depth_cleared_for_next_scene = false;
            SoftwareSplitC3dBackendError::ViewportExtentMismatch {
                viewport: DisplayExtent {
                    width: 0,
                    height: 0,
                },
                destination: self.extent,
            }
        })?;
        if viewport_extent != self.extent {
            self.depth_cleared_for_next_scene = false;
            return Err(SoftwareSplitC3dBackendError::ViewportExtentMismatch {
                viewport: viewport_extent,
                destination: self.extent,
            });
        }
        self.depth_cleared_for_next_scene = false;
        self.world = None;
        self.scene_active = true;
        Ok(())
    }

    fn reset_c3d_world(&mut self) -> Result<(), Self::SceneError> {
        self.require_active("reset_c3d_world")?;
        self.world = Some(cmr_core::frontend_c3d_scene::C3D_IDENTITY_VIEW);
        Ok(())
    }

    fn finish_c3d_scene_tail(&mut self) -> Result<C3dBindingState, Self::SceneError> {
        self.require_active("finish_c3d_scene_tail")?;
        Ok(C3dBindingState::Preserved)
    }

    fn end_c3d_scene(&mut self) -> Result<(), Self::SceneError> {
        self.require_active("end_c3d_scene")?;
        self.scene_active = false;
        self.world = None;
        Ok(())
    }
}

impl ChampionshipCarBatchBackend<SoftwareSplitC3dBinding> for SoftwareSplitC3dOracleBackend {
    type Error = SoftwareSplitC3dBackendError;

    fn preflight_championship_car_scene(
        &mut self,
        pre_draw_profile: ChampionshipCarPreDrawProfile,
    ) -> Result<(), Self::Error> {
        self.require_inactive("preflight_championship_car_scene")?;
        validate_championship_car_split_modulate_no_alpha_oracle_profile(
            self.configuration.profile,
        )
        .map_err(SoftwareSplitC3dBackendError::Raster)?;
        self.validate_pre_draw_profile(pre_draw_profile)
    }

    fn preflight_championship_car_batch(
        &mut self,
        batch: ChampionshipCarBatchPreflight<'_, '_, SoftwareSplitC3dBinding>,
    ) -> Result<(), Self::Error> {
        self.require_inactive("preflight_championship_car_batch")?;
        self.validate_retained_binding(&batch)?;
        let profile = self.configuration.profile;
        let prepared = preflight_championship_car_split_modulate_batch(
            batch.command,
            batch.vertices,
            SoftwareC3dTransformState::new(
                batch.actor_world,
                cmr_core::frontend_c3d_scene::C3D_IDENTITY_VIEW,
                ROOT_MENU_C3D_PROJECTION,
                self.preflight_viewport,
            ),
            profile,
        )
        .map_err(SoftwareSplitC3dBackendError::Raster)?;
        let source = match batch.binding {
            ChampionshipCarResolvedBinding::Resource { binding, .. } => {
                SoftwareChampionshipCarSplitSource::ConvertedTexture(binding.texture())
            }
            ChampionshipCarResolvedBinding::Missing => {
                SoftwareChampionshipCarSplitSource::BlackDiffuseDefault
            }
        };
        preflight_championship_car_split_modulate_logical_masked_rgb_oracle(
            &prepared,
            source,
            &self.depth,
            &self.target,
            profile,
        )
        .map_err(SoftwareSplitC3dBackendError::Compose)
    }

    fn set_championship_car_actor_world(
        &mut self,
        _actor_index: usize,
        world: C3dMatrixWords,
    ) -> Result<(), Self::Error> {
        self.require_active("set_championship_car_actor_world")?;
        self.world = Some(world);
        Ok(())
    }

    fn bind_championship_car_resource(
        &mut self,
        binding: &SoftwareSplitC3dBinding,
    ) -> Result<(), Self::Error> {
        self.require_active("bind_championship_car_resource")?;
        self.binding = Some(SoftwareSplitC3dBoundSource::Converted(binding.clone()));
        Ok(())
    }

    fn bind_championship_car_default(&mut self) -> Result<(), Self::Error> {
        self.require_active("bind_championship_car_default")?;
        self.binding = Some(SoftwareSplitC3dBoundSource::BlackDiffuseDefault);
        Ok(())
    }

    fn draw_championship_car_batch(
        &mut self,
        command: &C3dDrawCommand,
        vertices: &[C3dType2PreDrawVertex],
    ) -> Result<(), Self::Error> {
        self.require_active("draw_championship_car_batch")?;
        let world = self
            .world
            .ok_or(SoftwareSplitC3dBackendError::InvalidSequence(
                "draw_championship_car_batch without actor world",
            ))?;
        let view = self
            .view
            .ok_or(SoftwareSplitC3dBackendError::InvalidSequence(
                "draw_championship_car_batch without view",
            ))?;
        let viewport = self
            .viewport
            .ok_or(SoftwareSplitC3dBackendError::InvalidSequence(
                "draw_championship_car_batch without viewport",
            ))?;
        let binding = self
            .binding
            .clone()
            .ok_or(SoftwareSplitC3dBackendError::InvalidSequence(
                "draw_championship_car_batch without binding",
            ))?;
        let profile = self.configuration.profile;
        let prepared = preflight_championship_car_split_modulate_batch(
            command,
            vertices,
            SoftwareC3dTransformState::new(world, view, ROOT_MENU_C3D_PROJECTION, viewport),
            profile,
        )
        .map_err(SoftwareSplitC3dBackendError::Raster)?;
        let source = match &binding {
            SoftwareSplitC3dBoundSource::Converted(binding) => {
                SoftwareChampionshipCarSplitSource::ConvertedTexture(binding.texture())
            }
            SoftwareSplitC3dBoundSource::BlackDiffuseDefault => {
                SoftwareChampionshipCarSplitSource::BlackDiffuseDefault
            }
        };
        compose_championship_car_split_modulate_logical_masked_rgb_oracle(
            &prepared,
            source,
            &mut self.depth,
            &mut self.target,
            profile,
        )
        .map_err(SoftwareSplitC3dBackendError::Compose)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use cmr_core::{
        championship_car_execute::ChampionshipCarBatchBackend,
        frontend_c3d_scene::{
            C3dBindingState, C3dScenePrimaryError, C3dSceneStage, execute_frontend_c3d_scene,
        },
        root_menu_c3d_render_state::{
            C3dAlphaTestKnowledge, C3dBlendFactor, C3dFilterDeviceAction, C3dGraphicsBranch,
            ROOT_MENU_C3D_INITIAL_FILTER_REQUEST_CACHE, root_menu_c3d_filter_cache_outcome,
            root_menu_c3d_legacy_alpha_contract,
        },
    };

    use super::*;
    use crate::{
        c3d_software_raster::SoftwareC3dSplitRenderDepth,
        legacy_c3d_pixel_format::RawDdPixelFormat, packed_rgb_target::FramebufferRgbFormat,
    };

    const EXTENT: DisplayExtent = DisplayExtent {
        width: 4,
        height: 3,
    };

    fn framebuffer_rgb565() -> FramebufferRgbFormat {
        FramebufferRgbFormat::from_runtime_surface_descriptor(RawDdPixelFormat::from_dwords([
            32, 0x40, 0, 16, 0xf800, 0x07e0, 0x001f, 0,
        ]))
        .unwrap()
    }

    fn profile() -> SoftwareChampionshipCarSplitOracleProfile {
        SoftwareChampionshipCarSplitOracleProfile::new(
            SoftwareC3dSplitRenderDepth::Bits32,
            false,
            false,
        )
    }

    fn configuration() -> SoftwareSplitC3dOracleConfiguration {
        SoftwareSplitC3dOracleConfiguration {
            profile: profile(),
            alpha_contract: root_menu_c3d_legacy_alpha_contract(0),
            filter_cache_outcome: root_menu_c3d_filter_cache_outcome(
                C3dGraphicsBranch::Alternate,
                true,
                ROOT_MENU_C3D_INITIAL_FILTER_REQUEST_CACHE,
            ),
        }
    }

    fn backend() -> SoftwareSplitC3dOracleBackend {
        SoftwareSplitC3dOracleBackend::new(EXTENT, framebuffer_rgb565(), configuration()).unwrap()
    }

    #[test]
    fn constructor_owns_distinct_target_and_depth_with_explicit_provenance() {
        let first = backend();
        let other = backend();

        assert_eq!(first.extent(), EXTENT);
        assert_ne!(first.target_handle(), other.target_handle());
        assert_eq!(first.logical_target().extent(), [4, 3]);
        assert_eq!(first.logical_target().format(), framebuffer_rgb565());
        assert_eq!(first.logical_target().words(), &[0; 12]);
        assert_eq!(first.depth_values(), &[1.0; 12]);
        assert_eq!(first.configuration(), configuration());
        assert_eq!(
            first.configuration().filter_cache_outcome.device_action,
            C3dFilterDeviceAction::PreserveInheritedOrUnknown
        );
    }

    #[test]
    fn foreign_target_is_rejected_at_every_target_boundary_before_effects() {
        let owner = backend();
        let foreign = owner.target_handle();
        let mut backend = backend();
        let own = backend.target_handle();
        let words_before = backend.logical_target().words().to_vec();
        let depth_before = backend.depth_values().to_vec();
        let expected = SoftwareSplitC3dBackendError::ForeignTarget {
            expected: own,
            actual: foreign,
        };

        assert_eq!(
            backend.preflight_c3d_scene(foreign, EXTENT, C3dDepthClear::Clear),
            Err(expected.clone())
        );
        assert_eq!(
            backend.clear_c3d_depth(foreign, EXTENT),
            Err(expected.clone())
        );
        assert_eq!(backend.begin_c3d_scene(foreign), Err(expected));
        assert_eq!(backend.logical_target().words(), words_before);
        assert_eq!(backend.depth_values(), depth_before);
    }

    #[test]
    fn constructor_rejects_alpha_and_wrong_filter_contracts_without_backend() {
        let alpha = RootMenuC3dLegacyAlphaContract {
            alpha_blend_enabled: true,
            clear_depth_before_scene: false,
            source_blend: C3dBlendFactor::SourceAlpha,
            destination_blend: C3dBlendFactor::InverseSourceAlpha,
            alpha_test: C3dAlphaTestKnowledge::NotRecoveredFromLegacyGate,
        };
        let alpha_configuration = SoftwareSplitC3dOracleConfiguration {
            alpha_contract: alpha,
            ..configuration()
        };
        assert!(matches!(
            SoftwareSplitC3dOracleBackend::new(
                EXTENT,
                framebuffer_rgb565(),
                alpha_configuration
            ),
            Err(SoftwareSplitC3dBackendError::UnsupportedAlphaContract(value)) if value == alpha
        ));

        let wrong_filter = RootMenuC3dFilterCacheOutcome {
            requested_filter: 1,
            device_action: C3dFilterDeviceAction::Submit { submitted_value: 1 },
            resulting_filter_request_cache: 1,
        };
        let filter_configuration = SoftwareSplitC3dOracleConfiguration {
            filter_cache_outcome: wrong_filter,
            ..configuration()
        };
        assert_eq!(
            SoftwareSplitC3dOracleBackend::new(EXTENT, framebuffer_rgb565(), filter_configuration)
                .unwrap_err(),
            SoftwareSplitC3dBackendError::FilterRequestMismatch {
                expected: 2,
                actual: wrong_filter,
            }
        );
    }

    #[test]
    fn profile_preflight_rejection_is_effect_free_and_recoverable() {
        let mut backend = backend();
        backend
            .target
            .replace_from_canonical_rgb([4, 3], &[[20, 40, 60]; 12])
            .unwrap();
        let words_before = backend.logical_target().words().to_vec();
        let depth_before = backend.depth_values().to_vec();

        assert_eq!(
            ChampionshipCarBatchBackend::<SoftwareSplitC3dBinding>::preflight_championship_car_scene(
                &mut backend,
                ChampionshipCarPreDrawProfile {
                    type2_fog_enabled: true,
                    actor_rgb_enabled: false,
                },
            ),
            Err(
                SoftwareSplitC3dBackendError::ChampionshipCarFogStateMismatch {
                    profile_enabled: false,
                    prepared_enabled: true,
                }
            )
        );
        assert_eq!(backend.logical_target().words(), words_before);
        assert_eq!(backend.depth_values(), depth_before);

        ChampionshipCarBatchBackend::<SoftwareSplitC3dBinding>::preflight_championship_car_scene(
            &mut backend,
            ChampionshipCarPreDrawProfile {
                type2_fog_enabled: false,
                actor_rgb_enabled: false,
            },
        )
        .unwrap();
    }

    #[test]
    fn clear_is_required_and_empty_scene_preserves_owned_color() {
        let mut backend = backend();
        backend
            .target
            .replace_from_canonical_rgb([4, 3], &[[20, 40, 60]; 12])
            .unwrap();
        let words_before = backend.logical_target().words().to_vec();
        let handle = backend.target_handle();

        let first = execute_frontend_c3d_scene(
            &mut backend,
            handle,
            EXTENT,
            C3dDepthClear::Clear,
            None::<()>,
            |_, ()| Ok::<(), std::convert::Infallible>(()),
        )
        .unwrap();
        assert!(first.depth_cleared);
        assert_eq!(first.binding_state, C3dBindingState::Preserved);
        assert_eq!(backend.logical_target().words(), words_before);
        assert_eq!(backend.depth_values(), &[1.0; 12]);

        let skipped = execute_frontend_c3d_scene(
            &mut backend,
            handle,
            EXTENT,
            C3dDepthClear::Skip,
            None::<()>,
            |_, ()| Ok::<(), std::convert::Infallible>(()),
        )
        .unwrap_err();
        assert_eq!(
            skipped.primary,
            C3dScenePrimaryError::Backend {
                stage: C3dSceneStage::Preflight,
                source: SoftwareSplitC3dBackendError::DepthClearRequired,
            }
        );
        assert_eq!(backend.logical_target().words(), words_before);
        assert_eq!(backend.depth_values(), &[1.0; 12]);

        execute_frontend_c3d_scene(
            &mut backend,
            handle,
            EXTENT,
            C3dDepthClear::Clear,
            None::<()>,
            |_, ()| Ok::<(), std::convert::Infallible>(()),
        )
        .unwrap();
    }

    #[test]
    fn indexed_frame_bridge_is_atomic_and_rejected_during_an_active_scene() {
        let mut backend = backend();
        let handle = backend.target_handle();
        let mut palette = [PaletteEntry {
            red: 0,
            green: 0,
            blue: 0,
            flags: 0,
        }; 256];
        palette[7] = PaletteEntry {
            red: 255,
            green: 0,
            blue: 0,
            flags: 0,
        };
        backend
            .replace_logical_target_from_indexed_palette([4, 3], &[7; 12], &palette)
            .unwrap();
        assert_eq!(backend.logical_target().words(), &[0xf800; 12]);
        let words_before = backend.logical_target().words().to_vec();

        assert_eq!(
            backend.replace_logical_target_from_indexed_palette([4, 3], &[7; 11], &palette),
            Err(SoftwareSplitC3dBackendError::Target(
                LogicalMaskedRgbTargetError::InvalidSourceLength {
                    expected: 12,
                    actual: 11,
                }
            ))
        );
        assert_eq!(backend.logical_target().words(), words_before);

        backend.clear_c3d_depth(handle, EXTENT).unwrap();
        backend
            .set_c3d_view(cmr_core::frontend_c3d_scene::C3D_IDENTITY_VIEW)
            .unwrap();
        backend
            .set_c3d_viewport(
                cmr_core::root_menu_c3d_transform::root_menu_c3d_viewport(EXTENT).unwrap(),
            )
            .unwrap();
        backend.begin_c3d_scene(handle).unwrap();
        assert_eq!(
            backend.replace_logical_target_from_indexed_palette([4, 3], &[0; 12], &palette),
            Err(SoftwareSplitC3dBackendError::InvalidSequence(
                "replace_logical_target_from_indexed_palette"
            ))
        );
        assert_eq!(backend.logical_target().words(), words_before);
        backend.end_c3d_scene().unwrap();

        backend
            .replace_logical_target_from_indexed_palette([4, 3], &[0; 12], &palette)
            .unwrap();
        assert_eq!(backend.logical_target().words(), &[0; 12]);
    }

    #[test]
    fn invalid_clear_extent_is_atomic_and_does_not_arm_a_later_skip() {
        let mut backend = backend();
        let handle = backend.target_handle();
        let wrong = DisplayExtent {
            width: EXTENT.width - 1,
            height: EXTENT.height,
        };
        let words_before = backend.logical_target().words().to_vec();
        let depth_before = backend.depth_values().to_vec();

        assert_eq!(
            backend.clear_c3d_depth(handle, wrong),
            Err(SoftwareSplitC3dBackendError::DestinationExtentMismatch {
                expected: EXTENT,
                actual: wrong,
            })
        );
        assert_eq!(backend.logical_target().words(), words_before);
        assert_eq!(backend.depth_values(), depth_before);

        backend
            .set_c3d_view(cmr_core::frontend_c3d_scene::C3D_IDENTITY_VIEW)
            .unwrap();
        backend
            .set_c3d_viewport(
                cmr_core::root_menu_c3d_transform::root_menu_c3d_viewport(EXTENT).unwrap(),
            )
            .unwrap();
        assert_eq!(
            backend.begin_c3d_scene(handle),
            Err(SoftwareSplitC3dBackendError::DepthClearRequired)
        );
    }
}
