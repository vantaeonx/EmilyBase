//! Additional common-root checks at actual native publication/commit boundaries.
use emilybase_transactions::Database;

pub(crate) enum WriteCheck<'a> {
    BeforeBlob,
    BeforeCommit,
    Committed(&'a mut Database),
    Verified,
}
