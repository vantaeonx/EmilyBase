use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_commit_model::{
    AdmissionLimit as Bound, AdmissionLimits, AdmissionUsage, AdmittedStage, Error, ModelPool,
    ModelProject, ModelReader,
};
use emilybase_database::{Event, EventKind};
use proptest::prelude::*;
use std::collections::BTreeSet;
use std::sync::{Arc, Barrier};

fn schema(name: &str, data_type: DataType) -> Schema {
    Schema {
        name: name.into(),
        columns: vec![
            Column {
                name: "id".into(),
                data_type,
                nullable: false,
            },
            Column {
                name: "value".into(),
                data_type: DataType::Text,
                nullable: false,
            },
        ],
        primary_key: 0,
    }
}

fn pool(projects: usize, generations: usize, readers: usize, writers: usize) -> ModelPool {
    ModelPool::new(AdmissionLimits::new(projects, generations, readers, writers).unwrap())
}

fn usage(pool: &ModelPool, projects: usize, generations: usize, readers: usize, writers: usize) {
    assert_eq!(
        pool.usage().unwrap(),
        AdmissionUsage {
            projects,
            generations,
            readers,
            writers
        }
    );
}

fn populated(pool: &ModelPool, id: u8) -> ModelProject {
    let mut project = pool.create([id; 16]).unwrap();
    let mut stage = project.begin().unwrap();
    stage
        .apply(Event {
            table_id: 1,
            kind: EventKind::Create(schema("items", DataType::Integer)),
        })
        .unwrap();
    stage
        .apply(Event {
            table_id: 1,
            kind: EventKind::Insert(vec![Value::Integer(1), Value::Text("0".into())]),
        })
        .unwrap();
    stage.rebuild_index("items").unwrap();
    project.publish(stage.prepare().unwrap()).unwrap();
    project
}

fn change(stage: &mut AdmittedStage, value: usize) {
    stage
        .apply(Event {
            table_id: 1,
            kind: EventKind::Replace(vec![Value::Integer(1), Value::Text(value.to_string())]),
        })
        .unwrap();
    stage.rebuild_index("items").unwrap();
}

fn check(reader: &ModelReader, value: usize) {
    assert_eq!(reader.row_count(), 1);
    assert_eq!(
        reader.get("items", &Key::Integer(1)).unwrap().unwrap()[1],
        Value::Text(value.to_string())
    );
    assert!(
        reader
            .row_location("items", &Key::Integer(1))
            .unwrap()
            .is_some()
    );
    assert_eq!(reader.binding(1).unwrap().covered(), 1);
}

#[test]
fn validates_configuration_before_constructing_a_pool() {
    for args in [
        (0, 1, 0, 0),
        (129, 1, 0, 0),
        (1, 0, 0, 0),
        (1, 4097, 0, 0),
        (1, 1, 4097, 0),
        (1, 1, 0, 5),
        (usize::MAX, 1, 0, 0),
        (1, usize::MAX, 0, 0),
        (1, 1, usize::MAX, 0),
        (1, 1, 0, usize::MAX),
    ] {
        assert!(matches!(
            AdmissionLimits::new(args.0, args.1, args.2, args.3),
            Err(Error::AdmissionConfiguration)
        ));
    }
    let limits = AdmissionLimits::new(128, 4096, 4096, 4).unwrap();
    assert_eq!(
        (
            limits.projects(),
            limits.generations(),
            limits.readers(),
            limits.writers()
        ),
        (128, 4096, 4096, 4)
    );
    assert_eq!(ModelPool::new(limits).limits(), limits);
    assert!(AdmissionLimits::new(1, 1, 0, 0).is_ok());
}

#[test]
fn invalid_identity_duplicate_and_capacity_refusals_leave_no_registration() {
    let shared = pool(1, 2, 4, 1);
    assert!(shared.create([0; 16]).is_err());
    usage(&shared, 0, 0, 0, 0);
    let project = shared.create([1; 16]).unwrap();
    assert!(matches!(
        shared.create([1; 16]),
        Err(Error::DuplicateDatabase)
    ));
    assert!(matches!(
        shared.create([2; 16]),
        Err(Error::Admission(Bound::Projects))
    ));
    usage(&shared, 1, 1, 0, 0);
    drop(project);
    let project = shared.create([2; 16]).unwrap();
    drop(project);
    usage(&shared, 0, 0, 0, 0);
    let shared = pool(2, 1, 4, 1);
    let project = shared.create([1; 16]).unwrap();
    assert!(matches!(
        shared.create([2; 16]),
        Err(Error::Admission(Bound::Generations))
    ));
    usage(&shared, 1, 1, 0, 0);
    drop(project);
    assert!(shared.create([2; 16]).is_ok());
    usage(&shared, 0, 0, 0, 0);
}

