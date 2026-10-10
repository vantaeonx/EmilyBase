use super::{ARCHIVE_HEADER_BYTES, ArchiveReport, header};
use crate::directory::inventory::digest_components;
use crate::{
    Error, HEADER_BYTES, MAX_INVENTORY_BYTES, MAX_PAYLOAD_BYTES, ObjectId, ProjectId, Result,
};
use sha2::{Digest, Sha256};
use std::io::{self, Read, Seek, SeekFrom};

const SCRATCH_BYTES: usize = 8192;

/// Verify a complete seekable archive from offset zero and return metadata only.
/// Two bounded passes preserve outer-checksum priority over nested decoding.
/// This is not an authorization capability, snapshot lease or network deadline.
pub fn verify_archive_reader(
    reader: &mut (impl Read + Seek),
    exact_bytes: usize,
    project: ProjectId,
) -> Result<ArchiveReport> {
    verify_with(reader, exact_bytes, project, || {})
}

pub(super) fn verify_with(
    reader: &mut (impl Read + Seek),
    exact_bytes: usize,
    project: ProjectId,
    verified_body: impl FnOnce(),
) -> Result<ArchiveReport> {
    header::check_total(exact_bytes)?;
    reader.seek(SeekFrom::Start(0))?;
    let mut bytes = [0; ARCHIVE_HEADER_BYTES];
    exact(reader, &mut bytes)?;
    let header = header::decode(&bytes, exact_bytes, project)?;
    let body_hash = hash_body(reader, header.body_bytes)?;
    if body_hash != header.body_sha256 {
        return Err(Error::ArchiveChecksum);
    }
    verified_body();
    reader.seek(SeekFrom::Start(ARCHIVE_HEADER_BYTES as u64))?;
    let mut entries = Vec::new();
    entries
        .try_reserve_exact(header.count)
        .map_err(|_| Error::Allocation)?;
    let mut consumed = 0usize;
    let mut total = 0u64;
    let mut previous: Option<ObjectId> = None;
    for _ in 0..header.count {
        if header.body_bytes - consumed < 24 {
            return Err(Error::Archive);
        }
        let mut frame = [0; 24];
        exact(reader, &mut frame)?;
        consumed += 24;
        let mut object = [0; 16];
        object.copy_from_slice(&frame[..16]);
        let object = ObjectId::from_bytes(object);
        if previous.is_some_and(|old| old.as_bytes() >= object.as_bytes()) {
            return Err(Error::Archive);
        }
        previous = Some(object);
        let mut length = [0; 8];
        length.copy_from_slice(&frame[16..]);
        let length = u64::from_le_bytes(length);
        if length > (HEADER_BYTES + MAX_PAYLOAD_BYTES) as u64 {
            return Err(Error::Limit);
        }
        if length < HEADER_BYTES as u64 || length > (header.body_bytes - consumed) as u64 {
            return Err(Error::Archive);
        }
        // A bounded subreader's EOF probe cannot consume the next frame.
        let mut image = (&mut *reader).take(length);
        let report = crate::verify_stream(&mut image, length as usize, project, object)?;
        consumed += length as usize;
        total = total
            .checked_add(report.payload_bytes as u64)
            .ok_or(Error::Limit)?;
        if total > MAX_INVENTORY_BYTES {
            return Err(Error::Limit);
        }
        entries.push((object, report.payload_bytes as u64, report.sha256));
    }
    if consumed != header.body_bytes || total != header.payload_bytes {
        return Err(Error::Archive);
    }
    if digest_components(project, header.count as u32, total, entries.into_iter()) != header.digest
    {
        return Err(Error::ArchiveChecksum);
    }
    Ok(ArchiveReport {
        objects: header.count,
        payload_bytes: total,
        digest: header.digest,
    })
}

fn exact(reader: &mut impl Read, out: &mut [u8]) -> Result<()> {
    match reader.read_exact(out) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => Err(Error::Archive),
        Err(error) => Err(error.into()),
    }
}

fn hash_body(reader: &mut impl Read, bytes: usize) -> Result<[u8; 32]> {
    let mut scratch = [0; SCRATCH_BYTES];
    let mut left = bytes;
    let mut hash = Sha256::new();
    while left > 0 {
        let length = left.min(SCRATCH_BYTES);
        exact(reader, &mut scratch[..length])?;
        hash.update(&scratch[..length]);
        left -= length;
    }
    let mut probe = [0; 1];
    loop {
        match reader.read(&mut probe) {
            Ok(0) => break,
            Ok(_) => return Err(Error::Archive),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(hash.finalize().into())
}

#[cfg(test)]
mod tests;
