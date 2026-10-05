use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};

use crate::state::Rows;
use emilybase_catalog::Key;
use emilybase_index::{BPlusTree, MAX_KEY_BYTES, RecordPointer};

use crate::location::{Change, Locations};
use crate::{Error, Event, EventKind, Result, RowLocation};

/// Statistics for the derived in-memory primary-key point-lookup tree.
/// Its pages are reconstructed from relational history, not persisted independently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct PrimaryIndexInfo {
    pub entries: usize,
    pub excluded_long_keys: usize,
    pub pages: usize,
    pub root_id: u64,
}

type Cache = Arc<OnceLock<std::result::Result<BPlusTree, &'static str>>>;

/// Immutable snapshots share a tree. Writes stage maintenance of initialized
/// trees and replace only the affected cell. Building performs no filesystem I/O.
#[derive(Clone, Default)]
pub(crate) struct PrimaryIndexes(BTreeMap<u64, Cache>);

pub(crate) enum IndexChange {
    Reset(u64),
    Drop(u64),
    Ready(u64, BPlusTree),
    None,
}

fn pointer(location: RowLocation) -> RecordPointer {
    RecordPointer {
        page_id: location.page_id,
        slot_id: location.slot_id,
    }
}

pub(crate) fn eligible(key: &Key) -> bool {
    !matches!(key, Key::Text(text) if text.len() > MAX_KEY_BYTES)
}

impl PrimaryIndexes {
    pub(crate) fn install(&mut self, table_id: u64, tree: BPlusTree) {
        self.apply(IndexChange::Ready(table_id, tree));
    }
    pub(crate) fn from_tables(ids: impl Iterator<Item = u64>) -> Self {
        Self(ids.map(|id| (id, Arc::new(OnceLock::new()))).collect())
    }

    pub(crate) fn prepare(
        &self,
        event: &Event,
        change: &Change,
        locations: &Locations,
    ) -> Result<IndexChange> {
        let table_id = event.table_id;
        if matches!(event.kind, EventKind::Create(_)) {
            return Ok(IndexChange::Reset(table_id));
        }
        let key = match change {
            Change::Drop(_) => return Ok(IndexChange::Drop(table_id)),
            Change::Put(_, key, _) | Change::Delete(_, key) => key,
            Change::None => return Ok(IndexChange::None),
        };
        let cell = self
            .0
            .get(&table_id)
            .ok_or(Error::PrimaryIndex("missing table cell"))?;
        // A cloned uninitialized cell must not later build using a historical branch.
        let Some(cached) = cell.get() else {
            return Ok(IndexChange::Reset(table_id));
        };
        let tree = cached
            .as_ref()
            .map_err(|reason| Error::PrimaryIndex(reason))?;
        if !eligible(key) {
            return Ok(IndexChange::None);
        }
        let old = tree
            .get(key)
            .map_err(|_| Error::PrimaryIndex("maintenance lookup failed"))?;
        if old != locations.get(table_id, key).map(pointer) {
            return Err(Error::PrimaryIndex("maintenance pointer mismatch"));
        }
        let mut staged = tree.clone();
        let result = match change {
            Change::Put(_, key, location) if old.is_some() => {
                staged.replace(key, pointer(*location)).map(|_| ())
            }
            Change::Put(_, key, location) => staged.insert(key.clone(), pointer(*location)),
            Change::Delete(_, key) => staged.remove(key).map(|_| ()),
            _ => return Err(Error::PrimaryIndex("unexpected maintenance change")),
        };
        match result {
            Ok(()) => Ok(IndexChange::Ready(table_id, staged)),
            // A fragmented incremental arena can fill before the table row limit.
            // Drop the derived cache; the next lookup bulk-builds the accepted rows.
            Err(emilybase_index::Error::Limit) => Ok(IndexChange::Reset(table_id)),
            Err(_) => Err(Error::PrimaryIndex("staged maintenance failed")),
        }
    }

    pub(crate) fn apply(&mut self, change: IndexChange) {
        match change {
            IndexChange::Reset(id) => {
                self.0.insert(id, Arc::new(OnceLock::new()));
            }
            IndexChange::Drop(id) => {
                self.0.remove(&id);
            }
            IndexChange::Ready(id, tree) => {
                self.0.insert(id, Arc::new(OnceLock::from(Ok(tree))));
            }
            IndexChange::None => (),
        }
    }

