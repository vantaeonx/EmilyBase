//! Explicit startup selection, without filesystem access or format probing.
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountLayout {
    V1,
    V2,
}
#[derive(Debug, PartialEq, Eq)]
pub enum DataSelection {
    Registry(PathBuf),
    Accounts {
        path: PathBuf,
        layout: AccountLayout,
    },
}
impl DataSelection {
    pub fn from_environment() -> Result<Self, &'static str> {
        Self::select(
            std::env::var_os("EMILYBASE_DATA_DIR"),
            std::env::var_os("EMILYBASE_ACCOUNT_ROOT"),
            std::env::var_os("EMILYBASE_ACCOUNT_LAYOUT"),
        )
    }
    fn select(
        registry: Option<OsString>,
        accounts: Option<OsString>,
        layout: Option<OsString>,
    ) -> Result<Self, &'static str> {
        if registry.is_some() && accounts.is_some() {
            return Err("select one data mode");
        }
        let selected = match layout.as_deref() {
            None => AccountLayout::V1,
            Some(value) if value == OsStr::new("v1") => AccountLayout::V1,
            Some(value) if value == OsStr::new("v2") => AccountLayout::V2,
            Some(_) => return Err("invalid account layout"),
        };
        if layout.is_some() && accounts.is_none() {
            return Err("account layout requires account root");
        }
        Ok(match accounts {
            Some(path) => Self::Accounts {
                path: path.into(),
                layout: selected,
            },
            None => Self::Registry(registry.unwrap_or_else(|| "emilybase-data".into()).into()),
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStringExt;

    #[test]
    fn omitted_selector_preserves_registry_defaults_and_v1_account_admission() {
        assert_eq!(
            DataSelection::select(None, None, None).unwrap(),
            DataSelection::Registry("emilybase-data".into())
        );
        assert_eq!(
            DataSelection::select(Some("legacy".into()), None, None).unwrap(),
            DataSelection::Registry("legacy".into())
        );
        assert_eq!(
            DataSelection::select(None, Some("accounts".into()), None).unwrap(),
            DataSelection::Accounts {
                path: "accounts".into(),
                layout: AccountLayout::V1
            }
        );
    }
    #[test]
    fn explicit_selector_requires_an_account_root_and_never_initializes_a_registry() {
        for selector in ["v1", "v2"] {
            assert!(DataSelection::select(None, None, Some(selector.into())).is_err());
            assert!(
                DataSelection::select(Some("legacy".into()), None, Some(selector.into())).is_err()
            );
        }
        assert_eq!(
            DataSelection::select(None, Some("accounts".into()), Some("v2".into())).unwrap(),
            DataSelection::Accounts {
                path: "accounts".into(),
                layout: AccountLayout::V2
            }
        );
        assert_eq!(
            DataSelection::select(None, Some("accounts".into()), Some("v1".into())).unwrap(),
            DataSelection::Accounts {
                path: "accounts".into(),
                layout: AccountLayout::V1
            }
        );
    }
    #[test]
    fn unknown_empty_whitespace_non_utf8_and_conflicting_selection_never_fall_back() {
        for selector in [
            OsString::new(),
            "2".into(),
            "V2".into(),
            "v2 ".into(),
            " v1".into(),
            "v2\0".into(),
            OsString::from_vec(vec![0xff]),
        ] {
            assert_eq!(
                DataSelection::select(None, Some("accounts".into()), Some(selector)),
                Err("invalid account layout")
            );
        }
        for selector in [None, Some("v1".into()), Some("v2".into())] {
            assert_eq!(
                DataSelection::select(Some("legacy".into()), Some("accounts".into()), selector),
                Err("select one data mode")
            );
        }
    }
    #[test]
    fn native_paths_keep_exact_bytes_and_are_not_lossily_reinterpreted_as_selectors() {
        let native = OsString::from_vec(vec![b'r', 0xff]);
        assert_eq!(
            DataSelection::select(None, Some(native.clone()), Some("v2".into())).unwrap(),
            DataSelection::Accounts {
                path: native.clone().into(),
                layout: AccountLayout::V2
            }
        );
        assert_eq!(
            DataSelection::select(Some(native.clone()), None, None).unwrap(),
            DataSelection::Registry(native.into())
        );
    }
}
