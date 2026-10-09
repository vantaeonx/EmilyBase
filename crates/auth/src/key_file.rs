//! Bounded Linux private API-key files shared by server startup and offline tools.
use std::fs::{self, File, Metadata};
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use zeroize::Zeroizing;

#[derive(Debug, thiserror::Error)]
pub enum KeyFileError {
    #[error("private API key file unavailable")]
    Unavailable,
    #[error("unsafe private API key file")]
    Unsafe,
    #[error("private API key file changed")]
    Changed,
    #[error("invalid private API key file")]
    Format,
}
fn private(meta: &Metadata) -> bool {
    meta.is_file() && meta.nlink() == 1 && matches!(meta.mode() & 0o7777, 0o400 | 0o600)
}
fn same(a: &Metadata, b: &Metadata) -> bool {
    private(a)
        && private(b)
        && (
            a.dev(),
            a.ino(),
            a.len(),
            a.mode(),
            a.mtime(),
            a.mtime_nsec(),
            a.ctime(),
            a.ctime_nsec(),
        ) == (
            b.dev(),
            b.ino(),
            b.len(),
            b.mode(),
            b.mtime(),
            b.mtime_nsec(),
            b.ctime(),
            b.ctime_nsec(),
        )
}
/// Read one exact private API key; parents are controlled by the operator.
pub fn read_api_key_file(path: &Path) -> Result<Zeroizing<String>, KeyFileError> {
    read_after(path, || {})
}
fn read_after(path: &Path, after_read: impl FnOnce()) -> Result<Zeroizing<String>, KeyFileError> {
    let owner = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::NONBLOCK
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(|_| KeyFileError::Unavailable)?;
    let mut file = File::from(owner);
    let before = file.metadata().map_err(|_| KeyFileError::Unavailable)?;
    if !private(&before) || !(64..=65).contains(&before.len()) {
        return Err(KeyFileError::Unsafe);
    }
    let mut bytes = Zeroizing::new(Vec::with_capacity(66));
    file.by_ref()
        .take(66)
        .read_to_end(&mut bytes)
        .map_err(|_| KeyFileError::Unavailable)?;
    after_read();
    let after = file.metadata().map_err(|_| KeyFileError::Unavailable)?;
    let selected = fs::symlink_metadata(path).map_err(|_| KeyFileError::Unavailable)?;
    if bytes.len() as u64 != before.len() || !same(&before, &after) || !same(&before, &selected) {
        return Err(KeyFileError::Changed);
    }
    // A single final LF is a file transport convention, not secret normalization.
    let text = std::str::from_utf8(&bytes).map_err(|_| KeyFileError::Format)?;
    let key = Zeroizing::new(text.strip_suffix('\n').unwrap_or(text).to_owned());
    crate::KeyDigest::from_token(&key).map_err(|_| KeyFileError::Format)?;
    Ok(key)
}

#[cfg(test)]
mod tests;
