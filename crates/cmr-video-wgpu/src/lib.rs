//! wgpu implementation of the indexed-palette GPU conformance contract.
//!
//! This first slice is deliberately offscreen. It uploads an exact `R8Uint`
//! index plane and a 256-entry `Rgba8Uint` palette, resolves them with integer
//! `textureLoad` operations, and reads an `Rgba8Unorm` target back through a
//! driver-aligned buffer. No wgpu type crosses the shared contract boundary.

use std::fmt;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use cmr_gpu_contract::{
    BackendCapabilities, BackendInfo, BackendKind, ConformanceFrame, DeviceKind,
    FrameContractError, FrameTiming, GraphicsApi, IndexedFrameView, IndexedPaletteGpuBackend,
    ReadbackFormat, normalize_readback,
};

const PALETTE_LEN: u32 = 256;
const OUTPUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const COPY_ALIGNMENT: u32 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;

const SHADER: &str = r#"
@group(0) @binding(0) var indices: texture_2d<u32>;
@group(0) @binding(1) var palette: texture_2d<u32>;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> @builtin(position) vec4f {
    var positions = array<vec2f, 3>(
        vec2f(-1.0, -1.0),
        vec2f( 3.0, -1.0),
        vec2f(-1.0,  3.0),
    );
    return vec4f(positions[vertex_index], 0.0, 1.0);
}

@fragment
fn fragment_main(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let coordinate = vec2i(position.xy);
    let palette_index = textureLoad(indices, coordinate, 0).r;
    let color = textureLoad(palette, vec2i(i32(palette_index), 0), 0);
    return vec4f(vec3f(color.rgb) / 255.0, 1.0);
}
"#;

/// Native graphics API selected for the wgpu instance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WgpuApi {
    Vulkan,
    Direct3d12,
    OpenGl,
}

impl WgpuApi {
    const fn backends(self) -> wgpu::Backends {
        match self {
            Self::Vulkan => wgpu::Backends::VULKAN,
            Self::Direct3d12 => wgpu::Backends::DX12,
            Self::OpenGl => wgpu::Backends::GL,
        }
    }
}

/// Adapter class preference used when no exact name filter is supplied.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WgpuPowerPreference {
    LowPower,
    #[default]
    HighPerformance,
}

/// Explicit adapter selection for a headless wgpu backend.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WgpuBackendConfig {
    pub api: WgpuApi,
    pub adapter_name_contains: Option<String>,
    pub power_preference: WgpuPowerPreference,
}

impl Default for WgpuBackendConfig {
    fn default() -> Self {
        Self {
            api: WgpuApi::Vulkan,
            adapter_name_contains: None,
            power_preference: WgpuPowerPreference::HighPerformance,
        }
    }
}

/// Offscreen indexed-palette renderer backed by one selected wgpu adapter.
pub struct WgpuIndexedPaletteBackend {
    device: wgpu::Device,
    queue: wgpu::Queue,
    bind_group_layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    info: BackendInfo,
    capabilities: BackendCapabilities,
}

impl WgpuIndexedPaletteBackend {
    /// Enumerates only the requested API and selects an adapter deterministically.
    pub fn new(config: &WgpuBackendConfig) -> Result<Self, WgpuBackendError> {
        pollster::block_on(Self::new_async(config))
    }

