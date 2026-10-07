use emilybase_catalog::{
    Key, Row, Schema, Value, decode_row, decode_schema, encode_row, encode_schema,
};
use emilybase_storage::MAX_RECORD_SIZE;

use crate::{Error, Result};

const PREFIX_SIZE: usize = 16;
const VERSION: u16 = 1;
pub const DATABASE_MARKER: [u8; PREFIX_SIZE] =
    [b'E', b'T', b'B', b'L', 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub table_id: u64,
    pub kind: EventKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EventKind {
    Root,
    Create(Schema),
    Drop,
    Insert(Row),
    Replace(Row),
    Delete(Key),
}

impl Event {
    /// Called only after logical validation, before retaining owned live state.
    /// Incoming capacity is not part of ETBL data and must not survive storage.
    pub(crate) fn compact_payload(mut self) -> Self {
        match &mut self.kind {
            EventKind::Create(schema) => {
                compact_text(&mut schema.name);
                for column in &mut schema.columns {
                    compact_text(&mut column.name);
                }
                compact_vector(&mut schema.columns);
            }
            EventKind::Insert(row) | EventKind::Replace(row) => {
                for value in row.iter_mut() {
                    match value {
                        Value::Text(text) => compact_text(text),
                        Value::Bytes(bytes) => compact_vector(bytes),
                        _ => (),
                    }
                }
                compact_vector(row);
            }
            EventKind::Delete(Key::Text(text)) => compact_text(text),
            _ => (),
        }
        self
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        if matches!(self.kind, EventKind::Root) != (self.table_id == 0) {
            return Err(Error::Event("table ID"));
        }
        let (tag, payload) = match &self.kind {
            EventKind::Root => (0, Vec::new()),
            EventKind::Create(schema) => (1, encode_schema(schema)?),
            EventKind::Drop => (2, Vec::new()),
            EventKind::Insert(row) => (3, encode_row(row)?),
            EventKind::Replace(row) => (4, encode_row(row)?),
            EventKind::Delete(key) => (5, encode_row(&[key.to_value()])?),
        };
        if PREFIX_SIZE + payload.len() > MAX_RECORD_SIZE {
            return Err(Error::Event("size"));
        }
        let mut bytes = b"ETBL".to_vec();
        bytes.extend_from_slice(&VERSION.to_le_bytes());
        bytes.extend_from_slice(&[tag, 0]);
        bytes.extend_from_slice(&self.table_id.to_le_bytes());
        bytes.extend_from_slice(&payload);
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let (tag, table_id, payload) = envelope(bytes)?;
        let kind = match tag {
            0 if payload.is_empty() => EventKind::Root,
            1 => EventKind::Create(decode_schema(payload)?),
            2 if payload.is_empty() => EventKind::Drop,
            3 => EventKind::Insert(decode_row(payload)?),
            4 => EventKind::Replace(decode_row(payload)?),
            5 => {
                let row = decode_row(payload)?;
                if row.len() != 1 {
                    return Err(Error::Event("delete key arity"));
                }
                EventKind::Delete(Key::from_value(&row[0])?)
            }
            _ => return Err(Error::Event("kind or unexpected payload")),
        };
        if matches!(kind, EventKind::Root) != (table_id == 0) {
            return Err(Error::Event("table ID"));
        }
        Ok(Self { table_id, kind })
    }
    pub(crate) fn row_image_matches(
        bytes: &[u8],
        expected_table: u64,
        expected: &[Value],
    ) -> Result<bool> {
        let (tag, table_id, payload) = envelope(bytes)?;
        if matches!(tag, 3 | 4) {
            let equal = emilybase_catalog::row_matches(payload, expected)?;
            if table_id == 0 {
                return Err(Error::Event("table ID"));
            }
            Ok(table_id == expected_table && equal)
        } else {
            // Non-row images retain the original complete kind-specific validation.
            Self::decode(bytes)?;
            Ok(false)
        }
    }
}

fn compact_text(text: &mut String) {
    if text.capacity() != text.len() {
        // Box<str> conversion discards spare capacity; into_string exposes only
        // its exact length. Unlike shrink_to_fit, the returned shape is exact.
        *text = std::mem::take(text).into_boxed_str().into_string();
    }
}

fn compact_vector<T>(vector: &mut Vec<T>) {
    if vector.capacity() != vector.len() {
        *vector = std::mem::take(vector).into_boxed_slice().into_vec();
    }
}

#[cfg(test)]
#[path = "event_capacity_tests.rs"]
mod capacity_tests;

#[cfg(test)]
#[path = "row_image_tests.rs"]
mod row_image_tests;

fn envelope(bytes: &[u8]) -> Result<(u8, u64, &[u8])> {
    if bytes.len() < PREFIX_SIZE || bytes.len() > MAX_RECORD_SIZE || &bytes[..4] != b"ETBL" {
        return Err(Error::Event("length or magic"));
    }
    let version = u16::from_le_bytes([bytes[4], bytes[5]]);
    if version != VERSION {
        return Err(Error::EventVersion(version));
    }
    if bytes[7] != 0 {
        return Err(Error::Event("reserved byte"));
    }
    let mut id = [0; 8];
    id.copy_from_slice(&bytes[8..PREFIX_SIZE]);
    let table_id = u64::from_le_bytes(id);
    let payload = &bytes[PREFIX_SIZE..];
    Ok((bytes[6], table_id, payload))
}
