#![no_main]
use emilybase_auth::accounts::inspect_session_clock_record;
use emilybase_catalog::Value;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Pure three-cell metadata inspection; no clock update, I/O or credential.
    let data = &data[..data.len().min(512)];
    let mut integers = [1, 1, 0_i64];
    for (i, value) in integers.iter_mut().enumerate() {
        let mut bytes = [0; 8];
        for (j, b) in data.iter().skip(1 + i * 8).take(8).enumerate() {
            bytes[j] = *b;
        }
        *value = i64::from_le_bytes(bytes);
    }
    if data.first().is_some_and(|tag| tag & 1 == 0) {
        integers[0] = 1;
        integers[1] = 1;
    }
    let mut row = integers.map(Value::Integer).to_vec();
    if let Some(tag) = data.first() {
        let index = usize::from(*tag) % 3;
        match tag % 6 {
            0 => {}
            1 => row[index] = Value::Null,
            2 => row[index] = Value::Boolean(true),
            3 => row[index] = Value::Bytes(data.iter().take(128).copied().collect()),
            4 => row.truncate(index),
            _ => row.push(Value::Integer(0)),
        }
    }
    let expected = match row.as_slice() {
        [Value::Integer(1), Value::Integer(1), Value::Integer(time)] if *time >= 0 => {
            Some(*time as u64)
        }
        _ => None,
    };
    assert_eq!(inspect_session_clock_record(&row).ok(), expected);
});
