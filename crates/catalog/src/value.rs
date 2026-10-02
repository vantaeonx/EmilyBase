use serde::{Deserialize, Serialize};

use crate::{Error, Result};

pub const MAX_VALUE_BYTES: usize = 3072;
pub type Row = Vec<Value>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataType {
    Boolean,
    Integer,
    Float,
    Text,
    Bytes,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Value {
    Null,
    Boolean(bool),
    Integer(i64),
    Float(f64),
    Text(String),
    Bytes(Vec<u8>),
}

impl Value {
    pub fn data_type(&self) -> Option<DataType> {
        match self {
            Self::Null => None,
            Self::Boolean(_) => Some(DataType::Boolean),
            Self::Integer(_) => Some(DataType::Integer),
            Self::Float(_) => Some(DataType::Float),
            Self::Text(_) => Some(DataType::Text),
            Self::Bytes(_) => Some(DataType::Bytes),
        }
    }

    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Float(value) if !value.is_finite() => Err(Error::Float),
            Self::Text(value) if value.len() > MAX_VALUE_BYTES => Err(Error::ValueSize),
            Self::Bytes(value) if value.len() > MAX_VALUE_BYTES => Err(Error::ValueSize),
            _ => Ok(()),
        }
    }
}

/// Integer keys sort numerically; text keys sort by UTF-8 bytes, without collation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Key {
    Integer(i64),
    Text(String),
}

impl Key {
    pub fn to_value(&self) -> Value {
        match self {
            Self::Integer(value) => Value::Integer(*value),
            Self::Text(value) => Value::Text(value.clone()),
        }
    }

    pub fn from_value(value: &Value) -> Result<Self> {
        value.validate()?;
        match value {
            Value::Integer(value) => Ok(Self::Integer(*value)),
            Value::Text(value) => Ok(Self::Text(value.clone())),
            _ => Err(Error::PrimaryKey),
        }
    }
}
