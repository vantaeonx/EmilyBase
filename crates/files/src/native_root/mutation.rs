use super::*;
use crate::QuotaState;
use crate::mutation::{FileRemoval, MetadataBoundary};
mod checked;
pub use checked::NativeMutationCheck;

#[derive(Clone, Copy)]
enum Operation<'a> {
    Rename(FileId, u64, &'a str),
    Remove(FileId, u64),
    Quota(QuotaState, FileQuota),
}
enum Outcome {
    Reference(FileInfo, u64),
    Quota(QuotaState),
}

impl NativeFileRoot {
    /// Trusted outer coordination; this callback is not current user authority.
    pub fn rename_guarded(
        &mut self,
        id: FileId,
        revision: u64,
        name: &str,
        mut guard: impl FnMut(NativeWritePhase) -> Result<()>,
    ) -> Result<FileInfo> {
        self.guarded_operation(|root| {
            match root.edit_with_guard(
                Operation::Rename(id, revision, name),
                (|_| {}, Some(&mut guard)),
            )? {
                Outcome::Reference(info, _) => Ok(info),
                Outcome::Quota(_) => Err(Error::Corrupt),
            }
        })
    }
    pub fn remove_guarded(
        &mut self,
        id: FileId,
        revision: u64,
        mut guard: impl FnMut(NativeWritePhase) -> Result<()>,
    ) -> Result<FileRemoval> {
        self.guarded_operation(|root| {
            match root
                .edit_with_guard(Operation::Remove(id, revision), (|_| {}, Some(&mut guard)))?
            {
                Outcome::Reference(info, committed) => {
                    Ok(FileRemoval::from_committed(info, committed))
                }
                Outcome::Quota(_) => Err(Error::Corrupt),
            }
        })
    }
    pub fn set_quota_guarded(
        &mut self,
        expected: QuotaState,
        quota: FileQuota,
        mut guard: impl FnMut(NativeWritePhase) -> Result<()>,
    ) -> Result<QuotaState> {
        self.guarded_operation(|root| {
            match root.edit_with_guard(
                Operation::Quota(expected, quota),
                (|_| {}, Some(&mut guard)),
            )? {
                Outcome::Quota(state) => Ok(state),
                Outcome::Reference(_, _) => Err(Error::Corrupt),
            }
        })
    }
    /// Copied scoped/global CAS state, not a user grant or reservation.
    pub fn quota_state(&mut self) -> Result<QuotaState> {
        self.observe(|store, _| store.quota_state(), |_| {})
    }
    /// Bounded copied native metadata for one committed logical identity.
    pub fn file_info(&mut self, id: FileId) -> Result<Option<FileInfo>> {
        self.observe(
            |_, state| Ok(state.files.iter().find(|f| f.id() == id).cloned()),
            |_| {},
        )
    }
    /// Rename under exact logical revision, retaining original root/source checks
    /// across actual commit. A current equal name is a verified WAL-preserving no-op.
    pub fn rename(&mut self, id: FileId, revision: u64, name: &str) -> Result<FileInfo> {
        match self.edit_with(Operation::Rename(id, revision, name), |_| {})? {
            Outcome::Reference(info, _) => Ok(info),
            Outcome::Quota(_) => Err(Error::Corrupt),
        }
    }
    /// Remove logical visibility only. Every physical orphan remains charged.
    pub fn remove(&mut self, id: FileId, revision: u64) -> Result<FileRemoval> {
        match self.edit_with(Operation::Remove(id, revision), |_| {})? {
            Outcome::Reference(info, committed) => Ok(FileRemoval::from_committed(info, committed)),
            Outcome::Quota(_) => Err(Error::Corrupt),
        }
    }
    /// Change persisted physical limits with exact project/database/global CAS.
    /// Original quota update retains every actual blob through commit/readback.
    pub fn set_quota(&mut self, expected: QuotaState, quota: FileQuota) -> Result<QuotaState> {
        match self.edit_with(Operation::Quota(expected, quota), |_| {})? {
            Outcome::Quota(state) => Ok(state),
            Outcome::Reference(_, _) => Err(Error::Corrupt),
        }
    }
    fn edit_with(
        &mut self,
        operation: Operation<'_>,
        boundary: impl FnMut(MetadataBoundary),
    ) -> Result<Outcome> {
        self.edit_with_guard(operation, (boundary, None))
    }
    fn edit_with_guard(
        &mut self,
        operation: Operation<'_>,
        hooks: (impl FnMut(MetadataBoundary), super::guard::Guard<'_>),
    ) -> Result<Outcome> {
        let (boundary, guard) = hooks;
        match guard {
            Some(guard) => self.edit_with_checks(
                operation,
                (
                    boundary,
                    Some(&mut |event| match event {
                        NativeMutationCheck::Selected(_) => Ok(()),
                        NativeMutationCheck::Phase(phase) => guard(phase),
                    }),
                ),
            ),
            None => self.edit_with_checks(operation, (boundary, None)),
        }
    }
    fn edit_with_checks(
        &mut self,
        operation: Operation<'_>,
        hooks: (impl FnMut(MetadataBoundary), checked::MutationGuard<'_>),
    ) -> Result<Outcome> {
        let (boundary, mut guard) = hooks;
        if let Operation::Rename(_, _, name) = operation {
            crate::records::validate_name(name)?;
        }
        let before = self.before_write()?;
        let admitted = guard.as_mut().map_or(Ok(()), |check| {
            check(NativeMutationCheck::Phase(
                NativeWritePhase::BeforeOperation,
            ))
        });
        let selection = &mut self.selection;
        let mut source_failed = false;
        let check = |at: WriteCheck<'_>| {
            let event = NativeMutationCheck::of(&at);
            let result = selection.write_check(at);
            if result.is_err() {
                source_failed = true;
            }
            result?;
            if let Some(check) = guard.as_mut() {
                // Check the selected namespace even when the callback refuses.
                // A callback error must not conceal a substituted source.
                let admitted = check(event);
                let selected = selection.check();
                if selected.is_err() {
                    source_failed = true;
                }
                selected?;
                admitted?;
            }
            Ok(())
        };
        let result = admitted.and_then(|()| match operation {
            Operation::Rename(id, revision, name) => self
                .store
                .mutate_checked_with(id, revision, Some(name), (boundary, check))
                .map(|(info, committed)| Outcome::Reference(info, committed)),
            Operation::Remove(id, revision) => self
                .store
                .mutate_checked_with(id, revision, None, (boundary, check))
                .map(|(info, committed)| Outcome::Reference(info, committed)),
            Operation::Quota(expected, quota) => self
                .store
                .set_quota_checked_with(expected, quota, (boundary, check))
                .map(Outcome::Quota),
        });
        self.finish_write(&before, result, source_failed, |after, value| {
            if after.inventory != before.inventory {
                return Err(Error::Corrupt);
            }
            match (operation, value) {
                (Operation::Rename(id, _, name), Outcome::Reference(info, revision))
                    if after.quota == before.quota
                        && info.id() == id
                        && info.name() == name
                        && info.revision() == *revision
                        && after.files.contains(info)
                        && after.files.len() == before.files.len()
                        && before.files.iter().all(|old| {
                            if old.id() == id {
                                old.object() == info.object()
                                    && old.owner() == info.owner()
                                    && old.report() == info.report()
                            } else {
                                after.files.contains(old)
                            }
                        }) =>
                {
                    Ok(())
                }
                (Operation::Remove(id, _), Outcome::Reference(info, _))
                    if after.quota == before.quota
                        && info.id() == id
                        && before.files.contains(info)
                        && after.files.len() + 1 == before.files.len()
                        && after
                            .files
                            .iter()
                            .all(|f| f.id() != id && before.files.contains(f)) =>
                {
                    Ok(())
                }
                (Operation::Quota(_, quota), Outcome::Quota(state))
                    if state.quota() == quota
                        && after.quota == quota
                        && after.files == before.files =>
                {
                    Ok(())
                }
                _ => Err(Error::Corrupt),
            }
        })
    }
}

#[cfg(test)]
mod tests;
