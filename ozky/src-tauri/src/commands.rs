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

/// Spendable balance of one asset the wallet holds shielded notes in.
#[derive(Serialize)]
pub struct AssetBalance {
    /// v1 asset code (e.g. "USDC"), or the raw `asset_tag` decimal if unknown.
    pub code: String,
    /// The in-circuit `asset_tag` (decimal).
    pub asset_tag: String,
    /// Total spendable value in base units.
    pub raw: u64,
    /// Human-readable amount (base units scaled by `decimals`).
    pub display: String,
    pub decimals: u32,
}

/// Total spendable balance **per asset** (one row per known v1 asset; 0 if none held).
/// Notes carry their `asset_tag` in plaintext, so a single scan covers every asset. (A2/G6)
#[tauri::command]
pub fn balance() -> Result<Vec<AssetBalance>, CoreError> {
    let notes = core::scan::scan(0)?;
    let mut out = Vec::new();
    for a in core::config::ASSETS {
        let tag_dec = a.tag.to_string();
        let raw: u64 = notes
            .iter()
            .filter(|n| n.asset_tag.to_decimal() == tag_dec)
            .map(|n| n.value)
            .sum();
        out.push(AssetBalance {
            code: a.code.to_string(),
            asset_tag: tag_dec,
            raw,
            display: format_units(raw, a.decimals),
            decimals: a.decimals,
        });
    }
    Ok(out)
}

/// Format `raw` base units as a decimal string scaled by `decimals` (e.g. 1000 @ 7 → "0.0001000").
fn format_units(raw: u64, decimals: u32) -> String {
    if decimals == 0 {
        return raw.to_string();
    }
    let scale = 10u64.pow(decimals);
    let whole = raw / scale;
    let frac = raw % scale;
    format!("{whole}.{frac:0>width$}", width = decimals as usize)
}

/// This wallet's spending public key (`owner_pk`, hex) — share it with the ASP to be
/// enrolled into a shared pool's anonymity set. (A3 / ASP enrollment)
#[tauri::command]
pub fn spending_key() -> Result<String, CoreError> {
    core::enroll::spending_key()
}

/// Enroll this wallet into the configured pool's ASP approved set + deposit allow-list
/// (testnet/dev: the wallet must be the policy admin). Returns the tx hash. (A3)
#[tauri::command]
pub fn enroll() -> Result<String, CoreError> {
    core::enroll::enroll_self()
}

/// Deposit `amount` of `asset` (a v1 code, e.g. "USDC") into the shielded pool from the
/// wallet's Stellar account (the public on-ramp: fund [`funding_address`] from any
/// wallet, then deposit to shield it). Returns the tx hash. (A3/G6)
#[tauri::command]
pub fn deposit(asset: String, amount: u64) -> Result<String, CoreError> {
    core::deposit::deposit(&asset, amount)
}

/// Send `amount` of `asset` privately to `recipient` (a shielded payment code). Builds +
/// proves the transfer against live pool state and submits it; returns the tx hash. (A3/G6)
#[tauri::command]
pub fn send(asset: String, recipient: String, amount: u64) -> Result<String, CoreError> {
    core::send::send(&asset, &recipient, amount)
}

/// Withdraw `amount` of `asset` out of the shielded pool to a public Stellar `dest`
/// address (the off-ramp). Returns the tx hash. (A3/G6)
#[tauri::command]
pub fn withdraw(asset: String, dest: String, amount: u64) -> Result<String, CoreError> {
    core::withdraw::withdraw(&asset, &dest, amount)
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

/// Export a scoped, read-only disclosure for an auditor (a Stellar `G…`) and record the
/// auditable on-chain grant. Returns the disclosure package (JSON) to hand the auditor
/// out-of-band: it lets them re-derive + verify this wallet's notes for the scope, with
/// no spend authority. (A3 / G5)
#[tauri::command]
pub fn share_with_auditor(auditor: String, epoch: u32) -> Result<String, CoreError> {
    core::disclose::share_with_auditor(&auditor, epoch)
}

/// Auditor side: given a disclosure package (JSON from [`share_with_auditor`]), scan the
/// disclosed pool and return the owner's notes it reveals (each verified against its
/// on-chain commitment), as JSON. Read-only; needs no wallet. (A3 / G5)
#[tauri::command]
pub fn audit_disclosure(package: String) -> Result<String, CoreError> {
    let notes = core::disclose::audit(&package)?;
    let total = core::disclose::disclosed_total(&notes);
    serde_json::to_string(&serde_json::json!({ "total": total, "notes": notes }))
        .map_err(|e| CoreError::Crypto(format!("serialize audit: {e}")))
}
