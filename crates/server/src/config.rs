//! Startup secret selection. Data paths and network configuration remain separate.
use emilybase_server::{Error, Result};
use std::ffi::OsString;
use std::fs::{self, File, Metadata};
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use zeroize::Zeroizing;

pub fn load_master() -> Result<Zeroizing<String>> {
    let environment = match std::env::var("EMILYBASE_MASTER_KEY") {
        Ok(value) => Some(Zeroizing::new(value)),
        Err(std::env::VarError::NotPresent) => None,
        Err(_) => return Err(Error::Config("invalid master key environment")),
    };
    load(environment, std::env::var_os("EMILYBASE_MASTER_KEY_FILE"))
}
fn load(
    environment: Option<Zeroizing<String>>,
    path: Option<OsString>,
) -> Result<Zeroizing<String>> {
    let key = match (environment, path) {
        (Some(key), None) => key,
        (None, Some(path)) => read_key(Path::new(&path))?,
        (None, None) => return Err(Error::Config("master key required")),
        (Some(_), Some(_)) => return Err(Error::Config("select one master key source")),
    };
    emilybase_auth::KeyDigest::from_token(&key)?;
    Ok(key)
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
fn read_key(path: &Path) -> Result<Zeroizing<String>> {
    read_key_after(path, || {})
}
fn read_key_after(path: &Path, after_read: impl FnOnce()) -> Result<Zeroizing<String>> {
    let owner = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::NONBLOCK
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(|_| Error::Config("master key file unavailable"))?;
    let mut file = File::from(owner);
    let before = file
        .metadata()
        .map_err(|_| Error::Config("master key file unavailable"))?;
    if !private(&before) || !(64..=65).contains(&before.len()) {
        return Err(Error::Config("unsafe master key file"));
    }
    let mut bytes = Zeroizing::new(Vec::with_capacity(66));
    file.by_ref()
        .take(66)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::Config("master key file unavailable"))?;
    after_read();
    let after = file
        .metadata()
        .map_err(|_| Error::Config("master key file unavailable"))?;
    let selected =
        fs::symlink_metadata(path).map_err(|_| Error::Config("master key file unavailable"))?;
    if bytes.len() as u64 != before.len() || !same(&before, &after) || !same(&before, &selected) {
        return Err(Error::Config("master key file changed"));
    }
    // A single final LF is a file transport convention, not secret normalization.
    let text = std::str::from_utf8(&bytes).map_err(|_| Error::Config("invalid master key file"))?;
    Ok(Zeroizing::new(
        text.strip_suffix('\n').unwrap_or(text).into(),
    ))
}

#[cfg(test)]
mod tests;
