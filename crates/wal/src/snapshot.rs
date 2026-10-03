use emilybase_storage::Page;

use crate::codec::{Frame, Payload, encode_metadata};
use crate::{
    DatabaseId, Error, FRAME_SIZE, HEADER_SIZE, MAX_WAL_BYTES, Result, SNAPSHOT_WAL_VERSION,
};

/// Encode a complete baseline without filesystem I/O. Table replay is a separate
/// validation layer; page validity alone does not authorize relational history.
pub fn encode_snapshot(id: DatabaseId, transaction: u64, pages: &[Page]) -> Result<Vec<u8>> {
    if pages.is_empty() || pages.len() > (MAX_WAL_BYTES - HEADER_SIZE) / FRAME_SIZE - 1 {
        return Err(Error::Limit("baseline pages"));
    }
    if pages
        .iter()
        .enumerate()
        .any(|(index, page)| page.id() != index as u64 + 1)
    {
        return Err(Error::Format("noncontiguous baseline pages"));
    }
    let header = encode_metadata(id, SNAPSHOT_WAL_VERSION, transaction, pages.len() as u32)?;
    let mut bytes = Vec::with_capacity(HEADER_SIZE + (pages.len() + 1) * FRAME_SIZE);
    bytes.extend_from_slice(&header);
    let mut digest = crc32fast::Hasher::new();
    for (index, page) in pages.iter().enumerate() {
        let frame = Frame {
            transaction,
            sequence: index as u64 + 1,
            payload: Payload::BasePage(page.clone()),
        }
        .encode_version(SNAPSHOT_WAL_VERSION)?;
        digest.update(&frame);
        bytes.extend_from_slice(&frame);
    }
    let commit = Frame {
        transaction,
        sequence: pages.len() as u64 + 1,
        payload: Payload::BaseCommit {
            count: pages.len() as u32,
            digest: digest.finalize(),
        },
    }
    .encode_version(SNAPSHOT_WAL_VERSION)?;
    bytes.extend_from_slice(&commit);
    Ok(bytes)
}
