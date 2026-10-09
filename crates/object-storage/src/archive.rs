//! Experimental checked byte format; encoding does not durably publish a backup.
use crate::directory::inventory::digest_components;
use crate::{
    Error, HEADER_BYTES, MAX_INVENTORY_BYTES, MAX_INVENTORY_OBJECTS, MAX_PAYLOAD_BYTES, ObjectId,
    ObjectSnapshot, ProjectId, Result, VerifiedObject, verify,
};
use sha2::{Digest, Sha256};

const MAGIC: &[u8; 8] = b"EMILYOBK";
pub const ARCHIVE_HEADER_BYTES: usize = 128;
pub const MAX_ARCHIVE_BYTES: usize = ARCHIVE_HEADER_BYTES
    + MAX_INVENTORY_BYTES as usize
    + MAX_INVENTORY_OBJECTS * (24 + HEADER_BYTES);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveReport {
    pub objects: usize,
    pub payload_bytes: u64,
    pub digest: [u8; 32],
}
/// Bounded readonly operator inspection only, not publication or restore.
pub fn inspect_archive_file(
    path: impl AsRef<std::path::Path>,
    project: ProjectId,
) -> Result<ArchiveReport> {
    let path = path.as_ref();
    let mut file = crate::inspect::open_private(path)?;
    let (bytes, before) = crate::inspect::read_image(&mut file, MAX_ARCHIVE_BYTES)?;
    let view = verify_archive(&bytes, project)?;
    let report = ArchiveReport {
        objects: view.objects.len(),
        payload_bytes: view.payload_bytes,
        digest: view.digest,
    };
    let after = crate::inspect::recheck(&file, &before, MAX_ARCHIVE_BYTES)?;
    crate::inspect::check_visible(path, &after)?;
    Ok(report)
}

pub struct ArchivedObject<'a> {
    image: &'a [u8],
    view: VerifiedObject<'a>,
}
impl<'a> ArchivedObject<'a> {
    pub const fn object(&self) -> ObjectId {
        self.view.object()
    }
    pub const fn payload(&self) -> &'a [u8] {
        self.view.payload()
    }
    pub const fn sha256(&self) -> &[u8; 32] {
        self.view.sha256()
    }
}
impl std::fmt::Debug for ArchivedObject<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArchivedObject")
            .field("bytes", &self.view.payload().len())
            .finish_non_exhaustive()
    }
}
/// Fully verified borrowed archive, not a user capability or persisted backup.
pub struct VerifiedArchive<'a> {
    project: ProjectId,
    payload_bytes: u64,
    digest: [u8; 32],
    objects: Vec<ArchivedObject<'a>>,
}
impl<'a> VerifiedArchive<'a> {
    pub const fn project(&self) -> ProjectId {
        self.project
    }
    pub const fn payload_bytes(&self) -> u64 {
        self.payload_bytes
    }
    pub const fn digest(&self) -> &[u8; 32] {
        &self.digest
    }
    pub fn objects(&self) -> &[ArchivedObject<'a>] {
        &self.objects
    }
}
impl std::fmt::Debug for VerifiedArchive<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VerifiedArchive")
            .field("objects", &self.objects.len())
            .field("bytes", &self.payload_bytes)
            .finish_non_exhaustive()
    }
}

pub fn encode_archive(snapshot: &ObjectSnapshot) -> Result<Vec<u8>> {
    encode_checked(
        snapshot.inventory().project(),
        snapshot.inventory().payload_bytes(),
        *snapshot.inventory().digest(),
        snapshot
            .objects()
            .iter()
            .map(|object| (object.object(), object.encoded())),
    )
}
pub fn encode_verified_archive(archive: &VerifiedArchive<'_>) -> Result<Vec<u8>> {
    encode_checked(
        archive.project,
        archive.payload_bytes,
        archive.digest,
        archive
            .objects
            .iter()
            .map(|object| (object.object(), object.image)),
    )
}
fn encode_checked<'a>(
    project: ProjectId,
    payload_bytes: u64,
    digest: [u8; 32],
    objects: impl ExactSizeIterator<Item = (ObjectId, &'a [u8])>,
) -> Result<Vec<u8>> {
    let count = objects.len();
    if count > MAX_INVENTORY_OBJECTS || payload_bytes > MAX_INVENTORY_BYTES {
        return Err(Error::Limit);
    }
    let body_bytes = payload_bytes as usize + count * (24 + HEADER_BYTES);
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(ARCHIVE_HEADER_BYTES + body_bytes)
        .map_err(|_| Error::Allocation)?;
    bytes.resize(ARCHIVE_HEADER_BYTES, 0);
    bytes[..8].copy_from_slice(MAGIC);
    bytes[8..10].copy_from_slice(&1u16.to_le_bytes());
    bytes[12..16].copy_from_slice(&(ARCHIVE_HEADER_BYTES as u32).to_le_bytes());
    bytes[16..32].copy_from_slice(project.as_bytes());
    bytes[32..36].copy_from_slice(&(count as u32).to_le_bytes());
    bytes[40..48].copy_from_slice(&payload_bytes.to_le_bytes());
    bytes[48..56].copy_from_slice(&(body_bytes as u64).to_le_bytes());
    bytes[56..88].copy_from_slice(&digest);
    for (object, image) in objects {
        bytes.extend_from_slice(object.as_bytes());
        bytes.extend_from_slice(&(image.len() as u64).to_le_bytes());
        bytes.extend_from_slice(image);
    }
    if bytes.len() != ARCHIVE_HEADER_BYTES + body_bytes {
        return Err(Error::Archive);
    }
    let body_hash = Sha256::digest(&bytes[ARCHIVE_HEADER_BYTES..]);
    bytes[88..120].copy_from_slice(&body_hash);
    let crc = crc32fast::hash(&bytes[..124]);
    bytes[124..128].copy_from_slice(&crc.to_le_bytes());
    Ok(bytes)
}

