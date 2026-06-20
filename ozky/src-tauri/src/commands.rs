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

/// Send `amount` privately to `recipient`. (A2/A3)
#[tauri::command]
pub fn send(recipient: String, amount: u64) -> Result<String, CoreError> {
    let _ = core::proving::prove_transfer(&recipient, amount)?;
    Err(CoreError::not_implemented("send (A3)"))
}

/// The Stellar account address for this wallet (the public edge address for
/// funding/deposits). The shielded payment code is added with note encryption (A2).
#[tauri::command]
pub fn receive_address() -> Result<String, CoreError> {
    let keys = core::keys::current_wallet()?;
    Ok(keys.stellar_address().to_string())
}

/// Export a scoped disclosure for an auditor (account / asset / epoch). (A2/A3)
#[tauri::command]
pub fn share_with_auditor(_auditor: String, _epoch: u32) -> Result<String, CoreError> {
    Err(CoreError::not_implemented("share_with_auditor (A3)"))
}
