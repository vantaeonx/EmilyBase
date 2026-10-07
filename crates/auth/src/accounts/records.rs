use super::{AccountInfo, Error, MAX_LOGIN_BYTES, Result};
use crate::password::PasswordDigest;
use emilybase_catalog::{Column, DataType, Row, Schema, Value};

pub(super) fn validate_login(login: &str) -> Result<()> {
    let first = login.as_bytes().first().copied().ok_or(Error::Login)?;
    if login.len() > MAX_LOGIN_BYTES
        || !(first.is_ascii_lowercase() || first.is_ascii_digit())
        || !login
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
    {
        return Err(Error::Login);
    }
    Ok(())
}

fn schema(name: &str, columns: &[(&str, DataType)]) -> Schema {
    Schema {
        name: name.into(),
        primary_key: 0,
        columns: columns
            .iter()
            .map(|(name, ty)| Column {
                name: (*name).into(),
                data_type: *ty,
                nullable: false,
            })
            .collect(),
    }
}
pub(super) fn scope_schema() -> Schema {
    schema(
        "auth_scope",
        &[
            ("id", DataType::Integer),
            ("version", DataType::Integer),
            ("project", DataType::Text),
            ("dummy", DataType::Bytes),
        ],
    )
}
pub(super) fn user_schema() -> Schema {
    schema(
        "auth_users",
        &[
            ("login", DataType::Text),
            ("id", DataType::Bytes),
            ("digest", DataType::Bytes),
            ("epoch", DataType::Integer),
            ("disabled", DataType::Boolean),
        ],
    )
}

pub(super) struct Record {
    pub info: AccountInfo,
    pub digest: PasswordDigest,
}
impl Record {
    pub fn decode(row: &[Value]) -> Result<Self> {
        let [
            Value::Text(login),
            Value::Bytes(id),
            Value::Bytes(digest),
            Value::Integer(epoch),
            Value::Boolean(disabled),
        ] = row
        else {
            return Err(Error::Corrupt);
        };
        validate_login(login).map_err(|_| Error::Corrupt)?;
        if *epoch <= 0 {
            return Err(Error::Corrupt);
        }
        let id: [u8; 16] = id.as_slice().try_into().map_err(|_| Error::Corrupt)?;
        let digest = PasswordDigest::decode(digest).map_err(|_| Error::Corrupt)?;
        Ok(Self {
            info: AccountInfo {
                id,
                login: login.clone(),
                credential_epoch: *epoch as u64,
                disabled: *disabled,
            },
            digest,
        })
    }
    pub fn encode(&self) -> Row {
        vec![
            Value::Text(self.info.login.clone()),
            Value::Bytes(self.info.id.to_vec()),
            Value::Bytes(self.digest.encode().to_vec()),
            Value::Integer(self.info.credential_epoch as i64),
            Value::Boolean(self.info.disabled),
        ]
    }
}
