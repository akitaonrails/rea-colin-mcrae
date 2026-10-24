//! Shared indexed-palette GPU conformance runner.
//!
//! This is a correctness and plumbing probe, not a representative C3D
//! benchmark. Both candidates receive the same source-owned indexed frames and
//! must match the same CPU oracle byte for byte. Timings describe this small
//! upload/render/readback workload only and never select a winning backend.

use std::ffi::OsString;
use std::fmt;

use cmr_core::display::DisplayExtent;
use cmr_formats::riff_palette::PaletteEntry;
use cmr_gpu_contract::{
    BackendCapabilities, BackendInfo, CanonicalRgba8Frame, DeviceKind, GraphicsApi,
    IndexedFrameView, IndexedPaletteGpuBackend, TimingSummary,
};
use cmr_video::SoftwarePresentedFrame;
use cmr_video_sdl_gpu::{SdlGpuConfig, SdlGpuIndexedBackend};
use cmr_video_wgpu::{WgpuApi, WgpuBackendConfig, WgpuIndexedPaletteBackend, WgpuPowerPreference};

pub mod stage_benchmark;

pub const FIXTURE_EXTENT: DisplayExtent = DisplayExtent {
    width: 257,
    height: 2,
};

const DEFAULT_WARMUP: usize = 2;
const DEFAULT_FRAMES: usize = 8;
const MAX_ITERATIONS: usize = 100_000;

pub use cmr_gpu_stage::{PreparedRawStage, PreparedRawStageError, PreparedRawStageStats};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BackendSelection {
    Wgpu,
    SdlGpu,
    #[default]
    Both,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Options {
    pub backend: BackendSelection,
    pub wgpu_adapter: Option<String>,
    pub low_power: bool,
    pub warmup: usize,
    pub frames: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            backend: BackendSelection::Both,
            wgpu_adapter: None,
            low_power: false,
            warmup: DEFAULT_WARMUP,
            frames: DEFAULT_FRAMES,
        }
    }
}

impl Options {
    pub fn parse(arguments: impl IntoIterator<Item = OsString>) -> Result<Self, CliError> {
        let mut options = Self::default();
        let mut arguments = arguments.into_iter();
        let mut backend_seen = false;
        let mut adapter_seen = false;
        let mut low_power_seen = false;
        let mut warmup_seen = false;
        let mut frames_seen = false;

        while let Some(argument) = arguments.next() {
            let argument = argument.into_string().map_err(CliError::NonUtf8Argument)?;
            match argument.as_str() {
                "--backend" => {
                    if backend_seen {
                        return Err(CliError::DuplicateOption("--backend"));
                    }
                    backend_seen = true;
                    options.backend = match next_value(&mut arguments, "--backend")?.as_str() {
                        "wgpu" => BackendSelection::Wgpu,
                        "sdl-gpu" => BackendSelection::SdlGpu,
                        "both" => BackendSelection::Both,
                        value => return Err(CliError::InvalidBackend(value.to_owned())),
                    };
                }
                "--wgpu-adapter" => {
                    if adapter_seen {
                        return Err(CliError::DuplicateOption("--wgpu-adapter"));
                    }
                    adapter_seen = true;
                    let value = next_value(&mut arguments, "--wgpu-adapter")?;
                    if value.is_empty() {
                        return Err(CliError::EmptyValue("--wgpu-adapter"));
                    }
                    options.wgpu_adapter = Some(value);
                }
                "--low-power" => {
                    if low_power_seen {
                        return Err(CliError::DuplicateOption("--low-power"));
                    }
                    low_power_seen = true;
                    options.low_power = true;
                }
                "--warmup" => {
                    if warmup_seen {
                        return Err(CliError::DuplicateOption("--warmup"));
                    }
                    warmup_seen = true;
                    options.warmup =
                        parse_count("--warmup", &next_value(&mut arguments, "--warmup")?, true)?;
                }
                "--frames" => {
                    if frames_seen {
                        return Err(CliError::DuplicateOption("--frames"));
                    }
                    frames_seen = true;
                    options.frames =
                        parse_count("--frames", &next_value(&mut arguments, "--frames")?, false)?;
                }
                _ => return Err(CliError::UnknownArgument(argument)),
            }
        }

        Ok(options)
    }
}

fn next_value(
    arguments: &mut impl Iterator<Item = OsString>,
    option: &'static str,
) -> Result<String, CliError> {
    arguments
        .next()
        .ok_or(CliError::MissingValue(option))?
        .into_string()
        .map_err(CliError::NonUtf8Argument)
}

