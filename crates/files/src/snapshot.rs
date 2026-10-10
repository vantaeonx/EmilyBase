use crate::{Error, FileInfo, FileQuota, FileStore, Result, records};
use emilybase_object_storage::{ObjectSnapshot, ProjectId};

/// Immutable bounded native images captured together under the original metadata
/// and object owners. Sensitive bytes, not publication, a reservation or user
/// authority. The metadata history can occupy64 MiB and object payloads64 MiB;
/// decoded snapshots, scratch space and other allocations are additional costs.
pub struct FileSnapshot {
    project: ProjectId,
    quota: FileQuota,
    files: Vec<FileInfo>,
    metadata: Vec<u8>,
    metadata_report: emilybase_backup::Report,
    objects: ObjectSnapshot,
}
impl FileSnapshot {
    pub const fn project(&self) -> ProjectId {
        self.project
    }
    pub const fn quota(&self) -> FileQuota {
        self.quota
    }
    pub fn files(&self) -> &[FileInfo] {
        &self.files
    }
    /// The existing original-engine backup format, including its verified header.
    pub fn metadata_bytes(&self) -> &[u8] {
        &self.metadata
    }
    pub const fn metadata_report(&self) -> &emilybase_backup::Report {
        &self.metadata_report
    }
    /// Includes every valid physical orphan, preserving charged physical usage.
    pub const fn objects(&self) -> &ObjectSnapshot {
        &self.objects
    }
}
impl std::fmt::Debug for FileSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileSnapshot")
            .field("references", &self.files.len())
            .field("physical_objects", &self.objects.objects().len())
            .field("payload_bytes", &self.objects.inventory().payload_bytes())
            .field("metadata_bytes", &self.metadata.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum CaptureBoundary {
    Admitted,
    Metadata,
    Objects,
}

impl FileStore {
    /// Capture one checked pair without writing either source. The mutable borrow
    /// excludes native FileStore writes throughout capture. Actual descriptors for
    /// all physical objects remain retained across metadata export and object copy;
    /// exact source inventory, inodes and committed WAL are checked before return.
    /// Failure after initial graph validation requires reopen; nothing is repaired.
    /// This does not atomically publish or restore a common backup destination.
    pub fn capture(&mut self) -> Result<FileSnapshot> {
        self.capture_with(|_| {})
    }

    pub(crate) fn capture_with(
        &mut self,
        mut boundary: impl FnMut(CaptureBoundary),
    ) -> Result<FileSnapshot> {
        let (quota, files, inventory) = self.validated()?;
        let project = self.objects.project();
        let database_id = self.database.database_id();
        let last_transaction = self.database.last_transaction();
        let result = (|| {
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
            boundary(CaptureBoundary::Admitted);
            let wal = self.database.committed_wal()?;
            let metadata = emilybase_backup::encode(&wal)?;
            let verified = emilybase_backup::decode_verified(&metadata)?;
            let image = verified.image();
            if image.database_id != database_id || image.last_transaction != last_transaction {
                return Err(Error::Corrupt);
            }
            let (copied_quota, copied_files) = records::metadata_image(
                &image.snapshot,
                image.database_id,
                image.last_transaction,
                project,
            )?;
            if copied_quota != quota || copied_files != files {
                return Err(Error::Corrupt);
            }
            let metadata_report = verified.report().clone();
            drop(verified);
            boundary(CaptureBoundary::Metadata);
            let objects = self.objects.capture_inventory(&inventory)?;
            boundary(CaptureBoundary::Objects);
            for reader in &mut readers {
                reader.verify()?;
            }
            if self.objects.inventory()? != inventory
                || objects.inventory() != &inventory
                || self.database.committed_wal()? != wal
            {
                return Err(Error::Corrupt);
            }
            records::graph(quota, &files, objects.inventory())?;
            self.database.view()?;
            Ok(FileSnapshot {
                project,
                quota,
                files,
                metadata,
                metadata_report,
                objects,
            })
        })();
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }
}

#[cfg(test)]
mod tests;
