#![cfg(feature = "heap-profile")]
use emilybase_auth::password::{
    MAX_PASSWORD_BYTES, PASSWORD_MEMORY_BYTES, PasswordDigest, PasswordPool,
};

#[global_allocator]
static ALLOCATOR: dhat::Alloc = dhat::Alloc;

#[test]
fn password_workspace_is_single_bounded_allocation_and_rejections_allocate_nothing() {
    let pool = PasswordPool::new(1).unwrap();
    let digest = pool.hash(b"synthetic-password").unwrap();
    let mut malformed = digest.encode();
    malformed[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
    let oversized = vec![0x61; MAX_PASSWORD_BYTES + 1];
    let profiler = dhat::Profiler::builder().testing().build();
    for _ in 0..1000 {
        assert!(PasswordDigest::decode(&malformed).is_err());
        assert!(pool.hash(&[]).is_err());
        assert!(pool.verify(&oversized, &digest).is_err());
        assert_eq!(pool.usage().workspace_bytes, 0);
    }
    let rejects = dhat::HeapStats::get();
    drop(profiler);
    assert_eq!(
        (
            rejects.total_bytes,
            rejects.total_blocks,
            rejects.curr_bytes
        ),
        (0, 0, 0)
    );
    let mut samples = Vec::new();
    for correct in [true, false] {
        let profiler = dhat::Profiler::builder().testing().build();
        let verified = pool
            .verify(
                if correct {
                    b"synthetic-password"
                } else {
                    b"wrong-password"
                },
                &digest,
            )
            .unwrap();
        let sample = dhat::HeapStats::get();
        drop(profiler);
        assert_eq!(verified, correct);
        assert_eq!(sample.total_bytes, PASSWORD_MEMORY_BYTES as u64);
        assert_eq!(sample.total_blocks, 1);
        assert_eq!(sample.max_bytes, PASSWORD_MEMORY_BYTES);
        assert_eq!(sample.curr_bytes, 0);
        assert_eq!(pool.usage().workspace_bytes, 0);
        samples.push(sample.total_bytes);
    }
    eprintln!(
        "password_workspace rejects={} correct={} wrong={} live=0",
        rejects.total_bytes, samples[0], samples[1]
    );
}
