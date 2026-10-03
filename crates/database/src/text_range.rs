use crate::primary::eligible;
use crate::{Error, MAX_ROWS, Result, Snapshot};
use emilybase_catalog::{DataType, Key, Row};

impl Snapshot {
    /// UTF-8 byte-order text interval, inclusive lower and exclusive upper.
    /// Short tree entries merge with long live keys; long bounds use the live map.
    pub fn scan_text_range(
        &self,
        name: &str,
        lower: Option<&str>,
        upper: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Row>> {
        let schema = self.schema(name)?;
        if schema.columns[usize::from(schema.primary_key)].data_type != DataType::Text {
            return Err(Error::TextRangeType);
        }
        if limit > MAX_ROWS {
            return Err(Error::Limit("range rows"));
        }
        if [lower, upper]
            .into_iter()
            .flatten()
            .any(|text| text.len() > emilybase_catalog::MAX_VALUE_BYTES)
        {
            return Err(Error::Limit("range key bytes"));
        }
        if limit == 0 || matches!((lower, upper), (Some(a), Some(b)) if a >= b) {
            return Ok(Vec::new());
        }
        let table_id = self.table_id(name)?;
        let table = self.state.table(name)?;
        let lower = lower.map(|text| Key::Text(text.into()));
        let upper = upper.map(|text| Key::Text(text.into()));
        let bounds = (
            lower
                .as_ref()
                .map_or(std::ops::Bound::Unbounded, std::ops::Bound::Included),
            upper
                .as_ref()
                .map_or(std::ops::Bound::Unbounded, std::ops::Bound::Excluded),
        );
        if lower.iter().chain(&upper).all(eligible) {
            let tree = self
                .primary_indexes
                .tree(table_id, &table.rows, &self.locations)?;
            let entries = tree
                .range(lower.as_ref(), upper.as_ref(), limit)
                .map_err(|_| Error::PrimaryIndex("text range lookup failed"))?;
            let expected = table
                .rows
                .range(bounds)
                .filter(|(key, _)| eligible(key))
                .take(limit);
            if !entries
                .iter()
                .map(|(key, _)| key)
                .eq(expected.map(|(key, _)| key))
            {
                return Err(Error::PrimaryIndex("text range key mismatch"));
            }
            for (key, pointer) in entries {
                let location = self
                    .locations
                    .get(table_id, &key)
                    .ok_or(Error::PrimaryIndex("unknown text range key"))?;
                if (pointer.page_id, pointer.slot_id) != (location.page_id, location.slot_id) {
                    return Err(Error::PrimaryIndex("obsolete text range pointer"));
                }
                self.resolve_row_location(name, &key, location)?;
            }
        }
        // Live ordered keys include the excluded long keys and define the merged limit.
        // Every returned row still resolves its actual current physical image.
        table
            .rows
            .range(bounds)
            .take(limit)
            .map(|(key, _)| {
                let location = self
                    .locations
                    .get(table_id, key)
                    .ok_or(Error::PrimaryIndex("missing text range location"))?;
                Ok(self.resolve_row_location(name, key, location)?.clone())
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primary::IndexChange;
    use crate::{Event, EventKind};
    use emilybase_catalog::{Column, Schema, Value};
    use emilybase_index::{BPlusTree, RecordPointer};

    #[test]
    fn missing_extra_or_wrong_short_entries_fail_even_when_a_long_key_would_fill_the_limit() {
        let mut snapshot = Snapshot::empty().unwrap();
        snapshot
            .apply(Event {
                table_id: 1,
                kind: EventKind::Create(Schema {
                    name: "t".into(),
                    columns: vec![Column {
                        name: "id".into(),
                        data_type: DataType::Text,
                        nullable: false,
                    }],
                    primary_key: 0,
                }),
            })
            .unwrap();
        for key in ["b".into(), format!("a{}", "x".repeat(3071))] {
            snapshot
                .apply(Event {
                    table_id: 1,
                    kind: EventKind::Insert(vec![Value::Text(key)]),
                })
                .unwrap();
        }
        let before = snapshot.page_fingerprint();
        let valid = snapshot.export_primary_tree("t").unwrap();
        let mut extra = valid.clone();
        extra
            .insert(
                Key::Text("c".into()),
                RecordPointer {
                    page_id: 1,
                    slot_id: 0,
                },
            )
            .unwrap();
        let mut wrong = valid;
        wrong
            .replace(
                &Key::Text("b".into()),
                RecordPointer {
                    page_id: u64::MAX,
                    slot_id: u16::MAX,
                },
            )
            .unwrap();
        for (tree, limit) in [(BPlusTree::new_stable(), 1), (extra, MAX_ROWS), (wrong, 1)] {
            let mut branch = snapshot.clone();
            branch.primary_indexes.apply(IndexChange::Ready(1, tree));
            assert!(
                branch
                    .scan_text_range("t", Some("a"), Some("d"), limit)
                    .is_err()
            );
            assert_eq!(branch.page_fingerprint(), before);
        }
    }
}
