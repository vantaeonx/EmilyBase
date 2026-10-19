//! Current project-service mutations at actual native durable write boundaries.
use super::*;
use emilybase_files::{FileQuota, FileRemoval, NativeWritePhase, QuotaState};
use emilybase_object_storage::ObjectId;

/// Trusted backend input. The owner is metadata, not authenticated user authority.
/// No serialization or retained capability; payload/name are borrowed for this call.
pub struct ProjectFileUpload<'a> {
    pub id: FileId,
    pub object: ObjectId,
    pub owner: [u8; 16],
    pub name: &'a str,
    pub payload: &'a [u8],
}
impl std::fmt::Debug for ProjectFileUpload<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProjectFileUpload")
            .field("bytes", &self.payload.len())
            .finish_non_exhaustive()
    }
}
impl AccountRoot {
    /// Native backend authority only. No anonymous/user upload policy or HTTP API.
    pub fn publish_project_file(
        &mut self,
        project: &str,
        key: &str,
        upload: ProjectFileUpload<'_>,
    ) -> Result<FileInfo> {
        self.with_file_write(project, key, true, |native, guard| {
            native.publish_guarded(
                upload.id,
                upload.object,
                upload.owner,
                upload.name,
                upload.payload,
                guard,
            )
        })
    }
    pub fn rename_project_file(
        &mut self,
        project: &str,
        key: &str,
        id: FileId,
        expected_revision: u64,
        name: &str,
    ) -> Result<FileInfo> {
        self.with_file_write(project, key, false, |native, guard| {
            native.rename_guarded(id, expected_revision, name, guard)
        })
    }
    /// Remove a logical reference only; physical immutable orphan charge remains.
    pub fn remove_project_file(
        &mut self,
        project: &str,
        key: &str,
        id: FileId,
        expected_revision: u64,
    ) -> Result<FileRemoval> {
        self.with_file_write(project, key, false, |native, guard| {
            native.remove_guarded(id, expected_revision, guard)
        })
    }
    /// Current scoped/global CAS metadata, not a permission or quota reservation.
    pub fn project_file_quota(&mut self, project: &str, key: &str) -> Result<QuotaState> {
        self.with_file_component(project, key, |native, _| Ok(native.quota_state()?))
    }
    pub fn set_project_file_quota(
        &mut self,
        project: &str,
        key: &str,
        expected: QuotaState,
        quota: FileQuota,
    ) -> Result<QuotaState> {
        self.with_file_write(project, key, false, |native, guard| {
            native.set_quota_guarded(expected, quota, guard)
        })
    }
    fn with_file_write<T>(
        &mut self,
        project: &str,
        key: &str,
        publishes_blob: bool,
        work: impl FnOnce(
            &mut NativeFileRoot,
            &mut dyn FnMut(NativeWritePhase) -> emilybase_files::Result<()>,
        ) -> emilybase_files::Result<T>,
    ) -> Result<T> {
        let mut published = false;
        let result = self.with_file_component(project, key, |native, check| {
            let mut outer_error = None;
            let result = {
                let mut guard = |phase| {
                    if phase == NativeWritePhase::Committed
                        || (publishes_blob && phase == NativeWritePhase::BeforeCommit)
                    {
                        published = true;
                    }
                    checkpoint(match phase {
                        NativeWritePhase::BeforeOperation => "account_file_write_before_operation",
                        NativeWritePhase::BeforeBlob => "account_file_write_before_blob",
                        NativeWritePhase::BeforeCommit => "account_file_write_before_commit",
                        NativeWritePhase::Committed => "account_file_write_committed",
                        NativeWritePhase::Verified => "account_file_write_verified",
                    });
                    check().map_err(|error| {
                        outer_error = Some(error);
                        emilybase_files::Error::GuardRejected
                    })
                };
                work(native, &mut guard)
            };
            if matches!(&result, Err(emilybase_files::Error::OutcomeUnknown(_))) {
                published = true;
            }
            checkpoint("account_file_write_native_returned");
            result.map_err(|error| outer_error.unwrap_or(Error::Files(error)))
        });
        match result {
            Err(error) if published => {
                // Include a late common/key failure after native success. Even
                // restoring that external metadata cannot resume this owner.
                self.file_write_poisoned = true;
                Err(Error::AccountFileWriteUnknown(Box::new(error)))
            }
            other => other,
        }
    }
}
