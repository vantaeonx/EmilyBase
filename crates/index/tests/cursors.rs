use emilybase_index::{BPlusTree, Error, Key, MAX_INDEX_ENTRIES, RecordPointer};
use proptest::prelude::*;
use std::collections::{BTreeMap, VecDeque};

fn pointer(i: usize) -> RecordPointer {
    RecordPointer {
        page_id: i as u64 + 1,
        slot_id: i as u16,
    }
}
fn owned(entry: (&Key, RecordPointer)) -> (Key, RecordPointer) {
    (entry.0.clone(), entry.1)
}
fn ints(n: usize) -> Vec<(Key, RecordPointer)> {
    (0..n)
        .map(|i| (Key::Integer(i as i64 * 2 - 100), pointer(i)))
        .collect()
}

#[test]
fn every_separator_and_leaf_boundary_preserves_both_directions_and_interleaving() {
    for count in [0, 1, 7, 14, 15, 29, 210, 211, 400] {
        let entries = ints(count);
        let tree = BPlusTree::from_sorted(&entries).unwrap();
        let before = tree.page_images().unwrap();
        for bound in -103..=(count as i64 * 2 - 97) {
            let lower = Key::Integer(bound);
            let upper = Key::Integer(bound + 31);
            for (lower, upper) in [
                (None, None),
                (Some(&lower), None),
                (None, Some(&upper)),
                (Some(&lower), Some(&upper)),
                (Some(&upper), Some(&lower)),
                (Some(&lower), Some(&lower)),
            ] {
                let expected = entries
                    .iter()
                    .filter(|(key, _)| {
                        lower.is_none_or(|v| key >= v) && upper.is_none_or(|v| key < v)
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                let forward = tree
                    .cursor(lower, upper)
                    .unwrap()
                    .map(|v| owned(v.unwrap()))
                    .collect::<Vec<_>>();
                assert_eq!(forward, expected);
                let backward = tree
                    .cursor(lower, upper)
                    .unwrap()
                    .rev()
                    .map(|v| owned(v.unwrap()))
                    .collect::<Vec<_>>();
                assert_eq!(backward, expected.iter().rev().cloned().collect::<Vec<_>>());
                let mut remaining = VecDeque::from(expected);
                let mut cursor = tree.cursor(lower, upper).unwrap();
                for step in 0..remaining.len() + 4 {
                    let (actual, expected) = if step % 3 == 0 {
                        (cursor.next_back(), remaining.pop_back())
                    } else {
                        (cursor.next(), remaining.pop_front())
                    };
                    assert_eq!(actual.map(|v| owned(v.unwrap())), expected);
                    assert!(cursor.size_hint().1.unwrap() >= remaining.len());
                }
                assert!(cursor.next().is_none());
                assert!(cursor.next_back().is_none());
                assert_eq!(cursor.size_hint(), (0, Some(0)));
            }
        }
        assert_eq!(tree.page_images().unwrap(), before);
    }
}

#[test]
fn mixed_types_empty_nul_unicode_and_maximum_text_bounds_follow_key_order() {
    let mut entries = vec![
        (Key::Integer(i64::MIN), pointer(0)),
        (Key::Integer(i64::MAX), pointer(1)),
    ];
    for (i, key) in [
        String::new(),
        "\0".into(),
        "a".into(),
        "a\0".into(),
        "界".into(),
        "😀".into(),
        "z".repeat(256),
    ]
    .into_iter()
    .enumerate()
    {
        entries.push((Key::Text(key), pointer(i + 2)));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let tree = BPlusTree::from_sorted(&entries).unwrap();
    for (lower, _) in &entries {
        for (upper, _) in &entries {
            let expected = entries
                .iter()
                .filter(|(k, _)| k >= lower && k < upper)
                .cloned()
                .collect::<Vec<_>>();
            assert_eq!(
                tree.cursor(Some(lower), Some(upper))
                    .unwrap()
                    .map(|v| owned(v.unwrap()))
                    .collect::<Vec<_>>(),
                expected
            );
            assert_eq!(
                tree.cursor(Some(lower), Some(upper))
                    .unwrap()
                    .rev()
                    .map(|v| owned(v.unwrap()))
                    .collect::<Vec<_>>(),
                expected.into_iter().rev().collect::<Vec<_>>()
            );
        }
    }
    for tree in [&tree, &BPlusTree::new()] {
        let too_long = Key::Text("x".repeat(257));
        assert!(matches!(
            tree.cursor(Some(&too_long), Some(&too_long)),
            Err(Error::KeySize)
        ));
        assert!(matches!(
            tree.cursor(None, Some(&too_long)),
            Err(Error::KeySize)
        ));
    }
}

#[test]
fn full_capacity_stable_and_dense_trees_return_borrowed_keys_without_changing_images() {
    let entries = ints(MAX_INDEX_ENTRIES);
    let dense = BPlusTree::from_sorted(&entries).unwrap();
    let stable = BPlusTree::from_sorted_stable(&entries).unwrap();
    for tree in [&dense, &stable] {
        let before = tree.page_images().unwrap();
        let mut cursor = tree.cursor(None, None).unwrap();
        for (i, expected) in entries.iter().enumerate() {
            let (key, value) = cursor.next().unwrap().unwrap();
            assert_eq!((key, value), (&expected.0, expected.1));
            if i == 2 {
                break;
            }
        }
        assert_eq!(
            tree.cursor(None, None)
                .unwrap()
                .rev()
                .map(|v| owned(v.unwrap()))
                .collect::<Vec<_>>(),
            entries.iter().rev().cloned().collect::<Vec<_>>()
        );
        assert_eq!(tree.page_images().unwrap(), before);
        let restored = BPlusTree::from_pages(tree.root_id(), &before).unwrap();
        assert_eq!(
            restored.cursor(None, None).unwrap().count(),
            MAX_INDEX_ENTRIES
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn arbitrary_mixed_end_consumption_matches_an_independent_ordered_map(
        keys in prop::collection::btree_set(-2000i64..2000,0..700),
        lower in prop::option::of(-2200i64..2200), upper in prop::option::of(-2200i64..2200),
        ends in prop::collection::vec(any::<bool>(),0..750),
    ) {
        let model=keys.into_iter().enumerate().map(|(i,k)|(Key::Integer(k),pointer(i))).collect::<BTreeMap<_,_>>();
        let entries=model.iter().map(|(k,v)|(k.clone(),*v)).collect::<Vec<_>>();
        let tree=BPlusTree::from_sorted_stable(&entries).unwrap();
        let lower=lower.map(Key::Integer);let upper=upper.map(Key::Integer);
        let mut expected=model.iter().filter(|(k,_)| lower.as_ref().is_none_or(|v|*k>=v) && upper.as_ref().is_none_or(|v|*k<v)).map(|(k,v)|(k.clone(),*v)).collect::<VecDeque<_>>();
        let mut cursor=tree.cursor(lower.as_ref(),upper.as_ref()).unwrap();
        for backwards in ends {
            let (actual,want)=if backwards {(cursor.next_back(),expected.pop_back())}else{(cursor.next(),expected.pop_front())};
            prop_assert_eq!(actual.map(|v|owned(v.unwrap())),want);
        }
        prop_assert_eq!(cursor.map(|v|owned(v.unwrap())).collect::<Vec<_>>(),expected.into_iter().collect::<Vec<_>>());
        prop_assert_eq!(tree.page_images().unwrap(),BPlusTree::from_sorted_stable(&entries).unwrap().page_images().unwrap());
    }
}
