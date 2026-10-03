use std::path::Path;
use std::process::{Command, Output};

fn command(action: &str, root: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_emilybase"))
        .arg(action)
        .arg(root)
        .args(arguments)
        .output()
        .unwrap()
}

#[test]
fn actual_index_cli_creates_publishes_reads_deletes_and_preserves_rejected_writes() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("standalone synthetic index");
    assert!(command("index-create", &root, &[]).status.success());
    let key = r#"{"type":"integer","value":7}"#;
    assert!(
        command("index-insert", &root, &[key, "10", "3"])
            .status
            .success()
    );
    let response = command("index-get", &root, &[key]);
    assert!(response.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&response.stdout).unwrap(),
        serde_json::json!({"page":10,"slot":3})
    );
    let before = std::fs::read(root.join("tree.ebif")).unwrap();
    for action in ["index-create", "index-insert"] {
        let args = if action == "index-insert" {
            vec![key, "20", "4"]
        } else {
            vec![]
        };
        assert!(!command(action, &root, &args).status.success());
        assert_eq!(std::fs::read(root.join("tree.ebif")).unwrap(), before);
    }
    let invalid = r#"{"type":"text","value":"synthetic-error-redaction","extra":1}"#;
    let rejected = command("index-insert", &root, &[invalid, "10", "3"]);
    assert!(!rejected.status.success());
    assert!(!String::from_utf8_lossy(&rejected.stderr).contains("synthetic-error-redaction"));
    assert!(
        !command(
            "index-insert",
            &root,
            &[r#"{"type":"integer","value":8}"#, "0", "3"]
        )
        .status
        .success()
    );
    assert_eq!(std::fs::read(root.join("tree.ebif")).unwrap(), before);
    assert!(command("index-delete", &root, &[key]).status.success());
    let absent = command("index-get", &root, &[key]);
    assert!(absent.status.success());
    assert_eq!(absent.stdout, b"null\n");
    let verified = command("index-verify", &root, &[]);
    assert!(verified.status.success());
    assert_eq!(
        verified.stdout,
        b"verified index revision=3 pages=1 entries=0\n"
    );
    let missing = command("index-delete", &root, &[key]);
    assert!(!missing.status.success());
    let store = emilybase_index::IndexStore::open(&root).unwrap();
    assert_eq!(store.snapshot().unwrap().revision, 3);
}
