//! Explicit private v6 installed file policies under the original private owner.
use super::{AccountStore, Error, Result, SCOPE, SessionPrincipal};
use crate::file_policy::{
    Change, PolicyError, StoreContext,
    records::{self, DecodedPolicy},
};
use emilybase_catalog::{Key, Value};
use emilybase_database::Snapshot;

pub const MAX_FILE_POLICIES: usize = 128;

/// Trusted coordination at actual native phases, never user authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilePolicyWritePhase {
    BeforeOperation,
    BeforeCommit,
    Committed,
    Verified,
}
type Guard<'a> = Option<&'a mut dyn FnMut(FilePolicyWritePhase) -> Result<()>>;
fn check(guard: &mut Guard<'_>, phase: FilePolicyWritePhase) -> Result<()> {
    if let Some(guard) = guard {
        guard(phase)?;
    }
    Ok(())
}
fn committed_checks(guard: &mut Guard<'_>) -> Result<()> {
    for phase in [
        FilePolicyWritePhase::Committed,
        FilePolicyWritePhase::Verified,
    ] {
        check(guard, phase).map_err(|error| Error::FilePolicyWriteUnknown(Box::new(error)))?;
    }
    Ok(())
}
/// Copied operator observations; this is not an authorization or reservation.
#[derive(Clone, PartialEq, Eq)]
pub struct FilePolicyReceipt {
    pub database: [u8; 16],
    pub revision: u64,
    pub previous: u64,
    pub sha256: [u8; 32],
}
impl std::fmt::Debug for FilePolicyReceipt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FilePolicyReceipt(redacted)")
    }
}
impl From<&DecodedPolicy> for FilePolicyReceipt {
    fn from(value: &DecodedPolicy) -> Self {
        Self {
            database: *value.database(),
            revision: value.revision(),
            previous: value.previous(),
            sha256: *value.sha256(),
        }
    }
}
/// Actual installed data and genuine session share one original private borrow.
/// No public constructor, cloning, serialization or extraction of the model.
pub struct FilePolicyPrincipal<'store> {
    principal: SessionPrincipal<'store>,
    policy: DecodedPolicy,
}
impl std::fmt::Debug for FilePolicyPrincipal<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CurrentFilePolicyPrincipal(redacted)")
    }
}
impl FilePolicyPrincipal<'_> {
    pub fn account_id(&self) -> [u8; 16] {
        self.principal.account().id
    }
    pub fn receipt(&self) -> FilePolicyReceipt {
        FilePolicyReceipt::from(&self.policy)
    }
    /// Check even before missing-reference/empty-result processing.
    pub fn check_context(&self, context: StoreContext<'_>) -> std::result::Result<(), PolicyError> {
        self.policy.check_context(context, &self.principal)
    }
    /// The trusted adapter supplies actual current native ownership and consumes
    /// this decision immediately inside the original native storage operation.
    pub fn authorize(
        &self,
        context: StoreContext<'_>,
        change: Change,
    ) -> std::result::Result<(), PolicyError> {
        self.policy.authorize(context, &self.principal, change)
    }
}
fn enabled(snapshot: &Snapshot) -> Result<()> {
    if super::policy_catalog::version(snapshot)? != 6 {
        return Err(Error::FilePolicySchema);
    }
    Ok(())
}
fn load(snapshot: &Snapshot, context: StoreContext<'_>) -> Result<Option<DecodedPolicy>> {
    let schema = records::schema();
    snapshot
        .get(&schema.name, &Key::Text(records::key(&context.database)))
        .map_err(|_| Error::Corrupt)?
        .map(|row| records::inspect(context, row).map_err(|_| Error::Corrupt))
        .transpose()
}
/// Complete bounded inventory including canonical keys and confirmed revisions.
pub(super) fn validate(
    snapshot: &Snapshot,
    project: &str,
    last: u64,
) -> Result<Vec<FilePolicyReceipt>> {
    if !crate::valid_project_id(project) {
        return Err(Error::Corrupt);
    }
    enabled(snapshot)?;
    let schema = records::schema();
    if snapshot.table_count() != 9
        || snapshot.schema(&schema.name).map_err(|_| Error::Corrupt)? != &schema
    {
        return Err(Error::Corrupt);
    }
    let mut inventory = Vec::new();
    for row in snapshot
        .primary_rows(&schema.name, None, None)
        .map_err(|_| Error::Corrupt)?
    {
        let row = row.map_err(|_| Error::Corrupt)?;
        if inventory.len() == MAX_FILE_POLICIES {
            return Err(Error::Corrupt);
        }
        let Some(Value::Bytes(database)) = row.get(5) else {
            return Err(Error::Corrupt);
        };
        let database = database.as_slice().try_into().map_err(|_| Error::Corrupt)?;
        let decoded = records::inspect(StoreContext { project, database }, row)
            .map_err(|_| Error::Corrupt)?;
        if decoded.revision() > last {
            return Err(Error::Corrupt);
        }
        inventory.push(FilePolicyReceipt::from(&decoded));
    }
    Ok(inventory)
}
impl AccountStore {
    /// Explicit v5 -> v6 atomic migration, preserving existing admission/sessions.
    pub fn enable_file_policy_catalog(&mut self) -> Result<()> {
        self.enable_file_policy_catalog_with(|| {})
    }
    /// A refusal after successful commit withholds output and reports uncertainty.
    /// The enclosing coordinator must preserve committed data and inspect before retry.
    pub fn enable_file_policy_catalog_guarded(
        &mut self,
        mut guard: impl FnMut(FilePolicyWritePhase) -> Result<()>,
    ) -> Result<()> {
        self.enable_file_policy_catalog_inner(|| {}, Some(&mut guard))
    }
    pub(super) fn enable_file_policy_catalog_with(
        &mut self,
        before_commit: impl FnOnce(),
    ) -> Result<()> {
        self.enable_file_policy_catalog_inner(before_commit, None)
    }
    fn enable_file_policy_catalog_inner(
        &mut self,
        before_commit: impl FnOnce(),
        mut guard: Guard<'_>,
    ) -> Result<()> {
        check(&mut guard, FilePolicyWritePhase::BeforeOperation)?;
        let state = super::archive::validate_snapshot(
            self.database.view()?,
            &self.project,
            self.database.last_transaction(),
        )?;
        if state.version == 6 {
            return check(&mut guard, FilePolicyWritePhase::Verified);
        }
        if state.version != 5 {
            return Err(Error::FilePolicySchema);
        }
        let mut scope = self
            .database
            .view()?
            .get(SCOPE, &Key::Integer(1))
            .map_err(|_| Error::Corrupt)?
            .ok_or(Error::Corrupt)?
            .clone();
        scope[1] = Value::Integer(6);
        let mut tx = self.database.begin()?;
        tx.create_table(records::schema())?;
        tx.update(SCOPE, &Key::Integer(1), scope)?;
        before_commit();
        check(&mut guard, FilePolicyWritePhase::BeforeCommit)?;
        tx.commit()?;
        committed_checks(&mut guard)
    }
    pub fn file_policy_receipts(&self) -> Result<Vec<FilePolicyReceipt>> {
        enabled(self.database.view()?)?;
        validate(
            self.database.view()?,
            &self.project,
            self.database.last_transaction(),
        )
    }
    /// Exact original bytes and actual native commit revision; no implicit retry.
    pub fn install_file_policy(
        &mut self,
        context: StoreContext<'_>,
        expected: u64,
        document: &[u8],
    ) -> Result<FilePolicyReceipt> {
        self.install_file_policy_with(context, expected, document, || {})
    }
    /// Actual private commit phases under the original owner. Native commit errors
    /// retain their typed cause; an accepted BeforeCommit may precede a write attempt.
    pub fn install_file_policy_guarded(
        &mut self,
        context: StoreContext<'_>,
        expected: u64,
        document: &[u8],
        mut guard: impl FnMut(FilePolicyWritePhase) -> Result<()>,
    ) -> Result<FilePolicyReceipt> {
        self.install_file_policy_inner(context, expected, document, || {}, Some(&mut guard))
    }
    pub(super) fn install_file_policy_with(
        &mut self,
        context: StoreContext<'_>,
        expected: u64,
        document: &[u8],
        before_commit: impl FnOnce(),
    ) -> Result<FilePolicyReceipt> {
        self.install_file_policy_inner(context, expected, document, before_commit, None)
    }
    fn install_file_policy_inner(
        &mut self,
        context: StoreContext<'_>,
        expected: u64,
        document: &[u8],
        before_commit: impl FnOnce(),
        mut guard: Guard<'_>,
    ) -> Result<FilePolicyReceipt> {
        check(&mut guard, FilePolicyWritePhase::BeforeOperation)?;
        enabled(self.database.view()?)?;
        if context.project != self.project {
            return Err(Error::ScopeMismatch);
        }
        let inventory = self.file_policy_receipts()?;
        let prior = load(self.database.view()?, context)?;
        if let Some(previous) = &prior {
            if previous.document() == document
                && (expected == previous.revision() || expected == previous.previous())
            {
                check(&mut guard, FilePolicyWritePhase::Verified)?;
                return Ok(FilePolicyReceipt::from(previous));
            }
            if expected != previous.revision() {
                return Err(Error::FilePolicyConflict);
            }
        } else if expected != 0 {
            return Err(Error::FilePolicyConflict);
        } else if inventory.len() >= MAX_FILE_POLICIES {
            return Err(Error::FilePolicyCapacity);
        }
        let revision = self
            .database
            .last_transaction()
            .checked_add(1)
            .filter(|revision| *revision < u64::MAX)
            .ok_or(Error::FilePolicyCapacity)?;
        let predecessor = prior.as_ref().map_or(0, DecodedPolicy::revision);
        let encoded =
            records::encode(context, revision, predecessor, document).map_err(Error::FilePolicy)?;
        let decoded = records::inspect(context, encoded.row()).map_err(Error::FilePolicy)?;
        let receipt = FilePolicyReceipt::from(&decoded);
        let schema = records::schema();
        let mut tx = self.database.begin()?;
        if prior.is_some() {
            tx.update(
                &schema.name,
                &Key::Text(records::key(&context.database)),
                encoded.into_row(),
            )?;
        } else {
            tx.insert(&schema.name, encoded.into_row())?;
        }
        before_commit();
        check(&mut guard, FilePolicyWritePhase::BeforeCommit)?;
        tx.commit()?;
        committed_checks(&mut guard)?;
        Ok(receipt)
    }
    /// The borrow excludes policy/credential replacement in the same store.
    /// Caller supplies trusted current time and actual selected native context.
    /// ```compile_fail
    /// use emilybase_auth::{accounts::AccountStore,file_policy::StoreContext};
    /// fn replace(store:&mut AccountStore,context:StoreContext<'_>) {
    ///     let proof=store.verify_file_policy_access("synthetic",100,context).unwrap();
    ///     let _=store.install_file_policy(context,0,b"synthetic");
    ///     let _=proof.receipt();
    /// }
    /// ```
    pub fn verify_file_policy_access(
        &mut self,
        token: &str,
        now: u64,
        context: StoreContext<'_>,
    ) -> Result<FilePolicyPrincipal<'_>> {
        enabled(self.database.view()?)?;
        if context.project != self.project {
            return Err(Error::ScopeMismatch);
        }
        self.file_policy_receipts()?;
        let policy = load(self.database.view()?, context)?;
        let principal = self.verify_access(token, now)?;
        Ok(FilePolicyPrincipal {
            principal,
            policy: policy.ok_or(Error::FilePolicyDenied)?,
        })
    }
}
