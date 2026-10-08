//! Explicit private v4 catalog over bounded original typed policy groups.
use super::{AccountStore, Error, Result, SCOPE, SessionPrincipal};
use crate::row_policy::{
    Change, PolicyError, TableContext,
    records::{self, DecodedPolicy},
};
use emilybase_catalog::{Key, Value};
use emilybase_database::Snapshot;
use serde::Serialize;

pub const MAX_ROW_POLICIES: usize = 128;
#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct PolicyReceipt {
    pub table: u64,
    pub revision: u64,
    pub previous: u64,
    pub sha256: [u8; 32],
}
impl std::fmt::Debug for PolicyReceipt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PolicyReceipt(redacted)")
    }
}
impl From<&DecodedPolicy> for PolicyReceipt {
    fn from(value: &DecodedPolicy) -> Self {
        Self {
            table: value.table,
            revision: value.revision,
            previous: value.previous,
            sha256: value.sha256,
        }
    }
}
/// Current session and installed policy borrowed from one private owner.
/// No Clone, serialization, model extraction or detached storage authority.
pub struct PolicyPrincipal<'store> {
    principal: SessionPrincipal<'store>,
    policy: DecodedPolicy,
}
impl std::fmt::Debug for PolicyPrincipal<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PolicyPrincipal(redacted)")
    }
}
impl PolicyPrincipal<'_> {
    pub fn authorize(
        &self,
        context: TableContext<'_>,
        change: Change<'_>,
    ) -> std::result::Result<(), PolicyError> {
        self.policy
            .policy
            .authorize(context, &self.principal, change)
    }
    pub fn receipt(&self) -> PolicyReceipt {
        PolicyReceipt::from(&self.policy)
    }
}
fn version(snapshot: &Snapshot) -> Result<i64> {
    let row = snapshot
        .get(SCOPE, &Key::Integer(1))
        .map_err(|_| Error::Corrupt)?
        .ok_or(Error::Corrupt)?;
    match row.get(1) {
        Some(Value::Integer(version)) => Ok(*version),
        _ => Err(Error::Corrupt),
    }
}
fn enabled(snapshot: &Snapshot) -> Result<()> {
    if version(snapshot)? != 4 {
        return Err(Error::PolicySchema);
    }
    Ok(())
}
fn load(snapshot: &Snapshot, project: &str, table: u64) -> Result<Option<DecodedPolicy>> {
    let name = records::header_schema().name;
    let chunks_name = records::chunk_schema().name;
    let Some(header) = snapshot
        .get(&name, &Key::Text(table.to_string()))
        .map_err(|_| Error::Corrupt)?
    else {
        return Ok(None);
    };
    let mut chunks = Vec::with_capacity(records::MAX_POLICY_CHUNKS);
    for index in 0..records::MAX_POLICY_CHUNKS {
        if let Some(row) = snapshot
            .get(&chunks_name, &Key::Text(format!("{table}:{index}")))
            .map_err(|_| Error::Corrupt)?
        {
            chunks.push(row.as_slice());
        } else {
            break;
        }
    }
    records::inspect(project, header, chunks)
        .map(Some)
        .map_err(|_| Error::Corrupt)
}
/// Complete bounded inventory validation, including unreferenced chunks and LSNs.
pub(super) fn validate(
    snapshot: &Snapshot,
    project: &str,
    last_transaction: u64,
) -> Result<Vec<PolicyReceipt>> {
    let header_schema = records::header_schema();
    let chunk_schema = records::chunk_schema();
    if snapshot.table_count() != 7
        || snapshot
            .schema(&header_schema.name)
            .map_err(|_| Error::Corrupt)?
            != &header_schema
        || snapshot
            .schema(&chunk_schema.name)
            .map_err(|_| Error::Corrupt)?
            != &chunk_schema
    {
        return Err(Error::Corrupt);
    }
    let mut receipts = Vec::new();
    let mut expected_chunks = 0usize;
    for row in snapshot
        .primary_rows(&header_schema.name, None, None)
        .map_err(|_| Error::Corrupt)?
    {
        let row = row.map_err(|_| Error::Corrupt)?;
        if receipts.len() >= MAX_ROW_POLICIES {
            return Err(Error::Corrupt);
        }
        let Some(Value::Text(key)) = row.first() else {
            return Err(Error::Corrupt);
        };
        let table = key.parse::<u64>().map_err(|_| Error::Corrupt)?;
        if table == 0 || key != &table.to_string() {
            return Err(Error::Corrupt);
        }
        let decoded = load(snapshot, project, table)?.ok_or(Error::Corrupt)?;
        if decoded.revision > last_transaction {
            return Err(Error::Corrupt);
        }
        let (Value::Integer(schema_len), Value::Integer(document_len)) = (&row[5], &row[6]) else {
            return Err(Error::Corrupt);
        };
        // inspect already checked both lengths and their bounded sum.
        expected_chunks += (*schema_len as usize + *document_len as usize)
            .div_ceil(emilybase_catalog::MAX_VALUE_BYTES);
        receipts.push(PolicyReceipt::from(&decoded));
    }
    let mut actual_chunks = 0usize;
    for row in snapshot
        .primary_rows(&chunk_schema.name, None, None)
        .map_err(|_| Error::Corrupt)?
    {
        row.map_err(|_| Error::Corrupt)?;
        actual_chunks += 1;
        if actual_chunks > MAX_ROW_POLICIES * records::MAX_POLICY_CHUNKS {
            return Err(Error::Corrupt);
        }
    }
    if actual_chunks != expected_chunks {
        return Err(Error::Corrupt);
    }
    receipts.sort_by_key(|value| value.table);
    Ok(receipts)
}
impl AccountStore {
    /// Explicit v3-to-v4 migration. Does not observe time or revoke sessions.
    pub fn enable_row_policy_catalog(&mut self) -> Result<()> {
        self.enable_row_policy_catalog_with(|| {})
    }
    pub(super) fn enable_row_policy_catalog_with(
        &mut self,
        before_commit: impl FnOnce(),
    ) -> Result<()> {
        let state = super::archive::validate_snapshot(
            self.database.view()?,
            &self.project,
            self.database.last_transaction(),
        )?;
        if state.version == 4 {
            return Ok(());
        }
        if state.version != 3 {
            return Err(Error::PolicySchema);
        }
        let mut scope = self
            .database
            .view()?
            .get(SCOPE, &Key::Integer(1))
            .map_err(|_| Error::Corrupt)?
            .ok_or(Error::Corrupt)?
            .clone();
        scope[1] = Value::Integer(4);
        let mut tx = self.database.begin()?;
        tx.create_table(records::header_schema())?;
        tx.create_table(records::chunk_schema())?;
        tx.update(SCOPE, &Key::Integer(1), scope)?;
        before_commit();
        tx.commit()?;
        Ok(())
    }
    pub fn row_policy_receipts(&self) -> Result<Vec<PolicyReceipt>> {
        enabled(self.database.view()?)?;
        validate(
            self.database.view()?,
            &self.project,
            self.database.last_transaction(),
        )
    }
    /// Trusted context/expected revision, exact original bytes; no automatic retry.
    pub fn install_row_policy(
        &mut self,
        context: TableContext<'_>,
        expected: u64,
        document: &[u8],
    ) -> Result<PolicyReceipt> {
        self.install_row_policy_with(context, expected, document, || {})
    }
    pub(super) fn install_row_policy_with(
        &mut self,
        context: TableContext<'_>,
        expected: u64,
        document: &[u8],
        before_commit: impl FnOnce(),
    ) -> Result<PolicyReceipt> {
        enabled(self.database.view()?)?;
        if context.project != self.project {
            return Err(Error::ScopeMismatch);
        }
        let inventory = validate(
            self.database.view()?,
            &self.project,
            self.database.last_transaction(),
        )?;
        let previous = load(self.database.view()?, &self.project, context.id)?;
        if let Some(prior) = &previous {
            if prior.schema == *context.schema
                && prior.document() == document
                && (expected == prior.revision || expected == prior.previous)
            {
                return Ok(PolicyReceipt::from(prior));
            }
            if expected != prior.revision {
                return Err(Error::PolicyConflict);
            }
        } else if expected != 0 {
            return Err(Error::PolicyConflict);
        } else if inventory.len() >= MAX_ROW_POLICIES {
            return Err(Error::PolicyCapacity);
        }
        let revision = self
            .database
            .last_transaction()
            .checked_add(1)
            .ok_or(Error::PolicyCapacity)?;
        let predecessor = previous.as_ref().map_or(0, |value| value.revision);
        let encoded =
            records::encode(context, revision, predecessor, document).map_err(Error::Policy)?;
        let decoded = records::inspect(&self.project, encoded.header(), encoded.chunks())
            .map_err(Error::Policy)?;
        let receipt = PolicyReceipt::from(&decoded);
        let chunks_name = records::chunk_schema().name;
        let header_name = records::header_schema().name;
        let mut tx = self.database.begin()?;
        if previous.is_some() {
            for index in 0..records::MAX_POLICY_CHUNKS {
                let key = Key::Text(format!("{}:{index}", context.id));
                if tx
                    .view()?
                    .get(&chunks_name, &key)
                    .map_err(|_| Error::Corrupt)?
                    .is_some()
                {
                    tx.delete(&chunks_name, &key)?;
                }
            }
            tx.update(
                &header_name,
                &Key::Text(context.id.to_string()),
                encoded.header().to_vec(),
            )?;
        } else {
            tx.insert(&header_name, encoded.header().to_vec())?;
        }
        for row in encoded.chunks() {
            tx.insert(&chunks_name, row.to_vec())?;
        }
        before_commit();
        tx.commit()?;
        Ok(receipt)
    }
    /// Current installed policy and current session proof share this owner borrow.
    /// The private owner cannot change credentials/policies while this proof lives.
    /// ```compile_fail
    /// use emilybase_auth::{accounts::AccountStore,row_policy::TableContext};
    /// fn replace(store:&mut AccountStore,context:TableContext<'_>) {
    ///     let proof=store.verify_row_policy_access("synthetic",100,context.id).unwrap();
    ///     let _=store.install_row_policy(context,0,b"synthetic");
    ///     let _=proof.receipt();
    /// }
    /// ```
    pub fn verify_row_policy_access(
        &mut self,
        token: &str,
        now: u64,
        table: u64,
    ) -> Result<PolicyPrincipal<'_>> {
        enabled(self.database.view()?)?;
        validate(
            self.database.view()?,
            &self.project,
            self.database.last_transaction(),
        )?;
        let policy = load(self.database.view()?, &self.project, table)?;
        let principal = self.verify_access(token, now)?;
        Ok(PolicyPrincipal {
            principal,
            policy: policy.ok_or(Error::PolicyDenied)?,
        })
    }
}
