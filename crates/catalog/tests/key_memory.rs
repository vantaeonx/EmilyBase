#![cfg(target_os = "linux")]

use emilybase_catalog::{Column, DataType, Error, Key, Schema};

#[test]
fn oversized_key_rejection_fits_bounded_headroom_without_copying_the_input() {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "bounded_key_child", "--ignored", "--nocapture"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "child exited {:?}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "isolated process entry for the parent memory-bound regression"]
fn bounded_key_child() {
    let schema = Schema {
        name: "t".into(),
        columns: vec![Column {
            name: "id".into(),
            data_type: DataType::Text,
            nullable: false,
        }],
        primary_key: 0,
    };
    let key = Key::Text("x".repeat(128 * 1024 * 1024));
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    let kib = status
        .lines()
        .find_map(|line| line.strip_prefix("VmSize:"))
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .parse::<u64>()
        .unwrap();
    let limit = kib * 1024 + 8 * 1024 * 1024;
    let data_kib = status
        .lines()
        .find_map(|line| line.strip_prefix("VmData:"))
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .parse::<u64>()
        .unwrap();
    let data_limit = data_kib * 1024 + 8 * 1024 * 1024;
    rustix::process::setrlimit(
        rustix::process::Resource::Core,
        rustix::process::Rlimit {
            current: Some(0),
            maximum: Some(0),
        },
    )
    .unwrap();
    rustix::process::setrlimit(
        rustix::process::Resource::As,
        rustix::process::Rlimit {
            current: Some(limit),
            maximum: Some(limit),
        },
    )
    .unwrap();
    rustix::process::setrlimit(
        rustix::process::Resource::Data,
        rustix::process::Rlimit {
            current: Some(data_limit),
            maximum: Some(data_limit),
        },
    )
    .unwrap();
    assert!(matches!(schema.validate_key(&key), Err(Error::ValueSize)));
    assert!(schema.validate_key(&Key::Text("界".repeat(1024))).is_ok());
    assert!(matches!(
        schema.validate_key(&Key::Integer(0)),
        Err(Error::PrimaryKey)
    ));
}
