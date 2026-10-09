use super::*;
use std::cell::Cell;

const DENY: &[u8] = br#"{"version":1,"select":{"kind":"deny"},"insert":{"kind":"deny"},"update_using":{"kind":"deny"},"update_check":{"kind":"deny"},"delete":{"kind":"deny"}}"#;

#[test]
fn revision_digits_preserve_the_entire_u64_domain_without_normalization() {
    for value in [0, 1, 9_007_199_254_740_993, u64::MAX] {
        assert_eq!(expected(&value.to_string()).unwrap(), value);
    }
    for text in [
        "",
        "+0",
        "-1",
        "00",
        "01",
        " 1",
        "1 ",
        "1\n",
        "1e0",
        "18446744073709551616",
    ] {
        assert!(expected(text).is_err());
    }
    let receipt = Receipt::from(PolicyReceipt {
        table: u64::MAX,
        revision: u64::MAX,
        previous: 9_007_199_254_740_993,
        sha256: [255; 32],
    });
    let json = serde_json::to_value(receipt).unwrap();
    assert_eq!(json["table"], u64::MAX.to_string());
    assert_eq!(json["revision"], u64::MAX.to_string());
    assert_eq!(json["previous"], "9007199254740993");
    assert_eq!(json["sha256"], "ff".repeat(32));
}

#[test]
fn policy_stream_keeps_exact_bytes_and_reads_at_most_the_limit_plus_one() {
    struct Counted<'a> {
        read: &'a Cell<usize>,
    }
    impl Read for Counted<'_> {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            buffer.fill(b' ');
            self.read.set(self.read.get() + buffer.len());
            Ok(buffer.len())
        }
    }
    let mut exact = DENY.to_vec();
    exact.resize(row_policy::MAX_DOCUMENT_BYTES, b' ');
    assert_eq!(document(exact.as_slice()).unwrap().as_slice(), exact);
    exact.push(b' ');
    assert!(document(exact.as_slice()).is_err());
    let count = Cell::new(0);
    assert!(document(Counted { read: &count }).is_err());
    assert_eq!(count.get(), row_policy::MAX_DOCUMENT_BYTES + 1);
    for bytes in [b"".as_slice(), &[255], b"{\"private_value\":1}", b"[]"] {
        assert!(document(bytes).is_err());
    }
}

#[test]
fn read_failure_never_exposes_the_nested_io_message() {
    struct Failed;
    impl Read for Failed {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("synthetic-private-reader-value"))
        }
    }
    assert_eq!(
        document(Failed).unwrap_err().to_string(),
        "policy input unavailable"
    );
}
