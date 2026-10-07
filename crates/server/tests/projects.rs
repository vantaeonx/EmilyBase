use emilybase_catalog::Value;
use emilybase_server::{Error, MAX_PROJECTS, ProjectStore};
use proptest::prelude::*;
use std::os::unix::fs::{PermissionsExt, symlink};

#[test]
fn project_keys_scope_identical_sql_names_and_rotation_survives_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("projects");
    let mut store = ProjectStore::open(&root).unwrap();
    let a = store.create("alpha").unwrap();
    let b = store.create("beta").unwrap();
    for (created, value) in [(&a, 1), (&b, 2)] {
        store
            .authorize(&created.project.id, &created.api_key)
            .unwrap()
            .execute(
                "CREATE TABLE t(id INT PRIMARY KEY,value INT);INSERT INTO t VALUES(1,$1)",
                &[Value::Integer(value)],
            )
            .unwrap();
    }
    assert!(matches!(
        store.authorize(&a.project.id, &b.api_key),
        Err(Error::Denied)
    ));
    assert!(matches!(
        store.authorize(&b.project.id, &a.api_key),
        Err(Error::Denied)
    ));
    let result = store
        .authorize(&a.project.id, &a.api_key)
        .unwrap()
        .execute("SELECT * FROM t", &[])
        .unwrap();
    assert_eq!(
        result.results[0].rows,
        [vec![Value::Integer(1), Value::Integer(1)]]
    );
    let rotated = store.rotate(&a.project.id).unwrap();
    assert_eq!(rotated.project.key_epoch, 2);
    assert!(store.authorize(&a.project.id, &a.api_key).is_err());
    let metadata = std::fs::read_to_string(root.join(&a.project.id).join("project.json")).unwrap();
    assert!(!metadata.contains(&a.api_key) && !metadata.contains(&rotated.api_key));
    let list = serde_json::to_string(&store.list().unwrap()).unwrap();
    assert!(!list.contains(&rotated.api_key));
    drop(store);
    let store = ProjectStore::open(&root).unwrap();
    assert!(store.authorize(&a.project.id, &a.api_key).is_err());
    assert!(store.authorize(&a.project.id, &rotated.api_key).is_ok());
    let result = store
        .authorize(&b.project.id, &b.api_key)
        .unwrap()
        .execute("SELECT * FROM t", &[])
        .unwrap();
    assert_eq!(
        result.results[0].rows,
        [vec![Value::Integer(1), Value::Integer(2)]]
    );
}

#[test]
fn outstanding_request_capability_keeps_registry_ownership_until_consumed_or_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("projects");
    let mut store = ProjectStore::open(&root).unwrap();
    assert!(matches!(ProjectStore::open(&root), Err(Error::Busy)));
    let project = store.create("owner").unwrap();
    let request = store
        .authorize(&project.project.id, &project.api_key)
        .unwrap();
    drop(store);
    assert!(matches!(ProjectStore::open(&root), Err(Error::Busy)));
    drop(request);
    assert!(ProjectStore::open(&root).is_ok());
}

#[test]
fn moved_registry_namespace_cannot_redirect_create_or_rotation_to_a_new_root() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("source");
    let mut store = ProjectStore::open(&root).unwrap();
    let project = store.create("synthetic pinned owner").unwrap();
    let moved = temp.path().join("moved");
    std::fs::rename(&root, &moved).unwrap();
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&root)
        .unwrap();
    assert!(matches!(store.create("must refuse"), Err(Error::Path)));
    assert!(matches!(
        store.rotate(&project.project.id),
        Err(Error::Path)
    ));
    assert!(matches!(store.list(), Err(Error::Path)));
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
    assert_eq!(std::fs::read_dir(&moved).unwrap().count(), 1);
}