#[test]
fn cloneable_pool_handles_share_one_ledger_and_namespace() {
    let shared = pool(2, 3, 3, 1);
    let other = shared.clone();
    let project = shared.create([1; 16]).unwrap();
    assert!(matches!(
        other.create([1; 16]),
        Err(Error::DuplicateDatabase)
    ));
    assert_eq!(shared.usage().unwrap(), other.usage().unwrap());
    drop(shared);
    let stage = project.begin().unwrap();
    usage(&other, 1, 2, 0, 1);
    drop(stage);
    drop(project);
    usage(&other, 0, 0, 0, 0);
}

#[test]
fn multiple_readers_share_one_generation_but_every_clone_is_admitted() {
    let shared = pool(1, 3, 2, 1);
    let project = populated(&shared, 1);
    let first = project.read().unwrap();
    let second = first.try_clone().unwrap();
    assert!(matches!(
        first.try_clone(),
        Err(Error::Admission(Bound::Readers))
    ));
    assert!(matches!(
        project.read(),
        Err(Error::Admission(Bound::Readers))
    ));
    usage(&shared, 1, 1, 2, 0);
    assert_eq!(first.fingerprint(), second.fingerprint());
    assert_eq!(first.transaction(), 2);
    assert_eq!(first.database_id(), [1; 16]);
    assert_eq!(first.table_id("items").unwrap(), 1);
    assert_eq!(first.schema("items").unwrap().primary_key, 0);
    assert!(first.binding(99).is_none());
    check(&first, 0);
    drop(first);
    let third = second.try_clone().unwrap();
    usage(&shared, 1, 1, 2, 0);
    check(&third, 0);
    drop(project);
    usage(&shared, 1, 1, 2, 0);
    drop(second);
    drop(third);
    usage(&shared, 0, 0, 0, 0);
}

#[test]
fn disabled_readers_and_writers_refuse_without_reserving_anything() {
    let shared = pool(1, 1, 0, 0);
    let project = shared.create([1; 16]).unwrap();
    assert!(matches!(
        project.read(),
        Err(Error::Admission(Bound::Readers))
    ));
    assert!(matches!(
        project.begin(),
        Err(Error::Admission(Bound::Writers))
    ));
    usage(&shared, 1, 1, 0, 0);
    drop(project);
    usage(&shared, 0, 0, 0, 0);
}

#[test]
fn current_generation_and_pending_stage_are_reserved_before_mutation() {
    let shared = pool(1, 1, 4, 1);
    let project = shared.create([1; 16]).unwrap();
    let before = project.fingerprint();
    assert!(matches!(
        project.begin(),
        Err(Error::Admission(Bound::Generations))
    ));
    usage(&shared, 1, 1, 0, 0);
    assert_eq!(project.transaction(), 1);
    assert_eq!(project.fingerprint(), before);
    let reader = project.read().unwrap();
    assert_eq!(reader.encoded_components().unwrap().history_pages(), 1);
    assert_eq!(reader.row_count(), 0);
}

