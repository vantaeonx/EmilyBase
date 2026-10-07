#![cfg(feature = "heap-profile")]
use emilybase_auth::accounts::inspect_private_account_backup_bytes;

#[global_allocator]
static ALLOCATOR: dhat::Alloc = dhat::Alloc;

#[test]
fn early_scope_and_envelope_refusals_do_not_allocate_or_copy_input_sized_payloads() {
    let oversized = vec![0; 1024 * 1024];
    let header = [0; 128];
    let project = "11111111111111111111111111111111";
    let profiler = dhat::Profiler::builder().testing().build();
    for _ in 0..1000 {
        assert!(inspect_private_account_backup_bytes(&oversized, "../synthetic").is_err());
        assert!(inspect_private_account_backup_bytes(&oversized, project).is_err());
        assert!(inspect_private_account_backup_bytes(&header, project).is_err());
        assert!(inspect_private_account_backup_bytes(&[], project).is_err());
    }
    let observed = dhat::HeapStats::get();
    drop(profiler);
    assert_eq!(
        (
            observed.total_bytes,
            observed.total_blocks,
            observed.curr_bytes
        ),
        (0, 0, 0)
    );
    eprintln!(
        "private_archive early_refusal1000 requested_bytes={} live={}",
        observed.total_bytes, observed.curr_bytes
    );
}
