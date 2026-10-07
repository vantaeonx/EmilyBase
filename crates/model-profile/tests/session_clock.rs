#![cfg(feature = "heap-profile")]
use emilybase_auth::accounts::inspect_session_clock_record;
use emilybase_catalog::Value;

#[global_allocator]
static ALLOCATOR: dhat::Alloc = dhat::Alloc;

#[test]
fn clock_metadata_inspection_has_no_requested_heap_payload() {
    let valid = [
        Value::Integer(1),
        Value::Integer(1),
        Value::Integer(i64::MAX),
    ];
    let mut wrong = valid.clone();
    wrong[2] = Value::Integer(-1);
    let oversized = [
        Value::Bytes(vec![0; 1024 * 1024]),
        Value::Integer(1),
        Value::Integer(100),
    ];
    let profiler = dhat::Profiler::builder().testing().build();
    for _ in 0..1000 {
        assert_eq!(
            inspect_session_clock_record(&valid).unwrap(),
            i64::MAX as u64
        );
        assert!(inspect_session_clock_record(&wrong).is_err());
        assert!(inspect_session_clock_record(&oversized).is_err());
        assert!(inspect_session_clock_record(&[]).is_err());
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
        "session_clock parse1000 requested_bytes={} live={}",
        observed.total_bytes, observed.curr_bytes
    );
}
