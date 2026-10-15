//! Isolated early-refusal allocation sample without test-harness threads.
use emilybase_auth::accounts::inspect_private_account_backup_bytes;
use std::io::Write;
use std::process::ExitCode;

#[global_allocator]
static ALLOCATOR: dhat::Alloc = dhat::Alloc;

#[derive(serde::Serialize)]
struct Sample {
    iterations: usize,
    refusals: usize,
    input_bytes: usize,
    total_bytes: u64,
    total_blocks: u64,
    peak_bytes: usize,
    live_bytes: usize,
}
fn run(negative: bool) -> Result<Sample, &'static str> {
    let oversized = vec![0; 1024 * 1024];
    let header = [0; 128];
    let project = "11111111111111111111111111111111";
    let profiler = dhat::Profiler::builder().testing().build();
    let control = negative.then(|| Box::new([7_u8; 144]));
    for _ in 0..1000 {
        if inspect_private_account_backup_bytes(&oversized, "../synthetic").is_ok()
            || inspect_private_account_backup_bytes(&oversized, project).is_ok()
            || inspect_private_account_backup_bytes(&header, project).is_ok()
            || inspect_private_account_backup_bytes(&[], project).is_ok()
        {
            return Err("synthetic early-refusal check failed");
        }
    }
    std::hint::black_box(&control);
    let observed = dhat::HeapStats::get();
    drop(control);
    drop(profiler);
    Ok(Sample {
        iterations: 1000,
        refusals: 4000,
        input_bytes: oversized.len(),
        total_bytes: observed.total_bytes,
        total_blocks: observed.total_blocks,
        peak_bytes: observed.max_bytes,
        live_bytes: observed.curr_bytes,
    })
}
fn main() -> ExitCode {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() > 1 || args.first().is_some_and(|arg| arg != "--negative-control") {
        eprintln!("invalid private archive allocation arguments");
        return ExitCode::FAILURE;
    }
    let sample = match run(!args.is_empty()) {
        Ok(sample) => sample,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let zero = sample.total_bytes == 0
        && sample.total_blocks == 0
        && sample.peak_bytes == 0
        && sample.live_bytes == 0;
    // Report serialization, stdout and the parent harness are outside the sample.
    let mut out = std::io::stdout().lock();
    if serde_json::to_writer(&mut out, &sample).is_err() || out.flush().is_err() {
        return ExitCode::FAILURE;
    }
    if zero {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
