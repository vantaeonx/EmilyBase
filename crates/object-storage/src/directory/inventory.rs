//! Bounded complete native inventory, not a user capability or enforced quota.
use super::*;
use sha2::{Digest, Sha256};
use std::io::{Seek, SeekFrom};

pub const MAX_INVENTORY_OBJECTS: usize = 128;
pub const MAX_INVENTORY_BYTES: u64 = 64 * 1024 * 1024;
const DIGEST_DOMAIN: &[u8] = b"EmilyBase object inventory v1\0";

/// Canonical filename decoder only; never grants filesystem or user authority.
pub fn object_id_from_name(name: &[u8]) -> Result<ObjectId> {
    if name.len() != 39 || &name[32..] != b".object" {
        return Err(Error::Inventory);
    }
    let text = std::str::from_utf8(&name[..32]).map_err(|_| Error::Inventory)?;
    text.parse().map_err(|_| Error::Inventory)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryEntry {
    object: ObjectId,
    report: FileReport,
}
impl InventoryEntry {
    pub const fn object(&self) -> ObjectId {
        self.object
    }
    pub const fn report(&self) -> &FileReport {
        &self.report
    }
}
/// Complete checked metadata snapshot. Not a persistent manifest, backup,
/// authorization proof or lease protecting against later filesystem changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inventory {
    project: ProjectId,
    entries: Vec<InventoryEntry>,
    payload_bytes: u64,
    digest: [u8; 32],
}
impl Inventory {
    pub(crate) fn with_insert(&self, object: ObjectId, report: FileReport) -> Result<Self> {
        let position = match self
            .entries
            .binary_search_by_key(object.as_bytes(), |entry| *entry.object.as_bytes())
        {
            Ok(_) => return Err(Error::Exists),
            Err(position) => position,
        };
        let count = self.entries.len().checked_add(1).ok_or(Error::Limit)?;
        let payload_bytes = self
            .payload_bytes
            .checked_add(report.payload_bytes as u64)
            .ok_or(Error::Limit)?;
        if count > MAX_INVENTORY_OBJECTS
            || report.payload_bytes > MAX_PAYLOAD_BYTES
            || payload_bytes > MAX_INVENTORY_BYTES
        {
            return Err(Error::Limit);
        }
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(count)
            .map_err(|_| Error::Allocation)?;
        entries.extend_from_slice(&self.entries[..position]);
        entries.push(InventoryEntry { object, report });
        entries.extend_from_slice(&self.entries[position..]);
        let digest = digest(self.project, &entries, payload_bytes);
        Ok(Self {
            project: self.project,
            entries,
            payload_bytes,
            digest,
        })
    }
    pub const fn project(&self) -> ProjectId {
        self.project
    }
    pub fn entries(&self) -> &[InventoryEntry] {
        &self.entries
    }
    pub const fn payload_bytes(&self) -> u64 {
        self.payload_bytes
    }
    pub const fn digest(&self) -> &[u8; 32] {
        &self.digest
    }
}

/// Bounded immutable copies for a future backup encoder. Not a persisted backup
/// or global heap reservation. Later filesystem mutations cannot change bytes.
pub struct ObjectSnapshot {
    inventory: Inventory,
    objects: Vec<StoredObject>,
}
impl ObjectSnapshot {
    pub const fn inventory(&self) -> &Inventory {
        &self.inventory
    }
    pub fn objects(&self) -> &[StoredObject] {
        &self.objects
    }
}
impl std::fmt::Debug for ObjectSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ObjectSnapshot")
            .field("objects", &self.objects.len())
            .field("bytes", &self.inventory.payload_bytes)
            .finish_non_exhaustive()
    }
}

