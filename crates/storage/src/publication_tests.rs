use crate::{Error, Page, Pager};
use std::fs;

#[test]
fn bounded_private_bytes_keep_original_sync_failure_and_no_replace_contract() {
    let _serial = CASES.lock().unwrap();
    let data = b"synthetic-private-bytes";
    for (phase, after, published) in [
        ("file_sync", false, false),
        ("file_sync", true, false),
        ("parent_sync", false, true),
        ("parent_sync", true, true),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("selected");
        let _fault = FaultGuard::new(phase, after);
        let result = crate::publish_private_file(&target, data, 1024);
        if published {
            assert!(matches!(result, Err(Error::PublicationUnknown(_))));
            assert_eq!(fs::read(&target).unwrap(), data);
        } else {
            assert!(matches!(result, Err(Error::Io(_))));
            assert!(!target.exists());
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("selected");
    assert!(crate::publish_private_file(&target, data, data.len() - 1).is_err());
    assert!(!target.exists());
    crate::publish_private_file(&target, data, data.len()).unwrap();
    assert!(crate::publish_private_file(&target, b"replacement", 1024).is_err());
    assert_eq!(fs::read(&target).unwrap(), data);
}

#[test]
fn bounded_private_bytes_reject_changed_stage_content_before_selection() {
    let _serial = CASES.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("selected");
    let result = crate::byte_file::publish_with(
        &target,
        b"synthetic-original",
        1024,
        || {
            let stage = stage_name(dir.path());
            fs::write(dir.path().join(stage), b"synthetic-replaced").unwrap();
        },
        || {},
    );
    assert!(matches!(result, Err(Error::PathChanged)));
    assert!(!target.exists());
}

static CASES: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct FaultGuard;
impl FaultGuard {
    fn new(phase: &'static str, after: bool) -> Self {
        assert!(FAULT.replace(Some((phase, after))).is_none());
        Self
    }
}
impl Drop for FaultGuard {
    fn drop(&mut self) {
        FAULT.set(None);
    }
}

fn stage_name(parent: &std::path::Path) -> std::ffi::OsString {
    fs::read_dir(parent)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .find(|name| name.to_string_lossy().starts_with(".emilybase-create-"))
        .unwrap()
}

fn pages(count: u64) -> Vec<Page> {
    (1..=count)
        .map(|id| {
            let mut page = Page::new(id).unwrap();
            page.insert(format!("synthetic page {id}").as_bytes())
                .unwrap();
            page.insert(&[0, 255, 1, id as u8]).unwrap();
            page
        })
        .collect()
}

thread_local! {
    static FAULT: std::cell::Cell<Option<(&'static str, bool)>> = const { std::cell::Cell::new(None) };
}
pub(crate) fn sync_failure(phase: &'static str, after: bool) -> std::io::Result<()> {
    if FAULT.get() == Some((phase, after)) {
        Err(std::io::Error::other("synthetic page-file sync failure"))
    } else {
        Ok(())
    }
}

#[test]
fn raw_page_file_parent_substitution_cannot_select_or_delete_a_foreign_inode() {
    let _serial = CASES.lock().unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let parent = temporary.path().join("outputs");
    let moved = temporary.path().join("moved");
    fs::create_dir(&parent).unwrap();
    let mut page = Page::new(1).unwrap();
    page.insert(b"original synthetic page").unwrap();
    let foreign_path = temporary.path().join("foreign.emily");
    let mut foreign = Page::new(1).unwrap();
    foreign.insert(b"foreign synthetic page").unwrap();
    drop(Pager::create_with_pages(&foreign_path, &[foreign]).unwrap());
    let foreign_bytes = fs::read(&foreign_path).unwrap();
    let name = std::cell::RefCell::new(None);
    let target = parent.join("selected.emily");
    let result = Pager::create_with(
        &target,
        &[page],
        || {
            let stage = fs::read_dir(&parent)
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .find(|n| n.to_string_lossy().starts_with(".emilybase-create-"))
                .unwrap();
            fs::rename(&parent, &moved).unwrap();
            fs::create_dir(&parent).unwrap();
            fs::write(parent.join(&stage), &foreign_bytes).unwrap();
            *name.borrow_mut() = Some(stage);
        },
        || {},
    );
    assert!(matches!(result, Err(Error::PathChanged)));
    assert!(!target.exists());
    assert_eq!(
        fs::read(parent.join(name.borrow().as_ref().unwrap())).unwrap(),
        foreign_bytes
    );
}

#[test]
fn raw_creation_preserves_substituted_and_detached_staging_entries() {
    let _serial = CASES.lock().unwrap();
    for symbolic in [false, true] {
        let temporary = tempfile::tempdir().unwrap();
        let parent = temporary.path().join("outputs");
        fs::create_dir(&parent).unwrap();
        let protected = temporary.path().join("protected");
        fs::write(&protected, b"synthetic foreign file").unwrap();
        let detached = temporary.path().join("detached");
        let name = std::cell::RefCell::new(None);
        let target = parent.join("selected");
        let initial = pages(3);
        let result = Pager::create_with(
            &target,
            &initial,
            || {
                let stage = stage_name(&parent);
                fs::rename(parent.join(&stage), &detached).unwrap();
                if symbolic {
                    std::os::unix::fs::symlink(&protected, parent.join(&stage)).unwrap();
                } else {
                    fs::copy(&protected, parent.join(&stage)).unwrap();
                }
                *name.borrow_mut() = Some(stage);
            },
            || {},
        );
        assert!(matches!(result, Err(Error::PathChanged)));
        assert!(!target.exists());
        let foreign = parent.join(name.borrow().as_ref().unwrap());
        assert_eq!(fs::read(&foreign).unwrap(), b"synthetic foreign file");
        if symbolic {
            assert!(
                fs::symlink_metadata(foreign)
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
        }
        let mut original = Pager::open(detached).unwrap();
        for page in initial {
            assert_eq!(original.read_page(page.id()).unwrap(), page);
        }
    }
}

#[test]
fn raw_post_publication_parent_or_entry_changes_report_unknown_without_cleanup() {
    let _serial = CASES.lock().unwrap();
    for replace_parent in [false, true] {
        let temporary = tempfile::tempdir().unwrap();
        let parent = temporary.path().join("outputs");
        fs::create_dir(&parent).unwrap();
        let target = parent.join("selected");
        let moved = temporary.path().join("moved");
        let initial = pages(2);
        let result = Pager::create_with(
            &target,
            &initial,
            || {},
            || {
                if replace_parent {
                    fs::rename(&parent, &moved).unwrap();
                    fs::create_dir(&parent).unwrap();
                    fs::write(parent.join("keep"), b"foreign parent").unwrap();
                } else {
                    fs::rename(&target, &moved).unwrap();
                    fs::write(&target, b"foreign selection").unwrap();
                }
            },
        );
        assert!(matches!(result, Err(Error::PublicationUnknown(_))));
        let original = if replace_parent {
            moved.join("selected")
        } else {
            moved
        };
        let mut reopened = Pager::open(original).unwrap();
        for page in initial {
            assert_eq!(reopened.read_page(page.id()).unwrap(), page);
        }
        if replace_parent {
            assert_eq!(fs::read(parent.join("keep")).unwrap(), b"foreign parent");
        } else {
            assert_eq!(fs::read(target).unwrap(), b"foreign selection");
        }
    }
}

#[test]
fn directory_handle_creation_uses_exact_inode_and_rejects_noncomponent_names() {
    let _serial = CASES.lock().unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let parent = temporary.path().join("outputs");
    let moved = temporary.path().join("moved");
    fs::create_dir(&parent).unwrap();
    let directory = fs::File::open(&parent).unwrap();
    fs::rename(&parent, &moved).unwrap();
    fs::create_dir(&parent).unwrap();
    let initial = pages(4);
    let mut pager = Pager::create_with_pages_at(&directory, "own.emily", &initial).unwrap();
    for page in &initial {
        assert_eq!(pager.read_page(page.id()).unwrap(), *page);
    }
    assert!(!parent.join("own.emily").exists());
    assert!(matches!(
        Pager::open(moved.join("own.emily")),
        Err(Error::Busy)
    ));
    drop(pager);
    Pager::open(moved.join("own.emily"))
        .unwrap()
        .verify()
        .unwrap();
    for name in ["", ".", "..", "a/b", "../escaped", "/absolute"] {
        assert!(matches!(
            Pager::create_with_pages_at(&directory, name, &[]),
            Err(Error::Path)
        ));
    }
    let regular = fs::File::open(moved.join("own.emily")).unwrap();
    assert!(matches!(
        Pager::create_with_pages_at(&regular, "invalid", &[]),
        Err(Error::Path)
    ));
    assert_eq!(fs::read_dir(&parent).unwrap().count(), 0);
    assert_eq!(fs::read_dir(moved).unwrap().count(), 1);
}

#[test]
fn raw_file_inputs_and_creation_parents_refuse_aliases_and_nonregular_objects() {
    let _serial = CASES.lock().unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("source");
    drop(Pager::create(&source).unwrap());
    let bytes = fs::read(&source).unwrap();
    let alias = temporary.path().join("alias");
    std::os::unix::fs::symlink(&source, &alias).unwrap();
    assert!(Pager::open(&alias).is_err());
    let hard = temporary.path().join("hard");
    fs::hard_link(&source, &hard).unwrap();
    assert!(matches!(Pager::open(&source), Err(Error::Path)));
    assert!(matches!(Pager::open(&hard), Err(Error::Path)));
    assert!(matches!(
        Pager::open(temporary.path()),
        Err(Error::Path) | Err(Error::Io(_))
    ));
    let fifo = temporary.path().join("fifo");
    rustix::fs::mknodat(
        rustix::fs::CWD,
        &fifo,
        rustix::fs::FileType::Fifo,
        rustix::fs::Mode::RWXU,
        0,
    )
    .unwrap();
    assert!(matches!(Pager::open(&fifo), Err(Error::Path)));
    let parent_alias = temporary.path().join("parent-alias");
    std::os::unix::fs::symlink(temporary.path(), &parent_alias).unwrap();
    assert!(Pager::create(parent_alias.join("selected")).is_err());
    assert!(!temporary.path().join("selected").exists());
    assert_eq!(fs::read(&source).unwrap(), bytes);
    fs::remove_file(hard).unwrap();
    Pager::open(source).unwrap().verify().unwrap();
}

#[test]
fn staged_link_and_permission_changes_cannot_return_an_admitted_new_pager() {
    use std::os::unix::fs::PermissionsExt;
    let _serial = CASES.lock().unwrap();
    for after_rename in [false, true] {
        for extra_link in [false, true] {
            let temporary = tempfile::tempdir().unwrap();
            let target = temporary.path().join("selected");
            let alias = temporary.path().join("extra-link");
            let original = pages(2);
            let mutate = || {
                let selected = if after_rename {
                    target.clone()
                } else {
                    temporary.path().join(stage_name(temporary.path()))
                };
                if extra_link {
                    fs::hard_link(selected, &alias).unwrap();
                } else {
                    fs::set_permissions(selected, fs::Permissions::from_mode(0o644)).unwrap();
                }
            };
            let result = Pager::create_with(
                &target,
                &original,
                || {
                    if !after_rename {
                        mutate();
                    }
                },
                || {
                    if after_rename {
                        mutate();
                    }
                },
            );
            if after_rename {
                assert!(matches!(result, Err(Error::PublicationUnknown(_))));
                if extra_link {
                    fs::remove_file(&alias).unwrap();
                }
                let mut pager = Pager::open(&target).unwrap();
                for page in &original {
                    assert_eq!(pager.read_page(page.id()).unwrap(), *page);
                }
            } else {
                assert!(matches!(result, Err(Error::Path)));
                assert!(!target.exists());
                if extra_link {
                    let mut preserved = Pager::open(&alias).unwrap();
                    for page in &original {
                        assert_eq!(preserved.read_page(page.id()).unwrap(), *page);
                    }
                }
            }
        }
    }
}

#[test]
fn initial_readback_rejects_truncation_and_valid_but_substituted_page_bytes() {
    let _serial = CASES.lock().unwrap();
    for mutation in 0..3 {
        let temporary = tempfile::tempdir().unwrap();
        let target = temporary.path().join("selected");
        let initial = pages(2);
        let result = Pager::create_with(
            &target,
            &initial,
            || {
                let stage = temporary.path().join(stage_name(temporary.path()));
                let mut bytes = fs::read(&stage).unwrap();
                match mutation {
                    0 => {
                        bytes.truncate(crate::PAGE_SIZE);
                    }
                    1 => {
                        bytes[crate::PAGE_SIZE + 100] ^= 1;
                    }
                    _ => {
                        let mut forged = Page::new(1).unwrap();
                        forged.insert(b"synthetic valid foreign page").unwrap();
                        bytes[crate::PAGE_SIZE..2 * crate::PAGE_SIZE]
                            .copy_from_slice(&forged.encode());
                    }
                }
                fs::write(stage, bytes).unwrap();
            },
            || {},
        );
        assert!(matches!(
            result,
            Err(Error::FileLength(_)) | Err(Error::Layout(_))
        ));
        assert!(!target.exists());
        assert_eq!(fs::read_dir(temporary.path()).unwrap().count(), 0);
    }
}

#[test]
fn raw_sync_failures_preserve_selection_only_after_rename_and_release_ownership() {
    let _serial = CASES.lock().unwrap();
    for descriptor in [false, true] {
        for phase in ["file_sync", "parent_sync"] {
            for after in [false, true] {
                let temporary = tempfile::tempdir().unwrap();
                let target = temporary.path().join("selected");
                let directory = fs::File::open(temporary.path()).unwrap();
                let initial = pages(2);
                let fault = FaultGuard::new(phase, after);
                let result = if descriptor {
                    Pager::create_with_pages_at(&directory, "selected", &initial)
                } else {
                    Pager::create_with_pages(&target, &initial)
                };
                drop(fault);
                if phase == "parent_sync" {
                    assert!(matches!(result, Err(Error::PublicationUnknown(_))));
                    let mut reopened = Pager::open(&target).unwrap();
                    for page in &initial {
                        assert_eq!(reopened.read_page(page.id()).unwrap(), *page);
                    }
                    drop(reopened);
                    assert!(Pager::create(&target).is_err());
                } else {
                    assert!(matches!(result, Err(Error::Io(_))));
                    assert!(!target.exists());
                }
                assert_eq!(
                    fs::read_dir(temporary.path()).unwrap().count(),
                    usize::from(phase == "parent_sync")
                );
                drop(Pager::create_with_pages(temporary.path().join("retry"), &initial).unwrap());
            }
        }
    }
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(32))]
    #[test]
    fn generated_initial_records_keep_exact_bytes_and_independent_updates(
        descriptor in proptest::bool::ANY,
        failure in proptest::bool::ANY,
        after in proptest::bool::ANY,
        parent_failure in proptest::bool::ANY,
        records in proptest::collection::vec(proptest::collection::vec(proptest::num::u8::ANY, 0..1024), 0..12),
    ) {
        let _serial = CASES.lock().unwrap();
        let temporary = tempfile::tempdir().unwrap();
        let target = temporary.path().join("selected");
        let directory = fs::File::open(temporary.path()).unwrap();
        let initial = records.iter().enumerate().map(|(i, record)| { let mut page = Page::new(i as u64 + 1).unwrap();page.insert(record).unwrap();page }).collect::<Vec<_>>();
        let mut expected = crate::header::encode().to_vec();
        for page in &initial { expected.extend_from_slice(&page.encode()); }
        let phase = if parent_failure { "parent_sync" } else { "file_sync" };
        let fault = failure.then(|| FaultGuard::new(phase, after));
        let result = if descriptor { Pager::create_with_pages_at(&directory, "selected", &initial) } else { Pager::create_with_pages(&target, &initial) };
        drop(fault);
        if failure {
            proptest::prop_assert!(result.is_err());
            proptest::prop_assert_eq!(target.exists(), parent_failure);
            if !parent_failure { drop(Pager::create_with_pages(&target, &initial).unwrap()); }
        } else { drop(result.unwrap()); }
        proptest::prop_assert_eq!(fs::read(&target).unwrap(), expected);
        let mut reopened = Pager::open(&target).unwrap();
        for (i, record) in records.iter().enumerate() {
            let page = reopened.read_page(i as u64 + 1).unwrap();
            proptest::prop_assert_eq!(page.get(0).unwrap(), record.as_slice());
        }
        let mut appended = Page::new(initial.len() as u64 + 1).unwrap();
        appended.insert(b"independent appended record").unwrap();
        reopened.write_page(&appended).unwrap();
        drop(reopened);
        let mut reopened = Pager::open(&target).unwrap();
        proptest::prop_assert_eq!(reopened.page_count(), initial.len() as u64 + 1);
        let appended = reopened.read_page(initial.len() as u64 + 1).unwrap();
        proptest::prop_assert_eq!(appended.get(0).unwrap(), b"independent appended record");
    }
}

#[test]
#[ignore = "page creation subprocess helper invoked by its parent"]
fn creation_worker() {
    use std::io::{Read, Write};
    let target =
        std::path::PathBuf::from(std::env::var_os("EMILYBASE_PAGE_CREATE_TARGET").unwrap());
    let phase = std::env::var("EMILYBASE_PAGE_CREATE_PHASE").unwrap();
    let barrier = || {
        println!("PAGE_CREATION_BOUNDARY");
        std::io::stdout().flush().unwrap();
        let _ = std::io::stdin().read(&mut [0; 1]);
    };
    let pager = Pager::create_with(
        &target,
        &pages(3),
        || {
            if phase == "synced" {
                barrier();
            }
        },
        || {
            if phase == "renamed" {
                barrier();
            }
        },
    )
    .unwrap();
    if phase == "returned" {
        barrier();
    }
    assert_eq!(pager.page_count(), 3);
}

#[test]
fn native_creation_kills_keep_no_target_or_the_exact_synced_complete_image() {
    use std::io::{BufRead, BufReader};
    use std::process::{Child, Command, Stdio};
    use std::sync::mpsc;
    use std::time::Duration;
    struct Worker(Child);
    impl Drop for Worker {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let _serial = CASES.lock().unwrap();
    let initial = pages(3);
    let mut expected = crate::header::encode().to_vec();
    for page in &initial {
        expected.extend_from_slice(&page.encode());
    }
    for phase in ["synced", "renamed", "returned"] {
        let temporary = tempfile::tempdir().unwrap();
        let target = temporary.path().join("selected");
        let mut worker = Worker(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "publication_tests::creation_worker",
                    "--nocapture",
                    "--ignored",
                ])
                .env("EMILYBASE_PAGE_CREATE_TARGET", &target)
                .env("EMILYBASE_PAGE_CREATE_PHASE", phase)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let stdout = worker.0.stdout.take().unwrap();
        let (sender, receiver) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if line == "PAGE_CREATION_BOUNDARY" {
                    let _ = sender.send(());
                    break;
                }
            }
        });
        receiver.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(target.exists(), phase != "synced");
        if phase != "synced" {
            assert!(matches!(Pager::open(&target), Err(Error::Busy)));
        }
        worker.0.kill().unwrap();
        assert!(!worker.0.wait().unwrap().success());
        reader.join().unwrap();
        if phase == "synced" {
            assert!(!target.exists());
            assert!(fs::read_dir(temporary.path()).unwrap().any(|entry| {
                entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".emilybase-create-")
            }));
        } else {
            assert_eq!(fs::read(&target).unwrap(), expected);
            let mut pager = Pager::open(&target).unwrap();
            for page in &initial {
                assert_eq!(pager.read_page(page.id()).unwrap(), *page);
            }
            let mut appended = Page::new(4).unwrap();
            appended.insert(b"new write after kill").unwrap();
            pager.write_page(&appended).unwrap();
            drop(pager);
            assert_eq!(Pager::open(&target).unwrap().page_count(), 4);
        }
        drop(Pager::create_with_pages(temporary.path().join("retry"), &initial).unwrap());
    }
}
