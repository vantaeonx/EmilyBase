use emilybase_storage::{MAX_PAGES, PAGE_SIZE, Page};

use crate::{DatabaseId, Error, MAX_TRANSACTION_PAGES, Result, SNAPSHOT_WAL_VERSION, WAL_VERSION};

pub const HEADER_SIZE: usize = 64;
pub const FRAME_SIZE: usize = PAGE_SIZE + 64;
const PAYLOAD_OFFSET: usize = 60;
const CRC_OFFSET: usize = FRAME_SIZE - 4;

pub fn encode_header(id: DatabaseId) -> Result<[u8; HEADER_SIZE]> {
    encode_metadata(id, WAL_VERSION, 0, 0)
}

pub(crate) struct Metadata {
    pub id: DatabaseId,
    pub version: u16,
    pub base_transaction: u64,
    pub base_pages: u32,
}

pub(crate) fn encode_metadata(
    id: DatabaseId,
    version: u16,
    base_transaction: u64,
    base_pages: u32,
) -> Result<[u8; HEADER_SIZE]> {
    if id == [0; 16] {
        return Err(Error::Format("zero database identity"));
    }
    validate_metadata(version, base_transaction, base_pages)?;
    let mut bytes = [0; HEADER_SIZE];
    bytes[..8].copy_from_slice(b"EMILYWAL");
    bytes[8..10].copy_from_slice(&version.to_le_bytes());
    bytes[10..12].copy_from_slice(&(PAGE_SIZE as u16).to_le_bytes());
    bytes[12..16].copy_from_slice(&(FRAME_SIZE as u32).to_le_bytes());
    bytes[16..32].copy_from_slice(&id);
    bytes[32..40].copy_from_slice(&base_transaction.to_le_bytes());
    bytes[40..44].copy_from_slice(&base_pages.to_le_bytes());
    let crc = crc32fast::hash(&bytes[..60]);
    bytes[60..].copy_from_slice(&crc.to_le_bytes());
    Ok(bytes)
}

pub(crate) fn decode_header(bytes: &[u8]) -> Result<Metadata> {
    if bytes.len() != HEADER_SIZE || &bytes[..8] != b"EMILYWAL" {
        return Err(Error::Format("header length or magic"));
    }
    let version = u16_at(bytes, 8);
    if !matches!(version, WAL_VERSION | SNAPSHOT_WAL_VERSION) {
        return Err(Error::Version(version));
    }
    if u16_at(bytes, 10) as usize != PAGE_SIZE || u32_at(bytes, 12) as usize != FRAME_SIZE {
        return Err(Error::Format("header sizes"));
    }
    if u32_at(bytes, 60) != crc32fast::hash(&bytes[..60]) {
        return Err(Error::Checksum);
    }
    if bytes[44..60].iter().any(|&b| b != 0) {
        return Err(Error::Format("reserved header bytes"));
    }
    let mut id = [0; 16];
    id.copy_from_slice(&bytes[16..32]);
    if id == [0; 16] {
        return Err(Error::Format("zero database identity"));
    }
    let base_transaction = u64_at(bytes, 32);
    let base_pages = u32_at(bytes, 40);
    validate_metadata(version, base_transaction, base_pages)?;
    Ok(Metadata {
        id,
        version,
        base_transaction,
        base_pages,
    })
}

fn validate_metadata(version: u16, transaction: u64, pages: u32) -> Result<()> {
    match version {
        WAL_VERSION if transaction == 0 && pages == 0 => Ok(()),
        SNAPSHOT_WAL_VERSION
            if transaction > 0
                && transaction < u64::MAX
                && pages > 0
                && pages as u64 <= MAX_PAGES =>
        {
            Ok(())
        }
        WAL_VERSION | SNAPSHOT_WAL_VERSION => Err(Error::Format("baseline metadata")),
        _ => Err(Error::Version(version)),
    }
}

pub(crate) enum Payload {
    Page(Page),
    Commit { count: u32, digest: u32 },
    BasePage(Page),
    BaseCommit { count: u32, digest: u32 },
}

pub(crate) struct Frame {
    pub transaction: u64,
    pub sequence: u64,
    pub payload: Payload,
}

impl Frame {
    #[cfg(test)]
    pub fn encode(&self) -> Result<[u8; FRAME_SIZE]> {
        self.encode_version(WAL_VERSION)
    }

