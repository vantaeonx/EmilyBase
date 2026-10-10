use super::*;
use crate::publication_tests::{CASES, FaultGuard};
use proptest::prelude::*;
use std::fs;
use std::io::Cursor;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};

struct Input {
    bytes: Cursor<Vec<u8>>,
    chunk: usize,
    calls: usize,
    seeks: usize,
    consumed: usize,
    maximum_request: usize,
    interrupted: bool,
    read_failure: Option<(usize, usize)>,
    seek_failure: Option<usize>,
    change: u8,
    late_edit: Option<PathBuf>,
}
impl Input {
    fn new(bytes: &[u8]) -> Self {
        Self {
            bytes: Cursor::new(bytes.to_vec()),
            chunk: 8192,
            calls: 0,
            seeks: 0,
            consumed: 0,
            maximum_request: 0,
            interrupted: false,
            read_failure: None,
            seek_failure: None,
            change: 0,
            late_edit: None,
        }
    }
}
impl Read for Input {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        self.calls += 1;
        self.maximum_request = self.maximum_request.max(out.len());
        if self.interrupted && self.calls % 2 == 1 {
            return Err(io::ErrorKind::Interrupted.into());
        }
        let current = self.bytes.position() as usize;
        let mut count = self.chunk.min(out.len());
        if let Some((phase, position)) = self.read_failure
            && self.seeks == phase
        {
            if current >= position {
                return Err(io::Error::other("synthetic read fault"));
            }
            count = count.min(position - current);
        }
        if self.seeks == 2
            && current == self.bytes.get_ref().len()
            && let Some(parent) = self.late_edit.take()
        {
            let mut file = File::options().write(true).open(stage(&parent)).unwrap();
            file.write_all(b"x").unwrap();
        }
        let count = self.bytes.read(&mut out[..count])?;
        self.consumed += count;
        Ok(count)
    }
}
impl Seek for Input {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        self.seeks += 1;
        if self.seek_failure == Some(self.seeks) {
            return Err(io::Error::other("synthetic seek fault"));
        }
        if self.seeks == 2 {
            match self.change {
                1 => self.bytes.get_mut()[0] ^= 1,
                2 => {
                    self.bytes.get_mut().pop();
                }
                3 => self.bytes.get_mut().push(0xff),
                _ => {}
            }
        }
        self.bytes.seek(from)
    }
}
fn stage(parent: &Path) -> PathBuf {
    fs::read_dir(parent)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".emilybase-create-")
        })
        .unwrap()
}
fn at(parent: &Path) -> File {
    File::open(parent).unwrap()
}

#[test]
fn declared_bounds_and_invalid_components_refuse_before_source_io() {
    let temp = tempfile::tempdir().unwrap();
    let directory = at(temp.path());
    let mut input = Input::new(b"synthetic");
    assert!(matches!(
        publish_private_reader_at_retained(&directory, "selected", &mut input, 9, 8),
        Err(Error::FileLength(9))
    ));
    for name in ["", ".", "..", "a/b", "../escape", "/absolute"] {
        assert!(matches!(
            publish_private_reader_at_retained(&directory, name, &mut input, 9, 9),
            Err(Error::Path)
        ));
    }
    assert_eq!((input.calls, input.seeks), (0, 0));
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
}

#[test]
fn exact_empty_short_and_interrupted_sources_publish_complete_private_selected_inodes() {
    for bytes in [
        vec![],
        b"synthetic-private\0\xff".to_vec(),
        vec![0x93; 20000],
    ] {
        for chunk in [1, 7, 8192] {
            let temp = tempfile::tempdir().unwrap();
            let directory = at(temp.path());
            let mut input = Input::new(&bytes);
            input.bytes.set_position(u64::MAX);
            input.chunk = chunk;
            input.interrupted = true;
            let mut file = publish_private_reader_at_retained(
                &directory,
                "selected",
                &mut input,
                bytes.len(),
                bytes.len(),
            )
            .unwrap();
            assert_eq!(file.stream_position().unwrap(), bytes.len() as u64);
            assert_eq!(file.metadata().unwrap().mode() & 0o777, 0o600);
            assert_eq!(
                file.metadata().unwrap().ino(),
                fs::metadata(temp.path().join("selected")).unwrap().ino()
            );
            assert_eq!(input.seeks, 2);
            assert_eq!(input.consumed, 2 * bytes.len());
            assert!(input.maximum_request <= 8192);
            assert_eq!(fs::read(temp.path().join("selected")).unwrap(), bytes);
            file.rewind().unwrap();
            let mut readback = Vec::new();
            file.read_to_end(&mut readback).unwrap();
            assert_eq!(readback, bytes);
            assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
        }
    }
}

