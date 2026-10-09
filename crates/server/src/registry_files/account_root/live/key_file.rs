//! Save an external private secret before activating its original registry digest.
use super::super::{Pending, checkpoint, sync};
use super::AccountRoot;
use crate::{Error, ProjectInfo, Result};
use emilybase_auth::{KeyDigest, key_file::read_api_key_file};
use std::fs;
use std::io::Write;
use std::path::Path;
use zeroize::Zeroizing;

const PREFIX: &str = ".emilybase-service-key-";
fn unknown(error: impl std::error::Error + Send + Sync + 'static) -> Error {
    Error::PublicationUnknown(std::io::Error::other(error))
}
impl AccountRoot {
    /// Trusted filesystem operator action; no existing service/master key needed.
    /// Publish one new private external file before activating the generated key.
    /// Any error after publication requires inspection, never an automatic retry.
    pub fn rotate_project_key_to_file(
        &mut self,
        project: &str,
        target: impl AsRef<Path>,
    ) -> Result<ProjectInfo> {
        self.ready()?;
        let info = self
            .contents
            .registry
            .list()?
            .into_iter()
            .find(|item| item.id == project)
            .ok_or(Error::Denied)?;
        info.key_epoch.checked_add(1).ok_or(Error::Limit)?;
        let target = target.as_ref();
        match fs::symlink_metadata(target) {
            Ok(_) => return Err(Error::Path),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
        let mut pending = Pending::file(target, PREFIX)?;
        // Check the actual retained parent, including ancestors selected by aliases.
        // A transient empty owned stage is safely removed if it is inside the root.
        if pending.parent_path()?.starts_with(&self.selected) {
            return Err(Error::Path);
        }
        let key = Zeroizing::new(emilybase_auth::issue_key()?);
        let digest = KeyDigest::from_token(&key)?;
        pending.owner.write_all(key.as_bytes())?;
        sync(&pending.owner, "service_key_file_sync")?;
        checkpoint("service_key_file_synced");
        pending.publish()?;
        checkpoint("service_key_file_published");
        pending.finish("service_key_parent_sync")?;
        checkpoint("service_key_parent_synced");
        // Reuse exact private-file checks before activating a digest.
        let selected = read_api_key_file(target).map_err(unknown)?;
        if !digest.verifies(&selected) {
            return Err(unknown(std::io::Error::other(
                "published service key changed",
            )));
        }
        drop(selected);
        self.ready().map_err(unknown)?;
        let result = self
            .contents
            .registry
            .rotate_prepared(project, &key)
            .map_err(unknown)?;
        checkpoint("service_key_activated");
        pending.finish("service_key_final_parent_sync")?;
        let selected = read_api_key_file(target).map_err(unknown)?;
        if !digest.verifies(&selected) {
            return Err(unknown(std::io::Error::other(
                "published service key changed",
            )));
        }
        checkpoint("service_key_ack");
        Ok(result)
    }
}
