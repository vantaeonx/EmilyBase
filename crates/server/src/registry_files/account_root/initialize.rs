//! Explicit creation of one new fixed-roster private root, never implicit startup.
use super::*;

const INIT_PREFIX: &str = ".emilybase-account-init-";

/// Create one empty project and its private v3 store in a new owned root.
/// No user, password or reusable key is returned or printed. Obtain a service
/// key through authenticated online rotation or trusted offline private-file rotation.
/// Any failed prepared stage is retained for private operator inspection.
pub fn initialize_account_root(
    target: impl AsRef<Path>,
    name: &str,
    pool: PasswordPool,
    now: u64,
) -> Result<AccountBundleRootReport> {
    metadata::validate_name(name)?;
    validate_time(now)?;
    let target = target.as_ref();
    match fs::symlink_metadata(target) {
        Ok(_) => return Err(Error::Path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let mut pending = Pending::directory_with_prefix(target, INIT_PREFIX)?;
    // A failed preparation may contain foreign substitutions. Keep it rather
    // than recursively deleting names that were not individually owned.
    pending.retain();
    lock(&pending.owner)?;
    let root = pending.path();
    let mut registry = ProjectStore::open(root.join("registry"))?;
    let created = registry.create(name)?;
    let id = created.project.id;
    // The generated initial key is deliberately inaccessible to the operator;
    // only its digest persists. Explicit trusted rotation supplies the usable key.
    drop(zeroize::Zeroizing::new(created.api_key));
    fs::DirBuilder::new()
        .mode(0o700)
        .create(root.join("private"))?;
    let mut account = AccountStore::create(root.join("private").join(&id), &id, pool.clone())?;
    account.enable_session_clock(now)?;
    let expected = registry.capture_account_bundle(std::slice::from_mut(&mut account))?;
    drop(account);
    drop(registry);
    checkpoint("account_init_prepared");
    let manifest = AccountBundleRootManifest {
        version: 1,
        private_projects: vec![id],
        reset_at: now,
    };
    let encoded = manifest_bytes(&manifest)?;
    sync(
        &metadata::open_directory(&root.join("private"))?,
        "account_init_private_sync",
    )?;
    let mut file = File::options()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(root.join("root.json"))?;
    file.write_all(&encoded)?;
    sync(&file, "account_init_manifest_sync")?;
    checkpoint("account_init_manifest_synced");
    let state = inspect_owned(&root, &pending.owner, pool)?;
    let original = account_bundle::decode(&expected)?;
    if state.registry != original.registry
        || state.private_hashes
            != original
                .private
                .iter()
                .map(|(_, b)| hash(b))
                .collect::<Vec<_>>()
        || state.report.registry != original.report.registry
        || state.report.private_accounts != original.report.private_accounts
        || state.report.reset_at != now
    {
        return Err(Error::BundleRoot("initialized history changed"));
    }
    sync(&pending.owner, "account_init_stage_sync")?;
    checkpoint("account_init_stage_synced");
    check_manifest(&root, &file, &encoded)?;
    check_contents(&root, &pending.owner, &state.contents)?;
    pending.publish()?;
    checkpoint("account_init_renamed");
    pending.finish("account_init_parent_sync")?;
    checkpoint("account_init_parent_synced");
    Ok(state.report)
}
