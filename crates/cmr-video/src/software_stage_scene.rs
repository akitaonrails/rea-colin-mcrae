//! Software composition of a walkable C3D stage/car scene (reconstruction item
//! E08, the pixel half of the stage scene session).
//!
//! [`cmr_core::c3d_stage_scene::StageScene`] is the backend-neutral walk: it
//! yields a draw order plus each node's world matrix and active-LOD draw group.
//! [`compose_stage_scene_frame`] drives that walk through the existing software
//! rasterizer - for each prim it builds the gameplay transform state
//! ([`SoftwareC3dTransformState::for_gameplay_scene`]), preflights the mesh prim
//! ([`preflight_mesh_lod_prim`]), and composites it into one shared indexed
//! framebuffer and [`SoftwareC3dDepthBuffer`] via
//! [`compose_root_c3d_indexed_copy_no_blend`] - exactly the path the occlusion
//! oracle proved order-independent, so the draw order only matters once alpha
//! blending is wired (the current COPY path is opaque, Z-buffered).
//!
//! Binding a prim's group-2 material ordinal to a texture is the separate E03
//! step; this composer **defers** it to a caller-supplied `resolve_texture`
//! closure (mirroring how [`cmr_core::c3d_mesh::assemble_lod_draw_group`] defers
//! the material selectors). A prim whose ordinal the caller cannot resolve is
//! skipped and counted, so the composer stays honest about the unresolved
//! binding rather than inventing one.

use std::fmt;

use cmr_core::c3d_stage_scene::StageScene;
use cmr_core::display::DisplayExtent;
use cmr_core::root_menu_c3d_resources::FrontendC3dOwner;
use cmr_core::root_menu_c3d_transform::RootMenuC3dViewportError;
use cmr_formats::bitmap::BitmapLimits;
use cmr_formats::c3d::{C3dAnimationReferenceError, C3dFile, C3dResourceError, C3dResourceView};
use cmr_formats::riff_palette::PaletteEntry;

use crate::c3d_software::{SoftwareC3dBitmapError, SoftwareC3dSampledTexture};
use crate::c3d_software_raster::{
    SoftwareC3dComposeError, SoftwareC3dDepthBuffer, SoftwareC3dIndexedQuantization,
    SoftwareC3dRasterError, SoftwareC3dTransformState, compose_root_c3d_indexed_copy_color_key,
    compose_root_c3d_indexed_copy_no_blend, preflight_mesh_lod_prim,
};
use crate::{SoftwarePresentedFrame, SoftwareRgba8Error};

/// The counters a [`compose_stage_scene_frame`] run accumulates.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StageSceneFrameStats {
    /// Prims composited into the framebuffer.
    pub prims_composed: usize,
    /// Prims skipped because `resolve_texture` returned `None` for the ordinal.
    pub unresolved_prims: usize,
    /// Drawable nodes whose active LOD held no geometry (an empty/sentinel LOD).
    pub empty_lod_nodes: usize,
    /// Fragments the rasterizer produced across all composited prims.
    pub covered: usize,
    /// Fragments that passed the depth test (were written).
    pub depth_passed: usize,
    /// Fragments the depth test rejected (occluded).
    pub depth_rejected: usize,
}

/// A composed stage-scene frame: the indexed framebuffer, its display palette,
/// its depth buffer, and the run statistics.
#[derive(Clone, Debug)]
pub struct StageSceneFrame {
    extent: [u16; 2],
    pixels: Vec<u8>,
    palette: Box<[PaletteEntry; 256]>,
    depth: SoftwareC3dDepthBuffer,
    stats: StageSceneFrameStats,
}

impl StageSceneFrame {
    fn add_stats(&mut self, other: &StageSceneFrame) {
        self.stats.prims_composed += other.stats.prims_composed;
        self.stats.unresolved_prims += other.stats.unresolved_prims;
        self.stats.empty_lod_nodes += other.stats.empty_lod_nodes;
        self.stats.covered += other.stats.covered;
        self.stats.depth_passed += other.stats.depth_passed;
        self.stats.depth_rejected += other.stats.depth_rejected;
    }

