//! Standalone synthetic token sample without test-harness allocator traffic.
use emilybase_auth::tokens::{TokenDigest, TokenKind, TokenScope, issue, metadata};
use std::process::ExitCode;

#[global_allocator]
static ALLOCATOR: dhat::Alloc = dhat::Alloc;

#[derive(serde::Serialize)]
struct Heap {
    total_bytes: u64,
    total_blocks: u64,
    peak_bytes: usize,
    live_bytes: usize,
}
impl From<dhat::HeapStats> for Heap {
    fn from(value: dhat::HeapStats) -> Self {
        Self {
            total_bytes: value.total_bytes,
            total_blocks: value.total_blocks,
            peak_bytes: value.max_bytes,
            live_bytes: value.curr_bytes,
        }
    }
}
#[derive(serde::Serialize)]
struct Sample {
    matching: Heap,
    issued: Heap,
    token_bytes: usize,
    released_bytes: usize,
}
fn run(negative: bool) -> Result<Sample, Box<dyn std::error::Error>> {
    let scope = TokenScope::new(&"11".repeat(16), [0x22; 16])?;
    let foreign = TokenScope::new(&"12".repeat(16), [0x22; 16])?;
    let (token, digest) = issue(TokenKind::Access, &scope, [0x33; 16])?;
    let encoded = digest.encode();
    let mut wrong = token.expose().as_bytes().to_vec();
    wrong[101] = if wrong[101] == b'0' { b'1' } else { b'0' };
    let wrong = String::from_utf8(wrong)?;
    let oversized = "a".repeat(1024 * 1024);
    let profiler = dhat::Profiler::builder().testing().build();
    let control = negative.then(|| vec![7_u8; 144]);
    for _ in 0..1000 {
        if !digest.matches(token.expose(), &scope)?
            || digest.matches(&wrong, &scope)?
            || digest.matches(token.expose(), &foreign)?
            || digest.matches(&oversized, &scope).is_ok()
            || metadata(token.expose()).is_err()
            || TokenDigest::decode(&encoded).is_err()
            || TokenDigest::decode(&[]).is_ok()
        {
            return Err("synthetic token verification failed".into());
        }
    }
    std::hint::black_box(&control);
    let matching = dhat::HeapStats::get().into();
    drop(control);
    drop(profiler);
    let profiler = dhat::Profiler::builder().testing().build();
    let (fresh, _) = issue(TokenKind::Refresh, &scope, [0x44; 16])?;
    let issued = dhat::HeapStats::get().into();
    let token_bytes = fresh.expose().len();
    drop(fresh);
    let released_bytes = dhat::HeapStats::get().curr_bytes;
    drop(profiler);
    Ok(Sample {
        matching,
        issued,
        token_bytes,
        released_bytes,
    })
}
fn main() -> ExitCode {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() > 1 || args.first().is_some_and(|arg| arg != "--negative-control") {
        eprintln!("invalid token allocation arguments");
        return ExitCode::FAILURE;
    }
    let sample = match run(!args.is_empty()) {
        Ok(sample) => sample,
        Err(_) => {
            eprintln!("token allocation check failed");
            return ExitCode::FAILURE;
        }
    };
    let ok = sample.matching.total_bytes == 0
        && sample.matching.total_blocks == 0
        && sample.matching.peak_bytes == 0
        && sample.matching.live_bytes == 0
        && sample.issued.total_bytes == 102
        && sample.issued.total_blocks == 1
        && sample.issued.peak_bytes == 102
        && sample.issued.live_bytes == 102
        && sample.token_bytes == 102
        && sample.released_bytes == 0;
    // JSON, stdout and test-runner allocations are outside both samples.
    if serde_json::to_writer(std::io::stdout().lock(), &sample).is_err() {
        return ExitCode::FAILURE;
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
