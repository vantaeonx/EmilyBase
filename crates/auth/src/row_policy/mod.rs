//! Bounded pure row decisions for a current borrowed private session principal.
//! This library grants no database handle, user HTTP route or detached capability.
mod compile;
mod evaluate;
use crate::accounts::SessionPrincipal;
use emilybase_catalog::{Schema, Value};
use serde::Deserialize;

pub const MAX_DOCUMENT_BYTES: usize = 16384;
pub const MAX_NODES: usize = 64;
pub const MAX_DEPTH: usize = 8;
pub const MAX_LITERAL_BYTES: usize = 8192;

#[derive(Debug, thiserror::Error)]
pub enum PolicyError {
    #[error("invalid bounded row policy document")]
    Document,
    #[error("unsupported row policy version")]
    Version,
    #[error("row policy resource bound exceeded")]
    Limit,
    #[error("row policy schema or expression is invalid")]
    Schema,
    #[error("row policy context does not match its binding")]
    Scope,
    #[error("row policy requires valid original typed rows")]
    Row,
    #[error("row policy denied this operation")]
    Denied,
}
pub type Result<T> = std::result::Result<T, PolicyError>;

/// Every operation is explicit; missing rules never acquire an implicit allow.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub version: u16,
    pub select: Rule,
    pub insert: Rule,
    pub update_using: Rule,
    pub update_check: Rule,
    pub delete: Rule,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Rule {
    Deny {},
    Authenticated {},
    Owner { column: String },
    Equal { column: String, value: Value },
    IsNull { column: String },
    All { terms: Vec<Rule> },
    Any { terms: Vec<Rule> },
}
impl std::fmt::Debug for Definition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RowPolicyDefinition(redacted)")
    }
}
impl std::fmt::Debug for Rule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RowPolicyRule(redacted)")
    }
}

/// Decode only. Performs no I/O, identity verification or authorization.
pub fn decode(bytes: &[u8]) -> Result<Definition> {
    if bytes.len() > MAX_DOCUMENT_BYTES {
        return Err(PolicyError::Document);
    }
    serde_json::from_slice(bytes).map_err(|_| PolicyError::Document)
}
/// Metadata supplied by the trusted caller from the authoritative current table.
#[derive(Clone, Copy)]
pub struct TableContext<'a> {
    pub project: &'a str,
    pub id: u64,
    pub schema: &'a Schema,
}
pub enum Change<'a> {
    Select(&'a [Value]),
    Insert(&'a [Value]),
    Update { old: &'a [Value], new: &'a [Value] },
    Delete(&'a [Value]),
}

enum Expression {
    Deny,
    Authenticated,
    Owner(usize),
    Equal(usize, Value),
    IsNull(usize),
    All(Vec<Expression>),
    Any(Vec<Expression>),
}
/// Immutable schema/identity-bound decision model. Not a storage capability.
pub struct BoundPolicy {
    project: String,
    table: u64,
    schema: Schema,
    expressions: [Expression; 5],
}
impl std::fmt::Debug for BoundPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BoundRowPolicy(redacted)")
    }
}
impl BoundPolicy {
    pub fn compile(context: TableContext<'_>, definition: &Definition) -> Result<Self> {
        compile::bind(context, definition)
    }
    /// A decision for these exact rows while the current private proof is borrowed.
    /// The caller must enforce it inside its authoritative original transaction.
    pub fn authorize(
        &self,
        context: TableContext<'_>,
        principal: &SessionPrincipal<'_>,
        change: Change<'_>,
    ) -> Result<()> {
        evaluate::authorize(self, context, principal, change)
    }
}

#[cfg(test)]
mod tests;
