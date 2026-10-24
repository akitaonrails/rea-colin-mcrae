//! Raw recovered C3D opaque-COPY implementation for SDL_GPU/Vulkan.

use std::time::Instant;

use cmr_gpu_contract::{
    C3dPreparedFrame, C3dPreparedSceneLayout, C3dRawFrame, C3dSceneUpload, CanonicalRgba8Frame,
    ConformanceFrame, FrameTiming, PersistentRawC3dGpuBackend, RawC3dGpuBackend, ReadbackFormat,
    c3d_active_palette_l1_map, normalize_readback,
};
use sdl3::gpu::{
    Buffer, BufferBinding, BufferRegion, BufferUsageFlags, ColorTargetInfo, DepthStencilTargetInfo,
    IndexElementSize, LoadOp, StoreOp, Texture, TextureCreateInfo, TextureFormat, TextureRegion,
    TextureSamplerBinding, TextureTransferInfo, TextureType, TextureUsage, TransferBuffer,
    TransferBufferLocation, TransferBufferUsage,
};
use sdl3::pixels::Color;

use super::{
    PALETTE_BYTES, READBACK_ROW_ALIGNMENT, SdlGpuError, SdlGpuIndexedBackend, TRANSFER_ALIGNMENT,
    align_up, elapsed_ns, raw, u32_len,
};

const C3D_VERTEX_BYTES: usize = 32;

struct GpuMesh {
    vertices: Buffer,
    indices: Buffer,
    index_count: u32,
}

struct GpuMaterial {
    indices: Texture<'static>,
    palette: Texture<'static>,
}

#[repr(C)]
struct TransformUniform {
    world: [u32; 16],
    view: [u32; 16],
    projection: [u32; 16],
    clip_parameters: [u32; 4],
}

impl RawC3dGpuBackend for SdlGpuIndexedBackend {
    type Error = SdlGpuError;

    fn info(&self) -> &cmr_gpu_contract::BackendInfo {
        &self.info
    }

    fn capabilities(&self) -> cmr_gpu_contract::BackendCapabilities {
        <Self as cmr_gpu_contract::IndexedPaletteGpuBackend>::capabilities(self)
    }

