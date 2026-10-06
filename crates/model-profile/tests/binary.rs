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

#[test]
fn read_only_index_publication_keeps_rows_shared_with_retained_model_views() {
    for mode in ["state", "index-only"] {
        let report = run(&[
            "--mode",
            mode,
            "--case",
            "long-text",
            "--rows",
            "512",
            "--projects",
            "4",
            "--value-bytes",
            "768",
            "--retain-old",
        ]);
        let built = report.phases[0].heap;
        let staged = report.phases[1].heap;
        assert!(staged.current_bytes >= built.current_bytes);
        // Four page-handle vectors and bounded metadata, not copies of long rows.
        assert!(staged.current_bytes - built.current_bytes < 65536);
        let held = report.phases[4].heap.current_bytes;
        let released = report.phases[5].heap.current_bytes;
        if mode == "index-only" {
            assert!(held - released < 65536);
        } else {
            // Map nodes and replaced rows detach; unchanged keys/bodies stay shared.
            assert!(held - released > 0);
            assert!(held - released < 512 * 1024);
        }
    }
}

#[test]
fn real_image_replay_reports_count_bodies_and_release_owned_transients() {
    for mode in ["replay", "index-replay"] {
        for kind in ["integer", "short-text", "long-text"] {
            let report = run(&[
                "--mode",
                mode,
                "--case",
                kind,
                "--rows",
                "256",
                "--projects",
                "4",
                "--value-bytes",
                "768",
                "--retain-old",
            ]);
            assert_eq!(report.version, 2);
            assert!(report.comparison.is_none());
            assert_eq!(report.images_per_project.as_ref().unwrap().len(), 4);
            for image in report.images_per_project.as_ref().unwrap() {
                assert_eq!(image.changed_roots, 1);
                assert_eq!(image.retired_tables, 0);
                assert_eq!(image.history_pages, u64::from(mode == "replay"));
                if mode == "index-replay" || kind == "long-text" {
                    assert_eq!(image.primary_pages, 0);
                    assert_eq!(image.retired_pages, 0);
                } else {
                    assert!(image.primary_pages > 0);
                }
                assert_eq!(
                    image.image_body_bytes,
                    (image.history_pages + image.primary_pages) * 4096
                );
            }
            let sample = |phase| {
                report
                    .phases
                    .iter()
                    .find(|p| p.phase == phase)
                    .unwrap()
                    .heap
            };
            let plans = sample(PhaseKind::PlansBuilt);
            let replayed = sample(PhaseKind::Replayed);
            let released = sample(PhaseKind::ReplayReleased);
            assert!(replayed.current_bytes > plans.current_bytes);
            assert!(replayed.current_bytes > released.current_bytes);
            assert!(released.current_bytes.abs_diff(plans.current_bytes) < 65536);
            assert!(sample(PhaseKind::PlansReleased).current_bytes < released.current_bytes);
            // Cumulative counters cover all preceding work, not a phase-local peak.
            assert!(replayed.allocated_bytes > plans.allocated_bytes);
        }
    }
}

#[test]
fn history_replay_detaches_maps_while_retaining_unchanged_row_bodies() {
    let run_mode = |mode| {
        run(&[
            "--mode",
            mode,
            "--case",
            "long-text",
            "--rows",
            "1024",
            "--projects",
            "4",
            "--value-bytes",
            "768",
            "--retain-old",
        ])
    };
    let history = run_mode("replay");
    let index = run_mode("index-replay");
    let held_delta = |report: &Report| {
        let planned = report
            .phases
            .iter()
            .find(|p| p.phase == PhaseKind::PlansBuilt)
            .unwrap();
        let replayed = report
            .phases
            .iter()
            .find(|p| p.phase == PhaseKind::Replayed)
            .unwrap();
        replayed.heap.current_bytes - planned.heap.current_bytes
    };
    // Affected map structures detach, without copying every long body/key.
    assert!(held_delta(&history) > 128 * 1024);
    assert!(held_delta(&history) < 2 * 1024 * 1024);
    assert!(held_delta(&index) < 128 * 1024);
    for report in [&history, &index] {
        let phases = &report.phases;
        let plans = phases
            .iter()
            .find(|p| p.phase == PhaseKind::PlansBuilt)
            .unwrap();
        let released = phases
            .iter()
            .find(|p| p.phase == PhaseKind::ReplayReleased)
            .unwrap();
        assert!(
            released
                .heap
                .current_bytes
                .abs_diff(plans.heap.current_bytes)
                < 65536
        );
    }
}

#[test]
fn image_report_byte_counters_and_mode_are_checked_on_actual_process_output() {
    let report = run(&["--mode", "replay", "--rows", "1"]);
    let encoded = serde_json::to_vec(&report).unwrap();
    assert_eq!(decode_report(&encoded).unwrap(), report);
    for field in [
        "history_pages",
        "primary_pages",
        "retired_pages",
        "changed_roots",
        "retired_tables",
        "image_body_bytes",
    ] {
        let mut document = serde_json::to_value(&report).unwrap();
        document["images_per_project"][0][field] = serde_json::json!(u64::MAX);
        assert!(decode_report(&serde_json::to_vec(&document).unwrap()).is_err());
    }
    let mut wrong = serde_json::to_value(&report).unwrap();
    wrong["version"] = serde_json::json!(1);
    assert!(decode_report(&serde_json::to_vec(&wrong).unwrap()).is_err());
    wrong["version"] = serde_json::json!(2);
    wrong["images_per_project"][0]["private-field"] = serde_json::json!("private-marker");
    let error = decode_report(&serde_json::to_vec(&wrong).unwrap())
        .unwrap_err()
        .to_string();
    assert!(!error.contains("private-field"));
    assert!(!error.contains("private-marker"));
}