    pub(crate) fn tree(
        &self,
        table_id: u64,
        rows: &Rows,
        locations: &Locations,
    ) -> Result<&BPlusTree> {
        let cache = self
            .0
            .get(&table_id)
            .ok_or(Error::PrimaryIndex("missing table cell"))?;
        cache
            .get_or_init(|| build(table_id, rows, locations))
            .as_ref()
            .map_err(|reason| Error::PrimaryIndex(reason))
    }

    pub(crate) fn info(
        &self,
        table_id: u64,
        rows: &Rows,
        locations: &Locations,
    ) -> Result<PrimaryIndexInfo> {
        let tree = self.tree(table_id, rows, locations)?;
        Ok(PrimaryIndexInfo {
            entries: tree.len(),
            excluded_long_keys: rows
                .len()
                .checked_sub(tree.len())
                .ok_or(Error::PrimaryIndex("entry count exceeds live rows"))?,
            pages: tree.page_count(),
            root_id: tree.root_id(),
        })
    }
}

fn build(
    table_id: u64,
    rows: &Rows,
    locations: &Locations,
) -> std::result::Result<BPlusTree, &'static str> {
    let mut entries = Vec::with_capacity(rows.len());
    for key in rows.keys().map(Arc::as_ref).filter(|key| eligible(key)) {
        let location = locations
            .get(table_id, key)
            .ok_or("missing live row location")?;
        if location.table_id != table_id {
            return Err("location table mismatch");
        }
        entries.push((
            key.clone(),
            RecordPointer {
                page_id: location.page_id,
                slot_id: location.slot_id,
            },
        ));
    }
    // The 10000-row table bound fits a bottom-up bulk build (768 <= 1024 pages).
    // Incremental arena allocation limits must never narrow accepted table rows.
    BPlusTree::from_sorted(&entries).map_err(|_| "invalid derived tree")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Event, EventKind, Snapshot};
    use emilybase_catalog::{Column, DataType, Schema, Value};

    fn initialized() -> Snapshot {
        let mut snapshot = Snapshot::empty().unwrap();
        for name in ["a", "b"] {
            let table_id = snapshot.next_table_id();
            snapshot
                .apply(Event {
                    table_id,
                    kind: EventKind::Create(Schema {
                        name: name.into(),
                        columns: vec![Column {
                            name: "id".into(),
                            data_type: DataType::Integer,
                            nullable: false,
                        }],
                        primary_key: 0,
                    }),
                })
                .unwrap();
            snapshot
                .apply(Event {
                    table_id,
                    kind: EventKind::Insert(vec![Value::Integer(1)]),
                })
                .unwrap();
        }
        snapshot
    }

    #[test]
    fn caches_initialize_once_share_immutable_images_and_invalidate_only_successful_table_writes() {
        let mut snapshot = initialized();
        assert!(
            snapshot
                .primary_indexes
                .0
                .values()
                .all(|cell| cell.get().is_none())
        );
        snapshot.get("a", &Key::Integer(1)).unwrap();
        snapshot.get("b", &Key::Integer(1)).unwrap();
        let historical = snapshot.clone();
        let first = historical.primary_indexes.0.get(&1).unwrap();
        let sibling = historical.primary_indexes.0.get(&2).unwrap();
        assert!(Arc::ptr_eq(
            first,
            snapshot.primary_indexes.0.get(&1).unwrap()
        ));
        assert!(
            snapshot
                .apply(Event {
                    table_id: 1,
                    kind: EventKind::Insert(vec![Value::Integer(1)])
                })
                .is_err()
        );
        assert!(Arc::ptr_eq(
            first,
            snapshot.primary_indexes.0.get(&1).unwrap()
        ));
        snapshot
            .apply(Event {
                table_id: 1,
                kind: EventKind::Insert(vec![Value::Integer(2)]),
            })
            .unwrap();
        assert!(!Arc::ptr_eq(
            first,
            snapshot.primary_indexes.0.get(&1).unwrap()
        ));
        assert!(Arc::ptr_eq(
            sibling,
            snapshot.primary_indexes.0.get(&2).unwrap()
        ));
        assert!(snapshot.primary_indexes.0.get(&1).unwrap().get().is_some());
        assert!(snapshot.get("a", &Key::Integer(2)).unwrap().is_some());
        assert!(historical.get("a", &Key::Integer(2)).unwrap().is_none());
    }

    #[test]
    fn initialized_point_updates_keep_a_ready_tree_without_rescanning_table_rows() {
        let mut snapshot = initialized();
        snapshot.get("a", &Key::Integer(1)).unwrap();
        snapshot
            .apply(Event {
                table_id: 1,
                kind: EventKind::Replace(vec![Value::Integer(1)]),
            })
            .unwrap();
        assert!(snapshot.primary_indexes.0.get(&1).unwrap().get().is_some());
        assert_eq!(
            snapshot.get("a", &Key::Integer(1)).unwrap(),
            Some(&vec![Value::Integer(1)])
        );
    }

    #[test]
    fn actual_incremental_arena_exhaustion_rebuilds_without_narrowing_table_capacity() {
        let mut snapshot = initialized();
        snapshot.primary_index_info("a").unwrap();
        let mut rebuilt = 0;
        // One row belongs to b; a can admit the remaining 9999 rows.
        for key in 2..crate::MAX_ROWS as i64 {
            snapshot
                .apply(Event {
                    table_id: 1,
                    kind: EventKind::Insert(vec![Value::Integer(key)]),
                })
                .unwrap();
            if snapshot.primary_indexes.0.get(&1).unwrap().get().is_none() {
                rebuilt += 1;
                assert_eq!(
                    snapshot.primary_index_info("a").unwrap().entries,
                    key as usize
                );
            }
        }
        assert!(rebuilt > 0, "the real incremental page limit must execute");
        assert_eq!(snapshot.row_count(), crate::MAX_ROWS);
        assert_eq!(
            snapshot.primary_index_info("a").unwrap().entries,
            crate::MAX_ROWS - 1
        );
        for key in [1, 1000, 7000, crate::MAX_ROWS as i64 - 1] {
            assert_eq!(
                snapshot.get("a", &Key::Integer(key)).unwrap(),
                Some(&vec![Value::Integer(key)])
            );
        }
        let replay = Snapshot::from_pages(snapshot.pages().cloned().collect()).unwrap();
        assert_eq!(
            replay.primary_index_info("a").unwrap().entries,
            crate::MAX_ROWS - 1
        );
        assert_eq!(
            replay.get("b", &Key::Integer(1)).unwrap(),
            Some(&vec![Value::Integer(1)])
        );
    }

    #[test]
    fn historical_uninitialized_cells_cannot_build_using_the_new_branch_rows() {
        let mut snapshot = initialized();
        let historical = snapshot.clone();
        snapshot
            .apply(Event {
                table_id: 1,
                kind: EventKind::Insert(vec![Value::Integer(2)]),
            })
            .unwrap();
        assert_eq!(historical.primary_index_info("a").unwrap().entries, 1);
        assert!(historical.get("a", &Key::Integer(2)).unwrap().is_none());
        assert_eq!(snapshot.primary_index_info("a").unwrap().entries, 2);
        assert!(snapshot.get("a", &Key::Integer(2)).unwrap().is_some());
    }

    #[test]
    fn missing_or_wrong_index_pointers_fail_closed_instead_of_returning_stale_rows() {
        for missing in [false, true] {
            let mut snapshot = initialized();
            let tree = if missing {
                BPlusTree::new()
            } else {
                BPlusTree::from_sorted(&[(
                    Key::Integer(1),
                    RecordPointer {
                        page_id: 99,
                        slot_id: 0,
                    },
                )])
                .unwrap()
            };
            let cell = OnceLock::new();
            cell.set(Ok(tree)).unwrap();
            snapshot.primary_indexes.0.insert(1, Arc::new(cell));
            let before = snapshot
                .pages()
                .map(|page| page.encode())
                .collect::<Vec<_>>();
            assert!(matches!(
                snapshot.get("a", &Key::Integer(1)),
                Err(Error::PrimaryIndex(_))
            ));
            assert!(matches!(
                snapshot.scan_integer_range("a", None, None, 32),
                Err(Error::PrimaryIndex(_))
            ));
            assert!(matches!(
                snapshot.apply(Event {
                    table_id: 1,
                    kind: EventKind::Replace(vec![Value::Integer(1)])
                }),
                Err(Error::PrimaryIndex(_))
            ));
            assert_eq!(
                snapshot
                    .pages()
                    .map(|page| page.encode())
                    .collect::<Vec<_>>(),
                before
            );
            assert!(snapshot.get("b", &Key::Integer(1)).unwrap().is_some());
        }
    }
}
