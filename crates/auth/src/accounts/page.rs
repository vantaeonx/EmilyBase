//! Bounded metadata pagination, without password checks or session authority.
use super::{AccountInfo, AccountStore, Error, Result, USERS, records::validate_login};
use emilybase_catalog::Key;

/// Metadata page size, independently bounded from session cleanup work.
pub const MAX_ACCOUNT_PAGE: usize = 128;

/// Current metadata only; a page is not an authorization or snapshot receipt.
#[derive(Clone, PartialEq, Eq)]
pub struct AccountPage {
    pub users: Vec<AccountInfo>,
    pub next_after: Option<String>,
}
impl std::fmt::Debug for AccountPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AccountPage(redacted)")
    }
}
impl AccountStore {
    /// Exclusive canonical-login cursor. An absent login is a valid boundary.
    /// Each call reads current state; continuation does not pin a cross-call view.
    pub fn list_users(&self, after: Option<&str>, limit: usize) -> Result<AccountPage> {
        if !(1..=MAX_ACCOUNT_PAGE).contains(&limit) {
            return Err(Error::Page);
        }
        if let Some(after) = after {
            validate_login(after)?;
        }
        let key = after.map(|after| Key::Text(after.into()));
        let rows = self
            .database
            .view()?
            .primary_rows(USERS, key.as_ref(), None)
            .map_err(|_| Error::Corrupt)?;
        let mut page = AccountPage {
            users: Vec::with_capacity(limit),
            next_after: None,
        };
        for row in rows.take(limit + 2) {
            // Validate the complete private record, including the lookahead, but
            // return only explicit metadata. Never export verifier bytes.
            let info = super::inspect_account_record(row.map_err(|_| Error::Corrupt)?)?;
            if after.is_some_and(|after| info.login.as_str() <= after) {
                continue;
            }
            if page.users.len() == limit {
                page.next_after = page.users.last().map(|info| info.login.clone());
                break;
            }
            page.users.push(info);
        }
        Ok(page)
    }
}
