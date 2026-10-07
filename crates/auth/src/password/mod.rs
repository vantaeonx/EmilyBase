//! Synchronous salted password hashing. User/session storage and HTTP login are separate.
mod record;
#[cfg(test)]
mod tests;

use argon2::{Algorithm, Argon2, Block, Params, Version};
pub use record::{PASSWORD_RECORD_BYTES, PasswordDigest};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

pub const MAX_PASSWORD_BYTES: usize = 1024;
pub const MAX_PASSWORD_OPERATIONS: usize = 4;
pub const PASSWORD_MEMORY_KIB: u32 = 19 * 1024;
pub const PASSWORD_ITERATIONS: u32 = 2;
pub const PASSWORD_PARALLELISM: u32 = 1;
pub const PASSWORD_MEMORY_BYTES: usize = PASSWORD_MEMORY_KIB as usize * 1024;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PasswordError {
    #[error("password must contain 1..1024 bytes")]
    Input,
    #[error("invalid password digest record")]
    Record,
    #[error("unsupported password record version {0}")]
    Version(u16),
    #[error("unsupported password hash policy")]
    Policy,
    #[error("invalid password operation pool configuration")]
    Configuration,
    #[error("password operation capacity is exhausted")]
    Busy,
    #[error("password workspace allocation failed")]
    Allocation,
    #[error("operating-system randomness is unavailable")]
    Randomness,
    #[error("password hashing failed")]
    Hashing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PasswordUsage {
    pub operations: usize,
    /// Admitted fixed block payload. Allocator rounding/stacks/caller inputs are excluded.
    pub workspace_bytes: usize,
}

struct Shared {
    maximum: usize,
    active: AtomicUsize,
    hasher: Argon2<'static>,
    blocks: usize,
}

/// Clones share admission. Independent pools have independent limits; callers
/// should retain one shared pool across their intended project/request scope.
#[derive(Clone)]
pub struct PasswordPool {
    shared: Arc<Shared>,
}
impl std::fmt::Debug for PasswordPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PasswordPool")
            .field("maximum", &self.shared.maximum)
            .field("usage", &self.usage())
            .finish()
    }
}

struct Lease(Arc<Shared>);
impl Drop for Lease {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::Release);
    }
}
struct Workspace {
    // Field order matters: wipe/free blocks before returning the admission slot.
    blocks: Zeroizing<Vec<Block>>,
    _lease: Lease,
}

impl PasswordPool {
    pub fn new(maximum: usize) -> Result<Self, PasswordError> {
        if !(1..=MAX_PASSWORD_OPERATIONS).contains(&maximum) {
            return Err(PasswordError::Configuration);
        }
        let params = Params::new(
            PASSWORD_MEMORY_KIB,
            PASSWORD_ITERATIONS,
            PASSWORD_PARALLELISM,
            Some(32),
        )
        .map_err(|_| PasswordError::Configuration)?;
        let blocks = params.block_count();
        if blocks.checked_mul(std::mem::size_of::<Block>()) != Some(PASSWORD_MEMORY_BYTES) {
            return Err(PasswordError::Configuration);
        }
        Ok(Self {
            shared: Arc::new(Shared {
                maximum,
                active: AtomicUsize::new(0),
                hasher: Argon2::new(Algorithm::Argon2id, Version::V0x13, params),
                blocks,
            }),
        })
    }

    pub fn usage(&self) -> PasswordUsage {
        let operations = self.shared.active.load(Ordering::Acquire);
        PasswordUsage {
            operations,
            workspace_bytes: operations * PASSWORD_MEMORY_BYTES,
        }
    }

    fn reserve(&self) -> Result<Lease, PasswordError> {
        let mut current = self.shared.active.load(Ordering::Acquire);
        loop {
            if current >= self.shared.maximum {
                return Err(PasswordError::Busy);
            }
            match self.shared.active.compare_exchange_weak(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Ok(Lease(Arc::clone(&self.shared))),
                Err(actual) => current = actual,
            }
        }
    }

    fn workspace(&self) -> Result<Workspace, PasswordError> {
        // Reserve before requesting even the first block of memory. Error paths
        // keep the slot until the already-created buffer has been wiped/freed.
        let lease = self.reserve()?;
        let mut workspace = Workspace {
            blocks: Zeroizing::new(Vec::new()),
            _lease: lease,
        };
        workspace
            .blocks
            .try_reserve_exact(self.shared.blocks)
            .map_err(|_| PasswordError::Allocation)?;
        if workspace.blocks.capacity() != self.shared.blocks {
            return Err(PasswordError::Allocation);
        }
        workspace.blocks.resize(self.shared.blocks, Block::new());
        Ok(workspace)
    }

    fn derive(
        &self,
        password: &[u8],
        salt: &[u8; 16],
    ) -> Result<Zeroizing<[u8; 32]>, PasswordError> {
        validate_input(password)?;
        let mut workspace = self.workspace()?;
        let mut output = Zeroizing::new([0; 32]);
        self.shared
            .hasher
            .hash_password_into_with_memory(
                password,
                salt,
                output.as_mut_slice(),
                workspace.blocks.as_mut_slice(),
            )
            .map_err(|_| PasswordError::Hashing)?;
        Ok(output)
    }

    /// Use fresh OS entropy for every salt. Borrow the caller's exact password
    /// bytes without normalization/truncation/copying; caller owns their lifetime.
    pub fn hash(&self, password: &[u8]) -> Result<PasswordDigest, PasswordError> {
        validate_input(password)?;
        let mut salt = [0; 16];
        getrandom::fill(&mut salt).map_err(|_| PasswordError::Randomness)?;
        let digest = self.derive(password, &salt)?;
        Ok(PasswordDigest::from_parts(salt, *digest))
    }

    /// Only an already-validated fixed-policy digest can reach the costly KDF.
    /// A wrong password returns false after a timing-safe complete digest comparison.
    pub fn verify(&self, password: &[u8], digest: &PasswordDigest) -> Result<bool, PasswordError> {
        let actual = self.derive(password, &digest.salt)?;
        Ok(bool::from(actual.as_slice().ct_eq(&digest.hash)))
    }
}

fn validate_input(password: &[u8]) -> Result<(), PasswordError> {
    if password.is_empty() || password.len() > MAX_PASSWORD_BYTES {
        Err(PasswordError::Input)
    } else {
        Ok(())
    }
}