fn parse_count(option: &'static str, value: &str, allow_zero: bool) -> Result<usize, CliError> {
    let parsed = value.parse::<usize>().map_err(|_| CliError::InvalidCount {
        option,
        value: value.to_owned(),
    })?;
    if (!allow_zero && parsed == 0) || parsed > MAX_ITERATIONS {
        return Err(CliError::InvalidCount {
            option,
            value: value.to_owned(),
        });
    }
    Ok(parsed)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CliError {
    UnknownArgument(String),
    NonUtf8Argument(OsString),
    DuplicateOption(&'static str),
    MissingValue(&'static str),
    EmptyValue(&'static str),
    InvalidBackend(String),
    InvalidCount { option: &'static str, value: String },
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid GPU conformance options: {self:?}")
    }
}

impl std::error::Error for CliError {}

#[derive(Clone, Debug)]
pub struct SharedFixture {
    indices: Box<[u8]>,
    baseline_palette: Box<[PaletteEntry; 256]>,
    mutated_palette: Box<[PaletteEntry; 256]>,
}

impl SharedFixture {
    #[must_use]
    pub fn new() -> Self {
        let indices: Vec<u8> = (0..=u8::MAX)
            .chain(std::iter::once(0))
            .chain((0..=u8::MAX).rev())
            .chain(std::iter::once(u8::MAX))
            .collect();
        let baseline_palette = Box::new(std::array::from_fn(|index| PaletteEntry {
            red: index as u8,
            green: (u8::MAX - index as u8).rotate_left(1),
            blue: (index as u8).wrapping_mul(73),
            flags: (index as u8).wrapping_mul(29),
        }));
        let mut mutated_palette = baseline_palette.clone();
        mutated_palette[73] = PaletteEntry {
            red: 0x12,
            green: 0x34,
            blue: 0x56,
            flags: 0xfe,
        };
        Self {
            indices: indices.into_boxed_slice(),
            baseline_palette,
            mutated_palette,
        }
    }

    pub fn baseline(&self) -> IndexedFrameView<'_> {
        IndexedFrameView::new(
            FIXTURE_EXTENT,
            &self.indices,
            self.baseline_palette.as_slice(),
        )
        .expect("built-in baseline fixture is valid")
    }

    pub fn mutated(&self) -> IndexedFrameView<'_> {
        IndexedFrameView::new(
            FIXTURE_EXTENT,
            &self.indices,
            self.mutated_palette.as_slice(),
        )
        .expect("built-in mutated fixture is valid")
    }
}

impl Default for SharedFixture {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug)]
pub struct BackendRun {
    pub info: BackendInfo,
    pub capabilities: BackendCapabilities,
    pub cpu_encode_submit: TimingSummary,
    pub submit_to_completion: TimingSummary,
    baseline: CanonicalRgba8Frame,
    mutated: CanonicalRgba8Frame,
}

impl BackendRun {
    #[must_use]
    pub fn stable_summary(&self) -> String {
        format!(
            "backend={} api={} device_kind={} vendor_id={} device_id={} adapter={:?} driver={:?} sampled_r8_uint={} timestamps={} headless_readback={} samples={} cpu_ns_min={} cpu_ns_median={} cpu_ns_p95={} cpu_ns_p99={} cpu_ns_max={} completion_ns_min={} completion_ns_median={} completion_ns_p95={} completion_ns_p99={} completion_ns_max={} exact=true",
            backend_name(self.info.backend),
            api_name(self.info.graphics_api),
            device_kind_name(self.info.device_kind),
            optional_hex(self.info.vendor_id),
            optional_hex(self.info.device_id),
            self.info.adapter_name,
            self.info.driver_name,
            self.capabilities.sampled_r8_uint,
            self.capabilities.timestamp_queries,
            self.capabilities.headless_readback,
            self.cpu_encode_submit.samples,
            self.cpu_encode_submit.minimum_ns,
            self.cpu_encode_submit.median_ns,
            self.cpu_encode_submit.p95_ns,
            self.cpu_encode_submit.p99_ns,
            self.cpu_encode_submit.maximum_ns,
            self.submit_to_completion.minimum_ns,
            self.submit_to_completion.median_ns,
            self.submit_to_completion.p95_ns,
            self.submit_to_completion.p99_ns,
            self.submit_to_completion.maximum_ns,
        )
    }
}

#[derive(Clone, Debug)]
pub struct ConformanceReport {
    pub wgpu: Option<BackendRun>,
    pub sdl_gpu: Option<BackendRun>,
}

impl ConformanceReport {
    #[must_use]
    pub fn stable_summary(&self) -> String {
        let mut lines = Vec::new();
        if let Some(run) = &self.wgpu {
            lines.push(run.stable_summary());
        }
        if let Some(run) = &self.sdl_gpu {
            lines.push(run.stable_summary());
        }
        if let (Some(wgpu), Some(sdl)) = (&self.wgpu, &self.sdl_gpu) {
            lines.push(format!(
                "cross_backend_exact=true vendor_device_pair_equal={} adapter_names_equal={} winner=not-selected workload=indexed-palette-conformance-only",
                wgpu.info.same_vendor_device_pair(&sdl.info),
                wgpu.info.adapter_name == sdl.info.adapter_name,
            ));
        }
        lines.join("\n")
    }
}

pub fn execute(options: &Options) -> Result<ConformanceReport, RunError> {
    let fixture = SharedFixture::new();
    // Establish parity with the reconstruction's existing software
    // presentation path before initializing or submitting work to a GPU.
    let oracles = fixture_oracles(&fixture)?;
    let mut wgpu = None;
    let mut sdl_gpu = None;

    if matches!(
        options.backend,
        BackendSelection::Wgpu | BackendSelection::Both
    ) {
        let mut backend = WgpuIndexedPaletteBackend::new(&WgpuBackendConfig {
            api: WgpuApi::Vulkan,
            adapter_name_contains: options.wgpu_adapter.clone(),
            power_preference: if options.low_power {
                WgpuPowerPreference::LowPower
            } else {
                WgpuPowerPreference::HighPerformance
            },
        })
        .map_err(|error| RunError::BackendInit {
            backend: "wgpu",
            detail: error.to_string(),
        })?;
        wgpu = Some(run_backend(&mut backend, &fixture, &oracles, options)?);
    }
    if matches!(
        options.backend,
        BackendSelection::SdlGpu | BackendSelection::Both
    ) {
        let mut backend = SdlGpuIndexedBackend::new(SdlGpuConfig {
            prefer_low_power: options.low_power,
            debug_mode: false,
        })
        .map_err(|error| RunError::BackendInit {
            backend: "sdl-gpu",
            detail: error.to_string(),
        })?;
        sdl_gpu = Some(run_backend(&mut backend, &fixture, &oracles, options)?);
    }

    if let (Some(wgpu), Some(sdl)) = (&wgpu, &sdl_gpu)
        && (wgpu.baseline != sdl.baseline || wgpu.mutated != sdl.mutated)
    {
        return Err(RunError::CrossBackendMismatch);
    }
    Ok(ConformanceReport { wgpu, sdl_gpu })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FixtureState {
    Baseline,
    PaletteMutation,
}

#[derive(Clone, Debug)]
struct FixtureOracles {
    baseline: CanonicalRgba8Frame,
    mutated: CanonicalRgba8Frame,
}

fn fixture_oracles(fixture: &SharedFixture) -> Result<FixtureOracles, RunError> {
    let baseline = validated_fixture_oracle(FixtureState::Baseline, fixture.baseline())?;
    let mutated = validated_fixture_oracle(FixtureState::PaletteMutation, fixture.mutated())?;
    Ok(FixtureOracles { baseline, mutated })
}

fn validated_fixture_oracle(
    state: FixtureState,
    frame: IndexedFrameView<'_>,
) -> Result<CanonicalRgba8Frame, RunError> {
    let contract = frame
        .canonical_rgba8()
        .map_err(|error| RunError::Fixture(error.to_string()))?;
    let palette: [PaletteEntry; 256] = frame
        .palette()
        .try_into()
        .expect("IndexedFrameView guarantees a complete palette");
    let established = SoftwarePresentedFrame::from_indexed(
        frame.extent(),
        frame.indices().to_vec().into_boxed_slice(),
        Box::new(palette),
    )
    .and_then(|frame| frame.expand_rgba8())
    .map_err(|error| RunError::EstablishedOracle(error.to_string()))?;
    ensure_oracle_parity(state, contract.pixels(), established.pixels())?;
    Ok(contract)
}

fn ensure_oracle_parity(
    state: FixtureState,
    contract: &[u8],
    established: &[u8],
) -> Result<(), RunError> {
    if contract != established {
        return Err(RunError::ContractVsEstablishedOracleMismatch { state });
    }
    Ok(())
}

fn run_backend<B>(
    backend: &mut B,
    fixture: &SharedFixture,
    oracles: &FixtureOracles,
    options: &Options,
) -> Result<BackendRun, RunError>
where
    B: IndexedPaletteGpuBackend,
    B::Error: fmt::Display,
{
    let baseline_expected = &oracles.baseline;
    let mutated_expected = &oracles.mutated;

    for iteration in 0..options.warmup {
        let frame = if iteration % 2 == 0 {
            fixture.baseline()
        } else {
            fixture.mutated()
        };
        let expected = if iteration % 2 == 0 {
            baseline_expected
        } else {
            mutated_expected
        };
        let result = backend
            .render_readback(frame)
            .map_err(|error| RunError::BackendFrame(error.to_string()))?;
        if &result.rgba8 != expected {
            return Err(RunError::CpuOracleMismatch);
        }
    }

    let mut cpu = Vec::with_capacity(options.frames);
    let mut completion = Vec::with_capacity(options.frames);
    let mut baseline = None;
    let mut mutated = None;
    for iteration in 0..options.frames {
        let is_baseline = iteration % 2 == 0;
        let frame = if is_baseline {
            fixture.baseline()
        } else {
            fixture.mutated()
        };
        let expected = if is_baseline {
            baseline_expected
        } else {
            mutated_expected
        };
        let result = backend
            .render_readback(frame)
            .map_err(|error| RunError::BackendFrame(error.to_string()))?;
        if &result.rgba8 != expected {
            return Err(RunError::CpuOracleMismatch);
        }
        if is_baseline {
            baseline = Some(result.rgba8.clone());
        } else {
            mutated = Some(result.rgba8.clone());
        }
        cpu.push(result.timing.cpu_encode_submit_ns);
        completion.push(result.timing.submit_to_completion_ns);
    }

    // A one-frame run still proves both source states outside the timing sample.
    let baseline = baseline.unwrap_or_else(|| baseline_expected.clone());
    let mutated = match mutated {
        Some(frame) => frame,
        None => {
            let result = backend
                .render_readback(fixture.mutated())
                .map_err(|error| RunError::BackendFrame(error.to_string()))?;
            if &result.rgba8 != mutated_expected {
                return Err(RunError::CpuOracleMismatch);
            }
            result.rgba8
        }
    };
    Ok(BackendRun {
        info: backend.info().clone(),
        capabilities: backend.capabilities(),
        cpu_encode_submit: TimingSummary::from_samples(&cpu)
            .map_err(|error| RunError::Fixture(error.to_string()))?,
        submit_to_completion: TimingSummary::from_samples(&completion)
            .map_err(|error| RunError::Fixture(error.to_string()))?,
        baseline,
        mutated,
    })
}

pub(crate) fn backend_name(backend: cmr_gpu_contract::BackendKind) -> &'static str {
    match backend {
        cmr_gpu_contract::BackendKind::Wgpu => "wgpu",
        cmr_gpu_contract::BackendKind::SdlGpu => "sdl-gpu",
    }
}

pub(crate) fn api_name(api: GraphicsApi) -> &'static str {
    match api {
        GraphicsApi::Vulkan => "vulkan",
        GraphicsApi::Direct3d12 => "direct3d12",
        GraphicsApi::Metal => "metal",
        GraphicsApi::OpenGl => "opengl",
        GraphicsApi::Software => "software",
        GraphicsApi::Unknown => "unknown",
    }
}

