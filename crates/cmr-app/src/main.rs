use std::process::ExitCode;

use cmr_app::{
    AppCommand, RUNTIME_DATA_ENV, SOURCE_ENV, run_headless_root_frame, run_headless_stage_frame,
    run_windowed_root_frame, runtime_config,
};

fn main() -> ExitCode {
    match runtime_config(
        std::env::args_os().skip(1),
        std::env::var_os(RUNTIME_DATA_ENV),
        std::env::var_os(SOURCE_ENV),
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
            AppCommand::WindowedRootFrame => match run_windowed_root_frame(config.runtime_data()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("cmr-app: windowed-root-frame failed: {}", error.kind());
                    ExitCode::FAILURE
                }
            },
            AppCommand::HeadlessStageFrame => {
                // resolve() guarantees both a stage name and a media root for
                // this command, so these accessors are populated.
                let media_root = config
                    .media_root()
                    .expect("stage command resolves with a media root");
                let stage = config.stage().expect("stage command resolves with a stage");
                match run_headless_stage_frame(media_root, stage) {
                    Ok(output) => {
                        println!("{}", output.stable_summary());
                        ExitCode::SUCCESS
                    }
                    Err(error) => {
                        eprintln!("cmr-app: headless-stage-frame failed: {}", error.kind());
                        ExitCode::FAILURE
                    }
                }
            }
        },
        Err(error) => {
            eprintln!("cmr-app: {error}");
            ExitCode::FAILURE
        }
    }
}
