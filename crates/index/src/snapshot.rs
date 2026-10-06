use crate::{BPlusTree, Error, IndexPage, MAX_INDEX_ENTRIES, MAX_INDEX_PAGES, PAGE_SIZE, Result};

pub const SNAPSHOT_VERSION: u16 = 1;
pub const MAX_SNAPSHOT_BYTES: usize = (MAX_INDEX_PAGES + 1) * PAGE_SIZE;

/// A bounded standalone index envelope. Revision is local, not a table/WAL transaction ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexSnapshot {
    pub revision: u64,
    pub tree: BPlusTree,
}

impl IndexSnapshot {
    /// Complete admission with one temporary physical page at a time. Topology
    /// scratch remains bounded by the arena; no complete image set is created.
    pub fn validate(&self) -> Result<()> {
        Self::validate_tree(self.revision, &self.tree)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        Self::validate_structure(self.revision, &self.tree)?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact((self.tree.page_count() + 1) * PAGE_SIZE)
            .map_err(|_| Error::Allocation)?;
        bytes.extend_from_slice(&self.header(self.tree.page_count()));
        for page in self.tree.pages.values() {
            bytes.extend_from_slice(&Self::checked_image(page)?);
        }
        Ok(bytes)
    }

    /// Shared by hashing, encoding and borrowed delta-target admission.
    pub(crate) fn validate_tree(revision: u64, tree: &BPlusTree) -> Result<()> {
        Self::validate_structure(revision, tree)?;
        for page in tree.pages.values() {
            Self::checked_image(page)?;
        }
        Ok(())
    }

    pub(crate) fn validate_structure(revision: u64, tree: &BPlusTree) -> Result<()> {
        if revision == 0 || !tree.has_stable_ids() {
            return Err(Error::Layout("snapshot requires revision and stable IDs"));
        }
        if tree.page_count() == 0 || tree.page_count() > MAX_INDEX_PAGES {
            return Err(Error::Limit);
        }
        for (id, page) in &tree.pages {
            if *id == 0 || *id > MAX_INDEX_PAGES as u64 || *id != page.id() {
                return Err(Error::PageId);
            }
        }
        if tree.validate()? != tree.len() {
            return Err(Error::Layout("snapshot entry count"));
        }
        Ok(())
    }

    /// Preserve original local wire/CRC admission without cloning the complete
    /// arena. Topology and bounded map/page identities are checked separately.
    pub(crate) fn checked_image(page: &IndexPage) -> Result<[u8; PAGE_SIZE]> {
        let image = page.encode()?;
        if IndexPage::decode(&image, page.id())? != *page {
            return Err(Error::Layout("snapshot page round trip"));
        }
        Ok(image)
    }

    /// Called only after image admission; no untrusted count reaches this helper.
    pub(crate) fn header(&self, image_count: usize) -> [u8; PAGE_SIZE] {
        let mut bytes = [0; PAGE_SIZE];
        bytes[..8].copy_from_slice(b"EBIF\0\0\0\0");
        bytes[8..10].copy_from_slice(&SNAPSHOT_VERSION.to_le_bytes());
        bytes[12..16].copy_from_slice(&(PAGE_SIZE as u32).to_le_bytes());
        bytes[16..24].copy_from_slice(&self.revision.to_le_bytes());
        bytes[24..32].copy_from_slice(&self.tree.root_id().to_le_bytes());
        bytes[32..36].copy_from_slice(&(image_count as u32).to_le_bytes());
        bytes[40..48].copy_from_slice(&(self.tree.len() as u64).to_le_bytes());
        let checksum = header_crc(&bytes);
        bytes[60..64].copy_from_slice(&checksum.to_le_bytes());
        bytes
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if !(2 * PAGE_SIZE..=MAX_SNAPSHOT_BYTES).contains(&bytes.len())
            || !bytes.len().is_multiple_of(PAGE_SIZE)
        {
            return Err(Error::Layout("snapshot length"));
        }
        if &bytes[..8] != b"EBIF\0\0\0\0" {
            return Err(Error::Magic);
        }
        let version = u16::from_le_bytes([bytes[8], bytes[9]]);
        if version != SNAPSHOT_VERSION {
            return Err(Error::Version(version));
        }
        let header = &bytes[..PAGE_SIZE];
        if header_crc(header) != number32(bytes, 60)? {
            return Err(Error::Checksum);
        }
        if number32(bytes, 12)? != PAGE_SIZE as u32
            || bytes[10..12]
                .iter()
                .chain(&bytes[36..40])
                .chain(&bytes[48..60])
                .chain(&bytes[64..PAGE_SIZE])
                .any(|byte| *byte != 0)
        {
            return Err(Error::Layout("snapshot header"));
        }
        let revision = number64(bytes, 16)?;
        let count = number32(bytes, 32)? as usize;
        let entries = number64(bytes, 40)?;
        if revision == 0
            || count == 0
            || count > MAX_INDEX_PAGES
            || bytes.len() != (count + 1) * PAGE_SIZE
            || entries > MAX_INDEX_ENTRIES as u64
        {
            return Err(Error::Layout("snapshot bounds"));
        }
        let images = bytes[PAGE_SIZE..].as_chunks::<PAGE_SIZE>().0;
        let tree = BPlusTree::from_stable_pages(number64(bytes, 24)?, images)?;
        if tree.len() as u64 != entries {
            return Err(Error::Layout("snapshot entry count"));
        }
        Ok(Self { revision, tree })
    }
}

fn number32(bytes: &[u8], offset: usize) -> Result<u32> {
    let array = bytes
        .get(offset..offset + 4)
        .and_then(|part| part.try_into().ok())
        .ok_or(Error::Layout("snapshot integer"))?;
    Ok(u32::from_le_bytes(array))
}
fn number64(bytes: &[u8], offset: usize) -> Result<u64> {
    let array = bytes
        .get(offset..offset + 8)
        .and_then(|part| part.try_into().ok())
        .ok_or(Error::Layout("snapshot integer"))?;
    Ok(u64::from_le_bytes(array))
}
fn header_crc(bytes: &[u8]) -> u32 {
    let mut crc = crc32fast::Hasher::new();
    crc.update(&bytes[..60]);
    crc.update(&bytes[64..PAGE_SIZE]);
    crc.finalize()
}
