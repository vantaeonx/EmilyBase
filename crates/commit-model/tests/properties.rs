use emilybase_catalog::{DataType, Key, Value};
use emilybase_commit_model::Error;
use emilybase_database::{Event, EventKind};
use emilybase_index::RecordPointer;
use proptest::prelude::*;
use std::collections::BTreeMap;
mod support;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn two_table_staging_matches_independent_rows_and_rejected_plans_preserve_exact_state(
        commands in prop::collection::vec((0u8..5,1u64..3,-15i64..16,"[a-z]{0,12}"),1..50),
    ) {
        let mut live=support::model(&[("left",DataType::Integer),("right",DataType::Integer)]);
        let mut expected=BTreeMap::<(u64,i64),String>::new();
        for (operation,table,key,value) in commands {
            let before=live.clone();let fingerprint=live.fingerprint();
            let name=if table==1 {"left"} else {"right"};
            let mut staged=live.begin().unwrap();
            let row=vec![Value::Integer(key),Value::Text(value.clone())];
            let kind=match operation { 0|3=>EventKind::Insert(row),1|4=>EventKind::Replace(row),_=>EventKind::Delete(Key::Integer(key)) };
            let succeeds=match operation {0|3=>!expected.contains_key(&(table,key)),_=>expected.contains_key(&(table,key))};
            let result=staged.apply(Event {table_id:table,kind});
            prop_assert_eq!(result.is_ok(),succeeds);
            if succeeds && operation!=3 && operation!=4 {
                support::indexes(&live,&mut staged,&[name]);
                let prepared=staged.prepare().unwrap();
                prop_assert_eq!(live.fingerprint(),fingerprint);
                live.publish(prepared).unwrap();
                if operation==2 {expected.remove(&(table,key));} else {expected.insert((table,key),value);}
                prop_assert_eq!(live.transaction(),before.transaction()+1);
            } else {
                if succeeds && operation==4 {
                    let tree=staged.view().unwrap().export_primary_tree(name).unwrap();
                    let (binding,mut index)=support::candidate(&live,&staged,name,tree);
                    index.tree.replace(&Key::Integer(key),RecordPointer {page_id:1,slot_id:0}).unwrap();
                    staged.index(binding,index).unwrap();
                    prop_assert!(staged.prepare().is_err());
                } else if !succeeds {
                    prop_assert!(matches!(staged.prepare(),Err(Error::Aborted)));
                } else {drop(staged);}
                prop_assert_eq!(live.fingerprint(),fingerprint);
            }
            prop_assert_eq!(live.view().row_count(),expected.len());
            for table in [1u64,2] {
                let name=if table==1 {"left"} else {"right"};
                let count=expected.keys().filter(|(id,_)|*id==table).count();
                prop_assert_eq!(live.selection(table).unwrap().index().tree.len(),count);
                for key in -15..16 {
                    let actual=live.view().get(name,&Key::Integer(key)).unwrap();
                    let value=actual.map(|row|match &row[1] {Value::Text(text)=>text.as_str(),_=>panic!("unexpected schema value")});
                    prop_assert_eq!(value,expected.get(&(table,key)).map(String::as_str));
                    if actual.is_some() {
                        let pointer=live.selection(table).unwrap().index().tree.get(&Key::Integer(key)).unwrap().unwrap();
                        let location=live.view().row_location(name,&Key::Integer(key)).unwrap().unwrap();
                        prop_assert_eq!((pointer.page_id,pointer.slot_id),(location.page_id,location.slot_id));
                    }
                }
            }
            // Publishing the new Arc never changes the historical snapshot.
            prop_assert_eq!(before.fingerprint(),fingerprint);
        }
    }
}
