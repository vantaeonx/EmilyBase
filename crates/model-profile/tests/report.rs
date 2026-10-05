//! Codec fixtures exercise admission, not measured allocations.
use emilybase_model_profile::{
    Comparison, Components, Config, Heap, Kind, Mode, Phase, PhaseKind, Report, decode_report,
};
use proptest::prelude::*;

fn fixture(mode: Mode, kind: Kind, rows: u16, projects: u8) -> Report {
    let names: &[PhaseKind] = match mode {
        Mode::State | Mode::IndexOnly => &[
            PhaseKind::Built,
            PhaseKind::Staged,
            PhaseKind::IndexesStaged,
            PhaseKind::Prepared,
            PhaseKind::Published,
            PhaseKind::OldViewsReleased,
            PhaseKind::Released,
        ],
        Mode::Fingerprint => &[
            PhaseKind::Built,
            PhaseKind::FullEncoding,
            PhaseKind::Streamed,
            PhaseKind::Released,
        ],
    };
    let phases: Vec<_> = names
        .iter()
        .enumerate()
        .map(|(number, &phase)| Phase {
            phase,
            heap: Heap {
                current_bytes: 32,
                current_blocks: 2,
                peak_bytes: 64,
                peak_blocks: 4,
                allocated_bytes: 128 + number as u64 * 64,
                allocated_blocks: 8 + number as u64 * 2,
            },
        })
        .collect();
    let index_pages = if kind == Kind::LongText {
        1
    } else {
        u64::from(rows).div_ceil(14)
    };
    let index_bytes = (index_pages + 1) * 4096;
    let component = Components {
        history_pages: u64::from(mode != Mode::Fingerprint),
        root_bytes: if mode != Mode::Fingerprint { 192 } else { 0 },
        index_pages,
        index_bytes,
        total_bytes: index_bytes + if mode != Mode::Fingerprint { 4288 } else { 0 },
    };
    let comparison = if mode == Mode::Fingerprint {
        Some(Comparison::from_samples(phases[0].heap, phases[1].heap, phases[2].heap).unwrap())
    } else {
        None
    };
    Report {
        version: 1,
        config: Config {
            mode,
            kind,
            rows,
            projects,
            value_bytes: 0,
            retain_old: false,
        },
        phases,
        components_per_project: vec![component; projects as usize],
        comparison,
    }
}

#[test]
fn blocks_at_peak_cannot_exceed_total_allocated_blocks() {
    let mut report = fixture(Mode::State, Kind::Integer, 1, 1);
    report.phases[0].heap.peak_blocks = u64::MAX;
    assert!(report.validate().is_err());
}

#[test]
fn peak_block_count_is_not_a_monotonic_maximum_of_block_counts() {
    let mut report = fixture(Mode::State, Kind::Integer, 1, 1);
    // A later larger allocation peak can consist of fewer, larger blocks.
    report.phases[1].heap.peak_bytes = 65;
    report.phases[1].heap.peak_blocks = 1;
    report.phases[1].heap.current_blocks = 6;
    for phase in &mut report.phases[2..] {
        phase.heap.peak_bytes = 65;
        phase.heap.peak_blocks = 1;
    }
    report.validate().unwrap();
    assert_eq!(
        decode_report(&serde_json::to_vec(&report).unwrap()).unwrap(),
        report
    );
}

#[test]
fn counter_regressions_refuse_even_when_each_sample_is_locally_valid() {
    for field in ["allocated_bytes", "allocated_blocks", "peak_bytes"] {
        let report = fixture(Mode::State, Kind::Integer, 1, 1);
        let mut document = serde_json::to_value(report).unwrap();
        let before = document["phases"][0]["heap"][field].as_u64().unwrap();
        document["phases"][1]["heap"][field] = serde_json::json!(before - 1);
        assert!(decode_report(&serde_json::to_vec(&document).unwrap()).is_err());
    }
}

#[test]
fn every_nested_report_object_refuses_unknown_fields_without_echoing_them() {
    let original = fixture(Mode::Fingerprint, Kind::ShortText, 30, 2);
    for object in 0..6 {
        let mut value = serde_json::to_value(&original).unwrap();
        let target = match object {
            0 => &mut value,
            1 => &mut value["config"],
            2 => &mut value["phases"][0],
            3 => &mut value["phases"][0]["heap"],
            4 => &mut value["components_per_project"][0],
            _ => &mut value["comparison"],
        };
        target["unknown-private-field"] = serde_json::json!("private-marker");
        let error = decode_report(&serde_json::to_vec(&value).unwrap())
            .unwrap_err()
            .to_string();
        assert!(!error.contains("unknown-private-field"));
        assert!(!error.contains("private-marker"));
    }
}

