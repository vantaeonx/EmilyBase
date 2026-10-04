use crate::primary::eligible;
use crate::{Error, MAX_ROWS, Result, Snapshot};
use emilybase_catalog::{Key, Row};
use emilybase_index::{BPlusTree, RangeCursor};
use std::collections::btree_map::Range;
use std::ops::Bound;

enum Projection<'a> {
    Interval(RangeCursor<'a>),
    Points(&'a BPlusTree),
    Empty,
}

/// Borrowed live rows in primary-key order. Bounds are inclusive/exclusive.
/// Every consumed row resolves its physical image. Errors fuse both ends.
pub struct PrimaryRows<'a> {
    snapshot: &'a Snapshot,
    name: &'a str,
    table_id: u64,
    rows: Option<Range<'a, Key, Row>>,
    projection: Projection<'a>,
    emitted: usize,
    upper_hint: usize,
    done: bool,
}

impl Snapshot {
    /// Read without cloning result rows; the borrowed snapshot remains immutable.
    /// Short bounds verify the B+ interval as it is consumed, including reverse reads.
    /// Long text keys merge through the live order; long bounds use checked points.
    pub fn primary_rows(
        &self,
        name: &str,
        lower: Option<&Key>,
        upper: Option<&Key>,
    ) -> Result<PrimaryRows<'_>> {
        let table = self.state.table(name)?;
        for key in lower.into_iter().chain(upper) {
            table.schema.validate_key(key)?;
        }
        if table.rows.len() > MAX_ROWS {
            return Err(Error::Limit("primary cursor rows"));
        }
        let table_id = self.table_id(name)?;
        let done = matches!((lower, upper), (Some(a), Some(b)) if a >= b);
        let (rows, projection) = if done {
            (None, Projection::Empty)
        } else {
            let tree = self
                .primary_indexes
                .tree(table_id, &table.rows, &self.locations)?;
            let projection = if lower.into_iter().chain(upper).all(eligible) {
                Projection::Interval(
                    tree.cursor(lower, upper)
                        .map_err(|_| Error::PrimaryIndex("primary cursor seek"))?,
                )
            } else {
                Projection::Points(tree)
            };
            (
                Some(table.rows.range((
                    lower.map_or(Bound::Unbounded, Bound::Included),
                    upper.map_or(Bound::Unbounded, Bound::Excluded),
                ))),
                projection,
            )
        };
        Ok(PrimaryRows {
            snapshot: self,
            name: &table.schema.name,
            table_id,
            rows,
            projection,
            emitted: 0,
            upper_hint: table.rows.len() + 1,
            done,
        })
    }
}

impl<'a> PrimaryRows<'a> {
    fn advance(&mut self, backwards: bool) -> Result<Option<&'a Row>> {
        let row = self.rows.as_mut().and_then(|rows| {
            if backwards {
                rows.next_back()
            } else {
                rows.next()
            }
        });
        let Some((key, _)) = row else {
            if let Projection::Interval(cursor) = &mut self.projection {
                match cursor.next() {
                    Some(Ok(_)) => return Err(Error::PrimaryIndex("primary cursor extra key")),
                    Some(Err(_)) => return Err(Error::PrimaryIndex("primary cursor exhaustion")),
                    None => {}
                }
            }
            return Ok(None);
        };
        let location = self
            .snapshot
            .locations
            .get(self.table_id, key)
            .ok_or(Error::PrimaryIndex("primary cursor missing location"))?;
        if eligible(key) {
            let pointer = match &mut self.projection {
                Projection::Interval(cursor) => {
                    let entry = if backwards {
                        cursor.next_back()
                    } else {
                        cursor.next()
                    };
                    let (indexed, pointer) = entry
                        .transpose()
                        .map_err(|_| Error::PrimaryIndex("primary cursor traversal"))?
                        .ok_or(Error::PrimaryIndex("primary cursor missing key"))?;
                    if indexed != key {
                        return Err(Error::PrimaryIndex("primary cursor key mismatch"));
                    }
                    pointer
                }
                Projection::Points(tree) => tree
                    .get(key)
                    .map_err(|_| Error::PrimaryIndex("primary cursor point lookup"))?
                    .ok_or(Error::PrimaryIndex("primary cursor missing point"))?,
                Projection::Empty => {
                    return Err(Error::PrimaryIndex("primary cursor empty projection"));
                }
            };
            if (pointer.page_id, pointer.slot_id) != (location.page_id, location.slot_id) {
                return Err(Error::PrimaryIndex("primary cursor obsolete pointer"));
            }
        }
        let resolved = self
            .snapshot
            .resolve_row_location(self.name, key, location)?;
        self.emitted += 1;
        Ok(Some(resolved))
    }

    fn item(&mut self, backwards: bool) -> Option<Result<&'a Row>> {
        if self.done {
            return None;
        }
        match self.advance(backwards) {
            Ok(Some(row)) => Some(Ok(row)),
            Ok(None) => {
                self.done = true;
                None
            }
            Err(error) => {
                self.done = true;
                Some(Err(error))
            }
        }
    }
}
impl<'a> Iterator for PrimaryRows<'a> {
    type Item = Result<&'a Row>;
    fn next(&mut self) -> Option<Self::Item> {
        self.item(false)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (
            0,
            Some(if self.done {
                0
            } else {
                self.upper_hint.saturating_sub(self.emitted)
            }),
        )
    }
}
impl DoubleEndedIterator for PrimaryRows<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.item(true)
    }
}
impl std::iter::FusedIterator for PrimaryRows<'_> {}
