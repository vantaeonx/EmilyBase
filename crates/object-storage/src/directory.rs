//! Native project-scoped operations through a retained private directory handle.
use crate::{
    Error, FileReport, HEADER_BYTES, MAX_PAYLOAD_BYTES, ObjectId, ProjectId, Result, encode,
};
use rustix::fs::{AtFlags, Mode, OFlags};
use std::fs::{File, Metadata, TryLockError};
use std::os::unix::fs::MetadataExt;
use std::path::Path;

const SCOPE_FILE: &str = ".emilybase-objects";
const SCOPE_OBJECT: ObjectId = ObjectId::from_bytes([0; 16]);

/// An owned, fully verified image. Debug never reveals payload bytes.
pub struct StoredObject {
    image: Vec<u8>,
    report: FileReport,
    project: ProjectId,
    object: ObjectId,
}
impl StoredObject {
    pub fn payload(&self) -> &[u8] {
        &self.image[HEADER_BYTES..]
    }
    pub const fn report(&self) -> &FileReport {
        &self.report
    }
    pub const fn project(&self) -> ProjectId {
        self.project
    }
    pub const fn object(&self) -> ObjectId {
        self.object
    }
}
impl std::fmt::Debug for StoredObject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoredObject")
            .field("bytes", &self.report.payload_bytes)
            .finish_non_exhaustive()
    }
}

/// Native filesystem authority only. No user/session/policy authorization is
/// conferred. One cooperating owner holds the directory lock until drop.
pub struct ProjectDirectory {
    directory: DirectoryOwner,
    scope: File,
    project: ProjectId,
}
struct DirectoryOwner(File);
impl std::ops::Deref for DirectoryOwner {
    type Target = File;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl Drop for DirectoryOwner {
    fn drop(&mut self) {
        // Release our authorization lifetime explicitly. A transient fork may
        // still hold this open file description until its close-on-exec runs.
        let _ = self.0.unlock();
    }
}
impl ProjectDirectory {
    /// Initialize a marker inside an existing private 0700 directory. Never
    /// creates/repairs the directory or replaces an existing marker or object.
    /// Pre-existing unmanaged entries are preserved, not admitted as inventory.
    pub fn initialize(path: impl AsRef<Path>, project: ProjectId) -> Result<Self> {
        let directory = open_directory(path.as_ref())?;
        let image = encode(project, SCOPE_OBJECT, &[])?;
        publish_at(&directory, SCOPE_FILE, &image)?;
        let scope = match read_at(&directory, SCOPE_FILE, project, SCOPE_OBJECT) {
            Ok((scope, data)) if data.payload().is_empty() => scope,
            _ => return Err(Error::PublicationUnknown),
        };
        let result = Self {
            directory,
            scope,
            project,
        };
        result.check().map_err(|_| Error::PublicationUnknown)?;
        Ok(result)
    }

    /// Opening is readonly; absence/corruption/foreign scope never initializes.
    pub fn open(path: impl AsRef<Path>, project: ProjectId) -> Result<Self> {
        let directory = open_directory(path.as_ref())?;
        let (scope, data) = read_at(&directory, SCOPE_FILE, project, SCOPE_OBJECT)?;
        if !data.payload().is_empty() {
            return Err(Error::Directory);
        }
        Ok(Self {
            directory,
            scope,
            project,
        })
    }

    pub const fn project(&self) -> ProjectId {
        self.project
    }

    fn check(&self) -> Result<()> {
        private_directory(&self.directory)?;
        let (visible, data) = read_at(&self.directory, SCOPE_FILE, self.project, SCOPE_OBJECT)?;
        let retained = self.scope.metadata()?;
        let current = visible.metadata()?;
        if (retained.dev(), retained.ino()) != (current.dev(), current.ino())
            || !data.payload().is_empty()
        {
            return Err(Error::Directory);
        }
        Ok(())
    }

    /// Exact typed identifier only: no caller-provided path/name is accepted.
    /// Returned bytes are verified under the retained project scope.
    pub fn get(&self, object: ObjectId) -> Result<StoredObject> {
        self.check()?;
        let (_, data) = read_at(&self.directory, &object_name(object), self.project, object)?;
        self.check()?;
        Ok(data)
    }

