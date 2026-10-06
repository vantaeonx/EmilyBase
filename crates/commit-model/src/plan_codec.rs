//! EBIP-1 is a bounded standalone envelope, never a runtime WAL selection.
use emilybase_commit_format::{ADDRESS_BYTES, MAX_PRIMARY_PAGES, ROOT_BYTES};
use sha2::{Digest, Sha256};

use super::*;

pub const IMAGE_PLAN_VERSION: u16 = 1;
pub const IMAGE_PLAN_HEADER_BYTES: usize = 192;
/// Independent component maxima, including every address/root and the digest.
/// This is a serialized input/output limit, not a decoded-state heap quota.
pub const IMAGE_PLAN_MAX_BYTES: usize = 9_770_208;
const DIGEST_BYTES: usize = 32;
const MAGIC: &[u8; 8] = b"EBIP\0\0\0\0";
const WRITE_BYTES: usize = ADDRESS_BYTES + PAGE_SIZE;
const CHANGE_BYTES: usize = ROOT_BYTES + 8;
const RETIRE_BYTES: usize = ROOT_BYTES + DIGEST_BYTES;

impl PlanCounts {
    /// Exact EBIP-1 size including metadata, addresses and the outer SHA-256.
    /// Arithmetic does not prove root topology, physical coverage or a commit.
    pub fn envelope_bytes(self) -> Result<u64> {
        let images = self.history.checked_add(self.primary).ok_or(Error::Limit)?;
        let length = (IMAGE_PLAN_HEADER_BYTES + DIGEST_BYTES)
            .checked_add(images.checked_mul(WRITE_BYTES).ok_or(Error::Limit)?)
            .and_then(|n| n.checked_add(self.changed_roots.checked_mul(CHANGE_BYTES)?))
            .and_then(|n| n.checked_add(self.retired_pages.checked_mul(ADDRESS_BYTES)?))
            .and_then(|n| n.checked_add(self.retired_tables.checked_mul(RETIRE_BYTES)?))
            .filter(|n| *n <= IMAGE_PLAN_MAX_BYTES)
            .ok_or(Error::Limit)?;
        Ok(length as u64)
    }
}

impl ImagePlan {
    /// Canonical standalone bytes. No path, file write, fsync or ACK is involved.
    pub fn encode(&self) -> Result<Vec<u8>> {
        let counts = self.counts()?;
        let length = usize::try_from(counts.envelope_bytes()?).map_err(|_| Error::Limit)?;
        let mut bytes = reserved(length, true)?;
        bytes.resize(IMAGE_PLAN_HEADER_BYTES, 0);
        bytes[..8].copy_from_slice(MAGIC);
        bytes[8..10].copy_from_slice(&IMAGE_PLAN_VERSION.to_le_bytes());
        bytes[10..12].copy_from_slice(&(IMAGE_PLAN_HEADER_BYTES as u16).to_le_bytes());
        bytes[16..32].copy_from_slice(&self.database);
        bytes[32..40].copy_from_slice(&self.base_transaction.to_le_bytes());
        bytes[40..48].copy_from_slice(&self.transaction.to_le_bytes());
        bytes[48..80].copy_from_slice(&self.base);
        bytes[80..112].copy_from_slice(&self.next);
        for (offset, count) in [
            (112, counts.history),
            (116, counts.changed_roots),
            (120, counts.retired_tables),
            (124, counts.primary),
            (128, counts.retired_pages),
        ] {
            bytes[offset..offset + 4].copy_from_slice(&(count as u32).to_le_bytes());
        }
        bytes[132..140].copy_from_slice(&(length as u64).to_le_bytes());
        for write in &self.history {
            encode_write(&mut bytes, write)?;
        }
        for change in &self.roots {
            bytes.extend_from_slice(&change.binding.encode()?);
            bytes.extend_from_slice(&(change.upserts.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&(change.retired.len() as u32).to_le_bytes());
            for write in &change.upserts {
                encode_write(&mut bytes, write)?;
            }
            for address in &change.retired {
                bytes.extend_from_slice(&address.encode()?);
            }
        }
        for retired in &self.retired {
            bytes.extend_from_slice(&retired.binding.encode()?);
            bytes.extend_from_slice(&retired.fingerprint);
        }
        if bytes.len().checked_add(DIGEST_BYTES) != Some(length) {
            return Err(Error::PlanLength);
        }
        let checksum = Sha256::digest(&bytes);
        bytes.extend_from_slice(&checksum);
        // Refuse noncanonical private objects too; the borrowed pass creates no
        // vectors of page images. It validates one bounded page at a time.
        let header = Header::read(&bytes)?;
        scan(&header, &bytes, false)?;
        Ok(bytes)
    }

    /// Check total/count lengths and the complete digest, then preflight all
    /// nested counts/scopes/pages before allocating owned image vectors.
    /// Replay against an exact immutable base is still mandatory afterward.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let header = Header::read(bytes)?;
        scan(&header, bytes, false)?;
        scan(&header, bytes, true)?.ok_or(Error::Plan("missing decoded envelope"))
    }
}

