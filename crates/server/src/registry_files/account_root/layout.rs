//! Explicit format admission; a v2 manifest is never projected into v1 authority.
use super::*;
use crate::AccountFileRootManifest;
use emilybase_files::NativeFileRoot;

#[derive(Clone, Copy)]
pub(super) enum RequestedLayout {
    V1,
    V2,
}
impl RequestedLayout {
    pub(super) fn limit(self) -> usize {
        match self {
            Self::V1 => MANIFEST_LIMIT,
            Self::V2 => crate::MAX_ACCOUNT_FILE_ROOT_MANIFEST_BYTES,
        }
    }
    pub(super) fn entries(self) -> BTreeSet<String> {
        let mut names = BTreeSet::from(["private".into(), "registry".into(), "root.json".into()]);
        if matches!(self, Self::V2) {
            names.insert("files".into());
        }
        names
    }
}
pub(super) enum RootLayout {
    V1(AccountBundleRootManifest),
    V2(AccountFileRootManifest),
}
impl RootLayout {
    pub(super) fn parse(bytes: &[u8], requested: RequestedLayout) -> Result<Self> {
        Ok(match requested {
            RequestedLayout::V1 => Self::V1(inspect_account_bundle_root_manifest_bytes(bytes)?),
            RequestedLayout::V2 => {
                Self::V2(crate::inspect_account_file_root_manifest_bytes(bytes)?)
            }
        })
    }
    pub(super) fn private_projects(&self) -> &[String] {
        match self {
            Self::V1(m) => &m.private_projects,
            Self::V2(m) => m.private_projects(),
        }
    }
    pub(super) fn reset_at(&self) -> u64 {
        match self {
            Self::V1(m) => m.reset_at,
            Self::V2(m) => m.reset_at(),
        }
    }
    pub(super) fn v1(&self) -> Result<AccountBundleRootManifest> {
        match self {
            Self::V1(m) => Ok(m.clone()),
            Self::V2(_) => Err(Error::BundleRoot("v1 capture cannot omit v2 file bindings")),
        }
    }
    pub(super) fn v2(&self) -> Option<&AccountFileRootManifest> {
        match self {
            Self::V1(_) => None,
            Self::V2(m) => Some(m),
        }
    }
    pub(super) fn entries(&self) -> BTreeSet<String> {
        let requested = if self.v2().is_some() {
            RequestedLayout::V2
        } else {
            RequestedLayout::V1
        };
        requested.entries()
    }
}
// Actual native owners end before their original containing-directory anchor.
pub(super) struct FileOwners {
    roots: Vec<NativeFileRoot>,
    parent: Option<File>,
}
#[derive(Clone, Copy)]
pub(super) struct FileTopology<'a> {
    parent: Option<&'a File>,
}
impl FileTopology<'_> {
    pub(super) fn check(&self, owner: &File, layout: &RootLayout) -> Result<()> {
        match (layout.v2(), self.parent) {
            (None, None) => Ok(()),
            (Some(m), Some(parent)) => {
                metadata::owned_directory(&descriptor(owner).join("files"), parent)?;
                if names(&descriptor(parent))?
                    != m.file_stores().iter().map(|b| b.project().into()).collect()
                {
                    return Err(Error::BundleRoot("file inventory changed"));
                }
                Ok(())
            }
            _ => Err(Error::BundleRoot("file ownership layout")),
        }
    }
}
impl FileOwners {
    pub(super) fn topology(&self) -> FileTopology<'_> {
        FileTopology {
            parent: self.parent.as_ref(),
        }
    }
    pub(super) fn parts(&mut self) -> (&mut [NativeFileRoot], FileTopology<'_>) {
        (
            &mut self.roots,
            FileTopology {
                parent: self.parent.as_ref(),
            },
        )
    }
    pub(super) fn open(owner: &File, layout: &RootLayout) -> Result<Self> {
        let mut result = Self {
            roots: Vec::new(),
            parent: None,
        };
        if let Some(manifest) = layout.v2() {
            let parent = metadata::open_directory(&descriptor(owner).join("files"))?;
            if names(&descriptor(&parent))?
                != manifest
                    .file_stores()
                    .iter()
                    .map(|b| b.project().into())
                    .collect()
            {
                return Err(Error::BundleRoot("file entries"));
            }
            for binding in manifest.file_stores() {
                let project = binding
                    .project()
                    .parse()
                    .map_err(|_| Error::BundleRoot("file project"))?;
                let mut root = NativeFileRoot::open_at(&parent, binding.project(), project)?;
                if crate::component_snapshot::hex(root.inspect()?.database_id())
                    != binding.database_id()
                {
                    return Err(Error::BundleRoot("file database binding"));
                }
                result.roots.push(root);
            }
            result.parent = Some(parent);
        }
        result.check(owner, layout)?;
        Ok(result)
    }
    pub(super) fn check(&self, owner: &File, layout: &RootLayout) -> Result<()> {
        self.topology().check(owner, layout)?;
        match (layout.v2(), &self.parent) {
            (None, None) if self.roots.is_empty() => Ok(()),
            (Some(m), Some(_)) if m.file_stores().len() == self.roots.len() => {
                for root in &self.roots {
                    root.check_selection()?;
                }
                Ok(())
            }
            _ => Err(Error::BundleRoot("file ownership layout")),
        }
    }
    pub(super) fn validate(&mut self, identities: &mut BTreeSet<[u8; 16]>) -> Result<()> {
        for root in &mut self.roots {
            if !identities.insert(*root.inspect()?.database_id()) {
                return Err(Error::BundleRoot("duplicate component database identity"));
            }
        }
        Ok(())
    }
}
