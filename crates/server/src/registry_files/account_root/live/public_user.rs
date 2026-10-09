//! Native current-user operations under explicit admission.
use super::{AccountInfo, AccountRoot, AccountStore, IssuedSession, Result};
use crate::{Error, UserTableOperation, UserTableResult};
use emilybase_auth::accounts::Error as AccountError;

impl AccountRoot {
    /// Memory-only transport preadmission, never request authority. Native calls
    /// recheck selected filesystem identities/current state after all waits.
    pub(crate) fn admits_public_user(&self, project: &str) -> Result<()> {
        if !emilybase_auth::valid_project_id(project) {
            return Err(Error::Denied);
        }
        let account = self
            .contents
            .accounts
            .iter()
            .find(|store| store.project() == project)
            .ok_or(Error::Denied)?;
        admission(account)
    }
    fn public_account(&mut self, project: &str) -> Result<&mut AccountStore> {
        self.ready()?;
        let account = self
            .contents
            .accounts
            .iter_mut()
            .find(|store| store.project() == project)
            .ok_or(Error::Denied)?;
        admission(account)?;
        Ok(account)
    }
    /// Current explicit admission and original bounded password verification.
    /// Trusted service time only; no self-service account creation is offered.
    pub fn public_sign_in(
        &mut self,
        project: &str,
        login: &str,
        password: &[u8],
        now: u64,
    ) -> Result<IssuedSession> {
        Ok(self
            .public_account(project)?
            .sign_in(login, password, now)?)
    }
    /// Current admission and single-use refresh; never automatically retry.
    pub fn public_refresh_session(
        &mut self,
        project: &str,
        refresh: &str,
        now: u64,
    ) -> Result<IssuedSession> {
        Ok(self
            .public_account(project)?
            .refresh_session(refresh, now)?)
    }
    /// Current admission and refresh credential, not a copied identity receipt.
    pub fn public_logout_session(&mut self, project: &str, refresh: &str, now: u64) -> Result<()> {
        Ok(self.public_account(project)?.logout_session(refresh, now)?)
    }
    /// Current user's own metadata. The result cannot authorize later operations.
    pub fn public_user(&mut self, project: &str, access: &str, now: u64) -> Result<AccountInfo> {
        let principal = self.public_account(project)?.verify_access(access, now)?;
        Ok(principal.account().clone())
    }
    /// Original typed rows only, under current admission/session/table policy.
    /// No project service key, arbitrary SQL, DDL or unfiltered handle is returned.
    pub fn public_user_table(
        &mut self,
        project: &str,
        table: &str,
        access: &str,
        now: u64,
        operation: UserTableOperation,
    ) -> Result<UserTableResult> {
        // The internal capability stays here and is consumed under both owners.
        let authorized = self.contents.registry.owned_project(project)?;
        let account = self.public_account(project)?;
        crate::user_rows::validate(table, &operation)?;
        // Authenticate before opening public data or resolving its table metadata.
        // The private borrow prevents admission/credential changes while waiting
        // for the original data owner; the current policy proof is derived inside it.
        drop(account.verify_access(access, now)?);
        authorized.data_operation(|database| {
            super::run_user_table(database, account, project, table, access, now, operation)
        })
    }
}

fn admission(account: &AccountStore) -> Result<()> {
    match account.public_admission() {
        Ok(receipt) if receipt.enabled => Ok(()),
        Ok(_) | Err(AccountError::AdmissionSchema) => Err(Error::Denied),
        Err(error) => Err(error.into()),
    }
}
