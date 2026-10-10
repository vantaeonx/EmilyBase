//! Retained native selection across an external caller's work, not user authority.
use super::*;

/// A selected immutable object and its actual descriptor under the original
/// cooperating directory owner. This is not serializable, cloneable or a user
/// capability. Expected metadata is not a lease over later filesystem state.
///
/// The owner cannot end while a later verification still uses the selection:
/// ```compile_fail
/// use emilybase_object_storage::{ObjectId, ProjectDirectory};
/// fn cannot_release(mut owner: ProjectDirectory, object: ObjectId) {
///     let mut selected = owner.put_selected(object, b"synthetic").unwrap();
///     drop(owner);
///     selected.verify().unwrap();
/// }
/// ```
pub struct SelectedObject<'owner> {
    owner: &'owner ProjectDirectory,
    file: File,
    object: ObjectId,
    report: FileReport,
}
impl SelectedObject<'_> {
    pub const fn project(&self) -> ProjectId {
        self.owner.project
    }
    pub const fn object(&self) -> ObjectId {
        self.object
    }
    /// Expected metadata from selection; call verify to check current bytes and
    /// visible identity. This metadata confers no catalog or user authority.
    pub const fn report(&self) -> &FileReport {
        &self.report
    }

    /// Fully verify the actual selected descriptor, expected scope/hash and
    /// original visible inode under the still-retained owner. Any failure is an
    /// uncertain already-published result; do not blindly retry or replace it.
    pub fn verify(&mut self) -> Result<()> {
        self.verify_with(|| {})
    }

    fn verify_with(&mut self, checked_body: impl FnOnce()) -> Result<()> {
        self.check(checked_body)
            .map_err(|_| Error::PublicationUnknown)
    }
    fn check(&mut self, checked_body: impl FnOnce()) -> Result<()> {
        self.owner.check()?;
        let (report, metadata) =
            crate::inspect::read_open_report(&mut self.file, self.owner.project, self.object)?;
        checked_body();
        if report != self.report {
            return Err(Error::File);
        }
        crate::inspect::recheck(&self.file, &metadata, HEADER_BYTES + MAX_PAYLOAD_BYTES)?;
        let name = object_name(self.object);
        check_visible(&self.owner.directory, &name, &metadata)?;
        self.owner.check()?;
        let after =
            crate::inspect::recheck(&self.file, &metadata, HEADER_BYTES + MAX_PAYLOAD_BYTES)?;
        check_visible(&self.owner.directory, &name, &after)?;
        Ok(())
    }
}
impl std::fmt::Debug for SelectedObject<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SelectedObject")
            .field("bytes", &self.report.payload_bytes)
            .finish_non_exhaustive()
    }
}

/// Selected native bounded write with its expected complete inventory receipt.
/// Later verify checks the exact object/owner, not a current complete inventory
/// lease. Per-call limits remain distinct from authoritative persisted quotas.
/// ```compile_fail
/// use emilybase_object_storage::{ObjectId, ProjectDirectory, WriteLimits};
/// fn cannot_release(mut owner: ProjectDirectory, object: ObjectId) {
///     let limits = WriteLimits::new(1, 9).unwrap();
///     let mut selected = owner.put_bounded_selected(object, b"synthetic", limits).unwrap();
///     drop(owner);
///     selected.verify().unwrap();
/// }
/// ```
pub struct SelectedWrite<'owner> {
    selected: SelectedObject<'owner>,
    receipt: WriteReceipt,
}
impl SelectedWrite<'_> {
    pub const fn project(&self) -> ProjectId {
        self.selected.project()
    }
    pub const fn object(&self) -> ObjectId {
        self.receipt.object()
    }
    pub const fn report(&self) -> &FileReport {
        self.receipt.report()
    }
    /// Expected complete inventory at publication, not current authorization or
    /// a reservation. Revalidation does not silently refresh this receipt.
    pub const fn inventory(&self) -> &Inventory {
        self.receipt.inventory()
    }
    pub fn verify(&mut self) -> Result<()> {
        self.selected.verify()
    }
}
impl std::fmt::Debug for SelectedWrite<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SelectedWrite")
            .field("bytes", &self.report().payload_bytes)
            .field("objects", &self.inventory().entries().len())
            .finish_non_exhaustive()
    }
}

impl ProjectDirectory {
    /// Publish and keep the actual selected file open under this owner. This
    /// supplies native retention only, not a transaction across a catalog/file.
    pub fn put_selected(&mut self, object: ObjectId, payload: &[u8]) -> Result<SelectedObject<'_>> {
        let (file, report) = self.put_retained_with(object, payload, || {})?;
        Ok(SelectedObject {
            owner: self,
            file,
            object,
            report,
        })
    }

    /// Execute the original complete bounded write, retaining its selected inode
    /// and original owner across later caller work. No persisted quota is added.
    pub fn put_bounded_selected(
        &mut self,
        object: ObjectId,
        payload: &[u8],
        limits: WriteLimits,
    ) -> Result<SelectedWrite<'_>> {
        let (file, receipt) =
            self.put_bounded_retained_with(object, payload, limits, || {}, || {})?;
        let selected = SelectedObject {
            owner: self,
            file,
            object,
            report: receipt.report().clone(),
        };
        Ok(SelectedWrite { selected, receipt })
    }
}

#[cfg(test)]
mod tests;
