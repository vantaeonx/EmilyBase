//! Native synchronous lifecycle over the private original-WAL store, not HTTP.
use super::session_schema::{
    ABSOLUTE_SECONDS, ACCESS_SECONDS, FAMILIES, REFRESH_SECONDS, inspect_session_record,
};
use super::{AccountInfo, AccountStore, Error, MAX_SESSION_FAMILIES, Result, SessionRecordInfo};
use crate::tokens::{IssuedToken, TokenDigest, TokenKind, TokenScope, issue, metadata};
use emilybase_catalog::{Key, Row, Value};

/// Plaintext owners plus lifecycle metadata; no implicit serialization or Clone.
pub struct IssuedSession {
    pub access: IssuedToken,
    pub refresh: IssuedToken,
    pub metadata: SessionRecordInfo,
}
impl std::fmt::Debug for IssuedSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("IssuedSession(redacted)")
    }
}
/// Current single-request proof. Construction is private, no Clone/serialization.
/// Borrowing the owner prevents its credential state changing while this proof lives.
pub struct SessionPrincipal<'store> {
    account: AccountInfo,
    project: &'store str,
    family: [u8; 16],
    generation: u64,
}
impl std::fmt::Debug for SessionPrincipal<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SessionPrincipal(redacted)")
    }
}
impl SessionPrincipal<'_> {
    pub fn account(&self) -> &AccountInfo {
        &self.account
    }
    pub fn project(&self) -> &str {
        self.project
    }
    pub fn family(&self) -> &[u8; 16] {
        &self.family
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
}
struct Family {
    info: SessionRecordInfo,
    access: TokenDigest,
    refresh: TokenDigest,
}
fn hex(bytes: &[u8; 16]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(32);
    for b in bytes {
        text.push(HEX[usize::from(b >> 4)] as char);
        text.push(HEX[usize::from(b & 15)] as char);
    }
    text
}
fn end(now: u64, absolute: u64, seconds: i64) -> u64 {
    now + (seconds as u64).min(absolute - now)
}
impl Family {
    fn decode(row: &[Value], project: &str) -> Result<Self> {
        let info = inspect_session_record(row, project)?;
        let (Value::Bytes(access), Value::Bytes(refresh)) = (&row[11], &row[12]) else {
            return Err(Error::Corrupt);
        };
        Ok(Self {
            info,
            access: TokenDigest::decode(access)?,
            refresh: TokenDigest::decode(refresh)?,
        })
    }
    fn encode(&self) -> Row {
        let i = &self.info;
        vec![
            Value::Text(hex(&i.family)),
            Value::Bytes(i.incarnation.to_vec()),
            Value::Text(i.login.clone()),
            Value::Bytes(i.user.to_vec()),
            Value::Integer(i.credential_epoch as i64),
            Value::Integer(i.generation as i64),
            Value::Integer(i.created as i64),
            Value::Integer(i.issued as i64),
            Value::Integer(i.access_until as i64),
            Value::Integer(i.refresh_until as i64),
            Value::Integer(i.absolute_until as i64),
            Value::Bytes(self.access.encode().to_vec()),
            Value::Bytes(self.refresh.encode().to_vec()),
            Value::Boolean(i.revoked),
        ]
    }
}
impl AccountStore {
    fn current_session_scope(&self) -> Result<TokenScope> {
        self.database.view()?;
        self.session_clock.ok_or(Error::ClockDisabled)?;
        self.session_scope.clone().ok_or(Error::Corrupt)
    }
    fn family(&self, id: &[u8; 16]) -> Result<Option<Family>> {
        self.database
            .view()?
            .get(FAMILIES, &Key::Text(hex(id)))
            .map_err(|_| Error::Corrupt)?
            .map(|r| Family::decode(r, &self.project))
            .transpose()
    }
    pub fn session_family_count(&self) -> Result<usize> {
        self.current_session_scope()?;
        let mut count = 0;
        for row in self
            .database
            .view()?
            .primary_rows(FAMILIES, None, None)
            .map_err(|_| Error::Corrupt)?
        {
            row.map_err(|_| Error::Corrupt)?;
            count += 1;
            if count > MAX_SESSION_FAMILIES {
                return Err(Error::Corrupt);
            }
        }
        Ok(count)
    }
    fn active_account(&self, family: &Family, now: u64, scope: &TokenScope) -> Result<AccountInfo> {
        let i = &family.info;
        if i.revoked
            || now < i.issued
            || now >= i.absolute_until
            || &TokenScope::new(&self.project, i.incarnation)? != scope
        {
            return Err(Error::Denied);
        }
        let account = self.record(&i.login)?.ok_or(Error::Denied)?.info;
        if account.disabled
            || account.id != i.user
            || account.credential_epoch != i.credential_epoch
        {
            return Err(Error::Denied);
        }
        Ok(account)
    }
    fn replace_family(&mut self, family: &Family) -> Result<()> {
        let mut tx = self.database.begin()?;
        tx.update(
            FAMILIES,
            &Key::Text(hex(&family.info.family)),
            family.encode(),
        )?;
        tx.commit()?;
        Ok(())
    }
    fn issue_pair(
        &self,
        mut info: SessionRecordInfo,
        now: u64,
        scope: &TokenScope,
    ) -> Result<(Family, IssuedSession)> {
        info.issued = now;
        info.access_until = end(now, info.absolute_until, ACCESS_SECONDS);
        info.refresh_until = end(now, info.absolute_until, REFRESH_SECONDS);
        let (access, access_digest) = issue(TokenKind::Access, scope, info.family)?;
        let (refresh, refresh_digest) = issue(TokenKind::Refresh, scope, info.family)?;
        Ok((
            Family {
                info: info.clone(),
                access: access_digest,
                refresh: refresh_digest,
            },
            IssuedSession {
                access,
                refresh,
                metadata: info,
            },
        ))
    }