    /// Create a new immutable name; never overwrites, retries or deletes.
    /// A post-selection check failure reports an uncertain published result.
    pub fn put(&mut self, object: ObjectId, payload: &[u8]) -> Result<FileReport> {
        if payload.len() > MAX_PAYLOAD_BYTES {
            return Err(Error::Limit);
        }
        self.check()?;
        let image = encode(self.project, object, payload)?;
        let name = object_name(object);
        publish_at(&self.directory, &name, &image)?;
        let checked = read_at(&self.directory, &name, self.project, object);
        match checked {
            Ok((_, data)) if data.image == image => {
                self.check().map_err(|_| Error::PublicationUnknown)?;
                Ok(data.report)
            }
            _ => Err(Error::PublicationUnknown),
        }
    }
}

fn object_name(object: ObjectId) -> String {
    format!("{object}.object")
}
fn private_directory(file: &File) -> Result<()> {
    let m = file.metadata()?;
    if !m.is_dir() || m.mode() & 0o777 != 0o700 {
        return Err(Error::Directory);
    }
    Ok(())
}
fn open_directory(path: &Path) -> Result<DirectoryOwner> {
    let fd = rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(std::io::Error::from)?;
    let directory: File = fd.into();
    private_directory(&directory)?;
    match directory.try_lock() {
        Ok(()) => Ok(DirectoryOwner(directory)),
        Err(TryLockError::WouldBlock) => Err(Error::Busy),
        Err(TryLockError::Error(error)) => Err(error.into()),
    }
}
fn publish_at(directory: &File, name: &str, image: &[u8]) -> Result<()> {
    match emilybase_storage::publish_private_file_at(
        directory,
        name,
        image,
        HEADER_BYTES + MAX_PAYLOAD_BYTES,
    ) {
        Ok(()) => Ok(()),
        Err(emilybase_storage::Error::PublicationUnknown(_)) => Err(Error::PublicationUnknown),
        Err(error) => Err(Error::Publication(error)),
    }
}
fn read_at(
    directory: &File,
    name: &str,
    project: ProjectId,
    object: ObjectId,
) -> Result<(File, StoredObject)> {
    let fd = rustix::fs::openat(
        directory,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(std::io::Error::from)?;
    let mut file: File = fd.into();
    if name == SCOPE_FILE && crate::inspect::private(&file)?.len() != HEADER_BYTES as u64 {
        return Err(Error::Directory);
    }
    let (image, report, metadata) = crate::inspect::read_open_file(&mut file, project, object)?;
    check_visible(directory, name, &metadata)?;
    Ok((
        file,
        StoredObject {
            image,
            report,
            project,
            object,
        },
    ))
}
fn check_visible(directory: &File, name: &str, owned: &Metadata) -> Result<()> {
    let visible = rustix::fs::statat(directory, name, AtFlags::SYMLINK_NOFOLLOW)
        .map_err(std::io::Error::from)?;
    if (visible.st_dev, visible.st_ino) != (owned.dev(), owned.ino())
        || visible.st_mode & 0o170000 != 0o100000
        || !matches!(visible.st_mode & 0o777, 0o600 | 0o400)
        || visible.st_nlink != 1
    {
        return Err(Error::File);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::DirBuilderExt;

    #[test]
    fn owner_drop_releases_lock_even_if_open_file_description_was_inherited() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("objects");
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .unwrap();
        let project = ProjectId::from_bytes([1; 16]);
        let owner = ProjectDirectory::initialize(&path, project).unwrap();
        // A duplicate models a fork-inherited open file description without
        // unsafe fork/pre-exec code. It is not a second authorized owner.
        let inherited = owner.directory.try_clone().unwrap();
        drop(owner);
        let next = ProjectDirectory::open(&path, project).unwrap();
        assert_eq!(next.project(), project);
        drop(inherited);
        assert!(matches!(
            ProjectDirectory::open(&path, project),
            Err(Error::Busy)
        ));
        drop(next);
        // The same guard must release failed construction paths as well.
        let constructing = open_directory(&path).unwrap();
        let inherited = constructing.try_clone().unwrap();
        assert!(
            read_at(
                &constructing,
                SCOPE_FILE,
                ProjectId::from_bytes([3; 16]),
                SCOPE_OBJECT
            )
            .is_err()
        );
        drop(constructing);
        let next = ProjectDirectory::open(&path, project).unwrap();
        drop(inherited);
        assert!(matches!(
            ProjectDirectory::open(&path, project),
            Err(Error::Busy)
        ));
        drop(next);
    }
}
