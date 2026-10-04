use emilybase_index::{BPlusTree, IndexStore, Key, RecordPointer};
use std::path::Path;
use std::process::{Command, Output};

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_emilybase"))
        .arg("index-range")
        .arg(root)
        .args(args)
        .output()
        .unwrap()
}
fn keys(output: Output) -> Vec<serde_json::Value> {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn actual_cli_reads_both_directions_and_preserves_the_owned_snapshot() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("synthetic ranges");
    let entries = (0..400)
        .map(|i| {
            (
                Key::Integer(i),
                RecordPointer {
                    page_id: i as u64 + 100,
                    slot_id: i as u16,
                },
            )
        })
        .chain([
            (
                Key::Text(String::new()),
                RecordPointer {
                    page_id: 700,
                    slot_id: 0,
                },
            ),
            (
                Key::Text("界".into()),
                RecordPointer {
                    page_id: 701,
                    slot_id: 1,
                },
            ),
            (
                Key::Text("😀".into()),
                RecordPointer {
                    page_id: 702,
                    slot_id: 2,
                },
            ),
        ])
        .collect::<Vec<_>>();
    drop(IndexStore::create(&root, &BPlusTree::from_sorted_stable(&entries).unwrap()).unwrap());
    let before = std::fs::read(root.join("tree.ebif")).unwrap();
    let lower = r#"{"type":"integer","value":198}"#;
    let upper = r#"{"type":"integer","value":215}"#;
    for descending in [false, true] {
        let mut args = vec!["--lower", lower, "--upper", upper, "--limit", "4"];
        if descending {
            args.push("--descending");
        }
        let numbers = if descending {
            vec![214, 213, 212, 211]
        } else {
            vec![198, 199, 200, 201]
        };
        let expected = numbers
            .into_iter()
            .map(|i| serde_json::json!({"key":{"type":"integer","value":i},"page":i+100,"slot":i}))
            .collect::<Vec<_>>();
        assert_eq!(keys(run(&root, &args)), expected);
    }
    let last = keys(run(&root, &["--descending", "--limit", "2"]));
    assert_eq!(
        last[0]["key"],
        serde_json::json!({"type":"text","value":"😀"})
    );
    assert_eq!(
        last[1]["key"],
        serde_json::json!({"type":"text","value":"界"})
    );
    assert_eq!(keys(run(&root, &[])).len(), 100);
    assert!(keys(run(&root, &["--limit", "0"])).is_empty());
    assert!(keys(run(&root, &["--lower", upper, "--upper", lower])).is_empty());
    assert_eq!(std::fs::read(root.join("tree.ebif")).unwrap(), before);
    assert_eq!(
        IndexStore::open(&root)
            .unwrap()
            .snapshot()
            .unwrap()
            .revision,
        1
    );
}

#[test]
fn actual_cli_rejects_bad_bounds_limits_and_private_paths_without_partial_results() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("empty");
    drop(IndexStore::create(&root, &BPlusTree::new_stable()).unwrap());
    let long = serde_json::to_string(&Key::Text("secret-bound".repeat(30))).unwrap();
    let malformed = r#"{"type":"text","value":"secret-bound","extra":1}"#;
    for args in [
        vec!["--lower", &long, "--limit", "0"],
        vec!["--upper", malformed],
        vec!["--limit", "10001"],
    ] {
        let output = run(&root, &args);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("secret-bound"));
    }
    let link = temp.path().join("linked");
    std::os::unix::fs::symlink(&root, &link).unwrap();
    assert!(!run(&link, &[]).status.success());
    assert!(keys(run(&root, &["--descending"])).is_empty());
}
