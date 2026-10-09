use crate::{Error, Result};
use sha2::{Digest, Sha256};

const MAGIC: &[u8; 8] = b"EMILYOBJ";
pub const HEADER_BYTES: usize = 96;
pub const MAX_PAYLOAD_BYTES: usize = 8 * 1024 * 1024;

fn parse_id(text: &str) -> Result<[u8; 16]> {
    if text.len() != 32 {
        return Err(Error::Identity);
    }
    let mut id = [0; 16];
    for (output, pair) in id.iter_mut().zip(text.as_bytes().as_chunks::<2>().0) {
        let digit = |b| match b {
            b'0'..=b'9' => Ok(b - b'0'),
            b'a'..=b'f' => Ok(b - b'a' + 10),
            _ => Err(Error::Identity),
        };
        *output = (digit(pair[0])? << 4) | digit(pair[1])?;
    }
    Ok(id)
}
macro_rules! identity {
    ($name:ident) => {
        #[derive(Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $name([u8; 16]);
        impl $name {
            /// Bytes/shape are metadata, never authenticated authority.
            pub const fn from_bytes(bytes: [u8; 16]) -> Self {
                Self(bytes)
            }
            pub const fn as_bytes(&self) -> &[u8; 16] {
                &self.0
            }
        }
        impl std::str::FromStr for $name {
            type Err = Error;
            fn from_str(text: &str) -> Result<Self> {
                Ok(Self(parse_id(text)?))
            }
        }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                for b in self.0 {
                    write!(f, "{b:02x}")?;
                }
                Ok(())
            }
        }
        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, concat!(stringify!($name), "({})"), self)
            }
        }
    };
}
identity!(ProjectId);
identity!(ObjectId);

/// Borrows verified bytes; construction is private. Not a user access capability.
pub struct VerifiedObject<'a> {
    project: ProjectId,
    object: ObjectId,
    hash: [u8; 32],
    payload: &'a [u8],
}
impl std::fmt::Debug for VerifiedObject<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VerifiedObject")
            .field("bytes", &self.payload.len())
            .finish_non_exhaustive()
    }
}
impl<'a> VerifiedObject<'a> {
    pub const fn project(&self) -> ProjectId {
        self.project
    }
    pub const fn object(&self) -> ObjectId {
        self.object
    }
    pub const fn sha256(&self) -> &[u8; 32] {
        &self.hash
    }
    pub const fn payload(&self) -> &'a [u8] {
        self.payload
    }
}

/// Exact experimental v1 envelope. A byte vector is not durably published storage.
pub fn encode(project: ProjectId, object: ObjectId, payload: &[u8]) -> Result<Vec<u8>> {
    if payload.len() > MAX_PAYLOAD_BYTES {
        return Err(Error::Limit);
    }
    let size = HEADER_BYTES
        .checked_add(payload.len())
        .ok_or(Error::Limit)?;
    let mut out = Vec::new();
    out.try_reserve_exact(size).map_err(|_| Error::Allocation)?;
    out.resize(HEADER_BYTES, 0);
    out[..8].copy_from_slice(MAGIC);
    out[8..10].copy_from_slice(&1u16.to_le_bytes());
    out[12..16].copy_from_slice(&(HEADER_BYTES as u32).to_le_bytes());
    out[16..32].copy_from_slice(project.as_bytes());
    out[32..48].copy_from_slice(object.as_bytes());
    out[48..56].copy_from_slice(&(payload.len() as u64).to_le_bytes());
    out[56..88].copy_from_slice(&Sha256::digest(payload));
    let checksum = crc32fast::hash(&out[..92]);
    out[92..96].copy_from_slice(&checksum.to_le_bytes());
    out.extend_from_slice(payload);
    Ok(out)
}

/// Full header/length/payload verification before returning a borrowed view.
/// Expected scope comes from trusted caller context; header fields do not grant it.
pub fn verify(bytes: &[u8], project: ProjectId, object: ObjectId) -> Result<VerifiedObject<'_>> {
    if bytes.len() > HEADER_BYTES + MAX_PAYLOAD_BYTES {
        return Err(Error::Limit);
    }
    if bytes.len() < HEADER_BYTES {
        return Err(Error::Format);
    }
    let read_u16 = |start| u16::from_le_bytes([bytes[start], bytes[start + 1]]);
    let read_u32 = |start| {
        u32::from_le_bytes([
            bytes[start],
            bytes[start + 1],
            bytes[start + 2],
            bytes[start + 3],
        ])
    };
    if crc32fast::hash(&bytes[..92]) != read_u32(92) {
        return Err(Error::HeaderChecksum);
    }
    if &bytes[..8] != MAGIC {
        return Err(Error::Format);
    }
    if read_u16(8) != 1 {
        return Err(Error::Version(read_u16(8)));
    }
    if read_u16(10) != 0 || read_u32(12) != HEADER_BYTES as u32 || bytes[88..92] != [0; 4] {
        return Err(Error::Format);
    }
    if bytes[16..32] != project.0 || bytes[32..48] != object.0 {
        return Err(Error::Scope);
    }
    let mut length = [0; 8];
    length.copy_from_slice(&bytes[48..56]);
    let length = u64::from_le_bytes(length);
    if length > MAX_PAYLOAD_BYTES as u64 {
        return Err(Error::Limit);
    }
    if length != (bytes.len() - HEADER_BYTES) as u64 {
        return Err(Error::Format);
    }
    let mut hash = [0; 32];
    hash.copy_from_slice(&bytes[56..88]);
    let payload = &bytes[HEADER_BYTES..];
    if Sha256::digest(payload).as_slice() != hash {
        return Err(Error::PayloadChecksum);
    }
    Ok(VerifiedObject {
        project,
        object,
        hash,
        payload,
    })
}