pub(crate) fn device_kind_name(kind: DeviceKind) -> &'static str {
    match kind {
        DeviceKind::Integrated => "integrated",
        DeviceKind::Discrete => "discrete",
        DeviceKind::Virtual => "virtual",
        DeviceKind::Cpu => "cpu",
        DeviceKind::Unknown => "unknown",
    }
}

pub(crate) fn optional_hex(value: Option<u32>) -> String {
    value.map_or_else(|| "unknown".to_owned(), |value| format!("0x{value:04x}"))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RunError {
    BackendInit {
        backend: &'static str,
        detail: String,
    },
    BackendFrame(String),
    Fixture(String),
    EstablishedOracle(String),
    ContractVsEstablishedOracleMismatch {
        state: FixtureState,
    },
    CpuOracleMismatch,
    CrossBackendMismatch,
}

impl fmt::Display for RunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "GPU indexed-palette conformance failed: {self:?}"
        )
    }
}

impl std::error::Error for RunError {}

#[cfg(test)]
mod tests {
    use super::*;
    use cmr_core::c3d_camera::{MATRIX_IDENTITY_Q16, view_matrix_words, world_matrix_words};
    use cmr_core::c3d_render::VERTEXTYPE_LVERTEX;
    use cmr_core::c3d_stage_scene::StageScene;
    use cmr_formats::bitmap::BitmapLimits;
    use cmr_formats::c3d::{
        C3D_ENTRY24_ENTRY32_ORDINAL_OFFSETS, C3D_FORMAT_VERSION, C3D_GROUP0_VALUES_OFFSET,
        C3D_GROUP1_ENTRY24_LEN, C3D_GROUP1_ENTRY32_LEN, C3D_GROUP1_FIXED_LEN, C3D_HEADER_LEN,
        C3D_MAGIC, C3dLimits, parse_c3d,
    };
    use cmr_gpu_contract::{
        BackendKind, C3dSubmissionBracket, ConformanceFrame, FrameTiming, IndexedPaletteGpuBackend,
        RawC3dGpuBackend,
    };
    use cmr_video::software_stage_scene::StageC3dTextures;