#[test]
fn prepared_operation_keeps_both_reservations_until_publication_or_drop() {
    let shared = pool(2, 4, 8, 1);
    let mut first = populated(&shared, 1);
    let second = populated(&shared, 2);
    let old = first.read().unwrap();
    let before = first.fingerprint();
    let mut stage = first.begin().unwrap();
    assert_eq!(stage.next_table_id().unwrap(), 2);
    assert_eq!(stage.table_id("items").unwrap(), 1);
    assert_eq!(stage.row_count().unwrap(), 1);
    change(&mut stage, 1);
    assert_eq!(
        stage.get("items", &Key::Integer(1)).unwrap().unwrap()[1],
        Value::Text("1".into())
    );
    let prepared = stage.prepare().unwrap();
    assert_eq!(prepared.row_count(), 1);
    assert_eq!(prepared.transaction(), 3);
    assert_eq!(
        prepared.get("items", &Key::Integer(1)).unwrap().unwrap()[1],
        Value::Text("1".into())
    );
    assert_eq!(prepared.encoded_components().unwrap().roots(), 1);
    usage(&shared, 2, 3, 1, 1);
    assert!(matches!(
        first.begin(),
        Err(Error::Admission(Bound::ProjectWriter))
    ));
    assert!(matches!(
        second.begin(),
        Err(Error::Admission(Bound::Writers))
    ));
    assert_eq!(first.fingerprint(), before);
    first.publish(prepared).unwrap();
    usage(&shared, 2, 3, 1, 0);
    check(&old, 0);
    check(&first.read().unwrap(), 1);
    drop(old);
    usage(&shared, 2, 2, 0, 0);
    let stage = second.begin().unwrap();
    drop(stage);
    usage(&shared, 2, 2, 0, 0);
}

#[test]
fn every_discard_and_error_path_releases_its_writer_and_future_generation() {
    let shared = pool(1, 2, 4, 1);
    let project = populated(&shared, 1);
    let before = project.fingerprint();
    for fault in 0..5 {
        let mut stage = project.begin().unwrap();
        match fault {
            0 => {
                assert!(matches!(stage.prepare(), Err(Error::Empty)));
            }
            1 => {
                assert!(
                    stage
                        .apply(Event {
                            table_id: 1,
                            kind: EventKind::Insert(vec![
                                Value::Integer(1),
                                Value::Text("duplicate".into())
                            ])
                        })
                        .is_err()
                );
                assert!(matches!(stage.row_count(), Err(Error::Aborted)));
                assert!(matches!(stage.prepare(), Err(Error::Aborted)));
            }
            2 => {
                stage
                    .apply(Event {
                        table_id: 1,
                        kind: EventKind::Replace(vec![
                            Value::Integer(1),
                            Value::Text("no root".into()),
                        ]),
                    })
                    .unwrap();
                assert!(stage.prepare().is_err());
            }
            3 => {
                change(&mut stage, 1);
                drop(stage.prepare().unwrap());
            }
            _ => {
                change(&mut stage, 1);
                drop(stage);
            }
        }
        usage(&shared, 1, 1, 0, 0);
        assert_eq!(project.fingerprint(), before);
        check(&project.read().unwrap(), 0);
    }
}

#[test]
fn retained_old_generations_exhaust_capacity_and_last_reader_releases_it() {
    let shared = pool(1, 3, 8, 1);
    let mut project = populated(&shared, 1);
    let first = project.read().unwrap();
    let duplicate = first.try_clone().unwrap();
    let mut stage = project.begin().unwrap();
    change(&mut stage, 1);
    project.publish(stage.prepare().unwrap()).unwrap();
    let second = project.read().unwrap();
    let mut stage = project.begin().unwrap();
    change(&mut stage, 2);
    project.publish(stage.prepare().unwrap()).unwrap();
    usage(&shared, 1, 3, 3, 0);
    let before = project.fingerprint();
    assert!(matches!(
        project.begin(),
        Err(Error::Admission(Bound::Generations))
    ));
    drop(first);
    usage(&shared, 1, 3, 2, 0);
    assert!(matches!(
        project.begin(),
        Err(Error::Admission(Bound::Generations))
    ));
    check(&duplicate, 0);
    check(&second, 1);
    assert_eq!(project.fingerprint(), before);
    drop(duplicate);
    usage(&shared, 1, 2, 1, 0);
    let mut stage = project.begin().unwrap();
    change(&mut stage, 3);
    project.publish(stage.prepare().unwrap()).unwrap();
    usage(&shared, 1, 2, 1, 0);
    check(&project.read().unwrap(), 3);
    drop(second);
    drop(project);
    usage(&shared, 0, 0, 0, 0);
}

