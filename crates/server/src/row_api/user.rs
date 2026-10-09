//! The original strict row grammar mapped only to policy-enforced operations.
use super::*;
use crate::{UserTableOperation as Op, UserTableResult as Out, UserWrite};
#[derive(Debug, thiserror::Error)]
pub enum UserRowTransportError {
    #[error("invalid bounded user row request")]
    Request,
    #[error("user row response outcome requires inspection")]
    Response,
}
pub(crate) fn decode_user(
    op: Operation,
    bytes: &[u8],
) -> std::result::Result<(String, Op), UserRowTransportError> {
    let input = decode(op, bytes).map_err(|_| UserRowTransportError::Request)?;
    Ok(match input {
        Prepared::Get(table, key) => (table, Op::Get(key)),
        Prepared::Page(table, after, limit) => (table, Op::Page { after, limit }),
        Prepared::Batch(table, operations) => (
            table,
            Op::Write(
                operations
                    .into_iter()
                    .map(|op| match op {
                        WriteOperation::Insert(row) => UserWrite::Insert(row),
                        WriteOperation::Update(key, row) => UserWrite::Update { key, row },
                        WriteOperation::Delete(key) => UserWrite::Delete(key),
                    })
                    .collect(),
            ),
        ),
        _ => return Err(UserRowTransportError::Request),
    })
}
/// Pure bounded grammar inspection; no credentials, database or row authority.
pub fn validate_user_row_request(
    op: Operation,
    bytes: &[u8],
) -> std::result::Result<(), UserRowTransportError> {
    decode_user(op, bytes).map(drop)
}
pub(crate) fn user_response(
    result: &Out,
) -> std::result::Result<axum::response::Response, UserRowTransportError> {
    let response = match result {
        Out::Row(row) => response(&Found {
            row: row.as_ref().map(OutputRow),
        }),
        Out::Page { rows, next } => response(&Rows {
            rows: rows.iter().map(OutputRow).collect(),
            next: next.as_ref().map(OutputKey::from),
        }),
        Out::Committed {
            transaction,
            operations,
        } => response(&BatchChanged {
            changed: *operations,
            transaction: transaction.to_string(),
        }),
    };
    response.map_err(|_| UserRowTransportError::Response)
}
#[cfg(test)]
mod tests;