    fn args(values: &[&str]) -> impl Iterator<Item = OsString> {
        values.iter().map(OsString::from)
    }

    fn active_palette() -> [PaletteEntry; 256] {
        std::array::from_fn(|index| PaletteEntry {
            red: index as u8,
            green: (255 - index) as u8,
            blue: (index as u8).wrapping_mul(3),
            flags: (index as u8).wrapping_mul(17),
        })
    }

    #[test]
    fn parser_accepts_every_option_and_rejects_bad_or_duplicate_values() {
        let parsed = Options::parse(args(&[
            "--backend",
            "wgpu",
            "--wgpu-adapter",
            "RADV",
            "--low-power",
            "--warmup",
            "0",
            "--frames",
            "17",
        ]))
        .unwrap();
        assert_eq!(parsed.backend, BackendSelection::Wgpu);
        assert_eq!(parsed.wgpu_adapter.as_deref(), Some("RADV"));
        assert!(parsed.low_power);
        assert_eq!(parsed.warmup, 0);
        assert_eq!(parsed.frames, 17);

        assert!(matches!(
            Options::parse(args(&["--backend", "bad"])),
            Err(CliError::InvalidBackend(_))
        ));
        assert!(matches!(
            Options::parse(args(&["--frames", "0"])),
            Err(CliError::InvalidCount { .. })
        ));
        assert_eq!(
            Options::parse(args(&["--low-power", "--low-power"])),
            Err(CliError::DuplicateOption("--low-power"))
        );
        assert!(matches!(
            Options::parse(args(&["--unknown"])),
            Err(CliError::UnknownArgument(_))
        ));
    }

