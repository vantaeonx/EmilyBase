#![cfg(feature = "heap-profile")]
use std::process::{Command, Output};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Heap {
    total_bytes: u64,
    total_blocks: u64,
    peak_bytes: usize,
    live_bytes: usize,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Sample {
    matching: Heap,
    issued: Heap,
    token_bytes: usize,
    released_bytes: usize,
}
fn worker(negative: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_emilybase-token-allocation-check"));
    if negative {
        command.arg("--negative-control");
    }
    command.output().unwrap()
}
fn sample(output: &Output, matching_bytes: u64, matching_blocks: u64) {
    assert!(output.stderr.is_empty());
    assert!(output.stdout.len() < 1024);
    let value: Sample = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value.matching.total_bytes, matching_bytes);
    assert_eq!(value.matching.total_blocks, matching_blocks);
    assert_eq!(value.matching.peak_bytes, matching_bytes as usize);
    assert_eq!(value.matching.live_bytes, matching_bytes as usize);
    assert_eq!(value.issued.total_bytes, 102);
    assert_eq!(value.issued.total_blocks, 1);
    assert_eq!(value.issued.peak_bytes, 102);
    assert_eq!(value.issued.live_bytes, 102);
    assert_eq!(value.token_bytes, 102);
    assert_eq!(value.released_bytes, 0);
}
fn zero(output: &Output) {
    assert!(output.status.success());
    sample(output, 0, 0);
}

#[test]
fn matching_and_decoding_do_not_allocate_and_issued_text_has_one_bounded_owner() {
    zero(&worker(false));
    let negative = worker(true);
    assert!(!negative.status.success());
    sample(&negative, 144, 1);
}

#[test]
fn token_sample_excludes_parent_background_allocations_without_relaxing_counters() {
    let unrelated = std::thread::spawn(|| Box::new([7_u8; 144])).join().unwrap();
    zero(&worker(false));
    std::hint::black_box(&unrelated);
}

#[test]
fn token_diagnostic_refuses_unknown_and_repeated_options_without_sample_output() {
    for args in [
        vec!["--unknown"],
        vec!["--negative-control", "--negative-control"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_emilybase-token-allocation-check"))
            .args(args)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("panicked"));
    }
}
