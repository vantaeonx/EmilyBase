//! Current service-key readonly access to already owned declared file components.
use super::super::Structure;
use super::*;
use emilybase_files::{FileId, FileInfo, NativeFileRoot, NativeFileRootInfo};
mod mutation;
pub use mutation::ProjectFileUpload;

/// Copied output bound; native graph verification has its separate physical caps.
pub const MAX_ACCOUNT_FILE_READ_BYTES: usize = emilybase_object_storage::MAX_PAYLOAD_BYTES;

/// Sensitive copied native data, not a reader, permission or current-user grant.
pub struct AccountFileRead {
    info: FileInfo,
    payload: Vec<u8>,
}
impl AccountFileRead {
    pub fn info(&self) -> &FileInfo {
        &self.info
    }
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }
    pub fn into_payload(self) -> Vec<u8> {
        self.payload
    }
}
impl std::fmt::Debug for AccountFileRead {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountFileRead")
            .field("bytes", &self.payload.len())
            .field("revision", &self.info.revision())
            .finish_non_exhaustive()
    }
}
impl AccountRoot {
    /// Trusted project-service observation, not current user-file authorization.
    pub fn inspect_project_files(
        &mut self,
        project: &str,
        key: &str,
    ) -> Result<NativeFileRootInfo> {
        self.with_file_component(project, key, |root, _| Ok(root.inspect()?))
    }
    pub fn list_project_files(&mut self, project: &str, key: &str) -> Result<Vec<FileInfo>> {
        self.with_file_component(project, key, |root, _| Ok(root.list()?))
    }
    /// Committed logical reference metadata; a physical orphan remains absent.
    pub fn project_file_info(
        &mut self,
        project: &str,
        key: &str,
        id: FileId,
    ) -> Result<Option<FileInfo>> {
        self.with_file_component(project, key, |root, _| Ok(root.file_info(id)?))
    }
    /// Complete bounded copy, withheld until original reader/root/key checks end.
    /// A retained payload reader spans common checks; no partial copy escapes.
    pub fn read_project_file(
        &mut self,
        project: &str,
        key: &str,
        id: FileId,
        maximum_bytes: usize,
    ) -> Result<AccountFileRead> {
        self.with_file_component(project, key, |root, check| {
            if maximum_bytes > MAX_ACCOUNT_FILE_READ_BYTES {
                return Err(Error::Limit);
            }
            let mut reader = root.reader(id)?;
            checkpoint("account_file_read_retained");
            check()?;
            let expected = reader.info().report().payload_bytes;
            if expected > maximum_bytes {
                return Err(Error::Limit);
            }
            let mut payload = Vec::new();
            payload
                .try_reserve_exact(expected)
                .map_err(|_| Error::Files(emilybase_files::Error::Allocation))?;
            payload.resize(expected, 0);
            let mut position = 0;
            while position < expected {
                let read = reader.read_payload(&mut payload[position..])?;
                if read == 0 {
                    return Err(Error::Files(emilybase_files::Error::Corrupt));
                }
                position += read;
            }
            if reader.read_payload(&mut [0; 1])? != 0 {
                return Err(Error::Files(emilybase_files::Error::Corrupt));
            }
            checkpoint("account_file_read_copied");
            check()?;
            reader.verify()?;
            checkpoint("account_file_read_verified");
            check()?;
            let info = reader.finish()?;
            Ok(AccountFileRead { info, payload })
        })
    }
    fn with_file_component<T>(
        &mut self,
        project: &str,
        key: &str,
        work: impl FnOnce(&mut NativeFileRoot, &mut dyn FnMut() -> Result<()>) -> Result<T>,
    ) -> Result<T> {
        self.ready()?;
        drop(self.contents.registry.authorize(project, key)?);
        let index = self
            .contents
            .manifest
            .v2()
            .and_then(|m| m.file_stores().iter().position(|b| b.project() == project))
            .ok_or(Error::Denied)?;
        checkpoint("account_file_key_admitted");
        let contents = &mut self.contents;
        let (roots, topology) = contents.files.parts();
        let native = roots
            .get_mut(index)
            .ok_or(Error::BundleRoot("file ownership layout"))?;
        let registry = &contents.registry;
        let mut authority = registry.file_key_guard(project, key)?;
        let guard = Structure {
            path: &self.selected,
            owner: &self.owner,
            private_owner: &contents.private_owner,
            account_owners: &contents.account_owners,
            manifest_owner: &contents.manifest_owner,
            encoded: &contents.encoded,
            manifest: &contents.manifest,
            ids: &contents.ids,
            registry: registry.directory_identity()?,
            files: topology,
        };
        let parent = self.parent.as_ref();
        let mut check = || {
            Self::check_parent_owner(parent)?;
            guard.check()?;
            guard.check_wals()?;
            registry.check_registry_at(guard.owner)?;
            authority.check()?;
            drop(registry.authorize(project, key)?);
            Ok(())
        };
        let result = (|| {
            check()?;
            let value = work(native, &mut check);
            check()?;
            value
        })();
        let ready = self.ready();
        match (result, ready) {
            (Err(error), _) | (_, Err(error)) => Err(error),
            (Ok(value), Ok(())) => Ok(value),
        }
    }
}
fn checkpoint(point: &str) {
    super::super::checkpoint(point);
}
