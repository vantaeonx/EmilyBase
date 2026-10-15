#![cfg(feature = "heap-profile")]
use std::process::{Command, Output};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Sample {
    iterations: usize,
    refusals: usize,
    input_bytes: usize,
    total_bytes: u64,
    total_blocks: u64,
    peak_bytes: usize,
    live_bytes: usize,
}
fn worker(negative: bool) -> Output {
    let mut command = Command::new(env!(
        "CARGO_BIN_EXE_emilybase-private-archive-allocation-check"
    ));
    if negative {
        command.arg("--negative-control");
    }
    command.output().unwrap()
}
fn sample(output: &Output, bytes: u64, blocks: u64) {
    assert!(output.stderr.is_empty());
    assert!(output.stdout.len() < 1024);
    let observed: Sample = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(observed.iterations, 1000);
    assert_eq!(observed.refusals, 4000);
    assert_eq!(observed.input_bytes, 1024 * 1024);
    assert_eq!(observed.total_bytes, bytes);
    assert_eq!(observed.total_blocks, blocks);
    assert_eq!(observed.peak_bytes, bytes as usize);
    assert_eq!(observed.live_bytes, bytes as usize);
}
#[test]
fn early_scope_and_envelope_refusals_do_not_allocate_or_copy_input_sized_payloads() {
    let output = worker(false);
    assert!(output.status.success());
    sample(&output, 0, 0);
    let negative = worker(true);
    assert!(!negative.status.success());
    sample(&negative, 144, 1);
}

#[test]
fn early_refusal_measurement_excludes_unrelated_background_allocations() {
    use std::sync::{Arc, Barrier};
    let start = Arc::new(Barrier::new(2));
    let ready = Arc::new(Barrier::new(2));
    let finish = Arc::new(Barrier::new(2));
    let worker_start = Arc::clone(&start);
    let worker_ready = Arc::clone(&ready);
    let worker_finish = Arc::clone(&finish);
    let background = std::thread::spawn(move || {
        worker_start.wait();
        let unrelated = Box::new([7_u8; 144]);
        std::hint::black_box(&unrelated);
        worker_ready.wait();
        worker_finish.wait();
        drop(unrelated);
    });
    start.wait();
    ready.wait();
    let output = worker(false);
    finish.wait();
    background.join().unwrap();
    assert!(output.status.success());
    sample(&output, 0, 0);
}

#[test]
fn private_archive_diagnostic_refuses_unknown_and_repeated_options_without_output() {
    for args in [
        vec!["--unknown"],
        vec!["--negative-control", "--negative-control"],
    ] {
        let output = Command::new(env!(
            "CARGO_BIN_EXE_emilybase-private-archive-allocation-check"
        ))
        .args(args)
        .output()
        .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("panicked"));
    }
}
