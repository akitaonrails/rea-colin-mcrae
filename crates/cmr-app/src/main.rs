use std::process::ExitCode;

use cmr_app::{
    AppCommand, RUNTIME_DATA_ENV, render_root_frame, run_headless_root_frame, runtime_config,
};
use cmr_platform_sdl3::present_software_frame;

fn main() -> ExitCode {
    match runtime_config(
        std::env::args_os().skip(1),
        std::env::var_os(RUNTIME_DATA_ENV),
    ) {
        Ok(config) => match config.command() {
            AppCommand::HeadlessRootFrame => match run_headless_root_frame(config.runtime_data()) {
                Ok(output) => {
                    println!("{}", output.stable_summary());
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("cmr-app: headless-root-frame failed: {}", error.kind());
                    ExitCode::FAILURE
                }
            },
            AppCommand::WindowedRootFrame => match render_root_frame(config.runtime_data()) {
                Ok(output) => match present_software_frame(&output.frame) {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(_) => {
                        eprintln!("cmr-app: windowed-root-frame failed: host");
                        ExitCode::FAILURE
                    }
                },
                Err(error) => {
                    eprintln!("cmr-app: windowed-root-frame failed: {}", error.kind());
                    ExitCode::FAILURE
                }
            },
        },
        Err(error) => {
            eprintln!("cmr-app: {error}");
            ExitCode::FAILURE
        }
    }
}
