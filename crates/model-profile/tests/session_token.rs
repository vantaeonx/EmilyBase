#![cfg(feature = "heap-profile")]
use emilybase_auth::tokens::{TokenDigest, TokenKind, TokenScope, issue, metadata};

#[global_allocator]
static ALLOCATOR: dhat::Alloc = dhat::Alloc;

#[test]
fn matching_and_decoding_do_not_allocate_and_issued_text_has_one_bounded_owner() {
    let scope = TokenScope::new(&"11".repeat(16), [0x22; 16]).unwrap();
    let foreign = TokenScope::new(&"12".repeat(16), [0x22; 16]).unwrap();
    let (token, digest) = issue(TokenKind::Access, &scope, [0x33; 16]).unwrap();
    let encoded = digest.encode();
    let mut wrong = token.expose().as_bytes().to_vec();
    wrong[101] = if wrong[101] == b'0' { b'1' } else { b'0' };
    let wrong = String::from_utf8(wrong).unwrap();
    let oversized = "a".repeat(1024 * 1024);
    let profiler = dhat::Profiler::builder().testing().build();
    for _ in 0..1000 {
        assert!(digest.matches(token.expose(), &scope).unwrap());
        assert!(!digest.matches(&wrong, &scope).unwrap());
        assert!(!digest.matches(token.expose(), &foreign).unwrap());
        assert!(digest.matches(&oversized, &scope).is_err());
        assert!(metadata(token.expose()).is_ok());
        assert!(TokenDigest::decode(&encoded).is_ok());
        assert!(TokenDigest::decode(&[]).is_err());
    }
    let matching = dhat::HeapStats::get();
    drop(profiler);
    assert_eq!(
        (
            matching.total_bytes,
            matching.total_blocks,
            matching.curr_bytes
        ),
        (0, 0, 0)
    );
    let profiler = dhat::Profiler::builder().testing().build();
    let (fresh, _) = issue(TokenKind::Refresh, &scope, [0x44; 16]).unwrap();
    let live = dhat::HeapStats::get();
    assert_eq!(fresh.expose().len(), 102);
    drop(fresh);
    let released = dhat::HeapStats::get();
    drop(profiler);
    assert_eq!(live.total_blocks, 1);
    assert_eq!(live.total_bytes, 102);
    assert_eq!(live.curr_bytes, 102);
    assert_eq!(live.max_bytes, 102);
    assert_eq!(released.curr_bytes, 0);
    eprintln!(
        "session_token match_decode={} issued={} live_after_drop={}",
        matching.total_bytes, live.total_bytes, released.curr_bytes
    );
}