    /// The frame extent `[width, height]`.
    pub fn extent(&self) -> [u16; 2] {
        self.extent
    }

    /// The composed indexed framebuffer (`width * height` palette indices).
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// The display palette the frame was composed against.
    pub fn palette(&self) -> &[PaletteEntry; 256] {
        &self.palette
    }

    /// The depth buffer after composition.
    pub fn depth(&self) -> &SoftwareC3dDepthBuffer {
        &self.depth
    }

    /// The run statistics.
    pub fn stats(&self) -> &StageSceneFrameStats {
        &self.stats
    }

    /// Bridges the indexed frame into a [`SoftwarePresentedFrame`] the platform
    /// host can present (via `expand_rgba8` + a streaming texture upload),
    /// reusing the same presentation type the root-menu backend emits.
    pub fn to_presented_frame(&self) -> Result<SoftwarePresentedFrame, SoftwareRgba8Error> {
        let [width, height] = self.extent;
        SoftwarePresentedFrame::from_indexed(
            DisplayExtent { width, height },
            self.pixels.clone().into_boxed_slice(),
            self.palette.clone(),
        )
    }

    /// Places this world/foreground pass over an independently rendered
    /// background pass. Pixels whose foreground depth remains at the clear
    /// value (`1.0`) retain the background; every depth-writing foreground
    /// fragment replaces it. This models the shipped horizon as a background
    /// C3D pass without treating palette index zero as transparency.
    pub fn over_background(
        mut self,
        background: &StageSceneFrame,
    ) -> Result<Self, StageSceneComposeError> {
        if self.extent != background.extent {
            return Err(StageSceneComposeError::LayerExtentMismatch {
                foreground: self.extent,
                background: background.extent,
            });
        }
        for ((pixel, depth), background_pixel) in self
            .pixels
            .iter_mut()
            .zip(self.depth.values())
            .zip(background.pixels.iter())
        {
            if *depth == 1.0 {
                *pixel = *background_pixel;
            }
        }
        self.add_stats(background);
        Ok(self)
    }

    /// Depth-composites a later independently rendered opaque world graph into
    /// this frame. The nearer depth wins per pixel and `other` wins equal-depth
    /// ties, reproducing `LessEqual` submission to one shared depth buffer while
    /// keeping the graphs' C3D resource namespaces independent.
    pub fn merge_nearest(
        mut self,
        other: &StageSceneFrame,
    ) -> Result<Self, StageSceneComposeError> {
        if self.extent != other.extent {
            return Err(StageSceneComposeError::LayerExtentMismatch {
                foreground: self.extent,
                background: other.extent,
            });
        }
        for ((pixel, depth), (other_pixel, other_depth)) in self
            .pixels
            .iter_mut()
            .zip(self.depth.values_mut())
            .zip(other.pixels.iter().zip(other.depth.values()))
        {
            if other_depth <= depth {
                *pixel = *other_pixel;
                *depth = *other_depth;
            }
        }
        self.add_stats(other);
        Ok(self)
    }
}

/// Composites a walkable stage scene into a software frame.
///
/// Walks [`StageScene::draw_order`]; for each drawable node's active-LOD draw
/// group it composites every prim whose group-2 material ordinal
/// (`prim.selector()`) the caller's `resolve_texture` resolves to a texture. The
/// node's world matrix goes in the WORLD slot (via `for_gameplay_scene`), so a
/// prim's model-space vertices are transformed by its node placement; `camera`
/// supplies the VIEW inverse. Unresolved prims and empty LODs are skipped and
/// counted. The framebuffer starts cleared to index `0` and the depth buffer to
/// the far plane; composition is opaque COPY under the hardware-Z contract.
pub fn compose_stage_scene_frame<'tex, R>(
    scene: &StageScene<'_>,
    camera_world_q16: &[i32; 16],
    extent: [u16; 2],
    active_palette: &[PaletteEntry; 256],
    quantization: SoftwareC3dIndexedQuantization,
    resolve_texture: R,
) -> Result<StageSceneFrame, StageSceneComposeError>
where
    R: FnMut(i16) -> Option<&'tex SoftwareC3dSampledTexture>,
{
    compose_stage_scene_frame_with_projection(
        scene,
        camera_world_q16,
        extent,
        active_palette,
        quantization,
        cmr_core::c3d_camera::PROJECTION_BASELINE,
        resolve_texture,
    )
}

