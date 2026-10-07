use serde::{Deserialize, Serialize};

use crate::{DataType, Error, Key, MAX_VALUE_BYTES, Result, Value};

pub const MAX_NAME_BYTES: usize = 63;
pub const MAX_COLUMNS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Column {
    pub name: String,
    pub data_type: DataType,
    pub nullable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Schema {
    pub name: String,
    pub columns: Vec<Column>,
    /// Zero-based column position. Composite keys are not implemented.
    pub primary_key: u16,
}

impl Schema {
    pub fn validate(&self) -> Result<()> {
        validate_identifier(&self.name)?;
        if self.columns.is_empty() || self.columns.len() > MAX_COLUMNS {
            return Err(Error::ColumnCount);
        }
        // The public count bound admits a fixed stack buffer. Sort borrowed
        // names with original positions, then retain the original error order.
        let mut names = [("", 0usize); MAX_COLUMNS];
        for (index, column) in self.columns.iter().enumerate() {
            // Oversized names must not enter comparisons before their typed
            // refusal. Empty placeholders cannot affect any earlier valid duplicate.
            let name = if column.name.len() <= MAX_NAME_BYTES {
                column.name.as_str()
            } else {
                ""
            };
            names[index] = (name, index);
        }
        let names = &mut names[..self.columns.len()];
        names.sort_unstable();
        let mut duplicate = [false; MAX_COLUMNS];
        for pair in names.windows(2) {
            if pair[0].0 == pair[1].0 {
                duplicate[pair[1].1] = true;
            }
        }
        for (index, column) in self.columns.iter().enumerate() {
            validate_identifier(&column.name)?;
            if duplicate[index] {
                return Err(Error::DuplicateColumn);
            }
        }
        let primary = self
            .columns
            .get(usize::from(self.primary_key))
            .ok_or(Error::PrimaryKey)?;
        if primary.nullable || !matches!(primary.data_type, DataType::Integer | DataType::Text) {
            return Err(Error::PrimaryKey);
        }
        Ok(())
    }

    pub fn validate_row(&self, row: &[Value]) -> Result<()> {
        self.validate()?;
        if row.len() != self.columns.len() {
            return Err(Error::RowLength);
        }
        for (index, (value, column)) in row.iter().zip(&self.columns).enumerate() {
            value.validate()?;
            match value.data_type() {
                None if !column.nullable => return Err(Error::Null(index)),
                Some(kind) if kind != column.data_type => return Err(Error::Type(index)),
                _ => (),
            }
        }
        Ok(())
    }

    pub fn key(&self, row: &[Value]) -> Result<Key> {
        self.validate_row(row)?;
        Key::from_value(&row[usize::from(self.primary_key)])
    }

    pub fn validate_key(&self, key: &Key) -> Result<()> {
        self.validate()?;
        // Reject oversized borrowed keys before creating an owned value or buffer.
        let data_type = match key {
            Key::Integer(_) => DataType::Integer,
            Key::Text(text) => {
                if text.len() > MAX_VALUE_BYTES {
                    return Err(Error::ValueSize);
                }
                DataType::Text
            }
        };
        if data_type != self.columns[usize::from(self.primary_key)].data_type {
            return Err(Error::PrimaryKey);
        }
        Ok(())
    }
}

fn validate_identifier(name: &str) -> Result<()> {
    let bytes = name.as_bytes();
    if bytes.is_empty()
        || bytes.len() > MAX_NAME_BYTES
        || !(bytes[0].is_ascii_alphabetic() || bytes[0] == b'_')
        || !bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'_')
    {
        return Err(Error::Identifier);
    }
    Ok(())
}

#[cfg(test)]
#[path = "schema_validation_tests.rs"]
mod validation_tests;
