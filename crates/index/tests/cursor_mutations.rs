use emilybase_index::{BPlusTree, Error, IndexSnapshot, Key, MAX_INDEX_ENTRIES, RecordPointer};
use proptest::prelude::*;
use std::collections::BTreeMap;

fn owned(v: (&Key, RecordPointer)) -> (Key, RecordPointer) {
    (v.0.clone(), v.1)
}
fn entries(model: &BTreeMap<Key, RecordPointer>) -> Vec<(Key, RecordPointer)> {
    model.iter().map(|(k, v)| (k.clone(), *v)).collect()
}
fn key(number: i16, text: bool) -> Key {
    if text {
        Key::Text(format!("界-{number:04}"))
    } else {
        Key::Integer(i64::from(number))
    }
}

#[test]
fn stable_holes_root_collapse_and_reused_ids_keep_exact_reverse_order() {
    let mut model = (0..500)
        .map(|i| {
            (
                Key::Integer(i),
                RecordPointer {
                    page_id: i as u64 + 1,
                    slot_id: 0,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut tree = BPlusTree::from_sorted_stable(&entries(&model)).unwrap();
    let original = tree.clone();
    let mut old_cursor = original.cursor(None, None).unwrap();
    assert_eq!(old_cursor.next().unwrap().unwrap().0, &Key::Integer(0));
    for i in 0..490 {
        assert_eq!(
            tree.remove(&Key::Integer(i)).unwrap(),
            model.remove(&Key::Integer(i)).unwrap()
        );
    }
    assert!(tree.page_count() < original.page_count());
    for i in 600..900 {
        let value = RecordPointer {
            page_id: i as u64 + 1,
            slot_id: 7,
        };
        tree.insert(Key::Integer(i), value).unwrap();
        model.insert(Key::Integer(i), value);
    }
    let image = IndexSnapshot { revision: 1, tree };
    let restored = IndexSnapshot::decode(&image.encode().unwrap()).unwrap();
    assert_eq!(
        restored
            .tree
            .cursor(None, None)
            .unwrap()
            .rev()
            .map(|v| owned(v.unwrap()))
            .collect::<Vec<_>>(),
        entries(&model).into_iter().rev().collect::<Vec<_>>()
    );
    assert_eq!(
        old_cursor.next_back().unwrap().unwrap().0,
        &Key::Integer(499)
    );
    assert_eq!(old_cursor.count(), 498);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    #[test]
    fn rebalancing_and_reimport_preserve_mixed_key_intervals(
        operations in prop::collection::vec((0u8..4,-70i16..420,any::<bool>(),any::<u16>()),1..100),
        stable in any::<bool>(),
    ) {
        let mut model=(0..240).map(|i|(Key::Integer(i),RecordPointer{page_id:i as u64+1,slot_id:0})).collect::<BTreeMap<_,_>>();
        let mut tree=if stable {BPlusTree::from_sorted_stable(&entries(&model))} else {BPlusTree::from_sorted(&entries(&model))}.unwrap();
        for (step,(kind,number,text,slot)) in operations.into_iter().enumerate() {
            let key=key(number,text);let upper=match &key {Key::Integer(n)=>Key::Integer(n+31),Key::Text(s)=>Key::Text(format!("{s}z"))};
            let value=RecordPointer{page_id:step as u64+1000,slot_id:slot};
            let old=model.get(&key).copied();let before=tree.clone();
            match kind {
                0 if old.is_some()=>prop_assert_eq!(tree.insert(key.clone(),value),Err(Error::Duplicate)),
                0=>{tree.insert(key.clone(),value).unwrap();model.insert(key.clone(),value);}
                1=>{prop_assert_eq!(tree.replace(&key,value),old.ok_or(Error::NoKey));if old.is_some(){model.insert(key.clone(),value);}}
                2=>{prop_assert_eq!(tree.remove(&key),old.ok_or(Error::NoKey));model.remove(&key);}
                _=>{prop_assert_eq!(tree.get(&key).unwrap(),old);}
            }
            if kind==3 || kind==0 && old.is_some() || matches!(kind,1|2) && old.is_none() {prop_assert_eq!(&tree,&before);}
            let expected=model.iter().filter(|(k,_)|**k>=key && **k<upper).map(|(k,v)|(k.clone(),*v)).collect::<Vec<_>>();
            prop_assert_eq!(tree.cursor(Some(&key),Some(&upper)).unwrap().map(|v|owned(v.unwrap())).collect::<Vec<_>>(),expected.clone());
            prop_assert_eq!(tree.cursor(Some(&key),Some(&upper)).unwrap().rev().map(|v|owned(v.unwrap())).collect::<Vec<_>>(),expected.into_iter().rev().collect::<Vec<_>>());
            prop_assert_eq!(tree.cursor(None,None).unwrap().rev().map(|v|owned(v.unwrap())).collect::<Vec<_>>(),entries(&model).into_iter().rev().collect::<Vec<_>>());
            prop_assert_eq!(tree.validate().unwrap(),model.len());
            if step%11==0 {
                tree=if stable {BPlusTree::from_stable_pages(tree.root_id(),&tree.page_images().unwrap())} else {BPlusTree::from_pages(tree.root_id(),&tree.page_images().unwrap())}.unwrap();
            }
        }
        prop_assert_eq!(tree.range(None,None,MAX_INDEX_ENTRIES).unwrap(),entries(&model));
    }
}
