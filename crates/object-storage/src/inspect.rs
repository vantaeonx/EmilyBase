use crate::{Error, HEADER_BYTES, MAX_PAYLOAD_BYTES, ObjectId, ProjectId, Result, verify};
use rustix::fs::{Mode, OFlags};
use std::fs::File;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileReport {
    pub payload_bytes: usize,
    pub sha256: [u8; 32],
}
/// Original owned staging, file fsync, byte readback, no-replace rename and
/// parent fsync. A lost/uncertain result requires explicit target inspection.
/// Native caller filesystem authority only; no HTTP or user authorization.
pub fn publish_file(
    path: impl AsRef<Path>,
    project: ProjectId,
    object: ObjectId,
    payload: &[u8],
) -> Result<FileReport> {
    let image = crate::encode(project, object, payload)?;
    let mut sha256 = [0; 32];
    sha256.copy_from_slice(&image[56..88]);
    let expected = FileReport {
        payload_bytes: payload.len(),
        sha256,
    };
    match emilybase_storage::publish_private_file(
        path.as_ref(),
        &image,
        HEADER_BYTES + MAX_PAYLOAD_BYTES,
    ) {
        Ok(()) => (),
        Err(emilybase_storage::Error::PublicationUnknown(_)) => {
            return Err(Error::PublicationUnknown);
        }
        Err(error) => return Err(Error::Publication(error)),
    }
    match inspect_file(path, project, object) {
        Ok(report) if report == expected => Ok(report),
        _ => Err(Error::PublicationUnknown),
    }
}
pub(crate) fn private(file: &File) -> Result<std::fs::Metadata> {
    private_limit(file, HEADER_BYTES + MAX_PAYLOAD_BYTES)
}
fn private_limit(file: &File, maximum: usize) -> Result<std::fs::Metadata> {
    let m = file.metadata()?;
    if !m.is_file() || m.nlink() != 1 || !matches!(m.mode() & 0o777, 0o600 | 0o400) {
        return Err(Error::File);
    }
    if m.len() > maximum as u64 {
        return Err(Error::Limit);
    }
    Ok(m)
}
pub(crate) fn read_open_file(
    file: &mut File,
    project: ProjectId,
    object: ObjectId,
) -> Result<(Vec<u8>, FileReport, std::fs::Metadata)> {
    let maximum = HEADER_BYTES + MAX_PAYLOAD_BYTES;
    let (bytes, before) = read_image(file, maximum)?;
    let view = verify(&bytes, project, object)?;
    let report = FileReport {
        payload_bytes: view.payload().len(),
        sha256: *view.sha256(),
    };
    let after = recheck(file, &before, maximum)?;
    Ok((bytes, report, after))
}
pub(crate) fn read_image(file: &mut File, maximum: usize) -> Result<(Vec<u8>, std::fs::Metadata)> {
    let before = private_limit(file, maximum)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(before.len() as usize)
        .map_err(|_| Error::Allocation)?;
    (&mut *file)
        .take(maximum.checked_add(1).ok_or(Error::Limit)? as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > maximum {
        return Err(Error::Limit);
    }
    let after = recheck(file, &before, maximum)?;
    Ok((bytes, after))
}
pub(crate) fn recheck(
    file: &File,
    before: &std::fs::Metadata,
    maximum: usize,
) -> Result<std::fs::Metadata> {
    let after = private_limit(file, maximum)?;
    if before.len() != after.len()
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
        || before.ctime() != after.ctime()
        || before.ctime_nsec() != after.ctime_nsec()
    {
        return Err(Error::File);
    }
    Ok(after)
}
pub(crate) fn open_private(path: &Path) -> Result<File> {
    let fd = rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .map_err(std::io::Error::from)?;
    Ok(fd.into())
}
pub(crate) fn check_visible(path: &Path, after: &std::fs::Metadata) -> Result<()> {
    let visible = std::fs::symlink_metadata(path)?;
    if !visible.is_file()
        || (visible.dev(), visible.ino()) != (after.dev(), after.ino())
        || !matches!(visible.mode() & 0o777, 0o600 | 0o400)
        || visible.nlink() != 1
    {
        return Err(Error::File);
    }
    Ok(())
}
/// Bounded offline inspection only; never creates/repairs/publishes a file.
/// The supplied path is operator input, not an HTTP object name.
pub fn inspect_file(
    path: impl AsRef<Path>,
    project: ProjectId,
    object: ObjectId,
) -> Result<FileReport> {
    let path = path.as_ref();
    let mut file = open_private(path)?;
    let (_, report, after) = read_open_file(&mut file, project, object)?;
    check_visible(path, &after)?;
    Ok(report)
}
