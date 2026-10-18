//! Finite native ancestry checks through independent original descriptions.
use crate::{Error, Result};
use rustix::fs::{Mode, OFlags};
use std::fs::File;
use std::os::unix::fs::MetadataExt;

/// Maximum directory observations in a single ancestry walk, including target.
pub const MAX_DIRECTORY_ANCESTORS: usize = 256;

/// Readonly comparison, not an owner lock, permission or namespace lease.
/// Independent open descriptions deliberately do not prolong a source flock.
pub struct OutsideDirectoryGuard {
    source: File,
    destination: File,
}
impl OutsideDirectoryGuard {
    pub fn new(source: &File, destination: &File) -> Result<Self> {
        let guard = Self {
            source: directory(source, ".")?,
            destination: directory(destination, ".")?,
        };
        guard.check()?;
        Ok(guard)
    }
    /// Refuse the original source itself, any descendant or excessive ancestry.
    /// Repeat around writes; native concurrent rename/mount authority is not leased.
    pub fn check(&self) -> Result<()> {
        let source = identity(&self.source)?;
        let mut current = directory(&self.destination, ".")?;
        for _ in 0..MAX_DIRECTORY_ANCESTORS {
            let here = identity(&current)?;
            if here == source {
                return Err(Error::Path);
            }
            let parent = directory(&current, "..")?;
            if identity(&parent)? == here {
                return Ok(());
            }
            current = parent;
        }
        Err(Error::Path)
    }
}
fn directory(parent: &File, name: &str) -> Result<File> {
    Ok(rustix::fs::openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(std::io::Error::from)?
    .into())
}
fn identity(file: &File) -> Result<(u64, u64)> {
    let m = file.metadata()?;
    if !m.is_dir() {
        return Err(Error::Path);
    }
    Ok((m.dev(), m.ino()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn original_ancestry_refuses_self_descendants_and_follows_retained_moves() {
        let t = tempfile::tempdir().unwrap();
        let source = t.path().join("source");
        let output = t.path().join("output");
        fs::create_dir_all(source.join("child/deeper")).unwrap();
        fs::create_dir(&output).unwrap();
        let root = File::open(&source).unwrap();
        for path in [&source, &source.join("child"), &source.join("child/deeper")] {
            assert!(OutsideDirectoryGuard::new(&root, &File::open(path).unwrap()).is_err());
        }
        let guard = OutsideDirectoryGuard::new(&root, &File::open(&output).unwrap()).unwrap();
        let moved = source.join("moved-output");
        fs::rename(&output, &moved).unwrap();
        fs::create_dir(&output).unwrap();
        assert!(guard.check().is_err());
        fs::rename(&moved, t.path().join("outside-again")).unwrap();
        guard.check().unwrap();
        let original = t.path().join("original-source");
        fs::rename(&source, &original).unwrap();
        fs::create_dir(&source).unwrap();
        fs::rename(t.path().join("outside-again"), original.join("inside")).unwrap();
        assert!(guard.check().is_err());
    }

    #[test]
    fn independent_descriptions_release_original_flock_and_do_not_leak_on_refusal() {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir(t.path().join("source")).unwrap();
        let source = File::open(t.path().join("source")).unwrap();
        source.try_lock().unwrap();
        let output = File::open(t.path()).unwrap();
        let guard = OutsideDirectoryGuard::new(&source, &output).unwrap();
        drop(source);
        let next = File::open(t.path().join("source")).unwrap();
        next.try_lock().unwrap();
        guard.check().unwrap();
        let regular = File::create(t.path().join("regular")).unwrap();
        assert!(OutsideDirectoryGuard::new(&regular, &output).is_err());
        assert!(OutsideDirectoryGuard::new(&output, &regular).is_err());
    }

    #[test]
    fn ancestry_work_is_bounded_without_writing_or_adopting_a_name() {
        let t = tempfile::tempdir().unwrap();
        let source = File::open(t.path()).unwrap();
        let mut path = t.path().join("tree");
        fs::create_dir(&path).unwrap();
        for _ in 0..MAX_DIRECTORY_ANCESTORS {
            path = path.join("d");
            fs::create_dir(&path).unwrap();
        }
        let destination = File::open(&path).unwrap();
        let unrelated = tempfile::tempdir().unwrap();
        assert!(
            OutsideDirectoryGuard::new(&File::open(unrelated.path()).unwrap(), &destination)
                .is_err()
        );
        assert!(OutsideDirectoryGuard::new(&source, &destination).is_err());
        assert_eq!(fs::read_dir(path).unwrap().count(), 0);
    }
}
