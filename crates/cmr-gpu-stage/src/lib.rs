//! Backend-neutral preparation of a recovered C3D stage into the bounded raw
//! GPU profile.
//!
//! [`PreparedRawStage`] converts a walkable [`StageScene`] and its already
//! resolved group-2 textures into validated raw-C3D resources plus the recovered
//! draw list. It performs no transform, clipping, culling, or raster
//! preparation: the GPU applies WORLD/VIEW/PROJECTION, clips and culls, and
//! samples the indexed materials through the active display palette.
//!
//! The resources, textures, and draw list are camera-independent, so they are
//! converted once and reused for every frame. Only the VIEW matrix (and, when a
//! caller changes it, the PROJECTION matrix) depends on the camera, which is
//! supplied per frame by [`PreparedRawStage::prepared_frame_for`]. That is what
//! lets a windowed session render a moving follow camera through a single
//! persistent GPU upload.

use std::collections::BTreeMap;
use std::fmt;

use cmr_core::c3d_camera::{
    PROJECTION_BASELINE, projection_matrix_words, view_matrix_words, world_matrix_words,
};
use cmr_core::c3d_stage_scene::StageScene;
use cmr_core::display::DisplayExtent;
use cmr_formats::riff_palette::PaletteEntry;
use cmr_gpu_contract::{
    C3dDraw, C3dIndexedTexture, C3dMeshResource, C3dPreparedFrame, C3dRawFrame, C3dResourceSet,
    C3dRgb8, C3dSceneUpload, C3dSubmissionBracket, C3dVertexQ16, RawC3dContractError,
};
use cmr_video::software_stage_scene::StageC3dTextures;

/// Counts retained and skipped inputs while a [`StageScene`] is converted to
/// raw GPU resources. Skips mirror the established software stage composer.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PreparedRawStageStats {
    pub draws: usize,
    pub unique_textures: usize,
    pub unresolved_prims: usize,
    pub empty_lod_nodes: usize,
}

/// An owned raw-GPU view of one already parsed stage scene.
///
/// Geometry remains the C3D Q16.16 vertex words plus compact `u16` indices.
/// The adapter performs no transform, clipping, culling, or raster preparation.
/// A material ordinal is uploaded once and shared by every draw that names it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedRawStage {
    extent: DisplayExtent,
    active_palette: Box<[PaletteEntry; 256]>,
    resources: C3dResourceSet,
    draws: Box<[C3dDraw]>,
    stats: PreparedRawStageStats,
}

