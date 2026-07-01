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

/// A staged (not-yet-committed) wallet setup. Created at sign-up/restore but only written
/// to the vault + opened as a session once the user confirms their 2FA code. Holding it
/// here — instead of committing immediately — means an abandoned or reloaded onboarding
/// leaves NO usable wallet, so a user can't get locked out by skipping the authenticator
/// step (the vault would otherwise demand a TOTP they never finished setting up).
pub struct PendingSetup {
    pub password: Zeroizing<String>,
    pub content: VaultContent,
}

static PENDING: Mutex<Option<PendingSetup>> = Mutex::new(None);

fn pending_guard() -> std::sync::MutexGuard<'static, Option<PendingSetup>> {
    PENDING.lock().unwrap_or_else(|e| e.into_inner())
}

/// Stage a wallet setup pending 2FA confirmation (overwrites any prior staged setup).
pub fn set_pending(p: PendingSetup) {
    *pending_guard() = Some(p);
}

/// The staged setup's TOTP secret — to verify the confirmation code without consuming it.
pub fn pending_totp_secret() -> Option<[u8; super::totp::SECRET_LEN]> {
    pending_guard().as_ref().map(|p| p.content.totp_secret)
}

/// Consume the staged setup (on successful 2FA confirmation, to commit it).
pub fn take_pending() -> Option<PendingSetup> {
    pending_guard().take()
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

/// Remove the account at `index` (its `Zeroizing` mnemonic is zeroed on drop), re-encrypt
/// the vault, and remap the active account: removing an account before the active one
/// shifts it down; removing the active one selects the previous account (or the new first).
/// Refuses to remove the last remaining account. Returns the new active index.
pub fn remove_account(index: u32) -> Result<u32, CoreError> {
    let mut g = lock_guard();
    let s = g.as_mut().ok_or(CoreError::Locked)?;
    let i = index as usize;
    if i >= s.content.accounts.len() {
        return Err(CoreError::Crypto("no such account".into()));
    }
    if s.content.accounts.len() == 1 {
        return Err(CoreError::Crypto("cannot remove the only account".into()));
    }
    s.content.accounts.remove(i);
    super::vault::save(&s.key, &s.content)?;
    let mut active = s.active as usize;
    if active == i {
        active = i.saturating_sub(1);
    } else if active > i {
        active -= 1;
    }
    s.active = active.min(s.content.accounts.len() - 1) as u32;
    Ok(s.active)
}
