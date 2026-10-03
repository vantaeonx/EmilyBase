use emilybase_storage::Page;

use crate::codec::{Frame, Payload, decode_header};
use crate::{
    DatabaseId, Error, FRAME_SIZE, HEADER_SIZE, MAX_TRANSACTION_PAGES, MAX_WAL_BYTES, Result,
};

pub struct Committed {
    pub transaction: u64,
    pub pages: Vec<Page>,
}

pub struct Recovery {
    pub database_id: DatabaseId,
    pub committed: Vec<Committed>,
    pub valid_bytes: usize,
    pub discarded_bytes: usize,
    pub(crate) next_sequence: u64,
}

/// Validate the entire complete-frame prefix before exposing any commits.
/// Partial frames and valid page frames without a commit form an ignored tail.
/// A complete corrupt frame is always an error, including at the end of a log.
pub fn recover(bytes: &[u8], expected_id: Option<DatabaseId>) -> Result<Recovery> {
    if bytes.len() > MAX_WAL_BYTES {
        return Err(Error::Limit("journal bytes"));
    }
    let header = bytes
        .get(..HEADER_SIZE)
        .ok_or(Error::Format("truncated header"))?;
    let database_id = decode_header(header)?;
    if expected_id.is_some_and(|id| id != database_id) {
        return Err(Error::Identity);
    }
    let mut committed = Vec::new();
    let mut pending: Vec<Page> = Vec::new();
    let mut digest = crc32fast::Hasher::new();
    let mut transaction = 1u64;
    let mut sequence = 1u64;
    let mut committed_sequence = 1;
    let mut valid_bytes = HEADER_SIZE;
    let (frames, _) = bytes[HEADER_SIZE..].as_chunks::<FRAME_SIZE>();
    for (index, bytes) in frames.iter().enumerate() {
        let frame = Frame::decode(bytes)?;
        if frame.transaction != transaction || frame.sequence != sequence {
            return Err(Error::Format("nonsequential frame identifiers"));
        }
        sequence = sequence
            .checked_add(1)
            .ok_or(Error::Limit("sequence numbers"))?;
        match frame.payload {
            Payload::Page(page) => {
                if pending.len() >= MAX_TRANSACTION_PAGES {
                    return Err(Error::Limit("transaction pages"));
                }
                if pending.last().is_some_and(|old| old.id() >= page.id()) {
                    return Err(Error::Format("page images are not strictly ordered"));
                }
                pending.push(page);
                digest.update(bytes);
            }
            Payload::Commit {
                count,
                digest: expected,
            } => {
                if pending.len() != count as usize || digest.finalize() != expected {
                    return Err(Error::Format("commit count or digest"));
                }
                committed.push(Committed {
                    transaction,
                    pages: std::mem::take(&mut pending),
                });
                digest = crc32fast::Hasher::new();
                transaction = transaction
                    .checked_add(1)
                    .ok_or(Error::Limit("transaction identifiers"))?;
                valid_bytes = HEADER_SIZE + (index + 1) * FRAME_SIZE;
                committed_sequence = sequence;
            }
        }
    }
    Ok(Recovery {
        database_id,
        committed,
        valid_bytes,
        discarded_bytes: bytes.len() - valid_bytes,
        next_sequence: committed_sequence,
    })
}