    fn render_c3d_readback(
        &mut self,
        frame: C3dRawFrame<'_>,
    ) -> Result<ConformanceFrame, Self::Error> {
        if !raw::texture_supports(
            &self.device,
            TextureFormat::D32Float,
            TextureType::_2D,
            TextureUsage::DEPTH_STENCIL_TARGET,
        ) {
            return Err(SdlGpuError::UnsupportedFormat("D32_FLOAT depth target"));
        }
        // Palette quantization belongs to immutable material preparation in a
        // real stage session, not the measured per-frame draw interval.
        let mapped_palettes = frame
            .resources()
            .textures()
            .iter()
            .map(|texture| mapped_palette_rgba(texture, frame.active_palette()))
            .collect::<Vec<_>>();

        let extent = frame.extent();
        let width = u32::from(extent.width);
        let height = u32::from(extent.height);
        let encode_start = Instant::now();
        let tight_pitch = usize::from(extent.width)
            .checked_mul(4)
            .ok_or(SdlGpuError::SizeOverflow)?;
        let readback_pitch = align_up(tight_pitch, READBACK_ROW_ALIGNMENT)?;
        let readback_len = readback_pitch
            .checked_mul(usize::from(extent.height))
            .ok_or(SdlGpuError::SizeOverflow)?;

        let mut uploads: Vec<TransferBuffer> = Vec::new();
        let mut meshes = Vec::with_capacity(frame.resources().meshes().len());
        let command = self
            .device
            .acquire_command_buffer()
            .map_err(|error| SdlGpuError::sdl("C3D command buffer acquisition", error))?;
        let copy = self
            .device
            .begin_copy_pass(&command)
            .map_err(|error| SdlGpuError::sdl("C3D upload pass", error))?;

        for mesh in frame.resources().meshes() {
            let vertex_bytes = encode_vertices(mesh.vertices());
            let index_bytes = encode_indices(mesh.indices());
            let index_offset = align_up(vertex_bytes.len(), TRANSFER_ALIGNMENT)?;
            let upload_len = index_offset
                .checked_add(index_bytes.len())
                .ok_or(SdlGpuError::SizeOverflow)?;
            let upload = self
                .device
                .create_transfer_buffer()
                .with_size(u32_len(upload_len)?)
                .with_usage(TransferBufferUsage::UPLOAD)
                .build()
                .map_err(|error| SdlGpuError::sdl("C3D mesh upload buffer creation", error))?;
            raw::write_transfer(
                &self.device,
                &upload,
                upload_len,
                &[(0, &vertex_bytes), (index_offset, &index_bytes)],
            )?;
            let vertices = self
                .device
                .create_buffer()
                .with_size(u32_len(vertex_bytes.len())?)
                .with_usage(BufferUsageFlags::VERTEX)
                .build()
                .map_err(|error| SdlGpuError::sdl("C3D vertex buffer creation", error))?;
            let indices = self
                .device
                .create_buffer()
                .with_size(u32_len(index_bytes.len())?)
                .with_usage(BufferUsageFlags::INDEX)
                .build()
                .map_err(|error| SdlGpuError::sdl("C3D index buffer creation", error))?;
            copy.upload_to_gpu_buffer(
                TransferBufferLocation::new()
                    .with_transfer_buffer(&upload)
                    .with_offset(0),
                BufferRegion::new()
                    .with_buffer(&vertices)
                    .with_offset(0)
                    .with_size(u32_len(vertex_bytes.len())?),
                false,
            );
            copy.upload_to_gpu_buffer(
                TransferBufferLocation::new()
                    .with_transfer_buffer(&upload)
                    .with_offset(u32_len(index_offset)?),
                BufferRegion::new()
                    .with_buffer(&indices)
                    .with_offset(0)
                    .with_size(u32_len(index_bytes.len())?),
                false,
            );
            meshes.push(GpuMesh {
                vertices,
                indices,
                index_count: u32_len(mesh.indices().len())?,
            });
            uploads.push(upload);
        }

        let mut materials = Vec::with_capacity(frame.resources().textures().len());
        for (texture, palette_bytes) in frame.resources().textures().iter().zip(&mapped_palettes) {
            let texture_width = u32::from(texture.extent().width);
            let texture_height = u32::from(texture.extent().height);
            let palette_offset = align_up(texture.indices().len(), TRANSFER_ALIGNMENT)?;
            let upload_len = palette_offset
                .checked_add(PALETTE_BYTES)
                .ok_or(SdlGpuError::SizeOverflow)?;
            let upload = self
                .device
                .create_transfer_buffer()
                .with_size(u32_len(upload_len)?)
                .with_usage(TransferBufferUsage::UPLOAD)
                .build()
                .map_err(|error| SdlGpuError::sdl("C3D texture upload buffer creation", error))?;
            raw::write_transfer(
                &self.device,
                &upload,
                upload_len,
                &[(0, texture.indices()), (palette_offset, palette_bytes)],
            )?;
            let indices = self
                .device
                .create_texture(
                    TextureCreateInfo::new()
                        .with_format(TextureFormat::R8Uint)
                        .with_type(TextureType::_2D)
                        .with_width(texture_width)
                        .with_height(texture_height)
                        .with_layer_count_or_depth(1)
                        .with_num_levels(1)
                        .with_usage(TextureUsage::SAMPLER),
                )
                .map_err(|error| SdlGpuError::sdl("C3D index texture creation", error))?;
            let palette = self
                .device
                .create_texture(
                    TextureCreateInfo::new()
                        .with_format(TextureFormat::R8g8b8a8Uint)
                        .with_type(TextureType::_2D)
                        .with_width(256)
                        .with_height(1)
                        .with_layer_count_or_depth(1)
                        .with_num_levels(1)
                        .with_usage(TextureUsage::SAMPLER),
                )
                .map_err(|error| SdlGpuError::sdl("C3D palette texture creation", error))?;
            copy.upload_to_gpu_texture(
                TextureTransferInfo::new()
                    .with_transfer_buffer(&upload)
                    .with_pixels_per_row(texture_width)
                    .with_rows_per_layer(texture_height),
                TextureRegion::new()
                    .with_texture(&indices)
                    .with_width(texture_width)
                    .with_height(texture_height)
                    .with_depth(1),
                false,
            );
            copy.upload_to_gpu_texture(
                TextureTransferInfo::new()
                    .with_transfer_buffer(&upload)
                    .with_offset(u32_len(palette_offset)?)
                    .with_pixels_per_row(256)
                    .with_rows_per_layer(1),
                TextureRegion::new()
                    .with_texture(&palette)
                    .with_width(256)
                    .with_height(1)
                    .with_depth(1),
                false,
            );
            materials.push(GpuMaterial { indices, palette });
            uploads.push(upload);
        }
        self.device.end_copy_pass(copy);

        let target = self
            .device
            .create_texture(
                TextureCreateInfo::new()
                    .with_format(TextureFormat::R8g8b8a8Unorm)
                    .with_type(TextureType::_2D)
                    .with_width(width)
                    .with_height(height)
                    .with_layer_count_or_depth(1)
                    .with_num_levels(1)
                    .with_usage(TextureUsage::COLOR_TARGET),
            )
            .map_err(|error| SdlGpuError::sdl("C3D target texture creation", error))?;
        let mut depth = self
            .device
            .create_texture(
                TextureCreateInfo::new()
                    .with_format(TextureFormat::D32Float)
                    .with_type(TextureType::_2D)
                    .with_width(width)
                    .with_height(height)
                    .with_layer_count_or_depth(1)
                    .with_num_levels(1)
                    .with_usage(TextureUsage::DEPTH_STENCIL_TARGET),
            )
            .map_err(|error| SdlGpuError::sdl("C3D depth texture creation", error))?;
        let download = self
            .device
            .create_transfer_buffer()
            .with_size(u32_len(readback_len)?)
            .with_usage(TransferBufferUsage::DOWNLOAD)
            .build()
            .map_err(|error| SdlGpuError::sdl("C3D download buffer creation", error))?;

        let clear = frame.active_palette()[0];
        let color_targets = [ColorTargetInfo::default()
            .with_texture(&target)
            .with_clear_color(Color::RGBA(clear.red, clear.green, clear.blue, u8::MAX))
            .with_load_op(LoadOp::CLEAR)
            .with_store_op(StoreOp::STORE)];
        let depth_target = DepthStencilTargetInfo::new()
            .with_texture(&mut depth)
            .with_clear_depth(1.0)
            .with_load_op(LoadOp::CLEAR)
            .with_store_op(StoreOp::STORE);
        let render = self
            .device
            .begin_render_pass(&command, &color_targets, Some(&depth_target))
            .map_err(|error| SdlGpuError::sdl("C3D render pass", error))?;
        render.bind_graphics_pipeline(&self.c3d_pipeline);
        for draw in frame.draws().iter().copied() {
            let mesh = &meshes[draw.mesh() as usize];
            let material = &materials[draw.texture() as usize];
            render.bind_vertex_buffers(
                0,
                &[BufferBinding::new()
                    .with_buffer(&mesh.vertices)
                    .with_offset(0)],
            );
            render.bind_index_buffer(
                &BufferBinding::new()
                    .with_buffer(&mesh.indices)
                    .with_offset(0),
                IndexElementSize::_16BIT,
            );
            render.bind_fragment_samplers(
                0,
                &[
                    TextureSamplerBinding::new()
                        .with_texture(&material.indices)
                        .with_sampler(&self.sampler),
                    TextureSamplerBinding::new()
                        .with_texture(&material.palette)
                        .with_sampler(&self.sampler),
                ],
            );
            command.push_vertex_uniform_data(
                0,
                &TransformUniform {
                    world: draw.world().words(),
                    view: frame.view().words(),
                    projection: frame.projection().words(),
                    clip_parameters: [
                        (f32::from(extent.width) / f32::from(extent.height)).to_bits(),
                        0,
                        0,
                        0,
                    ],
                },
            );
            render.draw_indexed_primitives(mesh.index_count, 1, 0, 0, 0);
        }
        self.device.end_render_pass(render);

        let copy = self
            .device
            .begin_copy_pass(&command)
            .map_err(|error| SdlGpuError::sdl("C3D download pass", error))?;
        raw::download_texture(
            &copy,
            &target,
            &download,
            width,
            height,
            u32_len(readback_pitch / 4)?,
        );
        self.device.end_copy_pass(copy);
        let fence = command
            .submit_and_acquire_fence(&self.device)
            .map_err(|error| SdlGpuError::sdl("C3D command submission", error))?;
        let cpu_encode_submit_ns = elapsed_ns(encode_start);
        let completion_start = Instant::now();
        self.device
            .wait_fences(true, &[fence])
            .map_err(|error| SdlGpuError::sdl("C3D fence wait", error))?;
        let submit_to_completion_ns = elapsed_ns(completion_start);

        // Keep uploads live until the submitted copy work has completed.
        drop(uploads);
        let mapped = raw::read_download(&self.device, &download, readback_len)?;
        let rgba8 = normalize_readback(extent, &mapped, readback_pitch, ReadbackFormat::Rgba8)
            .map_err(SdlGpuError::FrameContract)?;
        Ok(ConformanceFrame {
            rgba8,
            timing: FrameTiming {
                cpu_encode_submit_ns,
                submit_to_completion_ns,
                gpu_timestamp_ns: None,
            },
        })
    }
}

