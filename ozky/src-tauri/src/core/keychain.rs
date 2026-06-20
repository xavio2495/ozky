//! OS keychain access (Windows Credential Manager / macOS Keychain / Linux secret
//! service), via the `keyring` crate. Wallet secrets — the BIP39 seed and derived
//! private keys — live here, never in app files. Wired in A0; A1 stores the seed
//! through it on wallet create/restore.

use super::CoreError;
use keyring::Entry;

/// Keychain service namespace for all ozky secrets.
const SERVICE: &str = "ozky-wallet";

fn entry(account: &str) -> Result<Entry, CoreError> {
    Entry::new(SERVICE, account).map_err(|e| CoreError::Keychain(e.to_string()))
}

/// Store `secret` under `account` in the OS keychain (overwrites).
pub fn store(account: &str, secret: &str) -> Result<(), CoreError> {
    entry(account)?
        .set_password(secret)
        .map_err(|e| CoreError::Keychain(e.to_string()))
}

/// Load the secret stored under `account`, if any.
pub fn load(account: &str) -> Result<Option<String>, CoreError> {
    match entry(account)?.get_password() {
        Ok(s) => Ok(Some(s)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(CoreError::Keychain(e.to_string())),
    }
}

/// Remove the secret stored under `account` (no error if absent).
pub fn delete(account: &str) -> Result<(), CoreError> {
    match entry(account)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(CoreError::Keychain(e.to_string())),
    }
}

/// Whether a secret exists under `account`.
pub fn exists(account: &str) -> Result<bool, CoreError> {
    Ok(load(account)?.is_some())
}
