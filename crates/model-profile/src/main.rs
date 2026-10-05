mod fingerprint;
mod state;
mod support;

use clap::Parser;
use emilybase_model_profile::{Config, Mode};
use std::process::ExitCode;

// This allocator is linked only into the opt-in diagnostic binary.
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let config = match Config::try_parse() {
        Ok(config) => config.validate()?,
        Err(error)
            if matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) =>
        {
            error.print()?;
            return Ok(());
        }
        Err(_) => return Err("invalid profiling arguments; use --help".into()),
    };
    // Testing mode suppresses profile-file output; the report contains counters only.
    let profiler = dhat::Profiler::builder()
        .testing()
        .trim_backtraces(Some(4))
        .build();
    let report = match config.mode {
        Mode::State => state::measure(config)?,
        Mode::Fingerprint => fingerprint::measure(config)?,
    };
    drop(profiler);
    report.validate()?;
    serde_json::to_writer(std::io::stdout().lock(), &report)?;
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