/// Native-resource operations performed over a prepared scene's lifetime.
///
/// The steady-state timed path must leave every counter unchanged.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SdlGpuPreparedC3dStats {
    pub buffer_creations: usize,
    pub texture_creations: usize,
    pub transfer_buffer_creations: usize,
    pub upload_submissions: usize,
    pub readback_copies: usize,
    pub readback_maps: usize,
}

/// Opaque SDL_GPU/Vulkan realization of one immutable raw-C3D scene.
pub struct SdlGpuPreparedC3dScene {
    backend_token: u64,
    layout: C3dPreparedSceneLayout,
    meshes: Vec<GpuMesh>,
    materials: Vec<GpuMaterial>,
    target: Texture<'static>,
    depth: Texture<'static>,
    download: TransferBuffer,
    readback_pitch: usize,
    readback_len: usize,
    clear: Color,
    stats: SdlGpuPreparedC3dStats,
    poisoned: bool,
    // SDL resource wrappers retain weak device references. Keep a strong
    // device owner last so every preceding native resource releases first.
    _device: sdl3::gpu::Device,
}

impl SdlGpuPreparedC3dScene {
    #[must_use]
    pub const fn stats(&self) -> SdlGpuPreparedC3dStats {
        self.stats
    }
}

impl PersistentRawC3dGpuBackend for SdlGpuIndexedBackend {
    type Error = SdlGpuError;
    type PreparedScene = SdlGpuPreparedC3dScene;

    fn info(&self) -> &cmr_gpu_contract::BackendInfo {
        &self.info
    }

    fn capabilities(&self) -> cmr_gpu_contract::BackendCapabilities {
        <Self as cmr_gpu_contract::IndexedPaletteGpuBackend>::capabilities(self)
    }

