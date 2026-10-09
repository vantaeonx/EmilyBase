//! Synchronous service ownership. Network admission and policies are separate.
use super::{Contents, inspect_owned_limited, lock, metadata};
use crate::{CreatedProject, Error, ProjectInfo, Result};
use emilybase_auth::accounts::{
    AccountInfo, AccountPage, AccountStore, IssuedSession, SessionPrincipal,
};
use emilybase_auth::password::PasswordPool;
use emilybase_catalog::Value;
use std::fs::File;
use std::path::{Path, PathBuf};
mod key_file;

/// Active private owner count, not a combined model/heap reservation.
pub const MAX_ACTIVE_PRIVATE_STORES: usize = 4;

/// An explicitly selected fixed root. No Clone, detached private owner or secret
/// serialization. Opening never creates paths, resets sessions or advances time.
pub struct AccountRoot {
    selected: PathBuf,
    contents: Contents,
    // Drop registry/private children before releasing the selected-root lock.
    owner: File,
}
impl std::fmt::Debug for AccountRoot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AccountRoot(redacted)")
    }
}
impl AccountRoot {
    pub(crate) fn table_operation<T>(
        &self,
        project: &str,
        key: &str,
        work: impl FnOnce(&mut emilybase_transactions::Database) -> crate::table_api::Result<T>,
    ) -> Result<T> {
        self.data_operation(project, key, |database| {
            work(database).map_err(Error::Tables)
        })
    }
    pub(crate) fn data_operation<T>(
        &self,
        project: &str,
        key: &str,
        work: impl FnOnce(&mut emilybase_transactions::Database) -> Result<T>,
    ) -> Result<T> {
        self.ready()?;
        self.contents
            .registry
            .authorize(project, key)?
            .data_operation(work)
    }
    pub(crate) fn export_table(
        &self,
        project: &str,
        key: &str,
        table: &str,
        limit: usize,
    ) -> Result<Vec<u8>> {
        self.ready()?;
        self.contents
            .registry
            .authorize(project, key)?
            .export_table(table, limit)
    }
    pub(crate) fn import_table(
        &self,
        project: &str,
        key: &str,
        table: emilybase_transfer::VerifiedTable,
    ) -> Result<crate::http::ImportedTable> {
        self.ready()?;
        self.contents
            .registry
            .authorize(project, key)?
            .import_table(table)
    }
    // Memory-only preadmission. Selected filesystem identities and the current
    // private service key are checked again inside the blocking operation.
    pub(crate) fn admits_project(&self, project: &str, key: &str, private: bool) -> Result<()> {
        drop(self.contents.registry.authorize(project, key)?);
        if private
            && !self
                .contents
                .accounts
                .iter()
                .any(|store| store.project() == project)
        {
            return Err(Error::Denied);
        }
        Ok(())
    }
    pub(crate) fn status(&self, project: &str, key: &str) -> Result<crate::ProjectStatus> {
        self.ready()?;
        self.contents.registry.authorize(project, key)?.status()
    }
    pub(crate) fn explain(
        &self,
        project: &str,
        key: &str,
        sql: &str,
        parameters: &[Value],
    ) -> Result<emilybase_query::PlanDescription> {
        self.ready()?;
        self.contents
            .registry
            .authorize(project, key)?
            .explain(sql, parameters)
    }
    pub fn open(path: impl AsRef<Path>, pool: PasswordPool) -> Result<Self> {
        let owner = metadata::open_directory(path.as_ref())?;
        lock(&owner)?;
        let selected = path.as_ref().canonicalize()?;
        let state = inspect_owned_limited(
            &selected,
            &owner,
            pool,
            MAX_ACTIVE_PRIVATE_STORES,
            |_, _| Ok(()),
        )?;
        // Move the exact inspected owners; do not reopen after validation.
        let result = Self {
            selected,
            owner,
            contents: state.contents,
        };
        result.ready()?;
        super::checkpoint("account_root_service_opened");
        Ok(result)
    }
    fn ready(&self) -> Result<()> {
        super::check_contents(&self.selected, &self.owner, &self.contents)
    }
    fn account(&mut self, project: &str, key: &str) -> Result<&mut AccountStore> {
        self.ready()?;
        // Current service key, never a user session or master-key fallback.
        drop(self.contents.registry.authorize(project, key)?);
        self.contents
            .accounts
            .iter_mut()
            .find(|store| store.project() == project)
            .ok_or(Error::Denied)
    }
    /// Explicit private v3-to-v4 migration, guarded by the current service key.
    pub fn enable_row_policy_catalog(&mut self, project: &str, key: &str) -> Result<()> {
        Ok(self.account(project, key)?.enable_row_policy_catalog()?)
    }
    /// Complete private policy metadata only; no clock observation or user grant.
    pub fn row_policy_receipts(
        &mut self,
        project: &str,
        key: &str,
    ) -> Result<Vec<emilybase_auth::accounts::PolicyReceipt>> {
        Ok(self.account(project, key)?.row_policy_receipts()?)
    }
    /// Derive the target identity/schema under the held public data gate/owner.
    /// The caller cannot substitute an asserted project, table ID or schema.
    pub fn install_row_policy(
        &mut self,
        project: &str,
        key: &str,
        table: &str,
        expected: u64,
        document: &[u8],
    ) -> Result<emilybase_auth::accounts::PolicyReceipt> {
        self.ready()?;
        let authorized = self.contents.registry.authorize(project, key)?;
        let account = self
            .contents
            .accounts
            .iter_mut()
            .find(|store| store.project() == project)
            .ok_or(Error::Denied)?;
        authorized.data_operation(|database| {
            let snapshot = database.view()?;
            let context = emilybase_auth::row_policy::TableContext {
                project,
                id: snapshot.table_id(table).map_err(crate::TableError::from)?,
                schema: snapshot.schema(table).map_err(crate::TableError::from)?,
            };
            super::checkpoint("root_policy_context_acquired");
            Ok(account.install_row_policy(context, expected, document)?)
        })
    }
    /// Enforce installed policy and current session within the actual data owner.
    /// Trusted service key/time only; no SQL, unfiltered scan or detached handle.
    pub fn user_table(
        &mut self,
        project: &str,
        key: &str,
        table: &str,
        access: &str,
        now: u64,
        operation: crate::UserTableOperation,
    ) -> Result<crate::UserTableResult> {
        self.ready()?;
        let authorized = self.contents.registry.authorize(project, key)?;
        let account = self
            .contents
            .accounts
            .iter_mut()
            .find(|store| store.project() == project)
            .ok_or(Error::Denied)?;
        crate::user_rows::validate(table, &operation)?;
        authorized.data_operation(|database| {
            let snapshot = database.view()?;
            let table_id = snapshot.table_id(table).map_err(crate::TableError::from)?;
            let schema = snapshot
                .schema(table)
                .map_err(crate::TableError::from)?
                .clone();
            let context = emilybase_auth::row_policy::TableContext {
                project,
                id: table_id,
                schema: &schema,
            };
            let proof = account.verify_row_policy_access(access, now, table_id)?;
            super::checkpoint("root_user_table_verified");
            Ok(crate::user_rows::run(database, &proof, context, operation)?)
        })
    }
    /// Trusted operator metadata, never proof of user authorization.
    pub fn projects(&self) -> Result<Vec<ProjectInfo>> {
        self.ready()?;
        self.contents.registry.list()
    }
    /// Explicit trusted operator action; no user family is silently reset.
    pub fn rotate_project_key(&mut self, project: &str) -> Result<CreatedProject> {
        self.ready()?;
        self.contents.registry.rotate(project)
    }
    /// Private provisioning requires the current project service key. This is
    /// not anonymous self-service registration or a user role permission.
    pub fn create_user(
        &mut self,
        project: &str,
        key: &str,
        login: &str,
        password: &[u8],
    ) -> Result<AccountInfo> {
        Ok(self.account(project, key)?.create_user(login, password)?)
    }
    /// Current metadata page only: no verifier, token or authorization receipt.
    /// Continuation does not retain a snapshot across separate calls.
    pub fn list_users(
        &mut self,
        project: &str,
        key: &str,
        after: Option<&str>,
        limit: usize,
    ) -> Result<AccountPage> {
        Ok(self.account(project, key)?.list_users(after, limit)?)
    }
    /// Trusted service time only; a forward denied attempt can persist its clock.
    pub fn sign_in(
        &mut self,
        project: &str,
        key: &str,
        login: &str,
        password: &[u8],
        now: u64,
    ) -> Result<IssuedSession> {
        Ok(self.account(project, key)?.sign_in(login, password, now)?)
    }
    /// Atomic single-use refresh. Never automatically retry an unknown outcome.
    pub fn refresh_session(
        &mut self,
        project: &str,
        key: &str,
        token: &str,
        now: u64,
    ) -> Result<IssuedSession> {
        Ok(self.account(project, key)?.refresh_session(token, now)?)
    }
    /// Logout requires the current refresh credential, never an access token.
    pub fn logout_session(
        &mut self,
        project: &str,
        key: &str,
        token: &str,
        now: u64,
    ) -> Result<()> {
        Ok(self.account(project, key)?.logout_session(token, now)?)
    }
    /// Explicit trusted cleanup of at most 128 inactive families in one commit.
    /// Access expiry alone does not make a refreshable family inactive. Clock
    /// advancement is separately durable even if no family needs removal.
    pub fn prune_session_families(
        &mut self,
        project: &str,
        key: &str,
        now: u64,
        limit: usize,
    ) -> Result<usize> {
        Ok(self
            .account(project, key)?
            .prune_session_families(now, limit)?)
    }
    /// Consume proof immediately while the private owner is borrowed. The result
    /// can contain metadata, but cannot detach a borrowed authorization proof.
    ///
    /// ```compile_fail
    /// use emilybase_server::{AccountRoot, Result};
    /// use emilybase_auth::accounts::SessionPrincipal;
    /// fn detach<'a>(root: &'a mut AccountRoot, project: &str, key: &str, token: &str)
    ///     -> Result<SessionPrincipal<'a>> {
    ///     root.with_access(project, key, token, 50, |principal| principal)
    /// }
    /// ```
    pub fn with_access<T>(
        &mut self,
        project: &str,
        key: &str,
        token: &str,
        now: u64,
        work: impl FnOnce(SessionPrincipal<'_>) -> T,
    ) -> Result<T> {
        let principal = self.account(project, key)?.verify_access(token, now)?;
        Ok(work(principal))
    }
    pub fn set_disabled(
        &mut self,
        project: &str,
        key: &str,
        login: &str,
        disabled: bool,
    ) -> Result<AccountInfo> {
        Ok(self.account(project, key)?.set_disabled(login, disabled)?)
    }
    pub fn change_password(
        &mut self,
        project: &str,
        key: &str,
        login: &str,
        current: &[u8],
        replacement: &[u8],
    ) -> Result<AccountInfo> {
        Ok(self
            .account(project, key)?
            .change_password(login, current, replacement)?)
    }
    /// Existing project-service SQL authority only. Session tokens cannot grant
    /// SQL access; user roles and row policies are not implemented here.
    pub fn execute(
        &self,
        project: &str,
        key: &str,
        sql: &str,
        parameters: &[Value],
    ) -> Result<emilybase_query::Report> {
        self.ready()?;
        self.contents
            .registry
            .authorize(project, key)?
            .execute(sql, parameters)
    }
}
