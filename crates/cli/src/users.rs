//! Trusted offline user administration through the original private account owner.
use clap::{Args, Subcommand};
use emilybase_auth::{
    accounts::AccountInfo,
    password::{MAX_PASSWORD_BYTES, PasswordPool},
};
use emilybase_server::AccountRoot;
use serde::Serialize;
use std::io::{IsTerminal, Read, Write};
use std::path::PathBuf;
use zeroize::Zeroizing;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
#[derive(Args)]
pub struct Arguments {
    /// Existing stopped private account root.
    path: PathBuf,
    project: String,
    /// Current project service key in an exact private file, not an argument value.
    #[arg(long)]
    key_file: PathBuf,
    #[command(subcommand)]
    command: Operation,
}
#[derive(Subcommand)]
enum Operation {
    /// Provision one canonical login using exact raw password bytes from stdin.
    Create { login: String },
    /// Read current metadata in canonical-login order; cursor is exclusive.
    List {
        #[arg(long)]
        after: Option<String>,
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    /// Revoke current credentials/families using the original epoch transition.
    Disable { login: String },
    /// Re-enable sign-in; previously revoked tokens remain invalid.
    Enable { login: String },
}
#[derive(Serialize)]
struct User {
    id: String,
    login: String,
    credential_epoch: String,
    disabled: bool,
}
impl From<AccountInfo> for User {
    fn from(info: AccountInfo) -> Self {
        Self {
            id: info.id.iter().map(|b| format!("{b:02x}")).collect(),
            login: info.login,
            credential_epoch: info.credential_epoch.to_string(),
            disabled: info.disabled,
        }
    }
}
fn password(input: impl Read) -> Result<Zeroizing<Vec<u8>>> {
    let mut bytes = Zeroizing::new(Vec::new());
    input
        .take((MAX_PASSWORD_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "password input unavailable")?;
    if bytes.is_empty() || bytes.len() > MAX_PASSWORD_BYTES {
        return Err("password must contain 1..1024 bytes".into());
    }
    Ok(bytes)
}
fn output(value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() > 65_536 {
        return Err("user operation output outcome requires inspection".into());
    }
    let mut out = std::io::stdout().lock();
    out.write_all(&bytes)?;
    out.write_all(b"\n")?;
    out.flush()?;
    Ok(())
}
fn operation<T>(value: emilybase_server::Result<T>) -> Result<T> {
    value.map_err(|error| match error {
        emilybase_server::Error::Accounts(account) => Box::new(account) as _,
        other => Box::new(other) as _,
    })
}
pub fn run(arguments: Arguments) -> Result<()> {
    let Arguments {
        path,
        project,
        key_file,
        command,
    } = arguments;
    if !emilybase_auth::valid_project_id(&project) {
        return Err("invalid account project identity".into());
    }
    let key = emilybase_auth::key_file::read_api_key_file(&key_file)?;
    // Do not lock a root while an operator's password stream is incomplete.
    let secret = match &command {
        Operation::Create { .. } => {
            if std::io::stdin().is_terminal() {
                return Err("password requires redirected stdin".into());
            }
            Some(password(std::io::stdin().lock())?)
        }
        _ => None,
    };
    let mut root = AccountRoot::open(path, PasswordPool::new(1)?)?;
    let disabled = matches!(&command, Operation::Disable { .. });
    match command {
        Operation::Create { login } => {
            let secret = secret.ok_or("password input unavailable")?;
            let user = operation(root.create_user(&project, &key, &login, &secret))?;
            output(&serde_json::json!({"user":User::from(user)}))
        }
        Operation::List { after, limit } => {
            let page = operation(root.list_users(&project, &key, after.as_deref(), limit))?;
            let users: Vec<User> = page.users.into_iter().map(Into::into).collect();
            output(&serde_json::json!({"users":users,"next_after":page.next_after}))
        }
        Operation::Disable { login } | Operation::Enable { login } => {
            let user = operation(root.set_disabled(&project, &key, &login, disabled))?;
            output(&serde_json::json!({"user":User::from(user)}))
        }
    }
}

#[cfg(test)]
mod tests;
