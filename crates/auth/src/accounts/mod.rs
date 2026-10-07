//! Separate private account storage; not attached to the server's public SQL database.
mod records;
mod session_clock;
#[cfg(test)]
mod session_clock_tests;
mod session_schema;
#[cfg(test)]
mod session_schema_tests;

pub use session_clock::inspect_session_clock_record;
pub use session_schema::{MAX_SESSION_FAMILIES, SessionRecordInfo, inspect_session_record};
#[cfg(all(test, target_os = "linux"))]
mod recovery_tests;
#[cfg(test)]
mod tests;

#[cfg(test)]
static TEST_IO: std::sync::Mutex<()> = std::sync::Mutex::new(());

use crate::password::{MAX_PASSWORD_BYTES, PasswordDigest, PasswordError, PasswordPool};
use crate::valid_project_id;
use emilybase_catalog::{Key, Value};
use emilybase_transactions::Database;
use records::{Record, scope_schema, user_schema, validate_login};
use std::path::Path;
use zeroize::Zeroizing;

pub const MAX_ACCOUNTS: usize = 1024;
pub const MAX_LOGIN_BYTES: usize = 64;
const SCOPE: &str = "auth_scope";
const USERS: &str = "auth_users";

#[derive(thiserror::Error)]
pub enum Error {
    #[error("invalid project account scope")]
    Scope,
    #[error("account store belongs to another project")]
    ScopeMismatch,
    #[error("invalid private account store")]
    Corrupt,
    #[error("invalid canonical account login")]
    Login,
    #[error("account login already exists")]
    Exists,
    #[error("account capacity exhausted")]
    Capacity,
    #[error("credential check failed")]
    Denied,
    #[error("credential epoch exhausted")]
    Epoch,
    #[error("session clock storage is not enabled")]
    ClockDisabled,
    #[error("invalid or backward session time")]
    Clock,
    #[error("operating-system randomness is unavailable")]
    Randomness,
    #[error("password operation failed")]
    Password(#[from] PasswordError),
    #[error("account storage operation failed")]
    Storage(#[from] emilybase_transactions::Error),
    #[error("account backup operation failed")]
    Backup(#[from] emilybase_backup::Error),
}
impl std::fmt::Debug for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Do not recursively format storage/query values or credential input.
        std::fmt::Display::fmt(self, f)
    }
}
pub type Result<T> = std::result::Result<T, Error>;

/// Bounded pure inspection of caller-supplied private row data. This does not
/// read storage, verify a password, authorize a user or issue a session.
pub fn inspect_account_record(row: &[Value]) -> Result<AccountInfo> {
    Ok(Record::decode(row)?.info)
}

/// Metadata, never a session token or an authorization capability.
#[derive(Clone, PartialEq, Eq)]
pub struct AccountInfo {
    pub id: [u8; 16],
    pub login: String,
    pub credential_epoch: u64,
    pub disabled: bool,
}
impl std::fmt::Debug for AccountInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountInfo")
            .field("identity", &"redacted")
            .field("credential_epoch", &self.credential_epoch)
            .field("disabled", &self.disabled)
            .finish()
    }
}

/// Owns a separate mandatory-WAL directory and its exclusive writer lock.
/// The caller selects a trusted private local path and binds it to a project.
/// Cloned crypto pools must be shared across stores in the intended service scope.
pub struct AccountStore {
    database: Database,
    project: String,
    pool: PasswordPool,
    dummy: PasswordDigest,
    session_scope: Option<crate::tokens::TokenScope>,
    session_clock: Option<u64>,
}

impl AccountStore {
    pub fn create(path: impl AsRef<Path>, project: &str, pool: PasswordPool) -> Result<Self> {
        if !valid_project_id(project) {
            return Err(Error::Scope);
        }
        // Fail before creating the directory if entropy/KDF admission is unavailable.
        let mut unknown = Zeroizing::new([0; 32]);
        getrandom::fill(unknown.as_mut_slice()).map_err(|_| Error::Randomness)?;
        let dummy = pool.hash(unknown.as_slice())?;
        drop(unknown);
        let mut database = Database::create(path)?;
        let mut transaction = database.begin()?;
        transaction.create_table(scope_schema())?;
        transaction.create_table(user_schema())?;
        transaction.insert(
            SCOPE,
            vec![
                Value::Integer(1),
                Value::Integer(1),
                Value::Text(project.into()),
                Value::Bytes(dummy.encode().to_vec()),
            ],
        )?;
        transaction.commit()?;
        Ok(Self {
            database,
            project: project.into(),
            pool,
            dummy,
            session_scope: None,
            session_clock: None,
        })
    }

    pub fn open(path: impl AsRef<Path>, project: &str, pool: PasswordPool) -> Result<Self> {
        if !valid_project_id(project) {
            return Err(Error::Scope);
        }
        let database = Database::open(path)?;
        let snapshot = database.view()?;
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
        Ok(Self {
            database,
            project: project.into(),
            pool,
            dummy,
            session_scope,
            session_clock,
        })
    }

