use std::collections::BTreeMap;
use std::sync::{Arc, Barrier};

use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_commit_model::{
    AdmissionLimit, AdmissionLimits, AdmissionUsage, AdmittedPlan, DecodedPlanLimits,
    DecodedPlanPool, Error, Model, ModelPool, ModelProject,
};
use emilybase_database::{Event, EventKind};
use proptest::prelude::*;

fn schema(primary: DataType) -> Schema {
    Schema {
        name: "items".into(),
        primary_key: 0,
        columns: vec![
            Column {
                name: "id".into(),
                data_type: primary,
                nullable: false,
            },
            Column {
                name: "value".into(),
                data_type: DataType::Text,
                nullable: false,
            },
        ],
    }
}
fn create(primary: DataType) -> Event {
    Event {
        table_id: 1,
        kind: EventKind::Create(schema(primary)),
    }
}
fn insert(key: Value, text: &str) -> Event {
    Event {
        table_id: 1,
        kind: EventKind::Insert(vec![key, Value::Text(text.into())]),
    }
}
fn plan(base: &mut Model, events: impl IntoIterator<Item = Event>, rebuild: bool) -> AdmittedPlan {
    let mut stage = base.begin().unwrap();
    for event in events {
        stage.apply(event).unwrap();
    }
    if rebuild {
        stage.rebuild_index("items").unwrap();
    }
    let prepared = stage.prepare().unwrap();
    let bytes = prepared.image_plan().unwrap().encode().unwrap();
    let pool = DecodedPlanPool::new(DecodedPlanLimits::new(1, 2 * 1024 * 1024).unwrap());
    let admitted = pool.decode(&bytes).unwrap();
    base.publish(prepared).unwrap();
    admitted
}
fn initialized(pool: &ModelPool, id: [u8; 16]) -> (ModelProject, Model) {
    let mut project = pool.create(id).unwrap();
    let mut raw = Model::new(id).unwrap();
    let input = plan(&mut raw, [create(DataType::Integer)], true);
    project
        .publish_replayed(project.replay(&input).unwrap())
        .unwrap();
    assert_eq!(project.fingerprint(), raw.fingerprint());
    (project, raw)
}
fn pool(generations: usize, readers: usize, writers: usize) -> ModelPool {
    ModelPool::new(AdmissionLimits::new(8, generations, readers, writers).unwrap())
}

#[test]
fn replay_reserves_until_publish_and_keeps_old_rows_and_current_locations() {
    let pool = pool(4, 2, 1);
    let (mut project, mut raw) = initialized(&pool, [7; 16]);
    let input = plan(&mut raw, [insert(Value::Integer(11), "new")], true);
    let old = project.read().unwrap();
    let old_fingerprint = project.fingerprint();
    let output = project.replay(&input).unwrap();
    assert_eq!(
        pool.usage().unwrap(),
        AdmissionUsage {
            projects: 1,
            generations: 2,
            readers: 1,
            writers: 1
        }
    );
    assert_eq!(project.fingerprint(), old_fingerprint);
    assert_eq!(output.database_id(), [7; 16]);
    assert_eq!(output.transaction(), raw.transaction());
    assert_eq!(output.fingerprint(), raw.fingerprint());
    assert_eq!(output.row_count(), 1);
    assert_eq!(output.table_id("items").unwrap(), 1);
    assert_eq!(output.schema("items").unwrap(), &schema(DataType::Integer));
    assert_eq!(
        output.encoded_components().unwrap(),
        raw.encoded_components().unwrap()
    );
    assert_eq!(output.binding(1), raw.selection(1).map(|s| s.binding()));
    assert_eq!(output.binding(2), None);
    let key = Key::Integer(11);
    assert_eq!(
        output.get("items", &key).unwrap(),
        raw.view().get("items", &key).unwrap()
    );
    assert_eq!(
        output.row_location("items", &key).unwrap(),
        raw.view().row_location("items", &key).unwrap()
    );
    assert!(matches!(
        project.begin(),
        Err(Error::Admission(AdmissionLimit::ProjectWriter))
    ));
    assert!(matches!(
        project.replay(&input),
        Err(Error::Admission(AdmissionLimit::ProjectWriter))
    ));
    drop(input);
    project.publish_replayed(output).unwrap();
    assert_eq!(project.fingerprint(), raw.fingerprint());
    assert_eq!(old.row_count(), 0);
    assert!(old.get("items", &key).unwrap().is_none());
    assert_eq!(pool.usage().unwrap().writers, 0);
    assert_eq!(pool.usage().unwrap().generations, 2);
    drop(old);
    assert_eq!(pool.usage().unwrap().generations, 1);
}

