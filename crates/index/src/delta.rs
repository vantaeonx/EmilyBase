use std::collections::{BTreeMap, BTreeSet};

use crate::{BPlusTree, Error, IndexPage, IndexSnapshot, MAX_INDEX_PAGES, PAGE_SIZE, Result};
use sha2::{Digest, Sha256};

/// In-memory atomic write set. There is no managed-WAL publication in this API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotDelta {
    pub base_revision: u64,
    pub base_fingerprint: [u8; 32],
    pub revision: u64,
    pub root: u64,
    pub entries: usize,
    pub upserts: Vec<[u8; PAGE_SIZE]>,
    pub retired: Vec<u64>,
}

impl IndexSnapshot {
    /// Bind a delta to the exact canonical base, including its revision/root.
    pub fn fingerprint(&self) -> Result<[u8; 32]> {
        Self::validate_structure(self.revision, &self.tree)?;
        let mut digest = Sha256::new();
        digest.update(self.header(self.tree.page_count()));
        for page in self.tree.pages.values() {
            digest.update(Self::checked_image(page)?);
        }
        Ok(digest.finalize().into())
    }

    pub fn delta_to(&self, tree: &BPlusTree) -> Result<SnapshotDelta> {
        let base_fingerprint = self.fingerprint()?;
        let revision = self.revision.checked_add(1).ok_or(Error::Limit)?;
        Self::validate_tree(revision, tree)?;
        let upserts = tree
            .pages
            .iter()
            .filter(|(id, page)| self.tree.pages.get(id) != Some(page))
            .map(|(_, page)| page.encode())
            .collect::<Result<Vec<_>>>()?;
        let retired = self
            .tree
            .pages
            .keys()
            .filter(|id| !tree.pages.contains_key(id))
            .copied()
            .collect();
        Ok(SnapshotDelta {
            base_revision: self.revision,
            base_fingerprint,
            revision,
            root: tree.root_id(),
            entries: tree.len(),
            upserts,
            retired,
        })
    }
}

impl SnapshotDelta {
    /// Validate an entire write set before returning a new tree; the base is never mutated.
    pub fn apply(&self, base: &IndexSnapshot) -> Result<IndexSnapshot> {
        if self.base_revision != base.revision
            || base.revision.checked_add(1) != Some(self.revision)
            || self.upserts.len() > MAX_INDEX_PAGES
            || self.retired.len() > MAX_INDEX_PAGES
        {
            return Err(Error::Layout("delta bounds or revision"));
        }
        if base.fingerprint()? != self.base_fingerprint {
            return Err(Error::Layout("delta base fingerprint"));
        }
        let mut removed = BTreeSet::new();
        let mut previous = 0;
        for id in &self.retired {
            if *id <= previous || !base.tree.pages.contains_key(id) {
                return Err(Error::Layout("delta retired IDs"));
            }
            removed.insert(*id);
            previous = *id;
        }
        let mut changes = BTreeMap::new();
        previous = 0;
        for image in &self.upserts {
            let id = u64::from_le_bytes(image[8..16].try_into().map_err(|_| Error::PageId)?);
            if id <= previous || id > MAX_INDEX_PAGES as u64 || removed.contains(&id) {
                return Err(Error::Layout("delta upsert IDs"));
            }
            let page = IndexPage::decode(image, id)?;
            if base.tree.pages.get(&id) == Some(&page) {
                return Err(Error::Layout("delta unchanged page"));
            }
            changes.insert(id, page);
            previous = id;
        }
        let mut pages = base.tree.pages.clone();
        for id in removed {
            pages.remove(&id);
        }
        for (id, page) in changes {
            pages.insert(id, page);
        }
        if pages.len() > MAX_INDEX_PAGES {
            return Err(Error::Limit);
        }
        let tree = BPlusTree {
            pages,
            root: self.root,
            len: self.entries,
            stable_ids: true,
        };
        let next = IndexSnapshot {
            revision: self.revision,
            tree,
        };
        // The owned candidate uses exactly the same complete topology, identity
        // and physical round-trip checks as a snapshot. Never publish a partial
        // map merely because every individual delta page decoded successfully.
        next.validate().map_err(|error| match error {
            Error::Layout("snapshot entry count") => Error::Layout("delta entry count"),
            other => other,
        })?;
        Ok(next)
    }
}
