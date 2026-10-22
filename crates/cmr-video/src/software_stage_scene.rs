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
use cmr_core::root_menu_c3d_transform::RootMenuC3dViewportError;
use cmr_formats::riff_palette::PaletteEntry;

use crate::c3d_software::SoftwareC3dSampledTexture;
use crate::c3d_software_raster::{
    SoftwareC3dComposeError, SoftwareC3dDepthBuffer, SoftwareC3dIndexedQuantization,
    SoftwareC3dRasterError, SoftwareC3dTransformState, compose_root_c3d_indexed_copy_no_blend,
    preflight_mesh_lod_prim,
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
        let transforms =
            SoftwareC3dTransformState::for_gameplay_scene(width, height, camera_world_q16, world)?;
        for prim in draw_group.prims() {
            let Some(texture) = resolve_texture(prim.selector()) else {
                stats.unresolved_prims += 1;
                continue;
            };
            let batch =
                preflight_mesh_lod_prim(prim.unique_entries_32(), prim.indices(), transforms)?;
            let outcome = compose_root_c3d_indexed_copy_no_blend(
                &batch,
                texture,
                &mut depth,
                extent,
                &mut pixels,
                active_palette,
                quantization,
            )?;
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
        }
    }
}

impl std::error::Error for StageSceneComposeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StageSceneComposeError::Viewport(error) => Some(error),
            StageSceneComposeError::Raster(error) => Some(error),
            StageSceneComposeError::Compose(error) => Some(error),
        }
    }
}
