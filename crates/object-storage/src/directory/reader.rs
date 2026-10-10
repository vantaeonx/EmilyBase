//! Bounded payload reads through one verified descriptor and its native owner.
use super::*;
use std::os::unix::fs::FileExt;

/// Maximum payload bytes copied by one read, independently of caller buffer size.
pub const MAX_OBJECT_READ_BYTES: usize = 8192;

/// A synchronous payload cursor over an admitted immutable native object.
/// The actual file and original directory owner remain retained. Admission and
/// explicit verify hash the complete envelope; each read rechecks the original
/// scope, private metadata and visible identity before and after copying bytes.
/// This is not current user authority or a sandbox against a hostile operator.
///
/// ```compile_fail
/// use emilybase_object_storage::{ObjectId, ProjectDirectory};
/// fn cannot_release(owner: ProjectDirectory, object: ObjectId) {
///     let mut reader = owner.reader(object).unwrap();
///     drop(owner);
///     reader.read_payload(&mut [0; 8]).unwrap();
/// }
/// ```
pub struct ObjectReader<'owner> {
    owner: &'owner ProjectDirectory,
    file: File,
    object: ObjectId,
    report: FileReport,
    baseline: Metadata,
    position: u64,
    poisoned: bool,
}

impl ProjectDirectory {
    /// Fully verify an existing object without retaining a payload image, then
    /// keep its actual descriptor under this owner for bounded payload reads.
    /// Never adopts a replacement inode or initializes a missing object/scope.
    pub fn reader(&self, object: ObjectId) -> Result<ObjectReader<'_>> {
        self.reader_with(object, || {})
    }

    fn reader_with(&self, object: ObjectId, verified: impl FnOnce()) -> Result<ObjectReader<'_>> {
        self.check()?;
        let (file, report, baseline) =
            report_at(&self.directory, &object_name(object), self.project, object)?;
        verified();
        let reader = ObjectReader {
            owner: self,
            file,
            object,
            report,
            baseline,
            position: 0,
            poisoned: false,
        };
        reader.check_current()?;
        Ok(reader)
    }
}

impl ObjectReader<'_> {
    pub const fn project(&self) -> ProjectId {
        self.owner.project
    }
    pub const fn object(&self) -> ObjectId {
        self.object
    }
    /// Expected metadata from full admission, not a current authorization lease.
    pub const fn report(&self) -> &FileReport {
        &self.report
    }
    pub const fn payload_position(&self) -> u64 {
        self.position
    }

    /// Read at most MAX_OBJECT_READ_BYTES, never including the native header.
    /// Empty reads and EOF still validate ownership. On any failed operation the
    /// attempted destination prefix is cleared, the logical cursor is unchanged
    /// and this reader refuses subsequent reads, seeks and verification. Caller
    /// bytes after that prefix are unchanged. Earlier successful reads cannot be
    /// revoked; arbitrary concurrent same-user filesystem writes are not isolated.
    pub fn read_payload(&mut self, destination: &mut [u8]) -> Result<usize> {
        self.read_payload_with(destination, || {})
    }

    fn read_payload_with(
        &mut self,
        destination: &mut [u8],
        copied: impl FnOnce(),
    ) -> Result<usize> {
        let count = destination.len().min(MAX_OBJECT_READ_BYTES).min(
            (self.report.payload_bytes as u64)
                .saturating_sub(self.position)
                .min(MAX_OBJECT_READ_BYTES as u64) as usize,
        );
        let result = self.copy_checked(&mut destination[..count], copied);
        match result {
            Ok(read) => {
                self.position += read as u64;
                Ok(read)
            }
            Err(error) => {
                destination[..count].fill(0);
                self.poisoned = true;
                Err(error)
            }
        }
    }

    fn copy_checked(&self, destination: &mut [u8], copied: impl FnOnce()) -> Result<usize> {
        self.check_current()?;
        let read = if destination.is_empty() {
            0
        } else {
            // Position is below bounded payload EOF whenever count is nonzero.
            let offset = HEADER_BYTES as u64 + self.position;
            loop {
                match self.file.read_at(destination, offset) {
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    other => break other?,
                }
            }
        };
        copied();
        if read == 0 && !destination.is_empty() {
            return Err(Error::File);
        }
        self.check_current()?;
        Ok(read)
    }

    /// Payload-relative seek. Positions beyond EOF are allowed; negative or
    /// overflowing positions return InvalidSeek without changing the cursor or
    /// poisoning a healthy reader. No filesystem seek or header access occurs.
    pub fn seek_payload(&mut self, from: SeekFrom) -> Result<u64> {
        if self.poisoned {
            return Err(Error::ReaderPoisoned);
        }
        let next = match from {
            SeekFrom::Start(position) => Some(position),
            SeekFrom::Current(delta) => self.position.checked_add_signed(delta),
            SeekFrom::End(delta) => (self.report.payload_bytes as u64).checked_add_signed(delta),
        }
        .ok_or(Error::InvalidSeek)?;
        if let Err(error) = self.check_current() {
            self.poisoned = true;
            return Err(error);
        }
        self.position = next;
        Ok(next)
    }

    /// Hash the complete retained envelope again and check the original report,
    /// scope and visible inode. The logical payload cursor is preserved.
    pub fn verify(&mut self) -> Result<()> {
        self.verify_with(|| {})
    }
    fn verify_with(&mut self, verified: impl FnOnce()) -> Result<()> {
        let result = self.verify_inner(verified);
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }
    fn verify_inner(&mut self, verified: impl FnOnce()) -> Result<()> {
        self.check_current()?;
        let (report, _) =
            crate::inspect::read_open_report(&mut self.file, self.owner.project, self.object)?;
        verified();
        if report != self.report {
            return Err(Error::File);
        }
        self.check_current()
    }

    /// Consume the reader after one final complete hash/owner check. This is a
    /// native file result, not proof of a completed network response or policy.
    pub fn finish(mut self) -> Result<FileReport> {
        self.verify()?;
        Ok(self.report)
    }

    fn check_current(&self) -> Result<()> {
        if self.poisoned {
            return Err(Error::ReaderPoisoned);
        }
        self.owner.check()?;
        let maximum = HEADER_BYTES + MAX_PAYLOAD_BYTES;
        let after = crate::inspect::recheck(&self.file, &self.baseline, maximum)?;
        let name = object_name(self.object);
        check_visible(&self.owner.directory, &name, &after)?;
        self.owner.check()?;
        let after = crate::inspect::recheck(&self.file, &self.baseline, maximum)?;
        check_visible(&self.owner.directory, &name, &after)
    }
}

impl std::fmt::Debug for ObjectReader<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ObjectReader")
            .field("bytes", &self.report.payload_bytes)
            .field("position", &self.position)
            .field("poisoned", &self.poisoned)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests;
