//! Per-operation original metadata selection for native project-service file reads.
use super::*;
use std::fs::Metadata as FileMetadata;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::MetadataExt;

pub(crate) struct ProjectFileKey<'a> {
    store: &'a ProjectStore,
    project: &'a Project,
    id: &'a str,
    file: File,
    baseline: FileMetadata,
    bytes: Vec<u8>,
}
impl ProjectStore {
    pub(crate) fn file_key_guard<'a>(
        &'a self,
        id: &'a str,
        key: &str,
    ) -> Result<ProjectFileKey<'a>> {
        drop(self.authorize(id, key)?);
        let project = self.projects.get(id).ok_or(Error::Denied)?;
        self.check_project_directory(id, project)?;
        let mut file = File::from(
            rustix::fs::openat(
                &*project.directory_owner,
                "project.json",
                rustix::fs::OFlags::RDONLY
                    | rustix::fs::OFlags::NOFOLLOW
                    | rustix::fs::OFlags::NONBLOCK
                    | rustix::fs::OFlags::CLOEXEC,
                rustix::fs::Mode::empty(),
            )
            .map_err(std::io::Error::from)?,
        );
        let baseline = file.metadata()?;
        if !baseline.is_file()
            || !matches!(baseline.mode() & 0o7777, 0o400 | 0o600)
            || baseline.nlink() != 1
        {
            return Err(Error::Metadata);
        }
        if baseline.len() > crate::MAX_METADATA_BYTES {
            return Err(Error::Limit);
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(baseline.len() as usize + 1)
            .map_err(|_| Error::Limit)?;
        (&mut file)
            .take(crate::MAX_METADATA_BYTES + 1)
            .read_to_end(&mut bytes)?;
        // Compare decoded canonical values, preserving the existing accepted
        // metadata encoding domain. Subsequent checks pin the exact initial bytes.
        if metadata::encoded(&metadata::decode(&bytes, id)?)?
            != metadata::encoded(&project.metadata)?
        {
            return Err(Error::Metadata);
        }
        let mut guard = ProjectFileKey {
            store: self,
            project,
            id,
            file,
            baseline,
            bytes,
        };
        guard.check()?;
        Ok(guard)
    }
}
fn stamp(m: &FileMetadata) -> (u64, u64, u64, u32, u64, i64, i64, i64, i64) {
    (
        m.dev(),
        m.ino(),
        m.len(),
        m.mode(),
        m.nlink(),
        m.mtime(),
        m.mtime_nsec(),
        m.ctime(),
        m.ctime_nsec(),
    )
}
impl ProjectFileKey<'_> {
    fn selected(&self) -> Result<()> {
        self.store.ready()?;
        metadata::owned_directory(&self.store.root, &self.store.owner)?;
        self.store.check_project_directory(self.id, self.project)?;
        let current = self.file.metadata()?;
        let visible = rustix::fs::statat(
            &*self.project.directory_owner,
            "project.json",
            rustix::fs::AtFlags::SYMLINK_NOFOLLOW,
        )
        .map_err(std::io::Error::from)?;
        if !current.is_file()
            || !matches!(current.mode() & 0o7777, 0o400 | 0o600)
            || current.nlink() != 1
            || stamp(&current) != stamp(&self.baseline)
            || current.len() != self.bytes.len() as u64
            || visible.st_mode & 0o170000 != 0o100000
            || (visible.st_dev, visible.st_ino) != (current.dev(), current.ino())
        {
            return Err(Error::Metadata);
        }
        Ok(())
    }
    pub(crate) fn check(&mut self) -> Result<()> {
        self.selected()?;
        self.file.seek(SeekFrom::Start(0))?;
        let mut scratch = [0; 8192];
        for expected in self.bytes.chunks(scratch.len()) {
            self.file.read_exact(&mut scratch[..expected.len()])?;
            if scratch[..expected.len()] != *expected {
                return Err(Error::Metadata);
            }
        }
        if self.file.read(&mut [0; 1])? != 0 {
            return Err(Error::Metadata);
        }
        self.selected()
    }
}