    async fn new_async(config: &WgpuBackendConfig) -> Result<Self, WgpuBackendError> {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = config.api.backends();
        let instance = wgpu::Instance::new(descriptor);
        let mut adapters = instance.enumerate_adapters(config.api.backends()).await;
        if let Some(filter) = config.adapter_name_contains.as_deref() {
            let filter = filter.to_lowercase();
            adapters.retain(|adapter| adapter.get_info().name.to_lowercase().contains(&filter));
        }
        adapters.sort_by_key(|adapter| {
            adapter_rank(adapter.get_info().device_type, config.power_preference)
        });
        let adapter = adapters
            .into_iter()
            .next()
            .ok_or_else(|| WgpuBackendError::NoAdapter {
                api: config.api,
                name_filter: config.adapter_name_contains.clone(),
            })?;
        let native_info = adapter.get_info();
        let adapter_features = adapter.features();
        let sampled_r8_uint = supports_usage(
            &adapter,
            wgpu::TextureFormat::R8Uint,
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        );
        if !sampled_r8_uint {
            return Err(WgpuBackendError::MissingRequiredTextureRole(
                WgpuTextureRole::IndexedPlane,
            ));
        }
        if !supports_usage(
            &adapter,
            wgpu::TextureFormat::Rgba8Uint,
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        ) {
            return Err(WgpuBackendError::MissingRequiredTextureRole(
                WgpuTextureRole::Palette,
            ));
        }
        if !supports_usage(
            &adapter,
            OUTPUT_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        ) {
            return Err(WgpuBackendError::MissingRequiredTextureRole(
                WgpuTextureRole::ReadbackTarget,
            ));
        }
        let required_features = adapter_features & wgpu::Features::TIMESTAMP_QUERY;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("cmr indexed-palette conformance device"),
                required_features,
                required_limits: wgpu::Limits::default(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|error| WgpuBackendError::RequestDevice(error.to_string()))?;
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cmr indexed-palette bind group layout"),
            entries: &[texture_layout_entry(0), texture_layout_entry(1)],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cmr indexed-palette pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cmr indexed-palette shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cmr indexed-palette pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: OUTPUT_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let info = BackendInfo {
            backend: BackendKind::Wgpu,
            graphics_api: graphics_api(native_info.backend),
            vendor_id: (native_info.vendor != 0).then_some(native_info.vendor),
            device_id: (native_info.device != 0).then_some(native_info.device),
            device_kind: device_kind(native_info.device_type),
            adapter_name: native_info.name,
            driver_name: native_info.driver,
            driver_info: native_info.driver_info,
        };
        let capabilities = BackendCapabilities {
            sampled_r8_uint,
            timestamp_queries: adapter_features.contains(wgpu::Features::TIMESTAMP_QUERY),
            headless_readback: true,
            device_recovery: false,
        };
        Ok(Self {
            device,
            queue,
            bind_group_layout,
            pipeline,
            info,
            capabilities,
        })
    }
}

impl IndexedPaletteGpuBackend for WgpuIndexedPaletteBackend {
    type Error = WgpuBackendError;

    fn info(&self) -> &BackendInfo {
        &self.info
    }

    fn capabilities(&self) -> BackendCapabilities {
        self.capabilities
    }

