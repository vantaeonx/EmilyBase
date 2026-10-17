//! Copied v2 roster codec. Ordinary AccountRoot still refuses this layout.
use crate::{Error, MAX_PROJECTS, Result};
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{self, SeqAccess, Visitor},
};
use std::collections::BTreeSet;
use std::fmt;

pub const ACCOUNT_FILE_ROOT_MANIFEST_VERSION: u16 = 2;
pub const MAX_ACCOUNT_FILE_ROOT_MANIFEST_BYTES: usize = 32768;

/// Copied declared scope, not a selected directory or permission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileRootBinding {
    #[serde(deserialize_with = "identity")]
    project: String,
    #[serde(deserialize_with = "identity")]
    database_id: String,
}
impl FileRootBinding {
    pub fn new(project: &str, database_id: &str) -> Result<Self> {
        if !emilybase_auth::valid_project_id(project)
            || !emilybase_auth::valid_project_id(database_id)
        {
            return Err(Error::FileRootManifest("canonical binding identity"));
        }
        Ok(Self {
            project: project.into(),
            database_id: database_id.into(),
        })
    }
    pub fn project(&self) -> &str {
        &self.project
    }
    pub fn database_id(&self) -> &str {
        &self.database_id
    }
}

/// Exact v2 inventory produced by native capture or explicit offline restore.
/// Ordinary live open/migration still refuses this layout. Never authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountFileRootManifest {
    private_projects: Vec<String>,
    file_stores: Vec<FileRootBinding>,
    reset_at: u64,
}
impl AccountFileRootManifest {
    /// Require already sorted canonical rosters; never infer, sort or provision
    /// missing members. File projects must belong to the declared private roster.
    pub fn new(
        private_projects: Vec<String>,
        file_stores: Vec<FileRootBinding>,
        reset_at: u64,
    ) -> Result<Self> {
        let value = Self {
            private_projects,
            file_stores,
            reset_at,
        };
        value.validate()?;
        Ok(value)
    }
    pub fn private_projects(&self) -> &[String] {
        &self.private_projects
    }
    pub fn file_stores(&self) -> &[FileRootBinding] {
        &self.file_stores
    }
    pub const fn reset_at(&self) -> u64 {
        self.reset_at
    }
    fn validate(&self) -> Result<()> {
        if self.private_projects.len() > MAX_PROJECTS || self.file_stores.len() > MAX_PROJECTS {
            return Err(Error::Limit);
        }
        if self.reset_at > i64::MAX as u64
            || self
                .private_projects
                .iter()
                .any(|p| !emilybase_auth::valid_project_id(p))
            || self.private_projects.windows(2).any(|p| p[0] >= p[1])
            || self
                .file_stores
                .windows(2)
                .any(|p| p[0].project >= p[1].project)
        {
            return Err(Error::FileRootManifest("time or canonical roster order"));
        }
        let mut databases = BTreeSet::new();
        for binding in &self.file_stores {
            if !emilybase_auth::valid_project_id(binding.project())
                || !emilybase_auth::valid_project_id(binding.database_id())
                || self
                    .private_projects
                    .binary_search(&binding.project)
                    .is_err()
                || !databases.insert(binding.database_id())
            {
                return Err(Error::FileRootManifest(
                    "private membership or database identity",
                ));
            }
        }
        Ok(())
    }
    /// Canonical CRC-protected metadata, not authenticated backup provenance.
    /// Bounds and semantics are rechecked before allocating encoded output.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        self.encode().map(|(bytes, _)| bytes)
    }
    fn encode(&self) -> Result<(Vec<u8>, u32)> {
        self.validate()?;
        let payload = Payload {
            version: ACCOUNT_FILE_ROOT_MANIFEST_VERSION,
            private_projects: &self.private_projects,
            file_stores: &self.file_stores,
            reset_at: self.reset_at,
        };
        let encoded =
            serde_json::to_vec(&payload).map_err(|_| Error::FileRootManifest("JSON encoding"))?;
        let checksum = crc32fast::hash(&encoded);
        let bytes = serde_json::to_vec(&Envelope { payload, checksum })
            .map_err(|_| Error::FileRootManifest("JSON encoding"))?;
        if bytes.len() > MAX_ACCOUNT_FILE_ROOT_MANIFEST_BYTES {
            return Err(Error::Limit);
        }
        Ok((bytes, checksum))
    }
}
#[derive(Serialize)]
struct Payload<'a> {
    version: u16,
    private_projects: &'a [String],
    file_stores: &'a [FileRootBinding],
    reset_at: u64,
}
#[derive(Serialize)]
struct Envelope<'a> {
    payload: Payload<'a>,
    checksum: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DecodedEnvelope {
    payload: DecodedPayload,
    checksum: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DecodedPayload {
    version: u16,
    #[serde(deserialize_with = "private_roster")]
    private_projects: Vec<String>,
    #[serde(deserialize_with = "bounded_roster")]
    file_stores: Vec<FileRootBinding>,
    reset_at: u64,
}
/// Bounded pure inspection of an exact canonical v2 roster. CRC detects accidental
/// changes only; registry/private/actual file membership still needs runtime checks.
pub fn inspect_account_file_root_manifest_bytes(bytes: &[u8]) -> Result<AccountFileRootManifest> {
    if bytes.len() > MAX_ACCOUNT_FILE_ROOT_MANIFEST_BYTES {
        return Err(Error::Limit);
    }
    let envelope: DecodedEnvelope =
        serde_json::from_slice(bytes).map_err(|_| Error::FileRootManifest("JSON envelope"))?;
    if envelope.payload.version != ACCOUNT_FILE_ROOT_MANIFEST_VERSION {
        return Err(Error::FileRootManifestVersion(envelope.payload.version));
    }
    let value = AccountFileRootManifest::new(
        envelope.payload.private_projects,
        envelope.payload.file_stores,
        envelope.payload.reset_at,
    )?;
    let (canonical, checksum) = value.encode()?;
    // Exact bytes enforce checksum, field order, escaping and no trailing data.
    // Compare the decoded scalar too so it is never accepted as an ignored field.
    if envelope.checksum != checksum || canonical != bytes {
        return Err(Error::FileRootManifest("canonical encoding or checksum"));
    }
    Ok(value)
}

fn identity<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<String, D::Error> {
    struct Canonical;
    impl Visitor<'_> for Canonical {
        type Value = String;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("canonical32 lowercase hexadecimal identity")
        }
        fn visit_str<E: de::Error>(self, value: &str) -> std::result::Result<String, E> {
            if !emilybase_auth::valid_project_id(value) {
                return Err(E::custom("invalid canonical identity"));
            }
            Ok(value.into())
        }
    }
    d.deserialize_str(Canonical)
}
struct CanonicalId(String);
impl<'de> Deserialize<'de> for CanonicalId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        identity(d).map(Self)
    }
}
fn private_roster<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Vec<String>, D::Error> {
    bounded_roster::<D, CanonicalId>(d).map(|ids| ids.into_iter().map(|id| id.0).collect())
}
fn bounded_roster<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> std::result::Result<Vec<T>, D::Error> {
    struct Bounded<T>(std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>> Visitor<'de> for Bounded<T> {
        type Value = Vec<T>;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "at most{MAX_PROJECTS} roster entries")
        }
        fn visit_seq<A: SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            if seq.size_hint().is_some_and(|n| n > MAX_PROJECTS) {
                return Err(de::Error::custom("roster limit"));
            }
            let mut values = Vec::new();
            while let Some(value) = seq.next_element()? {
                if values.len() == MAX_PROJECTS {
                    return Err(de::Error::custom("roster limit"));
                }
                values
                    .try_reserve(1)
                    .map_err(|_| de::Error::custom("roster allocation"))?;
                values.push(value);
            }
            Ok(values)
        }
    }
    d.deserialize_seq(Bounded(std::marker::PhantomData))
}

#[cfg(test)]
mod tests;