    fn prepare_c3d_scene(
        &mut self,
        source: C3dSceneUpload<'_>,
    ) -> Result<Self::PreparedScene, Self::Error> {
        if !raw::texture_supports(
            &self.device,
            TextureFormat::D32Float,
            TextureType::_2D,
            TextureUsage::DEPTH_STENCIL_TARGET,
        ) {
            return Err(SdlGpuError::UnsupportedFormat("D32_FLOAT depth target"));
        }
        let layout = source.layout();
        let extent = layout.extent();
        let width = u32::from(extent.width);
        let height = u32::from(extent.height);
        let tight_pitch = usize::from(extent.width)
            .checked_mul(4)
            .ok_or(SdlGpuError::SizeOverflow)?;
        let readback_pitch = align_up(tight_pitch, READBACK_ROW_ALIGNMENT)?;
        let readback_len = readback_pitch
            .checked_mul(usize::from(extent.height))
            .ok_or(SdlGpuError::SizeOverflow)?;
        let mapped_palettes = source
            .resources()
            .textures()
            .iter()
            .map(|texture| mapped_palette_rgba(texture, source.active_palette()))
            .collect::<Vec<_>>();

        let mut uploads: Vec<TransferBuffer> = Vec::new();
        let mut meshes = Vec::with_capacity(source.resources().meshes().len());
        let mut materials = Vec::with_capacity(source.resources().textures().len());
        let mut stats = SdlGpuPreparedC3dStats::default();
        let command = self
            .device
            .acquire_command_buffer()
            .map_err(|error| SdlGpuError::sdl("prepared C3D command acquisition", error))?;
        let copy = self
            .device
            .begin_copy_pass(&command)
            .map_err(|error| SdlGpuError::sdl("prepared C3D upload pass", error))?;

        for mesh in source.resources().meshes() {
            let vertex_bytes = encode_vertices(mesh.vertices());
            let index_bytes = encode_indices(mesh.indices());
            let index_offset = align_up(vertex_bytes.len(), TRANSFER_ALIGNMENT)?;
            let upload_len = index_offset
                .checked_add(index_bytes.len())
                .ok_or(SdlGpuError::SizeOverflow)?;
            let upload = self
                .device
                .create_transfer_buffer()
                .with_size(u32_len(upload_len)?)
                .with_usage(TransferBufferUsage::UPLOAD)
                .build()
                .map_err(|error| SdlGpuError::sdl("prepared mesh upload creation", error))?;
            raw::write_transfer(
                &self.device,
                &upload,
                upload_len,
                &[(0, &vertex_bytes), (index_offset, &index_bytes)],
            )?;
            let vertices = self
                .device
                .create_buffer()
                .with_size(u32_len(vertex_bytes.len())?)
                .with_usage(BufferUsageFlags::VERTEX)
                .build()
                .map_err(|error| SdlGpuError::sdl("prepared vertex buffer creation", error))?;
            let indices = self
                .device
                .create_buffer()
                .with_size(u32_len(index_bytes.len())?)
                .with_usage(BufferUsageFlags::INDEX)
                .build()
                .map_err(|error| SdlGpuError::sdl("prepared index buffer creation", error))?;
            copy.upload_to_gpu_buffer(
                TransferBufferLocation::new()
                    .with_transfer_buffer(&upload)
                    .with_offset(0),
                BufferRegion::new()
                    .with_buffer(&vertices)
                    .with_offset(0)
                    .with_size(u32_len(vertex_bytes.len())?),
                false,
            );
            copy.upload_to_gpu_buffer(
                TransferBufferLocation::new()
                    .with_transfer_buffer(&upload)
                    .with_offset(u32_len(index_offset)?),
                BufferRegion::new()
                    .with_buffer(&indices)
                    .with_offset(0)
                    .with_size(u32_len(index_bytes.len())?),
                false,
            );
            meshes.push(GpuMesh {
                vertices,
                indices,
                index_count: u32_len(mesh.indices().len())?,
            });
            uploads.push(upload);
            stats.buffer_creations += 2;
            stats.transfer_buffer_creations += 1;
        }

        for (texture, palette_bytes) in source.resources().textures().iter().zip(&mapped_palettes) {
            let texture_width = u32::from(texture.extent().width);
            let texture_height = u32::from(texture.extent().height);
            let palette_offset = align_up(texture.indices().len(), TRANSFER_ALIGNMENT)?;
            let upload_len = palette_offset
                .checked_add(PALETTE_BYTES)
                .ok_or(SdlGpuError::SizeOverflow)?;
            let upload = self
                .device
                .create_transfer_buffer()
                .with_size(u32_len(upload_len)?)
                .with_usage(TransferBufferUsage::UPLOAD)
                .build()
                .map_err(|error| SdlGpuError::sdl("prepared texture upload creation", error))?;
            raw::write_transfer(
                &self.device,
                &upload,
                upload_len,
                &[(0, texture.indices()), (palette_offset, palette_bytes)],
            )?;
            let indices = self
                .device
                .create_texture(
                    TextureCreateInfo::new()
                        .with_format(TextureFormat::R8Uint)
                        .with_type(TextureType::_2D)
                        .with_width(texture_width)
                        .with_height(texture_height)
                        .with_layer_count_or_depth(1)
                        .with_num_levels(1)
                        .with_usage(TextureUsage::SAMPLER),
                )
                .map_err(|error| SdlGpuError::sdl("prepared index texture creation", error))?;
            let palette = self
                .device
                .create_texture(
                    TextureCreateInfo::new()
                        .with_format(TextureFormat::R8g8b8a8Uint)
                        .with_type(TextureType::_2D)
                        .with_width(256)
                        .with_height(1)
                        .with_layer_count_or_depth(1)
                        .with_num_levels(1)
                        .with_usage(TextureUsage::SAMPLER),
                )
                .map_err(|error| SdlGpuError::sdl("prepared palette texture creation", error))?;
            copy.upload_to_gpu_texture(
                TextureTransferInfo::new()
                    .with_transfer_buffer(&upload)
                    .with_pixels_per_row(texture_width)
                    .with_rows_per_layer(texture_height),
                TextureRegion::new()
                    .with_texture(&indices)
                    .with_width(texture_width)
                    .with_height(texture_height)
                    .with_depth(1),
                false,
            );
            copy.upload_to_gpu_texture(
                TextureTransferInfo::new()
                    .with_transfer_buffer(&upload)
                    .with_offset(u32_len(palette_offset)?)
                    .with_pixels_per_row(256)
                    .with_rows_per_layer(1),
                TextureRegion::new()
                    .with_texture(&palette)
                    .with_width(256)
                    .with_height(1)
                    .with_depth(1),
                false,
            );
            materials.push(GpuMaterial { indices, palette });
            uploads.push(upload);
            stats.texture_creations += 2;
            stats.transfer_buffer_creations += 1;
        }
        self.device.end_copy_pass(copy);
        let fence = command
            .submit_and_acquire_fence(&self.device)
            .map_err(|error| SdlGpuError::sdl("prepared C3D upload submission", error))?;
        self.device
            .wait_fences(true, &[fence])
            .map_err(|error| SdlGpuError::sdl("prepared C3D upload wait", error))?;
        stats.upload_submissions = 1;
        drop(uploads);

        let target = self
            .device
            .create_texture(
                TextureCreateInfo::new()
                    .with_format(TextureFormat::R8g8b8a8Unorm)
                    .with_type(TextureType::_2D)
                    .with_width(width)
                    .with_height(height)
                    .with_layer_count_or_depth(1)
                    .with_num_levels(1)
                    .with_usage(TextureUsage::COLOR_TARGET),
            )
            .map_err(|error| SdlGpuError::sdl("prepared color target creation", error))?;
        let depth = self
            .device
            .create_texture(
                TextureCreateInfo::new()
                    .with_format(TextureFormat::D32Float)
                    .with_type(TextureType::_2D)
                    .with_width(width)
                    .with_height(height)
                    .with_layer_count_or_depth(1)
                    .with_num_levels(1)
                    .with_usage(TextureUsage::DEPTH_STENCIL_TARGET),
            )
            .map_err(|error| SdlGpuError::sdl("prepared depth target creation", error))?;
        let download = self
            .device
            .create_transfer_buffer()
            .with_size(u32_len(readback_len)?)
            .with_usage(TransferBufferUsage::DOWNLOAD)
            .build()
            .map_err(|error| SdlGpuError::sdl("prepared download creation", error))?;
        stats.texture_creations += 2;
        stats.transfer_buffer_creations += 1;
        let clear = source.active_palette()[0];
        Ok(SdlGpuPreparedC3dScene {
            backend_token: self.c3d_backend_token,
            layout,
            meshes,
            materials,
            target,
            depth,
            download,
            readback_pitch,
            readback_len,
            clear: Color::RGBA(clear.red, clear.green, clear.blue, u8::MAX),
            stats,
            poisoned: false,
            _device: self.device.clone(),
        })
    }