#[test]
fn dropped_project_identity_is_pinned_by_readers_stages_and_prepared_operations() {
    for descendant in 0..3 {
        let shared = pool(1, 2, 4, 1);
        let project = populated(&shared, 1);
        let mut reader = None;
        let mut stage = None;
        let mut prepared = None;
        match descendant {
            0 => reader = Some(project.read().unwrap()),
            1 => stage = Some(project.begin().unwrap()),
            _ => {
                let mut next = project.begin().unwrap();
                change(&mut next, 1);
                prepared = Some(next.prepare().unwrap());
            }
        }
        drop(project);
        assert!(matches!(
            shared.create([1; 16]),
            Err(Error::DuplicateDatabase)
        ));
        assert!(matches!(
            shared.create([2; 16]),
            Err(Error::Admission(Bound::Projects))
        ));
        drop(reader);
        drop(stage);
        drop(prepared);
        usage(&shared, 0, 0, 0, 0);
        let recreated = shared.create([1; 16]).unwrap();
        assert_eq!(recreated.transaction(), 1);
        assert_eq!(recreated.read().unwrap().row_count(), 0);
    }
}

#[test]
fn equal_fingerprints_in_foreign_pools_do_not_authorize_publication() {
    let left = pool(1, 2, 4, 1);
    let right = pool(1, 2, 4, 1);
    let mut a = populated(&left, 1);
    let b = populated(&right, 1);
    assert_eq!(a.fingerprint(), b.fingerprint());
    let before = a.fingerprint();
    let mut stage = b.begin().unwrap();
    change(&mut stage, 1);
    assert!(matches!(
        a.publish(stage.prepare().unwrap()),
        Err(Error::Conflict)
    ));
    usage(&left, 1, 1, 0, 0);
    usage(&right, 1, 1, 0, 0);
    assert_eq!(a.fingerprint(), before);
    check(&a.read().unwrap(), 0);
    check(&b.read().unwrap(), 0);
    let shared = pool(2, 3, 4, 1);
    let mut a = populated(&shared, 1);
    let b = populated(&shared, 2);
    let mut stage = b.begin().unwrap();
    change(&mut stage, 2);
    assert!(matches!(
        a.publish(stage.prepare().unwrap()),
        Err(Error::Conflict)
    ));
    usage(&shared, 2, 2, 0, 0);
    check(&a.read().unwrap(), 0);
}

#[test]
fn automatic_roots_cover_nonfirst_text_primary_keys_and_long_key_exclusions() {
    let shared = pool(1, 3, 4, 1);
    let mut project = shared.create([1; 16]).unwrap();
    let mut schema = schema("items", DataType::Text);
    schema.columns.swap(0, 1);
    schema.primary_key = 1;
    let mut stage = project.begin().unwrap();
    stage
        .apply(Event {
            table_id: 1,
            kind: EventKind::Create(schema),
        })
        .unwrap();
    let long = "λ".repeat(1536);
    for key in ["a\0λ".to_owned(), long.clone()] {
        stage
            .apply(Event {
                table_id: 1,
                kind: EventKind::Insert(vec![Value::Text("value".into()), Value::Text(key)]),
            })
            .unwrap();
    }
    stage.rebuild_index("items").unwrap();
    project.publish(stage.prepare().unwrap()).unwrap();
    let old = project.read().unwrap();
    assert_eq!(
        (
            old.binding(1).unwrap().covered(),
            old.binding(1).unwrap().excluded()
        ),
        (1, 1)
    );
    let mut stage = project.begin().unwrap();
    stage
        .apply(Event {
            table_id: 1,
            kind: EventKind::Delete(Key::Text(long.clone())),
        })
        .unwrap();
    stage.rebuild_index("items").unwrap();
    project.publish(stage.prepare().unwrap()).unwrap();
    let current = project.read().unwrap();
    assert!(
        old.get("items", &Key::Text(long.clone()))
            .unwrap()
            .is_some()
    );
    assert!(current.get("items", &Key::Text(long)).unwrap().is_none());
    assert_eq!(current.binding(1).unwrap().revision(), 2);
    assert_eq!(current.binding(1).unwrap().excluded(), 0);
}

#[test]
fn missing_and_duplicate_rebuilds_abort_the_whole_stage() {
    let shared = pool(1, 2, 4, 1);
    let project = populated(&shared, 1);
    let before = project.fingerprint();
    for missing in [true, false] {
        let mut stage = project.begin().unwrap();
        if missing {
            assert!(stage.rebuild_index("missing").is_err());
        } else {
            stage.rebuild_index("items").unwrap();
            assert!(stage.rebuild_index("items").is_err());
        }
        assert!(matches!(
            stage.get("items", &Key::Integer(1)),
            Err(Error::Aborted)
        ));
        assert!(matches!(stage.prepare(), Err(Error::Aborted)));
        usage(&shared, 1, 1, 0, 0);
        assert_eq!(project.fingerprint(), before);
    }
}