impl PreparedRawStage {
    /// Converts the recovered scene walk and its already-decoded group-2
    /// textures into the bounded opaque-COPY raw GPU profile.
    ///
    /// The result is camera-independent: the recovered baseline PROJECTION is
    /// baked into the scene layout, and each frame supplies its own VIEW matrix.
    pub fn prepare(
        scene: &StageScene<'_>,
        textures: &StageC3dTextures,
        extent: DisplayExtent,
        active_palette: &[PaletteEntry; 256],
    ) -> Result<Self, PreparedRawStageError> {
        let mut meshes = Vec::new();
        let mut prepared_textures = Vec::new();
        let mut texture_by_ordinal = BTreeMap::<i16, u32>::new();
        let mut draws = Vec::new();
        let mut stats = PreparedRawStageStats::default();

        for op in scene.draw_order() {
            let Some(mesh) = scene.mesh(op.node) else {
                continue;
            };
            let Some(draw_group) = mesh.active() else {
                stats.empty_lod_nodes += 1;
                continue;
            };
            for prim in draw_group.prims() {
                let ordinal = prim.selector();
                let Some(sampled) = textures.texture_for_ordinal(ordinal) else {
                    stats.unresolved_prims += 1;
                    continue;
                };

                let texture_id = if let Some(&existing) = texture_by_ordinal.get(&ordinal) {
                    existing
                } else {
                    let [width, height] = sampled.extent();
                    let texture = C3dIndexedTexture::new(
                        DisplayExtent { width, height },
                        sampled.indices().to_vec(),
                        sampled
                            .palette()
                            .iter()
                            .copied()
                            .map(C3dRgb8::from)
                            .collect::<Vec<_>>(),
                        // The established single-stage path is the opaque main
                        // graph. Layered preparation opts OBJ/SHT/car into
                        // their recovered key explicitly instead of changing
                        // this compatibility contract implicitly.
                        None,
                    )?;
                    let id = u32::try_from(prepared_textures.len()).map_err(|_| {
                        PreparedRawStageError::ResourceCountOverflow {
                            resource: "texture",
                            count: prepared_textures.len(),
                        }
                    })?;
                    prepared_textures.push(texture);
                    texture_by_ordinal.insert(ordinal, id);
                    id
                };

                let mesh_id = u32::try_from(meshes.len()).map_err(|_| {
                    PreparedRawStageError::ResourceCountOverflow {
                        resource: "mesh",
                        count: meshes.len(),
                    }
                })?;
                meshes.push(C3dMeshResource::new(
                    prim.unique_entries_32()
                        .iter()
                        .copied()
                        .map(C3dVertexQ16::from)
                        .collect::<Vec<_>>(),
                    prim.indices().to_vec(),
                )?);
                draws.push(C3dDraw::new(
                    mesh_id,
                    texture_id,
                    world_matrix_words(scene.world(op.node)),
                    if op.deferred_translucent {
                        C3dSubmissionBracket::DeferredTranslucent
                    } else {
                        C3dSubmissionBracket::Normal
                    },
                )?);
            }
        }

        stats.draws = draws.len();
        stats.unique_textures = prepared_textures.len();
        let resources = C3dResourceSet::new(meshes, prepared_textures);
        // Validate the complete cross-resource frame now; the per-frame
        // accessors repeat the cheap borrowed validation and cannot fail unless
        // this type is broken.
        let (projection_width, projection_height, z_near, z_far) = PROJECTION_BASELINE;
        C3dRawFrame::new(
            extent,
            view_matrix_words(&cmr_core::c3d_camera::MATRIX_IDENTITY_Q16),
            projection_matrix_words(projection_width, projection_height, z_near, z_far),
            active_palette,
            &resources,
            &draws,
        )?;

        Ok(Self {
            extent,
            active_palette: Box::new(*active_palette),
            resources,
            draws: draws.into_boxed_slice(),
            stats,
        })
    }

    /// Borrows the immutable upload inputs for a persistent GPU scene.
    pub fn upload(&self) -> C3dSceneUpload<'_> {
        C3dSceneUpload::new(self.extent, &self.resources, &self.active_palette)
            .expect("PreparedRawStage validated its immutable extent and resources")
    }

    /// Borrows the per-frame matrices and draw list for the uploaded scene.
    pub fn prepared_frame_for(&self, camera_world_q16: &[i32; 16]) -> C3dPreparedFrame<'_> {
        let (projection_width, projection_height, z_near, z_far) = PROJECTION_BASELINE;
        C3dPreparedFrame::new(
            self.upload().layout(),
            view_matrix_words(camera_world_q16),
            projection_matrix_words(projection_width, projection_height, z_near, z_far),
            &self.draws,
        )
        .expect("PreparedRawStage validated its immutable matrices and draws")
    }

    /// Borrows the synchronous raw frame (resources, draws, palette, camera) for
    /// a backend that still allocates and uploads per call.
    pub fn frame_for(&self, camera_world_q16: &[i32; 16]) -> C3dRawFrame<'_> {
        let (projection_width, projection_height, z_near, z_far) = PROJECTION_BASELINE;
        C3dRawFrame::new(
            self.extent,
            view_matrix_words(camera_world_q16),
            projection_matrix_words(projection_width, projection_height, z_near, z_far),
            &self.active_palette,
            &self.resources,
            &self.draws,
        )
        .expect("PreparedRawStage validated its immutable resources and draws")
    }

    #[must_use]
    pub const fn stats(&self) -> PreparedRawStageStats {
        self.stats
    }
}

/// Why a recovered stage could not be converted to the raw GPU profile.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PreparedRawStageError {
    Contract(RawC3dContractError),
    ResourceCountOverflow {
        resource: &'static str,
        count: usize,
    },
}

impl From<RawC3dContractError> for PreparedRawStageError {
    fn from(error: RawC3dContractError) -> Self {
        Self::Contract(error)
    }
}

impl fmt::Display for PreparedRawStageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "cannot prepare raw GPU stage: {self:?}")
    }
}

impl std::error::Error for PreparedRawStageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Contract(error) => Some(error),
            Self::ResourceCountOverflow { .. } => None,
        }
    }
}