    fn prepared_layout(scene: &Self::PreparedScene) -> C3dPreparedSceneLayout {
        scene.layout
    }

    fn render_prepared_readback(
        &mut self,
        scene: &mut Self::PreparedScene,
        frame: C3dPreparedFrame<'_>,
    ) -> Result<CanonicalRgba8Frame, Self::Error> {
        let (_, rgba8) = self.render_prepared(scene, frame, true)?;
        rgba8.ok_or(SdlGpuError::SizeOverflow)
    }

    fn render_prepared_timed(
        &mut self,
        scene: &mut Self::PreparedScene,
        frame: C3dPreparedFrame<'_>,
    ) -> Result<FrameTiming, Self::Error> {
        let (timing, rgba8) = self.render_prepared(scene, frame, false)?;
        debug_assert!(rgba8.is_none());
        Ok(timing)
    }
}

impl SdlGpuIndexedBackend {
    fn render_prepared(
        &mut self,
        scene: &mut SdlGpuPreparedC3dScene,
        frame: C3dPreparedFrame<'_>,
        readback: bool,
    ) -> Result<(FrameTiming, Option<CanonicalRgba8Frame>), SdlGpuError> {
        if scene.backend_token != self.c3d_backend_token {
            return Err(SdlGpuError::PreparedSceneBackendMismatch);
        }
        if scene.poisoned {
            return Err(SdlGpuError::PreparedScenePoisoned);
        }
        frame
            .ensure_layout(scene.layout)
            .map_err(SdlGpuError::RawC3dContract)?;
        let extent = scene.layout.extent();
        let width = u32::from(extent.width);
        let height = u32::from(extent.height);
        let encode_start = Instant::now();
        let command = self
            .device
            .acquire_command_buffer()
            .map_err(|error| SdlGpuError::sdl("prepared frame command acquisition", error))?;
        let color_targets = [ColorTargetInfo::default()
            .with_texture(&scene.target)
            .with_clear_color(scene.clear)
            .with_load_op(LoadOp::CLEAR)
            .with_store_op(StoreOp::STORE)];
        let mut depth = scene.depth.clone();
        let depth_target = DepthStencilTargetInfo::new()
            .with_texture(&mut depth)
            .with_clear_depth(1.0)
            .with_load_op(LoadOp::CLEAR)
            .with_store_op(StoreOp::STORE);
        let render = self
            .device
            .begin_render_pass(&command, &color_targets, Some(&depth_target))
            .map_err(|error| SdlGpuError::sdl("prepared frame render pass", error))?;
        render.bind_graphics_pipeline(&self.c3d_pipeline);
        for draw in frame.draws().iter().copied() {
            let mesh = &scene.meshes[draw.mesh() as usize];
            let material = &scene.materials[draw.texture() as usize];
            render.bind_vertex_buffers(
                0,
                &[BufferBinding::new()
                    .with_buffer(&mesh.vertices)
                    .with_offset(0)],
            );
            render.bind_index_buffer(
                &BufferBinding::new()
                    .with_buffer(&mesh.indices)
                    .with_offset(0),
                IndexElementSize::_16BIT,
            );
            render.bind_fragment_samplers(
                0,
                &[
                    TextureSamplerBinding::new()
                        .with_texture(&material.indices)
                        .with_sampler(&self.sampler),
                    TextureSamplerBinding::new()
                        .with_texture(&material.palette)
                        .with_sampler(&self.sampler),
                ],
            );
            command.push_vertex_uniform_data(
                0,
                &TransformUniform {
                    world: draw.world().words(),
                    view: frame.view().words(),
                    projection: frame.projection().words(),
                    clip_parameters: [
                        (f32::from(extent.width) / f32::from(extent.height)).to_bits(),
                        0,
                        0,
                        0,
                    ],
                },
            );
            render.draw_indexed_primitives(mesh.index_count, 1, 0, 0, 0);
        }
        self.device.end_render_pass(render);
        if readback {
            let copy = self
                .device
                .begin_copy_pass(&command)
                .map_err(|error| SdlGpuError::sdl("prepared frame download pass", error))?;
            raw::download_texture(
                &copy,
                &scene.target,
                &scene.download,
                width,
                height,
                u32_len(scene.readback_pitch / 4)?,
            );
            self.device.end_copy_pass(copy);
            scene.stats.readback_copies += 1;
        }
        let fence = command
            .submit_and_acquire_fence(&self.device)
            .map_err(|error| SdlGpuError::sdl("prepared frame submission", error))?;
        let cpu_encode_submit_ns = elapsed_ns(encode_start);
        let completion_start = Instant::now();
        if let Err(error) = self.device.wait_fences(true, &[fence]) {
            scene.poisoned = true;
            return Err(SdlGpuError::sdl("prepared frame fence wait", error));
        }
        let submit_to_completion_ns = elapsed_ns(completion_start);
        let timing = FrameTiming {
            cpu_encode_submit_ns,
            submit_to_completion_ns,
            gpu_timestamp_ns: None,
        };
        if !readback {
            return Ok((timing, None));
        }
        let mapped = raw::read_download(&self.device, &scene.download, scene.readback_len)?;
        scene.stats.readback_maps += 1;
        let rgba8 =
            normalize_readback(extent, &mapped, scene.readback_pitch, ReadbackFormat::Rgba8)
                .map_err(SdlGpuError::FrameContract)?;
        Ok((timing, Some(rgba8)))
    }
}