impl ProjectDirectory {
    /// Capture a newly verified complete inventory and its exact bounded bytes.
    pub fn capture(&self) -> Result<ObjectSnapshot> {
        self.capture_with(self.inventory()?, || {}, || {})
    }
    /// Require a previously observed metadata snapshot to remain current first.
    /// A matching receipt grants no additional access or filesystem authority.
    pub fn capture_inventory(&self, expected: &Inventory) -> Result<ObjectSnapshot> {
        if expected.project != self.project {
            return Err(Error::Scope);
        }
        if &self.inventory()? != expected {
            return Err(Error::InventoryChanged);
        }
        self.capture_with(expected.clone(), || {}, || {})
    }
    fn capture_with(
        &self,
        inventory: Inventory,
        checked: impl FnOnce(),
        captured: impl FnOnce(),
    ) -> Result<ObjectSnapshot> {
        checked();
        self.check()?;
        let mut objects = Vec::new();
        objects
            .try_reserve_exact(inventory.entries.len())
            .map_err(|_| Error::Allocation)?;
        for entry in &inventory.entries {
            let (_, data) = read_at(
                &self.directory,
                &object_name(entry.object),
                self.project,
                entry.object,
            )?;
            if data.report != entry.report {
                return Err(Error::InventoryChanged);
            }
            objects.push(data);
        }
        captured();
        if self.inventory()? != inventory {
            return Err(Error::InventoryChanged);
        }
        Ok(ObjectSnapshot { inventory, objects })
    }
    pub fn inventory(&self) -> Result<Inventory> {
        self.inventory_with(|| {}, || {})
    }
    fn inventory_with(&self, scanned: impl FnOnce(), verified: impl FnOnce()) -> Result<Inventory> {
        self.check()?;
        let names = scan(&self.directory)?;
        scanned();
        self.check()?;
        let mut entries = Vec::new();
        let mut receipts = Vec::new();
        entries
            .try_reserve_exact(names.len())
            .map_err(|_| Error::Allocation)?;
        receipts
            .try_reserve_exact(names.len())
            .map_err(|_| Error::Allocation)?;
        let mut payload_bytes = 0u64;
        for object in &names {
            let (file, data) = read_at(
                &self.directory,
                &object_name(*object),
                self.project,
                *object,
            )?;
            payload_bytes = payload_bytes
                .checked_add(data.report.payload_bytes as u64)
                .ok_or(Error::Limit)?;
            if payload_bytes > MAX_INVENTORY_BYTES {
                return Err(Error::Limit);
            }
            entries.push(InventoryEntry {
                object: *object,
                report: data.report,
            });
            // Retain checked inodes while releasing each bounded payload buffer.
            receipts.push((file, data.verified_metadata));
        }
        verified();
        if scan(&self.directory)? != names {
            return Err(Error::InventoryChanged);
        }
        for (((file, metadata), object), expected) in receipts.iter_mut().zip(&names).zip(&entries)
        {
            let current = crate::inspect::private(file)?;
            if !unchanged(metadata, &current) {
                return Err(Error::InventoryChanged);
            }
            check_visible(&self.directory, &object_name(*object), metadata)?;
            file.seek(SeekFrom::Start(0))?;
            let (_, report, _) = crate::inspect::read_open_file(file, self.project, *object)?;
            if report != expected.report {
                return Err(Error::InventoryChanged);
            }
            check_visible(&self.directory, &object_name(*object), metadata)?;
        }
        self.check()?;
        let digest = digest(self.project, &entries, payload_bytes);
        Ok(Inventory {
            project: self.project,
            entries,
            payload_bytes,
            digest,
        })
    }
}

fn unchanged(a: &Metadata, b: &Metadata) -> bool {
    a.dev() == b.dev()
        && a.ino() == b.ino()
        && a.len() == b.len()
        && a.mtime() == b.mtime()
        && a.mtime_nsec() == b.mtime_nsec()
        && a.ctime() == b.ctime()
        && a.ctime_nsec() == b.ctime_nsec()
}
fn scan(directory: &File) -> Result<Vec<ObjectId>> {
    let stream = rustix::fs::Dir::read_from(directory).map_err(std::io::Error::from)?;
    let mut objects = Vec::new();
    objects
        .try_reserve_exact(MAX_INVENTORY_OBJECTS)
        .map_err(|_| Error::Allocation)?;
    let mut marker = false;
    for (count, entry) in stream.enumerate() {
        if count >= MAX_INVENTORY_OBJECTS + 3 {
            return Err(Error::Limit);
        }
        let entry = entry.map_err(std::io::Error::from)?;
        let name = entry.file_name().to_bytes();
        if matches!(name, b"." | b"..") {
            continue;
        }
        if name == SCOPE_FILE.as_bytes() {
            if marker {
                return Err(Error::InventoryChanged);
            }
            marker = true;
            continue;
        }
        if objects.len() == MAX_INVENTORY_OBJECTS {
            return Err(Error::Limit);
        }
        let object = object_id_from_name(name)?;
        if objects.contains(&object) {
            return Err(Error::InventoryChanged);
        }
        objects.push(object);
    }
    if !marker {
        return Err(Error::InventoryChanged);
    }
    objects.sort_unstable_by_key(|object| *object.as_bytes());
    Ok(objects)
}
fn digest(project: ProjectId, entries: &[InventoryEntry], payload_bytes: u64) -> [u8; 32] {
    digest_components(
        project,
        entries.len() as u32,
        payload_bytes,
        entries.iter().map(|entry| {
            (
                entry.object,
                entry.report.payload_bytes as u64,
                entry.report.sha256,
            )
        }),
    )
}
pub(crate) fn digest_components(
    project: ProjectId,
    count: u32,
    payload_bytes: u64,
    entries: impl Iterator<Item = (ObjectId, u64, [u8; 32])>,
) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(DIGEST_DOMAIN);
    hash.update(project.as_bytes());
    hash.update(count.to_le_bytes());
    hash.update(payload_bytes.to_le_bytes());
    for (object, bytes, checksum) in entries {
        hash.update(object.as_bytes());
        hash.update(bytes.to_le_bytes());
        hash.update(checksum);
    }
    hash.finalize().into()
}

#[cfg(test)]
mod tests;