    #[test]
    fn shared_fixture_covers_all_indices_and_only_mutates_one_palette_entry() {
        let fixture = SharedFixture::new();
        assert_eq!(fixture.indices.len(), 514);
        assert_eq!(&fixture.indices[..256], &(0..=u8::MAX).collect::<Vec<_>>());
        assert_eq!(fixture.indices[256], 0);
        assert_eq!(fixture.indices[257], u8::MAX);
        assert_eq!(fixture.indices[513], u8::MAX);
        let differences: Vec<_> = fixture
            .baseline_palette
            .iter()
            .zip(fixture.mutated_palette.iter())
            .enumerate()
            .filter_map(|(index, (before, after))| (before != after).then_some(index))
            .collect();
        assert_eq!(differences, [73]);
        assert_ne!(
            fixture.baseline().canonical_rgba8().unwrap(),
            fixture.mutated().canonical_rgba8().unwrap()
        );
    }

    #[test]
    fn shared_fixture_contract_matches_established_software_oracle_for_both_states() {
        let fixture = SharedFixture::new();
        let oracles = fixture_oracles(&fixture).unwrap();
        assert_ne!(oracles.baseline, oracles.mutated);

        let mut corrupted = oracles.baseline.pixels().to_vec();
        corrupted[0] ^= 1;
        assert_eq!(
            ensure_oracle_parity(
                FixtureState::Baseline,
                &corrupted,
                oracles.baseline.pixels()
            ),
            Err(RunError::ContractVsEstablishedOracleMismatch {
                state: FixtureState::Baseline,
            })
        );
    }

    struct OracleBackend {
        info: BackendInfo,
        counter: u64,
    }

    impl IndexedPaletteGpuBackend for OracleBackend {
        type Error = std::convert::Infallible;

        fn info(&self) -> &BackendInfo {
            &self.info
        }

        fn capabilities(&self) -> BackendCapabilities {
            BackendCapabilities {
                sampled_r8_uint: true,
                timestamp_queries: false,
                headless_readback: true,
                device_recovery: false,
            }
        }

        fn render_readback(
            &mut self,
            frame: IndexedFrameView<'_>,
        ) -> Result<ConformanceFrame, Self::Error> {
            self.counter += 1;
            Ok(ConformanceFrame {
                rgba8: frame.canonical_rgba8().unwrap(),
                timing: FrameTiming {
                    cpu_encode_submit_ns: self.counter,
                    submit_to_completion_ns: self.counter + 10,
                    gpu_timestamp_ns: None,
                },
            })
        }
    }

