//! Bounded table schemas, typed values and original binary record codecs.
mod codec;
mod schema;
mod value;

pub use codec::{decode_row, decode_schema, encode_row, encode_schema, row_matches};
pub use schema::{Column, MAX_COLUMNS, MAX_NAME_BYTES, Schema};
pub use value::{DataType, Key, MAX_VALUE_BYTES, Row, Value};

/// Leaves room for the relational event envelope inside a slotted-page record.
pub const MAX_ENCODED_BYTES: usize = 4000;
pub const RECORD_VERSION: u16 = 1;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid identifier")]
    Identifier,
    #[error("column count is outside the supported range")]
    ColumnCount,
    #[error("duplicate column name")]
    DuplicateColumn,
    #[error("primary key must identify a non-null integer or text column")]
    PrimaryKey,
    #[error("row has an incorrect number of values")]
    RowLength,
    #[error("value at column {0} has the wrong type")]
    Type(usize),
    #[error("column {0} does not allow null")]
    Null(usize),
    #[error("value exceeds the supported size")]
    ValueSize,
    #[error("floating-point values must be finite")]
    Float,
    #[error("encoded record exceeds the supported size")]
    RecordSize,
    #[error("malformed catalog record: {0}")]
    Decode(&'static str),
    #[error("unsupported catalog record version: {0}")]
    Version(u16),
}

pub type Result<T> = std::result::Result<T, Error>;
