//! Current self-owned publication, including uncertainty before metadata commit.
use super::*;
use emilybase_object_storage::ObjectId;

/// Borrowed user input. Ownership comes exclusively from the current session.
/// IDs are typed native names; neither is a filesystem path or access grant.
pub struct AccountFileUpload<'a> {
    pub id: FileId,
    pub object: ObjectId,
    pub name: &'a str,
    pub payload: &'a [u8],
}
impl std::fmt::Debug for AccountFileUpload<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountFileUpload")
            .field("bytes", &self.payload.len())
            .finish_non_exhaustive()
    }
}
impl AccountRoot {
    /// Publish a fresh immutable blob and logical reference under current create
    /// policy. No overwrite, automatic retry or cleanup. Read permission is
    /// independent; the receipt discloses only ID/revisions, not file metadata.
    /// Time must be a trusted service observation, never a request timestamp.
    pub fn public_publish_file(
        &mut self,
        project: &str,
        access: &str,
        now: u64,
        upload: AccountFileUpload<'_>,
    ) -> Result<AccountFileWrite> {
        let mut published = false;
        let mut blob_attempt_admitted = false;
        let result = catch_unwind(AssertUnwindSafe(|| {
            self.with_user_files(project, access, now, |native, proof, context, check| {
                let owner = proof.account_id();
                let authorize = || {
                    proof.authorize(context, Change::Create { owner }).map_err(
                        |error| match error {
                            PolicyError::Denied => Error::Denied,
                            other => {
                                Error::Accounts(emilybase_auth::accounts::Error::FilePolicy(other))
                            }
                        },
                    )
                };
                // Refuse before native validation, collision/quota observations
                // or any object write. A previous copied result grants nothing.
                authorize()?;
                let mut outer_error = None;
                let result = native.publish_guarded(
                    upload.id,
                    upload.object,
                    owner,
                    upload.name,
                    upload.payload,
                    |phase| {
                        if matches!(
                            phase,
                            NativeWritePhase::BeforeCommit
                                | NativeWritePhase::Committed
                                | NativeWritePhase::Verified
                        ) {
                            // BeforeCommit already has a durable selected blob;
                            // rejection here must preserve its physical charge.
                            published = true;
                        }
                        let checked = (|| -> Result<()> {
                            checkpoint(match phase {
                                NativeWritePhase::BeforeOperation => {
                                    "account_user_file_upload_before_operation"
                                }
                                NativeWritePhase::BeforeBlob => {
                                    "account_user_file_upload_before_blob"
                                }
                                NativeWritePhase::BeforeCommit => {
                                    "account_user_file_upload_before_commit"
                                }
                                NativeWritePhase::Committed => "account_user_file_upload_committed",
                                NativeWritePhase::Verified => "account_user_file_upload_verified",
                            });
                            check()?;
                            proof
                                .check_context(context)
                                .map_err(emilybase_auth::accounts::Error::FilePolicy)?;
                            authorize()
                        })();
                        if let Err(error) = checked {
                            outer_error = Some(error);
                            return Err(emilybase_files::Error::GuardRejected);
                        }
                        if phase == NativeWritePhase::BeforeBlob {
                            // A panic during the next object write can leave a
                            // blob even without a later metadata callback.
                            blob_attempt_admitted = true;
                            checkpoint("account_user_file_upload_blob_admitted");
                        }
                        Ok(())
                    },
                );
                match &result {
                    Ok(_) | Err(emilybase_files::Error::OutcomeUnknown(_)) => {
                        published = true;
                    }
                    // The native contract reports every post-publication error
                    // as OutcomeUnknown. Ordinary quota/collision refusal can
                    // follow BeforeBlob but has no newly published object.
                    Err(_) => blob_attempt_admitted = false,
                }
                checkpoint("account_user_file_upload_native_returned");
                let info = result.map_err(|error| outer_error.unwrap_or(Error::Files(error)))?;
                Ok(AccountFileWrite {
                    id: upload.id,
                    revision: info.revision(),
                    // Absence at creation, not a claim about historical ID reuse.
                    previous_revision: 0,
                })
            })
        }));
        match result {
            Ok(Err(error)) if published => {
                self.file_write_poisoned = true;
                Err(Error::AccountFileWriteUnknown(Box::new(error)))
            }
            Err(unwind) => {
                if published || blob_attempt_admitted {
                    self.file_write_poisoned = true;
                }
                resume_unwind(unwind)
            }
            Ok(result) => result,
        }
    }
}