    #[test]
    fn shared_runner_checks_both_states_and_emits_stable_non_winner_summary() {
        let fixture = SharedFixture::new();
        let mut backend = OracleBackend {
            info: BackendInfo {
                backend: BackendKind::Wgpu,
                graphics_api: GraphicsApi::Vulkan,
                vendor_id: Some(0x1234),
                device_id: Some(0x5678),
                device_kind: DeviceKind::Discrete,
                adapter_name: "synthetic adapter".to_owned(),
                driver_name: "synthetic driver".to_owned(),
                driver_info: String::new(),
            },
            counter: 0,
        };
        let run = run_backend(
            &mut backend,
            &fixture,
            &fixture_oracles(&fixture).unwrap(),
            &Options {
                warmup: 1,
                frames: 3,
                ..Options::default()
            },
        )
        .unwrap();
        let summary = run.stable_summary();
        assert!(summary.contains("backend=wgpu api=vulkan"));
        assert!(summary.contains("samples=3"));
        assert!(summary.contains("exact=true"));
        assert!(!summary.contains('/'));

        let report = ConformanceReport {
            wgpu: Some(run.clone()),
            sdl_gpu: Some(BackendRun {
                info: BackendInfo {
                    backend: BackendKind::SdlGpu,
                    vendor_id: None,
                    device_id: None,
                    ..run.info.clone()
                },
                ..run
            }),
        };
        let summary = report.stable_summary();
        assert!(summary.contains("cross_backend_exact=true"));
        assert!(summary.contains("vendor_device_pair_equal=false"));
        assert!(!summary.contains("physical_device"));
        assert!(summary.contains("winner=not-selected"));
    }

    #[test]
    fn stage_adapter_preserves_raw_geometry_draw_state_and_texture_binding() {
        let bytes = drawable_stage_c3d(&indexed_bitmap_4x4());
        let file = parse_c3d(&bytes, C3dLimits::default()).unwrap();
        let scene = StageScene::from_c3d(&file, &MATRIX_IDENTITY_Q16, VERTEXTYPE_LVERTEX).unwrap();
        let textures = StageC3dTextures::prepare(&file, BitmapLimits::default()).unwrap();
        let active_palette = active_palette();
        let mut camera = MATRIX_IDENTITY_Q16;
        camera[14] = -(6 << 16);

        let prepared = PreparedRawStage::prepare(
            &scene,
            &textures,
            DisplayExtent {
                width: 64,
                height: 48,
            },
            &active_palette,
        )
        .unwrap();
        assert_eq!(
            prepared.stats(),
            PreparedRawStageStats {
                draws: 1,
                unique_textures: 1,
                unresolved_prims: 0,
                empty_lod_nodes: 0,
            }
        );

        let frame = prepared.frame_for(&camera);
        assert_eq!(frame.active_palette(), &active_palette);
        assert_eq!(frame.draws().len(), 1);
        assert_eq!(frame.draws()[0].submission(), C3dSubmissionBracket::Normal);
        assert!(frame.draws()[0].state().depth_write());
        assert_eq!(frame.resources().meshes().len(), 1);
        assert_eq!(frame.resources().textures().len(), 1);
        let source_vertices: Vec<_> = file
            .group1_records()
            .next()
            .unwrap()
            .entries_32()
            .map(|entry| entry.components_q16())
            .collect();
        let packet_vertices: Vec<_> = frame.resources().meshes()[0]
            .vertices()
            .iter()
            .map(|vertex| vertex.components)
            .collect();
        assert_eq!(packet_vertices, source_vertices);
        assert_eq!(frame.resources().meshes()[0].indices(), &[0, 1, 2]);
        assert_eq!(frame.resources().textures()[0].extent().width, 4);
        assert_eq!(
            frame.resources().textures()[0].indices(),
            &[12, 13, 14, 15, 8, 9, 10, 11, 4, 5, 6, 7, 0, 1, 2, 3]
        );
        assert_eq!(
            frame.draws()[0].world().words(),
            world_matrix_words(scene.world(0)).words()
        );
        assert_eq!(frame.view().words(), view_matrix_words(&camera).words());

        let upload = prepared.upload();
        assert_eq!(upload.layout().extent(), frame.extent());
        assert_eq!(upload.layout().mesh_count(), 1);
        assert_eq!(upload.layout().texture_count(), 1);
        assert_eq!(upload.resources(), frame.resources());
        assert_eq!(upload.active_palette(), frame.active_palette());

        let persistent_frame = prepared.prepared_frame_for(&camera);
        assert_eq!(persistent_frame.layout(), upload.layout());
        assert_eq!(persistent_frame.view(), frame.view());
        assert_eq!(persistent_frame.projection(), frame.projection());
        assert_eq!(persistent_frame.draws(), frame.draws());
        persistent_frame.ensure_layout(upload.layout()).unwrap();
    }

