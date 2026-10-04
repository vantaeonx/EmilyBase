use std::fs::File;

use emilybase_wal::Wal;

use crate::{Database, Error, Result, recover_image};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compaction {
    pub previous_wal_bytes: u64,
    pub compacted_wal_bytes: u64,
    pub transaction: u64,
    pub pages: usize,
}

impl Database {
    /// Explicitly replace repeated images with a self-contained version-2
    /// baseline. This retains all relational events and the transaction boundary.
    pub fn compact(&mut self) -> Result<Compaction> {
        self.compact_with(|| {}, || {}, File::sync_all)
    }

    pub(crate) fn compact_with(
        &mut self,
        synced: impl FnOnce(),
        published: impl FnOnce(),
        sync_directory: impl FnOnce(&File) -> std::io::Result<()>,
    ) -> Result<Compaction> {
        self.committed_wal()?;
        self.check_selected_journal()?;
        let report = Compaction {
            previous_wal_bytes: self.wal.valid_bytes(),
            compacted_wal_bytes: 0,
            transaction: self.last_transaction(),
            pages: self.snapshot.page_count(),
        };
        let mut pending = crate::journal_replacement::Pending::new(&self.ownership)?;
        let pages = self.snapshot.pages().cloned().collect::<Vec<_>>();
        let mut replacement = Wal::create_snapshot_from_file(
            pending.file.try_clone()?,
            self.database_id(),
            report.transaction,
            &pages,
        )?;
        synced();
        pending.check()?;
        self.check_selected_journal()?;
        let bytes = replacement.committed_bytes()?;
        let recovered = recover_image(&bytes, Some(self.database_id()))?;
        if recovered.last_transaction != report.transaction
            || !recovered.snapshot.pages().eq(self.snapshot.pages())
        {
            return Err(Error::History(
                "replacement baseline differs from committed state",
            ));
        }
        pending.publish()?;
        published();
        if let Err(error) = sync_directory(&self.ownership) {
            self.poisoned = true;
            return Err(Error::MaintenanceUnknown(error));
        }
        let selection = pending.selected_image(&bytes);
        if !matches!(selection, Ok(true)) {
            self.poisoned = true;
            return Err(Error::MaintenanceUnknown(selection.err().unwrap_or_else(
                || {
                    std::io::Error::other(
                        "selected replacement journal identity or contents changed",
                    )
                },
            )));
        }
        let report = Compaction {
            compacted_wal_bytes: replacement.valid_bytes(),
            ..report
        };
        self.wal = replacement;
        Ok(report)
    }

    fn check_selected_journal(&mut self) -> Result<()> {
        let result = crate::journal_replacement::source_selected(&self.ownership, &self.wal);
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }
}

#[cfg(test)]
#[path = "compaction_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "compaction_ownership_tests.rs"]
mod ownership_tests;