    pub fn encode_version(&self, version: u16) -> Result<[u8; FRAME_SIZE]> {
        if !matches!(version, WAL_VERSION | SNAPSHOT_WAL_VERSION) {
            return Err(Error::Version(version));
        }
        if self.transaction == 0 || self.sequence == 0 {
            return Err(Error::Format("zero frame identifiers"));
        }
        let mut bytes = [0; FRAME_SIZE];
        bytes[..4].copy_from_slice(b"EWFR");
        bytes[4..6].copy_from_slice(&version.to_le_bytes());
        bytes[8..16].copy_from_slice(&self.transaction.to_le_bytes());
        bytes[16..24].copy_from_slice(&self.sequence.to_le_bytes());
        match &self.payload {
            Payload::Page(page) | Payload::BasePage(page) => {
                if page.id() > MAX_PAGES {
                    return Err(Error::Limit("page ID"));
                }
                bytes[6] = if matches!(self.payload, Payload::BasePage(_)) {
                    3
                } else {
                    1
                };
                if bytes[6] == 3 && version != SNAPSHOT_WAL_VERSION {
                    return Err(Error::Format("baseline frame version"));
                }
                bytes[24..32].copy_from_slice(&page.id().to_le_bytes());
                bytes[32..36].copy_from_slice(&(PAGE_SIZE as u32).to_le_bytes());
                bytes[PAYLOAD_OFFSET..CRC_OFFSET].copy_from_slice(&page.encode());
            }
            Payload::Commit { count, digest } | Payload::BaseCommit { count, digest } => {
                let baseline = matches!(self.payload, Payload::BaseCommit { .. });
                validate_count(*count, baseline)?;
                bytes[6] = if baseline { 4 } else { 2 };
                if baseline && version != SNAPSHOT_WAL_VERSION {
                    return Err(Error::Format("baseline frame version"));
                }
                bytes[32..36].copy_from_slice(&8u32.to_le_bytes());
                bytes[60..64].copy_from_slice(&count.to_le_bytes());
                bytes[64..68].copy_from_slice(&digest.to_le_bytes());
            }
        }
        let crc = crc32fast::hash(&bytes[..CRC_OFFSET]);
        bytes[CRC_OFFSET..].copy_from_slice(&crc.to_le_bytes());
        Ok(bytes)
    }

    #[cfg(test)]
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Ok(Self::decode_versioned(bytes)?.0)
    }

    pub fn decode_versioned(bytes: &[u8]) -> Result<(Self, u16)> {
        if bytes.len() != FRAME_SIZE || &bytes[..4] != b"EWFR" {
            return Err(Error::Format("frame length or magic"));
        }
        let version = u16_at(bytes, 4);
        if !matches!(version, WAL_VERSION | SNAPSHOT_WAL_VERSION) {
            return Err(Error::Version(version));
        }
        if u32_at(bytes, CRC_OFFSET) != crc32fast::hash(&bytes[..CRC_OFFSET]) {
            return Err(Error::Checksum);
        }
        if bytes[7] != 0 || bytes[36..60].iter().any(|&b| b != 0) {
            return Err(Error::Format("reserved frame bytes"));
        }
        let transaction = u64_at(bytes, 8);
        let sequence = u64_at(bytes, 16);
        if transaction == 0 || sequence == 0 {
            return Err(Error::Format("zero frame identifiers"));
        }
        let page_id = u64_at(bytes, 24);
        let size = u32_at(bytes, 32);
        let payload = match bytes[6] {
            kind @ (1 | 3) if size as usize == PAGE_SIZE && page_id > 0 && page_id <= MAX_PAGES => {
                let page = Page::decode(&bytes[60..CRC_OFFSET], page_id)?;
                if kind == 3 {
                    if version != SNAPSHOT_WAL_VERSION {
                        return Err(Error::Format("baseline frame version"));
                    }
                    Payload::BasePage(page)
                } else {
                    Payload::Page(page)
                }
            }
            kind @ (2 | 4) if size == 8 && page_id == 0 => {
                if bytes[68..CRC_OFFSET].iter().any(|&b| b != 0) {
                    return Err(Error::Format("unused commit payload"));
                }
                let count = u32_at(bytes, 60);
                validate_count(count, kind == 4)?;
                let digest = u32_at(bytes, 64);
                if kind == 4 {
                    if version != SNAPSHOT_WAL_VERSION {
                        return Err(Error::Format("baseline frame version"));
                    }
                    Payload::BaseCommit { count, digest }
                } else {
                    Payload::Commit { count, digest }
                }
            }
            _ => return Err(Error::Format("frame kind or payload size")),
        };
        Ok((
            Self {
                transaction,
                sequence,
                payload,
            },
            version,
        ))
    }
}

