use crate::{Error, MAX_METADATA_BYTES, MAX_PROJECT_NAME_BYTES, Result};
use emilybase_auth::KeyDigest;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Metadata {
    pub version: u16,
    pub id: String,
    pub name: String,
    pub key: KeyDigest,
    pub epoch: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    payload: Metadata,
    checksum: u32,
}

pub(crate) fn validate_name(name: &str) -> Result<()> {
    if name.trim().is_empty()
        || name.len() > MAX_PROJECT_NAME_BYTES
        || name.chars().any(char::is_control)
    {
        Err(Error::Name)
    } else {
        Ok(())
    }
}
pub(crate) fn directory(path: &Path) -> Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(Error::Path);
    }
    Ok(())
}
pub(crate) fn file(path: &Path) -> Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(Error::Path);
    }
    Ok(())
}
pub(crate) fn encoded(metadata: &Metadata) -> Result<Vec<u8>> {
    let payload = serde_json::to_vec(metadata).map_err(|_| Error::Metadata)?;
    let envelope = Envelope {
        payload: metadata.clone(),
        checksum: crc32fast::hash(&payload),
    };
    let bytes = serde_json::to_vec(&envelope).map_err(|_| Error::Metadata)?;
    if bytes.len() as u64 > MAX_METADATA_BYTES {
        return Err(Error::Limit);
    }
    Ok(bytes)
}
pub(crate) fn read(path: &Path, id: &str) -> Result<Metadata> {
    file(path)?;
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_METADATA_BYTES + 1)
        .read_to_end(&mut bytes)?;
    decode(&bytes, id)
}
pub(crate) fn decode(bytes: &[u8], id: &str) -> Result<Metadata> {
    if bytes.len() as u64 > MAX_METADATA_BYTES || !emilybase_auth::valid_project_id(id) {
        return Err(Error::Metadata);
    }
    let envelope: Envelope = serde_json::from_slice(bytes).map_err(|_| Error::Metadata)?;
    let metadata = envelope.payload;
    let payload = serde_json::to_vec(&metadata).map_err(|_| Error::Metadata)?;
    if envelope.checksum != crc32fast::hash(&payload)
        || metadata.version != 1
        || metadata.id != id
        || metadata.epoch == 0
    {
        return Err(Error::Metadata);
    }
    validate_name(&metadata.name).map_err(|_| Error::Metadata)?;
    Ok(metadata)
}
pub(crate) fn write_new(path: &Path, metadata: &Metadata) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = File::options()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(&encoded(metadata)?)?;
    file.sync_all()?;
    Ok(())
}
