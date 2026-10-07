//! Immutable logical row over one source or two already checked JOIN sources.
use crate::execute::{ExecutionError, RunResult, value_bytes};
use crate::plan::SortKey;
use emilybase_catalog::{Row, Value};
use std::cmp::Ordering;
#[derive(Clone, Copy)]
pub(crate) struct RowView<'a> {
    left: &'a [Value],
    right: &'a [Value],
}
impl<'a> RowView<'a> {
    pub(crate) fn single(row: &'a Row) -> Self {
        Self {
            left: row,
            right: &[],
        }
    }
    pub(crate) fn joined(left: &'a Row, right: &'a Row) -> Self {
        Self { left, right }
    }
    pub(crate) fn get(self, index: usize) -> Option<&'a Value> {
        if index < self.left.len() {
            self.left.get(index)
        } else {
            self.right.get(index - self.left.len())
        }
    }
    pub(crate) fn bytes(self) -> RunResult<usize> {
        self.left
            .iter()
            .chain(self.right)
            .try_fold(24usize, |bytes, value| {
                bytes
                    .checked_add(value_bytes(value))
                    .ok_or(ExecutionError::Limit("intermediate rows/bytes"))
            })
    }
    pub(crate) fn validate_order(self, order: &[SortKey]) -> RunResult<()> {
        for key in order {
            self.get(key.index).ok_or(ExecutionError::Plan)?;
        }
        Ok(())
    }
    pub(crate) fn compare(self, row: &Row, order: &[SortKey]) -> RunResult<Ordering> {
        for key in order {
            let a = self.get(key.index).ok_or(ExecutionError::Plan)?;
            let b = row.get(key.index).ok_or(ExecutionError::Plan)?;
            let compared = crate::select::compare_values(a, b, key);
            if !compared.is_eq() {
                return Ok(compared);
            }
        }
        Ok(Ordering::Equal)
    }
    pub(crate) fn to_owned(self) -> Row {
        let mut row = Vec::with_capacity(self.left.len() + self.right.len());
        row.extend(self.left.iter().chain(self.right).cloned());
        row
    }
}
#[cfg(test)]
#[path = "row_view_tests.rs"]
mod tests;
