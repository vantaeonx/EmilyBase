//! Pure private archive validation and explicit sensitive snapshot export.
use super::*;
use emilybase_database::Snapshot;

/// Untrusted archive inventory only. No users, verifier exports or session scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivateArchiveReport {
    pub database: emilybase_backup::Report,
    pub private_version: u16,
    pub accounts: usize,
    pub session_families: usize,
    pub clock_floor: Option<u64>,
}
pub(super) struct ValidatedState {
    pub dummy: PasswordDigest,
    pub session_scope: Option<crate::tokens::TokenScope>,
    pub session_clock: Option<u64>,
    pub version: u16,
    pub accounts: usize,
    pub families: usize,
}
pub(super) fn validate_snapshot(snapshot: &Snapshot, project: &str) -> Result<ValidatedState> {
    if !valid_project_id(project) {
        return Err(Error::Scope);
    }
    if snapshot.schema(SCOPE).map_err(|_| Error::Corrupt)? != &scope_schema()
        || snapshot.schema(USERS).map_err(|_| Error::Corrupt)? != &user_schema()
    {
        return Err(Error::Corrupt);
    }
    let mut scope_rows = snapshot
        .primary_rows(SCOPE, None, None)
        .map_err(|_| Error::Corrupt)?;
    let scope = scope_rows
        .next()
        .transpose()
        .map_err(|_| Error::Corrupt)?
        .ok_or(Error::Corrupt)?;
    if scope_rows.next().is_some() {
        return Err(Error::Corrupt);
    }
    let [
        Value::Integer(1),
        Value::Integer(version),
        Value::Text(stored),
        Value::Bytes(dummy),
    ] = scope.as_slice()
    else {
        return Err(Error::Corrupt);
    };
    if !matches!(*version, 1..=3) || !valid_project_id(stored) {
        return Err(Error::Corrupt);
    }
    if stored != project {
        return Err(Error::ScopeMismatch);
    }
    let dummy = PasswordDigest::decode(dummy).map_err(|_| Error::Corrupt)?;
    if snapshot.row_count() > MAX_ACCOUNTS + MAX_SESSION_FAMILIES + 3 {
        return Err(Error::Corrupt);
    }
    let mut identities = std::collections::BTreeSet::new();
    for row in snapshot
        .primary_rows(USERS, None, None)
        .map_err(|_| Error::Corrupt)?
    {
        let record = Record::decode(row.map_err(|_| Error::Corrupt)?)?;
        if !identities.insert(record.info.id) || identities.len() > MAX_ACCOUNTS {
            return Err(Error::Corrupt);
        }
    }
    let session_scope = session_schema::validate_inventory(snapshot, project, *version)?;
    let session_clock = session_clock::validate_clock(snapshot, *version)?;
    let accounts = identities.len();
    let families = if session_scope.is_some() {
        snapshot
            .primary_rows(session_schema::FAMILIES, None, None)
            .map_err(|_| Error::Corrupt)?
            .try_fold(0_usize, |count, row| {
                row.map_err(|_| Error::Corrupt)?;
                count
                    .checked_add(1)
                    .filter(|n| *n <= MAX_SESSION_FAMILIES)
                    .ok_or(Error::Corrupt)
            })?
    } else {
        0
    };
    Ok(ValidatedState {
        dummy,
        session_scope,
        session_clock,
        version: *version as u16,
        accounts,
        families,
    })
}

/// Pure engine and complete private schema/project validation. It does not reset
/// sessions, authenticate a user, grant permissions or create a store on disk.
pub fn inspect_private_account_backup_bytes(
    bytes: &[u8],
    project: &str,
) -> Result<PrivateArchiveReport> {
    if !valid_project_id(project) {
        return Err(Error::Scope);
    }
    let verified = emilybase_backup::decode_verified(bytes)?;
    let state = validate_snapshot(&verified.image().snapshot, project)?;
    Ok(PrivateArchiveReport {
        database: verified.report().clone(),
        private_version: state.version,
        accounts: state.accounts,
        session_families: state.families,
        clock_floor: state.session_clock,
    })
}

impl AccountStore {
    /// Explicit private archive bytes, containing sensitive metadata/verifiers.
    /// Do not send through generic SQL/HTTP responses or automatically log them.
    /// Caller retention is outside the library's count bounds, not a heap quota.
    pub fn backup_image(&mut self) -> Result<Vec<u8>> {
        validate_snapshot(self.database.view()?, &self.project)?;
        let wal = self.database.committed_wal()?;
        Ok(emilybase_backup::encode(&wal)?)
    }
}