#[test]
fn discard_releases_only_pending_state_and_replays_the_same_source_again() {
    let pool = pool(2, 1, 1);
    let (project, mut raw) = initialized(&pool, [7; 16]);
    let before = project.fingerprint();
    let input = plan(&mut raw, [insert(Value::Integer(1), "a")], true);
    let output = project.replay(&input).unwrap();
    assert_eq!(pool.usage().unwrap().generations, 2);
    drop(output);
    assert_eq!(pool.usage().unwrap().generations, 1);
    assert_eq!(pool.usage().unwrap().writers, 0);
    assert_eq!(project.fingerprint(), before);
    assert_eq!(
        project.replay(&input).unwrap().fingerprint(),
        raw.fingerprint()
    );
}

#[test]
fn source_lifetime_is_separate_and_pending_output_keeps_its_project_namespace() {
    let pool = pool(2, 0, 1);
    let (project, mut raw) = initialized(&pool, [7; 16]);
    let input = plan(&mut raw, [insert(Value::Integer(1), "a")], true);
    let decoded = input.reserved_vector_bytes();
    assert!(decoded > 4096);
    let output = project.replay(&input).unwrap();
    drop(input);
    drop(project);
    assert_eq!(
        pool.usage().unwrap(),
        AdmissionUsage {
            projects: 1,
            generations: 2,
            readers: 0,
            writers: 1
        }
    );
    assert!(matches!(
        pool.create([7; 16]),
        Err(Error::DuplicateDatabase)
    ));
    assert_eq!(output.row_count(), 1);
    drop(output);
    assert_eq!(
        pool.usage().unwrap(),
        AdmissionUsage {
            projects: 0,
            generations: 0,
            readers: 0,
            writers: 0
        }
    );
    assert!(pool.create([7; 16]).is_ok());
}

#[test]
fn equal_states_in_foreign_pools_never_accept_another_outputs_generation() {
    let first = pool(2, 0, 1);
    let second = pool(2, 0, 1);
    let (one, mut raw) = initialized(&first, [7; 16]);
    let (mut two, _) = initialized(&second, [7; 16]);
    assert_eq!(one.fingerprint(), two.fingerprint());
    let before = two.fingerprint();
    let input = plan(&mut raw, [insert(Value::Integer(1), "a")], true);
    let output = one.replay(&input).unwrap();
    assert!(matches!(two.publish_replayed(output), Err(Error::Conflict)));
    assert_eq!(two.fingerprint(), before);
    assert_eq!(first.usage().unwrap().generations, 1);
    assert_eq!(first.usage().unwrap().writers, 0);
    assert_eq!(second.usage().unwrap().generations, 1);
}

#[test]
fn wrong_database_and_stale_predecessor_fail_without_changing_any_lease() {
    let pool = pool(4, 1, 1);
    let (mut project, mut raw) = initialized(&pool, [7; 16]);
    let mut foreign = Model::new([8; 16]).unwrap();
    let wrong = plan(&mut foreign, [create(DataType::Integer)], true);
    let usage = pool.usage().unwrap();
    let before = project.fingerprint();
    assert!(matches!(project.replay(&wrong), Err(Error::Conflict)));
    assert_eq!(pool.usage().unwrap(), usage);
    assert_eq!(project.fingerprint(), before);
    let input = plan(&mut raw, [insert(Value::Integer(1), "a")], true);
    project
        .publish_replayed(project.replay(&input).unwrap())
        .unwrap();
    let usage = pool.usage().unwrap();
    assert!(matches!(project.replay(&input), Err(Error::Conflict)));
    assert_eq!(pool.usage().unwrap(), usage);
    assert_eq!(project.fingerprint(), raw.fingerprint());
}

