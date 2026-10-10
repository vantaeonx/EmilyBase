use crate::mutation::MetadataBoundary;
use crate::{Error, FileQuota, FileStore, Result, records};
use emilybase_catalog::Key;
use emilybase_object_storage::ProjectId;

/// A native metadata snapshot, never user authority. Its revision is the global
/// metadata transaction: any later reference/quota commit makes it stale.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct QuotaState {
    project: ProjectId,
    database: [u8; 16],
    quota: FileQuota,
    revision: u64,
}
impl QuotaState {
    pub const fn project(self) -> ProjectId {
        self.project
    }
    pub const fn database_id(&self) -> &[u8; 16] {
        &self.database
    }
    pub const fn quota(self) -> FileQuota {
        self.quota
    }
    pub const fn revision(self) -> u64 {
        self.revision
    }
}
impl std::fmt::Debug for QuotaState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QuotaState")
            .field("quota", &self.quota)
            .field("revision", &self.revision)
            .finish_non_exhaustive()
    }
}

impl FileStore {
    /// Inspect persisted operator limits after complete graph validation. This
    /// copied state is a scoped CAS input, not a reservation or authorization.
    pub fn quota_state(&self) -> Result<QuotaState> {
        let (quota, _, _) = self.validated()?;
        Ok(QuotaState {
            project: self.objects.project(),
            database: self.database.database_id(),
            quota,
            revision: self.database.last_transaction(),
        })
    }

    /// Change persisted native operator limits with exact database/project/global
    /// revision CAS. Existing physical blobs, including orphans, must fit. Retain
    /// at most128 actual readonly descriptors through source checks and commit.
    pub fn set_quota(&mut self, expected: QuotaState, quota: FileQuota) -> Result<QuotaState> {
        self.set_quota_with(expected, quota, |_| {})
    }

    pub(crate) fn set_quota_with(
        &mut self,
        expected: QuotaState,
        quota: FileQuota,
        mut boundary: impl FnMut(MetadataBoundary),
    ) -> Result<QuotaState> {
        let (current_quota, current_files, inventory) = self.validated()?;
        let project = self.objects.project();
        let database = self.database.database_id();
        let last = self.database.last_transaction();
        if expected.project != project || expected.database != database {
            return Err(Error::Scope);
        }
        if expected.revision != last || expected.quota != current_quota {
            return Err(Error::Conflict);
        }
        records::graph(quota, &current_files, &inventory)?;
        if quota == current_quota {
            self.database.view()?;
            return Ok(QuotaState {
                project,
                database,
                quota,
                revision: last,
            });
        }
        let revision = last.checked_add(1).ok_or(Error::Revision)?;
        // Full inventory is already capped; failure leaves both resources untouched.
        let mut readers = Vec::new();
        readers
            .try_reserve_exact(inventory.entries().len())
            .map_err(|_| Error::Allocation)?;
        for entry in inventory.entries() {
            let reader = self.objects.reader(entry.object())?;
            if reader.report() != entry.report() {
                return Err(Error::Corrupt);
            }
            readers.push(reader);
        }
        let mut commit_attempted = false;
        let result = (|| {
            let mut tx = self.database.begin()?;
            tx.update(
                records::SCOPE,
                &Key::Integer(1),
                records::scope_row(project, database, quota),
            )?;
            boundary(MetadataBoundary::Staged);
            for reader in &mut readers {
                reader.verify()?;
            }
            if self.objects.inventory()? != inventory {
                return Err(Error::Corrupt);
            }
            tx.view()?;
            commit_attempted = true;
            if tx.commit()? != revision {
                return Err(Error::Corrupt);
            }
            boundary(MetadataBoundary::Committed);
            let (selected_quota, selected_files) = records::metadata(&self.database, project)?;
            if selected_quota != quota || selected_files != current_files {
                return Err(Error::Corrupt);
            }
            for reader in &mut readers {
                reader.verify()?;
            }
            let after = self.objects.inventory()?;
            if after != inventory {
                return Err(Error::Corrupt);
            }
            records::graph(quota, &selected_files, &after)?;
            self.database.view()?;
            boundary(MetadataBoundary::Verified);
            Ok(QuotaState {
                project,
                database,
                quota,
                revision,
            })
        })();
        if result.is_err() {
            self.poisoned = true;
        }
        if commit_attempted {
            result.map_err(|e| Error::OutcomeUnknown(Box::new(e)))
        } else {
            result
        }
    }
}

#[cfg(test)]
mod tests;
