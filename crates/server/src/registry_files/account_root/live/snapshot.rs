use super::super::{AccountBundleRootManifest, AccountBundleRootReport, Structure};
use super::*;

/// Sensitive immutable copy of the existing v1 bundle plus declared root metadata.
/// No live owner, current authorization, publication or combined file capture.
pub struct AccountRootSnapshot {
    image: Vec<u8>,
    manifest: AccountBundleRootManifest,
    report: AccountBundleRootReport,
}
impl AccountRootSnapshot {
    pub fn bundle_bytes(&self) -> &[u8] {
        &self.image
    }
    pub fn into_bytes(self) -> Vec<u8> {
        self.image
    }
    pub fn manifest(&self) -> &AccountBundleRootManifest {
        &self.manifest
    }
    pub fn report(&self) -> &AccountBundleRootReport {
        &self.report
    }
}
impl std::fmt::Debug for AccountRootSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountRootSnapshot")
            .field("bytes", &self.image.len())
            .field("private_stores", &self.report.private_accounts.len())
            .finish_non_exhaustive()
    }
}
impl AccountRoot {
    pub(crate) fn snapshot_manifest(&self) -> Result<AccountBundleRootManifest> {
        self.ready()?;
        Ok(self.contents.manifest.clone())
    }

    /// Trusted native readonly capture under original root/private/public owners.
    /// Returned sensitive v1 bytes are copied data, never user/file authority.
    pub fn capture(&mut self) -> Result<AccountRootSnapshot> {
        self.capture_with(Ok)
    }

    // Internal readonly coordination work runs before any original data/private
    // owner is released. Results are withheld if final source checks fail.
    pub(crate) fn capture_with<T>(
        &mut self,
        work: impl FnOnce(AccountRootSnapshot) -> Result<T>,
    ) -> Result<T> {
        self.ready()?;
        let contents = &mut self.contents;
        let guard = Structure {
            path: &self.selected,
            owner: &self.owner,
            private_owner: &contents.private_owner,
            account_owners: &contents.account_owners,
            manifest_owner: &contents.manifest_owner,
            encoded: &contents.encoded,
            manifest: &contents.manifest,
            ids: &contents.ids,
            registry: contents.registry.directory_identity()?,
        };
        let parent = self.parent.as_ref();
        let manifest = contents.manifest.clone();
        let result = contents.registry.capture_account_bundle_with(
            &mut contents.accounts,
            |image, accounts| {
                Self::check_parent_owner(parent)?;
                guard.check()?;
                guard.check_wals()?;
                for account in accounts.iter() {
                    account.check_directory_at(guard.private_owner, account.project())?;
                }
                let report = crate::inspect_account_bundle_bytes(&image)?;
                if report
                    .private_accounts
                    .iter()
                    .map(|p| p.project.as_str())
                    .ne(manifest.private_projects.iter().map(String::as_str))
                {
                    return Err(Error::BundleRoot("captured private roster changed"));
                }
                let snapshot = AccountRootSnapshot {
                    image,
                    report: AccountBundleRootReport {
                        registry: report.registry,
                        private_accounts: report.private_accounts,
                        reset_at: manifest.reset_at,
                    },
                    manifest,
                };
                super::super::checkpoint("root_capture_copied");
                let outcome = work(snapshot);
                super::super::checkpoint("root_capture_coordinated");
                for account in accounts.iter() {
                    account.check_directory_at(guard.private_owner, account.project())?;
                }
                guard.check()?;
                guard.check_wals()?;
                Self::check_parent_owner(parent)?;
                outcome
            },
        );
        self.ready()?;
        result
    }
}
