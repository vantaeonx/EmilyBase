#![no_main]
#![forbid(unsafe_code)]
use emilybase_catalog::{Column, DataType, Schema, Value};
use emilybase_commit_model::{
    AdmittedEnvelope, EnvelopeLimit, EnvelopeLimits, EnvelopePool, EnvelopeUsage, Error, ImagePlan,
    Model,
};
use emilybase_database::{Event, EventKind};
use libfuzzer_sys::fuzz_target;

fn plans() -> (Model, [ImagePlan; 2]) {
    let mut base = Model::new([7; 16]).unwrap();
    let mut staged = base.begin().unwrap();
    staged
        .apply(Event {
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
                        data_type: DataType::Text,
                        nullable: false,
                    },
                ],
            }),
        })
        .unwrap();
    staged.rebuild_index("items").unwrap();
    base.publish(staged.prepare().unwrap()).unwrap();
    let mut staged = base.begin().unwrap();
    staged.rebuild_index("items").unwrap();
    let small = staged.prepare().unwrap().image_plan().unwrap();
    let mut staged = base.begin().unwrap();
    staged
        .apply(Event {
            table_id: 1,
            kind: EventKind::Insert(vec![Value::Integer(7), Value::Text("synthetic".into())]),
        })
        .unwrap();
    staged.rebuild_index("items").unwrap();
    let large = staged.prepare().unwrap().image_plan().unwrap();
    (base, [small, large])
}

fuzz_target!(|input: &[u8]| {
    if input.len() < 2 || input.len() > 512 {
        return;
    }
    let (base, plans) = plans();
    let sources = [plans[0].encode().unwrap(), plans[1].encode().unwrap()];
    let limit = usize::from(input[0] % 9);
    let unit = u64::from(input[1]);
    let bytes_limit = match input[0] % 4 {
        0 => unit * 424,
        1 => unit * 424 + 423,
        2 => sources[1].len() as u64 + unit,
        _ => sources[1].len() as u64 - 1,
    };
    let pool = EnvelopePool::new(EnvelopeLimits::new(limit, bytes_limit).unwrap());
    let before = base.fingerprint();
    let mut retained = Vec::<(usize, AdmittedEnvelope)>::new();
    for command in input[2..].as_chunks::<2>().0.iter().take(64) {
        let operation = command[0] % 8;
        let index = usize::from(command[1]);
        match operation {
            3 if !retained.is_empty() => {
                retained.swap_remove(index % retained.len());
            }
            4 => retained.clear(),
            6 | 7 => {
                let mut source = sources[index % 2].clone();
                if operation == 6 {
                    source.truncate(index % source.len());
                } else {
                    let offset = index % source.len();
                    source[offset] ^= 1;
                }
                assert!(pool.copy_encoded(&source).is_err());
            }
            3 => (),
            _ => {
                let source = if operation == 2 && !retained.is_empty() {
                    retained[index % retained.len()].0
                } else {
                    index % 2
                };
                let live_bytes: u64 = retained
                    .iter()
                    .map(|(kind, _)| sources[*kind].len() as u64)
                    .sum();
                let expected = if retained.len() == limit {
                    Some(EnvelopeLimit::Buffers)
                } else if live_bytes + sources[source].len() as u64 > bytes_limit {
                    Some(EnvelopeLimit::Bytes)
                } else {
                    None
                };
                let result = match operation {
                    1 => pool.copy_encoded(&sources[source]),
                    2 if !retained.is_empty() => retained[index % retained.len()].1.try_clone(),
                    5 => pool.clone().encode(&plans[source]),
                    _ => pool.encode(&plans[source]),
                };
                match expected {
                    Some(expected) => assert!(
                        matches!(result, Err(Error::EnvelopeAdmission(actual)) if actual == expected)
                    ),
                    None => retained.push((source, result.unwrap())),
                }
            }
        }
        let live_bytes: u64 = retained
            .iter()
            .map(|(kind, _)| sources[*kind].len() as u64)
            .sum();
        assert_eq!(
            pool.usage().unwrap(),
            EnvelopeUsage {
                buffers: retained.len(),
                bytes: live_bytes
            }
        );
        for (kind, envelope) in &retained {
            assert_eq!(envelope.as_bytes(), sources[*kind]);
            assert_eq!(
                ImagePlan::decode(envelope.as_bytes())
                    .unwrap()
                    .replay(&base)
                    .unwrap()
                    .fingerprint(),
                plans[*kind].next_fingerprint()
            );
        }
        assert_eq!(base.fingerprint(), before);
    }
    drop(retained);
    assert_eq!(pool.usage().unwrap(), EnvelopeUsage::default());
});
