//! Standalone allocation sample: no test-harness threads share this allocator.
use emilybase_catalog::{Column, DataType, Key, Schema};
use std::process::ExitCode;

#[global_allocator]
static ALLOCATOR: dhat::Alloc = dhat::Alloc;

#[derive(serde::Serialize)]
struct Sample {
    columns: usize,
    total_bytes: u64,
    total_blocks: u64,
    peak_bytes: usize,
    live_bytes: usize,
}

fn run(negative: bool) -> Result<bool, Box<dyn std::error::Error>> {
    let mut samples = Vec::with_capacity(3);
    for count in [1, 2, 64] {
        let schema = Schema {
            name: "t".into(),
            columns: (0..count)
                .map(|i| Column {
                    name: format!("c{i:02}_{}", "x".repeat(50)),
                    data_type: DataType::Integer,
                    nullable: false,
                })
                .collect(),
            primary_key: 0,
        };
        let key = Key::Integer(7);
        let profiler = dhat::Profiler::builder().testing().build();
        // A negative control proves nonzero operation allocations still fail.
        let control = negative.then(|| vec![7_u8; 144]);
        for _ in 0..1000 {
            std::hint::black_box(&schema).validate()?;
            schema.validate_key(std::hint::black_box(&key))?;
        }
        std::hint::black_box(&control);
        let observed = dhat::HeapStats::get();
        drop(control);
        drop(profiler);
        samples.push(Sample {
            columns: count,
            total_bytes: observed.total_bytes,
            total_blocks: observed.total_blocks,
            peak_bytes: observed.max_bytes,
            live_bytes: observed.curr_bytes,
        });
    }
    // Output allocation is outside every measured operation.
    serde_json::to_writer(std::io::stdout().lock(), &samples)?;
    Ok(samples.iter().all(|s| {
        s.total_bytes == 0 && s.total_blocks == 0 && s.peak_bytes == 0 && s.live_bytes == 0
    }))
}

fn main() -> ExitCode {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() > 1 || args.first().is_some_and(|arg| arg != "--negative-control") {
        eprintln!("invalid schema allocation arguments");
        return ExitCode::FAILURE;
    }
    match run(!args.is_empty()) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(_) => {
            eprintln!("schema allocation check failed");
            ExitCode::FAILURE
        }
    }
}
