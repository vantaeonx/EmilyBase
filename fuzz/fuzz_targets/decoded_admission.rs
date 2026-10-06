#![no_main]
#![forbid(unsafe_code)]
use emilybase_catalog::{Column, DataType, Schema, Value};
use emilybase_commit_model::{
    AdmittedPlan, DecodedPlanLimit, DecodedPlanLimits, DecodedPlanPool, DecodedPlanUsage, Error,
    ImagePlan, Model,
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
    let charges = [
        plans[0].counts().unwrap().decoded_vector_bytes().unwrap(),
        plans[1].counts().unwrap().decoded_vector_bytes().unwrap(),
    ];
    let slots = usize::from(input[0] % 9);
    let bytes_limit = charges[0] * u64::from(input[1]);
    let pool = DecodedPlanPool::new(DecodedPlanLimits::new(slots, bytes_limit).unwrap());
    let mut retained = Vec::<(usize, usize, AdmittedPlan)>::new();
    let mut id = 0;
    for command in input[2..].as_chunks::<2>().0.iter().take(64) {
        let action = command[0] % 6;
        let index = usize::from(command[1]);
        if action == 0 {
            let kind = index % 2;
            let live: std::collections::BTreeMap<_, _> =
                retained.iter().map(|(id, kind, _)| (*id, *kind)).collect();
            let bytes: u64 = live.values().map(|kind| charges[*kind]).sum();
            let expected = if live.len() == slots {
                Some(DecodedPlanLimit::Plans)
            } else if bytes + charges[kind] > bytes_limit {
                Some(DecodedPlanLimit::Bytes)
            } else {
                None
            };
            match expected {
                Some(wanted) => assert!(
                    matches!(pool.decode(&sources[kind]), Err(Error::DecodedAdmission(actual)) if actual == wanted)
                ),
                None => {
                    retained.push((id, kind, pool.decode(&sources[kind]).unwrap()));
                    id += 1;
                }
            }
        } else if action == 4 {
            let mut corrupt = sources[index % 2].clone();
            let offset = index % corrupt.len();
            corrupt[offset] ^= 1;
            assert!(pool.decode(&corrupt).is_err());
        } else if action == 5 {
            retained.clear();
        } else if !retained.is_empty() {
            let position = index % retained.len();
            if action == 1 {
                let (id, kind, owner) = &retained[position];
                let copy = owner.clone();
                assert!(std::ptr::eq(copy.plan(), owner.plan()));
                retained.push((*id, *kind, copy));
            } else if action == 2 {
                retained.swap_remove(position);
            }
        }
        let live: std::collections::BTreeMap<_, _> =
            retained.iter().map(|(id, kind, _)| (*id, *kind)).collect();
        assert_eq!(
            pool.usage().unwrap(),
            DecodedPlanUsage {
                plans: live.len(),
                bytes: live.values().map(|kind| charges[*kind]).sum(),
            }
        );
        for (_, kind, owner) in &retained {
            assert_eq!(owner.plan().encode().unwrap(), sources[*kind]);
            assert_eq!(
                owner.plan().replay(&base).unwrap().fingerprint(),
                plans[*kind].next_fingerprint()
            );
        }
    }
    drop(retained);
    assert_eq!(pool.usage().unwrap(), DecodedPlanUsage::default());
});
