//! Operator metadata under actual original file/private/common owners.
use super::*;
use emilybase_auth::{
    accounts::{FilePolicyReceipt, FilePolicyWritePhase},
    file_policy::StoreContext,
};
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
mod private_wal;
use private_wal::PrivateWal;

impl AccountRoot {
    /// Explicit private v5 -> v6 migration; never changes public admission.
    /// Requires a declared native v2 file component and its real private owner.
    pub fn enable_file_policy_catalog(&mut self, project: &str, key: &str) -> Result<()> {
        self.with_file_policy_write(project, key, |account, _, guard| {
            account.enable_file_policy_catalog_guarded(guard)
        })
    }
    /// Current installed metadata for this actual native database, not a grant.
    pub fn project_file_policy(
        &mut self,
        project: &str,
        key: &str,
    ) -> Result<Option<FilePolicyReceipt>> {
        let mut selection = None;
        let result = self.with_file_component_accounts(project, key, |native, account, check| {
            let (account, owner) = account.ok_or(Error::Denied)?;
            let private = selection.insert(PrivateWal::new(account, owner)?);
            let database = *native.inspect()?.database_id();
            checkpoint("account_file_policy_context_acquired");
            native.check_selection()?;
            private.check()?;
            check()?;
            Ok(account
                .file_policy_receipts()?
                .into_iter()
                .find(|policy| policy.database == database))
        });
        if let Some(private) = selection {
            private.check()?;
        }
        result
    }
    /// Exact CAS for one actual native database; callers cannot assert its ID.
    /// No implicit migration, retry, user grant or public route is introduced.
    pub fn install_project_file_policy(
        &mut self,
        project: &str,
        key: &str,
        expected: u64,
        document: &[u8],
    ) -> Result<FilePolicyReceipt> {
        self.with_file_policy_write(project, key, |account, database, guard| {
            account.install_file_policy_guarded(
                StoreContext { project, database },
                expected,
                document,
                guard,
            )
        })
    }
    fn with_file_policy_write<T>(
        &mut self,
        project: &str,
        key: &str,
        work: impl FnOnce(
            &mut AccountStore,
            [u8; 16],
            &mut dyn FnMut(FilePolicyWritePhase) -> emilybase_auth::accounts::Result<()>,
        ) -> emilybase_auth::accounts::Result<T>,
    ) -> Result<T> {
        let mut may_have_written = false;
        let mut selection = None;
        let result = catch_unwind(AssertUnwindSafe(|| {
            let result =
                self.with_file_component_accounts(project, key, |native, account, check| {
                    let (account, owner) = account.ok_or(Error::Denied)?;
                    let private = selection.insert(PrivateWal::new(account, owner)?);
                    let database = *native.inspect()?.database_id();
                    checkpoint("account_file_policy_context_acquired");
                    native.check_selection()?;
                    private.check()?;
                    check()?;
                    let mut outer_error = None;
                    let result = {
                        let mut guard = |phase| {
                            if phase == FilePolicyWritePhase::Committed {
                                may_have_written = true;
                            }
                            checkpoint(match phase {
                                FilePolicyWritePhase::BeforeOperation => {
                                    "account_file_policy_before_operation"
                                }
                                FilePolicyWritePhase::BeforeCommit => {
                                    "account_file_policy_before_commit"
                                }
                                FilePolicyWritePhase::Committed => "account_file_policy_committed",
                                FilePolicyWritePhase::Verified => "account_file_policy_verified",
                            });
                            private
                                .check()
                                .and_then(|()| native.check_selection().map_err(Error::Files))
                                .and_then(|()| check())
                                .map_err(|error| {
                                    outer_error = Some(error);
                                    emilybase_auth::accounts::Error::FilePolicyGuardRejected
                                })?;
                            if phase == FilePolicyWritePhase::BeforeCommit {
                                // Only after accepting the last check: native commit
                                // may attempt durable I/O next, including on error/unwind.
                                may_have_written = true;
                                checkpoint("account_file_policy_commit_admitted");
                            }
                            Ok(())
                        };
                        work(account, database, &mut guard)
                    };
                    checkpoint("account_file_policy_native_returned");
                    native.check_selection()?;
                    private.check()?;
                    result.map_err(|error| outer_error.unwrap_or(Error::Accounts(error)))
                });
            if let Some(private) = &selection {
                private.check()?;
            }
            result
        }));
        match result {
            Ok(Err(error)) if may_have_written => {
                self.file_write_poisoned = true;
                Err(Error::AccountFilePolicyWriteUnknown(Box::new(error)))
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
