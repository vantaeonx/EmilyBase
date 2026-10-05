use crate::{DatabaseId, Error, MAX_HISTORY_PAGES, MAX_PRIMARY_PAGES, Result, VERSION, codec};

pub const ADDRESS_BYTES: usize = 64;
const MAGIC: &[u8; 8] = b"EBNS\0\0\0\0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Domain {
    /// Existing append-only relational pages can contain several table histories.
    RelationalHistory = 1,
    PrimaryIndex = 2,
}

impl Domain {
    pub(crate) fn decode(value: u8) -> Result<Self> {
        match value {
            1 => Ok(Self::RelationalHistory),
            2 => Ok(Self::PrimaryIndex),
            other => Err(Error::Domain(other)),
        }
    }
}

/// The full identity is database/domain/table/page; arena holes are not revisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PageAddress {
    database: DatabaseId,
    domain: Domain,
    table: u64,
    page: u64,
}

impl PageAddress {
    pub fn history(database: DatabaseId, page: u64) -> Result<Self> {
        Self::new(database, Domain::RelationalHistory, 0, page)
    }

    pub fn primary(database: DatabaseId, table: u64, page: u64) -> Result<Self> {
        Self::new(database, Domain::PrimaryIndex, table, page)
    }

    fn new(database: DatabaseId, domain: Domain, table: u64, page: u64) -> Result<Self> {
        crate::database_id(database)?;
        let limit = match domain {
            Domain::RelationalHistory => {
                if table != 0 {
                    return Err(Error::Invalid("history table scope"));
                }
                MAX_HISTORY_PAGES
            }
            Domain::PrimaryIndex => {
                if table == 0 {
                    return Err(Error::Invalid("primary table scope"));
                }
                MAX_PRIMARY_PAGES
            }
        };
        if !(1..=limit).contains(&page) {
            return Err(Error::Invalid("page identity"));
        }
        Ok(Self {
            database,
            domain,
            table,
            page,
        })
    }

    pub fn database(self) -> DatabaseId {
        self.database
    }

    pub fn domain(self) -> Domain {
        self.domain
    }

    pub fn table(self) -> u64 {
        self.table
    }

    pub fn page(self) -> u64 {
        self.page
    }

    pub fn encode(self) -> Result<[u8; ADDRESS_BYTES]> {
        Self::new(self.database, self.domain, self.table, self.page)?;
        let mut bytes = [0; ADDRESS_BYTES];
        bytes[..8].copy_from_slice(MAGIC);
        bytes[8..10].copy_from_slice(&VERSION.to_le_bytes());
        bytes[10] = self.domain as u8;
        bytes[12..28].copy_from_slice(&self.database);
        bytes[28..36].copy_from_slice(&self.table.to_le_bytes());
        bytes[36..44].copy_from_slice(&self.page.to_le_bytes());
        codec::finish(&mut bytes);
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        codec::verify(bytes, ADDRESS_BYTES, MAGIC)?;
        if bytes[11] != 0 || bytes[44..60].iter().any(|byte| *byte != 0) {
            return Err(Error::Reserved);
        }
        Self::new(
            codec::read(bytes, 12)?,
            Domain::decode(bytes[10])?,
            codec::number(bytes, 28)?,
            codec::number(bytes, 36)?,
        )
    }
}