#[test]
fn unwinding_releases_retained_readers_and_prepared_state() {
    let shared = pool(1, 2, 4, 1);
    let result = std::panic::catch_unwind(|| {
        let project = populated(&shared, 1);
        let _reader = project.read().unwrap();
        let mut stage = project.begin().unwrap();
        change(&mut stage, 1);
        let _prepared = stage.prepare().unwrap();
        panic!("synthetic unwind");
    });
    assert!(result.is_err());
    usage(&shared, 0, 0, 0, 0);
    assert!(shared.create([1; 16]).is_ok());
}

#[test]
fn simultaneous_projects_reserve_exactly_four_writers_and_release_for_retry() {
    let shared = pool(8, 12, 16, 4);
    let projects: Vec<_> = (1..=8).map(|id| Arc::new(populated(&shared, id))).collect();
    let acquired = Arc::new(Barrier::new(9));
    let release = Arc::new(Barrier::new(9));
    let workers: Vec<_> = projects
        .iter()
        .map(|project| {
            let project = Arc::clone(project);
            let acquired = Arc::clone(&acquired);
            let release = Arc::clone(&release);
            std::thread::spawn(move || {
                let stage = match project.begin() {
                    Ok(stage) => Some(stage),
                    Err(Error::Admission(Bound::Writers)) => None,
                    Err(error) => panic!("unexpected admission: {error}"),
                };
                acquired.wait();
                release.wait();
                let accepted = stage.is_some();
                drop(stage);
                accepted
            })
        })
        .collect();
    acquired.wait();
    usage(&shared, 8, 12, 0, 4);
    release.wait();
    assert_eq!(
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .filter(|accepted| *accepted)
            .count(),
        4
    );
    usage(&shared, 8, 8, 0, 0);
    for project in &projects {
        drop(project.begin().unwrap());
    }
    drop(projects);
    usage(&shared, 0, 0, 0, 0);
}

