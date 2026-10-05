use emilybase_commit_format::RootBinding;
use emilybase_index::IndexSnapshot;

use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    pub(crate) binding: RootBinding,
    pub(crate) index: IndexSnapshot,
}

impl Selection {
    pub(crate) fn new(binding: RootBinding, index: IndexSnapshot) -> Result<Self> {
        // Validate the complete bounded stable-ID arena, not only a root header.
        index.encode()?;
        if binding.revision() != index.revision
            || binding.address().page() != index.tree.root_id()
            || binding.pages() as usize != index.tree.page_count()
            || binding.covered() != index.tree.len() as u64
        {
            return Err(Error::Selection("root/index image mismatch"));
        }
        Ok(Self { binding, index })
    }

    pub fn binding(&self) -> RootBinding {
        self.binding
    }

    pub fn index(&self) -> &IndexSnapshot {
        &self.index
    }
}
