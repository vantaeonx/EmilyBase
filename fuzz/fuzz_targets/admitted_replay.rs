#![no_main]
#![forbid(unsafe_code)]
use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_commit_model::{
    AdmissionLimits, AdmissionUsage, AdmittedPlan, AdmittedReplay, AdmittedStage,
    DecodedPlanLimits, DecodedPlanPool, Model, ModelPool, ModelReader,
};
use emilybase_database::{Event, EventKind};
use libfuzzer_sys::fuzz_target;
use std::collections::{BTreeMap, BTreeSet};

fn input(base: &mut Model, event: Event) -> AdmittedPlan {
    let mut staged = base.begin().unwrap();
    staged.apply(event).unwrap();
    staged.rebuild_index("items").unwrap();
    let prepared = staged.prepare().unwrap();
    let bytes = prepared.image_plan().unwrap().encode().unwrap();
    let decoded = DecodedPlanPool::new(DecodedPlanLimits::new(1, 64 * 1024).unwrap());
    let plan = decoded.decode(&bytes).unwrap();
    base.publish(prepared).unwrap();
    plan
}
fuzz_target!(|data: &[u8]| {
    if data.len() < 2 || data.len() > 512 {
        return;
    }
    let generations = 2 + usize::from(data[0] % 7);
    let readers = usize::from(data[1] % 5);
    let pool = ModelPool::new(AdmissionLimits::new(1, generations, readers, 1).unwrap());
    let mut project = pool.create([7; 16]).unwrap();
    let mut raw = Model::new([7; 16]).unwrap();
    let plan = input(
        &mut raw,
        Event {
            table_id: 1,
            kind: EventKind::Create(Schema {
                name: "items".into(),
                primary_key: 0,
                columns: vec![
                    Column {
                        name: "id".into(),
                        data_type: DataType::Integer,
                        nullable: false,
                    },
                    Column {
                        name: "value".into(),
                        data_type: DataType::Integer,
                        nullable: false,
                    },
                ],
            }),
        },
    );
    project
        .publish_replayed(project.replay(&plan).unwrap())
        .unwrap();
    drop(plan);
    let mut expected = BTreeMap::<i64, i64>::new();
    let mut old = Vec::<(ModelReader, BTreeMap<i64, i64>)>::new();
    let mut pending: Option<(AdmittedReplay, Model, BTreeMap<i64, i64>)> = None;
    let mut stage: Option<AdmittedStage> = None;
    for command in data[2..].as_chunks::<2>().0.iter().take(64) {
        let action = command[0] % 8;
        if action == 0 {
            let key = i64::from(command[1] % 8);
            let value = i64::from(command[0]);
            let mut next = expected.clone();
            let event = if command[1] & 128 != 0 && next.contains_key(&key) {
                next.remove(&key);
                Event {
                    table_id: 1,
                    kind: EventKind::Delete(Key::Integer(key)),
                }
            } else {
                let exists = next.insert(key, value).is_some();
                let row = vec![Value::Integer(key), Value::Integer(value)];
                Event {
                    table_id: 1,
                    kind: if exists {
                        EventKind::Replace(row)
                    } else {
                        EventKind::Insert(row)
                    },
                }
            };
            let mut candidate = raw.clone();
            let plan = input(&mut candidate, event);
            let distinct: BTreeSet<_> = old
                .iter()
                .map(|(r, _)| r.transaction())
                .chain([project.transaction()])
                .collect();
            let refused = pending.is_some() || stage.is_some() || distinct.len() == generations;
            let output = project.replay(&plan);
            assert_eq!(output.is_err(), refused);
            if let Ok(output) = output {
                assert_eq!(output.fingerprint(), candidate.fingerprint());
                assert_eq!(output.row_count(), next.len());
                pending = Some((output, candidate, next));
            }
        } else if action == 1 {
            if let Some((output, candidate, next)) = pending.take() {
                project.publish_replayed(output).unwrap();
                raw = candidate;
                expected = next;
            }
        } else if action == 2 {
            pending = None;
        } else if action == 3 {
            let reader = project.read();
            assert_eq!(reader.is_err(), old.len() == readers);
            if let Ok(reader) = reader {
                old.push((reader, expected.clone()));
            }
        } else if action == 4 {
            if !old.is_empty() {
                old.swap_remove(usize::from(command[1]) % old.len());
            }
        } else if action == 5 {
            let distinct: BTreeSet<_> = old
                .iter()
                .map(|(r, _)| r.transaction())
                .chain([project.transaction()])
                .collect();
            let refused = stage.is_some() || pending.is_some() || distinct.len() == generations;
            let output = project.begin();
            assert_eq!(output.is_err(), refused);
            if let Ok(output) = output {
                stage = Some(output);
            }
        } else if action == 6 {
            stage = None;
        } else if let Some((output, _, _)) = pending.take() {
            let other = ModelPool::new(AdmissionLimits::new(1, 1, 0, 0).unwrap());
            let mut foreign = other.create([7; 16]).unwrap();
            let before = foreign.fingerprint();
            assert!(foreign.publish_replayed(output).is_err());
            assert_eq!(foreign.fingerprint(), before);
        }
        let distinct: BTreeSet<_> = old
            .iter()
            .map(|(r, _)| r.transaction())
            .chain([project.transaction()])
            .collect();
        let active = usize::from(pending.is_some()) + usize::from(stage.is_some());
        assert_eq!(
            pool.usage().unwrap(),
            AdmissionUsage {
                projects: 1,
                generations: distinct.len() + active,
                readers: old.len(),
                writers: active,
            }
        );
        assert_eq!(project.fingerprint(), raw.fingerprint());
        for (reader, rows) in &old {
            assert_eq!(reader.row_count(), rows.len());
            for key in 0..8 {
                assert_eq!(
                    reader
                        .get("items", &Key::Integer(key))
                        .unwrap()
                        .map(|row| row[1].clone()),
                    rows.get(&key).map(|v| Value::Integer(*v))
                );
            }
        }
        for key in 0..8 {
            assert_eq!(
                raw.view()
                    .get("items", &Key::Integer(key))
                    .unwrap()
                    .map(|row| row[1].clone()),
                expected.get(&key).map(|v| Value::Integer(*v))
            );
        }
    }
    drop(pending);
    drop(stage);
    drop(old);
    drop(project);
    assert_eq!(
        pool.usage().unwrap(),
        AdmissionUsage {
            projects: 0,
            generations: 0,
            readers: 0,
            writers: 0
        }
    );
});
