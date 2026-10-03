use std::collections::BTreeMap;

use emilybase_index::*;
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn persisted_mutations_and_revisions_match_an_independent_map_after_every_reopen(
        operations in prop::collection::vec((0u8..3,-16i64..16,1u64..1000),1..40)
    ) {
        let temp=tempfile::tempdir().unwrap();
        let root=temp.path().join("synthetic-index");
        let mut store=IndexStore::create(&root,&BPlusTree::new_stable()).unwrap();
        let mut expected=BTreeMap::new();
        let mut revision=1;
        for (action,key,value) in operations {
            let key=Key::Integer(key);
            let pointer=RecordPointer {page_id:value,slot_id:0};
            let mut tree=store.snapshot().unwrap().tree.clone();
            let previous=std::fs::read(root.join("tree.ebif")).unwrap();
            let success=match action {
                0=>{
                    let existed=expected.contains_key(&key);
                    prop_assert_eq!(tree.insert(key.clone(),pointer).is_ok(),!existed);
                    if !existed {expected.insert(key,pointer);}
                    !existed
                },
                1=>{
                    let old=expected.remove(&key);
                    prop_assert_eq!(tree.remove(&key).ok(),old);
                    old.is_some()
                },
                _=>{
                    let old=expected.get_mut(&key).map(|old|std::mem::replace(old,pointer));
                    prop_assert_eq!(tree.replace(&key,pointer).ok(),old);
                    old.is_some()
                },
            };
            if success {
                revision+=1;
                if revision%2==0 {prop_assert_eq!(store.replace(&tree).unwrap(),revision);}
                else {
                    let delta=store.snapshot().unwrap().delta_to(&tree).unwrap();
                    prop_assert_eq!(store.apply(&delta).unwrap(),revision);
                }
            } else {prop_assert_eq!(std::fs::read(root.join("tree.ebif")).unwrap(),previous);}
            drop(store);
            store=IndexStore::open(&root).unwrap();
            let snapshot=store.snapshot().unwrap();
            prop_assert_eq!(snapshot.revision,revision);
            prop_assert_eq!(snapshot.tree.range(None,None,MAX_INDEX_ENTRIES).unwrap(),expected.iter().map(|(key,value)|(key.clone(),*value)).collect::<Vec<_>>());
            prop_assert_eq!(std::fs::read(root.join("tree.ebif")).unwrap(),snapshot.encode().unwrap());
        }
    }
}