    /// Trusted service time only. Observe it before checking any supplied credential.
    /// Issued secrets are returned only after their family row is durably committed.
    pub fn sign_in(&mut self, login: &str, password: &[u8], now: u64) -> Result<IssuedSession> {
        let absolute = now
            .checked_add(ABSOLUTE_SECONDS as u64)
            .filter(|t| *t <= i64::MAX as u64)
            .ok_or(Error::Clock)?;
        self.advance_session_clock(now)?;
        let scope = self.current_session_scope()?;
        if self.session_family_count()? == MAX_SESSION_FAMILIES {
            return Err(Error::SessionCapacity);
        }
        let account = self.check_password(login, password)?.ok_or(Error::Denied)?;
        let mut family_id = [0; 16];
        let mut distinct = false;
        for _ in 0..4 {
            getrandom::fill(&mut family_id).map_err(|_| Error::Randomness)?;
            if self.family(&family_id)?.is_none() {
                distinct = true;
                break;
            }
        }
        if !distinct {
            return Err(Error::Randomness);
        }
        let incarnation = {
            let row = self
                .database
                .view()?
                .get(super::session_schema::META, &Key::Integer(1))
                .map_err(|_| Error::Corrupt)?
                .ok_or(Error::Corrupt)?;
            let Value::Bytes(bytes) = &row[2] else {
                return Err(Error::Corrupt);
            };
            bytes.as_slice().try_into().map_err(|_| Error::Corrupt)?
        };
        let info = SessionRecordInfo {
            family: family_id,
            incarnation,
            login: account.login,
            user: account.id,
            credential_epoch: account.credential_epoch,
            generation: 1,
            created: now,
            issued: now,
            access_until: now,
            refresh_until: now,
            absolute_until: absolute,
            revoked: false,
        };
        let (family, tokens) = self.issue_pair(info, now, &scope)?;
        let mut tx = self.database.begin()?;
        tx.insert(FAMILIES, family.encode())?;
        tx.commit()?;
        Ok(tokens)
    }

