use super::*;
use emilybase_object_storage::{MAX_OBJECT_READ_BYTES, ObjectReader};

/// A native payload cursor borrowing the original common root and both owners.
/// No owner, payload image or current account permission can be detached from it.
/// ```compile_fail
/// use emilybase_files::{FileId, NativeFileRoot};
/// fn cannot_release(mut root: NativeFileRoot, id: FileId) {
///     let mut reader = root.reader(id).unwrap();
///     drop(root);
///     reader.read_payload(&mut [0; 8]).unwrap();
/// }
/// ```
/// ```compile_fail
/// use emilybase_files::{FileId, NativeFileRoot};
/// fn cannot_change(root: &mut NativeFileRoot, id: FileId, revision: u64) {
///     let mut reader = root.reader(id).unwrap();
///     root.remove(id, revision).unwrap();
///     reader.read_payload(&mut [0; 8]).unwrap();
/// }
pub struct NativeFileReader<'root> {
    object: ObjectReader<'root>,
    database: &'root Database,
    objects: &'root ProjectDirectory,
    selection: &'root Selection,
    poisoned: &'root mut bool,
    info: FileInfo,
    position: u64,
}
impl NativeFileRoot {
    /// Admit an existing logical reference, verify the entire graph and retain
    /// the original blob for bounded reads. Orphans are never catalog downloads.
    pub fn reader(&mut self, id: FileId) -> Result<NativeFileReader<'_>> {
        self.reader_with(id, || {})
    }
    fn reader_with(&mut self, id: FileId, owned: impl FnOnce()) -> Result<NativeFileReader<'_>> {
        let before = self.before_write()?;
        let info = before
            .files
            .into_iter()
            .find(|f| f.id() == id)
            .ok_or(Error::Missing)?;
        let object = match self.store.objects.reader(info.object()) {
            Ok(object) => object,
            Err(error) => {
                self.store.poisoned = true;
                return Err(error.into());
            }
        };
        let mut reader = NativeFileReader {
            object,
            database: &self.store.database,
            objects: &self.store.objects,
            selection: &self.selection,
            poisoned: &mut self.store.poisoned,
            info,
            position: 0,
        };
        owned();
        reader.verify()?;
        Ok(reader)
    }
}
impl NativeFileReader<'_> {
    /// Copied expected metadata, not authorization or proof of delivered bytes.
    pub const fn info(&self) -> &FileInfo {
        &self.info
    }
    pub const fn payload_position(&self) -> u64 {
        self.position
    }

    fn check(&mut self) -> Result<()> {
        if *self.poisoned {
            return Err(Error::Poisoned);
        }
        self.selection.check()?;
        self.database
            .check_directory_at(&self.selection.root, "metadata")?;
        let current = crate::records::find(
            self.database.view()?,
            self.info.id(),
            self.database.last_transaction(),
        )?;
        if current.as_ref() != Some(&self.info) || self.object.report() != self.info.report() {
            return Err(Error::Corrupt);
        }
        self.object.check_selection()?;
        self.selection.check()
    }
    /// At most8KiB of payload, with common selection/catalog checks around the
    /// original bounded object read. EOF and empty buffers still validate source.
    /// Failure clears the attempted prefix, leaves this cursor unchanged and
    /// poisons the reader and root. Prior successful chunks cannot be revoked.
    pub fn read_payload(&mut self, destination: &mut [u8]) -> Result<usize> {
        self.read_payload_with(destination, || {})
    }
    fn read_payload_with(
        &mut self,
        destination: &mut [u8],
        copied: impl FnOnce(),
    ) -> Result<usize> {
        let count = destination.len().min(MAX_OBJECT_READ_BYTES).min(
            (self.info.report().payload_bytes as u64)
                .saturating_sub(self.position)
                .min(MAX_OBJECT_READ_BYTES as u64) as usize,
        );
        let result = (|| {
            self.check()?;
            let read = self.object.read_payload(&mut destination[..count])?;
            copied();
            self.check()?;
            Ok(read)
        })();
        match result {
            Ok(read) => {
                self.position += read as u64;
                Ok(read)
            }
            Err(error) => {
                destination[..count].fill(0);
                *self.poisoned = true;
                Err(error)
            }
        }
    }
    /// Payload-relative seek, allowing positions beyond EOF. Invalid arithmetic
    /// preserves a healthy reader; detected source failures poison the whole root.
    pub fn seek_payload(&mut self, from: SeekFrom) -> Result<u64> {
        self.seek_payload_with(from, || {})
    }
    fn seek_payload_with(&mut self, from: SeekFrom, moved: impl FnOnce()) -> Result<u64> {
        if *self.poisoned {
            return Err(Error::Poisoned);
        }
        let next = match from {
            SeekFrom::Start(position) => Some(position),
            SeekFrom::Current(delta) => self.position.checked_add_signed(delta),
            SeekFrom::End(delta) => {
                (self.info.report().payload_bytes as u64).checked_add_signed(delta)
            }
        }
        .ok_or(emilybase_object_storage::Error::InvalidSeek)?;
        let result = (|| {
            self.check()?;
            if self.object.seek_payload(from)? != next {
                return Err(Error::Corrupt);
            }
            moved();
            self.check()?;
            Ok(next)
        })();
        match result {
            Ok(position) => {
                self.position = position;
                Ok(position)
            }
            Err(error) => {
                *self.poisoned = true;
                Err(error)
            }
        }
    }
    /// Rehash the complete retained blob, validate the whole current physical
    /// graph and original selection, preserving the logical payload cursor.
    pub fn verify(&mut self) -> Result<()> {
        self.verify_with(|| {})
    }
    fn verify_with(&mut self, verified: impl FnOnce()) -> Result<()> {
        let result = (|| {
            self.check()?;
            self.object.verify()?;
            verified();
            let (quota, files) = crate::records::metadata(self.database, self.objects.project())?;
            let inventory = self.objects.inventory()?;
            crate::records::graph(quota, &files, &inventory)?;
            self.check()
        })();
        if result.is_err() {
            *self.poisoned = true;
        }
        result
    }
    /// Final complete verification, not proof of EOF or a delivered response.
    /// Dropping early releases the borrow without silently claiming verification.
    pub fn finish(mut self) -> Result<FileInfo> {
        self.verify()?;
        Ok(self.info)
    }
}
impl std::fmt::Debug for NativeFileReader<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeFileReader")
            .field("bytes", &self.info.report().payload_bytes)
            .field("revision", &self.info.revision())
            .field("position", &self.position)
            .field("poisoned", self.poisoned)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests;