#[test]
fn source_read_seek_length_and_between_pass_faults_never_select_partial_output() {
    let bytes = vec![0x84; 20000];
    for phase in [1, 2] {
        for position in [0, 1, 8192, 19999, 20000] {
            let temp = tempfile::tempdir().unwrap();
            let mut input = Input::new(&bytes);
            input.read_failure = Some((phase, position));
            assert!(matches!(
                publish_private_reader_at_retained(
                    &at(temp.path()),
                    "selected",
                    &mut input,
                    bytes.len(),
                    bytes.len()
                ),
                Err(Error::Io(_))
            ));
            assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
        }
        let temp = tempfile::tempdir().unwrap();
        let mut input = Input::new(&bytes);
        input.seek_failure = Some(phase);
        assert!(matches!(
            publish_private_reader_at_retained(
                &at(temp.path()),
                "selected",
                &mut input,
                bytes.len(),
                bytes.len()
            ),
            Err(Error::Io(_))
        ));
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
    }
    for change in [1, 2, 3, 4, 5] {
        let temp = tempfile::tempdir().unwrap();
        let mut input = Input::new(&bytes);
        if change == 4 {
            input.bytes.get_mut().pop();
        } else if change == 5 {
            input.bytes.get_mut().push(0);
        } else {
            input.change = change;
        }
        let result = publish_private_reader_at_retained(
            &at(temp.path()),
            "selected",
            &mut input,
            bytes.len(),
            bytes.len(),
        );
        if change == 1 {
            assert!(matches!(result, Err(Error::Readback)));
        } else {
            assert!(matches!(result, Err(Error::SourceLength)));
        }
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
        if change == 5 {
            assert_eq!(input.consumed, bytes.len() + 1);
        }
    }
}

#[test]
fn all_original_sync_fault_outcomes_keep_preselection_cleanup_and_postselection_uncertainty() {
    let _serial = CASES.lock().unwrap();
    for (phase, after, selected) in [
        ("file_sync", false, false),
        ("file_sync", true, false),
        ("parent_sync", false, true),
        ("parent_sync", true, true),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let _fault = FaultGuard::new(phase, after);
        let result = publish_private_reader_at_retained(
            &at(temp.path()),
            "selected",
            &mut Cursor::new(b"synthetic"),
            9,
            9,
        );
        if selected {
            assert!(matches!(result, Err(Error::PublicationUnknown(_))));
            assert_eq!(
                fs::read(temp.path().join("selected")).unwrap(),
                b"synthetic"
            );
        } else {
            assert!(matches!(result, Err(Error::Io(_))));
        }
        assert_eq!(
            fs::read_dir(temp.path()).unwrap().count(),
            usize::from(selected)
        );
    }
}

#[test]
fn stage_content_length_permissions_and_links_cannot_pass_readback_or_owned_checks() {
    for shape in 0..6 {
        let temp = tempfile::tempdir().unwrap();
        let result = initialize(
            Pending::at(&at(temp.path()), "selected".as_ref()).unwrap(),
            &mut Cursor::new(b"synthetic"),
            9,
            || {
                let path = stage(temp.path());
                match shape {
                    0 => fs::write(path, b"corrupted").unwrap(),
                    1 => fs::write(path, b"short").unwrap(),
                    2 => fs::write(path, b"synthetic-extra").unwrap(),
                    3 => fs::set_permissions(path, fs::Permissions::from_mode(0o644)).unwrap(),
                    4 => fs::set_permissions(path, fs::Permissions::from_mode(0o400)).unwrap(),
                    _ => fs::hard_link(path, temp.path().join("linked")).unwrap(),
                }
            },
            || {},
        );
        assert!(result.is_err());
        assert!(!temp.path().join("selected").exists());
        assert_eq!(
            fs::read_dir(temp.path()).unwrap().count(),
            usize::from(shape == 5)
        );
    }
    let temp = tempfile::tempdir().unwrap();
    let mut input = Input::new(b"synthetic");
    input.late_edit = Some(temp.path().to_path_buf());
    assert!(matches!(
        publish_private_reader_at_retained(&at(temp.path()), "selected", &mut input, 9, 9),
        Err(Error::PathChanged)
    ));
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
}