/// Composes a stage scene using one of the recovered projection parameter
/// sets. This is used by the horizon pass, whose native renderer temporarily
/// raises the far plane to 25000 world units.
pub fn compose_stage_scene_frame_with_projection<'tex, R>(
    scene: &StageScene<'_>,
    camera_world_q16: &[i32; 16],
    extent: [u16; 2],
    active_palette: &[PaletteEntry; 256],
    quantization: SoftwareC3dIndexedQuantization,
    projection: (f32, f32, f32, f32),
    resolve_texture: R,
) -> Result<StageSceneFrame, StageSceneComposeError>
where
    R: FnMut(i16) -> Option<&'tex SoftwareC3dSampledTexture>,
{
    compose_stage_scene_frame_profile(
        scene,
        camera_world_q16,
        extent,
        active_palette,
        quantization,
        projection,
        None,
        resolve_texture,
    )
}

/// Composes a world graph with the recovered stage index-zero color key. A
/// keyed texel updates neither color nor depth.
pub fn compose_stage_scene_frame_color_key<'tex, R>(
    scene: &StageScene<'_>,
    camera_world_q16: &[i32; 16],
    extent: [u16; 2],
    active_palette: &[PaletteEntry; 256],
    quantization: SoftwareC3dIndexedQuantization,
    color_key: u8,
    resolve_texture: R,
) -> Result<StageSceneFrame, StageSceneComposeError>
where
    R: FnMut(i16) -> Option<&'tex SoftwareC3dSampledTexture>,
{
    compose_stage_scene_frame_profile(
        scene,
        camera_world_q16,
        extent,
        active_palette,
        quantization,
        cmr_core::c3d_camera::PROJECTION_BASELINE,
        Some(color_key),
        resolve_texture,
    )
}

#[allow(clippy::too_many_arguments)]
fn compose_stage_scene_frame_profile<'tex, R>(
    scene: &StageScene<'_>,
    camera_world_q16: &[i32; 16],
    extent: [u16; 2],
    active_palette: &[PaletteEntry; 256],
    quantization: SoftwareC3dIndexedQuantization,
    projection: (f32, f32, f32, f32),
    color_key: Option<u8>,
    mut resolve_texture: R,
) -> Result<StageSceneFrame, StageSceneComposeError>
where
    R: FnMut(i16) -> Option<&'tex SoftwareC3dSampledTexture>,
{
    let [width, height] = extent;
    let mut pixels = vec![0u8; usize::from(width) * usize::from(height)];
    let mut depth = SoftwareC3dDepthBuffer::new_far(extent)?;
    let mut stats = StageSceneFrameStats::default();

    for op in scene.draw_order() {
        let world = scene.world(op.node);
        let Some(mesh) = scene.mesh(op.node) else {
            continue;
        };
        let Some(draw_group) = mesh.active() else {
            stats.empty_lod_nodes += 1;
            continue;
        };
        // The transform state depends only on the node (its world matrix), so it
        // is built once and reused across the node's prims.
        let transforms = SoftwareC3dTransformState::for_gameplay_scene_with_projection(
            width,
            height,
            camera_world_q16,
            world,
            projection,
        )?;
        for prim in draw_group.prims() {
            let Some(texture) = resolve_texture(prim.selector()) else {
                stats.unresolved_prims += 1;
                continue;
            };
            let batch =
                preflight_mesh_lod_prim(prim.unique_entries_32(), prim.indices(), transforms)?;
            let outcome = if let Some(color_key) = color_key {
                compose_root_c3d_indexed_copy_color_key(
                    &batch,
                    texture,
                    &mut depth,
                    extent,
                    &mut pixels,
                    active_palette,
                    color_key,
                )?
            } else {
                compose_root_c3d_indexed_copy_no_blend(
                    &batch,
                    texture,
                    &mut depth,
                    extent,
                    &mut pixels,
                    active_palette,
                    quantization,
                )?
            };
            stats.prims_composed += 1;
            stats.covered += outcome.covered();
            stats.depth_passed += outcome.depth_passed();
            stats.depth_rejected += outcome.depth_rejected();
        }
    }

    Ok(StageSceneFrame {
        extent,
        pixels,
        palette: Box::new(*active_palette),
        depth,
        stats,
    })
}

