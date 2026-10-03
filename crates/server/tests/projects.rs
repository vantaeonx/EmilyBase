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
