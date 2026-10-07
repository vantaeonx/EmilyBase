use super::*;
use emilybase_database::{Event, EventKind, Snapshot};

fn fixture() -> Snapshot {
    let mut view = Snapshot::empty().unwrap();
    for (table_id, name) in [(1, "l"), (2, "r")] {
        view.apply(Event {
            table_id,
            kind: EventKind::Create(Schema {
                name: name.into(),
                columns: vec![
                    Column {
                        name: "id".into(),
                        data_type: emilybase_catalog::DataType::Integer,
                        nullable: false,
                    },
                    Column {
                        name: "rank".into(),
                        data_type: emilybase_catalog::DataType::Integer,
                        nullable: false,
                    },
                ],
                primary_key: 0,
            }),
        })
        .unwrap();
        for id in 0..3 {
            view.apply(Event {
                table_id,
                kind: EventKind::Insert(vec![Value::Integer(id), Value::Integer(id % 2)]),
            })
            .unwrap();
        }
    }
    view
}
fn plan(view: &Snapshot, tail: &str) -> Plan {
    let sql = format!("SELECT a.id,b.id FROM l AS a JOIN r AS b ON a.rank=b.rank {tail}");
    let statements = parse(&sql).unwrap();
    let Statement::Select(select) = &statements[0] else {
        panic!("select fixture")
    };
    let plan = Plan::compile(view, select, &[]).unwrap();
    assert!(plan.primary_join.is_none());
    plan
}

#[test]
fn fallback_retains_original_pair_predicate_filter_work_and_stable_sort_order() {
    let view = fixture();
    let mut budget = Budget { work: 0, output: 0 };
    let result = crate::select::run(
        &view,
        plan(&view, "WHERE b.id>0 ORDER BY b.id LIMIT 2"),
        &mut budget,
    )
    .unwrap();
    assert_eq!(budget.work, 23);
    assert_eq!(budget.output, 176);
    assert_eq!(
        result.rows,
        [
            vec![Value::Integer(1), Value::Integer(1)],
            vec![Value::Integer(0), Value::Integer(2)]
        ]
    );
    let mut budget = Budget { work: 0, output: 0 };
    let result = crate::select::run(&view, plan(&view, "LIMIT 2"), &mut budget).unwrap();
    assert_eq!(budget.work, 6);
    assert_eq!(
        result.rows,
        [
            vec![Value::Integer(0), Value::Integer(0)],
            vec![Value::Integer(0), Value::Integer(2)]
        ]
    );
}

#[test]
fn left_filters_keep_original_full_fallback_work_including_contradictions() {
    let view = fixture();
    for (tail, work, count) in [
        ("WHERE a.id=2", 23, 2),
        ("WHERE a.id>=1 AND a.id<3", 33, 3),
        ("WHERE a.id>=3 AND a.id<1", 33, 0),
    ] {
        let mut budget = Budget { work: 0, output: 0 };
        let plan = plan(&view, tail);
        assert!(plan.key.is_none() && plan.range.is_none());
        let result = crate::select::run(&view, plan, &mut budget).unwrap();
        assert_eq!((budget.work, result.rows.len()), (work, count), "{tail}");
    }
}

#[test]
fn internally_bound_point_range_sources_still_execute_the_complete_filter() {
    let view = fixture();
    for (tail, work, count) in [
        ("WHERE a.id=2", 8, 2),
        ("WHERE a.id>=1 AND a.id<3", 21, 3),
        ("WHERE a.id>=3 AND a.id<1", 0, 0),
    ] {
        let mut plan = plan(&view, tail);
        let filter = plan.filter.as_ref().unwrap();
        plan.key = primary_key(filter, 0);
        if plan.key.is_none() {
            plan.range = crate::range::primary_range(filter, 0);
        }
        let mut budget = Budget { work: 0, output: 0 };
        let result = crate::select::run(&view, plan, &mut budget).unwrap();
        assert_eq!((budget.work, result.rows.len()), (work, count), "{tail}");
    }
}

#[test]
fn false_on_does_not_skip_boolean_branches_or_evaluate_where_for_rejected_pairs() {
    let view = fixture();
    let mut plan = plan(&view, "WHERE b.id>0");
    let (_, predicate) = plan.join.as_mut().unwrap();
    *predicate = Predicate::And(
        Box::new(Predicate::Truth(crate::predicate::BoundOperand::Value(
            Value::Boolean(false),
        ))),
        Box::new(Predicate::Compare(
            crate::predicate::BoundOperand::Column(1),
            Compare::Eq,
            crate::predicate::BoundOperand::Column(3),
        )),
    );
    let mut budget = Budget { work: 0, output: 0 };
    assert!(
        crate::select::run(&view, plan, &mut budget)
            .unwrap()
            .rows
            .is_empty()
    );
    assert_eq!((budget.work, budget.output), (36, 0));
}

#[test]
fn work_and_shared_output_refusals_keep_prior_charge_timing() {
    let view = fixture();
    let before = view.page_fingerprint();
    let mut budget = Budget {
        work: MAX_QUERY_WORK - 1,
        output: 17,
    };
    assert!(matches!(
        crate::select::run(&view, plan(&view, "LIMIT 2"), &mut budget),
        Err(ExecutionError::Limit("query work"))
    ));
    assert_eq!((budget.work, budget.output), (MAX_QUERY_WORK + 1, 17));
    let mut exact = Budget {
        work: 0,
        output: MAX_OUTPUT_BYTES - 176,
    };
    assert_eq!(
        crate::select::run(&view, plan(&view, "LIMIT 2"), &mut exact)
            .unwrap()
            .rows
            .len(),
        2
    );
    assert_eq!(exact.output, MAX_OUTPUT_BYTES);
    let mut refused = Budget {
        work: 0,
        output: MAX_OUTPUT_BYTES - 175,
    };
    assert!(matches!(
        crate::select::run(&view, plan(&view, "LIMIT 2"), &mut refused),
        Err(ExecutionError::Limit("output bytes"))
    ));
    assert_eq!((refused.work, refused.output), (6, MAX_OUTPUT_BYTES + 1));
    assert_eq!(view.page_fingerprint(), before);
}