/// Why [`compose_stage_scene_frame`] failed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StageSceneComposeError {
    /// The gameplay viewport could not be built for the frame extent.
    Viewport(RootMenuC3dViewportError),
    /// A prim failed transform/clip preflight, or the depth buffer was invalid.
    Raster(SoftwareC3dRasterError),
    /// A prim could not be composited into the target.
    Compose(SoftwareC3dComposeError),
    /// Independently rendered layers did not use the same target size.
    LayerExtentMismatch {
        foreground: [u16; 2],
        background: [u16; 2],
    },
}

impl From<RootMenuC3dViewportError> for StageSceneComposeError {
    fn from(error: RootMenuC3dViewportError) -> Self {
        StageSceneComposeError::Viewport(error)
    }
}

impl From<SoftwareC3dRasterError> for StageSceneComposeError {
    fn from(error: SoftwareC3dRasterError) -> Self {
        StageSceneComposeError::Raster(error)
    }
}

impl From<SoftwareC3dComposeError> for StageSceneComposeError {
    fn from(error: SoftwareC3dComposeError) -> Self {
        StageSceneComposeError::Compose(error)
    }
}

impl fmt::Display for StageSceneComposeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StageSceneComposeError::Viewport(error) => write!(f, "stage scene viewport: {error}"),
            StageSceneComposeError::Raster(error) => write!(f, "stage scene prim raster: {error}"),
            StageSceneComposeError::Compose(error) => write!(f, "stage scene compose: {error}"),
            StageSceneComposeError::LayerExtentMismatch {
                foreground,
                background,
            } => write!(
                f,
                "stage scene layer extents differ: foreground {foreground:?}, background {background:?}"
            ),
        }
    }
}

impl std::error::Error for StageSceneComposeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StageSceneComposeError::Viewport(error) => Some(error),
            StageSceneComposeError::Raster(error) => Some(error),
            StageSceneComposeError::Compose(error) => Some(error),
            StageSceneComposeError::LayerExtentMismatch { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(extent: [u16; 2], pixels: &[u8], depth: &[f32], prims: usize) -> StageSceneFrame {
        StageSceneFrame {
            extent,
            pixels: pixels.to_vec(),
            palette: Box::new(
                [PaletteEntry {
                    red: 0,
                    green: 0,
                    blue: 0,
                    flags: 0,
                }; 256],
            ),
            depth: SoftwareC3dDepthBuffer::from_test_values(extent, depth),
            stats: StageSceneFrameStats {
                prims_composed: prims,
                ..StageSceneFrameStats::default()
            },
        }
    }

    #[test]
    fn foreground_depth_not_palette_zero_selects_layer_coverage() {
        let background = frame([3, 1], &[3, 4, 5], &[0.5, 0.5, 0.5], 1);
        let foreground = frame([3, 1], &[0, 0, 9], &[1.0, 0.25, 0.25], 2);

        let composited = foreground.over_background(&background).unwrap();
        assert_eq!(composited.pixels(), &[3, 0, 9]);
        assert_eq!(composited.stats().prims_composed, 3);
    }

    #[test]
    fn layered_frames_require_matching_extents() {
        let background = frame([1, 1], &[3], &[0.5], 1);
        let foreground = frame([2, 1], &[0, 0], &[1.0, 1.0], 2);
        assert!(matches!(
            foreground.over_background(&background),
            Err(StageSceneComposeError::LayerExtentMismatch { .. })
        ));
    }

    #[test]
    fn independently_rendered_world_graphs_merge_by_nearest_depth() {
        let base = frame([4, 1], &[1, 2, 3, 7], &[0.8, 0.2, 1.0, 0.5], 1);
        let objects = frame([4, 1], &[4, 5, 6, 8], &[0.4, 0.7, 0.9, 0.5], 2);
        let merged = base.merge_nearest(&objects).unwrap();
        assert_eq!(merged.pixels(), &[4, 2, 6, 8]);
        assert_eq!(merged.depth().values(), &[0.4, 0.2, 0.9, 0.5]);
        assert_eq!(merged.stats().prims_composed, 3);
    }
}

