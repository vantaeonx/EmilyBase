//! Inspect the actual executor budget, independently of wall-clock timing.
use super::*;
use crate::select;
use emilybase_catalog::DataType;
use emilybase_database::{Event, EventKind, Snapshot};

fn fixture(count: usize) -> Snapshot {
    let mut snapshot = Snapshot::empty().unwrap();
    for (id, name) in [(1, "l"), (2, "r")] {
        snapshot
            .apply(Event {
                table_id: id,
                kind: EventKind::Create(Schema {
                    name: name.into(),
                    columns: vec![
                        Column {
                            name: "id".into(),
                            data_type: DataType::Integer,
                            nullable: false,
                        },
                        Column {
                            name: "link".into(),
                            data_type: DataType::Integer,
                            nullable: true,
                        },
                    ],
                    primary_key: 0,
                }),
            })
            .unwrap();
        for n in 0..count {
            snapshot
                .apply(Event {
                    table_id: id,
                    kind: EventKind::Insert(vec![
                        Value::Integer(n as i64),
                        Value::Integer(n as i64),
                    ]),
                })
                .unwrap();
        }
    }
    snapshot
}
fn compiled(snapshot: &Snapshot, on: &str) -> Plan {
    let sql = format!("SELECT a.id,b.id FROM l AS a JOIN r AS b ON {on} ORDER BY a.id");
    let statements = parse(&sql).unwrap();
    let Statement::Select(select) = &statements[0] else {
        panic!("SELECT fixture")
    };
    Plan::compile(snapshot, select, &[]).unwrap()
}

#[test]
fn unique_primary_probes_use_linear_work_and_return_identical_complete_rows() {
    let snapshot = fixture(200);
    let digest = snapshot.page_fingerprint();
    let fast = compiled(&snapshot, "a.link=b.id");
    assert_eq!(fast.primary_join, Some(1));
    let mut slow = compiled(&snapshot, "a.link=b.id");
    slow.primary_join = None;
    let mut fast_budget = Budget { work: 0, output: 0 };
    let mut slow_budget = Budget { work: 0, output: 0 };
    let fast = select::run(&snapshot, fast, &mut fast_budget).unwrap();
    let slow = select::run(&snapshot, slow, &mut slow_budget).unwrap();
    assert_eq!(fast, slow);
    assert_eq!(fast.rows.len(), 200);
    assert_eq!(fast_budget.work, 600);
    assert_eq!(slow_budget.work, 80000);
    assert_eq!(fast_budget.output, slow_budget.output);
    assert_eq!(snapshot.page_fingerprint(), digest);
}

#[test]
fn null_and_missing_probes_still_charge_each_source_row() {
    let mut snapshot = fixture(3);
    for (id, link) in [(0, Value::Null), (1, Value::Integer(-1))] {
        snapshot
            .apply(Event {
                table_id: 1,
                kind: EventKind::Replace(vec![Value::Integer(id), link]),
            })
            .unwrap();
    }
    let mut budget = Budget { work: 0, output: 0 };
    let result = select::run(&snapshot, compiled(&snapshot, "a.link=b.id"), &mut budget).unwrap();
    assert_eq!(result.rows, [vec![Value::Integer(2), Value::Integer(2)]]);
    assert_eq!(budget.work, 5); // three probes, one candidate, one predicate
    let mut budget = Budget {
        work: MAX_QUERY_WORK - 1,
        output: 0,
    };
    assert!(matches!(
        select::run(&snapshot, compiled(&snapshot, "a.link=b.id"), &mut budget),
        Err(ExecutionError::Limit("query work"))
    ));
}

#[test]
fn all_join_and_filter_nodes_keep_the_shared_work_budget() {
    let snapshot = fixture(1);
    let sql = "SELECT a.id FROM l AS a JOIN r AS b ON a.link=b.id AND b.link>=0 WHERE a.id>=0";
    let statements = parse(sql).unwrap();
    let Statement::Select(select) = &statements[0] else {
        panic!("SELECT fixture")
    };
    let plan = Plan::compile(&snapshot, select, &[]).unwrap();
    let mut budget = Budget { work: 0, output: 0 };
    assert_eq!(
        select::run(&snapshot, plan, &mut budget).unwrap().rows,
        [vec![Value::Integer(0)]]
    );
    assert_eq!(budget.work, 6); // probe + candidate + three ON nodes + WHERE
    let plan = Plan::compile(&snapshot, select, &[]).unwrap();
    let mut budget = Budget {
        work: MAX_QUERY_WORK - 5,
        output: 0,
    };
    assert!(matches!(
        select::run(&snapshot, plan, &mut budget),
        Err(ExecutionError::Limit("query work"))
    ));
}

fn selected(snapshot: &Snapshot, sql: &str) -> Plan {
    let parsed = parse(sql).unwrap();
    let Statement::Select(select) = &parsed[0] else {
        panic!("SELECT fixture")
    };
    Plan::compile(snapshot, select, &[]).unwrap()
}

