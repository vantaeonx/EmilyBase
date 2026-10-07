//! Actual executor work must include every candidate even with a tiny sort LIMIT.
use super::*;
use crate::select;
use emilybase_catalog::DataType;
use emilybase_database::{Event, EventKind, Snapshot};
fn fixture() -> Snapshot {
    let mut snapshot = Snapshot::empty().unwrap();
    snapshot
        .apply(Event {
            table_id: 1,
            kind: EventKind::Create(Schema {
                name: "t".into(),
                columns: vec![
                    Column {
                        name: "rank".into(),
                        data_type: DataType::Integer,
                        nullable: false,
                    },
                    Column {
                        name: "id".into(),
                        data_type: DataType::Integer,
                        nullable: false,
                    },
                ],
                primary_key: 1,
            }),
        })
        .unwrap();
    for id in 0..200 {
        snapshot
            .apply(Event {
                table_id: 1,
                kind: EventKind::Insert(vec![Value::Integer(199 - id), Value::Integer(id)]),
            })
            .unwrap();
    }
    snapshot
}
fn selected(snapshot: &Snapshot, sql: &str) -> Plan {
    let statements = parse(sql).unwrap();
    let Statement::Select(select) = &statements[0] else {
        panic!("SELECT fixture")
    };
    Plan::compile(snapshot, select, &[]).unwrap()
}
#[test]
fn tiny_limit_still_evaluates_every_source_in_each_sort_direction() {
    let snapshot = fixture();
    for (direction, expected) in [("ASC", [199, 198]), ("DESC", [0, 1])] {
        let plan = selected(
            &snapshot,
            &format!("SELECT id FROM t ORDER BY rank {direction} LIMIT 2"),
        );
        assert!(plan.primary_order.is_none());
        let mut budget = Budget { work: 0, output: 0 };
        let result = select::run(&snapshot, plan, &mut budget).unwrap();
        assert_eq!(budget.work, 200);
        assert_eq!(budget.output, 2 * (24 + 32));
        assert_eq!(result.rows, expected.map(|n| vec![Value::Integer(n)]));
    }
}
#[test]
fn necessary_points_ranges_and_empty_ranges_keep_exact_candidate_charges() {
    let snapshot = fixture();
    for (sql, work, expected) in [
        (
            "SELECT id FROM t WHERE id=150 ORDER BY rank LIMIT 1",
            2,
            vec![150],
        ),
        (
            "SELECT id FROM t WHERE id>=150 AND id<160 ORDER BY rank LIMIT 2",
            40,
            vec![159, 158],
        ),
        (
            "SELECT id FROM t WHERE id>=150 AND id<150 ORDER BY rank LIMIT 2",
            0,
            vec![],
        ),
        (
            "SELECT id FROM t WHERE id=999 ORDER BY rank LIMIT 1",
            0,
            vec![],
        ),
    ] {
        let mut budget = Budget { work: 0, output: 0 };
        let result = select::run(&snapshot, selected(&snapshot, sql), &mut budget).unwrap();
        assert_eq!(budget.work, work, "{sql}");
        assert_eq!(
            result.rows,
            expected
                .into_iter()
                .map(|n| vec![Value::Integer(n)])
                .collect::<Vec<_>>()
        );
    }
}
#[test]
fn false_filters_and_original_work_and_output_refusals_remain_active() {
    let snapshot = fixture();
    let sql = "SELECT id FROM t WHERE FALSE ORDER BY rank LIMIT 1";
    let mut budget = Budget { work: 0, output: 0 };
    assert!(
        select::run(&snapshot, selected(&snapshot, sql), &mut budget)
            .unwrap()
            .rows
            .is_empty()
    );
    assert_eq!(budget.work, 400);
    let mut budget = Budget {
        work: MAX_QUERY_WORK - 199,
        output: 0,
    };
    assert!(matches!(
        select::run(
            &snapshot,
            selected(&snapshot, "SELECT id FROM t ORDER BY rank LIMIT 1"),
            &mut budget
        ),
        Err(ExecutionError::Limit("query work"))
    ));
    let mut budget = Budget {
        work: 0,
        output: MAX_OUTPUT_BYTES,
    };
    assert!(matches!(
        select::run(
            &snapshot,
            selected(&snapshot, "SELECT id FROM t ORDER BY rank LIMIT 1"),
            &mut budget
        ),
        Err(ExecutionError::Limit("output bytes"))
    ));
}
