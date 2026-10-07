//! Explicit schema migration and strict records; session authorization is pending.
use super::{AccountStore, Error, Record, Result, SCOPE, USERS};
use crate::tokens::{TokenDigest, TokenKind, TokenScope};
use emilybase_catalog::{DataType, Key, Schema, Value};
use emilybase_database::Snapshot;

pub const MAX_SESSION_FAMILIES: usize = 4096;
pub(super) const META: &str = "auth_sessions_meta";
pub(super) const FAMILIES: &str = "auth_sessions";
pub(super) const ACCESS_SECONDS: i64 = 900;
pub(super) const REFRESH_SECONDS: i64 = 604800;
pub(super) const ABSOLUTE_SECONDS: i64 = 2592000;

pub(super) fn meta_schema() -> Schema {
    super::records::schema(
        META,
        &[
            ("id", DataType::Integer),
            ("version", DataType::Integer),
            ("incarnation", DataType::Bytes),
        ],
    )
}
pub(super) fn family_schema() -> Schema {
    super::records::schema(
        FAMILIES,
        &[
            ("family", DataType::Text),
            ("incarnation", DataType::Bytes),
            ("login", DataType::Text),
            ("user", DataType::Bytes),
            ("epoch", DataType::Integer),
            ("generation", DataType::Integer),
            ("created", DataType::Integer),
            ("issued", DataType::Integer),
            ("access_until", DataType::Integer),
            ("refresh_until", DataType::Integer),
            ("absolute_until", DataType::Integer),
            ("access", DataType::Bytes),
            ("refresh", DataType::Bytes),
            ("revoked", DataType::Boolean),
        ],
    )
}

