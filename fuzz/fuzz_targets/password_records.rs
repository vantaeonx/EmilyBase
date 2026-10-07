#![no_main]
use emilybase_auth::password::{PASSWORD_RECORD_BYTES, PasswordDigest};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Parser only: attacker-controlled fuzz input never triggers the costly KDF.
    let accepted = data.len() == PASSWORD_RECORD_BYTES
        && data[..8] == *b"EBPWD\0\0\0"
        && data[8..12] == [1, 0, 2, 0x13]
        && data[12..16] == 19456_u32.to_le_bytes()
        && data[16..20] == 2_u32.to_le_bytes()
        && data[20..24] == 1_u32.to_le_bytes();
    let decoded = PasswordDigest::decode(data);
    assert_eq!(decoded.is_ok(), accepted);
    if let Ok(decoded) = decoded {
        assert_eq!(decoded.encode(), data);
    }
    // Always explore valid records and mutations as well as arbitrary inputs.
    let mut canonical = [0; PASSWORD_RECORD_BYTES];
    canonical[..8].copy_from_slice(b"EBPWD\0\0\0");
    canonical[8..12].copy_from_slice(&[1, 0, 2, 0x13]);
    canonical[12..16].copy_from_slice(&19456_u32.to_le_bytes());
    canonical[16..20].copy_from_slice(&2_u32.to_le_bytes());
    canonical[20..24].copy_from_slice(&1_u32.to_le_bytes());
    for (position, byte) in data.iter().take(48).enumerate() {
        canonical[24 + position] = *byte;
    }
    assert_eq!(
        PasswordDigest::decode(&canonical).unwrap().encode(),
        canonical
    );
    if let Some(byte) = data.first() {
        let position = usize::from(*byte) % 24;
        canonical[position] ^= 1 << (data.get(1).copied().unwrap_or(0) % 8);
        assert!(PasswordDigest::decode(&canonical).is_err());
    }
});
