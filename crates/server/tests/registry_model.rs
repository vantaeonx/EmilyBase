use emilybase_catalog::Value;
use emilybase_server::{ProjectStore, inspect_registry_backup, restore_registry_backup};
use emilybase_transactions::Database;
use proptest::prelude::*;
use std::collections::BTreeMap;
use std::path::Path;

struct Expected {
    id: String,
    key: String,
    old_keys: Vec<String>,
    epoch: u64,
    transaction: u64,
    wal_version: u16,
    rows: BTreeMap<i64, i64>,
}
fn check(store: &ProjectStore, projects: &[Expected]) {
    let listed = store.list().unwrap();
    assert_eq!(listed.len(), projects.len());
    for project in projects {
        assert_eq!(
            listed
                .iter()
                .find(|p| p.id == project.id)
                .unwrap()
                .key_epoch,
            project.epoch
        );
        for old in &project.old_keys {
            assert!(store.authorize(&project.id, old).is_err());
        }
        for other in projects.iter().filter(|p| p.id != project.id) {
            assert!(store.authorize(&project.id, &other.key).is_err());
        }
        let report = store
            .authorize(&project.id, &project.key)
            .unwrap()
            .execute("SELECT id,v FROM t ORDER BY id", &[])
            .unwrap();
        assert_eq!(report.transaction, project.transaction);
        let expected: Vec<_> = project
            .rows
            .iter()
            .map(|(id, value)| vec![Value::Integer(*id), Value::Integer(*value)])
            .collect();
        assert_eq!(report.results[0].rows, expected);
    }
}
fn fork_and_check(store: &mut ProjectStore, root: &Path, projects: &[Expected], at: usize) {
    let archive = root.join(format!("prefix-{at}.backup"));
    let destination = root.join(format!("prefix-{at}-restored"));
    let image = store.backup_image().unwrap();
    let report = store.backup(&archive).unwrap();
    assert_eq!(inspect_registry_backup(&archive).unwrap(), report);
    for project in projects {
        let archived = report.projects.iter().find(|p| p.id == project.id).unwrap();
        assert_eq!(archived.transaction, project.transaction);
        assert_eq!(archived.key_epoch, project.epoch);
        assert_eq!(archived.wal_version, project.wal_version);
        assert_eq!(archived.rows, project.rows.len());
        assert_eq!(archived.tables, 1);
    }
    assert_eq!(
        restore_registry_backup(&archive, &destination).unwrap(),
        report
    );
    let mut restored = ProjectStore::open_existing(&destination).unwrap();
    assert_eq!(restored.backup_image().unwrap(), image);
    check(&restored, projects);
    let first = &projects[0];
    let rotated = restored.rotate(&first.id).unwrap();
    assert_eq!(rotated.project.key_epoch, first.epoch + 1);
    assert!(restored.authorize(&first.id, &first.key).is_err());
    let changed = restored
        .authorize(&first.id, &rotated.api_key)
        .unwrap()
        .execute("INSERT INTO t VALUES(999,123)", &[])
        .unwrap();
    assert_eq!(changed.transaction, first.transaction + 1);
    check(store, projects);
    assert_eq!(store.backup_image().unwrap(), image);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    #[test]
    fn generated_cross_project_histories_restore_every_checked_prefix(
        operations in proptest::collection::vec((0usize..3,0u8..7,0i64..5,-50i64..51),1..21)
    ) {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let mut store = ProjectStore::open(&source).unwrap();
        let mut projects = Vec::new();
        for _ in 0..3 {
            let created = store.create("synthetic model").unwrap();
            store.authorize(&created.project.id,&created.api_key).unwrap()
                .execute("CREATE TABLE t(id INT PRIMARY KEY,v INT)",&[]).unwrap();
            projects.push(Expected {id:created.project.id,key:created.api_key,old_keys:Vec::new(),
                epoch:1,transaction:2,wal_version:1,rows:BTreeMap::new()});
        }
        let last = operations.len()-1;
        for (at,(index,action,id,value)) in operations.into_iter().enumerate() {
            let project = &mut projects[index];
            match action {
                0 => {
                    let sql = if project.rows.contains_key(&id) {"UPDATE t SET v=$2 WHERE id=$1"}
                        else {"INSERT INTO t VALUES($1,$2)"};
                    store.authorize(&project.id,&project.key).unwrap().execute(sql,
                        &[Value::Integer(id),Value::Integer(value)]).unwrap();
                    project.rows.insert(id,value); project.transaction+=1;
                }
                1 => {
                    let report = store.authorize(&project.id,&project.key).unwrap()
                        .execute("DELETE FROM t WHERE id=$1",&[Value::Integer(id)]).unwrap();
                    if project.rows.remove(&id).is_some() {project.transaction+=1;}
                    assert_eq!(report.transaction,project.transaction);
                }
                2 => {
                    assert!(store.authorize(&project.id,&project.key).unwrap()
                        .execute("INSERT INTO t VALUES(777,1); INSERT INTO t VALUES(777,2)",&[]).is_err());
                }
                3 => {
                    let report = store.authorize(&project.id,&project.key).unwrap()
                        .execute("BEGIN; INSERT INTO t VALUES(778,1); ROLLBACK",&[]).unwrap();
                    assert!(!report.committed);
                }
                4 => {
                    let changed = store.rotate(&project.id).unwrap();
                    project.old_keys.push(std::mem::replace(&mut project.key,changed.api_key));
                    project.epoch+=1;
                }
                5 => {
                    Database::open(source.join(&project.id).join("data")).unwrap().compact().unwrap();
                    project.wal_version=2;
                }
                _ => {
                    let data = source.join(&project.id).join("data");
                    Database::open(&data).unwrap().checkpoint().unwrap();
                    std::fs::write(data.join("checkpoint.emily"),b"synthetic damaged optional cache").unwrap();
                }
            }
            check(&store,&projects);
            if at%4==0 || at==last {fork_and_check(&mut store,temp.path(),&projects,at);}
        }
    }
}

#[test]
fn real_128_project_archive_and_restore_preserve_the_full_registry_capacity() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let mut store = ProjectStore::open(&source).unwrap();
    let mut credentials = Vec::new();
    for _ in 0..emilybase_server::MAX_PROJECTS {
        let created = store.create("synthetic capacity").unwrap();
        credentials.push((created.project.id, created.api_key));
    }
    assert!(store.create("one too many").is_err());
    let archive = temp.path().join("capacity.backup");
    let target = temp.path().join("restored");
    let image = store.backup_image().unwrap();
    assert_eq!(store.backup(&archive).unwrap().projects.len(), 128);
    restore_registry_backup(&archive, &target).unwrap();
    let mut restored = ProjectStore::open_existing(&target).unwrap();
    assert_eq!(restored.backup_image().unwrap(), image);
    for (id, key) in &credentials {
        let status = restored.authorize(id, key).unwrap().status().unwrap();
        assert_eq!(status.transaction, 1);
        assert_eq!(status.rows, 0);
    }
    assert!(restored.create("one too many").is_err());
    assert_eq!(restored.list().unwrap().len(), 128);
}