#[test]
fn slot_refusal_precedes_replay_and_preserves_existing_readers() {
    let pool = pool(2, 1, 1);
    let (mut project, mut raw) = initialized(&pool, [7; 16]);
    let old = project.read().unwrap();
    let first = plan(&mut raw, [insert(Value::Integer(1), "a")], true);
    project
        .publish_replayed(project.replay(&first).unwrap())
        .unwrap();
    let next = plan(&mut raw, [insert(Value::Integer(2), "b")], true);
    let before = project.fingerprint();
    assert!(matches!(
        project.replay(&next),
        Err(Error::Admission(AdmissionLimit::Generations))
    ));
    assert_eq!(pool.usage().unwrap().writers, 0);
    assert_eq!(old.row_count(), 0);
    assert_eq!(project.fingerprint(), before);
    drop(old);
    assert_eq!(
        project.replay(&next).unwrap().fingerprint(),
        raw.fingerprint()
    );
}

#[test]
fn disabled_writer_refuses_even_a_valid_source_and_an_exact_base() {
    let pool = pool(2, 0, 0);
    let project = pool.create([7; 16]).unwrap();
    let mut raw = Model::new([7; 16]).unwrap();
    let input = plan(&mut raw, [create(DataType::Integer)], true);
    let usage = pool.usage().unwrap();
    assert!(matches!(
        project.replay(&input),
        Err(Error::Admission(AdmissionLimit::Writers))
    ));
    assert_eq!(pool.usage().unwrap(), usage);
    assert_eq!(project.transaction(), 1);
}

#[test]
fn live_stage_and_replay_share_one_project_writer_exclusion() {
    let pool = pool(3, 1, 1);
    let (mut project, mut raw) = initialized(&pool, [7; 16]);
    let input = plan(&mut raw, [insert(Value::Integer(1), "a")], true);
    let stage = project.begin().unwrap();
    assert!(matches!(
        project.replay(&input),
        Err(Error::Admission(AdmissionLimit::ProjectWriter))
    ));
    drop(stage);
    project
        .publish_replayed(project.replay(&input).unwrap())
        .unwrap();
    let mut stage = project.begin().unwrap();
    stage.apply(insert(Value::Integer(2), "b")).unwrap();
    stage.rebuild_index("items").unwrap();
    project.publish(stage.prepare().unwrap()).unwrap();
    assert_eq!(project.read().unwrap().row_count(), 2);
}

#[test]
fn text_unicode_long_exclusions_updates_and_retirements_replay_through_the_pool() {
    let pool = pool(4, 2, 1);
    let mut project = pool.create([7; 16]).unwrap();
    let mut raw = Model::new([7; 16]).unwrap();
    let short = "я".repeat(128);
    let long = "я".repeat(1536);
    let input = plan(
        &mut raw,
        [
            create(DataType::Text),
            insert(Value::Text(short.clone()), "a"),
            insert(Value::Text(long.clone()), "b"),
        ],
        true,
    );
    project
        .publish_replayed(project.replay(&input).unwrap())
        .unwrap();
    let old = project.read().unwrap();
    assert_eq!(
        (
            old.binding(1).unwrap().covered(),
            old.binding(1).unwrap().excluded()
        ),
        (1, 1)
    );
    let input = plan(
        &mut raw,
        [
            Event {
                table_id: 1,
                kind: EventKind::Replace(vec![
                    Value::Text(long.clone()),
                    Value::Text("changed".into()),
                ]),
            },
            Event {
                table_id: 1,
                kind: EventKind::Delete(Key::Text(short.clone())),
            },
        ],
        true,
    );
    project
        .publish_replayed(project.replay(&input).unwrap())
        .unwrap();
    assert_eq!(
        project
            .read()
            .unwrap()
            .get("items", &Key::Text(long.clone()))
            .unwrap()
            .unwrap()[1],
        Value::Text("changed".into())
    );
    assert_eq!(
        old.get("items", &Key::Text(long)).unwrap().unwrap()[1],
        Value::Text("b".into())
    );
    drop(old);
    let input = plan(
        &mut raw,
        [Event {
            table_id: 1,
            kind: EventKind::Drop,
        }],
        false,
    );
    project
        .publish_replayed(project.replay(&input).unwrap())
        .unwrap();
    let reader = project.read().unwrap();
    assert_eq!(reader.row_count(), 0);
    assert_eq!(reader.binding(1), None);
    assert!(reader.table_id("items").is_err());
}