#[test]
fn moved_project_or_data_namespace_cannot_redirect_an_accepted_capability() {
    for level in ["root", "project", "data"] {
        for action in ["status", "explain", "execute"] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("source");
            let mut store = ProjectStore::open(&root).unwrap();
            let project = store.create("synthetic pinned capability").unwrap();
            store
                .authorize(&project.project.id, &project.api_key)
                .unwrap()
                .execute(
                    "CREATE TABLE t(id INT PRIMARY KEY); INSERT INTO t VALUES(1)",
                    &[],
                )
                .unwrap();
            let archive = temp.path().join("private.backup");
            store.backup(&archive).unwrap();
            let clone = temp.path().join("clone");
            emilybase_server::restore_registry_backup(&archive, &clone).unwrap();
            let original = root.join(&project.project.id);
            let copied = clone.join(&project.project.id);
            let replacement = if level == "root" {
                clone
            } else if level == "data" {
                copied.join("data")
            } else {
                copied
            };
            let selected = if level == "root" {
                root
            } else if level == "data" {
                original.join("data")
            } else {
                original
            };
            let saved = temp.path().join("saved");
            let request = store
                .authorize(&project.project.id, &project.api_key)
                .unwrap();
            std::fs::rename(&selected, &saved).unwrap();
            std::fs::rename(&replacement, &selected).unwrap();
            let selected_wal = if level == "root" {
                selected.join(&project.project.id).join("data/redo.wal")
            } else if level == "data" {
                selected.join("redo.wal")
            } else {
                selected.join("data/redo.wal")
            };
            let saved_wal = if level == "root" {
                saved.join(&project.project.id).join("data/redo.wal")
            } else if level == "data" {
                saved.join("redo.wal")
            } else {
                saved.join("data/redo.wal")
            };
            let before = std::fs::read(&selected_wal).unwrap();
            let saved_before = std::fs::read(&saved_wal).unwrap();
            let rejected = match action {
                "status" => matches!(request.status(), Err(Error::Path)),
                "explain" => matches!(request.explain("SELECT * FROM t", &[]), Err(Error::Path)),
                _ => matches!(
                    request.execute("INSERT INTO t VALUES(2)", &[]),
                    Err(Error::Path)
                ),
            };
            assert!(
                rejected,
                "accepted capability followed a replacement directory"
            );
            assert_eq!(std::fs::read(selected_wal).unwrap(), before);
            assert_eq!(std::fs::read(saved_wal).unwrap(), saved_before);
        }
    }
}

#[test]
fn path_traversal_labels_never_become_paths_and_symlinks_or_broad_permissions_fail() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("projects");
    let mut store = ProjectStore::open(&root).unwrap();
    let project = store.create("../../outside").unwrap();
    assert!(!dir.path().join("outside").exists());
    for id in [
        "../outside",
        "/tmp/outside",
        &format!("{}/../other", project.project.id),
        "%2e%2e",
        ".",
    ] {
        assert!(matches!(
            store.authorize(id, &project.api_key),
            Err(Error::Denied)
        ));
    }
    for name in ["", "  ", "control\nname", &"x".repeat(129)] {
        assert!(matches!(store.create(name), Err(Error::Name)));
    }
    let data = root.join(&project.project.id).join("data");
    let original = root.join(&project.project.id).join("saved");
    std::fs::rename(&data, &original).unwrap();
    symlink(&original, &data).unwrap();
    let result = store
        .authorize(&project.project.id, &project.api_key)
        .unwrap()
        .execute("CREATE TABLE t(id INT PRIMARY KEY)", &[]);
    assert!(matches!(result, Err(Error::Path)));
    drop(store);
    assert!(matches!(ProjectStore::open(&root), Err(Error::Path)));
    std::fs::remove_file(&data).unwrap();
    std::fs::rename(&original, &data).unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(ProjectStore::open(&root), Err(Error::Path)));
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let alias = dir.path().join("alias");
    symlink(&root, &alias).unwrap();
    assert!(matches!(ProjectStore::open(&alias), Err(Error::Path)));
}

#[test]
fn project_limit_is_reached_without_creating_an_additional_database() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("projects");
    let mut store = ProjectStore::open(&root).unwrap();
    for i in 0..MAX_PROJECTS {
        store.create(&format!("synthetic-{i}")).unwrap();
    }
    let paths = std::fs::read_dir(&root).unwrap().count();
    assert!(matches!(store.create("over"), Err(Error::Limit)));
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), paths);
    assert_eq!(store.list().unwrap().len(), MAX_PROJECTS);
    drop(store);
    assert_eq!(
        ProjectStore::open(&root).unwrap().list().unwrap().len(),
        MAX_PROJECTS
    );
}

