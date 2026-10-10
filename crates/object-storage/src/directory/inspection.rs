use super::{ProjectDirectory, check_visible, object_name, report_at};
use crate::{FileReport, HEADER_BYTES, MAX_PAYLOAD_BYTES, ObjectId, Result};

impl ProjectDirectory {
    /// Fully verify one object's metadata through bounded payload scratch.
    /// Native operator authority only; this is not a complete inventory or lease.
    /// No payload image is returned or retained by this operation.
    pub fn inspect(&self, object: ObjectId) -> Result<FileReport> {
        self.inspect_with(object, || {})
    }

    fn inspect_with(&self, object: ObjectId, verified: impl FnOnce()) -> Result<FileReport> {
        self.check()?;
        let name = object_name(object);
        let (file, report, before) = report_at(&self.directory, &name, self.project, object)?;
        verified();
        // Keep the actual checked inode across the final project-marker check.
        // Reopening a name here could accept an identical replacement file.
        self.check()?;
        let after = crate::inspect::recheck(&file, &before, HEADER_BYTES + MAX_PAYLOAD_BYTES)?;
        check_visible(&self.directory, &name, &after)?;
        Ok(report)
    }
}

#[cfg(test)]
mod tests;
