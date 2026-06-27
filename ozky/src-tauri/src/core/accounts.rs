//! Account metadata: labels + which account is active. NOT secret (the seeds live
//! encrypted in [`super::vault`]); stored as plaintext JSON in the keychain alongside the
//! vault. The account count is the number of seeds in the vault — `labels` mirrors it.

use super::CoreError;
use serde::{Deserialize, Serialize};

/// Keychain account holding the account-metadata JSON.
const META_ACCOUNT: &str = "accounts";
/// Hard cap on accounts in one app (per product spec).
pub const MAX_ACCOUNTS: u32 = 5;

#[derive(Serialize, Deserialize, Clone)]
pub struct AccountMeta {
    /// Active account index.
    pub active: u32,
    /// Display labels, one per account index.
    pub labels: Vec<String>,
}

impl AccountMeta {
    pub fn initial() -> Self {
        AccountMeta {
            active: 0,
            labels: vec!["Account 1".to_string()],
        }
    }

    pub fn label_for(&self, index: u32) -> String {
        self.labels
            .get(index as usize)
            .cloned()
            .unwrap_or_else(|| format!("Account {}", index + 1))
    }
}

/// Load the account metadata, or the single-account default if none is stored yet.
pub fn load() -> Result<AccountMeta, CoreError> {
    match super::keychain::load(META_ACCOUNT)? {
        Some(json) => serde_json::from_str(&json)
            .map_err(|e| CoreError::Crypto(format!("account meta decode: {e}"))),
        None => Ok(AccountMeta::initial()),
    }
}

pub fn save(meta: &AccountMeta) -> Result<(), CoreError> {
    let json = serde_json::to_string(meta)
        .map_err(|e| CoreError::Crypto(format!("account meta encode: {e}")))?;
    super::keychain::store(META_ACCOUNT, &json)
}

/// Reset to a fresh single-account state (on create/restore).
pub fn reset() -> Result<(), CoreError> {
    save(&AccountMeta::initial())
}

/// Append a label for a newly added account and make it active. `label` falls back to
/// "Account N" when empty.
pub fn add(label: Option<String>) -> Result<(), CoreError> {
    let mut meta = load()?;
    let index = meta.labels.len() as u32;
    let label = label
        .filter(|l| !l.trim().is_empty())
        .unwrap_or_else(|| format!("Account {}", index + 1));
    meta.labels.push(label);
    meta.active = index;
    save(&meta)
}

/// Rename an existing account. Empty/whitespace labels fall back to "Account N".
pub fn rename(index: u32, label: String) -> Result<(), CoreError> {
    let mut meta = load()?;
    if index as usize >= meta.labels.len() {
        return Err(CoreError::Crypto("no such account".into()));
    }
    let label = if label.trim().is_empty() {
        format!("Account {}", index + 1)
    } else {
        label.trim().to_string()
    };
    meta.labels[index as usize] = label;
    save(&meta)
}

/// Remove the account-metadata entry entirely (logout / device wipe).
pub fn wipe() -> Result<(), CoreError> {
    super::keychain::delete(META_ACCOUNT)
}

/// Set the active account index.
pub fn set_active(index: u32) -> Result<(), CoreError> {
    let mut meta = load()?;
    if index as usize >= meta.labels.len() {
        return Err(CoreError::Crypto("no such account".into()));
    }
    meta.active = index;
    save(&meta)
}

pub fn label(index: u32) -> Result<String, CoreError> {
    Ok(load()?.label_for(index))
}