#[test]
fn same_project_writer_and_reader_admission_are_atomic_under_contention() {
    let shared = pool(1, 2, 4, 4);
    let project = Arc::new(populated(&shared, 1));
    let acquired = Arc::new(Barrier::new(9));
    let release = Arc::new(Barrier::new(9));
    let workers: Vec<_> = (0..8)
        .map(|_| {
            let project = Arc::clone(&project);
            let acquired = Arc::clone(&acquired);
            let release = Arc::clone(&release);
            std::thread::spawn(move || {
                let stage = match project.begin() {
                    Ok(value) => Some(value),
                    Err(Error::Admission(Bound::ProjectWriter)) => None,
                    Err(error) => panic!("unexpected admission: {error}"),
                };
                let reader = match project.read() {
                    Ok(value) => Some(value),
                    Err(Error::Admission(Bound::Readers)) => None,
                    Err(error) => panic!("unexpected reader admission: {error}"),
                };
                if let Some(reader) = &reader {
                    check(reader, 0);
                }
                acquired.wait();
                release.wait();
                (stage.is_some(), reader.is_some())
            })
        })
        .collect();
    acquired.wait();
    usage(&shared, 1, 2, 4, 1);
    release.wait();
    let results: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(results.iter().filter(|result| result.0).count(), 1);
    assert_eq!(results.iter().filter(|result| result.1).count(), 4);
    usage(&shared, 1, 1, 0, 0);
    drop(project);
    usage(&shared, 0, 0, 0, 0);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn retained_generation_and_reservation_counts_match_an_independent_sequence(
        actions in prop::collection::vec((0u8..7, 0usize..3), 1..80)
    ) {
        let shared = pool(3, 7, 8, 2);
        let mut projects: Vec<_> = (1..=3).map(|id| populated(&shared, id)).collect();
        let mut transactions = [2u64; 3];
        let mut values = [0usize; 3];
        let mut stages: Vec<Option<AdmittedStage>> = (0..3).map(|_| None).collect();
        let mut readers: Vec<(usize, u64, usize, ModelReader)> = Vec::new();
        for (action, index) in actions {
            let distinct: BTreeSet<_> = transactions.iter().copied().enumerate()
                .chain(readers.iter().map(|(index, tx, _, _)| (*index, *tx))).collect();
            let writers = stages.iter().filter(|stage| stage.is_some()).count();
            let expected = AdmissionUsage { projects: 3, generations: distinct.len() + writers, readers: readers.len(), writers };
            prop_assert_eq!(shared.usage().unwrap(), expected);
            match action {
                0 => {
                    let result = projects[index].read();
                    if readers.len() == 8 { prop_assert!(matches!(result, Err(Error::Admission(Bound::Readers)))); }
                    else { readers.push((index, transactions[index], values[index], result.unwrap())); }
                }
                1 => {
                    let result = projects[index].begin();
                    let refusal = if stages[index].is_some() { Some(Bound::ProjectWriter) }
                        else if writers == 2 { Some(Bound::Writers) }
                        else if expected.generations == 7 { Some(Bound::Generations) } else { None };
                    if let Some(bound) = refusal { prop_assert!(matches!(result, Err(Error::Admission(actual)) if actual == bound)); }
                    else { let mut stage = result.unwrap(); change(&mut stage, values[index] + 1); stages[index] = Some(stage); }
                }
                2 => if let Some(stage) = stages[index].take() {
                    projects[index].publish(stage.prepare().unwrap()).unwrap();
                    transactions[index] += 1; values[index] += 1;
                },
                3 => { drop(stages[index].take()); }
                4 => if !readers.is_empty() { drop(readers.remove(index % readers.len())); },
                5 => if !readers.is_empty() {
                    let (project, tx, value, reader) = &readers[index % readers.len()];
                    let result = reader.try_clone();
                    if readers.len() == 8 { prop_assert!(matches!(result, Err(Error::Admission(Bound::Readers)))); }
                    else { readers.push((*project, *tx, *value, result.unwrap())); }
                },
                _ => if let Some(stage) = stages[index].take() { drop(stage.prepare().unwrap()); },
            }
            for (project, tx, value, reader) in &readers {
                prop_assert_eq!(reader.database_id(), [(*project + 1) as u8; 16]);
                prop_assert_eq!(reader.transaction(), *tx);
                check(reader, *value);
            }
            for (index, project) in projects.iter().enumerate() {
                prop_assert_eq!(project.transaction(), transactions[index]);
            }
        }
        drop(stages); drop(readers); drop(projects);
        prop_assert_eq!(shared.usage().unwrap(), AdmissionUsage { projects: 0, generations: 0, readers: 0, writers: 0 });
    }
}

#[test]
fn dropped_tables_and_same_name_recreation_keep_old_reader_scope_distinct() {
    let shared = pool(1, 3, 4, 1);
    let mut project = populated(&shared, 1);
    let old = project.read().unwrap();
    let mut stage = project.begin().unwrap();
    stage
        .apply(Event {
            table_id: 1,
            kind: EventKind::Drop,
        })
        .unwrap();
    let next_id = stage.next_table_id().unwrap();
    assert_eq!(next_id, 2);
    stage
        .apply(Event {
            table_id: next_id,
            kind: EventKind::Create(schema("items", DataType::Integer)),
        })
        .unwrap();
    stage
        .apply(Event {
            table_id: next_id,
            kind: EventKind::Insert(vec![Value::Integer(1), Value::Text("1".into())]),
        })
        .unwrap();
    stage.rebuild_index("items").unwrap();
    project.publish(stage.prepare().unwrap()).unwrap();
    let current = project.read().unwrap();
    assert_eq!(old.table_id("items").unwrap(), 1);
    assert_eq!(current.table_id("items").unwrap(), 2);
    check(&old, 0);
    assert_eq!(
        current.get("items", &Key::Integer(1)).unwrap().unwrap()[1],
        Value::Text("1".into())
    );
    assert!(current.binding(1).is_none());
    assert_eq!(current.binding(2).unwrap().revision(), 1);
    assert!(current.binding(2).unwrap().predecessor().is_none());
    assert_ne!(
        old.row_location("items", &Key::Integer(1)).unwrap(),
        current.row_location("items", &Key::Integer(1)).unwrap()
    );
    usage(&shared, 1, 2, 2, 0);
    drop(old);
    drop(current);
    drop(project);
    usage(&shared, 0, 0, 0, 0);
}
