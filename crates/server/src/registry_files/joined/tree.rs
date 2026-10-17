//! Independent original readers for the last common-root proof after child unwind.
use crate::{Error, MAX_COMPONENT_IMAGE_BYTES, Result};
use rustix::fs::{AtFlags, Mode, OFlags};
use sha2::{Digest, Sha256};
use std::fs::{File, Metadata};
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::MetadataExt;

// Separate physical bounds include disposable checkpoints and reset commits.
const DIRECTORIES: usize = 512;
const FILES: usize = 1024;
const PHYSICAL_BYTES: u64 = (3 * MAX_COMPONENT_IMAGE_BYTES) as u64;
const DEPTH: usize = 6;

#[derive(Default)]
struct Charge {
    directories: usize,
    files: usize,
    bytes: u64,
}
struct OriginalFile {
    file: File,
    baseline: Metadata,
    hash: [u8; 32],
}
enum Original {
    Directory(Tree),
    File(OriginalFile),
}
struct Entry {
    name: String,
    original: Original,
}
pub(super) struct Tree {
    directory: File,
    entries: Vec<Entry>,
}
fn private(directory: &File) -> Result<()> {
    let m = directory.metadata()?;
    if !m.is_dir() || m.mode() & 0o7777 != 0o700 {
        return Err(Error::ComponentArchive("prepared tree directory privacy"));
    }
    Ok(())
}
fn names(directory: &File) -> Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in rustix::fs::Dir::read_from(directory).map_err(std::io::Error::from)? {
        let entry = entry.map_err(std::io::Error::from)?;
        let name = entry.file_name().to_str().map_err(|_| Error::Path)?;
        if matches!(name, "." | "..") {
            continue;
        }
        if names.len() >= DIRECTORIES + FILES || name.is_empty() || name.len() > 255 {
            return Err(Error::Limit);
        }
        names.push(name.to_owned());
    }
    names.sort();
    if names.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(Error::Path);
    }
    Ok(names)
}
fn stamp(m: &Metadata) -> (u64, u64, u64, u32, u64, i64, i64, i64, i64) {
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
fn file_hash(file: &mut File, baseline: &Metadata) -> Result<[u8; 32]> {
    let before = file.metadata()?;
    if !before.is_file()
        || !matches!(before.mode() & 0o7777, 0o400 | 0o600)
        || before.nlink() != 1
        || stamp(&before) != stamp(baseline)
    {
        return Err(Error::ComponentArchive("prepared physical file changed"));
    }
    file.seek(SeekFrom::Start(0))?;
    let mut left = before.len();
    let mut scratch = [0; 8192];
    let mut digest = Sha256::new();
    while left != 0 {
        let count = left.min(scratch.len() as u64) as usize;
        file.read_exact(&mut scratch[..count])?;
        digest.update(&scratch[..count]);
        left -= count as u64;
    }
    if stamp(&file.metadata()?) != stamp(baseline) {
        return Err(Error::Path);
    }
    Ok(digest.finalize().into())
}
impl Tree {
    pub(super) fn capture(directory: &File) -> Result<Self> {
        let mut tree = Self::walk(directory.try_clone()?, &mut Charge::default(), 0)?;
        tree.check()?;
        Ok(tree)
    }
    fn walk(directory: File, charge: &mut Charge, depth: usize) -> Result<Self> {
        private(&directory)?;
        if depth > DEPTH || charge.directories >= DIRECTORIES {
            return Err(Error::Limit);
        }
        charge.directories += 1;
        let mut entries = Vec::new();
        for name in names(&directory)? {
            let selected = rustix::fs::statat(&directory, &name, AtFlags::SYMLINK_NOFOLLOW)
                .map_err(std::io::Error::from)?;
            let kind = selected.st_mode & 0o170000;
            if !matches!(kind, 0o040000 | 0o100000) {
                return Err(Error::Path);
            }
            let flags = OFlags::RDONLY
                | OFlags::NOFOLLOW
                | OFlags::NONBLOCK
                | OFlags::CLOEXEC
                | if kind == 0o040000 {
                    OFlags::DIRECTORY
                } else {
                    OFlags::empty()
                };
            let mut file = File::from(
                rustix::fs::openat(&directory, &name, flags, Mode::empty())
                    .map_err(std::io::Error::from)?,
            );
            let baseline = file.metadata()?;
            if (baseline.dev(), baseline.ino()) != (selected.st_dev, selected.st_ino) {
                return Err(Error::Path);
            }
            let original = if kind == 0o040000 {
                Original::Directory(Self::walk(file, charge, depth + 1)?)
            } else {
                if charge.files >= FILES {
                    return Err(Error::Limit);
                }
                charge.files += 1;
                charge.bytes = charge
                    .bytes
                    .checked_add(baseline.len())
                    .filter(|n| *n <= PHYSICAL_BYTES)
                    .ok_or(Error::Limit)?;
                let hash = file_hash(&mut file, &baseline)?;
                Original::File(OriginalFile {
                    file,
                    baseline,
                    hash,
                })
            };
            entries.push(Entry { name, original });
        }
        Ok(Self { directory, entries })
    }
    pub(super) fn check(&mut self) -> Result<()> {
        private(&self.directory)?;
        self.roster()?;
        for entry in &mut self.entries {
            Self::selected(&self.directory, entry)?;
            match &mut entry.original {
                Original::Directory(tree) => tree.check()?,
                Original::File(original) => {
                    if file_hash(&mut original.file, &original.baseline)? != original.hash {
                        return Err(Error::ComponentArchive(
                            "prepared physical file contents changed",
                        ));
                    }
                }
            }
            Self::selected(&self.directory, entry)?;
        }
        self.roster()?;
        private(&self.directory)
    }
    fn roster(&self) -> Result<()> {
        if names(&self.directory)?
            .iter()
            .map(String::as_str)
            .ne(self.entries.iter().map(|entry| entry.name.as_str()))
        {
            return Err(Error::ComponentArchive("prepared physical roster changed"));
        }
        Ok(())
    }
    fn selected(parent: &File, entry: &Entry) -> Result<()> {
        let visible = rustix::fs::statat(parent, &entry.name, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(std::io::Error::from)?;
        let (file, kind) = match &entry.original {
            Original::Directory(tree) => (&tree.directory, 0o040000),
            Original::File(original) => (&original.file, 0o100000),
        };
        let original = file.metadata()?;
        if visible.st_mode & 0o170000 != kind
            || (visible.st_dev, visible.st_ino) != (original.dev(), original.ino())
        {
            return Err(Error::Path);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};

    #[test]
    fn native_depth_and_sparse_size_bounds_refuse_without_changing_original_files() {
        let _serial = crate::durability::PROCESS_TESTS.blocking_lock();
        let temp = tempfile::tempdir().unwrap();
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let root = File::open(temp.path()).unwrap();
        let baseline = fs::read_dir("/proc/self/fd").unwrap().count();
        let mut path = temp.path().to_path_buf();
        for _ in 0..DEPTH {
            path = path.join("child");
            fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        }
        drop(Tree::capture(&root).unwrap());
        fs::DirBuilder::new()
            .mode(0o700)
            .create(path.join("child"))
            .unwrap();
        assert!(matches!(Tree::capture(&root), Err(Error::Limit)));
        assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), baseline);
        let separate = tempfile::tempdir().unwrap();
        fs::set_permissions(separate.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let sparse = separate.path().join("oversized");
        File::options()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&sparse)
            .unwrap()
            .set_len(PHYSICAL_BYTES + 1)
            .unwrap();
        assert!(matches!(
            Tree::capture(&File::open(separate.path()).unwrap()),
            Err(Error::Limit)
        ));
        assert_eq!(fs::metadata(&sparse).unwrap().len(), PHYSICAL_BYTES + 1);
    }
}