    #[test]
    #[ignore = "requires SDL_VIDEO_DRIVER=offscreen plus Vulkan-capable wgpu and SDL_GPU devices"]
    fn both_backends_match_exactly_on_one_raw_recovered_c3d_packet() {
        let bytes = drawable_stage_c3d(&indexed_bitmap_4x4());
        let file = parse_c3d(&bytes, C3dLimits::default()).unwrap();
        let scene = StageScene::from_c3d(&file, &MATRIX_IDENTITY_Q16, VERTEXTYPE_LVERTEX).unwrap();
        let textures = StageC3dTextures::prepare(&file, BitmapLimits::default()).unwrap();
        let active_palette = active_palette();
        let mut camera = MATRIX_IDENTITY_Q16;
        camera[14] = -(6 << 16);
        let prepared = PreparedRawStage::prepare(
            &scene,
            &textures,
            DisplayExtent {
                width: 64,
                height: 48,
            },
            &active_palette,
        )
        .unwrap();

        let mut wgpu = WgpuIndexedPaletteBackend::new(&WgpuBackendConfig {
            adapter_name_contains: std::env::var("CMR_WGPU_TEST_ADAPTER").ok(),
            power_preference: if std::env::var_os("CMR_GPU_TEST_LOW_POWER").is_some() {
                WgpuPowerPreference::LowPower
            } else {
                WgpuPowerPreference::HighPerformance
            },
            ..WgpuBackendConfig::default()
        })
        .unwrap();
        let mut sdl = SdlGpuIndexedBackend::new(SdlGpuConfig {
            prefer_low_power: std::env::var_os("CMR_GPU_TEST_LOW_POWER").is_some(),
            debug_mode: false,
        })
        .unwrap();
        let wgpu_frame = wgpu
            .render_c3d_readback(prepared.frame_for(&camera))
            .unwrap();
        let sdl_frame = sdl
            .render_c3d_readback(prepared.frame_for(&camera))
            .unwrap();

        let clear = [
            active_palette[0].red,
            active_palette[0].green,
            active_palette[0].blue,
            u8::MAX,
        ];
        let mismatches: Vec<_> = wgpu_frame
            .rgba8
            .pixels()
            .as_chunks::<4>()
            .0
            .iter()
            .zip(sdl_frame.rgba8.pixels().as_chunks::<4>().0)
            .enumerate()
            .filter_map(|(index, (wgpu, sdl))| (wgpu != sdl).then_some((index, *wgpu, *sdl)))
            .collect();
        let bounds = |pixels: &[u8]| {
            let points: Vec<_> = pixels
                .as_chunks::<4>()
                .0
                .iter()
                .enumerate()
                .filter_map(|(index, pixel)| (*pixel != clear).then_some((index % 64, index / 64)))
                .collect();
            (
                points.len(),
                points.iter().map(|point| point.0).min(),
                points.iter().map(|point| point.0).max(),
                points.iter().map(|point| point.1).min(),
                points.iter().map(|point| point.1).max(),
            )
        };
        assert!(
            mismatches.is_empty(),
            "raw C3D readbacks differ at {} pixels; wgpu bounds {:?}; SDL bounds {:?}; first mismatches: {:?}",
            mismatches.len(),
            bounds(wgpu_frame.rgba8.pixels()),
            bounds(sdl_frame.rgba8.pixels()),
            &mismatches[..mismatches.len().min(8)]
        );
        assert!(
            wgpu_frame
                .rgba8
                .pixels()
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| *pixel != clear),
            "raw C3D packet must draw visible non-clear pixels"
        );
    }

    fn drawable_stage_c3d(bitmap: &[u8]) -> Vec<u8> {
        let group0_len = C3D_GROUP0_VALUES_OFFSET + 2;
        let group1_len = C3D_GROUP1_FIXED_LEN + 3 * C3D_GROUP1_ENTRY32_LEN + C3D_GROUP1_ENTRY24_LEN;
        let group1_start = C3D_HEADER_LEN + group0_len;
        let blob_start = group1_start + group1_len;
        let mut input = vec![0; blob_start + 16 + bitmap.len()];
        input[0..4].copy_from_slice(&C3D_MAGIC);
        input[4..8].copy_from_slice(&C3D_FORMAT_VERSION.to_le_bytes());
        input[12..14].copy_from_slice(&1u16.to_le_bytes());
        input[14..16].copy_from_slice(&1u16.to_le_bytes());
        input[16..18].copy_from_slice(&1u16.to_le_bytes());
        for (index, offset) in [C3D_HEADER_LEN, group1_start, blob_start]
            .into_iter()
            .enumerate()
        {
            let field = 20 + index * 4;
            input[field..field + 4].copy_from_slice(&(offset as u32).to_le_bytes());
        }

        let group0 = C3D_HEADER_LEN;
        input[group0..group0 + 4].copy_from_slice(&(group0_len as u32).to_le_bytes());
        input[group0 + 4..group0 + 6].copy_from_slice(&(-1i16).to_le_bytes());
        for index in [0, 5, 10, 15] {
            let field = group0 + 8 + index * 4;
            input[field..field + 4].copy_from_slice(&(1i32 << 16).to_le_bytes());
        }
        input[group0 + 76] = 1;
        input[group0 + C3D_GROUP0_VALUES_OFFSET..group0 + C3D_GROUP0_VALUES_OFFSET + 2]
            .copy_from_slice(&0u16.to_le_bytes());

        let group1 = group1_start;
        input[group1 + 12..group1 + 16].copy_from_slice(&(group1_len as u32).to_le_bytes());
        input[group1 + 16..group1 + 18].copy_from_slice(&3u16.to_le_bytes());
        input[group1 + 18..group1 + 20].copy_from_slice(&1u16.to_le_bytes());
        let vertices: [[i32; 8]; 3] = [
            [-(4 << 16), -(3 << 16), 0, 0, -(1 << 16), 0, 1 << 16, 0],
            [-(4 << 16), 3 << 16, 0, 0, 0, 0, 1 << 16, 0],
            [4 << 16, -(3 << 16), 0, 1 << 16, -(1 << 16), 0, 1 << 16, 0],
        ];
        for (vertex_index, vertex) in vertices.iter().enumerate() {
            let start = group1 + C3D_GROUP1_FIXED_LEN + vertex_index * C3D_GROUP1_ENTRY32_LEN;
            for (component_index, component) in vertex.iter().enumerate() {
                let field = start + component_index * 4;
                input[field..field + 4].copy_from_slice(&component.to_le_bytes());
            }
        }
        let entry24 = group1 + C3D_GROUP1_FIXED_LEN + 3 * C3D_GROUP1_ENTRY32_LEN;
        for (corner, ordinal) in [0u16, 1, 2].into_iter().enumerate() {
            let field = entry24 + C3D_ENTRY24_ENTRY32_ORDINAL_OFFSETS[corner];
            input[field..field + 2].copy_from_slice(&ordinal.to_le_bytes());
        }
        input[entry24 + 8..entry24 + 10].copy_from_slice(&0i16.to_le_bytes());
        input[blob_start..blob_start + 12].copy_from_slice(b"SCENE-TEX!!!");
        input[blob_start + 12..blob_start + 16]
            .copy_from_slice(&(bitmap.len() as u32).to_le_bytes());
        input[blob_start + 16..].copy_from_slice(bitmap);
        input
    }

    fn indexed_bitmap_4x4() -> Vec<u8> {
        const WIDTH: usize = 4;
        const HEIGHT: usize = 4;
        const PALETTE_BYTES: usize = 256 * 4;
        const PIXEL_OFFSET: usize = 14 + 40 + PALETTE_BYTES;
        const STORED_BYTES: usize = WIDTH * HEIGHT;
        let mut input = vec![0; PIXEL_OFFSET + STORED_BYTES];
        let file_size = input.len() as u32;
        input[0..2].copy_from_slice(b"BM");
        input[2..6].copy_from_slice(&file_size.to_le_bytes());
        input[10..14].copy_from_slice(&(PIXEL_OFFSET as u32).to_le_bytes());
        input[14..18].copy_from_slice(&40u32.to_le_bytes());
        input[18..22].copy_from_slice(&(WIDTH as i32).to_le_bytes());
        input[22..26].copy_from_slice(&(HEIGHT as i32).to_le_bytes());
        input[26..28].copy_from_slice(&1u16.to_le_bytes());
        input[28..30].copy_from_slice(&8u16.to_le_bytes());
        input[34..38].copy_from_slice(&(STORED_BYTES as u32).to_le_bytes());
        input[46..50].copy_from_slice(&256u32.to_le_bytes());
        for index in 0..256 {
            let start = 54 + index * 4;
            input[start] = index as u8;
            input[start + 1] = (index as u8).wrapping_add(1);
            input[start + 2] = (index as u8).wrapping_add(2);
            input[start + 3] = 0xa5;
        }
        input[PIXEL_OFFSET..].copy_from_slice(&(0..16).collect::<Vec<_>>());
        input
    }
}
