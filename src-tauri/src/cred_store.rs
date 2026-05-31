//! File-backed credential vault — ~/.seryai/.vault.json
//!
//! Drop-in replacement for `keyring::Entry`. Stores credentials as a
//! nested JSON map  { service → { account → value } }  so the file
//! is human-readable and every entry can be inspected or rotated with
//! a text editor.
//!
//! Security model: ~/.seryai/.vault.json is written with 0600
//! permissions (owner read/write only), same as ~/.aws/credentials.
//! No OS keychain involvement — no macOS authorization prompts.

use once_cell::sync::Lazy;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

// ── Error type (mirrors keyring::Error subset used by callers) ────────────

#[derive(Debug)]
pub enum Error {
    /// No entry found for (service, account).
    NoEntry,
    Other(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NoEntry => write!(f, "no entry"),
            Error::Other(s) => write!(f, "{s}"),
        }
    }
}

// ── Vault file I/O ────────────────────────────────────────────────────────

/// In-process cache of the full vault map to avoid re-reading the file
/// on every credential lookup. Invalidated on every write/delete.
static VAULT: Lazy<Mutex<Option<HashMap<String, HashMap<String, String>>>>> =
    Lazy::new(|| Mutex::new(None));

fn vault_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".seryai").join(".vault.json"))
}

fn read_vault() -> HashMap<String, HashMap<String, String>> {
    let path = match vault_path() {
        Some(p) => p,
        None => return HashMap::new(),
    };
    if !path.exists() {
        return HashMap::new();
    }
    let txt = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return HashMap::new(),
    };
    match serde_json::from_str::<HashMap<String, HashMap<String, String>>>(&txt) {
        Ok(m) => m,
        Err(_) => HashMap::new(),
    }
}

fn write_vault(map: &HashMap<String, HashMap<String, String>>) -> Result<(), Error> {
    let path = vault_path().ok_or_else(|| Error::Other("no home dir".to_string()))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| Error::Other(format!("create dir: {e}")))?;
    }
    let json = serde_json::to_string_pretty(&Value::Object(
        map.iter()
            .map(|(svc, accounts)| {
                let inner = accounts
                    .iter()
                    .map(|(acc, val)| (acc.clone(), Value::String(val.clone())))
                    .collect::<serde_json::Map<_, _>>();
                (svc.clone(), Value::Object(inner))
            })
            .collect(),
    ))
    .map_err(|e| Error::Other(format!("serialize: {e}")))?;
    fs::write(&path, json).map_err(|e| Error::Other(format!("write: {e}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn with_vault<F, T>(f: F) -> T
where
    F: FnOnce(&mut HashMap<String, HashMap<String, String>>) -> T,
{
    let mut guard = VAULT.lock().expect("VAULT poisoned");
    if guard.is_none() {
        *guard = Some(read_vault());
    }
    f(guard.as_mut().expect("just populated"))
}

fn flush(map: &HashMap<String, HashMap<String, String>>) -> Result<(), Error> {
    write_vault(map)?;
    Ok(())
}

// ── Entry API (mirrors keyring::Entry) ───────────────────────────────────

pub struct Entry {
    service: String,
    account: String,
}

impl Entry {
    /// Construct an entry handle. Infallible — mirroring `keyring::Entry::new`
    /// which returns `Result` only for platform setup failures we don't have.
    pub fn new(service: &str, account: &str) -> Result<Self, Error> {
        Ok(Self {
            service: service.to_string(),
            account: account.to_string(),
        })
    }

    pub fn get_password(&self) -> Result<String, Error> {
        with_vault(|map| {
            map.get(&self.service)
                .and_then(|svc| svc.get(&self.account))
                .cloned()
                .ok_or(Error::NoEntry)
        })
    }

    pub fn set_password(&self, password: &str) -> Result<(), Error> {
        with_vault(|map| {
            map.entry(self.service.clone())
                .or_default()
                .insert(self.account.clone(), password.to_string());
            flush(map)
        })
    }

    pub fn delete_password(&self) -> Result<(), Error> {
        with_vault(|map| {
            let empty = if let Some(svc) = map.get_mut(&self.service) {
                svc.remove(&self.account);
                svc.is_empty()
            } else {
                return Ok(());
            };
            if empty {
                map.remove(&self.service);
            }
            flush(map)
        })
    }
}
