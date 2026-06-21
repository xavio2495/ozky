//! In-memory unlocked-wallet session. The vault ([`super::vault`]) holds the seeds
//! encrypted at rest; once the user unlocks (password + TOTP), the decrypted contents
//! (all account mnemonics + the TOTP secret) and the derived vault key live here for the
//! lifetime of the app process (or until [`lock`]). Holding the key lets us add/import
//! accounts (re-encrypting the vault) without re-prompting for the password.
//!
//! Secret-bearing core paths ([`super::keys::current_wallet`]) read the active account's
//! mnemonic from here, so nothing works while locked — they get [`CoreError::Locked`].

use super::vault::{VaultContent, VaultKey};
use super::CoreError;
use std::sync::Mutex;
use zeroize::Zeroizing;

struct Session {
    content: VaultContent,
    key: VaultKey,
    /// Active account index (into `content.accounts`).
    active: u32,
}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);

fn lock_guard() -> std::sync::MutexGuard<'static, Option<Session>> {
    SESSION.lock().unwrap_or_else(|e| e.into_inner())
}

/// Establish the unlocked session from decrypted vault contents + derived key.
pub fn set(content: VaultContent, key: VaultKey, active: u32) {
    let active = active.min(content.accounts.len().saturating_sub(1) as u32);
    *lock_guard() = Some(Session { content, key, active });
}

/// Clear the session (lock the wallet).
pub fn clear() {
    *lock_guard() = None;
}

pub fn is_unlocked() -> bool {
    lock_guard().is_some()
}

/// Number of accounts (mnemonics) in the unlocked vault.
pub fn account_count() -> u32 {
    lock_guard()
        .as_ref()
        .map(|s| s.content.accounts.len() as u32)
        .unwrap_or(0)
}

/// The active account index (0 if locked — callers gate on unlock anyway).
pub fn active_account() -> u32 {
    lock_guard().as_ref().map(|s| s.active).unwrap_or(0)
}

/// Switch the active account (clamped to the valid range).
pub fn set_active_account(index: u32) {
    if let Some(s) = lock_guard().as_mut() {
        if (index as usize) < s.content.accounts.len() {
            s.active = index;
        }
    }
}

/// The active account's mnemonic, or [`CoreError::Locked`] if no session.
pub fn mnemonic() -> Result<Zeroizing<String>, CoreError> {
    let g = lock_guard();
    let s = g.as_ref().ok_or(CoreError::Locked)?;
    Ok(s.content.accounts[s.active as usize].clone())
}

/// The mnemonic for a specific account index (for listing addresses).
pub fn mnemonic_at(index: u32) -> Result<Zeroizing<String>, CoreError> {
    let g = lock_guard();
    let s = g.as_ref().ok_or(CoreError::Locked)?;
    s.content
        .accounts
        .get(index as usize)
        .cloned()
        .ok_or_else(|| CoreError::Crypto("no such account".into()))
}

/// The TOTP secret of the unlocked session.
pub fn totp_secret() -> Result<[u8; super::totp::SECRET_LEN], CoreError> {
    lock_guard()
        .as_ref()
        .map(|s| s.content.totp_secret)
        .ok_or(CoreError::Locked)
}

/// Add a new account mnemonic, persist the vault (re-encrypt with the held key), and
/// switch to it. Returns the new account's index.
pub fn add_account(mnemonic: String) -> Result<u32, CoreError> {
    let mut g = lock_guard();
    let s = g.as_mut().ok_or(CoreError::Locked)?;
    s.content.accounts.push(Zeroizing::new(mnemonic));
    super::vault::save(&s.key, &s.content)?;
    let index = (s.content.accounts.len() - 1) as u32;
    s.active = index;
    Ok(index)
}
