//! Current service-key readonly access to already owned declared file components.
use super::super::Structure;
use super::*;
use emilybase_files::{FileId, FileInfo, NativeFileRoot, NativeFileRootInfo};
mod mutation;
mod policies;
mod private_wal;
mod user;
pub use mutation::ProjectFileUpload;
pub use user::AccountFileWrite;

/// Copied output bound; native graph verification has its separate physical caps.
pub const MAX_ACCOUNT_FILE_READ_BYTES: usize = emilybase_object_storage::MAX_PAYLOAD_BYTES;

#[derive(Clone, Copy)]
enum FileAuthority<'a> {
    Service(&'a str),
    User { access: &'a str, now: u64 },
}

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
            read_file_checked(root, id, maximum_bytes, check, |_| Ok(()))
        })
    }
    fn with_file_component<T>(
        &mut self,
        project: &str,
        key: &str,
        work: impl FnOnce(&mut NativeFileRoot, &mut dyn FnMut() -> Result<()>) -> Result<T>,
    ) -> Result<T> {
        self.with_file_component_accounts(project, key, |native, _, check| work(native, check))
    }
    // Lend disjoint original owners in the same current-key/common-root frame.
    // Missing private ownership never implicitly provisions an account store.
    fn with_file_component_accounts<T>(
        &mut self,
        project: &str,
        key: &str,
        work: impl FnOnce(
            &mut NativeFileRoot,
            Option<(&mut AccountStore, &File)>,
            &mut dyn FnMut() -> Result<()>,
        ) -> Result<T>,
    ) -> Result<T> {
        self.with_file_scope(project, FileAuthority::Service(key), work)
    }
    fn with_file_scope<T>(
        &mut self,
        project: &str,
        authority: FileAuthority<'_>,
        work: impl FnOnce(
            &mut NativeFileRoot,
            Option<(&mut AccountStore, &File)>,
            &mut dyn FnMut() -> Result<()>,
        ) -> Result<T>,
    ) -> Result<T> {
        self.ready()?;
        match authority {
            FileAuthority::Service(key) => drop(self.contents.registry.authorize(project, key)?),
            FileAuthority::User { .. } => drop(self.contents.registry.owned_project(project)?),
        }
        let contents = &mut self.contents;
        let registry = &contents.registry;
        // Pin original project metadata before user authentication; a later
        // equal-byte replacement cannot become this operation's new selection.
        let mut metadata = match authority {
            FileAuthority::Service(key) => registry.file_key_guard(project, key)?,
            FileAuthority::User { .. } => registry.file_metadata_guard(project)?,
        };
        let mut account = contents
            .accounts
            .iter_mut()
            .find(|a| a.project() == project)
            .and_then(|account| {
                contents
                    .manifest
                    .private_projects()
                    .iter()
                    .position(|id| id == project)
                    .and_then(|index| contents.account_owners.get(index))
                    .map(|owner| (account, owner))
            });
        // Retain the actual private source before any forward authentication
        // clock append. Authenticate before selecting a requested file binding.
        let private_selection = if let FileAuthority::User { access, now } = authority {
            let (account, owner) = account.as_mut().ok_or(Error::Denied)?;
            let guard = private_wal::PrivateWal::new(account, owner)?;
            super::public_user::admission(account)?;
            drop(account.verify_access(access, now)?);
            guard.check()?;
            checkpoint("account_user_file_authenticated");
            Some(guard)
        } else {
            None
        };
        let index = contents
            .manifest
            .v2()
            .and_then(|m| m.file_stores().iter().position(|b| b.project() == project))
            .ok_or(Error::Denied)?;
        checkpoint("account_file_key_admitted");
        let (roots, topology) = contents.files.parts();
        let native = roots
            .get_mut(index)
            .ok_or(Error::BundleRoot("file ownership layout"))?;
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
            metadata.check()?;
            if let Some(private) = &private_selection {
                private.check()?;
            }
            match authority {
                FileAuthority::Service(key) => drop(registry.authorize(project, key)?),
                FileAuthority::User { .. } => drop(registry.owned_project(project)?),
            }
            Ok(())
        };
        let result = (|| {
            check()?;
            let value = work(native, account, &mut check);
            check()?;
            value
        })();
        let ready = self.ready();
        let private = private_selection
            .as_ref()
            .map_or(Ok(()), private_wal::PrivateWal::check);
        match (result, ready, private) {
            (Err(error), _, _) | (_, Err(error), _) | (_, _, Err(error)) => Err(error),
            (Ok(value), Ok(()), Ok(())) => Ok(value),
        }
    }
}
fn read_file_checked(
    root: &mut NativeFileRoot,
    id: FileId,
    maximum_bytes: usize,
    check: &mut dyn FnMut() -> Result<()>,
    authorize: impl FnOnce(&FileInfo) -> Result<()>,
) -> Result<AccountFileRead> {
    if maximum_bytes > MAX_ACCOUNT_FILE_READ_BYTES {
        return Err(Error::Limit);
    }
    let mut reader = root.reader(id)?;
    authorize(reader.info())?;
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
}
fn checkpoint(point: &str) {
    super::super::checkpoint(point);
}
