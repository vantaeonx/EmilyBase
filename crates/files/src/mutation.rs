use crate::{Error, FileId, FileInfo, FileStore, Result, records};
use emilybase_catalog::Key;

/// Expected metadata of a removed logical reference and its actual WAL commit.
/// The physical blob remains private, immutable and charged. This is no tombstone
/// history, retry capability or current account authorization.
#[derive(Clone, PartialEq, Eq)]
pub struct FileRemoval {
    removed: FileInfo,
    revision: u64,
}
impl FileRemoval {
    pub const fn removed(&self) -> &FileInfo {
        &self.removed
    }
    pub const fn revision(&self) -> u64 {
        self.revision
    }
}
impl std::fmt::Debug for FileRemoval {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileRemoval")
            .field("revision", &self.revision)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum MetadataBoundary {
    Staged,
    Committed,
    Verified,
}

impl FileStore {
    /// Rename display text under an exact current revision. A matching current
    /// name is a no-op only after revision matching; stale retries still conflict.
    /// Content, immutable object ID, owner metadata and quota are unchanged.
    /// ```compile_fail
    /// use emilybase_files::{FileId, FileStore};
    /// fn cannot_mutate(store: &mut FileStore, id: FileId, revision: u64) {
    ///     let mut reader = store.reader(id).unwrap();
    ///     store.rename(id, revision, "changed").unwrap();
    ///     reader.read_payload(&mut [0; 8]).unwrap();
    /// }
    /// ```
    pub fn rename(&mut self, id: FileId, expected_revision: u64, name: &str) -> Result<FileInfo> {
        Ok(self
            .mutate_with(id, expected_revision, Some(name), |_| {})?
            .0)
    }

    /// Commit logical visibility removal under an exact current revision. Never
    /// deletes the blob or frees physical quota. An absent reference is Missing,
    /// not an assumed successful replay of an earlier uncertain response.
    pub fn remove(&mut self, id: FileId, expected_revision: u64) -> Result<FileRemoval> {
        let (removed, revision) = self.mutate_with(id, expected_revision, None, |_| {})?;
        Ok(FileRemoval { removed, revision })
    }

    pub(crate) fn mutate_with(
        &mut self,
        id: FileId,
        expected_revision: u64,
        name: Option<&str>,
        mut boundary: impl FnMut(MetadataBoundary),
    ) -> Result<(FileInfo, u64)> {
        if let Some(name) = name {
            records::validate_name(name)?;
        }
        let (quota, files, _) = self.validated()?;
        let current = files
            .into_iter()
            .find(|info| info.id == id)
            .ok_or(Error::Missing)?;
        if current.revision != expected_revision {
            return Err(Error::Conflict);
        }
        let mut reader = self.objects.reader(current.object)?;
        if reader.report() != &current.report {
            return Err(Error::Corrupt);
        }
        if name == Some(current.name.as_str()) {
            reader.verify()?;
            self.database.view()?;
            return Ok((current.clone(), current.revision));
        }
        let revision = self
            .database
            .last_transaction()
            .checked_add(1)
            .ok_or(Error::Revision)?;
        let mut changed = current.clone();
        if let Some(name) = name {
            changed.name = name.into();
            changed.revision = revision;
        }
        let project = self.objects.project();
        let mut commit_attempted = false;
        let result = (|| {
            let mut tx = self.database.begin()?;
            let key = Key::Text(id.to_string());
            if name.is_some() {
                tx.update(records::FILES, &key, records::encode(&changed))?;
            } else {
                tx.delete(records::FILES, &key)?;
            }
            boundary(MetadataBoundary::Staged);
            // A source change here discards the staged transaction; no metadata
            // commit has been attempted yet.
            reader.verify()?;
            tx.view()?;
            commit_attempted = true;
            if tx.commit()? != revision {
                return Err(Error::Corrupt);
            }
            boundary(MetadataBoundary::Committed);
            let (current_quota, current_files) = records::metadata(&self.database, project)?;
            if current_quota != quota {
                return Err(Error::Corrupt);
            }
            match current_files.iter().find(|info| info.id == id) {
                Some(info) if name.is_some() && info == &changed => {}
                None if name.is_none() => {}
                _ => return Err(Error::Corrupt),
            }
            let inventory = self.objects.inventory()?;
            records::graph(current_quota, &current_files, &inventory)?;
            reader.verify()?;
            self.database.view()?;
            boundary(MetadataBoundary::Verified);
            Ok((changed, revision))
        })();
        if result.is_err() {
            self.poisoned = true;
        }
        if commit_attempted {
            result.map_err(|e| Error::OutcomeUnknown(Box::new(e)))
        } else {
            result
        }
    }
}

#[cfg(test)]
mod tests;
