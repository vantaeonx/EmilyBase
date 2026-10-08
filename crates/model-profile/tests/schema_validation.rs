#![cfg(feature = "heap-profile")]
use std::process::{Command, Output};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Sample {
    columns: usize,
    total_bytes: u64,
    total_blocks: u64,
    peak_bytes: usize,
    live_bytes: usize,
}
fn worker(negative: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_emilybase-schema-allocation-check"));
    if negative {
        command.arg("--negative-control");
    }
    command.output().unwrap()
}
fn samples(output: &Output) -> Vec<Sample> {
    assert!(output.stderr.is_empty());
    assert!(output.stdout.len() < 1024);
    let samples: Vec<Sample> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        samples.iter().map(|s| s.columns).collect::<Vec<_>>(),
        [1, 2, 64]
    );
    samples
}
fn zero(output: &Output) {
    assert!(output.status.success());
    for sample in samples(output) {
        assert_eq!(sample.total_bytes, 0);
        assert_eq!(sample.total_blocks, 0);
        assert_eq!(sample.peak_bytes, 0);
        assert_eq!(sample.live_bytes, 0);
    }
}

#[test]
fn bounded_schema_and_key_validation_use_no_temporary_heap() {
    zero(&worker(false));
    let negative = worker(true);
    assert!(!negative.status.success());
    for sample in samples(&negative) {
        assert_eq!(sample.total_bytes, 144);
        assert_eq!(sample.total_blocks, 1);
        assert_eq!(sample.peak_bytes, 144);
        assert_eq!(sample.live_bytes, 144);
    }
}

#[test]
fn schema_measurement_excludes_unrelated_parent_process_allocations() {
    // The original process-wide sample counted this144-byte background buffer
    // and thread startup as schema allocations. The dedicated process measures
    // only its own schema work; the negative control above still detects144 bytes.
    let unrelated = std::thread::spawn(|| Box::new([7_u8; 144])).join().unwrap();
    zero(&worker(false));
    std::hint::black_box(&unrelated);
}
