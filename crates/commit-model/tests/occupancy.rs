use emilybase_commit_model::{EncodedComponents, MAX_SELECTED_INDEX_PAGES};
use emilybase_index::{Key, RecordPointer};
use proptest::prelude::*;
#[path = "support/fragmented.rs"]
mod fragmented;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn independent_minimum_occupancy_forests_fit_the_global_image_bound(
        sizes in prop::collection::vec(0usize..1501, 1..129),
        minimum_branches in any::<bool>(),
        text in any::<bool>(),
    ) {
        let mut remaining=10000usize;
        let mut entries_total=0;
        let mut pages_total=0;
        let mut encoded_total=0;
        for wanted in &sizes {
            let count=(*wanted).min(remaining);
            remaining-=count;
            let entries: Vec<_>=(0..count).map(|number| {
                let key=if text {
                    // Exactly 256 UTF-8 bytes, including the ordered prefix.
                    Key::Text(format!("key-{number:04}{}", "я".repeat(124)))
                } else {Key::Integer(number as i64)};
                (key,RecordPointer {page_id:number as u64+1,slot_id:0})
            }).collect();
            let tree=fragmented::tree(&entries,minimum_branches);
            prop_assert_eq!(tree.validate().unwrap(),count);
            entries_total+=tree.len();pages_total+=tree.page_count();
            let images=tree.page_images().unwrap();
            encoded_total+=(images.len()+1)*4096;
            for (key,pointer) in entries {
                prop_assert_eq!(tree.get(&key).unwrap(),Some(pointer));
            }
        }
        prop_assert!(entries_total<=10000);
        // Independent occupancy bound: P <= floor(8*K/49) + root_count.
        prop_assert!(pages_total<=8*entries_total/49+sizes.len());
        prop_assert!(pages_total<=1760);
        prop_assert!(pages_total<MAX_SELECTED_INDEX_PAGES);
        let report=EncodedComponents::from_counts(1,sizes.len() as u64,pages_total as u64).unwrap();
        prop_assert_eq!(report.index_bytes(),encoded_total as u64);
    }
}