fn encode_vertices(vertices: &[cmr_gpu_contract::C3dVertexQ16]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vertices.len() * C3D_VERTEX_BYTES);
    for vertex in vertices {
        for component in vertex.components {
            bytes.extend_from_slice(&component.to_le_bytes());
        }
    }
    bytes
}

fn encode_indices(indices: &[u16]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(std::mem::size_of_val(indices));
    for &index in indices {
        bytes.extend_from_slice(&index.to_le_bytes());
    }
    bytes
}

fn mapped_palette_rgba(
    texture: &cmr_gpu_contract::C3dIndexedTexture,
    active: &[cmr_formats::riff_palette::PaletteEntry; 256],
) -> [u8; PALETTE_BYTES] {
    let mapping = c3d_active_palette_l1_map(texture, active);
    let fallback = active[0];
    let mut bytes = [0u8; PALETTE_BYTES];
    for index in 0..256 {
        let color = mapping
            .get(index)
            .map_or(fallback, |&mapped| active[usize::from(mapped)]);
        bytes[index * 4..index * 4 + 4].copy_from_slice(&[
            color.red,
            color.green,
            color.blue,
            u8::MAX,
        ]);
    }
    bytes
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use crate::SdlGpuConfig;
    use cmr_core::c3d_camera::{
        MATRIX_IDENTITY_Q16, PROJECTION_BASELINE, projection_matrix_words, view_matrix_words,
        world_matrix_words,
    };
    use cmr_core::display::DisplayExtent;
    use cmr_formats::riff_palette::PaletteEntry;
    use cmr_gpu_contract::{
        C3dDraw, C3dIndexedTexture, C3dMeshResource, C3dRawFrame, C3dResourceSet, C3dRgb8,
        C3dSubmissionBracket, C3dVertexQ16,
    };

    use super::*;

    static HARDWARE_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn raw_vertex_and_index_uploads_are_exact_little_endian() {
        let vertices = [C3dVertexQ16 {
            components: [0x0102_0304, -2, 0, 0x10000, -0x10000, i32::MIN, i32::MAX, 7],
        }];
        let encoded = encode_vertices(&vertices);
        assert_eq!(encoded.len(), 32);
        assert_eq!(&encoded[0..4], &[4, 3, 2, 1]);
        assert_eq!(&encoded[4..8], &(-2i32).to_le_bytes());
        assert_eq!(encode_indices(&[0x0102, 0xa0b0]), [2, 1, 0xb0, 0xa0]);
    }

    #[test]
    fn mapped_palette_uses_active_l1_colors_and_pads_opaque() {
        let texture = C3dIndexedTexture::new(
            DisplayExtent {
                width: 1,
                height: 1,
            },
            vec![0],
            vec![C3dRgb8 {
                red: 9,
                green: 20,
                blue: 30,
            }],
            None,
        )
        .unwrap();
        let mut active = [PaletteEntry {
            red: 0,
            green: 0,
            blue: 0,
            flags: 0,
        }; 256];
        active[0] = PaletteEntry {
            red: 1,
            green: 2,
            blue: 3,
            flags: 44,
        };
        active[7] = PaletteEntry {
            red: 10,
            green: 20,
            blue: 30,
            flags: 55,
        };
        let mapped = mapped_palette_rgba(&texture, &active);
        assert_eq!(&mapped[0..4], &[10, 20, 30, 255]);
        assert_eq!(&mapped[4..8], &[1, 2, 3, 255]);
    }

    fn palette() -> [PaletteEntry; 256] {
        let mut palette = [PaletteEntry {
            red: 0,
            green: 0,
            blue: 0,
            flags: 0,
        }; 256];
        palette[1] = PaletteEntry {
            red: 255,
            green: 0,
            blue: 0,
            flags: 0,
        };
        palette[2] = PaletteEntry {
            red: 0,
            green: 255,
            blue: 0,
            flags: 0,
        };
        palette
    }

    fn triangle(indices: [u16; 3]) -> C3dMeshResource {
        let normal = [0, 0, -(1 << 16)];
        C3dMeshResource::new(
            vec![
                C3dVertexQ16 {
                    components: [
                        -(4 << 16),
                        -(3 << 16),
                        0,
                        0,
                        0,
                        normal[0],
                        normal[1],
                        normal[2],
                    ],
                },
                C3dVertexQ16 {
                    components: [
                        -(4 << 16),
                        3 << 16,
                        0,
                        0,
                        0,
                        normal[0],
                        normal[1],
                        normal[2],
                    ],
                },
                C3dVertexQ16 {
                    components: [
                        4 << 16,
                        -(3 << 16),
                        0,
                        0,
                        0,
                        normal[0],
                        normal[1],
                        normal[2],
                    ],
                },
            ],
            indices.to_vec(),
        )
        .unwrap()
    }

    fn solid_texture(color: C3dRgb8) -> C3dIndexedTexture {
        C3dIndexedTexture::new(
            DisplayExtent {
                width: 1,
                height: 1,
            },
            vec![0],
            vec![color],
            None,
        )
        .unwrap()
    }

    fn matrices() -> (
        cmr_core::root_menu_c3d_transform::C3dMatrixWords,
        cmr_core::root_menu_c3d_transform::C3dMatrixWords,
    ) {
        let mut camera = MATRIX_IDENTITY_Q16;
        camera[14] = -(6 << 16);
        let (w, h, near, far) = PROJECTION_BASELINE;
        (
            view_matrix_words(&camera),
            projection_matrix_words(w, h, near, far),
        )
    }

    #[test]
    #[ignore = "requires SDL_VIDEO_DRIVER=offscreen and a Vulkan SDL_GPU device"]
    fn raw_triangle_draws_and_reversed_winding_is_culled() {
        let _serial = HARDWARE_TEST_LOCK.lock().unwrap();
        let extent = DisplayExtent {
            width: 64,
            height: 48,
        };
        let active = palette();
        let (view, projection) = matrices();
        let world = world_matrix_words(&MATRIX_IDENTITY_Q16);
        let draw = C3dDraw::new(0, 0, world, C3dSubmissionBracket::Normal).unwrap();

        let render = |mesh| {
            let resources = C3dResourceSet::new(
                vec![mesh],
                vec![solid_texture(C3dRgb8 {
                    red: 255,
                    green: 0,
                    blue: 0,
                })],
            );
            let draws = [draw];
            let frame =
                C3dRawFrame::new(extent, view, projection, &active, &resources, &draws).unwrap();
            let mut backend = SdlGpuIndexedBackend::new(SdlGpuConfig::default()).unwrap();
            backend.render_c3d_readback(frame).unwrap().rgba8
        };

        let visible = render(triangle([0, 1, 2]));
        assert!(
            visible
                .pixels()
                .as_chunks::<4>()
                .0
                .contains(&[255, 0, 0, 255])
        );
        let culled = render(triangle([0, 2, 1]));
        assert!(
            culled
                .pixels()
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| *pixel == [0, 0, 0, 255])
        );
    }

    #[test]
    #[ignore = "requires SDL_VIDEO_DRIVER=offscreen and a Vulkan SDL_GPU device"]
    fn active_palette_only_mutation_changes_c3d_readback() {
        let _serial = HARDWARE_TEST_LOCK.lock().unwrap();
        let extent = DisplayExtent {
            width: 64,
            height: 48,
        };
        let (view, projection) = matrices();
        let resources = C3dResourceSet::new(
            vec![triangle([0, 1, 2])],
            vec![solid_texture(C3dRgb8 {
                red: 255,
                green: 0,
                blue: 0,
            })],
        );
        let draws = [C3dDraw::new(
            0,
            0,
            world_matrix_words(&MATRIX_IDENTITY_Q16),
            C3dSubmissionBracket::Normal,
        )
        .unwrap()];
        let baseline_palette = palette();
        let mut mutated_palette = baseline_palette;
        // Red remains closer to this dark red than to the black fallback, so
        // the material-to-active mapping stays at index 1 while its RGB changes.
        mutated_palette[1].red = 200;
        let mut backend = SdlGpuIndexedBackend::new(SdlGpuConfig::default()).unwrap();
        let render = |backend: &mut SdlGpuIndexedBackend, active: &[PaletteEntry; 256]| {
            backend
                .render_c3d_readback(
                    C3dRawFrame::new(extent, view, projection, active, &resources, &draws).unwrap(),
                )
                .unwrap()
                .rgba8
        };
        let baseline = render(&mut backend, &baseline_palette);
        let mutated = render(&mut backend, &mutated_palette);
        assert_ne!(baseline, mutated);
        assert!(
            baseline
                .pixels()
                .as_chunks::<4>()
                .0
                .contains(&[255, 0, 0, 255])
        );
        assert!(
            mutated
                .pixels()
                .as_chunks::<4>()
                .0
                .contains(&[200, 0, 0, 255])
        );
    }

    #[test]
    #[ignore = "requires SDL_VIDEO_DRIVER=offscreen and a Vulkan SDL_GPU device"]
    fn depth_result_is_order_independent_and_near_wins() {
        let _serial = HARDWARE_TEST_LOCK.lock().unwrap();
        let extent = DisplayExtent {
            width: 64,
            height: 48,
        };
        let active = palette();
        let (view, projection) = matrices();
        let mut far_world = MATRIX_IDENTITY_Q16;
        far_world[14] = 8 << 16;
        let resources = C3dResourceSet::new(
            vec![triangle([0, 1, 2])],
            vec![
                solid_texture(C3dRgb8 {
                    red: 255,
                    green: 0,
                    blue: 0,
                }),
                solid_texture(C3dRgb8 {
                    red: 0,
                    green: 255,
                    blue: 0,
                }),
            ],
        );
        let near = C3dDraw::new(
            0,
            0,
            world_matrix_words(&MATRIX_IDENTITY_Q16),
            C3dSubmissionBracket::Normal,
        )
        .unwrap();
        let far = C3dDraw::new(
            0,
            1,
            world_matrix_words(&far_world),
            C3dSubmissionBracket::Normal,
        )
        .unwrap();
        let render = |draws: &[C3dDraw]| {
            let frame =
                C3dRawFrame::new(extent, view, projection, &active, &resources, draws).unwrap();
            let mut backend = SdlGpuIndexedBackend::new(SdlGpuConfig::default()).unwrap();
            backend.render_c3d_readback(frame).unwrap().rgba8
        };
        let near_then_far = render(&[near, far]);
        let far_then_near = render(&[far, near]);
        assert_eq!(near_then_far, far_then_near);
        assert!(
            near_then_far
                .pixels()
                .as_chunks::<4>()
                .0
                .contains(&[255, 0, 0, 255])
        );
        assert!(
            !near_then_far
                .pixels()
                .as_chunks::<4>()
                .0
                .contains(&[0, 255, 0, 255])
        );
    }

    #[test]
    #[ignore = "requires SDL_VIDEO_DRIVER=offscreen and a Vulkan SDL_GPU device"]
    fn prepared_scene_reuses_resources_and_timed_path_has_no_native_resource_operations() {
        let _serial = HARDWARE_TEST_LOCK.lock().unwrap();
        let extent = DisplayExtent {
            width: 64,
            height: 48,
        };
        let active = palette();
        let resources = C3dResourceSet::new(
            vec![triangle([0, 1, 2])],
            vec![solid_texture(C3dRgb8 {
                red: 255,
                green: 0,
                blue: 0,
            })],
        );
        let upload = C3dSceneUpload::new(extent, &resources, &active).unwrap();
        let mut backend = SdlGpuIndexedBackend::new(SdlGpuConfig::default()).unwrap();
        let mut prepared = backend.prepare_c3d_scene(upload).unwrap();
        assert_eq!(
            SdlGpuIndexedBackend::prepared_layout(&prepared),
            upload.layout()
        );
        let (view, projection) = matrices();
        let draws = [C3dDraw::new(
            0,
            0,
            world_matrix_words(&MATRIX_IDENTITY_Q16),
            C3dSubmissionBracket::Normal,
        )
        .unwrap()];
        let frame = C3dPreparedFrame::new(upload.layout(), view, projection, &draws).unwrap();

        let first = backend
            .render_prepared_readback(&mut prepared, frame)
            .unwrap();
        assert!(
            first
                .pixels()
                .as_chunks::<4>()
                .0
                .contains(&[255, 0, 0, 255])
        );
        let before_timed = prepared.stats();
        let timing = backend.render_prepared_timed(&mut prepared, frame).unwrap();
        assert!(timing.cpu_encode_submit_ns > 0);
        assert!(timing.submit_to_completion_ns > 0);
        assert_eq!(prepared.stats(), before_timed);
        let second = backend
            .render_prepared_readback(&mut prepared, frame)
            .unwrap();
        assert_eq!(first, second);
        assert_eq!(prepared.stats().readback_copies, 2);
        assert_eq!(prepared.stats().readback_maps, 2);

        let mut other = SdlGpuIndexedBackend::new(SdlGpuConfig::default()).unwrap();
        assert!(matches!(
            other.render_prepared_timed(&mut prepared, frame),
            Err(SdlGpuError::PreparedSceneBackendMismatch)
        ));
    }
}