#[test]
fn encoded_component_and_mode_boundaries_do_not_authorize_invalid_reports() {
    let original = fixture(Mode::State, Kind::Integer, 10000, 4);
    for mutation in 0..9 {
        let mut report = fixture(Mode::State, Kind::Integer, 10000, 4);
        match mutation {
            0 => report.phases.pop().map(|_| ()).unwrap(),
            1 => report.phases.push(report.phases[0]),
            2 => report.components_per_project.pop().map(|_| ()).unwrap(),
            3 => report.components_per_project[0].history_pages = 65537,
            4 => report.components_per_project[0].index_pages = 1,
            5 => report.components_per_project[0].root_bytes = 0,
            6 => report.config.rows = 10001,
            7 => report.config.projects = 5,
            _ => report.config.value_bytes = 769,
        }
        assert!(report.validate().is_err());
    }
    original.validate().unwrap();
    let mut fingerprint = fixture(Mode::Fingerprint, Kind::Integer, 1, 1);
    fingerprint.config.retain_old = true;
    assert!(fingerprint.validate().is_err());
    fingerprint.config.retain_old = false;
    fingerprint.config.kind = Kind::LongText;
    assert!(fingerprint.validate().is_err());
}

#[test]
fn comparison_uses_checked_differences_at_full_u64_boundaries() {
    let mut before = fixture(Mode::Fingerprint, Kind::Integer, 1, 1).phases[0].heap;
    before.allocated_bytes = u64::MAX - 2;
    before.allocated_blocks = u64::MAX - 2;
    let mut full = before;
    full.allocated_bytes += 1;
    full.allocated_blocks += 1;
    let mut streamed = full;
    streamed.allocated_bytes += 1;
    streamed.allocated_blocks += 1;
    assert_eq!(
        Comparison::from_samples(before, full, streamed).unwrap(),
        Comparison {
            full_encoding_bytes: 1,
            full_encoding_blocks: 1,
            streaming_bytes: 1,
            streaming_blocks: 1,
        }
    );
    assert!(Comparison::from_samples(streamed, full, before).is_err());
    full.allocated_bytes = before.allocated_bytes;
    full.allocated_blocks = before.allocated_blocks - 1;
    assert!(Comparison::from_samples(before, full, streamed).is_err());
}

#[test]
fn preserved_synthetic_release_reports_pass_bounded_admission() {
    for bytes in [
        include_bytes!(
            "../../../docs/measurements/2026-10-05-model-profile/fingerprint-short.json"
        )
        .as_slice(),
        include_bytes!("../../../docs/measurements/2026-10-05-model-profile/state-short.json")
            .as_slice(),
        include_bytes!("../../../docs/measurements/2026-10-05-model-profile/state-long.json")
            .as_slice(),
        include_bytes!("../../../docs/measurements/2026-10-05-model-profile/state-long-four.json")
            .as_slice(),
        include_bytes!("../../../docs/measurements/2026-10-05-shared-tables/index-only-four.json")
            .as_slice(),
        include_bytes!("../../../docs/measurements/2026-10-05-shared-tables/state-long-four.json")
            .as_slice(),
    ] {
        let report = decode_report(bytes).unwrap();
        assert_eq!(report.config.rows, 10000);
        assert!(report.phases.last().unwrap().heap.current_bytes < 1024);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn valid_bounded_shapes_round_trip_with_consistent_components(
        rows in 1u16..=10000, projects in 1u8..=4,
        kind in 0u8..3, requested_mode in 0u8..3, value_bytes in 0u16..=768
    ) {
        let kind = match kind { 0 => Kind::Integer, 1 => Kind::ShortText, _ => Kind::LongText };
        let mode = match requested_mode {
            1=>Mode::IndexOnly,
            2 if kind!=Kind::LongText=>Mode::Fingerprint,
            _=>Mode::State,
        };
        let mut report = fixture(mode, kind, rows, projects);
        report.config.value_bytes = value_bytes;
        let bytes = serde_json::to_vec(&report).unwrap();
        prop_assert_eq!(decode_report(&bytes).unwrap(), report);
    }

    #[test]
    fn eligible_row_capacity_and_component_bytes_are_checked_together(rows in 1u16..=10000) {
        let mut report = fixture(Mode::State, Kind::ShortText, rows, 1);
        let maximum = (8 * u64::from(rows) / 49 + 1).min(1024);
        let component = &mut report.components_per_project[0];
        component.index_pages = maximum + 1;
        component.index_bytes = (component.index_pages + 1) * 4096;
        component.total_bytes = component.index_bytes + component.root_bytes + component.history_pages * 4096;
        prop_assert!(report.validate().is_err());
    }

    #[test]
    fn arbitrary_bounded_documents_never_panic_or_echo_data(bytes in prop::collection::vec(any::<u8>(), 0..8193)) {
        match decode_report(&bytes) {
            Ok(report) => {
                prop_assert!(report.validate().is_ok());
                let canonical = serde_json::to_vec(&report).unwrap();
                prop_assert_eq!(decode_report(&canonical).unwrap(), report);
            }
            Err(error) => {
                // All public rejection text is selected from static errors.
                prop_assert!(error.to_string().len() < 128);
            }
        }
    }
}