fn encode_write(bytes: &mut Vec<u8>, write: &PageWrite) -> Result<()> {
    bytes.extend_from_slice(&write.address.encode()?);
    bytes.extend_from_slice(&write.image);
    Ok(())
}

struct Header {
    database: DatabaseId,
    base_transaction: u64,
    transaction: u64,
    base: [u8; 32],
    next: [u8; 32],
    counts: PlanCounts,
}

impl Header {
    fn read(bytes: &[u8]) -> Result<Self> {
        if !(IMAGE_PLAN_HEADER_BYTES + DIGEST_BYTES..=IMAGE_PLAN_MAX_BYTES).contains(&bytes.len()) {
            return Err(Error::PlanLength);
        }
        let mut cursor = Cursor::new(bytes);
        if cursor.take(8)? != MAGIC {
            return Err(Error::Plan("envelope magic"));
        }
        let version = u16::from_le_bytes(cursor.array()?);
        if version != IMAGE_PLAN_VERSION {
            return Err(Error::PlanVersion(version));
        }
        if u16::from_le_bytes(cursor.array()?) as usize != IMAGE_PLAN_HEADER_BYTES {
            return Err(Error::PlanLength);
        }
        if cursor.take(4)?.iter().any(|byte| *byte != 0) {
            return Err(Error::Plan("envelope reserved fields"));
        }
        let database = cursor.array()?;
        PageAddress::history(database, 1)?;
        let base_transaction = cursor.number()?;
        let transaction = cursor.number()?;
        if base_transaction == 0
            || base_transaction.checked_add(1) != Some(transaction)
            || transaction > emilybase_commit_format::MAX_TRANSACTION
        {
            return Err(Error::Plan("envelope transaction adjacency"));
        }
        let base = cursor.array()?;
        let next = cursor.array()?;
        let history = cursor.count()?;
        let roots = cursor.count()?;
        let retired_tables = cursor.count()?;
        let primary = cursor.count()?;
        let retired_pages = cursor.count()?;
        let counts =
            PlanCounts::from_counts(history, primary, retired_pages, roots, retired_tables)?;
        if history == 0 && roots == 0 && retired_tables == 0 {
            return Err(Error::Plan("empty envelope"));
        }
        let length = cursor.number()?;
        if length != counts.envelope_bytes()? || length != bytes.len() as u64 {
            return Err(Error::PlanLength);
        }
        if cursor.take(52)?.iter().any(|byte| *byte != 0) {
            return Err(Error::Plan("envelope reserved fields"));
        }
        let body_end = bytes.len() - DIGEST_BYTES;
        if Sha256::digest(&bytes[..body_end]).as_slice() != &bytes[body_end..] {
            return Err(Error::PlanChecksum);
        }
        Ok(Self {
            database,
            base_transaction,
            transaction,
            base,
            next,
            counts,
        })
    }
}

fn reserved<T>(length: usize, materialize: bool) -> Result<Vec<T>> {
    let mut values = Vec::new();
    if materialize {
        values
            .try_reserve_exact(length)
            .map_err(|_| Error::PlanAllocation)?;
    }
    Ok(values)
}

