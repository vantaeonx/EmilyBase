#![no_main]
use emilybase_auth::tokens::{TOKEN_DIGEST_BYTES, TokenDigest, TokenScope, metadata};
use libfuzzer_sys::fuzz_target;

fn canonical(bytes: &[u8]) -> bool {
    bytes.len() == 102
        && matches!(&bytes[..5], b"eba1_" | b"ebr1_")
        && bytes[37] == b'.'
        && bytes[5..37]
            .iter()
            .chain(&bytes[38..])
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
}
fn record_model(bytes: &[u8]) -> bool {
    bytes.len() == 92
        && bytes[..8] == *b"EBSK\0\0\0\0"
        && bytes[8..10] == [1, 0]
        && [1, 2].contains(&bytes[10])
        && bytes[11] == 0
}

fuzz_target!(|data: &[u8]| {
    // Bounded, pure parsing/matching. No issuance, randomness, KDF or storage.
    let data = &data[..data.len().min(512)];
    let decoded = TokenDigest::decode(data);
    assert_eq!(decoded.is_ok(), record_model(data));
    if let Ok(digest) = decoded {
        assert_eq!(digest.encode().as_slice(), data);
    }
    if let Ok(text) = std::str::from_utf8(data) {
        assert_eq!(metadata(text).is_ok(), canonical(data));
    }
    let scope = TokenScope::new("11111111111111111111111111111111", [0x22; 16]).unwrap();
    let foreign = TokenScope::new("11111111111111111111111111111111", [0x23; 16]).unwrap();
    let mut record = [0; TOKEN_DIGEST_BYTES];
    record[..8].copy_from_slice(b"EBSK\0\0\0\0");
    record[8..12].copy_from_slice(&[1, 0, 1, 0]);
    record[12..28].fill(0x11);
    record[28..44].fill(0x22);
    record[44..60].fill(0x33);
    record[60..].copy_from_slice(&[
        0xb2, 0xb3, 0x16, 0x49, 0xe1, 0x2c, 0x46, 0xa0, 0xe4, 0xc7, 0xe4, 0x48, 0x7d, 0xd9, 0x6f,
        0x0e, 0x81, 0x7e, 0x40, 0x88, 0x68, 0x94, 0x1d, 0x4c, 0xa0, 0xf8, 0x59, 0x39, 0xa4, 0x73,
        0x7c, 0x14,
    ]);
    let digest = TokenDigest::decode(&record).unwrap();
    let mut text = [b'4'; 102];
    text[..5].copy_from_slice(b"eba1_");
    text[5..37].fill(b'3');
    text[37] = b'.';
    let original = text;
    for (i, byte) in data.iter().take(102).enumerate() {
        text[i] = *byte;
    }
    if let Ok(candidate) = std::str::from_utf8(&text) {
        let actual = digest.matches(candidate, &scope);
        assert_eq!(actual.is_ok(), canonical(&text));
        assert_eq!(actual == Ok(true), text == original);
        assert_ne!(digest.matches(candidate, &foreign), Ok(true));
    }
    let mut changed = record;
    for (i, byte) in data.iter().take(92).enumerate() {
        changed[i] = *byte;
    }
    let decoded = TokenDigest::decode(&changed);
    assert_eq!(decoded.is_ok(), record_model(&changed));
    if let Ok(decoded) = decoded {
        assert_eq!(decoded.encode(), changed);
        let original_text = std::str::from_utf8(&original).unwrap();
        assert_eq!(
            decoded.matches(original_text, &scope).unwrap(),
            changed == record
        );
    }
});
