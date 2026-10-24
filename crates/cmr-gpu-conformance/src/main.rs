use std::process::ExitCode;

use cmr_gpu_conformance::{Options, execute};

fn main() -> ExitCode {
    let options = match Options::parse(std::env::args_os().skip(1)) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("cmr-gpu-conformance: {error}");
            return ExitCode::FAILURE;
        }
    };
    match execute(&options) {
        Ok(report) => {
            println!("{}", report.stable_summary());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("cmr-gpu-conformance: {error}");
            ExitCode::FAILURE
        }
    }
}
