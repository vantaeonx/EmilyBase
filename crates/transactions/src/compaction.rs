use std::fs::{self, File};
use std::path::PathBuf;

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
        let report = Compaction {
            previous_wal_bytes: self.wal.valid_bytes(),
            compacted_wal_bytes: 0,
            transaction: self.last_transaction(),
            pages: self.snapshot.page_count(),
        };
        let temporary = self.path.join("redo-next.wal");
        match fs::remove_file(&temporary) {
            Ok(()) => (),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
        let pending = PendingPath(temporary);
        let pages = self.snapshot.pages().cloned().collect::<Vec<_>>();
        let mut replacement =
            Wal::create_snapshot(&pending.0, self.database_id(), report.transaction, &pages)?;
        let bytes = replacement.committed_bytes()?;
        let recovered = recover_image(&bytes, Some(self.database_id()))?;
        if recovered.last_transaction != report.transaction
            || !recovered.snapshot.pages().eq(self.snapshot.pages())
        {
            return Err(Error::History(
                "replacement baseline differs from committed state",
            ));
        }
        synced();
        fs::rename(&pending.0, self.path.join("redo.wal"))?;
        published();
        if let Err(error) = sync_directory(&self.ownership) {
            self.poisoned = true;
            return Err(Error::MaintenanceUnknown(error));
        }
        let report = Compaction {
            compacted_wal_bytes: replacement.valid_bytes(),
            ..report
        };
        self.wal = replacement;
        Ok(report)
    }
}

#[cfg(test)]
#[path = "compaction_tests.rs"]
mod tests;

struct PendingPath(PathBuf);
impl Drop for PendingPath {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