pub fn verify_archive(bytes: &[u8], project: ProjectId) -> Result<VerifiedArchive<'_>> {
    if bytes.len() > MAX_ARCHIVE_BYTES {
        return Err(Error::Limit);
    }
    if bytes.len() < ARCHIVE_HEADER_BYTES {
        return Err(Error::Archive);
    }
    let u32_at = |start| {
        u32::from_le_bytes([
            bytes[start],
            bytes[start + 1],
            bytes[start + 2],
            bytes[start + 3],
        ])
    };
    let u64_at = |start| {
        u64::from_le_bytes([
            bytes[start],
            bytes[start + 1],
            bytes[start + 2],
            bytes[start + 3],
            bytes[start + 4],
            bytes[start + 5],
            bytes[start + 6],
            bytes[start + 7],
        ])
    };
    if crc32fast::hash(&bytes[..124]) != u32_at(124) {
        return Err(Error::ArchiveChecksum);
    }
    if &bytes[..8] != MAGIC {
        return Err(Error::Archive);
    }
    let version = u16::from_le_bytes([bytes[8], bytes[9]]);
    if version != 1 {
        return Err(Error::ArchiveVersion(version));
    }
    if bytes[10..12] != [0; 2]
        || u32_at(12) != ARCHIVE_HEADER_BYTES as u32
        || bytes[36..40] != [0; 4]
        || bytes[120..124] != [0; 4]
    {
        return Err(Error::Archive);
    }
    if &bytes[16..32] != project.as_bytes() {
        return Err(Error::Scope);
    }
    let count = u32_at(32) as usize;
    let payload_bytes = u64_at(40);
    let body_bytes = u64_at(48);
    if count > MAX_INVENTORY_OBJECTS
        || payload_bytes > MAX_INVENTORY_BYTES
        || body_bytes > (MAX_ARCHIVE_BYTES - ARCHIVE_HEADER_BYTES) as u64
    {
        return Err(Error::Limit);
    }
    if body_bytes != (bytes.len() - ARCHIVE_HEADER_BYTES) as u64 {
        return Err(Error::Archive);
    }
    let body = &bytes[ARCHIVE_HEADER_BYTES..];
    if Sha256::digest(body).as_slice() != &bytes[88..120] {
        return Err(Error::ArchiveChecksum);
    }
    let mut objects = Vec::new();
    objects
        .try_reserve_exact(count)
        .map_err(|_| Error::Allocation)?;
    let mut cursor = 0usize;
    let mut total = 0u64;
    let mut previous: Option<ObjectId> = None;
    for _ in 0..count {
        let frame = body
            .get(cursor..cursor.checked_add(24).ok_or(Error::Archive)?)
            .ok_or(Error::Archive)?;
        let mut id = [0; 16];
        id.copy_from_slice(&frame[..16]);
        let object = ObjectId::from_bytes(id);
        if previous.is_some_and(|old| old.as_bytes() >= object.as_bytes()) {
            return Err(Error::Archive);
        }
        previous = Some(object);
        let mut length = [0; 8];
        length.copy_from_slice(&frame[16..24]);
        let length = u64::from_le_bytes(length);
        if length > (HEADER_BYTES + MAX_PAYLOAD_BYTES) as u64 {
            return Err(Error::Limit);
        }
        if length < HEADER_BYTES as u64 {
            return Err(Error::Archive);
        }
        cursor = cursor.checked_add(24).ok_or(Error::Archive)?;
        let end = cursor.checked_add(length as usize).ok_or(Error::Archive)?;
        let image = body.get(cursor..end).ok_or(Error::Archive)?;
        let view = verify(image, project, object)?;
        total = total
            .checked_add(view.payload().len() as u64)
            .ok_or(Error::Limit)?;
        if total > MAX_INVENTORY_BYTES {
            return Err(Error::Limit);
        }
        objects.push(ArchivedObject { image, view });
        cursor = end;
    }
    if cursor != body.len() || total != payload_bytes {
        return Err(Error::Archive);
    }
    let mut digest = [0; 32];
    digest.copy_from_slice(&bytes[56..88]);
    let expected = digest_components(
        project,
        count as u32,
        total,
        objects.iter().map(|object| {
            (
                object.object(),
                object.payload().len() as u64,
                *object.sha256(),
            )
        }),
    );
    if expected != digest {
        return Err(Error::ArchiveChecksum);
    }
    Ok(VerifiedArchive {
        project,
        payload_bytes,
        digest,
        objects,
    })
}

#[cfg(test)]
mod tests;
