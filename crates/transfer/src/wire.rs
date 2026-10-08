use crate::{
    Error, MAX_TRANSFER_ROWS, Result,
    bounded::{List, Text},
};
use emilybase_catalog::{
    Column, DataType, MAX_COLUMNS, MAX_NAME_BYTES, MAX_VALUE_BYTES, Row, Schema, Value,
};
use serde::{Deserialize, Serialize, Serializer, ser::SerializeSeq};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Input {
    pub format: Text<24>,
    pub version: u16,
    pub schema: InputSchema,
    pub rows: List<List<InputValue, MAX_COLUMNS>, MAX_TRANSFER_ROWS>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InputSchema {
    name: Text<MAX_NAME_BYTES>,
    columns: List<InputColumn, MAX_COLUMNS>,
    primary_key: u16,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputColumn {
    name: Text<MAX_NAME_BYTES>,
    data_type: DataType,
    nullable: bool,
}
impl InputSchema {
    pub fn into_schema(self) -> Schema {
        Schema {
            name: self.name.0,
            columns: self
                .columns
                .0
                .into_iter()
                .map(|c| Column {
                    name: c.name.0,
                    data_type: c.data_type,
                    nullable: c.nullable,
                })
                .collect(),
            primary_key: self.primary_key,
        }
    }
}
#[derive(Deserialize)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(crate) enum InputValue {
    Null,
    Boolean(bool),
    Integer(i64),
    FloatBits(Text<16>),
    Text(Text<MAX_VALUE_BYTES>),
    Bytes(List<u8, MAX_VALUE_BYTES>),
}
impl InputValue {
    pub fn into_value(self) -> Result<Value> {
        Ok(match self {
            Self::Null => Value::Null,
            Self::Boolean(v) => Value::Boolean(v),
            Self::Integer(v) => Value::Integer(v),
            Self::Text(v) => Value::Text(v.0),
            Self::Bytes(v) => Value::Bytes(v.0),
            Self::FloatBits(v) => {
                if v.0.len() != 16
                    || !v
                        .0
                        .bytes()
                        .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
                {
                    return Err(Error::Document);
                }
                let bits = u64::from_str_radix(&v.0, 16).map_err(|_| Error::Document)?;
                Value::Float(f64::from_bits(bits))
            }
        })
    }
}
#[derive(Serialize)]
pub(crate) struct Output<'a> {
    pub format: &'static str,
    pub version: u16,
    pub schema: &'a Schema,
    #[serde(serialize_with = "output_rows")]
    pub rows: &'a [&'a Row],
}
fn output_rows<S: Serializer>(
    rows: &&[&Row],
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    let mut sequence = serializer.serialize_seq(Some(rows.len()))?;
    for row in *rows {
        sequence.serialize_element(&OutputRow(row))?;
    }
    sequence.end()
}
// The document borrows the original rows. Only the fixed-size float bit text is owned.
#[derive(Serialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
enum OutputValue<'a> {
    Null,
    Boolean(bool),
    Integer(i64),
    FloatBits(String),
    Text(&'a str),
    Bytes(&'a [u8]),
}
struct OutputRow<'a>(&'a Row);
impl Serialize for OutputRow<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for value in self.0 {
            let value = match value {
                Value::Null => OutputValue::Null,
                Value::Boolean(v) => OutputValue::Boolean(*v),
                Value::Integer(v) => OutputValue::Integer(*v),
                Value::Float(v) => OutputValue::FloatBits(format!("{:016x}", v.to_bits())),
                Value::Text(v) => OutputValue::Text(v),
                Value::Bytes(v) => OutputValue::Bytes(v),
            };
            sequence.serialize_element(&value)?;
        }
        sequence.end()
    }
}
