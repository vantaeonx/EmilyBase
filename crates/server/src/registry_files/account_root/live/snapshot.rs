use super::super::{AccountBundleRootManifest, AccountBundleRootReport, Structure};
use super::*;
use crate::{AccountComponentSnapshot, AccountFileSnapshot, ProjectStore, component_snapshot};

/// Sensitive immutable copy of the existing v1 bundle plus declared root metadata.
/// No live owner, current authorization, publication or combined file capture.
pub struct AccountRootSnapshot {
    component: AccountComponentSnapshot,
    manifest: AccountBundleRootManifest,
}
impl AccountRootSnapshot {
    pub fn bundle_bytes(&self) -> &[u8] {
        self.component.bundle_bytes()
    }
    pub fn into_bytes(self) -> Vec<u8> {
        self.component.into_bytes()
    }
    pub fn manifest(&self) -> &AccountBundleRootManifest {
        &self.manifest
    }
    pub fn report(&self) -> &AccountBundleRootReport {
        self.component.report()
    }
    pub(crate) fn into_component(self) -> AccountComponentSnapshot {
        self.component
    }
}
impl std::fmt::Debug for AccountRootSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountRootSnapshot")
            .field("bytes", &self.bundle_bytes().len())
            .field("private_stores", &self.report().private_accounts.len())
            .finish_non_exhaustive()
    }
}
impl AccountRoot {
    /// Copied declared private roster for trusted native maintenance preflight.
    /// This metadata neither discovers file roots nor authorizes a user.
    pub fn private_projects(&self) -> Result<Vec<String>> {
        self.ready()?;
        Ok(self.contents.manifest.private_projects().to_vec())
    }
    /// Copied native layout metadata only, not current user/file authority.
    pub fn file_manifest(&self) -> Result<Option<crate::AccountFileRootManifest>> {
        self.ready()?;
        Ok(self.contents.manifest.v2().cloned())
    }
    pub(crate) fn snapshot_manifest(&self) -> Result<AccountBundleRootManifest> {
        self.ready()?;
        self.contents.manifest.v1()
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
        self.capture_v1(false, work)
    }
    pub(crate) fn capture_physical_with<T>(
        &mut self,
        work: impl FnOnce(AccountRootSnapshot) -> Result<T>,
    ) -> Result<T> {
        self.capture_v1(true, work)
    }
    fn capture_v1<T>(
        &mut self,
        physical: bool,
        work: impl FnOnce(AccountRootSnapshot) -> Result<T>,
    ) -> Result<T> {
        self.ready()?;
        let manifest = self.contents.manifest.v1()?;
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
            files: contents.files.topology(),
        };
        let parent = self.parent.as_ref();
        let result = Capture {
            guard,
            parent,
            registry: &mut contents.registry,
            accounts: &mut contents.accounts,
        }
        .run(physical, |component| {
            work(AccountRootSnapshot {
                component,
                manifest,
            })
        });
        self.ready()?;
        result
    }

    /// Complete sensitive v2 copy from the internally retained native owners.
    /// No partial v1 root export, filesystem discovery, clock or session reset.
    pub fn capture_v2(&mut self) -> Result<AccountFileSnapshot> {
        self.capture_v2_with(|_| Ok(()))
    }
    pub(crate) fn capture_v2_with(
        &mut self,
        work: impl FnOnce(&AccountFileSnapshot) -> Result<()>,
    ) -> Result<AccountFileSnapshot> {
        self.ready()?;
        let manifest = self
            .contents
            .manifest
            .v2()
            .cloned()
            .ok_or(Error::BundleRoot("explicit v2 capture requires v2 root"))?;
        let contents = &mut self.contents;
        let (files, topology) = contents.files.parts();
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
            files: topology,
        };
        let capture = Capture {
            guard,
            parent: self.parent.as_ref(),
            registry: &mut contents.registry,
            accounts: &mut contents.accounts,
        };
        let result = component_snapshot::with_owned_files(files, &manifest, |files, admitted| {
            capture.run(true, |accounts| {
                let snapshot =
                    AccountFileSnapshot::from_owned(accounts, files, &admitted, manifest.clone())?;
                super::super::checkpoint("components_account_copied");
                work(&snapshot)?;
                Ok(snapshot)
            })
        });
        // All nested payload/native proofs have completed. Check every actual
        // child selection again, with no detached replacement owner in between.
        self.ready()?;
        result
    }
}
struct Capture<'a> {
    guard: Structure<'a>,
    parent: Option<&'a File>,
    registry: &'a mut ProjectStore,
    accounts: &'a mut [AccountStore],
}
impl Capture<'_> {
    fn run<T>(
        self,
        strict_physical: bool,
        work: impl FnOnce(AccountComponentSnapshot) -> Result<T>,
    ) -> Result<T> {
        let Self {
            guard,
            parent,
            registry,
            accounts,
        } = self;
        // Committed prefixes alone cannot detect external unacknowledged tails,
        // same-byte replacements or disposable-cache changes during publication.
        // Retain independent readers of the bounded original common tree too,
        // including public/private journals and disposable checkpoint files.
        // Plain v1 prefix capture deliberately excludes concurrent unacknowledged
        // tails. Joined capture/publication requires this stronger physical proof.
        let mut physical = strict_physical
            .then(|| crate::registry_files::physical_tree::Tree::capture(guard.owner))
            .transpose()?;
        let result = registry.capture_account_bundle_with(accounts, |image, accounts| {
            AccountRoot::check_parent_owner(parent)?;
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
                .ne(guard.manifest.private_projects().iter().map(String::as_str))
            {
                return Err(Error::BundleRoot("captured private roster changed"));
            }
            let snapshot = AccountComponentSnapshot::new(
                image,
                AccountBundleRootReport {
                    registry: report.registry,
                    private_accounts: report.private_accounts,
                    reset_at: guard.manifest.reset_at(),
                },
            );
            super::super::checkpoint("root_capture_copied");
            let outcome = work(snapshot);
            super::super::checkpoint("root_capture_coordinated");
            for account in accounts.iter() {
                account.check_directory_at(guard.private_owner, account.project())?;
            }
            guard.check()?;
            guard.check_wals()?;
            AccountRoot::check_parent_owner(parent)?;
            outcome
        });
        if let Some(physical) = &mut physical {
            physical.check()?;
        }
        result
    }
}
