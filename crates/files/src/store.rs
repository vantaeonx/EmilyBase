use crate::{Error, FileId, FileInfo, FileQuota, Result, records};
use emilybase_object_storage::{Inventory, ObjectId, ObjectReader, ProjectDirectory, ProjectId};
use emilybase_transactions::Database;

/// Counts verified under both retained native owners, not a later reservation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileUsage {
    pub physical_objects: usize,
    pub payload_bytes: u64,
    pub references: usize,
    pub orphans: usize,
}

/// A separate native metadata/object pair. Trusted callers acquire the original
/// metadata Database first, then ProjectDirectory, before passing ownership here.
/// No underlying owner/SQL handle is exported. This grants no user/file policy.
pub struct FileStore {
    database: Database,
    objects: ProjectDirectory,
    poisoned: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PublishBoundary {
    Blob,
    Metadata,
    Verified,
}

impl FileStore {
    /// Initialize only pristine metadata and an empty already-initialized object
    /// owner. The schema/quota/project/database binding commits in one own WAL.
    /// No directory is created/repaired here. A late failure requires inspection.
    pub fn initialize(
        mut database: Database,
        objects: ProjectDirectory,
        quota: FileQuota,
    ) -> Result<Self> {
        let inventory = objects.inventory()?;
        let pristine = database.view()?;
        // The original database marker is itself the first history event.
        if pristine.event_count() != 1
            || pristine.table_count() != 0
            || pristine.row_count() != 0
            || pristine.next_table_id() != 1
            || !inventory.entries().is_empty()
        {
            return Err(Error::NotEmpty);
        }
        let project = objects.project();
        let identity = database.database_id();
        let mut tx = database.begin()?;
        tx.create_table(records::scope_schema())?;
        tx.create_table(records::file_schema())?;
        tx.insert(records::SCOPE, records::scope_row(project, identity, quota))?;
        tx.commit()
            .map_err(|e| Error::OutcomeUnknown(Box::new(e.into())))?;
        let result = Self {
            database,
            objects,
            poisoned: false,
        };
        result
            .validated()
            .map_err(|e| Error::OutcomeUnknown(Box::new(e)))?;
        Ok(result)
    }

    /// Strict original-WAL recovery must already have opened metadata; opening
    /// here validates the exact schema, persisted scope/quota and complete graph.
    /// Valid unreferenced native blobs remain invisible and consume physical quota.
    pub fn open(database: Database, objects: ProjectDirectory) -> Result<Self> {
        let result = Self {
            database,
            objects,
            poisoned: false,
        };
        result.validated()?;
        Ok(result)
    }
    pub fn project(&self) -> ProjectId {
        self.objects.project()
    }
    pub fn quota(&self) -> Result<FileQuota> {
        Ok(self.validated()?.0)
    }
    pub fn list(&self) -> Result<Vec<FileInfo>> {
        Ok(self.validated()?.1)
    }
    pub fn info(&self, id: FileId) -> Result<Option<FileInfo>> {
        let (_, files, _) = self.validated()?;
        Ok(files.into_iter().find(|info| info.id == id))
    }
    pub fn usage(&self) -> Result<FileUsage> {
        let (_, files, inventory) = self.validated()?;
        Ok(FileUsage {
            physical_objects: inventory.entries().len(),
            payload_bytes: inventory.payload_bytes(),
            references: files.len(),
            orphans: inventory.entries().len() - files.len(),
        })
    }

    /// The caller has native authority over this pair. Copied owner metadata is
    /// not checked against accounts. Only a committed logical ID selects a reader;
    /// passing the ID of an unreferenced physical blob grants nothing here.
    /// ```compile_fail
    /// use emilybase_files::{FileId, FileStore};
    /// fn cannot_release(store: FileStore, id: FileId) {
    ///     let mut reader = store.reader(id).unwrap();
    ///     drop(store);
    ///     reader.read_payload(&mut [0; 8]).unwrap();
    /// }
    /// ```
    pub fn reader(&self, id: FileId) -> Result<ObjectReader<'_>> {
        self.validated()?;
        let info = records::find(self.database.view()?, id, self.database.last_transaction())?
            .ok_or(Error::Missing)?;
        let mut reader = self.objects.reader(info.object)?;
        if reader.report() != &info.report {
            return Err(Error::Corrupt);
        }
        self.database.view()?;
        reader.verify()?;
        Ok(reader)
    }

    /// Persist a fresh immutable blob first, then its logical reference through
    /// the original WAL. Retain the actual selected blob under its owner across
    /// the commit and final complete receipt checks. Never overwrite/retry/delete.
    pub fn publish(
        &mut self,
        id: FileId,
        object: ObjectId,
        owner: [u8; 16],
        name: &str,
        payload: &[u8],
    ) -> Result<FileInfo> {
        self.publish_with(id, object, owner, name, payload, |_| {})
    }
    pub(crate) fn publish_with(
        &mut self,
        id: FileId,
        object: ObjectId,
        owner: [u8; 16],
        name: &str,
        payload: &[u8],
        mut boundary: impl FnMut(PublishBoundary),
    ) -> Result<FileInfo> {
        records::validate_name(name)?;
        if payload.len() > emilybase_object_storage::MAX_PAYLOAD_BYTES {
            return Err(emilybase_object_storage::Error::Limit.into());
        }
        let (quota, infos, _) = self.validated()?;
        if infos.iter().any(|info| info.id == id) {
            return Err(Error::Exists);
        }
        let revision = self
            .database
            .last_transaction()
            .checked_add(1)
            .ok_or(Error::Revision)?;
        let project = self.objects.project();
        let mut selected = match self
            .objects
            .put_bounded_selected(object, payload, quota.limits()?)
        {
            Ok(selected) => selected,
            Err(error) => {
                if matches!(error, emilybase_object_storage::Error::PublicationUnknown) {
                    self.poisoned = true;
                }
                return Err(error.into());
            }
        };
        let expected = FileInfo {
            id,
            object,
            owner,
            name: name.into(),
            report: selected.report().clone(),
            revision,
        };
        let result = (|| {
            boundary(PublishBoundary::Blob);
            selected.verify_complete()?;
            let mut tx = self.database.begin()?;
            tx.insert(records::FILES, records::encode(&expected))?;
            if tx.commit()? != revision {
                return Err(Error::Corrupt);
            }
            boundary(PublishBoundary::Metadata);
            let (current_quota, current) = records::metadata(&self.database, project)?;
            if current_quota != quota || !current.contains(&expected) {
                return Err(Error::Corrupt);
            }
            records::graph(current_quota, &current, selected.inventory())?;
            selected.verify_complete()?;
            boundary(PublishBoundary::Verified);
            Ok(expected)
        })();
        if result.is_err() {
            self.poisoned = true;
        }
        result.map_err(|e| Error::OutcomeUnknown(Box::new(e)))
    }

    fn validated(&self) -> Result<(FileQuota, Vec<FileInfo>, Inventory)> {
        if self.poisoned {
            return Err(Error::Poisoned);
        }
        let (quota, files) = records::metadata(&self.database, self.objects.project())?;
        let inventory = self.objects.inventory()?;
        records::graph(quota, &files, &inventory)?;
        self.database.view()?;
        Ok((quota, files, inventory))
    }
}

#[cfg(all(test, target_os = "linux"))]
#[path = "crash_tests.rs"]
mod crash_tests;
#[cfg(test)]
#[path = "tests.rs"]
mod tests;
