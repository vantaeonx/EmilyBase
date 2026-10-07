#![cfg(feature = "heap-profile")]
//! Deliberately inflated input is inside each isolated sample.
use emilybase_index::{BPlusTree, IndexPage, Key, RecordPointer};

#[global_allocator]
static ALLOCATOR: dhat::Alloc = dhat::Alloc;
fn padded() -> Key {
    let mut text = String::with_capacity(1024 * 1024);
    text.push_str("я\0");
    Key::Text(text)
}
fn pointer() -> RecordPointer {
    RecordPointer {
        page_id: 1,
        slot_id: 0,
    }
}
fn assert_retained(live: &dhat::HeapStats) {
    assert!(
        live.max_bytes >= 1024 * 1024,
        "input construction must be measured"
    );
    assert!(
        live.curr_bytes < 64 * 1024,
        "index retained unused input: {} bytes",
        live.curr_bytes
    );
}
#[test]
fn accepted_index_pages_and_owned_insert_discard_large_input_capacity() {
    let mut tree = BPlusTree::new_stable();
    let old = tree.clone();
    let old_bytes = old.page_images().unwrap();
    let profiler = dhat::Profiler::builder().testing().build();
    tree.insert(padded(), pointer()).unwrap();
    let live = dhat::HeapStats::get();
    assert_retained(&live);
    drop(tree);
    let released = dhat::HeapStats::get();
    drop(profiler);
    assert_eq!(released.curr_bytes, 0);
    assert_eq!(old.page_images().unwrap(), old_bytes);
    eprintln!(
        "index insert live={} peak={} released={}",
        live.curr_bytes, live.max_bytes, released.curr_bytes
    );
    let profiler = dhat::Profiler::builder().testing().build();
    let mut entries = Vec::with_capacity(1024);
    entries.push((padded(), pointer()));
    let leaf = IndexPage::leaf(1, entries, None).unwrap();
    let live = dhat::HeapStats::get();
    assert_retained(&live);
    drop(leaf);
    let released = dhat::HeapStats::get();
    drop(profiler);
    assert_eq!(released.curr_bytes, 0);
    eprintln!(
        "index leaf live={} peak={} released={}",
        live.curr_bytes, live.max_bytes, released.curr_bytes
    );
    let profiler = dhat::Profiler::builder().testing().build();
    let mut keys = Vec::with_capacity(1024);
    keys.push(padded());
    let mut children = Vec::with_capacity(1024);
    children.extend([1, 2]);
    let branch = IndexPage::branch(3, keys, children).unwrap();
    let live = dhat::HeapStats::get();
    assert_retained(&live);
    drop(branch);
    let released = dhat::HeapStats::get();
    drop(profiler);
    assert_eq!(released.curr_bytes, 0);
    eprintln!(
        "index branch live={} peak={} released={}",
        live.curr_bytes, live.max_bytes, released.curr_bytes
    );
}
