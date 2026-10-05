use emilybase_catalog::{DataType, Key, Value};
use emilybase_commit_format::{IndexKeyType, PageAddress, Predecessor, RootBinding};
use emilybase_commit_model::{EncodedComponents, Error, Model, Staged};
use emilybase_database::{Event, EventKind};
use emilybase_index::{BPlusTree, IndexSnapshot, RecordPointer};
#[path = "support/fragmented.rs"]
mod fragmented;
mod support;

// A canonical full arena assembled independently from the production bulk loader.
// 954 seven-key leaves + 64 first-level branches + 5 branches + one root = 1024.
fn maximum_arena() -> BPlusTree {
    let entries: Vec<_> = (0..6678)
        .map(|key| {
            (
                Key::Integer(key),
                RecordPointer {
                    page_id: key as u64 + 1,
                    slot_id: 0,
                },
            )
        })
        .collect();
    let tree = fragmented::tree(&entries, false);
    assert_eq!(tree.page_count(), 1024);
    assert_eq!(tree.root_id(), 1024);
    assert_eq!(tree.len(), 6678);
    tree
}

fn candidate(
    live: &Model,
    staged: &Staged,
    table: u64,
    tree: BPlusTree,
) -> (RootBinding, IndexSnapshot) {
    let previous = live.selection(table).unwrap();
    let revision = previous.binding().revision() + 1;
    let binding = RootBinding::new(
        PageAddress::primary(live.database_id(), table, tree.root_id()).unwrap(),
        IndexKeyType::Integer,
        revision,
        staged.transaction(),
        tree.len() as u64,
        0,
        tree.page_count() as u32,
        Some(
            Predecessor::new(
                previous.binding().revision(),
                previous.binding().transaction(),
                previous.index_fingerprint(),
            )
            .unwrap(),
        ),
    )
    .unwrap();
    (binding, IndexSnapshot { revision, tree })
}

#[test]
fn combined_component_arithmetic_refuses_the_first_page_above_2048() {
    assert!(EncodedComponents::from_counts(65536, 128, 2048).is_ok());
    assert!(matches!(
        EncodedComponents::from_counts(1, 128, 2049),
        Err(Error::Limit)
    ));
}

#[test]
fn accumulated_candidate_images_refuse_before_validation_and_abort_all_staging() {
    let live = support::model(&[
        ("one", DataType::Integer),
        ("two", DataType::Integer),
        ("three", DataType::Integer),
    ]);
    let before = live.fingerprint();
    let arena = maximum_arena();
    let mut staged = live.begin().unwrap();
    staged
        .apply(Event {
            table_id: 1,
            kind: EventKind::Insert(vec![Value::Integer(1), Value::Text("staged-only".into())]),
        })
        .unwrap();
    for table in [1, 2] {
        let (binding, index) = candidate(&live, &staged, table, arena.clone());
        staged.index(binding, index).unwrap();
    }
    // Individual images are valid. Their row coverage is deliberately unselected:
    // complete relational projection validation remains a separate prepare gate.
    let (binding, mut excess) = candidate(&live, &staged, 3, BPlusTree::new_stable());
    excess.revision = 0;
    // The budget error must precede even this invalid revision's image admission.
    assert!(matches!(staged.index(binding, excess), Err(Error::Limit)));
    assert!(matches!(staged.view(), Err(Error::Aborted)));
    assert!(matches!(staged.prepare(), Err(Error::Aborted)));
    assert_eq!(live.fingerprint(), before);
    assert_eq!(live.view().row_count(), 0);
    assert_eq!(live.encoded_components().unwrap().index_pages(), 3);
}
