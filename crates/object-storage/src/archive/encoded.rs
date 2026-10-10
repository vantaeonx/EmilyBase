use super::{ARCHIVE_HEADER_BYTES, ArchiveReport, header};
use crate::{
    Error, HEADER_BYTES, MAX_INVENTORY_BYTES, MAX_INVENTORY_OBJECTS, MAX_PAYLOAD_BYTES, ObjectId,
    ObjectSnapshot, ProjectId, Result, VerifiedArchive,
};
use sha2::{Digest, Sha256};
use std::io::{self, Read, Seek, SeekFrom};

struct Part<'a> {
    start: u64,
    frame: [u8; 24],
    image: &'a [u8],
}

/// Canonical v1 bytes borrowed from an immutable, already verified snapshot.
/// Construction hashes the body once; reads/seeks allocate no payload image.
/// This reader does not publish a file, acquire authority or reserve global memory.
pub struct ArchiveReader<'a> {
    header: [u8; ARCHIVE_HEADER_BYTES],
    parts: Vec<Part<'a>>,
    report: ArchiveReport,
    length: usize,
    position: u64,
}

impl<'a> ArchiveReader<'a> {
    pub fn from_snapshot(snapshot: &'a ObjectSnapshot) -> Result<Self> {
        Self::new(
            snapshot.inventory().project(),
            snapshot.inventory().payload_bytes(),
            *snapshot.inventory().digest(),
            snapshot.objects().iter().map(|o| (o.object(), o.encoded())),
        )
    }

    pub fn from_verified(archive: &'a VerifiedArchive<'_>) -> Result<Self> {
        Self::new(
            archive.project,
            archive.payload_bytes,
            archive.digest,
            archive.objects.iter().map(|o| (o.object(), o.image)),
        )
    }

    fn new(
        project: ProjectId,
        payload_bytes: u64,
        digest: [u8; 32],
        objects: impl ExactSizeIterator<Item = (ObjectId, &'a [u8])>,
    ) -> Result<Self> {
        let count = objects.len();
        if count > MAX_INVENTORY_OBJECTS || payload_bytes > MAX_INVENTORY_BYTES {
            return Err(Error::Limit);
        }
        let body_bytes = payload_bytes as usize + count * (24 + HEADER_BYTES);
        let length = ARCHIVE_HEADER_BYTES + body_bytes;
        let mut parts = Vec::new();
        parts
            .try_reserve_exact(count)
            .map_err(|_| Error::Allocation)?;
        let mut hash = Sha256::new();
        let mut start = ARCHIVE_HEADER_BYTES;
        let mut previous: Option<ObjectId> = None;
        for (id, image) in objects {
            if image.len() > HEADER_BYTES + MAX_PAYLOAD_BYTES {
                return Err(Error::Limit);
            }
            if image.len() < HEADER_BYTES
                || previous.is_some_and(|old| old.as_bytes() >= id.as_bytes())
            {
                return Err(Error::Archive);
            }
            let end = start.checked_add(24 + image.len()).ok_or(Error::Limit)?;
            if end > length {
                return Err(Error::Archive);
            }
            let mut frame = [0; 24];
            frame[..16].copy_from_slice(id.as_bytes());
            frame[16..].copy_from_slice(&(image.len() as u64).to_le_bytes());
            hash.update(frame);
            hash.update(image);
            parts.push(Part {
                start: start as u64,
                frame,
                image,
            });
            start = end;
            previous = Some(id);
        }
        if start != length || parts.len() != count {
            return Err(Error::Archive);
        }
        let header = header::encode(
            project,
            &header::Header {
                count,
                payload_bytes,
                body_bytes,
                digest,
                body_sha256: hash.finalize().into(),
            },
        );
        Ok(Self {
            header,
            parts,
            report: ArchiveReport {
                objects: count,
                payload_bytes,
                digest,
            },
            length,
            position: 0,
        })
    }

    pub const fn encoded_bytes(&self) -> usize {
        self.length
    }

    pub const fn report(&self) -> &ArchiveReport {
        &self.report
    }
}

impl Read for ArchiveReader<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let mut written = 0;
        while written < out.len() && self.position < self.length as u64 {
            let bytes = if self.position < ARCHIVE_HEADER_BYTES as u64 {
                self.header.get(self.position as usize..)
            } else {
                let index = self.parts.partition_point(|p| p.start <= self.position);
                let part = index
                    .checked_sub(1)
                    .and_then(|i| self.parts.get(i))
                    .ok_or_else(invalid_state)?;
                let offset = (self.position - part.start) as usize;
                if offset < part.frame.len() {
                    part.frame.get(offset..)
                } else {
                    part.image.get(offset - part.frame.len()..)
                }
            }
            .filter(|bytes| !bytes.is_empty())
            .ok_or_else(invalid_state)?;
            let count = bytes.len().min(out.len() - written);
            out[written..written + count].copy_from_slice(&bytes[..count]);
            written += count;
            self.position += count as u64;
        }
        Ok(written)
    }
}

impl Seek for ArchiveReader<'_> {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let next = match from {
            SeekFrom::Start(position) => Some(position),
            SeekFrom::Current(offset) => self.position.checked_add_signed(offset),
            SeekFrom::End(offset) => (self.length as u64).checked_add_signed(offset),
        }
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        self.position = next;
        Ok(next)
    }
}

impl std::fmt::Debug for ArchiveReader<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArchiveReader")
            .field("objects", &self.report.objects)
            .field("encoded_bytes", &self.length)
            .field("position", &self.position)
            .finish_non_exhaustive()
    }
}

fn invalid_state() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid archive reader state")
}

#[cfg(test)]
mod tests;
