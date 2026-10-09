//! Startup secret selection. Data paths and network configuration remain separate.
use emilybase_server::{Error, Result};
use std::ffi::OsString;
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
fn read_key(path: &Path) -> Result<Zeroizing<String>> {
    use emilybase_auth::key_file::{KeyFileError, read_api_key_file};
    read_api_key_file(path).map_err(|error| {
        Error::Config(match error {
            KeyFileError::Unavailable => "master key file unavailable",
            KeyFileError::Unsafe => "unsafe master key file",
            KeyFileError::Changed => "master key file changed",
            KeyFileError::Format => "invalid master key file",
        })
    })
}
#[cfg(test)]
mod tests;