fn validate_count(count: u32, baseline: bool) -> Result<()> {
    let limit = if baseline {
        MAX_PAGES as usize
    } else {
        MAX_TRANSACTION_PAGES
    };
    if count == 0 || count as usize > limit {
        Err(Error::Format("commit page count"))
    } else {
        Ok(())
    }
}

// Fixed-length callers validate buffer sizes before these private reads.
fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    let mut value = [0; 4];
    value.copy_from_slice(&bytes[offset..offset + 4]);
    u32::from_le_bytes(value)
}

fn u64_at(bytes: &[u8], offset: usize) -> u64 {
    let mut value = [0; 8];
    value.copy_from_slice(&bytes[offset..offset + 8]);
    u64::from_le_bytes(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image() -> [u8; FRAME_SIZE] {
        let mut page = Page::new(1).unwrap();
        page.insert(b"synthetic").unwrap();
        Frame {
            transaction: 1,
            sequence: 1,
            payload: Payload::Page(page),
        }
        .encode()
        .unwrap()
    }

    fn fix_crc(bytes: &mut [u8]) {
        let end = bytes.len() - 4;
        let crc = crc32fast::hash(&bytes[..end]);
        bytes[end..].copy_from_slice(&crc.to_le_bytes());
    }

    #[test]
    fn header_has_explicit_version_sizes_and_identity() {
        let header = encode_header([3; 16]).unwrap();
        assert_eq!(&header[..16], b"EMILYWAL\x01\0\0\x10\x40\x10\0\0");
        assert_eq!(decode_header(&header).unwrap().id, [3; 16]);
        for size in 0..HEADER_SIZE {
            assert!(decode_header(&header[..size]).is_err());
        }
        for (offset, value) in [(8, 2), (10, 1), (12, 1), (32, 1)] {
            let mut invalid = header;
            invalid[offset] = value;
            fix_crc(&mut invalid);
            assert!(decode_header(&invalid).is_err());
        }
    }

    #[test]
    fn valid_crc_does_not_bypass_frame_semantics() {
        let original = image();
        assert!(Frame::decode(&original).is_ok());
        for (offset, value) in [
            (4, 3),
            (6, 3),
            (7, 1),
            (8, 0),
            (16, 0),
            (24, 0),
            (32, 1),
            (36, 1),
        ] {
            let mut invalid = original;
            invalid[offset] = value;
            fix_crc(&mut invalid);
            assert!(Frame::decode(&invalid).is_err(), "field {offset}");
        }
        // Recomputed outer CRC cannot hide a damaged inner page checksum.
        let mut invalid = original;
        invalid[60 + 100] ^= 1;
        fix_crc(&mut invalid);
        assert!(matches!(Frame::decode(&invalid), Err(Error::Storage(_))));
    }

    #[test]
    fn commit_record_is_bounded_and_has_no_unused_payload() {
        let frame = Frame {
            transaction: 1,
            sequence: 2,
            payload: Payload::Commit {
                count: 1,
                digest: 7,
            },
        };
        let original = frame.encode().unwrap();
        assert!(Frame::decode(&original).is_ok());
        for (offset, value) in [(24, 1), (32, 9), (60, 0), (68, 1)] {
            let mut invalid = original;
            invalid[offset] = value;
            fix_crc(&mut invalid);
            assert!(Frame::decode(&invalid).is_err());
        }
        assert!(
            Frame {
                transaction: 1,
                sequence: 1,
                payload: Payload::Commit {
                    count: 257,
                    digest: 0
                },
            }
            .encode()
            .is_err()
        );
    }

    #[test]
    fn commits_bind_all_ordered_page_frames() {
        use crate::{encode_header, recover};
        let image = image();
        for digest in [crc32fast::hash(&image), 0] {
            let commit = Frame {
                transaction: 1,
                sequence: 2,
                payload: Payload::Commit { count: 1, digest },
            }
            .encode()
            .unwrap();
            let mut bytes = encode_header([3; 16]).unwrap().to_vec();
            bytes.extend_from_slice(&image);
            bytes.extend_from_slice(&commit);
            assert_eq!(recover(&bytes, None).is_ok(), digest != 0);
        }
        let mut bytes = encode_header([3; 16]).unwrap().to_vec();
        bytes.extend_from_slice(&image);
        bytes.extend_from_slice(&image);
        assert!(recover(&bytes, None).is_err());
    }
}
