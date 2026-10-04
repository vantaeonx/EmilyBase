use super::*;
use emilybase_catalog::Key;

#[test]
fn mutation_selection_rejects_over_capacity_with_bounded_memory_growth() {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "execute::matching_memory::bounded_mutations_child",
            "--ignored",
            "--nocapture",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "child exited {:?}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "isolated child entry for mutation memory bounds"]
fn bounded_mutations_child() {
    let temporary = tempfile::tempdir().unwrap();
    let mut database = Database::create(temporary.path().join("synthetic")).unwrap();
    execute(
        &mut database,
        "CREATE TABLE t(id INT PRIMARY KEY,payload TEXT)",
        &[],
    )
    .unwrap();
    for start in (0..6000).step_by(200) {
        let tuples = (start..start + 200)
            .map(|id| format!("({id},$1)"))
            .collect::<Vec<_>>()
            .join(",");
        execute(
            &mut database,
            &format!("INSERT INTO t VALUES {tuples}"),
            &[Value::Text("x".repeat(3072))],
        )
        .unwrap();
    }
    let transaction = database.begin().unwrap();
    // Snapshot staging is outside selection's headroom and owns its normal copy.
    transaction
        .view()
        .unwrap()
        .get("t", &Key::Integer(0))
        .unwrap();
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    let size = |name: &str| {
        status
            .lines()
            .find_map(|line| line.strip_prefix(name))
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            * 1024
    };
    for (resource, current) in [
        (rustix::process::Resource::Core, 0),
        (
            rustix::process::Resource::As,
            size("VmSize:") + 8 * 1024 * 1024,
        ),
        (
            rustix::process::Resource::Data,
            size("VmData:") + 8 * 1024 * 1024,
        ),
    ] {
        rustix::process::setrlimit(
            resource,
            rustix::process::Rlimit {
                current: Some(current),
                maximum: Some(current),
            },
        )
        .unwrap();
    }
    let mut budget = Budget { work: 0, output: 0 };
    let result = matching(&transaction, "t", &None, &[], &mut budget, |schema, row| {
        Ok((schema.key(row)?, row.clone()))
    });
    assert!(matches!(
        result,
        Err(ExecutionError::Transaction(
            emilybase_transactions::Error::Limit
        ))
    ));
    assert_eq!(budget.work, 257);
    // Deletion collects only keys; it shares the exact event-capacity boundary.
    let mut budget = Budget { work: 0, output: 0 };
    let result = matching(&transaction, "t", &None, &[], &mut budget, |schema, row| {
        Ok(schema.key(row)?)
    });
    assert!(matches!(
        result,
        Err(ExecutionError::Transaction(
            emilybase_transactions::Error::Limit
        ))
    ));
    assert_eq!(budget.work, 257);
    let Statement::Delete { filter, .. } = parse("DELETE FROM t WHERE id<2").unwrap().remove(0)
    else {
        panic!("delete fixture")
    };
    let mut budget = Budget { work: 0, output: 0 };
    let selected = matching(
        &transaction,
        "t",
        &filter,
        &[],
        &mut budget,
        |schema, row| Ok(schema.key(row)?),
    )
    .unwrap();
    assert_eq!(selected, [Key::Integer(0), Key::Integer(1)]);
    assert_eq!(budget.work, 4);
    assert_eq!(transaction.view().unwrap().row_count(), 6000);
    transaction.rollback();
}
