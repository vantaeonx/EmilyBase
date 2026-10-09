//! Exact bounded reader verification; never exposes unverified payload bytes.
use crate::{Error, FileReport, HEADER_BYTES, MAX_PAYLOAD_BYTES, ObjectId, ProjectId, Result};
use sha2::{Digest, Sha256};
use std::io::{ErrorKind, Read};

const SCRATCH_BYTES: usize = 8192;

/// Verify one complete v1 envelope using a fixed 8192-byte payload scratch.
/// `bytes` is the expected exact stream length, not permission or a reservation.
/// Reads at most that length plus one EOF-probe byte; returns metadata only after
/// complete header/scope/length/hash/EOF checks. It does not provide a deadline:
/// callers must supply a suitable bounded synchronous reader, never reactor I/O.
pub fn verify_stream<R: Read>(
    reader: &mut R,
    bytes: usize,
    project: ProjectId,
    object: ObjectId,
) -> Result<FileReport> {
    if bytes > HEADER_BYTES + MAX_PAYLOAD_BYTES {
        return Err(Error::Limit);
    }
    if bytes < HEADER_BYTES {
        return Err(Error::Format);
    }
    let mut header = [0; HEADER_BYTES];
    exact(reader, &mut header)?;
    let header = crate::format::decode_header(&header, bytes as u64, project, object)?;
    let mut hash = Sha256::new();
    let mut scratch = [0; SCRATCH_BYTES];
    let mut remaining = header.payload_bytes;
    while remaining != 0 {
        let count = remaining.min(scratch.len());
        exact(reader, &mut scratch[..count])?;
        hash.update(&scratch[..count]);
        remaining -= count;
    }
    if hash.finalize().as_slice() != header.hash {
        return Err(Error::PayloadChecksum);
    }
    let mut extra = [0; 1];
    let read = loop {
        match reader.read(&mut extra) {
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            other => break other?,
        }
    };
    if read != 0 {
        return Err(Error::Format);
    }
    Ok(FileReport {
        payload_bytes: header.payload_bytes,
        sha256: header.hash,
    })
}

fn exact(reader: &mut impl Read, bytes: &mut [u8]) -> Result<()> {
    reader.read_exact(bytes).map_err(|error| {
        if error.kind() == ErrorKind::UnexpectedEof {
            Error::Format
        } else {
            Error::Io(error)
        }
    })
}

#[cfg(test)]
mod tests;
