use emilybase_catalog::{DataType, Key, Value};
use emilybase_commit_model::{EncodedComponents, Error, Model, Prepared};
use emilybase_database::{Event, EventKind, Snapshot};
use proptest::prelude::*;
use sha2::{Digest, Sha256};
mod support;

fn history_bytes(view: &Snapshot) -> Vec<u8> {
    view.pages().flat_map(|page| page.encode()).collect()
}

fn actual_components(view: &Snapshot, select: impl Fn(u64) -> Vec<u8>) -> u64 {
    let mut bytes = history_bytes(view);
    for schema in view.schemas() {
        bytes.extend(select(view.table_id(&schema.name).unwrap()));
    }
    bytes.len() as u64
}

fn verify_prepared(value: &Prepared) {
    let actual = actual_components(value.view(), |table| {
        let selection = value.selection(table).unwrap();
        let mut bytes = selection.binding().encode().unwrap().to_vec();
        bytes.extend(selection.index().encode().unwrap());
        bytes
    });
    assert_eq!(value.encoded_components().unwrap().total_bytes(), actual);
}

fn verify_model(value: &Model) {
    let history = history_bytes(value.view());
    let mut history_hash = Sha256::new();
    history_hash.update(&history);
    let mut table_ids: Vec<_> = value
        .view()
        .schemas()
        .iter()
        .map(|schema| value.view().table_id(&schema.name).unwrap())
        .collect();
    table_ids.sort_unstable();
    // Recompose from serialized objects, not the cached selection fingerprint.
    let mut state = Sha256::new();
    state.update(b"EBMODEL\0");
    state.update(value.database_id());
    state.update(value.transaction().to_le_bytes());
    state.update(history_hash.finalize());
    state.update((table_ids.len() as u32).to_le_bytes());
    let mut index_bytes = 0;
    let mut root_bytes = 0;
    for table in table_ids {
        let selection = value.selection(table).unwrap();
        let root = selection.binding().encode().unwrap();
        let index = selection.index().encode().unwrap();
        let digest: [u8; 32] = Sha256::digest(&index).into();
        assert_eq!(selection.index_fingerprint(), digest);
        state.update(root);
        state.update(digest);
        index_bytes += index.len() as u64;
        root_bytes += root.len() as u64;
    }
    let state_digest: [u8; 32] = state.finalize().into();
    assert_eq!(value.fingerprint(), state_digest);
    let report = value.encoded_components().unwrap();
    assert_eq!(report.history_bytes(), history.len() as u64);
    assert_eq!(report.index_bytes(), index_bytes);
    assert_eq!(report.root_bytes(), root_bytes);
    assert_eq!(
        report.total_bytes(),
        history.len() as u64 + index_bytes + root_bytes
    );
}

#[test]
fn component_reports_measure_real_empty_and_all_128_selected_roots() {
    let empty = Model::new([7; 16]).unwrap();
    verify_model(&empty);
    assert_eq!(empty.encoded_components().unwrap().total_bytes(), 4096);
    let names: Vec<_> = (0..128).map(|id| format!("table_{id}")).collect();
    let schemas: Vec<_> = names
        .iter()
        .map(|name| (name.as_str(), DataType::Integer))
        .collect();
    let live = support::model(&schemas);
    verify_model(&live);
    let report = live.encoded_components().unwrap();
    assert_eq!(report.roots(), 128);
    assert_eq!(report.index_pages(), 128);
    assert_eq!(report.index_bytes(), 1048576);
    assert_eq!(report.root_bytes(), 24576);
}

#[test]
fn reports_and_cached_digests_follow_changes_drops_recreation_and_old_views() {
    // Lexical schema order differs from monotonic table identity order.
    let mut live = support::model(&[("zeta", DataType::Integer), ("alpha", DataType::Text)]);
    let old = live.clone();
    let old_report = old.encoded_components().unwrap();
    let alpha = live.selection(2).unwrap().clone();
    let mut staged = live.begin().unwrap();
    for key in 0..200 {
        staged
            .apply(Event {
                table_id: 1,
                kind: EventKind::Insert(vec![Value::Integer(key), Value::Text("value".into())]),
            })
            .unwrap();
    }
    support::indexes(&live, &mut staged, &["zeta"]);
    let prepared = staged.prepare().unwrap();
    verify_prepared(&prepared);
    live.publish(prepared).unwrap();
    verify_model(&live);
    assert_eq!(live.selection(2).unwrap(), &alpha);
    assert_eq!(old.encoded_components().unwrap(), old_report);
    verify_model(&old);

    let mut dropped = live.begin().unwrap();
    dropped
        .apply(Event {
            table_id: 2,
            kind: EventKind::Drop,
        })
        .unwrap();
    let prepared = dropped.prepare().unwrap();
    assert_eq!(prepared.retired_tables(), &[2]);
    verify_prepared(&prepared);
    assert_eq!(prepared.encoded_components().unwrap().roots(), 1);
    live.publish(prepared).unwrap();
    verify_model(&live);

    let mut recreated = live.begin().unwrap();
    recreated
        .apply(Event {
            table_id: 3,
            kind: EventKind::Create(support::schema("alpha", DataType::Text)),
        })
        .unwrap();
    support::indexes(&live, &mut recreated, &["alpha"]);
    let prepared = recreated.prepare().unwrap();
    verify_prepared(&prepared);
    live.publish(prepared).unwrap();
    assert!(live.selection(2).is_none());
    assert!(live.selection(3).is_some());
    verify_model(&live);
    verify_model(&old);
}

