//! Synchronous service ownership. Network admission and policies are separate.
use super::{Contents, descriptor, inspect_owned_limited, lock, metadata, names};
use crate::{CreatedProject, Error, ProjectInfo, Result};
use emilybase_auth::accounts::{AccountInfo, AccountStore, IssuedSession, SessionPrincipal};
use emilybase_auth::password::PasswordPool;
use emilybase_catalog::Value;
use std::fs::File;
use std::path::{Path, PathBuf};

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
        metadata::owned_directory(&self.selected, &self.owner)?;
        let root = descriptor(&self.owner);
        if names(&root)? != ["private".into(), "registry".into(), "root.json".into()].into()
            || names(&root.join("registry"))? != self.contents.ids
            || names(&root.join("private"))?
                != self
                    .contents
                    .manifest
                    .private_projects
                    .iter()
                    .cloned()
                    .collect()
        {
            return Err(Error::BundleRoot("service root inventory changed"));
        }
        metadata::owned_directory(&root.join("private"), &self.contents.private_owner)?;
        for (id, owner) in self
            .contents
            .manifest
            .private_projects
            .iter()
            .zip(&self.contents.account_owners)
        {
            metadata::owned_directory(&root.join("private").join(id), owner)?;
        }
        super::check_manifest(&root, &self.contents.manifest_owner, &self.contents.encoded)?;
        self.contents.registry.list()?;
        Ok(())
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
