//! Explicit per-call native capacity admission; no persisted/service quota.
use super::*;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteLimits {
    objects: usize,
    payload_bytes: u64,
}
impl WriteLimits {
    pub fn new(objects: usize, payload_bytes: u64) -> Result<Self> {
        if objects > MAX_INVENTORY_OBJECTS || payload_bytes > MAX_INVENTORY_BYTES {
            return Err(Error::WriteLimits);
        }
        Ok(Self {
            objects,
            payload_bytes,
        })
    }
    pub const fn objects(self) -> usize {
        self.objects
    }
    pub const fn payload_bytes(self) -> u64 {
        self.payload_bytes
    }
    /// Metadata-only capacity check. A matching receipt is not user authority
    /// or a reservation; execution must recheck the complete owned directory.
    pub fn check(self, before: &Inventory, object: ObjectId, bytes: usize) -> Result<()> {
        if bytes > MAX_PAYLOAD_BYTES {
            return Err(Error::Limit);
        }
        if before
            .entries()
            .iter()
            .any(|entry| entry.object() == object)
        {
            return Err(Error::Exists);
        }
        if before.entries().len() >= self.objects
            || before
                .payload_bytes()
                .checked_add(bytes as u64)
                .is_none_or(|total| total > self.payload_bytes)
        {
            return Err(Error::Limit);
        }
        Ok(())
    }
}
#[derive(Debug)]
pub struct WriteReceipt {
    object: ObjectId,
    report: FileReport,
    inventory: Inventory,
}
impl WriteReceipt {
    pub const fn object(&self) -> ObjectId {
        self.object
    }
    pub const fn report(&self) -> &FileReport {
        &self.report
    }
    pub const fn inventory(&self) -> &Inventory {
        &self.inventory
    }
}

impl ProjectDirectory {
    /// Admit under explicit per-call limits before encoding or creating a stage.
    /// The complete directory must already be valid. The cooperating owner lock
    /// spans all checks/publication. Existing unbounded native put stays distinct.
    pub fn put_bounded(
        &mut self,
        object: ObjectId,
        payload: &[u8],
        limits: WriteLimits,
    ) -> Result<WriteReceipt> {
        self.put_bounded_with(object, payload, limits, || {}, || {})
    }
    fn put_bounded_with(
        &mut self,
        object: ObjectId,
        payload: &[u8],
        limits: WriteLimits,
        prepared: impl FnOnce(),
        selected: impl FnOnce(),
    ) -> Result<WriteReceipt> {
        if payload.len() > MAX_PAYLOAD_BYTES {
            return Err(Error::Limit);
        }
        let before = self.inventory()?;
        limits.check(&before, object, payload.len())?;
        let expected_report = FileReport {
            payload_bytes: payload.len(),
            sha256: Sha256::digest(payload).into(),
        };
        let expected = before.with_insert(object, expected_report.clone())?;
        prepared();
        if self.inventory()? != before {
            return Err(Error::InventoryChanged);
        }
        // Retain the actual selected inode across the wider receipt boundary;
        // a report alone cannot distinguish an identical replacement file.
        let (selected_file, report) = self.put_retained_with(object, payload, || {})?;
        selected();
        let inventory = self.inventory().map_err(|_| Error::PublicationUnknown)?;
        let (_, data) = read_selected(
            &self.directory,
            &object_name(object),
            selected_file,
            self.project,
            object,
        )
        .map_err(|_| Error::PublicationUnknown)?;
        self.check().map_err(|_| Error::PublicationUnknown)?;
        if report != expected_report
            || inventory != expected
            || data.report != report
            || data.payload() != payload
        {
            return Err(Error::PublicationUnknown);
        }
        Ok(WriteReceipt {
            object,
            report,
            inventory,
        })
    }
}

#[cfg(test)]
mod crash_tests;
#[cfg(test)]
mod tests;