/// Untrusted metadata inspection, not a session principal or permission.
#[derive(Clone, PartialEq, Eq)]
pub struct SessionRecordInfo {
    pub family: [u8; 16],
    pub incarnation: [u8; 16],
    pub login: String,
    pub user: [u8; 16],
    pub credential_epoch: u64,
    pub generation: u64,
    pub created: u64,
    pub issued: u64,
    pub access_until: u64,
    pub refresh_until: u64,
    pub absolute_until: u64,
    pub revoked: bool,
}
impl std::fmt::Debug for SessionRecordInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionRecordInfo")
            .field("identity", &"redacted")
            .field("generation", &self.generation)
            .field("revoked", &self.revoked)
            .finish()
    }
}
fn unhex_family(text: &str) -> Result<[u8; 16]> {
    if !crate::valid_project_id(text) {
        return Err(Error::Corrupt);
    }
    let mut bytes = [0; 16];
    for (i, pair) in text.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        let digit = |byte| {
            if byte <= b'9' {
                byte - b'0'
            } else {
                byte - b'a' + 10
            }
        };
        bytes[i] = digit(pair[0]) * 16 + digit(pair[1]);
    }
    Ok(bytes)
}
fn expiry(issued: i64, absolute: i64, ttl: i64) -> Option<i64> {
    let remaining = absolute.checked_sub(issued)?;
    if remaining <= 0 {
        return None;
    }
    issued.checked_add(remaining.min(ttl))
}
/// Pure bounded validation; no storage, password/token verification or authority.
pub fn inspect_session_record(row: &[Value], project: &str) -> Result<SessionRecordInfo> {
    let [
        Value::Text(family),
        Value::Bytes(incarnation),
        Value::Text(login),
        Value::Bytes(user),
        Value::Integer(epoch),
        Value::Integer(generation),
        Value::Integer(created),
        Value::Integer(issued),
        Value::Integer(access_until),
        Value::Integer(refresh_until),
        Value::Integer(absolute_until),
        Value::Bytes(access),
        Value::Bytes(refresh),
        Value::Boolean(revoked),
    ] = row
    else {
        return Err(Error::Corrupt);
    };
    super::records::validate_login(login).map_err(|_| Error::Corrupt)?;
    let family = unhex_family(family)?;
    let incarnation: [u8; 16] = incarnation
        .as_slice()
        .try_into()
        .map_err(|_| Error::Corrupt)?;
    let user = user.as_slice().try_into().map_err(|_| Error::Corrupt)?;
    let scope = TokenScope::new(project, incarnation).map_err(|_| Error::Corrupt)?;
    if *epoch <= 0
        || *generation <= 0
        || *created < 0
        || *issued < *created
        || created.checked_add(ABSOLUTE_SECONDS) != Some(*absolute_until)
        || *issued >= *absolute_until
        || expiry(*issued, *absolute_until, ACCESS_SECONDS) != Some(*access_until)
        || expiry(*issued, *absolute_until, REFRESH_SECONDS) != Some(*refresh_until)
    {
        return Err(Error::Corrupt);
    }
    let access = TokenDigest::decode(access).map_err(|_| Error::Corrupt)?;
    let refresh = TokenDigest::decode(refresh).map_err(|_| Error::Corrupt)?;
    if !access.belongs_to(TokenKind::Access, &scope, &family)
        || !refresh.belongs_to(TokenKind::Refresh, &scope, &family)
    {
        return Err(Error::Corrupt);
    }
    Ok(SessionRecordInfo {
        family,
        incarnation,
        login: login.clone(),
        user,
        credential_epoch: *epoch as u64,
        generation: *generation as u64,
        created: *created as u64,
        issued: *issued as u64,
        access_until: *access_until as u64,
        refresh_until: *refresh_until as u64,
        absolute_until: *absolute_until as u64,
        revoked: *revoked,
    })
}
pub(super) fn validate_inventory(
    snapshot: &Snapshot,
    project: &str,
    version: i64,
) -> Result<Option<TokenScope>> {
    if version == 1 {
        return if snapshot.table_count() == 2 {
            Ok(None)
        } else {
            Err(Error::Corrupt)
        };
    }
    if version != 2
        || snapshot.table_count() != 4
        || snapshot.schema(META).map_err(|_| Error::Corrupt)? != &meta_schema()
        || snapshot.schema(FAMILIES).map_err(|_| Error::Corrupt)? != &family_schema()
    {
        return Err(Error::Corrupt);
    }
    let mut rows = snapshot
        .primary_rows(META, None, None)
        .map_err(|_| Error::Corrupt)?;
    let row = rows
        .next()
        .transpose()
        .map_err(|_| Error::Corrupt)?
        .ok_or(Error::Corrupt)?;
    let [
        Value::Integer(1),
        Value::Integer(1),
        Value::Bytes(incarnation),
    ] = row.as_slice()
    else {
        return Err(Error::Corrupt);
    };
    if rows.next().is_some() {
        return Err(Error::Corrupt);
    }
    let incarnation = incarnation
        .as_slice()
        .try_into()
        .map_err(|_| Error::Corrupt)?;
    let scope = TokenScope::new(project, incarnation).map_err(|_| Error::Corrupt)?;
    let mut count = 0;
    for row in snapshot
        .primary_rows(FAMILIES, None, None)
        .map_err(|_| Error::Corrupt)?
    {
        count += 1;
        if count > MAX_SESSION_FAMILIES {
            return Err(Error::Corrupt);
        }
        let family = inspect_session_record(row.map_err(|_| Error::Corrupt)?, project)?;
        let account = snapshot
            .get(USERS, &Key::Text(family.login.clone()))
            .map_err(|_| Error::Corrupt)?
            .ok_or(Error::Corrupt)?;
        let account = Record::decode(account)?;
        // Older epochs/incarnations may remain as invalidated history; future
        // admission must independently check current state on every request.
        if account.info.id != family.user || family.credential_epoch > account.info.credential_epoch
        {
            return Err(Error::Corrupt);
        }
    }
    Ok(Some(scope))
}

impl AccountStore {
    /// Explicit local migration, not sign-in. Both tables/meta/version commit in
    /// one mandatory-WAL transaction; unchanged v1 stores remain supported.
    pub fn enable_session_storage(&mut self) -> Result<TokenScope> {
        self.database.view()?;
        if let Some(scope) = &self.session_scope {
            return Ok(scope.clone());
        }
        let mut incarnation = [0; 16];
        getrandom::fill(&mut incarnation).map_err(|_| Error::Randomness)?;
        let scope = TokenScope::new(&self.project, incarnation).map_err(|_| Error::Corrupt)?;
        let mut transaction = self.database.begin()?;
        transaction.create_table(meta_schema())?;
        transaction.create_table(family_schema())?;
        transaction.insert(
            META,
            vec![
                Value::Integer(1),
                Value::Integer(1),
                Value::Bytes(incarnation.to_vec()),
            ],
        )?;
        transaction.update(
            SCOPE,
            &Key::Integer(1),
            vec![
                Value::Integer(1),
                Value::Integer(2),
                Value::Text(self.project.clone()),
                Value::Bytes(self.dummy.encode().to_vec()),
            ],
        )?;
        transaction.commit()?;
        self.session_scope = Some(scope.clone());
        Ok(scope)
    }
    /// Current persisted metadata, never an authorization capability.
    pub fn session_storage_scope(&self) -> Result<Option<TokenScope>> {
        self.database.view()?;
        Ok(self.session_scope.clone())
    }
}
