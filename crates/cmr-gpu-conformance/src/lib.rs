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

pub const FIXTURE_EXTENT: DisplayExtent = DisplayExtent {
    width: 257,
    height: 2,
};

const DEFAULT_WARMUP: usize = 2;
const DEFAULT_FRAMES: usize = 8;
const MAX_ITERATIONS: usize = 100_000;

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

fn backend_name(backend: cmr_gpu_contract::BackendKind) -> &'static str {
    match backend {
        cmr_gpu_contract::BackendKind::Wgpu => "wgpu",
        cmr_gpu_contract::BackendKind::SdlGpu => "sdl-gpu",
    }
}

fn api_name(api: GraphicsApi) -> &'static str {
    match api {
        GraphicsApi::Vulkan => "vulkan",
        GraphicsApi::Direct3d12 => "direct3d12",
        GraphicsApi::Metal => "metal",
        GraphicsApi::OpenGl => "opengl",
        GraphicsApi::Software => "software",
        GraphicsApi::Unknown => "unknown",
    }
}

fn device_kind_name(kind: DeviceKind) -> &'static str {
    match kind {
        DeviceKind::Integrated => "integrated",
        DeviceKind::Discrete => "discrete",
        DeviceKind::Virtual => "virtual",
        DeviceKind::Cpu => "cpu",
        DeviceKind::Unknown => "unknown",
    }
}

fn optional_hex(value: Option<u32>) -> String {
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
    use cmr_gpu_contract::{BackendKind, ConformanceFrame, FrameTiming, IndexedPaletteGpuBackend};

    fn args(values: &[&str]) -> impl Iterator<Item = OsString> {
        values.iter().map(OsString::from)
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
}
