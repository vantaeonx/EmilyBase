use super::{FILE_ARCHIVE_HEADER_BYTES, MAX_FILE_ARCHIVE_BYTES, VerifiedFileArchive, header};
use crate::{Error, FileSnapshot, Result};
use emilybase_object_storage::{ArchiveReader, ProjectId};
use sha2::{Digest, Sha256};
use std::io::{self, Read, Seek, SeekFrom};

/// Canonical borrowed pair encoding; never allocates another complete payload
/// image. The owners are immutable bytes, not source filesystem descriptors.
pub struct FileArchiveReader<'a> {
    header: [u8; FILE_ARCHIVE_HEADER_BYTES],
    metadata: &'a [u8],
    objects: ArchiveReader<'a>,
    length: usize,
    position: u64,
}
impl<'a> FileArchiveReader<'a> {
    pub fn from_snapshot(snapshot: &'a FileSnapshot) -> Result<Self> {
        Self::new(
            snapshot.project(),
            snapshot.metadata_bytes(),
            snapshot.metadata_report(),
            ArchiveReader::from_snapshot(snapshot.objects())?,
        )
    }
    pub fn from_verified(archive: &'a VerifiedFileArchive<'_>) -> Result<Self> {
        Self::new(
            archive.project,
            archive.metadata,
            &archive.report,
            ArchiveReader::from_verified(&archive.objects)?,
        )
    }
    fn new(
        project: ProjectId,
        metadata: &'a [u8],
        report: &emilybase_backup::Report,
        mut objects: ArchiveReader<'a>,
    ) -> Result<Self> {
        let length = FILE_ARCHIVE_HEADER_BYTES
            .checked_add(metadata.len())
            .and_then(|n| n.checked_add(objects.encoded_bytes()))
            .filter(|n| *n <= MAX_FILE_ARCHIVE_BYTES)
            .ok_or(Error::Archive)?;
        let mut hash = Sha256::new();
        let mut scratch = [0; 8192];
        loop {
            // ArchiveReader borrows already verified immutable images; its reads
            // cannot acquire or substitute filesystem owners during this hash.
            let n = objects.read(&mut scratch).map_err(|_| Error::Archive)?;
            if n == 0 {
                break;
            }
            hash.update(&scratch[..n]);
        }
        objects
            .seek(SeekFrom::Start(0))
            .map_err(|_| Error::Archive)?;
        let header = header::encode(
            project,
            &header::Header {
                database_id: report.database_id,
                last_transaction: report.last_transaction,
                metadata_bytes: metadata.len(),
                object_bytes: objects.encoded_bytes(),
                metadata_hash: Sha256::digest(metadata).into(),
                object_hash: hash.finalize().into(),
            },
        );
        Ok(Self {
            header,
            metadata,
            objects,
            length,
            position: 0,
        })
    }
    pub const fn encoded_bytes(&self) -> usize {
        self.length
    }
}
impl Read for FileArchiveReader<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let mut written = 0;
        while written < out.len() && self.position < self.length as u64 {
            let metadata_end = (FILE_ARCHIVE_HEADER_BYTES + self.metadata.len()) as u64;
            let count = if self.position < FILE_ARCHIVE_HEADER_BYTES as u64 {
                let bytes = &self.header[self.position as usize..];
                let n = bytes.len().min(out.len() - written);
                out[written..written + n].copy_from_slice(&bytes[..n]);
                n
            } else if self.position < metadata_end {
                let bytes =
                    &self.metadata[(self.position - FILE_ARCHIVE_HEADER_BYTES as u64) as usize..];
                let n = bytes.len().min(out.len() - written);
                out[written..written + n].copy_from_slice(&bytes[..n]);
                n
            } else {
                self.objects
                    .seek(SeekFrom::Start(self.position - metadata_end))?;
                let n = self.objects.read(&mut out[written..])?;
                if n == 0 {
                    return Err(io::Error::from(io::ErrorKind::InvalidData));
                }
                n
            };
            self.position += count as u64;
            written += count;
        }
        Ok(written)
    }
}
impl Seek for FileArchiveReader<'_> {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let next = match from {
            SeekFrom::Start(n) => Some(n),
            SeekFrom::Current(n) => self.position.checked_add_signed(n),
            SeekFrom::End(n) => (self.length as u64).checked_add_signed(n),
        }
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        self.position = next;
        Ok(next)
    }
}
impl std::fmt::Debug for FileArchiveReader<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileArchiveReader")
            .field("encoded_bytes", &self.length)
            .field("position", &self.position)
            .finish_non_exhaustive()
    }
}
