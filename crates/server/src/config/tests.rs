use super::*;
use proptest::prelude::*;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt, symlink};
const KEY: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
fn secret(path: &Path, bytes: &[u8]) {
    let mut file = File::options()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.write_all(bytes).unwrap();
}
fn path_load(path: &Path) -> Result<Zeroizing<String>> {
    load(None, Some(path.as_os_str().into()))
}
#[test]
fn exactly_one_source_is_required_and_environment_policy_remains_exact() {
    assert!(matches!(
        load(None, None),
        Err(Error::Config("master key required"))
    ));
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("never-opened");
    assert!(matches!(
        load(
            Some(Zeroizing::new(KEY.into())),
            Some(missing.as_os_str().into())
        ),
        Err(Error::Config("select one master key source"))
    ));
    assert!(!missing.exists());
    assert_eq!(
        load(Some(Zeroizing::new(KEY.into())), None)
            .unwrap()
            .as_str(),
        KEY
    );
    for invalid in ["", "synthetic-private-value", &(KEY.to_string() + "\n")] {
        let error = load(Some(Zeroizing::new(invalid.into())), None).unwrap_err();
        if !invalid.is_empty() {
            assert!(!error.to_string().contains(invalid));
        }
    }
}
#[test]
fn private_file_accepts_exact_key_or_one_final_lf_with_readonly_owner_mode() {
    let dir = tempfile::tempdir().unwrap();
    for mode in [0o400, 0o600] {
        for newline in [false, true] {
            let path = dir.path().join(format!("ключ 界 {mode}-{newline}"));
            let bytes = if newline {
                format!("{KEY}\n")
            } else {
                KEY.into()
            };
            secret(&path, bytes.as_bytes());
            fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
            assert_eq!(path_load(&path).unwrap().as_str(), KEY);
            assert_eq!(fs::read(&path).unwrap(), bytes.as_bytes());
        }
    }
}
#[test]
fn oversized_short_invalid_utf8_and_noncanonical_content_refuse_without_echo() {
    let dir = tempfile::tempdir().unwrap();
    let inputs = vec![
        vec![b'a'; 63],
        vec![b'a'; 66],
        vec![b'a'; 4096],
        vec![b'A'; 64],
        vec![0; 64],
        vec![255; 64],
        format!("{KEY}\r\n").into_bytes(),
        format!(" {KEY}").into_bytes(),
        format!("{KEY} ").into_bytes(),
    ];
    for (i, bytes) in inputs.into_iter().enumerate() {
        let path = dir.path().join(format!("synthetic-private-{i}"));
        secret(&path, &bytes);
        let error = path_load(&path).unwrap_err();
        assert!(!error.to_string().contains(path.to_str().unwrap()));
        if let Ok(text) = std::str::from_utf8(&bytes) {
            assert!(!error.to_string().contains(text));
        }
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
}
#[test]
fn broad_permissions_execute_bits_special_bits_and_link_aliases_refuse() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secret");
    secret(&path, KEY.as_bytes());
    for mode in [0o640, 0o644, 0o700, 0o660, 0o4600, 0o1600] {
        fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
        assert!(path_load(&path).is_err());
    }
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let link = dir.path().join("alias");
    symlink(&path, &link).unwrap();
    assert!(path_load(&link).is_err());
    fs::remove_file(&link).unwrap();
    fs::hard_link(&path, &link).unwrap();
    assert!(path_load(&link).is_err());
    assert!(path_load(&path).is_err());
    fs::remove_file(&link).unwrap();
    assert!(path_load(dir.path()).is_err());
    assert_eq!(path_load(&path).unwrap().as_str(), KEY);
}
#[test]
fn replacement_or_metadata_change_after_read_refuses_the_selected_secret() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secret");
    let old = dir.path().join("detached");
    secret(&path, KEY.as_bytes());
    assert!(matches!(
        read_key_after(&path, || {
            fs::rename(&path, &old).unwrap();
            secret(&path, "b".repeat(64).as_bytes());
        }),
        Err(Error::Config("master key file changed"))
    ));
    assert_eq!(fs::read(&old).unwrap(), KEY.as_bytes());
    assert_eq!(fs::read(&path).unwrap(), "b".repeat(64).as_bytes());
    assert!(matches!(
        read_key_after(&path, || {
            fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
        }),
        Err(Error::Config("master key file changed"))
    ));
}
proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    #[test]
    fn generated_hexadecimal_keys_follow_exact_file_transport_policy(
        nibbles in prop::collection::vec(0..16_u8,64),newline in any::<bool>()
    ) {
        let key:String=nibbles.into_iter().map(|n|char::from(b"0123456789abcdef"[usize::from(n)])).collect();
        let dir=tempfile::tempdir().unwrap();
        let path=dir.path().join("synthetic-key");
        let bytes=if newline {format!("{key}\n")}else{key.clone()};
        secret(&path,bytes.as_bytes());
        let loaded=path_load(&path).unwrap();
        prop_assert_eq!(loaded.as_str(),key.as_str());
        fs::write(&path,format!("{key}\n\n")).unwrap();
        prop_assert!(path_load(&path).is_err());
    }
}
