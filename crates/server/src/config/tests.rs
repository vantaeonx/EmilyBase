use super::*;
const KEY: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
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
