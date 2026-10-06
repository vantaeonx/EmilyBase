#![no_main]
#![forbid(unsafe_code)]

use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_commit_model::{
    AdmissionLimit as Bound, AdmissionLimits, AdmissionUsage, AdmittedPrepared, AdmittedStage,
    Error, ModelPool, ModelProject, ModelReader,
};
use emilybase_database::{Event, EventKind};
use libfuzzer_sys::fuzz_target;
use std::collections::BTreeSet;

#[derive(Clone, Copy)]
struct Reference {
    transaction: u64,
    value: Option<u64>,
}

struct Reader {
    owner: usize,
    reference: Reference,
    fingerprint: [u8; 32],
    actual: ModelReader,
}

enum Pending {
    Staged {
        base: Reference,
        next: Reference,
        aborted: bool,
        actual: AdmittedStage,
    },
    Prepared {
        base: Reference,
        next: Reference,
        actual: AdmittedPrepared,
    },
}

impl Pending {
    fn base(&self) -> Reference {
        match self {
            Self::Staged { base, .. } | Self::Prepared { base, .. } => *base,
        }
    }
}

fn counts(
    projects: &[Option<Reference>],
    readers: &[Reader],
    pending: &[Option<Pending>],
) -> AdmissionUsage {
    let mut owners = BTreeSet::new();
    let mut generations = BTreeSet::new();
    for (owner, project) in projects.iter().enumerate() {
        if let Some(project) = project {
            owners.insert(owner);
            generations.insert((owner, project.transaction));
        }
    }
    for reader in readers {
        owners.insert(reader.owner);
        generations.insert((reader.owner, reader.reference.transaction));
    }
    let mut writers = 0;
    for (owner, frame) in pending.iter().enumerate() {
        if let Some(frame) = frame {
            owners.insert(owner);
            generations.insert((owner, frame.base().transaction));
            writers += 1;
        }
    }
    AdmissionUsage {
        projects: owners.len(),
        generations: generations.len() + writers,
        readers: readers.len(),
        writers,
    }
}

fn registered(
    owner: usize,
    refs: &[Option<Reference>],
    readers: &[Reader],
    pending: &[Option<Pending>],
) -> bool {
    refs[owner].is_some()
        || pending[owner].is_some()
        || readers.iter().any(|reader| reader.owner == owner)
}

fn check(reader: &Reader) {
    assert_eq!(reader.actual.database_id(), [(reader.owner + 1) as u8; 16]);
    assert_eq!(reader.actual.transaction(), reader.reference.transaction);
    assert_eq!(reader.actual.fingerprint(), reader.fingerprint);
    assert_eq!(
        reader.actual.row_count(),
        usize::from(reader.reference.value.is_some())
    );
    if let Some(value) = reader.reference.value {
        assert_eq!(reader.actual.table_id("items").unwrap(), 1);
        assert_eq!(
            reader
                .actual
                .get("items", &Key::Integer(1))
                .unwrap()
                .unwrap()[1],
            Value::Text(value.to_string())
        );
        let binding = reader.actual.binding(1).unwrap();
        assert_eq!((binding.covered(), binding.excluded()), (1, 0));
        assert!(
            reader
                .actual
                .row_location("items", &Key::Integer(1))
                .unwrap()
                .is_some()
        );
    } else {
        assert!(reader.actual.get("items", &Key::Integer(1)).is_err());
        assert!(reader.actual.binding(1).is_none());
    }
}

fn changes(stage: &mut AdmittedStage, base: Reference) -> Reference {
    let value = base.value.map_or(0, |value| value + 1);
    if base.value.is_none() {
        let schema = Schema {
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
                    data_type: DataType::Text,
                    nullable: false,
                },
            ],
        };
        stage
            .apply(Event {
                table_id: 1,
                kind: EventKind::Create(schema),
            })
            .unwrap();
    }
    let row = vec![Value::Integer(1), Value::Text(value.to_string())];
    stage
        .apply(Event {
            table_id: 1,
            kind: if base.value.is_none() {
                EventKind::Insert(row)
            } else {
                EventKind::Replace(row)
            },
        })
        .unwrap();
    stage.rebuild_index("items").unwrap();
    Reference {
        transaction: base.transaction + 1,
        value: Some(value),
    }
}

