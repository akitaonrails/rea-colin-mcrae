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

/// One independently loaded C3D resource namespace in a combined foreground.
///
/// Texture ordinals are local to the layer. `dynamic_world_set` identifies a
/// per-frame exact node-world set (the player car today). Supplying propagated
/// worlds preserves the recovered WORLD_ROOT_CLASS short-circuit; applying one
/// blanket root multiplication would not.
#[derive(Clone, Copy)]
pub struct PreparedRawStageLayer<'a, 'scene> {
    scene: &'a StageScene<'scene>,
    textures: &'a StageC3dTextures,
    color_key_index: Option<u8>,
    dynamic_world_set: Option<usize>,
}

impl<'a, 'scene> PreparedRawStageLayer<'a, 'scene> {
    #[must_use]
    pub const fn opaque(scene: &'a StageScene<'scene>, textures: &'a StageC3dTextures) -> Self {
        Self {
            scene,
            textures,
            color_key_index: None,
            dynamic_world_set: None,
        }
    }

    #[must_use]
    pub const fn keyed(
        scene: &'a StageScene<'scene>,
        textures: &'a StageC3dTextures,
        color_key_index: u8,
    ) -> Self {
        Self {
            scene,
            textures,
            color_key_index: Some(color_key_index),
            dynamic_world_set: None,
        }
    }

    #[must_use]
    pub const fn keyed_dynamic(
        scene: &'a StageScene<'scene>,
        textures: &'a StageC3dTextures,
        color_key_index: u8,
        dynamic_world_set: usize,
    ) -> Self {
        Self {
            scene,
            textures,
            color_key_index: Some(color_key_index),
            dynamic_world_set: Some(dynamic_world_set),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DynamicDraw {
    draw: usize,
    world_set: usize,
    node: usize,
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
    dynamic_draws: Box<[DynamicDraw]>,
    dynamic_world_set_count: usize,
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
        Self::prepare_layers(
            [PreparedRawStageLayer::opaque(scene, textures)],
            extent,
            active_palette,
        )
    }

    /// Combines stable layer-local mesh and texture namespaces into one shared
    /// foreground upload in submission order.
    ///
    /// Main-stage callers use an opaque layer. OBJ/SHT and car callers opt into
    /// source index zero (or another recovered key) explicitly. Dynamic layers
    /// must have been assembled under the identity root; their exact propagated
    /// node worlds are supplied through [`Self::prepared_frame_for_worlds`].
    pub fn prepare_layers<'a, 'scene: 'a>(
        layers: impl IntoIterator<Item = PreparedRawStageLayer<'a, 'scene>>,
        extent: DisplayExtent,
        active_palette: &[PaletteEntry; 256],
    ) -> Result<Self, PreparedRawStageError> {
        let mut meshes = Vec::new();
        let mut prepared_textures = Vec::new();
        let mut texture_by_ordinal = BTreeMap::<(usize, i16), u32>::new();
        let mut draws = Vec::new();
        let mut dynamic_draws = Vec::new();
        let mut dynamic_world_set_count = 0;
        let mut stats = PreparedRawStageStats::default();

        for (layer_index, layer) in layers.into_iter().enumerate() {
            if let Some(world_set) = layer.dynamic_world_set {
                dynamic_world_set_count = dynamic_world_set_count.max(world_set.saturating_add(1));
            }
            for op in layer.scene.draw_order() {
                let Some(mesh) = layer.scene.mesh(op.node) else {
                    continue;
                };
                let Some(draw_group) = mesh.active() else {
                    stats.empty_lod_nodes += 1;
                    continue;
                };
                for prim in draw_group.prims() {
                    let ordinal = prim.selector();
                    let Some(sampled) = layer.textures.texture_for_ordinal(ordinal) else {
                        stats.unresolved_prims += 1;
                        continue;
                    };

                    let key = (layer_index, ordinal);
                    let texture_id = if let Some(&existing) = texture_by_ordinal.get(&key) {
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
                            layer.color_key_index,
                        )?;
                        let id = u32::try_from(prepared_textures.len()).map_err(|_| {
                            PreparedRawStageError::ResourceCountOverflow {
                                resource: "texture",
                                count: prepared_textures.len(),
                            }
                        })?;
                        prepared_textures.push(texture);
                        texture_by_ordinal.insert(key, id);
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
                    let local_world = *layer.scene.world(op.node);
                    let draw_index = draws.len();
                    draws.push(C3dDraw::new(
                        mesh_id,
                        texture_id,
                        world_matrix_words(&local_world),
                        if op.deferred_translucent {
                            C3dSubmissionBracket::DeferredTranslucent
                        } else {
                            C3dSubmissionBracket::Normal
                        },
                    )?);
                    if let Some(world_set) = layer.dynamic_world_set {
                        dynamic_draws.push(DynamicDraw {
                            draw: draw_index,
                            world_set,
                            node: op.node,
                        });
                    }
                }
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
            dynamic_draws: dynamic_draws.into_boxed_slice(),
            dynamic_world_set_count,
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
        assert_eq!(
            self.dynamic_world_set_count, 0,
            "dynamic foreground requires prepared_frame_for_worlds"
        );
        self.borrow_prepared_frame(camera_world_q16)
    }

    /// Updates dynamic-layer WORLD matrices from exact per-node propagation and
    /// borrows the resulting frame.
    pub fn prepared_frame_for_worlds(
        &mut self,
        camera_world_q16: &[i32; 16],
        dynamic_worlds: &[&[[i32; 16]]],
    ) -> Result<C3dPreparedFrame<'_>, PreparedRawStageError> {
        if dynamic_worlds.len() != self.dynamic_world_set_count {
            return Err(PreparedRawStageError::DynamicWorldSetCount {
                expected: self.dynamic_world_set_count,
                actual: dynamic_worlds.len(),
            });
        }
        for dynamic in self.dynamic_draws.iter().copied() {
            let old = self.draws[dynamic.draw];
            let world = dynamic_worlds[dynamic.world_set].get(dynamic.node).ok_or(
                PreparedRawStageError::DynamicWorldNodeCount {
                    world_set: dynamic.world_set,
                    required_node: dynamic.node,
                    actual: dynamic_worlds[dynamic.world_set].len(),
                },
            )?;
            self.draws[dynamic.draw] = C3dDraw::new(
                old.mesh(),
                old.texture(),
                world_matrix_words(world),
                old.submission(),
            )?;
        }
        Ok(self.borrow_prepared_frame(camera_world_q16))
    }

    fn borrow_prepared_frame(&self, camera_world_q16: &[i32; 16]) -> C3dPreparedFrame<'_> {
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

    #[must_use]
    pub const fn dynamic_world_set_count(&self) -> usize {
        self.dynamic_world_set_count
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
    DynamicWorldSetCount {
        expected: usize,
        actual: usize,
    },
    DynamicWorldNodeCount {
        world_set: usize,
        required_node: usize,
        actual: usize,
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
            Self::ResourceCountOverflow { .. }
            | Self::DynamicWorldSetCount { .. }
            | Self::DynamicWorldNodeCount { .. } => None,
        }
    }
}
