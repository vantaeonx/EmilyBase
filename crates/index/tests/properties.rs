use emilybase_index::*;
use proptest::prelude::*;
use std::collections::BTreeMap;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    #[test]
    fn generated_operations_match_an_independent_sorted_map(
        operations in prop::collection::vec((0u8..3, -300i64..300, any::<u16>()), 1..400)
    ) {
        let mut tree = BPlusTree::new();
        let mut model = BTreeMap::new();
        for (step, (kind, value, slot)) in operations.into_iter().enumerate() {
            let key = if kind == 0 { Key::Text(format!("clé-{value:04}")) } else { Key::Integer(value) };
            let pointer = RecordPointer { page_id: step as u64 + 1, slot_id: slot };
            let before = tree.clone();
            if kind < 2 {
                if model.contains_key(&key) {
                    prop_assert_eq!(tree.insert(key.clone(), pointer), Err(Error::Duplicate));
                    prop_assert_eq!(&tree, &before);
                } else {
                    tree.insert(key.clone(), pointer).unwrap();
                    model.insert(key.clone(), pointer);
                }
            }
            prop_assert_eq!(tree.get(&key).unwrap(), model.get(&key).copied());
            prop_assert_eq!(tree.len(), model.len());
            prop_assert_eq!(tree.validate().unwrap(), model.len());
            let expected = model.range(key.clone()..).take(23).map(|(k,v)| (k.clone(), *v)).collect::<Vec<_>>();
            prop_assert_eq!(tree.range(Some(&key), None, 23).unwrap(), expected);
            if step % 19 == 0 {
                tree = BPlusTree::from_pages(tree.root_id(), &tree.page_images().unwrap()).unwrap();
                prop_assert_eq!(tree.range(None, None, MAX_INDEX_ENTRIES).unwrap(), model.iter().map(|(k,v)| (k.clone(), *v)).collect::<Vec<_>>());
            }
        }
    }
}