#[test]
fn same_project_requests_serialize_while_distinct_project_files_remain_separate() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = ProjectStore::open(dir.path().join("projects")).unwrap();
    let created = store.create("concurrent").unwrap();
    store
        .authorize(&created.project.id, &created.api_key)
        .unwrap()
        .execute("CREATE TABLE t(id INT PRIMARY KEY)", &[])
        .unwrap();
    let mut requests = Vec::new();
    for i in 0..32 {
        requests.push((
            store
                .authorize(&created.project.id, &created.api_key)
                .unwrap(),
            i,
        ));
    }
    let threads = requests
        .into_iter()
        .map(|(request, i)| {
            std::thread::spawn(move || {
                request
                    .execute("INSERT INTO t VALUES($1)", &[Value::Integer(i)])
                    .unwrap();
            })
        })
        .collect::<Vec<_>>();
    for thread in threads {
        thread.join().unwrap();
    }
    let report = store
        .authorize(&created.project.id, &created.api_key)
        .unwrap()
        .execute("SELECT id FROM t ORDER BY id", &[])
        .unwrap();
    assert_eq!(
        report.results[0].rows,
        (0..32).map(|i| vec![Value::Integer(i)]).collect::<Vec<_>>()
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    #[test]
    fn generated_rotation_histories_keep_only_current_key_and_data(
        rotations in prop::collection::vec(any::<bool>(),0..12)
    ) {
        let dir=tempfile::tempdir().unwrap();let root=dir.path().join("projects");let mut store=ProjectStore::open(&root).unwrap();
        let mut issued=store.create("rotating").unwrap();let id=issued.project.id.clone();let mut old=Vec::new();
        store.authorize(&id,&issued.api_key).unwrap().execute("CREATE TABLE t(id INT PRIMARY KEY);INSERT INTO t VALUES(1)",&[]).unwrap();
        for reopen in rotations {
            old.push(issued.api_key);issued=store.rotate(&id).unwrap();
            if reopen {drop(store);store=ProjectStore::open(&root).unwrap();}
            for token in &old {prop_assert!(store.authorize(&id,token).is_err());}
            let result=store.authorize(&id,&issued.api_key).unwrap().execute("SELECT * FROM t",&[]).unwrap();
            prop_assert_eq!(&result.results[0].rows,&vec![vec![Value::Integer(1)]]);
        }
    }
}

#[test]
fn status_counts_only_current_tables_after_recreation_and_atomic_failure_in_separate_projects() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("projects");
    let mut store = ProjectStore::open(&root).unwrap();
    let a = store.create("inventory alpha").unwrap();
    let b = store.create("inventory beta").unwrap();
    store.authorize(&a.project.id,&a.api_key).unwrap().execute(
        "CREATE TABLE z(id INT PRIMARY KEY); CREATE TABLE a(id INT PRIMARY KEY); CREATE TABLE b(id INT PRIMARY KEY); INSERT INTO a VALUES(7)",&[]).unwrap();
    store
        .authorize(&b.project.id, &b.api_key)
        .unwrap()
        .execute(
            "CREATE TABLE a(id INT PRIMARY KEY); INSERT INTO a VALUES(8)",
            &[],
        )
        .unwrap();
    let previous = store
        .authorize(&a.project.id, &a.api_key)
        .unwrap()
        .status()
        .unwrap();
    assert_eq!((previous.tables, previous.rows), (3, 1));
    assert!(
        store
            .authorize(&a.project.id, &a.api_key)
            .unwrap()
            .execute("DROP TABLE b; CREATE TABLE a(id INT PRIMARY KEY)", &[])
            .is_err()
    );
    let unchanged = store
        .authorize(&a.project.id, &a.api_key)
        .unwrap()
        .status()
        .unwrap();
    assert_eq!(
        (unchanged.transaction, unchanged.tables, unchanged.rows),
        (previous.transaction, 3, 1)
    );
    store
        .authorize(&a.project.id, &a.api_key)
        .unwrap()
        .execute(
            "DROP TABLE a; DROP TABLE b; CREATE TABLE a(id INT PRIMARY KEY)",
            &[],
        )
        .unwrap();
    drop(store);
    let store = ProjectStore::open(&root).unwrap();
    let current = store
        .authorize(&a.project.id, &a.api_key)
        .unwrap()
        .status()
        .unwrap();
    assert_eq!((current.tables, current.rows), (2, 0));
    let other = store
        .authorize(&b.project.id, &b.api_key)
        .unwrap()
        .status()
        .unwrap();
    assert_eq!((other.tables, other.rows), (1, 1));
    assert!(store.authorize(&a.project.id, &b.api_key).is_err());
}
