//! Installed current-user read rules under actual private/native/common owners.
use super::*;
use emilybase_auth::{
    accounts::FilePolicyPrincipal,
    file_policy::{Change, PolicyError, StoreContext},
};
mod mutation;
pub use mutation::AccountFileWrite;

fn readable(
    proof: &FilePolicyPrincipal<'_>,
    context: StoreContext<'_>,
    info: &FileInfo,
) -> Result<bool> {
    match proof.authorize(
        context,
        Change::Read {
            owner: *info.owner(),
        },
    ) {
        Ok(()) => Ok(true),
        Err(PolicyError::Denied) => Ok(false),
        Err(error) => Err(Error::Accounts(
            emilybase_auth::accounts::Error::FilePolicy(error),
        )),
    }
}
impl AccountRoot {
    /// Current user visibility only. No service key, physical orphan or handle.
    /// The complete result remains bounded by the native metadata/inventory caps.
    /// Time is a trusted service observation; a forward denied authentication may
    /// append its private clock. No implicit migration/provisioning occurs.
    pub fn public_list_files(
        &mut self,
        project: &str,
        access: &str,
        now: u64,
    ) -> Result<Vec<FileInfo>> {
        self.with_user_files(project, access, now, |native, proof, context, _| {
            let entries = native.list()?;
            let mut visible = Vec::new();
            visible
                .try_reserve_exact(entries.len())
                .map_err(|_| Error::Files(emilybase_files::Error::Allocation))?;
            for info in entries {
                if readable(proof, context, &info)? {
                    visible.push(info);
                }
            }
            Ok(visible)
        })
    }
    /// Missing and unreadable references have the same copied metadata result.
    pub fn public_file_info(
        &mut self,
        project: &str,
        access: &str,
        now: u64,
        id: FileId,
    ) -> Result<Option<FileInfo>> {
        self.with_user_files(
            project,
            access,
            now,
            |native, proof, context, _| match native.file_info(id)? {
                Some(info) if readable(proof, context, &info)? => Ok(Some(info)),
                _ => Ok(None),
            },
        )
    }
    /// Complete bounded payload, authorized from the actual retained reader's
    /// current owner. Output is withheld through original reader/common checks.
    /// Trusted service time only; a forward denied authentication can append its clock.
    pub fn public_read_file(
        &mut self,
        project: &str,
        access: &str,
        now: u64,
        id: FileId,
        maximum_bytes: usize,
    ) -> Result<AccountFileRead> {
        self.with_user_files(project, access, now, |native, proof, context, check| {
            read_file_checked(native, id, maximum_bytes, check, |info| {
                if readable(proof, context, info)? {
                    Ok(())
                } else {
                    Err(Error::Denied)
                }
            })
            .map_err(|error| match error {
                Error::Files(emilybase_files::Error::Missing) => Error::Denied,
                other => other,
            })
        })
    }
    fn with_user_files<T>(
        &mut self,
        project: &str,
        access: &str,
        now: u64,
        work: impl FnOnce(
            &mut NativeFileRoot,
            &FilePolicyPrincipal<'_>,
            StoreContext<'_>,
            &mut dyn FnMut() -> Result<()>,
        ) -> Result<T>,
    ) -> Result<T> {
        self.with_file_scope(
            project,
            FileAuthority::User { access, now },
            |native, account, check| {
                let (account, _) = account.ok_or(Error::Denied)?;
                super::super::public_user::admission(account)?;
                let context = StoreContext {
                    project,
                    database: *native.inspect()?.database_id(),
                };
                let proof = account.verify_file_policy_access(access, now, context)?;
                proof
                    .check_context(context)
                    .map_err(emilybase_auth::accounts::Error::FilePolicy)?;
                checkpoint("account_user_file_principal");
                check()?;
                let result = work(native, &proof, context, check);
                check()?;
                checkpoint("account_user_file_result");
                check()?;
                result
            },
        )
    }
}
