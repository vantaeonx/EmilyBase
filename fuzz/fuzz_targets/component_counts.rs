#![no_main]
#![forbid(unsafe_code)]

use emilybase_commit_model::{EncodedComponents, Error, PlanCounts};
use libfuzzer_sys::fuzz_target;

fn check(history: u64, roots: u64, pages: u64) {
    let h = u128::from(history);
    let r = u128::from(roots);
    let p = u128::from(pages);
    let valid = (1..=65536).contains(&h) && r <= 128 && p >= r && p <= r * 1024 && p <= 2048;
    match EncodedComponents::from_counts(history, roots, pages) {
        Ok(value) => {
            assert!(valid);
            assert_eq!(value.history_pages(), history);
            assert_eq!(value.roots(), roots);
            assert_eq!(value.index_pages(), pages);
            assert_eq!(u128::from(value.history_bytes()), h * 4096);
            assert_eq!(u128::from(value.index_bytes()), (p + r) * 4096);
            assert_eq!(u128::from(value.root_bytes()), r * 192);
            assert_eq!(
                u128::from(value.total_bytes()),
                h * 4096 + (p + r) * 4096 + r * 192
            );
        }
        Err(Error::Limit) => assert!(!valid),
        Err(other) => panic!("unexpected bounded-count refusal: {other}"),
    }
}

fn check_plan(fields: [usize; 5]) {
    let [history, primary, retired, roots, tables] = fields;
    let valid = history <= 256
        && primary <= 2048
        && retired <= 2048
        && roots <= 128
        && tables <= 128
        && (roots != 0 || (primary == 0 && retired == 0));
    match PlanCounts::from_counts(history, primary, retired, roots, tables) {
        Ok(counts) => {
            assert!(valid);
            assert_eq!(counts.history_pages(), history);
            assert_eq!(counts.primary_pages(), primary);
            assert_eq!(counts.retired_pages(), retired);
            assert_eq!(counts.changed_roots(), roots);
            assert_eq!(counts.retired_tables(), tables);
            assert_eq!(
                u128::from(counts.image_body_bytes()),
                (history as u128 + primary as u128) * 4096
            );
        }
        Err(Error::Limit) => assert!(!valid),
        Err(error) => panic!("unexpected plan-count refusal: {error}"),
    }
}

fuzz_target!(|bytes: &[u8]| {
    if bytes.len() < 24 || bytes.len() > 64 {
        return;
    }
    if bytes.len() >= 40 {
        let fields: [[u8; 8]; 5] = bytes[..40].as_chunks::<8>().0.try_into().unwrap();
        let raw =
            fields.map(|field| usize::try_from(u64::from_le_bytes(field)).unwrap_or(usize::MAX));
        check_plan(raw);
        check_plan([
            raw[0] % 257,
            raw[1] % 2049,
            raw[2] % 2049,
            raw[3] % 129,
            raw[4] % 129,
        ]);
    }
    let fields = bytes[..24].as_chunks::<8>().0;
    let history = u64::from_le_bytes(fields[0]);
    let roots = u64::from_le_bytes(fields[1]);
    let pages = u64::from_le_bytes(fields[2]);
    check(history, roots, pages);
    // Raw extremes and repaired bounded shapes are both compared to u128 math.
    check(history % 65536 + 1, roots % 129, pages % 2049);
    check(
        65536,
        128,
        2048 + u64::from(bytes.get(24).copied().unwrap_or(0)),
    );
});
