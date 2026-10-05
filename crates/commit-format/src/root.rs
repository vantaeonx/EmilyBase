use crate::{DatabaseId, Domain, Error, MAX_LIVE_KEYS, MAX_PRIMARY_PAGES, PageAddress};
use crate::{Result, VERSION, codec};

pub const ROOT_BYTES: usize = 192;
const MAGIC: &[u8; 8] = b"EBIR\0\0\0\0";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum IndexKeyType {
    Integer = 1,
    Text = 2,
}

impl IndexKeyType {
    fn decode(value: u8) -> Result<Self> {
        match value {
            1 => Ok(Self::Integer),
            2 => Ok(Self::Text),
            other => Err(Error::KeyType(other)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Predecessor {
    revision: u64,
    transaction: u64,
    fingerprint: [u8; 32],
}

impl Predecessor {
    pub fn new(revision: u64, transaction: u64, fingerprint: [u8; 32]) -> Result<Self> {
        if revision == 0 {
            return Err(Error::Invalid("predecessor revision"));
        }
        crate::transaction(transaction)?;
        Ok(Self {
            revision,
            transaction,
            fingerprint,
        })
    }

    pub fn revision(self) -> u64 {
        self.revision
    }

    pub fn transaction(self) -> u64 {
        self.transaction
    }

    pub fn fingerprint(self) -> [u8; 32] {
        self.fingerprint
    }
}

/// Structural metadata only. Page topology and live row coverage need validation
/// against the complete selected state before any runtime commit or publication.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RootBinding {
    address: PageAddress,
    key_type: IndexKeyType,
    revision: u64,
    transaction: u64,
    covered: u64,
    excluded: u64,
    pages: u32,
    predecessor: Option<Predecessor>,
}

impl RootBinding {
    #[allow(clippy::too_many_arguments)] // The fixed codec fields are individually validated.
    pub fn new(
        address: PageAddress,
        key_type: IndexKeyType,
        revision: u64,
        transaction: u64,
        covered: u64,
        excluded: u64,
        pages: u32,
        predecessor: Option<Predecessor>,
    ) -> Result<Self> {
        if address.domain() != Domain::PrimaryIndex {
            return Err(Error::Invalid("root domain"));
        }
        crate::transaction(transaction)?;
        if revision == 0 {
            return Err(Error::Invalid("root revision"));
        }
        if !(1..=MAX_PRIMARY_PAGES).contains(&u64::from(pages)) {
            return Err(Error::Invalid("root page count"));
        }
        if covered
            .checked_add(excluded)
            .is_none_or(|total| total > MAX_LIVE_KEYS)
        {
            return Err(Error::Invalid("root key count"));
        }
        if key_type == IndexKeyType::Integer && excluded != 0 {
            return Err(Error::Invalid("integer excluded keys"));
        }
        match predecessor {
            None if revision == 1 => (),
            Some(base)
                if base.revision.checked_add(1) == Some(revision)
                    && base.transaction < transaction => {}
            _ => return Err(Error::Invalid("root predecessor")),
        }
        Ok(Self {
            address,
            key_type,
            revision,
            transaction,
            covered,
            excluded,
            pages,
            predecessor,
        })
    }

    pub fn address(self) -> PageAddress {
        self.address
    }
    pub fn key_type(self) -> IndexKeyType {
        self.key_type
    }
    pub fn revision(self) -> u64 {
        self.revision
    }
    pub fn transaction(self) -> u64 {
        self.transaction
    }
    pub fn covered(self) -> u64 {
        self.covered
    }
    pub fn excluded(self) -> u64 {
        self.excluded
    }
    pub fn pages(self) -> u32 {
        self.pages
    }
    pub fn predecessor(self) -> Option<Predecessor> {
        self.predecessor
    }

    pub fn verify_owner(self, database: DatabaseId, table: u64, transaction: u64) -> Result<()> {
        if (
            self.address.database(),
            self.address.table(),
            self.transaction,
        ) != (database, table, transaction)
        {
            return Err(Error::Identity);
        }
        Ok(())
    }

    /// The caller computes the exact canonical index-state fingerprint, not a CRC.
    /// The root page may move; the database/table/key type and exact base cannot.
    pub fn verify_predecessor(self, previous: Self, fingerprint: [u8; 32]) -> Result<()> {
        if self.address.database() != previous.address.database()
            || self.address.table() != previous.address.table()
            || self.key_type != previous.key_type
            || self.predecessor
                != Some(Predecessor {
                    revision: previous.revision,
                    transaction: previous.transaction,
                    fingerprint,
                })
        {
            return Err(Error::Predecessor);
        }
        Ok(())
    }

    pub fn encode(self) -> Result<[u8; ROOT_BYTES]> {
        let mut bytes = [0; ROOT_BYTES];
        bytes[..8].copy_from_slice(MAGIC);
        bytes[8..10].copy_from_slice(&VERSION.to_le_bytes());
        bytes[10] = self.key_type as u8;
        bytes[12..28].copy_from_slice(&self.address.database());
        bytes[28..36].copy_from_slice(&self.address.table().to_le_bytes());
        bytes[36..44].copy_from_slice(&self.address.page().to_le_bytes());
        bytes[44..52].copy_from_slice(&self.revision.to_le_bytes());
        bytes[52..60].copy_from_slice(&self.transaction.to_le_bytes());
        if let Some(base) = self.predecessor {
            bytes[60..68].copy_from_slice(&base.revision.to_le_bytes());
            bytes[68..76].copy_from_slice(&base.transaction.to_le_bytes());
            bytes[76..108].copy_from_slice(&base.fingerprint);
        }
        bytes[108..116].copy_from_slice(&self.covered.to_le_bytes());
        bytes[116..124].copy_from_slice(&self.excluded.to_le_bytes());
        bytes[124..128].copy_from_slice(&self.pages.to_le_bytes());
        codec::finish(&mut bytes);
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        codec::verify(bytes, ROOT_BYTES, MAGIC)?;
        if bytes[11] != 0 || bytes[128..188].iter().any(|byte| *byte != 0) {
            return Err(Error::Reserved);
        }
        let revision = codec::number(bytes, 60)?;
        let transaction = codec::number(bytes, 68)?;
        let fingerprint = codec::read(bytes, 76)?;
        let predecessor = if revision == 0 {
            if transaction != 0 || fingerprint != [0; 32] {
                return Err(Error::Invalid("absent predecessor fields"));
            }
            None
        } else {
            Some(Predecessor::new(revision, transaction, fingerprint)?)
        };
        Self::new(
            PageAddress::primary(
                codec::read(bytes, 12)?,
                codec::number(bytes, 28)?,
                codec::number(bytes, 36)?,
            )?,
            IndexKeyType::decode(bytes[10])?,
            codec::number(bytes, 44)?,
            codec::number(bytes, 52)?,
            codec::number(bytes, 108)?,
            codec::number(bytes, 116)?,
            u32::from_le_bytes(codec::read(bytes, 124)?),
            predecessor,
        )
    }
}
