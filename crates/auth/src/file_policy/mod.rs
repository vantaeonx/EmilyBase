//! Pure bounded file decisions for a borrowed current private session principal.
//! No policy installation, file handle, HTTP route or detached permission grant.
use crate::accounts::SessionPrincipal;
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;

pub const VERSION: u16 = 1;
pub const MAX_DOCUMENT_BYTES: usize = 1024;

#[derive(Debug, thiserror::Error)]
pub enum PolicyError {
    #[error("invalid bounded file policy document")]
    Document,
    #[error("unsupported file policy version")]
    Version,
    #[error("file policy document exceeds its resource bound")]
    Limit,
    #[error("file policy context does not match its binding")]
    Scope,
    #[error("file policy denied this operation")]
    Denied,
}
pub type Result<T> = std::result::Result<T, PolicyError>;

/// Explicit rules for existing references. Owner metadata must come from storage.
#[derive(Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Rule {
    Deny {},
    Owner {},
    Authenticated {},
}
/// User creation never assigns another account as owner, even for an open rule.
#[derive(Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CreateRule {
    Deny {},
    SelfOwned {},
}
/// All four operations are required. No implicit rule or read/write inheritance.
#[derive(PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub version: u16,
    pub read: Rule,
    pub create: CreateRule,
    pub rename: Rule,
    pub remove: Rule,
}
impl Definition {
    pub const fn deny_all() -> Self {
        Self {
            version: VERSION,
            read: Rule::Deny {},
            create: CreateRule::Deny {},
            rename: Rule::Deny {},
            remove: Rule::Deny {},
        }
    }
}
impl std::fmt::Debug for Definition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FilePolicyDefinition(redacted)")
    }
}
impl std::fmt::Debug for Rule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FilePolicyRule(redacted)")
    }
}
impl std::fmt::Debug for CreateRule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FileCreateRule(redacted)")
    }
}
/// Syntax only. Decoding neither verifies identity nor installs current policy.
pub fn decode(bytes: &[u8]) -> Result<Definition> {
    if bytes.len() > MAX_DOCUMENT_BYTES {
        return Err(PolicyError::Limit);
    }
    serde_json::from_slice(bytes).map_err(|_| PolicyError::Document)
}
/// Supplied by the trusted adapter from the actual selected native component.
/// Opaque database IDs are compared exactly; this model performs no discovery.
#[derive(Clone, Copy)]
pub struct StoreContext<'a> {
    pub project: &'a str,
    pub database: [u8; 16],
}
/// Existing owners must be read from the current authoritative native reference.
/// A user adapter derives a proposed create owner from the verified principal.
pub enum Change {
    Read { owner: [u8; 16] },
    Create { owner: [u8; 16] },
    Rename { owner: [u8; 16] },
    Remove { owner: [u8; 16] },
}
/// Immutable project/native-database-bound decision model, not installed authority.
pub struct BoundPolicy {
    project: String,
    database: [u8; 16],
    rules: [Rule; 3],
    create: CreateRule,
}
impl std::fmt::Debug for BoundPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BoundFilePolicy(redacted)")
    }
}
impl BoundPolicy {
    pub fn compile(context: StoreContext<'_>, definition: &Definition) -> Result<Self> {
        if !crate::valid_project_id(context.project) {
            return Err(PolicyError::Scope);
        }
        if definition.version != VERSION {
            return Err(PolicyError::Version);
        }
        Ok(Self {
            project: context.project.into(),
            database: context.database,
            rules: [definition.read, definition.rename, definition.remove],
            create: definition.create,
        })
    }
    /// Check scope before reference lookup, including empty/missing results.
    pub fn check_context(
        &self,
        context: StoreContext<'_>,
        principal: &SessionPrincipal<'_>,
    ) -> Result<()> {
        if context.project != self.project
            || context.database != self.database
            || principal.project() != self.project
            || principal.account().disabled
            || principal.account().credential_epoch == 0
        {
            return Err(PolicyError::Scope);
        }
        Ok(())
    }
    /// Consume immediately inside the trusted original storage operation. The
    /// caller obtains a fresh principal/current installed model and real metadata;
    /// this pure function observes no wall clock, filesystem or policy catalog.
    ///
    /// ```compile_fail
    /// use emilybase_auth::{accounts::AccountStore, file_policy::{BoundPolicy, StoreContext}};
    /// fn change_credentials(store: &mut AccountStore, policy: &BoundPolicy, context: StoreContext<'_>) {
    ///     let principal = store.verify_access("synthetic",100).unwrap();
    ///     store.set_disabled("synthetic",true).unwrap();
    ///     let _ = policy.check_context(context,&principal);
    /// }
    /// ```
    pub fn authorize(
        &self,
        context: StoreContext<'_>,
        principal: &SessionPrincipal<'_>,
        change: Change,
    ) -> Result<()> {
        self.check_context(context, principal)?;
        let user = &principal.account().id;
        let permitted = match change {
            Change::Read { owner } => matches(self.rules[0], &owner, user),
            Change::Rename { owner } => matches(self.rules[1], &owner, user),
            Change::Remove { owner } => matches(self.rules[2], &owner, user),
            Change::Create { owner } => {
                self.create == CreateRule::SelfOwned {} && bool::from(owner.ct_eq(user))
            }
        };
        if permitted {
            Ok(())
        } else {
            Err(PolicyError::Denied)
        }
    }
}
fn matches(rule: Rule, owner: &[u8; 16], user: &[u8; 16]) -> bool {
    match rule {
        Rule::Deny {} => false,
        Rule::Owner {} => bool::from(owner.ct_eq(user)),
        Rule::Authenticated {} => true,
    }
}
#[cfg(test)]
mod tests;