fuzz_target!(|bytes: &[u8]| {
    if bytes.len() < 4 || bytes.len() > 256 {
        return;
    }
    let limits = AdmissionLimits::new(
        usize::from(bytes[0] % 3 + 1),
        usize::from(bytes[1] % 7 + 1),
        usize::from(bytes[2] % 9),
        usize::from(bytes[3] % 5),
    )
    .unwrap();
    let pool = ModelPool::new(limits);
    let other_handle = pool.clone();
    let mut projects: Vec<Option<ModelProject>> = (0..3).map(|_| None).collect();
    let mut refs: Vec<Option<Reference>> = vec![None; 3];
    let mut pending: Vec<Option<Pending>> = (0..3).map(|_| None).collect();
    let mut readers: Vec<Reader> = Vec::new();
    for step in bytes[4..].as_chunks::<2>().0 {
        let owner = usize::from(step[1] % 3);
        let before = counts(&refs, &readers, &pending);
        assert_eq!(pool.usage().unwrap(), before);
        assert_eq!(other_handle.usage().unwrap(), before);
        match step[0] % 12 {
            0 => {
                let result = pool.create([(owner + 1) as u8; 16]);
                if registered(owner, &refs, &readers, &pending) {
                    assert!(matches!(result, Err(Error::DuplicateDatabase)));
                } else if before.projects == limits.projects() {
                    assert!(matches!(result, Err(Error::Admission(Bound::Projects))));
                } else if before.generations == limits.generations() {
                    assert!(matches!(result, Err(Error::Admission(Bound::Generations))));
                } else {
                    projects[owner] = Some(result.unwrap());
                    refs[owner] = Some(Reference {
                        transaction: 1,
                        value: None,
                    });
                }
            }
            1 => {
                drop(projects[owner].take());
                refs[owner] = None;
            }
            2 => {
                if let Some(project) = &projects[owner] {
                    let result = project.read();
                    if before.readers == limits.readers() {
                        assert!(matches!(result, Err(Error::Admission(Bound::Readers))));
                    } else {
                        let actual = result.unwrap();
                        readers.push(Reader {
                            owner,
                            reference: refs[owner].unwrap(),
                            fingerprint: actual.fingerprint(),
                            actual,
                        });
                    }
                }
            }
            3 => {
                if !readers.is_empty() {
                    let reader = &readers[usize::from(step[1]) % readers.len()];
                    let result = reader.actual.try_clone();
                    if before.readers == limits.readers() {
                        assert!(matches!(result, Err(Error::Admission(Bound::Readers))));
                    } else {
                        readers.push(Reader {
                            owner: reader.owner,
                            reference: reader.reference,
                            fingerprint: reader.fingerprint,
                            actual: result.unwrap(),
                        });
                    }
                }
            }
            4 => {
                if !readers.is_empty() {
                    drop(readers.remove(usize::from(step[1]) % readers.len()));
                }
            }
            5 => {
                if let Some(project) = &projects[owner] {
                    let result = project.begin();
                    let refusal = if pending[owner].is_some() {
                        Some(Bound::ProjectWriter)
                    } else if before.writers == limits.writers() {
                        Some(Bound::Writers)
                    } else if before.generations == limits.generations() {
                        Some(Bound::Generations)
                    } else {
                        None
                    };
                    if let Some(bound) = refusal {
                        assert!(matches!(result, Err(Error::Admission(actual)) if actual == bound));
                    } else {
                        let mut actual = result.unwrap();
                        let base = refs[owner].unwrap();
                        let next = changes(&mut actual, base);
                        pending[owner] = Some(Pending::Staged {
                            base,
                            next,
                            aborted: false,
                            actual,
                        });
                    }
                }
            }
            6 => {
                if let Some(frame) = pending[owner].take() {
                    pending[owner] = match frame {
                        Pending::Staged {
                            base,
                            next,
                            aborted,
                            actual,
                        } => {
                            let result = actual.prepare();
                            if aborted {
                                assert!(matches!(result, Err(Error::Aborted)));
                                None
                            } else {
                                Some(Pending::Prepared {
                                    base,
                                    next,
                                    actual: result.unwrap(),
                                })
                            }
                        }
                        frame => Some(frame),
                    };
                }
            }
            7 => {
                if let Some(frame) = pending[owner].take() {
                    match frame {
                        Pending::Prepared { next, actual, .. } => {
                            if let Some(project) = &mut projects[owner] {
                                project.publish(actual).unwrap();
                                refs[owner] = Some(next);
                            }
                        }
                        frame => pending[owner] = Some(frame),
                    }
                }
            }
            8 => {
                drop(pending[owner].take());
            }
            9 => {
                if let Some(Pending::Staged {
                    aborted, actual, ..
                }) = &mut pending[owner]
                {
                    assert!(
                        actual
                            .apply(Event {
                                table_id: 0,
                                kind: EventKind::Root
                            })
                            .is_err()
                    );
                    *aborted = true;
                }
            }
            10 => {
                if let Some(frame) = pending[owner].take() {
                    match frame {
                        Pending::Prepared { actual, .. } => {
                            if let Some(project) = &mut projects[(owner + 1) % 3] {
                                let fingerprint = project.fingerprint();
                                assert!(matches!(project.publish(actual), Err(Error::Conflict)));
                                assert_eq!(project.fingerprint(), fingerprint);
                            }
                        }
                        frame => pending[owner] = Some(frame),
                    }
                }
            }
            _ => {
                assert!(pool.create([0; 16]).is_err());
            }
        }
        for reader in &readers {
            check(reader);
        }
        for (project, reference) in projects.iter().zip(&refs) {
            if let Some(project) = project {
                assert_eq!(project.transaction(), reference.unwrap().transaction);
            }
        }
        let after = counts(&refs, &readers, &pending);
        assert_eq!(pool.usage().unwrap(), after);
        assert!(
            after.projects <= limits.projects()
                && after.generations <= limits.generations()
                && after.readers <= limits.readers()
                && after.writers <= limits.writers()
        );
    }
    drop(projects);
    drop(readers);
    drop(pending);
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