    fn render_readback(
        &mut self,
        frame: IndexedFrameView<'_>,
    ) -> Result<ConformanceFrame, Self::Error> {
        let extent = frame.extent();
        let size = wgpu::Extent3d {
            width: u32::from(extent.width),
            height: u32::from(extent.height),
            depth_or_array_layers: 1,
        };
        let encode_started = Instant::now();
        let index_texture = create_texture(
            &self.device,
            "cmr indexed plane",
            size,
            wgpu::TextureFormat::R8Uint,
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        );
        self.queue.write_texture(
            texture_copy(&index_texture),
            frame.indices(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(u32::from(extent.width)),
                rows_per_image: Some(u32::from(extent.height)),
            },
            size,
        );
        let palette_texture = create_texture(
            &self.device,
            "cmr palette",
            wgpu::Extent3d {
                width: PALETTE_LEN,
                height: 1,
                depth_or_array_layers: 1,
            },
            wgpu::TextureFormat::Rgba8Uint,
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        );
        let mut palette_bytes = [0u8; PALETTE_LEN as usize * 4];
        let (palette_pixels, remainder) = palette_bytes.as_chunks_mut::<4>();
        debug_assert!(remainder.is_empty());
        for (bytes, entry) in palette_pixels.iter_mut().zip(frame.palette()) {
            bytes.copy_from_slice(&[entry.red, entry.green, entry.blue, u8::MAX]);
        }
        self.queue.write_texture(
            texture_copy(&palette_texture),
            &palette_bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(PALETTE_LEN * 4),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: PALETTE_LEN,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        let output_texture = create_texture(
            &self.device,
            "cmr conformance output",
            size,
            OUTPUT_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let index_view = index_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let palette_view = palette_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let output_view = output_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cmr indexed-palette bind group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&index_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&palette_view),
                },
            ],
        });
        let tight_pitch = u32::from(extent.width) * 4;
        let readback_pitch = tight_pitch.div_ceil(COPY_ALIGNMENT) * COPY_ALIGNMENT;
        let readback_size = u64::from(readback_pitch) * u64::from(extent.height);
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cmr conformance readback"),
            size: readback_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("cmr indexed-palette commands"),
            });
        {
            let color_attachment = Some(wgpu::RenderPassColorAttachment {
                view: &output_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("cmr indexed-palette pass"),
                color_attachments: &[color_attachment],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        encoder.copy_texture_to_buffer(
            texture_copy(&output_texture),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(readback_pitch),
                    rows_per_image: Some(u32::from(extent.height)),
                },
            },
            size,
        );
        let submission = self.queue.submit([encoder.finish()]);
        let cpu_encode_submit_ns = elapsed_ns(encode_started.elapsed());
        let completion_started = Instant::now();
        let (sender, receiver) = mpsc::sync_channel(1);
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = sender.send(result);
            });
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: None,
            })
            .map_err(|error| WgpuBackendError::Poll(error.to_string()))?;
        receiver
            .recv()
            .map_err(|error| WgpuBackendError::MapChannel(error.to_string()))?
            .map_err(|error| WgpuBackendError::Map(error.to_string()))?;
        let submit_to_completion_ns = elapsed_ns(completion_started.elapsed());
        let mapped = readback
            .slice(..)
            .get_mapped_range()
            .map_err(|error| WgpuBackendError::Map(error.to_string()))?;
        let rgba8 = normalize_readback(
            extent,
            &mapped,
            readback_pitch as usize,
            ReadbackFormat::Rgba8,
        )
        .map_err(WgpuBackendError::Contract)?;
        drop(mapped);
        readback.unmap();
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

fn texture_layout_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Uint,
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn supports_usage(
    adapter: &wgpu::Adapter,
    format: wgpu::TextureFormat,
    required: wgpu::TextureUsages,
) -> bool {
    adapter
        .get_texture_format_features(format)
        .allowed_usages
        .contains(required)
}

fn create_texture(
    device: &wgpu::Device,
    label: &'static str,
    size: wgpu::Extent3d,
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    })
}

fn texture_copy(texture: &wgpu::Texture) -> wgpu::TexelCopyTextureInfo<'_> {
    wgpu::TexelCopyTextureInfo {
        texture,
        mip_level: 0,
        origin: wgpu::Origin3d::ZERO,
        aspect: wgpu::TextureAspect::All,
    }
}

fn adapter_rank(device_type: wgpu::DeviceType, preference: WgpuPowerPreference) -> u8 {
    match (preference, device_type) {
        (WgpuPowerPreference::HighPerformance, wgpu::DeviceType::DiscreteGpu)
        | (WgpuPowerPreference::LowPower, wgpu::DeviceType::IntegratedGpu) => 0,
        (_, wgpu::DeviceType::IntegratedGpu | wgpu::DeviceType::DiscreteGpu) => 1,
        (_, wgpu::DeviceType::VirtualGpu) => 2,
        (_, wgpu::DeviceType::Cpu) => 3,
        (_, wgpu::DeviceType::Other) => 4,
    }
}

fn graphics_api(backend: wgpu::Backend) -> GraphicsApi {
    match backend {
        wgpu::Backend::Vulkan => GraphicsApi::Vulkan,
        wgpu::Backend::Dx12 => GraphicsApi::Direct3d12,
        wgpu::Backend::Metal => GraphicsApi::Metal,
        wgpu::Backend::Gl => GraphicsApi::OpenGl,
        _ => GraphicsApi::Unknown,
    }
}