    pub fn project(&self) -> &str {
        &self.project
    }

    pub fn count(&self) -> Result<usize> {
        let mut count = 0;
        for row in self
            .database
            .view()?
            .primary_rows(USERS, None, None)
            .map_err(|_| Error::Corrupt)?
        {
            row.map_err(|_| Error::Corrupt)?;
            count += 1;
            if count > MAX_ACCOUNTS {
                return Err(Error::Corrupt);
            }
        }
        Ok(count)
    }

    fn record(&self, login: &str) -> Result<Option<Record>> {
        validate_login(login)?;
        self.database
            .view()?
            .get(USERS, &Key::Text(login.into()))
            .map_err(|_| Error::Corrupt)?
            .map(|row| Record::decode(row))
            .transpose()
    }

    /// Trusted local provisioning; this is not public self-service signup.
    pub fn create_user(&mut self, login: &str, password: &[u8]) -> Result<AccountInfo> {
        validate_login(login)?;
        if self.record(login)?.is_some() {
            return Err(Error::Exists);
        }
        if self.count()? >= MAX_ACCOUNTS {
            return Err(Error::Capacity);
        }
        let digest = self.pool.hash(password)?;
        let mut identities = std::collections::BTreeSet::new();
        for row in self
            .database
            .view()?
            .primary_rows(USERS, None, None)
            .map_err(|_| Error::Corrupt)?
        {
            identities.insert(Record::decode(row.map_err(|_| Error::Corrupt)?)?.info.id);
        }
        let mut id = [0; 16];
        let mut distinct = false;
        for _ in 0..4 {
            getrandom::fill(&mut id).map_err(|_| Error::Randomness)?;
            if !identities.contains(&id) {
                distinct = true;
                break;
            }
        }
        if !distinct {
            return Err(Error::Randomness);
        }
        let record = Record {
            info: AccountInfo {
                id,
                login: login.into(),
                credential_epoch: 1,
                disabled: false,
            },
            digest,
        };
        let mut transaction = self.database.begin()?;
        transaction.insert(USERS, record.encode())?;
        transaction.commit()?;
        Ok(record.info)
    }

    /// Return current metadata after password verification. No session is issued.
    /// Missing/disabled logins still run the fixed-policy KDF; total flow timing
    /// and HTTP enumeration resistance are not established by this library.
    pub fn check_password(&self, login: &str, password: &[u8]) -> Result<Option<AccountInfo>> {
        let record = self.record(login)?;
        let digest = record.as_ref().map_or(&self.dummy, |r| &r.digest);
        let correct = self.pool.verify(password, digest)?;
        Ok(record
            .filter(|r| correct && !r.info.disabled)
            .map(|r| r.info))
    }

    pub fn change_password(
        &mut self,
        login: &str,
        current: &[u8],
        replacement: &[u8],
    ) -> Result<AccountInfo> {
        if replacement.is_empty() || replacement.len() > MAX_PASSWORD_BYTES {
            return Err(PasswordError::Input.into());
        }
        let info = self.check_password(login, current)?.ok_or(Error::Denied)?;
        let epoch = info
            .credential_epoch
            .checked_add(1)
            .filter(|e| *e <= i64::MAX as u64)
            .ok_or(Error::Epoch)?;
        let digest = self.pool.hash(replacement)?;
        let record = Record {
            info: AccountInfo {
                credential_epoch: epoch,
                ..info
            },
            digest,
        };
        self.replace(record)
    }

    /// Trusted local administrative operation. Epoch changes invalidate future
    /// epoch-bound sessions only once that separate session layer is implemented.
    pub fn set_disabled(&mut self, login: &str, disabled: bool) -> Result<AccountInfo> {
        let mut record = self.record(login)?.ok_or(Error::Denied)?;
        if record.info.disabled == disabled {
            return Ok(record.info);
        }
        record.info.credential_epoch = record
            .info
            .credential_epoch
            .checked_add(1)
            .filter(|e| *e <= i64::MAX as u64)
            .ok_or(Error::Epoch)?;
        record.info.disabled = disabled;
        self.replace(record)
    }

    fn replace(&mut self, record: Record) -> Result<AccountInfo> {
        let mut transaction = self.database.begin()?;
        transaction.update(
            USERS,
            &Key::Text(record.info.login.clone()),
            record.encode(),
        )?;
        transaction.commit()?;
        Ok(record.info)
    }

    pub fn backup(&mut self, destination: impl AsRef<Path>) -> Result<emilybase_backup::Report> {
        Ok(emilybase_backup::create(&mut self.database, destination)?)
    }

    pub fn compact(&mut self) -> Result<()> {
        self.database.compact()?;
        Ok(())
    }
}
