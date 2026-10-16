//! Actual private owner and durable fresh session scope through nested restore.
use super::{AccountStore, Error, Result};
use crate::accounts::{PrivateArchiveReport, inspect_private_account_backup_bytes};
use crate::password::PasswordPool;
use emilybase_backup::{
    PreparedDatabaseOwner, PreparedRestoreError, with_guarded_prepared_owner_bytes_at,
};
use emilybase_transactions::Database;
use std::fs::File;

// Never implement this trait on public AccountStore: the mutable getter must
// remain hidden behind this private adapter and report-only coordination.
struct PreparedAccounts {
    store: AccountStore,
    report: PrivateArchiveReport,
}
impl PreparedDatabaseOwner for PreparedAccounts {
    fn database(&mut self) -> &mut Database {
        &mut self.store.database
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Boundary {
    Original,
    Reset,
    Selected,
    Worked,
}

/// Validate private scope/time before staging, move the actual restored database
/// into a private adapter and reset sessions durably before child selection.
/// Retain that original owner and WAL through trusted report-only nested work and
/// final core proofs. v1/v2 activate v3; v4/v5 remain, with v5 admission closed.
/// Work errors may be nested values; propagate both result layers. This grants no
/// current user permission and does not restore/adopt a complete platform root.
pub fn with_restored_private_account_bytes_at<T>(
    bytes: &[u8],
    project: &str,
    parent: &File,
    name: &str,
    pool: PasswordPool,
    now: u64,
    work: impl FnOnce(&PrivateArchiveReport) -> T,
) -> Result<T> {
    with_boundary(bytes, project, parent, name, pool, now, (work, |_| {}))
}

fn with_boundary<T>(
    bytes: &[u8],
    project: &str,
    parent: &File,
    name: &str,
    pool: PasswordPool,
    now: u64,
    hooks: (
        impl FnOnce(&PrivateArchiveReport) -> T,
        impl FnMut(Boundary),
    ),
) -> Result<T> {
    let (work, mut boundary) = hooks;
    if !crate::valid_project_id(project) {
        return Err(Error::Scope);
    }
    if now > i64::MAX as u64 {
        return Err(Error::Clock);
    }
    let source = inspect_private_account_backup_bytes(bytes, project)?;
    // FnOnce phases run sequentially. RefCell permits two bounded closures to
    // invoke the same private test boundary without exporting mutable adapters.
    let boundary = std::cell::RefCell::new(&mut boundary);
    let result = with_guarded_prepared_owner_bytes_at(
        bytes,
        parent,
        name,
        |database, original| -> Result<PreparedAccounts> {
            let mut store = AccountStore::from_database(database, project, pool)?;
            boundary.borrow_mut()(Boundary::Original);
            original.check()?;
            if store.session_clock_floor()?.is_some() {
                store.reset_session_clock(now)?;
            } else {
                store.enable_session_clock(now)?;
            }
            boundary.borrow_mut()(Boundary::Reset);
            let report = inspect_private_account_backup_bytes(&store.backup_image()?, project)?;
            if report.database.database_id != source.database.database_id
                || report.private_version != source.private_version.max(3)
                || report.accounts != source.accounts
                || report.session_families != source.session_families
                || report.clock_floor != Some(now)
            {
                return Err(Error::Corrupt);
            }
            Ok(PreparedAccounts { store, report })
        },
        |owner, report| -> Result<T> {
            if report != &owner.report.database {
                return Err(Error::Corrupt);
            }
            boundary.borrow_mut()(Boundary::Selected);
            let value = work(&owner.report);
            boundary.borrow_mut()(Boundary::Worked);
            Ok(value)
        },
    );
    match result {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(Error::RetainedRestoreUnknown(Box::new(error))),
        Err(PreparedRestoreError::Preparation(error)) => Err(error),
        Err(PreparedRestoreError::Backup(error)) => Err(Error::Backup(error)),
    }
}

#[cfg(all(test, target_os = "linux"))]
pub(crate) mod tests;
