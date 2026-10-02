use crate::header::{u16_at, u32_at};
use crate::{Error, FORMAT_VERSION, PAGE_SIZE, Result};

const HEADER_SIZE: usize = 32;
const SLOT_SIZE: usize = 6;
const MAX_SLOTS: usize = (PAGE_SIZE - HEADER_SIZE) / SLOT_SIZE;
pub const MAX_RECORD_SIZE: usize = PAGE_SIZE - HEADER_SIZE - SLOT_SIZE;
pub type SlotId = u16;

/// A bounded, compactable record page. Slot IDs survive compaction but may be reused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    id: u64,
    records: Vec<Option<Vec<u8>>>,
}

impl Page {
    pub fn new(id: u64) -> Result<Self> {
        if id == 0 {
            return Err(Error::PageId(id));
        }
        Ok(Self {
            id,
            records: Vec::new(),
        })
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn slot_count(&self) -> usize {
        self.records.len()
    }

    pub fn record_count(&self) -> usize {
        self.records.iter().filter(|r| r.is_some()).count()
    }

    pub fn free_bytes(&self) -> usize {
        let payload: usize = self.records.iter().flatten().map(Vec::len).sum();
        PAGE_SIZE - HEADER_SIZE - SLOT_SIZE * self.records.len() - payload
    }

    pub fn get(&self, slot: SlotId) -> Result<&[u8]> {
        self.records
            .get(usize::from(slot))
            .and_then(Option::as_deref)
            .ok_or(Error::Slot(slot))
    }

    pub fn insert(&mut self, value: &[u8]) -> Result<SlotId> {
        if value.len() > MAX_RECORD_SIZE {
            return Err(Error::RecordTooLarge);
        }
        let vacant = self.records.iter().position(Option::is_none);
        let overhead = if vacant.is_some() { 0 } else { SLOT_SIZE };
        if value.len() + overhead > self.free_bytes()
            || (vacant.is_none() && self.records.len() == MAX_SLOTS)
        {
            return Err(Error::PageFull);
        }
        let slot = vacant.unwrap_or(self.records.len());
        if vacant.is_some() {
            self.records[slot] = Some(value.to_vec());
        } else {
            self.records.push(Some(value.to_vec()));
        }
        Ok(slot as SlotId)
    }

    pub fn update(&mut self, slot: SlotId, value: &[u8]) -> Result<()> {
        let old_len = self.get(slot)?.len();
        if value.len() > MAX_RECORD_SIZE {
            return Err(Error::RecordTooLarge);
        }
        if value.len() > self.free_bytes() + old_len {
            return Err(Error::PageFull);
        }
        self.records[usize::from(slot)] = Some(value.to_vec());
        Ok(())
    }

    pub fn delete(&mut self, slot: SlotId) -> Result<()> {
        self.get(slot)?;
        self.records[usize::from(slot)] = None;
        Ok(())
    }

    pub fn encode(&self) -> [u8; PAGE_SIZE] {
        let mut bytes = [0; PAGE_SIZE];
        bytes[..4].copy_from_slice(b"EBPG");
        put_u16(&mut bytes, 4, FORMAT_VERSION);
        put_u16(&mut bytes, 6, 1);
        bytes[8..16].copy_from_slice(&self.id.to_le_bytes());
        put_u16(&mut bytes, 16, self.records.len() as u16);
        let lower = HEADER_SIZE + SLOT_SIZE * self.records.len();
        put_u16(&mut bytes, 18, lower as u16);
        let mut upper = PAGE_SIZE;
        for (slot, record) in self.records.iter().enumerate() {
            if let Some(record) = record {
                upper -= record.len();
                bytes[upper..upper + record.len()].copy_from_slice(record);
                let entry = HEADER_SIZE + SLOT_SIZE * slot;
                put_u16(&mut bytes, entry, upper as u16);
                put_u16(&mut bytes, entry + 2, record.len() as u16);
                put_u16(&mut bytes, entry + 4, 1);
            }
        }
        put_u16(&mut bytes, 20, upper as u16);
        let crc = checksum(&bytes);
        bytes[28..32].copy_from_slice(&crc.to_le_bytes());
        bytes
    }

    /// Validate all ranges before copying payload from an untrusted page.
    pub fn decode(bytes: &[u8], expected_id: u64) -> Result<Self> {
        if bytes.len() != PAGE_SIZE {
            return Err(Error::Layout("page length"));
        }
        if &bytes[..4] != b"EBPG" {
            return Err(Error::Magic);
        }
        let version = u16_at(bytes, 4);
        if version != FORMAT_VERSION {
            return Err(Error::Version(version));
        }
        if u16_at(bytes, 6) != 1 {
            return Err(Error::Layout("unsupported page kind"));
        }
        if u32_at(bytes, 28) != checksum(bytes) {
            return Err(Error::Checksum);
        }
        let mut id_bytes = [0; 8];
        id_bytes.copy_from_slice(&bytes[8..16]);
        let id = u64::from_le_bytes(id_bytes);
        if id == 0 || id != expected_id {
            return Err(Error::PageId(id));
        }
        let count = usize::from(u16_at(bytes, 16));
        let lower = usize::from(u16_at(bytes, 18));
        let upper = usize::from(u16_at(bytes, 20));
        if count > MAX_SLOTS
            || lower != HEADER_SIZE + SLOT_SIZE * count
            || lower > upper
            || upper > PAGE_SIZE
        {
            return Err(Error::Layout("slot directory bounds"));
        }
        if bytes[22..28]
            .iter()
            .chain(bytes[lower..upper].iter())
            .any(|&b| b != 0)
        {
            return Err(Error::Layout("nonzero reserved or free bytes"));
        }
        let mut records = Vec::with_capacity(count);
        let mut ranges = Vec::with_capacity(count);
        for slot in 0..count {
            let entry = HEADER_SIZE + SLOT_SIZE * slot;
            let offset = usize::from(u16_at(bytes, entry));
            let len = usize::from(u16_at(bytes, entry + 2));
            match u16_at(bytes, entry + 4) {
                0 if offset == 0 && len == 0 => records.push(None),
                1 if offset >= upper && offset + len <= PAGE_SIZE => {
                    ranges.push((offset, offset + len));
                    records.push(Some(bytes[offset..offset + len].to_vec()));
                }
                _ => return Err(Error::Layout("invalid slot")),
            }
        }
        ranges.sort_unstable();
        let mut cursor = upper;
        for (start, end) in ranges {
            if start != cursor {
                return Err(Error::Layout("overlapping or unpacked records"));
            }
            cursor = end;
        }
        if cursor != PAGE_SIZE {
            return Err(Error::Layout("payload coverage"));
        }
        Ok(Self { id, records })
    }
}

fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn checksum(bytes: &[u8]) -> u32 {
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(&bytes[..28]);
    hasher.update(&bytes[32..]);
    hasher.finalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compaction_preserves_slots_and_reuses_deleted_slot() {
        let mut page = Page::new(1).unwrap();
        let first = page.insert(b"one").unwrap();
        let second = page.insert(b"two").unwrap();
        page.update(first, b"longer record").unwrap();
        page.delete(first).unwrap();
        let mut page = Page::decode(&page.encode(), 1).unwrap();
        assert_eq!(page.get(second).unwrap(), b"two");
        assert!(matches!(page.get(first), Err(Error::Slot(_))));
        assert_eq!(page.insert(b"").unwrap(), first);
        assert_eq!(Page::decode(&page.encode(), 1).unwrap(), page);
    }

    #[test]
    fn failed_mutation_keeps_page_intact() {
        let mut page = Page::new(2).unwrap();
        let slot = page.insert(&vec![7; MAX_RECORD_SIZE]).unwrap();
        let before = page.encode();
        assert!(matches!(page.insert(b"x"), Err(Error::PageFull)));
        assert!(matches!(
            page.update(slot, &vec![0; MAX_RECORD_SIZE + 1]),
            Err(Error::RecordTooLarge)
        ));
        assert!(page.delete(u16::MAX).is_err());
        assert_eq!(page.encode(), before);
    }

    #[test]
    fn checksum_and_identity_are_checked() {
        let page = Page::new(1).unwrap();
        let mut bytes = page.encode();
        assert!(matches!(Page::decode(&bytes, 2), Err(Error::PageId(1))));
        bytes[4000] ^= 1;
        assert!(matches!(Page::decode(&bytes, 1), Err(Error::Checksum)));
    }

    #[test]
    fn recomputed_checksum_does_not_hide_invalid_layout() {
        let mut bytes = Page::new(1).unwrap().encode();
        put_u16(&mut bytes, 16, u16::MAX);
        let crc = checksum(&bytes);
        bytes[28..32].copy_from_slice(&crc.to_le_bytes());
        assert!(matches!(Page::decode(&bytes, 1), Err(Error::Layout(_))));
    }
}