fn scan(header: &Header, bytes: &[u8], materialize: bool) -> Result<Option<ImagePlan>> {
    let mut cursor = Cursor::new(&bytes[IMAGE_PLAN_HEADER_BYTES..bytes.len() - DIGEST_BYTES]);
    let counts = header.counts;
    let mut history = reserved(counts.history, materialize)?;
    let mut previous_page = 0;
    for _ in 0..counts.history {
        let (address, image) =
            read_write(&mut cursor, header.database, 0, Domain::RelationalHistory)?;
        if previous_page != 0 && previous_page + 1 != address.page() {
            return Err(Error::Plan("envelope history continuity"));
        }
        previous_page = address.page();
        if !materialize {
            Page::decode(image, address.page()).map_err(emilybase_database::Error::from)?;
        } else {
            history.push(PageWrite {
                address,
                image: *image,
            });
        }
    }
    let mut roots = reserved(counts.changed_roots, materialize)?;
    let mut changed_tables = [0u64; emilybase_database::MAX_TABLES];
    let mut remaining_primary = counts.primary;
    let mut remaining_retired = counts.retired_pages;
    let mut previous_table = 0;
    for table_slot in changed_tables.iter_mut().take(counts.changed_roots) {
        let binding = RootBinding::decode(cursor.take(ROOT_BYTES)?)?;
        let table = binding.address().table();
        binding.verify_owner(header.database, table, header.transaction)?;
        if table <= previous_table
            || binding
                .predecessor()
                .is_some_and(|base| base.transaction() > header.base_transaction)
        {
            return Err(Error::Plan("envelope changed root order/predecessor"));
        }
        previous_table = table;
        *table_slot = table;
        let primary = cursor.count()?;
        let retired_count = cursor.count()?;
        if primary > MAX_PRIMARY_PAGES as usize || retired_count > MAX_PRIMARY_PAGES as usize {
            return Err(Error::Limit);
        }
        remaining_primary = remaining_primary
            .checked_sub(primary)
            .ok_or(Error::PlanLength)?;
        remaining_retired = remaining_retired
            .checked_sub(retired_count)
            .ok_or(Error::PlanLength)?;
        if binding.predecessor().is_none() && retired_count != 0 {
            return Err(Error::Plan("envelope new root retirements"));
        }
        let mut upserts = reserved(primary, materialize)?;
        let mut ids = [0u64; MAX_PRIMARY_PAGES as usize];
        let mut previous_id = 0;
        for slot in ids.iter_mut().take(primary) {
            let (address, image) =
                read_write(&mut cursor, header.database, table, Domain::PrimaryIndex)?;
            if address.page() <= previous_id {
                return Err(Error::Plan("envelope index image order"));
            }
            previous_id = address.page();
            *slot = previous_id;
            if !materialize {
                IndexPage::decode(image, address.page())?;
            } else {
                upserts.push(PageWrite {
                    address,
                    image: *image,
                });
            }
        }
        let mut retired = reserved(retired_count, materialize)?;
        previous_id = 0;
        for _ in 0..retired_count {
            let address = PageAddress::decode(cursor.take(ADDRESS_BYTES)?)?;
            scope(address, header.database, table, Domain::PrimaryIndex)?;
            if address.page() <= previous_id
                || ids[..primary].binary_search(&address.page()).is_ok()
            {
                return Err(Error::Plan("envelope index retirement order/overlap"));
            }
            previous_id = address.page();
            if materialize {
                retired.push(address);
            }
        }
        if materialize {
            roots.push(RootChange {
                binding,
                upserts,
                retired,
            });
        }
    }
    if remaining_primary != 0 || remaining_retired != 0 {
        return Err(Error::PlanLength);
    }
    let mut retired = reserved(counts.retired_tables, materialize)?;
    previous_table = 0;
    for _ in 0..counts.retired_tables {
        let binding = RootBinding::decode(cursor.take(ROOT_BYTES)?)?;
        let fingerprint = cursor.array()?;
        let table = binding.address().table();
        if binding.address().database() != header.database
            || binding.transaction() > header.base_transaction
            || table <= previous_table
            || changed_tables[..counts.changed_roots]
                .binary_search(&table)
                .is_ok()
        {
            return Err(Error::Plan("envelope retired root order/scope"));
        }
        previous_table = table;
        if materialize {
            retired.push(RetiredRoot {
                binding,
                fingerprint,
            });
        }
    }
    if cursor.position != cursor.bytes.len() {
        return Err(Error::PlanLength);
    }
    Ok(materialize.then_some(ImagePlan {
        database: header.database,
        base_transaction: header.base_transaction,
        transaction: header.transaction,
        base: header.base,
        next: header.next,
        history,
        roots,
        retired,
    }))
}

fn read_write<'a>(
    cursor: &mut Cursor<'a>,
    database: DatabaseId,
    table: u64,
    domain: Domain,
) -> Result<(PageAddress, &'a [u8; PAGE_SIZE])> {
    let address = PageAddress::decode(cursor.take(ADDRESS_BYTES)?)?;
    scope(address, database, table, domain)?;
    let image = cursor
        .take(PAGE_SIZE)?
        .try_into()
        .map_err(|_| Error::PlanLength)?;
    Ok((address, image))
}

fn scope(address: PageAddress, database: DatabaseId, table: u64, domain: Domain) -> Result<()> {
    if address.database() != database || address.table() != table || address.domain() != domain {
        return Err(Error::Plan("envelope page scope"));
    }
    Ok(())
}

struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
}
impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }
    fn take(&mut self, length: usize) -> Result<&'a [u8]> {
        let end = self.position.checked_add(length).ok_or(Error::PlanLength)?;
        let value = self
            .bytes
            .get(self.position..end)
            .ok_or(Error::PlanLength)?;
        self.position = end;
        Ok(value)
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        self.take(N)?.try_into().map_err(|_| Error::PlanLength)
    }
    fn number(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.array()?))
    }
    fn count(&mut self) -> Result<usize> {
        Ok(u32::from_le_bytes(self.array()?) as usize)
    }
}
