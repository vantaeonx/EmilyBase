use super::*;
use crate::AccountFileSnapshot;
use emilybase_files::FileArchiveReader;
use std::io::{self, Read, Seek, SeekFrom};

struct Framed<'a> {
    header: [u8; FRAME],
    reader: FileArchiveReader<'a>,
    start: usize,
}
/// Borrowed canonical encoder. Retains small framing/readers and no extra complete
/// payload images or source descriptors. Encoding does not publish a destination.
pub struct AccountFileArchiveReader<'a> {
    header: [u8; HEADER],
    manifest: &'a [u8],
    accounts: &'a [u8],
    files: Vec<Framed<'a>>,
    length: usize,
    position: u64,
}
impl<'a> AccountFileArchiveReader<'a> {
    pub fn from_snapshot(snapshot: &'a AccountFileSnapshot) -> Result<Self> {
        let files = snapshot
            .files()
            .iter()
            .map(|file| Ok((file.project(), FileArchiveReader::from_snapshot(file)?)))
            .collect::<Result<Vec<_>>>()?;
        Self::new(
            snapshot.manifest_bytes(),
            snapshot.accounts().bundle_bytes(),
            files,
        )
    }
    pub fn from_verified(archive: &'a VerifiedAccountFileArchive<'_>) -> Result<Self> {
        let files = archive
            .files
            .iter()
            .map(|file| Ok((file.project(), FileArchiveReader::from_verified(file)?)))
            .collect::<Result<Vec<_>>>()?;
        Self::new(archive.manifest_bytes, archive.accounts, files)
    }
    fn new(
        manifest: &'a [u8],
        accounts: &'a [u8],
        files: Vec<(ProjectId, FileArchiveReader<'a>)>,
    ) -> Result<Self> {
        if files.len() > MAX_COMPONENT_FILE_STORES {
            return Err(Error::Limit);
        }
        let mut length = charge(HEADER, manifest.len(), MAX_ACCOUNT_FILE_ARCHIVE_BYTES)?;
        length = charge(length, accounts.len(), MAX_ACCOUNT_FILE_ARCHIVE_BYTES)?;
        let mut framed = Vec::with_capacity(files.len());
        for (project, reader) in files {
            let start = length;
            length = charge(length, FRAME, MAX_ACCOUNT_FILE_ARCHIVE_BYTES)?;
            length = charge(
                length,
                reader.encoded_bytes(),
                MAX_ACCOUNT_FILE_ARCHIVE_BYTES,
            )?;
            let mut header = [0; FRAME];
            header[..32].copy_from_slice(project.to_string().as_bytes());
            header[32..40].copy_from_slice(&(reader.encoded_bytes() as u64).to_le_bytes());
            framed.push(Framed {
                header,
                reader,
                start,
            });
        }
        let mut value = Self {
            header: [0; HEADER],
            manifest,
            accounts,
            files: framed,
            length,
            position: HEADER as u64,
        };
        let mut hash = Sha256::new();
        let mut scratch = [0; 8192];
        loop {
            let n = value.read(&mut scratch)?;
            if n == 0 {
                break;
            }
            hash.update(&scratch[..n]);
        }
        value.header[..8].copy_from_slice(MAGIC);
        value.header[8..10].copy_from_slice(&ACCOUNT_FILE_ARCHIVE_VERSION.to_le_bytes());
        value.header[10..12].copy_from_slice(&(HEADER as u16).to_le_bytes());
        value.header[12..16].copy_from_slice(&(value.files.len() as u32).to_le_bytes());
        value.header[16..20].copy_from_slice(&(manifest.len() as u32).to_le_bytes());
        value.header[24..32].copy_from_slice(&(accounts.len() as u64).to_le_bytes());
        value.header[32..40].copy_from_slice(&((length - HEADER) as u64).to_le_bytes());
        value.header[40..72].copy_from_slice(&hash.finalize());
        let crc = crc32fast::hash(&value.header[..124]);
        value.header[124..].copy_from_slice(&crc.to_le_bytes());
        value.position = 0;
        Ok(value)
    }
    pub const fn encoded_bytes(&self) -> usize {
        self.length
    }
}
impl Read for AccountFileArchiveReader<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let mut written = 0;
        while written < out.len() && self.position < self.length as u64 {
            let at = self.position as usize;
            let manifest_end = HEADER + self.manifest.len();
            let account_end = manifest_end + self.accounts.len();
            let bytes = if at < HEADER {
                Some(&self.header[at..])
            } else if at < manifest_end {
                Some(&self.manifest[at - HEADER..])
            } else if at < account_end {
                Some(&self.accounts[at - manifest_end..])
            } else {
                None
            };
            let n = if let Some(bytes) = bytes {
                let n = bytes.len().min(out.len() - written);
                out[written..written + n].copy_from_slice(&bytes[..n]);
                n
            } else {
                let file = self
                    .files
                    .iter_mut()
                    .find(|f| at >= f.start && at < f.start + FRAME + f.reader.encoded_bytes())
                    .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidData))?;
                if at < file.start + FRAME {
                    let bytes = &file.header[at - file.start..];
                    let n = bytes.len().min(out.len() - written);
                    out[written..written + n].copy_from_slice(&bytes[..n]);
                    n
                } else {
                    let offset = at - file.start - FRAME;
                    file.reader.seek(SeekFrom::Start(offset as u64))?;
                    let n = file.reader.read(&mut out[written..])?;
                    if n == 0 {
                        return Err(io::Error::from(io::ErrorKind::InvalidData));
                    }
                    n
                }
            };
            self.position += n as u64;
            written += n;
        }
        Ok(written)
    }
}
impl Seek for AccountFileArchiveReader<'_> {
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
impl std::fmt::Debug for AccountFileArchiveReader<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountFileArchiveReader")
            .field("encoded_bytes", &self.length)
            .field("position", &self.position)
            .finish_non_exhaustive()
    }
}
