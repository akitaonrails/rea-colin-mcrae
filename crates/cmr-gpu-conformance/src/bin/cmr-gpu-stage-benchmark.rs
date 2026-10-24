use std::process::ExitCode;

use cmr_gpu_conformance::stage_benchmark::{SOURCE_ENV, StageBenchmarkOptions, run};

fn main() -> ExitCode {
    let options = match StageBenchmarkOptions::parse(
        std::env::args_os().skip(1),
        std::env::var_os(SOURCE_ENV),
    ) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("cmr-gpu-stage-benchmark: {error}");
            return ExitCode::FAILURE;
        }
    };
    match run(&options) {
        Ok(report) => {
            println!("{}", report.stable_summary());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!(
                "cmr-gpu-stage-benchmark: stage benchmark failed: {} ({error})",
                error.kind()
            );
            ExitCode::FAILURE
        }
    }
}
