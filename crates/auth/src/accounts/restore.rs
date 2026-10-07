//! Private schema validation and scope reset before owned backup publication.
use super::{AccountStore, Error, Result};
use crate::password::PasswordPool;
use std::path::Path;

/// Restore a separately captured private account archive, not a whole platform.
/// Validate the expected project and reset session scope/time before publication.
/// Trusted operator paths/time only; destination is never replaced or retried.
pub fn restore_private_accounts(
    archive: impl AsRef<Path>,
    target: impl AsRef<Path>,
    project: &str,
    pool: PasswordPool,
    now: u64,
) -> Result<emilybase_backup::Report> {
    restore_private_with(
        archive.as_ref(),
        target.as_ref(),
        project,
        pool,
        now,
        |_| {},
    )
}

/// Restore supplied sensitive private bytes directly. Complete schema/scope
/// validation and durable fresh session incarnation precede directory publication.
pub fn restore_private_account_bytes(
    bytes: &[u8],
    target: impl AsRef<Path>,
    project: &str,
    pool: PasswordPool,
    now: u64,
) -> Result<emilybase_backup::Report> {
    restore_private_bytes_with(bytes, target.as_ref(), project, pool, now, |_| {})
}

enum Source<'a> {
    File(&'a Path),
    Bytes(&'a [u8]),
}
pub(super) fn restore_private_bytes_with(
    bytes: &[u8],
    target: &Path,
    project: &str,
    pool: PasswordPool,
    now: u64,
    prepared: impl FnOnce(&Path),
) -> Result<emilybase_backup::Report> {
    restore_input(Source::Bytes(bytes), target, project, pool, now, prepared)
}

pub(super) fn restore_private_with(
    archive: &Path,
    target: &Path,
    project: &str,
    pool: PasswordPool,
    now: u64,
    prepared: impl FnOnce(&Path),
) -> Result<emilybase_backup::Report> {
    restore_input(Source::File(archive), target, project, pool, now, prepared)
}
fn restore_input(
    source: Source<'_>,
    target: &Path,
    project: &str,
    pool: PasswordPool,
    now: u64,
    prepared: impl FnOnce(&Path),
) -> Result<emilybase_backup::Report> {
    if !crate::valid_project_id(project) {
        return Err(Error::Scope);
    }
    if now > i64::MAX as u64 {
        return Err(Error::Clock);
    }
    let prepare = |path: &Path| {
        let mut store = AccountStore::open(path, project, pool)?;
        if store.session_clock_floor()?.is_some() {
            store.reset_session_clock(now)?;
        } else {
            // v1/v2 activate a fresh incarnation rather than using legacy state.
            store.enable_session_clock(now)?;
        }
        drop(store);
        prepared(path);
        Ok::<(), Error>(())
    };
    let result = match source {
        Source::File(archive) => emilybase_backup::restore_prepared(archive, target, prepare),
        Source::Bytes(bytes) => emilybase_backup::restore_prepared_bytes(bytes, target, prepare),
    };
    match result {
        Ok(report) => Ok(report),
        Err(emilybase_backup::PreparedRestoreError::Backup(error)) => Err(Error::Backup(error)),
        Err(emilybase_backup::PreparedRestoreError::Preparation(error)) => Err(error),
    }
}
