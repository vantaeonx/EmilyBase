use crate::{Database, Error, Result};
use emilybase_database::PrimaryIndexInfo;
use emilybase_index::IndexSnapshot;
use emilybase_wal::DatabaseId;
use sha2::{Digest, Sha256};

pub const INDEX_IMAGE_VERSION: u16 = 1;
pub const INDEX_IMAGE_HEADER: usize = 128;
pub const MAX_INDEX_IMAGE_BYTES: usize = INDEX_IMAGE_HEADER + emilybase_index::MAX_SNAPSHOT_BYTES;

/// Structural inspection excludes row keys, history fingerprint and credentials.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct IndexImageReport {
    pub transaction: u64,
    pub table_id: u64,
    pub entries: usize,
    pub pages: usize,
    pub root_id: u64,
}
struct Image {
    database_id: DatabaseId,
    history: [u8; 32],
    table_id: u64,
    index: IndexSnapshot,
}

impl Database {
    /// Export a complete eligible primary projection bound to this acknowledged view.
    /// Does not publish a file, change WAL or make indexes durable participants.
    pub fn primary_index_image(&self, table: &str) -> Result<Vec<u8>> {
        let snapshot = self.view()?;
        Image {
            database_id: self.database_id(),
            history: snapshot.page_fingerprint(),
            table_id: snapshot.table_id(table)?,
            index: IndexSnapshot {
                revision: self.last_transaction(),
                tree: snapshot.export_primary_tree(table)?,
            },
        }
        .encode()
    }
    /// Validate identity, revision, exact pages and every eligible key/pointer.
    pub fn verify_primary_index_image(
        &self,
        table: &str,
        bytes: &[u8],
    ) -> Result<IndexImageReport> {
        let image = self.bound_primary_image(table, bytes)?;
        let report = image.report();
        self.view()?.verify_primary_tree(table, &image.index.tree)?;
        Ok(report)
    }
    /// Replace only the matching derived cell after validation; no files change.
    pub fn load_primary_index_image(
        &mut self,
        table: &str,
        bytes: &[u8],
    ) -> Result<PrimaryIndexInfo> {
        let image = self.bound_primary_image(table, bytes)?;
        Ok(self
            .snapshot
            .install_primary_tree(table, image.index.tree)?)
    }
    fn bound_primary_image(&self, table: &str, bytes: &[u8]) -> Result<Image> {
        let snapshot = self.view()?;
        let image = Image::decode(bytes)?;
        if image.database_id != self.database_id() || image.table_id != snapshot.table_id(table)? {
            return Err(Error::IndexImage("database/table binding"));
        }
        if image.index.revision != self.last_transaction()
            || image.history != snapshot.page_fingerprint()
        {
            return Err(Error::IndexImage("stale acknowledged view"));
        }
        Ok(image)
    }
}
/// Structural integrity alone cannot authorize a cache or establish row liveness.
pub fn inspect_primary_index_image(bytes: &[u8]) -> Result<IndexImageReport> {
    Ok(Image::decode(bytes)?.report())
}

impl Image {
    fn report(&self) -> IndexImageReport {
        IndexImageReport {
            transaction: self.index.revision,
            table_id: self.table_id,
            entries: self.index.tree.len(),
            pages: self.index.tree.page_count(),
            root_id: self.index.tree.root_id(),
        }
    }
    fn encode(&self) -> Result<Vec<u8>> {
        if self.table_id == 0 {
            return Err(Error::IndexImage("zero table identity"));
        }
        let payload = self
            .index
            .encode()
            .map_err(|_| Error::IndexImage("invalid nested tree"))?;
        let mut bytes = vec![0; INDEX_IMAGE_HEADER];
        bytes[..8].copy_from_slice(b"EBTI\0\0\0\0");
        bytes[8..10].copy_from_slice(&INDEX_IMAGE_VERSION.to_le_bytes());
        bytes[10..12].copy_from_slice(&(INDEX_IMAGE_HEADER as u16).to_le_bytes());
        bytes[16..32].copy_from_slice(&self.database_id);
        bytes[32..40].copy_from_slice(&self.index.revision.to_le_bytes());
        bytes[40..48].copy_from_slice(&self.table_id.to_le_bytes());
        bytes[48..80].copy_from_slice(&self.history);
        bytes[80..88].copy_from_slice(&(payload.len() as u64).to_le_bytes());
        bytes[88..120].copy_from_slice(&Sha256::digest(&payload));
        let checksum = crc32fast::hash(&bytes[..124]);
        bytes[124..128].copy_from_slice(&checksum.to_le_bytes());
        bytes.extend_from_slice(&payload);
        Ok(bytes)
    }
    fn decode(bytes: &[u8]) -> Result<Self> {
        if !(INDEX_IMAGE_HEADER + 2 * emilybase_index::PAGE_SIZE..=MAX_INDEX_IMAGE_BYTES)
            .contains(&bytes.len())
        {
            return Err(Error::IndexImage("length"));
        }
        if &bytes[..8] != b"EBTI\0\0\0\0" {
            return Err(Error::IndexImage("magic"));
        }
        if u16::from_le_bytes([bytes[8], bytes[9]]) != INDEX_IMAGE_VERSION {
            return Err(Error::IndexImage("version"));
        }
        if crc32fast::hash(&bytes[..124])
            != u32::from_le_bytes(
                bytes[124..128]
                    .try_into()
                    .map_err(|_| Error::IndexImage("checksum field"))?,
            )
        {
            return Err(Error::IndexImage("header checksum"));
        }
        let size = number(bytes, 80)?;
        if u16::from_le_bytes([bytes[10], bytes[11]]) != INDEX_IMAGE_HEADER as u16
            || bytes[12..16]
                .iter()
                .chain(&bytes[120..124])
                .any(|byte| *byte != 0)
            || size != (bytes.len() - INDEX_IMAGE_HEADER) as u64
        {
            return Err(Error::IndexImage("header layout"));
        }
        let payload = &bytes[INDEX_IMAGE_HEADER..];
        let digest: [u8; 32] = Sha256::digest(payload).into();
        if digest.as_slice() != &bytes[88..120] {
            return Err(Error::IndexImage("payload checksum"));
        }
        let index = IndexSnapshot::decode(payload).map_err(|_| Error::IndexImage("nested tree"))?;
        let table_id = number(bytes, 40)?;
        if table_id == 0 || number(bytes, 32)? != index.revision {
            return Err(Error::IndexImage("revision/table identity"));
        }
        Ok(Self {
            database_id: bytes[16..32]
                .try_into()
                .map_err(|_| Error::IndexImage("database identity"))?,
            history: bytes[48..80]
                .try_into()
                .map_err(|_| Error::IndexImage("history binding"))?,
            table_id,
            index,
        })
    }
}
fn number(bytes: &[u8], offset: usize) -> Result<u64> {
    let field = bytes
        .get(offset..offset + 8)
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or(Error::IndexImage("integer"))?;
    Ok(u64::from_le_bytes(field))
}
