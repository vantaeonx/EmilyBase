use crate::{Error, FileId, FileInfo, FileQuota, MAX_FILE_NAME_BYTES, Result};
use emilybase_catalog::{Column, DataType, Key, Row, Schema, Value};
use emilybase_database::Snapshot;
use emilybase_object_storage::{
    FileReport, Inventory, MAX_INVENTORY_OBJECTS, MAX_PAYLOAD_BYTES, ObjectId, ProjectId,
};
use emilybase_transactions::Database;
use std::collections::BTreeSet;

pub(crate) const SCOPE: &str = "file_scope";
pub(crate) const FILES: &str = "file_references";
fn schema(name: &str, columns: &[(&str, DataType)]) -> Schema {
    Schema {
        name: name.into(),
        primary_key: 0,
        columns: columns
            .iter()
            .map(|(name, kind)| Column {
                name: (*name).into(),
                data_type: *kind,
                nullable: false,
            })
            .collect(),
    }
}
pub(crate) fn scope_schema() -> Schema {
    schema(
        SCOPE,
        &[
            ("id", DataType::Integer),
            ("version", DataType::Integer),
            ("project", DataType::Bytes),
            ("database", DataType::Bytes),
            ("objects", DataType::Integer),
            ("bytes", DataType::Integer),
        ],
    )
}
pub(crate) fn file_schema() -> Schema {
    schema(
        FILES,
        &[
            ("id", DataType::Text),
            ("object", DataType::Bytes),
            ("owner", DataType::Bytes),
            ("name", DataType::Text),
            ("bytes", DataType::Integer),
            ("hash", DataType::Bytes),
            ("revision", DataType::Bytes),
        ],
    )
}
pub(crate) fn scope_row(project: ProjectId, database: [u8; 16], quota: FileQuota) -> Row {
    vec![
        Value::Integer(1),
        Value::Integer(1),
        Value::Bytes(project.as_bytes().to_vec()),
        Value::Bytes(database.to_vec()),
        Value::Integer(quota.objects() as i64),
        Value::Integer(quota.payload_bytes() as i64),
    ]
}
pub(crate) fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > MAX_FILE_NAME_BYTES || name.chars().any(char::is_control) {
        return Err(Error::Name);
    }
    Ok(())
}
pub(crate) fn encode(info: &FileInfo) -> Row {
    vec![
        Value::Text(info.id.to_string()),
        Value::Bytes(info.object.as_bytes().to_vec()),
        Value::Bytes(info.owner.to_vec()),
        Value::Text(info.name.clone()),
        Value::Integer(info.report.payload_bytes as i64),
        Value::Bytes(info.report.sha256.to_vec()),
        Value::Bytes(info.revision.to_le_bytes().to_vec()),
    ]
}
pub(crate) fn decode(row: &[Value], last: u64) -> Result<FileInfo> {
    let [
        Value::Text(id),
        Value::Bytes(object),
        Value::Bytes(owner),
        Value::Text(name),
        Value::Integer(bytes),
        Value::Bytes(hash),
        Value::Bytes(revision),
    ] = row
    else {
        return Err(Error::Corrupt);
    };
    let id: FileId = id.parse().map_err(|_| Error::Corrupt)?;
    let object = ObjectId::from_bytes(object.as_slice().try_into().map_err(|_| Error::Corrupt)?);
    let owner = owner.as_slice().try_into().map_err(|_| Error::Corrupt)?;
    let sha256 = hash.as_slice().try_into().map_err(|_| Error::Corrupt)?;
    let revision = u64::from_le_bytes(revision.as_slice().try_into().map_err(|_| Error::Corrupt)?);
    validate_name(name).map_err(|_| Error::Corrupt)?;
    if *bytes < 0 || *bytes as u64 > MAX_PAYLOAD_BYTES as u64 || revision == 0 || revision > last {
        return Err(Error::Corrupt);
    }
    Ok(FileInfo {
        id,
        object,
        owner,
        name: name.clone(),
        report: FileReport {
            payload_bytes: *bytes as usize,
            sha256,
        },
        revision,
    })
}
pub(crate) fn metadata(
    database: &Database,
    project: ProjectId,
) -> Result<(FileQuota, Vec<FileInfo>)> {
    metadata_image(
        database.view()?,
        database.database_id(),
        database.last_transaction(),
        project,
    )
}
/// Shared exact schema/scope validation for a live owner and a replayed immutable
/// backup. A decoded image grants no filesystem or current user authority.
pub(crate) fn metadata_image(
    view: &Snapshot,
    database_id: [u8; 16],
    last_transaction: u64,
    project: ProjectId,
) -> Result<(FileQuota, Vec<FileInfo>)> {
    let invalid = || Error::Corrupt;
    if view.table_count() != 2
        || view.schema(SCOPE).map_err(|_| invalid())? != &scope_schema()
        || view.schema(FILES).map_err(|_| invalid())? != &file_schema()
        || view.row_count() > MAX_INVENTORY_OBJECTS + 1
    {
        return Err(Error::Corrupt);
    }
    let rows = view.scan(SCOPE, 2).map_err(|_| invalid())?;
    let [row] = rows.as_slice() else {
        return Err(Error::Corrupt);
    };
    let [
        Value::Integer(1),
        Value::Integer(1),
        Value::Bytes(stored_project),
        Value::Bytes(stored_database),
        Value::Integer(objects),
        Value::Integer(bytes),
    ] = row.as_slice()
    else {
        return Err(Error::Corrupt);
    };
    if stored_project.as_slice() != project.as_bytes() || stored_database.as_slice() != database_id
    {
        return Err(Error::Scope);
    }
    if *objects < 0 || *bytes < 0 {
        return Err(Error::Corrupt);
    }
    let quota = FileQuota::new(
        usize::try_from(*objects).map_err(|_| invalid())?,
        *bytes as u64,
    )
    .map_err(|_| invalid())?;
    let rows = view
        .scan(FILES, MAX_INVENTORY_OBJECTS + 1)
        .map_err(|_| invalid())?;
    if rows.len() > MAX_INVENTORY_OBJECTS || view.row_count() != rows.len() + 1 {
        return Err(Error::Corrupt);
    }
    let infos = rows
        .iter()
        .map(|r| decode(r, last_transaction))
        .collect::<Result<Vec<_>>>()?;
    let mut objects = BTreeSet::new();
    if infos
        .iter()
        .any(|info| !objects.insert(*info.object.as_bytes()))
    {
        return Err(Error::Corrupt);
    }
    Ok((quota, infos))
}
pub(crate) fn graph(quota: FileQuota, infos: &[FileInfo], inventory: &Inventory) -> Result<()> {
    if inventory.entries().len() > quota.objects()
        || inventory.payload_bytes() > quota.payload_bytes()
    {
        return Err(Error::Quota);
    }
    for info in infos {
        let entry = inventory
            .entries()
            .iter()
            .find(|e| e.object() == info.object)
            .ok_or(Error::Corrupt)?;
        if entry.report() != &info.report {
            return Err(Error::Corrupt);
        }
    }
    Ok(())
}
pub(crate) fn find(view: &Snapshot, id: FileId, last: u64) -> Result<Option<FileInfo>> {
    view.get(FILES, &Key::Text(id.to_string()))
        .map_err(|_| Error::Corrupt)?
        .map(|row| decode(row, last))
        .transpose()
}
