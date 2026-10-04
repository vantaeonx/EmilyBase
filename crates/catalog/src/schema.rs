use std::collections::BTreeSet;

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
        let mut names = BTreeSet::new();
        for column in &self.columns {
            validate_identifier(&column.name)?;
            if !names.insert(&column.name) {
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
