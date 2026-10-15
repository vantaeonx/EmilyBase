use emilybase_server::{
    AccountFileRootManifest, FileRootBinding, MAX_ACCOUNT_FILE_ROOT_MANIFEST_BYTES,
};
use serde_json::Value;
use std::fs::{self, File};
use std::io::Write;
use std::process::{Command, Output, Stdio};

fn manifest(count: usize) -> Vec<u8> {
    AccountFileRootManifest::new(
        (0..count).map(|n| format!("{n:032x}")).collect(),
        (0..count)
            .map(|n| {
                FileRootBinding::new(&format!("{n:032x}"), &format!("{:032x}", 1000 + n)).unwrap()
            })
            .collect(),
        100,
    )
    .unwrap()
    .to_bytes()
    .unwrap()
}
fn run(bytes: &[u8], output: Option<File>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_emilybase"));
    command
        .arg("account-file-roster-check")
        .stdin(Stdio::piped())
        .stderr(Stdio::piped());
    command.stdout(output.map(Stdio::from).unwrap_or_else(Stdio::piped));
    let mut child = command.spawn().unwrap();
    child.stdin.take().unwrap().write_all(bytes).unwrap();
    child.wait_with_output().unwrap()
}
fn refused(output: Output) {
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    for private in [
        "private-payload",
        "panicked",
        "000000000000000000000000000003e8",
    ] {
        assert!(!stderr.contains(private));
    }
}

#[test]
fn actual_cli_counts_empty_full_rosters_without_creating_a_root_or_printing_bindings() {
    for count in [0, 1, 128] {
        let bytes = manifest(count);
        let output = run(&bytes, None);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value.as_object().unwrap().len(), 5);
        assert_eq!(value["format"], 2);
        assert_eq!(value["private_projects"], count);
        assert_eq!(value["file_stores"], count);
        assert_eq!(value["reset_at"], "100");
        assert_eq!(value["manifest_bytes"], bytes.len());
        assert!(
            !String::from_utf8_lossy(&output.stdout).contains("000000000000000000000000000003e8")
        );
    }
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("manifest");
    fs::write(&input, manifest(1)).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_emilybase"))
        .arg("account-file-roster-check")
        .current_dir(temp.path())
        .stdin(Stdio::from(File::open(&input).unwrap()))
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
    assert_eq!(fs::read(input).unwrap(), manifest(1));
}

#[test]
fn actual_cli_refuses_oversize_corrupt_noncanonical_legacy_and_malformed_without_output() {
    let valid = manifest(1);
    let mut corrupt = valid.clone();
    *corrupt.last_mut().unwrap() ^= 1;
    let mut trailing = valid;
    trailing.push(b'\n');
    for bytes in [
        vec![],
        b"private-payload".to_vec(),
        corrupt,
        trailing,
        vec![b' '; MAX_ACCOUNT_FILE_ROOT_MANIFEST_BYTES + 1],
        b"{\"payload\":{\"version\":1,\"private_projects\":[],\"reset_at\":0},\"checksum\":0}"
            .to_vec(),
    ] {
        refused(run(&bytes, None));
    }
}

#[test]
fn failed_count_receipt_delivery_is_process_failure_and_does_not_touch_manifest() {
    let bytes = manifest(1);
    let expected = bytes.clone();
    refused(run(
        &bytes,
        Some(File::options().write(true).open("/dev/full").unwrap()),
    ));
    assert_eq!(bytes, expected);
}
