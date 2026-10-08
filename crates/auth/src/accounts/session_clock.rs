//! Durable time watermark and explicit incarnation reset; no session admission.
use super::session_schema::{FAMILIES, META, family_schema, inspect_session_record, meta_schema};
use super::{AccountStore, Error, Result, SCOPE};
use crate::tokens::TokenScope;
use emilybase_catalog::{DataType, Key, Schema, Value};
use emilybase_database::Snapshot;

pub(super) const CLOCK: &str = "auth_session_clock";
pub(super) fn clock_schema() -> Schema {
    super::records::schema(
        CLOCK,
        &[
            ("id", DataType::Integer),
            ("version", DataType::Integer),
            ("observed", DataType::Integer),
        ],
    )
}
pub(super) fn validate_clock(snapshot: &Snapshot, version: i64) -> Result<Option<u64>> {
    if version < 3 {
        return Ok(None);
    }
    if snapshot.schema(CLOCK).map_err(|_| Error::Corrupt)? != &clock_schema() {
        return Err(Error::Corrupt);
    }
    let mut rows = snapshot
        .primary_rows(CLOCK, None, None)
        .map_err(|_| Error::Corrupt)?;
    let row = rows
        .next()
        .transpose()
        .map_err(|_| Error::Corrupt)?
        .ok_or(Error::Corrupt)?;
    if rows.next().is_some() {
        return Err(Error::Corrupt);
    }
    Ok(Some(inspect_session_clock_record(row)?))
}
/// Fixed-size metadata inspection only; no persisted clock or permission changes.
pub fn inspect_session_clock_record(row: &[Value]) -> Result<u64> {
    let [
        Value::Integer(1),
        Value::Integer(1),
        Value::Integer(observed),
    ] = row
    else {
        return Err(Error::Corrupt);
    };
    if *observed < 0 {
        return Err(Error::Corrupt);
    }
    Ok(*observed as u64)
}
fn timestamp(now: u64) -> Result<i64> {
    i64::try_from(now).map_err(|_| Error::Clock)
}
fn clock_row(now: i64) -> Vec<Value> {
    vec![Value::Integer(1), Value::Integer(1), Value::Integer(now)]
}
fn meta_row(incarnation: [u8; 16]) -> Vec<Value> {
    vec![
        Value::Integer(1),
        Value::Integer(1),
        Value::Bytes(incarnation.to_vec()),
    ]
}

impl AccountStore {
    fn unique_incarnation(
        &self,
        mut fill: impl FnMut(&mut [u8; 16]) -> Result<()>,
    ) -> Result<([u8; 16], TokenScope)> {
        let mut history = std::collections::BTreeSet::new();
        if self.session_scope.is_some() {
            for row in self
                .database
                .view()?
                .primary_rows(FAMILIES, None, None)
                .map_err(|_| Error::Corrupt)?
            {
                let info = inspect_session_record(row.map_err(|_| Error::Corrupt)?, &self.project)?;
                history.insert(info.incarnation);
                if history.len() > super::MAX_SESSION_FAMILIES {
                    return Err(Error::Corrupt);
                }
            }
        }
        for _ in 0..4 {
            let mut incarnation = [0; 16];
            fill(&mut incarnation)?;
            let scope = TokenScope::new(&self.project, incarnation).map_err(|_| Error::Corrupt)?;
            if self.session_scope.as_ref() != Some(&scope) && !history.contains(&incarnation) {
                return Ok((incarnation, scope));
            }
        }
        Err(Error::Randomness)
    }
    fn fresh_incarnation(&self) -> Result<([u8; 16], TokenScope)> {
        self.unique_incarnation(|bytes| getrandom::fill(bytes).map_err(|_| Error::Randomness))
    }

    /// Explicit private v1/v2 -> v3 migration. Caller supplies a trusted service
    /// timestamp, never a client parameter. Activation changes incarnation so
    /// historical v2 records cannot gain future authority under this new clock.
    pub fn enable_session_clock(&mut self, now: u64) -> Result<TokenScope> {
        self.database.view()?;
        let time = timestamp(now)?;
        if self.session_clock.is_some() {
            self.advance_session_clock(now)?;
            return self.session_scope.clone().ok_or(Error::Corrupt);
        }
        let (incarnation, scope) = self.fresh_incarnation()?;
        let existing = self.session_scope.is_some();
        let mut tx = self.database.begin()?;
        if !existing {
            tx.create_table(meta_schema())?;
            tx.create_table(family_schema())?;
            tx.insert(META, meta_row(incarnation))?;
        } else {
            tx.update(META, &Key::Integer(1), meta_row(incarnation))?;
        }
        tx.create_table(clock_schema())?;
        tx.insert(CLOCK, clock_row(time))?;
        tx.update(
            SCOPE,
            &Key::Integer(1),
            vec![
                Value::Integer(1),
                Value::Integer(3),
                Value::Text(self.project.clone()),
                Value::Bytes(self.dummy.encode().to_vec()),
            ],
        )?;
        tx.commit()?;
        self.session_scope = Some(scope.clone());
        self.session_clock = Some(now);
        Ok(scope)
    }
    /// Persist a nondecreasing trusted timestamp before future session checks.
    /// A lower time is refused even after restart. Equal time changes no history.
    pub fn advance_session_clock(&mut self, now: u64) -> Result<()> {
        self.database.view()?;
        let time = timestamp(now)?;
        let floor = self.session_clock.ok_or(Error::ClockDisabled)?;
        if now < floor {
            return Err(Error::Clock);
        }
        if now == floor {
            return Ok(());
        }
        let mut tx = self.database.begin()?;
        tx.update(CLOCK, &Key::Integer(1), clock_row(time))?;
        tx.commit()?;
        self.session_clock = Some(now);
        Ok(())
    }
    pub fn session_clock_floor(&self) -> Result<Option<u64>> {
        self.database.view()?;
        Ok(self.session_clock)
    }
    /// Trusted operator reset/restore step: replace incarnation and time together.
    /// Historical incarnations remain excluded; this is metadata invalidation,
    /// not an implemented logout/authorization API or coordinated platform restore.
    pub fn reset_session_clock(&mut self, now: u64) -> Result<TokenScope> {
        self.database.view()?;
        let time = timestamp(now)?;
        self.session_clock.ok_or(Error::ClockDisabled)?;
        let (incarnation, scope) = self.fresh_incarnation()?;
        let mut tx = self.database.begin()?;
        tx.update(META, &Key::Integer(1), meta_row(incarnation))?;
        tx.update(CLOCK, &Key::Integer(1), clock_row(time))?;
        tx.commit()?;
        self.session_scope = Some(scope.clone());
        self.session_clock = Some(now);
        Ok(scope)
    }
}

#[cfg(test)]
pub(super) fn choose_for_test(
    store: &AccountStore,
    candidates: &[[u8; 16]],
) -> Result<([u8; 16], TokenScope)> {
    let mut next = candidates.iter();
    store.unique_incarnation(|bytes| {
        *bytes = *next.next().ok_or(Error::Randomness)?;
        Ok(())
    })
}
