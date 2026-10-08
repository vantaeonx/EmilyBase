//! Pure bounded policy records for a future private original-engine catalog.
//! Checksums detect corruption, not a trusted owner's intentional replacement.
use super::{BoundPolicy, MAX_DOCUMENT_BYTES, PolicyError, Result, TableContext, decode};
use emilybase_catalog::{
    MAX_ENCODED_BYTES, MAX_VALUE_BYTES, Row, Schema, Value, decode_schema, encode_row,
    encode_schema,
};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

pub const MAX_POLICY_CHUNKS: usize =
    (MAX_ENCODED_BYTES + MAX_DOCUMENT_BYTES).div_ceil(MAX_VALUE_BYTES);
const DOMAIN: &[u8] = b"emilybase-row-policy-records-v1\0";

/// Encoded original typed rows, not a database handle or authorization capability.
pub struct PolicyRecords {
    header: Row,
    chunks: Vec<Row>,
}
impl std::fmt::Debug for PolicyRecords {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PolicyRecords(redacted)")
    }
}
impl PolicyRecords {
    pub fn header(&self) -> &[Value] {
        &self.header
    }
    pub fn chunks(&self) -> impl ExactSizeIterator<Item = &[Value]> {
        self.chunks.iter().map(Vec::as_slice)
    }
    /// The trusted catalog writer must commit all returned rows atomically.
    pub fn into_rows(self) -> (Row, Vec<Row>) {
        (self.header, self.chunks)
    }
}
/// Integrity-checked schema-bound metadata. Still grants no storage authority.
pub struct DecodedPolicy {
    pub table: u64,
    pub revision: u64,
    pub previous: u64,
    pub sha256: [u8; 32],
    pub schema: Schema,
    pub policy: BoundPolicy,
    document: Vec<u8>,
}
impl std::fmt::Debug for DecodedPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DecodedPolicyRecords(redacted)")
    }
}
impl DecodedPolicy {
    /// Exact original bytes, for trusted comparison/explicit retry, never logs.
    pub fn document(&self) -> &[u8] {
        &self.document
    }
}
fn invalid<T>() -> Result<T> {
    Err(PolicyError::Document)
}
fn decimal(value: &Value, minimum: u64) -> Result<u64> {
    let Value::Text(text) = value else {
        return invalid();
    };
    if text.len() > 20 {
        return invalid();
    }
    let number = text.parse::<u64>().map_err(|_| PolicyError::Document)?;
    if number < minimum || number.to_string() != *text {
        return invalid();
    }
    Ok(number)
}
fn revision(value: u64, previous: u64) -> Result<()> {
    if value < 2 || previous == 1 || previous >= value {
        return invalid();
    }
    Ok(())
}
fn digest(
    project: &str,
    table: u64,
    revision: u64,
    previous: u64,
    schema_len: usize,
    document_len: usize,
    body: &[u8],
) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(DOMAIN);
    hash.update(project.as_bytes());
    hash.update(table.to_be_bytes());
    hash.update(revision.to_be_bytes());
    hash.update(previous.to_be_bytes());
    hash.update((schema_len as u32).to_be_bytes());
    hash.update((document_len as u32).to_be_bytes());
    hash.update(body);
    hash.finalize().into()
}
/// Validate before owned fragmentation. No I/O, revision allocation or commit.
pub fn encode(
    context: TableContext<'_>,
    revision_id: u64,
    previous: u64,
    document: &[u8],
) -> Result<PolicyRecords> {
    revision(revision_id, previous)?;
    let definition = decode(document)?;
    let _ = BoundPolicy::compile(context, &definition)?;
    let schema = encode_schema(context.schema).map_err(|_| PolicyError::Schema)?;
    let schema_len = schema.len();
    let mut body = schema;
    body.extend_from_slice(document);
    let checksum = digest(
        context.project,
        context.id,
        revision_id,
        previous,
        schema_len,
        document.len(),
        &body,
    );
    let header = vec![
        Value::Text(context.id.to_string()),
        Value::Integer(1),
        Value::Text(context.project.into()),
        Value::Text(revision_id.to_string()),
        Value::Text(previous.to_string()),
        Value::Integer(schema_len as i64),
        Value::Integer(document.len() as i64),
        Value::Bytes(checksum.to_vec()),
    ];
    encode_row(&header).map_err(|_| PolicyError::Document)?;
    let chunks = body
        .chunks(MAX_VALUE_BYTES)
        .enumerate()
        .map(|(index, bytes)| {
            let row = vec![
                Value::Text(format!("{}:{index}", context.id)),
                Value::Bytes(bytes.to_vec()),
            ];
            encode_row(&row).map_err(|_| PolicyError::Document)?;
            Ok(row)
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(PolicyRecords { header, chunks })
}
/// Inspect a bounded ordered complete record group, then compile its exact schema.
/// Expected project comes from a trusted private catalog scope, never the header.
pub fn inspect<'a>(
    expected_project: &str,
    header: &[Value],
    chunks: impl IntoIterator<Item = &'a [Value]>,
) -> Result<DecodedPolicy> {
    if !crate::valid_project_id(expected_project) {
        return Err(PolicyError::Scope);
    }
    let [
        table,
        Value::Integer(1),
        Value::Text(project),
        revision_id,
        previous_id,
        Value::Integer(schema_len),
        Value::Integer(document_len),
        Value::Bytes(checksum),
    ] = header
    else {
        return invalid();
    };
    if project != expected_project {
        return Err(PolicyError::Scope);
    }
    let table = decimal(table, 1)?;
    let revision_id = decimal(revision_id, 2)?;
    let previous = decimal(previous_id, 0)?;
    revision(revision_id, previous)?;
    let schema_len = usize::try_from(*schema_len).map_err(|_| PolicyError::Document)?;
    let document_len = usize::try_from(*document_len).map_err(|_| PolicyError::Document)?;
    if !(1..=MAX_ENCODED_BYTES).contains(&schema_len)
        || !(1..=MAX_DOCUMENT_BYTES).contains(&document_len)
        || checksum.len() != 32
    {
        return invalid();
    }
    let total = schema_len
        .checked_add(document_len)
        .ok_or(PolicyError::Document)?;
    let count = total.div_ceil(MAX_VALUE_BYTES);
    if count > MAX_POLICY_CHUNKS {
        return invalid();
    }
    let mut body = Vec::with_capacity(total);
    let mut chunks = chunks.into_iter();
    for index in 0..count {
        let [Value::Text(key), Value::Bytes(bytes)] = chunks.next().ok_or(PolicyError::Document)?
        else {
            return invalid();
        };
        let expected = (total - body.len()).min(MAX_VALUE_BYTES);
        if *key != format!("{table}:{index}") || bytes.len() != expected {
            return invalid();
        }
        body.extend_from_slice(bytes);
    }
    if chunks.next().is_some() {
        return invalid();
    }
    let expected = digest(
        project,
        table,
        revision_id,
        previous,
        schema_len,
        document_len,
        &body,
    );
    if !bool::from(checksum.as_slice().ct_eq(&expected)) {
        return invalid();
    }
    let schema = decode_schema(&body[..schema_len]).map_err(|_| PolicyError::Schema)?;
    let document = body[schema_len..].to_vec();
    let definition = decode(&document)?;
    let policy = BoundPolicy::compile(
        TableContext {
            project,
            id: table,
            schema: &schema,
        },
        &definition,
    )?;
    Ok(DecodedPolicy {
        table,
        revision: revision_id,
        previous,
        sha256: expected,
        schema,
        policy,
        document,
    })
}

fn schema(name: &str, fields: &[(&str, emilybase_catalog::DataType)]) -> Schema {
    Schema {
        name: name.into(),
        columns: fields
            .iter()
            .map(|(name, data_type)| emilybase_catalog::Column {
                name: (*name).into(),
                data_type: *data_type,
                nullable: false,
            })
            .collect(),
        primary_key: 0,
    }
}
/// Exact future private catalog schemas; this does not create them on disk.
pub fn header_schema() -> Schema {
    use emilybase_catalog::DataType::{Bytes, Integer, Text};
    schema(
        "auth_row_policy_headers",
        &[
            ("table", Text),
            ("version", Integer),
            ("project", Text),
            ("revision", Text),
            ("previous", Text),
            ("schema_length", Integer),
            ("document_length", Integer),
            ("sha256", Bytes),
        ],
    )
}
pub fn chunk_schema() -> Schema {
    use emilybase_catalog::DataType::{Bytes, Text};
    schema(
        "auth_row_policy_chunks",
        &[("key", Text), ("payload", Bytes)],
    )
}
#[cfg(test)]
mod tests;
