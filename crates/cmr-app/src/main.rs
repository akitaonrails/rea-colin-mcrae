use std::process::ExitCode;

use cmr_app::{AppCommand, RUNTIME_DATA_ENV, run_headless_root_frame, runtime_config};

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
        },
        Err(error) => {
            eprintln!("cmr-app: {error}");
            ExitCode::FAILURE
        }
    }
}
