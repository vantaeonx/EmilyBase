use crate::primary::eligible;
use crate::{Error, MAX_ROWS, PrimaryIndexInfo, Result, Snapshot};
use emilybase_index::BPlusTree;
use sha2::{Digest, Sha256};

impl Snapshot {
    /// Streaming fingerprint of exact relational pages. Not a credential or database ID.
    pub fn page_fingerprint(&self) -> [u8; 32] {
        let mut hash = Sha256::new();
        for page in self.pages() {
            hash.update(page.encode());
        }
        hash.finalize().into()
    }
    /// Export a validated stable-ID copy of the current eligible primary tree.
    pub fn export_primary_tree(&self, name: &str) -> Result<BPlusTree> {
        let table = self.state.table(name)?;
        let tree = self
            .primary_indexes
            .tree(self.table_id(name)?, &table.rows, &self.locations)?;
        let images = tree
            .page_images()
            .map_err(|_| Error::PrimaryIndex("tree export failed"))?;
        let tree = BPlusTree::from_stable_pages(tree.root_id(), &images)
            .map_err(|_| Error::PrimaryIndex("stable tree export failed"))?;
        self.verify_primary_tree(name, &tree)?;
        Ok(tree)
    }
    /// Verify every eligible live key and pointer without cloning row state.
    /// Persistent identity/history binding belongs to the managed caller.
    pub fn verify_primary_tree(&self, name: &str, tree: &BPlusTree) -> Result<PrimaryIndexInfo> {
        let table_id = self.table_id(name)?;
        let table = self.state.table(name)?;
        if tree
            .validate()
            .map_err(|_| Error::PrimaryIndex("import topology"))?
            != tree.len()
        {
            return Err(Error::PrimaryIndex("import entry count"));
        }
        let entries = tree
            .range(None, None, MAX_ROWS)
            .map_err(|_| Error::PrimaryIndex("import range"))?;
        if !entries
            .iter()
            .map(|(key, _)| key)
            .eq(table.rows.keys().filter(|key| eligible(key)))
        {
            return Err(Error::PrimaryIndex("import live key mismatch"));
        }
        for (key, pointer) in &entries {
            let location = self
                .locations
                .get(table_id, key)
                .ok_or(Error::PrimaryIndex("import missing location"))?;
            if (pointer.page_id, pointer.slot_id) != (location.page_id, location.slot_id) {
                return Err(Error::PrimaryIndex("import obsolete pointer"));
            }
            self.resolve_row_location(name, key, location)?;
        }
        let info = PrimaryIndexInfo {
            entries: tree.len(),
            excluded_long_keys: table
                .rows
                .len()
                .checked_sub(tree.len())
                .ok_or(Error::PrimaryIndex("import oversized entries"))?,
            pages: tree.page_count(),
            root_id: tree.root_id(),
        };
        Ok(info)
    }

    /// Publish only a fully validated derived cell; failure changes nothing.
    pub fn install_primary_tree(
        &mut self,
        name: &str,
        tree: BPlusTree,
    ) -> Result<PrimaryIndexInfo> {
        let info = self.verify_primary_tree(name, &tree)?;
        self.primary_indexes.install(self.table_id(name)?, tree);
        Ok(info)
    }
}