#[test]
fn foreign_stage_substitutions_are_preserved_and_detached_original_is_not_selected() {
    for symbolic in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let protected = temp.path().join("protected");
        fs::write(&protected, b"foreign").unwrap();
        let detached = temp.path().join("detached");
        let name = std::cell::RefCell::new(None);
        let result = initialize(
            Pending::at(&at(temp.path()), "selected".as_ref()).unwrap(),
            &mut Cursor::new(b"synthetic"),
            9,
            || {
                let path = stage(temp.path());
                fs::rename(&path, &detached).unwrap();
                if symbolic {
                    symlink(&protected, &path).unwrap();
                } else {
                    fs::write(&path, b"foreign").unwrap();
                }
                *name.borrow_mut() = Some(path);
            },
            || {},
        );
        assert!(matches!(result, Err(Error::PathChanged)));
        assert!(!temp.path().join("selected").exists());
        assert_eq!(fs::read(detached).unwrap(), b"synthetic");
        assert_eq!(
            fs::read(name.borrow().as_ref().unwrap()).unwrap(),
            b"foreign"
        );
        assert_eq!(fs::read(protected).unwrap(), b"foreign");
    }
}

#[test]
fn selected_substitution_links_or_permissions_report_unknown_without_deleting_any_entry() {
    for shape in 0..3 {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("selected");
        let saved = temp.path().join("saved");
        let result = initialize(
            Pending::at(&at(temp.path()), "selected".as_ref()).unwrap(),
            &mut Cursor::new(b"synthetic"),
            9,
            || {},
            || match shape {
                0 => {
                    fs::rename(&target, &saved).unwrap();
                    fs::write(&target, b"synthetic").unwrap();
                }
                1 => fs::hard_link(&target, &saved).unwrap(),
                _ => fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).unwrap(),
            },
        );
        assert!(matches!(result, Err(Error::PublicationUnknown(_))));
        assert_eq!(fs::read(&target).unwrap(), b"synthetic");
        if shape != 2 {
            assert_eq!(fs::read(saved).unwrap(), b"synthetic");
        }
    }
}

#[test]
fn retained_directory_and_selected_descriptor_keep_original_namespace_while_refusing_overwrites() {
    let temp = tempfile::tempdir().unwrap();
    let original = temp.path().join("outputs");
    let moved = temp.path().join("moved");
    fs::create_dir(&original).unwrap();
    let directory = at(&original);
    fs::rename(&original, &moved).unwrap();
    fs::create_dir(&original).unwrap();
    let mut file = publish_private_reader_at_retained(
        &directory,
        "selected",
        &mut Cursor::new(b"synthetic"),
        9,
        9,
    )
    .unwrap();
    assert!(!original.join("selected").exists());
    assert_eq!(fs::read(moved.join("selected")).unwrap(), b"synthetic");
    assert!(
        publish_private_reader_at_retained(
            &directory,
            "selected",
            &mut Cursor::new(b"different"),
            9,
            9
        )
        .is_err()
    );
    fs::rename(moved.join("selected"), moved.join("saved")).unwrap();
    fs::write(moved.join("selected"), b"foreign").unwrap();
    file.rewind().unwrap();
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"synthetic");
    assert_eq!(fs::read(moved.join("selected")).unwrap(), b"foreign");
    let regular = File::open(moved.join("selected")).unwrap();
    assert!(matches!(
        publish_private_reader_at_retained(&regular, "invalid", &mut Cursor::new(b"x"), 1, 1),
        Err(Error::Path)
    ));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn generated_binary_sources_publish_identical_bytes_with_bounded_reads(
        bytes in prop::collection::vec(any::<u8>(),0..30000), chunk in 1usize..10000,
    ) {
        let temp=tempfile::tempdir().unwrap();let mut input=Input::new(&bytes);
        input.chunk=chunk;input.interrupted=true;
        let file=publish_private_reader_at_retained(&at(temp.path()),"selected",&mut input,bytes.len(),30000).unwrap();
        prop_assert_eq!(file.metadata().unwrap().len(),bytes.len() as u64);
        prop_assert_eq!(fs::read(temp.path().join("selected")).unwrap(),bytes);
        prop_assert!(input.maximum_request<=8192);
    }
}
