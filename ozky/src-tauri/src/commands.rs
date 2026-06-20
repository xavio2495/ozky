//! The `invoke` command surface — the only thing the Svelte UI calls. Each command
//! is a thin shim over [`crate::core`]; the UI never sees a crypto primitive. A0
//! wires the skeleton: `wallet_status` is a real working command (it reads the OS
//! keychain), the action commands return `NotImplemented` until their phase lands.

use crate::core::{self, CoreError};
use serde::Serialize;

/// High-level wallet state for the UI shell.
#[derive(Serialize)]
pub struct WalletStatus {
    /// Whether a wallet seed exists in the OS keychain.
    pub initialized: bool,
    /// Target network (testnet through Part 1/2).
    pub network: String,
}

/// Real (stub) core command: report whether a wallet exists. Exercises the
/// UI -> command -> core -> OS keychain path end-to-end (A0 smoke test).
#[tauri::command]
pub fn wallet_status() -> Result<WalletStatus, CoreError> {
    let initialized = core::keychain::exists(core::keys::SEED_ACCOUNT)?;
    Ok(WalletStatus {
        initialized,
        network: core::chain::DEFAULT_NETWORK.to_string(),
    })
}

/// Create a new wallet: generate a 12-word phrase, validate it derives, store it in
/// the OS keychain, and return the phrase ONCE so the user can back it up. (A1)
#[tauri::command]
pub fn create_wallet() -> Result<String, CoreError> {
    let phrase = core::keys::generate_mnemonic()?;
    // Validate it derives cleanly before persisting.
    core::keys::derive_from_mnemonic(&phrase)?;
    core::keychain::store(core::keys::SEED_ACCOUNT, &phrase)?;
    Ok(phrase)
}

/// Restore a wallet from a 12-word phrase: validate it derives, then store it in the
/// OS keychain. (Chain re-scan to re-discover notes is A2.)
#[tauri::command]
pub fn restore_wallet(phrase: String) -> Result<(), CoreError> {
    core::keys::derive_from_mnemonic(&phrase)?; // validates the phrase
    core::keychain::store(core::keys::SEED_ACCOUNT, phrase.trim())
}

/// Total spendable balance per asset. (A2)
#[tauri::command]
pub fn balance() -> Result<u64, CoreError> {
    core::scan::scan(0).map(|notes| notes.iter().map(|n| n.value).sum())
}

/// Deposit `amount` of the configured asset into the shielded pool from the wallet's
/// Stellar account (the public on-ramp: fund [`funding_address`] from any wallet, then
/// deposit to shield it). Returns the tx hash. (A3)
#[tauri::command]
pub fn deposit(amount: u64) -> Result<String, CoreError> {
    core::deposit::deposit(amount)
}

/// Send `amount` privately to `recipient` (a shielded payment code). Builds + proves
/// the transfer against live pool state and submits it; returns the tx hash. (A3)
#[tauri::command]
pub fn send(recipient: String, amount: u64) -> Result<String, CoreError> {
    core::send::send(&recipient, amount)
}

/// Withdraw `amount` out of the shielded pool to a public Stellar `dest` address (the
/// off-ramp). Returns the tx hash. (A3)
#[tauri::command]
pub fn withdraw(dest: String, amount: u64) -> Result<String, CoreError> {
    core::withdraw::withdraw(&dest, amount)
}

/// This wallet's **public Stellar funding address** (`G…`). Give this to any wallet or
/// exchange to receive funds publicly; then [`deposit`] shields them into the pool.
/// This is a normal Stellar account — usable from non-ozky wallets. (A3)
#[tauri::command]
pub fn funding_address() -> Result<String, CoreError> {
    let keys = core::keys::current_wallet()?;
    Ok(keys.stellar_address().to_string())
}

/// This wallet's **shielded receive address** (an `ozky…` payment code). Give this to
/// another ozky wallet to receive a PRIVATE transfer. Not usable from non-ozky wallets —
/// for external/public funding use [`funding_address`]. (A3)
#[tauri::command]
pub fn receive_address() -> Result<String, CoreError> {
    core::send::receive_code()
}

/// Export a scoped disclosure for an auditor (account / asset / epoch). (A2/A3)
#[tauri::command]
pub fn share_with_auditor(_auditor: String, _epoch: u32) -> Result<String, CoreError> {
    Err(CoreError::not_implemented("share_with_auditor (A3)"))
}
