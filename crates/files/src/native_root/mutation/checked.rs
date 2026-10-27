//! Trusted selection-aware checks, never detached user authority.
use super::*;

/// An actual native mutation boundary under the existing original owners.
/// `Selected` borrows the current validated reference, before CAS comparison or
/// staging. Missing IDs have no selection event. Metadata is sensitive native
/// data; a trusted caller must not expose it before its outer result succeeds.
/// The borrowed row cannot become an authorization handle for a later operation.
/// ```compile_fail
/// use emilybase_files::{FileId, NativeFileRoot, NativeMutationCheck};
/// fn cannot_detach(root: &mut NativeFileRoot, id: FileId) {
///     let mut selected = None;
///     root.rename_with_checks(id, 1, "name", |event| {
///         if let NativeMutationCheck::Selected(info) = event {
///             selected = Some(info);
///         }
///         Ok(())
///     }).unwrap();
///     let _ = selected;
/// }
/// ```
pub enum NativeMutationCheck<'a> {
    Selected(&'a FileInfo),
    Phase(NativeWritePhase),
}
impl<'a> NativeMutationCheck<'a> {
    pub(super) fn of(check: &WriteCheck<'a>) -> Self {
        match check {
            WriteCheck::Selected(info) => Self::Selected(info),
            WriteCheck::BeforeBlob => Self::Phase(NativeWritePhase::BeforeBlob),
            WriteCheck::BeforeCommit => Self::Phase(NativeWritePhase::BeforeCommit),
            WriteCheck::Committed(_) => Self::Phase(NativeWritePhase::Committed),
            WriteCheck::Verified => Self::Phase(NativeWritePhase::Verified),
        }
    }
}
pub(super) type MutationGuard<'a> =
    Option<&'a mut dyn for<'info> FnMut(NativeMutationCheck<'info>) -> Result<()>>;

impl NativeFileRoot {
    /// Rename with a trusted check of the actual selected owner/reference before
    /// revision matching, no-op acceptance or staging. No session authorization
    /// is implied. Source checks surround callbacks; panic requires reopen.
    pub fn rename_with_checks(
        &mut self,
        id: FileId,
        revision: u64,
        name: &str,
        mut check: impl for<'info> FnMut(NativeMutationCheck<'info>) -> Result<()>,
    ) -> Result<FileInfo> {
        self.guarded_operation(|root| {
            match root.edit_with_checks(
                Operation::Rename(id, revision, name),
                (|_| {}, Some(&mut check)),
            )? {
                Outcome::Reference(info, _) => Ok(info),
                Outcome::Quota(_) => Err(Error::Corrupt),
            }
        })
    }

    /// Remove logical visibility under a trusted current-reference check. The
    /// original blob remains charged. This native receipt includes sensitive
    /// removed metadata; an outer user gateway must choose its own bounded reply.
    pub fn remove_with_checks(
        &mut self,
        id: FileId,
        revision: u64,
        mut check: impl for<'info> FnMut(NativeMutationCheck<'info>) -> Result<()>,
    ) -> Result<FileRemoval> {
        self.guarded_operation(|root| {
            match root
                .edit_with_checks(Operation::Remove(id, revision), (|_| {}, Some(&mut check)))?
            {
                Outcome::Reference(info, committed) => {
                    Ok(FileRemoval::from_committed(info, committed))
                }
                Outcome::Quota(_) => Err(Error::Corrupt),
            }
        })
    }
}

#[cfg(test)]
mod tests;