/// The group-2 textures of one stage/car C3D scene, prepared and sampled into
/// the software [`SoftwareC3dSampledTexture`] form, indexed by group-2 ordinal.
///
/// This resolves the material binding the C3D composer defers: each group-2 blob
/// record becomes one sampled texture at its blob-record position, so a face's
/// `group2_ordinal` (`prim.selector()`) indexes directly into
/// [`texture_for_ordinal`](Self::texture_for_ordinal) - the same positional
/// face -> texture mapping the loader resolves through its material slot table
/// (`rea.official-eu.c3d-material-binding`). An untextured ordinal (`-1`) yields
/// `None`.
///
/// Each group-2 record is classified and decoded in place, so `by_ordinal` is
/// positional (one slot per group-2 record): a slot holds the sampled texture
/// when it decoded, or `None` for an unsupported palette-less/direct-24-bit
/// source. An inline `BM` bitmap must decode or preparation fails closed.
/// `ANIM` composites resolve their selected target frame.
#[derive(Clone, Debug)]
pub struct StageC3dTextures {
    by_ordinal: Vec<Option<SoftwareC3dSampledTexture>>,
}

impl StageC3dTextures {
    /// Prepares every group-2 record of `scene` into its sampled form, indexed
    /// by group-2 ordinal.
    ///
    /// A record is classified ([`C3dResourceView`]) then decoded per kind:
    /// - `BM` bitmap bytes decode through the frontend BM path and **must**
    ///   succeed, or the whole preparation fails
    ///   ([`StageC3dTextureError::Bitmap`]) - a malformed shipped-format bitmap
    ///   is a real defect, not a material to drop.
    /// - A non-`BM`/non-`ANIM` blob is the engine inline texture format; indexed
    ///   variants decode, while direct 24-bit and palette-less sources stay empty.
    /// - An `ANIM` composite resolves frame zero unless the caller selects another.
    ///
    /// A record whose `ANIM` framing is itself malformed aborts preparation
    /// ([`StageC3dTextureError::Classify`]), since that is corrupt input, not an
    /// unsupported-but-valid texture.
    pub fn prepare(
        scene: &C3dFile,
        bitmap_limits: BitmapLimits,
    ) -> Result<Self, StageC3dTextureError> {
        Self::prepare_with_animation_frames(scene, bitmap_limits, &[])
    }

    /// Prepare materials with a caller-selected frame per `ANIM` ordinal.
    /// Missing entries select frame zero, the static-loading baseline.
    pub fn prepare_with_animation_frames(
        scene: &C3dFile,
        bitmap_limits: BitmapLimits,
        animation_frames: &[usize],
    ) -> Result<Self, StageC3dTextureError> {
        let mut by_ordinal = Vec::with_capacity(scene.blob_records().len());
        for (ordinal, record) in scene.blob_records().enumerate() {
            if matches!(
                record.classify_resource(),
                Ok(C3dResourceView::Animation(_))
            ) {
                let frame = animation_frames.get(ordinal).copied().unwrap_or(0);
                let mut frames = scene
                    .animation_frame_references(ordinal)
                    .map_err(|source| StageC3dTextureError::Animation { ordinal, source })?;
                let frame_count = frames.len();
                let target = frames
                    .nth(frame)
                    .ok_or(StageC3dTextureError::AnimationFrame {
                        ordinal,
                        frame,
                        frame_count,
                    })?
                    .map_err(|_| StageC3dTextureError::AnimationFrame {
                        ordinal,
                        frame,
                        frame_count,
                    })?
                    .target();
                by_ordinal.push(Self::sample_material(target, ordinal, bitmap_limits)?);
            } else {
                by_ordinal.push(Self::sample_material(record, ordinal, bitmap_limits)?);
            }
        }
        Ok(StageC3dTextures { by_ordinal })
    }

