#![cfg(feature = "heap-profile")]

use emilybase_model_profile::{Error, MAX_REPORT_BYTES, Mode, PhaseKind, Report, decode_report};
use std::process::Command;

fn run(arguments: &[&str]) -> Report {
    let output = Command::new(env!("CARGO_BIN_EXE_emilybase-model-profile"))
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let report = decode_report(&output.stdout).unwrap();
    report.validate().unwrap();
    assert!(report.phases[0].heap.allocated_blocks > 0);
    assert!(report.phases.last().unwrap().heap.current_bytes < 65536);
    report
}

#[test]
fn actual_profile_process_preserves_row_state_and_releases_retained_views() {
    for kind in ["integer", "short-text", "long-text"] {
        let report = run(&[
            "--case",
            kind,
            "--rows",
            "128",
            "--projects",
            "4",
            "--value-bytes",
            "768",
            "--retain-old",
        ]);
        assert_eq!(report.config.mode, Mode::State);
        assert_eq!(report.components_per_project.len(), 4);
        let published = report
            .phases
            .iter()
            .find(|p| p.phase == PhaseKind::Published)
            .unwrap();
        let released = report
            .phases
            .iter()
            .find(|p| p.phase == PhaseKind::OldViewsReleased)
            .unwrap();
        assert!(published.heap.current_bytes > released.heap.current_bytes);
        for components in report.components_per_project {
            assert!(components.history_pages > 1);
            if kind == "long-text" {
                assert_eq!(components.index_pages, 1);
            }
        }
    }
}

#[test]
fn full_encoding_and_streaming_fingerprints_match_with_lower_allocation_traffic() {
    for kind in ["integer", "short-text"] {
        let report = run(&[
            "--mode",
            "fingerprint",
            "--case",
            kind,
            "--rows",
            "225",
            "--projects",
            "2",
        ]);
        let comparison = report.comparison.unwrap();
        assert!(comparison.full_encoding_bytes > comparison.streaming_bytes);
        assert!(comparison.full_encoding_blocks > comparison.streaming_blocks);
        let encoded: u64 = report
            .components_per_project
            .iter()
            .map(|value| value.index_bytes)
            .sum();
        assert!(comparison.full_encoding_bytes - comparison.streaming_bytes >= encoded);
    }
}

#[test]
fn invalid_shapes_refuse_before_instrumentation_and_do_not_echo_private_arguments() {
    for arguments in [
        vec!["--rows", "0"],
        vec!["--rows", "10001"],
        vec!["--projects", "5"],
        vec!["--value-bytes", "769"],
        vec!["--mode", "fingerprint", "--case", "long-text"],
        vec!["--unknown-private-marker", "synthetic-private-argument"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_emilybase-model-profile"))
            .args(arguments)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(!stderr.contains("synthetic-private-argument"));
        assert!(!stderr.contains("unknown-private-marker"));
    }
    let help = Command::new(env!("CARGO_BIN_EXE_emilybase-model-profile"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(help.status.success());
    assert!(help.stderr.is_empty());
    assert!(
        String::from_utf8(help.stdout)
            .unwrap()
            .contains("--retain-old")
    );
}

#[test]
fn reports_from_actual_counters_refuse_corrupted_totals_order_and_epochs() {
    let original = run(&["--mode", "fingerprint", "--rows", "30"]);
    for mutation in 0..6 {
        let bytes = serde_json::to_vec(&original).unwrap();
        let mut report: Report = serde_json::from_slice(&bytes).unwrap();
        match mutation {
            0 => report.version += 1,
            1 => report.components_per_project[0].total_bytes += 1,
            2 => report.phases.swap(0, 1),
            3 => report.phases[1].heap.allocated_bytes = 0,
            4 => report.comparison.as_mut().unwrap().streaming_bytes += 1,
            _ => report.components_per_project.clear(),
        }
        assert!(matches!(
            report.validate(),
            Err(Error::Report(_)) | Err(Error::Counter)
        ));
    }
}

#[test]
fn actual_report_codec_checks_lengths_unknown_fields_counter_extremes_and_round_trip() {
    let report = run(&["--case", "long-text", "--rows", "32", "--retain-old"]);
    let encoded = serde_json::to_vec(&report).unwrap();
    assert_eq!(decode_report(&encoded).unwrap(), report);
    for cut in 0..encoded.len() {
        assert!(decode_report(&encoded[..cut]).is_err());
    }
    let mut trailing = encoded.clone();
    trailing.extend_from_slice(b" private-marker");
    let error = decode_report(&trailing).unwrap_err().to_string();
    assert!(!error.contains("private-marker"));
    assert!(decode_report(&vec![b' '; MAX_REPORT_BYTES + 1]).is_err());
    for field in [
        "index_pages",
        "history_pages",
        "root_bytes",
        "index_bytes",
        "total_bytes",
    ] {
        let mut value = serde_json::to_value(&report).unwrap();
        value["components_per_project"][0][field] = serde_json::json!(u64::MAX);
        assert!(decode_report(&serde_json::to_vec(&value).unwrap()).is_err());
    }
    let mut unknown = serde_json::to_value(&report).unwrap();
    unknown["extra_private_field"] = serde_json::json!("private-marker");
    let error = decode_report(&serde_json::to_vec(&unknown).unwrap())
        .unwrap_err()
        .to_string();
    assert!(!error.contains("private-marker"));
    assert!(!error.contains("extra_private_field"));
}
