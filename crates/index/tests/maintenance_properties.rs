use emilybase_index::*;
use proptest::prelude::*;
use std::collections::BTreeMap;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    #[test]
    fn mixed_crud_rebalances_like_an_independent_model(
        operations in prop::collection::vec((0u8..4,-50i64..320,any::<u16>(),any::<bool>()),1..400)
    ) {
        let mut model:BTreeMap<_,_>=(0..240).map(|i|(Key::Integer(i),RecordPointer {page_id:i as u64+1,slot_id:i as u16})).collect();
        let mut tree=BPlusTree::from_sorted(&model.iter().map(|(k,v)|(k.clone(),*v)).collect::<Vec<_>>()).unwrap();
        for (step,(kind,number,slot,text)) in operations.into_iter().enumerate() {
            let key=if text {Key::Text(format!("clé-{number:04}"))} else {Key::Integer(number)};
            let end=if text {Key::Text(format!("clé-{:04}",number+17))} else {Key::Integer(number+17)};
            let value=RecordPointer {page_id:step as u64+1000,slot_id:slot};
            let old=model.get(&key).copied();
            let before=tree.clone();
            match kind {
                0=>{
                    if old.is_some() {prop_assert_eq!(tree.insert(key.clone(),value),Err(Error::Duplicate));}
                    else {tree.insert(key.clone(),value).unwrap();model.insert(key.clone(),value);}
                }
                1=>{
                    prop_assert_eq!(tree.replace(&key,value),old.ok_or(Error::NoKey));
                    if old.is_some(){model.insert(key.clone(),value);}
                }
                2=>{prop_assert_eq!(tree.remove(&key),old.ok_or(Error::NoKey));model.remove(&key);}
                _=>{prop_assert_eq!(tree.get(&key).unwrap(),old);}
            }
            if (kind==0 && old.is_some()) || ((kind==1 || kind==2) && old.is_none()) || kind==3 {
                prop_assert_eq!(&tree,&before);
            }
            prop_assert_eq!(tree.len(),model.len());
            prop_assert_eq!(tree.validate().unwrap(),model.len());
            prop_assert_eq!(tree.get(&key).unwrap(),model.get(&key).copied());
            let limit=usize::from(slot%32);
            let expected=model.iter().filter(|(k,_)|**k>=key && **k<end).take(limit).map(|(k,v)|(k.clone(),*v)).collect::<Vec<_>>();
            prop_assert_eq!(tree.range(Some(&key),Some(&end),limit).unwrap(),expected);
            if step%19==0 {
                let restored=BPlusTree::from_pages(tree.root_id(),&tree.page_images().unwrap()).unwrap();
                prop_assert_eq!(&restored,&tree);
                tree=restored;
            }
            if step%41==0 {
                let source=model.iter().map(|(k,v)|(k.clone(),*v)).collect::<Vec<_>>();
                tree=BPlusTree::from_sorted(&source).unwrap();
                prop_assert_eq!(tree.range(None,None,MAX_INDEX_ENTRIES).unwrap(),source);
            }
        }
    }

    #[test]
    fn generated_bulk_import_and_incremental_build_have_identical_queries(
        input in prop::collection::btree_map(-1000i64..1000,any::<u16>(),0..500)
    ) {
        let source=input.iter().map(|(k,v)|(Key::Integer(*k),RecordPointer {page_id:u64::MAX,slot_id:*v})).collect::<Vec<_>>();
        let bulk=BPlusTree::from_sorted(&source).unwrap();
        let mut incremental=BPlusTree::new();
        for (key,value) in source.iter().rev() {incremental.insert(key.clone(),*value).unwrap();}
        prop_assert_eq!(bulk.range(None,None,MAX_INDEX_ENTRIES).unwrap(),incremental.range(None,None,MAX_INDEX_ENTRIES).unwrap());
        prop_assert_eq!(bulk.validate().unwrap(),input.len());
        let mut restored=BPlusTree::from_pages(bulk.root_id(),&bulk.page_images().unwrap()).unwrap();
        for (key,old) in source.iter().step_by(3) {
            prop_assert_eq!(restored.remove(key).unwrap(),*old);
            prop_assert_eq!(restored.get(key).unwrap(),None);
        }
        prop_assert_eq!(restored.validate().unwrap(),source.len()-source.iter().step_by(3).count());
    }
}