    fn sample_material(
        record: cmr_formats::c3d::C3dBlobRecord<'_>,
        ordinal: usize,
        bitmap_limits: BitmapLimits,
    ) -> Result<Option<SoftwareC3dSampledTexture>, StageC3dTextureError> {
        match record
            .classify_resource()
            .map_err(|source| StageC3dTextureError::Classify { ordinal, source })?
        {
            C3dResourceView::BitmapBytes(bytes) => Ok(Some(
                SoftwareC3dSampledTexture::from_bm_blob(
                    bytes,
                    FrontendC3dOwner::Stage,
                    bitmap_limits,
                )
                .map_err(|source| StageC3dTextureError::Bitmap { ordinal, source })?,
            )),
            C3dResourceView::Unknown(_) => Ok(record.engine_bitmap().ok().and_then(|bitmap| {
                SoftwareC3dSampledTexture::from_engine_bitmap(&bitmap, FrontendC3dOwner::Stage).ok()
            })),
            C3dResourceView::Animation(_) => Ok(None),
        }
    }

    /// The sampled texture for a face's group-2 ordinal, or `None` for the
    /// untextured sentinel (`-1`), an out-of-range ordinal, or an ordinal whose
    /// record decoded to an empty slot (a composite or unsupported texture).
    pub fn texture_for_ordinal(&self, ordinal: i16) -> Option<&SoftwareC3dSampledTexture> {
        usize::try_from(ordinal)
            .ok()
            .and_then(|index| self.by_ordinal.get(index))
            .and_then(|slot| slot.as_ref())
    }

    /// The number of group-2 ordinal slots (decoded or empty).
    pub fn len(&self) -> usize {
        self.by_ordinal.len()
    }

    /// Whether the scene has no group-2 records.
    pub fn is_empty(&self) -> bool {
        self.by_ordinal.is_empty()
    }
}

/// Why [`StageC3dTextures::prepare`] failed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StageC3dTextureError {
    /// A group-2 record's resource framing (an `ANIM` composite header) is
    /// malformed and could not be classified.
    Classify {
        ordinal: usize,
        source: C3dResourceError,
    },
    /// An `ANIM` selection could not be resolved.
    Animation {
        ordinal: usize,
        source: C3dAnimationReferenceError,
    },
    /// The selected frame or its target ordinal was invalid.
    AnimationFrame {
        ordinal: usize,
        frame: usize,
        frame_count: usize,
    },
    /// A group-2 `BM` bitmap blob could not be decoded or sampled.
    Bitmap {
        ordinal: usize,
        source: SoftwareC3dBitmapError,
    },
}

impl fmt::Display for StageC3dTextureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StageC3dTextureError::Classify { ordinal, source } => {
                write!(f, "stage C3D group-2 ordinal {ordinal} classify: {source}")
            }
            StageC3dTextureError::Animation { ordinal, source } => {
                write!(f, "stage C3D group-2 ordinal {ordinal} animation: {source}")
            }
            StageC3dTextureError::AnimationFrame {
                ordinal,
                frame,
                frame_count,
            } => write!(
                f,
                "stage C3D group-2 ordinal {ordinal} frame {frame} outside {frame_count} frames or invalid target"
            ),
            StageC3dTextureError::Bitmap { ordinal, source } => {
                write!(f, "stage C3D group-2 ordinal {ordinal} bitmap: {source}")
            }
        }
    }
}

impl std::error::Error for StageC3dTextureError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StageC3dTextureError::Classify { source, .. } => Some(source),
            StageC3dTextureError::Animation { source, .. } => Some(source),
            StageC3dTextureError::AnimationFrame { .. } => None,
            StageC3dTextureError::Bitmap { source, .. } => Some(source),
        }
    }
}