#[test]
fn actual_eight_thread_replays_share_the_global_writer_cap() {
    let pool = ModelPool::new(AdmissionLimits::new(8, 10, 0, 2).unwrap());
    let mut projects = Vec::new();
    let mut inputs = Vec::new();
    for number in 1..=8 {
        projects.push(pool.create([number; 16]).unwrap());
        let mut raw = Model::new([number; 16]).unwrap();
        inputs.push(plan(
            &mut raw,
            [create(DataType::Integer), insert(Value::Integer(1), "a")],
            true,
        ));
    }
    let start = Arc::new(Barrier::new(9));
    let held = Arc::new(Barrier::new(9));
    let release = Arc::new(Barrier::new(9));
    std::thread::scope(|scope| {
        let handles: Vec<_> = projects
            .iter()
            .zip(inputs.iter())
            .map(|(project, input)| {
                let (start, held, release) =
                    (Arc::clone(&start), Arc::clone(&held), Arc::clone(&release));
                scope.spawn(move || {
                    start.wait();
                    let result = project.replay(input);
                    held.wait();
                    release.wait();
                    match result {
                        Ok(output) => {
                            assert_eq!(output.row_count(), 1);
                            true
                        }
                        Err(Error::Admission(AdmissionLimit::Writers)) => false,
                        Err(error) => panic!("unexpected replay admission {error}"),
                    }
                })
            })
            .collect();
        start.wait();
        held.wait();
        let usage = pool.usage().unwrap();
        release.wait();
        let accepted = handles
            .into_iter()
            .map(|h| h.join().unwrap() as usize)
            .sum::<usize>();
        assert_eq!(accepted, 2);
        assert_eq!(
            usage,
            AdmissionUsage {
                projects: 8,
                generations: 10,
                readers: 0,
                writers: 2
            }
        );
    });
    assert_eq!(
        pool.usage().unwrap(),
        AdmissionUsage {
            projects: 8,
            generations: 8,
            readers: 0,
            writers: 0
        }
    );
}

fn check_rows(project: &ModelProject, expected: &BTreeMap<i64, String>) {
    let reader = project.read().unwrap();
    assert_eq!(reader.row_count(), expected.len());
    for key in -8..8 {
        let row = reader.get("items", &Key::Integer(key)).unwrap();
        assert_eq!(
            row.map(|r| &r[1]),
            expected.get(&key).map(|v| Value::Text(v.clone())).as_ref()
        );
    }
}
proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    #[test]
    fn independent_sequences_preserve_published_and_discarded_replay_states(
        actions in prop::collection::vec((-8i64..8,any::<u8>(),any::<bool>()),1..80)
    ) {
        let pool=pool(12,10,1);
        let (mut project,mut raw)=initialized(&pool,[7;16]);
        let mut expected=BTreeMap::<i64,String>::new();
        let mut retained=Vec::new();
        for (key,action,publish) in actions {
            if action%7==0 {
                if retained.len()==8 {retained.remove(0);}
                retained.push((project.read().unwrap(),expected.clone()));
            }
            let mut next=expected.clone();
            let event=if action%3==0 && next.contains_key(&key) {
                next.remove(&key);
                Event {table_id:1,kind:EventKind::Delete(Key::Integer(key))}
            } else {
                let value=format!("synthetic-{action}");
                let existed=next.insert(key,value.clone()).is_some();
                let row=vec![Value::Integer(key),Value::Text(value)];
                Event {table_id:1,kind:if existed {EventKind::Replace(row)} else {EventKind::Insert(row)}}
            };
            let mut candidate=raw.clone();
            let input=plan(&mut candidate,[event],true);
            let output=project.replay(&input).unwrap();
            assert_eq!(output.fingerprint(),candidate.fingerprint());
            assert_eq!(output.row_count(),next.len());
            assert_eq!(pool.usage().unwrap().writers,1);
            for (reader,rows) in &retained {
                assert_eq!(reader.row_count(),rows.len());
                for (key,value) in rows {assert_eq!(reader.get("items",&Key::Integer(*key)).unwrap().unwrap()[1],Value::Text(value.clone()));}
            }
            if publish {project.publish_replayed(output).unwrap();expected=next;raw=candidate;} else {drop(output);}
            assert_eq!(pool.usage().unwrap().writers,0);
            check_rows(&project,&expected);
        }
        drop(retained);drop(project);
        prop_assert_eq!(pool.usage().unwrap(),AdmissionUsage { projects: 0, generations: 0, readers: 0, writers: 0 });
    }
}