fn device_kind(kind: wgpu::DeviceType) -> DeviceKind {
    match kind {
        wgpu::DeviceType::IntegratedGpu => DeviceKind::Integrated,
        wgpu::DeviceType::DiscreteGpu => DeviceKind::Discrete,
        wgpu::DeviceType::VirtualGpu => DeviceKind::Virtual,
        wgpu::DeviceType::Cpu => DeviceKind::Cpu,
        wgpu::DeviceType::Other => DeviceKind::Unknown,
    }
}

fn elapsed_ns(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WgpuTextureRole {
    IndexedPlane,
    Palette,
    ReadbackTarget,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WgpuBackendError {
    NoAdapter {
        api: WgpuApi,
        name_filter: Option<String>,
    },
    MissingRequiredTextureRole(WgpuTextureRole),
    RequestDevice(String),
    Poll(String),
    MapChannel(String),
    Map(String),
    Contract(FrameContractError),
}

impl fmt::Display for WgpuBackendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "wgpu indexed-palette backend failed: {self:?}")
    }
}

impl std::error::Error for WgpuBackendError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Contract(source) => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cmr_core::display::DisplayExtent;
    use cmr_formats::riff_palette::PaletteEntry;
    use cmr_gpu_contract::IndexedPaletteGpuBackend;

    #[test]
    fn adapter_rank_honors_power_preference_before_fallbacks() {
        assert!(
            adapter_rank(
                wgpu::DeviceType::DiscreteGpu,
                WgpuPowerPreference::HighPerformance
            ) < adapter_rank(
                wgpu::DeviceType::IntegratedGpu,
                WgpuPowerPreference::HighPerformance
            )
        );
        assert!(
            adapter_rank(
                wgpu::DeviceType::IntegratedGpu,
                WgpuPowerPreference::LowPower
            ) < adapter_rank(wgpu::DeviceType::DiscreteGpu, WgpuPowerPreference::LowPower)
        );
    }

    #[test]
    fn elapsed_nanoseconds_saturate_instead_of_wrapping() {
        assert_eq!(elapsed_ns(Duration::from_nanos(17)), 17);
        assert_eq!(elapsed_ns(Duration::new(u64::MAX, 999_999_999)), u64::MAX);
    }

    #[test]
    #[ignore = "requires a Vulkan adapter; run explicitly for GPU conformance"]
    fn vulkan_readback_matches_cpu_oracle_across_palette_mutation() {
        let config = WgpuBackendConfig {
            adapter_name_contains: std::env::var("CMR_WGPU_TEST_ADAPTER").ok(),
            ..WgpuBackendConfig::default()
        };
        let mut backend = WgpuIndexedPaletteBackend::new(&config).unwrap();
        let extent = DisplayExtent {
            width: 3,
            height: 2,
        };
        let indices = [0, 1, 255, 2, 1, 0];
        let mut palette: [PaletteEntry; 256] = std::array::from_fn(|index| PaletteEntry {
            red: index as u8,
            green: (255 - index) as u8,
            blue: (index as u8).wrapping_mul(7),
            flags: (index as u8).wrapping_mul(13),
        });

        for changed in [false, true] {
            if changed {
                palette[1] = PaletteEntry {
                    red: 9,
                    green: 8,
                    blue: 7,
                    flags: 0xfe,
                };
            }
            let frame = IndexedFrameView::new(extent, &indices, &palette).unwrap();
            let expected = frame.canonical_rgba8().unwrap();
            let actual = backend.render_readback(frame).unwrap();
            assert_eq!(actual.rgba8, expected);
        }

        assert_eq!(backend.info().graphics_api, GraphicsApi::Vulkan);
        assert!(backend.capabilities().sampled_r8_uint);
        assert!(backend.capabilities().headless_readback);
        eprintln!("wgpu adapter: {:?}", backend.info());
    }
}
