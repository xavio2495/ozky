//! ASP enrollment (Phase A3 / FEATURE_SET G2). To transact in a shared pool a wallet's
//! spending key (`owner_pk`) must be in the policy's approved set (so it can prove
//! `owner_pk ∈ asp_root`) and its funding address must be on the deposit allow-list.
//!
//! In production this is an **ASP operation**: the user shares their `owner_pk`
//! ([`spending_key`]) and the ASP admin enrolls them. On testnet the wallet is the
//! policy admin, so [`enroll_self`] performs the enrollment end-to-end (approve member
//! + allow-list funding address + sync the pool's cached root).

use super::config::PoolConfig;
use super::{chain, keys, scan, CoreError};

/// This wallet's spending public key (`owner_pk`, hex) — the value an external ASP
/// approves to enroll the wallet into a shared pool's anonymity set.
pub fn spending_key() -> Result<String, CoreError> {
    let wallet = keys::current_wallet()?;
    Ok(scan::wallet_identity(&wallet)?.owner_pk.to_hex())
}

/// Enroll THIS wallet into the configured pool's ASP set (testnet/dev: the wallet must
/// be the policy admin). Approves `owner_pk`, allow-lists the funding `G…` address, and
/// syncs the pool's cached root. Returns the tx hash.
pub fn enroll_self() -> Result<String, CoreError> {
    let wallet = keys::current_wallet()?;
    let cfg = PoolConfig::load()?;
    enroll_self_with(&wallet, &cfg)
}

/// Keychain-independent enrollment (used by the live-run driver).
pub fn enroll_self_with(wallet: &keys::WalletKeys, cfg: &PoolConfig) -> Result<String, CoreError> {
    let id = scan::wallet_identity(wallet)?;
    chain::submit_enroll(
        cfg,
        wallet.stellar_secret(),
        &id.owner_pk.to_decimal(),
        wallet.stellar_address(),
    )
}
