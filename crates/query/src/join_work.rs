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