#[test]
fn ordered_primary_prefix_stops_after_matching_rows_in_each_direction() {
    let snapshot = fixture(200);
    for (direction, expected) in [("ASC", vec![0, 1]), ("DESC", vec![199, 198])] {
        let sql = format!(
            "SELECT a.id FROM l AS a JOIN r AS b ON a.link=b.id ORDER BY a.id {direction},b.link LIMIT 2"
        );
        let plan = selected(&snapshot, &sql);
        assert_eq!(plan.primary_order, Some(direction == "DESC"));
        let mut budget = Budget { work: 0, output: 0 };
        let result = select::run(&snapshot, plan, &mut budget).unwrap();
        assert_eq!(
            result.rows,
            expected
                .into_iter()
                .map(|v| vec![Value::Integer(v)])
                .collect::<Vec<_>>()
        );
        assert_eq!(budget.work, 6);
    }
}

#[test]
fn left_point_and_range_admission_charge_only_selected_source_rows() {
    let snapshot = fixture(200);
    let point = selected(
        &snapshot,
        "SELECT a.id FROM l AS a JOIN r AS b ON a.link=b.id WHERE a.id=150 ORDER BY a.id DESC LIMIT 1",
    );
    assert_eq!(point.key, Some(emilybase_catalog::Key::Integer(150)));
    assert!(point.range.is_none());
    let mut budget = Budget { work: 0, output: 0 };
    assert_eq!(
        select::run(&snapshot, point, &mut budget).unwrap().rows,
        [vec![Value::Integer(150)]]
    );
    assert_eq!(budget.work, 4);
    let range = selected(
        &snapshot,
        "SELECT a.id FROM l AS a JOIN r AS b ON a.link=b.id WHERE a.id>=150 AND a.id<160 ORDER BY a.id DESC LIMIT 2",
    );
    assert!(range.key.is_none());
    assert!(range.range.is_some());
    let mut budget = Budget { work: 0, output: 0 };
    assert_eq!(
        select::run(&snapshot, range, &mut budget).unwrap().rows,
        [vec![Value::Integer(159)], vec![Value::Integer(158)]]
    );
    assert_eq!(budget.work, 12);
    let empty = selected(
        &snapshot,
        "SELECT a.id FROM l AS a JOIN r AS b ON a.link=b.id WHERE a.id>=150 AND a.id<150 ORDER BY a.id DESC LIMIT 2",
    );
    let mut budget = Budget { work: 0, output: 0 };
    assert!(
        select::run(&snapshot, empty, &mut budget)
            .unwrap()
            .rows
            .is_empty()
    );
    assert_eq!(budget.work, 0);
}

#[test]
fn right_or_nonprimary_order_does_not_receive_a_unique_source_prefix() {
    let snapshot = fixture(200);
    for order in ["b.id DESC,a.id DESC", "a.link DESC,a.id ASC"] {
        let sql =
            format!("SELECT a.id FROM l AS a JOIN r AS b ON a.link=b.id ORDER BY {order} LIMIT 2");
        let plan = selected(&snapshot, &sql);
        assert_eq!(plan.primary_order, None);
        let mut budget = Budget { work: 0, output: 0 };
        assert_eq!(
            select::run(&snapshot, plan, &mut budget)
                .unwrap()
                .rows
                .len(),
            2
        );
        assert_eq!(budget.work, 600);
    }
    let sql = "SELECT a.id FROM l AS a JOIN r AS b ON a.link=b.id OR TRUE WHERE a.id>=150 ORDER BY a.id DESC LIMIT 2";
    let plan = selected(&snapshot, sql);
    assert!(plan.primary_join.is_none());
    assert!(plan.key.is_none());
    assert!(plan.range.is_none());
    assert!(plan.primary_order.is_none());
}

#[test]
fn false_filters_and_script_output_continue_to_enforce_shared_budgets() {
    let snapshot = fixture(200);
    let plan = selected(
        &snapshot,
        "SELECT a.id FROM l AS a JOIN r AS b ON a.link=b.id WHERE b.id<0 ORDER BY a.id DESC LIMIT 1",
    );
    let mut budget = Budget { work: 0, output: 0 };
    assert!(
        select::run(&snapshot, plan, &mut budget)
            .unwrap()
            .rows
            .is_empty()
    );
    assert_eq!(budget.work, 800);
    let plan = selected(
        &snapshot,
        "SELECT a.id FROM l AS a JOIN r AS b ON a.link=b.id ORDER BY a.id DESC LIMIT 1",
    );
    let mut budget = Budget {
        work: 0,
        output: MAX_OUTPUT_BYTES,
    };
    assert!(matches!(
        select::run(&snapshot, plan, &mut budget),
        Err(ExecutionError::Limit("output bytes"))
    ));
}
