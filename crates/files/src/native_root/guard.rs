//! Trusted outer coordination at the actual native write boundaries.
use super::*;

/// A phase label only; no metadata database, selected blob or owner escapes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeWritePhase {
    BeforeOperation,
    BeforeBlob,
    BeforeCommit,
    Committed,
    Verified,
}
impl NativeWritePhase {
    pub(super) fn of(check: &WriteCheck<'_>) -> Option<Self> {
        match check {
            WriteCheck::Selected(_) => None,
            WriteCheck::BeforeBlob => Some(Self::BeforeBlob),
            WriteCheck::BeforeCommit => Some(Self::BeforeCommit),
            WriteCheck::Committed(_) => Some(Self::Committed),
            WriteCheck::Verified => Some(Self::Verified),
        }
    }
}
pub(super) type Guard<'a> = Option<&'a mut dyn FnMut(NativeWritePhase) -> Result<()>>;
impl NativeFileRoot {
    /// Native coordination only; the callback must enforce any outer authority.
    /// It runs at actual blob/metadata boundaries under the existing owners.
    /// Rejection after blob selection/commit preserves data and is uncertain.
    pub fn publish_guarded(
        &mut self,
        id: FileId,
        object: ObjectId,
        owner: [u8; 16],
        name: &str,
        payload: &[u8],
        mut guard: impl FnMut(NativeWritePhase) -> Result<()>,
    ) -> Result<FileInfo> {
        self.guarded_operation(|root| {
            root.publish_with_guard(id, object, owner, name, payload, (|_| {}, Some(&mut guard)))
        })
    }
    pub(super) fn guarded_operation<T>(
        &mut self,
        work: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        // A trusted callback can unwind after a durable side effect. Never let
        // a caller catch that panic and resume this partially observed owner.
        // Mutable state is not reused on unwind: poison before propagating it.
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(self))) {
            Ok(value) => value,
            Err(panic) => {
                self.store.poisoned = true;
                std::panic::resume_unwind(panic)
            }
        }
    }
}
