//! Stable bounded selection for fully evaluated, non-streamed primary joins.
use crate::MAX_RESULT_ROWS;
use crate::execute::{ExecutionError, MAX_OUTPUT_BYTES, RunResult, row_bytes};
use crate::plan::SortKey;
use emilybase_catalog::Row;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

struct Ranked<'a> {
    row: Row,
    ordinal: usize,
    order: &'a [SortKey],
}

impl Ord for Ranked<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        // Private entries in one heap always borrow the same resolved order.
        // Later input loses equal-key ties, exactly like the original stable sort.
        crate::select::compare_rows(&self.row, &other.row, self.order)
            .then_with(|| self.ordinal.cmp(&other.ordinal))
    }
}
impl PartialOrd for Ranked<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl PartialEq for Ranked<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}
impl Eq for Ranked<'_> {}

/// The heap owns at most LIMIT full candidates, with the worst one at its root.
/// Retained bytes remain bounded; every accepted match still consumes the original
/// intermediate row-count allowance, even when it is subsequently discarded.
pub(crate) struct TopK<'a> {
    heap: BinaryHeap<Ranked<'a>>,
    order: &'a [SortKey],
    limit: usize,
    seen: usize,
    bytes: usize,
}

impl<'a> TopK<'a> {
    pub(crate) fn new(order: &'a [SortKey], limit: usize) -> Self {
        Self {
            heap: BinaryHeap::new(),
            order,
            limit,
            seen: 0,
            bytes: 0,
        }
    }

    pub(crate) fn push(&mut self, row: Row) -> RunResult<()> {
        if self.seen >= MAX_RESULT_ROWS {
            return Err(ExecutionError::Limit("intermediate rows/bytes"));
        }
        let entry = Ranked {
            row,
            ordinal: self.seen,
            order: self.order,
        };
        self.seen += 1;
        if self.limit == 0 {
            return Ok(());
        }
        if self.heap.len() < self.limit {
            let bytes = self.bytes + row_bytes(&entry.row);
            if bytes > MAX_OUTPUT_BYTES {
                return Err(ExecutionError::Limit("intermediate rows/bytes"));
            }
            self.heap.push(entry);
            self.bytes = bytes;
        } else if let Some(mut worst) = self.heap.peek_mut()
            && entry < *worst
        {
            let bytes = self.bytes - row_bytes(&worst.row) + row_bytes(&entry.row);
            if bytes > MAX_OUTPUT_BYTES {
                return Err(ExecutionError::Limit("intermediate rows/bytes"));
            }
            *worst = entry;
            self.bytes = bytes;
        }
        Ok(())
    }

    pub(crate) fn into_rows(self) -> Vec<Row> {
        self.heap
            .into_sorted_vec()
            .into_iter()
            .map(|entry| entry.row)
            .collect()
    }
}

#[cfg(test)]
#[path = "topk_tests.rs"]
mod tests;
