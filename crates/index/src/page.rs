use crate::{Error, INDEX_VERSION, Key, MAX_KEYS, PAGE_SIZE, Result, validate_key};

const HEADER: usize = 64;

/// Physical record location; the owning table must separately validate its target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordPointer {
    pub page_id: u64,
    pub slot_id: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Body {
    Leaf {
        values: Vec<RecordPointer>,
        next: Option<u64>,
    },
    Branch {
        children: Vec<u64>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexPage {
    pub(crate) id: u64,
    pub(crate) keys: Vec<Key>,
    pub(crate) body: Body,
}

impl IndexPage {
    pub fn leaf(id: u64, entries: Vec<(Key, RecordPointer)>, next: Option<u64>) -> Result<Self> {
        let (keys, values) = entries.into_iter().unzip();
        let page = Self {
            id,
            keys,
            body: Body::Leaf { values, next },
        };
        page.validate()?;
        Ok(page.compact_owned())
    }

    pub fn branch(id: u64, keys: Vec<Key>, children: Vec<u64>) -> Result<Self> {
        let page = Self {
            id,
            keys,
            body: Body::Branch { children },
        };
        page.validate()?;
        Ok(page.compact_owned())
    }

    /// Normalize owned payloads only when publishing an immutable page. Input
    /// lengths bound the wire format; caller-provided capacities must not become
    /// retained index memory. This is reported payload capacity, not an allocator
    /// usable-size or process-memory guarantee. Exact buffers keep their address.
    pub(crate) fn compact_owned(mut self) -> Self {
        for key in &mut self.keys {
            if let Key::Text(text) = key
                && text.capacity() != text.len()
            {
                *text = std::mem::take(text).into_boxed_str().into_string();
            }
        }
        compact_vector(&mut self.keys);
        match &mut self.body {
            Body::Leaf { values, .. } => compact_vector(values),
            Body::Branch { children } => compact_vector(children),
        }
        self
    }

    pub fn id(&self) -> u64 {
        self.id
    }
    pub fn keys(&self) -> &[Key] {
        &self.keys
    }
    pub fn is_leaf(&self) -> bool {
        matches!(self.body, Body::Leaf { .. })
    }

    pub fn next_leaf(&self) -> Option<u64> {
        match self.body {
            Body::Leaf { next, .. } => next,
            Body::Branch { .. } => None,
        }
    }

    pub fn children(&self) -> Option<&[u64]> {
        match &self.body {
            Body::Branch { children } => Some(children),
            _ => None,
        }
    }

    pub fn pointers(&self) -> Option<&[RecordPointer]> {
        match &self.body {
            Body::Leaf { values, .. } => Some(values),
            _ => None,
        }
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if self.id == 0 {
            return Err(Error::PageId);
        }
        if self.keys.len() > MAX_KEYS {
            return Err(Error::Limit);
        }
        for key in &self.keys {
            validate_key(key)?;
        }
        if self.keys.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(Error::Layout("unsorted or duplicate keys"));
        }
        match &self.body {
            Body::Leaf { values, next } => {
                if values.len() != self.keys.len() || values.iter().any(|v| v.page_id == 0) {
                    return Err(Error::Layout("leaf pointers"));
                }
                if matches!(next, Some(id) if *id == 0 || *id == self.id) {
                    return Err(Error::Layout("leaf successor"));
                }
            }
            Body::Branch { children } => {
                if children.len() != self.keys.len() + 1
                    || children.iter().any(|id| *id == 0 || *id == self.id)
                    || children
                        .iter()
                        .enumerate()
                        .any(|(i, id)| children[..i].contains(id))
                {
                    return Err(Error::Layout("branch children"));
                }
            }
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<[u8; PAGE_SIZE]> {
        self.validate()?;
        let mut bytes = [0; PAGE_SIZE];
        bytes[..4].copy_from_slice(b"EBIX");
        bytes[4..6].copy_from_slice(&INDEX_VERSION.to_le_bytes());
        bytes[6] = if self.is_leaf() { 1 } else { 2 };
        bytes[8..16].copy_from_slice(&self.id.to_le_bytes());
        bytes[16..18].copy_from_slice(&(self.keys.len() as u16).to_le_bytes());
        match &self.body {
            Body::Leaf { next, .. } => {
                bytes[24..32].copy_from_slice(&next.unwrap_or(0).to_le_bytes())
            }
            Body::Branch { children } => bytes[32..40].copy_from_slice(&children[0].to_le_bytes()),
        }
        let mut payload = Vec::new();
        for (i, key) in self.keys.iter().enumerate() {
            match key {
                Key::Integer(value) => {
                    payload.push(1);
                    payload.extend_from_slice(&8u16.to_le_bytes());
                    payload.extend_from_slice(&value.to_le_bytes());
                }
                Key::Text(value) => {
                    payload.push(2);
                    payload.extend_from_slice(&(value.len() as u16).to_le_bytes());
                    payload.extend_from_slice(value.as_bytes());
                }
            }
            match &self.body {
                Body::Leaf { values, .. } => {
                    payload.extend_from_slice(&values[i].page_id.to_le_bytes());
                    payload.extend_from_slice(&values[i].slot_id.to_le_bytes());
                    payload.extend_from_slice(&[0; 6]);
                }
                Body::Branch { children } => {
                    payload.extend_from_slice(&children[i + 1].to_le_bytes())
                }
            }
        }
        let end = HEADER + payload.len();
        if end > PAGE_SIZE {
            return Err(Error::Limit);
        }
        bytes[18..20].copy_from_slice(&(end as u16).to_le_bytes());
        bytes[HEADER..end].copy_from_slice(&payload);
        let crc = checksum(&bytes);
        bytes[60..64].copy_from_slice(&crc.to_le_bytes());
        Ok(bytes)
    }

    /// Decode one untrusted fixed-size image, checking local structure before allocation.
    pub fn decode(bytes: &[u8], expected_id: u64) -> Result<Self> {
        if bytes.len() != PAGE_SIZE {
            return Err(Error::Layout("page length"));
        }
        if &bytes[..4] != b"EBIX" {
            return Err(Error::Magic);
        }
        let version = u16_at(bytes, 4);
        if version != INDEX_VERSION {
            return Err(Error::Version(version));
        }
        if u32::from_le_bytes(bytes[60..64].try_into().map_err(|_| Error::Checksum)?)
            != checksum(bytes)
        {
            return Err(Error::Checksum);
        }
        let id = u64_at(bytes, 8);
        if id == 0 || id != expected_id {
            return Err(Error::PageId);
        }
        let count = usize::from(u16_at(bytes, 16));
        let end = usize::from(u16_at(bytes, 18));
        if count > MAX_KEYS || !(HEADER..=PAGE_SIZE).contains(&end) {
            return Err(Error::Layout("payload bounds"));
        }
        if bytes[7] != 0
            || bytes[20..24]
                .iter()
                .chain(bytes[40..60].iter())
                .chain(bytes[end..].iter())
                .any(|b| *b != 0)
        {
            return Err(Error::Layout("reserved or unused bytes"));
        }
        let next = u64_at(bytes, 24);
        let first = u64_at(bytes, 32);
        if !matches!((bytes[6], next, first), (1, _, 0) | (2, 0, 1..)) {
            return Err(Error::Layout("page kind or links"));
        }
        let mut reader = Reader {
            bytes: &bytes[HEADER..end],
            cursor: 0,
        };
        let mut keys = Vec::with_capacity(count);
        let mut values = Vec::with_capacity(count);
        let mut children = vec![first];
        for _ in 0..count {
            let tag = reader.take(1)?[0];
            let len = usize::from(u16_at(reader.take(2)?, 0));
            if len > crate::MAX_KEY_BYTES {
                return Err(Error::KeySize);
            }
            let data = reader.take(len)?;
            keys.push(match tag {
                1 if len == 8 => Key::Integer(i64::from_le_bytes(
                    data.try_into().map_err(|_| Error::Layout("integer"))?,
                )),
                2 => Key::Text(
                    std::str::from_utf8(data)
                        .map_err(|_| Error::Layout("UTF-8"))?
                        .to_owned(),
                ),
                _ => return Err(Error::Layout("key kind or length")),
            });
            if bytes[6] == 1 {
                let pointer = reader.take(16)?;
                if pointer[10..].iter().any(|b| *b != 0) {
                    return Err(Error::Layout("pointer reserved"));
                }
                values.push(RecordPointer {
                    page_id: u64_at(pointer, 0),
                    slot_id: u16_at(pointer, 8),
                });
            } else {
                children.push(u64_at(reader.take(8)?, 0));
            }
        }
        if reader.cursor != reader.bytes.len() {
            return Err(Error::Layout("payload coverage"));
        }
        let body = if bytes[6] == 1 {
            Body::Leaf {
                values,
                next: (next != 0).then_some(next),
            }
        } else {
            Body::Branch { children }
        };
        let page = Self { id, keys, body };
        page.validate()?;
        Ok(page.compact_owned())
    }
}

fn compact_vector<T>(values: &mut Vec<T>) {
    if values.capacity() != values.len() {
        *values = std::mem::take(values).into_boxed_slice().into_vec();
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    cursor: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self.cursor.checked_add(len).ok_or(Error::Limit)?;
        let data = self
            .bytes
            .get(self.cursor..end)
            .ok_or(Error::Layout("truncated payload"))?;
        self.cursor = end;
        Ok(data)
    }
}
fn u16_at(bytes: &[u8], start: usize) -> u16 {
    u16::from_le_bytes([bytes[start], bytes[start + 1]])
}
fn u64_at(bytes: &[u8], start: usize) -> u64 {
    let mut value = [0; 8];
    value.copy_from_slice(&bytes[start..start + 8]);
    u64::from_le_bytes(value)
}
fn checksum(bytes: &[u8]) -> u32 {
    let mut crc = crc32fast::Hasher::new();
    crc.update(&bytes[..60]);
    crc.update(&bytes[64..]);
    crc.finalize()
}
