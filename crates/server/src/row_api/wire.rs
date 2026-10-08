use crate::table_api::{Result, TableError};
use emilybase_catalog::{Key, Row, Value};
use serde::{Deserialize, Serialize, Serializer, ser::SerializeSeq};

#[derive(Deserialize)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(super) enum Input {
    Null,
    Boolean(bool),
    Integer(String),
    FloatBits(String),
    Text(String),
    Bytes(Vec<u8>),
}
fn integer(text: &str) -> Result<i64> {
    if text.len() > 20 {
        return Err(TableError::Document);
    }
    let value = text.parse::<i64>().map_err(|_| TableError::Document)?;
    if value.to_string() != text {
        return Err(TableError::Document);
    }
    Ok(value)
}
impl Input {
    pub(super) fn value(self) -> Result<Value> {
        let value = match self {
            Self::Null => Value::Null,
            Self::Boolean(v) => Value::Boolean(v),
            Self::Integer(v) => Value::Integer(integer(&v)?),
            Self::Text(v) => Value::Text(v),
            Self::Bytes(v) => Value::Bytes(v),
            Self::FloatBits(v) => {
                if v.len() != 16
                    || !v
                        .bytes()
                        .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
                {
                    return Err(TableError::Document);
                }
                Value::Float(f64::from_bits(
                    u64::from_str_radix(&v, 16).map_err(|_| TableError::Document)?,
                ))
            }
        };
        value.validate()?;
        Ok(value)
    }
}
#[derive(Deserialize)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(super) enum InputKey {
    Integer(String),
    Text(String),
}
impl InputKey {
    pub(super) fn key(self) -> Result<Key> {
        Key::from_value(&match self {
            Self::Integer(v) => Value::Integer(integer(&v)?),
            Self::Text(v) => Value::Text(v),
        })
        .map_err(Into::into)
    }
}
#[derive(Serialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub(super) enum Output<'a> {
    Null,
    Boolean(bool),
    Integer(String),
    FloatBits(String),
    Text(&'a str),
    Bytes(&'a [u8]),
}
impl<'a> From<&'a Value> for Output<'a> {
    fn from(v: &'a Value) -> Self {
        match v {
            Value::Null => Self::Null,
            Value::Boolean(v) => Self::Boolean(*v),
            Value::Integer(v) => Self::Integer(v.to_string()),
            Value::Float(v) => Self::FloatBits(format!("{:016x}", v.to_bits())),
            Value::Text(v) => Self::Text(v),
            Value::Bytes(v) => Self::Bytes(v),
        }
    }
}
#[derive(Serialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub(super) enum OutputKey<'a> {
    Integer(String),
    Text(&'a str),
}
impl<'a> From<&'a Key> for OutputKey<'a> {
    fn from(k: &'a Key) -> Self {
        match k {
            Key::Integer(v) => Self::Integer(v.to_string()),
            Key::Text(v) => Self::Text(v),
        }
    }
}
pub(super) struct OutputRow<'a>(pub &'a Row);
impl Serialize for OutputRow<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for value in self.0 {
            sequence.serialize_element(&Output::from(value))?;
        }
        sequence.end()
    }
}