#[test]
fn cancelled_and_refused_staging_cannot_change_reports_or_cached_fingerprints() {
    let live = support::model(&[("items", DataType::Text)]);
    let before = live.encoded_components().unwrap();
    let digest = live.selection(1).unwrap().index_fingerprint();
    let mut staged = live.begin().unwrap();
    let row = vec![Value::Text("я".repeat(1536)), Value::Text("value".into())];
    staged
        .apply(Event {
            table_id: 1,
            kind: EventKind::Insert(row.clone()),
        })
        .unwrap();
    support::indexes(&live, &mut staged, &["items"]);
    let prepared = staged.prepare().unwrap();
    verify_prepared(&prepared);
    assert_eq!(prepared.selection(1).unwrap().binding().excluded(), 1);
    drop(prepared);
    assert_eq!(live.encoded_components().unwrap(), before);
    assert_eq!(live.selection(1).unwrap().index_fingerprint(), digest);
    verify_model(&live);
    let mut refused = live.begin().unwrap();
    refused
        .apply(Event {
            table_id: 1,
            kind: EventKind::Insert(row.clone()),
        })
        .unwrap();
    assert!(
        refused
            .apply(Event {
                table_id: 1,
                kind: EventKind::Insert(row),
            })
            .is_err()
    );
    assert!(matches!(refused.prepare(), Err(Error::Aborted)));
    assert_eq!(live.encoded_components().unwrap(), before);
    verify_model(&live);
}

#[test]
fn count_admission_refuses_domains_and_extreme_values_without_allocating_images() {
    for counts in [
        (0, 0, 0),
        (65537, 0, 0),
        (u64::MAX, 0, 0),
        (1, 129, 129),
        (1, u64::MAX, u64::MAX),
        (1, 0, 1),
        (1, 1, 0),
        (1, 2, 1),
        (1, 1, 1025),
        (1, 128, 131073),
    ] {
        assert!(matches!(
            EncodedComponents::from_counts(counts.0, counts.1, counts.2),
            Err(Error::Limit)
        ));
    }
    // Loose arithmetic maximum, not proof that such a live state is reachable.
    let maximum = EncodedComponents::from_counts(65536, 128, 2048).unwrap();
    assert_eq!(maximum.history_bytes(), 268435456);
    assert_eq!(maximum.index_bytes(), 8912896);
    assert_eq!(maximum.root_bytes(), 24576);
    assert_eq!(maximum.total_bytes(), 277372928);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn bounded_components_match_wider_integer_reference(
        history in prop_oneof![1u64..65538, any::<u64>()],
        roots in prop_oneof![0u64..257, any::<u64>()],
        pages in prop_oneof![0u64..262145, any::<u64>()],
    ) {
        let h=u128::from(history);let r=u128::from(roots);let p=u128::from(pages);
        let valid=(1..=65536).contains(&h) && r<=128 && p>=r && p<=r*1024 && p<=2048;
        let actual=EncodedComponents::from_counts(history,roots,pages);
        prop_assert_eq!(actual.is_ok(),valid);
        if let Ok(report)=actual {
            prop_assert_eq!(u128::from(report.total_bytes()),h*4096+(p+r)*4096+r*192);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn generated_preparation_and_rollback_keep_reports_and_digests_exact(
        commands in prop::collection::vec((0u8..4, 1u64..3, -20i64..21, "[a-z]{0,24}"), 1..32),
    ) {
        let mut live=support::model(&[("zeta",DataType::Integer),("alpha",DataType::Integer)]);
        for (operation,table,key,value) in commands {
            let before=live.clone();let report=before.encoded_components().unwrap();
            let name=if table==1 {"zeta"} else {"alpha"};
            let present=live.view().get(name,&Key::Integer(key)).unwrap().is_some();
            let row=vec![Value::Integer(key),Value::Text(value)];
            let kind=if operation==2 {EventKind::Delete(Key::Integer(key))}
                else if present {EventKind::Replace(row)} else {EventKind::Insert(row)};
            let mut staged=live.begin().unwrap();
            if staged.apply(Event {table_id:table,kind}).is_ok() {
                support::indexes(&live,&mut staged,&[name]);
                let prepared=staged.prepare().unwrap();verify_prepared(&prepared);
                if operation==3 {drop(prepared);} else {live.publish(prepared).unwrap();}
            } else {prop_assert!(matches!(staged.prepare(),Err(Error::Aborted)));}
            verify_model(&live);verify_model(&before);
            prop_assert_eq!(before.encoded_components().unwrap(),report);
        }
    }
}