    /// A borrowed one-request principal; future server code must additionally
    /// enforce project capability, roles and row policies before data access.
    pub fn verify_access(&mut self, text: &str, now: u64) -> Result<SessionPrincipal<'_>> {
        self.advance_session_clock(now)?;
        let scope = self.current_session_scope()?;
        let meta = metadata(text)?;
        if meta.kind != TokenKind::Access {
            return Err(Error::Denied);
        }
        let family = self.family(&meta.family_id)?.ok_or(Error::Denied)?;
        let account = self.active_account(&family, now, &scope)?;
        if now >= family.info.access_until || !family.access.matches(text, &scope)? {
            return Err(Error::Denied);
        }
        Ok(SessionPrincipal {
            account,
            project: &self.project,
            family: family.info.family,
            generation: family.info.generation,
        })
    }
    /// Atomic single-winner replacement: old access and refresh stop working.
    /// Ambiguous storage failure requires reauthentication, not automatic replay.
    pub fn refresh_session(&mut self, text: &str, now: u64) -> Result<IssuedSession> {
        self.advance_session_clock(now)?;
        let scope = self.current_session_scope()?;
        let meta = metadata(text)?;
        if meta.kind != TokenKind::Refresh {
            return Err(Error::Denied);
        }
        let mut family = self.family(&meta.family_id)?.ok_or(Error::Denied)?;
        self.active_account(&family, now, &scope)?;
        if now >= family.info.refresh_until || !family.refresh.matches(text, &scope)? {
            return Err(Error::Denied);
        }
        family.info.generation = family
            .info
            .generation
            .checked_add(1)
            .filter(|g| *g <= i64::MAX as u64)
            .ok_or(Error::Generation)?;
        let (replacement, tokens) = self.issue_pair(family.info, now, &scope)?;
        self.replace_family(&replacement)?;
        Ok(tokens)
    }
    /// Authenticated logout using the currently active refresh credential.
    pub fn logout_session(&mut self, text: &str, now: u64) -> Result<()> {
        self.advance_session_clock(now)?;
        let scope = self.current_session_scope()?;
        let meta = metadata(text)?;
        if meta.kind != TokenKind::Refresh {
            return Err(Error::Denied);
        }
        let mut family = self.family(&meta.family_id)?.ok_or(Error::Denied)?;
        self.active_account(&family, now, &scope)?;
        if now >= family.info.refresh_until || !family.refresh.matches(text, &scope)? {
            return Err(Error::Denied);
        }
        family.info.revoked = true;
        self.replace_family(&family)
    }
    /// Trusted local administration; family metadata alone is not a client credential.
    pub fn revoke_session_family(&mut self, id: &[u8; 16], now: u64) -> Result<()> {
        self.advance_session_clock(now)?;
        let mut family = self.family(id)?.ok_or(Error::Denied)?;
        if family.info.revoked {
            return Ok(());
        }
        family.info.revoked = true;
        self.replace_family(&family)
    }
    /// Delete at most 128 inactive families in one commit; all history still counts
    /// toward capacity until explicitly pruned. Clock observation is separate.
    pub fn prune_session_families(&mut self, now: u64, limit: usize) -> Result<usize> {
        if !(1..=128).contains(&limit) {
            return Err(Error::Cleanup);
        }
        self.advance_session_clock(now)?;
        let scope = self.current_session_scope()?;
        let mut selected = Vec::with_capacity(limit);
        for row in self
            .database
            .view()?
            .primary_rows(FAMILIES, None, None)
            .map_err(|_| Error::Corrupt)?
        {
            let family = Family::decode(row.map_err(|_| Error::Corrupt)?, &self.project)?;
            let inactive = if family.info.revoked || now >= family.info.refresh_until {
                true
            } else {
                match self.active_account(&family, now, &scope) {
                    Ok(_) => false,
                    Err(Error::Denied) => true,
                    Err(error) => return Err(error),
                }
            };
            if inactive {
                selected.push(family.info.family);
                if selected.len() == limit {
                    break;
                }
            }
        }
        if selected.is_empty() {
            return Ok(0);
        }
        let mut tx = self.database.begin()?;
        for id in &selected {
            tx.delete(FAMILIES, &Key::Text(hex(id)))?;
        }
        tx.commit()?;
        Ok(selected.len())
    }
}
