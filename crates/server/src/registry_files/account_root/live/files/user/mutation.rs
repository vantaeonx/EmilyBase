//! User metadata edits consume current policy and the actual selected reference.
use super::*;
use emilybase_files::{NativeMutationCheck, NativeWritePhase};
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};

/// Copied logical write result, never a current grant, payload or owner disclosure.
/// Equal revisions describe a verified rename no-op. Removal's revision is the
/// actual commit, while previous_revision describes the removed logical row.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct AccountFileWrite {
    id: FileId,
    revision: u64,
    previous_revision: u64,
}
impl AccountFileWrite {
    pub const fn id(self) -> FileId {
        self.id
    }
    pub const fn revision(self) -> u64 {
        self.revision
    }
    pub const fn previous_revision(self) -> u64 {
        self.previous_revision
    }
}
impl std::fmt::Debug for AccountFileWrite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountFileWrite")
            .field("revision", &self.revision)
            .field("previous_revision", &self.previous_revision)
            .finish_non_exhaustive()
    }
}
#[derive(Clone, Copy)]
enum Edit<'a> {
    Rename(&'a str),
    Remove,
}

impl AccountRoot {
    /// Current user rename under installed per-action policy and exact revision.
    /// Authentication/admission precede native lookup and name validation. Time
    /// is a trusted service observation, not an untrusted request timestamp.
    pub fn public_rename_file(
        &mut self,
        project: &str,
        access: &str,
        now: u64,
        id: FileId,
        expected_revision: u64,
        name: &str,
    ) -> Result<AccountFileWrite> {
        self.edit_user_file(
            project,
            access,
            now,
            id,
            expected_revision,
            Edit::Rename(name),
        )
    }

    /// Current user removal changes visibility only. Immutable physical objects
    /// remain charged; no public receipt reveals removed owner/object/payload.
    pub fn public_remove_file(
        &mut self,
        project: &str,
        access: &str,
        now: u64,
        id: FileId,
        expected_revision: u64,
    ) -> Result<AccountFileWrite> {
        self.edit_user_file(project, access, now, id, expected_revision, Edit::Remove)
    }

    fn edit_user_file(
        &mut self,
        project: &str,
        access: &str,
        now: u64,
        id: FileId,
        expected: u64,
        edit: Edit<'_>,
    ) -> Result<AccountFileWrite> {
        let mut may_have_written = false;
        let result = catch_unwind(AssertUnwindSafe(|| {
            self.with_user_files(project, access, now, |native, proof, context, check| {
                let mut previous = None;
                let mut outer_error = None;
                let result = {
                    let mut guard = |event: NativeMutationCheck<'_>| {
                        let phase = match &event {
                            NativeMutationCheck::Phase(phase) => Some(*phase),
                            NativeMutationCheck::Selected(_) => None,
                        };
                        if phase == Some(NativeWritePhase::Committed) {
                            may_have_written = true;
                        }
                        let checked = (|| -> Result<()> {
                            checkpoint(match &event {
                                NativeMutationCheck::Selected(_) => {
                                    "account_user_file_write_selected"
                                }
                                NativeMutationCheck::Phase(NativeWritePhase::BeforeOperation) => {
                                    "account_user_file_write_before_operation"
                                }
                                NativeMutationCheck::Phase(NativeWritePhase::BeforeBlob) => {
                                    "account_user_file_write_before_blob"
                                }
                                NativeMutationCheck::Phase(NativeWritePhase::BeforeCommit) => {
                                    "account_user_file_write_before_commit"
                                }
                                NativeMutationCheck::Phase(NativeWritePhase::Committed) => {
                                    "account_user_file_write_committed"
                                }
                                NativeMutationCheck::Phase(NativeWritePhase::Verified) => {
                                    "account_user_file_write_verified"
                                }
                            });
                            check()?;
                            proof
                                .check_context(context)
                                .map_err(emilybase_auth::accounts::Error::FilePolicy)?;
                            if let NativeMutationCheck::Selected(info) = event {
                                let change = match edit {
                                    Edit::Rename(_) => Change::Rename {
                                        owner: *info.owner(),
                                    },
                                    Edit::Remove => Change::Remove {
                                        owner: *info.owner(),
                                    },
                                };
                                proof
                                    .authorize(context, change)
                                    .map_err(|error| match error {
                                        PolicyError::Denied => Error::Denied,
                                        other => Error::Accounts(
                                            emilybase_auth::accounts::Error::FilePolicy(other),
                                        ),
                                    })?;
                                previous = Some(info.revision());
                            }
                            Ok(())
                        })();
                        if let Err(error) = checked {
                            outer_error = Some(error);
                            return Err(emilybase_files::Error::GuardRejected);
                        }
                        if phase == Some(NativeWritePhase::BeforeCommit) {
                            // The original commit can perform durable I/O next;
                            // retain uncertainty even if it errors or unwinds.
                            may_have_written = true;
                            checkpoint("account_user_file_write_commit_admitted");
                        }
                        Ok(())
                    };
                    match edit {
                        Edit::Rename(name) => native
                            .rename_with_checks(id, expected, name, &mut guard)
                            .map(|info| info.revision()),
                        Edit::Remove => native
                            .remove_with_checks(id, expected, &mut guard)
                            .map(|receipt| receipt.revision()),
                    }
                };
                if matches!(&result, Err(emilybase_files::Error::OutcomeUnknown(_))) {
                    may_have_written = true;
                }
                checkpoint("account_user_file_write_native_returned");
                let revision =
                    result.map_err(|error| match outer_error.unwrap_or(Error::Files(error)) {
                        Error::Files(emilybase_files::Error::Missing) => Error::Denied,
                        other => other,
                    })?;
                Ok(AccountFileWrite {
                    id,
                    revision,
                    previous_revision: previous
                        .ok_or(Error::Files(emilybase_files::Error::Corrupt))?,
                })
            })
        }));
        match result {
            Ok(Err(error)) if may_have_written => {
                self.file_write_poisoned = true;
                Err(Error::AccountFileWriteUnknown(Box::new(error)))
            }
            Err(unwind) => {
                if may_have_written {
                    self.file_write_poisoned = true;
                }
                resume_unwind(unwind)
            }
            Ok(result) => result,
        }
    }
}
